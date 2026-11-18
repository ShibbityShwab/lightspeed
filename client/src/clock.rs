//! Local wall-clock helpers for the ML time features.
//!
//! The ML inputs `time_of_day` and `day_of_week` are read from the **local**
//! wall clock, both when the collector records a live measurement
//! ([`crate::ml::collector`]) and when features are extracted for inference
//! ([`crate::ml::features`] and [`crate::route::selector`]). The trainer and
//! the collector must agree on the mapping or the model silently sees a
//! shifted feature, so the conversion lives in exactly one place.
//!
//! The mapping matches what the engine produced before the `time` migration
//! (RUSTSEC-2026-0543):
//!
//! - `time_of_day` is the local hour, 0-23 (the old `%H` format);
//! - `day_of_week` is 0-6 with **Monday = 0** (the old ISO weekday `%u` minus
//!   one); [`time::Weekday::number_days_from_monday`] is that value directly.
//!
//! Unlike the previous clock crate, `time::OffsetDateTime::now_local()` reports
//! an indeterminate system UTC offset as an error instead of panicking. A
//! clock read must not panic a running tunnel, so the documented fallback is
//! **UTC** (offset zero) - which is also what the previous crate used on Unix
//! when no local zone could be found. [`resolve_local`] isolates that decision
//! so the fallback path can be tested deterministically.

use time::error::IndeterminateOffset;
use time::OffsetDateTime;

/// The current local wall clock, falling back to UTC when the system's UTC
/// offset cannot be determined.
///
/// This is the only "now" the ML time features read; see the module docs for
/// why the fallback is UTC.
pub(crate) fn now_local_or_utc() -> OffsetDateTime {
    resolve_local(OffsetDateTime::now_local())
}

/// Resolve a [`OffsetDateTime::now_local`] result, falling back to UTC.
///
/// Split out from [`now_local_or_utc`] so the fallback path is unit-testable:
/// the live call cannot be made to fail on demand.
fn resolve_local(local: Result<OffsetDateTime, IndeterminateOffset>) -> OffsetDateTime {
    match local {
        Ok(now) => now,
        Err(_) => OffsetDateTime::now_utc(),
    }
}

/// Hour of day (0-23) and day of week (0-6, Monday = 0) of `at`, in `at`'s own
/// offset - i.e. local wall-clock values when `at` came from
/// [`now_local_or_utc`].
pub(crate) fn hour_and_weekday(at: OffsetDateTime) -> (u8, u8) {
    (at.hour(), at.weekday().number_days_from_monday())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{datetime, offset};

    #[test]
    fn hour_and_weekday_follow_the_offset() {
        // 2026-11-16 is a Monday. 23:30 UTC is still Monday in UTC ...
        let monday_utc = datetime!(2026-11-16 23:30 UTC);
        assert_eq!(hour_and_weekday(monday_utc), (23, 0));

        // ... but 01:30 on Tuesday at UTC+01:00: the hour wraps and the day
        // advances. This is the case that would silently shift the feature if
        // the offset were dropped.
        let tuesday_tokyo = monday_utc.to_offset(offset!(+9));
        assert_eq!(hour_and_weekday(tuesday_tokyo), (8, 1));

        // A negative offset pulls the same instant back into Monday afternoon.
        let monday_los_angeles = monday_utc.to_offset(offset!(-8));
        assert_eq!(hour_and_weekday(monday_los_angeles), (15, 0));
    }

    #[test]
    fn weekday_mapping_is_monday_zero_through_sunday_six() {
        // The legacy ISO-weekday semantics that must be preserved: `%u` is
        // 1..=7 starting at Monday, and the feature stored `%u - 1`.
        let monday = datetime!(2026-11-16 12:00 UTC);
        for (days, expected) in [(0u64, 0u8), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6)] {
            let at = monday + time::Duration::days(days as i64);
            assert_eq!(
                hour_and_weekday(at).1,
                expected,
                "{days} day(s) after Monday must map to {expected}"
            );
        }
        assert_eq!(hour_and_weekday(datetime!(2026-11-22 12:00 UTC)).1, 6);
    }

    #[test]
    fn local_hour_is_zero_padded_as_a_number() {
        // `%H` parsed to `u8` yields 0 at midnight (the old code's string round
        // trip is gone; the numeric value must be identical).
        assert_eq!(hour_and_weekday(datetime!(2026-11-16 0:00 UTC)), (0, 0));
        assert_eq!(hour_and_weekday(datetime!(2026-11-16 9:05 UTC)), (9, 0));
    }

    #[test]
    fn resolve_local_falls_back_to_utc_when_the_offset_is_indeterminate() {
        let before = OffsetDateTime::now_utc();
        let resolved = resolve_local(Err(IndeterminateOffset));
        let after = OffsetDateTime::now_utc();

        assert!(
            resolved.offset().is_utc(),
            "the fallback must be a UTC instant, got offset {:?}",
            resolved.offset()
        );
        assert!(
            resolved >= before && resolved <= after,
            "the fallback must be the current instant"
        );
    }

    #[test]
    fn resolve_local_keeps_a_determined_local_time() {
        let local = datetime!(2026-11-16 23:30 +5:30);
        let resolved = resolve_local(Ok(local));
        assert_eq!(resolved, local);
        assert_eq!(hour_and_weekday(resolved), (23, 0));
    }
}
