//! Ce qui dépend du bureau : la liaison avec l'agent, l'instance unique,
//! les notifications, l'icône de barre et l'ouverture d'une page. Même
//! interface sous Unix (D-Bus, socket) et sous Windows (toasts, zone de
//! notification, tube nommé) ; `main.rs` ne voit pas la différence.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{DEFAULT_SOCKET, Desktop, Notifier, Tray, connect, open_url};
#[cfg(windows)]
pub use windows::{DEFAULT_SOCKET, Desktop, Notifier, Tray, connect, open_url};

use tokio::io::{AsyncRead, AsyncWrite};

/// Moitié lecture de la liaison avec l'agent.
pub type Reader = Box<dyn AsyncRead + Unpin + Send>;
/// Moitié écriture de la liaison avec l'agent.
pub type Writer = Box<dyn AsyncWrite + Unpin + Send>;
