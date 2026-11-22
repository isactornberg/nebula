//! The Cursor MODEL / EFFORT catalogue.
//!
//! `cursor-agent --model` takes one flat id per (family, effort, fast) triple —
//! `claude-opus-5-thinking-high`, `gpt-5.6-sol-xhigh-fast`, `gpt-5.3-codex` — and
//! rejects the `family[effort=…]` bracket form its `--help` describes. nebula
//! keeps the *family* as the MODEL and the *suffix* (`high`, `high-fast`,
//! `fast`) as the EFFORT, and the DAEMON joins them with a `-` at spawn.
//!
//! Two sources feed one [`Live`] catalogue (`family_catalogue`): [`SEED_IDS`],
//! the catalogue as observed on 2026-08-28, and — at runtime —
//! `cursor-agent --list-models`, cached in the DATA DIR for a day and
//! refreshed on a background thread by [`bootstrap`]. The two are unioned
//! (seed order first), so a family Cursor adds shows up by itself while
//! nothing the seed knows is lost: `--list-models` prints only the featured
//! ids, and many accepted variants (`claude-fable-5-low`, …) never appear in
//! it.

use std::process::{Command, Stdio};

use crate::family_catalogue::Live;

/// Catalogue ids as `cursor-agent` accepted them on 2026-08-28 (cursor-agent
/// 2026.08.25), curated to the current families. Order is the pick order.
pub const SEED_IDS: &[&str] = &[
    // auto
    "auto",
    // claude-fable-5
    "claude-fable-5-low",
    "claude-fable-5-medium",
    "claude-fable-5-high",
    "claude-fable-5-xhigh",
    "claude-fable-5-max",
    // claude-fable-5-thinking
    "claude-fable-5-thinking-low",
    "claude-fable-5-thinking-medium",
    "claude-fable-5-thinking-high",
    "claude-fable-5-thinking-xhigh",
    "claude-fable-5-thinking-max",
    // claude-opus-5
    "claude-opus-5-low",
    "claude-opus-5-low-fast",
    "claude-opus-5-medium",
    "claude-opus-5-medium-fast",
    "claude-opus-5-high",
    "claude-opus-5-high-fast",
    // claude-opus-5-thinking
    "claude-opus-5-thinking-low",
    "claude-opus-5-thinking-low-fast",
    "claude-opus-5-thinking-medium",
    "claude-opus-5-thinking-medium-fast",
    "claude-opus-5-thinking-high",
    "claude-opus-5-thinking-high-fast",
    "claude-opus-5-thinking-xhigh",
    "claude-opus-5-thinking-xhigh-fast",
    "claude-opus-5-thinking-max",
    "claude-opus-5-thinking-max-fast",
    // claude-sonnet-5
    "claude-sonnet-5-low",
    "claude-sonnet-5-medium",
    "claude-sonnet-5-high",
    "claude-sonnet-5-xhigh",
    "claude-sonnet-5-max",
    // claude-sonnet-5-thinking
    "claude-sonnet-5-thinking-low",
    "claude-sonnet-5-thinking-medium",
    "claude-sonnet-5-thinking-high",
    "claude-sonnet-5-thinking-xhigh",
    "claude-sonnet-5-thinking-max",
    // gpt-5.6-sol
    "gpt-5.6-sol-none",
    "gpt-5.6-sol-none-fast",
    "gpt-5.6-sol-low",
    "gpt-5.6-sol-low-fast",
    "gpt-5.6-sol-medium",
    "gpt-5.6-sol-medium-fast",
    "gpt-5.6-sol-high",
    "gpt-5.6-sol-high-fast",
    "gpt-5.6-sol-xhigh",
    "gpt-5.6-sol-xhigh-fast",
    "gpt-5.6-sol-max",
    "gpt-5.6-sol-max-fast",
    // gpt-5.6-terra
    "gpt-5.6-terra-none",
    "gpt-5.6-terra-none-fast",
    "gpt-5.6-terra-low",
    "gpt-5.6-terra-low-fast",
    "gpt-5.6-terra-medium",
    "gpt-5.6-terra-medium-fast",
    "gpt-5.6-terra-high",
    "gpt-5.6-terra-high-fast",
    "gpt-5.6-terra-xhigh",
    "gpt-5.6-terra-xhigh-fast",
    "gpt-5.6-terra-max",
    "gpt-5.6-terra-max-fast",
    // gpt-5.6-luna
    "gpt-5.6-luna-none",
    "gpt-5.6-luna-none-fast",
    "gpt-5.6-luna-low",
    "gpt-5.6-luna-low-fast",
    "gpt-5.6-luna-medium",
    "gpt-5.6-luna-medium-fast",
    "gpt-5.6-luna-high",
    "gpt-5.6-luna-high-fast",
    "gpt-5.6-luna-xhigh",
    "gpt-5.6-luna-xhigh-fast",
    "gpt-5.6-luna-max",
    "gpt-5.6-luna-max-fast",
    // gpt-5.5
    "gpt-5.5-none",
    "gpt-5.5-none-fast",
    "gpt-5.5-low",
    "gpt-5.5-low-fast",
    "gpt-5.5-medium",
    "gpt-5.5-medium-fast",
    "gpt-5.5-high",
    "gpt-5.5-high-fast",
    "gpt-5.5-extra-high",
    "gpt-5.5-extra-high-fast",
    // gpt-5.3-codex
    "gpt-5.3-codex-low",
    "gpt-5.3-codex-low-fast",
    "gpt-5.3-codex",
    "gpt-5.3-codex-fast",
    "gpt-5.3-codex-high",
    "gpt-5.3-codex-high-fast",
    "gpt-5.3-codex-xhigh",
    "gpt-5.3-codex-xhigh-fast",
    // gpt-5.2
    "gpt-5.2",
    "gpt-5.2-low",
    "gpt-5.2-low-fast",
    "gpt-5.2-fast",
    "gpt-5.2-high",
    "gpt-5.2-high-fast",
    "gpt-5.2-xhigh",
    "gpt-5.2-xhigh-fast",
    // composer-2.5
    "composer-2.5",
    "composer-2.5-fast",
    // cursor-grok-4.6
    "cursor-grok-4.6-low",
    "cursor-grok-4.6-low-fast",
    "cursor-grok-4.6-medium",
    "cursor-grok-4.6-medium-fast",
    "cursor-grok-4.6-high",
    "cursor-grok-4.6-high-fast",
    "cursor-grok-4.6-xhigh",
    "cursor-grok-4.6-xhigh-fast",
    // gemini-3.7-flash
    "gemini-3.7-flash-low",
    "gemini-3.7-flash-medium",
    "gemini-3.7-flash-high",
];

const CACHE_FILE: &str = "cursor_models.json";

/// The installed Cursor catalogue.
pub static LIVE: Live = Live::new(SEED_IDS);

/// The MODEL choices: `default`, then every family.
pub fn models() -> &'static [&'static str] {
    LIVE.models()
}

/// The EFFORT choices for a family (see [`Live::efforts`]).
pub fn efforts(family: Option<&str>) -> &'static [&'static str] {
    LIVE.efforts(family)
}

/// What "default" launches for a family with no bare id (see
/// [`Live::fallback_effort`]).
pub fn fallback_effort(family: &str) -> Option<&'static str> {
    LIVE.fallback_effort(family)
}

/// Parse `cursor-agent --list-models` output (`<id> - <label>` lines under an
/// "Available models" header) into ids.
pub fn parse_list_models(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (id, _label) = line.split_once(" - ")?;
            let id = id.trim();
            (!id.is_empty() && !id.contains(char::is_whitespace)).then(|| id.to_string())
        })
        .collect()
}

/// Run `cursor-agent --list-models` and parse it; None when the CLI is
/// missing, fails (not logged in), or lists nothing.
pub fn fetch_ids() -> Option<Vec<String>> {
    let output = Command::new(nebula_core::AgentKind::Cursor.cli_program())
        .arg("--list-models")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let ids = parse_list_models(&String::from_utf8_lossy(&output.stdout));
    (!ids.is_empty()).then_some(ids)
}

/// TUI startup: install the cached catalogue at once and, when the cache is
/// missing or a day old, refresh it on a background thread. Skipped when
/// Cursor is switched off (HARNESS TOGGLE) and under `NEBULA_AGENT_CMD`
/// (tests, STUB AGENTS), so no test ever shells out.
pub fn bootstrap(cursor_enabled: bool) {
    if !cursor_enabled || nebula_core::env::non_empty(nebula_core::env::AGENT_CMD).is_some() {
        return;
    }
    crate::family_catalogue::refresh(&LIVE, CACHE_FILE, fetch_ids);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DEFAULT_CHOICE;
    use crate::family_catalogue::Catalogue;

    #[test]
    fn seed_matches_the_static_view() {
        let cat = Catalogue::from_ids(SEED_IDS.iter().copied());
        assert_eq!(models()[0], DEFAULT_CHOICE);
        assert_eq!(
            models()[1..].to_vec(),
            cat.families
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(models()[1], "auto");
        assert!(efforts(Some("auto")).is_empty());
        assert!(efforts(None).is_empty());
        assert!(efforts(Some("default")).is_empty());
        assert!(efforts(Some("not-a-family")).is_empty());
        assert_eq!(
            efforts(Some("claude-opus-5")),
            &[
                "low",
                "low-fast",
                "medium",
                "medium-fast",
                "high",
                "high-fast"
            ]
        );
        assert_eq!(efforts(Some("gpt-5.3-codex"))[0], DEFAULT_CHOICE);
        assert_eq!(fallback_effort("gpt-5.3-codex"), None);
        assert_eq!(fallback_effort("claude-fable-5"), Some("high"));
        assert_eq!(fallback_effort("gpt-5.5"), Some("high"));
        assert_eq!(fallback_effort("auto"), None);
        for family in &cat.families {
            assert!(
                family.efforts.is_empty()
                    || family
                        .efforts
                        .iter()
                        .all(|e| e == DEFAULT_CHOICE || !e.is_empty()),
                "{}: no empty suffix survives",
                family.name
            );
        }
    }

    #[test]
    fn parse_list_models_takes_the_id_column() {
        let text = "Available models\n\nauto - Auto (default)\ngpt-5.3-codex-low - Codex 5.3 Low\n                    claude-opus-5-thinking-high - Claude Opus 5 1M Thinking\nnot a model line\n";
        assert_eq!(
            parse_list_models(text),
            vec!["auto", "gpt-5.3-codex-low", "claude-opus-5-thinking-high"]
        );
        assert!(parse_list_models("").is_empty());
    }
}
