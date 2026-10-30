//! Espace enfant : ce que l'enfant voit de sa situation, et ses demandes.
//!
//! Principe de transparence (D7) : cette vue contient les mêmes données de
//! suivi que celles présentées aux parents.

use super::grants::{REQUEST_COLUMNS, check_target, request_json, request_sentence};
use crate::auth::{self, ChildSession, Parent};
use crate::error::{ApiError, ApiResult};
use crate::state::Shared;
use crate::{holidays, logos, model, notify, util};
use anyhow::Context;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::response::{IntoResponse, Response};
use chrono::{Datelike, Local};
use cotutelle_common::GrantTarget;
use cotutelle_common::session::minutes_label;
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_PENDING: i64 = 5;

async fn space(state: &Shared, child_id: &str) -> Result<Value, ApiError> {
    let child =
        model::child(&state.db, child_id).await?.ok_or_else(|| ApiError::not_found("enfant"))?;
    let grants = model::live_grants(&state.db).await?;
    let seconds = model::usage_seconds(&state.db, child_id, None).await?;
    let child_grants = model::grants_of_child(&grants, child_id);
    let holidays = holidays::periods(state).await;
    let now = Local::now();
    let rules = child.policy.rules_on(now.date_naive(), &holidays);
    let status = model::access_now(rules, &child_grants, model::usage_from_seconds(seconds));
    let next = rules.schedule.next_opening(now.weekday(), now.time());

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
    let filter = rules.filter;
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
        "today_ranges": rules.schedule.ranges_for(now.weekday()),
        "holiday": rules.holiday.map(|h| json!({ "label": h.label, "end": h.end })),
        "grants": grants.iter().filter(|g| g.child_id.as_deref() == Some(child_id)).collect::<Vec<_>>(),
        "services": services,
        "requestable_services": requestable_services,
        "allow_requests": filter.allow_requests,
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

/// Pourquoi une demande n'est pas enregistrée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NotAllowed,
    Unsupported,
    TooMany,
}

impl Refusal {
    /// Phrase pour l'enfant.
    pub fn text(self) -> &'static str {
        match self {
            Refusal::NotAllowed => "Les demandes ne sont pas activées pour toi.",
            Refusal::Unsupported => "Cette demande n'est pas possible.",
            Refusal::TooMany => {
                "Tu as déjà plusieurs demandes en attente : attends la réponse de tes parents."
            }
        }
    }
}

/// Enregistre une demande de l'enfant et prévient les parents. Même règle
/// quelle que soit l'origine, espace web ou bureau de l'enfant (D11) : un
/// service, un site ou du temps, si ses parents ont activé les demandes.
pub async fn create_request(
    state: &Shared,
    child_id: &str,
    device_id: Option<&str>,
    target: GrantTarget,
    minutes: Option<u32>,
    message: Option<String>,
) -> anyhow::Result<Result<String, Refusal>> {
    let child = model::child(&state.db, child_id).await?.context("enfant introuvable")?;
    let holidays = holidays::periods(state).await;
    if !model::rules_today(&child.policy, &holidays).filter.allow_requests {
        return Ok(Err(Refusal::NotAllowed));
    }
    let services = model::services(&state.db).await?;
    let target = match check_target(&services, target) {
        Ok(t @ (GrantTarget::Service { .. } | GrantTarget::Domain { .. })) => t,
        Ok(GrantTarget::ExtraMinutes { minutes }) if (5..=120).contains(&minutes) => {
            GrantTarget::ExtraMinutes { minutes }
        }
        _ => return Ok(Err(Refusal::Unsupported)),
    };
    let minutes = match target {
        GrantTarget::ExtraMinutes { .. } => None,
        _ => Some(minutes.unwrap_or(60).clamp(5, 240)),
    };
    let message =
        message.map(|m| m.trim().chars().take(200).collect::<String>()).filter(|m| !m.is_empty());

    let (pending,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM requests WHERE child_id = ? AND status = 'pending'")
            .bind(&child.id)
            .fetch_one(&state.db)
            .await?;
    if pending >= MAX_PENDING {
        return Ok(Err(Refusal::TooMany));
    }

    let id = util::new_id();
    sqlx::query(
        "INSERT INTO requests (id, child_id, device_id, target, minutes, message, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&child.id)
    .bind(device_id)
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
    let approve_label = match (&target, minutes) {
        (GrantTarget::ExtraMinutes { minutes }, _) => {
            format!("Accorder {}", minutes_label(*minutes))
        }
        (_, Some(m)) => format!("Accorder {}", minutes_label(m)),
        (_, None) => "Accorder".to_string(),
    };
    notify::push_request(state, &id, &text, &approve_label).await;
    Ok(Ok(id))
}

pub async fn request(
    session: ChildSession,
    State(state): State<Shared>,
    Json(body): Json<RequestInput>,
) -> ApiResult<Value> {
    let outcome = create_request(
        &state,
        &session.child_id,
        session.device_id.as_deref(),
        body.target,
        body.minutes,
        body.message,
    )
    .await?;
    match outcome {
        Ok(id) => Ok(Json(json!({ "id": id }))),
        Err(Refusal::NotAllowed) => {
            Err(ApiError::new(StatusCode::FORBIDDEN, "les demandes ne sont pas activées"))
        }
        Err(Refusal::Unsupported) => Err(ApiError::bad_request("demande non prise en charge")),
        Err(Refusal::TooMany) => {
            Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "trop de demandes en attente"))
        }
    }
}
