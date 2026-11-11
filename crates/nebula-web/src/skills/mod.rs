//! The SKILLS page's resolver: every skill the agent CLIs on this machine
//! load, found where each CLI looks for them, since none of them lists what
//! it resolved. A skill is one real folder holding a `SKILL.md` (or one
//! legacy `.claude/commands` file), however many links and CLIs reach it.
//! The scan says which CLI finds it where and under what name, which
//! setting keeps it from one, where it comes from (the git repository it
//! is in, the skills installer's source, a plugin and its marketplace), and
//! what is wrong with it.
//!
//! [`scan`] writes nothing: it reads the disk and runs `git` read-only (one
//! `git ls-files` per project, and a few for each fix that removes a file),
//! so it runs on a blocking thread behind [`Cache`], and every place it
//! reads is a field of [`Places`], so tests scan temp dirs. How often Claude
//! used each skill is read apart from the scan ([`usage`]), from its
//! transcripts, so the list never waits for it. The page writes a switch,
//! in the settings file of the CLI it is for ([`write`]), and acts on a fix:
//! it removes broken links or a command a skill hides, or keeps a fix off
//! the list ([`acts`]).
//!
//! Claude Code's plugin state is read from its own files
//! (`~/.claude/plugins/installed_plugins.json`, `known_marketplaces.json`
//! and the `enabledPlugins` settings) rather than `claude plugin list
//! --json`, a process per scan. A plugin no settings file enables is off,
//! as that command reports one.

mod acts;
mod edit;
mod file;
mod finds;
mod fixes;
mod save;
mod settings;
#[cfg(test)]
mod tests;
mod usage;
mod write;

use crate::api::{self, SkillCli as Cli, SkillScope as Scope};
pub use acts::{act, compare, undo, Undo};
use file::{field, file_entry, files_in, frontmatter, read_text, skill_file, valid_name};
use finds::{claude_finds, codex_finds, cursor_finds, is_link, read_json, InstallerLock, Repos};
pub use save::{check, save, BODY_MAX};
use serde::Deserialize;
use settings::{switches, Settings};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
pub use write::{switch, switch_plugin};

/// The Agent Skills spec's limits (agentskills.io).
const NAME_MAX: usize = 64;
const DESCRIPTION_MAX: usize = 1024;
/// How long one scan answers before the disk is read again.
const FRESH: Duration = Duration::from_secs(5);

/// Every place the scan reads: the CLIs' homes and config roots, and
/// nebula's projects.
#[derive(Debug, Clone)]
pub struct Places {
    pub home: PathBuf,
    /// Claude Code's config dir: `~/.claude`, or `$CLAUDE_CONFIG_DIR`.
    pub claude: PathBuf,
    /// Claude Code's managed settings dir.
    pub claude_managed: PathBuf,
    /// Codex's home: `~/.codex`, or `$CODEX_HOME`.
    pub codex: PathBuf,
    /// Codex's admin dir, `/etc/codex`.
    pub codex_admin: PathBuf,
    pub cursor: PathBuf,
    pub projects: Vec<ProjectRoot>,
    pub added: Vec<AddedDir>,
    /// The fixes the user kept off the list: `skills-kept.json` in nebula's
    /// data dir.
    pub kept: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProjectRoot {
    pub name: String,
    /// The project's root checkout.
    pub root: PathBuf,
    /// Its other checkouts, removed ones too: a session in one is a
    /// session in the project.
    pub worktrees: Vec<PathBuf>,
    pub space: Option<String>,
}

/// A directory a space's `add_dirs` names (`config.json`), which every
/// Claude session of a project in that space gets with `--add-dir`.
#[derive(Debug, Clone)]
pub struct AddedDir {
    pub space: String,
    pub dir: PathBuf,
}

impl Places {
    /// This machine's places, with `projects` nebula's, each filed under
    /// its space here.
    pub fn here(projects: Vec<ProjectRoot>) -> Places {
        use nebula_core::env;
        let home = env::home_dir().unwrap_or_default();
        let spaces = api::spaces();
        let managed = if cfg!(target_os = "macos") {
            "/Library/Application Support/ClaudeCode"
        } else {
            "/etc/claude-code"
        };
        Places {
            claude: nebula_core::paths::claude_config_dir().unwrap_or_else(|| home.join(".claude")),
            claude_managed: PathBuf::from(managed),
            codex: env::non_empty(env::CODEX_HOME)
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex")),
            codex_admin: PathBuf::from("/etc/codex"),
            cursor: home.join(".cursor"),
            projects: projects
                .into_iter()
                .map(|p| ProjectRoot {
                    space: spaces.get(&p.root).cloned(),
                    ..p
                })
                .collect(),
            added: added_dirs(&home),
            kept: nebula_core::paths::data_dir().join("skills-kept.json"),
            home,
        }
    }

    /// The CLIs installed here, by their home folder.
    fn clis(&self) -> Vec<Cli> {
        [
            (Cli::Claude, &self.claude),
            (Cli::Codex, &self.codex),
            (Cli::Cursor, &self.cursor),
        ]
        .into_iter()
        .filter(|(_, dir)| dir.is_dir())
        .map(|(cli, _)| cli)
        .collect()
    }
}

/// Every space's `add_dirs` in `config.json`, a leading `~` the home, as
/// the daemon hands them to Claude: one that is still not absolute is none.
fn added_dirs(home: &Path) -> Vec<AddedDir> {
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct Settings {
        spaces: BTreeMap<String, Space>,
    }
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct Space {
        add_dirs: Vec<String>,
    }
    let settings = nebula_core::settings::load::<Settings>(
        &nebula_core::paths::config_path(),
        &nebula_core::paths::config_local_path(),
    );
    let mut out = Vec::new();
    for (space, s) in settings.value.spaces {
        for dir in s.add_dirs {
            let dir = dir.trim();
            let dir = match dir.strip_prefix("~/") {
                Some(rest) => home.join(rest),
                None if dir == "~" => home.to_path_buf(),
                None => PathBuf::from(dir),
            };
            if dir.is_absolute() {
                out.push(AddedDir {
                    space: space.clone(),
                    dir,
                });
            }
        }
    }
    out
}

/// A space's name as projects are filed under it: without case, runs of
/// blanks as one.
fn same_space(a: &str, b: &str) -> bool {
    let label = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    label(a) == label(b)
}

/// One scan: every skill found, by name, what to fix and what the user
/// kept off the list, its JSON, and the places it read.
pub struct Scan {
    pub found: Vec<Found>,
    fixes: Vec<api::SkillFix>,
    kept: Vec<api::SkillFix>,
    pub json: String,
    places: Places,
}

impl Scan {
    pub fn get(&self, id: &str) -> Option<&Found> {
        self.found.iter().find(|f| f.skill.id == id)
    }

    pub fn places(&self) -> &Places {
        &self.places
    }

    /// The fix the list gave `id`, on the list or kept off it.
    fn fix(&self, id: &str) -> Option<&api::SkillFix> {
        self.fixes.iter().chain(&self.kept).find(|f| f.id == id)
    }

    /// The skills `fix` is about, as the scan holds them.
    fn members(&self, fix: &api::SkillFix) -> Vec<&Found> {
        fix.skills.iter().filter_map(|id| self.get(id)).collect()
    }
}

/// A skill as the scan holds it: what the API says of it, its real folder
/// (or command file), and each place a CLI finds it, beside its load.
#[derive(Debug, Clone)]
pub struct Found {
    pub skill: api::Skill,
    /// `None` for a broken link.
    real: Option<PathBuf>,
    /// A broken link's target, as it read.
    target: Option<PathBuf>,
    /// A hash of its file's text, as it read.
    hash: Option<u64>,
    finds: Vec<Find>,
}

/// One place a CLI finds a skill.
#[derive(Debug, Clone)]
struct Find {
    cli: Cli,
    scope: Scope,
    /// The folder (or command file) as the CLI reads it.
    path: PathBuf,
    /// The name it goes by when its frontmatter names none: its folder's,
    /// or a command's path under `commands/`, `:`-joined.
    fallback: String,
    /// What the CLI puts in front of the name: `plugin:` for a plugin's.
    prefix: String,
    command: bool,
    project: Option<usize>,
    space: Option<String>,
    origin: Origin,
}

impl Find {
    fn at(cli: Cli, scope: Scope) -> Find {
        Find {
            cli,
            scope,
            path: PathBuf::new(),
            fallback: String::new(),
            prefix: String::new(),
            command: false,
            project: None,
            space: None,
            origin: Origin::Disk,
        }
    }

    fn of(self, project: usize) -> Find {
        Find {
            project: Some(project),
            ..self
        }
    }

    /// Seen by sessions everywhere, not only one project's or space's.
    fn everywhere(&self) -> bool {
        self.project.is_none() && self.space.is_none()
    }

    /// Seen by a session in `context`: everywhere (`None`), or in that
    /// project.
    fn seen_in(&self, context: Option<usize>, places: &Places) -> bool {
        match (context, self.project, &self.space) {
            (None, _, _) => self.everywhere(),
            (Some(p), Some(q), _) => p == q,
            (Some(p), None, Some(space)) => places.projects[p]
                .space
                .as_deref()
                .is_some_and(|s| same_space(s, space)),
            (Some(_), None, None) => true,
        }
    }

    /// Named under a prefix of its own, so no other skill can share it.
    fn namespaced(&self) -> bool {
        matches!(self.scope, Scope::Plugin | Scope::ClaudeAi)
    }
}

#[derive(Debug, Clone)]
enum Origin {
    Disk,
    Plugin(Plugin),
    ClaudeAi,
    Bundled(Cli),
}

/// An installed Claude plugin.
#[derive(Debug, Clone)]
struct Plugin {
    /// `<entry>@<marketplace>`, as `enabledPlugins` names it.
    id: String,
    /// The manifest's name, which its skills are named under.
    name: String,
    version: Option<String>,
    marketplace: String,
    /// What the marketplace comes from.
    from: Option<String>,
    /// The folder Claude loads it from: its source folder for a plugin of a
    /// marketplace added from a local path, else its cached copy.
    root: PathBuf,
    /// Loaded from its source folder, which no update of it replaces.
    in_place: bool,
    /// Installed for one project only.
    project: Option<usize>,
}

/// Every skill the installed CLIs find in `places`, with its problems.
pub fn scan(places: &Places) -> Scan {
    let clis = places.clis();
    let mut finds = Vec::new();
    for cli in &clis {
        match cli {
            Cli::Claude => claude_finds(places, &mut finds),
            Cli::Codex => codex_finds(places, &mut finds),
            Cli::Cursor => cursor_finds(places, &mut finds),
        }
    }

    let mut groups: Vec<(PathBuf, Option<PathBuf>, Vec<Find>)> = Vec::new();
    let mut at: HashMap<PathBuf, usize> = HashMap::new();
    for find in finds {
        let real = fs::canonicalize(&find.path).ok();
        let key = real.clone().unwrap_or_else(|| find.path.clone());
        match at.get(&key) {
            Some(&i) => groups[i].2.push(find),
            None => {
                at.insert(key.clone(), groups.len());
                groups.push((key, real, vec![find]));
            }
        }
    }

    let lock = InstallerLock::read(places);
    let mut repos = Repos::default();
    let mut found: Vec<Found> = groups
        .into_iter()
        .map(|(key, real, finds)| build(key, real, finds, places, &lock, &mut repos))
        .collect();
    found.sort_by(|a, b| {
        let key = |f: &Found| (f.skill.name.to_lowercase(), f.skill.path.clone());
        key(a).cmp(&key(b))
    });
    let settings = Settings::read(places);
    switches(&mut found, places, &settings);
    let dups = duplicates(&mut found, places);
    if clis.len() > 1 {
        only_one_cli(&mut found);
    }
    write::controls(&mut found, places, &settings);
    let fixes = fixes::fixes(&found, &dups, places, &clis);
    let (fixes, kept) = acts::kept(fixes, &mut found, places);

    let json = serde_json::to_string(&api::Skills {
        skills: found.iter().map(|f| &f.skill).collect(),
        fixes: &fixes,
        kept: &kept,
        home: places.home.display().to_string(),
        clis,
    })
    .expect("the skills serialize");
    Scan {
        found,
        fixes,
        kept,
        json,
        places: places.clone(),
    }
}

/// FNV-1a over `bytes`.
fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// A stable id for the skill at `path`: a hash of the path.
fn id_of(path: &Path) -> String {
    format!("{:016x}", fnv(path.as_os_str().as_encoded_bytes()))
}

/// `target` read from `dir`, its `..` taken off what is before it: where a
/// link points, whether or not anything is there.
fn lexical(dir: &Path, target: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in dir.join(target).components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            part => out.push(part),
        }
    }
    out
}

/// `path` under `~` when it is in `home`, for a sentence.
fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

fn problem(kind: api::SkillProblemKind, text: impl Into<String>) -> api::SkillProblem {
    api::SkillProblem {
        kind,
        text: text.into(),
        kept: false,
    }
}

/// One skill out of every place a CLI finds the folder (or file) at `key`,
/// its real path `real` (`None` for a broken link), with what its own
/// file says is wrong with it.
fn build(
    key: PathBuf,
    real: Option<PathBuf>,
    finds: Vec<Find>,
    places: &Places,
    lock: &InstallerLock,
    repos: &mut Repos,
) -> Found {
    use api::SkillProblemKind as P;
    let command = finds[0].command;
    let mut problems = Vec::new();
    let target = real.is_none().then(|| fs::read_link(&key).ok()).flatten();
    let text = match &real {
        None => {
            let at = target.as_ref().map_or_else(
                || "?".into(),
                |t| tilde(&lexical(key.parent().unwrap_or(&key), t), &places.home),
            );
            problems.push(problem(
                P::BrokenLink,
                format!("The link points at {at}, which is not there."),
            ));
            None
        }
        Some(real) => match skill_file(real, command).and_then(|f| read_text(&f)) {
            Ok(text) => Some(text),
            Err((kind, why)) => {
                problems.push(problem(kind, why));
                None
            }
        },
    };
    let (fields, body) = own_file(text.as_deref(), &finds, real.as_deref(), &mut problems);
    let fm_name = field(&fields, "name").map(String::from);
    let fm_description = field(&fields, "description");

    let description = fm_description.map(String::from).or_else(|| {
        body.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(String::from)
    });
    let loads = finds
        .iter()
        .map(|f| api::SkillLoad {
            cli: f.cli,
            name: match (&fm_name, f.command) {
                (Some(name), false) => format!("{}{name}", f.prefix),
                _ => format!("{}{}", f.prefix, f.fallback),
            },
            scope: f.scope,
            path: f.path.display().to_string(),
            project: f.project.map(|i| places.projects[i].name.clone()),
            space: f.space.clone(),
            shadowed_by: None,
        })
        .collect();
    let repo = repos.of(real.as_deref().unwrap_or(&key));
    let source = finds
        .iter()
        .find_map(|f| match &f.origin {
            Origin::Disk => None,
            Origin::Plugin(p) => Some(api::SkillSource::Plugin {
                plugin: p.name.clone(),
                version: p.version.clone(),
                marketplace: p.marketplace.clone(),
                from: p.from.clone(),
            }),
            Origin::ClaudeAi => Some(api::SkillSource::ClaudeAi),
            Origin::Bundled(cli) => Some(api::SkillSource::Bundled { cli: *cli }),
        })
        .or_else(|| real.as_deref().and_then(|r| lock.source(r)))
        .unwrap_or(match repo {
            Some(_) => api::SkillSource::Repo,
            None => api::SkillSource::Folder,
        });
    Found {
        skill: api::Skill {
            id: id_of(&key),
            name: fm_name
                .filter(|_| !command)
                .unwrap_or_else(|| finds[0].fallback.clone()),
            description,
            kind: if command {
                api::SkillKind::Command
            } else {
                api::SkillKind::Skill
            },
            path: key.display().to_string(),
            source,
            repo,
            loads,
            switches: Vec::new(),
            controls: Vec::new(),
            problems,
        },
        hash: text.as_deref().map(|t| fnv(t.as_bytes())),
        real,
        target,
        finds,
    }
}

/// What a skill's own file, `text` as read (`None`: not read), says: its
/// frontmatter fields and what follows them, with what is wrong with them
/// pushed onto `problems`, for the skill the CLIs find as `finds` in the
/// real folder `real`. The one check of a file's text, so the page's check
/// of a text not yet saved says what the scan will.
fn own_file<'a>(
    text: Option<&'a str>,
    finds: &[Find],
    real: Option<&Path>,
    problems: &mut Vec<api::SkillProblem>,
) -> (file::Fields, &'a str) {
    use api::SkillProblemKind as P;
    let command = finds[0].command;
    let (parsed, body) = text.map_or((Ok(Vec::new()), ""), frontmatter);
    let parses = parsed.is_ok();
    let fields = match parsed {
        Ok(fields) => fields,
        Err(why) => {
            problems.push(problem(P::BadFrontmatter, why));
            Vec::new()
        }
    };
    if command {
        return (fields, body);
    }
    let fm_name = field(&fields, "name");
    let fm_description = field(&fields, "description");
    let folder = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    if let Some(name) = fm_name {
        // A folder's name, and each link's to it; a plugin whose root is
        // its one skill names it after itself instead.
        let natural = finds
            .iter()
            .any(|f| folder(&f.path).as_ref() == Some(&f.fallback));
        let real_folder = real.filter(|_| natural).and_then(folder);
        let links = finds
            .iter()
            .filter(|f| is_link(&f.path))
            .map(|f| f.fallback.clone());
        let mismatch = real_folder
            .map(|f| (f, "its folder is"))
            .into_iter()
            .chain(links.map(|f| (f, "the link to it is")))
            .find(|(f, _)| f != name);
        if let Some((other, what)) = mismatch {
            problems.push(problem(
                P::NameMismatch,
                format!("The frontmatter names it `{name}`, but {what} `{other}`."),
            ));
        }
        if !valid_name(name) {
            problems.push(problem(
                P::BadName,
                format!(
                    "The name `{name}` breaks the Agent Skills rule: at most {NAME_MAX} \
                     characters, lowercase letters, digits and single hyphens."
                ),
            ));
        }
    }
    if let Some(d) = fm_description
        .map(|d| d.chars().count())
        .filter(|n| *n > DESCRIPTION_MAX)
    {
        problems.push(problem(
            P::LongDescription,
            format!(
                "The description is {d} characters, over the 1,024 the Agent Skills spec allows."
            ),
        ));
    }
    let missing = match (fm_name.is_some(), fm_description.is_some()) {
        (false, false) => Some("neither"),
        (false, true) => Some("no `name`"),
        (true, false) => Some("no `description`"),
        (true, true) => None,
    };
    let codex = parses && text.is_some() && finds.iter().any(|f| f.cli == Cli::Codex);
    if let Some(missing) = missing.filter(|_| codex) {
        problems.push(problem(
            P::MissingField,
            format!(
                "Codex needs a `name` and a `description` in the frontmatter, and this one \
                 has {missing}."
            ),
        ));
    }
    (fields, body)
}

/// How Claude ranks the places it finds two skills of one name: managed
/// over personal over project, an added directory's under all three, and
/// any skill over a command.
fn claude_rank(f: &Find) -> (bool, u8) {
    let rank = match f.scope {
        Scope::Managed => 0,
        Scope::Personal => 1,
        Scope::Project => 2,
        _ => 3,
    };
    (f.command, rank)
}

/// Skills one CLI finds under one name, everywhere or in a project.
struct Dup {
    cli: Cli,
    name: String,
    /// Each skill, by its index in the scan, and the find it is found by.
    members: Vec<(usize, usize)>,
    /// The skill Claude uses, when its ranks tell one.
    winner: Option<usize>,
}

impl Found {
    /// Switched off for `cli` in a session in `context`, so that CLI does
    /// not list it there.
    fn off_for(&self, cli: Cli, context: Option<usize>, places: &Places) -> bool {
        let project = context.map(|p| places.projects[p].name.as_str());
        self.skill.switches.iter().any(|w| {
            w.cli == cli
                && w.state == api::SkillState::Off
                && (w.project.is_none() || w.project.as_deref() == project)
        })
    }
}

/// Two skills one CLI finds under one name, everywhere or in a project,
/// each told about the other and which one wins, and a load Claude passes
/// over for the winner told which one that is. A plugin's and a synced
/// skill are named apart, Claude loads a nested skill beside a project's
/// of the same name, and a skill switched off for a CLI is not one it
/// lists.
fn duplicates(found: &mut [Found], places: &Places) -> Vec<Dup> {
    let contexts: Vec<Option<usize>> = std::iter::once(None)
        .chain((0..places.projects.len()).map(Some))
        .collect();
    let mut said: Vec<(usize, String)> = Vec::new();
    let mut dups = Vec::new();
    for cli in [Cli::Claude, Cli::Codex, Cli::Cursor] {
        for &context in &contexts {
            let mut by_name: BTreeMap<&str, BTreeMap<usize, usize>> = BTreeMap::new();
            for (s, skill) in found.iter().enumerate() {
                for (f, find) in skill.finds.iter().enumerate() {
                    let counts = find.cli == cli
                        && !find.namespaced()
                        && find.scope != Scope::Nested
                        && find.seen_in(context, places)
                        && !skill.off_for(cli, context, places);
                    if !counts {
                        continue;
                    }
                    let best = by_name.entry(&skill.skill.loads[f].name).or_default();
                    match best.get(&s) {
                        Some(&g) if claude_rank(&skill.finds[g]) <= claude_rank(find) => {}
                        _ => {
                            best.insert(s, f);
                        }
                    }
                }
            }
            for (name, members) in by_name {
                if members.len() < 2 {
                    continue;
                }
                let members: Vec<(usize, usize)> = members.into_iter().collect();
                let finds: Vec<(usize, &Find)> = members
                    .iter()
                    .map(|&(s, f)| (s, &found[s].finds[f]))
                    .collect();
                if context.is_some() && finds.iter().all(|(_, f)| f.everywhere()) {
                    continue;
                }
                let (texts, winner) = duplicate_texts(cli, name, &finds, places);
                said.extend(texts);
                dups.push(Dup {
                    cli,
                    name: name.to_string(),
                    members,
                    winner,
                });
            }
        }
    }
    for (s, text) in said {
        let problems = &mut found[s].skill.problems;
        if !problems.iter().any(|p| p.text == text) {
            problems.push(problem(api::SkillProblemKind::DuplicateName, text));
        }
    }
    for dup in &dups {
        let Some(winner) = dup.winner else { continue };
        let id = found[winner].skill.id.clone();
        for &(s, f) in dup.members.iter().filter(|(s, _)| *s != winner) {
            // An added directory's loses its name, not its place.
            if found[s].finds[f].scope != Scope::AddedDir {
                found[s].skill.loads[f].shadowed_by = Some(id.clone());
            }
        }
    }
    dups
}

/// What each of `members` says of the others, and the one Claude uses
/// when its ranks tell.
fn duplicate_texts(
    cli: Cli,
    name: &str,
    members: &[(usize, &Find)],
    places: &Places,
) -> (Vec<(usize, String)>, Option<usize>) {
    let describe = |f: &Find| {
        let place = match (f.scope, f.project, &f.space) {
            (_, Some(p), _) => format!("project {}", places.projects[p].name),
            (_, None, Some(space)) => format!("space {space}"),
            (Scope::Managed, ..) => "managed".into(),
            (Scope::Admin, ..) => "admin".into(),
            (Scope::System, ..) => "bundled".into(),
            _ => "personal".into(),
        };
        format!("{} ({place})", tilde(&f.path, &places.home))
    };
    let others = |me: usize| {
        members
            .iter()
            .filter(|(s, _)| *s != me)
            .map(|(_, f)| describe(f))
            .collect::<Vec<_>>()
            .join(" and ")
    };
    let title = cli.title();
    let mut ranked: Vec<&(usize, &Find)> = members.iter().collect();
    ranked.sort_by_key(|(_, f)| claude_rank(f));
    let tie = claude_rank(ranked[0].1) == claude_rank(ranked[1].1);
    let winner = (cli == Cli::Claude && !tie).then_some(ranked[0].0);
    let texts = members
        .iter()
        .map(|&(s, f)| {
            let head = format!("{title} finds another skill named `{name}`, {}", others(s));
            let text = match cli {
                Cli::Claude if tie => format!("{head}; it documents no order between them."),
                Cli::Claude if s == ranked[0].0 => format!("{head}, and uses this one."),
                Cli::Claude if f.scope == Scope::AddedDir => format!(
                    "{head}, and uses {} instead, running this one under a prefixed name.",
                    describe(ranked[0].1)
                ),
                Cli::Claude => format!("{head}, and uses {} instead.", describe(ranked[0].1)),
                Cli::Codex => format!("{head}, and lists both."),
                Cli::Cursor => format!("{head}; it documents no order between them."),
            };
            (s, text)
        })
        .collect();
    (texts, winner)
}

/// A skill of the user's own that one CLI alone loads. A plugin's, a
/// synced, managed or bundled one belongs to the CLI that put it there.
fn only_one_cli(found: &mut [Found]) {
    for f in found {
        let mut clis = f.finds.iter().map(|f| f.cli);
        let Some(cli) = clis.next() else { continue };
        let own = f.finds.iter().all(|f| {
            matches!(
                f.scope,
                Scope::Personal | Scope::Project | Scope::Nested | Scope::AddedDir
            )
        });
        if own && !f.finds[0].command && f.real.is_some() && clis.all(|c| c == cli) {
            f.skill.problems.push(problem(
                api::SkillProblemKind::OneCli,
                format!("Only {} loads it.", cli.title()),
            ));
        }
    }
}

/// A skill's detail: its file read fresh, through [`skill_file`], with
/// what the page needs to edit it or why it does not, the files in its
/// folder, and what its repository has not committed.
pub fn detail(found: &Found, places: &Places) -> api::SkillDetail {
    let command = found.skill.kind == api::SkillKind::Command;
    let mut detail = api::SkillDetail {
        skill: found.skill.clone(),
        file: None,
        frontmatter: Vec::new(),
        frontmatter_error: None,
        body: None,
        unread: None,
        files: Vec::new(),
        truncated: false,
        edit: None,
        uneditable: None,
        changes: found
            .skill
            .repo
            .as_ref()
            .and_then(|r| write::changed_paths(Path::new(&r.root)))
            .filter(|(n, _)| *n > 0)
            .map(|(n, deleted)| write::changes_text(n, deleted)),
    };
    let Some(real) = &found.real else {
        detail.unread = Some("The link is broken: there is no file to read.".into());
        detail.uneditable = save::uneditable(found, places);
        return detail;
    };
    match skill_file(real, command).and_then(|file| read_text(&file).map(|text| (file, text))) {
        Ok((file, text)) => {
            let (parsed, body) = frontmatter(&text);
            match parsed {
                Ok(fields) => {
                    detail.frontmatter = fields
                        .into_iter()
                        .map(|(key, value)| api::SkillField { key, value })
                        .collect();
                }
                Err(why) => detail.frontmatter_error = Some(why),
            }
            detail.body = Some(body.to_string());
            detail.file = Some(file.display().to_string());
            match save::edit_of(found, places, &text) {
                Ok(edit) => detail.edit = Some(edit),
                Err(why) => detail.uneditable = Some(why),
            }
        }
        Err((_, why)) => {
            detail.unread = Some(why);
            detail.uneditable = Some(
                save::uneditable(found, places)
                    .unwrap_or_else(|| save::not_edited("the page cannot read the file")),
            );
        }
    }
    if command {
        detail.files = vec![file_entry(
            real.file_name().map(PathBuf::from).unwrap_or_default(),
            real,
        )];
    } else {
        (detail.files, detail.truncated) = files_in(real);
    }
    detail
}

/// The latest scan, kept [`FRESH`] and shared by every request meanwhile:
/// a request that finds it stale scans again, on a blocking thread, while
/// the rest wait for that scan. Beside it, what Claude's transcripts held
/// when they were read.
#[derive(Clone)]
pub struct Cache {
    held: Arc<tokio::sync::Mutex<Option<Held>>>,
    places: Arc<PlacesFor>,
    transcripts: Arc<std::sync::Mutex<usage::Transcripts>>,
    /// Held while the page writes a setting, so two of its writes never race.
    writing: Arc<tokio::sync::Mutex<()>>,
    /// What each act the page made can undo, by the token its answer gave.
    undos: Arc<std::sync::Mutex<Undos>>,
}

/// Undos by token: this server's start time and a count, so a token a page
/// kept from an earlier server names nothing in this one.
struct Undos {
    run: String,
    next: u64,
    held: HashMap<String, Undo>,
}

/// A scan and when it was taken.
type Held = (Instant, Arc<Scan>);

/// The places to scan for nebula's projects.
type PlacesFor = dyn Fn(Vec<ProjectRoot>) -> Places + Send + Sync;

impl Cache {
    /// Scans of this machine.
    pub fn here() -> Cache {
        Cache::of(Places::here)
    }

    /// Scans of the places `places` gives for nebula's projects.
    pub fn of(places: impl Fn(Vec<ProjectRoot>) -> Places + Send + Sync + 'static) -> Cache {
        Cache {
            held: Arc::default(),
            places: Arc::new(places),
            transcripts: Arc::default(),
            writing: Arc::default(),
            undos: Arc::new(std::sync::Mutex::new(Undos {
                run: format!(
                    "{:x}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                ),
                next: 0,
                held: HashMap::new(),
            })),
        }
    }

    /// A scan at most [`FRESH`] old, with `projects` nebula's; `None` when
    /// one failed.
    pub async fn scan(&self, projects: Vec<ProjectRoot>) -> Option<Arc<Scan>> {
        let mut held = self.held.lock().await;
        if let Some((_, scan)) = held.as_ref().filter(|(at, _)| at.elapsed() < FRESH) {
            return Some(scan.clone());
        }
        let places = self.places.clone();
        let fresh = tokio::task::spawn_blocking(move || scan(&places(projects)))
            .await
            .ok()?;
        let fresh = Arc::new(fresh);
        *held = Some((Instant::now(), fresh.clone()));
        Some(fresh)
    }

    /// `write` run on a blocking thread, one write at a time, after which
    /// the next scan reads the disk again; `None` when it did not finish.
    pub async fn write<T: Send + 'static>(
        &self,
        write: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let _one = self.writing.lock().await;
        let done = tokio::task::spawn_blocking(write).await.ok();
        *self.held.lock().await = None;
        done
    }

    /// `act` made as [`Cache::write`] makes a write, its Undo held under the
    /// token its answer carries.
    pub async fn act(
        &self,
        act: impl FnOnce() -> Result<(api::SkillDone, Option<Undo>), crate::Refusal> + Send + 'static,
    ) -> Option<Result<api::SkillDone, crate::Refusal>> {
        let done = self.write(act).await?;
        Some(done.map(|(mut done, undo)| {
            done.undo = undo.map(|undo| {
                let mut undos = self
                    .undos
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                undos.next += 1;
                let token = format!("{}.{}", undos.run, undos.next);
                undos.held.insert(token.clone(), undo);
                token
            });
            done
        }))
    }

    /// The Undo `token` names, which only one asking gets.
    pub fn take_undo(&self, token: &str) -> Option<Undo> {
        self.undos
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .held
            .remove(token)
    }

    /// How often Claude used each skill of `scan` lately, on a blocking
    /// thread; a request meanwhile waits for this one's read and then
    /// opens only what changed since. `None` when the read failed.
    pub async fn usage(&self, scan: Arc<Scan>) -> Option<api::SkillUsage> {
        let transcripts = self.transcripts.clone();
        tokio::task::spawn_blocking(move || {
            let mut held = transcripts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            usage::usage(&scan, &mut held, std::time::SystemTime::now())
        })
        .await
        .ok()
    }
}
