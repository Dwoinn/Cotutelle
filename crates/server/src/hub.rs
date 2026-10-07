//! Hub des agents : connexions WebSocket, construction et diffusion de l'état.

use crate::state::Shared;
use crate::{lan, model, notify, util};
use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket};
use chrono::Utc;
use cotutelle_common::protocol::{
    AccountState, AgentMessage, DeviceState, ServerMessage, TamperKind,
};
use cotutelle_common::{ChildId, DeviceId, PROTOCOL_VERSION};
use futures_util::{SinkExt, StreamExt};
use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Un compte est « actif » s'il a signalé de l'activité depuis moins de…
const ACTIVE_WINDOW: Duration = Duration::from_secs(150);

struct Connection {
    /// Distingue deux connexions successives du même appareil.
    serial: u64,
    tx: mpsc::UnboundedSender<ServerMessage>,
}

#[derive(Default)]
pub struct Hub {
    connections: Mutex<HashMap<String, Connection>>,
    /// (appareil, compte) → dernier signal d'activité.
    active: Mutex<HashMap<(String, String), Instant>>,
    serial: std::sync::atomic::AtomicU64,
}

impl Hub {
    pub fn is_online(&self, device_id: &str) -> bool {
        self.connections.lock().expect("verrou hub").contains_key(device_id)
    }

    pub fn online_devices(&self) -> Vec<String> {
        self.connections.lock().expect("verrou hub").keys().cloned().collect()
    }

    /// Comptes actuellement au premier plan sur un appareil.
    pub fn active_accounts(&self, device_id: &str) -> Vec<String> {
        let active = self.active.lock().expect("verrou activité");
        active
            .iter()
            .filter(|((d, _), at)| d == device_id && at.elapsed() < ACTIVE_WINDOW)
            .map(|((_, account), _)| account.clone())
            .collect()
    }

    pub fn send(&self, device_id: &str, message: ServerMessage) -> bool {
        let connections = self.connections.lock().expect("verrou hub");
        connections.get(device_id).is_some_and(|c| c.tx.send(message).is_ok())
    }

    fn register(&self, device_id: &str, tx: mpsc::UnboundedSender<ServerMessage>) -> u64 {
        let serial = self.serial.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.connections
            .lock()
            .expect("verrou hub")
            .insert(device_id.to_string(), Connection { serial, tx });
        serial
    }

    fn unregister(&self, device_id: &str, serial: u64) {
        let mut connections = self.connections.lock().expect("verrou hub");
        // Ne retire pas une connexion plus récente du même appareil.
        if connections.get(device_id).is_some_and(|c| c.serial == serial) {
            connections.remove(device_id);
        }
        self.active.lock().expect("verrou activité").retain(|(d, _), _| d != device_id);
    }

    fn mark(&self, device_id: &str, account: &str, active: bool) {
        let mut map = self.active.lock().expect("verrou activité");
        let key = (device_id.to_string(), account.to_string());
        if active {
            map.insert(key, Instant::now());
        } else {
            map.remove(&key);
        }
    }
}

/// Construit l'état complet destiné à un appareil avec agent.
pub async fn device_state(state: &Shared, device_id: &str) -> Result<DeviceState> {
    let accounts: Vec<(String, String)> = sqlx::query_as(
        "SELECT os_account, child_id FROM device_accounts WHERE device_id = ? ORDER BY os_account",
    )
    .bind(device_id)
    .fetch_all(&state.db)
    .await?;
    let grants = model::live_grants(&state.db).await?;

    let mut out = Vec::new();
    let mut categories = BTreeSet::new();
    for (os_account, child_id) in accounts {
        let Some(child) = model::child(&state.db, &child_id).await? else { continue };
        let elsewhere = model::usage_seconds(&state.db, &child_id, Some(device_id)).await?;
        categories.extend(child.policy.filter.blocked_categories.iter().cloned());
        out.push(AccountState {
            os_account,
            child: ChildId(child_id.parse().context("identifiant d'enfant")?),
            child_name: child.name,
            policy: child.policy,
            grants: model::grants_of_child(&grants, &child_id),
            usage_elsewhere: model::usage_from_seconds(elsewhere),
        });
    }

    Ok(DeviceState {
        device: DeviceId(device_id.parse().context("identifiant d'appareil")?),
        issued_at: Utc::now(),
        accounts: out,
        blocklists: state.blocklists.refs_for(&categories),
        upstream_dns: lan::upstreams_setting(state).await.iter().map(ToString::to_string).collect(),
    })
}

pub async fn push(state: &Shared, device_id: &str) {
    if !state.hub.is_online(device_id) {
        return;
    }
    match device_state(state, device_id).await {
        Ok(device_state) => {
            state.hub.send(device_id, ServerMessage::State(Box::new(device_state)));
        }
        Err(e) => tracing::error!(device_id, error = ?e, "construction de l'état"),
    }
}

pub async fn push_all(state: &Shared) {
    for device_id in state.hub.online_devices() {
        push(state, &device_id).await;
    }
}

/// Boucle d'une connexion d'agent, jusqu'à sa fermeture.
pub async fn serve_agent(state: Shared, device_id: String, socket: WebSocket, peer_ip: String) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let serial = state.hub.register(&device_id, tx);
    tracing::info!(device_id, "agent connecté");

    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            let Ok(json) = serde_json::to_string(&message) else { continue };
            if sink.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(frame)) = stream.next().await {
        let text = match frame {
            Message::Text(text) => text,
            Message::Close(_) => break,
            _ => continue,
        };
        let message: AgentMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(device_id, error = %e, "message d'agent illisible");
                continue;
            }
        };
        if let Err(e) = handle(&state, &device_id, &peer_ip, message).await {
            tracing::error!(device_id, error = ?e, "traitement d'un message d'agent");
        }
    }

    state.hub.unregister(&device_id, serial);
    writer.abort();
    tracing::info!(device_id, "agent déconnecté");
}

async fn touch(state: &Shared, device_id: &str) -> Result<()> {
    sqlx::query("UPDATE devices SET last_seen = ? WHERE id = ?")
        .bind(util::now())
        .bind(device_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn child_of_account(
    state: &Shared,
    device_id: &str,
    os_account: &str,
) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT child_id FROM device_accounts WHERE device_id = ? AND os_account = ?",
    )
    .bind(device_id)
    .bind(os_account)
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(|(id,)| id))
}

async fn handle(
    state: &Shared,
    device_id: &str,
    peer_ip: &str,
    message: AgentMessage,
) -> Result<()> {
    match message {
        AgentMessage::Hello { agent_version, protocol_version, hostname, os_accounts, .. } => {
            if protocol_version != PROTOCOL_VERSION {
                tracing::warn!(device_id, protocol_version, "version de protocole différente");
            }
            sqlx::query(
                "UPDATE devices SET agent_version = ?, hostname = ?, os_accounts = ?, ip = ?, last_seen = ?
                 WHERE id = ?",
            )
            .bind(agent_version)
            .bind(hostname)
            .bind(serde_json::to_string(&os_accounts)?)
            .bind(peer_ip)
            .bind(util::now())
            .bind(device_id)
            .execute(&state.db)
            .await?;
            push(state, device_id).await;
        }
        AgentMessage::Ping => {
            touch(state, device_id).await?;
            state.hub.send(device_id, ServerMessage::Pong);
        }
        AgentMessage::Usage { os_account, day, seconds, active } => {
            state.hub.mark(device_id, &os_account, active);
            let Some(child_id) = child_of_account(state, device_id, &os_account).await? else {
                return Ok(());
            };
            sqlx::query(
                "INSERT INTO usage (device_id, os_account, day, child_id, seconds) VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT (device_id, os_account, day) DO UPDATE SET
                    seconds = MAX(seconds, excluded.seconds), child_id = excluded.child_id",
            )
            .bind(device_id)
            .bind(&os_account)
            .bind(util::day_str(day))
            .bind(&child_id)
            .bind(seconds)
            .execute(&state.db)
            .await?;
            touch(state, device_id).await?;
            // Le quota est partagé : les autres appareils de l'enfant doivent
            // connaître le temps consommé ici.
            let others: Vec<(String,)> = sqlx::query_as(
                "SELECT DISTINCT device_id FROM device_accounts WHERE child_id = ? AND device_id != ?",
            )
            .bind(&child_id)
            .bind(device_id)
            .fetch_all(&state.db)
            .await?;
            for (other,) in others {
                push(state, &other).await;
            }
        }
        AgentMessage::DnsSummary { os_account, day, domains } => {
            let child_id = match &os_account {
                Some(account) => child_of_account(state, device_id, account).await?,
                None => None,
            };
            // On ne conserve que l'activité des comptes rattachés à un enfant.
            if let Some(child_id) = child_id {
                lan::store_counts(state, &util::day_str(day), device_id, &child_id, &domains)
                    .await?;
            }
        }
        AgentMessage::Tamper { kind, details, .. } => {
            let (name,): (String,) = sqlx::query_as("SELECT name FROM devices WHERE id = ?")
                .bind(device_id)
                .fetch_one(&state.db)
                .await?;
            let what = match kind {
                TamperKind::ResolverConfigChanged => "la configuration DNS a été modifiée",
                TamperKind::FirewallRulesMissing => "les règles réseau de protection ont disparu",
                TamperKind::ClockChanged => "l'horloge a été modifiée",
                TamperKind::AgentRestarted => "l'agent a redémarré de façon inattendue",
                TamperKind::LockFailed => {
                    "la session n'a pas pu être verrouillée alors que l'accès est fermé"
                }
            };
            let message = if details.is_empty() {
                format!("{name} : {what}")
            } else {
                format!("{name} : {what} ({details})")
            };
            notify::alert(state, "tamper", Some(device_id), None, &message).await?;
        }
    }
    Ok(())
}
