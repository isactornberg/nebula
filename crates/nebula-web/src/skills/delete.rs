//! Deleting a skill from its page, by the id the list gave it and the
//! version its detail gave what goes: its real folder (or command file) and
//! every link the scan found to it, each moved to the Trash, and the `npx
//! skills` installer's entry for it, which would install it again. Each
//! path is looked at again right before, and nothing goes when one changed
//! since the page read it. Its Undo puts each back from the Trash only where
//! nothing is now. Nothing is committed.

use super::acts::{as_git_sees, changed, Undo};
use super::edit::{json_edit, Unedited};
use super::fixes::count;
use super::save::{no_skill, not_yours};
use super::write::{conflict, in_repo, listed, refuse, replace, repo_root, repo_status, Unwritten};
use super::*;
use crate::Refusal;
use axum::http::StatusCode;

/// What a delete moved, for its Undo.
#[derive(Debug, Clone)]
pub struct Deleted {
    /// Where each thing was, where it is in the Trash, and whether it is a
    /// link: the real folder or file first, when there is one.
    moved: Vec<(PathBuf, PathBuf, bool)>,
    command: bool,
    /// The installer's lock, as the delete wrote it and as it was.
    lock: Option<(PathBuf, Vec<u8>, Vec<u8>)>,
}

/// What deleting a skill takes, as it is on disk now.
struct Plan {
    /// Each path and whether it is a link: the real folder or file first,
    /// when there is one.
    goes: Vec<(PathBuf, bool)>,
    /// The installer's lock and the skill's key in it.
    lock: Option<(PathBuf, String)>,
    version: String,
}

fn not_deleted(why: &str) -> String {
    format!("Not deleted here: {why}.")
}

/// What deleting `f` takes, each path looked at now: why not, as the end
/// of a sentence, when something is not as the scan read it.
fn plan(f: &Found, places: &Places) -> Result<Plan, String> {
    let shown = |p: &Path| tilde(p, &places.home);
    let command = f.skill.kind == api::SkillKind::Command;
    let mut goes = Vec::new();
    let mut print = String::new();
    if let Some(real) = &f.real {
        let plugin = f.finds.iter().find_map(|find| match &find.origin {
            Origin::Plugin(p) if fs::canonicalize(&p.root).ok().as_ref() == Some(real) => {
                Some(&p.name)
            }
            _ => None,
        });
        if let Some(plugin) = plugin {
            return Err(format!(
                "it is the `{plugin}` plugin's own folder, which goes with the plugin"
            ));
        }
        let gone = || format!("{} is not there any more", shown(real));
        let meta = fs::symlink_metadata(real).map_err(|_| gone())?;
        if meta.is_dir() == command || meta.file_type().is_symlink() {
            return Err(gone());
        }
        let file = skill_file(real, command).ok();
        let hash = file
            .as_deref()
            .and_then(|file| read_text(file).ok())
            .map(|text| fnv(text.as_bytes()));
        if hash != f.hash {
            let file = file.unwrap_or_else(|| real.clone());
            return Err(format!("{} changed since the list was read", shown(&file)));
        }
        print.push_str(&format!("{}:{hash:?}\n", real.display()));
        goes.push((real.clone(), false));
    }
    for find in f.finds.iter().filter(|find| is_link(&find.path)) {
        let link = as_git_sees(&find.path);
        if goes.iter().any(|(p, _)| *p == link) {
            continue;
        }
        let why = match (&f.real, &f.target) {
            (Some(real), _) => (fs::canonicalize(&link).ok().as_ref() != Some(real))
                .then_some("leads somewhere else now"),
            (None, Some(target)) => changed(&link, target),
            (None, None) => Some("could not be read as a link"),
        };
        if let Some(why) = why {
            return Err(format!("{} {why}", shown(&link)));
        }
        let to = fs::read_link(&link).unwrap_or_default();
        print.push_str(&format!("{}->{}\n", link.display(), to.display()));
        goes.push((link, true));
    }
    if goes.is_empty() {
        return Err("nothing of it is on disk".into());
    }
    let lock = match (&f.skill.source, &f.real) {
        (api::SkillSource::Installer { .. }, Some(real)) => real.file_name().map(|key| {
            (
                places.home.join(".agents/.skill-lock.json"),
                key.to_string_lossy().into_owned(),
            )
        }),
        _ => None,
    };
    Ok(Plan {
        goes,
        lock,
        version: format!("{:016x}", fnv(print.as_bytes())),
    })
}

/// What the page says before it deletes `f`: why not, in a line, when it
/// does not.
pub(super) fn offer(f: &Found, places: &Places) -> Result<api::SkillDelete, String> {
    let plan = plan(f, places).map_err(|why| not_deleted(&why))?;
    Ok(api::SkillDelete {
        goes: plan
            .goes
            .iter()
            .map(|(path, link)| api::SkillGoes {
                path: path.display().to_string(),
                link: *link,
            })
            .collect(),
        says: git_says(&plan.goes, f.skill.kind == api::SkillKind::Command),
        note: plan.lock.map(|(file, _)| {
            format!(
                "Its entry in {} goes too, so `npx skills update` does not install it again.",
                tilde(&file, &places.home)
            )
        }),
        version: plan.version,
    })
}

/// `items` by the git repository each is in, in their order.
fn by_repo<T>(items: &[T], path: impl Fn(&T) -> &Path) -> Vec<(Option<&Path>, Vec<&T>)> {
    let mut out: Vec<(Option<&Path>, Vec<&T>)> = Vec::new();
    for item in items {
        let root = repo_root(path(item));
        match out.iter_mut().find(|(r, _)| *r == root) {
            Some((_, them)) => them.push(item),
            None => out.push((root, vec![item])),
        }
    }
    out
}

/// What deleting `goes` shows in git, a sentence for each repository, and
/// one for a folder or file in none, which only the Trash then holds.
fn git_says(goes: &[(PathBuf, bool)], command: bool) -> Vec<String> {
    by_repo(goes, |(p, _)| p)
        .into_iter()
        .filter_map(|(root, items)| {
            let what = match &items[..] {
                [(_, true)] => "the link".to_string(),
                [_] if command => "the file".into(),
                [_] => "the folder".into(),
                _ if items.iter().all(|(_, link)| *link) => format!("the {} links", items.len()),
                _ => "they".into(),
            };
            let Some(root) = root else {
                let it = if command { "file" } else { "folder" };
                return items.iter().any(|(_, link)| !link).then(|| {
                    format!("The {it} is in no git repository, so the Trash holds its only copy.")
                });
            };
            let name = root.file_name().unwrap_or_default().to_string_lossy();
            let paths: Vec<PathBuf> = items.iter().map(|(p, _)| p.clone()).collect();
            let rels = in_repo(root, &paths);
            let tracked = listed(root, "ls-files", &rels);
            Some(if tracked > 0 {
                format!(
                    "In {name}, this shows as {}, and nothing is committed.",
                    count(tracked, "deleted file")
                )
            } else if listed(root, "check-ignore", &rels) == rels.len() {
                format!("Git ignores {what} in {name}, so this changes nothing it tracks.")
            } else {
                format!("Git does not track {what} in {name}, so this changes nothing it tracks.")
            })
        })
        .collect()
}

/// What was moved, as a sentence names it: `its folder and 2 links`.
fn things(moved: &[&(PathBuf, PathBuf, bool)], command: bool) -> String {
    let links = moved.iter().filter(|(_, _, link)| *link).count();
    let real = moved.len() > links;
    match (real, links) {
        (true, 0) if command => "its file".into(),
        (true, 0) => "its folder".into(),
        (true, n) => format!(
            "its {} and {}",
            if command { "file" } else { "folder" },
            count(n, "link")
        ),
        (false, 1) => "its link".into(),
        (false, n) => format!("its {n} links"),
    }
}

/// Each repository the paths of `moved` are in, as it stands now.
fn repos(moved: &[&(PathBuf, PathBuf, bool)]) -> Vec<String> {
    by_repo(moved, |(from, _, _)| from)
        .into_iter()
        .filter(|(root, _)| root.is_some())
        .filter_map(|(_, them)| {
            repo_status(
                &them
                    .iter()
                    .map(|(from, _, _)| from.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// The installer's lock at `file` without the entry `key`, ready to write:
/// none when there is no such entry.
fn lock_without(
    file: &Path,
    key: &str,
    places: &Places,
) -> Result<Option<(PathBuf, Vec<u8>, String)>, Refusal> {
    let shown = tilde(file, &places.home);
    let real = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let read = match fs::read(&real) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{shown} cannot be read, so the page deleted nothing: {e}."),
            ))
        }
    };
    let unsafe_lock =
        |why: String| conflict(format!("{shown} {why}, so the page deleted nothing."));
    let text = std::str::from_utf8(&read).map_err(|_| unsafe_lock("is not UTF-8 text".into()))?;
    let new = json_edit(text, "skills", key, None).map_err(|e| match e {
        Unedited::Refused(why) => unsafe_lock(why),
        Unedited::Unsafe(what) => unsafe_lock(format!("could not have {what} taken out safely")),
    })?;
    Ok((new != text).then_some((real, read, new)))
}

/// Delete the skill `to.id` names, as the scan has it, while what goes is
/// the version its detail gave: each path moved to the Trash, the real
/// folder or file first, so nothing goes when that cannot.
pub fn delete(
    scan: &Scan,
    to: &api::DeleteSkill,
) -> Result<(api::SkillDone, Option<Undo>), Refusal> {
    let places = &scan.places;
    let f = scan.get(&to.id).ok_or_else(|| no_skill(&to.id))?;
    let at = f.skill.name.clone();
    let command = f.skill.kind == api::SkillKind::Command;
    if let Some(why) = not_yours(f, places) {
        return Err(conflict(not_deleted(&why)));
    }
    let plan =
        plan(f, places).map_err(|why| conflict(format!("The page deleted nothing: {why}.")))?;
    if plan.version != to.version {
        return Err(conflict(format!(
            "{at} changed on disk since the page read it, so the page deleted nothing."
        )));
    }
    let lock = match &plan.lock {
        Some((file, key)) => lock_without(file, key, places)?,
        None => None,
    };
    let (mut moved, mut skipped) = (Vec::new(), Vec::new());
    for (i, (path, link)) in plan.goes.iter().enumerate() {
        // ponytail: the look in `plan` and the move are two steps, so a
        // change made between them goes to the Trash with the rest.
        match trash::trash(path, places.trash.as_deref()) {
            Ok(there) => moved.push((path.clone(), there, *link)),
            Err(e) if i == 0 => {
                return Err(refuse(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "The page deleted nothing: {} could not be moved to the Trash: {e}.",
                        tilde(path, &places.home)
                    ),
                ))
            }
            Err(e) => skipped.push(format!(
                "{} could not be moved to the Trash: {e}.",
                tilde(path, &places.home)
            )),
        }
    }
    let lock = lock.and_then(|(file, read, new)| {
        let shown = tilde(&file, &places.home);
        match replace(&file, Some(&read), new.as_bytes()) {
            Ok(()) => Some((file, new.into_bytes(), read)),
            Err(e) => {
                let why = match e {
                    Unwritten::Changed => "it changed while the page was writing it".into(),
                    Unwritten::Io(e) => e.to_string(),
                };
                skipped.push(format!(
                    "{shown} was left as it is ({why}), so `npx skills update` may install it again."
                ));
                None
            }
        }
    });
    let all: Vec<&(PathBuf, PathBuf, bool)> = moved.iter().collect();
    let lock_said = lock.as_ref().map_or(String::new(), |(file, ..)| {
        format!(", and its entry in {} is gone", tilde(file, &places.home))
    });
    let n = moved.len();
    let done = api::SkillDone {
        said: format!(
            "Deleted {at}: {} {} in the Trash{lock_said}.",
            things(&all, command),
            if n == 1 { "is" } else { "are" }
        ),
        repos: repos(&all),
        skipped,
        undo: None,
    };
    let deleted = Deleted {
        moved,
        command,
        lock,
    };
    Ok((done, Some(Undo::Deleted { at, deleted })))
}

/// Put back what a delete of `at` moved to the Trash, each only where
/// nothing is now, and the installer's entry once its folder is back.
pub(super) fn put_back(
    places: &Places,
    at: &str,
    deleted: &Deleted,
) -> Result<api::SkillDone, Refusal> {
    let shown = |p: &Path| tilde(p, &places.home);
    let (mut put, mut skipped) = (Vec::new(), Vec::new());
    for item in &deleted.moved {
        let (from, there, _) = item;
        let why = if !from.parent().is_some_and(Path::is_dir) {
            Some("its folder is not there any more".to_string())
        } else if fs::symlink_metadata(there).is_err() {
            Some("it is not in the Trash any more".into())
        } else {
            trash::put_back(there, from).err().map(|e| match e.kind() {
                std::io::ErrorKind::AlreadyExists => "something is at its place again".into(),
                _ => e.to_string(),
            })
        };
        match why {
            None => put.push(item),
            Some(why) => skipped.push(format!("{} was not put back: {why}.", shown(from))),
        }
    }
    if put.is_empty() {
        return Err(conflict(format!(
            "Nothing was put back. {}",
            skipped.join(" ")
        )));
    }
    let real_back = put.iter().any(|(_, _, link)| !link);
    let mut lock_said = String::new();
    if let Some((file, saved, before)) = deleted.lock.as_ref().filter(|_| real_back) {
        match super::save::put_back(&shown(file), file, saved, before) {
            Ok(_) => lock_said = format!(", and its entry in {}", shown(file)),
            Err(refusal) => skipped.push(refusal.message),
        }
    }
    Ok(api::SkillDone {
        said: format!(
            "Put back {at} from the Trash: {}{lock_said}.",
            things(&put, deleted.command)
        ),
        repos: repos(&put),
        skipped,
        undo: None,
    })
}
