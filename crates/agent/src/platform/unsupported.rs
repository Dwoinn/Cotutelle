//! Systèmes sans intégration : l'agent s'y compile, mais ne sait pas encore y
//! protéger quoi que ce soit. Il refuse donc de démarrer hors `--dry-run`
//! plutôt que de laisser croire à une protection (D3).

use super::{Platform, Session};
use anyhow::{Result, bail};
use cotutelle_common::protocol::TamperKind;
use std::net::SocketAddr;

pub struct Unsupported;

const OS: &str = std::env::consts::OS;

impl Platform for Unsupported {
    fn name(&self) -> &'static str {
        OS
    }

    fn hostname(&self) -> String {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "inconnu".to_string())
    }

    fn os_accounts(&self) -> Vec<String> {
        Vec::new()
    }

    fn sessions(&self) -> Result<Vec<Session>> {
        Ok(Vec::new())
    }

    fn lock_session(&self, _session: &Session) -> Result<()> {
        bail!("verrouillage de session non pris en charge sous {OS}")
    }

    fn notify(&self, _session: &Session, _title: &str, _body: &str) -> Result<()> {
        bail!("notifications non prises en charge sous {OS}")
    }

    fn network_dns(&self) -> Vec<SocketAddr> {
        Vec::new()
    }

    fn enforce_dns(&self, _resolver: SocketAddr) -> Result<()> {
        bail!("l'agent Cotutelle ne sait pas encore protéger un appareil sous {OS}")
    }

    fn release_dns(&self) -> Result<()> {
        Ok(())
    }

    fn check_enforcement(&self, _resolver: SocketAddr) -> Option<(TamperKind, String)> {
        None
    }
}
