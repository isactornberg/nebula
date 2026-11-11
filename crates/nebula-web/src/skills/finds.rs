//! Where each CLI finds skills: the folders it reads, the plugins Claude
//! loads, the installer's record of what it installed, and the git
//! repository a folder is in.

use super::*;

/// What Claude Code 2.1 lists a skill synced from claude.ai under.
const SYNCED_PREFIX: &str = "anthropic-skills:";

pub(super) fn claude_finds(places: &Places, out: &mut Vec<Find>) {
    let at = |scope| Find::at(Cli::Claude, scope);
    skills_in(
        &places.claude_managed.join(".claude/skills"),
        &at(Scope::Managed),
        out,
    );
    skills_in(&places.claude.join("skills"), &at(Scope::Personal), out);
    commands_in(&places.claude.join("commands"), &at(Scope::Personal), out);
    for bucket in subdirs(&places.claude.join("skills/synced")) {
        let synced = Find {
            prefix: SYNCED_PREFIX.into(),
            origin: Origin::ClaudeAi,
            ..at(Scope::ClaudeAi)
        };
        skills_in(&bucket, &synced, out);
    }
    for plugin in plugins(places) {
        plugin_finds(plugin, out);
    }
    for (i, p) in places.projects.iter().enumerate() {
        let dot = p.root.join(".claude");
        skills_in(&dot.join("skills"), &at(Scope::Project).of(i), out);
        commands_in(&dot.join("commands"), &at(Scope::Project).of(i), out);
        for dot in nested(&p.root) {
            skills_in(&dot.join("skills"), &at(Scope::Nested).of(i), out);
            commands_in(&dot.join("commands"), &at(Scope::Nested).of(i), out);
        }
    }
    for added in &places.added {
        let dot = added.dir.join(".claude");
        let base = Find {
            space: Some(added.space.clone()),
            ..at(Scope::AddedDir)
        };
        skills_in(&dot.join("skills"), &base, out);
        commands_in(&dot.join("commands"), &base, out);
    }
}

pub(super) fn codex_finds(places: &Places, out: &mut Vec<Find>) {
    let at = |scope| Find::at(Cli::Codex, scope);
    for (i, p) in places.projects.iter().enumerate() {
        skills_in(
            &p.root.join(".agents/skills"),
            &at(Scope::Project).of(i),
            out,
        );
    }
    skills_in(
        &places.home.join(".agents/skills"),
        &at(Scope::Personal),
        out,
    );
    skills_in(&places.codex.join("skills"), &at(Scope::Personal), out);
    skills_in(&places.codex_admin.join("skills"), &at(Scope::Admin), out);
    let system = Find {
        origin: Origin::Bundled(Cli::Codex),
        ..at(Scope::System)
    };
    skills_in(&places.codex.join("skills/.system"), &system, out);
}

/// Cursor reads its own folders and the Claude and Codex ones too.
pub(super) fn cursor_finds(places: &Places, out: &mut Vec<Find>) {
    let at = |scope| Find::at(Cli::Cursor, scope);
    const OWN: [&str; 4] = [
        ".agents/skills",
        ".cursor/skills",
        ".claude/skills",
        ".codex/skills",
    ];
    for (i, p) in places.projects.iter().enumerate() {
        for dir in OWN {
            skills_in(&p.root.join(dir), &at(Scope::Project).of(i), out);
        }
    }
    for dir in OWN {
        skills_in(&places.home.join(dir), &at(Scope::Personal), out);
    }
    let system = Find {
        origin: Origin::Bundled(Cli::Cursor),
        ..at(Scope::System)
    };
    skills_in(&places.cursor.join("skills-cursor"), &system, out);
}

/// Every skill folder in `dir`, as `base` finds it: each entry holding a
/// `SKILL.md`, a symlink followed, and each broken link, which may have
/// been one. Hidden entries are not skills, and Claude keeps `synced` for
/// what it syncs from claude.ai.
pub(super) fn skills_in(dir: &Path, base: &Find, out: &mut Vec<Find>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut found: Vec<(PathBuf, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let reserved = base.cli == Cli::Claude
            && base.scope != Scope::ClaudeAi
            && name.eq_ignore_ascii_case("synced");
        if name.starts_with('.') || reserved {
            continue;
        }
        let path = entry.path();
        let skill = match fs::metadata(&path) {
            Ok(meta) => meta.is_dir() && path.join("SKILL.md").is_file(),
            Err(_) => is_link(&path),
        };
        if skill {
            found.push((path, name));
        }
    }
    found.sort();
    out.extend(found.into_iter().map(|(path, fallback)| Find {
        path,
        fallback,
        ..base.clone()
    }));
}

/// Every command file under a `commands` folder, named by its path in it
/// (`frontend/component.md` is `frontend:component`).
pub(super) fn commands_in(dir: &Path, base: &Find, out: &mut Vec<Find>) {
    pub(super) fn walk(dir: &Path, under: &str, depth: usize, files: &mut Vec<(PathBuf, String)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let named = |stem: &str| match under {
                "" => stem.to_string(),
                _ => format!("{under}:{stem}"),
            };
            if path.is_dir() {
                if depth < 3 {
                    walk(&path, &named(&name), depth + 1, files);
                }
            } else if let Some(stem) = name.strip_suffix(".md") {
                files.push((path, named(stem)));
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, "", 0, &mut files);
    files.sort();
    out.extend(files.into_iter().map(|(path, fallback)| Find {
        path,
        fallback,
        command: true,
        ..base.clone()
    }));
}

pub(super) fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

pub(super) fn is_link(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
}

/// Every `<subdir>/.claude` in the repository at `root` that holds skills
/// or commands, by `git ls-files` (tracked, and untracked but not ignored),
/// so a walk never wanders into a build's output. One process per project.
pub(super) fn nested(root: &Path) -> Vec<PathBuf> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
        ])
        .args([
            ":(glob)**/.claude/skills/**",
            ":(glob)**/.claude/commands/**",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    let Some(out) = out.ok().filter(|o| o.status.success()) else {
        return Vec::new();
    };
    let mut dirs = BTreeSet::new();
    for file in out.stdout.split(|b| *b == 0) {
        let Ok(file) = std::str::from_utf8(file) else {
            continue;
        };
        if let Some((sub, _)) = file.split_once("/.claude/") {
            dirs.insert(root.join(sub).join(".claude"));
        }
    }
    dirs.into_iter().collect()
}

pub(super) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// Every installed Claude plugin, as Claude Code's own records have it.
pub(super) fn plugins(places: &Places) -> Vec<Plugin> {
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct Installed {
        plugins: BTreeMap<String, Vec<Install>>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Install {
        install_path: PathBuf,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        project_path: Option<PathBuf>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Known {
        #[serde(default)]
        source: serde_json::Value,
        install_location: PathBuf,
    }
    let dir = places.claude.join("plugins");
    let installed: Installed = read_json(&dir.join("installed_plugins.json")).unwrap_or_default();
    let known: BTreeMap<String, Known> =
        read_json(&dir.join("known_marketplaces.json")).unwrap_or_default();
    let mut out = Vec::new();
    for (id, installs) in installed.plugins {
        let Some((entry, marketplace)) = id.rsplit_once('@') else {
            continue;
        };
        let known = known.get(marketplace);
        let source = known.map(|k| &k.source);
        let kind = source.and_then(|s| s["source"].as_str()).unwrap_or("");
        let from = source.and_then(|s| match kind {
            "github" => s["repo"].as_str().map(|r| format!("github.com/{r}")),
            "directory" | "file" => s["path"].as_str().map(String::from),
            _ => s["url"].as_str().map(String::from),
        });
        // A marketplace added from a local path loads its plugins in place.
        let in_place = known.filter(|_| kind == "directory").and_then(|k| {
            let listed: serde_json::Value =
                read_json(&k.install_location.join(".claude-plugin/marketplace.json"))?;
            let entry = listed["plugins"]
                .as_array()?
                .iter()
                .find(|p| p["name"] == entry)?;
            inside(&k.install_location, entry["source"].as_str()?)
        });
        for install in installs {
            let project = match &install.project_path {
                None => None,
                Some(path) => match places.projects.iter().position(|p| &p.root == path) {
                    Some(i) => Some(i),
                    None => continue,
                },
            };
            let root = in_place.clone().unwrap_or(install.install_path);
            let manifest: serde_json::Value =
                read_json(&root.join(".claude-plugin/plugin.json")).unwrap_or_default();
            out.push(Plugin {
                id: id.clone(),
                name: manifest["name"].as_str().unwrap_or(entry).to_string(),
                version: install.version,
                marketplace: marketplace.to_string(),
                from: from.clone(),
                root,
                project,
            });
        }
    }
    out
}

/// `rel` under `root`, refused when it climbs out or is absolute: what
/// Claude Code takes for a plugin's paths.
pub(super) fn inside(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    let plain = rel
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    plain.then(|| root.join(rel))
}

/// A plugin's skills: its `skills/`, and each folder its manifest's
/// `skills` adds (one holding skills, or one holding a `SKILL.md`); with
/// neither, a `SKILL.md` at its root is its one skill.
pub(super) fn plugin_finds(plugin: Plugin, out: &mut Vec<Find>) {
    let manifest: serde_json::Value =
        read_json(&plugin.root.join(".claude-plugin/plugin.json")).unwrap_or_default();
    let listed: Vec<&str> = match &manifest["skills"] {
        serde_json::Value::String(s) => vec![s.as_str()],
        serde_json::Value::Array(a) => a.iter().filter_map(|v| v.as_str()).collect(),
        _ => Vec::new(),
    };
    let default = plugin.root.join("skills");
    let base = Find {
        prefix: format!("{}:", plugin.name),
        project: plugin.project,
        ..Find::at(Cli::Claude, Scope::Plugin)
    };
    let single = |dir: PathBuf, name: &str| Find {
        path: dir,
        fallback: name.to_string(),
        ..base.clone()
    };
    let mut found = Vec::new();
    skills_in(&default, &base, &mut found);
    for dir in listed.iter().filter_map(|rel| inside(&plugin.root, rel)) {
        if dir.join("SKILL.md").is_file() {
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
            found.push(single(
                dir.clone(),
                &name.unwrap_or_else(|| plugin.name.clone()),
            ));
        } else {
            skills_in(&dir, &base, &mut found);
        }
    }
    if listed.is_empty() && !default.is_dir() && plugin.root.join("SKILL.md").is_file() {
        found.push(single(plugin.root.clone(), &plugin.name));
    }
    out.extend(found.into_iter().map(|f| Find {
        origin: Origin::Plugin(plugin.clone()),
        ..f
    }));
}

/// The `npx skills` installer's record of what it put in `~/.agents/skills`.
pub(super) struct InstallerLock {
    dir: Option<PathBuf>,
    skills: BTreeMap<String, LockEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LockEntry {
    source: String,
    #[serde(default)]
    source_url: Option<String>,
    #[serde(default)]
    skill_path: Option<String>,
}

impl InstallerLock {
    pub(super) fn read(places: &Places) -> InstallerLock {
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct Lock {
            skills: BTreeMap<String, LockEntry>,
        }
        let agents = places.home.join(".agents");
        let lock: Lock = read_json(&agents.join(".skill-lock.json")).unwrap_or_default();
        InstallerLock {
            dir: fs::canonicalize(agents.join("skills")).ok(),
            skills: lock.skills,
        }
    }

    /// What the installer installed the skill in folder `real` from.
    pub(super) fn source(&self, real: &Path) -> Option<api::SkillSource> {
        if real.parent() != self.dir.as_deref() {
            return None;
        }
        let entry = self.skills.get(real.file_name()?.to_str()?)?;
        Some(api::SkillSource::Installer {
            source: entry.source.clone(),
            url: entry.source_url.clone(),
            path: entry.skill_path.clone(),
        })
    }
}

/// The git repository each folder is in, by the nearest `.git` above it,
/// read off disk rather than asked of `git`.
#[derive(Default)]
pub(super) struct Repos(HashMap<PathBuf, Option<api::GitRepo>>);

impl Repos {
    pub(super) fn of(&mut self, path: &Path) -> Option<api::GitRepo> {
        let mut walked = Vec::new();
        let mut found = None;
        for dir in path.ancestors() {
            if let Some(known) = self.0.get(dir) {
                found = known.clone();
                break;
            }
            walked.push(dir.to_path_buf());
            if dir.join(".git").symlink_metadata().is_ok() {
                found = Some(git_repo(dir));
                break;
            }
        }
        for dir in walked {
            self.0.insert(dir, found.clone());
        }
        found
    }
}

pub(super) fn git_repo(root: &Path) -> api::GitRepo {
    let dot = root.join(".git");
    // A linked worktree's `.git` is a file naming its git dir.
    let git_dir = match fs::read_to_string(&dot) {
        Ok(file) => file
            .trim()
            .strip_prefix("gitdir:")
            .map(|d| root.join(d.trim()))
            .unwrap_or(dot),
        Err(_) => dot,
    };
    let branch = fs::read_to_string(git_dir.join("HEAD"))
        .ok()
        .and_then(|head| {
            head.trim()
                .strip_prefix("ref: refs/heads/")
                .map(String::from)
        });
    api::GitRepo {
        root: root.display().to_string(),
        branch,
    }
}
