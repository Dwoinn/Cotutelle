//! Lecture des entités en base et règles métier partagées par les routes.

use crate::util;
use anyhow::Result;
use chrono::{Datelike, Local, NaiveTime};
use cotutelle_common::catalog;
use cotutelle_common::schedule::{TimeRange, WeeklySchedule};
use cotutelle_common::{
    AccessStatus, FilterPolicy, GrantId, GrantTarget, Policy, Quota, TemporaryGrant, Usage,
    evaluate_access,
};
use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize)]
pub struct Child {
    pub id: String,
    pub name: String,
    pub birth_year: Option<i64>,
    pub emoji: String,
    pub color: String,
    pub policy: Policy,
}

type ChildRow = (String, String, Option<i64>, String, String, String);

fn child_from_row((id, name, birth_year, emoji, color, policy): ChildRow) -> Child {
    // Une politique illisible retombe sur la politique vide, qui bloque tout (D3).
    let policy = serde_json::from_str(&policy).unwrap_or_default();
    Child { id, name, birth_year, emoji, color, policy }
}

const CHILD_COLUMNS: &str = "id, name, birth_year, emoji, color, policy";

pub async fn children(db: &SqlitePool) -> Result<Vec<Child>> {
    let rows: Vec<ChildRow> = sqlx::query_as(&format!(
        "SELECT {CHILD_COLUMNS} FROM children ORDER BY birth_year IS NULL, birth_year, name"
    ))
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(child_from_row).collect())
}

pub async fn child(db: &SqlitePool, id: &str) -> Result<Option<Child>> {
    let row: Option<ChildRow> =
        sqlx::query_as(&format!("SELECT {CHILD_COLUMNS} FROM children WHERE id = ?"))
            .bind(id)
            .fetch_optional(db)
            .await?;
    Ok(row.map(child_from_row))
}

#[derive(Debug, Clone, Serialize)]
pub struct Grant {
    pub id: String,
    pub child_id: Option<String>,
    pub device_id: Option<String>,
    pub target: GrantTarget,
    pub starts_at: i64,
    pub expires_at: i64,
    pub granted_by: Option<String>,
}

impl Grant {
    pub fn to_protocol(&self) -> Option<TemporaryGrant> {
        Some(TemporaryGrant {
            id: GrantId(self.id.parse().ok()?),
            target: self.target.clone(),
            starts_at: util::from_ts(self.starts_at),
            expires_at: util::from_ts(self.expires_at),
        })
    }
}

/// Exceptions non expirées, de la plus proche de sa fin à la plus lointaine.
pub async fn live_grants(db: &SqlitePool) -> Result<Vec<Grant>> {
    type Row = (String, Option<String>, Option<String>, String, i64, i64, Option<String>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT g.id, g.child_id, g.device_id, g.target, g.starts_at, g.expires_at, p.name
         FROM grants g LEFT JOIN parents p ON p.id = g.granted_by
         WHERE g.expires_at > ? ORDER BY g.expires_at",
    )
    .bind(util::now())
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, child_id, device_id, target, starts_at, expires_at, granted_by)| {
            let target = serde_json::from_str(&target).ok()?;
            Some(Grant { id, child_id, device_id, target, starts_at, expires_at, granted_by })
        })
        .collect())
}

pub fn grants_of_child(grants: &[Grant], child_id: &str) -> Vec<TemporaryGrant> {
    grants
        .iter()
        .filter(|g| g.child_id.as_deref() == Some(child_id))
        .filter_map(Grant::to_protocol)
        .collect()
}

pub fn grants_of_device(grants: &[Grant], device_id: &str) -> Vec<TemporaryGrant> {
    grants
        .iter()
        .filter(|g| g.device_id.as_deref() == Some(device_id))
        .filter_map(Grant::to_protocol)
        .collect()
}

/// Secondes d'écran d'un enfant aujourd'hui et cette semaine, en excluant
/// éventuellement un appareil (pour calculer le temps passé « ailleurs »).
pub async fn usage_seconds(
    db: &SqlitePool,
    child_id: &str,
    except_device: Option<&str>,
) -> Result<(i64, i64)> {
    let row: (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(CASE WHEN day = ?1 THEN seconds END), 0), COALESCE(SUM(seconds), 0)
         FROM usage WHERE child_id = ?2 AND day >= ?3 AND device_id IS NOT ?4",
    )
    .bind(util::day_str(util::today()))
    .bind(child_id)
    .bind(util::day_str(util::week_start()))
    .bind(except_device)
    .fetch_one(db)
    .await?;
    Ok(row)
}

pub fn usage_from_seconds((today, week): (i64, i64)) -> Usage {
    Usage { today_minutes: (today / 60) as u32, week_minutes: (week / 60) as u32 }
}

/// État d'accès à l'instant présent, dans le fuseau du serveur.
pub fn access_now(policy: &Policy, grants: &[TemporaryGrant], usage: Usage) -> AccessStatus {
    let now = Local::now();
    evaluate_access(policy, grants, now.to_utc(), now.weekday(), now.time(), usage)
}

fn hm(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).expect("heure valide")
}

/// Politique proposée à la création d'un enfant, selon son âge.
pub fn default_policy(birth_year: Option<i64>) -> Policy {
    let age = birth_year.map(|y| i64::from(util::today().year()) - y);
    let young = age.is_none_or(|a| a < 10);
    let preteen = age.is_none_or(|a| a < 13);

    let mut blocked_services = Vec::new();
    if preteen {
        blocked_services.extend(["tiktok", "instagram", "snapchat", "discord"]);
    }
    if young {
        blocked_services.extend(["youtube", "twitch", "chatgpt"]);
    }

    let school_day = || vec![TimeRange::new(hm(17, 0), hm(if young { 19 } else { 20 }, 0))];
    let free_day = || vec![TimeRange::new(hm(9, 0), hm(if young { 19 } else { 20 }, 0))];

    Policy {
        filter: FilterPolicy {
            blocked_categories: catalog::recommended_categories().map(str::to_string).collect(),
            blocked_services: blocked_services.into_iter().map(str::to_string).collect(),
            youtube_restricted: true,
            allow_requests: true,
            ..Default::default()
        },
        schedule: WeeklySchedule {
            monday: school_day(),
            tuesday: school_day(),
            wednesday: free_day(),
            thursday: school_day(),
            friday: school_day(),
            saturday: free_day(),
            sunday: free_day(),
        },
        quota: Quota { daily_minutes: Some(if young { 60 } else { 90 }), weekly_minutes: None },
    }
}

/// Politique d'un appareil partagé : la plus stricte de la maison, c'est-à-dire
/// l'union des blocages de tous les enfants, sans horaires ni quota.
pub fn strictest_policy(children: &[Child]) -> Policy {
    let mut filter = FilterPolicy {
        blocked_categories: catalog::recommended_categories().map(str::to_string).collect(),
        youtube_restricted: true,
        ..Default::default()
    };
    for child in children {
        filter.blocked_categories.extend(child.policy.filter.blocked_categories.iter().cloned());
        filter.blocked_services.extend(child.policy.filter.blocked_services.iter().cloned());
        filter.deny.extend(child.policy.filter.deny.iter().cloned());
    }
    Policy { filter, schedule: WeeklySchedule::always(), quota: Quota::default() }
}
