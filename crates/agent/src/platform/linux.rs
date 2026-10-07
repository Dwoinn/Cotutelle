//! Linux : logind pour les sessions, systemd-resolved et nftables pour le DNS.

use super::{Platform, Session};
use anyhow::{Context, Result, bail};
use cotutelle_common::protocol::TamperKind;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const RESOLVED_DROPIN: &str = "/etc/systemd/resolved.conf.d/cotutelle.conf";
const NFT_TABLE: &str = "cotutelle";

/// Navigateur dont le DNS chiffré intégré doit être désactivé par politique.
struct Browser {
    policy: &'static str,
    content: &'static str,
    /// Chemins dont la présence indique que le navigateur est installé.
    installed_if: &'static [&'static str],
}

const BROWSERS: &[Browser] = &[
    Browser {
        policy: "/etc/firefox/policies/policies.json",
        content: FIREFOX_POLICY_JSON,
        installed_if: &[
            "/etc/firefox",
            "/usr/lib/firefox",
            "/usr/bin/firefox",
            "/usr/bin/firefox-esr",
            "/snap/bin/firefox",
        ],
    },
    Browser {
        policy: "/etc/chromium/policies/managed/cotutelle.json",
        content: CHROME_POLICY_JSON,
        installed_if: &["/etc/chromium", "/usr/bin/chromium", "/usr/lib/chromium"],
    },
    Browser {
        policy: "/etc/chromium-browser/policies/managed/cotutelle.json",
        content: CHROME_POLICY_JSON,
        installed_if: &["/etc/chromium-browser", "/usr/bin/chromium-browser", "/snap/bin/chromium"],
    },
    Browser {
        policy: "/etc/opt/chrome/policies/managed/cotutelle.json",
        content: CHROME_POLICY_JSON,
        installed_if: &["/etc/opt/chrome", "/opt/google/chrome"],
    },
    Browser {
        policy: "/etc/brave/policies/managed/cotutelle.json",
        content: CHROME_POLICY_JSON,
        installed_if: &["/etc/brave", "/opt/brave.com", "/usr/bin/brave", "/usr/bin/brave-browser"],
    },
    Browser {
        policy: "/etc/opt/edge/policies/managed/cotutelle.json",
        content: CHROME_POLICY_JSON,
        installed_if: &["/etc/opt/edge", "/opt/microsoft/msedge"],
    },
];
/// Marqueur des fichiers que l'agent a le droit de réécrire et de supprimer.
const MARKER: &str = "cotutelle";

/// Verrous d'écran essayés, dans l'ordre, quand le bureau ignore l'ordre de
/// verrouillage de logind (c'est le cas de Hyprland sans hypridle).
const FALLBACK_LOCKERS: &[&str] =
    &["omarchy-system-lock", "hyprlock", "swaylock -f", "xdg-screensaver lock", "dm-tool lock"];
/// Délai laissé au bureau pour se déclarer verrouillé.
const LOCK_SETTLE: Duration = Duration::from_millis(2500);
const LOCK_POLL: Duration = Duration::from_millis(250);

pub struct Linux;

fn run(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("lancement de {program}"))?;
    if !output.status.success() {
        bail!("{program} {} : {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Propriétés de session demandées à logind. Toute propriété lue par
/// [`session_from_properties`] doit figurer ici, sinon la session est écartée.
const SESSION_PROPERTIES: &[&str] =
    &["Name", "User", "Active", "LockedHint", "IdleHint", "Class", "Type", "Remote", "Seat"];

/// Lit une sortie `clé=valeur` de `loginctl show-session`.
fn parse_properties(text: &str) -> HashMap<&str, &str> {
    text.lines().filter_map(|line| line.split_once('=')).collect()
}

fn session_from_properties(id: &str, props: &HashMap<&str, &str>) -> Option<Session> {
    let yes = |key: &str| props.get(key).is_some_and(|v| *v == "yes");
    // On ignore l'écran de connexion (`greeter`), les services et le SSH.
    if props.get("Class").copied() != Some("user") || yes("Remote") {
        return None;
    }
    if !matches!(props.get("Type").copied(), Some("x11" | "wayland" | "mir" | "tty")) {
        return None;
    }
    // Une session sans siège n'a ni écran ni clavier locaux : console série
    // d'une machine virtuelle, par exemple. Personne n'est « devant ».
    if props.get("Seat").is_none_or(|seat| seat.is_empty()) {
        return None;
    }
    Some(Session {
        id: id.to_string(),
        user: (*props.get("Name")?).to_string(),
        uid: props.get("User")?.parse().ok()?,
        active: yes("Active"),
        locked: yes("LockedHint"),
        idle: yes("IdleHint"),
    })
}

/// Découpe le contenu de `/proc/<pid>/environ` en paires `nom=valeur`.
fn parse_environ(raw: &[u8]) -> Vec<(String, String)> {
    raw.split(|b| *b == 0)
        .filter_map(|entry| std::str::from_utf8(entry).ok())
        .filter_map(|entry| entry.split_once('='))
        .filter(|(name, _)| {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

/// Environnement de la session graphique d'un utilisateur, repris d'un de ses
/// processus. Les outils du bureau (verrou, notifications, `hyprctl`) en
/// dépendent : adresse du bus, socket Wayland, variables propres au bureau.
fn session_environment(uid: u32) -> Vec<(String, String)> {
    use std::os::unix::fs::MetadataExt;
    let mut fallback = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else { return fallback };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_pid = entry.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit());
        if !is_pid || std::fs::metadata(&path).map(|m| m.uid()).ok() != Some(uid) {
            continue;
        }
        let Ok(raw) = std::fs::read(path.join("environ")) else { continue };
        let env = parse_environ(&raw);
        let has = |name: &str| env.iter().any(|(n, _)| n == name);
        if has("WAYLAND_DISPLAY") {
            return env;
        }
        if has("DISPLAY") && fallback.is_empty() {
            fallback = env;
        }
    }
    fallback
}

/// Lit la sortie de `hyprctl -j monitors` : sous Hyprland, un écran verrouillé
/// porte le motif `LOCK` dans `solitaryBlockedBy`. Rend `None` si la sortie
/// ne permet pas de conclure.
fn hyprland_locked(monitors_json: &str) -> Option<bool> {
    let monitors: serde_json::Value = serde_json::from_str(monitors_json).ok()?;
    let mut known = false;
    for monitor in monitors.as_array()? {
        let Some(blockers) = monitor.get("solitaryBlockedBy").and_then(|b| b.as_array()) else {
            continue;
        };
        known = true;
        if blockers.iter().any(|b| b.as_str() == Some("LOCK")) {
            return Some(true);
        }
    }
    known.then_some(false)
}

/// Lance un programme dans la session graphique de l'utilisateur.
fn run_in_session(session: &Session, program: &str, args: &[&str]) -> Result<String> {
    let mut env = session_environment(session.uid);
    if env.is_empty() {
        // Session sans processus graphique repéré : le strict minimum.
        let runtime_dir = format!("/run/user/{}", session.uid);
        env.push(("DBUS_SESSION_BUS_ADDRESS".into(), format!("unix:path={runtime_dir}/bus")));
        env.push(("XDG_RUNTIME_DIR".into(), runtime_dir));
        env.push(("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()));
    }
    let assignments: Vec<String> = env.iter().map(|(n, v)| format!("{n}={v}")).collect();
    let mut argv: Vec<&str> = vec!["-u", &session.user, "--", "env", "-i"];
    argv.extend(assignments.iter().map(String::as_str));
    argv.push(program);
    argv.extend(args);
    run("runuser", &argv)
}

/// État de verrouillage vu du bureau, quand logind ne le connaît pas.
fn desktop_locked(session: &Session) -> Option<bool> {
    let is_hyprland =
        session_environment(session.uid).iter().any(|(n, _)| n == "HYPRLAND_INSTANCE_SIGNATURE");
    if !is_hyprland {
        return None;
    }
    hyprland_locked(&run_in_session(session, "hyprctl", &["-j", "monitors"]).ok()?)
}

fn logind_locked(session: &Session) -> bool {
    run("loginctl", &["show-session", &session.id, "-p", "LockedHint", "--value"])
        .is_ok_and(|v| v.trim() == "yes")
}

fn is_locked(session: &Session) -> bool {
    logind_locked(session) || desktop_locked(session) == Some(true)
}

/// Attend que la session se déclare verrouillée, dans la limite du délai.
fn wait_locked(session: &Session) -> bool {
    let deadline = Instant::now() + LOCK_SETTLE;
    loop {
        if is_locked(session) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(LOCK_POLL);
    }
}

/// Script essayant le premier verrou disponible dans la session.
fn fallback_lock_script() -> String {
    let mut lockers: Vec<String> = FALLBACK_LOCKERS.iter().map(|l| (*l).to_string()).collect();
    // Un administrateur peut imposer sa propre commande de verrouillage.
    let custom = std::env::var("COTUTELLE_LOCK_COMMAND").unwrap_or_default();
    if !custom.trim().is_empty() {
        lockers.insert(0, custom.trim().to_string());
    }
    let attempts: String = lockers
        .iter()
        .map(|l| {
            let program = l.split_whitespace().next().unwrap_or_default();
            format!("if command -v {program} >/dev/null 2>&1; then ({l} >/dev/null 2>&1 &); exit 0; fi; ")
        })
        .collect();
    format!("{attempts}exit 1")
}

/// Lit la sortie de `resolvectl dns` et rend les résolveurs des interfaces :
///
/// ```text
/// Global: 127.0.0.1:5354
/// Link 2 (enp1s0): 172.30.0.10
/// Link 3 (enp2s0): 192.168.3.1 fe80::1%3
/// ```
///
/// La ligne `Global` (c'est l'agent lui-même) et la boucle locale sont écartées.
fn parse_resolvectl_dns(text: &str) -> Vec<SocketAddr> {
    let mut servers = Vec::new();
    for line in text.lines().filter(|l| l.trim_start().starts_with("Link ")) {
        let Some((_, list)) = line.split_once("):") else { continue };
        for entry in list.split_whitespace() {
            // Forme `adresse`, `adresse:port` ou `adresse#nom` ; les adresses
            // de portée lien (`%interface`) ne sont pas utilisables telles quelles.
            let entry = entry.split('#').next().unwrap_or(entry);
            let parsed = entry.parse::<SocketAddr>().ok().or_else(|| {
                entry.parse::<std::net::IpAddr>().ok().map(|ip| SocketAddr::new(ip, 53))
            });
            if let Some(addr) = parsed.filter(|a| !a.ip().is_loopback() && !servers.contains(a)) {
                servers.push(addr);
            }
        }
    }
    servers
}

/// Comptes humains de `/etc/passwd` : UID 1000 à 59999 avec un vrai shell.
fn parse_passwd(passwd: &str) -> Vec<String> {
    passwd
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            let [name, _, uid, _, _, _, shell] = fields.as_slice() else { return None };
            let uid: u32 = uid.parse().ok()?;
            let real_shell = !shell.ends_with("nologin") && !shell.ends_with("false");
            ((1000..60000).contains(&uid) && real_shell).then(|| (*name).to_string())
        })
        .collect()
}

pub(crate) fn resolved_dropin(resolver: SocketAddr) -> String {
    format!(
        "# Géré par {MARKER} — ne pas modifier.\n[Resolve]\nDNS={resolver}\nDomains=~.\nFallbackDNS=\nDNSOverTLS=no\nLLMNR=no\n"
    )
}

/// Règles nftables : seuls root (l'agent) et la boucle locale peuvent parler
/// DNS. Tout autre trafic DNS sortant, en clair ou sur TLS, est rejeté.
pub(crate) fn nft_ruleset() -> String {
    format!(
        "table inet {NFT_TABLE}\n\
         delete table inet {NFT_TABLE}\n\
         table inet {NFT_TABLE} {{\n\
         \tchain output {{\n\
         \t\ttype filter hook output priority filter; policy accept;\n\
         \t\toifname \"lo\" accept\n\
         \t\tmeta skuid 0 accept\n\
         \t\tudp dport {{ 53, 853 }} reject\n\
         \t\ttcp dport {{ 53, 853 }} reject with tcp reset\n\
         \t}}\n\
         }}\n"
    )
}

const FIREFOX_POLICY_JSON: &str = r#"{
  "_comment": "Géré par cotutelle — ne pas modifier.",
  "policies": {
    "DNSOverHTTPS": { "Enabled": false, "Locked": true }
  }
}
"#;

const CHROME_POLICY_JSON: &str = r#"{
  "DnsOverHttpsMode": "off",
  "BuiltInDnsClientEnabled": false
}
"#;

fn write_if_ours(path: &str, content: &str) -> Result<()> {
    let path = Path::new(path);
    if let Ok(existing) = std::fs::read_to_string(path) {
        if existing == content {
            return Ok(());
        }
        // Ne jamais écraser une configuration posée par quelqu'un d'autre.
        if !existing.contains(MARKER) && !path.ends_with("cotutelle.json") {
            bail!("{} existe déjà et n'est pas géré par Cotutelle", path.display());
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content).with_context(|| format!("écriture de {}", path.display()))
}

/// Fait relire sa configuration à systemd-resolved.
///
/// Sans `--no-block`, l'appel attend la fin du redémarrage. Or l'unité de
/// l'agent est ordonnée après systemd-resolved : lancé depuis l'arrêt de
/// l'agent, le redémarrage attendrait cet arrêt, qui l'attendrait en retour.
fn restart_resolved() -> Result<()> {
    run("systemctl", &["--no-block", "restart", "systemd-resolved"]).map(drop)
}

/// Pose la politique des navigateurs installés. Un navigateur absent n'a
/// droit à aucun fichier : on ne crée rien dans `/etc` pour lui. Appelé aussi
/// périodiquement, pour couvrir un navigateur installé après coup.
fn install_browser_policies() {
    for browser in BROWSERS {
        if !browser.installed_if.iter().any(|p| Path::new(p).exists()) {
            continue;
        }
        if let Err(e) = write_if_ours(browser.policy, browser.content) {
            tracing::warn!(error = %e, "politique navigateur non installée");
        }
    }
}

/// Supprime un fichier posé par l'agent. Les répertoires sont laissés en
/// place : rien ne permet de savoir s'ils existaient avant lui.
fn remove_if_ours(path: &str) {
    let ours = Path::new(path).ends_with("cotutelle.json")
        || std::fs::read_to_string(path).is_ok_and(|c| c.contains(MARKER));
    if ours {
        let _ = std::fs::remove_file(path);
    }
}

impl Platform for Linux {
    fn name(&self) -> &'static str {
        "linux"
    }

    fn hostname(&self) -> String {
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .map(|h| h.trim().to_string())
            .unwrap_or_else(|_| "inconnu".to_string())
    }

    fn os_accounts(&self) -> Vec<String> {
        std::fs::read_to_string("/etc/passwd").map(|p| parse_passwd(&p)).unwrap_or_default()
    }

    fn sessions(&self) -> Result<Vec<Session>> {
        let list = run("loginctl", &["list-sessions", "--no-legend"])?;
        let mut sessions = Vec::new();
        for id in list.lines().filter_map(|l| l.split_whitespace().next()) {
            let mut args = vec!["show-session", id];
            args.extend(SESSION_PROPERTIES.iter().flat_map(|p| ["-p", p]));
            // Une session peut se fermer entre les deux appels : on l'ignore.
            let Ok(text) = run("loginctl", &args) else { continue };
            if let Some(mut session) = session_from_properties(id, &parse_properties(&text)) {
                // Certains bureaux ne renseignent pas LockedHint : on leur demande.
                if session.active && !session.locked && desktop_locked(&session) == Some(true) {
                    session.locked = true;
                }
                sessions.push(session);
            }
        }
        Ok(sessions)
    }

    fn lock_session(&self, session: &Session) -> Result<()> {
        // Voie normale : logind relaie l'ordre au bureau (GNOME, KDE…).
        let asked = run("loginctl", &["lock-session", &session.id]);
        if asked.is_ok() && wait_locked(session) {
            return Ok(());
        }
        // Le bureau n'a pas réagi : on lance son verrou directement.
        run_in_session(session, "sh", &["-c", &fallback_lock_script()])
            .context("aucun verrou d'écran disponible dans la session")?;
        if wait_locked(session) {
            return Ok(());
        }
        bail!("la session de {} ne s'est pas verrouillée", session.user)
    }

    fn notify(&self, session: &Session, title: &str, body: &str) -> Result<()> {
        run_in_session(
            session,
            "notify-send",
            &["--app-name=Cotutelle", "--urgency=critical", "--icon=appointment-soon", title, body],
        )
        .map(drop)
    }

    fn network_dns(&self) -> Vec<SocketAddr> {
        run("resolvectl", &["dns"]).map(|out| parse_resolvectl_dns(&out)).unwrap_or_default()
    }

    fn enforce_dns(&self, resolver: SocketAddr) -> Result<()> {
        write_if_ours(RESOLVED_DROPIN, &resolved_dropin(resolver))?;
        restart_resolved()?;

        let rules = std::env::temp_dir().join("cotutelle.nft");
        std::fs::write(&rules, nft_ruleset())?;
        let applied = run("nft", &["-f", &rules.to_string_lossy()]);
        let _ = std::fs::remove_file(&rules);
        applied?;

        // Les politiques navigateur sont un plus : leur échec n'est pas bloquant.
        install_browser_policies();
        Ok(())
    }

    fn release_dns(&self) -> Result<()> {
        let _ = run("nft", &["delete", "table", "inet", NFT_TABLE]);
        remove_if_ours(RESOLVED_DROPIN);
        for browser in BROWSERS {
            remove_if_ours(browser.policy);
        }
        restart_resolved()
    }

    fn check_enforcement(&self, resolver: SocketAddr) -> Option<(TamperKind, String)> {
        if std::fs::read_to_string(RESOLVED_DROPIN).ok().as_deref()
            != Some(&resolved_dropin(resolver))
        {
            return Some((TamperKind::ResolverConfigChanged, RESOLVED_DROPIN.to_string()));
        }
        if run("nft", &["list", "table", "inet", NFT_TABLE]).is_err() {
            return Some((TamperKind::FirewallRulesMissing, format!("table inet {NFT_TABLE}")));
        }
        install_browser_policies();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphical_user_session_is_kept() {
        let text = "Name=louis\nUser=1001\nActive=yes\nLockedHint=no\nIdleHint=no\nClass=user\nType=wayland\nRemote=no\nSeat=seat0\n";
        let session = session_from_properties("4", &parse_properties(text)).unwrap();
        assert_eq!((session.user.as_str(), session.uid, session.in_use()), ("louis", 1001, true));
    }

    #[test]
    fn greeter_ssh_and_services_are_ignored() {
        let greeter =
            "Name=gdm\nUser=120\nActive=yes\nClass=greeter\nType=wayland\nRemote=no\nSeat=seat0\n";
        let ssh = "Name=louis\nUser=1001\nActive=yes\nClass=user\nType=tty\nRemote=yes\nSeat=\n";
        let manager =
            "Name=louis\nUser=1001\nActive=yes\nClass=manager\nType=unspecified\nRemote=no\n";
        for text in [greeter, ssh, manager] {
            assert_eq!(session_from_properties("1", &parse_properties(text)), None);
        }
    }

    #[test]
    fn a_session_described_by_the_requested_properties_is_recognised() {
        // Ce que logind renvoie pour une session locale, limité aux seules
        // propriétés demandées : si l'analyse en lit une autre, ce test échoue.
        let values = [
            ("Name", "louis"),
            ("User", "1001"),
            ("Active", "yes"),
            ("LockedHint", "no"),
            ("IdleHint", "no"),
            ("Class", "user"),
            ("Type", "wayland"),
            ("Remote", "no"),
            ("Seat", "seat0"),
        ];
        let text: String = values
            .iter()
            .filter(|(key, _)| SESSION_PROPERTIES.contains(key))
            .map(|(key, value)| format!("{key}={value}\n"))
            .collect();
        assert_eq!(values.len(), SESSION_PROPERTIES.len());
        assert!(session_from_properties("1", &parse_properties(&text)).is_some());
    }

    #[test]
    fn environ_is_split_into_valid_pairs() {
        let raw = b"WAYLAND_DISPLAY=wayland-1\0OMARCHY_PATH=/usr/share/omarchy\0BAD NAME=x\0=vide\0A=b=c\0";
        let env = parse_environ(raw);
        let expected: Vec<(String, String)> = [
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("OMARCHY_PATH", "/usr/share/omarchy"),
            ("A", "b=c"),
        ]
        .iter()
        .map(|(n, v)| ((*n).to_string(), (*v).to_string()))
        .collect();
        assert_eq!(env, expected);
    }

    #[test]
    fn hyprland_lock_state_is_read_from_monitors() {
        // Sorties relevées sur une vraie session Omarchy, avant et après verrouillage.
        let unlocked = r#"[{"name":"Virtual-1","solitaryBlockedBy":["WINDOWED","CANDIDATE"]}]"#;
        let locked =
            r#"[{"name":"Virtual-1","solitaryBlockedBy":["LOCK","WINDOWED","CANDIDATE"]}]"#;
        assert_eq!(hyprland_locked(unlocked), Some(false));
        assert_eq!(hyprland_locked(locked), Some(true));
        // Version de Hyprland sans ce champ, ou sortie illisible : on ne conclut pas.
        assert_eq!(hyprland_locked(r#"[{"name":"Virtual-1"}]"#), None);
        assert_eq!(hyprland_locked("erreur"), None);
    }

    #[test]
    fn fallback_script_tries_lockers_in_order() {
        let script = fallback_lock_script();
        let omarchy = script.find("omarchy-system-lock").unwrap();
        let swaylock = script.find("command -v swaylock").unwrap();
        assert!(omarchy < swaylock && script.ends_with("exit 1"));
        assert!(script.contains("(swaylock -f >/dev/null 2>&1 &)"));
    }

    #[test]
    fn network_dns_is_read_from_resolvectl() {
        // Sortie relevée sur une vraie machine, agent actif.
        let text = "Global: 127.0.0.1:5354\n\
                    Link 3 (enp2s0): 192.168.3.1\n\
                    Link 2 (enp1s0): 172.30.0.10 192.168.3.1 9.9.9.9#dns.quad9.net fe80::1%3\n\
                    Link 4 (docker0):\n\
                    Link 5 (lo): 127.0.0.53\n";
        let expected: Vec<SocketAddr> = ["192.168.3.1:53", "172.30.0.10:53", "9.9.9.9:53"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(parse_resolvectl_dns(text), expected);
        assert!(parse_resolvectl_dns("Global: 127.0.0.1:5354\n").is_empty());
    }

    #[test]
    fn locked_session_is_not_in_use() {
        let text = "Name=louis\nUser=1001\nActive=yes\nLockedHint=yes\nIdleHint=no\nClass=user\nType=x11\nRemote=no\nSeat=seat0\n";
        assert!(!session_from_properties("4", &parse_properties(text)).unwrap().in_use());
    }

    #[test]
    fn passwd_keeps_human_accounts_only() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                      daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin\n\
                      papa:x:1000:1000:Papa,,,:/home/papa:/bin/bash\n\
                      louis:x:1001:1001::/home/louis:/usr/bin/zsh\n\
                      svc:x:1002:1002::/home/svc:/usr/sbin/nologin\n\
                      nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n";
        assert_eq!(parse_passwd(passwd), ["papa", "louis"]);
    }

    #[test]
    fn generated_configuration_is_stable() {
        let dropin = resolved_dropin("127.0.0.1:5354".parse().unwrap());
        assert!(dropin.contains("DNS=127.0.0.1:5354\n") && dropin.contains("Domains=~.\n"));
        let rules = nft_ruleset();
        assert!(
            rules.contains("meta skuid 0 accept") && rules.contains("udp dport { 53, 853 } reject")
        );
        // La table est déclarée avant d'être supprimée : le script est rejouable.
        assert!(rules.starts_with("table inet cotutelle\ndelete table inet cotutelle\n"));
    }
}
