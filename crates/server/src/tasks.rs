//! Tâches de fond : statistiques, purge, mise à jour des listes, veille.

use crate::state::Shared;
use crate::{lan, logos, model, notify, util};
use anyhow::Result;
use std::time::Duration;

const DAY: i64 = 86_400;
const SILENT_AFTER_DAYS: i64 = 3;
const DEFAULT_RETENTION_DAYS: i64 = 30;

pub fn spawn(state: Shared) {
    let s = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            if let Err(e) = lan::flush_stats(&s).await {
                tracing::error!(error = ?e, "écriture des statistiques DNS");
            }
        }
    });

    let s = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            if let Err(e) = housekeeping(&s).await {
                tracing::error!(error = ?e, "entretien de la base");
            }
            // Dès le démarrage, puis pour retenter les recherches infructueuses.
            if let Err(e) = fetch_logos(&s).await {
                tracing::error!(error = ?e, "récupération des logos des services");
            }
        }
    });

    tokio::spawn(async move {
        loop {
            let age = util::now() - state.blocklists.manifest().updated_at;
            if age >= DAY && !state.config.blocklist_source.is_empty() {
                refresh_blocklists(&state).await;
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

pub async fn refresh_blocklists(state: &Shared) {
    tracing::info!(source = %state.config.blocklist_source, "mise à jour des listes de blocage");
    match state.blocklists.update(&state.config.blocklist_source).await {
        Ok(count) => {
            tracing::info!(count, "listes de blocage à jour");
            state.changed().await;
        }
        Err(e) => tracing::error!(error = ?e, "mise à jour des listes impossible"),
    }
}

async fn fetch_logos(state: &Shared) -> Result<()> {
    let services = model::services(&state.db).await?;
    logos::ensure(state, &services).await
}

async fn housekeeping(state: &Shared) -> Result<()> {
    let now = util::now();
    let retention = notify::setting(state, "retention_days")
        .await
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|d| (1..=365).contains(d))
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    let oldest_day = util::day_str(util::today() - chrono::Duration::days(retention));

    for table in ["web_sessions", "enroll_tokens", "child_links"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE expires_at < ?"))
            .bind(now)
            .execute(&state.db)
            .await?;
    }
    sqlx::query("DELETE FROM grants WHERE expires_at < ?")
        .bind(now - 7 * DAY)
        .execute(&state.db)
        .await?;
    sqlx::query("DELETE FROM requests WHERE status != 'pending' AND created_at < ?")
        .bind(now - retention * DAY)
        .execute(&state.db)
        .await?;
    sqlx::query("DELETE FROM alerts WHERE acked_at IS NOT NULL AND created_at < ?")
        .bind(now - retention * DAY)
        .execute(&state.db)
        .await?;
    for table in ["dns_daily", "usage"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE day < ?"))
            .bind(&oldest_day)
            .execute(&state.db)
            .await?;
    }

    // Un agent muet depuis plusieurs jours : une alerte, pas une par heure.
    let silent: Vec<(String, String)> = sqlx::query_as(
        "SELECT d.id, d.name FROM devices d
         WHERE d.kind = 'agent' AND d.last_seen IS NOT NULL AND d.last_seen < ?
           AND NOT EXISTS (SELECT 1 FROM alerts a
                           WHERE a.device_id = d.id AND a.kind = 'silent' AND a.created_at > d.last_seen)",
    )
    .bind(now - SILENT_AFTER_DAYS * DAY)
    .fetch_all(&state.db)
    .await?;
    for (id, name) in silent {
        let message =
            format!("{name} n'a pas contacté le serveur depuis plus de {SILENT_AFTER_DAYS} jours");
        notify::alert(state, "silent", Some(&id), None, &message).await?;
    }
    Ok(())
}
