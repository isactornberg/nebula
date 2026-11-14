//! The DAY FILE: a small JSON file another program writes, agentos's
//! `today.json` for one, whose path `today_file` in `config.json` names,
//! with no default. The page shows it in its Today band. The hub looks at
//! it on every board beat and reads it again once the setting or the
//! file's size, time or inode changed, so a file renamed into place is
//! read whole.

use crate::api::Today;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The `version` of the file this build reads.
const VERSION: u64 = 1;

/// A file's size, time and inode.
type Stamp = (u64, SystemTime, u64);

/// The day file as last read.
#[derive(Debug, Default)]
pub struct TodayFile {
    /// The setting, and the file's stamp when it was read.
    seen: Option<(String, Option<Stamp>)>,
    /// None while nothing is set, or the file is missing or unreadable.
    pub today: Option<Today>,
    /// Why the setting or the file could not be read as a day.
    pub problem: Option<String>,
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
        self.seen = seen;
        (self.today, self.problem) = match path {
            Err(why) => (None, why),
            Ok(path) => read(&path),
        };
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

fn read(path: &Path) -> (Option<Today>, Option<String>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (None, None);
    };
    match parse(&text) {
        Ok(today) => (Some(today), None),
        Err(why) => (
            None,
            Some(format!(
                "The day in {} could not be read: {why}.",
                path.display()
            )),
        ),
    }
}

fn parse(text: &str) -> Result<Today, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match value.get("version").and_then(|v| v.as_u64()) {
        Some(VERSION) => serde_json::from_value(value).map_err(|e| e.to_string()),
        Some(v) => Err(format!(
            "it is version {v}, and this nebula reads version {VERSION}"
        )),
        None => Err("it has no version".into()),
    }
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
            "quiet": [{"name": "Garden shed", "brain": "home", "days": 41, "url": null}],
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

    /// Nothing set and a missing file are no day and no problem; a file
    /// is read once, again only when it changes, and served as it holds;
    /// a training that could not be read says why, and an older file's
    /// training, with no such field, is read as one that could; one that
    /// is not a day of this version says why, and a file gone is no day
    /// again.
    #[test]
    fn the_day_file_is_read_when_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("today.json");
        let setting = path.to_str().unwrap();
        let mut file = TodayFile::default();

        assert!(file.check(""));
        assert_eq!((&file.today, &file.problem), (&None, &None));
        assert!(!file.check(""), "nothing changed");

        assert!(file.check("today.json"));
        assert_eq!(
            file.problem.as_deref(),
            Some(r#"today_file is "today.json", which is not a path from / or ~/."#)
        );

        assert!(file.check(setting));
        assert_eq!((&file.today, &file.problem), (&None, &None), "no file yet");

        write(&path, &day(false).to_string());
        assert!(file.check(setting));
        let today = file.today.clone().expect("a day");
        assert_eq!(file.problem, None);
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

        write(&path, r#"{"version": 1, "at": 5}"#);
        assert!(file.check(setting));
        assert_eq!(file.today, None);
        assert_eq!(
            file.problem,
            Some(format!(
                "The day in {} could not be read: missing field `todos`.",
                path.display()
            ))
        );

        write(&path, r#"{"version": 2}"#);
        file.check(setting);
        assert!(file
            .problem
            .as_deref()
            .unwrap()
            .ends_with("it is version 2, and this nebula reads version 1."));

        std::fs::remove_file(&path).unwrap();
        assert!(file.check(setting));
        assert_eq!((&file.today, &file.problem), (&None, &None));
    }
}
