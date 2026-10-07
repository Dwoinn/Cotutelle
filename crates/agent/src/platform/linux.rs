//! Linux : logind pour les sessions, systemd-resolved et nftables pour le DNS.

use super::{Platform, Session};
use anyhow::{Context, Result, bail};
use cotutelle_common::protocol::TamperKind;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;

const RESOLVED_DROPIN: &str = "/etc/systemd/resolved.conf.d/cotutelle.conf";
const NFT_TABLE: &str = "cotutelle";
const FIREFOX_POLICY: &str = "/etc/firefox/policies/policies.json";
const CHROME_POLICIES: &[&str] = &[
    "/etc/opt/chrome/policies/managed/cotutelle.json",
    "/etc/chromium/policies/managed/cotutelle.json",
    "/etc/chromium-browser/policies/managed/cotutelle.json",
    "/etc/brave/policies/managed/cotutelle.json",
    "/etc/opt/edge/policies/managed/cotutelle.json",
];
/// Marqueur des fichiers que l'agent a le droit de réécrire et de supprimer.
const MARKER: &str = "cotutelle";

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
    Some(Session {
        id: id.to_string(),
        user: (*props.get("Name")?).to_string(),
        uid: props.get("User")?.parse().ok()?,
        active: yes("Active"),
        locked: yes("LockedHint"),
        idle: yes("IdleHint"),
    })
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
            // Une session peut se fermer entre les deux appels : on l'ignore.
            let Ok(text) = run(
                "loginctl",
                &[
                    "show-session",
                    id,
                    "-p",
                    "Name",
                    "-p",
                    "User",
                    "-p",
                    "Active",
                    "-p",
                    "LockedHint",
                    "-p",
                    "IdleHint",
                    "-p",
                    "Class",
                    "-p",
                    "Type",
                    "-p",
                    "Remote",
                ],
            ) else {
                continue;
            };
            sessions.extend(session_from_properties(id, &parse_properties(&text)));
        }
        Ok(sessions)
    }

    fn lock_session(&self, session: &Session) -> Result<()> {
        run("loginctl", &["lock-session", &session.id]).map(drop)
    }

    fn notify(&self, session: &Session, title: &str, body: &str) -> Result<()> {
        let runtime_dir = format!("/run/user/{}", session.uid);
        let bus = format!("DBUS_SESSION_BUS_ADDRESS=unix:path={runtime_dir}/bus");
        let xdg = format!("XDG_RUNTIME_DIR={runtime_dir}");
        run(
            "runuser",
            &[
                "-u",
                &session.user,
                "--",
                "env",
                &bus,
                &xdg,
                "notify-send",
                "--app-name=Cotutelle",
                "--urgency=critical",
                "--icon=appointment-soon",
                title,
                body,
            ],
        )
        .map(drop)
    }

    fn enforce_dns(&self, resolver: SocketAddr) -> Result<()> {
        write_if_ours(RESOLVED_DROPIN, &resolved_dropin(resolver))?;
        run("systemctl", &["restart", "systemd-resolved"])?;

        let rules = std::env::temp_dir().join("cotutelle.nft");
        std::fs::write(&rules, nft_ruleset())?;
        let applied = run("nft", &["-f", &rules.to_string_lossy()]);
        let _ = std::fs::remove_file(&rules);
        applied?;

        // Les politiques navigateur sont un plus : leur échec n'est pas bloquant.
        if let Err(e) = write_if_ours(FIREFOX_POLICY, FIREFOX_POLICY_JSON) {
            tracing::warn!(error = %e, "politique Firefox non installée");
        }
        for path in CHROME_POLICIES {
            if let Err(e) = write_if_ours(path, CHROME_POLICY_JSON) {
                tracing::warn!(error = %e, "politique Chromium non installée");
            }
        }
        Ok(())
    }

    fn release_dns(&self) -> Result<()> {
        let _ = run("nft", &["delete", "table", "inet", NFT_TABLE]);
        remove_if_ours(RESOLVED_DROPIN);
        remove_if_ours(FIREFOX_POLICY);
        for path in CHROME_POLICIES {
            remove_if_ours(path);
        }
        run("systemctl", &["restart", "systemd-resolved"]).map(drop)
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
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphical_user_session_is_kept() {
        let text = "Name=louis\nUser=1001\nActive=yes\nLockedHint=no\nIdleHint=no\nClass=user\nType=wayland\nRemote=no\n";
        let session = session_from_properties("4", &parse_properties(text)).unwrap();
        assert_eq!((session.user.as_str(), session.uid, session.in_use()), ("louis", 1001, true));
    }

    #[test]
    fn greeter_ssh_and_services_are_ignored() {
        let greeter = "Name=gdm\nUser=120\nActive=yes\nClass=greeter\nType=wayland\nRemote=no\n";
        let ssh = "Name=louis\nUser=1001\nActive=yes\nClass=user\nType=tty\nRemote=yes\n";
        let manager =
            "Name=louis\nUser=1001\nActive=yes\nClass=manager\nType=unspecified\nRemote=no\n";
        for text in [greeter, ssh, manager] {
            assert_eq!(session_from_properties("1", &parse_properties(text)), None);
        }
    }

    #[test]
    fn locked_session_is_not_in_use() {
        let text = "Name=louis\nUser=1001\nActive=yes\nLockedHint=yes\nIdleHint=no\nClass=user\nType=x11\nRemote=no\n";
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
