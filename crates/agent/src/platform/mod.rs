//! Intégration avec le système d'exploitation. Tout ce qui diffère entre
//! Linux, Windows et macOS passe par le trait [`Platform`].

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(any(target_os = "linux", windows)))]
mod unsupported;
#[cfg(windows)]
pub(crate) mod windows;

use anyhow::Result;
use cotutelle_common::protocol::TamperKind;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Session ouverte sur l'appareil.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub user: String,
    pub uid: u32,
    /// Session au premier plan sur son siège.
    pub active: bool,
    pub locked: bool,
    /// Personne ne s'en sert : sans activité selon logind, ou écran éteint
    /// par l'économiseur du bureau. Le temps d'écran ne compte pas.
    pub idle: bool,
}

impl Session {
    /// Quelqu'un est réellement devant l'écran de cette session.
    pub fn in_use(&self) -> bool {
        self.active && !self.locked
    }

    /// État lisible, pour le journal et `cotutelle-agent status`.
    pub fn state(&self) -> &'static str {
        if !self.active {
            "arrière-plan"
        } else if self.locked {
            "verrouillée"
        } else if self.idle {
            "écran éteint ou inactive"
        } else {
            "en usage"
        }
    }
}

pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;

    fn hostname(&self) -> String;

    /// Comptes de session « humains » présents sur l'appareil.
    fn os_accounts(&self) -> Vec<String>;

    /// Nom du compte portant cet identifiant d'utilisateur. Sous Windows,
    /// l'assistant est reconnu au jeton de son processus : inutilisé.
    #[cfg_attr(windows, allow(dead_code))]
    fn account_name(&self, uid: u32) -> Option<String>;

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

#[cfg(windows)]
pub fn detect() -> Box<dyn Platform> {
    Box::new(windows::Windows)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn detect() -> Box<dyn Platform> {
    Box::new(unsupported::Unsupported)
}

/// Répertoire d'état par défaut : réservé aux administrateurs.
pub fn default_state_dir() -> PathBuf {
    if cfg!(windows) {
        let program_data = std::env::var_os("ProgramData")
            .map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
        program_data.join("Cotutelle")
    } else {
        PathBuf::from("/var/lib/cotutelle")
    }
}

/// Adresse du résolveur local. Le client DNS de Windows ne parle qu'au port
/// 53 ; sous Linux, systemd-resolved accepte un autre port.
pub const DEFAULT_DNS_ADDR: &str = if cfg!(windows) { "127.0.0.1:53" } else { "127.0.0.1:5354" };

/// Liaison avec l'assistant de session : socket Unix, ou tube nommé.
pub const DEFAULT_HELPER_SOCKET: &str =
    if cfg!(windows) { r"\\.\pipe\cotutelle-agent" } else { "/run/cotutelle/agent.sock" };

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
