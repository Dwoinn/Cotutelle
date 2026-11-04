//! Protection DNS sous Windows : une règle NRPT envoie toute résolution au
//! résolveur de l'agent, le pare-feu Windows rejette le DNS direct des
//! comptes ordinaires, et les navigateurs reçoivent par le registre la
//! politique qui désactive leur DNS chiffré intégré.
//!
//! Le pare-feu ne sait pas détourner : le DNS direct est rejeté, pas
//! redirigé comme sous Linux. Le résolveur système, lui, suit la NRPT.

use super::{powershell, run};
use anyhow::{Context, Result, bail};
use cotutelle_common::protocol::TamperKind;
use std::net::{IpAddr, SocketAddr};
use windows_registry::{Key, LOCAL_MACHINE};

/// Marque la règle NRPT de l'agent.
const NRPT_COMMENT: &str = "cotutelle";
/// Où le client DNS range ses règles NRPT : les règles locales, puis celles
/// d'une stratégie de groupe.
const NRPT_KEYS: &[&str] = &[
    r"SYSTEM\CurrentControlSet\Services\Dnscache\Parameters\DnsPolicyConfig",
    r"SOFTWARE\Policies\Microsoft\Windows NT\DNSClient\DnsPolicyConfig",
];
const FIREWALL_GROUP: &str = "Cotutelle";
/// Règles de pare-feu : (identifiant, nom affiché, protocole).
const FIREWALL_RULES: &[(&str, &str, &str)] = &[
    ("CotutelleDnsUdp", "Cotutelle : DNS direct (UDP)", "UDP"),
    ("CotutelleDnsTcp", "Cotutelle : DNS direct (TCP)", "TCP"),
];
/// À qui les règles s'appliquent : tout le monde, sauf SYSTEM (l'agent, qui
/// relaie vers l'amont) et les administrateurs, comme root sous Linux.
const FIREWALL_USERS_SDDL: &str = "O:LSD:(D;;CC;;;SY)(D;;CC;;;BA)(A;;CC;;;S-1-1-0)";
const APP_PATHS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths";

#[derive(Clone, Copy)]
enum Engine {
    Firefox,
    Chromium,
}

/// Navigateur dont le DNS chiffré intégré doit être désactivé par politique.
struct Browser {
    engine: Engine,
    /// Clé de politique sous `HKLM`.
    policy: &'static str,
    /// Exécutable déclaré dans `App Paths` quand le navigateur est installé.
    executable: &'static str,
}

const BROWSERS: &[Browser] = &[
    Browser {
        engine: Engine::Firefox,
        policy: r"SOFTWARE\Policies\Mozilla\Firefox",
        executable: "firefox.exe",
    },
    Browser {
        engine: Engine::Chromium,
        policy: r"SOFTWARE\Policies\Google\Chrome",
        executable: "chrome.exe",
    },
    Browser {
        engine: Engine::Chromium,
        policy: r"SOFTWARE\Policies\Microsoft\Edge",
        executable: "msedge.exe",
    },
    Browser {
        engine: Engine::Chromium,
        policy: r"SOFTWARE\Policies\BraveSoftware\Brave",
        executable: "brave.exe",
    },
];

/// Script posant la règle NRPT et les règles de pare-feu. Rejouable : ce
/// que l'agent a déjà posé est d'abord retiré.
pub(crate) fn enforce_script(resolver: IpAddr) -> String {
    let mut script = release_script();
    script.push_str(&format!(
        "Add-DnsClientNrptRule -Namespace '.' -NameServers '{resolver}' -Comment '{NRPT_COMMENT}' -DisplayName 'Cotutelle' | Out-Null\n"
    ));
    for (name, label, protocol) in FIREWALL_RULES {
        script.push_str(&format!(
            "New-NetFirewallRule -Name '{name}' -DisplayName '{label}' -Group '{FIREWALL_GROUP}' \
             -Direction Outbound -Protocol {protocol} -RemotePort 53,853 -Action Block \
             -LocalUser '{FIREWALL_USERS_SDDL}' -Profile Any | Out-Null\n"
        ));
    }
    script
}

/// Script retirant ce que [`enforce_script`] a posé.
pub(crate) fn release_script() -> String {
    format!(
        "Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq '{NRPT_COMMENT}' }} | Remove-DnsClientNrptRule -Force\n\
         Remove-NetFirewallRule -Group '{FIREWALL_GROUP}' -ErrorAction SilentlyContinue\n"
    )
}

fn installed(browser: &Browser) -> bool {
    LOCAL_MACHINE.open(format!(r"{APP_PATHS}\{}", browser.executable)).is_ok()
}

fn install_policy(browser: &Browser) -> Result<()> {
    match browser.engine {
        Engine::Firefox => {
            let key = LOCAL_MACHINE.create(format!(r"{}\DNSOverHTTPS", browser.policy))?;
            key.set_u32("Enabled", 0)?;
            key.set_u32("Locked", 1)?;
        }
        Engine::Chromium => {
            let key = LOCAL_MACHINE.create(browser.policy)?;
            key.set_string("DnsOverHttpsMode", "off")?;
            key.set_u32("BuiltInDnsClientEnabled", 0)?;
        }
    }
    Ok(())
}

/// Retire la politique d'un navigateur, et sa clé si elle ne porte plus rien.
fn remove_policy(browser: &Browser) {
    let Ok(key) = LOCAL_MACHINE.open(browser.policy) else { return };
    match browser.engine {
        Engine::Firefox => {
            let _ = key.remove_tree("DNSOverHTTPS");
        }
        Engine::Chromium => {
            let _ = key.remove_value("DnsOverHttpsMode");
            let _ = key.remove_value("BuiltInDnsClientEnabled");
        }
    }
    let empty = |key: &Key| {
        key.keys().is_ok_and(|mut k| k.next().is_none())
            && key.values().is_ok_and(|mut v| v.next().is_none())
    };
    if empty(&key) {
        let (parent, name) = browser.policy.rsplit_once('\\').unwrap_or(("", browser.policy));
        if let Ok(parent) = LOCAL_MACHINE.open(parent) {
            let _ = parent.remove_tree(name);
        }
    }
}

/// Pose la politique des navigateurs installés. Un navigateur absent n'a
/// droit à aucune clé. Appelé aussi périodiquement, pour couvrir un
/// navigateur installé après coup.
fn install_browser_policies() {
    for browser in BROWSERS.iter().filter(|b| installed(b)) {
        if let Err(e) = install_policy(browser) {
            tracing::warn!(error = %e, key = browser.policy, "politique navigateur non installée");
        }
    }
}

/// Une règle NRPT de l'agent, telle que le client DNS l'a enregistrée,
/// pointe-t-elle sur le résolveur ?
fn nrpt_rule_present(resolver: IpAddr) -> bool {
    let expected = resolver.to_string();
    NRPT_KEYS.iter().any(|path| {
        let Ok(key) = LOCAL_MACHINE.open(path) else { return false };
        let Ok(rules) = key.keys() else { return false };
        rules.filter_map(|name| key.open(name).ok()).any(|rule| {
            rule.get_string("Comment").is_ok_and(|c| c == NRPT_COMMENT)
                && rule
                    .get_string("GenericDNSServers")
                    .is_ok_and(|servers| servers.split([';', ',']).any(|s| s.trim() == expected))
        })
    })
}

fn firewall_rule_present(label: &str) -> bool {
    run("netsh.exe", &["advfirewall", "firewall", "show", "rule", &format!("name={label}")]).is_ok()
}

fn flush_cache() {
    if let Err(e) = run("ipconfig.exe", &["/flushdns"]) {
        tracing::warn!(error = %e, "vidage du cache DNS");
    }
}

pub fn enforce(resolver: SocketAddr) -> Result<()> {
    // Le client DNS de Windows ne parle qu'au port 53.
    if resolver.port() != 53 {
        bail!("sous Windows, le résolveur local doit écouter sur le port 53 (--dns-addr)");
    }
    powershell(&enforce_script(resolver.ip())).context("règle NRPT et pare-feu")?;
    // Les politiques navigateur sont un plus : leur échec n'est pas bloquant.
    install_browser_policies();
    flush_cache();
    Ok(())
}

pub fn release() -> Result<()> {
    let released = powershell(&release_script()).map(drop);
    for browser in BROWSERS {
        remove_policy(browser);
    }
    flush_cache();
    released.context("retrait de la règle NRPT et du pare-feu")
}

pub fn check(resolver: SocketAddr) -> Option<(TamperKind, String)> {
    if !nrpt_rule_present(resolver.ip()) {
        return Some((TamperKind::ResolverConfigChanged, "règle NRPT".to_string()));
    }
    for (_, label, _) in FIREWALL_RULES {
        if !firewall_rule_present(label) {
            return Some((TamperKind::FirewallRulesMissing, (*label).to_string()));
        }
    }
    install_browser_policies();
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforce_script_is_replayable_and_spares_the_agent() {
        let script = enforce_script("127.0.0.1".parse().unwrap());
        // Ce qui est posé est d'abord retiré : le script se rejoue.
        let remove = script.find("Remove-DnsClientNrptRule").unwrap();
        let add =
            script.find("Add-DnsClientNrptRule -Namespace '.' -NameServers '127.0.0.1'").unwrap();
        assert!(remove < add);
        assert!(
            script.find("Remove-NetFirewallRule").unwrap()
                < script.find("New-NetFirewallRule").unwrap()
        );
        // UDP et TCP, ports 53 et 853, en rejet, sauf SYSTEM et administrateurs.
        assert_eq!(script.matches("-Action Block").count(), 2);
        assert!(
            script.contains("-Protocol UDP -RemotePort 53,853")
                && script.contains("-Protocol TCP -RemotePort 53,853")
        );
        assert!(script.contains("(D;;CC;;;SY)") && script.contains("(D;;CC;;;BA)"));
    }

    #[test]
    fn release_script_removes_only_what_is_ours() {
        let script = release_script();
        assert!(script.contains("$_.Comment -eq 'cotutelle'"));
        assert!(script.contains("-Group 'Cotutelle'"));
        assert!(!script.contains("Add-") && !script.contains("New-"));
    }
}
