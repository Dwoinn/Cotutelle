//! Alertes et notifications aux parents.
//!
//! Chaque alerte est enregistrée et visible dans l'interface. Si un sujet
//! ntfy est configuré, elle est aussi poussée sur le téléphone des parents.

use crate::state::Shared;
use crate::util;
use anyhow::Result;

pub async fn setting(state: &Shared, key: &str) -> Option<String> {
    sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .map(|(v,)| v)
        .filter(|v| !v.trim().is_empty())
}

/// Envoie une notification, sans jamais faire échouer l'appelant.
pub async fn push(state: &Shared, title: &str, body: &str) {
    let Some(url) = setting(state, "ntfy_url").await else { return };
    let request = state
        .http
        .post(url.trim())
        .header("Title", title_header(title))
        .header("Tags", "family")
        .body(body.to_string())
        .timeout(std::time::Duration::from_secs(10));
    if let Err(e) = request.send().await.and_then(reqwest::Response::error_for_status) {
        tracing::warn!(error = %e, "notification ntfy non envoyée");
    }
}

/// Les en-têtes HTTP sont en ASCII : ntfy accepte l'UTF-8 encodé RFC 2047.
fn title_header(title: &str) -> String {
    if title.is_ascii() {
        return title.to_string();
    }
    let hex: String = title.bytes().map(|b| format!("={b:02X}")).collect();
    format!("=?UTF-8?Q?{hex}?=")
}

pub async fn alert(
    state: &Shared,
    kind: &str,
    device_id: Option<&str>,
    child_id: Option<&str>,
    message: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO alerts (id, kind, device_id, child_id, message, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(util::new_id())
    .bind(kind)
    .bind(device_id)
    .bind(child_id)
    .bind(message)
    .bind(util::now())
    .execute(&state.db)
    .await?;
    push(state, "Cotutelle : alerte", message).await;
    Ok(())
}
