//! État partagé du serveur.

use crate::blocklists::BlocklistStore;
use crate::hub::Hub;
use crate::lan::LanFilter;
use sqlx::SqlitePool;
use std::sync::Arc;

pub struct Config {
    pub blocklist_source: String,
    /// Marque les cookies `Secure` : à activer derrière un proxy HTTPS.
    pub secure_cookies: bool,
}

pub struct AppState {
    pub db: SqlitePool,
    pub config: Config,
    pub hub: Hub,
    pub blocklists: Arc<BlocklistStore>,
    pub lan: Arc<LanFilter>,
    pub http: reqwest::Client,
}

pub type Shared = Arc<AppState>;

impl AppState {
    /// À appeler après toute modification d'enfant, d'appareil, de politique
    /// ou d'exception : recalcule le filtrage LAN et pousse l'état aux agents.
    pub async fn changed(self: &Arc<Self>) {
        if let Err(e) = crate::lan::rebuild(self).await {
            tracing::error!(error = ?e, "reconstruction du filtrage LAN");
        }
        crate::hub::push_all(self).await;
    }
}
