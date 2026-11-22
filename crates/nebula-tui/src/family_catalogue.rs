//! A MODEL / EFFORT catalogue read off an agent CLI's own model listing,
//! for CLIs whose model ids are a *family* plus an effort suffix
//! (`claude-opus-5-thinking-high`, `gemini-4-neon-high`). nebula keeps the
//! family as the MODEL and the suffix as the EFFORT, so a family offers only
//! the efforts it ships. Cursor's (`cursor_catalogue`) and Antigravity's
//! (`agy_catalogue`) are the two; each brings its listing, its parser and
//! its seed, and this module the grouping, the installed view, and the
//! cache in the DATA DIR that a background refresh keeps a day fresh.
//!
//! Lists are handed out as `&'static` slices — the same shape as
//! `CLAUDE_MODELS` — by leaking each installed catalogue once; a refresh happens
//! at most once per TUI process.

use nebula_core::clock::now_secs;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::DEFAULT_CHOICE;

/// Effort words in pick order; `-fast` twins follow their base.
const EFFORT_ORDER: &[&str] = &[
    "none",
    "low",
    "medium",
    "high",
    "xhigh",
    "extra-high",
    "max",
];
const FAST: &str = "fast";

/// How long a cached listing is trusted before a refresh.
pub const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// One model family and the effort suffixes it ships. `efforts` leads with
/// [`DEFAULT_CHOICE`] only when the bare family id exists (`gpt-5.3-codex`,
/// `auto`); most families have no bare id, so "default" for them is the
/// [`Catalogue::fallback_effort`] the launch substitutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Family {
    pub name: String,
    pub efforts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Catalogue {
    pub families: Vec<Family>,
}

/// Split a catalogue id into (family, suffix): the trailing `-fast`, then a
/// trailing effort word (`extra-high` counts as one), are the suffix; the
/// rest is the family. `claude-4.6-sonnet-medium-thinking` ends in neither,
/// so it is a bare family of its own — ugly, but it launches.
pub fn split_id(id: &str) -> (String, String) {
    let id = id.trim();
    let (base, fast) = match id.strip_suffix("-fast") {
        Some(base) => (base, true),
        None => (id, false),
    };
    let mut family = base;
    let mut effort = "";
    // Longest word first: `-extra-high` must win over its `-high` tail.
    let mut words = EFFORT_ORDER.to_vec();
    words.sort_by_key(|w| std::cmp::Reverse(w.len()));
    for word in words {
        if let Some(rest) = base.strip_suffix(&format!("-{word}")) {
            if !rest.is_empty() {
                family = rest;
                effort = word;
                break;
            }
        }
    }
    let suffix = match (effort, fast) {
        ("", false) => String::new(),
        ("", true) => FAST.to_string(),
        (e, false) => e.to_string(),
        (e, true) => format!("{e}-{FAST}"),
    };
    (family.to_string(), suffix)
}

/// Pick-order rank of a suffix: default, bare fast, then each effort word
/// with its fast twin right behind it.
fn suffix_rank(suffix: &str) -> (usize, bool) {
    if suffix.is_empty() {
        return (0, false);
    }
    let (base, fast) = match suffix.strip_suffix("-fast") {
        Some(base) => (base, true),
        None if suffix == FAST => ("", true),
        None => (suffix, false),
    };
    let rank = if base.is_empty() {
        0
    } else {
        EFFORT_ORDER
            .iter()
            .position(|w| *w == base)
            .map_or(EFFORT_ORDER.len() + 1, |p| p + 1)
    };
    (rank, fast)
}

impl Catalogue {
    /// Build from ids, deduplicating, families in first-seen order, efforts
    /// in pick order.
    pub fn from_ids<'a>(ids: impl IntoIterator<Item = &'a str>) -> Catalogue {
        let mut families: Vec<(String, Vec<String>)> = Vec::new();
        for id in ids {
            let id = id.trim();
            if id.is_empty() || id.contains(char::is_whitespace) {
                continue;
            }
            let (family, suffix) = split_id(id);
            let slot = match families.iter_mut().find(|(f, _)| *f == family) {
                Some(slot) => slot,
                None => {
                    families.push((family, Vec::new()));
                    families.last_mut().expect("just pushed")
                }
            };
            if !slot.1.contains(&suffix) {
                slot.1.push(suffix);
            }
        }
        let families = families
            .into_iter()
            .map(|(name, mut suffixes)| {
                suffixes.sort_by_key(|s| suffix_rank(s));
                let efforts = suffixes
                    .into_iter()
                    .map(|s| {
                        if s.is_empty() {
                            DEFAULT_CHOICE.to_string()
                        } else {
                            s
                        }
                    })
                    .collect::<Vec<_>>();
                // A family whose only variant is the bare id has no effort
                // choice at all (`auto`).
                let efforts = if efforts.len() == 1 && efforts[0] == DEFAULT_CHOICE {
                    Vec::new()
                } else {
                    efforts
                };
                Family { name, efforts }
            })
            .collect();
        Catalogue { families }
    }

    pub fn family(&self, name: &str) -> Option<&Family> {
        let name = name.trim();
        self.families
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
    }

    /// What "default" launches for a family with no bare id: `high` if it
    /// ships one, else `medium`, else its first non-fast variant, else its
    /// first. None for a family with a bare id (default = no suffix) or none.
    pub fn fallback_effort(&self, name: &str) -> Option<&str> {
        let family = self.family(name)?;
        let efforts = &family.efforts;
        if efforts.is_empty() || efforts[0] == DEFAULT_CHOICE {
            return None;
        }
        ["high", "medium"]
            .iter()
            .find_map(|want| efforts.iter().find(|e| e == want))
            .or_else(|| efforts.iter().find(|e| !e.ends_with(FAST)))
            .or_else(|| efforts.first())
            .map(String::as_str)
    }
}

// --- the installed, 'static view --------------------------------------------

struct Installed {
    models: &'static [&'static str],
    families: Vec<(&'static str, &'static [&'static str])>,
    fallbacks: Vec<(&'static str, Option<&'static str>)>,
}

fn leak(catalogue: &Catalogue) -> &'static Installed {
    let mut models: Vec<&'static str> = vec![DEFAULT_CHOICE];
    let mut families = Vec::new();
    let mut fallbacks = Vec::new();
    for family in &catalogue.families {
        let name: &'static str = Box::leak(family.name.clone().into_boxed_str());
        let efforts: Vec<&'static str> = family
            .efforts
            .iter()
            .map(|e| {
                if e == DEFAULT_CHOICE {
                    DEFAULT_CHOICE
                } else {
                    Box::leak(e.clone().into_boxed_str()) as &'static str
                }
            })
            .collect();
        models.push(name);
        families.push((
            name,
            Box::leak(efforts.into_boxed_slice()) as &'static [&'static str],
        ));
        fallbacks.push((
            name,
            catalogue
                .fallback_effort(&family.name)
                .map(|e| Box::leak(e.to_string().into_boxed_str()) as &'static str),
        ));
    }
    Box::leak(Box::new(Installed {
        models: Box::leak(models.into_boxed_slice()),
        families,
        fallbacks,
    }))
}

/// One CLI's installed catalogue: its seed until a listing is installed
/// over it, as seed ∪ listing.
pub struct Live {
    seed: &'static [&'static str],
    cell: OnceLock<RwLock<&'static Installed>>,
}

impl Live {
    pub const fn new(seed: &'static [&'static str]) -> Self {
        Self {
            seed,
            cell: OnceLock::new(),
        }
    }

    fn current(&self) -> &'static Installed {
        let lock = self
            .cell
            .get_or_init(|| RwLock::new(leak(&Catalogue::from_ids(self.seed.iter().copied()))));
        *lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The MODEL choices: [`DEFAULT_CHOICE`], then every family.
    pub fn models(&self) -> &'static [&'static str] {
        self.current().models
    }

    /// Whether `family` is one this catalogue lists.
    pub fn knows(&self, family: &str) -> bool {
        let name = family.trim();
        self.current()
            .families
            .iter()
            .any(|(f, _)| f.eq_ignore_ascii_case(name))
    }

    /// The EFFORT choices for a family; empty for no family, "default", an
    /// unknown family, or one with no effort variants.
    pub fn efforts(&self, family: Option<&str>) -> &'static [&'static str] {
        let Some(name) = family
            .map(str::trim)
            .filter(|m| !m.eq_ignore_ascii_case(DEFAULT_CHOICE))
        else {
            return &[];
        };
        self.current()
            .families
            .iter()
            .find(|(f, _)| f.eq_ignore_ascii_case(name))
            .map_or(&[], |(_, efforts)| efforts)
    }

    /// See [`Catalogue::fallback_effort`].
    pub fn fallback_effort(&self, family: &str) -> Option<&'static str> {
        let name = family.trim();
        self.current()
            .fallbacks
            .iter()
            .find(|(f, _)| f.eq_ignore_ascii_case(name))
            .and_then(|(_, e)| *e)
    }

    /// Replace the installed catalogue with seed ∪ `runtime_ids`. Not called
    /// from tests on the process-global ones.
    pub fn install(&self, runtime_ids: &[String]) {
        let merged = Catalogue::from_ids(
            self.seed
                .iter()
                .copied()
                .chain(runtime_ids.iter().map(String::as_str)),
        );
        let installed = leak(&merged);
        let lock = self.cell.get_or_init(|| RwLock::new(installed));
        *lock
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = installed;
    }
}

// --- cache and refresh -------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    fetched_at: u64,
    ids: Vec<String>,
}

fn cache_path(file: &str) -> PathBuf {
    nebula_core::paths::data_dir().join(file)
}

fn read_cache(path: &Path) -> Option<Cache> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_cache(path: &Path, ids: &[String]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let cache = Cache {
        fetched_at: now_secs(),
        ids: ids.to_vec(),
    };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&cache)?)?;
    std::fs::rename(tmp, path)
}

/// TUI startup: install the catalogue cached in `cache_file` at once and,
/// when that cache is missing or older than [`CACHE_TTL`], `fetch` a fresh
/// listing on a background thread, cache it and install it. Callers skip
/// this when the CLI is switched off and under `NEBULA_AGENT_CMD` (tests,
/// STUB AGENTS), so no test ever shells out.
pub fn refresh(
    live: &'static Live,
    cache_file: &str,
    fetch: impl FnOnce() -> Option<Vec<String>> + Send + 'static,
) {
    let path = cache_path(cache_file);
    let cached = read_cache(&path);
    if let Some(cache) = &cached {
        live.install(&cache.ids);
    }
    let fresh =
        cached.is_some_and(|c| now_secs().saturating_sub(c.fetched_at) < CACHE_TTL.as_secs());
    if fresh {
        return;
    }
    let name = cache_file.to_string();
    std::thread::spawn(move || {
        if let Some(ids) = fetch() {
            if let Err(err) = write_cache(&path, &ids) {
                tracing::warn!("{name} not written: {err}");
            }
            live.install(&ids);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_id_peels_fast_then_effort() {
        assert_eq!(split_id("auto"), ("auto".into(), "".into()));
        assert_eq!(
            split_id("gpt-5.3-codex"),
            ("gpt-5.3-codex".into(), "".into())
        );
        assert_eq!(
            split_id("gpt-5.3-codex-fast"),
            ("gpt-5.3-codex".into(), "fast".into())
        );
        assert_eq!(
            split_id("claude-opus-5-thinking-high"),
            ("claude-opus-5-thinking".into(), "high".into())
        );
        assert_eq!(
            split_id("claude-opus-5-thinking-max-fast"),
            ("claude-opus-5-thinking".into(), "max-fast".into())
        );
        assert_eq!(
            split_id("gpt-5.5-extra-high"),
            ("gpt-5.5".into(), "extra-high".into())
        );
        assert_eq!(
            split_id("gpt-5.6-sol-none-fast"),
            ("gpt-5.6-sol".into(), "none-fast".into())
        );
        // Neither a fast nor an effort tail: a bare family of its own.
        assert_eq!(
            split_id("claude-4.6-sonnet-medium-thinking"),
            ("claude-4.6-sonnet-medium-thinking".into(), "".into())
        );
        // An id that *is* an effort word stays a family, not an empty one.
        assert_eq!(split_id("high"), ("high".into(), "".into()));
    }

    #[test]
    fn from_ids_groups_dedups_and_orders() {
        let cat = Catalogue::from_ids([
            "gpt-5.3-codex-xhigh",
            "gpt-5.3-codex",
            "gpt-5.3-codex-fast",
            "gpt-5.3-codex-low",
            "gpt-5.3-codex-low",
            "auto",
            "claude-fable-5-max",
            "claude-fable-5-low",
            "bad id",
            "",
        ]);
        assert_eq!(
            cat.families
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-5.3-codex", "auto", "claude-fable-5"]
        );
        assert_eq!(
            cat.family("gpt-5.3-codex").unwrap().efforts,
            vec!["default", "fast", "low", "xhigh"]
        );
        assert!(
            cat.family("auto").unwrap().efforts.is_empty(),
            "bare-only = no choice"
        );
        assert_eq!(
            cat.family("claude-fable-5").unwrap().efforts,
            vec!["low", "max"]
        );
        assert_eq!(
            cat.family("CLAUDE-FABLE-5").map(|f| f.name.as_str()),
            Some("claude-fable-5")
        );
    }

    #[test]
    fn fallback_prefers_high_then_medium_then_a_non_fast_variant() {
        let cat = Catalogue::from_ids([
            "gpt-5.3-codex",
            "gpt-5.3-codex-high",
            "claude-fable-5-low",
            "claude-fable-5-high",
            "gemini-x-low",
            "gemini-x-medium",
            "odd-none-fast",
            "odd-none",
            "onlyfast-low-fast",
            "auto",
        ]);
        assert_eq!(
            cat.fallback_effort("gpt-5.3-codex"),
            None,
            "bare id = default is real"
        );
        assert_eq!(cat.fallback_effort("claude-fable-5"), Some("high"));
        assert_eq!(cat.fallback_effort("gemini-x"), Some("medium"));
        assert_eq!(cat.fallback_effort("odd"), Some("none"));
        assert_eq!(cat.fallback_effort("onlyfast"), Some("low-fast"));
        assert_eq!(cat.fallback_effort("auto"), None);
        assert_eq!(cat.fallback_effort("nope"), None);
    }

    /// A catalogue shows its seed until a listing lands, then seed ∪ the
    /// listing, families in first-seen order.
    #[test]
    fn a_live_catalogue_is_its_seed_then_seed_and_listing() {
        static LIVE: Live = Live::new(&["alpha-low", "alpha-high"]);
        assert_eq!(LIVE.models(), &[DEFAULT_CHOICE, "alpha"]);
        assert!(LIVE.knows("ALPHA") && !LIVE.knows("beta"));
        LIVE.install(&["beta".to_string(), "beta-low".to_string()]);
        assert_eq!(LIVE.models(), &[DEFAULT_CHOICE, "alpha", "beta"]);
        assert_eq!(LIVE.efforts(Some("beta")), &[DEFAULT_CHOICE, "low"]);
        assert_eq!(LIVE.efforts(Some("default")), &[] as &[&str]);
        assert_eq!(LIVE.fallback_effort("alpha"), Some("high"));
        assert_eq!(LIVE.fallback_effort("beta"), None);
    }

    #[test]
    fn cache_round_trips_and_ages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("models.json");
        assert!(read_cache(&path).is_none());
        write_cache(&path, &["auto".to_string(), "gpt-5.2".to_string()]).unwrap();
        let cache = read_cache(&path).unwrap();
        assert_eq!(cache.ids, vec!["auto", "gpt-5.2"]);
        assert!(now_secs().saturating_sub(cache.fetched_at) < CACHE_TTL.as_secs());
        assert!(
            !path.with_extension("json.tmp").exists(),
            "tmp renamed away"
        );
    }
}
