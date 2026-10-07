//! Agent Cotutelle : tourne en service root sur l'appareil protégé.
//!
//! Phase 0 : charge la politique mise en cache (ou une politique vide, qui
//! bloque tout) et affiche l'état. Le résolveur DNS, nftables et logind
//! arrivent en phase 1 derrière le trait [`platform::Platform`].

mod platform;

use anyhow::{Context, Result};
use chrono::{Datelike, Local};
use clap::Parser;
use cotutelle_common::protocol::SessionPolicy;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "cotutelle-agent", version, about)]
struct Cli {
    /// URL du serveur Cotutelle.
    #[arg(
        long,
        env = "COTUTELLE_SERVER",
        default_value = "http://cotutelle.local:8080"
    )]
    server: String,

    /// Répertoire d'état : politique en cache, listes, jeton.
    #[arg(
        long,
        env = "COTUTELLE_STATE_DIR",
        default_value = "/var/lib/cotutelle"
    )]
    state_dir: PathBuf,
}

/// Charge la dernière politique connue (D3 : jamais d'ouverture par défaut).
fn load_cached_policy(state_dir: &std::path::Path) -> Result<Vec<SessionPolicy>> {
    let path = state_dir.join("policy.json");
    if !path.exists() {
        tracing::warn!(?path, "aucune politique en cache : tout est bloqué");
        return Ok(Vec::new());
    }
    let raw = std::fs::read(&path).with_context(|| format!("lecture de {}", path.display()))?;
    serde_json::from_slice(&raw).context("politique en cache illisible")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let platform = platform::detect();
    tracing::info!(server = %cli.server, platform = platform.name(), "démarrage de l'agent");

    let sessions = load_cached_policy(&cli.state_dir)?;
    let now = Local::now();
    for s in &sessions {
        let allowed = s.policy.schedule.allows(now.weekday(), now.time());
        tracing::info!(
            account = s.os_account.as_deref().unwrap_or("<défaut>"),
            allowed,
            grants = s.grants.len(),
            "politique chargée"
        );
    }

    Ok(())
}
