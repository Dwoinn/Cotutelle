//! Périodes de vacances scolaires. Les dates sont locales, la fin incluse :
//! « du 17 octobre au 1er novembre » couvre les deux jours nommés.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolidayPeriod {
    /// Nom affiché : « Vacances de la Toussaint », « Voyage en Bretagne ».
    pub label: String,
    pub start: NaiveDate,
    /// Dernier jour de vacances.
    pub end: NaiveDate,
}

impl HolidayPeriod {
    pub fn contains(&self, date: NaiveDate) -> bool {
        self.start <= date && date <= self.end
    }
}

/// La période de vacances qui couvre cette date, s'il y en a une.
pub fn holiday_on(periods: &[HolidayPeriod], date: NaiveDate) -> Option<&HolidayPeriod> {
    periods.iter().find(|p| p.contains(date))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn both_ends_are_included() {
        let periods = vec![HolidayPeriod {
            label: "Toussaint".into(),
            start: d(2026, 10, 17),
            end: d(2026, 11, 1),
        }];
        assert!(holiday_on(&periods, d(2026, 10, 16)).is_none());
        assert!(holiday_on(&periods, d(2026, 10, 17)).is_some());
        assert!(holiday_on(&periods, d(2026, 11, 1)).is_some());
        assert!(holiday_on(&periods, d(2026, 11, 2)).is_none());
    }
}
