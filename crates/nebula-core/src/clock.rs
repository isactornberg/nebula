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

/// An RFC 3339 stamp in UTC, as GitHub and Linear write one
/// (`2026-11-12T10:42:13.123Z`, the fraction optional), as epoch ms. None
/// for anything else, an offset other than `Z` included.
pub fn utc_ms(stamp: &str) -> Option<i64> {
    let (date, time) = stamp.strip_suffix('Z')?.split_once('T')?;
    let mut ymd = date.split('-').map(digits);
    let (y, m, d) = (ymd.next()??, ymd.next()??, ymd.next()??);
    let (clock, fraction) = time.split_once('.').unwrap_or((time, ""));
    let mut hms = clock.split(':').map(digits);
    let (h, min, s) = (hms.next()??, hms.next()??, hms.next()??);
    let valid = ymd.next().is_none()
        && hms.next().is_none()
        && (1..=12).contains(&m)
        && (1..=31).contains(&d)
        && h < 24
        && min < 60
        && s < 61;
    if !valid || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ms: i64 = format!("{fraction:0<3}")[..3].parse().ok()?;
    let secs =
        days_from_civil(i32::try_from(y).ok()?, m, d) * 86_400 + i64::from(h * 3600 + min * 60 + s);
    Some(secs * 1000 + ms)
}

/// `s` as a number when it is digits and nothing else: `parse` alone takes
/// a leading `+`.
fn digits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
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

    #[test]
    fn a_utc_stamp_reads_as_epoch_ms() {
        assert_eq!(utc_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(utc_ms("1970-01-02T00:00:01.5Z"), Some(86_401_500));
        assert_eq!(
            utc_ms("2026-11-12T10:42:13.123Z"),
            Some((days_from_civil(2026, 11, 12) * 86_400 + 38_533) * 1000 + 123)
        );
        for bad in [
            "",
            "2026-11-12",
            "2026-11-12T10:42:13",
            "2026-11-12T10:42:13+01:00",
            "2026-13-12T10:42:13Z",
            "2026-11-12T10:42Z",
            "2026-11-12T10:42:13.x1Z",
            "2026-11-12T+0:42:13Z",
        ] {
            assert_eq!(utc_ms(bad), None, "{bad}");
        }
    }
}
