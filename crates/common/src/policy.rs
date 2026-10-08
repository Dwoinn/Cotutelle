//! Politique appliquée à un couple appareil + compte de session.

use crate::schedule::WeeklySchedule;
use chrono::{DateTime, Utc};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Policy {
    pub filter: FilterPolicy,
    pub schedule: WeeklySchedule,
    pub quota: Quota,
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
