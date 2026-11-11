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

/// Every setting that keeps a skill from a CLI: Claude's `skillOverrides`,
/// `enabledPlugins` and `syncClaudeAiSkills` in whichever settings file
/// wins for a session everywhere and in each project, and Codex's
/// `[[skills.config]]` and `[skills.bundled]`.
pub(super) fn switches(found: &mut [Found], places: &Places) {
    let claude = ClaudeLayers::read(places);
    let codex_file = places.codex.join("config.toml");
    let codex = read_toml(&codex_file);
    let codex_projects: Vec<(PathBuf, CodexConfig)> = places
        .projects
        .iter()
        .map(|p| {
            let file = p.root.join(".codex/config.toml");
            let config = read_toml(&file);
            (file, config)
        })
        .collect();
    let contexts: Vec<Option<usize>> = std::iter::once(None)
        .chain((0..places.projects.len()).map(Some))
        .collect();
    let label = |file: &Path| tilde(file, &places.home);
    let project_of = |layer: &Layer| layer.project.map(|p| places.projects[p].name.clone());

    for f in found.iter_mut() {
        let mut out: Vec<api::SkillSwitch> = Vec::new();
        for (find, load) in f.finds.iter().zip(&f.skill.loads) {
            match find.cli {
                Cli::Claude => {
                    for &context in contexts.iter().filter(|c| find.seen_in(**c, places)) {
                        let layers = claude.in_context(context);
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
                    let skill_md = |dir: &Path| dir.join("SKILL.md");
                    let matches = |e: &&CodexEntry| match (&e.path, &e.name) {
                        (Some(path), _) => {
                            path == &skill_md(&find.path)
                                || f.real.as_deref().is_some_and(|r| path == &skill_md(r))
                        }
                        (None, Some(name)) => name == &load.name,
                        (None, None) => false,
                    };
                    if codex
                        .skills
                        .config
                        .iter()
                        .rfind(matches)
                        .is_some_and(|e| !e.enabled)
                    {
                        out.push(api::SkillSwitch {
                            cli: Cli::Codex,
                            state: api::SkillState::Off,
                            setting: "skills.config",
                            file: Some(label(&codex_file)),
                            project: None,
                        });
                    }
                    if find.scope == Scope::System {
                        let user = std::iter::once((None, &codex_file, &codex));
                        let projects = codex_projects
                            .iter()
                            .enumerate()
                            .map(|(i, (file, config))| (Some(i), file, config));
                        for (project, file, config) in user.chain(projects) {
                            if config.skills.bundled.enabled == Some(false) {
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
                _ => None,
            }
        }
        _ => {
            let (value, layer) = decide(&|s| s.skill_overrides.get(name).cloned())?;
            let state = match value.as_str() {
                "off" => api::SkillState::Off,
                "name-only" => api::SkillState::NameOnly,
                "user-invocable-only" => api::SkillState::UserInvocableOnly,
                _ => return None,
            };
            Some((state, "skillOverrides", Some(layer)))
        }
    }
}
