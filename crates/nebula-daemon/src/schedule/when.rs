//! When a SCHEDULE is due, on this Mac's clock: each form's due times,
//! and what a beat that finds a run due does with it, by the rules for a
//! run that comes late.
//!
//! Time of day is local time, the zone and its daylight saving rules the
//! system's, read through a [`Zone`] so the tests can hand in one of their
//! own. A local time the clock skips as it springs forward runs as late as
//! the clock skipped, 02:30 at 03:30; one it shows twice as it falls back
//! runs once, the first time.

use nebula_core::{Days, ScheduleWhen, DAYS_ALL};

/// How late a run may be and still count as on time. A time-of-day run
/// later than this is skipped, but a daily schedule's, which runs once
/// late on the day it was due with a note saying so.
pub const LATE_MS: i64 = 10 * 60 * 1000;

const DAY_SECS: i64 = 86_400;
const MINUTE_MS: i64 = 60_000;

/// A clock's offset from UTC.
pub trait Zone {
    /// Seconds east of UTC at the instant `utc`, epoch seconds.
    fn offset(&self, utc: i64) -> i64;
}

/// This Mac's clock, as `localtime_r` reads it.
pub struct Local;

impl Zone for Local {
    fn offset(&self, utc: i64) -> i64 {
        let t = utc as nix::libc::time_t;
        // SAFETY: localtime_r writes only the `tm` it is handed.
        let mut tm: nix::libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { nix::libc::localtime_r(&t, &mut tm) }.is_null() {
            return 0;
        }
        tm.tm_gmtoff
    }
}

/// A local date and time of day, as a clock shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// Monday 0 to Sunday 6.
    pub weekday: u32,
}

/// Days since 1970-01-01 of a date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `days` after 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The local day `at` (epoch ms) falls on, as days since 1970-01-01.
fn local_day(at: i64, zone: &impl Zone) -> i64 {
    let secs = at.div_euclid(1000);
    (secs + zone.offset(secs)).div_euclid(DAY_SECS)
}

fn weekday(day: i64) -> u32 {
    // 1970-01-01 was a Thursday.
    (day + 3).rem_euclid(7) as u32
}

/// `at` (epoch ms) as the clock shows it.
pub fn civil(at: i64, zone: &impl Zone) -> Civil {
    let secs = at.div_euclid(1000);
    let local = secs + zone.offset(secs);
    let day = local.div_euclid(DAY_SECS);
    let into = local.rem_euclid(DAY_SECS);
    let (year, month, d) = civil_from_days(day);
    Civil {
        year,
        month,
        day: d,
        hour: (into / 3600) as u32,
        minute: (into % 3600 / 60) as u32,
        weekday: weekday(day),
    }
}

/// When the clock shows `hour`:`minute` on the local day `day`, epoch ms:
/// the first time when it shows it twice, and as late as the clock skipped
/// when it skips it.
fn instant(day: i64, hour: u32, minute: u32, zone: &impl Zone) -> i64 {
    let naive = day * DAY_SECS + i64::from(hour) * 3600 + i64::from(minute) * 60;
    let before = zone.offset(naive - DAY_SECS);
    let after = zone.offset(naive + DAY_SECS);
    let shows = |utc: i64| utc + zone.offset(utc) == naive;
    let secs = [naive - before, naive - after]
        .into_iter()
        .filter(|&utc| shows(utc))
        .min()
        .unwrap_or(naive - before);
    secs * 1000
}

/// When the clock shows the date and time `civil` names, epoch ms; `None`
/// for a date that is none.
pub fn at_civil(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    zone: &impl Zone,
) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 || hour > 23 || minute > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    (civil_from_days(days) == (year, month, day)).then(|| instant(days, hour, minute, zone))
}

/// The first time `when` is due strictly after `after`, epoch ms: `None`
/// for a once schedule's time already past, and for a time of day on no
/// day.
pub fn due_after(when: &ScheduleWhen, after: i64, zone: &impl Zone) -> Option<i64> {
    match *when {
        ScheduleWhen::Every { minutes } => Some(after + i64::from(minutes.max(1)) * MINUTE_MS),
        ScheduleWhen::Once { at } => (at > after).then_some(at),
        ScheduleWhen::At { hour, minute, days } => {
            let first = local_day(after, zone);
            (first..first + 9)
                .filter(|&d| runs_on(days, weekday(d)))
                .map(|d| instant(d, hour.into(), minute.into(), zone))
                .find(|&at| at > after)
        }
    }
}

fn runs_on(days: Days, weekday: u32) -> bool {
    days & (1 << weekday) != 0
}

/// What a beat that finds a run due at `due` does at `now`, epoch ms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// A run: when it was due, and the note it carries when it is not on
    /// time.
    pub run: Option<(i64, Option<String>)>,
    /// The runs skipped, each with when it was due and why.
    pub skipped: Vec<(i64, String)>,
    /// When it is due next.
    pub next: Option<i64>,
}

/// The rules for a run found due at `now`:
///
/// - On time, ten minutes late at most, it runs.
/// - An interval run later than its period runs once for every run it
///   missed, never a burst, and the next is a period after it.
/// - A time-of-day run more than ten minutes late is skipped, each one
///   missed with it, but a daily schedule's, which runs once late on the
///   day it was due, with a note; one missed on an earlier day is skipped.
/// - A once run more than ten minutes late is skipped.
pub fn decide(when: &ScheduleWhen, due: i64, now: i64, zone: &impl Zone) -> Decision {
    let late = now - due;
    match *when {
        ScheduleWhen::Every { minutes } => {
            let period = i64::from(minutes.max(1)) * MINUTE_MS;
            let missed = late / period;
            let note = (late > LATE_MS).then(|| {
                if missed > 0 {
                    format!(
                        "{} late, once for the {} runs the Mac slept or nebula's daemon was down through",
                        span(late),
                        missed + 1
                    )
                } else {
                    format!("{} late, as the Mac slept or nebula's daemon was down", span(late))
                }
            });
            let next = match due + period {
                next if next > now => next,
                _ => now + period,
            };
            Decision {
                run: Some((due, note)),
                skipped: Vec::new(),
                next: Some(next),
            }
        }
        ScheduleWhen::Once { .. } => {
            if late <= LATE_MS {
                return Decision {
                    run: Some((due, None)),
                    skipped: Vec::new(),
                    next: None,
                };
            }
            Decision {
                run: None,
                skipped: vec![(due, missed_why(late))],
                next: None,
            }
        }
        ScheduleWhen::At { days, .. } => {
            let next = due_after(when, now, zone);
            let mut dues = vec![due];
            while let Some(d) = due_after(when, *dues.last().unwrap(), zone).filter(|&d| d <= now) {
                dues.push(d);
                if dues.len() > crate::schedule::RUNS_KEPT {
                    dues.remove(0);
                }
            }
            let latest = *dues.last().unwrap();
            let latest_late = now - latest;
            let on_time = latest_late <= LATE_MS;
            let catch_up = !on_time
                && days & DAYS_ALL == DAYS_ALL
                && local_day(latest, zone) == local_day(now, zone);
            let run = if on_time {
                Some((latest, None))
            } else if catch_up {
                Some((latest, Some(format!(
                    "Caught up {} late, as the Mac slept or nebula's daemon was down: a daily schedule runs once late on the day it was due",
                    span(latest_late)
                ))))
            } else {
                None
            };
            let ran = if run.is_some() {
                dues.len() - 1
            } else {
                dues.len()
            };
            Decision {
                run,
                skipped: dues[..ran]
                    .iter()
                    .map(|&d| (d, missed_why(now - d)))
                    .collect(),
                next,
            }
        }
    }
}

fn missed_why(late: i64) -> String {
    format!(
        "Missed: the daemon first saw it {} late, as the Mac slept or nebula's daemon was down, and a run more than ten minutes late is skipped",
        span(late)
    )
}

/// A span of time as the run log says it: `12 min`, `2 h 5 min`, `3 days`.
pub fn span(ms: i64) -> String {
    let minutes = ms.max(0) / MINUTE_MS;
    match minutes {
        0..60 => format!("{} min", minutes.max(1)),
        60..1440 if minutes % 60 == 0 => format!("{} h", minutes / 60),
        60..1440 => format!("{} h {} min", minutes / 60, minutes % 60),
        _ if minutes / 1440 == 1 => "1 day".into(),
        _ => format!("{} days", minutes / 1440),
    }
}

/// `at` (epoch ms) on the clock, as `nebula schedule list` prints it:
/// `Mon 2026-11-16 07:30`.
pub fn label(at: i64, zone: &impl Zone) -> String {
    let c = civil(at, zone);
    let day = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][c.weekday as usize];
    format!(
        "{day} {}-{:02}-{:02} {:02}:{:02}",
        c.year, c.month, c.day, c.hour, c.minute
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::DAYS_WEEKDAYS;

    /// Central European time: UTC+1, UTC+2 from the last Sunday of March at
    /// 01:00 UTC to the last Sunday of October at 01:00 UTC. The 2026
    /// changes alone, which is all these tests cross.
    struct Stockholm;

    /// 2026-03-29 01:00 UTC, when 02:00 becomes 03:00.
    const SPRING: i64 = 1_774_746_000;
    /// 2026-10-25 01:00 UTC, when 03:00 becomes 02:00.
    const FALL: i64 = 1_792_890_000;

    impl Zone for Stockholm {
        fn offset(&self, utc: i64) -> i64 {
            if (SPRING..FALL).contains(&utc) {
                7200
            } else {
                3600
            }
        }
    }

    struct Utc;
    impl Zone for Utc {
        fn offset(&self, _: i64) -> i64 {
            0
        }
    }

    fn at(year: i64, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        at_civil(year, month, day, hour, minute, &Stockholm).unwrap()
    }

    fn shown(ms: i64) -> String {
        label(ms, &Stockholm)
    }

    const DAILY_0730: ScheduleWhen = ScheduleWhen::At {
        hour: 7,
        minute: 30,
        days: DAYS_ALL,
    };

    #[test]
    fn the_calendar_holds() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        for days in [-1000, 0, 365, 11_016, 20_408, 20_409, 60_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(weekday(days_from_civil(2026, 11, 16)), 0, "a Monday");
        assert_eq!(at_civil(2026, 2, 29, 7, 0, &Utc), None, "no such day");
        assert_eq!(at_civil(2026, 1, 1, 24, 0, &Utc), None);
        assert_eq!(SPRING, at_civil(2026, 3, 29, 1, 0, &Utc).unwrap() / 1000);
        assert_eq!(FALL, at_civil(2026, 10, 25, 1, 0, &Utc).unwrap() / 1000);
        assert_eq!(shown(at(2026, 11, 16, 7, 30)), "Mon 2026-11-16 07:30");
    }

    #[test]
    fn a_time_of_day_is_due_on_its_days_on_the_local_clock() {
        let monday = at(2026, 11, 16, 7, 30);
        assert_eq!(
            shown(due_after(&DAILY_0730, monday - 1, &Stockholm).unwrap()),
            "Mon 2026-11-16 07:30"
        );
        assert_eq!(
            shown(due_after(&DAILY_0730, monday, &Stockholm).unwrap()),
            "Tue 2026-11-17 07:30",
            "strictly after"
        );
        let weekdays = ScheduleWhen::At {
            hour: 7,
            minute: 30,
            days: DAYS_WEEKDAYS,
        };
        let friday = at(2026, 11, 20, 8, 0);
        assert_eq!(
            shown(due_after(&weekdays, friday, &Stockholm).unwrap()),
            "Mon 2026-11-23 07:30",
            "over the weekend"
        );
        let sundays = ScheduleWhen::At {
            hour: 23,
            minute: 59,
            days: 1 << 6,
        };
        assert_eq!(
            shown(due_after(&sundays, monday, &Stockholm).unwrap()),
            "Sun 2026-11-22 23:59"
        );
    }

    #[test]
    fn a_time_the_clock_skips_springing_forward_runs_as_late_as_it_skipped() {
        let early = ScheduleWhen::At {
            hour: 2,
            minute: 30,
            days: DAYS_ALL,
        };
        let saturday = at(2026, 3, 28, 12, 0);
        let due = due_after(&early, saturday, &Stockholm).unwrap();
        assert_eq!(shown(due), "Sun 2026-03-29 03:30");
        assert_eq!(
            due / 1000,
            SPRING + 1800,
            "01:30 UTC, half an hour after the change"
        );
        // The next day is back on 02:30, a summer one.
        assert_eq!(
            shown(due_after(&early, due, &Stockholm).unwrap()),
            "Mon 2026-03-30 02:30"
        );
        // A time of day outside the gap moves with the clock: 07:30 is an
        // hour sooner in UTC after the change.
        let before = due_after(&DAILY_0730, saturday, &Stockholm).unwrap();
        let after = due_after(&DAILY_0730, before, &Stockholm).unwrap();
        assert_eq!(shown(before), "Sun 2026-03-29 07:30");
        assert_eq!(shown(after), "Mon 2026-03-30 07:30");
        assert_eq!(after - before, 24 * 3_600_000);
        let friday = due_after(&DAILY_0730, at(2026, 3, 27, 8, 0), &Stockholm).unwrap();
        assert_eq!(before - friday, 23 * 3_600_000, "the short day");
    }

    #[test]
    fn a_time_the_clock_shows_twice_falling_back_runs_once_the_first_time() {
        let late_night = ScheduleWhen::At {
            hour: 2,
            minute: 30,
            days: DAYS_ALL,
        };
        let saturday = at(2026, 10, 24, 12, 0);
        let due = due_after(&late_night, saturday, &Stockholm).unwrap();
        assert_eq!(due / 1000, FALL - 1800, "00:30 UTC, still summer time");
        assert_eq!(shown(due), "Sun 2026-10-25 02:30");
        let again = due_after(&late_night, due, &Stockholm).unwrap();
        assert_eq!(
            shown(again),
            "Mon 2026-10-26 02:30",
            "not at the second 02:30"
        );
        assert_eq!(again - due, 25 * 3_600_000, "the long day");
    }

    #[test]
    fn a_run_on_time_runs_and_one_ten_minutes_late_still_does() {
        let due = at(2026, 11, 16, 7, 30);
        let next = at(2026, 11, 17, 7, 30);
        for now in [due, due + 1_000, due + LATE_MS] {
            assert_eq!(
                decide(&DAILY_0730, due, now, &Stockholm),
                Decision {
                    run: Some((due, None)),
                    skipped: Vec::new(),
                    next: Some(next),
                }
            );
        }
    }

    #[test]
    fn a_weekday_run_more_than_ten_minutes_late_is_skipped() {
        let weekdays = ScheduleWhen::At {
            hour: 7,
            minute: 30,
            days: DAYS_WEEKDAYS,
        };
        let due = at(2026, 11, 16, 7, 30);
        let d = decide(&weekdays, due, due + LATE_MS + 1_000, &Stockholm);
        assert_eq!(d.run, None);
        assert_eq!(d.skipped.len(), 1);
        assert_eq!(d.skipped[0].0, due);
        assert!(
            d.skipped[0]
                .1
                .starts_with("Missed: the daemon first saw it 10 min late"),
            "{:?}",
            d.skipped
        );
        assert_eq!(shown(d.next.unwrap()), "Tue 2026-11-17 07:30");
    }

    #[test]
    fn a_daily_run_missed_in_sleep_catches_up_once_on_its_own_day() {
        let due = at(2026, 11, 16, 7, 30);
        let woke = at(2026, 11, 16, 9, 42);
        let d = decide(&DAILY_0730, due, woke, &Stockholm);
        assert_eq!(
            d.run,
            Some((
                due,
                Some(
                    "Caught up 2 h 12 min late, as the Mac slept or nebula's daemon was down: a \
                     daily schedule runs once late on the day it was due"
                        .into()
                )
            ))
        );
        assert!(d.skipped.is_empty());
        assert_eq!(shown(d.next.unwrap()), "Tue 2026-11-17 07:30");
    }

    #[test]
    fn a_sleep_over_days_skips_the_runs_of_the_days_before() {
        // Asleep from Friday night to Monday 09:00: Saturday's and Sunday's
        // are skipped, Monday's catches up.
        let due = at(2026, 11, 14, 7, 30);
        let woke = at(2026, 11, 16, 9, 0);
        let d = decide(&DAILY_0730, due, woke, &Stockholm);
        let (ran, note) = d.run.unwrap();
        assert_eq!(shown(ran), "Mon 2026-11-16 07:30", "today's");
        assert!(note.is_some());
        let skipped: Vec<String> = d.skipped.iter().map(|(at, _)| shown(*at)).collect();
        assert_eq!(skipped, ["Sat 2026-11-14 07:30", "Sun 2026-11-15 07:30"]);
        // Woken the next morning before 07:30: yesterday's is skipped, and
        // today's runs on time.
        let woke = at(2026, 11, 17, 6, 0);
        let d = decide(&DAILY_0730, at(2026, 11, 16, 7, 30), woke, &Stockholm);
        assert_eq!(d.run, None, "not a catch-up on the day after");
        assert_eq!(d.skipped.len(), 1);
        assert_eq!(shown(d.next.unwrap()), "Tue 2026-11-17 07:30");
    }

    #[test]
    fn a_late_interval_run_fires_once_for_all_it_missed() {
        let every = ScheduleWhen::Every { minutes: 15 };
        let due = at(2026, 11, 16, 10, 0);
        // A beat a second late: on time, and the next on the grid.
        let d = decide(&every, due, due + 1_000, &Stockholm);
        assert_eq!(d.run, Some((due, None)));
        assert_eq!(d.next, Some(due + 15 * MINUTE_MS));
        // Asleep for two hours: one run, then one a period after it.
        let woke = due + 2 * 3_600_000 + 5 * MINUTE_MS;
        let d = decide(&every, due, woke, &Stockholm);
        assert_eq!(
            d.run,
            Some((
                due,
                Some(
                    "2 h 5 min late, once for the 9 runs the Mac slept or nebula's daemon was \
                     down through"
                        .into()
                )
            ))
        );
        assert!(d.skipped.is_empty(), "coalesced, not skipped one by one");
        assert_eq!(d.next, Some(woke + 15 * MINUTE_MS));
        // Late by less than ten minutes but past its period: one run, no
        // note, and the next a period on.
        let every5 = ScheduleWhen::Every { minutes: 5 };
        let d = decide(&every5, due, due + 7 * MINUTE_MS, &Stockholm);
        assert_eq!(d.run, Some((due, None)));
        assert_eq!(d.next, Some(due + 12 * MINUTE_MS));
    }

    #[test]
    fn a_once_run_runs_once_or_is_skipped() {
        let due = at(2026, 11, 16, 15, 0);
        let once = ScheduleWhen::Once { at: due };
        assert_eq!(due_after(&once, due - 1, &Stockholm), Some(due));
        assert_eq!(due_after(&once, due, &Stockholm), None);
        let d = decide(&once, due, due + 5_000, &Stockholm);
        assert_eq!((d.run, d.next), (Some((due, None)), None));
        let d = decide(&once, due, due + 3_600_000, &Stockholm);
        assert_eq!(d.run, None);
        assert_eq!(d.skipped.len(), 1);
        assert_eq!(d.next, None, "never again");
    }

    #[test]
    fn spans_read_as_the_run_log_says_them() {
        assert_eq!(span(0), "1 min");
        assert_eq!(span(12 * MINUTE_MS), "12 min");
        assert_eq!(span(120 * MINUTE_MS), "2 h");
        assert_eq!(span(125 * MINUTE_MS), "2 h 5 min");
        assert_eq!(span(1440 * MINUTE_MS), "1 day");
        assert_eq!(span(3 * 1440 * MINUTE_MS), "3 days");
    }
}
