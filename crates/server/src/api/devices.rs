//! Appareils : avec agent (enrôlés par code) ou réseau (filtrés par IP).

use super::children::{ActivityQuery, activity_for, sanitize};
use crate::auth::Parent;
use crate::error::{ApiError, ApiResult};
use crate::model;
use crate::state::Shared;
use crate::util;
use axum::Json;
use axum::extract::{Path, Query, State};
use cotutelle_common::Policy;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::net::IpAddr;

const ENROLL_CODE_MINUTES: i64 = 15;

type DeviceRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<i64>,
);

pub async fn list_devices(state: &Shared) -> Result<Vec<Value>, ApiError> {
    let rows: Vec<DeviceRow> = sqlx::query_as(
        "SELECT id, name, kind, hostname, os, agent_version, ip, child_id, policy, os_accounts, last_seen
         FROM devices ORDER BY kind, name",
    )
    .fetch_all(&state.db)
    .await?;
    let links: Vec<(String, String, String)> =
        sqlx::query_as("SELECT device_id, os_account, child_id FROM device_accounts")
            .fetch_all(&state.db)
            .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name, kind, hostname, os, agent_version, ip, child_id, policy, os_accounts, last_seen)| {
            let accounts: BTreeMap<&str, &str> =
                links.iter().filter(|(d, _, _)| *d == id).map(|(_, a, c)| (a.as_str(), c.as_str())).collect();
            json!({
                "id": id,
                "name": name,
                "kind": kind,
                "hostname": hostname,
                "os": os,
                "agent_version": agent_version,
                "ip": ip,
                "child_id": child_id,
                "policy": policy.and_then(|p| serde_json::from_str::<Policy>(&p).ok()),
                "os_accounts": serde_json::from_str::<Vec<String>>(&os_accounts).unwrap_or_default(),
                "accounts": accounts,
                "last_seen": last_seen,
                "online": state.hub.is_online(&id),
                "active_accounts": state.hub.active_accounts(&id),
            })
        })
        .collect())
}

pub async fn list(_: Parent, State(state): State<Shared>) -> ApiResult<Vec<Value>> {
    Ok(Json(list_devices(&state).await?))
}

#[derive(Deserialize)]
pub struct NetworkDeviceInput {
    name: String,
    ip: String,
    /// Enfant auquel l'appareil est attribué ; absent pour un appareil partagé.
    child_id: Option<String>,
    /// Politique propre d'un appareil partagé.
    policy: Option<Policy>,
}

async fn check_network_input(
    state: &Shared,
    body: &NetworkDeviceInput,
    own_id: Option<&str>,
) -> Result<(String, String), ApiError> {
    let name = super::clean_name(&body.name, "nom")?;
    let ip: IpAddr =
        body.ip.trim().parse().map_err(|_| ApiError::bad_request("adresse IP invalide"))?;
    let ip = ip.to_string();
    let clash: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM devices WHERE kind = 'network' AND ip = ? AND id IS NOT ?",
    )
    .bind(&ip)
    .bind(own_id)
    .fetch_optional(&state.db)
    .await?;
    if let Some((other,)) = clash {
        return Err(ApiError::conflict(format!(
            "l'adresse {ip} est déjà utilisée par « {other} »"
        )));
    }
    if let Some(child_id) = &body.child_id {
        model::child(&state.db, child_id).await?.ok_or_else(|| ApiError::not_found("enfant"))?;
    }
    Ok((name, ip))
}

/// Déclare un appareil sans agent (TV, console…), filtré par le DNS du serveur.
pub async fn create_network(
    _: Parent,
    State(state): State<Shared>,
    Json(body): Json<NetworkDeviceInput>,
) -> ApiResult<Value> {
    let (name, ip) = check_network_input(&state, &body, None).await?;
    // Appareil partagé sans politique fournie : la plus stricte de la maison.
    let policy = match (&body.child_id, body.policy) {
        (Some(_), _) => None,
        (None, Some(policy)) => Some(sanitize(policy)),
        (None, None) => Some(model::strictest_policy(&model::children(&state.db).await?)),
    };
    let id = util::new_id();
    sqlx::query(
        "INSERT INTO devices (id, name, kind, ip, child_id, policy, created_at) VALUES (?, ?, 'network', ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(ip)
    .bind(&body.child_id)
    .bind(policy.map(|p| serde_json::to_string(&p)).transpose()?)
    .bind(util::now())
    .execute(&state.db)
    .await?;
    state.changed().await;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct DeviceUpdate {
    name: String,
    ip: Option<String>,
    child_id: Option<String>,
    policy: Option<Policy>,
}

pub async fn update(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<DeviceUpdate>,
) -> ApiResult<Value> {
    let (kind,): (String,) = sqlx::query_as("SELECT kind FROM devices WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::not_found("appareil"))?;

    if kind == "agent" {
        let name = super::clean_name(&body.name, "nom")?;
        sqlx::query("UPDATE devices SET name = ? WHERE id = ?")
            .bind(name)
            .bind(&id)
            .execute(&state.db)
            .await?;
    } else {
        let input = NetworkDeviceInput {
            name: body.name,
            ip: body.ip.unwrap_or_default(),
            child_id: body.child_id,
            policy: body.policy,
        };
        let (name, ip) = check_network_input(&state, &input, Some(&id)).await?;
        let policy = match (&input.child_id, input.policy) {
            (Some(_), _) => None,
            (None, Some(policy)) => Some(sanitize(policy)),
            (None, None) => Some(model::strictest_policy(&model::children(&state.db).await?)),
        };
        sqlx::query("UPDATE devices SET name = ?, ip = ?, child_id = ?, policy = ? WHERE id = ?")
            .bind(name)
            .bind(ip)
            .bind(&input.child_id)
            .bind(policy.map(|p| serde_json::to_string(&p)).transpose()?)
            .bind(&id)
            .execute(&state.db)
            .await?;
    }
    state.changed().await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    let done = sqlx::query("DELETE FROM devices WHERE id = ?").bind(&id).execute(&state.db).await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("appareil"));
    }
    state.changed().await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct EnrollCodeInput {
    name: String,
}

/// Génère le code à saisir lors de l'installation de l'agent.
pub async fn enroll_code(
    _: Parent,
    State(state): State<Shared>,
    Json(body): Json<EnrollCodeInput>,
) -> ApiResult<Value> {
    let name = super::clean_name(&body.name, "nom")?;
    let code = util::new_enroll_code();
    let expires_at = util::now() + ENROLL_CODE_MINUTES * 60;
    sqlx::query("INSERT INTO enroll_tokens (token_hash, device_name, expires_at) VALUES (?, ?, ?)")
        .bind(util::hash_token(&util::normalize_code(&code)))
        .bind(name)
        .bind(expires_at)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "code": code, "expires_at": expires_at })))
}

/// Rattache les comptes de session d'un appareil à des enfants.
/// Corps : `{ "louis": "<id enfant>", "papa": null }`.
pub async fn set_accounts(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<BTreeMap<String, Option<String>>>,
) -> ApiResult<Value> {
    let exists: Option<(String,)> =
        sqlx::query_as("SELECT id FROM devices WHERE id = ? AND kind = 'agent'")
            .bind(&id)
            .fetch_optional(&state.db)
            .await?;
    exists.ok_or_else(|| ApiError::not_found("appareil"))?;

    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM device_accounts WHERE device_id = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    for (account, child_id) in &body {
        let Some(child_id) = child_id else { continue };
        if account.trim().is_empty() {
            continue;
        }
        sqlx::query(
            "INSERT INTO device_accounts (device_id, os_account, child_id) VALUES (?, ?, ?)",
        )
        .bind(&id)
        .bind(account.trim())
        .bind(child_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError::bad_request("enfant inconnu"))?;
    }
    tx.commit().await?;
    state.changed().await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn activity(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(query): Query<ActivityQuery>,
) -> ApiResult<Value> {
    Ok(Json(activity_for(&state, "device_id", &id, query.days.unwrap_or(7)).await?))
}
