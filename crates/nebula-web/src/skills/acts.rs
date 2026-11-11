//! The page's acts on a fix, each by the id the list gave it: a folder's
//! broken links removed, a command a skill hides removed when git can bring
//! it back, a fix kept off the list until one of its files changes, and two
//! copies of a skill read side by side. Every path is the scan's, looked at
//! again right before anything is removed: nothing but a link that still
//! points where it did at nothing, or a file git holds as it is, is ever
//! removed, and nothing is committed. Each act hands back its [`Undo`],
//! which puts a thing back only where nothing is.

use super::fixes::{count, project_of, shown};
use super::write::{
    conflict, git, in_repo, listed, refuse, replace, repo_root, repo_status, Unwritten,
};
use super::*;
use crate::Refusal;
use api::{SkillAct as Act, SkillProblemKind as P};
use axum::http::StatusCode;
use serde::Serialize;

/// What an act can put back.
#[derive(Debug, Clone)]
pub enum Undo {
    /// The links it removed from the folder `at` names, each with the
    /// target it had.
    Links {
        at: String,
        links: Vec<(PathBuf, PathBuf)>,
    },
    /// The command it removed, `at` naming it, which git holds.
    Command { at: String, held: Held },
    /// The fix it kept off the list.
    Kept { fix: String, title: String },
    /// The text a save replaced in `file`, `at` naming it, and the text it
    /// saved there.
    Saved {
        at: String,
        file: PathBuf,
        saved: Vec<u8>,
        before: Vec<u8>,
    },
}

/// A file git holds as it is on disk: its repository, and the blob and
/// mode HEAD has for it.
#[derive(Debug, Clone)]
pub struct Held {
    file: PathBuf,
    root: PathBuf,
    blob: String,
    mode: u32,
}

/// The fixes the user kept, in nebula's data dir.
#[derive(Debug, Default, Serialize, Deserialize)]
struct KeptFile {
    kept: Vec<Kept>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Kept {
    fix: String,
    /// [`print`] of its files when it was kept.
    print: String,
    /// What it was called then, for whoever reads the file.
    title: String,
}

fn can(act: Act) -> api::SkillFixAct {
    api::SkillFixAct {
        act,
        says: None,
        refused: None,
    }
}

/// What the page can do about a fix of `kind` about `members`: remove a
/// folder's broken links; for skills of one name, remove the command they
/// hide, keep them all, and compare two copies; keep a skill one CLI loads.
pub(super) fn acts(kind: P, members: &[&Found]) -> Vec<api::SkillFixAct> {
    let mut out = Vec::new();
    match kind {
        P::BrokenLink if !members.is_empty() => out.push(api::SkillFixAct {
            says: Some(links_say(members)),
            ..can(Act::RemoveLinks)
        }),
        P::DuplicateName => {
            if let Some(command) = members
                .iter()
                .find(|f| f.skill.kind == api::SkillKind::Command)
            {
                let held = if hidden(command, members) {
                    held(command)
                } else {
                    Err("loads where no skill of its name does".into())
                };
                out.push(match held {
                    Ok(held) => api::SkillFixAct {
                        says: Some(command_say(&held)),
                        ..can(Act::RemoveCommand)
                    },
                    Err(why) => api::SkillFixAct {
                        refused: Some(format!("The page leaves the command to you: it {why}.")),
                        ..can(Act::RemoveCommand)
                    },
                });
            }
            out.push(can(Act::Keep));
            if copies(members) {
                out.push(can(Act::Compare));
            }
        }
        P::OneCli => out.push(can(Act::Keep)),
        _ => {}
    }
    out
}

/// Two copies of one skill: two of them, each a skill of the user's own
/// that is there to read.
fn copies(members: &[&Found]) -> bool {
    members.len() == 2
        && members.iter().all(|f| {
            f.skill.kind == api::SkillKind::Skill
                && f.real.is_some()
                && !matches!(f.skill.source, api::SkillSource::Bundled { .. })
        })
}

/// `command` is the one command of `members`, and wherever Claude finds it a
/// skill of theirs that Claude uses over it is there too, so removing it
/// takes it from no session that runs it.
fn hidden(command: &Found, members: &[&Found]) -> bool {
    let skills: Vec<&Find> = members
        .iter()
        .filter(|f| f.skill.kind == api::SkillKind::Skill)
        .flat_map(|f| {
            f.finds
                .iter()
                .zip(&f.skill.loads)
                .filter(|(find, load)| find.cli == Cli::Claude && load.shadowed_by.is_none())
                .map(|(find, _)| find)
        })
        .collect();
    let commands = members
        .iter()
        .filter(|f| f.skill.kind == api::SkillKind::Command)
        .count();
    commands == 1
        && command.finds.iter().all(|c| {
            skills.iter().any(|s| {
                s.everywhere()
                    || (s.project.is_some() && s.project == c.project)
                    || (s.space.is_some() && s.space == c.space && c.project.is_none())
            })
        })
}

/// What removing `links` does in git, said before it is done.
fn links_say(links: &[&Found]) -> String {
    let paths: Vec<PathBuf> = links.iter().map(|f| as_git_sees(link_of(f))).collect();
    let n = paths.len();
    let (them, they) = if n == 1 {
        ("it", "It is")
    } else {
        ("them", "They are")
    };
    let Some(root) = repo_root(&paths[0]) else {
        return format!("{they} in no git repository.");
    };
    let name = root.file_name().unwrap_or_default().to_string_lossy();
    let rels = in_repo(root, &paths);
    let tracked = listed(root, "ls-files", &rels);
    if tracked > 0 {
        format!(
            "In {name}, removing {them} shows as {}, and nothing is committed.",
            count(tracked, "deleted file")
        )
    } else if listed(root, "check-ignore", &rels) == n {
        format!("Git ignores {them} in {name}, so removing {them} changes nothing it tracks.")
    } else {
        format!(
            "Git does not track {them} in {name}, so removing {them} changes nothing it tracks."
        )
    }
}

/// What removing the command `held` is does in git, said before it is done.
fn command_say(held: &Held) -> String {
    let name = held.root.file_name().unwrap_or_default().to_string_lossy();
    format!(
        "It is committed in {name}, so removing it shows there as 1 deleted file, nothing is \
         committed, and Undo brings it back from git."
    )
}

/// `command`'s file, when git can bring it back as it is: a plain file
/// that no link leads to, whose bytes are the ones HEAD has, with nothing
/// about it in `git status`. Else why not, as the end of a sentence about it.
fn held(command: &Found) -> Result<Held, String> {
    let file = command.real.as_deref().ok_or("is not there")?;
    if command.finds.iter().any(|f| is_link(&f.path)) {
        return Err(
            "is reached through a link, and the page removes no file a link leads to".into(),
        );
    }
    if !fs::symlink_metadata(file).is_ok_and(|m| m.is_file()) {
        return Err("is not a plain file".into());
    }
    let root = repo_root(file).ok_or("is in no git repository, so it could not be brought back")?;
    let rel = in_repo(root, &[file.to_path_buf()])
        .pop()
        .ok_or("is not in its repository")?;
    let uncommitted = "is not committed in git, so it could not be brought back";
    let tree = git(root, &["ls-tree", "-z", "HEAD", "--", &rel]).ok_or(uncommitted)?;
    let entry = String::from_utf8_lossy(&tree);
    let (head, _) = entry.split_once('\t').ok_or(uncommitted)?;
    let [mode, kind, blob] = head.split(' ').collect::<Vec<_>>()[..] else {
        return Err(uncommitted.into());
    };
    let mode = mode
        .strip_prefix("100")
        .filter(|_| kind == "blob")
        .and_then(|m| u32::from_str_radix(m, 8).ok())
        .ok_or(uncommitted)?;
    let changed = "has changes not committed in git, so it could not be brought back as it is";
    let disk = git(root, &["hash-object", "--no-filters", "--", &rel]).ok_or(changed)?;
    if String::from_utf8_lossy(&disk).trim() != blob {
        return Err(changed.into());
    }
    let status = git(root, &["status", "--porcelain=v1", "-z", "--", &rel]).ok_or(changed)?;
    if !status.is_empty() {
        return Err(changed.into());
    }
    Ok(Held {
        file: file.to_path_buf(),
        root: root.to_path_buf(),
        blob: blob.to_string(),
        mode,
    })
}

/// A fingerprint of the files `fix` is about: each one's path and a hash of
/// its text, so a file touched and left as it was keeps it.
fn print(fix: &api::SkillFix, found: &[Found]) -> String {
    let mut text = String::new();
    for id in &fix.skills {
        let hash = found
            .iter()
            .find(|f| f.skill.id == *id)
            .and_then(|f| f.hash);
        text.push_str(&format!("{id}:{hash:?};"));
    }
    format!("{:016x}", fnv(text.as_bytes()))
}

/// `fixes` parted into those on the list and those the user kept while
/// their files are as they were, whose problems are then marked kept.
pub(super) fn kept(
    fixes: Vec<api::SkillFix>,
    found: &mut [Found],
    places: &Places,
) -> (Vec<api::SkillFix>, Vec<api::SkillFix>) {
    let file: KeptFile = read_json(&places.kept).unwrap_or_default();
    let (mut kept, live): (Vec<_>, Vec<_>) = fixes.into_iter().partition(|f| {
        file.kept
            .iter()
            .any(|k| k.fix == f.id && k.print == print(f, found))
    });
    for fix in &mut kept {
        for s in found
            .iter_mut()
            .filter(|s| fix.skills.contains(&s.skill.id))
        {
            for p in s.skill.problems.iter_mut().filter(|p| p.kind == fix.kind) {
                p.kept = true;
            }
        }
        let compare = fix.acts.iter().any(|a| a.act == Act::Compare);
        fix.acts = std::iter::once(Act::Unkeep)
            .chain(compare.then_some(Act::Compare))
            .map(can)
            .collect();
    }
    (live, kept)
}

/// Do `act` about the fix the list gave `id`, as the scan has it.
pub fn act(scan: &Scan, id: &str, act: Act) -> Result<(api::SkillDone, Option<Undo>), Refusal> {
    let fix = scan
        .fix(id)
        .ok_or_else(|| refuse(StatusCode::NOT_FOUND, format!("There is no fix {id}.")))?;
    if act == Act::Compare {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "Compare is a read: GET /api/skills/compare/<fix>.",
        ));
    }
    let offered = fix
        .acts
        .iter()
        .find(|a| a.act == act)
        .ok_or_else(|| conflict(format!("The page does not do that for {}.", fix.title)))?;
    if let Some(why) = &offered.refused {
        return Err(conflict(why.clone()));
    }
    let members = scan.members(fix);
    let places = &scan.places;
    match act {
        Act::RemoveLinks => remove_links(&members, places),
        Act::RemoveCommand => {
            let command = members
                .iter()
                .find(|f| f.skill.kind == api::SkillKind::Command)
                .ok_or_else(|| conflict(format!("{} holds no command.", fix.title)))?;
            remove_command(command, places)
        }
        Act::Keep => keep(scan, fix),
        Act::Unkeep => Ok((unkeep(places, &fix.id, &fix.title)?, None)),
        Act::Compare => unreachable!("refused above"),
    }
}

/// `path` as a sentence names it: a project's own under its name, else
/// under `~`.
fn place(path: &Path, project: Option<usize>, places: &Places) -> String {
    match project {
        Some(p) if path.starts_with(&places.projects[p].root) => {
            format!(
                "{}'s {}",
                places.projects[p].name,
                shown(path, project, places)
            )
        }
        _ => tilde(path, &places.home),
    }
}

/// A broken link's own path, as the scan found it.
fn link_of(f: &Found) -> &Path {
    &f.finds[0].path
}

/// `link` in its folder's real path, as git sees it: a folder of skills is
/// often itself a link into a repository.
fn as_git_sees(link: &Path) -> PathBuf {
    match (
        link.parent().and_then(|p| fs::canonicalize(p).ok()),
        link.file_name(),
    ) {
        (Some(dir), Some(name)) => dir.join(name),
        _ => link.to_path_buf(),
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Why `link` is not the link the scan found, pointing at `target`, which
/// is not there: none when it still is.
fn changed(link: &Path, target: &Path) -> Option<&'static str> {
    let Ok(meta) = fs::symlink_metadata(link) else {
        return Some("is not there any more");
    };
    if !meta.file_type().is_symlink() {
        return Some("is no longer a link");
    }
    if fs::read_link(link).ok().as_deref() != Some(target) {
        return Some("points somewhere else now");
    }
    match fs::metadata(link) {
        Ok(_) => Some("points at something that is there now"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some("cannot be followed to see what it points at"),
    }
}

/// Remove each of `links`, the broken links of one folder, that is still
/// the link the scan found: never what a link points at, and never one
/// that changed, which is said instead.
fn remove_links(
    links: &[&Found],
    places: &Places,
) -> Result<(api::SkillDone, Option<Undo>), Refusal> {
    let first = link_of(
        links
            .first()
            .ok_or_else(|| conflict("The fix has no links."))?,
    );
    let at = place(
        first.parent().unwrap_or(first),
        project_of(links[0]),
        places,
    );
    let (mut removed, mut skipped, mut failed) = (Vec::new(), Vec::new(), false);
    for f in links {
        let link = link_of(f).to_path_buf();
        let name = name_of(&link);
        let why = match &f.target {
            Some(target) => changed(&link, target),
            None => Some("could not be read as a link"),
        };
        if let Some(why) = why {
            skipped.push(format!("`{name}` {why}, so it was left as it is."));
            continue;
        }
        // ponytail: the look and the removal are two steps, so a file put at
        // the link's path between them would go; no call removes a path only
        // while it is a link.
        match fs::remove_file(&link) {
            Ok(()) => removed.push((link, f.target.clone().expect("looked at above"))),
            Err(e) => {
                failed = true;
                skipped.push(format!("`{name}` could not be removed: {e}."));
            }
        }
    }
    if removed.is_empty() {
        let status = if failed {
            StatusCode::INTERNAL_SERVER_ERROR
        } else {
            StatusCode::CONFLICT
        };
        return Err(refuse(
            status,
            format!("Nothing was removed. {}", skipped.join(" ")),
        ));
    }
    let paths: Vec<PathBuf> = removed.iter().map(|(link, _)| as_git_sees(link)).collect();
    let done = api::SkillDone {
        said: format!("Removed {} from {at}.", count(removed.len(), "link")),
        repos: repo_status(&paths).into_iter().collect(),
        skipped,
        undo: None,
    };
    Ok((done, Some(Undo::Links { at, links: removed })))
}

/// Remove `command`, once it is seen again to be a file git holds as it is.
fn remove_command(
    command: &Found,
    places: &Places,
) -> Result<(api::SkillDone, Option<Undo>), Refusal> {
    let file = command
        .real
        .as_deref()
        .unwrap_or(Path::new(&command.skill.path));
    let at = place(file, project_of(command), places);
    let held = held(command)
        .map_err(|why| conflict(format!("The page did not remove {at}: it {why}.")))?;
    // ponytail: held's look and the removal are two steps, so a change made
    // between them goes with the file; git holds no lock to take for it.
    fs::remove_file(&held.file).map_err(|e| {
        refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{at} could not be removed: {e}."),
        )
    })?;
    let done = api::SkillDone {
        said: format!("Removed {at}."),
        repos: repo_status(std::slice::from_ref(&held.file))
            .into_iter()
            .collect(),
        skipped: Vec::new(),
        undo: None,
    };
    Ok((done, Some(Undo::Command { at, held })))
}

/// Keep `fix` off the list while its files are as they are now.
fn keep(scan: &Scan, fix: &api::SkillFix) -> Result<(api::SkillDone, Option<Undo>), Refusal> {
    let print = print(fix, &scan.found);
    write_kept(&scan.places, |kept| {
        // A kept fix that is on the list again has changed since: its
        // entry is spent.
        kept.retain(|k| !scan.fixes.iter().any(|f| f.id == k.fix));
        kept.push(Kept {
            fix: fix.id.clone(),
            print,
            title: fix.title.clone(),
        });
    })?;
    let done = api::SkillDone {
        said: format!(
            "Kept {} off the list until one of its files changes.",
            fix.title
        ),
        repos: Vec::new(),
        skipped: Vec::new(),
        undo: None,
    };
    let undo = Undo::Kept {
        fix: fix.id.clone(),
        title: fix.title.clone(),
    };
    Ok((done, Some(undo)))
}

fn unkeep(places: &Places, fix: &str, title: &str) -> Result<api::SkillDone, Refusal> {
    write_kept(places, |kept| kept.retain(|k| k.fix != fix))?;
    Ok(api::SkillDone {
        said: format!("{title} is on the list again."),
        repos: Vec::new(),
        skipped: Vec::new(),
        undo: None,
    })
}

/// `change` made to the kept fixes, written as the switches are: beside
/// the file, and renamed over it only while it is as read.
fn write_kept(places: &Places, change: impl FnOnce(&mut Vec<Kept>)) -> Result<(), Refusal> {
    let file = &places.kept;
    let shown = tilde(file, &places.home);
    let failed = |why: String| refuse(StatusCode::INTERNAL_SERVER_ERROR, why);
    let read = match fs::read(file) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(failed(format!("{shown} cannot be read: {e}."))),
    };
    let mut kept: KeptFile = match &read {
        Some(bytes) => serde_json::from_slice(bytes).map_err(|e| {
            failed(format!(
                "{shown} does not parse ({e}), so the page did not write it."
            ))
        })?,
        None => KeptFile::default(),
    };
    change(&mut kept.kept);
    let mut text = serde_json::to_string_pretty(&kept).expect("kept fixes serialize");
    text.push('\n');
    replace(file, read.as_deref(), text.as_bytes()).map_err(|e| match e {
        Unwritten::Changed => conflict(format!(
            "{shown} changed while the page was writing it, so it was left as it is. Try again."
        )),
        Unwritten::Io(e) => failed(format!("{shown} cannot be written: {e}.")),
    })
}

/// Put back what an act removed or kept, only where nothing is now, or
/// the text a save replaced, only while the file holds what it saved.
pub fn undo(places: &Places, undo: &Undo) -> Result<api::SkillDone, Refusal> {
    match undo {
        Undo::Links { at, links } => put_back_links(at, links),
        Undo::Command { at, held } => put_back_command(at, held),
        Undo::Kept { fix, title } => unkeep(places, fix, title),
        Undo::Saved {
            at,
            file,
            saved,
            before,
        } => super::save::put_back(at, file, saved, before),
    }
}

fn put_back_links(at: &str, links: &[(PathBuf, PathBuf)]) -> Result<api::SkillDone, Refusal> {
    let (mut put, mut skipped) = (Vec::new(), Vec::new());
    for (link, target) in links {
        let put_back = if !link.parent().is_some_and(Path::is_dir) {
            Err("its folder is not there any more".to_string())
        } else {
            // A link is made only where nothing is, so this is the check.
            std::os::unix::fs::symlink(target, link).map_err(|e| match e.kind() {
                std::io::ErrorKind::AlreadyExists => "something is at its place again".to_string(),
                _ => e.to_string(),
            })
        };
        match put_back {
            Ok(()) => put.push(link.clone()),
            Err(why) => skipped.push(format!("`{}` was not put back: {why}.", name_of(link))),
        }
    }
    if put.is_empty() {
        return Err(conflict(format!(
            "Nothing was put back. {}",
            skipped.join(" ")
        )));
    }
    Ok(api::SkillDone {
        said: format!(
            "Put back {} in {at}, each pointing where it did.",
            count(put.len(), "link")
        ),
        repos: repo_status(&put.iter().map(|l| as_git_sees(l)).collect::<Vec<_>>())
            .into_iter()
            .collect(),
        skipped,
        undo: None,
    })
}

fn put_back_command(at: &str, held: &Held) -> Result<api::SkillDone, Refusal> {
    use std::os::unix::fs::PermissionsExt;
    let not = |why: &str| conflict(format!("{at} was not put back: {why}."));
    let failed = |why: String| refuse(StatusCode::INTERNAL_SERVER_ERROR, why);
    if !held.file.parent().is_some_and(Path::is_dir) {
        return Err(not("its folder is not there any more"));
    }
    if fs::symlink_metadata(&held.file).is_ok() {
        return Err(not("something is at its place again"));
    }
    let bytes = git(&held.root, &["cat-file", "blob", &held.blob]).ok_or_else(|| {
        failed(format!(
            "{at} was not put back: git no longer has the version it had."
        ))
    })?;
    replace(&held.file, None, &bytes).map_err(|e| match e {
        Unwritten::Changed => not("something is at its place again"),
        Unwritten::Io(e) => failed(format!("{at} was not put back: {e}.")),
    })?;
    fs::set_permissions(&held.file, fs::Permissions::from_mode(held.mode))
        .map_err(|e| failed(format!("{at} is back, but its mode could not be set: {e}.")))?;
    Ok(api::SkillDone {
        said: format!("Put back {at} from git, as it was."),
        repos: repo_status(std::slice::from_ref(&held.file))
            .into_iter()
            .collect(),
        skipped: Vec::new(),
        undo: None,
    })
}

/// The two copies the fix `id` is about, read as the page reads a skill.
pub fn compare(scan: &Scan, id: &str) -> Result<api::SkillCompare, Refusal> {
    let fix = scan
        .fix(id)
        .ok_or_else(|| refuse(StatusCode::NOT_FOUND, format!("There is no fix {id}.")))?;
    if !fix.acts.iter().any(|a| a.act == Act::Compare) {
        return Err(conflict(format!(
            "{} is not two copies of one skill, so there is nothing to compare.",
            fix.title
        )));
    }
    let read: Vec<(api::SkillCompared, Option<u64>)> = scan
        .members(fix)
        .into_iter()
        .map(|f| {
            let got = f
                .real
                .as_deref()
                .ok_or((P::BrokenLink, "The link is broken.".to_string()))
                .and_then(|real| skill_file(real, false))
                .and_then(|file| read_text(&file).map(|text| (file, text)));
            match got {
                Ok((file, text)) => {
                    let size = fs::metadata(&file).ok().map(|m| m.len());
                    let compared = api::SkillCompared {
                        skill: f.skill.id.clone(),
                        file: Some(file.display().to_string()),
                        text: Some(text),
                        unread: None,
                    };
                    (compared, size)
                }
                Err((_, why)) => {
                    let compared = api::SkillCompared {
                        skill: f.skill.id.clone(),
                        file: None,
                        text: None,
                        unread: Some(why),
                    };
                    (compared, None)
                }
            }
        })
        .collect();
    let same =
        matches!(&read[..], [(a, sa), (b, sb)] if a.text.is_some() && a.text == b.text && sa == sb);
    Ok(api::SkillCompare {
        files: read.into_iter().map(|(c, _)| c).collect(),
        same,
    })
}
