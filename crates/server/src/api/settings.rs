//! Catalogue des filtres et réglages du serveur.

use crate::auth::Parent;
use crate::error::{ApiError, ApiResult};
use crate::state::Shared;
use crate::{holidays, lan, notify, tasks};
use axum::Json;
use axum::extract::State;
use chrono::NaiveDate;
use cotutelle_common::{HolidayPeriod, catalog};
use serde::Deserialize;
use serde_json::{Value, json};

pub async fn catalog(_: Parent, State(state): State<Shared>) -> ApiResult<Value> {
    let services = super::services::listing(&state).await?;
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
        "services": services,
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

async fn holidays_json(state: &Shared, warning: Option<String>) -> Value {
    let settings = holidays::settings(state).await;
    let calendar = holidays::calendar_for(state, &settings).await;
    json!({
        "zone": settings.zone,
        "zones": holidays::ZONES,
        "fetched_at": (calendar.fetched_at > 0).then_some(calendar.fetched_at),
        "calendar": calendar.periods,
        "removed": settings.removed,
        "added": settings.added,
        "periods": holidays::effective(&settings, &calendar),
        "warning": warning,
    })
}

/// Vacances scolaires : zone, calendrier officiel, ajustements et périodes effectives.
pub async fn holidays(_: Parent, State(state): State<Shared>) -> ApiResult<Value> {
    Ok(Json(holidays_json(&state, None).await))
}

#[derive(Deserialize)]
pub struct HolidaysInput {
    zone: String,
    #[serde(default)]
    removed: Vec<NaiveDate>,
    #[serde(default)]
    added: Vec<HolidayPeriod>,
}

pub async fn update_holidays(
    _: Parent,
    State(state): State<Shared>,
    Json(body): Json<HolidaysInput>,
) -> ApiResult<Value> {
    let zone = body.zone.trim().to_string();
    if !zone.is_empty() && !holidays::ZONES.contains(&zone.as_str()) {
        return Err(ApiError::bad_request("zone inconnue"));
    }
    let mut added = Vec::new();
    for period in body.added {
        let label = super::clean_name(&period.label, "nom de la période")?;
        if period.end < period.start {
            return Err(ApiError::bad_request(format!("{label} : la fin précède le début")));
        }
        added.push(HolidayPeriod { label, ..period });
    }
    let settings =
        holidays::HolidaySettings { zone, removed: body.removed.into_iter().collect(), added };
    holidays::save_settings(&state, &settings).await?;
    // Une zone nouvellement choisie montre ses périodes sans attendre.
    let warning = match holidays::refresh(&state, false).await {
        Ok(_) => None,
        Err(e) => Some(format!(
            "Calendrier officiel indisponible pour le moment ({e:#}). Nouvel essai chaque heure."
        )),
    };
    state.changed().await;
    Ok(Json(holidays_json(&state, warning).await))
}
