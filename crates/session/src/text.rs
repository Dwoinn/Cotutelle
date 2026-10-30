//! Phrases de l'indicateur, du message d'accueil et des demandes.

use chrono::{Datelike, Local};
use cotutelle_common::session::{SessionState, minutes_label, time_label};
use cotutelle_common::{ClosedReason, GrantTarget};

/// La ligne principale : « Il te reste 1 h 20 », « Temps d'écran terminé ».
pub fn headline(state: &SessionState) -> String {
    if !state.access.open {
        return match state.access.reason {
            Some(ClosedReason::Paused) => "Écran en pause",
            Some(ClosedReason::OutsideSchedule) => "Ce n'est pas l'heure",
            _ => "Temps d'écran terminé",
        }
        .to_string();
    }
    match state.access.remaining_minutes {
        Some(m) => format!("Il te reste {}", minutes_label(m)),
        None => "Pas de limite de temps".to_string(),
    }
}

/// Les lignes de détail sous la principale.
pub fn details(state: &SessionState) -> Vec<String> {
    let mut lines = Vec::new();
    let today = Local::now().weekday();
    if !state.access.open {
        if let Some(opening) = state.next_opening {
            lines.push(format!("Ça rouvre {}", opening.label(today)));
        }
    } else if let Some(end) = state.closes_at {
        lines.push(format!("Jusqu'à {}", time_label(end)));
    }
    let used = minutes_label(state.access.used_today_minutes);
    lines.push(match state.access.quota_today_minutes {
        Some(quota) => format!("Aujourd'hui : {used} sur {}", minutes_label(quota)),
        None => format!("Aujourd'hui : {used}"),
    });
    if let Some(holiday) = &state.holiday {
        lines.push(format!("Règles de vacances ({holiday})"));
    }
    for grant in &state.grants {
        let until = time_label(grant.expires_at.with_timezone(&Local).time());
        lines.push(format!("{} jusqu'à {until}", grant.label));
    }
    if !state.server_connected {
        lines.push("Serveur injoignable : tes règles habituelles s'appliquent".to_string());
    }
    if state.dry_run {
        lines.push("Mode d'essai : rien ne se verrouille".to_string());
    }
    lines
}

/// Le nom d'un service demandable, ou son identifiant s'il n'est plus proposé.
pub fn service_label(state: &SessionState, id: &str) -> String {
    state
        .requestable_services
        .iter()
        .find(|s| s.id == id)
        .map_or_else(|| id.to_string(), |s| s.label.clone())
}

/// Ce que l'enfant a demandé : « 30 min de plus », « YouTube pendant 1 h ».
pub fn request_label(state: &SessionState, target: &GrantTarget, minutes: Option<u32>) -> String {
    let what = match target {
        GrantTarget::ExtraMinutes { minutes } => return format!("{minutes} min de plus"),
        GrantTarget::Service { service } => service_label(state, service),
        GrantTarget::Domain { domain } => domain.clone(),
        GrantTarget::Category { category } => category.clone(),
        GrantTarget::IgnoreSchedule => "hors horaires".to_string(),
        GrantTarget::Pause => "une pause".to_string(),
    };
    match minutes {
        Some(m) => format!("{what} pendant {}", minutes_label(m)),
        None => what,
    }
}

/// Le message d'accueil quand l'accès est ouvert.
pub fn today(state: &SessionState) -> String {
    let mut text = match state.access.remaining_minutes {
        Some(m) => format!("Il te reste {} aujourd'hui", minutes_label(m)),
        None => "Pas de limite de temps aujourd'hui".to_string(),
    };
    if let Some(end) = state.closes_at {
        text.push_str(&format!(", jusqu'à {}", time_label(end)));
    }
    text.push('.');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;
    use cotutelle_common::AccessStatus;
    use cotutelle_common::session::ServiceRef;

    fn state(open: bool, remaining: Option<u32>) -> SessionState {
        SessionState {
            child_name: "Louis".into(),
            access: AccessStatus {
                open,
                reason: (!open).then_some(ClosedReason::DailyQuota),
                remaining_minutes: remaining,
                quota_today_minutes: Some(90),
                used_today_minutes: 10,
            },
            closes_at: NaiveTime::from_hms_opt(19, 0, 0),
            next_opening: None,
            holiday: None,
            grants: vec![],
            allow_requests: true,
            requestable_services: vec![],
            server_connected: true,
            dry_run: false,
        }
    }

    #[test]
    fn lines_describe_the_day() {
        let open = state(true, Some(80));
        assert_eq!(headline(&open), "Il te reste 1 h 20");
        assert_eq!(details(&open), vec!["Jusqu'à 19 h", "Aujourd'hui : 10 min sur 1 h 30"]);
        assert_eq!(today(&open), "Il te reste 1 h 20 aujourd'hui, jusqu'à 19 h.");
        assert_eq!(headline(&state(false, Some(0))), "Temps d'écran terminé");
    }

    #[test]
    fn requests_read_as_the_child_meant_them() {
        let mut open = state(true, Some(80));
        open.requestable_services =
            vec![ServiceRef { id: "youtube".into(), label: "YouTube".into() }];
        let youtube = GrantTarget::Service { service: "youtube".into() };
        assert_eq!(request_label(&open, &youtube, Some(60)), "YouTube pendant 1 h");
        let site = GrantTarget::Domain { domain: "exemple.fr".into() };
        assert_eq!(request_label(&open, &site, Some(30)), "exemple.fr pendant 30 min");
        let more = GrantTarget::ExtraMinutes { minutes: 30 };
        assert_eq!(request_label(&open, &more, None), "30 min de plus");
    }
}
