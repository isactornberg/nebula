//! The settings that keep a skill from a CLI: Claude's settings files, in
//! the order a session reads them, and Codex's `config.toml`.

use super::*;

/// One Claude settings file: the keys the scan reads in it.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct ClaudeSettings {
    skill_overrides: BTreeMap<String, String>,
    enabled_plugins: BTreeMap<String, serde_json::Value>,
    sync_claude_ai_skills: Option<bool>,
}

pub(super) struct Layer {
    file: PathBuf,
    project: Option<usize>,
    settings: ClaudeSettings,
}

impl Layer {
    pub(super) fn read(file: PathBuf, project: Option<usize>) -> Layer {
        Layer {
            settings: read_json(&file).unwrap_or_default(),
            file,
            project,
        }
    }
}

/// Claude's settings files, each project's with them.
pub(super) struct ClaudeLayers {
    managed: Layer,
    user: Layer,
    /// `.claude/settings.local.json` and `.claude/settings.json`, by project.
    projects: Vec<[Layer; 2]>,
}

impl ClaudeLayers {
    pub(super) fn read(places: &Places) -> ClaudeLayers {
        ClaudeLayers {
            managed: Layer::read(places.claude_managed.join("managed-settings.json"), None),
            user: Layer::read(places.claude.join("settings.json"), None),
            projects: places
                .projects
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let dot = p.root.join(".claude");
                    [
                        Layer::read(dot.join("settings.local.json"), Some(i)),
                        Layer::read(dot.join("settings.json"), Some(i)),
                    ]
                })
                .collect(),
        }
    }

    /// The files a session in `context` reads, the one that wins first.
    pub(super) fn in_context(&self, context: Option<usize>) -> Vec<&Layer> {
        let mut layers = vec![&self.managed];
        if let Some(p) = context {
            layers.extend(&self.projects[p]);
        }
        layers.push(&self.user);
        layers
    }
}

/// Codex's `config.toml`: the keys the scan reads in it.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct CodexConfig {
    skills: CodexSkills,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct CodexSkills {
    config: Vec<CodexEntry>,
    bundled: CodexBundled,
}

#[derive(Deserialize)]
pub(super) struct CodexEntry {
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default = "yes")]
    enabled: bool,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct CodexBundled {
    enabled: Option<bool>,
}

pub(super) fn yes() -> bool {
    true
}

pub(super) fn read_toml(path: &Path) -> CodexConfig {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

/// Every settings file a switch is in, read once for a scan.
pub(super) struct Settings {
    pub(super) claude: ClaudeLayers,
    pub(super) codex_file: PathBuf,
    pub(super) codex: CodexConfig,
    /// Each project's `.codex/config.toml`, by project.
    codex_projects: Vec<(PathBuf, CodexConfig)>,
}

impl Settings {
    pub(super) fn read(places: &Places) -> Settings {
        let codex_file = places.codex.join("config.toml");
        Settings {
            claude: ClaudeLayers::read(places),
            codex: read_toml(&codex_file),
            codex_file,
            codex_projects: places
                .projects
                .iter()
                .map(|p| {
                    let file = p.root.join(".codex/config.toml");
                    let config = read_toml(&file);
                    (file, config)
                })
                .collect(),
        }
    }
}

impl CodexConfig {
    /// Whether Codex loads the skill whose `SKILL.md` is at one of `paths`
    /// and whose name is `name`: the last `[[skills.config]]` entry that
    /// selects it says, and an entry with both selectors or neither is
    /// one Codex ignores.
    pub(super) fn enabled(&self, paths: &[PathBuf], name: &str) -> bool {
        self.skills
            .config
            .iter()
            .rfind(|e| match (&e.path, e.name.as_deref()) {
                (Some(path), None) => paths.contains(path),
                (None, Some(n)) => !n.is_empty() && n == name,
                _ => false,
            })
            .is_none_or(|e| e.enabled)
    }

    pub(super) fn bundled_off(&self) -> bool {
        self.skills.bundled.enabled == Some(false)
    }
}

impl Found {
    /// The `SKILL.md` of each place Codex finds it at, its real one first.
    pub(super) fn codex_skill_mds(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self.real.iter().map(|r| r.join("SKILL.md")).collect();
        for find in self.finds.iter().filter(|f| f.cli == Cli::Codex) {
            let md = find.path.join("SKILL.md");
            if !out.contains(&md) {
                out.push(md);
            }
        }
        out
    }
}

/// Every setting that keeps a skill from a CLI: Claude's `skillOverrides`,
/// `enabledPlugins` and `syncClaudeAiSkills` in whichever settings file
/// wins for a session everywhere and in each project, and Codex's
/// `[[skills.config]]` and `[skills.bundled]`.
pub(super) fn switches(found: &mut [Found], places: &Places, settings: &Settings) {
    let contexts: Vec<Option<usize>> = std::iter::once(None)
        .chain((0..places.projects.len()).map(Some))
        .collect();
    let label = |file: &Path| tilde(file, &places.home);
    let project_of = |layer: &Layer| layer.project.map(|p| places.projects[p].name.clone());

    for f in found.iter_mut() {
        let mut out: Vec<api::SkillSwitch> = Vec::new();
        let skill_mds = f.codex_skill_mds();
        for (find, load) in f.finds.iter().zip(&f.skill.loads) {
            match find.cli {
                Cli::Claude => {
                    for &context in contexts.iter().filter(|c| find.seen_in(**c, places)) {
                        let layers = settings.claude.in_context(context);
                        if let Some((state, setting, layer)) =
                            claude_switch(find, &load.name, &layers)
                        {
                            out.push(api::SkillSwitch {
                                cli: Cli::Claude,
                                state,
                                setting,
                                file: layer.map(|l| label(&l.file)),
                                project: layer.and_then(project_of),
                            });
                        }
                    }
                }
                Cli::Codex => {
                    if !settings.codex.enabled(&skill_mds, &load.name) {
                        out.push(api::SkillSwitch {
                            cli: Cli::Codex,
                            state: api::SkillState::Off,
                            setting: "skills.config",
                            file: Some(label(&settings.codex_file)),
                            project: None,
                        });
                    }
                    if find.scope == Scope::System {
                        let user = std::iter::once((None, &settings.codex_file, &settings.codex));
                        let projects = settings
                            .codex_projects
                            .iter()
                            .enumerate()
                            .map(|(i, (file, config))| (Some(i), file, config));
                        for (project, file, config) in user.chain(projects) {
                            if config.bundled_off() {
                                out.push(api::SkillSwitch {
                                    cli: Cli::Codex,
                                    state: api::SkillState::Off,
                                    setting: "skills.bundled",
                                    file: Some(label(file)),
                                    project: project.map(|p| places.projects[p].name.clone()),
                                });
                            }
                        }
                    }
                }
                Cli::Cursor => {}
            }
        }
        let mut unique = Vec::new();
        for switch in out {
            if !unique.contains(&switch) {
                unique.push(switch);
            }
        }
        f.skill.switches = unique;
    }
}

/// What keeps Claude from the skill `find` finds as `name`, in a session
/// that reads `layers`, the file that wins first: a plugin no file enables,
/// or one a file disables; synced skills when syncing is off; else its
/// `skillOverrides` entry. The layer is `None` for what no file says.
pub(super) fn claude_switch<'a>(
    find: &Find,
    name: &str,
    layers: &[&'a Layer],
) -> Option<(api::SkillState, &'static str, Option<&'a Layer>)> {
    let decide = |get: &dyn Fn(&ClaudeSettings) -> Option<String>| {
        layers
            .iter()
            .find_map(|l| get(&l.settings).map(|v| (v, *l)))
    };
    match &find.origin {
        Origin::Plugin(plugin) => {
            let enabled = |s: &ClaudeSettings| {
                let on = s.enabled_plugins.get(&plugin.id)?.as_bool()?;
                Some(on.to_string())
            };
            match decide(&enabled) {
                Some((on, _)) if on == "true" => None,
                decided => Some((
                    api::SkillState::Off,
                    "enabledPlugins",
                    decided.map(|(_, l)| l),
                )),
            }
        }
        Origin::ClaudeAi => {
            let off = |s: &ClaudeSettings| s.sync_claude_ai_skills.map(|on| on.to_string());
            match decide(&off) {
                Some((on, layer)) if on == "false" => {
                    Some((api::SkillState::Off, "syncClaudeAiSkills", Some(layer)))
                }
                _ => overridden(name, layers),
            }
        }
        _ => overridden(name, layers),
    }
}

/// The `skillOverrides` entry for `name` in the first of `layers` that has
/// one, and that layer; none for `on`, or a value Claude does not know.
fn overridden<'a>(
    name: &str,
    layers: &[&'a Layer],
) -> Option<(api::SkillState, &'static str, Option<&'a Layer>)> {
    let (value, layer) = layers
        .iter()
        .find_map(|l| Some((l.settings.skill_overrides.get(name)?, *l)))?;
    let state = match setting_of(value)? {
        api::SkillSetting::Off => api::SkillState::Off,
        api::SkillSetting::NameOnly => api::SkillState::NameOnly,
        api::SkillSetting::UserInvocableOnly => api::SkillState::UserInvocableOnly,
        api::SkillSetting::On => return None,
    };
    Some((state, "skillOverrides", Some(layer)))
}

/// A `skillOverrides` value, as Claude Code's settings schema spells it.
pub(super) fn setting_of(value: &str) -> Option<api::SkillSetting> {
    use api::SkillSetting as S;
    match value {
        "on" => Some(S::On),
        "name-only" => Some(S::NameOnly),
        "user-invocable-only" => Some(S::UserInvocableOnly),
        "off" => Some(S::Off),
        _ => None,
    }
}

pub(super) fn value_of(setting: api::SkillSetting) -> &'static str {
    use api::SkillSetting as S;
    match setting {
        S::On => "on",
        S::NameOnly => "name-only",
        S::UserInvocableOnly => "user-invocable-only",
        S::Off => "off",
    }
}

impl Layer {
    /// What this file's `skillOverrides` says of `name`, if anything.
    pub(super) fn override_of(&self, name: &str) -> Option<api::SkillSetting> {
        setting_of(self.settings.skill_overrides.get(name)?)
    }
}

impl ClaudeLayers {
    pub(super) fn is_managed(&self, layer: &Layer) -> bool {
        std::ptr::eq(layer, &self.managed)
    }
}
