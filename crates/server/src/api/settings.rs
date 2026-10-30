//! Catalogue des filtres et réglages du serveur.

use crate::auth::Parent;
use crate::error::{ApiError, ApiResult};
use crate::state::Shared;
use crate::{holidays, lan, notify, tasks};
use axum::Json;
use axum::extract::State;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
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

/// Réglages ntfy tels que l'interface les montre : jamais le secret ni le
/// sujet des réponses.
async fn ntfy_json(state: &Shared) -> Value {
    let enabled = notify::enabled(state).await;
    match notify::stored(state).await {
        Some(ntfy) => json!({
            "enabled": enabled,
            "server": ntfy.server,
            "topic": ntfy.topic,
            "subscribe_url": ntfy.subscribe_url(),
            "app_link": ntfy.app_link(),
        }),
        None => {
            json!({ "enabled": false, "server": notify::DEFAULT_SERVER, "topic": null, "subscribe_url": null, "app_link": null })
        }
    }
}

pub async fn get(_: Parent, State(state): State<Shared>) -> ApiResult<Value> {
    let manifest = state.blocklists.manifest();
    let entries: u64 = manifest.categories.values().map(|e| e.entries).sum();
    Ok(Json(json!({
        "ntfy": ntfy_json(&state).await,
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
    ntfy_enabled: Option<bool>,
    /// Serveur ntfy, pour qui en héberge un ; vide pour ntfy.sh.
    ntfy_server: Option<String>,
    upstream_dns: Option<String>,
    retention_days: Option<i64>,
}

async fn save(state: &Shared, key: &str, value: &str) -> Result<(), ApiError> {
    notify::save_setting(state, key, value).await?;
    Ok(())
}

pub async fn update(
    _: Parent,
    State(state): State<Shared>,
    Json(body): Json<SettingsInput>,
) -> ApiResult<Value> {
    if let Some(server) = &body.ntfy_server {
        let server = notify::clean_server(server).map_err(ApiError::bad_request)?;
        notify::set_server(&state, &server).await?;
    }
    match body.ntfy_enabled {
        Some(true) => {
            notify::enable(&state).await?;
        }
        Some(false) => notify::disable(&state).await?,
        None => {}
    }
    if let Some(upstreams) = &body.upstream_dns {
        if lan::parse_upstreams(upstreams).is_empty() {
            return Err(ApiError::bad_request(
                "aucun résolveur DNS valide (format : 86.54.11.1 ou 86.54.11.1:53)",
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

/// Envoie une notification de test sur le téléphone des parents.
pub async fn ntfy_test(_: Parent, State(state): State<Shared>) -> ApiResult<Value> {
    notify::test(&state).await.map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(Json(json!({ "ok": true })))
}

/// Nouveaux sujets : à refaire suivre sur chaque téléphone.
pub async fn ntfy_reset(_: Parent, State(state): State<Shared>) -> ApiResult<Value> {
    notify::reset(&state).await?;
    Ok(Json(ntfy_json(&state).await))
}

/// QR du lien d'abonnement, à scanner avec un téléphone Android.
pub async fn ntfy_qr(_: Parent, State(state): State<Shared>) -> Result<Response, ApiError> {
    let ntfy = notify::stored(&state).await.ok_or_else(|| ApiError::not_found("réglage ntfy"))?;
    let svg = notify::qr_svg(&ntfy)?;
    Ok(([(CONTENT_TYPE, "image/svg+xml"), (CACHE_CONTROL, "no-store")], svg).into_response())
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
