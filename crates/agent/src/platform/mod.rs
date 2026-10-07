//! Intégration avec le système d'exploitation. Tout ce qui diffère entre
//! Linux, Windows et macOS passe par le trait [`Platform`].

mod linux;

use anyhow::Result;
use cotutelle_common::protocol::TamperKind;
use std::net::SocketAddr;

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

pub fn detect() -> Box<dyn Platform> {
    Box::new(linux::Linux)
}
