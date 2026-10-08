//! Intégration avec le système d'exploitation. Tout ce qui diffère entre
//! Linux, Windows et macOS passe par le trait [`Platform`].

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_os = "linux"))]
mod unsupported;

use anyhow::Result;
use cotutelle_common::protocol::TamperKind;
use std::net::SocketAddr;
use std::path::Path;

/// Session ouverte sur l'appareil.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub user: String,
    pub uid: u32,
    /// Session au premier plan sur son siège.
    pub active: bool,
    pub locked: bool,
    pub idle: bool,
}

impl Session {
    /// Quelqu'un est réellement devant l'écran de cette session.
    pub fn in_use(&self) -> bool {
        self.active && !self.locked
    }
}

pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;

    fn hostname(&self) -> String;

    /// Comptes de session « humains » présents sur l'appareil.
    fn os_accounts(&self) -> Vec<String>;

    /// Sessions interactives ouvertes (hors écran de connexion et services).
    fn sessions(&self) -> Result<Vec<Session>>;

    fn lock_session(&self, session: &Session) -> Result<()>;

    /// Affiche un message dans la session de l'enfant.
    fn notify(&self, session: &Session, title: &str, body: &str) -> Result<()>;

    /// Résolveurs DNS annoncés par le réseau (DHCP), hors boucle locale.
    fn network_dns(&self) -> Vec<SocketAddr>;

    /// Force toutes les résolutions DNS vers le résolveur local de l'agent et
    /// bloque les sorties DNS directes.
    fn enforce_dns(&self, resolver: SocketAddr) -> Result<()>;

    /// Retire tout ce que [`Platform::enforce_dns`] a mis en place.
    fn release_dns(&self) -> Result<()>;

    /// Vérifie que la protection est intacte ; rend la nature du problème sinon.
    fn check_enforcement(&self, resolver: SocketAddr) -> Option<(TamperKind, String)>;
}

#[cfg(target_os = "linux")]
pub fn detect() -> Box<dyn Platform> {
    Box::new(linux::Linux)
}

#[cfg(not(target_os = "linux"))]
pub fn detect() -> Box<dyn Platform> {
    Box::new(unsupported::Unsupported)
}

/// Attend la demande d'arrêt : Ctrl-C, ou SIGTERM qu'envoie systemd.
#[cfg(unix)]
pub async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let Ok(mut terminate) = signal(SignalKind::terminate()) else {
        let _ = tokio::signal::ctrl_c().await;
        return;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(not(unix))]
pub async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

/// Crée (ou vide) un fichier réservé à son propriétaire.
#[cfg(unix)]
pub fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)
}

/// Hors Unix, le fichier hérite des droits du répertoire d'état.
#[cfg(not(unix))]
pub fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::create(path)
}
