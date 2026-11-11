//! What the page lists to fix: one row for each cause, however many
//! problems it gives. The broken links in one folder are one fix, and so
//! are the skills one name is found under, each told about the others.

use super::*;
use api::SkillProblemKind as P;

/// The problems a skill's own file has, which it fixes on its own.
const OWN: [P; 7] = [
    P::LinkOutside,
    P::Unreadable,
    P::BadFrontmatter,
    P::NameMismatch,
    P::BadName,
    P::LongDescription,
    P::MissingField,
];

/// Every fix for the problems `found` has: broken links by the folder
/// they are in, then names found twice, then each skill's own file, then
/// skills one CLI alone loads, by that CLI.
pub(super) fn fixes(
    found: &[Found],
    dups: &[Dup],
    places: &Places,
    clis: &[Cli],
) -> Vec<api::SkillFix> {
    let mut out = broken_links(found, places);
    out.extend(same_names(found, dups, places));
    out.extend(own_files(found, places));
    out.extend(one_cli(found, clis));
    out
}

/// `n` and `one`, made plural unless `n` is 1.
fn count(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// `items` as a list in a sentence: `a`, `a and b`, `a, b and c`.
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// `path` as a sentence names it: under the project's root when it is in
/// `project`, else under `~`.
fn shown(path: &Path, project: Option<usize>, places: &Places) -> String {
    project
        .and_then(|p| path.strip_prefix(&places.projects[p].root).ok())
        .filter(|rest| !rest.as_os_str().is_empty())
        .map_or_else(
            || tilde(path, &places.home),
            |rest| rest.display().to_string(),
        )
}

/// The one project every place a CLI finds `skill` is in, if there is one.
fn project_of(skill: &Found) -> Option<usize> {
    let first = skill.finds.first()?.project?;
    skill
        .finds
        .iter()
        .all(|f| f.project == Some(first))
        .then_some(first)
}

/// Where a skill is, for a fix about it: its project, or the folder it is in.
fn place_of(skill: &Found, places: &Places) -> String {
    match project_of(skill) {
        Some(p) => format!("in {}", places.projects[p].name),
        None => {
            let path = Path::new(&skill.skill.path);
            format!("in {}", tilde(path.parent().unwrap_or(path), &places.home))
        }
    }
}

fn ids<'a>(skills: impl IntoIterator<Item = &'a Found>) -> Vec<String> {
    skills.into_iter().map(|f| f.skill.id.clone()).collect()
}

fn broken_links(found: &[Found], places: &Places) -> Vec<api::SkillFix> {
    let mut by_folder: BTreeMap<PathBuf, Vec<&Found>> = BTreeMap::new();
    for f in found.iter().filter(|f| f.real.is_none()) {
        let link = Path::new(&f.skill.path);
        let folder = link.parent().unwrap_or(link).to_path_buf();
        by_folder.entry(folder).or_default().push(f);
    }
    by_folder
        .into_iter()
        .map(|(folder, links)| {
            // One folder's links are one project's, or none's.
            let project = project_of(links[0]);
            let show = |p: &Path| shown(p, project, places);
            let into: BTreeSet<PathBuf> = links
                .iter()
                .filter_map(|f| fs::read_link(&f.skill.path).ok())
                .filter_map(|t| lexical(&folder, &t).parent().map(Path::to_path_buf))
                .collect();
            let n = links.len();
            let (text, fix) = match (n, into.first()) {
                (1, _) => (
                    links[0]
                        .skill
                        .problems
                        .iter()
                        .find(|p| p.kind == P::BrokenLink)
                        .map(|p| p.text.clone())
                        .unwrap_or_default(),
                    "Remove the link, or put back what it points at.".to_string(),
                ),
                (_, Some(dir)) if into.len() == 1 => {
                    let what = if dir.exists() {
                        "which holds none of them"
                    } else {
                        "which is not there"
                    };
                    (
                        format!(
                            "{n} links in {} point into {}, {what}.",
                            show(&folder),
                            show(dir)
                        ),
                        format!("Remove the links, or put back {}.", show(dir)),
                    )
                }
                _ => (
                    format!(
                        "{n} links in {} point at folders that are not there.",
                        show(&folder)
                    ),
                    "Remove the links, or put back what they point at.".to_string(),
                ),
            };
            api::SkillFix {
                kind: P::BrokenLink,
                title: count(n, "broken link"),
                place: place_of(links[0], places),
                text,
                fix,
                skills: ids(links),
            }
        })
        .collect()
}

/// One fix for each name skills are found under, whichever CLIs and
/// contexts find them, with what each CLI does about it: the sets of
/// skills under one name that share one are one fix.
fn same_names(found: &[Found], dups: &[Dup], places: &Places) -> Vec<api::SkillFix> {
    let skills_of = |dup: &Dup| {
        let mut skills: Vec<usize> = dup.members.iter().map(|(s, _)| *s).collect();
        skills.sort_unstable();
        skills.dedup();
        skills
    };
    let mut merged: Vec<(&str, Vec<usize>, Vec<&Dup>)> = Vec::new();
    for dup in dups {
        let skills = skills_of(dup);
        match merged
            .iter_mut()
            .find(|(name, s, _)| *name == dup.name && s.iter().any(|x| skills.contains(x)))
        {
            Some((_, s, of)) => {
                s.extend(skills);
                s.sort_unstable();
                s.dedup();
                of.push(dup);
            }
            None => merged.push((&dup.name, skills, vec![dup])),
        }
    }
    merged
        .into_iter()
        .map(|(name, skills, dups)| {
            let finds = || {
                dups.iter()
                    .flat_map(|d| d.members.iter())
                    .map(|&(s, f)| (s, &found[s].finds[f]))
            };
            let describe = |find: &Find| {
                let path = shown(&find.path, find.project, places);
                match (find.scope, find.command) {
                    (Scope::System, _) => "the built-in one".to_string(),
                    (_, true) => format!("the command {path}"),
                    _ => path,
                }
            };
            // In Claude's order: a skill before a command, personal before
            // a project's.
            let named = |dup: &Dup, skip: Option<usize>| {
                let mut members: Vec<&Find> = dup
                    .members
                    .iter()
                    .filter(|(s, _)| Some(*s) != skip)
                    .map(|&(s, f)| &found[s].finds[f])
                    .collect();
                members.sort_by_key(|f| claude_rank(f));
                let all: Vec<String> = members.into_iter().map(describe).collect();
                list(&all)
            };
            let mut clis: Vec<Cli> = Vec::new();
            let mut said: Vec<String> = Vec::new();
            for dup in &dups {
                if clis.contains(&dup.cli) {
                    continue;
                }
                clis.push(dup.cli);
                // Once the skills are named, the next CLI's sentence says
                // "both" of the same ones.
                let them = match skills_of(dup).len() {
                    _ if said.is_empty() || skills_of(dup) != skills => named(dup, None),
                    2 => "both".to_string(),
                    n => format!("all {n}"),
                };
                said.push(match (dup.cli, dup.winner) {
                    (Cli::Claude, Some(w)) => {
                        let &(_, f) = dup
                            .members
                            .iter()
                            .find(|(s, _)| *s == w)
                            .expect("the winner is a member");
                        let win = describe(&found[w].finds[f]);
                        format!("Claude uses {win} over {}.", named(dup, Some(w)))
                    }
                    (Cli::Codex, _) => format!("Codex lists {them}."),
                    (cli, _) => format!(
                        "{} finds {them} and documents no order between them.",
                        cli.title()
                    ),
                });
            }
            let titles: Vec<String> = clis.iter().map(|c| c.title().to_string()).collect();
            let mut place = format!("in {}", list(&titles));
            if let Some(p) = finds().find_map(|(_, f)| f.project) {
                place.push_str(&format!(", in {}", places.projects[p].name));
            } else if let Some(space) = finds().find_map(|(_, f)| f.space.clone()) {
                place.push_str(&format!(", in space {space}"));
            }
            let bundled = finds()
                .find(|(_, f)| f.scope == Scope::System)
                .map(|(_, f)| f.cli);
            let fix = if finds().any(|(_, f)| f.command) {
                "Remove the old command, or rename one of them.".to_string()
            } else {
                match bundled {
                    Some(Cli::Cursor) => {
                        "Rename yours: Cursor has no switch for its built-in one.".to_string()
                    }
                    Some(cli) => format!(
                        "Rename yours, or switch the built-in one off in {}'s settings.",
                        cli.title()
                    ),
                    None => "Rename one of them, or remove the one you do not use.".to_string(),
                }
            };
            let times = match skills.len() {
                2 => "twice".to_string(),
                3 => "three times".to_string(),
                n => format!("{n} times"),
            };
            api::SkillFix {
                kind: P::DuplicateName,
                title: format!("{name} {times}"),
                place,
                text: said.join(" "),
                fix,
                skills: ids(skills.iter().map(|&s| &found[s])),
            }
        })
        .collect()
}

/// What a skill's own file gets wrong, as one fix for each skill.
fn own_files(found: &[Found], places: &Places) -> Vec<api::SkillFix> {
    found
        .iter()
        .filter_map(|f| {
            let problems: Vec<&api::SkillProblem> = f
                .skill
                .problems
                .iter()
                .filter(|p| OWN.contains(&p.kind))
                .collect();
            let first = problems.first()?;
            let mut fixes: Vec<&str> = Vec::new();
            for p in &problems {
                let fix = match p.kind {
                    P::LinkOutside => "Make its SKILL.md a file in its own folder.",
                    P::Unreadable => "Make the file readable UTF-8 text.",
                    P::BadFrontmatter => "Fix the YAML between its `---` lines.",
                    P::NameMismatch => "Give the frontmatter and the folder one name.",
                    P::BadName => "Rename it in lowercase letters, digits and single hyphens.",
                    P::LongDescription => "Cut the description to 1,024 characters.",
                    _ => "Add what is missing to the frontmatter.",
                };
                if !fixes.contains(&fix) {
                    fixes.push(fix);
                }
            }
            let texts: Vec<&str> = problems.iter().map(|p| p.text.as_str()).collect();
            Some(api::SkillFix {
                kind: first.kind,
                title: f.skill.name.clone(),
                place: place_of(f, places),
                text: texts.join(" "),
                fix: fixes.join(" "),
                skills: ids([f]),
            })
        })
        .collect()
}

/// The skills of the user's own one CLI alone loads, as one fix for each
/// CLI.
fn one_cli(found: &[Found], clis: &[Cli]) -> Vec<api::SkillFix> {
    let mut out = Vec::new();
    for &cli in clis {
        let only: Vec<&Found> = found
            .iter()
            .filter(|f| {
                f.skill.problems.iter().any(|p| p.kind == P::OneCli)
                    && f.finds.first().is_some_and(|find| find.cli == cli)
            })
            .collect();
        let Some(first) = only.first() else { continue };
        let others: Vec<String> = clis
            .iter()
            .filter(|c| **c != cli)
            .map(|c| c.title().to_string())
            .collect();
        let them = if only.len() == 1 { "it" } else { "them" };
        let (they, verb) = if others.len() == 1 {
            ("it", "s")
        } else {
            ("they", "")
        };
        let not = if others.len() == 1 { "does" } else { "do" };
        let others = list(&others);
        out.push(api::SkillFix {
            kind: P::OneCli,
            title: if only.len() == 1 {
                first.skill.name.clone()
            } else {
                count(only.len(), "skill")
            },
            place: format!("for {} only", cli.title()),
            text: format!("{others} {not} not find {them} where {they} look{verb}."),
            fix: format!(
                "Link {them} into a folder {others} read{verb}, if {they} should load {them} too."
            ),
            skills: ids(only),
        });
    }
    out
}
