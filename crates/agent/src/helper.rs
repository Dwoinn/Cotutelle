//! Liaison avec l'assistant de session (`cotutelle-session`) : une socket
//! Unix locale, un JSON par ligne dans chaque sens (D19). L'agent y pousse
//! la situation du compte et ses avis, et en reçoit les demandes de
//! l'enfant, qu'il relaie au serveur. L'assistant tourne sans privilège dans
//! la session ; l'agent reconnaît son compte à l'identité du pair et ne lui
//! montre rien d'autre.

use crate::runtime::Runtime;
use anyhow::{Context, Result};
use chrono::Local;
use cotutelle_common::GrantTarget;
use cotutelle_common::protocol::AgentMessage;
use cotutelle_common::session::{AgentEvent, HelperMessage, Notice};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream, unix::OwnedWriteHalf};

/// Au-delà, la ligne n'est pas un message de l'assistant.
const MAX_LINE: u64 = 16 * 1024;

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
            if let Err(e) = handle(rt, stream).await {
                tracing::info!(error = %e, "assistant de session");
            }
        });
    }
}

async fn send(writer: &mut OwnedWriteHalf, event: &AgentEvent) -> Result<()> {
    let mut line = serde_json::to_string(event)?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    Ok(())
}

async fn handle(rt: Arc<Runtime>, stream: UnixStream) -> Result<()> {
    let uid = stream.peer_cred().context("identité de l'assistant")?.uid();
    let account = rt
        .platform
        .account_name(uid)
        .with_context(|| format!("compte inconnu pour l'uid {uid}"))?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let mut events = rt.attach_helper(&account);
    let hello = rt.helper_event(&account, Local::now());
    rt.helper_states
        .lock()
        .expect("verrou états assistants")
        .insert(account.clone(), hello.clone());
    send(&mut writer, &hello).await?;
    tracing::info!(account, "assistant de session connecté");

    // Les événements partent sur un fil à part : la lecture ne les retient pas.
    let writing = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            if send(&mut writer, &event).await.is_err() {
                break;
            }
        }
    });

    let mut line = String::new();
    let outcome = loop {
        line.clear();
        let read = (&mut reader).take(MAX_LINE).read_line(&mut line).await?;
        if read == 0 {
            break Ok(());
        }
        if !line.ends_with('\n') {
            break Err(anyhow::anyhow!("ligne trop longue"));
        }
        handle_message(&rt, &account, line.trim());
    };
    writing.abort();
    tracing::info!(account, "assistant de session parti");
    outcome
}

fn handle_message(rt: &Runtime, account: &str, text: &str) {
    match serde_json::from_str::<HelperMessage>(text) {
        Ok(HelperMessage::Hello { version }) => {
            tracing::info!(account, version, "assistant de session");
        }
        Ok(HelperMessage::Request { target, minutes, message }) => {
            if let Err(refusal) = request(rt, account, target, minutes, message) {
                tracing::info!(account, refusal, "demande non relayée");
                let text = refusal.to_string();
                rt.tell_helper(
                    account,
                    AgentEvent::Notice(Notice::Answered { approved: false, text }),
                );
            }
        }
        Err(e) => tracing::warn!(account, error = %e, "message d'assistant illisible"),
    }
}

/// Relaie une demande au serveur, aux mêmes conditions que depuis l'espace
/// web. Le serveur reste juge ; on n'encombre juste pas la file avec ce
/// qu'il refusera.
fn request(
    rt: &Runtime,
    account: &str,
    target: GrantTarget,
    minutes: Option<u32>,
    message: Option<String>,
) -> Result<(), &'static str> {
    let today = Local::now().date_naive();
    let allowed = rt
        .account(account)
        .is_some_and(|a| a.policy.rules_on(today, &rt.holidays()).filter.allow_requests);
    if !allowed {
        return Err("Les demandes ne sont pas activées pour toi.");
    }
    let target = match target {
        t @ (GrantTarget::Service { .. } | GrantTarget::Domain { .. }) => t,
        GrantTarget::ExtraMinutes { minutes } => {
            GrantTarget::ExtraMinutes { minutes: minutes.clamp(5, 120) }
        }
        _ => return Err("Cette demande n'est pas possible."),
    };
    let minutes = match target {
        GrantTarget::ExtraMinutes { .. } => None,
        _ => Some(minutes.unwrap_or(60).clamp(5, 240)),
    };
    let message =
        message.map(|m| m.trim().chars().take(200).collect::<String>()).filter(|m| !m.is_empty());
    tracing::info!(account, ?target, minutes, "demande de l'enfant relayée au serveur");
    rt.queue(AgentMessage::ChildRequest {
        os_account: account.to_string(),
        target,
        minutes,
        message,
    });
    Ok(())
}
