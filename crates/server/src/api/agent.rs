//! Routes utilisées par les agents installés sur les appareils.

use crate::auth::AgentDevice;
use crate::error::{ApiError, ApiResult};
use crate::state::Shared;
use crate::{hub, util};
use axum::Json;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use cotutelle_common::DeviceId;
use cotutelle_common::protocol::{
    ChildLinkRequest, ChildLinkResponse, EnrollRequest, EnrollResponse,
};
use std::net::SocketAddr;

const CHILD_LINK_SECONDS: i64 = 60;

/// Échange le code d'enrôlement contre le jeton permanent de l'appareil.
pub async fn enroll(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<EnrollRequest>,
) -> ApiResult<EnrollResponse> {
    // Freine les essais de codes au hasard.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let row: Option<(String,)> = sqlx::query_as(
        "DELETE FROM enroll_tokens WHERE token_hash = ? AND expires_at > ? RETURNING device_name",
    )
    .bind(util::hash_token(&util::normalize_code(&body.token)))
    .bind(util::now())
    .fetch_optional(&state.db)
    .await?;
    let (name,) = row.ok_or_else(|| {
        ApiError::new(StatusCode::FORBIDDEN, "code d'enrôlement invalide ou expiré")
    })?;

    let device = DeviceId::new();
    let token = util::new_token();
    sqlx::query(
        "INSERT INTO devices (id, name, kind, token_hash, hostname, os, agent_version, ip, os_accounts, last_seen, created_at)
         VALUES (?, ?, 'agent', ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(device.to_string())
    .bind(name)
    .bind(util::hash_token(&token))
    .bind(&body.hostname)
    .bind(&body.os)
    .bind(&body.agent_version)
    .bind(peer.ip().to_string())
    .bind(serde_json::to_string(&body.os_accounts)?)
    .bind(util::now())
    .bind(util::now())
    .execute(&state.db)
    .await?;
    tracing::info!(device = %device, hostname = %body.hostname, "appareil enrôlé");
    Ok(Json(EnrollResponse { device, device_token: token }))
}

pub async fn ws(
    device: AgentDevice,
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    upgrade: WebSocketUpgrade,
) -> Response {
    upgrade
        .on_upgrade(move |socket| hub::serve_agent(state, device.id, socket, peer.ip().to_string()))
}

/// Sert une liste de blocage au format binaire de `DomainSet`.
pub async fn blocklist(
    _: AgentDevice,
    State(state): State<Shared>,
    Path(category): Path<String>,
) -> Result<Response, ApiError> {
    let path = state.blocklists.path(&category).ok_or_else(|| ApiError::not_found("liste"))?;
    let bytes = tokio::fs::read(path).await.map_err(|_| ApiError::not_found("liste"))?;
    Ok(([(CONTENT_TYPE, "application/octet-stream")], bytes).into_response())
}

/// Fabrique le lien à usage unique qui ouvre l'espace enfant depuis l'appareil.
pub async fn child_link(
    device: AgentDevice,
    State(state): State<Shared>,
    Json(body): Json<ChildLinkRequest>,
) -> ApiResult<ChildLinkResponse> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT child_id FROM device_accounts WHERE device_id = ? AND os_account = ?",
    )
    .bind(&device.id)
    .bind(&body.os_account)
    .fetch_optional(&state.db)
    .await?;
    let (child_id,) = row.ok_or_else(|| ApiError::not_found("compte géré"))?;
    let token = util::new_token();
    sqlx::query(
        "INSERT INTO child_links (token_hash, child_id, device_id, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(util::hash_token(&token))
    .bind(child_id)
    .bind(&device.id)
    .bind(util::now() + CHILD_LINK_SECONDS)
    .execute(&state.db)
    .await?;
    // Chemin relatif : l'agent le complète avec l'adresse du serveur qu'il connaît.
    Ok(Json(ChildLinkResponse { url: format!("/api/v1/child/enter?token={token}") }))
}
