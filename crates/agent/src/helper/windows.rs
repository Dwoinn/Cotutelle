//! Transport Windows : un tube nommé, le compte du pair par le jeton de son
//! processus. Le tube est ouvert à toutes les sessions locales, jamais au
//! réseau ; l'agent en garde la première instance, personne ne peut donc se
//! faire passer pour lui auprès des assistants.

use super::handle;
use crate::platform::windows::wts;
use crate::runtime::Runtime;
use anyhow::{Context, Result};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::Arc;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows::Win32::Foundation::{HANDLE, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::core::PCWSTR;

/// Tout le monde peut lire et écrire ; SYSTEM et les administrateurs ont
/// tous les droits. Les connexions réseau sont de toute façon refusées.
const PIPE_SDDL: &str = "D:(A;;GRGW;;;WD)(A;;GA;;;SY)(A;;GA;;;BA)";

/// Descripteur de sécurité du tube, libéré avec lui.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn from_sddl(sddl: &str) -> Result<Self> {
        let sddl = crate::platform::windows::wide(sddl);
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY : appel documenté ; la mémoire rendue est libérée par Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .context("descripteur de sécurité du tube")?;
        Ok(Self(descriptor))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY : mémoire allouée par ConvertStringSecurityDescriptor…, à nous.
        unsafe {
            LocalFree(Some(windows::Win32::Foundation::HLOCAL(self.0.0)));
        }
    }
}

fn instance(path: &str, descriptor: &Descriptor, first: bool) -> Result<NamedPipeServer> {
    let mut attributes = descriptor.attributes();
    // SAFETY : les attributs vivent le temps de l'appel ; le descripteur
    // qu'ils désignent, le temps du serveur.
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(path, &mut attributes as *mut _ as *mut _)
    }
    .with_context(|| format!("ouverture du tube {path}"))
}

/// Ouvre le tube et sert chaque assistant qui s'y connecte.
pub async fn serve(rt: Arc<Runtime>, path: &Path) -> Result<()> {
    let path = path.to_string_lossy().into_owned();
    let descriptor = Descriptor::from_sddl(PIPE_SDDL)?;
    let mut server = instance(&path, &descriptor, true)?;
    tracing::info!(path, "liaison avec les assistants de session ouverte");
    loop {
        server.connect().await.context("connexion au tube")?;
        let connected = std::mem::replace(&mut server, instance(&path, &descriptor, false)?);
        let rt = rt.clone();
        tokio::spawn(async move {
            if let Err(e) = accept(rt, connected).await {
                tracing::info!(error = %e, "assistant de session");
            }
        });
    }
}

async fn accept(rt: Arc<Runtime>, pipe: NamedPipeServer) -> Result<()> {
    let account = wts::pipe_client_account(HANDLE(pipe.as_raw_handle()))
        .context("identité de l'assistant")?;
    let (reader, writer) = tokio::io::split(pipe);
    handle(rt, account, reader, writer).await
}
