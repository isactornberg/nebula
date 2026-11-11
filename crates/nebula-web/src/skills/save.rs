//! Editing a skill's own file from its page: which files the page edits and
//! why not the rest, what the scan would say of a text not yet saved, and
//! the save, by the id the list gave the skill and the version its detail
//! gave the text. A save is written at a link's target, the link left a
//! link and the file's mode kept, and refused when the file is not what was
//! read; its Undo puts the text back only while the file is what was saved.
//! Nothing is committed.

use super::acts::Undo;
use super::file::FILE_MAX;
use super::write::{conflict, refuse, replace, repo_status, Unwritten};
use super::*;
use crate::Refusal;
use axum::http::StatusCode;

/// The most a save or a check takes in its body: a file's text with every
/// byte of it escaped.
pub const BODY_MAX: usize = 2 * FILE_MAX as usize;

/// A file's version, which a save sends back with the new text: a hash of
/// its bytes.
pub(super) fn version_of(bytes: &[u8]) -> String {
    format!("{:016x}", fnv(bytes))
}

/// Why the page does not edit `f`'s file, in a line; none when it does.
/// What a CLI's next update or sync writes over, or an admin keeps, is not
/// the user's to edit here.
pub(super) fn uneditable(f: &Found, places: &Places) -> Option<String> {
    let Some(real) = &f.real else {
        return Some(not_edited("the link is broken, so there is no file"));
    };
    let plugins = fs::canonicalize(places.claude.join("plugins")).ok();
    let why = f
        .finds
        .iter()
        .find_map(|find| match (&find.origin, find.scope) {
            (Origin::ClaudeAi, _) => {
                Some("it is synced from claude.ai, so it changes there".into())
            }
            (Origin::Bundled(cli), _) => Some(format!(
                "it is built into {}, and an update puts it back",
                cli.title()
            )),
            (Origin::Plugin(p), _) if !p.in_place => Some(format!(
                "it is the `{}` plugin's copy, which an update replaces",
                p.name
            )),
            (_, Scope::Managed) => Some("it is in Claude Code's managed settings folder".into()),
            (_, Scope::Admin) => Some("it is in Codex's admin folder".into()),
            _ => None,
        })
        .or_else(|| {
            plugins
                .filter(|p| real.starts_with(p))
                .map(|_| "it is in Claude's plugins folder, which updates replace".into())
        })?;
    Some(not_edited(&why))
}

pub(super) fn not_edited(why: &str) -> String {
    format!("Not edited here: {why}.")
}

/// The page's edit of `f`'s file, which holds `text`: why not, for a file
/// it does not edit.
pub(super) fn edit_of(f: &Found, places: &Places, text: &str) -> Result<api::SkillEdit, String> {
    if let Some(why) = uneditable(f, places) {
        return Err(why);
    }
    // A file the read cut short is never written back.
    if text.len() as u64 >= FILE_MAX {
        return Err(not_edited("it is over 1 MiB, more than the page reads"));
    }
    let note = matches!(f.skill.source, api::SkillSource::Installer { .. }).then(|| {
        "The `npx skills` installer replaces this file when it next updates the skill.".into()
    });
    Ok(api::SkillEdit {
        text: text.to_string(),
        version: version_of(text.as_bytes()),
        note,
    })
}

fn no_skill(id: &str) -> Refusal {
    refuse(StatusCode::NOT_FOUND, format!("There is no skill {id}."))
}

fn short_enough(text: &str) -> Result<(), Refusal> {
    if text.len() as u64 >= FILE_MAX {
        return Err(refuse(
            StatusCode::PAYLOAD_TOO_LARGE,
            "The text is over 1 MiB, more than the page saves.",
        ));
    }
    Ok(())
}

/// What the scan would say of skill `id`'s file if it held `text`: its
/// problems, and what follows its frontmatter.
pub fn check(scan: &Scan, id: &str, text: &str) -> Result<api::SkillCheck, Refusal> {
    let f = scan.get(id).ok_or_else(|| no_skill(id))?;
    short_enough(text)?;
    let mut problems = Vec::new();
    let (_, body) = own_file(Some(text), &f.finds, f.real.as_deref(), &mut problems);
    Ok(api::SkillCheck {
        problems,
        body: body.to_string(),
    })
}

/// Write `to.text` to the file of the skill `to.id` names, as the scan has
/// it, while the file is still the one whose version the page read.
pub fn save(scan: &Scan, to: &api::SaveSkill) -> Result<(api::SkillDone, Option<Undo>), Refusal> {
    let places = &scan.places;
    let f = scan.get(&to.id).ok_or_else(|| no_skill(&to.id))?;
    if let Some(why) = uneditable(f, places) {
        return Err(conflict(why));
    }
    short_enough(&to.text)?;
    let real = f.real.as_deref().expect("a broken link is not edited");
    let file = skill_file(real, f.skill.kind == api::SkillKind::Command)
        .map_err(|(_, why)| conflict(why))?;
    let at = tilde(&file, &places.home);
    let read = fs::read(&file).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => conflict(format!(
            "{at} is not there any more, so the page did not save it."
        )),
        _ => refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{at} cannot be read: {e}."),
        ),
    })?;
    if version_of(&read) != to.version {
        return Err(conflict(format!(
            "{at} changed on disk since the page read it, so the page did not save it."
        )));
    }
    let text = to.text.as_bytes();
    if read == text {
        let done = api::SkillDone {
            said: format!("{at} holds that text already, so nothing was written."),
            repos: Vec::new(),
            skipped: Vec::new(),
            undo: None,
        };
        return Ok((done, None));
    }
    replace(&file, Some(&read), text).map_err(|e| match e {
        Unwritten::Changed => conflict(format!(
            "{at} changed on disk while the page was saving it, so it was left as it is."
        )),
        Unwritten::Io(e) => refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{at} cannot be written: {e}."),
        ),
    })?;
    let done = api::SkillDone {
        said: format!("Saved {at}."),
        repos: repo_status(std::slice::from_ref(&file))
            .into_iter()
            .collect(),
        skipped: Vec::new(),
        undo: None,
    };
    let undo = Undo::Saved {
        at,
        file,
        saved: text.to_vec(),
        before: read,
    };
    Ok((done, Some(undo)))
}

/// Put `before` back in `file`, `at` naming it, only while it holds `saved`.
pub(super) fn put_back(
    at: &str,
    file: &Path,
    saved: &[u8],
    before: &[u8],
) -> Result<api::SkillDone, Refusal> {
    let not = |why: &str| conflict(format!("{at} was not put back: {why}."));
    let changed = "it changed since the save, so it was left as it is";
    let failed = |e: std::io::Error| {
        refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{at} was not put back: {e}."),
        )
    };
    // Looked at first, so a folder gone since is not made again.
    match fs::read(file) {
        Ok(now) if now == saved => {}
        Ok(_) => return Err(not(changed)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(not("it is not there any more"))
        }
        Err(e) => return Err(failed(e)),
    }
    replace(file, Some(saved), before).map_err(|e| match e {
        Unwritten::Changed => not(changed),
        Unwritten::Io(e) => failed(e),
    })?;
    Ok(api::SkillDone {
        said: format!("Put back the text {at} had before the save."),
        repos: repo_status(&[file.to_path_buf()]).into_iter().collect(),
        skipped: Vec::new(),
        undo: None,
    })
}
