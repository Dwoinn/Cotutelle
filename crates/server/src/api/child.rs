//! Espace enfant : ce que l'enfant voit de sa situation, et ses demandes.
//!
//! Principe de transparence (D7) : cette vue contient les mêmes données de
//! suivi que celles présentées aux parents.

use super::grants::{REQUEST_COLUMNS, check_target, request_json, request_sentence};
use crate::auth::{self, ChildSession, Parent};
use crate::error::{ApiError, ApiResult};
use crate::state::Shared;
use crate::{logos, model, notify, util};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::response::{IntoResponse, Response};
use chrono::{Datelike, Local};
use cotutelle_common::GrantTarget;
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_PENDING: i64 = 5;

async fn space(state: &Shared, child_id: &str) -> Result<Value, ApiError> {
    let child =
        model::child(&state.db, child_id).await?.ok_or_else(|| ApiError::not_found("enfant"))?;
    let grants = model::live_grants(&state.db).await?;
    let seconds = model::usage_seconds(&state.db, child_id, None).await?;
    let child_grants = model::grants_of_child(&grants, child_id);
    let status =
        model::access_now(&child.policy, &child_grants, model::usage_from_seconds(seconds));
    let now = Local::now();
    let next = child.policy.schedule.next_opening(now.weekday(), now.time());

    let requests: Vec<_> = sqlx::query_as(&format!(
        "SELECT {REQUEST_COLUMNS} FROM requests r JOIN children c ON c.id = r.child_id
         WHERE r.child_id = ? AND (r.status = 'pending' OR r.created_at > ?) ORDER BY r.created_at DESC LIMIT 10"
    ))
    .bind(child_id)
    .bind(util::now() - 86_400)
    .fetch_all(&state.db)
    .await?;

    // L'enfant ne reçoit que les services qui le concernent : ceux que ses
    // parents lui proposent, et ceux qu'ils lui ont ouverts.
    let filter = &child.policy.filter;
    let requestable =
        |id: &str| filter.blocked_services.contains(id) && filter.requestable_services.contains(id);
    let granted = |id: &str| {
        child_grants
            .iter()
            .any(|g| matches!(&g.target, GrantTarget::Service { service } if service == id))
    };
    let catalog = model::services(&state.db).await?;
    let logos = logos::index(state).await?;
    let services: Vec<Value> = catalog
        .iter()
        .filter(|s| requestable(&s.id) || granted(&s.id))
        .map(|s| json!({ "id": s.id, "label": s.label, "logo": logos.get(&s.id) }))
        .collect();
    let requestable_services: Vec<&str> =
        catalog.iter().map(|s| s.id.as_str()).filter(|id| requestable(id)).collect();

    let activity = super::children::activity_for(state, "child_id", child_id, 7).await?;
    let blocked_today: i64 = model::blocked_today(&state.db)
        .await?
        .iter()
        .filter(|(c, _, _)| c == child_id)
        .map(|(_, _, n)| n)
        .sum();

    Ok(json!({
        "child": { "id": child.id, "name": child.name, "emoji": child.emoji, "color": child.color },
        "status": status,
        "used_today_seconds": seconds.0,
        "next_opening": next.map(|(day, time)| json!({ "day": day.to_string(), "time": time.format("%H:%M").to_string() })),
        "today_ranges": child.policy.schedule.ranges_for(now.weekday()),
        "grants": grants.iter().filter(|g| g.child_id.as_deref() == Some(child_id)).collect::<Vec<_>>(),
        "services": services,
        "requestable_services": requestable_services,
        "allow_requests": child.policy.filter.allow_requests,
        "requests": requests.into_iter().map(|row| request_json(&catalog, row)).collect::<Vec<_>>(),
        "activity": activity,
        "blocked_today": blocked_today,
        "now": util::now(),
    }))
}

#[derive(Deserialize)]
pub struct EnterQuery {
    token: String,
}

/// Échange le lien à usage unique fourni par l'agent contre une session
/// enfant, puis redirige vers l'espace enfant.
pub async fn enter(
    State(state): State<Shared>,
    Query(query): Query<EnterQuery>,
) -> Result<Response, ApiError> {
    let row: Option<(String, String)> = sqlx::query_as(
        "DELETE FROM child_links WHERE token_hash = ? AND expires_at > ? RETURNING child_id, device_id",
    )
    .bind(util::hash_token(&query.token))
    .bind(util::now())
    .fetch_optional(&state.db)
    .await?;
    let Some((child_id, device_id)) = row else {
        return Ok((StatusCode::SEE_OTHER, [(LOCATION, "/moi?lien=expire")]).into_response());
    };
    let cookie = auth::create_child_session(&state, &child_id, &device_id).await?;
    Ok((StatusCode::SEE_OTHER, [(LOCATION, "/moi".to_string()), (SET_COOKIE, cookie)])
        .into_response())
}

pub async fn me(session: ChildSession, State(state): State<Shared>) -> ApiResult<Value> {
    Ok(Json(space(&state, &session.child_id).await?))
}

/// Aperçu de l'espace enfant pour un parent.
pub async fn preview(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    Ok(Json(space(&state, &id).await?))
}

#[derive(Deserialize)]
pub struct RequestInput {
    target: GrantTarget,
    minutes: Option<u32>,
    message: Option<String>,
}

pub async fn request(
    session: ChildSession,
    State(state): State<Shared>,
    Json(body): Json<RequestInput>,
) -> ApiResult<Value> {
    let child = model::child(&state.db, &session.child_id)
        .await?
        .ok_or_else(|| ApiError::not_found("enfant"))?;
    if !child.policy.filter.allow_requests {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "les demandes ne sont pas activées"));
    }
    // Un enfant peut demander un service, un site ou du temps : rien d'autre.
    let services = model::services(&state.db).await?;
    let target = match check_target(&services, body.target)? {
        t @ (GrantTarget::Service { .. } | GrantTarget::Domain { .. }) => t,
        GrantTarget::ExtraMinutes { minutes } if (5..=120).contains(&minutes) => {
            GrantTarget::ExtraMinutes { minutes }
        }
        _ => return Err(ApiError::bad_request("demande non prise en charge")),
    };
    let minutes = match target {
        GrantTarget::ExtraMinutes { .. } => None,
        _ => Some(body.minutes.unwrap_or(60).clamp(5, 240)),
    };
    let message = body
        .message
        .map(|m| m.trim().chars().take(200).collect::<String>())
        .filter(|m| !m.is_empty());

    let (pending,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM requests WHERE child_id = ? AND status = 'pending'")
            .bind(&child.id)
            .fetch_one(&state.db)
            .await?;
    if pending >= MAX_PENDING {
        return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "trop de demandes en attente"));
    }

    let id = util::new_id();
    sqlx::query(
        "INSERT INTO requests (id, child_id, device_id, target, minutes, message, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&child.id)
    .bind(&session.device_id)
    .bind(serde_json::to_string(&target)?)
    .bind(minutes)
    .bind(&message)
    .bind(util::now())
    .execute(&state.db)
    .await?;

    let mut text = request_sentence(&services, &child.name, &target, minutes);
    if let Some(message) = &message {
        text.push_str(&format!(" : « {message} »"));
    }
    notify::push(&state, "Cotutelle : nouvelle demande", &text).await;
    Ok(Json(json!({ "id": id })))
}
