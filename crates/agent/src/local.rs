//! Petit serveur HTTP local (boucle locale uniquement) : ouvre l'espace
//! enfant du compte devant l'écran et expose l'état de l'agent.

use crate::runtime::Runtime;
use crate::sync::api_url;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Local;
use cotutelle_common::protocol::{ChildLinkRequest, ChildLinkResponse};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html><html lang=\"fr\"><meta charset=\"utf-8\"><title>Cotutelle</title>\
         <body style=\"font-family:system-ui;max-width:32rem;margin:15vh auto;padding:0 1rem;text-align:center\">\
         <h1>{title}</h1><p>{body}</p></body></html>"
    ))
}

/// Redirige le compte au premier plan vers son espace sur le serveur.
async fn open(State(rt): State<Arc<Runtime>>) -> Response {
    let Some(os_account) = rt.foreground().filter(|a| rt.account(a).is_some()) else {
        return page(
            "Cet ordinateur est protégé par Cotutelle",
            "Ce compte n'est soumis à aucune règle.",
        )
        .into_response();
    };
    let link = async {
        rt.http
            .post(api_url(&rt.identity.server, "/agent/child-link"))
            .bearer_auth(&rt.identity.token)
            .json(&ChildLinkRequest { os_account })
            .send()
            .await?
            .error_for_status()?
            .json::<ChildLinkResponse>()
            .await
    };
    match link.await {
        Ok(link) => {
            Redirect::to(&format!("{}{}", rt.identity.server.trim_end_matches('/'), link.url))
                .into_response()
        }
        Err(e) => {
            tracing::warn!(error = %e, "lien vers l'espace enfant");
            (
                StatusCode::BAD_GATEWAY,
                page(
                    "Serveur injoignable",
                    "Tes règles habituelles restent appliquées. Réessaie plus tard.",
                ),
            )
                .into_response()
        }
    }
}

async fn status(State(rt): State<Arc<Runtime>>) -> Json<Value> {
    let now = Local::now();
    let accounts: Vec<Value> = rt
        .managed_accounts()
        .iter()
        .map(|account| {
            let local = rt
                .usage
                .lock()
                .expect("verrou temps")
                .seconds(&account.os_account, now.date_naive());
            json!({
                "os_account": account.os_account,
                "child": account.child_name,
                "access": rt.access(account, now),
                "local_seconds_today": local,
                "grants": account.grants.len(),
            })
        })
        .collect();
    let state = rt.state.read().expect("verrou état");
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "server": rt.identity.server,
        "connected": rt.is_connected(),
        "policy_issued_at": state.as_ref().map(|s| s.issued_at),
        "foreground": rt.foreground(),
        "accounts": accounts,
        "blocklists": rt.loaded_lists.lock().expect("verrou listes").iter().map(|(c, _)| c.clone()).collect::<Vec<_>>(),
        "dry_run": rt.dry_run,
    }))
}

pub async fn serve(rt: Arc<Runtime>, addr: SocketAddr) -> anyhow::Result<()> {
    anyhow::ensure!(addr.ip().is_loopback(), "le serveur local n'écoute que sur la boucle locale");
    let app = Router::new().route("/", get(open)).route("/status", get(status)).with_state(rt);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
