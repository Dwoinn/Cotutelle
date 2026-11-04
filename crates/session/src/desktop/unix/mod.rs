//! Bureau Unix : bus de session D-Bus pour l'instance unique et les
//! notifications, StatusNotifierItem pour l'icône, socket Unix vers l'agent.

mod notify;
mod tray;

use super::{Reader, Writer};
use crate::Command;
use anyhow::{Context, Result};
use cotutelle_common::session::{Notice, SessionState};
use std::path::Path;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use zbus::fdo::{RequestNameFlags, RequestNameReply};

pub const DEFAULT_SOCKET: &str = "/run/cotutelle/agent.sock";
/// Nom pris sur le bus de session : un seul assistant par session.
const BUS_NAME: &str = "fr.cotutelle.Session";

/// La session graphique, tenue par la connexion au bus.
pub struct Desktop {
    bus: zbus::Connection,
}

impl Desktop {
    /// Se déclare sur le bus ; échoue si un autre assistant y est déjà.
    pub async fn claim() -> Result<Self> {
        let bus = zbus::Connection::session().await.context("bus de session")?;
        let owner = bus
            .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
            .await
            .context("nom sur le bus de session")?;
        if owner != RequestNameReply::PrimaryOwner {
            anyhow::bail!("un autre assistant de session tourne déjà dans cette session");
        }
        Ok(Self { bus })
    }

    pub async fn notifier(&self, commands: mpsc::UnboundedSender<Command>) -> Result<Notifier> {
        Ok(Notifier(notify::Notifier::new(&self.bus, commands).await?))
    }

    pub async fn tray(
        &self,
        commands: mpsc::UnboundedSender<Command>,
        icon: String,
        icon_dir: String,
    ) -> Result<Tray> {
        Ok(Tray(tray::spawn(commands, icon, icon_dir).await?))
    }
}

pub struct Notifier(notify::Notifier);

impl Notifier {
    pub async fn plain(&self, title: &str, body: &str) {
        self.0.plain(title, body).await;
    }

    pub async fn show(&mut self, notice: &Notice, state: Option<&SessionState>) {
        self.0.show(notice, state).await;
    }
}

pub struct Tray(ksni::Handle<tray::Tray>);

impl Tray {
    pub async fn update(&self, state: Option<SessionState>) {
        self.0.update(|tray| tray.state = state).await;
    }
}

pub async fn connect(path: &Path) -> std::io::Result<(Reader, Writer)> {
    let (reader, writer) = UnixStream::connect(path).await?.into_split();
    Ok((Box::new(reader), Box::new(writer)))
}

/// Ouvre une adresse dans le navigateur de la session.
pub fn open_url(url: &str) -> Result<()> {
    std::process::Command::new("xdg-open").arg(url).spawn().context("xdg-open")?;
    Ok(())
}
