//! START: the page's **Start** on an issue in a lane. The serve asks the
//! DAEMON to hand the issue to the captain of the project the page picked
//! (`ClientRequest::StartIssue`), one whose `issues` setting names the
//! issue's Linear team, then moves the issue to its team's first started
//! state in Linear, unless it is started already. What it did is kept in
//! `issue-starts.json` in nebula's data dir, so the page says it across a
//! serve restart and in every browser: each issue it handed over, and the
//! project it last handed each Linear project's issues to, which the page
//! picks first next time.

use crate::api::{IssueStarted, StartIssue};
use crate::hub::Hub;
use crate::refresh::Refresher;
use crate::{sentence, Refusal};
use axum::http::StatusCode;
use nebula_forge::issues::IssueRef;
use nebula_forge::linear::Assigned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// How many handed-over issues the file keeps, the newest.
const KEPT: usize = 100;

/// What Start keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Starts {
    /// The project Start last handed an issue to, by the issue's Linear
    /// project ([`linear_project`]).
    pub last_project: BTreeMap<String, String>,
    /// The issues Start handed to a captain, by key.
    pub handed: BTreeMap<String, Handed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handed {
    pub project_id: String,
    pub project_name: String,
    pub captain_id: String,
    pub at: i64,
    pub move_failed: Option<String>,
}

impl Starts {
    /// What `file` keeps: nothing when there is no file or it does not read.
    pub fn load(file: &Path) -> Starts {
        std::fs::read(file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Write it to `file`, renamed into place so a reader never sees half
    /// of it, keeping the [`KEPT`] newest issues.
    fn save(&mut self, file: &Path) {
        while self.handed.len() > KEPT {
            if let Some(oldest) = self
                .handed
                .iter()
                .min_by_key(|(_, h)| h.at)
                .map(|(key, _)| key.clone())
            {
                self.handed.remove(&oldest);
            }
        }
        let tmp = file.with_extension(format!("json.{}", std::process::id()));
        let written = serde_json::to_vec_pretty(self)
            .map_err(std::io::Error::other)
            .and_then(|json| std::fs::write(&tmp, json))
            .and_then(|()| std::fs::rename(&tmp, file));
        if let Err(err) = written {
            eprintln!("nebula serve: {} not written: {err}", file.display());
        }
    }
}

/// The key Start keeps the last project by: the issue's team and its
/// Linear project, none being one too.
pub fn linear_project(issue: &Assigned) -> String {
    format!("{}/{}", issue.team, issue.project.as_deref().unwrap_or(""))
}

/// The issues a Start is under way for, so a second press of one waits for
/// the first to answer instead of handing it over twice.
#[derive(Clone, Default)]
pub struct Starting(Arc<Mutex<HashSet<String>>>);

struct Underway(Starting, String);

impl Starting {
    fn begin(&self, key: &str) -> Option<Underway> {
        let mut keys = self.0.lock().unwrap_or_else(|e| e.into_inner());
        keys.insert(key.to_string())
            .then(|| Underway(self.clone(), key.to_string()))
    }
}

impl Drop for Underway {
    fn drop(&mut self) {
        let mut keys = self.0 .0.lock().unwrap_or_else(|e| e.into_inner());
        keys.remove(&self.1);
    }
}

/// Start the issue `key` in the project `to` names: refused for an issue
/// the last refresh did not read, a project that does not read its issues
/// from that issue's team, an issue handed over already unless `to` says
/// again, and while another Start of it is under way. The move in Linear
/// comes once the daemon has taken the hand-off, which stands whether the
/// move does or not.
pub(crate) async fn start(
    hub: &Hub,
    refresher: &Refresher,
    file: &Path,
    starting: &Starting,
    key: &str,
    to: StartIssue,
) -> Result<IssueStarted, Refusal> {
    let live = hub.live().ok_or_else(Refusal::down)?;
    let fetched = refresher.fetched();
    let issue = fetched
        .issues
        .iter()
        .find(|i| i.key == key)
        .cloned()
        .ok_or_else(|| {
            Refusal::new(
                StatusCode::NOT_FOUND,
                format!(
                    "{key} is not among the issues assigned to you that the last refresh read."
                ),
            )
        })?;
    let conflict = |message: String| Refusal::new(StatusCode::CONFLICT, message);
    let project = live
        .state
        .projects
        .iter()
        .find(|p| p.id == to.project)
        .ok_or_else(|| {
            Refusal::new(
                StatusCode::NOT_FOUND,
                format!("There is no project {}.", to.project),
            )
        })?;
    if project.team.as_deref() != Some(issue.team.as_str()) {
        return Err(conflict(format!(
            "{} does not read its issues from Linear's {} team.",
            project.name, issue.team
        )));
    }
    if let Some(handed) = fetched.starts.handed.get(key).filter(|_| !to.again) {
        return Err(conflict(format!(
            "{key} was handed to the captain of {} already.",
            handed.project_name
        )));
    }
    let _underway = starting
        .begin(key)
        .ok_or_else(|| conflict(format!("A Start of {key} is under way.")))?;
    let mut client = nebula_client::Client::connect("nothing was started")
        .await
        .map_err(|_| Refusal::down())?;
    let task = IssueRef {
        url: issue.url.clone(),
        key: issue.key.clone(),
        title: issue.title.clone(),
    }
    .default_task();
    let (captain, held) = client
        .start_issue(
            &nebula_core::ProjectId(project.id.clone()),
            &issue.url,
            &issue.key,
            &issue.title,
            &task,
        )
        .await
        .map_err(|err| {
            if err.is::<nebula_client::Closed>() {
                Refusal::down()
            } else {
                conflict(sentence(&format!("{err:#}")))
            }
        })?;
    let handed = Handed {
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        captain_id: captain.0.clone(),
        at: nebula_core::clock::now_ms(),
        move_failed: None,
    };
    refresher.patch(|f| {
        f.starts
            .last_project
            .insert(linear_project(&issue), project.id.clone());
        f.starts.handed.insert(issue.key.clone(), handed);
        f.starts.save(file);
    });
    let moved = match issue.state_type.as_str() {
        "started" => Ok(None),
        _ => nebula_forge::linear::start(&issue.key).await,
    };
    // The page says it is Linear's answer: `Linear: Entity not found` is
    // `Entity not found` there.
    let (moved_to, move_failed) = match moved {
        Ok(moved) => (moved, None),
        Err(why) => (
            None,
            Some(sentence(why.strip_prefix("Linear: ").unwrap_or(&why))),
        ),
    };
    refresher.patch(|f| {
        if let Some(state) = moved_to.as_ref().filter(|s| !s.name.is_empty()) {
            for i in f.issues.iter_mut().filter(|i| i.key == issue.key) {
                i.state = state.name.clone();
                i.state_type = state.kind.clone();
            }
        }
        if let Some(handed) = f.starts.handed.get_mut(&issue.key) {
            handed.move_failed = move_failed.clone();
        }
        f.starts.save(file);
    });
    Ok(IssueStarted {
        captain_id: captain.0,
        held,
        moved_to: moved_to.map(|s| s.name),
        move_failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handed(at: i64) -> Handed {
        Handed {
            project_id: "p".into(),
            project_name: "api".into(),
            captain_id: "c".into(),
            at,
            move_failed: None,
        }
    }

    /// The file round-trips, keeps the newest issues once it holds too
    /// many, and one that does not read keeps nothing.
    #[test]
    fn what_start_keeps_outlasts_the_serve_in_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("issue-starts.json");
        assert_eq!(Starts::load(&file), Starts::default());
        let mut starts = Starts::default();
        starts
            .last_project
            .insert("ACME/Accounts".into(), "p".into());
        for n in 0..=KEPT as i64 {
            starts.handed.insert(format!("ACME-{n}"), handed(n));
        }
        starts.save(&file);
        let read = Starts::load(&file);
        assert_eq!(read.handed.len(), KEPT);
        assert!(!read.handed.contains_key("ACME-0"), "the oldest went");
        assert_eq!(read.last_project["ACME/Accounts"], "p");
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(json["handed"]["ACME-1"]["projectName"], "api");
        std::fs::write(&file, "{").unwrap();
        assert_eq!(Starts::load(&file), Starts::default());
    }

    /// A second Start of an issue waits for the first to answer, and once
    /// it has, it may go.
    #[test]
    fn a_start_under_way_holds_off_a_second() {
        let starting = Starting::default();
        let first = starting.begin("ACME-1").unwrap();
        assert!(starting.begin("ACME-1").is_none());
        assert!(starting.begin("ACME-2").is_some());
        drop(first);
        assert!(starting.begin("ACME-1").is_some());
    }

    #[test]
    fn the_last_project_is_by_team_and_linear_project() {
        let mut issue = crate::refresh::tests::issue("ACME-1", "unstarted", 0, 0);
        assert_eq!(linear_project(&issue), "ACME/");
        issue.project = Some("Accounts".into());
        assert_eq!(linear_project(&issue), "ACME/Accounts");
    }
}
