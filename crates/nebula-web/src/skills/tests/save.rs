//! Editing a skill's file from its page, each in a temp home: what a save
//! writes and where, what it refuses, the problems a text not yet saved
//! has, and how its Undo puts the text back.

use super::*;
use std::os::unix::fs::PermissionsExt;

fn message(refusal: crate::Refusal) -> (u16, String) {
    (refusal.status.as_u16(), refusal.message)
}

/// The edit the detail of the skill `name` offers.
fn edit(scan: &Scan, name: &str) -> api::SkillEdit {
    let detail = detail(one(scan, name), &scan.places);
    detail
        .edit
        .unwrap_or_else(|| panic!("{name} is edited: {:?}", detail.uneditable))
}

fn save_as(
    scan: &Scan,
    name: &str,
    version: &str,
    text: &str,
) -> Result<(api::SkillDone, Option<Undo>), crate::Refusal> {
    save(
        scan,
        &api::SaveSkill {
            id: one(scan, name).skill.id.clone(),
            version: version.into(),
            text: text.into(),
        },
    )
}

/// `git args` in `dir`, which must succeed.
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// A skill in a committed git repository, `toolbox`, linked into
/// `~/.claude/skills`, its SKILL.md of mode 0640.
fn linked(m: &Machine) -> (PathBuf, PathBuf) {
    let toolbox = m.home().join("code/toolbox");
    let folder = plain(&toolbox.join("skills"), "deploy");
    let file = folder.join("SKILL.md");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    git(&toolbox, &["init", "-q"]);
    git(&toolbox, &["add", "-A"]);
    git(&toolbox, &["commit", "-qm", "start"]);
    let link_path = m.home().join(".claude/skills/deploy");
    link(&link_path, &folder);
    (file, link_path)
}

/// A save through a link writes the file the link leads to, keeps the
/// link a link and the file's mode, says how the repository stands, and
/// writes the bytes exactly as sent: CRLF, and no final newline.
#[test]
fn a_save_writes_the_links_target_byte_for_byte() {
    let m = Machine::new();
    let (file, link_path) = linked(&m);
    let scan = m.scan();
    let found = edit(&scan, "deploy");
    assert_eq!(found.text, fs::read_to_string(&file).unwrap());
    assert_eq!(found.note, None);
    let deploy = detail(one(&scan, "deploy"), &scan.places);
    assert_eq!(deploy.changes, None, "nothing to commit");

    let text = "---\r\nname: deploy\r\ndescription: Ships it.\r\n---\r\n\r\nShip it.";
    let (done, undo) = save_as(&scan, "deploy", &found.version, text).unwrap();
    assert_eq!(fs::read(&file).unwrap(), text.as_bytes());
    assert!(fs::symlink_metadata(&link_path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_link(&link_path).unwrap(), file.parent().unwrap());
    let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode, 0o640);
    assert_eq!(done.said, "Saved ~/code/toolbox/skills/deploy/SKILL.md.");
    assert_eq!(done.repos, ["toolbox: 1 changed file, nothing committed"]);
    assert!(undo.is_some());
    let leftovers: Vec<_> = fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, ["SKILL.md"], "no temp file is left beside it");

    let scan = m.scan();
    let deploy = detail(one(&scan, "deploy"), &scan.places);
    assert_eq!(deploy.body.as_deref(), Some("\r\nShip it."));
    assert_eq!(deploy.edit.unwrap().text, text);
    assert_eq!(deploy.changes.as_deref(), Some("1 changed file"));
}

/// A version other than the file's now is refused, and nothing is written.
#[test]
fn a_save_of_a_file_changed_since_it_was_read_is_refused() {
    let m = Machine::new();
    let (file, _) = linked(&m);
    let scan = m.scan();
    let found = edit(&scan, "deploy");
    let theirs = "---\nname: deploy\ndescription: Changed by an agent.\n---\n";
    fs::write(&file, theirs).unwrap();
    let refused = save_as(&scan, "deploy", &found.version, "mine").unwrap_err();
    assert_eq!(
        message(refused),
        (
            409,
            "~/code/toolbox/skills/deploy/SKILL.md changed on disk since the page read it, so \
             the page did not save it."
                .into()
        )
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), theirs);

    let scan = m.scan();
    let now = edit(&scan, "deploy");
    assert_ne!(now.version, found.version, "the version is of the bytes");
    assert_eq!(now.text, theirs);
    save_as(&scan, "deploy", &now.version, "mine").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "mine");
}

/// Undo puts back the text the save replaced, once, and only while the
/// file still holds what was saved.
#[test]
fn undo_puts_the_text_back_only_while_the_file_is_as_saved() {
    let m = Machine::new();
    let (file, _) = linked(&m);
    let before = fs::read_to_string(&file).unwrap();
    let scan = m.scan();
    let version = edit(&scan, "deploy").version;
    let (_, undo) = save_as(&scan, "deploy", &version, "mine\n").unwrap();
    let done = super::undo(&scan.places, &undo.unwrap()).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), before);
    assert_eq!(
        done.said,
        "Put back the text ~/code/toolbox/skills/deploy/SKILL.md had before the save."
    );
    assert_eq!(done.repos, ["toolbox: no changed file, nothing committed"]);
    let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode, 0o640);

    let scan = m.scan();
    let version = edit(&scan, "deploy").version;
    let (_, undo) = save_as(&scan, "deploy", &version, "mine\n").unwrap();
    fs::write(&file, "an agent's\n").unwrap();
    let refused = super::undo(&scan.places, &undo.clone().unwrap()).unwrap_err();
    assert_eq!(
        message(refused),
        (
            409,
            "~/code/toolbox/skills/deploy/SKILL.md was not put back: it changed since the \
             save, so it was left as it is."
                .into()
        )
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "an agent's\n");

    fs::remove_dir_all(file.parent().unwrap()).unwrap();
    let refused = super::undo(&scan.places, &undo.unwrap()).unwrap_err();
    assert_eq!(refused.status.as_u16(), 409, "{}", refused.message);
    assert!(
        !file.parent().unwrap().exists(),
        "a folder gone is not made again"
    );
}

/// What the scan would say of a text is said before it is saved, and does
/// not keep it from being saved.
#[test]
fn a_check_says_what_the_scan_would_and_does_not_block_the_save() {
    let m = Machine::new();
    let (file, _) = linked(&m);
    link(
        &m.home().join(".agents/skills/deploy"),
        file.parent().unwrap(),
    );
    let scan = m.scan();
    let id = one(&scan, "deploy").skill.id.clone();
    let kinds = |text: &str| -> Vec<P> {
        check(&scan, &id, text)
            .unwrap()
            .problems
            .iter()
            .map(|p| p.kind)
            .collect()
    };
    assert_eq!(kinds(&fs::read_to_string(&file).unwrap()), []);
    let checked = check(&scan, &id, "---\nname: [deploy\n---\nBody.\n").unwrap();
    assert_eq!(
        checked.problems.iter().map(|p| p.kind).collect::<Vec<_>>(),
        [P::BadFrontmatter]
    );
    assert_eq!(checked.body, "Body.\n");
    assert_eq!(kinds("---\nname: deploy\n"), [P::BadFrontmatter]);
    assert_eq!(
        kinds("---\nname: Deploy_It\ndescription: x\n---\n"),
        [P::NameMismatch, P::BadName]
    );
    assert_eq!(kinds("---\ndescription: x\n---\n"), [P::MissingField]);
    let long = format!(
        "---\nname: deploy\ndescription: {}\n---\n",
        "x".repeat(1025)
    );
    assert_eq!(kinds(&long), [P::LongDescription]);
    assert_eq!(
        check(&scan, "0123456789abcdef", "")
            .unwrap_err()
            .status
            .as_u16(),
        404
    );

    let version = edit(&scan, "deploy").version;
    save_as(&scan, "deploy", &version, "---\nname: [deploy\n---\n").unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "---\nname: [deploy\n---\n"
    );
    let scan = m.scan();
    assert_eq!(kinds_of(one(&scan, "deploy")), [P::BadFrontmatter]);
}

fn kinds_of(found: &Found) -> Vec<P> {
    found.skill.problems.iter().map(|p| p.kind).collect()
}

/// Each file a CLI, Claude's plugins or an admin keep is not edited, and
/// the page says why; a save of one is refused and writes nothing.
#[test]
fn a_file_the_user_does_not_keep_is_not_edited() {
    let m = Machine::new();
    let plugins = m.home().join(".claude/plugins");
    let cached = plugins.join("cache/tools-market/kit/1.0.0");
    write(
        &cached.join(".claude-plugin/plugin.json"),
        r#"{"name": "kit"}"#,
    );
    plain(&cached.join("skills"), "lint");
    write(
        &plugins.join("installed_plugins.json"),
        &serde_json::json!({"version": 2, "plugins": {
            "kit@tools-market": [{"scope": "user", "installPath": cached, "version": "1.0.0"}],
        }})
        .to_string(),
    );
    plain(&m.home().join(".claude/skills/synced/bucket-1"), "docs");
    plain(&m.home().join(".codex/skills/.system"), "imagegen");
    plain(&m.home().join(".cursor/skills-cursor"), "rules");
    plain(&m.places.claude_managed.join(".claude/skills"), "policy");
    plain(&m.places.codex_admin.join("skills"), "audit");
    link(
        &m.home().join(".claude/skills/gone"),
        &m.home().join("nowhere"),
    );
    let stale = plugins.join("cache/old-market/notes/1.0.0/skills/jot");
    plain(stale.parent().unwrap(), "jot");
    link(&m.home().join(".claude/skills/jot"), &stale);
    let outside = plain(&m.home().join("elsewhere"), "outside").join("SKILL.md");
    let sneaky = m.home().join(".claude/skills/sneaky");
    fs::create_dir_all(&sneaky).unwrap();
    symlink(&outside, sneaky.join("SKILL.md")).unwrap();

    let scan = m.scan();
    for (name, why) in [
        (
            "lint",
            "it is the `kit` plugin's copy, which an update replaces",
        ),
        ("docs", "it is synced from claude.ai, so it changes there"),
        (
            "imagegen",
            "it is built into Codex, and an update puts it back",
        ),
        (
            "rules",
            "it is built into Cursor, and an update puts it back",
        ),
        ("policy", "it is in Claude Code's managed settings folder"),
        ("audit", "it is in Codex's admin folder"),
        ("gone", "the link is broken, so there is no file"),
        (
            "jot",
            "it is in Claude's plugins folder, which updates replace",
        ),
    ] {
        let found = one(&scan, name);
        let detail = detail(found, &scan.places);
        let said = format!("Not edited here: {why}.");
        assert!(detail.edit.is_none(), "{name}");
        assert_eq!(detail.uneditable.as_deref(), Some(said.as_str()), "{name}");
        let file = found.real.as_ref().map(|r| r.join("SKILL.md"));
        let was = file.as_ref().map(|f| fs::read(f).unwrap());
        let refused = save_as(&scan, name, "0000000000000000", "mine").unwrap_err();
        assert_eq!(message(refused), (409, said), "{name}");
        assert_eq!(file.map(|f| fs::read(f).unwrap()), was, "{name}");
    }
    let detail = detail(one(&scan, "sneaky"), &scan.places);
    assert!(detail.edit.is_none());
    assert_eq!(
        detail.uneditable.as_deref(),
        Some("Not edited here: the page cannot read the file.")
    );
    let refused = save_as(&scan, "sneaky", "0000000000000000", "mine").unwrap_err();
    assert_eq!(refused.status.as_u16(), 409, "{}", refused.message);
    assert_eq!(
        fs::read_to_string(&outside).unwrap().lines().nth(1),
        Some("name: outside")
    );
}

/// The user's own skills are edited: one in no repository, a project's, a
/// command, a plugin's from a marketplace in a local folder, and one the
/// skills installer put there, which says its next update replaces it.
#[test]
fn the_users_own_files_are_edited() {
    let mut m = Machine::new();
    plain(&m.home().join(".claude/skills"), "notes");
    let shop = m.project("shop");
    plain(&shop.join(".claude/skills"), "release");
    write(&m.home().join(".claude/commands/review.md"), "Review it.\n");
    let market = m.home().join("src/market");
    write(
        &market.join(".claude-plugin/marketplace.json"),
        r#"{"name": "local", "plugins": [{"name": "kit", "source": "./kit"}]}"#,
    );
    write(
        &market.join("kit/.claude-plugin/plugin.json"),
        r#"{"name": "kit"}"#,
    );
    plain(&market.join("kit/skills"), "lint");
    let plugins = m.home().join(".claude/plugins");
    write(
        &plugins.join("installed_plugins.json"),
        &serde_json::json!({"version": 2, "plugins": {
            "kit@local": [{"scope": "user", "installPath": plugins.join("cache/local/kit/1"), "version": "1"}],
        }})
        .to_string(),
    );
    write(
        &plugins.join("known_marketplaces.json"),
        &serde_json::json!({
            "local": {"source": {"source": "directory", "path": market}, "installLocation": market},
        })
        .to_string(),
    );
    plain(&m.home().join(".agents/skills"), "watch");
    write(
        &m.home().join(".agents/.skill-lock.json"),
        r#"{"version": 3, "skills": {"watch": {"source": "acme/video"}}}"#,
    );

    let scan = m.scan();
    for name in ["notes", "release", "review", "lint"] {
        assert_eq!(edit(&scan, name).note, None, "{name}");
    }
    assert_eq!(
        edit(&scan, "watch").note.as_deref(),
        Some("The `npx skills` installer replaces this file when it next updates the skill.")
    );
    let version = edit(&scan, "review").version;
    save_as(&scan, "review", &version, "Review it twice.\n").unwrap();
    assert_eq!(
        fs::read_to_string(m.home().join(".claude/commands/review.md")).unwrap(),
        "Review it twice.\n"
    );
    let version = edit(&scan, "lint").version;
    save_as(&scan, "lint", &version, "mine\n").unwrap();
    assert_eq!(
        fs::read_to_string(market.join("kit/skills/lint/SKILL.md")).unwrap(),
        "mine\n"
    );
}

/// An id the scan did not give names nothing, so nothing is written; a
/// text over what the page reads is refused.
#[test]
fn an_unknown_id_or_a_text_too_long_writes_nothing() {
    let m = Machine::new();
    let (file, _) = linked(&m);
    let before = fs::read(&file).unwrap();
    let scan = m.scan();
    let version = edit(&scan, "deploy").version;
    for id in ["0123456789abcdef", "../../code/toolbox/skills/deploy", ""] {
        let to = api::SaveSkill {
            id: id.into(),
            version: version.clone(),
            text: "mine".into(),
        };
        let refused = save(&scan, &to).unwrap_err();
        assert_eq!(message(refused), (404, format!("There is no skill {id}.")));
    }
    let long = "x".repeat(1 << 20);
    let refused = save_as(&scan, "deploy", &version, &long).unwrap_err();
    assert_eq!(refused.status.as_u16(), 413, "{}", refused.message);
    assert_eq!(fs::read(&file).unwrap(), before);
}
