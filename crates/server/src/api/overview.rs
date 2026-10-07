//! Tableau de bord et alertes.

use super::devices::list_devices;
use super::grants::pending_requests;
use crate::auth::Parent;
use crate::error::{ApiError, ApiResult};
use crate::model;
use crate::state::Shared;
use crate::util;
use axum::Json;
use axum::extract::{Path, State};
use chrono::{Datelike, Local};
use serde_json::{Value, json};

async fn open_alerts(state: &Shared) -> Result<Vec<Value>, ApiError> {
    type Row = (String, String, Option<String>, Option<String>, String, i64);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, kind, device_id, child_id, message, created_at FROM alerts
         WHERE acked_at IS NULL ORDER BY created_at DESC LIMIT 50",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, kind, device_id, child_id, message, created_at)| {
            json!({ "id": id, "kind": kind, "device_id": device_id, "child_id": child_id, "message": message, "created_at": created_at })
        })
        .collect())
}

pub async fn dashboard(_: Parent, State(state): State<Shared>) -> ApiResult<Value> {
    let grants = model::live_grants(&state.db).await?;
    let devices = list_devices(&state).await?;
    let now = Local::now();
    let blocked = model::blocked_today(&state.db).await?;

    let mut children = Vec::new();
    for child in model::children(&state.db).await? {
        let seconds = model::usage_seconds(&state.db, &child.id, None).await?;
        let child_grants = model::grants_of_child(&grants, &child.id);
        let status =
            model::access_now(&child.policy, &child_grants, model::usage_from_seconds(seconds));
        let next = child.policy.schedule.next_opening(now.weekday(), now.time());

        // Appareils de l'enfant : comptes rattachés ou appareil réseau attribué.
        let mine: Vec<Value> = devices
            .iter()
            .filter_map(|d| {
                let accounts: Vec<&str> = d["accounts"]
                    .as_object()?
                    .iter()
                    .filter(|(_, c)| c.as_str() == Some(child.id.as_str()))
                    .map(|(a, _)| a.as_str())
                    .collect();
                let assigned = d["child_id"].as_str() == Some(child.id.as_str());
                if accounts.is_empty() && !assigned {
                    return None;
                }
                let active = d["active_accounts"]
                    .as_array()
                    .is_some_and(|list| list.iter().any(|a| accounts.contains(&a.as_str().unwrap_or(""))));
                Some(json!({ "id": d["id"], "name": d["name"], "kind": d["kind"], "online": d["online"], "active": active }))
            })
            .collect();

        children.push(json!({
            "id": child.id,
            "name": child.name,
            "emoji": child.emoji,
            "color": child.color,
            "status": status,
            "used_today_seconds": seconds.0,
            "used_week_seconds": seconds.1,
            "today_ranges": child.policy.schedule.ranges_for(now.weekday()),
            "blocked_today": blocked.iter().filter(|(c, _, _)| *c == child.id).map(|(_, _, n)| n).sum::<i64>(),
            "next_opening": next.map(|(day, time)| json!({ "day": day.to_string(), "time": time.format("%H:%M").to_string() })),
            "grants": grants.iter().filter(|g| g.child_id.as_deref() == Some(child.id.as_str())).collect::<Vec<_>>(),
            "blocked_services": child.policy.filter.blocked_services,
            "devices": mine,
        }));
    }

    let manifest = state.blocklists.manifest();
    Ok(Json(json!({
        "children": children,
        "devices": devices,
        "device_grants": grants.iter().filter(|g| g.device_id.is_some()).collect::<Vec<_>>(),
        // Requêtes bloquées aujourd'hui sur les appareils partagés, par appareil.
        "device_blocked_today": blocked
            .iter()
            .filter(|(child, _, _)| child.is_empty())
            .map(|(_, device, n)| (device.clone(), *n))
            .collect::<std::collections::BTreeMap<_, _>>(),
        "requests": pending_requests(&state).await?,
        "alerts": open_alerts(&state).await?,
        "blocklists": { "updated_at": manifest.updated_at, "categories": manifest.categories.len() },
        "now": util::now(),
    })))
}

pub async fn alerts(_: Parent, State(state): State<Shared>) -> ApiResult<Vec<Value>> {
    Ok(Json(open_alerts(&state).await?))
}

pub async fn ack(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    sqlx::query("UPDATE alerts SET acked_at = ? WHERE id = ? AND acked_at IS NULL")
        .bind(util::now())
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn ack_all(_: Parent, State(state): State<Shared>) -> ApiResult<Value> {
    sqlx::query("UPDATE alerts SET acked_at = ? WHERE acked_at IS NULL")
        .bind(util::now())
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "ok": true })))
}
