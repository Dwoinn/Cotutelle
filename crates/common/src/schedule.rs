//! Plages horaires hebdomadaires. Les heures sont locales à l'appareil.

use chrono::{NaiveTime, Timelike, Weekday};
use serde::{Deserialize, Serialize};

/// Une plage `[start, end)` dans la journée. `end` peut valoir 24:00 via
/// [`TimeRange::until_midnight`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: NaiveTime,
    /// Fin exclusive. `None` signifie « jusqu'à minuit ».
    pub end: Option<NaiveTime>,
}

impl TimeRange {
    pub fn new(start: NaiveTime, end: NaiveTime) -> Self {
        Self { start, end: Some(end) }
    }

    pub fn until_midnight(start: NaiveTime) -> Self {
        Self { start, end: None }
    }

    pub fn contains(&self, t: NaiveTime) -> bool {
        match self.end {
            Some(end) => self.start <= t && t < end,
            None => self.start <= t,
        }
    }
}

/// Plages autorisées pour chaque jour. Un jour sans plage = aucun accès.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WeeklySchedule {
    pub monday: Vec<TimeRange>,
    pub tuesday: Vec<TimeRange>,
    pub wednesday: Vec<TimeRange>,
    pub thursday: Vec<TimeRange>,
    pub friday: Vec<TimeRange>,
    pub saturday: Vec<TimeRange>,
    pub sunday: Vec<TimeRange>,
}

impl WeeklySchedule {
    pub fn ranges_for(&self, day: Weekday) -> &[TimeRange] {
        match day {
            Weekday::Mon => &self.monday,
            Weekday::Tue => &self.tuesday,
            Weekday::Wed => &self.wednesday,
            Weekday::Thu => &self.thursday,
            Weekday::Fri => &self.friday,
            Weekday::Sat => &self.saturday,
            Weekday::Sun => &self.sunday,
        }
    }

    /// L'accès est-il autorisé à cet instant local ?
    pub fn allows(&self, day: Weekday, time: NaiveTime) -> bool {
        self.ranges_for(day).iter().any(|r| r.contains(time))
    }

    /// Minutes restantes avant la fin de la plage en cours, `None` hors plage.
    ///
    /// Une plage qui va jusqu'à minuit se prolonge sur le lendemain s'il
    /// rouvre à 00:00 ; le résultat est plafonné à sept jours.
    pub fn minutes_until_close(&self, day: Weekday, time: NaiveTime) -> Option<u32> {
        let mut total = 0;
        let (mut day, mut time) = (day, time);
        for _ in 0..7 {
            let now = minute_of_day(time);
            let range = self
                .ranges_for(day)
                .iter()
                .filter(|r| r.contains(time))
                .max_by_key(|r| r.end.map_or(24 * 60, minute_of_day));
            let Some(range) = range else {
                return (total > 0).then_some(total);
            };
            match range.end {
                Some(end) => return Some(total + minute_of_day(end).saturating_sub(now)),
                None => {
                    total += 24 * 60 - now;
                    day = day.succ();
                    time = NaiveTime::MIN;
                }
            }
        }
        Some(total)
    }

    /// Prochaine ouverture strictement après cet instant, sur sept jours.
    pub fn next_opening(&self, day: Weekday, time: NaiveTime) -> Option<(Weekday, NaiveTime)> {
        let mut current = day;
        for offset in 0..=7 {
            let mut starts: Vec<NaiveTime> = self
                .ranges_for(current)
                .iter()
                .map(|r| r.start)
                .filter(|s| offset > 0 || *s > time)
                .collect();
            starts.sort();
            if let Some(start) = starts.first() {
                return Some((current, *start));
            }
            current = current.succ();
        }
        None
    }

    /// Une plage « toute la journée » pour chaque jour : aucune restriction.
    pub fn always() -> Self {
        let all_day = || vec![TimeRange::until_midnight(NaiveTime::MIN)];
        Self {
            monday: all_day(),
            tuesday: all_day(),
            wednesday: all_day(),
            thursday: all_day(),
            friday: all_day(),
            saturday: all_day(),
            sunday: all_day(),
        }
    }
}

fn minute_of_day(t: NaiveTime) -> u32 {
    t.hour() * 60 + t.minute()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn empty_schedule_blocks_everything() {
        let s = WeeklySchedule::default();
        assert!(!s.allows(Weekday::Wed, t(15, 0)));
    }

    #[test]
    fn range_end_is_exclusive() {
        let s = WeeklySchedule {
            wednesday: vec![TimeRange::new(t(14, 0), t(18, 0))],
            ..Default::default()
        };
        assert!(s.allows(Weekday::Wed, t(14, 0)));
        assert!(s.allows(Weekday::Wed, t(17, 59)));
        assert!(!s.allows(Weekday::Wed, t(18, 0)));
        assert!(!s.allows(Weekday::Thu, t(15, 0)));
    }

    #[test]
    fn minutes_until_close_and_next_opening() {
        let s = WeeklySchedule {
            wednesday: vec![TimeRange::new(t(14, 0), t(18, 0))],
            saturday: vec![TimeRange::new(t(10, 0), t(12, 0)), TimeRange::until_midnight(t(20, 0))],
            ..Default::default()
        };
        assert_eq!(s.minutes_until_close(Weekday::Wed, t(17, 30)), Some(30));
        assert_eq!(s.minutes_until_close(Weekday::Wed, t(18, 0)), None);
        assert_eq!(s.minutes_until_close(Weekday::Sat, t(23, 0)), Some(60));
        assert!(
            WeeklySchedule::always().minutes_until_close(Weekday::Sat, t(23, 0)).unwrap() > 24 * 60
        );
        assert_eq!(s.next_opening(Weekday::Wed, t(13, 0)), Some((Weekday::Wed, t(14, 0))));
        assert_eq!(s.next_opening(Weekday::Wed, t(19, 0)), Some((Weekday::Sat, t(10, 0))));
        assert_eq!(s.next_opening(Weekday::Sat, t(11, 0)), Some((Weekday::Sat, t(20, 0))));
        assert_eq!(WeeklySchedule::default().next_opening(Weekday::Mon, t(8, 0)), None);
        assert!(WeeklySchedule::always().allows(Weekday::Sun, t(3, 0)));
    }

    #[test]
    fn until_midnight_covers_late_evening() {
        let r = TimeRange::until_midnight(t(20, 0));
        assert!(r.contains(t(23, 59)));
        assert!(!r.contains(t(19, 59)));
    }
}
