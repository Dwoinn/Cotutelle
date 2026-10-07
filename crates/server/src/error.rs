//! Erreurs de l'API, rendues en JSON `{ "error": "…" }`.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "authentification requise")
    }

    pub fn not_found(what: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, format!("{what} introuvable"))
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

/// Toute erreur interne devient un 500 ; le détail reste dans les journaux.
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        let error = error.into();
        tracing::error!(error = ?error, "erreur interne");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "erreur interne du serveur")
    }
}

pub type ApiResult<T> = Result<Json<T>, ApiError>;
