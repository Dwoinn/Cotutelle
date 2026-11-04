//! Notifications toast de Windows, avec un bouton pour demander du temps
//! ou un accès quand les parents ont activé les demandes.
//!
//! Sans identifiant d'application enregistré (l'assistant n'est pas un
//! paquet), les toasts partent sous l'identité de PowerShell : Windows les
//! affiche, le nom en tête est le sien.

use crate::Command;
use cotutelle_common::session::{Notice, SessionState};
use tauri_winrt_notification::{Duration, Scenario, Toast};
use tokio::sync::mpsc;

pub struct Notifier {
    commands: mpsc::UnboundedSender<Command>,
}

impl Notifier {
    pub fn new(commands: mpsc::UnboundedSender<Command>) -> Self {
        Self { commands }
    }

    fn toast(&self, title: &str, body: &str, button: Option<(String, &str)>, urgent: bool) {
        let mut toast = Toast::new(Toast::POWERSHELL_APP_ID)
            .title(title)
            .text1(body)
            .duration(if urgent { Duration::Long } else { Duration::Short });
        if urgent {
            // Reste à l'écran jusqu'à ce que l'enfant le ferme.
            toast = toast.scenario(Scenario::Reminder);
        }
        if let Some((key, label)) = button {
            toast = toast.add_button(label, &key);
        }
        let commands = self.commands.clone();
        toast = toast.on_activated(move |action| {
            if let Some(command) = action.as_deref().and_then(Command::from_key) {
                let _ = commands.send(command);
            }
            Ok(())
        });
        if let Err(e) = toast.show() {
            tracing::warn!(error = %e, title, "notification impossible");
        }
    }

    /// Un simple message, sans bouton.
    pub fn plain(&self, title: &str, body: &str) {
        self.toast(title, body, None, false);
    }

    pub fn show(&self, notice: &Notice, state: Option<&SessionState>) {
        let may_ask = state.is_some_and(|s| s.allow_requests);
        let ask_time = (Command::AskTime(crate::ASK_MINUTES).key(), "Demander du temps");
        let (button, urgent) = match notice {
            Notice::Warning { minutes } => (may_ask.then_some(ask_time), *minutes <= 5),
            Notice::Closed { .. } => (may_ask.then_some(ask_time), true),
            Notice::Blocked { service, requestable, .. } => (
                match service {
                    Some(service) if *requestable && may_ask => {
                        Some((Command::AskService(service.id.clone()).key(), "Demander l'accès"))
                    }
                    _ => None,
                },
                false,
            ),
            Notice::Answered { .. } => (None, false),
        };
        self.toast(&notice.title(), &notice.body(), button, urgent);
    }
}
