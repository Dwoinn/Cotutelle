//! Assistant de session Cotutelle : tourne sans privilège dans la session de
//! l'enfant et lui montre ce que l'agent décide (D19). Temps restant dans
//! la barre, préavis et motifs de fermeture en notifications, sites bloqués
//! expliqués, demandes aux parents depuis le bureau.
//!
//! Il n'impose rien : l'agent reste seul à verrouiller et à filtrer (D3,
//! D4), et garde ses notifications de base si l'assistant n'est pas là.

mod notify;
mod text;
mod tray;

use anyhow::{Context, Result};
use chrono::{Datelike, Local};
use clap::Parser;
use cotutelle_common::GrantTarget;
use cotutelle_common::session::{AgentEvent, HelperMessage, Notice, Opening, SessionState};
use notify::Notifier;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;
use zbus::fdo::{RequestNameFlags, RequestNameReply};

/// Nom pris sur le bus de session : un seul assistant par session.
const BUS_NAME: &str = "fr.cotutelle.Session";
const RETRY_MIN: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(30);
/// Durées proposées d'un clic.
pub const ASK_MINUTES: u32 = 30;
pub const ASK_SERVICE_MINUTES: u32 = 60;

#[derive(Parser, Debug)]
#[command(name = "cotutelle-session", version, about)]
struct Cli {
    /// Socket de l'agent.
    #[arg(long, env = "COTUTELLE_HELPER_SOCKET", default_value = "/run/cotutelle/agent.sock")]
    socket: PathBuf,
    /// Adresse locale de l'agent, qui ouvre l'espace enfant.
    #[arg(long, env = "COTUTELLE_LOCAL_ADDR", default_value = "127.0.0.1:7465")]
    local_addr: String,
    /// Nom de l'icône de barre dans le thème.
    #[arg(long, default_value = "cotutelle")]
    icon: String,
    /// Dossier contenant l'icône, quand elle n'est pas installée dans le thème.
    #[arg(long, default_value = "")]
    icon_dir: String,
}

/// Ce que l'enfant déclenche, depuis le menu ou une notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    AskTime(u32),
    AskService(String),
    /// Ouvre l'espace enfant dans le navigateur.
    OpenSpace,
}

impl Command {
    /// Clé d'action de notification, et retour.
    pub fn key(&self) -> String {
        match self {
            Command::AskTime(m) => format!("time:{m}"),
            Command::AskService(id) => format!("service:{id}"),
            Command::OpenSpace => "open".to_string(),
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        if key == "open" {
            return Some(Command::OpenSpace);
        }
        let (kind, value) = key.split_once(':')?;
        match kind {
            "time" => value.parse().ok().map(Command::AskTime),
            "service" => Some(Command::AskService(value.to_string())),
            _ => None,
        }
    }
}

struct App {
    cli: Cli,
    notifier: Notifier,
    tray: ksni::Handle<tray::Tray>,
    state: Option<SessionState>,
    writer: Option<OwnedWriteHalf>,
    /// Le message d'accueil n'est montré qu'une fois par session.
    welcomed: bool,
}

impl App {
    async fn send(&mut self, message: HelperMessage) {
        let Some(writer) = self.writer.as_mut() else { return };
        let Ok(mut line) = serde_json::to_string(&message) else { return };
        line.push('\n');
        if let Err(e) = writer.write_all(line.as_bytes()).await {
            tracing::warn!(error = %e, "envoi à l'agent");
        }
    }

    async fn apply(&mut self, state: Option<SessionState>) {
        self.state = state.clone();
        self.tray.update(|tray| tray.state = state).await;
    }

    async fn on_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Unmanaged => self.apply(None).await,
            AgentEvent::State(state) => {
                if !self.welcomed {
                    self.welcomed = true;
                    self.welcome(&state).await;
                }
                self.apply(Some(state)).await;
            }
            AgentEvent::Notice(notice) => self.notifier.show(&notice, self.state.as_ref()).await,
        }
    }

    /// À l'ouverture de session : la situation du jour, ou le motif si c'est
    /// déjà fermé (le verrou a pu précéder l'assistant).
    async fn welcome(&mut self, state: &SessionState) {
        if state.access.open {
            self.notifier
                .plain(&format!("Bonjour {} !", state.child_name), &text::today(state))
                .await;
            return;
        }
        let today = Local::now().weekday();
        let notice = Notice::Closed {
            reason: state.access.reason.unwrap_or(cotutelle_common::ClosedReason::DailyQuota),
            reopens: state
                .next_opening
                .map(|Opening { day, time }| Opening { day, time }.label(today)),
            quota_minutes: state.access.quota_today_minutes,
            grace_seconds: 0,
        };
        self.notifier.show(&notice, Some(state)).await;
    }

    async fn on_command(&mut self, command: Command) {
        let Some(state) = self.state.clone() else { return };
        let (target, minutes, label) = match &command {
            Command::OpenSpace => {
                let url = format!("http://{}/", self.cli.local_addr);
                if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                    tracing::warn!(error = %e, "ouverture de l'espace enfant");
                }
                return;
            }
            Command::AskTime(m) => {
                (GrantTarget::ExtraMinutes { minutes: *m }, None, format!("{m} min de plus"))
            }
            Command::AskService(id) => {
                let label = state
                    .requestable_services
                    .iter()
                    .find(|s| s.id == *id)
                    .map_or_else(|| id.clone(), |s| s.label.clone());
                (GrantTarget::Service { service: id.clone() }, Some(ASK_SERVICE_MINUTES), label)
            }
        };
        if !state.allow_requests || self.writer.is_none() {
            self.notifier
                .plain("Demande impossible", "Tes parents n'ont pas activé les demandes.")
                .await;
            return;
        }
        self.send(HelperMessage::Request { target, minutes, message: None }).await;
        let body = if state.server_connected {
            format!("Tes parents vont recevoir ta demande : {label}.")
        } else {
            format!("Ta demande ({label}) partira dès que le serveur sera joignable.")
        };
        self.notifier.plain("Demande envoyée", &body).await;
    }

    /// Plus d'agent : l'indicateur se fait discret.
    async fn offline(&mut self) {
        self.writer = None;
        self.apply(None).await;
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        // zbus avertit qu'aucun objet n'est servi derrière le nom : c'est voulu.
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,zbus=error".into()),
        )
        .init();
    let cli = Cli::parse();

    let bus = zbus::Connection::session().await.context("bus de session")?;
    let owner = bus
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
        .await
        .context("nom sur le bus de session")?;
    if owner != RequestNameReply::PrimaryOwner {
        anyhow::bail!("un autre assistant de session tourne déjà dans cette session");
    }

    let (commands_tx, mut commands) = mpsc::unbounded_channel();
    let notifier = Notifier::new(&bus, commands_tx.clone()).await?;
    let tray = tray::spawn(commands_tx, cli.icon.clone(), cli.icon_dir.clone()).await?;
    let mut app = App { cli, notifier, tray, state: None, writer: None, welcomed: false };
    tracing::info!(socket = %app.cli.socket.display(), "assistant de session démarré");

    let mut delay = RETRY_MIN;
    loop {
        let stream = match UnixStream::connect(&app.cli.socket).await {
            Ok(stream) => stream,
            Err(e) => {
                tracing::debug!(error = %e, "agent injoignable");
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(RETRY_MAX);
                continue;
            }
        };
        delay = RETRY_MIN;
        let (reader, writer) = stream.into_split();
        app.writer = Some(writer);
        app.send(HelperMessage::Hello { version: env!("CARGO_PKG_VERSION").to_string() }).await;
        tracing::info!("connecté à l'agent");

        let mut lines = BufReader::new(reader).lines();
        loop {
            tokio::select! {
                line = lines.next_line() => match line {
                    Ok(Some(line)) => match serde_json::from_str::<AgentEvent>(&line) {
                        Ok(event) => app.on_event(event).await,
                        Err(e) => tracing::warn!(error = %e, "message de l'agent illisible"),
                    },
                    _ => break,
                },
                command = commands.recv() => {
                    if let Some(command) = command {
                        app.on_command(command).await;
                    }
                }
            }
        }
        tracing::info!("agent parti");
        app.offline().await;
    }
}

#[cfg(test)]
mod tests {
    use super::Command;

    #[test]
    fn commands_survive_their_action_key() {
        for command in
            [Command::AskTime(30), Command::AskService("youtube".into()), Command::OpenSpace]
        {
            assert_eq!(Command::from_key(&command.key()), Some(command));
        }
        assert_eq!(Command::from_key("default"), None);
    }
}
