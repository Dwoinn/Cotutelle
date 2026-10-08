//! Catalogue des services de la famille : ceux fournis avec Cotutelle, que
//! les parents peuvent ajuster, et ceux qu'ils ajoutent.

use crate::auth::{Parent, Viewer};
use crate::error::{ApiError, ApiResult};
use crate::model::{self, ServiceEntry};
use crate::state::Shared;
use crate::{logos, util};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::response::{IntoResponse, Response};
use cotutelle_common::{Service, catalog, clean_domain};
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::IpAddr;
use std::time::Duration;

const MAX_DOMAINS: usize = 60;
/// Attente du logo avant de répondre ; passé ce délai, il arrive en arrière-plan.
const LOGO_WAIT: Duration = Duration::from_secs(4);

fn entry_json(entry: &ServiceEntry, logo: Option<&logos::LogoRef>) -> Value {
    json!({
        "id": entry.service.id,
        "label": entry.service.label,
        "domains": entry.service.domains,
        "builtin": entry.builtin,
        "modified": entry.modified,
        "logo": logo,
    })
}

/// Le catalogue tel que l'interface des parents l'affiche, logos compris.
pub async fn listing(state: &Shared) -> Result<Vec<Value>, ApiError> {
    let logos = logos::index(state).await?;
    let entries = model::service_entries(&state.db).await?;
    Ok(entries.iter().map(|e| entry_json(e, logos.get(&e.service.id))).collect())
}

#[derive(Deserialize)]
pub struct ServiceInput {
    label: String,
    domains: Vec<String>,
}

/// Vérifie et normalise la liste des sites d'un service.
fn clean_domains(domains: &[String]) -> Result<Vec<String>, ApiError> {
    let mut out: Vec<String> = Vec::new();
    for entry in domains.iter().map(|d| d.trim()).filter(|d| !d.is_empty()) {
        let domain = clean_domain(entry)
            // Une adresse IP ne passe jamais par le DNS : rien à filtrer.
            .filter(|d| d.parse::<IpAddr>().is_err())
            .ok_or_else(|| {
                ApiError::bad_request(format!("« {entry} » n'est pas un nom de site"))
            })?;
        if !out.contains(&domain) {
            out.push(domain);
        }
    }
    if out.is_empty() {
        return Err(ApiError::bad_request("indiquez au moins un site"));
    }
    if out.len() > MAX_DOMAINS {
        return Err(ApiError::bad_request(format!("pas plus de {MAX_DOMAINS} sites par service")));
    }
    Ok(out)
}

/// Identifiant lisible tiré du nom : « Brawl Stars » → `brawl-stars`.
fn slug(label: &str) -> String {
    let mut out = String::new();
    for c in label.to_lowercase().chars() {
        let c = match c {
            'à' | 'â' | 'ä' => 'a',
            'ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() { "service".to_string() } else { out.to_string() }
}

fn check_label(
    entries: &[ServiceEntry],
    label: &str,
    own_id: Option<&str>,
) -> Result<String, ApiError> {
    let label = super::clean_name(label, "nom")?;
    let taken = entries.iter().any(|e| {
        Some(e.service.id.as_str()) != own_id
            && e.service.label.to_lowercase() == label.to_lowercase()
    });
    if taken {
        return Err(ApiError::conflict(format!("un service s'appelle déjà « {label} »")));
    }
    Ok(label)
}

async fn store(state: &Shared, service: &Service) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO services (id, label, domains, created_at) VALUES (?, ?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET label = excluded.label, domains = excluded.domains",
    )
    .bind(&service.id)
    .bind(&service.label)
    .bind(serde_json::to_string(&service.domains)?)
    .bind(util::now())
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Cherche le logo d'un service. Attend un court instant, pour que la réponse
/// le porte si le site répond vite ; sinon la recherche se termine seule.
async fn fetch_logo(state: &Shared, service: Service) {
    let state = state.clone();
    let search = tokio::spawn(async move { logos::refresh(&state, &service).await });
    let _ = tokio::time::timeout(LOGO_WAIT, search).await;
}

/// Oublie un logo qui ne correspond plus au site principal du service.
async fn drop_stale_logo(state: &Shared, service: &Service) -> Result<bool, ApiError> {
    let done = sqlx::query("DELETE FROM service_logos WHERE service_id = ? AND domain != ?")
        .bind(&service.id)
        .bind(service.domains.first())
        .execute(&state.db)
        .await?;
    Ok(done.rows_affected() > 0)
}

async fn respond(state: &Shared, id: &str) -> ApiResult<Value> {
    let listing = listing(state).await?;
    listing
        .into_iter()
        .find(|s| s["id"] == id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("service"))
}

pub async fn create(
    _: Parent,
    State(state): State<Shared>,
    Json(body): Json<ServiceInput>,
) -> ApiResult<Value> {
    let entries = model::service_entries(&state.db).await?;
    let label = check_label(&entries, &body.label, None)?;
    let domains = clean_domains(&body.domains)?;

    let base = slug(&label);
    let taken = |id: &str| entries.iter().any(|e| e.service.id == id);
    let id = std::iter::once(base.clone())
        .chain((2..).map(|n| format!("{base}-{n}")))
        .find(|id| !taken(id))
        .expect("suite infinie");

    let service = Service { id, label, domains };
    store(&state, &service).await?;
    state.changed().await;
    fetch_logo(&state, service.clone()).await;
    respond(&state, &service.id).await
}

pub async fn update(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<ServiceInput>,
) -> ApiResult<Value> {
    let entries = model::service_entries(&state.db).await?;
    if !entries.iter().any(|e| e.service.id == id) {
        return Err(ApiError::not_found("service"));
    }
    let service = Service {
        label: check_label(&entries, &body.label, Some(&id))?,
        domains: clean_domains(&body.domains)?,
        id,
    };

    // Un service fourni remis à l'identique n'a plus besoin de ligne propre :
    // il suivra de nouveau les mises à jour du catalogue.
    if catalog::builtin_services().contains(&service) {
        sqlx::query("DELETE FROM services WHERE id = ?")
            .bind(&service.id)
            .execute(&state.db)
            .await?;
    } else {
        store(&state, &service).await?;
    }
    state.changed().await;
    if drop_stale_logo(&state, &service).await? {
        fetch_logo(&state, service.clone()).await;
    }
    respond(&state, &service.id).await
}

/// Supprime un service ajouté par les parents, ou rend à un service fourni
/// son nom et ses sites d'origine.
pub async fn remove(
    _: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    let entries = model::service_entries(&state.db).await?;
    let entry = entries
        .iter()
        .find(|e| e.service.id == id)
        .ok_or_else(|| ApiError::not_found("service"))?;
    if entry.builtin && !entry.modified {
        return Err(ApiError::bad_request(
            "ce service est fourni avec Cotutelle : il se modifie, mais ne se supprime pas",
        ));
    }

    sqlx::query("DELETE FROM services WHERE id = ?").bind(&id).execute(&state.db).await?;
    let restored = catalog::builtin_services().into_iter().find(|s| s.id == id);
    match &restored {
        Some(service) => {
            if drop_stale_logo(&state, service).await? {
                fetch_logo(&state, service.clone()).await;
            }
        }
        None => model::forget_service(&state.db, &id).await?,
    }
    state.changed().await;
    Ok(Json(json!({ "ok": true, "restored": restored.is_some() })))
}

/// Logo d'un service, pour les parents comme pour l'enfant dans son espace.
pub async fn logo(
    _: Viewer,
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let Some((content_type, image)) = logos::image(&state, &id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let headers = [
        (CONTENT_TYPE, content_type),
        // L'adresse porte la date du logo : elle change quand il change.
        (CACHE_CONTROL, "private, max-age=31536000, immutable".to_string()),
        (X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        // L'image vient d'un site tiers : ouverte seule, elle n'exécute rien.
        (
            CONTENT_SECURITY_POLICY,
            "default-src 'none'; style-src 'unsafe-inline'; sandbox".to_string(),
        ),
    ];
    Ok((headers, image).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_readable() {
        assert_eq!(slug("Brawl Stars"), "brawl-stars");
        assert_eq!(slug("  Pokémon GO ! "), "pokemon-go");
        assert_eq!(slug("Disney+"), "disney");
        assert_eq!(slug("★"), "service");
    }

    #[test]
    fn domains_are_cleaned_and_deduplicated() {
        let input = |list: &[&str]| list.iter().map(|d| d.to_string()).collect::<Vec<_>>();
        let cleaned = clean_domains(&input(&[
            "https://www.Brawlstars.com/fr",
            "",
            "supercell.com",
            "brawlstars.com",
        ]));
        assert_eq!(cleaned.ok(), Some(input(&["brawlstars.com", "supercell.com"])));
        assert!(clean_domains(&input(&["brawl stars"])).is_err());
        assert!(clean_domains(&input(&["192.168.1.10"])).is_err());
        assert!(clean_domains(&input(&[" "])).is_err());
    }
}
