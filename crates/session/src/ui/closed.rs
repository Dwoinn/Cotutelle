//! L'écran plein d'une fermeture : le motif en grand, le compte à rebours
//! avant le verrou quand il y en a un, un bouton pour demander du temps.

use super::{adw, close_on_escape, countdown_label, glib, gtk};
use crate::{ASK_MINUTES, Command};
use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

struct Screen {
    window: adw::Window,
    page: adw::StatusPage,
    countdown: gtk::Label,
    caption: gtk::Label,
    ask: gtk::Button,
    /// Change à chaque affichage : un compte à rebours périmé s'arrête seul.
    generation: Rc<Cell<u64>>,
}

thread_local! {
    static SCREEN: RefCell<Option<Screen>> = const { RefCell::new(None) };
}

pub fn show(
    commands: &mpsc::UnboundedSender<Command>,
    icon: &str,
    title: &str,
    body: &str,
    grace_seconds: u32,
    may_ask: bool,
) {
    let window = SCREEN.with(|cell| {
        let mut slot = cell.borrow_mut();
        let screen = slot.get_or_insert_with(|| build(commands.clone(), icon));
        screen.page.set_title(title);
        screen.page.set_description(Some(body));
        screen.ask.set_visible(may_ask);
        screen.ask.set_sensitive(true);
        screen.ask.set_label("Demander du temps");
        screen.generation.set(screen.generation.get() + 1);
        screen.countdown.set_visible(grace_seconds > 0);
        screen.caption.set_visible(grace_seconds > 0);
        if grace_seconds > 0 {
            countdown(screen, grace_seconds);
        }
        screen.window.clone()
    });
    window.present();
    window.fullscreen();
}

/// L'accès a rouvert, ou il n'y a plus rien à montrer.
pub fn dismiss() {
    SCREEN.with(|cell| {
        if let Some(screen) = cell.borrow().as_ref() {
            screen.generation.set(screen.generation.get() + 1);
            screen.window.set_visible(false);
        }
    });
}

fn build(commands: mpsc::UnboundedSender<Command>, icon: &str) -> Screen {
    let countdown = gtk::Label::builder().css_classes(["countdown"]).build();
    let caption = gtk::Label::builder()
        .label("avant le verrouillage de l'écran")
        .css_classes(["caption-line"])
        .build();
    let ask = gtk::Button::builder()
        .label("Demander du temps")
        .css_classes(["pill", "suggested-action", "sun"])
        .build();
    let close = gtk::Button::builder().label("Fermer").css_classes(["pill"]).build();
    let buttons = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::Center)
        .margin_top(28)
        .build();
    buttons.append(&ask);
    buttons.append(&close);
    let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
    column.append(&countdown);
    column.append(&caption);
    column.append(&buttons);
    let page = adw::StatusPage::builder().icon_name(icon).child(&column).build();
    let window = adw::Window::builder()
        .title("Cotutelle")
        .content(&page)
        .hide_on_close(true)
        .default_width(960)
        .default_height(640)
        .css_classes(["cotutelle-closed"])
        .build();
    close_on_escape(&window);
    let target = window.clone();
    close.connect_clicked(move |_| target.close());
    ask.connect_clicked(move |button| {
        let _ = commands.send(Command::AskTime(ASK_MINUTES));
        button.set_sensitive(false);
        button.set_label("Demande envoyée");
    });
    Screen { window, page, countdown, caption, ask, generation: Rc::default() }
}

/// Décompte jusqu'au verrou, seconde par seconde.
fn countdown(screen: &Screen, seconds: u32) {
    let deadline = Instant::now() + Duration::from_secs(seconds.into());
    let (countdown, caption) = (screen.countdown.clone(), screen.caption.clone());
    caption.set_label("avant le verrouillage de l'écran");
    let (generation, mine) = (screen.generation.clone(), screen.generation.get());
    let tick = move || {
        if generation.get() != mine {
            return glib::ControlFlow::Break;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        let left = u32::try_from(left.as_millis().div_ceil(1000)).unwrap_or(u32::MAX);
        countdown.set_label(&countdown_label(left));
        if left == 0 {
            caption.set_label("L'écran se verrouille.");
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    };
    if tick() == glib::ControlFlow::Continue {
        glib::timeout_add_seconds_local(1, tick);
    }
}
