//! Intégration avec le système d'exploitation. Tout ce qui diffère entre
//! Linux, Windows et macOS passe par ce trait.

use anyhow::Result;

// Les méthodes sont consommées en phase 1 ; on garde le contrat visible dès maintenant.
#[allow(dead_code)]
pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;

    /// Verrouille la session graphique du compte donné.
    fn lock_session(&self, os_account: &str) -> Result<()>;

    /// Affiche un préavis dans la session de l'enfant (« 5 minutes restantes »).
    fn notify(&self, os_account: &str, title: &str, body: &str) -> Result<()>;

    /// Force toutes les résolutions DNS vers le résolveur local et bloque
    /// les sorties DNS/DoH directes.
    fn enforce_dns_redirect(&self) -> Result<()>;
}

pub struct Linux;

impl Platform for Linux {
    fn name(&self) -> &'static str {
        "linux"
    }

    fn lock_session(&self, _os_account: &str) -> Result<()> {
        // Phase 1 : logind via D-Bus (zbus), `LockSession` sur chaque session
        // du compte.
        anyhow::bail!("non implémenté")
    }

    fn notify(&self, _os_account: &str, _title: &str, _body: &str) -> Result<()> {
        // Phase 1 : helper de session → org.freedesktop.Notifications.
        anyhow::bail!("non implémenté")
    }

    fn enforce_dns_redirect(&self) -> Result<()> {
        // Phase 1 : table nftables dédiée `cotutelle`.
        anyhow::bail!("non implémenté")
    }
}

pub fn detect() -> Box<dyn Platform> {
    Box::new(Linux)
}
