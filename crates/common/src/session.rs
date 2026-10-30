//! Échanges entre l'agent et l'assistant de session (`cotutelle-session`),
//! par une socket Unix locale, un message JSON par ligne (D19).
//!
//! L'agent informe : situation du compte, préavis, fermetures, sites
//! bloqués, réponses des parents. L'assistant affiche et transmet les
//! demandes de l'enfant. Rien de ce qu'il envoie n'ouvre un accès : l'agent
//! reste seul à imposer quoi que ce soit (D3, D4) et garde ses notifications
//! de base quand aucun assistant n'est connecté.

use crate::{AccessStatus, BlockReason, ClosedReason, GrantTarget, catalog};
use chrono::{DateTime, NaiveTime, Utc, Weekday};
use serde::{Deserialize, Serialize};

/// Un service que l'enfant peut demander à ses parents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceRef {
    pub id: String,
    pub label: String,
}

/// Prochaine ouverture des horaires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opening {
    pub day: Weekday,
    pub time: NaiveTime,
}

impl Opening {
    /// « à 17 h », « demain à 8 h », « lundi à 8 h ».
    pub fn label(&self, today: Weekday) -> String {
        let when = if self.day == today {
            String::new()
        } else if self.day == today.succ() {
            "demain ".to_string()
        } else {
            format!("{} ", weekday_label(self.day))
        };
        format!("{when}à {}", time_label(self.time))
    }
}

/// Ouverture en cours accordée par les parents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenGrant {
    pub label: String,
    pub expires_at: DateTime<Utc>,
}

/// Situation du compte devant l'écran, montrée en permanence par l'assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionState {
    pub child_name: String,
    pub access: AccessStatus,
    /// Fin de la plage horaire du moment, quand elle en a une.
    pub closes_at: Option<NaiveTime>,
    pub next_opening: Option<Opening>,
    /// Nom de la période de vacances en cours.
    pub holiday: Option<String>,
    pub grants: Vec<OpenGrant>,
    pub allow_requests: bool,
    pub requestable_services: Vec<ServiceRef>,
    /// Le serveur est joignable : une demande part tout de suite.
    pub server_connected: bool,
    pub dry_run: bool,
}

/// Un événement à montrer à l'enfant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Notice {
    /// Il reste `minutes` minutes.
    Warning { minutes: u32 },
    /// L'accès s'est fermé. La session se verrouille après `grace_seconds`
    /// (0 : tout de suite).
    Closed {
        reason: ClosedReason,
        /// « demain à 8 h », déjà formulé.
        reopens: Option<String>,
        /// Quota du jour, pour expliquer un temps écoulé.
        quota_minutes: Option<u32>,
        grace_seconds: u32,
    },
    /// Le résolveur vient de bloquer un site pour ce compte.
    Blocked {
        domain: String,
        reason: BlockReason,
        /// Le service dont le site fait partie, s'il y en a un.
        service: Option<ServiceRef>,
        /// L'enfant peut en demander l'ouverture.
        requestable: bool,
    },
    /// Les parents ont répondu, ou la demande n'a pas pu être enregistrée.
    Answered { approved: bool, text: String },
}

impl Notice {
    pub fn title(&self) -> String {
        match self {
            Notice::Warning { minutes: 1 } => "Plus qu'une minute".into(),
            Notice::Warning { minutes } => format!("Plus que {minutes} minutes"),
            Notice::Closed { reason, .. } => match reason {
                ClosedReason::Paused => "Écran en pause",
                ClosedReason::OutsideSchedule => "Ce n'est pas l'heure",
                ClosedReason::DailyQuota | ClosedReason::WeeklyQuota => "Temps d'écran terminé",
            }
            .into(),
            Notice::Blocked { domain, .. } => format!("Site bloqué : {domain}"),
            Notice::Answered { approved: true, .. } => "Demande acceptée".into(),
            Notice::Answered { approved: false, .. } => "Demande refusée".into(),
        }
    }

    pub fn body(&self) -> String {
        match self {
            Notice::Warning { minutes: 1 } => "L'écran va se verrouiller.".into(),
            Notice::Warning { .. } => "Pense à enregistrer ce que tu fais.".into(),
            Notice::Closed { reason, reopens, quota_minutes, grace_seconds } => {
                let mut text = match reason {
                    ClosedReason::Paused => "Tes parents ont mis l'écran en pause.".to_string(),
                    ClosedReason::OutsideSchedule => {
                        "L'écran est verrouillé en dehors de tes horaires.".to_string()
                    }
                    ClosedReason::DailyQuota => match quota_minutes {
                        Some(q) => {
                            format!("Tu as utilisé tes {} d'aujourd'hui.", minutes_label(*q))
                        }
                        None => "Tu as utilisé tout ton temps pour aujourd'hui.".to_string(),
                    },
                    ClosedReason::WeeklyQuota => {
                        "Tu as utilisé tout ton temps pour cette semaine.".to_string()
                    }
                };
                if let Some(when) = reopens.as_deref().filter(|_| *reason != ClosedReason::Paused) {
                    text.push_str(&format!(" Ça rouvre {when}."));
                }
                if *grace_seconds > 0 {
                    text.push_str(&format!(
                        " L'écran se verrouille dans {grace_seconds} secondes."
                    ));
                }
                text
            }
            Notice::Blocked { reason, service, requestable, .. } => {
                let mut text = match (reason, service) {
                    (_, Some(service)) => format!("{} est bloqué par tes parents.", service.label),
                    (BlockReason::Category(id), None) => format!(
                        "Ce site fait partie d'une catégorie bloquée : {}.",
                        category_label(id)
                    ),
                    _ => "Tes parents ont bloqué ce site.".to_string(),
                };
                if *requestable {
                    text.push_str(" Tu peux leur demander l'accès.");
                }
                text
            }
            Notice::Answered { text, .. } => text.clone(),
        }
    }
}

/// Agent → assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Le compte de la session n'est géré par aucune règle : rien à montrer.
    Unmanaged,
    State(SessionState),
    Notice(Notice),
}

/// Assistant → agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HelperMessage {
    Hello {
        version: String,
    },
    /// Demande de l'enfant, relayée au serveur par l'agent.
    Request {
        target: GrantTarget,
        minutes: Option<u32>,
        message: Option<String>,
    },
}

/// « 45 min », « 1 h », « 1 h 30 ».
pub fn minutes_label(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// « 8 h », « 17 h 30 ».
pub fn time_label(time: NaiveTime) -> String {
    time_label_hm(time.format("%H").to_string(), time.format("%M").to_string())
}

fn time_label_hm(hour: String, minute: String) -> String {
    let hour = hour.trim_start_matches('0');
    let hour = if hour.is_empty() { "0" } else { hour };
    if minute == "00" { format!("{hour} h") } else { format!("{hour} h {minute}") }
}

pub fn weekday_label(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "lundi",
        Weekday::Tue => "mardi",
        Weekday::Wed => "mercredi",
        Weekday::Thu => "jeudi",
        Weekday::Fri => "vendredi",
        Weekday::Sat => "samedi",
        Weekday::Sun => "dimanche",
    }
}

fn category_label(id: &str) -> String {
    catalog::CATEGORIES
        .iter()
        .find(|c| c.id == id)
        .map_or_else(|| id.to_string(), |c| c.label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_read_naturally() {
        assert_eq!(minutes_label(45), "45 min");
        assert_eq!(minutes_label(60), "1 h");
        assert_eq!(minutes_label(90), "1 h 30");
        assert_eq!(time_label(NaiveTime::from_hms_opt(8, 0, 0).unwrap()), "8 h");
        assert_eq!(time_label(NaiveTime::from_hms_opt(17, 30, 0).unwrap()), "17 h 30");
        let eight = Opening { day: Weekday::Tue, time: NaiveTime::from_hms_opt(8, 0, 0).unwrap() };
        assert_eq!(eight.label(Weekday::Tue), "à 8 h");
        assert_eq!(eight.label(Weekday::Mon), "demain à 8 h");
        assert_eq!(eight.label(Weekday::Sat), "mardi à 8 h");
    }

    #[test]
    fn closed_notice_explains_and_announces_the_lock() {
        let notice = Notice::Closed {
            reason: ClosedReason::DailyQuota,
            reopens: Some("demain à 8 h".into()),
            quota_minutes: Some(90),
            grace_seconds: 30,
        };
        assert_eq!(notice.title(), "Temps d'écran terminé");
        assert_eq!(
            notice.body(),
            "Tu as utilisé tes 1 h 30 d'aujourd'hui. Ça rouvre demain à 8 h. L'écran se verrouille dans 30 secondes."
        );
        let paused = Notice::Closed {
            reason: ClosedReason::Paused,
            reopens: Some("à 17 h".into()),
            quota_minutes: None,
            grace_seconds: 0,
        };
        assert_eq!(paused.body(), "Tes parents ont mis l'écran en pause.");
    }

    #[test]
    fn blocked_notice_names_the_service() {
        let notice = Notice::Blocked {
            domain: "youtube.com".into(),
            reason: BlockReason::Service("youtube".into()),
            service: Some(ServiceRef { id: "youtube".into(), label: "YouTube".into() }),
            requestable: true,
        };
        assert_eq!(notice.title(), "Site bloqué : youtube.com");
        assert_eq!(
            notice.body(),
            "YouTube est bloqué par tes parents. Tu peux leur demander l'accès."
        );
        let json = serde_json::to_string(&AgentEvent::Notice(notice)).unwrap();
        assert!(json.starts_with(r#"{"type":"notice","kind":"blocked""#));
    }
}
