//! Plages horaires hebdomadaires. Les heures sont locales à l'appareil.

use chrono::{NaiveTime, Weekday};
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
        Self {
            start,
            end: Some(end),
        }
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
    fn until_midnight_covers_late_evening() {
        let r = TimeRange::until_midnight(t(20, 0));
        assert!(r.contains(t(23, 59)));
        assert!(!r.contains(t(19, 59)));
    }
}
