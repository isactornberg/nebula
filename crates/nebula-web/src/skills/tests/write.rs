//! The page's writes, each in a temp home: what a switch writes where, and
//! what it never does.

use super::*;
use api::SkillSetting as S;

fn to(found: &Found, state: S) -> api::SkillTo {
    api::SkillTo {
        id: found.skill.id.clone(),
        state,
    }
}

fn set(m: &Machine, cli: Cli, name: &str, state: S) -> Result<api::SkillWrite, crate::Refusal> {
    let scan = m.scan();
    let found = one(&scan, name);
    switch(&scan, cli, &[to(found, state)])
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn message(refusal: crate::Refusal) -> (u16, String) {
    (refusal.status.as_u16(), refusal.message)
}

fn control(scan: &Scan, name: &str, cli: Cli) -> Option<api::SkillControl> {
    one(scan, name)
        .skill
        .controls
        .iter()
        .find(|c| c.cli == cli)
        .cloned()
}

/// Each of Claude's states is its `skillOverrides` value in the user's
/// settings, On no entry at all, and every other key stays as it was, in
/// its order and indent.
#[test]
fn claudes_states_are_skill_overrides_in_the_users_settings() {
    let m = Machine::new();
    plain(&m.home().join(".claude/skills"), "capture");
    let file = m.home().join(".claude/settings.json");
    let original = "{\n    \"zeta\": 1,\n    \"skillOverrides\": {\n        \"other\": \"off\"\n    },\n    \"alpha\": {\"nested\": [true, null]}\n}\n";
    write(&file, original);
    assert_eq!(
        control(&m.scan(), "capture", Cli::Claude),
        Some(api::SkillControl {
            cli: Cli::Claude,
            setting: "skillOverrides",
            file: "~/.claude/settings.json".into(),
            state: S::On,
            plugin: None,
            project: None,
        })
    );
    for (state, value) in [
        (S::NameOnly, "name-only"),
        (S::UserInvocableOnly, "user-invocable-only"),
        (S::Off, "off"),
    ] {
        let written = set(&m, Cli::Claude, "capture", state).unwrap();
        assert_eq!(written.file, "~/.claude/settings.json");
        assert!(written.changed);
        assert_eq!(
            read(&file),
            original.replace(
                "\"other\": \"off\"",
                &format!("\"other\": \"off\",\n        \"capture\": \"{value}\"")
            )
        );
        assert_eq!(
            control(&m.scan(), "capture", Cli::Claude).unwrap().state,
            state
        );
    }
    let written = set(&m, Cli::Claude, "capture", S::On).unwrap();
    assert_eq!(written.before, [to(one(&m.scan(), "capture"), S::Off)]);
    assert_eq!(
        read(&file),
        original,
        "On takes the entry out, and nothing else"
    );
    let again = set(&m, Cli::Claude, "capture", S::On).unwrap();
    assert!(!again.changed, "nothing to change is not a write");
}

/// A project's skill is switched in its `.claude/settings.local.json`,
/// which is not committed, and On there says `on` when a file under it
/// says otherwise, since Claude reads the local file first.
#[test]
fn a_projects_skill_is_switched_in_its_local_settings() {
    let mut m = Machine::new();
    let shop = m.project("shop");
    plain(&shop.join(".claude/skills"), "translate");
    write(
        &shop.join(".claude/settings.json"),
        r#"{"skillOverrides": {"translate": "off"}}"#,
    );
    let local = shop.join(".claude/settings.local.json");
    let c = control(&m.scan(), "translate", Cli::Claude).unwrap();
    assert_eq!(
        (c.file.as_str(), c.state, c.project.as_deref()),
        (
            "~/code/shop/.claude/settings.local.json",
            S::Off,
            Some("shop")
        )
    );
    set(&m, Cli::Claude, "translate", S::On).unwrap();
    assert_eq!(
        read(&local),
        "{\n  \"skillOverrides\": {\n    \"translate\": \"on\"\n  }\n}\n",
        "a file that is not there gets only that key"
    );
    assert_eq!(
        control(&m.scan(), "translate", Cli::Claude).unwrap().state,
        S::On
    );
    let written = set(&m, Cli::Claude, "translate", S::Off).unwrap();
    assert_eq!(written.before, [to(one(&m.scan(), "translate"), S::On)]);
    assert_eq!(
        read(&local),
        "{\n  \"skillOverrides\": {}\n}\n",
        "off is what the shared file says already"
    );
    assert_eq!(
        read(&shop.join(".claude/settings.json")),
        r#"{"skillOverrides": {"translate": "off"}}"#,
        "the committed file is never written"
    );
}

/// A plugin goes on and off in `enabledPlugins`, as a whole; its skills
/// have no switch of their own.
#[test]
fn a_plugin_is_switched_in_enabled_plugins() {
    let m = Machine::new();
    let plugins = m.home().join(".claude/plugins");
    let cached = plugins.join("cache/market/kit/1.0.0");
    write(
        &cached.join(".claude-plugin/plugin.json"),
        r#"{"name": "kit"}"#,
    );
    plain(&cached.join("skills"), "lint");
    write(
        &plugins.join("installed_plugins.json"),
        &serde_json::json!({"plugins": {"kit@market": [{"installPath": cached, "version": "1.0.0"}]}})
            .to_string(),
    );
    let file = m.home().join(".claude/settings.json");
    write(&file, "{\n  \"theme\": \"dark\"\n}\n");
    let c = control(&m.scan(), "lint", Cli::Claude).unwrap();
    assert_eq!(
        (c.setting, c.plugin.as_deref(), c.state),
        ("enabledPlugins", Some("kit@market"), S::Off)
    );
    assert_eq!(
        message(set(&m, Cli::Claude, "lint", S::On).unwrap_err()).0,
        409,
        "a plugin's skill is switched with its plugin"
    );
    switch_plugin(&m.scan(), "kit@market", true).unwrap();
    assert_eq!(
        read(&file),
        "{\n  \"theme\": \"dark\",\n  \"enabledPlugins\": {\n    \"kit@market\": true\n  }\n}\n"
    );
    assert_eq!(
        control(&m.scan(), "lint", Cli::Claude).unwrap().state,
        S::On
    );
    switch_plugin(&m.scan(), "kit@market", false).unwrap();
    assert!(read(&file).contains("\"kit@market\": false"));
    assert_eq!(
        control(&m.scan(), "lint", Cli::Claude).unwrap().state,
        S::Off
    );
    let (status, text) = message(switch_plugin(&m.scan(), "other@market", true).unwrap_err());
    assert_eq!(
        (status, text.as_str()),
        (404, "There is no plugin other@market.")
    );
}

/// Codex's switch is a `[[skills.config]]` entry by the `SKILL.md`'s path,
/// for a skill of the user's and for one it has built in, and every
/// comment and key of the file is kept.
#[test]
fn codexs_switch_is_skills_config_by_path() {
    let m = Machine::new();
    let draw = plain(&m.home().join(".agents/skills"), "draw");
    let creator = plain(&m.home().join(".codex/skills/.system"), "skill-creator");
    let file = m.home().join(".codex/config.toml");
    let original = "# my settings\nmodel = \"gpt\" # the model\n\n[tui]\ntheme = \"dark\"\n";
    write(&file, original);
    let c = control(&m.scan(), "draw", Cli::Codex).unwrap();
    assert_eq!(
        (c.setting, c.file.as_str(), c.state),
        ("skills.config", "~/.codex/config.toml", S::On)
    );
    assert_eq!(
        message(set(&m, Cli::Codex, "draw", S::NameOnly).unwrap_err()).0,
        400
    );
    set(&m, Cli::Codex, "draw", S::Off).unwrap();
    set(&m, Cli::Codex, "skill-creator", S::Off).unwrap();
    let entry = |dir: &Path| {
        format!(
            "\n[[skills.config]]\npath = \"{}\"\nenabled = false\n",
            dir.join("SKILL.md").display()
        )
    };
    assert_eq!(
        read(&file),
        format!("{original}{}{}", entry(&draw), entry(&creator))
    );
    let scan = m.scan();
    assert_eq!(control(&scan, "draw", Cli::Codex).unwrap().state, S::Off);
    assert_eq!(
        control(&scan, "skill-creator", Cli::Codex).unwrap().state,
        S::Off
    );
    set(&m, Cli::Codex, "draw", S::On).unwrap();
    assert_eq!(
        read(&file),
        format!("{original}{}{}", entry(&draw), entry(&creator)).replacen(
            "enabled = false",
            "enabled = true",
            1
        ),
        "the same entry, turned on"
    );
}

/// A Codex skill reached through a link gets an entry for each path, so
/// it is off whichever of them Codex goes by.
#[test]
fn a_linked_codex_skill_is_switched_by_each_of_its_paths() {
    let m = Machine::new();
    let real = plain(&m.home().join("code/toolbox/skills"), "notes");
    let linked = m.home().join(".agents/skills/notes");
    link(&linked, &real);
    set(&m, Cli::Codex, "notes", S::Off).unwrap();
    let text = read(&m.home().join(".codex/config.toml"));
    for dir in [&real, &linked] {
        assert!(
            text.contains(&format!("path = \"{}\"", dir.join("SKILL.md").display())),
            "{text}"
        );
    }
}

/// Codex's built-in skill-creator beside one of the user's is one fix,
/// and once the built-in is off it is none.
#[test]
fn a_built_in_switched_off_is_no_longer_a_name_found_twice() {
    let m = Machine::new();
    plain(&m.home().join(".agents/skills"), "skill-creator");
    plain(&m.home().join(".codex/skills/.system"), "skill-creator");
    assert_eq!(fixes(&m.scan()).len(), 1);
    let scan = m.scan();
    let builtin = named(&scan, "skill-creator")
        .into_iter()
        .find(|f| f.finds[0].scope == Scope::System)
        .unwrap();
    switch(&scan, Cli::Codex, &[to(builtin, S::Off)]).unwrap();
    assert!(fixes(&m.scan()).is_empty(), "{:?}", fixes(&m.scan()));
}

/// A settings file that is a link is written at its target, and the link
/// stays a link.
#[test]
fn a_linked_settings_file_is_written_at_its_target() {
    let m = Machine::new();
    plain(&m.home().join(".claude/skills"), "capture");
    let target = m.home().join("dotfiles/claude/settings.json");
    write(&target, "{}\n");
    let file = m.home().join(".claude/settings.json");
    link(&file, &target);
    let written = set(&m, Cli::Claude, "capture", S::Off).unwrap();
    assert_eq!(written.file, "~/dotfiles/claude/settings.json");
    assert_eq!(written.link.as_deref(), Some("~/.claude/settings.json"));
    assert!(is_link(&file));
    assert_eq!(
        read(&target),
        "{\n  \"skillOverrides\": {\n    \"capture\": \"off\"\n  }\n}\n"
    );

    let gone = m.home().join(".claude/gone.json");
    fs::remove_file(&file).unwrap();
    link(&file, &gone);
    let (status, text) = message(set(&m, Cli::Claude, "capture", S::On).unwrap_err());
    assert_eq!(status, 409);
    assert!(text.contains("which is not there"), "{text}");
    assert!(!gone.exists());
}

/// A file that does not parse, or is not text, is never written.
#[test]
fn a_file_that_does_not_parse_is_refused_and_left_alone() {
    let m = Machine::new();
    plain(&m.home().join(".claude/skills"), "capture");
    plain(&m.home().join(".agents/skills"), "draw");
    let json = m.home().join(".claude/settings.json");
    write(&json, "{\"model\": \"x\",}\n");
    let (status, text) = message(set(&m, Cli::Claude, "capture", S::Off).unwrap_err());
    assert_eq!(status, 409);
    assert!(
        text.starts_with("~/.claude/settings.json is not valid JSON")
            && text.ends_with("so the page did not write it."),
        "{text}"
    );
    assert_eq!(read(&json), "{\"model\": \"x\",}\n");
    let toml = m.home().join(".codex/config.toml");
    write(&toml, "model = \n");
    let (_, text) = message(set(&m, Cli::Codex, "draw", S::Off).unwrap_err());
    assert!(
        text.starts_with("~/.codex/config.toml is not valid TOML"),
        "{text}"
    );
    assert_eq!(read(&toml), "model = \n");
    let dir = m.home().join(".claude");
    let left: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(left.is_empty(), "no temp file is left: {left:?}");
}

/// What another program writes between the read and the rename is never
/// lost: the write is refused and the file is left as it now is, and a
/// file made meanwhile where there was none is not replaced.
#[test]
fn a_file_changed_while_it_is_written_is_refused() {
    let m = Machine::new();
    let file = m.home().join(".claude/settings.json");
    write(&file, "{\"a\": 1}\n");
    let read_then = fs::read(&file).unwrap();
    write(&file, "{\"a\": 2}\n");
    assert!(matches!(
        crate::skills::write::replace(&file, Some(&read_then), b"{\"a\": 1, \"b\": 2}\n"),
        Err(crate::skills::write::Unwritten::Changed)
    ));
    assert_eq!(read(&file), "{\"a\": 2}\n");

    let new = m.home().join(".claude/settings.local.json");
    write(&new, "{\"made\": true}\n");
    assert!(matches!(
        crate::skills::write::replace(&new, None, b"{}\n"),
        Err(crate::skills::write::Unwritten::Changed)
    ));
    assert_eq!(read(&new), "{\"made\": true}\n");
    let left = fs::read_dir(m.home().join(".claude"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(left, 0);
}

/// Tidying up makes several skills Slash menu only in one write, and what
/// it answers puts each back as it was.
#[test]
fn the_tidy_up_and_its_undo_put_every_skill_back() {
    let m = Machine::new();
    for name in ["a", "b", "c"] {
        plain(&m.home().join(".claude/skills"), name);
    }
    let file = m.home().join(".claude/settings.json");
    let original =
        "{\n  \"skillOverrides\": {\n    \"b\": \"name-only\"\n  },\n  \"model\": \"x\"\n}\n";
    write(&file, original);
    let scan = m.scan();
    let all: Vec<api::SkillTo> = ["a", "b", "c"]
        .iter()
        .map(|n| to(one(&scan, n), S::UserInvocableOnly))
        .collect();
    let done = switch(&scan, Cli::Claude, &all).unwrap();
    let scan = m.scan();
    for n in ["a", "b", "c"] {
        assert_eq!(
            control(&scan, n, Cli::Claude).unwrap().state,
            S::UserInvocableOnly
        );
    }
    assert_eq!(
        done.before,
        [
            to(one(&scan, "a"), S::On),
            to(one(&scan, "b"), S::NameOnly),
            to(one(&scan, "c"), S::On),
        ]
    );
    switch(&scan, Cli::Claude, &done.before).unwrap();
    assert_eq!(read(&file), original);
}

/// A request naming a skill the scan did not find writes nothing, not
/// even for the ones it did.
#[test]
fn an_unknown_id_writes_nothing() {
    let m = Machine::new();
    plain(&m.home().join(".claude/skills"), "capture");
    let file = m.home().join(".claude/settings.json");
    write(&file, "{}\n");
    let scan = m.scan();
    let unknown = api::SkillTo {
        id: "0000000000000000".into(),
        state: S::Off,
    };
    let (status, text) = message(
        switch(
            &scan,
            Cli::Claude,
            &[to(one(&scan, "capture"), S::Off), unknown],
        )
        .unwrap_err(),
    );
    assert_eq!(
        (status, text.as_str()),
        (404, "There is no skill 0000000000000000.")
    );
    assert_eq!(read(&file), "{}\n");
    assert_eq!(message(switch(&scan, Cli::Claude, &[]).unwrap_err()).0, 400);
    assert_eq!(
        message(switch(&scan, Cli::Cursor, &[to(one(&scan, "capture"), S::Off)]).unwrap_err()).0,
        409
    );
}

/// After a write the page is told the state of the repository the file is
/// in, and that nothing was committed.
#[test]
fn a_write_says_how_its_repository_stands() {
    let m = Machine::new();
    plain(&m.home().join(".claude/skills"), "capture");
    let dotfiles = m.home().join("dotfiles");
    fs::create_dir_all(&dotfiles).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&dotfiles)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    };
    git(&["init", "-q"]);
    write(&dotfiles.join("settings.json"), "{}\n");
    write(&dotfiles.join(".gitignore"), "local.json\n");
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.com",
        "commit",
        "-qm",
        "start",
    ]);
    link(
        &m.home().join(".claude/settings.json"),
        &dotfiles.join("settings.json"),
    );
    let written = set(&m, Cli::Claude, "capture", S::Off).unwrap();
    assert_eq!(
        written.repo.as_deref(),
        Some("dotfiles: 1 changed file, nothing committed")
    );
}

/// An edit whose text, parsed again, says more than its one change is
/// never written, in JSON or TOML, whatever made it.
#[test]
fn an_edit_that_says_more_than_its_change_writes_nothing() {
    use crate::skills::edit::{codex_checked, json_checked};
    let m = Machine::new();
    let json = m.home().join(".claude/settings.json");
    let original = "{\n  \"model\": \"x\"\n}\n";
    write(&json, original);
    let wrong = "{\n  \"model\": \"y\",\n  \"skillOverrides\": {\"a\": \"off\"}\n}\n";
    let (status, text) = message(
        crate::skills::write::edit(&json, &m.places, |old| {
            json_checked(old, wrong.into(), "skillOverrides", "a", Some("\"off\""))
        })
        .unwrap_err(),
    );
    assert_eq!(status, 500);
    assert_eq!(
        text,
        "The change to `skillOverrides.a` in ~/.claude/settings.json could not be made safely: \
         the changed file said more than that change, so the page did not write it."
    );
    assert_eq!(read(&json), original);

    let toml = m.home().join(".codex/config.toml");
    let original = "model = \"x\"\n";
    write(&toml, original);
    let md = [m.home().join(".agents/skills/a/SKILL.md")];
    let wrong = format!(
        "model = \"y\"\n\n[[skills.config]]\npath = \"{}\"\nenabled = false\n",
        md[0].display()
    );
    let (status, _) = message(
        crate::skills::write::edit(&toml, &m.places, |old| {
            codex_checked(old, wrong.clone(), &md, "a", false)
        })
        .unwrap_err(),
    );
    assert_eq!(status, 500);
    assert_eq!(read(&toml), original);
    let left = ["/.claude", "/.codex"]
        .iter()
        .flat_map(|d| {
            fs::read_dir(format!("{}{d}", m.home().display()))
                .unwrap()
                .flatten()
        })
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(left, 0, "no temp file is left");
}
