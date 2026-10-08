//! Enfants et leur politique.

use crate::auth::Parent;
use crate::error::{ApiError, ApiResult};
use crate::model::{self, Child};
use crate::state::Shared;
use crate::util;
use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::Duration;
use cotutelle_common::{Policy, clean_domain};
use serde::Deserialize;
use serde_json::{Value, json};

const EMOJIS: &[&str] = &["🦊", "🐼", "🦁", "🐙", "🦄", "🐢", "🚀", "⚽", "🎨", "🎸"];
const COLORS: &[&str] = &["#6366f1", "#f59e0b", "#10b981", "#ec4899", "#0ea5e9", "#8b5cf6"];

#[derive(Deserialize)]
pub struct ChildInput {
    name: String,
    birth_year: Option<i64>,
    emoji: Option<String>,
    color: Option<String>,
    policy: Option<Policy>,
}

fn check_birth_year(year: Option<i64>) -> Result<(), ApiError> {
    use chrono::Datelike;
    let this_year = i64::from(util::today().year());
    if year.is_some_and(|y| !(this_year - 25..=this_year).contains(&y)) {
        return Err(ApiError::bad_request("année de naissance invalide"));
    }
    Ok(())
}

/// Nettoie une politique reçue de l'interface : listes personnelles en forme
/// canonique et sans vides, services proposés pris parmi les services bloqués.
pub fn sanitize(mut policy: Policy) -> Policy {
    let filter = &mut policy.filter;
    for list in [&mut filter.allow, &mut filter.deny] {
        *list = list.iter().filter_map(|d| clean_domain(d)).collect();
    }
    filter.requestable_services.retain(|s| filter.blocked_services.contains(s));
    policy
}

pub async fn list(_: Parent, State(state): State<Shared>) -> ApiResult<Vec<Child>> {
    Ok(Json(model::children(&state.db).await?))
}

pub async fn get(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Child> {
    model::child(&state.db, &id).await?.map(Json).ok_or_else(|| ApiError::not_found("enfant"))
}

pub async fn create(
    _: Parent,
    State(state): State<Shared>,
    Json(body): Json<ChildInput>,
) -> ApiResult<Child> {
    let name = super::clean_name(&body.name, "prénom")?;
    check_birth_year(body.birth_year)?;
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM children").fetch_one(&state.db).await?;
    let pick = |list: &[&str]| list[count as usize % list.len()].to_string();
    let child = Child {
        id: util::new_id(),
        name,
        birth_year: body.birth_year,
        emoji: body.emoji.filter(|e| !e.is_empty()).unwrap_or_else(|| pick(EMOJIS)),
        color: body.color.filter(|c| !c.is_empty()).unwrap_or_else(|| pick(COLORS)),
        policy: sanitize(body.policy.unwrap_or_else(|| model::default_policy(body.birth_year))),
    };
    sqlx::query(
        "INSERT INTO children (id, name, birth_year, emoji, color, policy, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&child.id)
    .bind(&child.name)
    .bind(child.birth_year)
    .bind(&child.emoji)
    .bind(&child.color)
    .bind(serde_json::to_string(&child.policy)?)
    .bind(util::now())
    .execute(&state.db)
    .await?;
    Ok(Json(child))
}

pub async fn update(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<ChildInput>,
) -> ApiResult<Child> {
    let mut child =
        model::child(&state.db, &id).await?.ok_or_else(|| ApiError::not_found("enfant"))?;
    child.name = super::clean_name(&body.name, "prénom")?;
    check_birth_year(body.birth_year)?;
    child.birth_year = body.birth_year;
    if let Some(emoji) = body.emoji.filter(|e| !e.is_empty()) {
        child.emoji = emoji;
    }
    if let Some(color) = body.color.filter(|c| !c.is_empty()) {
        child.color = color;
    }
    if let Some(policy) = body.policy {
        child.policy = sanitize(policy);
    }
    sqlx::query("UPDATE children SET name = ?, birth_year = ?, emoji = ?, color = ?, policy = ? WHERE id = ?")
        .bind(&child.name)
        .bind(child.birth_year)
        .bind(&child.emoji)
        .bind(&child.color)
        .bind(serde_json::to_string(&child.policy)?)
        .bind(&id)
        .execute(&state.db)
        .await?;
    state.changed().await;
    Ok(Json(child))
}

pub async fn remove(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    let done =
        sqlx::query("DELETE FROM children WHERE id = ?").bind(&id).execute(&state.db).await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("enfant"));
    }
    // dns_daily n'a pas de clé étrangère sur l'enfant ('' = appareil partagé).
    sqlx::query("DELETE FROM dns_daily WHERE child_id = ?").bind(&id).execute(&state.db).await?;
    state.changed().await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ActivityQuery {
    pub days: Option<i64>,
}

/// Activité sur `days` jours : temps d'écran et sites (niveaux 1 et 2).
/// `filter` est une colonne (`child_id` ou `device_id`) et sa valeur.
pub async fn activity_for(
    state: &Shared,
    column: &str,
    value: &str,
    days: i64,
) -> Result<Value, ApiError> {
    let days = days.clamp(1, 90);
    let first = util::today() - Duration::days(days - 1);
    let since = util::day_str(first);

    let per_day: Vec<(String, i64)> = sqlx::query_as(&format!(
        "SELECT day, SUM(seconds) FROM usage WHERE {column} = ? AND day >= ? GROUP BY day"
    ))
    .bind(value)
    .bind(&since)
    .fetch_all(&state.db)
    .await?;
    let day_list: Vec<Value> = (0..days)
        .map(|offset| {
            let day = util::day_str(first + Duration::days(offset));
            let seconds = per_day.iter().find(|(d, _)| *d == day).map_or(0, |(_, s)| *s);
            json!({ "day": day, "seconds": seconds })
        })
        .collect();

    let per_device: Vec<(String, String, i64)> = sqlx::query_as(&format!(
        "SELECT d.id, d.name, SUM(u.seconds) FROM usage u JOIN devices d ON d.id = u.device_id
         WHERE u.{column} = ? AND u.day >= ? GROUP BY d.id ORDER BY 3 DESC"
    ))
    .bind(value)
    .bind(&since)
    .fetch_all(&state.db)
    .await?;

    let top: Vec<(String, i64, i64)> = sqlx::query_as(&format!(
        "SELECT domain, SUM(allowed), SUM(blocked) FROM dns_daily WHERE {column} = ? AND day >= ?
         GROUP BY domain HAVING SUM(allowed) > 0 ORDER BY 2 DESC LIMIT 25"
    ))
    .bind(value)
    .bind(&since)
    .fetch_all(&state.db)
    .await?;

    let blocked: Vec<(String, i64, Option<String>)> = sqlx::query_as(&format!(
        "SELECT domain, SUM(blocked), MAX(reason) FROM dns_daily WHERE {column} = ? AND day >= ?
         GROUP BY domain HAVING SUM(blocked) > 0 ORDER BY 2 DESC LIMIT 25"
    ))
    .bind(value)
    .bind(&since)
    .fetch_all(&state.db)
    .await?;

    Ok(json!({
        "days": day_list,
        "devices": per_device.into_iter().map(|(id, name, seconds)| json!({ "id": id, "name": name, "seconds": seconds })).collect::<Vec<_>>(),
        "top_domains": top.into_iter().map(|(domain, allowed, blocked)| json!({ "domain": domain, "allowed": allowed, "blocked": blocked })).collect::<Vec<_>>(),
        "blocked": blocked.into_iter().map(|(domain, count, reason)| json!({
            "domain": domain,
            "blocked": count,
            "reason": reason.and_then(|r| serde_json::from_str::<Value>(&r).ok()),
        })).collect::<Vec<_>>(),
    }))
}

pub async fn activity(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(query): Query<ActivityQuery>,
) -> ApiResult<Value> {
    model::child(&state.db, &id).await?.ok_or_else(|| ApiError::not_found("enfant"))?;
    Ok(Json(activity_for(&state, "child_id", &id, query.days.unwrap_or(7)).await?))
}
