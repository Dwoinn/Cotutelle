//! Exceptions temporaires et demandes des enfants.

use super::{describe_target, minutes_label};
use crate::auth::Parent;
use crate::error::{ApiError, ApiResult};
use crate::model::{self, Grant};
use crate::state::Shared;
use crate::{notify, util};
use anyhow::Result;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use cotutelle_common::protocol::ServerMessage;
use cotutelle_common::{GrantTarget, Service, clean_domain, service_for_domain};
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
///
/// Un site qui fait partie d'un service désigne le service entier : ouvrir
/// `youtube.com` seul laisserait ses vidéos bloquées sur `googlevideo.com`.
pub fn check_target(services: &[Service], target: GrantTarget) -> Result<GrantTarget, ApiError> {
    match target {
        GrantTarget::Service { ref service } if !services.iter().any(|s| s.id == *service) => {
            Err(ApiError::bad_request("service inconnu"))
        }
        GrantTarget::Domain { domain } => {
            let domain = clean_domain(&domain)
                .ok_or_else(|| ApiError::bad_request("nom de site invalide"))?;
            Ok(match service_for_domain(services, &domain) {
                Some(service) => GrantTarget::Service { service: service.id.clone() },
                None => GrantTarget::Domain { domain },
            })
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
    parent_id: Option<&str>,
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
    let services = model::services(&state.db).await?;
    let target = check_target(&services, body.target)?;
    if body.device_id.is_some() && matches!(target, GrantTarget::ExtraMinutes { .. }) {
        return Err(ApiError::bad_request("le temps bonus s'accorde à un enfant"));
    }
    replace_same(&state, body.child_id.as_deref(), body.device_id.as_deref(), &target).await?;
    let (id, expires_at) = insert_grant(
        &state,
        body.child_id.as_deref(),
        body.device_id.as_deref(),
        &target,
        body.minutes,
        Some(&parent.id),
    )
    .await?;
    state.changed().await;
    Ok(Json(json!({
        "id": id,
        "expires_at": expires_at,
        // La cible retenue : le service entier quand le site en fait partie.
        "target": target,
        "label": describe_target(&services, &target),
    })))
}

/// Une mesure remplace celle de même cible encore en cours : rouvrir YouTube
/// en change la durée, sans afficher deux ouvertures. Seul le temps bonus se cumule.
async fn replace_same(
    state: &Shared,
    child_id: Option<&str>,
    device_id: Option<&str>,
    target: &GrantTarget,
) -> Result<(), ApiError> {
    if matches!(target, GrantTarget::ExtraMinutes { .. }) {
        return Ok(());
    }
    sqlx::query(
        "DELETE FROM grants WHERE target = ? AND expires_at > ? AND child_id IS ? AND device_id IS ?",
    )
    .bind(serde_json::to_string(target)?)
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
    services: &[Service],
    (id, child_id, child_name, target, minutes, message, status, created_at): RequestRow,
) -> Value {
    let target: Option<GrantTarget> = serde_json::from_str(&target).ok();
    json!({
        "id": id,
        "child_id": child_id,
        "child_name": child_name,
        "label": target.as_ref().map(|t| describe_target(services, t)),
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
    let services = model::services(&state.db).await?;
    Ok(rows.into_iter().map(|row| request_json(&services, row)).collect())
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

async fn resolve(
    state: &Shared,
    id: &str,
    status: &str,
    parent_id: Option<&str>,
) -> Result<(), ApiError> {
    sqlx::query("UPDATE requests SET status = ?, resolved_at = ?, resolved_by = ? WHERE id = ?")
        .bind(status)
        .bind(util::now())
        .bind(parent_id)
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// Porte la réponse des parents à l'appareil d'où la demande est partie,
/// pour que l'enfant la voie sur son bureau.
async fn answer_device(state: &Shared, request_id: &str, approved: bool, text: String) {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT r.device_id, a.os_account FROM requests r
         JOIN device_accounts a ON a.device_id = r.device_id AND a.child_id = r.child_id
         WHERE r.id = ?",
    )
    .bind(request_id)
    .fetch_optional(&state.db)
    .await
    .unwrap_or_default();
    if let Some((device_id, os_account)) = row {
        state.hub.send(&device_id, ServerMessage::RequestAnswered { os_account, approved, text });
    }
}

/// Qui tranche une demande : un parent connecté, ou un parent depuis la
/// notification de son téléphone, que le serveur ne sait pas nommer.
pub struct Decider {
    id: Option<String>,
    name: String,
}

impl From<&Parent> for Decider {
    fn from(parent: &Parent) -> Self {
        Decider { id: Some(parent.id.clone()), name: parent.name.clone() }
    }
}

impl Decider {
    fn phone() -> Self {
        Decider { id: None, name: "Un parent".to_string() }
    }
}

/// Une demande tranchée, de quoi en rendre compte.
pub struct Answered {
    pub child_name: String,
    pub what: String,
    pub minutes: Option<u32>,
    /// Exception créée, avec son expiration ; rien pour un refus.
    pub grant: Option<(String, i64)>,
}

async fn child_name(state: &Shared, child_id: &str) -> String {
    model::child(&state.db, child_id).await.ok().flatten().map(|c| c.name).unwrap_or_default()
}

/// Accorde une demande en attente ; `minutes` remplace la durée demandée.
pub async fn approve_request(
    state: &Shared,
    id: &str,
    minutes: Option<u32>,
    who: &Decider,
) -> Result<Answered, ApiError> {
    let (child_id, mut target, requested) = take_pending(state, id).await?;
    let minutes = minutes.or(requested.map(|m| m as u32));
    // Pour du temps bonus, la durée accordée remplace la durée demandée.
    if let (GrantTarget::ExtraMinutes { minutes: asked }, Some(granted)) = (&mut target, minutes) {
        *asked = granted.clamp(1, 600);
    }
    replace_same(state, Some(&child_id), None, &target).await?;
    let grant =
        insert_grant(state, Some(&child_id), None, &target, minutes, who.id.as_deref()).await?;
    resolve(state, id, "approved", who.id.as_deref()).await?;
    let services = model::services(&state.db).await?;
    let what = describe_target(&services, &target);
    let text = match (&target, minutes) {
        (GrantTarget::ExtraMinutes { .. }, _) => format!("{} t'a accordé {what}.", who.name),
        (_, Some(m)) => format!("{} a accordé {what} pendant {}.", who.name, minutes_label(m)),
        (_, None) => format!("{} a accordé {what}.", who.name),
    };
    answer_device(state, id, true, text).await;
    state.changed().await;
    let child_name = child_name(state, &child_id).await;
    Ok(Answered { child_name, what, minutes, grant: Some(grant) })
}

pub async fn deny_request(state: &Shared, id: &str, who: &Decider) -> Result<Answered, ApiError> {
    let (child_id, target, _) = take_pending(state, id).await?;
    resolve(state, id, "denied", who.id.as_deref()).await?;
    let services = model::services(&state.db).await?;
    let what = describe_target(&services, &target);
    answer_device(state, id, false, format!("{} n'a pas accordé {what}.", who.name)).await;
    let child_name = child_name(state, &child_id).await;
    Ok(Answered { child_name, what, minutes: None, grant: None })
}

pub async fn approve(
    parent: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
    body: Option<Json<ApproveInput>>,
) -> ApiResult<Value> {
    let minutes = body.and_then(|b| b.minutes);
    let answered = approve_request(&state, &id, minutes, &Decider::from(&parent)).await?;
    let (grant_id, expires_at) = answered.grant.unwrap_or_default();
    Ok(Json(json!({ "grant_id": grant_id, "expires_at": expires_at })))
}

pub async fn deny(
    parent: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    deny_request(&state, &id, &Decider::from(&parent)).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Applique la réponse d'un parent depuis la notification de son téléphone.
/// Rend le compte rendu à lui renvoyer, ou rien si la demande n'était plus
/// en attente.
pub async fn answer_from_phone(state: &Shared, reply: &notify::Reply) -> Result<Option<String>> {
    let who = Decider::phone();
    let outcome = match reply.decision {
        notify::Decision::Approve => approve_request(state, &reply.request_id, None, &who).await,
        notify::Decision::Deny => deny_request(state, &reply.request_id, &who).await,
    };
    let answered = match outcome {
        Ok(answered) => answered,
        Err(e) if e.status() == StatusCode::NOT_FOUND => return Ok(None),
        Err(e) => return Err(anyhow::anyhow!("{}", e.message())),
    };
    let Answered { child_name, what, minutes, grant } = answered;
    Ok(Some(match (grant.is_some(), minutes) {
        (true, Some(m)) => format!("Accordé à {child_name} : {what} pendant {}.", minutes_label(m)),
        (true, None) => format!("Accordé à {child_name} : {what}."),
        (false, _) => format!("Refusé à {child_name} : {what}."),
    }))
}

/// Texte d'une demande pour la notification aux parents.
pub fn request_sentence(
    services: &[Service],
    child_name: &str,
    target: &GrantTarget,
    minutes: Option<u32>,
) -> String {
    let what = describe_target(services, target);
    match (target, minutes) {
        (GrantTarget::ExtraMinutes { .. }, _) | (_, None) => format!("{child_name} demande {what}"),
        (_, Some(m)) => format!("{child_name} demande {what} pendant {}", minutes_label(m)),
    }
}
