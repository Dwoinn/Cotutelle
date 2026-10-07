//! Évaluation de l'accès : horaires, quota et exceptions de temps.

use crate::policy::{GrantTarget, Policy, TemporaryGrant};
use chrono::{DateTime, NaiveTime, Utc, Weekday};
use serde::{Deserialize, Serialize};

/// Temps d'écran déjà consommé, tous appareils confondus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    pub today_minutes: u32,
    pub week_minutes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClosedReason {
    /// Un parent a mis l'accès en pause.
    Paused,
    OutsideSchedule,
    DailyQuota,
    WeeklyQuota,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessStatus {
    pub open: bool,
    pub reason: Option<ClosedReason>,
    /// Minutes avant la prochaine fermeture ; `None` = aucune limite en vue.
    pub remaining_minutes: Option<u32>,
    /// Quota du jour, bonus compris ; `None` = pas de quota journalier.
    pub quota_today_minutes: Option<u32>,
    pub used_today_minutes: u32,
}

fn min_opt(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Détermine si l'accès est ouvert à cet instant et pour combien de temps.
///
/// `day` et `time` sont l'heure locale de l'appareil ; `now` sert uniquement
/// à savoir quelles exceptions sont actives.
pub fn evaluate_access(
    policy: &Policy,
    grants: &[TemporaryGrant],
    now: DateTime<Utc>,
    day: Weekday,
    time: NaiveTime,
    usage: Usage,
) -> AccessStatus {
    let active = || grants.iter().filter(|g| g.is_active_at(now));

    let extra: u32 = active()
        .filter_map(|g| match g.target {
            GrantTarget::ExtraMinutes { minutes } => Some(minutes),
            _ => None,
        })
        .sum();
    let quota_today = policy.quota.daily_minutes.map(|q| q + extra);
    let quota_week = policy.quota.weekly_minutes.map(|q| q + extra);

    // Fin de l'exception « ignorer les horaires » la plus tardive, en minutes.
    let schedule_override = active()
        .filter(|g| matches!(g.target, GrantTarget::IgnoreSchedule))
        .map(|g| (g.expires_at - now).num_minutes().max(0) as u32)
        .max();

    let in_schedule = policy.schedule.minutes_until_close(day, time);
    // Au-delà d'une journée, la fin de plage n'est pas une limite à annoncer.
    let no_limit_in_sight = |m: u32| m > 24 * 60;
    let mut status = AccessStatus {
        open: true,
        reason: None,
        remaining_minutes: None,
        quota_today_minutes: quota_today,
        used_today_minutes: usage.today_minutes,
    };

    let schedule_left = match (in_schedule, schedule_override) {
        (Some(s), Some(o)) => Some(s.max(o)),
        (Some(s), None) => Some(s),
        (None, Some(o)) => Some(o),
        (None, None) => {
            status.open = false;
            status.reason = Some(ClosedReason::OutsideSchedule);
            Some(0)
        }
    };
    let schedule_left = schedule_left.filter(|m| !no_limit_in_sight(*m));
    let daily_left = quota_today.map(|q| q.saturating_sub(usage.today_minutes));
    let weekly_left = quota_week.map(|q| q.saturating_sub(usage.week_minutes));

    let paused = active().any(|g| matches!(g.target, GrantTarget::Pause));
    if paused {
        status.open = false;
        status.reason = Some(ClosedReason::Paused);
    }

    if status.open {
        if daily_left == Some(0) {
            status.open = false;
            status.reason = Some(ClosedReason::DailyQuota);
        } else if weekly_left == Some(0) {
            status.open = false;
            status.reason = Some(ClosedReason::WeeklyQuota);
        }
    }

    status.remaining_minutes = if status.open {
        min_opt(schedule_left, min_opt(daily_left, weekly_left))
    } else {
        Some(0)
    };
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GrantId;
    use crate::policy::Quota;
    use crate::schedule::{TimeRange, WeeklySchedule};
    use chrono::Duration;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn policy(daily: Option<u32>) -> Policy {
        Policy {
            schedule: WeeklySchedule {
                wednesday: vec![TimeRange::new(t(14, 0), t(18, 0))],
                ..Default::default()
            },
            quota: Quota { daily_minutes: daily, weekly_minutes: None },
            ..Default::default()
        }
    }

    fn grant(target: GrantTarget, now: DateTime<Utc>, minutes: i64) -> TemporaryGrant {
        TemporaryGrant {
            id: GrantId::new(),
            target,
            starts_at: now,
            expires_at: now + Duration::minutes(minutes),
        }
    }

    fn usage(today: u32) -> Usage {
        Usage { today_minutes: today, week_minutes: today }
    }

    #[test]
    fn open_inside_schedule_with_quota_left() {
        let now = Utc::now();
        let s = evaluate_access(&policy(Some(60)), &[], now, Weekday::Wed, t(15, 0), usage(20));
        assert!(s.open);
        assert_eq!(s.remaining_minutes, Some(40));
        assert_eq!(s.quota_today_minutes, Some(60));
    }

    #[test]
    fn schedule_end_caps_remaining_time() {
        let now = Utc::now();
        let s = evaluate_access(&policy(Some(120)), &[], now, Weekday::Wed, t(17, 45), usage(0));
        assert_eq!(s.remaining_minutes, Some(15));
    }

    #[test]
    fn closed_outside_schedule_and_on_quota() {
        let now = Utc::now();
        let s = evaluate_access(&policy(Some(60)), &[], now, Weekday::Thu, t(15, 0), usage(0));
        assert_eq!((s.open, s.reason), (false, Some(ClosedReason::OutsideSchedule)));
        let s = evaluate_access(&policy(Some(60)), &[], now, Weekday::Wed, t(15, 0), usage(60));
        assert_eq!((s.open, s.reason), (false, Some(ClosedReason::DailyQuota)));
        assert_eq!(s.remaining_minutes, Some(0));
    }

    #[test]
    fn extra_minutes_extend_quota() {
        let now = Utc::now();
        let g = [grant(GrantTarget::ExtraMinutes { minutes: 30 }, now, 600)];
        let s = evaluate_access(&policy(Some(60)), &g, now, Weekday::Wed, t(15, 0), usage(60));
        assert!(s.open);
        assert_eq!(s.remaining_minutes, Some(30));
        assert_eq!(s.quota_today_minutes, Some(90));
    }

    #[test]
    fn ignore_schedule_opens_until_grant_expires() {
        let now = Utc::now();
        let g = [grant(GrantTarget::IgnoreSchedule, now, 45)];
        let s = evaluate_access(&policy(None), &g, now, Weekday::Thu, t(21, 0), usage(0));
        assert!(s.open);
        assert!(matches!(s.remaining_minutes, Some(44..=45)));
    }

    #[test]
    fn pause_closes_even_inside_schedule() {
        let now = Utc::now();
        let g = [grant(GrantTarget::Pause, now, 30)];
        let s = evaluate_access(&policy(None), &g, now, Weekday::Wed, t(15, 0), usage(0));
        assert_eq!((s.open, s.reason), (false, Some(ClosedReason::Paused)));
    }

    #[test]
    fn no_limits_means_no_remaining() {
        let now = Utc::now();
        let p = Policy { schedule: WeeklySchedule::always(), ..Default::default() };
        let s = evaluate_access(&p, &[], now, Weekday::Sun, t(12, 0), usage(500));
        assert!(s.open);
        assert_eq!(s.remaining_minutes, None);
    }
}
