//! Serveur Cotutelle : API, interface web, DNS réseau local.
//!
//! Phase 0 : expose uniquement `/healthz` et `/api/v1/version`.

use anyhow::Result;
use axum::{Json, Router, routing::get};
use clap::Parser;
use serde::Serialize;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "cotutelle-server", version, about)]
struct Cli {
    /// Adresse d'écoute HTTP (API + interface web).
    #[arg(long, env = "COTUTELLE_HTTP_ADDR", default_value = "0.0.0.0:8080")]
    http_addr: SocketAddr,

    /// Chemin de la base SQLite.
    #[arg(long, env = "COTUTELLE_DATABASE", default_value = "data/cotutelle.db")]
    database: String,
}

#[derive(Serialize)]
struct Version {
    name: &'static str,
    version: &'static str,
    protocol: u16,
}

async fn healthz() -> &'static str {
    "ok"
}

async fn version() -> Json<Version> {
    Json(Version {
        name: "cotutelle-server",
        version: env!("CARGO_PKG_VERSION"),
        protocol: cotutelle_common::PROTOCOL_VERSION,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    tracing::info!(addr = %cli.http_addr, db = %cli.database, "démarrage du serveur");

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/version", get(version))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(cli.http_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
