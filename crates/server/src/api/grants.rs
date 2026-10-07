//! Exceptions temporaires et demandes des enfants.

use super::{describe_target, minutes_label};
use crate::auth::Parent;
use crate::error::{ApiError, ApiResult};
use crate::model::{self, Grant};
use crate::state::Shared;
use crate::util;
use axum::Json;
use axum::extract::{Path, State};
use cotutelle_common::{GrantTarget, catalog, normalize_domain};
use serde::Deserialize;
use serde_json::{Value, json};

const DEFAULT_MINUTES: u32 = 60;
const MAX_MINUTES: u32 = 24 * 60;
const MAX_PAUSE_MINUTES: u32 = 7 * 24 * 60;

pub async fn list(_: Parent, State(state): State<Shared>) -> ApiResult<Vec<Grant>> {
    Ok(Json(model::live_grants(&state.db).await?))
}

#[derive(Deserialize)]
pub struct GrantInput {
    child_id: Option<String>,
    device_id: Option<String>,
    target: GrantTarget,
    /// Durée de l'exception. Ignorée pour le temps bonus, valable jusqu'à minuit.
    minutes: Option<u32>,
}

/// Vérifie et normalise la cible d'une exception.
pub fn check_target(target: GrantTarget) -> Result<GrantTarget, ApiError> {
    match target {
        GrantTarget::Service { ref service } if catalog::service(service).is_none() => {
            Err(ApiError::bad_request("service inconnu"))
        }
        GrantTarget::Domain { domain } => {
            let domain = normalize_domain(
                domain
                    .trim()
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .unwrap_or(""),
            );
            let domain = domain.trim_start_matches("www.").to_string();
            if !domain.contains('.') || domain.contains(char::is_whitespace) || domain.len() > 253 {
                return Err(ApiError::bad_request("nom de site invalide"));
            }
            Ok(GrantTarget::Domain { domain })
        }
        GrantTarget::ExtraMinutes { minutes } if !(1..=600).contains(&minutes) => {
            Err(ApiError::bad_request("temps bonus : entre 1 et 600 minutes"))
        }
        other => Ok(other),
    }
}

/// Enregistre une exception et rend sa date d'expiration.
pub async fn insert_grant(
    state: &Shared,
    child_id: Option<&str>,
    device_id: Option<&str>,
    target: &GrantTarget,
    minutes: Option<u32>,
    parent_id: &str,
) -> Result<(String, i64), ApiError> {
    let now = util::now();
    let expires_at = match target {
        // Le temps bonus vaut pour la journée en cours.
        GrantTarget::ExtraMinutes { .. } => util::end_of_today(),
        GrantTarget::Pause => {
            now + i64::from(minutes.unwrap_or(DEFAULT_MINUTES).clamp(1, MAX_PAUSE_MINUTES)) * 60
        }
        _ => now + i64::from(minutes.unwrap_or(DEFAULT_MINUTES).clamp(1, MAX_MINUTES)) * 60,
    };
    let id = util::new_id();
    sqlx::query(
        "INSERT INTO grants (id, child_id, device_id, target, starts_at, expires_at, granted_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(child_id)
    .bind(device_id)
    .bind(serde_json::to_string(target)?)
    .bind(now)
    .bind(expires_at)
    .bind(parent_id)
    .bind(now)
    .execute(&state.db)
    .await
    .map_err(|_| ApiError::bad_request("enfant ou appareil inconnu"))?;
    Ok((id, expires_at))
}

pub async fn create(
    parent: Parent,
    State(state): State<Shared>,
    Json(body): Json<GrantInput>,
) -> ApiResult<Value> {
    if body.child_id.is_some() == body.device_id.is_some() {
        return Err(ApiError::bad_request("indiquer un enfant ou un appareil, pas les deux"));
    }
    let target = check_target(body.target)?;
    if body.device_id.is_some() && matches!(target, GrantTarget::ExtraMinutes { .. }) {
        return Err(ApiError::bad_request("le temps bonus s'accorde à un enfant"));
    }
    // Une nouvelle pause remplace la précédente ; lever la pause la supprime.
    if matches!(target, GrantTarget::Pause) {
        clear_pause(&state, body.child_id.as_deref(), body.device_id.as_deref()).await?;
    }
    let (id, expires_at) = insert_grant(
        &state,
        body.child_id.as_deref(),
        body.device_id.as_deref(),
        &target,
        body.minutes,
        &parent.id,
    )
    .await?;
    state.changed().await;
    Ok(Json(json!({ "id": id, "expires_at": expires_at })))
}

async fn clear_pause(
    state: &Shared,
    child_id: Option<&str>,
    device_id: Option<&str>,
) -> Result<(), ApiError> {
    sqlx::query(
        "DELETE FROM grants WHERE target = ? AND expires_at > ? AND child_id IS ? AND device_id IS ?",
    )
    .bind(serde_json::to_string(&GrantTarget::Pause)?)
    .bind(util::now())
    .bind(child_id)
    .bind(device_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

pub async fn revoke(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    let done = sqlx::query("DELETE FROM grants WHERE id = ?").bind(&id).execute(&state.db).await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("exception"));
    }
    state.changed().await;
    Ok(Json(json!({ "ok": true })))
}

type RequestRow = (String, String, String, String, Option<i64>, Option<String>, String, i64);

pub fn request_json(
    (id, child_id, child_name, target, minutes, message, status, created_at): RequestRow,
) -> Value {
    let target: Option<GrantTarget> = serde_json::from_str(&target).ok();
    json!({
        "id": id,
        "child_id": child_id,
        "child_name": child_name,
        "label": target.as_ref().map(describe_target),
        "target": target,
        "minutes": minutes,
        "message": message,
        "status": status,
        "created_at": created_at,
    })
}

pub const REQUEST_COLUMNS: &str =
    "r.id, r.child_id, c.name, r.target, r.minutes, r.message, r.status, r.created_at";

pub async fn pending_requests(state: &Shared) -> Result<Vec<Value>, ApiError> {
    let rows: Vec<RequestRow> = sqlx::query_as(&format!(
        "SELECT {REQUEST_COLUMNS} FROM requests r JOIN children c ON c.id = r.child_id
         WHERE r.status = 'pending' ORDER BY r.created_at"
    ))
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(request_json).collect())
}

pub async fn requests(_: Parent, State(state): State<Shared>) -> ApiResult<Vec<Value>> {
    Ok(Json(pending_requests(&state).await?))
}

#[derive(Deserialize, Default)]
pub struct ApproveInput {
    /// Durée accordée ; par défaut celle demandée par l'enfant.
    minutes: Option<u32>,
}

async fn take_pending(
    state: &Shared,
    id: &str,
) -> Result<(String, GrantTarget, Option<i64>), ApiError> {
    let row: Option<(String, String, Option<i64>)> = sqlx::query_as(
        "SELECT child_id, target, minutes FROM requests WHERE id = ? AND status = 'pending'",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let (child_id, target, minutes) =
        row.ok_or_else(|| ApiError::not_found("demande en attente"))?;
    Ok((child_id, serde_json::from_str(&target)?, minutes))
}

async fn resolve(state: &Shared, id: &str, status: &str, parent_id: &str) -> Result<(), ApiError> {
    sqlx::query("UPDATE requests SET status = ?, resolved_at = ?, resolved_by = ? WHERE id = ?")
        .bind(status)
        .bind(util::now())
        .bind(parent_id)
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(())
}

pub async fn approve(
    parent: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
    body: Option<Json<ApproveInput>>,
) -> ApiResult<Value> {
    let (child_id, mut target, requested) = take_pending(&state, &id).await?;
    let minutes = body.and_then(|b| b.minutes).or(requested.map(|m| m as u32));
    // Pour du temps bonus, la durée accordée remplace la durée demandée.
    if let (GrantTarget::ExtraMinutes { minutes: asked }, Some(granted)) = (&mut target, minutes) {
        *asked = granted.clamp(1, 600);
    }
    let (grant_id, expires_at) =
        insert_grant(&state, Some(&child_id), None, &target, minutes, &parent.id).await?;
    resolve(&state, &id, "approved", &parent.id).await?;
    state.changed().await;
    Ok(Json(json!({ "grant_id": grant_id, "expires_at": expires_at })))
}

pub async fn deny(
    parent: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    take_pending(&state, &id).await?;
    resolve(&state, &id, "denied", &parent.id).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Texte d'une demande pour la notification aux parents.
pub fn request_sentence(child_name: &str, target: &GrantTarget, minutes: Option<u32>) -> String {
    match (target, minutes) {
        (GrantTarget::ExtraMinutes { .. }, _) => {
            format!("{child_name} demande {}", describe_target(target))
        }
        (_, Some(m)) => {
            format!("{child_name} demande {} pendant {}", describe_target(target), minutes_label(m))
        }
        (_, None) => format!("{child_name} demande {}", describe_target(target)),
    }
}
