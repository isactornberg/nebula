//! The page's **Schedules**: the daemon's SCHEDULES and run log as the
//! STATE carries them ([`of`]), and the add, remove and run now it sends
//! the daemon, each over a client of its own, as Start does.

use crate::api::{self, RunOutcomeName, ScheduleTaskName};
use crate::{body, hub::Hub, refused, App, Refusal};
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use nebula_client::Tree;
use nebula_core::{
    AgentKind, ProjectId, RunOutcome, ScheduleSpec, ScheduleTask, ScheduleWhen, DAY_NAMES,
};

/// The schedules and the run log, newest run first, as the page shows
/// them, with what the add form offers.
pub fn of(tree: &Tree) -> api::Schedules {
    let book = &tree.schedules;
    let schedules = book
        .schedules
        .iter()
        .map(|s| {
            let mut view = api::Schedule {
                id: s.id.clone(),
                name: s.spec.name.clone(),
                task: ScheduleTaskName::Rank,
                project_id: None,
                project_name: None,
                prompt: None,
                harness: None,
                model: None,
                effort: None,
                permission_mode: None,
                target: None,
                when: when_of(&s.spec.when),
                next_at: s.next_at,
                created_at: s.created_at,
                session_id: s.session.as_ref().map(|id| id.0.clone()),
            };
            if let ScheduleTask::Prompt {
                project,
                prompt,
                kind,
                custom_harness,
                model,
                effort,
                permission_mode,
                target,
            } = &s.spec.task
            {
                view = api::Schedule {
                    task: ScheduleTaskName::Prompt,
                    project_id: Some(project.0.clone()),
                    project_name: tree
                        .projects
                        .iter()
                        .find(|p| &p.id == project)
                        .map(|p| p.name.clone()),
                    prompt: Some(prompt.clone()),
                    harness: Some(
                        custom_harness
                            .clone()
                            .unwrap_or_else(|| kind.as_str().to_string()),
                    ),
                    model: model.clone(),
                    effort: effort.clone(),
                    permission_mode: permission_mode.clone(),
                    target: Some((*target).into()),
                    ..view
                };
            }
            view
        })
        .collect();
    let runs = book.runs.iter().rev().map(run_of).collect();
    api::Schedules {
        enabled: true,
        available: true,
        schedules,
        runs,
        harnesses: api::harnesses()
            .iter()
            .map(|h| api::HarnessChoice {
                id: h.id.clone(),
                label: h.display_label().to_string(),
                models: h.model.models.iter().map(|m| m.id.clone()).collect(),
                efforts: h.effort.efforts.clone(),
                reports_status: h.hook_dialect().is_some(),
                resumes: h.resumes(),
            })
            .collect(),
        permission_modes: nebula_core::PERMISSION_MODES.to_vec(),
        login_item: login_item(),
    }
}

/// Whether the LOGIN ITEM `nebula login-item on` writes is there.
fn login_item() -> Option<bool> {
    let plist = nebula_core::paths::login_item_plist()?;
    cfg!(target_os = "macos").then(|| plist.exists())
}

fn when_of(when: &ScheduleWhen) -> api::ScheduleWhen {
    match *when {
        ScheduleWhen::Every { minutes } => api::ScheduleWhen::Every { minutes },
        ScheduleWhen::At { hour, minute, days } => api::ScheduleWhen::At {
            hour,
            minute,
            days: DAY_NAMES
                .iter()
                .enumerate()
                .filter(|(i, _)| days & (1 << i) != 0)
                .map(|(_, d)| d.to_string())
                .collect(),
        },
        ScheduleWhen::Once { at } => api::ScheduleWhen::Once { at },
    }
}

fn run_of(run: &nebula_core::ScheduleRun) -> api::ScheduleRun {
    let (outcome, why) = match &run.outcome {
        RunOutcome::Started { .. } => (RunOutcomeName::Started, None),
        RunOutcome::Held { .. } => (RunOutcomeName::Held, None),
        RunOutcome::Typed { .. } => (RunOutcomeName::Typed, None),
        RunOutcome::Restarted { .. } => (RunOutcomeName::Restarted, None),
        RunOutcome::Skipped { why } => (RunOutcomeName::Skipped, Some(why.clone())),
        RunOutcome::Failed { why } => (RunOutcomeName::Failed, Some(why.clone())),
    };
    api::ScheduleRun {
        schedule_id: run.schedule.clone(),
        due_at: run.due_at,
        at: run.at,
        outcome,
        session_id: run.outcome.session().map(|id| id.0.clone()),
        why,
        note: run.note.clone(),
    }
}

/// The page's add as the daemon takes it, or why it is no schedule.
fn spec_of(add: api::AddSchedule) -> Result<ScheduleSpec, String> {
    let when = match add.when {
        api::ScheduleWhen::Every { minutes } => ScheduleWhen::Every { minutes },
        api::ScheduleWhen::At { hour, minute, days } => {
            let mut set = 0;
            for day in &days {
                let i = DAY_NAMES
                    .iter()
                    .position(|d| d == day)
                    .ok_or_else(|| format!("{day} is no day of the week."))?;
                set |= 1 << i;
            }
            ScheduleWhen::At {
                hour,
                minute,
                days: set,
            }
        }
        api::ScheduleWhen::Once { at } => ScheduleWhen::Once { at },
    };
    let task = match add.task {
        ScheduleTaskName::Rank => ScheduleTask::Rank,
        ScheduleTaskName::Prompt => {
            let project = add
                .project_id
                .filter(|p| !p.is_empty())
                .ok_or("Pick the project it runs in.")?;
            let prompt = add.prompt.unwrap_or_default();
            let harness = add.harness.unwrap_or_else(|| "claude".into());
            let (kind, custom_harness) = match AgentKind::parse(&harness) {
                Some(AgentKind::Custom) | None => (AgentKind::Custom, Some(harness)),
                Some(kind) => (kind, None),
            };
            ScheduleTask::Prompt {
                project: ProjectId(project),
                prompt,
                kind,
                custom_harness,
                model: add.model,
                effort: add.effort,
                permission_mode: add.permission_mode,
                target: add.target.ok_or("Pick where each run goes.")?.into(),
            }
        }
    };
    Ok(ScheduleSpec {
        name: add.name,
        task,
        when,
    })
}

/// A client of the daemon's own for one request, or why not.
async fn client(hub: &Hub, undone: &str) -> Result<nebula_client::Client, Refusal> {
    let client = hub.connect(undone).await?;
    if client.protocol < nebula_core::SCHEDULE_PROTOCOL {
        return Err(Refusal::new(
            StatusCode::CONFLICT,
            "The running daemon keeps no schedules: `nebula reload` moves it onto this build, \
             its sessions kept.",
        ));
    }
    Ok(client)
}

/// `POST /api/schedules`: add one.
pub async fn add(
    State(app): State<App>,
    got: Result<Json<api::AddSchedule>, JsonRejection>,
) -> Result<Json<api::ScheduleAdded>, Refusal> {
    let spec = spec_of(body(got)?).map_err(|why| Refusal::new(StatusCode::BAD_REQUEST, why))?;
    let mut client = client(&app.hub, "no schedule was added").await?;
    let id = client.add_schedule(spec).await.map_err(refused)?;
    Ok(Json(api::ScheduleAdded { id }))
}

/// A client of the daemon's own, once it keeps the schedule `id`.
async fn client_with(hub: &Hub, id: &str, undone: &str) -> Result<nebula_client::Client, Refusal> {
    let mut client = client(hub, undone).await?;
    let book = client.schedules().await.map_err(refused)?;
    if !book.schedules.iter().any(|s| s.id == id) {
        return Err(Refusal::new(
            StatusCode::NOT_FOUND,
            format!("There is no schedule {id}."),
        ));
    }
    Ok(client)
}

/// `POST /api/schedules/:id/remove`.
pub async fn remove(State(app): State<App>, Path(id): Path<String>) -> Result<StatusCode, Refusal> {
    let mut client = client_with(&app.hub, &id, "no schedule was removed").await?;
    client.remove_schedule(&id).await.map_err(refused)?;
    Ok(StatusCode::ACCEPTED)
}

/// `POST /api/schedules/:id/run`: run it now, answering with the run.
pub async fn run(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<api::ScheduleRun>, Refusal> {
    let mut client = client_with(&app.hub, &id, "no schedule was run").await?;
    let run = client.run_schedule(&id).await.map_err(refused)?;
    Ok(Json(run_of(&run)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(when: api::ScheduleWhen) -> api::AddSchedule {
        api::AddSchedule {
            name: String::new(),
            task: ScheduleTaskName::Prompt,
            project_id: Some("p".into()),
            prompt: Some("Triage".into()),
            harness: Some("codex".into()),
            model: None,
            effort: None,
            permission_mode: None,
            target: Some(api::ScheduleTargetName::FreshSession),
            when,
        }
    }

    /// The page's add reads as the daemon takes it, its days by name and
    /// its harness by registry id, and one with no project, an unknown day
    /// or no target is refused.
    #[test]
    fn the_pages_add_is_the_daemons_spec() {
        let spec = spec_of(add(api::ScheduleWhen::At {
            hour: 7,
            minute: 30,
            days: vec!["mon".into(), "fri".into()],
        }))
        .unwrap();
        assert_eq!(
            spec.when,
            ScheduleWhen::At {
                hour: 7,
                minute: 30,
                days: 0b1_0001
            }
        );
        let ScheduleTask::Prompt {
            kind,
            custom_harness,
            target,
            ..
        } = &spec.task
        else {
            unreachable!()
        };
        assert_eq!((*kind, custom_harness.as_deref()), (AgentKind::Codex, None));
        assert_eq!(*target, nebula_core::ScheduleTarget::FreshSession);
        let mut custom = add(api::ScheduleWhen::Every { minutes: 5 });
        custom.harness = Some("agy".into());
        let ScheduleTask::Prompt {
            kind,
            custom_harness,
            ..
        } = spec_of(custom).unwrap().task
        else {
            unreachable!()
        };
        assert_eq!(
            (kind, custom_harness.as_deref()),
            (AgentKind::Custom, Some("agy"))
        );
        let mut nowhere = add(api::ScheduleWhen::Every { minutes: 5 });
        nowhere.project_id = None;
        assert_eq!(
            spec_of(nowhere).unwrap_err(),
            "Pick the project it runs in."
        );
        let someday = add(api::ScheduleWhen::At {
            hour: 7,
            minute: 30,
            days: vec!["someday".into()],
        });
        assert_eq!(
            spec_of(someday).unwrap_err(),
            "someday is no day of the week."
        );
        let rank = api::AddSchedule {
            task: ScheduleTaskName::Rank,
            target: None,
            ..add(api::ScheduleWhen::Every { minutes: 5 })
        };
        assert_eq!(spec_of(rank).unwrap().task, ScheduleTask::Rank);
    }

    /// The STATE says each schedule as the page shows it, its runs newest
    /// first with the session each went to and why one was skipped.
    #[test]
    fn the_state_says_the_schedules_and_their_runs() {
        let mut tree = Tree::default();
        tree.projects.push(nebula_core::Project {
            id: ProjectId("p".into()),
            name: "api".into(),
            repo_path: "/code/api".into(),
            sort_order: 0,
        });
        let spec = spec_of(add(api::ScheduleWhen::At {
            hour: 7,
            minute: 30,
            days: vec!["mon".into(), "tue".into()],
        }))
        .unwrap();
        tree.schedules.schedules.push(nebula_core::Schedule {
            id: "s".into(),
            spec,
            created_at: 1,
            next_at: Some(2),
            session: Some(nebula_core::AgentId("a".into())),
        });
        let run = |at, outcome| nebula_core::ScheduleRun {
            schedule: "s".into(),
            due_at: Some(at),
            at,
            outcome,
            note: None,
        };
        tree.schedules.runs = vec![
            run(
                1,
                RunOutcome::Started {
                    session: nebula_core::AgentId("a".into()),
                },
            ),
            run(2, RunOutcome::Skipped { why: "late".into() }),
        ];
        let got = of(&tree);
        let json = serde_json::to_value(&got).unwrap();
        assert_eq!(json["schedules"][0]["projectName"], "api");
        assert_eq!(json["schedules"][0]["harness"], "codex");
        assert_eq!(json["schedules"][0]["target"], "freshSession");
        assert_eq!(
            json["schedules"][0]["when"],
            serde_json::json!({"kind": "at", "hour": 7, "minute": 30, "days": ["mon", "tue"]})
        );
        assert_eq!(json["schedules"][0]["sessionId"], "a");
        assert_eq!(json["runs"][0]["outcome"], "skipped");
        assert_eq!(json["runs"][0]["why"], "late");
        assert_eq!(json["runs"][1]["sessionId"], "a");
        assert!(got
            .harnesses
            .iter()
            .any(|h| h.id == "claude" && h.reports_status));
        assert!(got
            .harnesses
            .iter()
            .any(|h| h.id == "muse" && !h.reports_status));
    }
}
