//! Icône de barre (StatusNotifierItem) : le temps restant d'un coup d'œil,
//! le détail et les demandes dans son menu.

use crate::{ASK_MINUTES, ASK_SERVICE_MINUTES, Command, text};
use anyhow::{Context, Result};
use cotutelle_common::session::{SessionState, minutes_label};
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Status, ToolTip, TrayMethods};
use tokio::sync::mpsc;

pub struct Tray {
    pub state: Option<SessionState>,
    commands: mpsc::UnboundedSender<Command>,
    icon: String,
    icon_dir: String,
}

impl Tray {
    fn item(label: String, command: Option<Command>) -> MenuItem<Self> {
        let enabled = command.is_some();
        StandardItem {
            label,
            enabled,
            activate: Box::new(move |tray: &mut Self| {
                if let Some(command) = &command {
                    let _ = tray.commands.send(command.clone());
                }
            }),
            ..Default::default()
        }
        .into()
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "cotutelle-session".into()
    }

    fn title(&self) -> String {
        "Cotutelle".into()
    }

    fn icon_name(&self) -> String {
        self.icon.clone()
    }

    fn icon_theme_path(&self) -> String {
        self.icon_dir.clone()
    }

    /// Sans compte géré, l'icône n'a rien à dire : elle se cache.
    fn status(&self) -> Status {
        if self.state.is_some() { Status::Active } else { Status::Passive }
    }

    fn tool_tip(&self) -> ToolTip {
        let Some(state) = &self.state else { return ToolTip::default() };
        ToolTip {
            icon_name: self.icon.clone(),
            title: text::headline(state),
            description: text::details(state).join("\n"),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let Some(state) = &self.state else { return vec![] };
        let mut items = vec![Self::item(text::headline(state), None)];
        items.extend(text::details(state).into_iter().map(|line| Self::item(line, None)));
        if state.allow_requests {
            items.push(MenuItem::Separator);
            items.push(Self::item(
                format!("Demander {} de plus", minutes_label(ASK_MINUTES)),
                Some(Command::AskTime(ASK_MINUTES)),
            ));
            for service in &state.requestable_services {
                items.push(Self::item(
                    format!("Demander {} ({})", service.label, minutes_label(ASK_SERVICE_MINUTES)),
                    Some(Command::AskService(service.id.clone())),
                ));
            }
            items.push(Self::item("Demander autre chose…".into(), Some(Command::OpenSpace)));
        }
        items.push(MenuItem::Separator);
        items.push(Self::item("Mon espace".into(), Some(Command::OpenSpace)));
        items
    }
}

pub async fn spawn(
    commands: mpsc::UnboundedSender<Command>,
    icon: String,
    icon_dir: String,
) -> Result<ksni::Handle<Tray>> {
    Tray { state: None, commands, icon, icon_dir }.spawn().await.context("icône de barre")
}
