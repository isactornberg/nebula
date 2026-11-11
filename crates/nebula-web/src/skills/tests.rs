use super::*;
use api::SkillProblemKind as P;

mod write;
use std::os::unix::fs::symlink;

/// A machine in a temp dir: a home with Claude, Codex and Cursor in it, and
/// the projects added to it.
struct Machine {
    _tmp: tempfile::TempDir,
    places: Places,
}

impl Machine {
    fn new() -> Machine {
        let tmp = tempfile::tempdir().unwrap();
        // macOS's temp dir is under a link (/var is /private/var).
        let root = fs::canonicalize(tmp.path()).unwrap();
        let home = root.join("home");
        for dir in [".claude", ".codex", ".cursor"] {
            fs::create_dir_all(home.join(dir)).unwrap();
        }
        let places = Places {
            claude: home.join(".claude"),
            claude_managed: root.join("managed"),
            codex: home.join(".codex"),
            codex_admin: root.join("etc-codex"),
            cursor: home.join(".cursor"),
            projects: Vec::new(),
            added: Vec::new(),
            home,
        };
        Machine { _tmp: tmp, places }
    }

    fn home(&self) -> &Path {
        &self.places.home
    }

    /// A project whose root checkout is `~/code/<name>`.
    fn project(&mut self, name: &str) -> PathBuf {
        let root = self.home().join("code").join(name);
        fs::create_dir_all(&root).unwrap();
        self.places.projects.push(ProjectRoot {
            name: name.into(),
            root: root.clone(),
            worktrees: Vec::new(),
            space: None,
        });
        root
    }

    fn scan(&self) -> Scan {
        scan(&self.places)
    }
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A skill folder `dir/<name>` whose SKILL.md has `fields` for frontmatter.
fn skill(dir: &Path, name: &str, fields: &str) -> PathBuf {
    let folder = dir.join(name);
    write(
        &folder.join("SKILL.md"),
        &format!("---\n{fields}\n---\n\n# {name}\n\nWhat it does.\n"),
    );
    folder
}

/// A skill with a name matching its folder and a description.
fn plain(dir: &Path, name: &str) -> PathBuf {
    skill(
        dir,
        name,
        &format!("name: {name}\ndescription: Does {name}."),
    )
}

fn link(link: &Path, target: &Path) {
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    symlink(target, link).unwrap();
}

/// A folder that is a git repository on `branch`, as `.git/HEAD` says.
fn repo(dir: &Path, branch: &str) {
    write(
        &dir.join(".git/HEAD"),
        &format!("ref: refs/heads/{branch}\n"),
    );
}

fn named<'a>(scan: &'a Scan, name: &str) -> Vec<&'a Found> {
    scan.found.iter().filter(|f| f.skill.name == name).collect()
}

fn one<'a>(scan: &'a Scan, name: &str) -> &'a Found {
    let found = named(scan, name);
    assert_eq!(found.len(), 1, "one skill {name}: {found:#?}");
    found[0]
}

fn loads(found: &Found) -> Vec<(Cli, Scope, String)> {
    found
        .skill
        .loads
        .iter()
        .map(|l| (l.cli, l.scope, l.name.clone()))
        .collect()
}

fn claude_dupes(found: &Found) -> Vec<&str> {
    texts(found, P::DuplicateName)
        .into_iter()
        .filter(|t| t.starts_with("Claude"))
        .collect()
}

/// The fixes a scan lists, as its JSON has them.
fn fixes(scan: &Scan) -> Vec<serde_json::Value> {
    let json: serde_json::Value = serde_json::from_str(&scan.json).unwrap();
    json["fixes"].as_array().unwrap().clone()
}

fn kinds(found: &Found) -> Vec<P> {
    found.skill.problems.iter().map(|p| p.kind).collect()
}

fn texts(found: &Found, kind: P) -> Vec<&str> {
    found
        .skill
        .problems
        .iter()
        .filter(|p| p.kind == kind)
        .map(|p| p.text.as_str())
        .collect()
}

/// A skill reached through links from several CLIs' folders is one skill
/// at its real path, in the repository the target is in, and every CLI
/// that reads one of those folders loads it.
#[test]
fn links_into_a_repository_are_one_skill_in_it() {
    let m = Machine::new();
    let toolbox = m.home().join("src/toolbox");
    repo(&toolbox, "main");
    let real = plain(&toolbox.join("skills"), "capture");
    link(&m.home().join(".claude/skills/capture"), &real);
    link(&m.home().join(".agents/skills/capture"), &real);

    let scan = m.scan();
    let capture = one(&scan, "capture");
    assert_eq!(capture.skill.path, real.display().to_string());
    assert_eq!(capture.skill.source, api::SkillSource::Repo);
    assert_eq!(
        capture.skill.repo,
        Some(api::GitRepo {
            root: toolbox.display().to_string(),
            branch: Some("main".into()),
        })
    );
    assert_eq!(capture.skill.description.as_deref(), Some("Does capture."));
    let mut clis: Vec<(Cli, Scope)> = capture
        .skill
        .loads
        .iter()
        .map(|l| (l.cli, l.scope))
        .collect();
    clis.sort_by_key(|(cli, _)| *cli);
    assert_eq!(
        clis,
        [
            (Cli::Claude, Scope::Personal),
            (Cli::Codex, Scope::Personal),
            (Cli::Cursor, Scope::Personal),
            (Cli::Cursor, Scope::Personal),
        ]
    );
    let paths: BTreeSet<&str> = capture
        .skill
        .loads
        .iter()
        .map(|l| l.path.as_str())
        .collect();
    assert!(
        paths.contains(m.home().join(".claude/skills/capture").to_str().unwrap()),
        "a load names the link it went through: {paths:?}"
    );
    assert!(
        capture.skill.problems.is_empty(),
        "{:?}",
        capture.skill.problems
    );
    assert_eq!(capture.skill.id, id_of(&real), "the id is the real path's");
}

/// A git worktree's `.git` is a file naming its git dir, whose HEAD says
/// the branch.
#[test]
fn a_linked_worktree_names_its_own_branch() {
    let m = Machine::new();
    let checkout = m.home().join("src/toolbox-feature");
    write(
        &m.home().join("src/toolbox/.git/worktrees/feature/HEAD"),
        "ref: refs/heads/feature\n",
    );
    let git_dir = m.home().join("src/toolbox/.git/worktrees/feature");
    write(
        &checkout.join(".git"),
        &format!("gitdir: {}\n", git_dir.display()),
    );
    plain(&checkout.join(".claude/skills"), "inner");
    let mut repos = Repos::default();
    let repo = repos.of(&checkout.join(".claude/skills/inner")).unwrap();
    assert_eq!(repo.root, checkout.display().to_string());
    assert_eq!(repo.branch.as_deref(), Some("feature"));
}

/// A link whose target is gone is listed, so the page can say so, and has
/// no file to read.
#[test]
fn a_broken_link_is_a_problem_and_reads_nothing() {
    let m = Machine::new();
    let gone = m.home().join("src/gone/skills/old");
    link(&m.home().join(".claude/skills/old"), &gone);

    let scan = m.scan();
    let old = one(&scan, "old");
    assert_eq!(kinds(old), [P::BrokenLink]);
    assert_eq!(
        texts(old, P::BrokenLink),
        ["The link points at ~/src/gone/skills/old, which is not there."]
    );
    let relative = m.home().join(".claude/skills/older");
    symlink("../../src/gone/skills/older", &relative).unwrap();
    let scan = m.scan();
    assert_eq!(
        texts(one(&scan, "older"), P::BrokenLink),
        ["The link points at ~/src/gone/skills/older, which is not there."],
        "a relative target, read from the link's folder"
    );
    let old = one(&scan, "old");
    assert_eq!(
        old.skill.path,
        m.home().join(".claude/skills/old").display().to_string()
    );
    let detail = detail(old);
    assert_eq!(detail.body, None);
    assert_eq!(detail.file, None);
    assert!(detail.unread.is_some());
}

/// Claude ranks personal over project and a skill over a command of the
/// same name; Codex lists two of one name side by side. Each skill says
/// which one wins.
#[test]
fn two_skills_of_one_name_say_which_one_wins() {
    let mut m = Machine::new();
    let shop = m.project("shop");
    let personal = plain(&m.home().join(".claude/skills"), "deploy");
    let project = plain(&shop.join(".claude/skills"), "deploy");
    write(
        &shop.join(".claude/commands/deploy.md"),
        "Deploy the shop.\n",
    );
    plain(&m.home().join(".agents/skills"), "skill-creator");
    plain(&m.home().join(".codex/skills/.system"), "skill-creator");

    let scan = m.scan();
    let deploys = named(&scan, "deploy");
    assert_eq!(deploys.len(), 3, "{deploys:#?}");
    let at = |path: &Path| {
        *deploys
            .iter()
            .find(|f| f.skill.path == path.display().to_string())
            .unwrap()
    };
    let won = claude_dupes(at(&personal));
    assert_eq!(won.len(), 1, "{won:?}");
    assert!(won[0].ends_with("and uses this one."), "{won:?}");
    assert!(
        won[0].contains("~/code/shop/.claude/skills/deploy (project shop)"),
        "{won:?}"
    );
    let lost = claude_dupes(at(&project));
    assert!(
        lost.iter()
            .any(|t| t.contains("uses ~/.claude/skills/deploy (personal) instead")),
        "{lost:?}"
    );
    let command = deploys
        .iter()
        .find(|f| f.skill.kind == api::SkillKind::Command)
        .unwrap();
    assert!(
        claude_dupes(command)
            .iter()
            .all(|t| t.ends_with("instead.")),
        "a command loses to a skill: {:?}",
        claude_dupes(command)
    );

    let creators = named(&scan, "skill-creator");
    assert_eq!(creators.len(), 2);
    for f in creators {
        let codex = texts(f, P::DuplicateName);
        assert_eq!(codex.len(), 1, "{codex:?}");
        assert!(codex[0].starts_with("Codex finds another skill named `skill-creator`"));
        assert!(codex[0].ends_with("and lists both."));
    }
}

/// A plugin's skills are Claude's under `plugin:`, from its cached copy,
/// or in place from a marketplace added from a local folder; one that no
/// settings file enables is off, and `enabledPlugins` says which is on.
#[test]
fn plugin_skills_are_named_under_the_plugin_and_switched_by_enabled_plugins() {
    let m = Machine::new();
    let plugins = m.home().join(".claude/plugins");
    let cached = plugins.join("cache/tools-market/kit/1.0.0");
    write(
        &cached.join(".claude-plugin/plugin.json"),
        r#"{"name": "kit"}"#,
    );
    plain(&cached.join("skills"), "lint");
    let extra = cached.join("extra/format");
    write(
        &extra.join("SKILL.md"),
        "---\nname: format\ndescription: Formats.\n---\n",
    );
    write(
        &cached.join(".claude-plugin/plugin.json"),
        r#"{"name": "kit", "skills": ["./extra/format", "../escape"]}"#,
    );
    let local_market = m.home().join("src/market");
    repo(&local_market, "main");
    write(
        &local_market.join(".claude-plugin/marketplace.json"),
        r#"{"name": "local", "plugins": [{"name": "notes", "source": "./plugins/notes"}]}"#,
    );
    let in_place = local_market.join("plugins/notes");
    write(
        &in_place.join(".claude-plugin/plugin.json"),
        r#"{"name": "notes"}"#,
    );
    plain(&in_place.join("skills"), "jot");
    let stale = plugins.join("cache/local/notes/abc");
    plain(&stale.join("skills"), "jot-copy");
    write(
        &plugins.join("installed_plugins.json"),
        &serde_json::json!({
            "version": 2,
            "plugins": {
                "kit@tools-market": [{"scope": "user", "installPath": cached, "version": "1.0.0"}],
                "notes@local": [{"scope": "user", "installPath": stale, "version": "abc"}],
            }
        })
        .to_string(),
    );
    write(
        &plugins.join("known_marketplaces.json"),
        &serde_json::json!({
            "tools-market": {"source": {"source": "github", "repo": "acme/tools"}, "installLocation": plugins.join("marketplaces/tools-market")},
            "local": {"source": {"source": "directory", "path": local_market}, "installLocation": local_market},
        })
        .to_string(),
    );
    write(
        &m.home().join(".claude/settings.json"),
        r#"{"enabledPlugins": {"notes@local": true}}"#,
    );

    let scan = m.scan();
    let lint = one(&scan, "lint");
    assert_eq!(
        loads(lint),
        [(Cli::Claude, Scope::Plugin, "kit:lint".into())]
    );
    assert_eq!(
        lint.skill.source,
        api::SkillSource::Plugin {
            plugin: "kit".into(),
            version: Some("1.0.0".into()),
            marketplace: "tools-market".into(),
            from: Some("github.com/acme/tools".into()),
        }
    );
    assert_eq!(
        lint.skill.switches,
        [api::SkillSwitch {
            cli: Cli::Claude,
            state: api::SkillState::Off,
            setting: "enabledPlugins",
            file: None,
            project: None,
        }],
        "no settings file enables kit"
    );
    assert_eq!(
        loads(one(&scan, "format"))[0].2,
        "kit:format",
        "the manifest adds a folder"
    );
    assert!(scan.found.iter().all(|f| !f.skill.path.contains("escape")));

    let jot = one(&scan, "jot");
    assert_eq!(
        jot.skill.path,
        in_place.join("skills/jot").display().to_string()
    );
    assert!(jot.skill.switches.is_empty(), "{:?}", jot.skill.switches);
    assert_eq!(
        jot.skill.repo.as_ref().map(|r| r.root.clone()),
        Some(local_market.display().to_string())
    );
    assert!(
        named(&scan, "jot-copy").is_empty(),
        "the cached copy is not what loads"
    );
    assert!(
        !kinds(jot).contains(&P::OneCli),
        "a plugin's skill is Claude's own"
    );
}

/// Claude's `skillOverrides`, in the user's settings or a project's, and
/// Codex's `[[skills.config]]` and `[skills.bundled]`, each say what they
/// switch and where.
#[test]
fn overrides_say_what_they_switch_off_and_where() {
    let mut m = Machine::new();
    let shop = m.project("shop");
    let capture = plain(&m.home().join(".claude/skills"), "capture");
    link(&m.home().join(".agents/skills/capture"), &capture);
    plain(&m.home().join(".claude/skills"), "notes");
    plain(&m.home().join(".agents/skills"), "draw");
    plain(&m.home().join(".codex/skills/.system"), "imagegen");
    write(
        &m.home().join(".claude/settings.json"),
        r#"{"skillOverrides": {"notes": "name-only", "capture": "on"}}"#,
    );
    write(
        &shop.join(".claude/settings.local.json"),
        r#"{"skillOverrides": {"capture": "user-invocable-only"}}"#,
    );
    write(
        &m.home().join(".codex/config.toml"),
        &format!(
            "model = \"x\"\n\n[[skills.config]]\nname = \"capture\"\nenabled = false\n\n\
             [[skills.config]]\npath = \"{}\"\nenabled = false\n\n[skills.bundled]\nenabled = false\n",
            m.home().join(".agents/skills/draw/SKILL.md").display()
        ),
    );

    let scan = m.scan();
    let switch = |cli, state, setting, file: &str, project: Option<&str>| api::SkillSwitch {
        cli,
        state,
        setting,
        file: Some(file.into()),
        project: project.map(String::from),
    };
    assert_eq!(
        one(&scan, "notes").skill.switches,
        [switch(
            Cli::Claude,
            api::SkillState::NameOnly,
            "skillOverrides",
            "~/.claude/settings.json",
            None
        )]
    );
    assert_eq!(
        one(&scan, "capture").skill.switches,
        [
            switch(
                Cli::Claude,
                api::SkillState::UserInvocableOnly,
                "skillOverrides",
                "~/code/shop/.claude/settings.local.json",
                Some("shop")
            ),
            switch(
                Cli::Codex,
                api::SkillState::Off,
                "skills.config",
                "~/.codex/config.toml",
                None
            ),
        ],
        "on everywhere but in shop for Claude, and off by name for Codex"
    );
    assert_eq!(
        one(&scan, "draw").skill.switches,
        [switch(
            Cli::Codex,
            api::SkillState::Off,
            "skills.config",
            "~/.codex/config.toml",
            None
        )],
        "off by its path"
    );
    assert_eq!(
        one(&scan, "imagegen").skill.switches,
        [switch(
            Cli::Codex,
            api::SkillState::Off,
            "skills.bundled",
            "~/.codex/config.toml",
            None
        )]
    );
}

/// A project's skills load for its sessions, a nested `.claude` once
/// Claude works in that folder, and a space's added directory for the
/// sessions of its projects.
#[test]
fn project_nested_and_added_skills_load_where_they_apply() {
    let mut m = Machine::new();
    let shop = m.project("shop");
    m.places.projects[0].space = Some("Acme  Corp".into());
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(&shop)
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    plain(&shop.join(".claude/skills"), "release");
    plain(&shop.join("web/.claude/skills"), "styles");
    plain(&shop.join("build/.claude/skills"), "ignored");
    write(&shop.join(".gitignore"), "build/\n");
    let shared = m.home().join("shared");
    plain(&shared.join(".claude/skills"), "glossary");
    m.places.added.push(AddedDir {
        space: "acme corp".into(),
        dir: shared,
    });

    let scan = m.scan();
    let release = one(&scan, "release");
    let claude: Vec<&api::SkillLoad> = release
        .skill
        .loads
        .iter()
        .filter(|l| l.cli == Cli::Claude)
        .collect();
    assert_eq!(claude.len(), 1);
    assert_eq!(
        (claude[0].scope, claude[0].project.as_deref()),
        (Scope::Project, Some("shop"))
    );
    assert!(
        release
            .skill
            .loads
            .iter()
            .any(|l| l.cli == Cli::Cursor && l.scope == Scope::Project),
        "Cursor reads a project's .claude/skills too"
    );
    assert_eq!(
        loads(one(&scan, "styles")),
        [(Cli::Claude, Scope::Nested, "styles".into())]
    );
    assert!(named(&scan, "ignored").is_empty(), "git ignores build/");
    let glossary = one(&scan, "glossary");
    assert_eq!(glossary.skill.loads[0].scope, Scope::AddedDir);
    assert_eq!(glossary.skill.loads[0].space.as_deref(), Some("acme corp"));
    assert!(
        glossary.finds[0].seen_in(Some(0), &m.places),
        "shop is in the space"
    );
    assert!(!glossary.finds[0].seen_in(None, &m.places));
}

/// The file the page shows is the folder's own SKILL.md: one that links
/// out of the folder is never read, and an id the scan has not found
/// names nothing.
#[test]
fn a_skill_md_linking_out_of_its_folder_is_refused() {
    let m = Machine::new();
    let secret = m.home().join("secret.txt");
    write(&secret, "---\nname: secret\n---\nDo not show.\n");
    let folder = m.home().join(".claude/skills/sneaky");
    fs::create_dir_all(&folder).unwrap();
    symlink(&secret, folder.join("SKILL.md")).unwrap();

    let scan = m.scan();
    let sneaky = one(&scan, "sneaky");
    assert_eq!(kinds(sneaky), [P::LinkOutside]);
    let detail = detail(sneaky);
    assert_eq!(detail.body, None);
    assert!(detail.unread.unwrap().contains("link out of its folder"));
    assert_eq!(detail.files.len(), 1);
    assert_eq!(
        detail.files[0].link.as_deref(),
        Some(secret.to_str().unwrap()),
        "listed, not followed"
    );
    for id in [
        "../../secret.txt",
        "%2e%2e%2fsecret.txt",
        "",
        &secret.display().to_string(),
    ] {
        assert!(scan.get(id).is_none(), "{id}");
    }
}

/// What a skill's own file gets wrong: a name other than its folder's, one
/// the spec does not allow, a description over its limit, frontmatter
/// that does not parse, and a field Codex needs.
#[test]
fn a_skill_says_what_its_frontmatter_gets_wrong() {
    let m = Machine::new();
    let skills = m.home().join(".agents/skills");
    skill(&skills, "tidy", "name: Tidy_Up\ndescription: Tidies.");
    skill(
        &skills,
        "wordy",
        &format!("name: wordy\ndescription: {}", "x".repeat(1025)),
    );
    skill(&skills, "broken", "name: [broken\ndescription: x");
    skill(&skills, "bare", "name: bare");
    write(&skills.join("loose/SKILL.md"), "No frontmatter at all.\n");

    let scan = m.scan();
    let tidy = one(&scan, "Tidy_Up");
    assert_eq!(kinds(tidy), [P::NameMismatch, P::BadName]);
    assert_eq!(
        texts(tidy, P::NameMismatch),
        ["The frontmatter names it `Tidy_Up`, but its folder is `tidy`."]
    );
    assert_eq!(kinds(one(&scan, "wordy")), [P::LongDescription]);
    let broken = one(&scan, "broken");
    assert_eq!(kinds(broken), [P::BadFrontmatter]);
    assert!(texts(broken, P::BadFrontmatter)[0].starts_with("The frontmatter is not valid YAML"));
    assert!(texts(broken, P::BadFrontmatter)[0].ends_with('.'));
    assert_eq!(
        texts(one(&scan, "bare"), P::MissingField),
        ["Codex needs a `name` and a `description` in the frontmatter, and this one has no `description`."]
    );
    let loose = one(&scan, "loose");
    assert_eq!(kinds(loose), [P::MissingField]);
    assert_eq!(
        loose.skill.description.as_deref(),
        Some("No frontmatter at all."),
        "the first line, as Claude takes it"
    );
}

/// A skill of the user's own that one CLI alone reads says so; one every
/// installed CLI can see does not.
#[test]
fn a_skill_only_one_cli_loads_says_so() {
    let m = Machine::new();
    plain(&m.home().join(".codex/skills"), "figma");
    plain(&m.home().join(".cursor/skills"), "cursor-only");
    plain(&m.home().join(".claude/skills"), "shared");
    write(&m.home().join(".claude/commands/review.md"), "Review it.\n");

    let scan = m.scan();
    assert!(
        !kinds(one(&scan, "figma")).contains(&P::OneCli),
        "Cursor reads ~/.codex/skills"
    );
    assert_eq!(
        texts(one(&scan, "cursor-only"), P::OneCli),
        ["Only Cursor loads it."]
    );
    assert!(!kinds(one(&scan, "shared")).contains(&P::OneCli));
    let review = one(&scan, "review");
    assert_eq!(review.skill.kind, api::SkillKind::Command);
    assert!(
        review.skill.problems.is_empty(),
        "a command is Claude's alone by its nature"
    );

    fs::remove_dir_all(m.home().join(".cursor")).unwrap();
    fs::remove_dir_all(m.home().join(".codex")).unwrap();
    let scan = m.scan();
    assert!(
        scan.found.iter().all(|f| !kinds(f).contains(&P::OneCli)),
        "with one CLI installed, nothing can be seen by another"
    );
}

/// The skills installer's lock file names the source of what it installed.
#[test]
fn the_installer_lock_names_the_source() {
    let m = Machine::new();
    let installed = plain(&m.home().join(".agents/skills"), "watch");
    link(&m.home().join(".claude/skills/watch"), &installed);
    write(
        &m.home().join(".agents/.skill-lock.json"),
        r#"{"version": 3, "skills": {"watch": {"source": "acme/video", "sourceType": "github",
            "sourceUrl": "https://github.com/acme/video.git", "skillPath": "skills/watch/SKILL.md"}}}"#,
    );
    let scan = m.scan();
    assert_eq!(
        one(&scan, "watch").skill.source,
        api::SkillSource::Installer {
            source: "acme/video".into(),
            url: Some("https://github.com/acme/video.git".into()),
            path: Some("skills/watch/SKILL.md".into()),
        }
    );
}

/// Skills synced from claude.ai are Claude's under its own name for them,
/// and their folder is no skill of the user's.
#[test]
fn synced_skills_are_named_apart() {
    let m = Machine::new();
    let synced = m.home().join(".claude/skills/synced");
    write(&synced.join(".bucket-1"), "");
    plain(&synced.join("bucket-1"), "docs");
    plain(&m.home().join(".claude/skills"), "docs");

    let scan = m.scan();
    let docs = named(&scan, "docs");
    assert_eq!(docs.len(), 2);
    let account = docs
        .iter()
        .find(|f| f.skill.source == api::SkillSource::ClaudeAi)
        .unwrap();
    assert_eq!(
        loads(account),
        [(Cli::Claude, Scope::ClaudeAi, "anthropic-skills:docs".into())]
    );
    assert!(
        docs.iter().all(|f| texts(f, P::DuplicateName).is_empty()),
        "named apart"
    );
    assert!(named(&scan, "synced").is_empty());
    assert!(named(&scan, "bucket-1").is_empty());
}

/// A command's name is its path under `commands/`, `:` for each folder.
#[test]
fn a_command_is_named_by_its_path() {
    let m = Machine::new();
    let commands = m.home().join(".claude/commands");
    write(
        &commands.join("train/log.md"),
        "---\ndescription: Log a session.\n---\nLog it.\n",
    );
    let scan = m.scan();
    let log = one(&scan, "train:log");
    assert_eq!(log.skill.description.as_deref(), Some("Log a session."));
    let detail = detail(log);
    assert_eq!(detail.body.as_deref(), Some("Log it.\n"));
    assert_eq!(detail.files.len(), 1);
}

/// The detail reads the file fresh: its frontmatter field by field in the
/// file's order, its body after it, and the files in its folder.
#[test]
fn the_detail_reads_the_file_in_order() {
    let m = Machine::new();
    let folder = skill(
        &m.home().join(".claude/skills"),
        "deploy",
        "name: deploy\ndescription: Ships it.\nallowed-tools:\n  - Bash(git *)\nmetadata:\n  owner: ops\ndisable-model-invocation: true",
    );
    write(&folder.join("scripts/run.sh"), "echo hi\n");
    link(&folder.join("shared"), &m.home().join("elsewhere"));

    let scan = m.scan();
    let detail = detail(one(&scan, "deploy"));
    let keys: Vec<&str> = detail.frontmatter.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "name",
            "description",
            "allowed-tools",
            "metadata",
            "disable-model-invocation"
        ]
    );
    assert_eq!(
        detail.frontmatter[2].value,
        serde_json::json!(["Bash(git *)"])
    );
    assert_eq!(
        detail.frontmatter[3].value,
        serde_json::json!({"owner": "ops"})
    );
    assert_eq!(detail.frontmatter[4].value, serde_json::json!(true));
    assert_eq!(
        detail.body.as_deref(),
        Some("\n# deploy\n\nWhat it does.\n")
    );
    let files: Vec<(&str, Option<&str>)> = detail
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.link.as_deref()))
        .collect();
    let elsewhere = m.home().join("elsewhere").display().to_string();
    assert_eq!(
        files,
        [
            ("SKILL.md", None),
            ("scripts/run.sh", None),
            ("shared", Some(elsewhere.as_str()))
        ]
    );
}

#[test]
fn frontmatter_is_read_only_from_the_first_line() {
    let (fields, body) = frontmatter("# Title\n---\nname: x\n---\n");
    assert_eq!(fields.unwrap(), []);
    assert_eq!(body, "# Title\n---\nname: x\n---\n");
    let (fields, _) = frontmatter("---\nname: x\n");
    assert_eq!(
        fields.unwrap_err(),
        "The frontmatter has no closing `---` line."
    );
    let (fields, body) = frontmatter("\u{feff}---\r\nname: x\r\n---\r\nBody\r\n");
    assert_eq!(
        fields.unwrap(),
        [("name".to_string(), serde_json::json!("x"))]
    );
    assert_eq!(body, "Body\r\n");
    let (fields, _) = frontmatter("---\n- a\n- b\n---\n");
    assert!(fields.is_err(), "a list is not fields");
}

#[test]
fn a_name_follows_the_agent_skills_spec() {
    for ok in ["pdf", "skill-creator", "v2-notes"] {
        assert!(valid_name(ok), "{ok}");
    }
    for bad in ["Pdf", "-x", "x-", "a--b", "a_b", "a b", &"x".repeat(65)] {
        assert!(!valid_name(bad), "{bad}");
    }
}

/// The broken links in one folder are one fix, said in the project's own
/// terms when the folder is a project's; a lone one keeps its own words.
#[test]
fn broken_links_in_one_folder_are_one_fix() {
    let mut m = Machine::new();
    let shop = m.project("shop");
    for name in ["vendor-auth", "vendor-cache", "vendor-search"] {
        link(
            &shop.join(".claude/skills").join(name),
            &shop.join(".agents/skills").join(name),
        );
    }
    plain(&shop.join(".claude/skills"), "release");
    link(
        &m.home().join(".claude/skills/old"),
        &m.home().join("src/gone/skills/old"),
    );

    let scan = m.scan();
    let fixes = fixes(&scan);
    assert_eq!(fixes.len(), 2, "{fixes:#?}");
    let vendor = &fixes[1];
    assert_eq!(vendor["kind"], "brokenLink");
    assert_eq!(vendor["title"], "3 broken links");
    assert_eq!(vendor["place"], "in shop");
    assert_eq!(
        vendor["text"],
        "3 links in .claude/skills point into .agents/skills, which is not there."
    );
    assert_eq!(
        vendor["fix"],
        "Remove the links, or put back .agents/skills."
    );
    let ids: Vec<&str> = ["vendor-auth", "vendor-cache", "vendor-search"]
        .iter()
        .map(|n| one(&scan, n).skill.id.as_str())
        .collect();
    assert_eq!(vendor["skills"], serde_json::json!(ids));
    let old = &fixes[0];
    assert_eq!(
        (&old["title"], &old["place"]),
        (&"1 broken link".into(), &"in ~/.claude/skills".into())
    );
    assert_eq!(
        old["text"],
        "The link points at ~/src/gone/skills/old, which is not there."
    );
}

/// Skills one name is found under are one fix, however many CLIs report
/// them and from whichever side, saying what each CLI does; the load Claude
/// passes over names the one it uses instead.
#[test]
fn a_name_found_twice_is_one_fix_saying_what_each_cli_does() {
    let mut m = Machine::new();
    let shop = m.project("shop");
    let personal = plain(&m.home().join(".claude/skills"), "deploy");
    let project = plain(&shop.join(".claude/skills"), "deploy");
    plain(&m.home().join(".agents/skills"), "skill-creator");
    plain(&m.home().join(".codex/skills/.system"), "skill-creator");

    let scan = m.scan();
    let fixes = fixes(&scan);
    assert_eq!(fixes.len(), 2, "{fixes:#?}");
    let deploy = &fixes[0];
    assert_eq!(deploy["title"], "deploy twice");
    assert_eq!(deploy["place"], "in Claude and Cursor, in shop");
    assert_eq!(
        deploy["text"],
        "Claude uses ~/.claude/skills/deploy over .claude/skills/deploy. Cursor finds both and \
         documents no order between them."
    );
    assert_eq!(
        deploy["fix"],
        "Rename one of them, or remove the one you do not use."
    );
    let at = |path: &Path| {
        scan.found
            .iter()
            .find(|f| f.skill.path == path.display().to_string())
            .unwrap()
    };
    let (personal, project) = (at(&personal), at(&project));
    assert_eq!(
        deploy["skills"],
        serde_json::json!([personal.skill.id, project.skill.id])
    );
    let shadowed: Vec<(Cli, Option<&str>)> = project
        .skill
        .loads
        .iter()
        .map(|l| (l.cli, l.shadowed_by.as_deref()))
        .collect();
    assert_eq!(
        shadowed,
        [
            (Cli::Claude, Some(personal.skill.id.as_str())),
            (Cli::Cursor, None)
        ],
        "Cursor documents no order, so it passes over neither"
    );
    assert!(personal.skill.loads.iter().all(|l| l.shadowed_by.is_none()));

    let creator = &fixes[1];
    assert_eq!(
        (&creator["title"], &creator["place"]),
        (&"skill-creator twice".into(), &"in Codex".into())
    );
    assert_eq!(
        creator["text"],
        "Codex lists ~/.agents/skills/skill-creator and the built-in one."
    );
    assert_eq!(
        creator["fix"],
        "Rename yours, or switch the built-in one off in Codex's settings."
    );
}

/// A command one of a project's skills hides makes the fix about the
/// command, and the CLIs that find a different set of the three name them.
#[test]
fn a_command_under_a_skills_name_is_in_the_same_fix() {
    let mut m = Machine::new();
    let shop = m.project("shop");
    plain(&m.home().join(".claude/skills"), "deploy");
    plain(&shop.join(".claude/skills"), "deploy");
    write(
        &shop.join(".claude/commands/deploy.md"),
        "Deploy the shop.\n",
    );

    let fixes = fixes(&m.scan());
    assert_eq!(fixes.len(), 1, "{fixes:#?}");
    assert_eq!(fixes[0]["title"], "deploy three times");
    assert_eq!(
        fixes[0]["text"],
        "Claude uses ~/.claude/skills/deploy over .claude/skills/deploy and the command \
         .claude/commands/deploy.md. Cursor finds ~/.claude/skills/deploy and \
         .claude/skills/deploy and documents no order between them."
    );
    assert_eq!(
        fixes[0]["fix"],
        "Remove the old command, or rename one of them."
    );
}

/// What one skill's own file gets wrong is one fix for that skill, and the
/// skills one CLI alone loads are one fix for that CLI.
#[test]
fn a_skills_own_file_and_one_cli_are_a_fix_each() {
    let m = Machine::new();
    skill(
        &m.home().join(".agents/skills"),
        "tidy",
        "name: Tidy_Up\ndescription: Tidies.",
    );
    plain(&m.home().join(".cursor/skills"), "cursor-only");
    plain(&m.home().join(".cursor/skills"), "cursor-too");

    let fixes = fixes(&m.scan());
    assert_eq!(fixes.len(), 2, "{fixes:#?}");
    assert_eq!(
        (&fixes[0]["kind"], &fixes[0]["title"], &fixes[0]["place"]),
        (
            &"nameMismatch".into(),
            &"Tidy_Up".into(),
            &"in ~/.agents/skills".into()
        )
    );
    assert_eq!(
        fixes[0]["fix"],
        "Give the frontmatter and the folder one name. Rename it in lowercase letters, digits \
         and single hyphens."
    );
    assert_eq!(fixes[1]["title"], "2 skills");
    assert_eq!(fixes[1]["place"], "for Cursor only");
    assert_eq!(
        fixes[1]["text"],
        "Claude and Codex do not find them where they look."
    );
    assert_eq!(
        fixes[1]["fix"],
        "Link them into a folder Claude and Codex read, if they should load them too."
    );
}

/// 2026-11-11 at noon, UTC: the time the use tests read at.
fn noon() -> std::time::SystemTime {
    let days = nebula_core::clock::days_from_civil(2026, 11, 11) as u64;
    std::time::UNIX_EPOCH + Duration::from_secs(days * 86_400 + 12 * 3600)
}

/// A transcript at `~/.claude/projects/-home-code/<file>` of `lines`.
fn transcript(m: &Machine, file: &str, lines: &[String]) -> PathBuf {
    let path = m.home().join(".claude/projects/-home-code").join(file);
    write(&path, &(lines.join("\n") + "\n"));
    path
}

/// An assistant's `Skill` call, as Claude Code writes the line.
fn call(id: &str, skill: &str, at: &str, cwd: &Path) -> String {
    format!(
        r#"{{"parentUuid":null,"isSidechain":false,"type":"assistant","uuid":"u-{id}","timestamp":"{at}","cwd":"{}","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"{id}","name":"Skill","input":{{"skill":"{skill}","args":"now"}}}}]}}}}"#,
        cwd.display()
    )
}

/// A user's line that ran the slash command `/name`.
fn command(uuid: &str, name: &str, at: &str, cwd: &Path) -> String {
    format!(
        r#"{{"parentUuid":null,"isSidechain":false,"type":"user","uuid":"{uuid}","timestamp":"{at}","cwd":"{}","message":{{"role":"user","content":"<command-message>{name}</command-message>\n<command-name>/{name}</command-name>"}}}}"#,
        cwd.display()
    )
}

/// Each skill used, by its name: how often, when last and in which project.
fn used(scan: &Scan, read: &mut usage::Transcripts) -> Vec<(String, u32, i64, Option<String>)> {
    usage::usage(scan, read, noon())
        .used
        .into_iter()
        .map(|u| {
            let name = scan.get(&u.id).unwrap().skill.name.clone();
            (name, u.count, u.last, u.project)
        })
        .collect()
}

fn at(iso: &str) -> i64 {
    usage::epoch_ms(iso).unwrap()
}

/// A use is a `Skill` call, in a session or its subagent, or a slash
/// command, counted when Claude loads a skill under its name: a plugin's
/// with its prefix, and never a built-in command or Claude Code's own
/// lines. A call a resumed session repeats counts once, and a session in a
/// worktree is in its project.
#[test]
fn uses_are_skill_calls_and_slash_commands_named_as_claude_loads_skills() {
    let mut m = Machine::new();
    let skills = m.home().join(".claude/skills");
    plain(&skills, "deploy");
    plain(&skills, "notes");
    let shop = m.project("shop");
    plain(&shop.join(".claude/skills"), "translate");
    let fix = m.home().join("code/shop-worktrees/fix");
    m.places.projects[0].worktrees.push(fix.clone());
    let plugins = m.home().join(".claude/plugins");
    let cached = plugins.join("cache/market/kit/1.0.0");
    write(
        &cached.join(".claude-plugin/plugin.json"),
        r#"{"name": "kit"}"#,
    );
    plain(&cached.join("skills"), "lint");
    write(
        &plugins.join("installed_plugins.json"),
        &serde_json::json!({"plugins": {"kit@market": [{"installPath": cached, "version": "1.0.0"}]}})
            .to_string(),
    );
    let elsewhere = m.home().join("notes");
    let meta = r#"{"type":"user","isMeta":true,"uuid":"m1","timestamp":"2026-11-10T08:00:00.000Z","cwd":"/","message":{"role":"user","content":"<command-name>/notes</command-name>"}}"#;
    transcript(
        &m,
        "s1.jsonl",
        &[
            call("t1", "deploy", "2026-11-09T10:00:00.000Z", &shop),
            call("t2", "kit:lint", "2026-11-10T10:00:00.000Z", &shop),
            command("c1", "translate", "2026-11-10T08:00:00.000Z", &shop),
            command("c2", "model", "2026-11-10T08:30:00.000Z", &shop),
            call("t3", "simplify", "2026-11-10T09:00:00.000Z", &shop),
            meta.to_string(),
            r#"{"type":"assistant","name":"Skill","cut off"#.to_string(),
        ],
    );
    transcript(
        &m,
        "s2.jsonl",
        &[
            call("t1", "deploy", "2026-11-09T10:00:00.000Z", &shop),
            command(
                "c3",
                "translate",
                "2026-11-11T09:00:00.000Z",
                &fix.join("src"),
            ),
        ],
    );
    transcript(
        &m,
        "s2/subagents/agent-a1.jsonl",
        &[call("t4", "deploy", "2026-11-11T10:00:00.000Z", &elsewhere)],
    );

    let scan = m.scan();
    assert_eq!(
        used(&scan, &mut usage::Transcripts::default()),
        [
            ("deploy".into(), 2, at("2026-11-11T10:00:00Z"), None),
            (
                "translate".into(),
                2,
                at("2026-11-11T09:00:00Z"),
                Some("shop".into())
            ),
            (
                "lint".into(),
                1,
                at("2026-11-10T10:00:00Z"),
                Some("shop".into())
            ),
        ]
    );
}

/// Only the last 30 days count, and a transcript last written before them
/// is not opened at all.
#[test]
fn only_the_window_counts_and_an_older_transcript_is_not_opened() {
    let m = Machine::new();
    plain(&m.home().join(".claude/skills"), "deploy");
    let cwd = m.home().join("code");
    transcript(
        &m,
        "new.jsonl",
        &[
            call("t1", "deploy", "2026-10-01T10:00:00.000Z", &cwd),
            call("t2", "deploy", "2026-10-20T10:00:00.000Z", &cwd),
        ],
    );
    let old = transcript(
        &m,
        "old.jsonl",
        &[call("t3", "deploy", "2026-11-10T10:00:00.000Z", &cwd)],
    );
    let october = noon() - Duration::from_secs(40 * 86_400);
    fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(october)
        .unwrap();

    let scan = m.scan();
    let got = used(&scan, &mut usage::Transcripts::default());
    assert_eq!(
        got,
        [("deploy".into(), 1, at("2026-10-20T10:00:00Z"), None)]
    );
}

/// A second read opens only the transcripts whose size or time changed,
/// and forgets one that is gone.
#[test]
fn a_second_read_opens_only_the_transcripts_that_changed() {
    let m = Machine::new();
    let skills = m.home().join(".claude/skills");
    plain(&skills, "deploy");
    plain(&skills, "notes");
    let cwd = m.home().join("code");
    let a = transcript(
        &m,
        "a.jsonl",
        &[call("t1", "deploy", "2026-11-10T10:00:00.000Z", &cwd)],
    );
    let b = transcript(
        &m,
        "b.jsonl",
        &[call("t2", "notes", "2026-11-10T10:00:00.000Z", &cwd)],
    );
    let scan = m.scan();
    let mut read = usage::Transcripts::default();
    let counts = |read: &mut usage::Transcripts| -> BTreeMap<String, u32> {
        used(&scan, read)
            .into_iter()
            .map(|(n, c, ..)| (n, c))
            .collect()
    };
    let of = |pairs: &[(&str, u32)]| -> BTreeMap<String, u32> {
        pairs.iter().map(|(n, c)| (n.to_string(), *c)).collect()
    };
    assert_eq!(counts(&mut read), of(&[("deploy", 1), ("notes", 1)]));

    let mut more = fs::OpenOptions::new().append(true).open(&a).unwrap();
    use std::io::Write;
    writeln!(
        more,
        "{}",
        call("t3", "deploy", "2026-11-11T10:00:00.000Z", &cwd)
    )
    .unwrap();
    // Opened again, this one would name a skill there is none of.
    let modified = fs::metadata(&b).unwrap().modified().unwrap();
    let text = fs::read_to_string(&b).unwrap().replace("notes", "nodes");
    fs::write(&b, text).unwrap();
    fs::File::options()
        .write(true)
        .open(&b)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(counts(&mut read), of(&[("deploy", 2), ("notes", 1)]));

    fs::remove_file(&a).unwrap();
    assert_eq!(counts(&mut read), of(&[("notes", 1)]));
}

#[test]
fn a_transcript_time_is_read_to_the_second() {
    assert_eq!(
        usage::epoch_ms("1970-01-02T00:00:01.999Z"),
        Some(86_401_000)
    );
    assert_eq!(usage::epoch_ms("2026-11-11"), None);
    assert_eq!(usage::epoch_ms(""), None);
}
