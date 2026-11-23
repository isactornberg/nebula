//! The DAY FILE: a small JSON file another program writes, agentos's
//! `today.json` for one, whose path `today_file` in `config.json` names,
//! with no default. The page shows it in its Today band. The hub looks at
//! it on every board beat and reads it again once the setting or the
//! file's size, time or inode changed, so a file renamed into place is
//! read whole.
//!
//! Version 2 says what each surface shows, word for word; version 1 says
//! what the day holds, and is read as version 2 by `from_v1`, by the rules
//! the page drew it by. A page that draws version 1 still has it as read.

use crate::api::{
    Day, DayLine, DayMeter, DayPill, DayRow, DayStatus, DayTile, DayTone, DayWord, Today,
    TodayNightly, TodayTodos,
};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A file's size, time and inode.
type Stamp = (u64, SystemTime, u64);

/// The day file as last read.
#[derive(Debug, Default)]
pub struct TodayFile {
    /// The setting, and the file's stamp when it was read.
    seen: Option<(String, Option<Stamp>)>,
    /// The file as version 1 has it: none while nothing is set, the file
    /// is missing or unreadable, or it is not version 1.
    pub today: Option<Today>,
    /// Why the setting or the file could not be read as a version 1 day.
    pub problem: Option<String>,
    /// The day as version 2 says it: none while nothing is set, or the
    /// file is missing or unreadable.
    pub day: Option<Day>,
    /// Why the setting or the file could not be read as a day.
    pub day_problem: Option<String>,
}

impl TodayFile {
    /// Read the file `setting` names again when the setting or the file
    /// changed since the last look. Whether it was read again.
    pub fn check(&mut self, setting: &str) -> bool {
        let path = resolve(setting.trim());
        let stamp = path.as_ref().ok().and_then(|p| {
            let meta = std::fs::metadata(p).ok()?;
            Some((meta.len(), meta.modified().ok()?, meta.ino()))
        });
        let seen = Some((setting.to_string(), stamp));
        if seen == self.seen {
            return false;
        }
        let read = match path {
            Err(why) => TodayFile {
                problem: why.clone(),
                day_problem: why,
                ..TodayFile::default()
            },
            Ok(path) => read(&path),
        };
        *self = TodayFile { seen, ..read };
        true
    }
}

/// The file `setting` names: `Err` with why it names none, `None` when it
/// is not set.
fn resolve(setting: &str) -> Result<PathBuf, Option<String>> {
    if setting.is_empty() {
        return Err(None);
    }
    let path = match setting.strip_prefix("~/") {
        Some(rest) => nebula_core::env::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(setting),
    };
    if !path.is_absolute() {
        return Err(Some(format!(
            "today_file is {setting:?}, which is not a path from / or ~/."
        )));
    }
    Ok(path)
}

fn read(path: &Path) -> TodayFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return TodayFile::default();
    };
    let said = |why: &str| {
        Some(format!(
            "The day in {} could not be read: {why}.",
            path.display()
        ))
    };
    match parse(&text) {
        Ok(Read::V1(today)) => TodayFile {
            day: Some(from_v1(&today)),
            today: Some(today),
            ..TodayFile::default()
        },
        Ok(Read::V2(day)) => TodayFile {
            day: Some(day),
            problem: said("it is version 2, and this page draws version 1"),
            ..TodayFile::default()
        },
        Err(why) => TodayFile {
            problem: said(&why),
            day_problem: said(&why),
            ..TodayFile::default()
        },
    }
}

/// A day file as read, by its `version`.
#[derive(Debug, PartialEq)]
enum Read {
    V1(Today),
    V2(Day),
}

fn parse(text: &str) -> Result<Read, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match value.get("version").and_then(|v| v.as_u64()) {
        Some(1) => serde_json::from_value(value)
            .map(Read::V1)
            .map_err(|e| e.to_string()),
        Some(2) => serde_json::from_value(value)
            .map(Read::V2)
            .map_err(|e| e.to_string()),
        Some(v) => Err(format!(
            "it is version {v}, and this nebula reads versions 1 and 2"
        )),
        None => Err("it has no version".into()),
    }
}

/// How many todos the band names; the rest are a count.
const TOP: usize = 3;

/// A version 1 day as version 2 says it: the words, tones and links the
/// page drew it with, the clock left to the page as version 2 leaves it.
/// The one difference is the nightly run's failure, which no longer says
/// when it ran, as the chip beside it does.
pub fn from_v1(t: &Today) -> Day {
    Day {
        at: t.at,
        tiles: t
            .training
            .iter()
            .map(training_tile)
            .chain([todos_tile(&t.todos)])
            .collect(),
        chip: chip(t),
        pills: pills(t),
        status: status(t),
    }
}

/// A field version 1 said: an empty one was drawn as none.
fn said(field: &Option<String>) -> Option<String> {
    field.clone().filter(|s| !s.is_empty())
}

/// `text` ending on a stop, as a sentence.
fn stop(text: &str) -> String {
    if text.ends_with(['.', '!', '?']) {
        text.into()
    } else {
        format!("{text}.")
    }
}

/// Today's session up to its first comma.
fn first(today: &str) -> &str {
    today.split(',').next().unwrap_or_default()
}

fn word(text: impl Into<String>, tone: Option<DayTone>) -> DayWord {
    DayWord {
        text: text.into(),
        tone,
        at: None,
    }
}

fn unread(problem: &str) -> String {
    format!("Could not be read: {}", stop(problem))
}

fn training_tile(t: &crate::api::TodayTraining) -> DayTile {
    let problem = said(&t.problem);
    let main = match said(&t.today) {
        Some(today) => DayLine {
            text: today,
            url: said(&t.url),
            tone: None,
        },
        None => DayLine {
            text: "Nothing today".into(),
            url: None,
            tone: Some(DayTone::Muted),
        },
    };
    let meter = DayMeter {
        value: t.lifts,
        of: t.lifts_per_week,
        text: format!("{} of {} lifts this week", t.lifts, t.lifts_per_week),
    };
    DayTile {
        head: "Training".into(),
        icon: Some("training".into()),
        tone: Some(DayTone::Ok),
        sub: said(&t.block)
            .map(|block| word(block, None))
            .into_iter()
            .collect(),
        main: problem.is_none().then_some(main),
        meter: problem.is_none().then_some(meter),
        problem: problem.as_deref().map(unread),
        rows: Vec::new(),
        more: None,
    }
}

/// `7 due, 5 late`, `7 due, all late`, `7 due` or `nothing due`.
fn todos_line(t: &TodayTodos) -> String {
    match (t.count, t.overdue) {
        (0, _) => "nothing due".into(),
        (count, 0) => format!("{count} due"),
        (count, overdue) => format!("{count} due, {} late", late(count, overdue)),
    }
}

fn late(count: u32, overdue: u32) -> String {
    if overdue == count {
        "all".into()
    } else {
        overdue.to_string()
    }
}

fn todos_tile(t: &TodayTodos) -> DayTile {
    let problem = said(&t.problem);
    let shown = t.top.len().min(TOP);
    let sub = match (t.count, t.overdue) {
        (0, _) | (_, 0) => vec![word(todos_line(t), None)],
        (count, overdue) => vec![
            word(format!("{count} due"), None),
            word(
                format!("{} late", late(count, overdue)),
                Some(DayTone::Late),
            ),
        ],
    };
    let rows = t.top.iter().take(TOP).map(|todo| DayRow {
        text: todo.title.clone(),
        url: said(&todo.url),
        due: said(&todo.due),
    });
    DayTile {
        head: "Todos".into(),
        icon: Some("todos".into()),
        tone: Some(DayTone::Late),
        sub: if problem.is_none() { sub } else { Vec::new() },
        main: None,
        meter: None,
        rows: if problem.is_none() {
            rows.collect()
        } else {
            Vec::new()
        },
        problem: problem.as_deref().map(unread),
        more: (t.count as usize > shown).then(|| DayLine {
            text: format!("+{} more", t.count as usize - shown),
            url: said(&t.url),
            tone: None,
        }),
    }
}

/// The nightly run in a word, `nightly ok` or `nightly failed`.
fn nightly_word(n: &TodayNightly, tone: Option<DayTone>) -> DayWord {
    DayWord {
        text: if n.ok { "nightly ok" } else { "nightly failed" }.into(),
        tone,
        at: Some(n.at),
    }
}

/// Today's training up to its first comma, the todos late or due, the
/// nightly run's word, and `automation off` while it is.
fn chip(t: &Today) -> Vec<DayWord> {
    let mut chip = Vec::new();
    if let Some(training) = &t.training {
        if said(&training.problem).is_some() {
            chip.push(word("training unread", Some(DayTone::Error)));
        } else if let Some(today) = said(&training.today) {
            chip.push(word(first(&today), Some(DayTone::Ok)));
        }
    }
    let todos = &t.todos;
    if said(&todos.problem).is_some() {
        chip.push(word("todos unread", Some(DayTone::Error)));
    } else if todos.overdue > 0 {
        chip.push(word(format!("{} late", todos.overdue), Some(DayTone::Late)));
    } else if todos.count > 0 {
        chip.push(word(format!("{} due", todos.count), Some(DayTone::Muted)));
    }
    if let Some(n) = &t.nightly {
        let tone = if n.ok { DayTone::Muted } else { DayTone::Error };
        chip.push(nightly_word(n, Some(tone)));
    }
    if t.off {
        chip.push(word("automation off", Some(DayTone::Late)));
    }
    chip
}

/// The todos due and today's training, each opening its own link, and how
/// the nightly run and the automation stand.
fn pills(t: &Today) -> Vec<DayPill> {
    let todos = &t.todos;
    let todos_said = match said(&todos.problem) {
        Some(_) => "unread".into(),
        None => todos_line(todos),
    };
    let mut pills = vec![DayPill {
        label: Some("todos".into()),
        url: said(&todos.url),
        dot: None,
        words: vec![word(todos_said, None)],
    }];
    if let Some(training) = &t.training {
        let training_said = match said(&training.problem) {
            Some(_) => Some("unread".into()),
            None => said(&training.today).map(|today| {
                let (lifts, of) = (training.lifts, training.lifts_per_week);
                format!("{} · {lifts}/{of}", first(&today))
            }),
        };
        pills.extend(training_said.map(|text| DayPill {
            label: Some("training".into()),
            url: said(&training.url),
            dot: None,
            words: vec![word(text, None)],
        }));
    }
    let failed = t.nightly.as_ref().is_some_and(|n| !n.ok);
    let mut words = vec![match &t.nightly {
        Some(n) => nightly_word(n, None),
        None => word("no nightly yet", None),
    }];
    if t.off {
        words.push(word("automation off", None));
    }
    pills.push(DayPill {
        label: None,
        url: None,
        dot: Some(if t.off || failed {
            DayTone::Error
        } else {
            DayTone::Ok
        }),
        words,
    });
    pills
}

/// How the nightly run last went, and whether the automation is on.
fn status(t: &Today) -> Vec<DayStatus> {
    let nightly = t.nightly.as_ref().map(|n| DayStatus {
        text: if n.ok { "Nightly ok" } else { "Nightly failed" }.into(),
        tone: Some(if n.ok { DayTone::Ok } else { DayTone::Error }),
        at: Some(n.at),
        problem: said(&n.why)
            .filter(|_| !n.ok)
            .map(|why| format!("The nightly run failed: {}", stop(&why))),
    });
    let automation = DayStatus {
        text: if t.off {
            "Automation off"
        } else {
            "Automation on"
        }
        .into(),
        tone: Some(if t.off { DayTone::Late } else { DayTone::Ok }),
        at: None,
        problem: None,
    };
    nightly.into_iter().chain([automation]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn day(off: bool) -> serde_json::Value {
        json!({
            "version": 1,
            "at": 1_700_000_000_000_i64,
            "todos": {
                "count": 4,
                "overdue": 1,
                "top": [{"title": "Renew the car insurance", "due": "2026-11-10", "url": "https://example.com/1"}],
                "url": "https://example.com/upcoming",
                "problem": null,
            },
            "training": {"block": "Week 2, build", "today": "Day 1 Upper", "url": null, "lifts": 1, "liftsPerWeek": 5, "problem": null},
            "nightly": {"at": 1_699_990_000_000_i64, "ok": true, "why": null},
            "off": off,
            "later": "a field this build does not know",
        })
    }

    /// Written whole and renamed into place, as agentos writes it.
    fn write(path: &Path, text: &str) {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text).unwrap();
        std::fs::rename(&tmp, path).unwrap();
    }

    /// What the page reads from the file: the version 1 day and its
    /// problem, then the version 2 day and its.
    fn shown(file: &TodayFile) -> (bool, Option<&str>, bool, Option<&str>) {
        (
            file.today.is_some(),
            file.problem.as_deref(),
            file.day.is_some(),
            file.day_problem.as_deref(),
        )
    }

    /// Nothing set and a missing file are no day and no problem; a file
    /// is read once, again only when it changes, and served as it holds,
    /// a version 1 day also as version 2; a training that could not be
    /// read says why, and an older file's training, with no such field,
    /// is read as one that could; a version 2 day is served as version 2
    /// alone, and one that is no day of either version says why; a file
    /// gone is no day again.
    #[test]
    fn the_day_file_is_read_when_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("today.json");
        let setting = path.to_str().unwrap();
        let mut file = TodayFile::default();

        assert!(file.check(""));
        assert_eq!(shown(&file), (false, None, false, None));
        assert!(!file.check(""), "nothing changed");

        assert!(file.check("today.json"));
        let relative = r#"today_file is "today.json", which is not a path from / or ~/."#;
        assert_eq!(shown(&file), (false, Some(relative), false, Some(relative)));

        assert!(file.check(setting));
        assert_eq!(shown(&file), (false, None, false, None), "no file yet");

        write(&path, &day(false).to_string());
        assert!(file.check(setting));
        let today = file.today.clone().expect("a day");
        assert_eq!(shown(&file), (true, None, true, None));
        assert_eq!(file.day, Some(from_v1(&today)));
        assert_eq!(today.todos.top[0].title, "Renew the car insurance");
        assert_eq!(today.training.as_ref().unwrap().lifts_per_week, 5);
        assert!(!file.check(setting), "read once until it changes");
        let mut served = day(false);
        let served = served.as_object_mut().unwrap();
        served.remove("version");
        served.remove("later");
        assert_eq!(serde_json::to_value(&today).unwrap(), json!(served));

        write(&path, &day(true).to_string());
        assert!(file.check(setting));
        assert!(file.today.as_ref().unwrap().off);

        let training = |file: &TodayFile| file.today.as_ref().unwrap().training.clone().unwrap();
        let mut unread = day(false);
        unread["training"] = json!({"block": null, "today": null, "url": null, "lifts": 0, "liftsPerWeek": 5, "problem": "the lifting log refused the key"});
        write(&path, &unread.to_string());
        assert!(file.check(setting));
        assert_eq!(
            training(&file).problem.as_deref(),
            Some("the lifting log refused the key")
        );
        assert_eq!(training(&file).today, None);

        let mut older = day(false);
        older["training"].as_object_mut().unwrap().remove("problem");
        write(&path, &older.to_string());
        assert!(file.check(setting));
        assert_eq!(training(&file).problem, None);
        assert_eq!(training(&file).today.as_deref(), Some("Day 1 Upper"));

        let v2 = json!({
            "version": 2,
            "at": 5,
            "tiles": [{"head": "Reviews", "tone": "late", "sub": [{"text": "2 waiting"}], "main": null, "later": 1}],
            "chip": [{"text": "2 reviews", "tone": "late"}],
            "later": "a field this build does not know",
        });
        write(&path, &v2.to_string());
        assert!(file.check(setting));
        let unseen = format!(
            "The day in {} could not be read: it is version 2, and this page draws version 1.",
            path.display()
        );
        assert_eq!(shown(&file), (false, Some(unseen.as_str()), true, None));
        assert_eq!(
            serde_json::to_value(file.day.as_ref().unwrap()).unwrap(),
            json!({
                "at": 5,
                "tiles": [{"head": "Reviews", "tone": "late", "sub": [{"text": "2 waiting"}]}],
                "chip": [{"text": "2 reviews", "tone": "late"}],
                "pills": [],
                "status": [],
            }),
            "served as read, with no field that is empty or none"
        );

        let refused = |file: &TodayFile, why: &str| {
            let said = format!("The day in {} could not be read: {why}.", path.display());
            assert_eq!(
                shown(file),
                (false, Some(said.as_str()), false, Some(said.as_str()))
            );
        };
        for (text, why) in [
            (r#"{"version": 1, "at": 5}"#, "missing field `todos`"),
            (r#"{"version": 2}"#, "missing field `at`"),
            (
                r#"{"version": 2, "at": 5, "status": [{"text": "Backup", "tone": "red"}]}"#,
                "unknown variant `red`, expected one of `ok`, `late`, `error`, `muted`",
            ),
            (
                r#"{"version": 3, "at": 5}"#,
                "it is version 3, and this nebula reads versions 1 and 2",
            ),
            (r#"{"at": 5}"#, "it has no version"),
            ("{", "EOF while parsing an object at line 1 column 1"),
        ] {
            write(&path, text);
            assert!(file.check(setting));
            refused(&file, why);
        }

        std::fs::remove_file(&path).unwrap();
        assert!(file.check(setting));
        assert_eq!(shown(&file), (false, None, false, None));
    }

    /// The days the page's day-parity test draws are days this build
    /// serves as they stand, beside their version, each one also as the
    /// version 2 day beside it under `v2/`, which reads as itself; the one
    /// it refuses says what that test shows for it.
    #[test]
    fn the_page_s_test_days_are_read_as_they_stand() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("app/src/testdata");
        let held = |path: &Path| {
            let text = std::fs::read_to_string(path).unwrap();
            let mut held: serde_json::Value = serde_json::from_str(&text).unwrap();
            held.as_object_mut().unwrap().remove("version");
            (parse(&text), held)
        };
        let mut read = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if !path.is_file() {
                continue;
            }
            read += 1;
            let (v1, v1_held) = held(&path);
            if path.file_stem().unwrap() == "problem" {
                assert_eq!(v1, Err("missing field `todos`".into()));
                continue;
            }
            let Ok(Read::V1(today)) = v1 else {
                panic!("{} is no version 1 day", path.display())
            };
            assert_eq!(
                serde_json::to_value(&today).unwrap(),
                v1_held,
                "{}",
                path.display()
            );

            let golden = dir.join("v2").join(path.file_name().unwrap());
            let (v2, v2_held) = held(&golden);
            let day = from_v1(&today);
            assert_eq!(
                serde_json::to_value(&day).unwrap(),
                v2_held,
                "{}",
                golden.display()
            );
            assert_eq!(v2, Ok(Read::V2(day)), "{}", golden.display());
        }
        assert_eq!(read, 10);
        assert_eq!(std::fs::read_dir(dir.join("v2")).unwrap().count(), 9);
    }
}
