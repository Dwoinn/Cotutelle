//! Authentification : mots de passe, sessions par cookie, jetons d'appareil.

use crate::error::ApiError;
use crate::state::Shared;
use crate::util;
use anyhow::{Result, anyhow};
use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::extract::FromRequestParts;
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::request::Parts;

pub const PARENT_COOKIE: &str = "cotutelle_session";
pub const CHILD_COOKIE: &str = "cotutelle_enfant";
const PARENT_SESSION_DAYS: i64 = 30;
const CHILD_SESSION_HOURS: i64 = 12;

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow!("hachage du mot de passe : {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
}

fn cookie<'a>(parts: &'a Parts, name: &str) -> Option<&'a str> {
    parts
        .headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(k, v)| (k == name).then_some(v))
}

/// En-tête `Set-Cookie`. `max_age` nul efface le cookie.
pub fn set_cookie(state: &Shared, name: &str, value: &str, max_age: i64) -> String {
    let secure = if state.config.secure_cookies { "; Secure" } else { "" };
    format!("{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}")
}

pub async fn create_parent_session(state: &Shared, parent_id: &str) -> Result<String> {
    let token = util::new_token();
    let max_age = PARENT_SESSION_DAYS * 86_400;
    sqlx::query("INSERT INTO web_sessions (token_hash, parent_id, expires_at) VALUES (?, ?, ?)")
        .bind(util::hash_token(&token))
        .bind(parent_id)
        .bind(util::now() + max_age)
        .execute(&state.db)
        .await?;
    Ok(set_cookie(state, PARENT_COOKIE, &token, max_age))
}

pub async fn create_child_session(
    state: &Shared,
    child_id: &str,
    device_id: &str,
) -> Result<String> {
    let token = util::new_token();
    let max_age = CHILD_SESSION_HOURS * 3_600;
    sqlx::query("INSERT INTO web_sessions (token_hash, child_id, device_id, expires_at) VALUES (?, ?, ?, ?)")
        .bind(util::hash_token(&token))
        .bind(child_id)
        .bind(device_id)
        .bind(util::now() + max_age)
        .execute(&state.db)
        .await?;
    Ok(set_cookie(state, CHILD_COOKIE, &token, max_age))
}

pub async fn destroy_session(state: &Shared, parts: &Parts) -> Result<()> {
    if let Some(token) = cookie(parts, PARENT_COOKIE) {
        sqlx::query("DELETE FROM web_sessions WHERE token_hash = ?")
            .bind(util::hash_token(token))
            .execute(&state.db)
            .await?;
    }
    Ok(())
}

/// Parent connecté. Sa présence dans une signature protège la route.
#[derive(Debug, Clone)]
pub struct Parent {
    pub id: String,
    pub name: String,
}

impl FromRequestParts<Shared> for Parent {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, ApiError> {
        let token = cookie(parts, PARENT_COOKIE).ok_or_else(ApiError::unauthorized)?;
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT p.id, p.name FROM web_sessions s JOIN parents p ON p.id = s.parent_id
             WHERE s.token_hash = ? AND s.expires_at > ?",
        )
        .bind(util::hash_token(token))
        .bind(util::now())
        .fetch_optional(&state.db)
        .await?;
        let (id, name) = row.ok_or_else(ApiError::unauthorized)?;
        Ok(Parent { id, name })
    }
}

/// Enfant arrivé sur son espace par le lien de son appareil.
#[derive(Debug, Clone)]
pub struct ChildSession {
    pub child_id: String,
    pub device_id: Option<String>,
}

impl FromRequestParts<Shared> for ChildSession {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, ApiError> {
        let token = cookie(parts, CHILD_COOKIE).ok_or_else(ApiError::unauthorized)?;
        let row: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT child_id, device_id FROM web_sessions
             WHERE token_hash = ? AND child_id IS NOT NULL AND expires_at > ?",
        )
        .bind(util::hash_token(token))
        .bind(util::now())
        .fetch_optional(&state.db)
        .await?;
        let (child_id, device_id) = row.ok_or_else(ApiError::unauthorized)?;
        Ok(ChildSession { child_id, device_id })
    }
}

/// Parent connecté ou enfant dans son espace : pour ce que les deux voient.
#[derive(Debug, Clone, Copy)]
pub struct Viewer;

impl FromRequestParts<Shared> for Viewer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, ApiError> {
        if Parent::from_request_parts(parts, state).await.is_ok() {
            return Ok(Viewer);
        }
        ChildSession::from_request_parts(parts, state).await.map(|_| Viewer)
    }
}

/// Agent authentifié par le jeton permanent de son appareil.
#[derive(Debug, Clone)]
pub struct AgentDevice {
    pub id: String,
}

impl FromRequestParts<Shared> for AgentDevice {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, ApiError> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(ApiError::unauthorized)?;
        let row: Option<(String,)> =
            sqlx::query_as("SELECT id FROM devices WHERE token_hash = ? AND kind = 'agent'")
                .bind(util::hash_token(token))
                .fetch_optional(&state.db)
                .await?;
        row.map(|(id,)| AgentDevice { id }).ok_or_else(ApiError::unauthorized)
    }
}
