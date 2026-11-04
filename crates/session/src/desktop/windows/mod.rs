//! Bureau Windows : mutex nommé pour l'instance unique, notifications
//! toast, icône de la zone de notification, tube nommé vers l'agent.

mod notify;
mod tray;

use super::{Reader, Writer};
use crate::Command;
use anyhow::{Context, Result};
use cotutelle_common::session::{Notice, SessionState};
use std::path::Path;
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::sync::mpsc;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, ERROR_PIPE_BUSY, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

pub const DEFAULT_SOCKET: &str = r"\\.\pipe\cotutelle-agent";
/// Un seul assistant par session : le mutex est local à la session.
const INSTANCE_MUTEX: PCWSTR = w!("Local\\CotutelleSession");

/// Chaîne UTF-16 terminée par un nul, pour l'API Win32.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// La session, tenue par le mutex d'instance unique.
pub struct Desktop {
    mutex: HANDLE,
}

// SAFETY : le descripteur du mutex se ferme depuis n'importe quel fil.
unsafe impl Send for Desktop {}
unsafe impl Sync for Desktop {}

impl Desktop {
    /// Prend le mutex de la session ; échoue si un autre assistant le tient.
    pub async fn claim() -> Result<Self> {
        // SAFETY : appel documenté ; le descripteur est fermé par Drop.
        let mutex = unsafe { CreateMutexW(None, false, INSTANCE_MUTEX) }
            .context("mutex d'instance unique")?;
        if windows::core::Error::from_thread().code() == ERROR_ALREADY_EXISTS.to_hresult() {
            // SAFETY : descripteur valide, à nous.
            let _ = unsafe { CloseHandle(mutex) };
            anyhow::bail!("un autre assistant de session tourne déjà dans cette session");
        }
        Ok(Self { mutex })
    }

    pub async fn notifier(&self, commands: mpsc::UnboundedSender<Command>) -> Result<Notifier> {
        Ok(Notifier(notify::Notifier::new(commands)))
    }

    pub async fn tray(
        &self,
        commands: mpsc::UnboundedSender<Command>,
        _icon: String,
        _icon_dir: String,
    ) -> Result<Tray> {
        Ok(Tray(tray::spawn(commands)?))
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        // SAFETY : descripteur valide, à nous.
        let _ = unsafe { CloseHandle(self.mutex) };
    }
}

pub struct Notifier(notify::Notifier);

impl Notifier {
    pub async fn plain(&self, title: &str, body: &str) {
        self.0.plain(title, body);
    }

    pub async fn show(&mut self, notice: &Notice, state: Option<&SessionState>) {
        self.0.show(notice, state);
    }
}

pub struct Tray(tray::Tray);

impl Tray {
    pub async fn update(&self, state: Option<SessionState>) {
        self.0.update(state);
    }
}

pub async fn connect(path: &Path) -> std::io::Result<(Reader, Writer)> {
    let pipe = loop {
        match ClientOptions::new().open(path) {
            Ok(pipe) => break pipe,
            // Toutes les instances sont prises : l'agent en rouvre une aussitôt.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Err(e) => return Err(e),
        }
    };
    let (reader, writer) = tokio::io::split(pipe);
    Ok((Box::new(reader), Box::new(writer)))
}

/// Ouvre une adresse dans le navigateur de la session.
pub fn open_url(url: &str) -> Result<()> {
    let url = wide(url);
    // SAFETY : appel documenté ; une valeur au plus égale à 32 signale un échec.
    let result =
        unsafe { ShellExecuteW(None, w!("open"), PCWSTR(url.as_ptr()), None, None, SW_SHOWNORMAL) };
    if result.0 as usize <= 32 {
        anyhow::bail!("ShellExecute : code {}", result.0 as usize);
    }
    Ok(())
}
