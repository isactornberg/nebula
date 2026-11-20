//! A REFRESH: what `nebula serve` reads of Linear and GitHub, and when. It
//! reads in two cases only, both the page's asking: when the page opens
//! and nothing has been read yet, and when its Refresh button is pressed.
//! Never on a timer. One refresh asks Linear, in one paged query, for the
//! open issues assigned to you in every team the projects' `issues`
//! settings name, and `gh`, in one query per project, for the pull request
//! on each checkout a session works in, all at once. A part that fails
//! keeps what the refresh before read of it, and says why.
//!
//! The rest is the JOIN: which assigned issue a session works on, and the
//! lanes of those no session does.

use crate::api::IssueLaneName;
use crate::hub::Live;
use nebula_core::env::LINEAR_API_KEY;
use nebula_forge::linear::Assigned;
use nebula_forge::pull_request::{self, PullRequest};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch;

/// What the refreshes read, kept from one to the next, with what Start
/// changed of it since and keeps of its own ([`crate::start`]), and the
/// focus picks with what was said of them ([`crate::focus`]).
#[derive(Debug, Clone, Default)]
pub struct Fetched {
    /// A refresh is under way.
    pub running: bool,
    /// When the last one finished, epoch ms.
    pub at: Option<i64>,
    /// Why parts of the last one failed, a sentence each.
    pub problems: Vec<String>,
    /// The open issues assigned to you.
    pub issues: Vec<Assigned>,
    /// Each project's pull requests by checkout, by the project's root.
    pub prs: BTreeMap<PathBuf, HashMap<PathBuf, PullRequest>>,
    /// The issues Start handed to captains, and where it last sent each
    /// Linear project's.
    pub starts: crate::start::Starts,
    pub focus: crate::focus::Kept,
}

/// What one refresh reads.
#[derive(Debug, Clone, Default)]
pub struct Ask {
    /// The Linear teams the projects' `issues` settings name.
    pub teams: Vec<String>,
    /// The projects with a session in one of their checkouts.
    pub projects: Vec<ProjectAsk>,
}

#[derive(Debug, Clone)]
pub struct ProjectAsk {
    pub name: String,
    pub root: PathBuf,
    /// The checkouts its sessions work in.
    pub checkouts: Vec<PathBuf>,
}

impl Ask {
    /// What a refresh of the daemon's projects as `live` holds them reads.
    pub fn of(live: &Live) -> Ask {
        let roots = live.repos.iter().map(|r| r.root.as_path());
        Ask {
            teams: crate::api::linear_teams(roots).into_iter().collect(),
            projects: live
                .repos
                .iter()
                .filter_map(|r| {
                    let checkouts = live.checkouts.get(&r.root)?;
                    Some(ProjectAsk {
                        name: r.name.clone(),
                        root: r.root.clone(),
                        checkouts: checkouts.iter().cloned().collect(),
                    })
                })
                .collect(),
        }
    }
}

/// What one refresh got back: the issues, and each asked project's pull
/// requests in the order asked.
struct Got {
    issues: Result<Vec<Assigned>, String>,
    prs: Vec<Result<HashMap<PathBuf, PullRequest>, String>>,
}

/// Starts refreshes, one at a time, and publishes what each read.
#[derive(Clone)]
pub struct Refresher {
    fetched: Arc<watch::Sender<Arc<Fetched>>>,
}

impl Refresher {
    pub fn new() -> Refresher {
        let (fetched, _) = watch::channel(Arc::new(Fetched::default()));
        Refresher {
            fetched: Arc::new(fetched),
        }
    }

    /// What the refreshes read, now and each time one lands.
    pub fn subscribe(&self) -> watch::Receiver<Arc<Fetched>> {
        self.fetched.subscribe()
    }

    /// What the refreshes read now.
    pub fn fetched(&self) -> Arc<Fetched> {
        self.fetched.borrow().clone()
    }

    /// Change what is kept, and publish it.
    pub fn patch(&self, change: impl FnOnce(&mut Fetched)) {
        self.fetched.send_modify(|f| change(Arc::make_mut(f)));
    }

    /// Read what `ask` names, unless a refresh is under way, which this
    /// then joins: there is never a second.
    pub fn start(&self, ask: Ask) {
        let began = self.fetched.send_if_modified(|f| {
            if f.running {
                return false;
            }
            Arc::make_mut(f).running = true;
            true
        });
        if !began {
            return;
        }
        let fetched = self.fetched.clone();
        tokio::spawn(async move {
            let got = fetch(&ask).await;
            let at = nebula_core::clock::now_ms();
            fetched.send_modify(|f| *f = Arc::new(land(f, &ask, got, at)));
        });
    }
}

/// Linear and every project's `gh` at once.
async fn fetch(ask: &Ask) -> Got {
    let issues = async {
        let has_key = std::env::var(LINEAR_API_KEY).is_ok_and(|k| !k.trim().is_empty());
        if ask.teams.is_empty() {
            Ok(Vec::new())
        } else if !has_key {
            Err(format!(
                "{LINEAR_API_KEY} is not set in nebula serve's environment"
            ))
        } else {
            nebula_forge::linear::assigned(&ask.teams).await
        }
    };
    let prs = futures_util::future::join_all(
        ask.projects
            .iter()
            .map(|p| pull_request::checkout_prs(&p.root, &p.checkouts)),
    );
    let (issues, prs) = tokio::join!(issues, prs);
    Got { issues, prs }
}

/// What the refresh that asked `ask` and got `got` leaves, at `at`: each
/// part as read, or as `before` had it where it failed, with why. A
/// project no longer asked about is dropped.
fn land(before: &Fetched, ask: &Ask, got: Got, at: i64) -> Fetched {
    let mut problems = Vec::new();
    let issues = got.issues.unwrap_or_else(|why| {
        let said = if why.starts_with("Linear") || why.starts_with(LINEAR_API_KEY) {
            why
        } else {
            format!("Linear: {why}")
        };
        problems.push(crate::sentence(&said));
        before.issues.clone()
    });
    let mut prs = BTreeMap::new();
    // Each reason once, with the projects it held for, in the order asked.
    let mut failed: Vec<(String, Vec<&str>)> = Vec::new();
    for (project, found) in ask.projects.iter().zip(got.prs) {
        let found = found.or_else(|why| {
            match failed.iter_mut().find(|(said, _)| *said == why) {
                Some((_, names)) => names.push(&project.name),
                None => failed.push((why, vec![&project.name])),
            }
            before.prs.get(&project.root).cloned().ok_or(())
        });
        if let Ok(found) = found {
            prs.insert(project.root.clone(), found);
        }
    }
    for (why, names) in failed {
        problems.push(crate::sentence(&format!(
            "Pull requests in {}: {why}",
            projects_named(&names)
        )));
    }
    Fetched {
        running: false,
        at: Some(at),
        problems,
        issues,
        prs,
        starts: before.starts.clone(),
        focus: before.focus.clone(),
    }
}

/// Where pull requests failed to be read: the projects by name, `api`,
/// `api and web`, `api, web and docs`, and past three how many.
fn projects_named(names: &[&str]) -> String {
    match names {
        [one] => one.to_string(),
        [rest @ .., last] if names.len() <= 3 => format!("{} and {last}", rest.join(", ")),
        _ => format!("{} projects", names.len()),
    }
}

/// The assigned issue a session works on, of `issues`: the one its issue
/// URL names (`issue_key`) when it has one, else the one whose key is in
/// its checkout's `branch` as a whole word, as nebula cuts a worktree for
/// one (`acme-123-fix-login` is ACME-123's, `acme-1234-x` is not), the key
/// earliest in the branch first, else the one Linear suggests that branch
/// for (`branchName`).
pub fn joined<'a>(
    issues: &'a [Assigned],
    issue_key: Option<&str>,
    branch: &str,
) -> Option<&'a Assigned> {
    if let Some(key) = issue_key {
        return issues.iter().find(|i| i.key.eq_ignore_ascii_case(key));
    }
    let lower = branch.to_ascii_lowercase();
    issues
        .iter()
        .filter_map(|i| Some((key_at(&lower, &i.key.to_ascii_lowercase())?, i)))
        .min_by_key(|(at, _)| *at)
        .map(|(_, i)| i)
        .or_else(|| {
            issues
                .iter()
                .find(|i| !i.branch_name.is_empty() && i.branch_name == branch)
        })
}

/// Where `key` first stands in `branch` with no letter or digit right
/// before or after it.
fn key_at(branch: &str, key: &str) -> Option<usize> {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    branch.match_indices(key).map(|(at, _)| at).find(|&at| {
        !word(branch[..at].chars().next_back()) && !word(branch[at + key.len()..].chars().next())
    })
}

/// The lanes of the assigned issues no session works on (`joined` holds
/// the keys of those one does): started in Linear, then not started yet,
/// each by priority, urgent first and none last, then the latest changed
/// first. An empty lane is left out, and so is every other state.
pub fn lanes<'a>(
    issues: &'a [Assigned],
    joined: &HashSet<&str>,
) -> Vec<(IssueLaneName, Vec<&'a Assigned>)> {
    [
        (IssueLaneName::InFlight, "started"),
        (IssueLaneName::Todo, "unstarted"),
    ]
    .into_iter()
    .map(|(lane, state_type)| {
        let mut rows: Vec<&Assigned> = issues
            .iter()
            .filter(|i| i.state_type == state_type && !joined.contains(i.key.as_str()))
            .collect();
        rows.sort_by_key(|i| {
            let rank = if i.priority == 0 { u8::MAX } else { i.priority };
            (rank, Reverse(i.updated_at))
        });
        (lane, rows)
    })
    .filter(|(_, rows)| !rows.is_empty())
    .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn issue(key: &str, state_type: &str, priority: u8, updated_at: i64) -> Assigned {
        Assigned {
            key: key.into(),
            url: format!("https://linear.app/acme/issue/{key}"),
            title: format!("{key} title"),
            priority,
            state: state_type.into(),
            state_type: state_type.into(),
            assignee: "Ada".into(),
            branch_name: format!("ada/{}-work", key.to_ascii_lowercase()),
            updated_at,
            team: key.split('-').next().unwrap().into(),
            project: None,
        }
    }

    fn key<'a>(issues: &'a [Assigned], issue_key: Option<&str>, branch: &str) -> Option<&'a str> {
        joined(issues, issue_key, branch).map(|i| i.key.as_str())
    }

    /// A session's issue URL names its issue, without case and over
    /// anything its branch says, and one it names that is not assigned to
    /// you joins nothing.
    #[test]
    fn a_session_joins_the_issue_its_url_names_first() {
        let issues = [
            issue("ACME-7", "started", 2, 1),
            issue("ACME-8", "started", 2, 1),
        ];
        assert_eq!(key(&issues, Some("acme-8"), "acme-7-fix"), Some("ACME-8"));
        assert_eq!(key(&issues, Some("ACME-9"), "acme-7-fix"), None);
        assert_eq!(key(&issues, Some("#3"), "acme-7-fix"), None);
    }

    /// Else the key in its branch joins it, as a whole word and without
    /// case: ACME-12 is not in `acme-123`, nor ACME-123 in `acme-1234` or
    /// `xrel-123`. With two keys in a branch, the first wins.
    #[test]
    fn a_session_joins_the_issue_whose_key_is_in_its_branch() {
        let issues = [
            issue("ACME-12", "started", 2, 1),
            issue("ACME-123", "unstarted", 2, 1),
            issue("ACME-5", "started", 2, 1),
        ];
        assert_eq!(key(&issues, None, "acme-123-fix-login"), Some("ACME-123"));
        assert_eq!(key(&issues, None, "ada/ACME-123"), Some("ACME-123"));
        assert_eq!(key(&issues, None, "acme-12_fix"), Some("ACME-12"));
        assert_eq!(key(&issues, None, "acme-1234-other"), None);
        assert_eq!(key(&issues, None, "xrel-123-other"), None);
        assert_eq!(key(&issues, None, "acme-123x"), None);
        assert_eq!(key(&issues, None, "acme-5-then-acme-12"), Some("ACME-5"));
        assert_eq!(key(&issues, None, "main"), None);
    }

    /// Else the branch Linear suggests for an issue joins it, as Linear
    /// spells it, which matters once its format leaves the key out.
    #[test]
    fn a_session_joins_the_issue_linear_suggests_its_branch_for() {
        let mut keyless = issue("ACME-7", "started", 2, 1);
        keyless.branch_name = "ada/fix-login".into();
        let issues = [keyless];
        assert_eq!(key(&issues, None, "ada/fix-login"), Some("ACME-7"));
        assert_eq!(key(&issues, None, "ada/Fix-Login"), None);
        assert_eq!(key(&issues, None, ""), None);
    }

    /// The lanes hold what no session works on, started first, then not
    /// started, each urgent first and no priority last, then the latest
    /// changed first; a backlog issue is in neither.
    #[test]
    fn the_lanes_are_by_state_then_priority_then_change() {
        let issues = [
            issue("A-1", "started", 0, 50),
            issue("A-2", "started", 3, 10),
            issue("A-3", "started", 1, 5),
            issue("A-4", "started", 3, 20),
            issue("A-5", "unstarted", 4, 1),
            issue("A-6", "unstarted", 2, 1),
            issue("A-7", "backlog", 1, 1),
            issue("A-8", "started", 1, 99),
        ];
        let keys = |joined: &[&str]| -> Vec<(IssueLaneName, Vec<&str>)> {
            lanes(&issues, &joined.iter().copied().collect())
                .into_iter()
                .map(|(lane, rows)| (lane, rows.iter().map(|i| i.key.as_str()).collect()))
                .collect()
        };
        assert_eq!(
            keys(&["A-8"]),
            [
                (IssueLaneName::InFlight, vec!["A-3", "A-4", "A-2", "A-1"]),
                (IssueLaneName::Todo, vec!["A-6", "A-5"]),
            ]
        );
        assert_eq!(
            keys(&["A-1", "A-2", "A-3", "A-4", "A-8"]),
            [(IssueLaneName::Todo, vec!["A-6", "A-5"])]
        );
    }

    fn project(name: &str) -> ProjectAsk {
        ProjectAsk {
            name: name.into(),
            root: PathBuf::from(format!("/code/{name}")),
            checkouts: vec![PathBuf::from(format!("/code/{name}"))],
        }
    }

    /// Projects whose pull requests failed for one reason are named in
    /// one line, and past three counted, as gh missing fails them all.
    #[test]
    fn the_projects_one_reason_failed_are_named_and_past_three_counted() {
        assert_eq!(projects_named(&["api"]), "api");
        assert_eq!(projects_named(&["api", "web"]), "api and web");
        assert_eq!(projects_named(&["api", "web", "docs"]), "api, web and docs");
        assert_eq!(projects_named(&["a", "b", "c", "d"]), "4 projects");
    }

    /// A refresh takes each part it read, keeps what the one before read
    /// of a part that failed, says why in a sentence each, and drops a
    /// project no longer asked about.
    #[test]
    fn a_failed_part_keeps_what_was_read_before_and_says_why() {
        let pr = |number| PullRequest {
            number,
            url: format!("https://github.com/o/r/pull/{number}"),
            title: String::new(),
            state: "OPEN".into(),
            is_draft: false,
            head: "feat".into(),
            head_sha: String::new(),
            base: "main".into(),
            health: Default::default(),
            activity: Vec::new(),
        };
        let at = |name: &str, number| {
            HashMap::from([(PathBuf::from(format!("/code/{name}")), pr(number))])
        };
        let before = Fetched {
            issues: vec![issue("ACME-1", "started", 1, 1)],
            prs: BTreeMap::from([
                (PathBuf::from("/code/api"), at("api", 1)),
                (PathBuf::from("/code/gone"), at("gone", 2)),
            ]),
            ..Fetched::default()
        };
        let ask = Ask {
            teams: vec!["ACME".into()],
            projects: vec![
                project("api"),
                project("web"),
                project("new"),
                project("docs"),
            ],
        };
        let got = Got {
            issues: Err("Linear: Authentication required".into()),
            prs: vec![
                Err("gh timed out".into()),
                Ok(at("web", 3)),
                Err("HTTP 401".into()),
                Err("gh timed out".into()),
            ],
        };
        let after = land(&before, &ask, got, 42);
        assert_eq!(after.at, Some(42));
        assert!(!after.running);
        assert_eq!(after.issues, before.issues);
        assert_eq!(
            after.problems,
            [
                "Linear: Authentication required.",
                "Pull requests in api and docs: gh timed out.",
                "Pull requests in new: HTTP 401.",
            ]
        );
        let numbers: Vec<(&str, u64)> = after
            .prs
            .iter()
            .flat_map(|(root, prs)| {
                prs.values()
                    .map(move |p| (root.to_str().unwrap(), p.number))
            })
            .collect();
        assert_eq!(numbers, [("/code/api", 1), ("/code/web", 3)]);

        let ok = Got {
            issues: Ok(Vec::new()),
            prs: vec![Ok(HashMap::new()), Ok(HashMap::new()), Ok(HashMap::new())],
        };
        let cleared = land(&after, &ask, ok, 43);
        assert!(cleared.problems.is_empty());
        assert!(cleared.issues.is_empty(), "nothing assigned is an answer");
        assert!(cleared.prs.values().all(HashMap::is_empty));

        let missing = Got {
            issues: Err(format!(
                "{LINEAR_API_KEY} is not set in nebula serve's environment"
            )),
            prs: Vec::new(),
        };
        let ask = Ask {
            teams: vec!["ACME".into()],
            projects: Vec::new(),
        };
        assert_eq!(
            land(&before, &ask, missing, 44).problems,
            ["LINEAR_API_KEY is not set in nebula serve's environment."]
        );
    }

    /// A Refresh while one is under way joins it.
    #[tokio::test]
    async fn a_refresh_under_way_is_joined_not_doubled() {
        let refresher = Refresher::new();
        let mut fetched = refresher.subscribe();
        refresher.start(Ask::default());
        assert!(fetched.borrow_and_update().running);
        refresher.start(Ask::default());
        assert!(
            !fetched.has_changed().unwrap(),
            "the second start changed nothing"
        );
        fetched.changed().await.unwrap();
        let landed = fetched.borrow_and_update().clone();
        assert!(!landed.running);
        assert!(landed.at.is_some());
        assert!(landed.problems.is_empty(), "{:?}", landed.problems);
    }
}
