//! Serveur Cotutelle : API, interface web, DNS réseau local.

mod api;
mod auth;
mod blocklists;
mod error;
mod hub;
mod lan;
mod logos;
mod model;
mod notify;
mod state;
mod tasks;
mod util;

use anyhow::{Context, Result};
use axum::Router;
use clap::Parser;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use state::{AppState, Config, Shared};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

const UT1_ARCHIVE: &str = "https://dsi.ut-capitole.fr/blacklists/download/blacklists.tar.gz";

#[derive(Parser, Debug)]
#[command(name = "cotutelle-server", version, about)]
struct Cli {
    /// Adresse d'écoute HTTP (API + interface web).
    #[arg(long, env = "COTUTELLE_HTTP_ADDR", default_value = "0.0.0.0:8080")]
    http_addr: SocketAddr,

    /// Répertoire des données : base SQLite et listes de blocage.
    #[arg(long, env = "COTUTELLE_DATA_DIR", default_value = "data")]
    data_dir: PathBuf,

    /// Répertoire de l'interface web compilée.
    #[arg(long, env = "COTUTELLE_WEB_DIR", default_value = "web/build")]
    web_dir: PathBuf,

    /// Adresse d'écoute du DNS réseau local (ex. `0.0.0.0:53`). Désactivé si absent.
    #[arg(long, env = "COTUTELLE_DNS_ADDR")]
    dns_addr: Option<SocketAddr>,

    /// Archive des listes de blocage : URL ou fichier local. Vide = aucune mise à jour.
    #[arg(long, env = "COTUTELLE_BLOCKLIST_SOURCE", default_value = UT1_ARCHIVE)]
    blocklist_source: String,

    /// Marque les cookies `Secure` (à activer derrière un proxy HTTPS).
    #[arg(long, env = "COTUTELLE_SECURE_COOKIES")]
    secure_cookies: bool,
}

async fn open_database(data_dir: &std::path::Path) -> Result<sqlx::SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(data_dir.join("cotutelle.db"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new().max_connections(4).connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await.context("migrations")?;
    Ok(pool)
}

fn app(state: Shared, web_dir: &std::path::Path) -> Router {
    // Interface monopage : toute route inconnue rend index.html.
    let web = ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")));
    Router::new()
        .nest("/api/v1", api::router())
        .route("/healthz", axum::routing::get(|| async { "ok" }))
        .fallback_service(web)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.data_dir)
        .with_context(|| format!("création de {}", cli.data_dir.display()))?;

    let db = open_database(&cli.data_dir).await.context("ouverture de la base")?;
    let blocklists = Arc::new(blocklists::BlocklistStore::open(cli.data_dir.join("blocklists"))?);
    let state: Shared = Arc::new(AppState {
        db,
        config: Config {
            blocklist_source: cli.blocklist_source.trim().to_string(),
            secure_cookies: cli.secure_cookies,
        },
        hub: hub::Hub::default(),
        lan: Arc::new(lan::LanFilter::new(blocklists.clone())),
        blocklists,
        http: reqwest::Client::new(),
    });

    lan::rebuild(&state).await.context("chargement du filtrage LAN")?;
    tasks::spawn(state.clone());

    if let Some(dns_addr) = cli.dns_addr {
        let server = cotutelle_dns::DnsServer::bind(dns_addr, state.lan.clone()).await?;
        tracing::info!(addr = %dns_addr, "DNS réseau local à l'écoute");
        tokio::spawn(async move {
            if let Err(e) = server.run().await {
                tracing::error!(error = ?e, "arrêt du DNS réseau local");
            }
        });
    }

    let listener = tokio::net::TcpListener::bind(cli.http_addr).await?;
    tracing::info!(addr = %cli.http_addr, data = %cli.data_dir.display(), "serveur Cotutelle démarré");
    axum::serve(
        listener,
        app(state, &cli.web_dir).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
