//! The SKILLS page's resolver: every skill the agent CLIs on this machine
//! load, found where each CLI looks for them, since none of them lists what
//! it resolved. A skill is one real folder holding a `SKILL.md` (or one
//! legacy `.claude/commands` file), however many links and CLIs reach it.
//! The scan says which CLI finds it where and under what name, which
//! setting keeps it from one, where it comes from (the git repository it
//! is in, the skills installer's source, a plugin and its marketplace), and
//! what is wrong with it.
//!
//! It writes nothing. [`scan`] reads the disk and runs one `git ls-files`
//! per project, so it runs on a blocking thread behind [`Cache`], and every
//! place it reads is a field of [`Places`], so tests scan temp dirs. How
//! often Claude used each skill is read apart from the scan ([`usage`]),
//! from its transcripts, so the list never waits for it.
//!
//! Claude Code's plugin state is read from its own files
//! (`~/.claude/plugins/installed_plugins.json`, `known_marketplaces.json`
//! and the `enabledPlugins` settings) rather than `claude plugin list
//! --json`, a process per scan. A plugin no settings file enables is off,
//! as that command reports one.

mod file;
mod finds;
mod fixes;
mod settings;
#[cfg(test)]
mod tests;
mod usage;

use crate::api::{self, SkillCli as Cli, SkillScope as Scope};
use file::{field, file_entry, files_in, frontmatter, read_text, skill_file, valid_name};
use finds::{claude_finds, codex_finds, cursor_finds, is_link, read_json, InstallerLock, Repos};
use serde::Deserialize;
use settings::switches;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// One scan: every skill found, by name, its JSON, which also says what
/// to fix, and the places it read.
pub struct Scan {
    pub found: Vec<Found>,
    pub json: String,
    places: Places,
}

impl Scan {
    pub fn get(&self, id: &str) -> Option<&Found> {
        self.found.iter().find(|f| f.skill.id == id)
    }
}

/// A skill as the scan holds it: what the API says of it, its real folder
/// (or command file), and each place a CLI finds it, beside its load.
#[derive(Debug, Clone)]
pub struct Found {
    pub skill: api::Skill,
    /// `None` for a broken link.
    real: Option<PathBuf>,
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
    let dups = duplicates(&mut found, places);
    if clis.len() > 1 {
        only_one_cli(&mut found);
    }
    switches(&mut found, places);
    let fixes = fixes::fixes(&found, &dups, places, &clis);

    let json = serde_json::to_string(&api::Skills {
        skills: found.iter().map(|f| &f.skill).collect(),
        fixes: &fixes,
        home: places.home.display().to_string(),
        clis,
    })
    .expect("the skills serialize");
    Scan {
        found,
        json,
        places: places.clone(),
    }
}

/// A stable id for the skill at `path`: FNV-1a over the path.
fn id_of(path: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
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
    let text = match &real {
        None => {
            let target = fs::read_link(&key).map_or_else(
                |_| "?".into(),
                |t| tilde(&lexical(key.parent().unwrap_or(&key), &t), &places.home),
            );
            problems.push(problem(
                P::BrokenLink,
                format!("The link points at {target}, which is not there."),
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
    let (parsed, body) = text.as_deref().map_or((Ok(Vec::new()), ""), frontmatter);
    let parses = parsed.is_ok();
    let fields = match parsed {
        Ok(fields) => fields,
        Err(why) => {
            problems.push(problem(P::BadFrontmatter, why));
            Vec::new()
        }
    };
    let fm_name = field(&fields, "name").map(String::from);
    let fm_description = field(&fields, "description");
    let folder = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned());

    if !command {
        if let Some(name) = &fm_name {
            // A folder's name, and each link's to it; a plugin whose root is
            // its one skill names it after itself instead.
            let natural = finds
                .iter()
                .any(|f| folder(&f.path).as_ref() == Some(&f.fallback));
            let real_folder = real.as_deref().filter(|_| natural).and_then(folder);
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
                format!("The description is {d} characters, over the 1,024 the Agent Skills spec allows."),
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
    }

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
            problems,
        },
        real,
        finds,
    }
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

/// Two skills one CLI finds under one name, everywhere or in a project,
/// each told about the other and which one wins, and a load Claude passes
/// over for the winner told which one that is. A plugin's and a synced
/// skill are named apart, and Claude loads a nested skill beside a
/// project's of the same name.
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
                        && find.seen_in(context, places);
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

/// A skill's detail: its file read fresh, through [`skill_file`], and the
/// files in its folder.
pub fn detail(found: &Found) -> api::SkillDetail {
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
    };
    let Some(real) = &found.real else {
        detail.unread = Some("The link is broken: there is no file to read.".into());
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
        }
        Err((_, why)) => detail.unread = Some(why),
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
