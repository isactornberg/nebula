//! The page's acts on a fix, each in a temp home: what each removes, what
//! it leaves alone, and how Undo puts it back.

use super::*;
use api::SkillAct as Act;
use std::os::unix::fs::PermissionsExt;

/// `git args` in `dir`, which must succeed.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn commit_all(dir: &Path) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "start"]);
}

fn fix_of(scan: &Scan, kind: P) -> api::SkillFix {
    let found: Vec<&api::SkillFix> = scan.fixes.iter().filter(|f| f.kind == kind).collect();
    assert_eq!(found.len(), 1, "one {kind:?} fix: {:#?}", scan.fixes);
    found[0].clone()
}

fn acts_of(fix: &api::SkillFix) -> Vec<Act> {
    fix.acts.iter().map(|a| a.act).collect()
}

fn message(refusal: crate::Refusal) -> (u16, String) {
    (refusal.status.as_u16(), refusal.message)
}

fn is_there(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// A project `shop` in a git repository whose `.claude/skills` holds three
/// links into `.agents/skills`, which is not there, a real skill and a
/// plain file, all committed.
fn shop_with_links(m: &mut Machine) -> PathBuf {
    let shop = m.project("shop");
    git(&shop, &["init", "-q"]);
    for name in ["vendor-auth", "vendor-cache", "vendor-search"] {
        link(
            &shop.join(".claude/skills").join(name),
            &Path::new("../../.agents/skills").join(name),
        );
    }
    plain(&shop.join(".claude/skills"), "release");
    write(&shop.join(".claude/skills/notes.txt"), "Keep me.\n");
    commit_all(&shop);
    shop
}

/// Remove links removes exactly the folder's broken links, and nothing a
/// link points at or anything else in the folder; Undo makes each again
/// with the target it had.
#[test]
fn removing_broken_links_takes_only_the_links_and_undo_puts_them_back() {
    let mut m = Machine::new();
    let shop = shop_with_links(&mut m);
    let skills = shop.join(".claude/skills");
    let scan = m.scan();
    let fix = fix_of(&scan, P::BrokenLink);
    assert_eq!(
        fix.acts,
        [api::SkillFixAct {
            act: Act::RemoveLinks,
            says: Some(
                "In shop, removing them shows as 3 deleted files, and nothing is committed.".into()
            ),
            refused: None,
        }]
    );

    let (done, put_back) = act(&scan, &fix.id, Act::RemoveLinks).unwrap();
    assert_eq!(done.said, "Removed 3 links from shop's .claude/skills.");
    assert_eq!(done.repos, ["shop: 3 deleted files, nothing committed"]);
    assert!(done.skipped.is_empty());
    for name in ["vendor-auth", "vendor-cache", "vendor-search"] {
        assert!(!is_there(&skills.join(name)), "{name} is gone");
    }
    assert!(
        skills.join("release/SKILL.md").is_file(),
        "the real skill stays"
    );
    assert_eq!(
        fs::read_to_string(skills.join("notes.txt")).unwrap(),
        "Keep me.\n"
    );
    assert!(
        m.scan().fixes.iter().all(|f| f.kind != P::BrokenLink),
        "the fix is gone from the list"
    );

    let back = undo(m.scan().places(), &put_back.unwrap()).unwrap();
    assert_eq!(
        back.said,
        "Put back 3 links in shop's .claude/skills, each pointing where it did."
    );
    assert_eq!(back.repos, ["shop: no changed file, nothing committed"]);
    assert_eq!(
        fs::read_link(skills.join("vendor-cache")).unwrap(),
        Path::new("../../.agents/skills/vendor-cache")
    );
    assert_eq!(
        fix_of(&m.scan(), P::BrokenLink).id,
        fix.id,
        "the same fix again"
    );
}

/// A link that changed after the scan, pointed elsewhere, at something now
/// there or swapped for a real folder, is left as it is and said; with no
/// link left as the scan found it, nothing is removed.
#[test]
fn a_link_that_changed_since_the_scan_is_left_alone() {
    let mut m = Machine::new();
    let shop = shop_with_links(&mut m);
    let skills = shop.join(".claude/skills");
    link(
        &skills.join("vendor-mail"),
        Path::new("../../.agents/skills/vendor-mail"),
    );
    let scan = m.scan();
    let fix = fix_of(&scan, P::BrokenLink);

    fs::remove_file(skills.join("vendor-auth")).unwrap();
    link(&skills.join("vendor-auth"), Path::new("../../elsewhere"));
    fs::create_dir_all(shop.join(".agents/skills/vendor-cache")).unwrap();
    fs::remove_file(skills.join("vendor-search")).unwrap();
    plain(&skills, "vendor-search");

    let (done, _) = act(&scan, &fix.id, Act::RemoveLinks).unwrap();
    assert_eq!(done.said, "Removed 1 link from shop's .claude/skills.");
    assert_eq!(
        done.skipped,
        [
            "`vendor-auth` points somewhere else now, so it was left as it is.",
            "`vendor-cache` points at something that is there now, so it was left as it is.",
            "`vendor-search` is no longer a link, so it was left as it is.",
        ]
    );
    assert!(!is_there(&skills.join("vendor-mail")));
    assert_eq!(
        fs::read_link(skills.join("vendor-auth")).unwrap(),
        Path::new("../../elsewhere")
    );
    assert!(is_link(&skills.join("vendor-cache")));
    assert!(skills.join("vendor-search/SKILL.md").is_file());

    // The same scan's fix again: not one of its links is as it found them.
    let (status, text) = message(act(&scan, &fix.id, Act::RemoveLinks).unwrap_err());
    assert_eq!(status, 409);
    assert!(
        text.starts_with("Nothing was removed. `vendor-auth` points somewhere"),
        "{text}"
    );
    assert!(skills.join("vendor-search/SKILL.md").is_file());
}

/// Links git does not track are said to change nothing in it, and links in
/// no repository to be in none.
#[test]
fn removing_links_says_first_what_git_will_show() {
    let mut m = Machine::new();
    let shop = m.project("shop");
    git(&shop, &["init", "-q"]);
    write(&shop.join(".gitignore"), ".claude/*\n");
    commit_all(&shop);
    link(
        &shop.join(".claude/skills/gone"),
        Path::new("../../nowhere/gone"),
    );
    link(
        &m.home().join(".claude/skills/old"),
        &m.home().join("src/old"),
    );
    let says: Vec<Option<String>> = m
        .scan()
        .fixes
        .iter()
        .map(|f| f.acts[0].says.clone())
        .collect();
    assert_eq!(
        says,
        [
            Some("It is in no git repository.".into()),
            Some("Git ignores it in shop, so removing it changes nothing it tracks.".into()),
        ]
    );
    let scan = m.scan();
    let (done, _) = act(&scan, &scan.fixes[1].id, Act::RemoveLinks).unwrap();
    assert_eq!(done.repos, ["git ignores it in shop"]);
}

/// A folder of skills that is a link into a repository is in that
/// repository, for what removing its links shows and for its Undo.
#[test]
fn links_in_a_folder_linked_into_a_repository_are_in_it() {
    let m = Machine::new();
    let dotfiles = m.home().join("dotfiles");
    let skills = dotfiles.join("claude/skills");
    fs::create_dir_all(&dotfiles).unwrap();
    git(&dotfiles, &["init", "-q"]);
    link(&skills.join("old"), Path::new("../../gone/old"));
    commit_all(&dotfiles);
    fs::remove_dir_all(m.home().join(".claude")).unwrap();
    fs::create_dir_all(m.home().join(".claude")).unwrap();
    link(&m.home().join(".claude/skills"), &skills);
    let scan = m.scan();
    let fix = fix_of(&scan, P::BrokenLink);
    assert_eq!(
        fix.acts[0].says.as_deref(),
        Some("In dotfiles, removing it shows as 1 deleted file, and nothing is committed.")
    );
    let (done, put_back) = act(&scan, &fix.id, Act::RemoveLinks).unwrap();
    assert_eq!(done.said, "Removed 1 link from ~/.claude/skills.");
    assert_eq!(done.repos, ["dotfiles: 1 deleted file, nothing committed"]);
    assert!(
        is_link(&m.home().join(".claude/skills")),
        "the folder's link stays"
    );
    let back = undo(m.scan().places(), &put_back.unwrap()).unwrap();
    assert_eq!(back.repos, ["dotfiles: no changed file, nothing committed"]);
}

/// A project `shop` in a git repository with a skill and the older command
/// of its name, its mode 755, committed.
fn shop_with_command(m: &mut Machine) -> (PathBuf, PathBuf) {
    let shop = m.project("shop");
    git(&shop, &["init", "-q"]);
    plain(&shop.join(".claude/skills"), "translate");
    let command = shop.join(".claude/commands/translate.md");
    write(&command, "Translate the strings.\n");
    fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).unwrap();
    commit_all(&shop);
    (shop, command)
}

/// The command a skill hides is removed only while git holds it as it is,
/// checked again right before, and Undo writes it back from git with the
/// bytes and mode it had, only where nothing is.
#[test]
fn a_command_is_removed_only_when_git_can_bring_it_back() {
    let mut m = Machine::new();
    let (shop, command) = shop_with_command(&mut m);
    let scan = m.scan();
    let fix = fix_of(&scan, P::DuplicateName);
    assert_eq!(
        acts_of(&fix),
        [Act::RemoveCommand, Act::Keep],
        "no Compare for a command"
    );
    assert_eq!(
        fix.acts[0].says.as_deref(),
        Some(
            "It is committed in shop, so removing it shows there as 1 deleted file, nothing is \
             committed, and Undo brings it back from git."
        )
    );

    write(&command, "Translate them all.\n");
    let changed = m.scan();
    let refused = &fix_of(&changed, P::DuplicateName).acts[0];
    let why = "The page leaves the command to you: it has changes not committed in git, so it \
               could not be brought back as it is.";
    assert_eq!(refused.refused.as_deref(), Some(why));
    assert_eq!(
        message(act(&changed, &refused_id(&changed), Act::RemoveCommand).unwrap_err()),
        (409, why.to_string())
    );
    // The scan from before the change still offers it; the act looks again.
    let (status, text) = message(act(&scan, &fix.id, Act::RemoveCommand).unwrap_err());
    assert_eq!(status, 409);
    assert_eq!(
        text,
        "The page did not remove shop's .claude/commands/translate.md: it has changes not \
         committed in git, so it could not be brought back as it is."
    );
    assert_eq!(
        fs::read_to_string(&command).unwrap(),
        "Translate them all.\n"
    );

    git(&shop, &["checkout", "--", "."]);
    let scan = m.scan();
    let (done, held) = act(&scan, &fix.id, Act::RemoveCommand).unwrap();
    assert_eq!(done.said, "Removed shop's .claude/commands/translate.md.");
    assert_eq!(done.repos, ["shop: 1 deleted file, nothing committed"]);
    assert!(!is_there(&command));
    assert!(shop.join(".claude/skills/translate/SKILL.md").is_file());
    assert!(m.scan().fixes.is_empty(), "{:#?}", m.scan().fixes);

    let held = held.unwrap();
    let back = undo(m.scan().places(), &held).unwrap();
    assert_eq!(
        back.said,
        "Put back shop's .claude/commands/translate.md from git, as it was."
    );
    assert_eq!(back.repos, ["shop: no changed file, nothing committed"]);
    assert_eq!(
        fs::read_to_string(&command).unwrap(),
        "Translate the strings.\n"
    );
    assert_eq!(
        fs::metadata(&command).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(git(&shop, &["status", "--porcelain"]), "");

    // Something at its place again is left there.
    let (_, held) = act(&m.scan(), &fix.id, Act::RemoveCommand).unwrap();
    write(&command, "A new one.\n");
    assert_eq!(
        message(undo(m.scan().places(), &held.unwrap()).unwrap_err()),
        (
            409,
            "shop's .claude/commands/translate.md was not put back: something is at its place \
             again."
                .to_string()
        )
    );
    assert_eq!(fs::read_to_string(&command).unwrap(), "A new one.\n");
}

fn refused_id(scan: &Scan) -> String {
    fix_of(scan, P::DuplicateName).id
}

/// A command git does not hold, or that sessions run where no skill of its
/// name hides it, is never removed: the row says why instead.
#[test]
fn a_command_git_cannot_bring_back_is_left_to_the_user() {
    let mut m = Machine::new();
    let shop = m.project("shop");
    git(&shop, &["init", "-q"]);
    plain(&shop.join(".claude/skills"), "translate");
    commit_all(&shop);
    let command = shop.join(".claude/commands/translate.md");
    write(&command, "Translate.\n");
    plain(&m.home().join(".claude/skills"), "review");
    write(&m.home().join(".claude/commands/review.md"), "Review.\n");
    let docs = m.project("docs");
    plain(&docs.join(".claude/skills"), "deploy");
    write(&m.home().join(".claude/commands/deploy.md"), "Deploy.\n");

    let scan = m.scan();
    let refused: Vec<(String, Option<String>)> = scan
        .fixes
        .iter()
        .map(|f| (f.title.clone(), f.acts[0].refused.clone()))
        .collect();
    let leave = |why: &str| Some(format!("The page leaves the command to you: it {why}."));
    assert_eq!(
        refused,
        [
            (
                "review twice".into(),
                leave("is in no git repository, so it could not be brought back")
            ),
            (
                "translate twice".into(),
                leave("is not committed in git, so it could not be brought back")
            ),
            (
                "deploy twice".into(),
                leave("loads where no skill of its name does")
            ),
        ]
    );
    for fix in &scan.fixes {
        assert_eq!(
            message(act(&scan, &fix.id, Act::RemoveCommand).unwrap_err()).0,
            409
        );
    }
    assert!(command.is_file());
    assert!(m.home().join(".claude/commands/review.md").is_file());
    assert!(m.home().join(".claude/commands/deploy.md").is_file());
}

/// Two copies of one skill, `capture`, in Claude's and in Codex's folder,
/// which Cursor reads both of.
fn two_copies(m: &Machine) -> (PathBuf, PathBuf) {
    let claude = plain(&m.home().join(".claude/skills"), "capture");
    let codex = plain(&m.home().join(".codex/skills"), "capture");
    (claude.join("SKILL.md"), codex.join("SKILL.md"))
}

/// Keep both hides a fix while its files read as they did, a touch that
/// changes nothing included, and its problems are kept; a change to one
/// brings it back, and Undo or Unkeep puts it back on the list.
#[test]
fn keep_hides_a_fix_until_one_of_its_files_changes() {
    let m = Machine::new();
    let (claude, codex) = two_copies(&m);
    let scan = m.scan();
    let fix = fix_of(&scan, P::DuplicateName);
    assert_eq!(fix.title, "capture twice");
    assert_eq!(acts_of(&fix), [Act::Keep, Act::Compare]);

    let (done, kept) = act(&scan, &fix.id, Act::Keep).unwrap();
    assert_eq!(
        done.said,
        "Kept capture twice off the list until one of its files changes."
    );
    let scan = m.scan();
    assert!(scan.fixes.is_empty(), "{:#?}", scan.fixes);
    assert_eq!(scan.kept.len(), 1);
    assert_eq!(scan.kept[0].id, fix.id);
    assert_eq!(acts_of(&scan.kept[0]), [Act::Unkeep, Act::Compare]);
    for f in named(&scan, "capture") {
        assert!(
            f.skill.problems.iter().all(|p| p.kept),
            "{:#?}",
            f.skill.problems
        );
    }
    let json: serde_json::Value = serde_json::from_str(&scan.json).unwrap();
    assert_eq!(json["kept"][0]["id"], fix.id.as_str());

    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&claude)
        .unwrap()
        .set_modified(later)
        .unwrap();
    assert!(m.scan().fixes.is_empty(), "a touch changes nothing");

    let back = undo(m.scan().places(), &kept.unwrap()).unwrap();
    assert_eq!(back.said, "capture twice is on the list again.");
    assert_eq!(fix_of(&m.scan(), P::DuplicateName).id, fix.id);

    act(&m.scan(), &fix.id, Act::Keep).unwrap();
    write(&codex, &read(&codex).replace("Does capture.", "Captures."));
    let scan = m.scan();
    assert_eq!(
        fix_of(&scan, P::DuplicateName).id,
        fix.id,
        "back once a file changed"
    );
    assert!(scan.kept.is_empty());
    assert!(named(&scan, "capture")
        .iter()
        .all(|f| f.skill.problems.iter().all(|p| !p.kept)));

    act(&scan, &fix.id, Act::Keep).unwrap();
    let file: serde_json::Value = serde_json::from_str(&read(&m.places.kept)).unwrap();
    assert_eq!(
        file["kept"].as_array().unwrap().len(),
        1,
        "the spent entry went: {file}"
    );
    let scan = m.scan();
    let (done, again) = act(&scan, &fix.id, Act::Unkeep).unwrap();
    assert_eq!(done.said, "capture twice is on the list again.");
    assert!(again.is_none());
    assert_eq!(read(&m.places.kept), "{\n  \"kept\": []\n}\n");
    assert_eq!(fix_of(&m.scan(), P::DuplicateName).id, fix.id);
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// Keep is for a choice the user may decline: two skills of one name and
/// one only one CLI loads, not a broken link or a skill's own file.
#[test]
fn keep_is_offered_only_where_the_cure_is_a_choice() {
    let m = Machine::new();
    plain(&m.home().join(".cursor/skills"), "cursor-only");
    skill(
        &m.home().join(".agents/skills"),
        "tidy",
        "name: Tidy_Up\ndescription: Tidies.",
    );
    link(&m.home().join(".claude/skills/old"), &m.home().join("gone"));
    let scan = m.scan();
    let offered: Vec<(P, Vec<Act>)> = scan.fixes.iter().map(|f| (f.kind, acts_of(f))).collect();
    assert_eq!(
        offered,
        [
            (P::BrokenLink, vec![Act::RemoveLinks]),
            (P::NameMismatch, vec![]),
            (P::OneCli, vec![Act::Keep]),
        ]
    );
    let mismatch = &scan.fixes[1];
    assert_eq!(
        message(act(&scan, &mismatch.id, Act::Keep).unwrap_err()),
        (409, "The page does not do that for Tidy_Up.".to_string())
    );
    assert_eq!(
        message(act(&scan, &scan.fixes[0].id, Act::Keep).unwrap_err()).0,
        409
    );
    assert!(!is_there(&m.places.kept), "nothing was written");
    let (done, _) = act(&scan, &scan.fixes[2].id, Act::Keep).unwrap();
    assert_eq!(
        done.said,
        "Kept cursor-only off the list until one of its files changes."
    );
}

/// Compare reads both copies, and says when they are the same byte for
/// byte; a fix that is not two copies has nothing to compare.
#[test]
fn compare_reads_two_copies_and_says_when_they_are_the_same() {
    let m = Machine::new();
    let (claude, codex) = two_copies(&m);
    let scan = m.scan();
    let fix = fix_of(&scan, P::DuplicateName);
    let same = compare(&scan, &fix.id).unwrap();
    assert!(same.same);
    let files: Vec<Option<String>> = same.files.iter().map(|f| f.file.clone()).collect();
    assert_eq!(
        files,
        [
            Some(claude.display().to_string()),
            Some(codex.display().to_string())
        ]
    );
    assert_eq!(same.files[0].text.as_deref(), Some(read(&claude).as_str()));

    write(
        &codex,
        &read(&codex).replace("What it does.", "What it does, for Codex."),
    );
    let differ = compare(&m.scan(), &fix.id).unwrap();
    assert!(!differ.same);
    assert!(differ.files[1]
        .text
        .as_deref()
        .unwrap()
        .contains("for Codex"));

    plain(&m.home().join(".codex/skills/.system"), "imagegen");
    plain(&m.home().join(".agents/skills"), "imagegen");
    let scan = m.scan();
    let builtin = scan
        .fixes
        .iter()
        .find(|f| f.title == "imagegen twice")
        .unwrap();
    assert_eq!(acts_of(builtin), [Act::Keep], "a built-in is not a copy");
    assert_eq!(
        message(compare(&scan, &builtin.id).unwrap_err()),
        (
            409,
            "imagegen twice is not two copies of one skill, so there is nothing to compare."
                .to_string()
        )
    );
}

/// An id the scan did not hand out names nothing, so nothing is written or
/// removed; Compare is a read, not an act.
#[test]
fn an_id_the_list_did_not_give_does_nothing() {
    let mut m = Machine::new();
    let shop = shop_with_links(&mut m);
    let scan = m.scan();
    for id in ["0123456789abcdef", "../../etc/passwd", ""] {
        for a in [Act::RemoveLinks, Act::RemoveCommand, Act::Keep, Act::Unkeep] {
            assert_eq!(message(act(&scan, id, a).unwrap_err()).0, 404, "{id} {a:?}");
        }
        assert_eq!(message(compare(&scan, id).unwrap_err()).0, 404);
    }
    let fix = fix_of(&scan, P::BrokenLink);
    assert_eq!(
        message(act(&scan, &fix.id, Act::Compare).unwrap_err()).0,
        400
    );
    assert!(is_link(&shop.join(".claude/skills/vendor-auth")));
    assert!(!is_there(&m.places.kept));
}
