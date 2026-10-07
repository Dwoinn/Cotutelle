//! Installation initiale, connexion et comptes parents.

use crate::auth::{self, Parent};
use crate::error::{ApiError, ApiResult};
use crate::state::Shared;
use crate::util;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

const MIN_PASSWORD: usize = 8;

#[derive(Deserialize)]
pub struct Credentials {
    name: String,
    password: String,
}

fn check_password(password: &str) -> Result<(), ApiError> {
    if password.chars().count() < MIN_PASSWORD {
        return Err(ApiError::bad_request(format!(
            "mot de passe : {MIN_PASSWORD} caractères minimum"
        )));
    }
    Ok(())
}

async fn parent_count(state: &Shared) -> Result<i64, ApiError> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM parents").fetch_one(&state.db).await?;
    Ok(count)
}

async fn insert_parent(state: &Shared, name: &str, password: &str) -> Result<String, ApiError> {
    let name = super::clean_name(name, "nom")?;
    check_password(password)?;
    let id = util::new_id();
    let hash = auth::hash_password(password)?;
    let inserted = sqlx::query(
        "INSERT INTO parents (id, name, password_hash, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(hash)
    .bind(util::now())
    .execute(&state.db)
    .await;
    match inserted {
        Ok(_) => Ok(id),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            Err(ApiError::conflict("un parent porte déjà ce nom"))
        }
        Err(e) => Err(e.into()),
    }
}

pub async fn status(State(state): State<Shared>) -> ApiResult<Value> {
    Ok(Json(json!({
        "name": "cotutelle-server",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": cotutelle_common::PROTOCOL_VERSION,
        "setup_done": parent_count(&state).await? > 0,
    })))
}

/// Crée le premier compte parent. Refusé dès qu'un parent existe.
pub async fn setup(
    State(state): State<Shared>,
    Json(body): Json<Credentials>,
) -> Result<Response, ApiError> {
    if parent_count(&state).await? > 0 {
        return Err(ApiError::conflict("l'installation est déjà faite"));
    }
    let id = insert_parent(&state, &body.name, &body.password).await?;
    let cookie = auth::create_parent_session(&state, &id).await?;
    Ok(([(SET_COOKIE, cookie)], Json(json!({ "id": id }))).into_response())
}

pub async fn login(
    State(state): State<Shared>,
    Json(body): Json<Credentials>,
) -> Result<Response, ApiError> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT id, password_hash FROM parents WHERE name = ?")
            .bind(body.name.trim())
            .fetch_optional(&state.db)
            .await?;
    // Même coût et même réponse que le nom existe ou non.
    let (id, hash) = row.unzip();
    let valid = auth::verify_password(&body.password, hash.as_deref().unwrap_or(DUMMY_HASH));
    let Some(id) = id.filter(|_| valid) else {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "nom ou mot de passe incorrect"));
    };
    let cookie = auth::create_parent_session(&state, &id).await?;
    Ok(([(SET_COOKIE, cookie)], Json(json!({ "id": id }))).into_response())
}

/// Empreinte factice vérifiée quand le nom est inconnu.
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$Qn5cWZ8Sg1Y4k5U0m0y0t3mY2Vf8Y3b3e3x5p8yq3fI";

pub async fn logout(State(state): State<Shared>, parts: Parts) -> Result<Response, ApiError> {
    auth::destroy_session(&state, &parts).await?;
    let cookie = auth::set_cookie(&state, auth::PARENT_COOKIE, "", 0);
    Ok(([(SET_COOKIE, cookie)], Json(json!({ "ok": true }))).into_response())
}

pub async fn me(parent: Parent) -> ApiResult<Value> {
    Ok(Json(json!({ "id": parent.id, "name": parent.name })))
}

pub async fn list(_: Parent, State(state): State<Shared>) -> ApiResult<Value> {
    let rows: Vec<(String, String, i64)> =
        sqlx::query_as("SELECT id, name, created_at FROM parents ORDER BY created_at")
            .fetch_all(&state.db)
            .await?;
    let parents: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, created_at)| json!({ "id": id, "name": name, "created_at": created_at }))
        .collect();
    Ok(Json(json!(parents)))
}

pub async fn create(
    _: Parent,
    State(state): State<Shared>,
    Json(body): Json<Credentials>,
) -> ApiResult<Value> {
    let id = insert_parent(&state, &body.name, &body.password).await?;
    Ok(Json(json!({ "id": id })))
}

pub async fn remove(
    parent: Parent,
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Value> {
    if id == parent.id {
        return Err(ApiError::bad_request("impossible de supprimer son propre compte"));
    }
    let done = sqlx::query("DELETE FROM parents WHERE id = ?").bind(&id).execute(&state.db).await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::not_found("parent"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct PasswordChange {
    current: String,
    new: String,
}

pub async fn change_password(
    parent: Parent,
    State(state): State<Shared>,
    Json(body): Json<PasswordChange>,
) -> ApiResult<Value> {
    check_password(&body.new)?;
    let (hash,): (String,) = sqlx::query_as("SELECT password_hash FROM parents WHERE id = ?")
        .bind(&parent.id)
        .fetch_one(&state.db)
        .await?;
    if !auth::verify_password(&body.current, &hash) {
        return Err(ApiError::bad_request("mot de passe actuel incorrect"));
    }
    sqlx::query("UPDATE parents SET password_hash = ? WHERE id = ?")
        .bind(auth::hash_password(&body.new)?)
        .bind(&parent.id)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "ok": true })))
}
