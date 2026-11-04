//! Windows : sessions par WTS, résolveur imposé par une règle NRPT, DNS
//! direct des comptes ordinaires rejeté par le pare-feu Windows (D20).
//!
//! Ce qui a une commande PowerShell passe par [`powershell`], comme `nft` et
//! `loginctl` sous Linux ; le reste (sessions, verrou, message, jeton du
//! pair) parle à l'API Win32 par le crate `windows`.

mod dns;
mod net;
pub(crate) mod wts;

use super::{Platform, Session};
use anyhow::{Context, Result, bail};
use cotutelle_common::protocol::TamperKind;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

/// Délai laissé au bureau pour se déclarer verrouillé.
const LOCK_SETTLE: Duration = Duration::from_millis(2500);
const LOCK_POLL: Duration = Duration::from_millis(250);

pub struct Windows;

/// Chaîne UTF-16 terminée par un nul, pour l'API Win32.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Lit un tampon UTF-16 à taille fixe, jusqu'au premier nul.
pub(crate) fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// Répertoire système, `C:\Windows` en général.
pub(crate) fn system_root() -> PathBuf {
    std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from)
}

/// Lance un script PowerShell et rend sa sortie. Une erreur d'exécution
/// (cmdlet absent, accès refusé) fait échouer le script dès la première
/// commande en faute.
pub(crate) fn powershell(script: &str) -> Result<String> {
    let exe = system_root().join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let script = format!("$ErrorActionPreference = 'Stop'\n{script}");
    let output = Command::new(&exe)
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .output()
        .with_context(|| format!("lancement de {}", exe.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("échec sans message");
        bail!("PowerShell : {}", first.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Lance un programme du système et rend sa sortie.
pub(crate) fn run(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(system_root().join("System32").join(program))
        .args(args)
        .output()
        .with_context(|| format!("lancement de {program}"))?;
    if !output.status.success() {
        bail!("{program} {} : {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Attend que la session se déclare verrouillée, dans la limite du délai.
fn wait_locked(session: &Session) -> bool {
    let deadline = Instant::now() + LOCK_SETTLE;
    loop {
        if wts::is_locked(session.uid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(LOCK_POLL);
    }
}

impl Platform for Windows {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn hostname(&self) -> String {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "inconnu".to_string())
    }

    fn os_accounts(&self) -> Vec<String> {
        net::local_accounts().unwrap_or_default()
    }

    /// Pas d'uid sous Windows : l'assistant de session est reconnu au jeton
    /// de son processus, pas par ce chemin.
    fn account_name(&self, _uid: u32) -> Option<String> {
        None
    }

    fn sessions(&self) -> Result<Vec<Session>> {
        wts::sessions()
    }

    fn lock_session(&self, session: &Session) -> Result<()> {
        // Voie normale : LockWorkStation, lancé dans la session elle-même.
        match wts::lock_workstation(session.uid) {
            Ok(()) if wait_locked(session) => return Ok(()),
            Ok(()) => {
                tracing::info!(user = %session.user, "la session ne se déclare pas verrouillée")
            }
            Err(e) => tracing::info!(user = %session.user, error = %e, "verrou dans la session"),
        }
        // Secours : la déconnexion de la session ramène l'écran de connexion,
        // les programmes de l'enfant continuent en arrière-plan.
        wts::disconnect(session.uid).context("déconnexion de la session")?;
        if wait_locked(session) {
            return Ok(());
        }
        bail!("la session de {} ne s'est pas verrouillée", session.user)
    }

    fn notify(&self, session: &Session, title: &str, body: &str) -> Result<()> {
        wts::send_message(session.uid, title, body)
    }

    fn network_dns(&self) -> Vec<SocketAddr> {
        net::adapter_dns().unwrap_or_default()
    }

    fn enforce_dns(&self, resolver: SocketAddr) -> Result<()> {
        dns::enforce(resolver)
    }

    fn release_dns(&self) -> Result<()> {
        dns::release()
    }

    fn check_enforcement(&self, resolver: SocketAddr) -> Option<(TamperKind, String)> {
        dns::check(resolver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_strings_roundtrip() {
        let text = "Écran verrouillé";
        let buffer = wide(text);
        assert_eq!(buffer.last(), Some(&0));
        assert_eq!(from_wide(&buffer), text);
        // Un tampon à taille fixe est lu jusqu'au premier nul.
        let mut fixed = [0u16; 8];
        fixed[..3].copy_from_slice(&[b'a' as u16, b'b' as u16, b'c' as u16]);
        assert_eq!(from_wide(&fixed), "abc");
    }
}
