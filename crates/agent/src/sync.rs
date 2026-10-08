//! Synchronisation avec le serveur : connexion WebSocket persistante,
//! réception de l'état, téléchargement des listes, remontée du suivi.

use crate::runtime::Runtime;
use anyhow::{Context, Result, bail};
use chrono::Local;
use cotutelle_common::PROTOCOL_VERSION;
use cotutelle_common::protocol::{AgentMessage, DeviceState, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const REPORT_EVERY: Duration = Duration::from_secs(30);
const RETRY_MIN: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(120);

fn ws_url(server: &str) -> String {
    let base = server.trim_end_matches('/');
    let base = base
        .strip_prefix("https://")
        .map(|rest| format!("wss://{rest}"))
        .unwrap_or_else(|| format!("ws://{}", base.strip_prefix("http://").unwrap_or(base)));
    format!("{base}/api/v1/agent/ws")
}

pub fn api_url(server: &str, path: &str) -> String {
    format!("{}/api/v1{path}", server.trim_end_matches('/'))
}

/// Boucle sans fin : se reconnecte avec un délai croissant.
pub async fn run(rt: Arc<Runtime>) {
    let mut delay = RETRY_MIN;
    loop {
        match session(&rt).await {
            Ok(()) => delay = RETRY_MIN,
            Err(e) => {
                tracing::warn!(error = %e, "serveur injoignable, la dernière politique connue reste appliquée")
            }
        }
        rt.connected.store(false, Ordering::Relaxed);
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(RETRY_MAX);
    }
}

async fn session(rt: &Arc<Runtime>) -> Result<()> {
    let mut request = ws_url(&rt.identity.server).into_client_request()?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", rt.identity.token).parse().context("jeton invalide")?,
    );
    let (socket, _) = tokio_tungstenite::connect_async(request).await?;
    let (mut sink, mut stream) = socket.split();
    rt.connected.store(true, Ordering::Relaxed);
    tracing::info!(server = %rt.identity.server, "connecté au serveur");

    let hello = AgentMessage::Hello {
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        protocol_version: PROTOCOL_VERSION,
        hostname: rt.platform.hostname(),
        os_accounts: rt.platform.os_accounts(),
        state_issued_at: rt.state.read().expect("verrou état").as_ref().map(|s| s.issued_at),
    };
    sink.send(Message::Text(serde_json::to_string(&hello)?.into())).await?;

    let mut report = tokio::time::interval(REPORT_EVERY);
    report.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut awaiting_pong = false;

    loop {
        tokio::select! {
            frame = stream.next() => {
                let Some(frame) = frame else { bail!("connexion fermée par le serveur") };
                match frame? {
                    Message::Text(text) => match serde_json::from_str::<ServerMessage>(text.as_str()) {
                        Ok(ServerMessage::State(state)) => apply_state(rt, *state).await,
                        Ok(ServerMessage::LockNow { os_account }) => crate::watch::lock_account(rt, &os_account),
                        Ok(ServerMessage::Pong) => awaiting_pong = false,
                        Err(e) => tracing::warn!(error = %e, "message du serveur illisible"),
                    },
                    Message::Close(_) => bail!("connexion fermée par le serveur"),
                    _ => {}
                }
            }
            _ = report.tick() => {
                if awaiting_pong {
                    bail!("le serveur ne répond plus");
                }
                awaiting_pong = true;
                for message in pending_messages(rt) {
                    sink.send(Message::Text(serde_json::to_string(&message)?.into())).await?;
                }
            }
        }
    }
}

/// Rassemble ce qu'il y a à remonter : battement, temps d'écran, DNS, alertes.
fn pending_messages(rt: &Runtime) -> Vec<AgentMessage> {
    let today = Local::now().date_naive();
    let foreground = rt.foreground();
    let mut messages = vec![AgentMessage::Ping];

    {
        let usage = rt.usage.lock().expect("verrou temps");
        for account in rt.managed_accounts() {
            let seconds = usage.seconds(&account.os_account, today);
            let active = foreground.as_deref() == Some(account.os_account.as_str());
            if seconds > 0 || active {
                messages.push(AgentMessage::Usage {
                    os_account: account.os_account,
                    day: today,
                    seconds,
                    active,
                });
            }
        }
    }
    for (os_account, domains) in rt.stats.lock().expect("verrou stats").drain() {
        messages.push(AgentMessage::DnsSummary {
            os_account: Some(os_account),
            day: today,
            domains,
        });
    }
    messages.append(&mut rt.outbox.lock().expect("verrou file"));
    messages
}

/// Adopte un nouvel état : sauvegarde sur disque, puis mise à jour des listes.
async fn apply_state(rt: &Arc<Runtime>, state: DeviceState) {
    if let Err(e) = rt.paths.save_state(&state) {
        tracing::error!(error = ?e, "sauvegarde de la politique");
    }
    tracing::info!(
        accounts = state.accounts.len(),
        lists = state.blocklists.len(),
        services = state.services.len(),
        "politique reçue"
    );
    *rt.state.write().expect("verrou état") = Some(state);

    // Le téléchargement peut être long : hors de la boucle de messages.
    let rt = rt.clone();
    tokio::spawn(async move {
        if let Err(e) = refresh_blocklists(&rt).await {
            tracing::warn!(error = %e, "mise à jour des listes de blocage");
        }
    });
}

/// Télécharge les listes manquantes ou périmées et recharge la mémoire.
pub async fn refresh_blocklists(rt: &Arc<Runtime>) -> Result<()> {
    let wanted = rt
        .state
        .read()
        .expect("verrou état")
        .as_ref()
        .map(|s| s.blocklists.clone())
        .unwrap_or_default();

    for list in &wanted {
        if rt.paths.blocklist_checksum(&list.category).as_deref() == Some(list.checksum.as_str()) {
            continue;
        }
        let url = api_url(&rt.identity.server, &format!("/agent/blocklists/{}", list.category));
        let bytes = rt
            .http
            .get(url)
            .bearer_auth(&rt.identity.token)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        if hex::encode(Sha256::digest(&bytes)) != list.checksum {
            tracing::warn!(category = %list.category, "liste corrompue, ignorée");
            continue;
        }
        rt.paths.save_blocklist(&list.category, &bytes, &list.checksum)?;
        tracing::info!(category = %list.category, entries = list.entries, "liste téléchargée");
    }

    // Un état identique arrive souvent (temps consommé ailleurs) : ne recharge
    // des dizaines de Mo depuis le disque que si les listes ont changé.
    let key: Vec<(String, String)> = wanted.into_iter().map(|l| (l.category, l.checksum)).collect();
    if *rt.loaded_lists.lock().expect("verrou listes") == key {
        return Ok(());
    }
    let rt = rt.clone();
    tokio::task::spawn_blocking(move || load_blocklists(&rt, key)).await?;
    Ok(())
}

/// Charge depuis le disque les listes demandées et décharge les autres.
pub fn load_blocklists(rt: &Runtime, lists: Vec<(String, String)>) {
    let mut fresh = cotutelle_common::Blocklists::default();
    for (category, _) in &lists {
        match rt.paths.load_blocklist(category) {
            Ok(set) => fresh.insert(category.clone(), set),
            Err(e) => tracing::warn!(category, error = %e, "liste absente du disque"),
        }
    }
    *rt.blocklists.write().expect("verrou listes") = fresh;
    *rt.loaded_lists.lock().expect("verrou listes") = lists;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_url_follows_server_scheme() {
        assert_eq!(
            ws_url("http://cotutelle.local:8080/"),
            "ws://cotutelle.local:8080/api/v1/agent/ws"
        );
        assert_eq!(ws_url("https://cotutelle.example"), "wss://cotutelle.example/api/v1/agent/ws");
        assert_eq!(api_url("http://h:1/", "/agent/enroll"), "http://h:1/api/v1/agent/enroll");
    }
}
