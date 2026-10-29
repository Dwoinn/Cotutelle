//! Vacances scolaires : calendrier officiel de l'Éducation nationale pour la
//! zone choisie, ajustements des parents, périodes effectives.
//!
//! Le calendrier vient du jeu de données ouvert `fr-en-calendrier-scolaire`
//! (data.education.gouv.fr). Il est gardé en base avec sa date de
//! téléchargement et rafraîchi chaque semaine ; hors ligne, la dernière
//! version connue reste en vigueur. Les périodes effectives sont celles du
//! calendrier moins celles que les parents ont retirées, plus celles qu'ils
//! ont ajoutées.

use crate::state::Shared;
use crate::util;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use cotutelle_common::HolidayPeriod;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Zones du calendrier officiel, telles que le jeu de données les nomme.
pub const ZONES: &[&str] = &[
    "Zone A",
    "Zone B",
    "Zone C",
    "Corse",
    "Guadeloupe",
    "Guyane",
    "Martinique",
    "Mayotte",
    "Nouvelle Calédonie",
    "Polynésie",
    "Réunion",
    "Saint Pierre et Miquelon",
    "Wallis et Futuna",
];

const DATASET_URL: &str = "https://data.education.gouv.fr/api/explore/v2.1/catalog/datasets/fr-en-calendrier-scolaire/records";
const REFRESH_AFTER_SECONDS: i64 = 7 * 86_400;
const SETTINGS_KEY: &str = "holidays";
const CALENDAR_KEY: &str = "holiday_calendar";

/// Ce que les parents ont choisi.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HolidaySettings {
    /// Zone du calendrier officiel ; vide = pas de calendrier.
    pub zone: String,
    /// Périodes du calendrier écartées, repérées par leur premier jour.
    pub removed: BTreeSet<NaiveDate>,
    /// Périodes ajoutées à la main : pont, voyage, stage.
    pub added: Vec<HolidayPeriod>,
}

/// Le calendrier officiel tel que téléchargé la dernière fois.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Calendar {
    pub zone: String,
    pub fetched_at: i64,
    pub periods: Vec<HolidayPeriod>,
}

async fn load<T: Default + for<'de> Deserialize<'de>>(state: &Shared, key: &str) -> T {
    crate::notify::setting(state, key)
        .await
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

async fn store<T: Serialize>(state: &Shared, key: &str, value: &T) -> Result<()> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(serde_json::to_string(value)?)
        .execute(&state.db)
        .await?;
    Ok(())
}

pub async fn settings(state: &Shared) -> HolidaySettings {
    load(state, SETTINGS_KEY).await
}

pub async fn save_settings(state: &Shared, settings: &HolidaySettings) -> Result<()> {
    store(state, SETTINGS_KEY, settings).await
}

pub async fn calendar(state: &Shared) -> Calendar {
    load(state, CALENDAR_KEY).await
}

/// Les périodes du calendrier qui comptent : celles de la zone choisie.
pub async fn calendar_for(state: &Shared, settings: &HolidaySettings) -> Calendar {
    let calendar = calendar(state).await;
    if settings.zone.is_empty() || calendar.zone != settings.zone {
        return Calendar { zone: settings.zone.clone(), ..Default::default() };
    }
    calendar
}

/// Les périodes effectives, triées : importées moins retirées, plus ajoutées.
pub fn effective(settings: &HolidaySettings, calendar: &Calendar) -> Vec<HolidayPeriod> {
    let mut periods: Vec<HolidayPeriod> = calendar
        .periods
        .iter()
        .filter(|p| !settings.removed.contains(&p.start))
        .chain(settings.added.iter())
        .cloned()
        .collect();
    periods.sort_by_key(|p| (p.start, p.end));
    periods
}

pub async fn periods(state: &Shared) -> Vec<HolidayPeriod> {
    let settings = settings(state).await;
    let calendar = calendar_for(state, &settings).await;
    effective(&settings, &calendar)
}

/// Date locale d'une borne du calendrier. Les bornes sont des minuits
/// locaux exprimés en UTC (`2026-10-16T22:00:00+00:00` pour Paris) : on
/// arrondit au jour le plus proche, ce qui vaut pour toutes les zones, de
/// la Polynésie à Wallis-et-Futuna, sans table des fuseaux.
fn local_date(value: &str) -> Option<NaiveDate> {
    let at = DateTime::parse_from_rfc3339(value).ok()?.with_timezone(&Utc);
    Some((at + Duration::hours(12)).date_naive())
}

/// Lit les enregistrements du jeu de données en périodes, dédoublonnées :
/// le calendrier répète chaque période pour chaque académie de la zone.
pub fn parse_records(records: &[Value]) -> Vec<HolidayPeriod> {
    let mut seen = BTreeSet::new();
    let mut periods = Vec::new();
    for record in records {
        // Les dates des enseignants (prérentrée) ne concernent pas les enfants.
        if record["population"].as_str() == Some("Enseignants") {
            continue;
        }
        let Some(label) = record["description"].as_str().map(str::trim).filter(|l| !l.is_empty())
        else {
            continue;
        };
        let (Some(start), Some(end)) = (
            record["start_date"].as_str().and_then(local_date),
            record["end_date"].as_str().and_then(local_date),
        ) else {
            continue;
        };
        // La fin du calendrier est le jour de la reprise, exclu ; un pont
        // est noté avec ses deux bornes égales.
        let end = if end > start { end - Duration::days(1) } else { start };
        if seen.insert((start, end, label.to_string())) {
            periods.push(HolidayPeriod { label: label.to_string(), start, end });
        }
    }
    periods.sort_by_key(|p| (p.start, p.end));
    periods
}

/// Télécharge le calendrier d'une zone : l'année scolaire en cours et la
/// suivante, quand elle est publiée.
pub async fn fetch(http: &reqwest::Client, zone: &str) -> Result<Vec<HolidayPeriod>> {
    let since = util::today() - Duration::days(400);
    let query = [
        ("where", format!("zones=\"{zone}\" AND end_date>=\"{}\"", util::day_str(since))),
        ("select", "description,start_date,end_date,population".to_string()),
        ("order_by", "start_date".to_string()),
        ("limit", "100".to_string()),
    ];
    let body: Value = http
        .get(DATASET_URL)
        .query(&query)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .context("calendrier scolaire injoignable")?
        .error_for_status()
        .context("calendrier scolaire : réponse en erreur")?
        .json()
        .await
        .context("calendrier scolaire illisible")?;
    let records = body["results"].as_array().context("calendrier scolaire sans résultats")?;
    let periods = parse_records(records);
    anyhow::ensure!(!periods.is_empty(), "aucune période de vacances pour la zone {zone}");
    Ok(periods)
}

/// Met à jour le calendrier s'il manque ou date de plus d'une semaine.
/// Rend `true` quand les périodes ont changé.
pub async fn refresh(state: &Shared, force: bool) -> Result<bool> {
    let settings = settings(state).await;
    if settings.zone.is_empty() {
        return Ok(false);
    }
    let current = calendar_for(state, &settings).await;
    let stale = current.fetched_at == 0 || util::now() - current.fetched_at > REFRESH_AFTER_SECONDS;
    if !force && !stale {
        return Ok(false);
    }
    let periods = fetch(&state.http, &settings.zone).await?;
    let changed = periods != current.periods;
    let calendar = Calendar { zone: settings.zone, fetched_at: util::now(), periods };
    store(state, CALENDAR_KEY, &calendar).await?;
    if changed {
        tracing::info!(zone = %calendar.zone, count = calendar.periods.len(), "calendrier scolaire à jour");
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn d(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn records_become_local_periods_with_inclusive_end() {
        let record = |label: &str, start: &str, end: &str, population: &str| json!({ "description": label, "start_date": start, "end_date": end, "population": population });
        let records = vec![
            record(
                "Vacances de la Toussaint",
                "2026-10-16T22:00:00+00:00",
                "2026-11-01T23:00:00+00:00",
                "-",
            ),
            // La même période pour une autre académie.
            record(
                "Vacances de la Toussaint",
                "2026-10-16T22:00:00+00:00",
                "2026-11-01T23:00:00+00:00",
                "-",
            ),
            record(
                "Pont de l'Ascension",
                "2027-05-06T22:00:00+00:00",
                "2027-05-06T22:00:00+00:00",
                "-",
            ),
            record(
                "Vacances d'Été",
                "2027-07-02T22:00:00+00:00",
                "2027-08-31T22:00:00+00:00",
                "Enseignants",
            ),
            record(
                "Vacances d'Été",
                "2027-07-02T22:00:00+00:00",
                "2027-09-01T22:00:00+00:00",
                "Élèves",
            ),
            // Polynésie : minuit local tombe à 10 h UTC.
            record(
                "Vacances de Noël",
                "2026-12-18T10:00:00+00:00",
                "2027-01-04T10:00:00+00:00",
                "-",
            ),
        ];
        let periods = parse_records(&records);
        let summary: Vec<(&str, NaiveDate, NaiveDate)> =
            periods.iter().map(|p| (p.label.as_str(), p.start, p.end)).collect();
        assert_eq!(
            summary,
            vec![
                ("Vacances de la Toussaint", d(2026, 10, 17), d(2026, 11, 1)),
                ("Vacances de Noël", d(2026, 12, 18), d(2027, 1, 3)),
                ("Pont de l'Ascension", d(2027, 5, 7), d(2027, 5, 7)),
                ("Vacances d'Été", d(2027, 7, 3), d(2027, 9, 1)),
            ]
        );
    }

    #[test]
    fn parents_adjust_the_calendar() {
        let period = |label: &str, start: NaiveDate, end: NaiveDate| HolidayPeriod {
            label: label.into(),
            start,
            end,
        };
        let calendar = Calendar {
            zone: "Zone A".into(),
            fetched_at: 1,
            periods: vec![
                period("Toussaint", d(2026, 10, 17), d(2026, 11, 1)),
                period("Noël", d(2026, 12, 19), d(2027, 1, 3)),
            ],
        };
        let settings = HolidaySettings {
            zone: "Zone A".into(),
            removed: [d(2026, 12, 19)].into_iter().collect(),
            added: vec![period("Pont", d(2026, 11, 11), d(2026, 11, 11))],
        };
        let periods = effective(&settings, &calendar);
        let labels: Vec<&str> = periods.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, vec!["Toussaint", "Pont"]);
    }
}
