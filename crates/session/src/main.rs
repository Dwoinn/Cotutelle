//! Assistant de session Cotutelle : tourne sans privilège dans la session de
//! l'enfant et lui montre ce que l'agent décide (D19). Temps restant dans
//! la barre, préavis et motifs de fermeture en notifications, sites bloqués
//! expliqués, demandes aux parents depuis le bureau.
//!
//! Il n'impose rien : l'agent reste seul à verrouiller et à filtrer (D3,
//! D4), et garde ses notifications de base si l'assistant n'est pas là.
//!
//! Avec la feature `gtk`, deux fenêtres natives s'ajoutent : l'écran plein
//! d'une fermeture et le formulaire de demande. Sans elle, ou sans affichage
//! au lancement, notifications et icône suffisent.

mod notify;
mod text;
mod tray;
#[cfg(feature = "gtk")]
mod ui;
#[cfg(not(feature = "gtk"))]
#[path = "ui/stub.rs"]
mod ui;

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

/// Ce que l'enfant déclenche, depuis le menu, une notification ou une fenêtre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    AskTime(u32),
    AskService(String),
    /// Une demande composée dans la fenêtre : cible, durée, mot pour les parents.
    Ask {
        target: GrantTarget,
        minutes: Option<u32>,
        message: Option<String>,
    },
    /// Ouvre la fenêtre de demande, ou l'espace enfant sans fenêtres.
    AskMore,
    /// Ouvre l'espace enfant dans le navigateur.
    OpenSpace,
}

impl Command {
    /// Clé d'action de notification, et retour. Une demande composée ne
    /// vient jamais d'une notification : sa clé ne se relit pas.
    pub fn key(&self) -> String {
        match self {
            Command::AskTime(m) => format!("time:{m}"),
            Command::AskService(id) => format!("service:{id}"),
            Command::Ask { .. } => "ask".to_string(),
            Command::AskMore => "more".to_string(),
            Command::OpenSpace => "open".to_string(),
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "open" => return Some(Command::OpenSpace),
            "more" => return Some(Command::AskMore),
            _ => {}
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
    /// Les fenêtres natives, quand GTK a pu démarrer.
    ui: Option<ui::Ui>,
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
        // L'écran de fermeture n'a plus lieu d'être dès que l'accès rouvre,
        // ou qu'il n'y a plus rien à montrer.
        if let Some(ui) = &self.ui
            && state.as_ref().is_none_or(|s| s.access.open)
        {
            ui.dismiss_closed();
        }
        self.state = state.clone();
        self.tray.update(|tray| tray.state = state).await;
    }

    async fn on_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Unmanaged => self.apply(None).await,
            AgentEvent::State(state) => {
                self.apply(Some(state.clone())).await;
                if !self.welcomed {
                    self.welcomed = true;
                    self.welcome(&state).await;
                }
            }
            AgentEvent::Notice(notice @ Notice::Closed { .. }) => self.closed(&notice).await,
            AgentEvent::Notice(notice) => self.notifier.show(&notice, self.state.as_ref()).await,
        }
    }

    /// Une fermeture : l'écran plein quand les fenêtres sont là, la
    /// notification critique sinon.
    async fn closed(&mut self, notice: &Notice) {
        let may_ask = self.state.as_ref().is_some_and(|s| s.allow_requests);
        match &self.ui {
            Some(ui) => ui.show_closed(notice, may_ask),
            None => self.notifier.show(notice, self.state.as_ref()).await,
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
        self.closed(&notice).await;
    }

    fn open_space(&self) {
        let url = format!("http://{}/", self.cli.local_addr);
        if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
            tracing::warn!(error = %e, "ouverture de l'espace enfant");
        }
    }

    async fn on_command(&mut self, command: Command) {
        let Some(state) = self.state.clone() else { return };
        let (target, minutes, message) = match command {
            Command::OpenSpace => return self.open_space(),
            Command::AskMore => {
                return match &self.ui {
                    Some(ui) if state.allow_requests => ui.show_request(state),
                    _ => self.open_space(),
                };
            }
            Command::AskTime(m) => (GrantTarget::ExtraMinutes { minutes: m }, None, None),
            Command::AskService(id) => {
                (GrantTarget::Service { service: id }, Some(ASK_SERVICE_MINUTES), None)
            }
            Command::Ask { target, minutes, message } => (target, minutes, message),
        };
        if !state.allow_requests || self.writer.is_none() {
            self.notifier
                .plain("Demande impossible", "Tes parents n'ont pas activé les demandes.")
                .await;
            return;
        }
        let label = text::request_label(&state, &target, minutes);
        self.send(HelperMessage::Request { target, minutes, message }).await;
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
    let tray = tray::spawn(commands_tx.clone(), cli.icon.clone(), cli.icon_dir.clone()).await?;
    let ui = ui::Ui::start(commands_tx, cli.icon.clone(), cli.icon_dir.clone());
    let mut app = App { cli, notifier, tray, ui, state: None, writer: None, welcomed: false };
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
        for command in [
            Command::AskTime(30),
            Command::AskService("youtube".into()),
            Command::AskMore,
            Command::OpenSpace,
        ] {
            assert_eq!(Command::from_key(&command.key()), Some(command));
        }
        assert_eq!(Command::from_key("default"), None);
        assert_eq!(Command::from_key("ask"), None);
    }
}
