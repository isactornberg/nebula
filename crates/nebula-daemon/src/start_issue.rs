//! START: the web app's **Start** on an issue (`ClientRequest::StartIssue`),
//! which any socket client may send. The issue goes to its project's
//! CAPTAIN, found or started as `nebula orchestrator captain` finds or
//! starts one, in a BRIEF that has it start one worker on the issue, the
//! worker carrying the issue (`nebula spawn --issue`) so the board joins
//! the session to it.
//!
//! A brief is never typed into a captain at work or into a prompt of the
//! user's: it goes once the captain is at rest with its input box empty
//! (`Daemon::when_ready`, as a DONE NOTE waits for the orchestrator), at
//! once when it is, and until then it is HELD. Held briefs are kept in
//! `held-briefs.json` in the data dir, so a daemon restart delivers them
//! too, and every subscriber is told which are held
//! (`ServerEvent::HeldBriefs`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use nebula_core::{AgentId, AgentRole, HeldBrief, ProjectId, ServerEvent, SessionRef};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::registry::Daemon;

/// How long a Start waits, once its captain is up, for the brief to be
/// typed before it answers that the brief is held.
const BRIEF_GRACE: Duration = Duration::from_secs(3);

/// The briefs held, and the file they are kept in.
#[derive(Default)]
pub(crate) struct Briefs {
    held: Vec<Owed>,
    /// None until the daemon restores them as it boots, and in tests, where
    /// nothing is written.
    file: Option<PathBuf>,
}

/// A held brief, with the text typed once its captain is free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Owed {
    #[serde(flatten)]
    brief: HeldBrief,
    text: String,
}

impl Briefs {
    fn holds(&self, brief: &HeldBrief) -> bool {
        self.held.iter().any(|o| &o.brief == brief)
    }

    /// What every subscriber is told of them.
    pub(crate) fn event(&self) -> ServerEvent {
        ServerEvent::HeldBriefs {
            briefs: self.held.iter().map(|o| o.brief.clone()).collect(),
        }
    }

    /// Write the file again, renamed into place so a reader never sees half
    /// of it, and tell every subscriber, both under the lock the change was
    /// made in, so they land in the order the changes did.
    fn changed(&self, daemon: &Daemon) {
        if let Some(file) = &self.file {
            if let Err(err) = write(file, &self.held) {
                tracing::warn!(file = %file.display(), "held briefs not written: {err:#}");
            }
        }
        daemon.broadcast(self.event());
    }
}

fn write(file: &std::path::Path, held: &[Owed]) -> Result<()> {
    let tmp = file.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(held)?)?;
    std::fs::rename(&tmp, file)?;
    Ok(())
}

/// The held briefs `file` keeps: none when there is no file, and none, said
/// once, when it does not read.
fn read(file: &std::path::Path) -> Vec<Owed> {
    match std::fs::read(file) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            tracing::warn!(file = %file.display(), "held briefs not read: {err}");
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

/// The brief for the issue at `url`: start one worker on `task`, on
/// `branch`, carrying the issue. One `nebula spawn` the captain can run as
/// written, the task in a quoted heredoc, which its `dontAsk` mode lets
/// through whatever the task holds.
fn issue_brief(url: &str, key: &str, title: &str, task: &str, branch: &str) -> String {
    let tracker = if key.starts_with('#') {
        "GitHub"
    } else {
        "Linear"
    };
    let named = if title.is_empty() {
        String::new()
    } else {
        format!(", \"{title}\"")
    };
    format!(
        "[nebula] Start was pressed on the {tracker} issue {key}{named} in nebula's web app: it is \
         yours to start. Start one worker on it, on its own branch and carrying the issue, so \
         nebula ties the session to it:\n\n  nebula spawn --issue {url} --worktree {branch} --name \
         <short title> --json \"$(cat <<'EOF'\n{task}\nEOF\n)\"\n\nwith the task ended as every \
         task you give a worker ends. Then follow it as you follow every worker."
    )
}

impl Daemon {
    /// `ClientRequest::StartIssue`: hand the issue at `url` to the CAPTAIN
    /// of `project`, found or started ([`Self::captain_for`]) and booted
    /// when it has no live process, in a brief to start a worker on `task`.
    /// Returns the captain, and whether the brief is held: it is once the
    /// captain is not up, and when it is up but has not taken the brief
    /// within [`BRIEF_GRACE`].
    pub async fn start_issue(
        self: &Arc<Self>,
        project: &ProjectId,
        url: &str,
        key: &str,
        title: &str,
        task: &str,
    ) -> Result<(AgentId, bool)> {
        let url = crate::pr_scope::validate_issue_url(url)?;
        match nebula_core::issue_key(&url) {
            Some(named) if named == key => {}
            named => bail!(
                "the issue's key {key} is not the one its URL names, {}",
                named.as_deref().unwrap_or("none")
            ),
        }
        let task = crate::orchestrator::validate_message(task)?;
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        let lookup = project.clone();
        self.store_blocking(move |store| store.get_project(&lookup))
            .await?
            .with_context(|| format!("there is no project {project}"))?;
        let (captain, _) = self.captain_for(project).await?;
        let lookup = project.clone();
        let taken = self
            .store_blocking(move |store| {
                Ok(store
                    .load_tree()?
                    .1
                    .into_iter()
                    .filter(|w| w.project_id == lookup)
                    .map(|w| w.branch)
                    .collect::<Vec<_>>())
            })
            .await?;
        let branch = nebula_core::issue_branch(key, &title, &taken);
        let text = issue_brief(&url, key, &title, task, &branch);
        let text = crate::orchestrator::validate_message(&text)?.to_string();
        self.ensure_session(
            &SessionRef::Agent(captain.clone()),
            crate::pty::DEFAULT_COLS,
            crate::pty::DEFAULT_ROWS,
        )?;
        let up = self
            .await_session_up(&captain, Duration::ZERO)
            .await
            .is_ok();
        let brief = HeldBrief {
            issue_url: url,
            captain: captain.clone(),
            at: nebula_core::clock::now_ms(),
        };
        let (taken_tx, taken) = oneshot::channel();
        self.hold_brief(Owed { brief, text }, Some(taken_tx));
        if !up {
            return Ok((captain, true));
        }
        match tokio::time::timeout(BRIEF_GRACE, taken).await {
            Ok(Ok(())) => Ok((captain, false)),
            Err(_) => Ok((captain, true)),
            Ok(Err(_)) => bail!("the captain {captain} went before it took the brief"),
        }
    }

    /// Hold `owed` until its captain takes it ([`Self::deliver_brief`]).
    fn hold_brief(self: &Arc<Self>, owed: Owed, taken: Option<oneshot::Sender<()>>) {
        {
            let mut work = self.sent_work.lock();
            work.briefs.held.push(owed.clone());
            work.briefs.changed(self);
        }
        let daemon = self.clone();
        tokio::spawn(async move { daemon.deliver_brief(owed, taken).await });
    }

    /// Type `owed` into its captain once that is at rest with its input box
    /// empty, then tell `taken`. A captain archived or deleted before that
    /// takes its brief with it.
    async fn deliver_brief(self: &Arc<Self>, owed: Owed, taken: Option<oneshot::Sender<()>>) {
        let Owed { brief, text } = owed;
        let session = self
            .when_ready(
                &brief.captain,
                AgentRole::Captain,
                |work| work.briefs.holds(&brief).then_some(true),
                |work| {
                    work.briefs.held.retain(|o| o.brief != brief);
                    work.briefs.changed(self);
                },
            )
            .await;
        let Some(session) = session else {
            let mut work = self.sent_work.lock();
            if work.briefs.holds(&brief) {
                work.briefs.held.retain(|o| o.brief != brief);
                work.briefs.changed(self);
                tracing::warn!(captain = %brief.captain, "a held brief went with its captain");
            }
            return;
        };
        if let Some(taken) = taken {
            let _ = taken.send(());
        }
        if let Err(err) = self.type_turn(&brief.captain, &session, &text).await {
            tracing::warn!(captain = %brief.captain, "brief typed, not taken whole: {err:#}");
        }
    }

    /// The briefs held when the last daemon stopped, kept in `file` from now
    /// on: each one's captain booted when it has no live process, as a Start
    /// boots it, and the brief delivered as any held one is.
    pub fn restore_held_briefs(self: &Arc<Self>, file: PathBuf) {
        let held = read(&file);
        {
            let mut work = self.sent_work.lock();
            work.briefs = Briefs {
                held: held.clone(),
                file: Some(file),
            };
        }
        for owed in held {
            let sref = SessionRef::Agent(owed.brief.captain.clone());
            if let Err(err) =
                self.ensure_session(&sref, crate::pty::DEFAULT_COLS, crate::pty::DEFAULT_ROWS)
            {
                tracing::warn!(captain = %owed.brief.captain, "held brief's captain not booted: {err:#}");
            }
            let daemon = self.clone();
            tokio::spawn(async move { daemon.deliver_brief(owed, None).await });
        }
    }

    /// What every subscriber is told of the held briefs, as a new one is
    /// after its Snapshot.
    pub fn held_briefs(&self) -> ServerEvent {
        self.sent_work.lock().briefs.event()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The brief names the issue and has the captain start its worker in
    /// one `nebula spawn` that carries it, the task in a quoted heredoc
    /// whatever it holds.
    #[test]
    fn the_brief_has_the_captain_start_one_worker_carrying_the_issue() {
        let url = "https://linear.app/acme/issue/REL-7/fix-login";
        let task = "Fix Linear issue REL-7: Fix login (https://linear.app/acme/issue/REL-7/fix-login)\n# a heading, $HOME and `ticks`";
        let brief = issue_brief(url, "REL-7", "Fix login", task, "rel-7-fix-login");
        assert!(brief.starts_with(
            "[nebula] Start was pressed on the Linear issue REL-7, \"Fix login\" in nebula's web app"
        ));
        assert!(brief.contains(&format!(
            "nebula spawn --issue {url} --worktree rel-7-fix-login --name <short title> --json \
             \"$(cat <<'EOF'\n{task}\nEOF\n)\""
        )));
        assert!(issue_brief(
            "https://github.com/o/r/issues/3",
            "#3",
            "",
            "Fix it",
            "issue-3"
        )
        .starts_with("[nebula] Start was pressed on the GitHub issue #3 in nebula's web app"));
    }

    /// The file round-trips, and one that does not read holds nothing.
    #[test]
    fn held_briefs_outlast_the_daemon_in_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("held-briefs.json");
        assert!(read(&file).is_empty(), "no file holds nothing");
        let held = vec![Owed {
            brief: HeldBrief {
                issue_url: "https://linear.app/acme/issue/REL-7".into(),
                captain: AgentId("c".into()),
                at: 5,
            },
            text: "the brief".into(),
        }];
        write(&file, &held).unwrap();
        assert_eq!(read(&file), held);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(json[0]["issue_url"], "https://linear.app/acme/issue/REL-7");
        std::fs::write(&file, "not json").unwrap();
        assert!(read(&file).is_empty());
    }

    /// A Start is refused before any captain is asked for: an issue URL the
    /// daemon does not take, a key the URL does not name, an empty task and
    /// a project that is not there.
    #[tokio::test]
    async fn a_start_is_refused_before_a_captain_is_asked_for() {
        let daemon = Daemon::new(
            Arc::new(crate::store::Store::open_in_memory().unwrap()),
            crate::hooks::HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        let project = ProjectId("nope".into());
        let url = "https://linear.app/acme/issue/REL-7/fix-login";
        let refused = |result: Result<(AgentId, bool)>| result.unwrap_err().to_string();
        let err = refused(
            daemon
                .start_issue(&project, "https://evil.dev/x", "REL-7", "t", "task")
                .await,
        );
        assert!(err.contains("not an issue URL"), "{err}");
        let err = refused(
            daemon
                .start_issue(&project, url, "REL-8", "t", "task")
                .await,
        );
        assert!(err.contains("not the one its URL names, REL-7"), "{err}");
        let err = refused(daemon.start_issue(&project, url, "REL-7", "t", " ").await);
        assert!(err.contains("empty"), "{err}");
        let err = refused(
            daemon
                .start_issue(&project, url, "REL-7", "t", "task")
                .await,
        );
        assert!(err.contains("there is no project nope"), "{err}");
        assert!(daemon.store.load_tree().unwrap().2.is_empty(), "no captain");
    }
}
