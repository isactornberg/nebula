//! The Antigravity (`agy`) MODEL / EFFORT catalogue, for a harness the
//! config adds with `"catalog": "agy"`.
//!
//! `agy models` lists one slug per (family, effort) — `gemini-4-neon-high`,
//! `claude-opus-5-5-low`, `gpt-oss-120b-medium` — and what it lists follows
//! the signed-in account. nebula keeps the family as the MODEL and the
//! suffix as the EFFORT, and passes them apart, `--model <family> --effort
//! <effort>`. A family offers only the efforts it ships: agy runs its own
//! default model, not the one asked for, when the two do not match (verified
//! on agy 1.5.14). Every family also launches bare, at its own default
//! effort, so `default` heads each family's efforts.
//!
//! No seed: the listing is the account's, so until the first `agy models`
//! lands (cached in the DATA DIR for a day, see `family_catalogue`) the
//! MODEL list is `default` alone.

use std::process::{Command, Stdio};

use nebula_core::harness::HarnessCatalog;

use crate::family_catalogue::{split_id, Live};

const CACHE_FILE: &str = "agy_models.json";

/// The installed agy catalogue.
pub static LIVE: Live = Live::new(&[]);

/// Parse `agy models` output (`<slug>\t<label>` lines under a "Fetching
/// available models..." line) into slugs, each family's bare name before
/// its first slug.
pub fn parse_models(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in text.lines() {
        let Some((id, _label)) = line.split_once('\t') else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() || id.contains(char::is_whitespace) {
            continue;
        }
        let family = split_id(id).0;
        if !ids.contains(&family) {
            ids.push(family);
        }
        ids.push(id.to_string());
    }
    ids
}

/// Run `<program> models` and parse it; None when the CLI is missing,
/// fails (signed out) or lists nothing.
pub fn fetch_ids(program: &str) -> Option<Vec<String>> {
    let output = Command::new(program)
        .arg("models")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let ids = parse_models(&String::from_utf8_lossy(&output.stdout));
    (!ids.is_empty()).then_some(ids)
}

/// TUI startup: install the cached catalogue at once and refresh it in the
/// background when a day old, from the program of the first enabled harness
/// that reads this catalogue. Skipped when none does, and under
/// `NEBULA_AGENT_CMD` (tests, STUB AGENTS), so no test ever shells out.
pub fn bootstrap(cfg: &crate::config::Config) {
    if nebula_core::env::non_empty(nebula_core::env::AGENT_CMD).is_some() {
        return;
    }
    let Some(program) = cfg
        .harness_registry()
        .into_iter()
        .find(|h| {
            h.enabled && h.problem().is_none() && h.model.catalog == Some(HarnessCatalog::Agy)
        })
        .map(|h| h.program.trim().to_string())
    else {
        return;
    };
    crate::family_catalogue::refresh(&LIVE, CACHE_FILE, move || fetch_ids(&program));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DEFAULT_CHOICE;
    use crate::family_catalogue::Catalogue;

    /// `agy models` as agy 1.5.14 printed it on 2026-11-22, trimmed.
    const LISTED: &str = "Fetching available models...\n\
        gemini-4-neon-high\tGemini 4 Neon (High)\n\
        gemini-4-neon-xhigh\tGemini 4 Neon (XHigh)\n\
        gemini-4-neon-medium\tGemini 4 Neon (Medium)\n\
        gemini-4-neon-low\tGemini 4 Neon (Low)\n\
        gemini-4-argon-medium\tGemini 4 Argon (Medium)\n\
        gemini-4-argon-high\tGemini 4 Argon (High)\n\
        gemini-4-argon-low\tGemini 4 Argon (Low)\n\
        gemini-pro-orchestrator-neon\tOrchestrator (Neon)\n\
        gemini-pro-orchestrator-g1-pro\tOrchestrator (Argon)\n\
        gemini-3.8-flash-high\tGemini 3.8 Flash (High)\n\
        claude-opus-5-5-low\tClaude Opus 5.5 (Low)\n\
        claude-opus-5-5-high\tClaude Opus 5.5 (High)\n\
        gpt-oss-120b-medium\tGPT-OSS 120B (Medium)\n";

    /// Each family is a MODEL, `default` (its bare name, the family's own
    /// effort) then the efforts it ships in pick order; a slug with no
    /// effort tail is a model with no effort to pick.
    #[test]
    fn the_listing_reads_as_families_and_the_efforts_each_ships() {
        let ids = parse_models(LISTED);
        assert_eq!(
            &ids[..3],
            ["gemini-4-neon", "gemini-4-neon-high", "gemini-4-neon-xhigh"]
        );
        let cat = Catalogue::from_ids(ids.iter().map(String::as_str));
        let names: Vec<&str> = cat.families.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "gemini-4-neon",
                "gemini-4-argon",
                "gemini-pro-orchestrator-neon",
                "gemini-pro-orchestrator-g1-pro",
                "gemini-3.8-flash",
                "claude-opus-5-5",
                "gpt-oss-120b",
            ]
        );
        let efforts = |family: &str| cat.family(family).unwrap().efforts.clone();
        assert_eq!(
            efforts("gemini-4-neon"),
            [DEFAULT_CHOICE, "low", "medium", "high", "xhigh"]
        );
        assert_eq!(
            efforts("gemini-4-argon"),
            [DEFAULT_CHOICE, "low", "medium", "high"]
        );
        assert_eq!(efforts("gpt-oss-120b"), [DEFAULT_CHOICE, "medium"]);
        assert!(efforts("gemini-pro-orchestrator-neon").is_empty());
        assert_eq!(cat.fallback_effort("gemini-4-argon"), None, "bare runs");
        assert!(parse_models("Fetching available models...\n").is_empty());
    }
}
