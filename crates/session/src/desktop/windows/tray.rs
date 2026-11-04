//! Icône de la zone de notification : le temps restant d'un coup d'œil dans
//! l'infobulle, le détail et les demandes dans son menu.
//!
//! L'icône et son menu appartiennent au fil qui les a créés, qui doit faire
//! tourner une boucle de messages Win32. Il vit donc à part de tokio : un
//! nouvel état lui parvient par un message de fil, et il refait le menu.

use crate::{ASK_MINUTES, ASK_SERVICE_MINUTES, Command, text};
use anyhow::{Context, Result};
use cotutelle_common::session::{SessionState, minutes_label};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW,
    TranslateMessage, WM_APP,
};

/// Le logo, 32 pixels de côté, en RGBA brut (`scripts/build-icons.py`).
const ICON: &[u8] = include_bytes!("../../../assets/tray-32.rgba");
const ICON_SIZE: u32 = 32;
/// Message de fil : l'état partagé a changé.
const WM_STATE: u32 = WM_APP + 1;

pub struct Tray {
    thread: u32,
    state: Arc<Mutex<Option<SessionState>>>,
}

impl Tray {
    pub fn update(&self, state: Option<SessionState>) {
        *self.state.lock().expect("verrou état de l'icône") = state;
        // SAFETY : appel documenté ; le fil a une file de messages depuis
        // qu'il a créé l'icône.
        let _ = unsafe { PostThreadMessageW(self.thread, WM_STATE, WPARAM(0), LPARAM(0)) };
    }
}

/// Une entrée du menu : cliquable quand elle porte une commande.
fn item(menu: &Menu, label: &str, command: Option<Command>) -> Result<()> {
    let id = command.as_ref().map_or_else(|| format!("info:{label}"), Command::key);
    menu.append(&MenuItem::with_id(id, label, command.is_some(), None))?;
    Ok(())
}

/// Le menu pour un état : même contenu que sous Linux.
fn build_menu(state: &SessionState) -> Result<Menu> {
    let menu = Menu::new();
    item(&menu, &text::headline(state), None)?;
    for line in text::details(state) {
        item(&menu, &line, None)?;
    }
    if state.allow_requests {
        menu.append(&PredefinedMenuItem::separator())?;
        item(
            &menu,
            &format!("Demander {} de plus", minutes_label(ASK_MINUTES)),
            Some(Command::AskTime(ASK_MINUTES)),
        )?;
        for service in &state.requestable_services {
            item(
                &menu,
                &format!("Demander {} ({})", service.label, minutes_label(ASK_SERVICE_MINUTES)),
                Some(Command::AskService(service.id.clone())),
            )?;
        }
        item(&menu, "Demander autre chose…", Some(Command::AskMore))?;
    }
    menu.append(&PredefinedMenuItem::separator())?;
    item(&menu, "Mon espace", Some(Command::OpenSpace))?;
    Ok(menu)
}

/// Applique un état à l'icône : sans compte géré, elle se cache.
fn apply(tray: &TrayIcon, state: Option<&SessionState>) {
    let Some(state) = state else {
        let _ = tray.set_visible(false);
        return;
    };
    match build_menu(state) {
        Ok(menu) => tray.set_menu(Some(Box::new(menu))),
        Err(e) => tracing::warn!(error = %e, "menu de l'icône"),
    }
    let mut tooltip = vec![text::headline(state)];
    tooltip.extend(text::details(state));
    let _ = tray.set_tooltip(Some(tooltip.join("\n")));
    let _ = tray.set_visible(true);
}

/// Crée l'icône sur son fil et rend de quoi la mettre à jour.
pub fn spawn(commands: mpsc::UnboundedSender<Command>) -> Result<Tray> {
    let state: Arc<Mutex<Option<SessionState>>> = Arc::default();
    let shared = state.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<u32>>();
    std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || {
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Some(command) = Command::from_key(event.id.as_ref()) {
                    let _ = commands.send(command);
                }
            }));
            let built = Icon::from_rgba(ICON.to_vec(), ICON_SIZE, ICON_SIZE)
                .context("icône")
                .and_then(|icon| {
                    TrayIconBuilder::new()
                        .with_icon(icon)
                        .with_tooltip("Cotutelle")
                        .with_menu_on_left_click(true)
                        .build()
                        .context("icône de la zone de notification")
                });
            let tray = match built {
                Ok(tray) => tray,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _ = tray.set_visible(false);
            let mut message = MSG::default();
            // SAFETY : appels documentés de la boucle de messages Win32.
            unsafe {
                // La file de messages existe dès le premier appel : le
                // premier état posté ne se perd pas.
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    if message.message == WM_STATE {
                        let state = shared.lock().expect("verrou état de l'icône").clone();
                        apply(&tray, state.as_ref());
                    } else {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
            }
        })
        .context("fil de l'icône")?;
    let thread = ready_rx.recv().context("fil de l'icône parti")??;
    Ok(Tray { thread, state })
}
