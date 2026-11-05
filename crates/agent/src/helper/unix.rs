//! Transport Unix : une socket locale, le compte du pair par son uid.

use super::handle;
use crate::runtime::Runtime;
use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};

/// Ouvre la socket et sert chaque assistant qui s'y connecte.
pub async fn serve(rt: Arc<Runtime>, path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
    }
    // Socket laissée par un arrêt brutal.
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let listener =
        UnixListener::bind(path).with_context(|| format!("écoute sur {}", path.display()))?;
    // Toute session locale peut s'y connecter : chacune n'y voit que son compte.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    tracing::info!(path = %path.display(), "liaison avec les assistants de session ouverte");
    loop {
        let (stream, _) = listener.accept().await?;
        let rt = rt.clone();
        tokio::spawn(async move {
            if let Err(e) = accept(rt, stream).await {
                tracing::info!(error = %e, "assistant de session");
            }
        });
    }
}

async fn accept(rt: Arc<Runtime>, stream: UnixStream) -> Result<()> {
    let uid = stream.peer_cred().context("identité de l'assistant")?.uid();
    let account = rt
        .platform
        .account_name(uid)
        .with_context(|| format!("compte inconnu pour l'uid {uid}"))?;
    let (reader, writer) = stream.into_split();
    handle(rt, account, reader, writer).await
}
