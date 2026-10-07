//! Catalogue des filtres et réglages du serveur.

use crate::auth::Parent;
use crate::error::{ApiError, ApiResult};
use crate::state::Shared;
use crate::{lan, notify, tasks};
use axum::Json;
use axum::extract::State;
use cotutelle_common::catalog;
use serde::Deserialize;
use serde_json::{Value, json};

pub async fn catalog(_: Parent, State(state): State<Shared>) -> ApiResult<Value> {
    let manifest = state.blocklists.manifest();
    let groups: Vec<Value> = catalog::CATEGORY_GROUPS
        .iter()
        .map(|(group, label)| {
            let categories: Vec<Value> = catalog::CATEGORIES
                .iter()
                .filter(|c| c.group == *group)
                .map(|c| {
                    json!({
                        "id": c.id,
                        "label": c.label,
                        "description": c.description,
                        "recommended": c.recommended,
                        "entries": manifest.categories.get(c.id).map(|e| e.entries),
                    })
                })
                .collect();
            json!({ "id": group, "label": label, "categories": categories })
        })
        .collect();
    Ok(Json(json!({
        "groups": groups,
        "services": catalog::SERVICES,
        "blocklists_ready": !manifest.categories.is_empty(),
    })))
}

pub async fn get(_: Parent, State(state): State<Shared>) -> ApiResult<Value> {
    let manifest = state.blocklists.manifest();
    let entries: u64 = manifest.categories.values().map(|e| e.entries).sum();
    Ok(Json(json!({
        "ntfy_url": notify::setting(&state, "ntfy_url").await.unwrap_or_default(),
        "upstream_dns": notify::setting(&state, "upstream_dns").await.unwrap_or_else(|| lan::DEFAULT_UPSTREAMS.to_string()),
        "retention_days": notify::setting(&state, "retention_days").await.and_then(|v| v.parse::<i64>().ok()).unwrap_or(30),
        "blocklists": {
            "updated_at": manifest.updated_at,
            "source": state.config.blocklist_source,
            "categories": manifest.categories.len(),
            "entries": entries,
        },
        "version": env!("CARGO_PKG_VERSION"),
    })))
}

#[derive(Deserialize)]
pub struct SettingsInput {
    ntfy_url: Option<String>,
    upstream_dns: Option<String>,
    retention_days: Option<i64>,
}

async fn save(state: &Shared, key: &str, value: &str) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(&state.db)
        .await?;
    Ok(())
}

pub async fn update(
    _: Parent,
    State(state): State<Shared>,
    Json(body): Json<SettingsInput>,
) -> ApiResult<Value> {
    if let Some(url) = &body.ntfy_url {
        let url = url.trim();
        if !url.is_empty() && !url.starts_with("https://") && !url.starts_with("http://") {
            return Err(ApiError::bad_request(
                "l'adresse ntfy doit commencer par http:// ou https://",
            ));
        }
        save(&state, "ntfy_url", url).await?;
    }
    if let Some(upstreams) = &body.upstream_dns {
        if lan::parse_upstreams(upstreams).is_empty() {
            return Err(ApiError::bad_request(
                "aucun résolveur DNS valide (format : 9.9.9.9 ou 9.9.9.9:53)",
            ));
        }
        save(&state, "upstream_dns", upstreams.trim()).await?;
    }
    if let Some(days) = body.retention_days {
        if !(1..=365).contains(&days) {
            return Err(ApiError::bad_request("durée de conservation : entre 1 et 365 jours"));
        }
        save(&state, "retention_days", &days.to_string()).await?;
    }
    state.changed().await;
    Ok(Json(json!({ "ok": true })))
}

/// Lance en arrière-plan le téléchargement des listes de blocage.
pub async fn refresh_blocklists(_: Parent, State(state): State<Shared>) -> ApiResult<Value> {
    if state.config.blocklist_source.is_empty() {
        return Err(ApiError::bad_request("aucune source de listes configurée"));
    }
    tokio::spawn(async move { tasks::refresh_blocklists(&state).await });
    Ok(Json(json!({ "started": true })))
}
