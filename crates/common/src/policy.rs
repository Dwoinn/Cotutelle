//! Politique appliquée à un couple appareil + compte de session.

use crate::holidays::{HolidayPeriod, holiday_on};
use crate::schedule::WeeklySchedule;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FilterPolicy {
    /// Catégories de listes bloquées (identifiants UT1, ex. `adult`).
    pub blocked_categories: BTreeSet<String>,
    /// Services bloqués (identifiants de [`crate::Service`]).
    pub blocked_services: BTreeSet<String>,
    /// Parmi les services bloqués, ceux que l'enfant voit dans son espace et
    /// peut demander à ses parents. Les autres sont bloqués sans être proposés.
    pub requestable_services: BTreeSet<String>,
    /// Domaines toujours autorisés, prioritaires sur tout le reste.
    pub allow: BTreeSet<String>,
    /// Domaines toujours bloqués, prioritaires sur les catégories.
    pub deny: BTreeSet<String>,
    /// Impose le mode restreint YouTube via CNAME `restrict.youtube.com`.
    pub youtube_restricted: bool,
    /// Autorise l'enfant à envoyer une demande depuis son espace.
    pub allow_requests: bool,
}

/// Quota de temps d'écran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Quota {
    /// Minutes par jour ; `None` = illimité dans les plages horaires.
    pub daily_minutes: Option<u32>,
    /// Minutes par semaine ; `None` = pas de plafond hebdomadaire.
    pub weekly_minutes: Option<u32>,
}

/// Règles propres aux vacances scolaires. Absentes, celles de la période
/// scolaire valent toute l'année.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HolidayPolicy {
    pub filter: FilterPolicy,
    pub schedule: WeeklySchedule,
    pub quota: Quota,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Policy {
    pub filter: FilterPolicy,
    pub schedule: WeeklySchedule,
    pub quota: Quota,
    pub holidays: Option<HolidayPolicy>,
}

/// Les règles en vigueur un jour donné : celles de la période scolaire ou
/// celles des vacances.
#[derive(Debug, Clone, Copy)]
pub struct Rules<'a> {
    pub filter: &'a FilterPolicy,
    pub schedule: &'a WeeklySchedule,
    pub quota: Quota,
    /// La période de vacances dont les règles s'appliquent, s'il y en a une.
    pub holiday: Option<&'a HolidayPeriod>,
}

impl Policy {
    /// Les règles de la période scolaire.
    pub fn school_rules(&self) -> Rules<'_> {
        Rules { filter: &self.filter, schedule: &self.schedule, quota: self.quota, holiday: None }
    }

    /// Les règles en vigueur à une date locale : celles des vacances si la
    /// date tombe dans une période et que la politique en a.
    ///
    /// Le quota hebdomadaire garde sa semaine du lundi au dimanche ; seul
    /// son plafond change selon le jour courant. C'est un choix simple, sans
    /// proratisation d'une semaine à cheval sur la rentrée.
    pub fn rules_on<'a>(&'a self, date: NaiveDate, periods: &'a [HolidayPeriod]) -> Rules<'a> {
        match (&self.holidays, holiday_on(periods, date)) {
            (Some(rules), Some(holiday)) => Rules {
                filter: &rules.filter,
                schedule: &rules.schedule,
                quota: rules.quota,
                holiday: Some(holiday),
            },
            _ => self.school_rules(),
        }
    }

    /// Les catégories bloquées à un moment ou un autre de l'année : les
    /// listes qu'un appareil doit détenir.
    pub fn blocked_categories(&self) -> BTreeSet<String> {
        let mut all = self.filter.blocked_categories.clone();
        if let Some(rules) = &self.holidays {
            all.extend(rules.filter.blocked_categories.iter().cloned());
        }
        all
    }
}

/// Ce qu'une exception temporaire autorise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrantTarget {
    /// Lève le blocage d'un service (ex. `youtube`).
    Service { service: String },
    /// Lève le blocage d'une catégorie entière.
    Category { category: String },
    /// Lève le blocage d'un domaine précis et de ses sous-domaines.
    Domain { domain: String },
    /// Ajoute des minutes au quota jusqu'à expiration (fin de journée).
    ExtraMinutes { minutes: u32 },
    /// Ignore les plages horaires jusqu'à expiration.
    IgnoreSchedule,
    /// Mesure inverse : ferme l'accès jusqu'à expiration (« pause »).
    Pause,
}

/// Exception temporaire accordée par un parent. Toujours bornée dans le temps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporaryGrant {
    pub id: crate::GrantId,
    pub target: GrantTarget,
    pub starts_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl TemporaryGrant {
    pub fn is_active_at(&self, now: DateTime<Utc>) -> bool {
        self.starts_at <= now && now < self.expires_at
    }
}
