//! The page's writes: a skill's switch for Claude or Codex, and a Claude
//! plugin's, each in the settings file of that CLI the scan says it reads.
//! A request names skills and plugins by the ids the scan handed out, and
//! the file is the scan's, never one the page names.
//!
//! Claude Code reads `skillOverrides` from its user settings and, in a
//! project, from `.claude/settings.json` and `.claude/settings.local.json`,
//! the local file winning, keyed by the name it lists the skill under; it
//! watches those files, so a running session goes by a change from its next
//! turn. A plugin's skills follow `enabledPlugins` only. Codex reads
//! `[[skills.config]]` from `~/.codex/config.toml`, by a `SKILL.md`'s path
//! or a name, the last entry that selects a skill winning, when it starts.
//!
//! A file is read, changed in its text, and written to a file beside it
//! that is renamed over it once the file is seen to be as it was read;
//! a link is written at its target and stays a link. Nothing is committed.

use super::edit::{
    codex_checked, codex_set, codex_was, json_edit, json_string, quoted, toml_doc, Unedited,
};
use super::fixes::count;
use super::settings::{claude_switch, value_of, ClaudeLayers, Layer, Settings};
use super::*;
use crate::Refusal;
use api::SkillSetting as Setting;
use axum::http::StatusCode;
use std::io::Write;

/// Where a switch is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    /// Claude's `skillOverrides[key]` in `file`, the files under it, which
    /// say what it is without an entry there, in `lower`.
    Override {
        file: PathBuf,
        key: String,
        lower: Vec<PathBuf>,
    },
    /// Claude's `enabledPlugins[plugin]` in `file`.
    Plugin { file: PathBuf, plugin: String },
    /// Codex's `[[skills.config]]` for the skill whose `SKILL.md` is at
    /// one of `paths`, named `name`.
    Codex {
        file: PathBuf,
        paths: Vec<PathBuf>,
        name: String,
    },
}

impl Target {
    fn file(&self) -> &Path {
        match self {
            Target::Override { file, .. }
            | Target::Plugin { file, .. }
            | Target::Codex { file, .. } => file,
        }
    }
}

/// A switch the page can set, and where it stands.
pub(super) struct Control {
    target: Target,
    state: Setting,
    project: Option<usize>,
}

/// Claude's switch for `f`: its plugin's, for a plugin's skill; else its
/// `skillOverrides` entry in the user's settings for a skill Claude loads
/// everywhere, and in the project's `.claude/settings.local.json`, which
/// is not committed, for one it loads in one project only. None for a
/// skill it does not load, loads through a space or in several projects,
/// or whose state the managed settings or `syncClaudeAiSkills` decide.
pub(super) fn claude_control(f: &Found, places: &Places, layers: &ClaudeLayers) -> Option<Control> {
    f.real.as_ref()?;
    let live: Vec<(&Find, &api::SkillLoad)> = f
        .finds
        .iter()
        .zip(&f.skill.loads)
        .filter(|(find, load)| find.cli == Cli::Claude && load.shadowed_by.is_none())
        .collect();
    let &(find, load) = live.first()?;
    let context = if live.iter().any(|(f, _)| f.everywhere()) {
        None
    } else {
        let p = find.project?;
        if !live
            .iter()
            .all(|(f, _)| f.project == Some(p) && f.space.is_none())
        {
            return None;
        }
        Some(p)
    };
    let decided = claude_switch(find, &load.name, &layers.in_context(context));
    if let Some((_, setting, layer)) = &decided {
        if *setting == "syncClaudeAiSkills" || layer.is_some_and(|l| layers.is_managed(l)) {
            return None;
        }
    }
    let state = decided.map_or(Setting::On, |(s, ..)| s.into());
    let user = places.claude.join("settings.json");
    let dot = |p: usize| places.projects[p].root.join(".claude");
    let target = match &find.origin {
        Origin::Plugin(plugin) => Target::Plugin {
            file: context.map_or(user, |p| dot(p).join("settings.local.json")),
            plugin: plugin.id.clone(),
        },
        _ => Target::Override {
            key: load.name.clone(),
            lower: context.map_or_else(Vec::new, |p| {
                vec![dot(p).join("settings.json"), user.clone()]
            }),
            file: context.map_or(user, |p| dot(p).join("settings.local.json")),
        },
    };
    Some(Control {
        target,
        state,
        project: context,
    })
}

/// Codex's switch for `f`, in `~/.codex/config.toml`: none for a skill it
/// does not load, or a built-in one `[skills.bundled]` turns off with the
/// rest.
pub(super) fn codex_control(f: &Found, settings: &Settings) -> Option<Control> {
    f.real.as_ref()?;
    let codex: Vec<&Find> = f.finds.iter().filter(|f| f.cli == Cli::Codex).collect();
    let first = f.finds.iter().position(|find| find.cli == Cli::Codex)?;
    if codex.iter().any(|f| f.scope == Scope::System) && settings.codex.bundled_off() {
        return None;
    }
    let paths = f.codex_skill_mds();
    let name = f.skill.loads[first].name.clone();
    let on = settings.codex.enabled(&paths, &name);
    Some(Control {
        target: Target::Codex {
            file: settings.codex_file.clone(),
            paths,
            name,
        },
        state: if on { Setting::On } else { Setting::Off },
        project: None,
    })
}

/// Every switch the page can set for each skill, once the scan knows which
/// loads another skill of the same name hides.
pub(super) fn controls(found: &mut [Found], places: &Places, settings: &Settings) {
    for f in found.iter_mut() {
        let controls = [
            (Cli::Claude, claude_control(f, places, &settings.claude)),
            (Cli::Codex, codex_control(f, settings)),
        ];
        f.skill.controls = controls
            .into_iter()
            .filter_map(|(cli, c)| Some((cli, c?)))
            .map(|(cli, c)| {
                let (setting, plugin) = match &c.target {
                    Target::Override { .. } => ("skillOverrides", None),
                    Target::Plugin { plugin, .. } => ("enabledPlugins", Some(plugin.clone())),
                    Target::Codex { .. } => ("skills.config", None),
                };
                api::SkillControl {
                    cli,
                    setting,
                    file: tilde(c.target.file(), &places.home),
                    state: c.state,
                    plugin,
                    project: c.project.map(|p| places.projects[p].name.clone()),
                }
            })
            .collect();
    }
}

pub(super) fn refuse(status: StatusCode, message: impl Into<String>) -> Refusal {
    Refusal::new(status, message)
}

pub(super) fn conflict(message: impl Into<String>) -> Refusal {
    refuse(StatusCode::CONFLICT, message)
}

/// Set `cli`'s switch for each skill `set` names, in the one file they are
/// all switched in. Every id is checked before anything is written.
pub fn switch(scan: &Scan, cli: Cli, set: &[api::SkillTo]) -> Result<api::SkillWrite, Refusal> {
    let places = &scan.places;
    if set.is_empty() {
        return Err(refuse(StatusCode::BAD_REQUEST, "Name a skill to switch."));
    }
    if cli == Cli::Cursor {
        return Err(conflict("Cursor has no switch for a skill."));
    }
    let settings = Settings::read(places);
    let mut changes: Vec<(&Found, Target, Setting)> = Vec::new();
    for to in set {
        let found = scan.get(&to.id).ok_or_else(|| {
            refuse(
                StatusCode::NOT_FOUND,
                format!("There is no skill {}.", to.id),
            )
        })?;
        let name = &found.skill.name;
        let control = match cli {
            Cli::Claude => claude_control(found, places, &settings.claude),
            _ => codex_control(found, &settings),
        }
        .ok_or_else(|| {
            conflict(format!(
                "`{name}` has no switch for {} the page can set.",
                cli.title()
            ))
        })?;
        match (&control.target, to.state) {
            (Target::Plugin { .. }, _) => {
                return Err(conflict(format!(
                    "`{name}` is a plugin's skill: the plugin's switch turns it on or off."
                )))
            }
            (Target::Codex { .. }, Setting::NameOnly | Setting::UserInvocableOnly) => {
                return Err(refuse(
                    StatusCode::BAD_REQUEST,
                    "Codex's switch is On or Off.",
                ))
            }
            _ => {}
        }
        changes.push((found, control.target, to.state));
    }
    let file = changes[0].1.file().to_path_buf();
    if changes.iter().any(|(_, t, _)| t.file() != file) {
        return Err(conflict(
            "Those skills are switched in different files, so switch them one at a time.",
        ));
    }
    let mut before = Vec::new();
    let written = edit(&file, places, |text| {
        let mut text = text.to_string();
        for (found, target, to) in &changes {
            let id = found.skill.id.clone();
            match target {
                Target::Override { key, lower, .. } => {
                    let below = lower
                        .iter()
                        .find_map(|f| Layer::read(f.clone(), None).override_of(key))
                        .unwrap_or(Setting::On);
                    let now = json_string(&text, "skillOverrides", key)
                        .and_then(|v| super::settings::setting_of(&v));
                    before.push(api::SkillTo {
                        id,
                        state: now.unwrap_or(below),
                    });
                    // An entry that says what the files under it do is none.
                    let value = (*to != below).then(|| quoted(value_of(*to)));
                    text = json_edit(&text, "skillOverrides", key, value.as_deref())?;
                }
                Target::Codex { paths, name, .. } => {
                    let mut doc = toml_doc(&text)?;
                    let was = codex_was(&doc, paths, name);
                    before.push(api::SkillTo {
                        id,
                        state: if was { Setting::On } else { Setting::Off },
                    });
                    let on = *to == Setting::On;
                    if was != on {
                        codex_set(&mut doc, paths, name, on)?;
                        text = codex_checked(&text, doc.to_string(), paths, name, on)?;
                    }
                }
                Target::Plugin { .. } => unreachable!("refused above"),
            }
        }
        Ok(text)
    })?;
    Ok(api::SkillWrite { before, ..written })
}

/// Turn the Claude plugin `plugin` (`<name>@<marketplace>`, as the scan
/// has it) on or off in `enabledPlugins`.
pub fn switch_plugin(scan: &Scan, plugin: &str, on: bool) -> Result<api::SkillWrite, Refusal> {
    let places = &scan.places;
    let settings = Settings::read(places);
    let target = scan
        .found
        .iter()
        .filter_map(|f| claude_control(f, places, &settings.claude))
        .map(|c| c.target)
        .find(|t| matches!(t, Target::Plugin { plugin: p, .. } if p == plugin))
        .ok_or_else(|| {
            refuse(
                StatusCode::NOT_FOUND,
                format!("There is no plugin {plugin}."),
            )
        })?;
    edit(target.file(), places, |text| {
        json_edit(
            text,
            "enabledPlugins",
            plugin,
            Some(if on { "true" } else { "false" }),
        )
    })
}

/// `change` made to the text of settings file `file`, which is written
/// only when that changes it: at a link's target, refused when it does not
/// parse or is not UTF-8, when the changed text says more than the change,
/// or when the file changed while being written.
pub(super) fn edit(
    file: &Path,
    places: &Places,
    change: impl FnOnce(&str) -> Result<String, Unedited>,
) -> Result<api::SkillWrite, Refusal> {
    let shown = |p: &Path| tilde(p, &places.home);
    let real = match fs::symlink_metadata(file) {
        Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(file).map_err(|_| {
            let to = fs::read_link(file).map_or_else(|_| "?".into(), |t| t.display().to_string());
            conflict(format!(
                "{} is a link to {to}, which is not there, so the page did not write it.",
                shown(file)
            ))
        })?,
        _ => file.to_path_buf(),
    };
    let unreadable = |e: std::io::Error| {
        refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{} cannot be read: {e}.", shown(&real)),
        )
    };
    let read = match fs::read(&real) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(unreadable(e)),
    };
    let text = match &read {
        Some(bytes) => std::str::from_utf8(bytes).map_err(|_| {
            conflict(format!(
                "{} is not UTF-8 text, so the page did not write it.",
                shown(&real)
            ))
        })?,
        None => "",
    };
    let new = change(text).map_err(|e| match e {
        Unedited::Refused(why) => conflict(format!(
            "{} {why}, so the page did not write it.",
            shown(&real)
        )),
        Unedited::Unsafe(what) => refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "The change to {what} in {} could not be made safely: the changed file said more \
                 than that change, so the page did not write it.",
                shown(&real)
            ),
        ),
    })?;
    let changed = new != text;
    if changed {
        replace(&real, read.as_deref(), new.as_bytes()).map_err(|e| match e {
            Unwritten::Changed => conflict(format!(
                "{} changed while the page was writing it, so it was left as it is. Try again.",
                shown(&real)
            )),
            Unwritten::Io(e) => refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{} cannot be written: {e}.", shown(&real)),
            ),
        })?;
    }
    Ok(api::SkillWrite {
        file: shown(&real),
        link: (real != file).then(|| shown(file)),
        changed,
        repo: changed
            .then(|| repo_status(std::slice::from_ref(&real)))
            .flatten(),
        before: Vec::new(),
    })
}

/// Why a file was not replaced.
#[derive(Debug)]
pub(super) enum Unwritten {
    /// It is not what was read any more.
    Changed,
    Io(std::io::Error),
}

impl From<std::io::Error> for Unwritten {
    fn from(e: std::io::Error) -> Self {
        Unwritten::Io(e)
    }
}

/// Put `text` in place of `file`, which held `read` (`None`: was not
/// there): written beside it first, and renamed over it once the file is
/// seen to hold `read` still. A file another program makes meanwhile is
/// never replaced, since a new one is linked into place, which fails when
/// one is there.
// ponytail: the check and the rename are two steps, so a write that lands
// between them is lost; neither CLI locks its settings, so there is nothing
// to hold. A lock or compare-and-swap would close it, if either CLI ever
// offers one.
pub(super) fn replace(file: &Path, read: Option<&[u8]>, text: &[u8]) -> Result<(), Unwritten> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let dir = file.parent().ok_or(Unwritten::Changed)?;
    fs::create_dir_all(dir)?;
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.nebula-{}.tmp", std::process::id()));
    let mode = fs::metadata(file).map_or(0o644, |m| m.permissions().mode() & 0o7777);
    let _ = fs::remove_file(&tmp);
    let put = (|| -> Result<(), Unwritten> {
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)?;
        out.write_all(text)?;
        out.sync_all()?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
        match read {
            None => match fs::hard_link(&tmp, file) {
                Ok(()) => Ok(fs::remove_file(&tmp)?),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(Unwritten::Changed),
                Err(e) => Err(e.into()),
            },
            Some(read) => {
                if fs::read(file)? != read {
                    return Err(Unwritten::Changed);
                }
                Ok(fs::rename(&tmp, file)?)
            }
        }
    })();
    if put.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    put
}

/// The top folder of the git repository `path` is in: the nearest folder
/// above it with a `.git`.
pub(super) fn repo_root(path: &Path) -> Option<&Path> {
    path.ancestors()
        .skip(1)
        .find(|d| d.join(".git").symlink_metadata().is_ok())
}

/// `git -C root args`, which takes no optional lock, so a read never writes
/// the index; `None` when it did not run or failed.
pub(super) fn git<S: AsRef<std::ffi::OsStr>>(root: &Path, args: &[S]) -> Option<Vec<u8>> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// `paths` under `root`, as git names them there.
pub(super) fn in_repo(root: &Path, paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// How many of `rels`, paths in the repository at `root`, `git ls-files`
/// (tracked) and `git check-ignore` (ignored, untracked) list, one a line:
/// git quotes a path that would take more.
pub(super) fn listed(root: &Path, command: &str, rels: &[String]) -> usize {
    let mut args = vec![command, "--"];
    args.extend(rels.iter().map(String::as_str));
    git(root, &args).map_or(0, |out| {
        out.split(|b| *b == b'\n').filter(|f| !f.is_empty()).count()
    })
}

/// The git repository `files` are in, and its state in a few words:
/// `dotfiles: 1 changed file, nothing committed`, `acme: 28 deleted files,
/// nothing committed`, or that git ignores them.
pub(super) fn repo_status(files: &[PathBuf]) -> Option<String> {
    let root = repo_root(files.first()?)?;
    let name = root.file_name()?.to_string_lossy();
    let rels = in_repo(root, files);
    if !rels.is_empty() && listed(root, "check-ignore", &rels) == rels.len() {
        let them = if rels.len() == 1 { "it" } else { "them" };
        return Some(format!("git ignores {them} in {name}"));
    }
    let out = git(root, &["status", "--porcelain=v1", "-z"])?;
    let (mut n, mut deleted) = (0, 0);
    let mut fields = out.split(|b| *b == 0).filter(|f| !f.is_empty());
    while let Some(entry) = fields.next() {
        n += 1;
        if entry.get(..2).is_some_and(|xy| xy.contains(&b'D')) {
            deleted += 1;
        }
        // A rename or copy is followed by the path it came from.
        if matches!(entry.first(), Some(b'R' | b'C')) {
            fields.next();
        }
    }
    let changed = match (n, deleted) {
        (0, _) => "no changed file".to_string(),
        (n, 0) => count(n, "changed file"),
        (n, d) if d == n => count(n, "deleted file"),
        (n, d) => format!("{n} changed files, {d} of them deleted"),
    };
    Some(format!("{name}: {changed}, nothing committed"))
}
