//! Notifications du bureau par D-Bus (`org.freedesktop.Notifications`), avec
//! boutons : l'enfant demande du temps ou un accès sans ouvrir de page.

use crate::Command;
use anyhow::{Context, Result};
use cotutelle_common::session::{Notice, SessionState};
use futures_util::StreamExt;
use std::collections::HashMap;
use tokio::sync::mpsc;
use zbus::zvariant::Value;

const APP_NAME: &str = "Cotutelle";
/// Entrée de bureau installée par le paquet : donne le nom et l'icône.
const DESKTOP_ENTRY: &str = "cotutelle";
const URGENCY_NORMAL: u8 = 1;
const URGENCY_CRITICAL: u8 = 2;
/// Durée d'affichage d'un avis ordinaire, en millisecondes.
const TIMEOUT_MS: i32 = 10_000;
/// Ne disparaît pas tout seul.
const NEVER: i32 = 0;

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: &str) -> zbus::Result<()>;
}

pub struct Notifier {
    proxy: NotificationsProxy<'static>,
    /// Les avis sur le temps se remplacent : un seul à l'écran.
    time_notice: u32,
}

impl Notifier {
    pub async fn new(
        bus: &zbus::Connection,
        commands: mpsc::UnboundedSender<Command>,
    ) -> Result<Self> {
        let proxy = NotificationsProxy::new(bus).await.context("service de notifications")?;
        // Les clics sur les boutons reviennent par un signal.
        let mut actions = proxy.receive_action_invoked().await?;
        tokio::spawn(async move {
            while let Some(signal) = actions.next().await {
                let Ok(args) = signal.args() else { continue };
                if let Some(command) = Command::from_key(args.action_key) {
                    let _ = commands.send(command);
                }
            }
        });
        Ok(Self { proxy, time_notice: 0 })
    }

    async fn notify(
        &self,
        replaces: u32,
        title: &str,
        body: &str,
        actions: &[(String, &str)],
        urgency: u8,
        timeout: i32,
    ) -> u32 {
        let flat: Vec<&str> =
            actions.iter().flat_map(|(key, label)| [key.as_str(), *label]).collect();
        let hints = HashMap::from([
            ("urgency", Value::U8(urgency)),
            ("desktop-entry", Value::from(DESKTOP_ENTRY)),
        ]);
        match self
            .proxy
            .notify(APP_NAME, replaces, DESKTOP_ENTRY, title, body, &flat, hints, timeout)
            .await
        {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(error = %e, title, "notification impossible");
                0
            }
        }
    }

    /// Un simple message, sans bouton.
    pub async fn plain(&self, title: &str, body: &str) {
        self.notify(0, title, body, &[], URGENCY_NORMAL, TIMEOUT_MS).await;
    }

    pub async fn show(&mut self, notice: &Notice, state: Option<&SessionState>) {
        let may_ask = state.is_some_and(|s| s.allow_requests);
        let ask_time = (Command::AskTime(crate::ASK_MINUTES).key(), "Demander du temps");
        let (actions, urgency, timeout, about_time): (Vec<(String, &str)>, u8, i32, bool) =
            match notice {
                Notice::Warning { minutes } => (
                    if may_ask { vec![ask_time] } else { vec![] },
                    if *minutes > 5 { URGENCY_NORMAL } else { URGENCY_CRITICAL },
                    if *minutes > 5 { TIMEOUT_MS } else { NEVER },
                    true,
                ),
                Notice::Closed { .. } => {
                    (if may_ask { vec![ask_time] } else { vec![] }, URGENCY_CRITICAL, NEVER, true)
                }
                Notice::Blocked { service, requestable, .. } => (
                    match service {
                        Some(service) if *requestable && may_ask => {
                            vec![(
                                Command::AskService(service.id.clone()).key(),
                                "Demander l'accès",
                            )]
                        }
                        _ => vec![],
                    },
                    URGENCY_NORMAL,
                    TIMEOUT_MS,
                    false,
                ),
                Notice::Answered { .. } => (vec![], URGENCY_NORMAL, TIMEOUT_MS, false),
            };
        let replaces = if about_time { self.time_notice } else { 0 };
        let id = self
            .notify(replaces, &notice.title(), &notice.body(), &actions, urgency, timeout)
            .await;
        if about_time {
            self.time_notice = id;
        }
    }
}
