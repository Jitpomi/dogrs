//! Anchor-preserving UTC calendar arithmetic for durable schedulers.
//! Domain policy selects a period; this module has no financial or tenant rules.
use crate::{QueueError, QueueResult};
use chrono::{DateTime, Duration, Months};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "unit", rename_all = "snake_case")]
pub enum CalendarPeriod {
    Once,
    Days { every: u32 },
    Months { every: u32 },
}
impl CalendarPeriod {
    pub fn occurrence(self, anchor: i64, sequence: u32) -> QueueResult<Option<i64>> {
        let invalid = || {
            QueueError::InvalidConfig("Invalid calendar period or exhausted UTC calendar".into())
        };
        let at = DateTime::from_timestamp(anchor, 0).ok_or_else(invalid)?;
        let result = match self {
            Self::Once => {
                if sequence == 0 {
                    Some(at)
                } else {
                    None
                }
            }
            Self::Days { every } => {
                if every == 0 {
                    return Err(invalid());
                }
                let days = i64::from(every)
                    .checked_mul(i64::from(sequence))
                    .ok_or_else(invalid)?;
                Some(
                    at.checked_add_signed(Duration::try_days(days).ok_or_else(invalid)?)
                        .ok_or_else(invalid)?,
                )
            }
            Self::Months { every } => {
                if every == 0 {
                    return Err(invalid());
                }
                Some(
                    at.checked_add_months(Months::new(
                        every.checked_mul(sequence).ok_or_else(invalid)?,
                    ))
                    .ok_or_else(invalid)?,
                )
            }
        };
        Ok(result.map(|v| v.timestamp()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn stamp(v: &str) -> i64 {
        DateTime::parse_from_rfc3339(v).unwrap().timestamp()
    }
    #[test]
    fn original_anchor_survives_month_ends_and_leap_years() {
        let anchor = stamp("2027-01-31T15:00:00Z");
        let monthly = CalendarPeriod::Months { every: 1 };
        assert_eq!(
            monthly.occurrence(anchor, 1).unwrap(),
            Some(stamp("2027-02-28T15:00:00Z"))
        );
        assert_eq!(
            monthly.occurrence(anchor, 2).unwrap(),
            Some(stamp("2027-03-31T15:00:00Z"))
        );
        let leap = stamp("2024-02-29T01:00:00Z");
        let yearly = CalendarPeriod::Months { every: 12 };
        assert_eq!(
            yearly.occurrence(leap, 1).unwrap(),
            Some(stamp("2025-02-28T01:00:00Z"))
        );
        assert_eq!(
            yearly.occurrence(leap, 4).unwrap(),
            Some(stamp("2028-02-29T01:00:00Z"))
        );
    }
    #[test]
    fn once_and_fixed_days_preserve_utc() {
        let anchor = stamp("2026-03-07T12:00:00Z");
        assert_eq!(
            CalendarPeriod::Once.occurrence(anchor, 0).unwrap(),
            Some(anchor)
        );
        assert_eq!(CalendarPeriod::Once.occurrence(anchor, 1).unwrap(), None);
        assert_eq!(
            CalendarPeriod::Days { every: 7 }
                .occurrence(anchor, 2)
                .unwrap(),
            Some(stamp("2026-03-21T12:00:00Z"))
        );
    }
    #[test]
    fn rejects_zero_invalid_dates_and_all_overflows_without_panicking() {
        for period in [
            CalendarPeriod::Days { every: 0 },
            CalendarPeriod::Months { every: 0 },
            CalendarPeriod::Days { every: u32::MAX },
            CalendarPeriod::Months { every: u32::MAX },
        ] {
            assert!(period.occurrence(0, u32::MAX).is_err());
        }
        assert!(CalendarPeriod::Once.occurrence(i64::MAX, 0).is_err());
    }
}
