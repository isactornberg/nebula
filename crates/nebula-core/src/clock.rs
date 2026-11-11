//! Wall-clock stamps, in the units the database and the on-disk caches
//! store them in.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch — the stamp every row's `created_at`
/// and `status_changed_at` carries, so the daemon's and the TUI's compare.
/// A clock set before 1970 reads as 0.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Whole seconds since the Unix epoch — the on-disk caches' stamp. A clock
/// set before 1970 reads as 0.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Days since the Unix epoch of a proleptic Gregorian date (Howard
/// Hinnant's `days_from_civil`).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_from_civil_counts_from_the_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(
            days_from_civil(2026, 9, 24) - days_from_civil(2026, 8, 22),
            33
        );
    }
}
