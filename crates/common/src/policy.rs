//! Politique appliquée à un couple appareil + compte de session.

use crate::schedule::WeeklySchedule;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Catégories de la liste UT1 (Université de Toulouse). La liste complète
/// sera dérivée des métadonnées des listes en phase 1 ; on fixe ici celles
/// qui structurent l'interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Adult,
    Agressif,
    Arjel,
    Dating,
    Drogue,
    Gambling,
    Games,
    Hacking,
    Malware,
    Phishing,
    Publicite,
    Redirector,
    Sexual,
    SocialNetworks,
    Shopping,
    Vpn,
    Warez,
    /// Catégorie Cotutelle, hors UT1 : fournisseurs DNS-over-HTTPS connus.
    DohProviders,
    /// Catégorie Cotutelle, hors UT1 : plateformes vidéo (YouTube, Twitch, …).
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FilterPolicy {
    /// Catégories bloquées en permanence.
    pub blocked_categories: BTreeSet<Category>,
    /// Domaines toujours autorisés, prioritaires sur tout le reste.
    pub allow: BTreeSet<String>,
    /// Domaines toujours bloqués, prioritaires sur les catégories.
    pub deny: BTreeSet<String>,
    /// Impose le mode restreint YouTube via CNAME `restrict.youtube.com`.
    pub youtube_restricted: bool,
    /// Autorise l'enfant à envoyer une demande depuis la page de blocage.
    pub allow_requests: bool,
}

/// Quota de temps d'écran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Quota {
    /// Minutes par jour ; `None` = illimité dans les plages horaires.
    pub daily_minutes: Option<u32>,
    /// Minutes par semaine ; `None` = pas de plafond hebdomadaire.
    pub weekly_minutes: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Policy {
    pub filter: FilterPolicy,
    pub schedule: WeeklySchedule,
    pub quota: Quota,
}

/// Ce qu'une exception temporaire autorise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrantTarget {
    /// Lève le blocage d'une catégorie (ex. `Video` pour YouTube).
    Category { category: Category },
    /// Lève le blocage d'un domaine précis et de ses sous-domaines.
    Domain { domain: String },
    /// Ajoute des minutes au quota du jour.
    ExtraMinutes { minutes: u32 },
    /// Ignore les plages horaires jusqu'à expiration.
    IgnoreSchedule,
}

/// Exception temporaire accordée par un parent. Toujours bornée dans le temps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporaryGrant {
    pub id: crate::GrantId,
    pub child: crate::ChildId,
    pub granted_by: crate::ParentId,
    pub target: GrantTarget,
    pub starts_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl TemporaryGrant {
    pub fn is_active_at(&self, now: DateTime<Utc>) -> bool {
        self.starts_at <= now && now < self.expires_at
    }
}
