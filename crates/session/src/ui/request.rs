//! Le formulaire de demande : un service proposé, un autre site ou du temps
//! en plus, pour combien de temps, et un mot pour les parents.

use super::{adw, close_on_escape, glib, gtk};
use crate::Command;
use adw::prelude::*;
use cotutelle_common::session::{ServiceRef, SessionState, minutes_label};
use cotutelle_common::{GrantTarget, clean_domain};
use std::cell::RefCell;
use std::rc::Rc;
use tokio::sync::mpsc;

/// Les choix proposés, dans l'ordre d'affichage.
#[derive(Clone)]
enum Choice {
    Service(ServiceRef),
    Site,
    Time,
}

/// Durée proposée au départ, et bornes de ce que l'agent relaie.
const DEFAULT_MINUTES: f64 = 30.0;
const MIN_MINUTES: f64 = 5.0;
const MAX_MINUTES: f64 = 120.0;
const MESSAGE_MAX: i32 = 200;

thread_local! {
    static WINDOW: RefCell<Option<adw::Window>> = const { RefCell::new(None) };
}

pub fn show(commands: &mpsc::UnboundedSender<Command>, state: SessionState) {
    // Une fenêtre déjà ouverte revient au premier plan sans perdre la saisie.
    if let Some(window) = WINDOW.with(|w| w.borrow().clone()) {
        window.present();
        return;
    }
    let window = build(commands.clone(), state);
    WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));
    window.present();
}

fn build(commands: mpsc::UnboundedSender<Command>, state: SessionState) -> adw::Window {
    let window = adw::Window::builder()
        .title("Demander à tes parents")
        .default_width(540)
        .default_height(640)
        .build();

    let what = adw::PreferencesGroup::builder().title("Ce que tu veux demander").build();
    let mut choices: Vec<(gtk::CheckButton, Choice)> = Vec::new();
    let mut add = |title: &str, subtitle: Option<&str>, choice: Choice| {
        let check = gtk::CheckButton::new();
        check.set_group(choices.first().map(|(first, _)| first));
        let row = adw::ActionRow::builder().title(title).activatable_widget(&check).build();
        if let Some(subtitle) = subtitle {
            row.set_subtitle(subtitle);
        }
        row.add_prefix(&check);
        what.add(&row);
        choices.push((check, choice));
    };
    for service in &state.requestable_services {
        add(&service.label, None, Choice::Service(service.clone()));
    }
    add("Un autre site", Some("Donne son adresse en dessous"), Choice::Site);
    add("Du temps en plus", Some("Des minutes ajoutées à ton temps d'aujourd'hui"), Choice::Time);
    // Au départ : le premier service proposé, sinon le temps.
    let start = if state.requestable_services.is_empty() { choices.len() - 1 } else { 0 };
    choices[start].0.set_active(true);

    let details = adw::PreferencesGroup::builder().title("Les détails").build();
    let site = gtk::Entry::builder()
        .placeholder_text("exemple.fr")
        .input_purpose(gtk::InputPurpose::Url)
        .activates_default(true)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    let site_row = adw::ActionRow::builder().title("Le site").activatable_widget(&site).build();
    site_row.add_suffix(&site);
    let spin = gtk::SpinButton::with_range(MIN_MINUTES, MAX_MINUTES, 5.0);
    spin.set_value(DEFAULT_MINUTES);
    spin.set_valign(gtk::Align::Center);
    let duration_row = adw::ActionRow::builder()
        .title("Pendant combien de temps ?")
        .subtitle(minutes_label(DEFAULT_MINUTES as u32))
        .build();
    duration_row.add_suffix(&spin);
    let message = gtk::Entry::builder()
        .placeholder_text("Je finis mon exposé")
        .max_length(MESSAGE_MAX)
        .activates_default(true)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    let message_row = adw::ActionRow::builder()
        .title("Un mot pour tes parents")
        .subtitle("si tu veux")
        .activatable_widget(&message)
        .build();
    message_row.add_suffix(&message);
    details.add(&site_row);
    details.add(&duration_row);
    details.add(&message_row);

    let offline = gtk::Label::builder()
        .label("Le serveur est injoignable : ta demande partira dès qu'il le sera.")
        .css_classes(["dim-label"])
        .wrap(true)
        .justify(gtk::Justification::Center)
        .visible(!state.server_connected)
        .build();
    let send = gtk::Button::builder()
        .label("Envoyer la demande")
        .css_classes(["pill", "suggested-action", "sun"])
        .halign(gtk::Align::Center)
        .build();
    // Entrée dans un champ envoie, quand il y a de quoi.
    window.set_default_widget(Some(&send));

    // Le site ne se saisit que pour « un autre site », et il faut qu'il en
    // ait l'air pour envoyer.
    let refresh = {
        let (choices, site, site_row, send) =
            (choices.clone(), site.clone(), site_row.clone(), send.clone());
        Rc::new(move || {
            let choice = selected(&choices);
            site_row.set_visible(matches!(choice, Some(Choice::Site)));
            send.set_sensitive(match choice {
                Some(Choice::Site) => clean_domain(&site.text()).is_some(),
                Some(_) => true,
                None => false,
            });
        })
    };
    for (check, _) in &choices {
        let refresh = refresh.clone();
        check.connect_toggled(move |_| refresh());
    }
    {
        let refresh = refresh.clone();
        site.connect_changed(move |_| refresh());
    }
    {
        let row = duration_row.clone();
        spin.connect_value_changed(move |spin| {
            row.set_subtitle(&minutes_label(spin.value_as_int().unsigned_abs()));
        });
    }
    refresh();

    let target = window.clone();
    send.connect_clicked(move |_| {
        let Some(choice) = selected(&choices) else { return };
        let minutes = spin.value_as_int().clamp(MIN_MINUTES as i32, MAX_MINUTES as i32) as u32;
        let (target_grant, minutes) = match choice {
            Choice::Service(service) => {
                (GrantTarget::Service { service: service.id }, Some(minutes))
            }
            Choice::Site => {
                let Some(domain) = clean_domain(&site.text()) else { return };
                (GrantTarget::Domain { domain }, Some(minutes))
            }
            Choice::Time => (GrantTarget::ExtraMinutes { minutes }, None),
        };
        let text = message.text().trim().to_string();
        let message = (!text.is_empty()).then_some(text);
        let _ = commands.send(Command::Ask { target: target_grant, minutes, message });
        target.close();
    });

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(12)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&what);
    content.append(&details);
    content.append(&offline);
    content.append(&send);
    let clamp = adw::Clamp::builder().maximum_size(560).child(&content).build();
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&adw::HeaderBar::new());
    root.append(&scrolled);
    window.set_content(Some(&root));
    close_on_escape(&window);
    window.connect_close_request(|_| {
        WINDOW.with(|w| w.borrow_mut().take());
        glib::Propagation::Proceed
    });
    window
}

fn selected(choices: &[(gtk::CheckButton, Choice)]) -> Option<Choice> {
    choices.iter().find(|(check, _)| check.is_active()).map(|(_, choice)| choice.clone())
}
