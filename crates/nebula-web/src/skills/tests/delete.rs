//! Deleting a skill from its page, each in a temp home whose Trash is a
//! folder of its own: what goes and what git will show, what it refuses,
//! and how Undo puts each thing back from the Trash.

use super::*;

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

fn message(refusal: crate::Refusal) -> (u16, String) {
    (refusal.status.as_u16(), refusal.message)
}

fn is_there(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// The delete the detail of the skill `name` offers.
fn offer(scan: &Scan, name: &str) -> api::SkillDelete {
    let detail = detail(one(scan, name), &scan.places);
    detail
        .delete
        .unwrap_or_else(|| panic!("{name} is deleted: {:?}", detail.undeletable))
}

fn delete_as(
    scan: &Scan,
    name: &str,
    version: &str,
) -> Result<(api::SkillDone, Option<Undo>), crate::Refusal> {
    delete(
        scan,
        &api::DeleteSkill {
            id: one(scan, name).skill.id.clone(),
            version: version.into(),
        },
    )
}

/// What the Trash's `files/` holds, by name.
fn trashed(m: &Machine) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(m.places.trash.as_ref().unwrap().join("files"))
        .map(|d| {
            d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// A skill in a committed repository linked into `~/.claude/skills` goes
/// with its link, the page saying first what git will show, each to the
/// Trash with its `.trashinfo`; Undo puts both back as they were.
#[test]
fn a_linked_skill_goes_to_the_trash_with_its_link_and_comes_back() {
    let m = Machine::new();
    let toolbox = m.home().join("code/toolbox");
    let folder = plain(&toolbox.join("skills"), "deploy");
    write(&folder.join("scripts/run.sh"), "echo run\n");
    git(&toolbox, &["init", "-q"]);
    git(&toolbox, &["add", "-A"]);
    git(&toolbox, &["commit", "-qm", "start"]);
    let link_path = m.home().join(".claude/skills/deploy");
    link(&link_path, &folder);
    let before = fs::read(folder.join("SKILL.md")).unwrap();

    let scan = m.scan();
    let offered = offer(&scan, "deploy");
    assert_eq!(
        offered.goes,
        [
            api::SkillGoes {
                path: folder.display().to_string(),
                link: false
            },
            api::SkillGoes {
                path: link_path.display().to_string(),
                link: true
            },
        ]
    );
    assert_eq!(
        offered.says,
        ["In toolbox, this shows as 2 deleted files, and nothing is committed."]
    );
    assert_eq!(offered.note, None);

    let (done, back) = delete_as(&scan, "deploy", &offered.version).unwrap();
    assert_eq!(
        done.said,
        "Deleted deploy: its folder and 1 link are in the Trash."
    );
    assert_eq!(done.repos, ["toolbox: 2 deleted files, nothing committed"]);
    assert!(done.skipped.is_empty(), "{:?}", done.skipped);
    assert!(!is_there(&folder) && !is_there(&link_path));
    assert_eq!(trashed(&m), ["deploy", "deploy.2"]);
    let info = fs::read_to_string(
        m.places
            .trash
            .as_ref()
            .unwrap()
            .join("info/deploy.trashinfo"),
    )
    .unwrap();
    assert!(
        info.starts_with(&format!("[Trash Info]\nPath={}\n", folder.display())),
        "{info}"
    );
    assert!(m.scan().found.iter().all(|f| f.skill.name != "deploy"));

    let done = undo(&m.places, &back.unwrap()).unwrap();
    assert_eq!(
        done.said,
        "Put back deploy from the Trash: its folder and 1 link."
    );
    assert_eq!(done.repos, ["toolbox: no changed file, nothing committed"]);
    assert_eq!(fs::read(folder.join("SKILL.md")).unwrap(), before);
    assert_eq!(
        fs::read_to_string(folder.join("scripts/run.sh")).unwrap(),
        "echo run\n"
    );
    assert_eq!(fs::read_link(&link_path).unwrap(), folder);
    assert!(trashed(&m).is_empty());
    let infos = fs::read_dir(m.places.trash.as_ref().unwrap().join("info")).unwrap();
    assert_eq!(infos.count(), 0, "each .trashinfo goes with its file");
}

/// A plain folder of the user's, in no repository and with no link to it,
/// is only that folder, and the page says the Trash holds its only copy.
#[test]
fn a_plain_folder_says_the_trash_holds_its_only_copy() {
    let m = Machine::new();
    let folder = plain(&m.home().join(".claude/skills"), "sketch");
    let scan = m.scan();
    let offered = offer(&scan, "sketch");
    assert_eq!(offered.goes.len(), 1);
    assert_eq!(
        offered.says,
        ["The folder is in no git repository, so the Trash holds its only copy."]
    );
    let (done, _) = delete_as(&scan, "sketch", &offered.version).unwrap();
    assert_eq!(done.said, "Deleted sketch: its folder is in the Trash.");
    assert!(done.repos.is_empty());
    assert!(!is_there(&folder));
}

/// An installer's skill takes its entry in the lock with it, every other
/// byte of the lock kept, and Undo puts the lock back as it was.
#[test]
fn an_installer_skill_takes_its_lock_entry_and_undo_puts_it_back() {
    let m = Machine::new();
    let installed = plain(&m.home().join(".agents/skills"), "watch");
    plain(&m.home().join(".agents/skills"), "lint");
    link(&m.home().join(".claude/skills/watch"), &installed);
    let lock = m.home().join(".agents/.skill-lock.json");
    let text = "{\n  \"version\": 3,\n  \"skills\": {\n    \"watch\": {\n      \"source\": \
                \"acme/video\",\n      \"sourceType\": \"github\"\n    },\n    \"lint\": {\n      \
                \"source\": \"acme/lint\",\n      \"sourceType\": \"github\"\n    }\n  }\n}";
    write(&lock, text);
    let scan = m.scan();
    let offered = offer(&scan, "watch");
    assert_eq!(
        offered.note.as_deref(),
        Some(
            "Its entry in ~/.agents/.skill-lock.json goes too, so `npx skills update` does not \
             install it again."
        )
    );
    let (done, back) = delete_as(&scan, "watch", &offered.version).unwrap();
    assert_eq!(
        done.said,
        "Deleted watch: its folder and 1 link are in the Trash, and its entry in \
         ~/.agents/.skill-lock.json is gone."
    );
    assert_eq!(
        fs::read_to_string(&lock).unwrap(),
        "{\n  \"version\": 3,\n  \"skills\": {\n    \"lint\": {\n      \"source\": \
         \"acme/lint\",\n      \"sourceType\": \"github\"\n    }\n  }\n}"
    );
    let done = undo(&m.places, &back.unwrap()).unwrap();
    assert_eq!(
        done.said,
        "Put back watch from the Trash: its folder and 1 link, and its entry in \
         ~/.agents/.skill-lock.json."
    );
    assert_eq!(fs::read_to_string(&lock).unwrap(), text);
    assert!(is_there(&installed.join("SKILL.md")));
}

/// A `SKILL.md` changed since the page read it, a link that leads
/// elsewhere now, and a skill a CLI keeps are each refused, and nothing
/// goes.
#[test]
fn what_changed_since_it_was_read_or_is_not_yours_is_refused() {
    let m = Machine::new();
    let folder = plain(&m.home().join("code/kit"), "deploy");
    let link_path = m.home().join(".claude/skills/deploy");
    link(&link_path, &folder);
    let scan = m.scan();
    let offered = offer(&scan, "deploy");

    write(
        &folder.join("SKILL.md"),
        "---\nname: deploy\n---\nChanged.\n",
    );
    let refused = delete_as(&scan, "deploy", &offered.version).unwrap_err();
    assert_eq!(
        message(refused),
        (
            409,
            "The page deleted nothing: ~/code/kit/deploy/SKILL.md changed since the list was \
             read."
                .into()
        )
    );
    let scan = m.scan();
    let refused = delete_as(&scan, "deploy", &offered.version).unwrap_err();
    assert_eq!(
        message(refused),
        (
            409,
            "deploy changed on disk since the page read it, so the page deleted nothing.".into()
        )
    );

    let other = plain(&m.home().join("code/other"), "deploy");
    fs::remove_file(&link_path).unwrap();
    link(&link_path, &other);
    let refused = delete_as(&scan, "deploy", &offered.version).unwrap_err();
    assert_eq!(
        message(refused),
        (
            409,
            "The page deleted nothing: ~/.claude/skills/deploy leads somewhere else now.".into()
        )
    );
    let seen = detail(one(&scan, "deploy"), &scan.places);
    assert_eq!(
        seen.undeletable.as_deref(),
        Some("Not deleted here: ~/.claude/skills/deploy leads somewhere else now.")
    );
    assert!(is_there(&folder) && is_there(&other) && is_there(&link_path));
    assert!(trashed(&m).is_empty());

    plain(&m.home().join(".codex/skills/.system"), "imagegen");
    let scan = m.scan();
    let seen = detail(one(&scan, "imagegen"), &scan.places);
    assert!(seen.delete.is_none());
    let why = "Not edited or deleted here: it is built into Codex, and an update puts it back.";
    assert_eq!(seen.undeletable.as_deref(), Some(why));
    assert_eq!(seen.uneditable.as_deref(), Some(why));
    let refused = delete_as(&scan, "imagegen", "0").unwrap_err();
    assert_eq!(
        message(refused),
        (
            409,
            "Not deleted here: it is built into Codex, and an update puts it back.".into()
        )
    );
}

/// A broken link is only its link, and Undo makes nothing where something
/// is at its place again.
#[test]
fn a_broken_link_goes_alone_and_undo_leaves_what_is_there_now() {
    let m = Machine::new();
    let link_path = m.home().join(".claude/skills/old");
    link(&link_path, &m.home().join("gone"));
    let scan = m.scan();
    let detail = detail(one(&scan, "old"), &scan.places);
    assert_eq!(
        detail.uneditable.as_deref(),
        Some("Not edited here: the link is broken, so there is no file.")
    );
    let offered = detail.delete.unwrap();
    assert_eq!(offered.goes.len(), 1);
    assert!(offered.goes[0].link);
    assert!(offered.says.is_empty(), "{:?}", offered.says);
    let (done, back) = delete_as(&scan, "old", &offered.version).unwrap();
    assert_eq!(done.said, "Deleted old: its link is in the Trash.");
    assert!(!is_there(&link_path));

    write(&link_path, "mine");
    let refused = undo(&m.places, &back.unwrap()).unwrap_err();
    assert_eq!(
        message(refused),
        (
            409,
            "Nothing was put back. ~/.claude/skills/old was not put back: something is at its \
             place again."
                .into()
        )
    );
    assert_eq!(fs::read_to_string(&link_path).unwrap(), "mine");
    assert_eq!(trashed(&m), ["old"]);
}

/// macOS's own Trash takes a link as the link, says where it put it, and
/// gives it back; nothing of the test is left in it.
#[cfg(target_os = "macos")]
#[test]
fn the_macos_trash_takes_a_link_and_gives_it_back() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = fs::canonicalize(tmp.path()).unwrap();
    let target = dir.join("target");
    write(&target.join("SKILL.md"), "kept\n");
    let link_path = dir.join(format!("nebula-test-link-{}", std::process::id()));
    link(&link_path, &target);
    let there = super::super::trash::trash(&link_path, None).unwrap();
    let gone = !is_there(&link_path);
    let as_link = fs::symlink_metadata(&there).is_ok_and(|m| m.file_type().is_symlink());
    let read = fs::read_link(&there).ok();
    let back = super::super::trash::put_back(&there, &link_path);
    if back.is_err() {
        let _ = fs::remove_file(&there);
    }
    assert!(gone && as_link, "{there:?}");
    assert_eq!(read.as_ref(), Some(&target));
    back.unwrap();
    assert_eq!(fs::read_link(&link_path).unwrap(), target);
    assert!(!is_there(&there));
}
