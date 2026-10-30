//! Sans la feature `gtk` : pas de fenêtres. L'assistant garde notifications
//! et icône de barre, et `Ui::start` ne rend jamais rien.

use crate::Command;
use cotutelle_common::session::{Notice, SessionState};
use tokio::sync::mpsc;

/// Jamais construit : les fenêtres ne sont pas compilées.
#[allow(dead_code)]
pub struct Ui(());

impl Ui {
    pub fn start(
        _commands: mpsc::UnboundedSender<Command>,
        _icon: String,
        _icon_dir: String,
    ) -> Option<Self> {
        None
    }

    pub fn show_closed(&self, _notice: &Notice, _may_ask: bool) {}

    pub fn dismiss_closed(&self) {}

    pub fn show_request(&self, _state: SessionState) {}
}
