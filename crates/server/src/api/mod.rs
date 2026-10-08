//! Routes HTTP de l'API (`/api/v1`).

mod agent;
mod child;
mod children;
mod devices;
mod grants;
mod overview;
mod parents;
mod services;
mod settings;

use crate::error::ApiError;
use crate::state::Shared;
use axum::Router;
use axum::routing::{delete, get, post, put};
use cotutelle_common::catalog;
use cotutelle_common::{GrantTarget, Service};

pub fn router() -> Router<Shared> {
    Router::new()
        // Installation et authentification
        .route("/status", get(parents::status))
        .route("/setup", post(parents::setup))
        .route("/auth/login", post(parents::login))
        .route("/auth/logout", post(parents::logout))
        .route("/auth/me", get(parents::me))
        .route("/parents", get(parents::list).post(parents::create))
        .route("/parents/{id}", delete(parents::remove))
        .route("/parents/me/password", put(parents::change_password))
        // Enfants
        .route("/children", get(children::list).post(children::create))
        .route("/children/{id}", get(children::get).put(children::update).delete(children::remove))
        .route("/children/{id}/activity", get(children::activity))
        .route("/children/{id}/space", get(child::preview))
        // Appareils
        .route("/devices", get(devices::list).post(devices::create_network))
        .route("/devices/enroll-code", post(devices::enroll_code))
        .route("/devices/{id}", put(devices::update).delete(devices::remove))
        .route("/devices/{id}/accounts", put(devices::set_accounts))
        .route("/devices/{id}/activity", get(devices::activity))
        // Exceptions et demandes
        .route("/grants", get(grants::list).post(grants::create))
        .route("/grants/{id}", delete(grants::revoke))
        .route("/requests", get(grants::requests))
        .route("/requests/{id}/approve", post(grants::approve))
        .route("/requests/{id}/deny", post(grants::deny))
        // Vue d'ensemble
        .route("/dashboard", get(overview::dashboard))
        .route("/alerts", get(overview::alerts))
        .route("/alerts/ack-all", post(overview::ack_all))
        .route("/alerts/{id}/ack", post(overview::ack))
        .route("/catalog", get(settings::catalog))
        .route("/services", post(services::create))
        .route("/services/{id}", put(services::update).delete(services::remove))
        .route("/services/{id}/logo", get(services::logo))
        .route("/settings", get(settings::get).put(settings::update))
        .route("/blocklists/refresh", post(settings::refresh_blocklists))
        // Espace enfant
        .route("/child/enter", get(child::enter))
        .route("/child/me", get(child::me))
        .route("/child/requests", post(child::request))
        // Agents
        .route("/agent/enroll", post(agent::enroll))
        .route("/agent/ws", get(agent::ws))
        .route("/agent/blocklists/{category}", get(agent::blocklist))
        .route("/agent/child-link", post(agent::child_link))
}

/// Valide un nom saisi par un parent.
fn clean_name(name: &str, what: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err(ApiError::bad_request(format!("{what} : entre 1 et 40 caractères")));
    }
    Ok(name.to_string())
}

fn minutes_label(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// Décrit une cible d'exception en français, pour les notifications.
fn describe_target(services: &[Service], target: &GrantTarget) -> String {
    match target {
        GrantTarget::Service { service } => services
            .iter()
            .find(|s| s.id == *service)
            .map_or_else(|| service.clone(), |s| s.label.clone()),
        GrantTarget::Category { category } => catalog::CATEGORIES
            .iter()
            .find(|c| c.id == category)
            .map_or_else(|| category.clone(), |c| c.label.to_string()),
        GrantTarget::Domain { domain } => domain.clone(),
        GrantTarget::ExtraMinutes { minutes } => format!("{} de plus", minutes_label(*minutes)),
        GrantTarget::IgnoreSchedule => "hors horaires".to_string(),
        GrantTarget::Pause => "pause".to_string(),
    }
}
