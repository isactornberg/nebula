//! A project's issues, from its ISSUE SOURCE: the repo's GitHub issues
//! through `gh` in the checkout, or a Linear team's through Linear's API
//! ([`crate::linear`]). One open list per project, one conversation per
//! issue, and the two writes a user makes from the ISSUES MODAL: a comment
//! and an edit of the title and description.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::pull_request::{gh, login, str_at, web_url};

/// How long a lookup may run before we give up on it — the PR lookups'
/// budget, for the same reason: `gh` retries on a stalled network.
const TIMEOUT: std::time::Duration = crate::pull_request::TIMEOUT;
/// The most issues one list asks for. `gh` pages past its own 30-row
/// default; a repo with hundreds of open issues would spend several API
/// calls filling rows nobody scrolls to.
pub const LIST_LIMIT: usize = 100;

/// One open issue, as `gh issue list` or Linear reports it. The body rides
/// the list — one call paints the whole reading pane — and only the
/// comments are a second, per-row call ([`detail`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// What names the issue: `#15` on GitHub, `REL-123` on Linear.
    pub key: String,
    pub url: String,
    pub title: String,
    pub author: String,
    /// RFC 3339, as GitHub gives it.
    pub created_at: String,
    pub updated_at: String,
    pub labels: Vec<String>,
    /// The description, verbatim markdown; the reading pane renders it
    /// (the MARKDOWN module) with GitHub's comment rule that a newline is
    /// a line break, like a pull request's.
    pub body: String,
}

impl Issue {
    /// Row text: `#15 title` (or `REL-123 title`), the shape the OPEN PRS
    /// rows use.
    pub fn label(&self) -> String {
        if self.title.is_empty() {
            self.key.clone()
        } else {
            format!("{} {}", self.key, self.title)
        }
    }

    /// What a launch carries from the row to the DAEMON.
    pub fn launch_ref(&self) -> IssueRef {
        IssueRef {
            url: self.url.clone(),
            key: self.key.clone(),
            title: self.title.clone(),
        }
    }
}

/// The issue a QUICK PROMPT or AGENT PRESET launch is for: enough to name
/// it in the box's title, cut a branch for it, write the default task, and
/// send the URL the DAEMON persists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRef {
    pub url: String,
    /// `#15` or `REL-123`, as [`Issue::key`].
    pub key: String,
    pub title: String,
}

impl IssueRef {
    /// The task sent when the box is submitted empty: the issue itself.
    pub fn default_task(&self) -> String {
        let tracker = if self.key.starts_with('#') {
            "GitHub"
        } else {
            "Linear"
        };
        if self.title.trim().is_empty() {
            format!("Fix {tracker} issue {} ({})", self.key, self.url)
        } else {
            format!(
                "Fix {tracker} issue {}: {} ({})",
                self.key,
                self.title.trim(),
                self.url
            )
        }
    }
}

/// One thing somebody said on an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueComment {
    pub author: String,
    /// RFC 3339.
    pub at: String,
    pub body: String,
}

/// The per-row second call: the issue's conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueDetail {
    pub url: String,
    pub comments: Vec<IssueComment>,
}

/// An issue's title and description as GitHub holds them: what the editor
/// opens on, what a save sends, and what lands back on the row once
/// GitHub has taken it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueText {
    pub title: String,
    pub body: String,
}

/// Where a project's issues come from: the `issues` setting of its
/// `projects` entry, or of its space's in `spaces`. Every call below
/// matches on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueSource {
    /// The repo's GitHub issues, through `gh` in the checkout.
    GitHub,
    /// A Linear team's issues, by its key (`REL`), through Linear's API.
    Linear { team: String },
}

impl IssueSource {
    /// The setting's value: empty or `github` is GitHub, `linear:REL` the
    /// Linear team `REL` (the key is uppercased, as Linear spells it).
    /// None for anything else.
    pub fn parse(setting: &str) -> Option<Self> {
        let setting = setting.trim();
        if setting.is_empty() || setting.eq_ignore_ascii_case("github") {
            return Some(IssueSource::GitHub);
        }
        let (scheme, team) = setting.split_once(':')?;
        let team = team.trim();
        let valid = scheme.trim().eq_ignore_ascii_case("linear")
            && !team.is_empty()
            && team.chars().all(|c| c.is_ascii_alphanumeric());
        valid.then(|| IssueSource::Linear {
            team: team.to_ascii_uppercase(),
        })
    }

    /// The tracker's name, for the pane's words.
    pub fn name(&self) -> &'static str {
        match self {
            IssueSource::GitHub => "GitHub",
            IssueSource::Linear { .. } => "Linear",
        }
    }
}

/// Every open issue `source` holds for the project checked out at `dir`,
/// newest first. `None` is "couldn't ask" and is kept apart from
/// `Some(vec![])`, the real answer "nothing is open": the caller keeps the
/// last good list over a failed call.
pub async fn list(source: &IssueSource, dir: &Path) -> Option<Vec<Issue>> {
    match source {
        IssueSource::GitHub => gh_list(dir).await,
        IssueSource::Linear { team } => crate::linear::list(team).await,
    }
}

/// One issue's conversation, from `source`.
pub async fn detail(source: &IssueSource, dir: &Path, key: &str) -> Option<IssueDetail> {
    match source {
        IssueSource::GitHub => gh_detail(dir, gh_number(key)).await,
        IssueSource::Linear { .. } => crate::linear::detail(key).await,
    }
}

/// Post `body` as a comment on the issue `key`, as you. True when it
/// landed; anything else is "couldn't post".
pub async fn comment(source: &IssueSource, dir: &Path, key: &str, body: &str) -> bool {
    match source {
        IssueSource::GitHub => comment_via("gh", dir, gh_number(key), body).await,
        IssueSource::Linear { .. } => crate::linear::comment(key, body).await,
    }
}

/// Send the issue `key` a new title and description, as you; `Err` is
/// why the tracker refused.
pub async fn edit(
    source: &IssueSource,
    dir: &Path,
    key: &str,
    text: &IssueText,
) -> Result<(), String> {
    match source {
        IssueSource::GitHub => edit_via("gh", dir, gh_number(key), text).await,
        IssueSource::Linear { .. } => crate::linear::edit(key, text).await,
    }
}

/// `15` out of the key `#15`: what `gh` takes.
fn gh_number(key: &str) -> &str {
    key.trim_start_matches('#')
}

/// Ask `gh` for every open issue on `dir`'s repo, newest first. `gh issue
/// list` leaves pull requests out on its own.
async fn gh_list(dir: &Path) -> Option<Vec<Issue>> {
    let limit = LIST_LIMIT.to_string();
    let out = gh(
        Some(dir),
        &[
            "issue",
            "list",
            "--state",
            "open",
            "--limit",
            &limit,
            "--json",
            "number,url,title,author,createdAt,updatedAt,labels,body",
        ],
        TIMEOUT,
    )
    .await?;
    parse_list(&out)
}

/// Ask `gh` for one issue's conversation. `number` picks it, so any
/// checkout of the repo will do.
async fn gh_detail(dir: &Path, number: &str) -> Option<IssueDetail> {
    let out = gh(
        Some(dir),
        &["issue", "view", number, "--json", "url,comments"],
        TIMEOUT,
    )
    .await?;
    parse_detail(&out)
}

/// Post `body` as a comment on issue `number` with `program` — `gh` in
/// the app, a script on disk in the tests, since the real thing would
/// post — as the `gh` user. The body goes down stdin (`--body-file -`),
/// never argv: a comment can be long, and one opening with `-` must not
/// read as a flag. True when `gh` exited clean; anything else — no `gh`,
/// not logged in, a network that stalled past [`TIMEOUT`] — is "couldn't
/// post". A `gh` still running at the timeout is killed with the future,
/// not left behind.
async fn comment_via(
    program: impl AsRef<std::ffi::OsStr>,
    dir: &Path,
    number: &str,
    body: &str,
) -> bool {
    use tokio::io::AsyncWriteExt;
    let mut child = match tokio::process::Command::new(program)
        .args(["issue", "comment", number, "--body-file", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .current_dir(dir)
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let Some(mut stdin) = child.stdin.take() else {
        return false;
    };
    let body = body.to_string();
    let feed = async move {
        stdin.write_all(body.as_bytes()).await?;
        stdin.shutdown().await
    };
    // Feed and wait together: a `gh` that exits before reading (bad auth)
    // would otherwise leave the write blocked on a closed pipe.
    let run = async {
        let (_, status) = tokio::join!(feed, child.wait());
        status.is_ok_and(|s| s.success())
    };
    tokio::time::timeout(TIMEOUT, run).await.unwrap_or(false)
}

/// Send one issue a new title and description with `program` — `gh` in
/// the app, a script on disk in the tests, since the real thing would
/// edit — as the `gh` user. Unlike the reads this wants `gh`'s complaint,
/// not just its silence: the first line it printed is what the form shows
/// when GitHub refuses. The description goes down stdin (`--body-file -`)
/// as a comment's does; the title rides argv as `--title=…`, one token, so
/// one opening with `-` can't read as a flag either.
async fn edit_via(
    program: impl AsRef<std::ffi::OsStr>,
    dir: &Path,
    number: &str,
    text: &IssueText,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let title = format!("--title={}", text.title);
    let mut child = tokio::process::Command::new(program)
        .args(["issue", "edit", number, &title, "--body-file", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .current_dir(dir)
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("couldn't run gh: {e}"))?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err("couldn't feed gh".into());
    };
    let body = text.body.clone();
    let feed = async move {
        stdin.write_all(body.as_bytes()).await?;
        stdin.shutdown().await
    };
    // Feed and wait together, as a comment does: a `gh` that exits before
    // reading (bad auth) would otherwise leave the write blocked on a
    // closed pipe.
    let run = async {
        let (_, out) = tokio::join!(feed, child.wait_with_output());
        match out {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => Err(complaint(&out.stderr)),
            Err(e) => Err(format!("gh failed: {e}")),
        }
    };
    tokio::time::timeout(TIMEOUT, run)
        .await
        .unwrap_or_else(|_| Err("gh timed out".into()))
}

/// The first line `gh` printed that says anything, as a shell would have
/// shown it; a silent refusal gets a stock one.
fn complaint(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map_or_else(|| "gh refused the edit".to_string(), str::to_string)
}

/// Parse `gh issue list --json …` — a bare array. A row with no number or
/// no openable URL drops out rather than failing the list; a payload that
/// isn't an array is a miss.
fn parse_list(json: &str) -> Option<Vec<Issue>> {
    let rows = serde_json::from_str::<serde_json::Value>(json).ok()?;
    let rows = rows.as_array()?;
    Some(
        rows.iter()
            .filter_map(|v| {
                let url = web_url(v)?;
                Some(Issue {
                    key: format!("#{}", v.get("number")?.as_u64()?),
                    url,
                    title: str_at(v, "title"),
                    author: login(v.get("author")),
                    created_at: str_at(v, "createdAt"),
                    updated_at: str_at(v, "updatedAt"),
                    labels: v
                        .get("labels")
                        .and_then(|l| l.as_array())
                        .map(|labels| {
                            labels
                                .iter()
                                .map(|l| str_at(l, "name"))
                                .filter(|n| !n.is_empty())
                                .collect()
                        })
                        .unwrap_or_default(),
                    body: str_at(v, "body"),
                })
            })
            .collect(),
    )
}

fn parse_detail(json: &str) -> Option<IssueDetail> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let url = web_url(&v)?;
    let mut comments: Vec<IssueComment> = v
        .get("comments")
        .and_then(|c| c.as_array())
        .map(|list| {
            list.iter()
                .map(|c| IssueComment {
                    author: login(c.get("author")),
                    at: str_at(c, "createdAt"),
                    body: str_at(c, "body"),
                })
                .collect()
        })
        .unwrap_or_default();
    // RFC 3339 UTC stamps sort lexicographically into chronological order.
    comments.sort_by(|a, b| a.at.cmp(&b.at));
    Some(IssueDetail { url, comments })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(number: u64, title: &str) -> Issue {
        Issue {
            key: format!("#{number}"),
            url: format!("https://github.com/o/r/issues/{number}"),
            title: title.into(),
            author: "webdevcody".into(),
            created_at: "2026-09-10T12:00:00Z".into(),
            updated_at: "2026-09-11T12:00:00Z".into(),
            labels: vec!["bug".into(), "ui".into()],
            body: "Login bounces back to /.".into(),
        }
    }

    /// A stand-in for `gh` at `dir/name` running `sh`, renamed into place
    /// once it is whole: one still open for writing can't be run.
    #[cfg(unix)]
    fn script(dir: &Path, name: &str, sh: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        let tmp = dir.join(format!("{name}.tmp"));
        std::fs::write(&tmp, format!("#!/bin/sh\n{sh}\n")).unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(&tmp, &path).unwrap();
        path
    }

    #[test]
    fn parses_a_gh_issue_list_payload_newest_first_as_given() {
        let issues = parse_list(
            r#"[
              {"number":15,"title":"Fix login redirect","url":"https://github.com/o/r/issues/15",
               "author":{"login":"webdevcody"},"createdAt":"2026-09-10T12:00:00Z",
               "updatedAt":"2026-09-11T12:00:00Z","labels":[{"name":"bug","color":"d73a4a"},{"name":"ui"}],
               "body":"Login bounces."},
              {"number":14,"title":"","url":"https://github.com/o/r/issues/14"},
              {"number":13,"url":"file:///etc/passwd"},
              {"url":"https://github.com/o/r/issues/12"}
            ]"#,
        )
        .expect("parsed");
        assert_eq!(
            issues.len(),
            2,
            "rows without a number or an http url drop out"
        );
        assert_eq!(issues[0].label(), "#15 Fix login redirect");
        assert_eq!(issues[0].author, "webdevcody");
        assert_eq!(issues[0].labels, ["bug", "ui"]);
        assert_eq!(issues[0].body, "Login bounces.");
        assert_eq!(
            issues[1].label(),
            "#14",
            "a missing title still names the issue"
        );
        assert!(issues[1].labels.is_empty());
        assert_eq!(parse_list("[]"), Some(vec![]), "an empty repo is an answer");
        assert!(parse_list("").is_none());
        assert!(parse_list("{}").is_none());
    }

    #[test]
    fn parses_a_gh_issue_view_payload_into_an_oldest_first_thread() {
        let d = parse_detail(
            r#"{"url":"https://github.com/o/r/issues/15","comments":[
              {"author":{"login":"kate"},"createdAt":"2026-09-11T08:00:00Z","body":"same here"},
              {"author":{"login":"steiza"},"createdAt":"2026-09-10T20:00:00Z","body":"repro?"}
            ]}"#,
        )
        .expect("parsed");
        assert_eq!(d.url, "https://github.com/o/r/issues/15");
        assert_eq!(d.comments.len(), 2);
        assert_eq!(d.comments[0].author, "steiza", "sorted by time");
        assert_eq!(d.comments[1].author, "kate");
        let bare = parse_detail(r#"{"url":"https://github.com/o/r/issues/1"}"#).expect("parsed");
        assert!(bare.comments.is_empty());
        assert!(parse_detail("{}").is_none());
    }

    #[test]
    fn the_launch_ref_and_its_default_task_name_the_issue() {
        let r = issue(15, "Fix login redirect").launch_ref();
        assert_eq!(r.key, "#15");
        assert_eq!(
            r.default_task(),
            "Fix GitHub issue #15: Fix login redirect (https://github.com/o/r/issues/15)"
        );
        let untitled = issue(3, "  ").launch_ref();
        assert_eq!(
            untitled.default_task(),
            "Fix GitHub issue #3 (https://github.com/o/r/issues/3)"
        );
        let linear = IssueRef {
            url: "https://linear.app/acme/issue/REL-123/fix-login".into(),
            key: "REL-123".into(),
            title: "Fix login".into(),
        };
        assert_eq!(
            linear.default_task(),
            "Fix Linear issue REL-123: Fix login (https://linear.app/acme/issue/REL-123/fix-login)"
        );
    }

    #[test]
    fn the_issues_setting_names_github_or_a_linear_team() {
        assert_eq!(IssueSource::parse(""), Some(IssueSource::GitHub));
        assert_eq!(IssueSource::parse(" github "), Some(IssueSource::GitHub));
        let rel = Some(IssueSource::Linear { team: "REL".into() });
        assert_eq!(IssueSource::parse("linear:REL"), rel);
        assert_eq!(IssueSource::parse("Linear: rel"), rel);
        for bad in ["linear:", "linear:R-1", "jira:REL", "REL", "linear:REL x"] {
            assert_eq!(IssueSource::parse(bad), None, "{bad}");
        }
    }

    /// The body reaches `gh` whole, down stdin — `--body-file -`, so a
    /// comment longer than a pipe buffer, or one opening with `-`, arrives
    /// as typed — and a `gh` that quits without reading it (bad auth) is a
    /// clean "couldn't post", not a write stuck on a closed pipe. So is a
    /// `gh` that isn't there.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_comment_body_goes_down_stdin_and_a_refusal_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let script = |name: &str, sh: &str| script(dir.path(), name, sh);
        let records = script(
            "gh",
            "printf '%s\\n' \"$@\" > \"$(dirname \"$0\")/args\"\ncat > \"$(dirname \"$0\")/body\"",
        );
        let body = format!("-- starts like a flag\n{}", "x".repeat(200_000));
        assert!(comment_via(&records, dir.path(), "15", &body).await);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("args")).unwrap(),
            "issue\ncomment\n15\n--body-file\n-\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("body")).unwrap(),
            body
        );

        let refuses = script("gh-refuses", "exit 1");
        assert!(!comment_via(&refuses, dir.path(), "15", &body).await);
        let missing = dir.path().join("gh-missing");
        assert!(!comment_via(&missing, dir.path(), "15", "hi").await);
    }

    /// The title rides argv as one `--title=` token and the description
    /// goes down stdin, however long and whatever it starts with; `gh`'s
    /// first stderr line is the refusal's reason, a silent one gets a
    /// stock reason, and a `gh` that isn't there says so.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_edit_sends_the_title_on_argv_and_the_body_down_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let script = |name: &str, sh: &str| script(dir.path(), name, sh);
        let records = script(
            "gh",
            "printf '%s\\n' \"$@\" > \"$(dirname \"$0\")/args\"\ncat > \"$(dirname \"$0\")/body\"",
        );
        let text = IssueText {
            title: "-- starts like a flag".into(),
            body: format!("-- so does this\n{}", "x".repeat(200_000)),
        };
        assert_eq!(edit_via(&records, dir.path(), "15", &text).await, Ok(()));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("args")).unwrap(),
            "issue\nedit\n15\n--title=-- starts like a flag\n--body-file\n-\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("body")).unwrap(),
            text.body
        );

        let refuses = script(
            "gh-refuses",
            "echo >&2\necho '  GraphQL: Resource not accessible' >&2\nexit 1",
        );
        assert_eq!(
            edit_via(&refuses, dir.path(), "15", &text).await,
            Err("GraphQL: Resource not accessible".into())
        );
        let silent = script("gh-silent", "exit 1");
        assert_eq!(
            edit_via(&silent, dir.path(), "15", &text).await,
            Err("gh refused the edit".into())
        );
        let missing = dir.path().join("gh-missing");
        let why = edit_via(&missing, dir.path(), "15", &text)
            .await
            .unwrap_err();
        assert!(why.starts_with("couldn't run gh"), "{why}");
    }
}
