//! Agent Cotutelle : tourne en service root sur l'appareil protégé.
//!
//! Il applique en local la politique reçue du serveur : résolveur DNS
//! filtrant, décompte du temps d'écran, verrouillage. Sans serveur, la
//! dernière politique connue reste appliquée (D3).

mod local;
mod platform;
mod runtime;
mod store;
mod sync;
mod watch;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use cotutelle_common::protocol::{EnrollRequest, EnrollResponse};
use runtime::Runtime;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use store::{Identity, Paths};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "cotutelle-agent", version, about)]
struct Cli {
    /// Répertoire d'état : identité, politique en cache, listes, temps d'écran.
    #[arg(long, env = "COTUTELLE_STATE_DIR", default_value = "/var/lib/cotutelle", global = true)]
    state_dir: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Rattache cet appareil à un serveur avec le code affiché dans l'interface.
    Enroll {
        /// Adresse du serveur, ex. `http://192.168.1.10:8080`.
        #[arg(long, env = "COTUTELLE_SERVER")]
        server: String,
        /// Code d'enrôlement, ex. `K7QF-2MXD`.
        #[arg(long)]
        code: String,
    },
    /// Lance l'agent (mode service).
    Run {
        /// Adresse du résolveur DNS local.
        #[arg(long, env = "COTUTELLE_DNS_ADDR", default_value = "127.0.0.1:5354")]
        dns_addr: SocketAddr,
        /// Adresse du serveur HTTP local (espace enfant, état).
        #[arg(long, env = "COTUTELLE_LOCAL_ADDR", default_value = "127.0.0.1:7465")]
        local_addr: SocketAddr,
        /// Ne touche ni à systemd-resolved ni à nftables.
        #[arg(long, env = "COTUTELLE_NO_ENFORCE")]
        no_enforce: bool,
        /// Journalise verrouillages et notifications sans les exécuter.
        #[arg(long, env = "COTUTELLE_DRY_RUN")]
        dry_run: bool,
    },
    /// Affiche l'état de l'agent en cours d'exécution.
    Status {
        #[arg(long, env = "COTUTELLE_LOCAL_ADDR", default_value = "127.0.0.1:7465")]
        local_addr: SocketAddr,
    },
    /// Retire la protection DNS du système (avant désinstallation).
    Release,
}

async fn enroll(paths: &Paths, server: &str, code: &str) -> Result<()> {
    if paths.load_identity()?.is_some() {
        bail!("cet appareil est déjà rattaché à un serveur");
    }
    let platform = platform::detect();
    let server = server.trim_end_matches('/').to_string();
    let response = reqwest::Client::new()
        .post(sync::api_url(&server, "/agent/enroll"))
        .json(&EnrollRequest {
            token: code.to_string(),
            hostname: platform.hostname(),
            os: platform.name().to_string(),
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            os_accounts: platform.os_accounts(),
        })
        .send()
        .await
        .with_context(|| format!("connexion à {server}"))?;
    if !response.status().is_success() {
        let detail: serde_json::Value = response.json().await.unwrap_or_default();
        bail!("enrôlement refusé : {}", detail["error"].as_str().unwrap_or("erreur inconnue"));
    }
    let enrolled: EnrollResponse = response.json().await?;
    paths.save_identity(&Identity {
        server,
        device: enrolled.device,
        token: enrolled.device_token,
    })?;
    println!("Appareil rattaché. Associez maintenant les comptes aux enfants dans l'interface.");
    Ok(())
}

async fn run(
    paths: Paths,
    dns_addr: SocketAddr,
    local_addr: SocketAddr,
    no_enforce: bool,
    dry_run: bool,
) -> Result<()> {
    let identity = paths
        .load_identity()?
        .context("appareil non rattaché : lancez d'abord `cotutelle-agent enroll`")?;
    let state = paths.load_state()?;
    if state.is_none() {
        tracing::warn!("aucune politique en cache : en attente du serveur");
    }

    let rt = Arc::new(Runtime {
        usage: paths.load_usage().into(),
        identity,
        paths,
        platform: platform::detect(),
        dry_run,
        http: reqwest::Client::builder().timeout(std::time::Duration::from_secs(120)).build()?,
        state: state.into(),
        blocklists: Default::default(),
        loaded_lists: Default::default(),
        foreground: Default::default(),
        stats: Default::default(),
        outbox: Default::default(),
        connected: AtomicBool::new(false),
    });

    // Les listes déjà sur disque protègent dès le démarrage, serveur joignable ou non.
    let cached: Vec<(String, String)> = rt
        .state
        .read()
        .expect("verrou état")
        .as_ref()
        .map(|s| s.blocklists.iter().map(|l| (l.category.clone(), l.checksum.clone())).collect())
        .unwrap_or_default();
    {
        let rt = rt.clone();
        tokio::task::spawn_blocking(move || sync::load_blocklists(&rt, cached)).await?;
    }

    let dns = cotutelle_dns::DnsServer::bind(dns_addr, rt.clone()).await?;
    tracing::info!(addr = %dns_addr, platform = rt.platform.name(), dry_run, "agent Cotutelle démarré");

    let enforced = if no_enforce || dry_run {
        tracing::warn!("protection DNS du système non appliquée (--no-enforce ou --dry-run)");
        None
    } else {
        rt.platform.enforce_dns(dns_addr).context("mise en place de la protection DNS")?;
        Some(dns_addr)
    };

    tokio::select! {
        result = dns.run() => result.context("résolveur DNS")?,
        result = local::serve(rt.clone(), local_addr) => result.context("serveur local")?,
        () = sync::run(rt.clone()) => {}
        () = watch::run(rt.clone(), enforced) => {}
        () = shutdown_signal() => tracing::info!("arrêt demandé"),
    }

    // Dernière sauvegarde du temps d'écran avant de quitter.
    rt.paths.save_usage(&rt.usage.lock().expect("verrou temps"))?;
    Ok(())
}

/// Attend SIGINT ou SIGTERM, ce dernier étant celui qu'envoie systemd.
async fn shutdown_signal() {
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

async fn status(local_addr: SocketAddr) -> Result<()> {
    let status: serde_json::Value = reqwest::get(format!("http://{local_addr}/status"))
        .await
        .context("l'agent ne répond pas : est-il lancé ?")?
        .json()
        .await?;
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let paths = Paths::new(cli.state_dir);
    match cli.command {
        Command::Enroll { server, code } => enroll(&paths, &server, &code).await,
        Command::Run { dns_addr, local_addr, no_enforce, dry_run } => {
            run(paths, dns_addr, local_addr, no_enforce, dry_run).await
        }
        Command::Status { local_addr } => status(local_addr).await,
        Command::Release => platform::detect().release_dns(),
    }
}
