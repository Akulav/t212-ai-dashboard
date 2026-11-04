//! NYSE regular-session clock: "time until the opening / closing bell".
//!
//! Regular hours are 09:30–16:00 America/New_York, Monday–Friday, excluding
//! exchange holidays. DST is handled by `chrono-tz`. Early-close days
//! (13:00) are not modelled yet.

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Utc, Weekday};
use chrono_tz::America::New_York;

/// Full-day NYSE closures (`year, month, day`). Extend yearly.
const HOLIDAYS: &[(i32, u32, u32)] = &[
    // 2026
    (2026, 1, 1),
    (2026, 1, 19),
    (2026, 2, 16),
    (2026, 4, 3),
    (2026, 5, 25),
    (2026, 6, 19),
    (2026, 7, 3),
    (2026, 9, 7),
    (2026, 11, 26),
    (2026, 12, 25),
    // 2027
    (2027, 1, 1),
    (2027, 1, 18),
    (2027, 2, 15),
    (2027, 3, 26),
    (2027, 5, 31),
    (2027, 6, 18),
    (2027, 7, 5),
    (2027, 9, 6),
    (2027, 11, 25),
    (2027, 12, 24),
];

/// Whether the regular session is in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketPhase {
    /// Between the opening and closing bell.
    Open,
    /// Outside regular hours.
    Closed,
}

/// Current phase and the next bell.
#[derive(Debug, Clone, Copy)]
pub struct MarketClock {
    /// Current phase.
    pub phase: MarketPhase,
    /// Instant of the next bell (close if open, open if closed).
    pub next_bell: DateTime<Utc>,
    /// Time remaining until [`Self::next_bell`].
    pub remaining: chrono::Duration,
}

impl MarketClock {
    /// `"2d 04h 11m"` or `"03:12:09"`.
    pub fn countdown(&self) -> String {
        let secs = self.remaining.num_seconds().max(0);
        let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
        if d > 0 {
            format!("{d}d {h:02}h {m:02}m")
        } else {
            format!("{h:02}:{m:02}:{s:02}")
        }
    }
}

/// `true` if NYSE holds a regular session on `date`.
pub fn is_trading_day(date: NaiveDate) -> bool {
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
        && !HOLIDAYS.contains(&(date.year(), date.month(), date.day()))
}

fn at(date: NaiveDate, h: u32, m: u32) -> DateTime<Utc> {
    let naive = date.and_time(NaiveTime::from_hms_opt(h, m, 0).expect("valid time"));
    New_York
        .from_local_datetime(&naive)
        .earliest()
        .expect("09:30/16:00 never fall in a DST gap")
        .with_timezone(&Utc)
}

/// Computes the NYSE clock at `now`.
pub fn nyse_clock(now: DateTime<Utc>) -> MarketClock {
    let today = now.with_timezone(&New_York).date_naive();
    let make = |phase, next_bell: DateTime<Utc>| MarketClock {
        phase,
        next_bell,
        remaining: next_bell - now,
    };

    if is_trading_day(today) {
        let (open, close) = (at(today, 9, 30), at(today, 16, 0));
        if now < open {
            return make(MarketPhase::Closed, open);
        }
        if now < close {
            return make(MarketPhase::Open, close);
        }
    }
    let mut day = today.succ_opt().expect("date in range");
    while !is_trading_day(day) {
        day = day.succ_opt().expect("date in range");
    }
    make(MarketPhase::Closed, at(day, 9, 30))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn et(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        New_York
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn open_during_session() {
        let c = nyse_clock(et(2026, 11, 4, 10, 0)); // Wednesday
        assert_eq!(c.phase, MarketPhase::Open);
        assert_eq!(c.remaining.num_minutes(), 6 * 60);
    }

    #[test]
    fn weekend_rolls_to_monday() {
        let c = nyse_clock(et(2026, 11, 7, 12, 0)); // Saturday
        assert_eq!(c.phase, MarketPhase::Closed);
        assert_eq!(c.next_bell, et(2026, 11, 9, 9, 30));
    }

    #[test]
    fn skips_thanksgiving() {
        let c = nyse_clock(et(2026, 11, 25, 17, 0));
        assert_eq!(c.next_bell, et(2026, 11, 27, 9, 30));
    }

    #[test]
    fn countdown_format() {
        let c = nyse_clock(et(2026, 11, 4, 15, 58));
        assert_eq!(c.countdown(), "00:02:00");
    }
}
