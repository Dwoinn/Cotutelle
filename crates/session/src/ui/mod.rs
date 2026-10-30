//! Fenêtres natives (GTK 4 / libadwaita) : l'écran plein d'une fermeture et
//! le formulaire de demande.
//!
//! GTK vit sur un fil à lui, propriétaire du contexte principal GLib par
//! défaut. La boucle tokio lui passe des fermetures par
//! `MainContext::invoke`, et les fenêtres répondent par les `Command`
//! habituels, sur le même canal que le menu et les notifications. Si GTK ne
//! démarre pas (pas d'affichage, session texte), `Ui::start` rend `None` et
//! l'assistant s'en tient aux notifications.

mod closed;
mod request;

use crate::Command;
use cotutelle_common::session::{Notice, SessionState};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use std::sync::mpsc as sync_mpsc;
use tokio::sync::mpsc;

/// Feuille de style commune : Nuit et Soleil de Cotutelle.
const STYLE: &str = include_str!("style.css");

pub struct Ui {
    context: glib::MainContext,
    commands: mpsc::UnboundedSender<Command>,
    icon: String,
}

impl Ui {
    /// Démarre le fil GTK ; `None` si GTK ne peut pas s'initialiser.
    pub fn start(
        commands: mpsc::UnboundedSender<Command>,
        icon: String,
        icon_dir: String,
    ) -> Option<Self> {
        let (ready_tx, ready_rx) = sync_mpsc::channel();
        let thread = std::thread::Builder::new().name("gtk".into()).spawn(move || {
            glib::set_application_name("Cotutelle");
            if let Err(e) = adw::init() {
                tracing::info!(error = %e, "pas de fenêtres : GTK ne démarre pas");
                let _ = ready_tx.send(None);
                return;
            }
            style(&icon_dir);
            let _ = ready_tx.send(Some(glib::MainContext::default()));
            glib::MainLoop::new(None, false).run();
        });
        if let Err(e) = thread {
            tracing::warn!(error = %e, "fil GTK");
            return None;
        }
        let context = ready_rx.recv().ok().flatten()?;
        tracing::info!("fenêtres natives prêtes");
        Some(Self { context, commands, icon })
    }

    fn on_gtk(&self, work: impl FnOnce() + Send + 'static) {
        self.context.invoke(work);
    }

    /// Montre (ou met à jour) l'écran plein d'une fermeture.
    pub fn show_closed(&self, notice: &Notice, may_ask: bool) {
        let (title, body, grace) = closed_screen(notice);
        let (commands, icon) = (self.commands.clone(), self.icon.clone());
        self.on_gtk(move || closed::show(&commands, &icon, &title, &body, grace, may_ask));
    }

    /// Cache l'écran de fermeture : l'accès a rouvert.
    pub fn dismiss_closed(&self) {
        self.on_gtk(closed::dismiss);
    }

    /// Ouvre le formulaire de demande avec ce que l'enfant peut demander.
    pub fn show_request(&self, state: SessionState) {
        let commands = self.commands.clone();
        self.on_gtk(move || request::show(&commands, state));
    }
}

/// Ce que l'écran plein d'une fermeture affiche : le titre, le motif sans la
/// phrase sur le verrou (le compte à rebours le montre à part), et le délai.
fn closed_screen(notice: &Notice) -> (String, String, u32) {
    match notice {
        Notice::Closed { reason, reopens, quota_minutes, grace_seconds } => {
            let silent = Notice::Closed {
                reason: *reason,
                reopens: reopens.clone(),
                quota_minutes: *quota_minutes,
                grace_seconds: 0,
            };
            (silent.title(), silent.body(), *grace_seconds)
        }
        other => (other.title(), other.body(), 0),
    }
}

/// « 30 s », « 1 min 05 s ».
fn countdown_label(seconds: u32) -> String {
    match (seconds / 60, seconds % 60) {
        (0, s) => format!("{s} s"),
        (m, s) => format!("{m} min {s:02} s"),
    }
}

/// Style commun et icône de Cotutelle, quand elle n'est pas dans le thème.
fn style(icon_dir: &str) {
    let Some(display) = gtk::gdk::Display::default() else { return };
    if !icon_dir.is_empty() {
        gtk::IconTheme::for_display(&display).add_search_path(icon_dir);
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_data(STYLE);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// Échap ferme la fenêtre.
fn close_on_escape(window: &adw::Window) {
    let keys = gtk::EventControllerKey::new();
    let target = window.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            target.close();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cotutelle_common::ClosedReason;

    #[test]
    fn closed_screen_keeps_the_countdown_apart() {
        let notice = Notice::Closed {
            reason: ClosedReason::DailyQuota,
            reopens: Some("demain à 8 h".into()),
            quota_minutes: Some(90),
            grace_seconds: 30,
        };
        let (title, body, grace) = closed_screen(&notice);
        assert_eq!(title, "Temps d'écran terminé");
        assert_eq!(body, "Tu as utilisé tes 1 h 30 d'aujourd'hui. Ça rouvre demain à 8 h.");
        assert_eq!(grace, 30);
        assert_eq!(countdown_label(30), "30 s");
        assert_eq!(countdown_label(65), "1 min 05 s");
    }
}
