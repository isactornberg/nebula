//! How often Claude Code used each skill, and when last, read from its
//! transcripts: a JSONL file for each session in `<config>/projects/<folder>`,
//! and its subagents' in the folders beside it. A use is a `Skill` tool call
//! or a slash command, counted when its name is one Claude loads a skill
//! under, so `/model` and `/clear` are not. Codex and Cursor are not read.
//!
//! A transcript last written before the window holds no use in it and is
//! never opened. The rest are read a line at a time, and only a line holding
//! a tool call's or a command's marker is parsed. What each file held is
//! kept by its path, size and modification time, so the next read opens
//! only the files that changed.

use super::{Places, Scan};
use crate::api::{self, SkillCli as Cli};
use memchr::memmem::Finder;
use serde::de::{Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How many days back use is counted.
pub const DAYS: u32 = 30;

/// What a line holding a `Skill` tool call has in it, and a slash
/// command's.
const CALL: &str = r#""name":"Skill""#;
const COMMAND: &str = "<command-name>";

/// One use, as a transcript has it.
#[derive(Debug, Clone)]
struct Use {
    /// The tool call's id, or the command line's uuid: a resumed session's
    /// transcript can repeat the lines of the one it resumed.
    key: String,
    name: String,
    /// Epoch milliseconds.
    at: i64,
    cwd: PathBuf,
}

/// The uses each transcript held when it was read, by its path.
#[derive(Default)]
pub struct Transcripts(HashMap<PathBuf, Held>);

struct Held {
    len: u64,
    modified: SystemTime,
    uses: Vec<Use>,
}

impl Transcripts {
    /// Every use in the transcripts in `dir` written since `since`, opening
    /// only the files that changed since the last read, several at once. A
    /// file no longer there, or written before `since`, is forgotten.
    fn read(&mut self, dir: &Path, since: SystemTime) -> impl Iterator<Item = &Use> {
        let mut files = Vec::new();
        transcripts_in(dir, &mut files);
        let mut kept = HashMap::new();
        let mut changed = Vec::new();
        for path in files {
            let Some((len, modified)) = fs::metadata(&path)
                .ok()
                .and_then(|m| Some((m.len(), m.modified().ok()?)))
            else {
                continue;
            };
            if modified < since {
                continue;
            }
            match self.0.remove(&path) {
                Some(held) if held.len == len && held.modified == modified => {
                    kept.insert(path, held);
                }
                _ => changed.push((path, len, modified)),
            }
        }
        let finders = (Finder::new(CALL), Finder::new(COMMAND));
        let next = AtomicUsize::new(0);
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
        std::thread::scope(|s| {
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    s.spawn(|| {
                        let mut read = Vec::new();
                        while let Some((path, len, modified)) =
                            changed.get(next.fetch_add(1, Relaxed))
                        {
                            let uses = uses_in(path, &finders);
                            read.push((
                                path.clone(),
                                Held {
                                    len: *len,
                                    modified: *modified,
                                    uses,
                                },
                            ));
                        }
                        read
                    })
                })
                .collect();
            for worker in workers {
                kept.extend(worker.join().unwrap_or_default());
            }
        });
        self.0 = kept;
        self.0.values().flat_map(|h| &h.uses)
    }
}

/// Every transcript in `dir`, Claude's projects folder: each session's in
/// its project's folder, and its subagents' in `<session>/subagents`, a
/// workflow's in a folder under that. The rest of a session's folder (its
/// tool results) is not walked.
fn transcripts_in(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(projects) = fs::read_dir(dir) else {
        return;
    };
    for project in projects.flatten() {
        let Ok(entries) = fs::read_dir(project.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => jsonl_in(&path.join("subagents"), 2, out),
                Ok(t) if t.is_file() && is_jsonl(&path) => out.push(path),
                _ => {}
            }
        }
    }
}

/// Every `.jsonl` file in `dir`, and in the folders under it `depth` deep.
fn jsonl_in(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() && depth > 0 => jsonl_in(&path, depth - 1, out),
            Ok(t) if t.is_file() && is_jsonl(&path) => out.push(path),
            _ => {}
        }
    }
}

fn is_jsonl(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "jsonl")
}

/// The uses in the transcript at `path`, each line looked at for
/// `finders`' markers before it is parsed.
fn uses_in(path: &Path, (call, command): &(Finder, Finder)) -> Vec<Use> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut reader = BufReader::with_capacity(1 << 16, file);
    let mut line = Vec::new();
    let mut out = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if call.find(&line).is_none() && command.find(&line).is_none() {
            continue;
        }
        if let Ok(parsed) = serde_json::from_slice::<Line>(&line) {
            parsed.uses(&mut out);
        }
    }
    out
}

/// A transcript line, as much of it as a use needs.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Line {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    is_meta: bool,
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    cwd: PathBuf,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    content: Content,
}

/// A message's content, as much as a use needs: the command a user's line
/// ran, or an assistant's `Skill` calls.
#[derive(Default)]
enum Content {
    #[default]
    Other,
    Command(String),
    Calls(Vec<(String, String)>),
}

#[derive(Deserialize)]
struct Part {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    input: Option<Input>,
}

#[derive(Deserialize)]
struct Input {
    skill: Option<String>,
}

impl<'de> Deserialize<'de> for Content {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Content, D::Error> {
        struct Of;
        impl<'de> Visitor<'de> for Of {
            type Value = Content;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a message's content")
            }

            /// A user's line that ran a slash command opens with its tags:
            /// `<command-message>…<command-name>/name</command-name>`.
            fn visit_str<E>(self, text: &str) -> Result<Content, E> {
                let name = text
                    .trim_start()
                    .starts_with("<command-")
                    .then(|| text.split_once(COMMAND)?.1.split_once("</command-name>"))
                    .flatten()
                    .map(|(name, _)| Content::Command(name.trim().to_string()));
                Ok(name.unwrap_or_default())
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut parts: A) -> Result<Content, A::Error> {
                let mut calls = Vec::new();
                while let Some(p) = parts.next_element::<Part>()? {
                    if p.kind == "tool_use" && p.name == "Skill" {
                        if let Some(skill) = p.input.and_then(|i| i.skill) {
                            calls.push((p.id, skill));
                        }
                    }
                }
                Ok(Content::Calls(calls))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Content, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Content::Other)
            }

            fn visit_unit<E>(self) -> Result<Content, E> {
                Ok(Content::Other)
            }
        }
        d.deserialize_any(Of)
    }
}

impl Line {
    fn uses(self, out: &mut Vec<Use>) {
        let Some(at) = epoch_ms(&self.timestamp) else {
            return;
        };
        let content = self.message.map(|m| m.content).unwrap_or_default();
        let found: Vec<(String, String)> = match (self.kind.as_str(), content) {
            ("assistant", Content::Calls(calls)) => calls,
            // Claude Code's own lines (a skill's text it adds) are meta.
            ("user", Content::Command(name)) if !self.is_meta => vec![(self.uuid, name)],
            _ => return,
        };
        out.extend(found.into_iter().map(|(key, name)| Use {
            key,
            name: name.strip_prefix('/').unwrap_or(&name).to_string(),
            at,
            cwd: self.cwd.clone(),
        }));
    }
}

/// `2026-11-02T09:30:00.000Z` as epoch milliseconds, to the second.
pub(super) fn epoch_ms(iso: &str) -> Option<i64> {
    let num = |from: usize, len: usize| iso.get(from..from + len)?.parse::<u32>().ok();
    let b = iso.as_bytes();
    if (b.get(4), b.get(7), b.get(10)) != (Some(&b'-'), Some(&b'-'), Some(&b'T')) {
        return None;
    }
    let (y, m, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let secs = num(11, 2)? * 3600 + num(14, 2)? * 60 + num(17, 2)?;
    let days = nebula_core::clock::days_from_civil(i32::try_from(y).ok()?, m, d);
    Some((days * 86_400 + i64::from(secs)) * 1000)
}

fn ms(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The project a session at `cwd` is in: the one whose root checkout, or
/// one of its worktrees, is the nearest folder above it.
fn project_of(cwd: &Path, places: &Places) -> Option<usize> {
    places
        .projects
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            std::iter::once(&p.root)
                .chain(&p.worktrees)
                .map(move |d| (i, d))
        })
        .filter(|(_, dir)| cwd.starts_with(dir))
        .max_by_key(|(_, dir)| dir.components().count())
        .map(|(i, _)| i)
}

/// How often each skill of `scan` was used in the [`DAYS`] before `now`,
/// by what `transcripts` read. A name several skills go by is the one's
/// that Claude loads in that session's project.
pub fn usage(scan: &Scan, transcripts: &mut Transcripts, now: SystemTime) -> api::SkillUsage {
    let places = &scan.places;
    let since = now
        .checked_sub(Duration::from_secs(u64::from(DAYS) * 86_400))
        .unwrap_or(UNIX_EPOCH);
    let mut names: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
    for (s, found) in scan.found.iter().enumerate() {
        for (f, find) in found.finds.iter().enumerate() {
            if find.cli == Cli::Claude {
                let name = found.skill.loads[f].name.as_str();
                names.entry(name).or_default().push((s, f));
            }
        }
    }
    let mut seen = HashSet::new();
    // By skill: how many uses, the last one's time, and its project.
    let mut tally: HashMap<usize, (u32, i64, Option<usize>)> = HashMap::new();
    for u in transcripts.read(&places.claude.join("projects"), since) {
        if u.at < ms(since) || (!u.key.is_empty() && !seen.insert(u.key.as_str())) {
            continue;
        }
        let Some(skills) = names.get(u.name.as_str()) else {
            continue;
        };
        let project = project_of(&u.cwd, places);
        let (s, _) = skills
            .iter()
            .find(|&&(s, f)| {
                let found = &scan.found[s];
                found.skill.loads[f].shadowed_by.is_none()
                    && found.finds[f].seen_in(project, places)
            })
            .unwrap_or(&skills[0]);
        let t = tally.entry(*s).or_insert((0, i64::MIN, None));
        t.0 += 1;
        if u.at > t.1 {
            (t.1, t.2) = (u.at, project);
        }
    }
    let mut used: Vec<api::SkillUse> = tally
        .into_iter()
        .map(|(s, (count, last, project))| api::SkillUse {
            id: scan.found[s].skill.id.clone(),
            count,
            last,
            project: project.map(|p| places.projects[p].name.clone()),
        })
        .collect();
    used.sort_by(|a, b| (b.count, b.last, &a.id).cmp(&(a.count, a.last, &b.id)));
    api::SkillUsage { days: DAYS, used }
}
