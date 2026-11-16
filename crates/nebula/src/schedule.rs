//! `nebula schedule add | list | rm | run`: the daemon's SCHEDULES from a
//! shell, for scripts and agents, over the socket like `nebula session`.

use anyhow::{bail, Context, Result};
use nebula_client::Client;
use nebula_core::{
    AgentKind, ProjectId, RunOutcome, Schedule, ScheduleRun, ScheduleSpec, ScheduleTarget,
    ScheduleTask, ScheduleWhen, DAYS_ALL,
};
use nebula_daemon::schedule::when::{self, Local};

/// What `nebula schedule add` was asked for, each flag under its name.
pub(crate) struct AddOpts {
    pub(crate) prompt: String,
    pub(crate) rank: bool,
    pub(crate) project: Option<String>,
    pub(crate) every: Option<u32>,
    pub(crate) at: Option<String>,
    pub(crate) days: Option<String>,
    pub(crate) once: Option<String>,
    pub(crate) target: Option<ScheduleTarget>,
    pub(crate) kind: Option<AgentKind>,
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) permission_mode: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) json: bool,
}

pub(crate) enum Op {
    Add(Box<AddOpts>),
    List { json: bool },
    Rm { schedule: String },
    Run { schedule: String, json: bool },
}

pub(crate) fn run(op: Op) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run_op(op))
}

async fn run_op(op: Op) -> Result<()> {
    let undone = match &op {
        Op::Add(_) => "no schedule added",
        Op::List { .. } => "no schedules to list",
        Op::Rm { .. } => "no schedule removed",
        Op::Run { .. } => "no schedule run",
    };
    let mut client = Client::connect(undone).await?;
    client.schedules().await?;
    match op {
        Op::Add(opts) => {
            let json = opts.json;
            let spec = spec_of(*opts, &client)?;
            let id = client.add_schedule(spec).await?;
            let added = client
                .schedules()
                .await?
                .schedules
                .iter()
                .find(|s| s.id == id);
            if json {
                println!("{}", serde_json::json!({ "id": id }));
            } else if let Some(s) = added {
                println!("Added schedule {id}, {}: {}.", s.spec.name, said_when(s));
            } else {
                println!("Added schedule {id}.");
            }
        }
        Op::List { json } => {
            let book = &client.tree.schedules;
            if json {
                println!("{}", serde_json::to_string(book)?);
            } else if book.schedules.is_empty() {
                println!("No schedules.");
            } else {
                for s in &book.schedules {
                    let last = book.runs.iter().rev().find(|r| r.schedule == s.id);
                    println!("{}  {}", s.id, s.spec.name);
                    println!("    {}", what(s, &client));
                    println!("    {}", said_when(s));
                    if let Some(run) = last {
                        println!("    last: {}", said_run(run));
                    }
                }
            }
        }
        Op::Rm { schedule } => {
            let s = find(&client, &schedule)?.clone();
            client.remove_schedule(&s.id).await?;
            println!("Removed schedule {}, {}.", s.id, s.spec.name);
        }
        Op::Run { schedule, json } => {
            let s = find(&client, &schedule)?.clone();
            let run = client.run_schedule(&s.id).await?;
            if json {
                println!("{}", serde_json::to_string(&run)?);
            } else {
                println!("{}: {}", s.spec.name, said_run(&run));
            }
            if matches!(run.outcome, RunOutcome::Failed { .. }) {
                bail!("the run failed");
            }
        }
    }
    Ok(())
}

/// The schedule `named` names: by its id, the start of its id, or its name.
fn find<'a>(client: &'a Client, named: &str) -> Result<&'a Schedule> {
    let all = &client.tree.schedules.schedules;
    let lower = named.to_ascii_lowercase();
    if let Some(s) = all.iter().find(|s| s.id == lower) {
        return Ok(s);
    }
    let by: Vec<&Schedule> = all
        .iter()
        .filter(|s| s.id.starts_with(&lower) || s.spec.name.eq_ignore_ascii_case(named))
        .collect();
    match by.as_slice() {
        [one] => Ok(one),
        [] => bail!("no schedule is {named}: `nebula schedule list` lists them"),
        _ => bail!("{named} names more than one schedule: give more of its id"),
    }
}

fn spec_of(opts: AddOpts, client: &Client) -> Result<ScheduleSpec> {
    let when = match (opts.every, &opts.at, &opts.once) {
        (Some(minutes), None, None) => ScheduleWhen::Every { minutes },
        (None, Some(at), None) => {
            let (hour, minute) = parse_time(at)?;
            let days = match &opts.days {
                Some(days) => nebula_core::parse_days(days).with_context(|| {
                    format!("--days {days}: name daily, weekdays, weekends, or days such as mon,wed,fri")
                })?,
                None => DAYS_ALL,
            };
            ScheduleWhen::At { hour, minute, days }
        }
        (None, None, Some(once)) => ScheduleWhen::Once {
            at: parse_once(once)?,
        },
        _ => bail!("say when it runs: one of --every <minutes>, --at <HH:MM> or --once <WHEN>"),
    };
    let name = opts.name.unwrap_or_default();
    if opts.rank {
        if !opts.prompt.trim().is_empty() {
            bail!("a rank takes no prompt: its turn is nebula's own");
        }
        return Ok(ScheduleSpec {
            name,
            task: ScheduleTask::Rank,
            when,
        });
    }
    if opts.prompt.trim().is_empty() {
        bail!("give the prompt each run starts on, or --rank");
    }
    let project = match &opts.project {
        Some(named) => project_named(client, named)?,
        None => project_here(client)?,
    };
    Ok(ScheduleSpec {
        name,
        task: ScheduleTask::Prompt {
            project,
            prompt: opts.prompt,
            kind: opts.kind.unwrap_or_default(),
            custom_harness: None,
            model: opts.model,
            effort: opts.effort,
            permission_mode: opts.permission_mode,
            target: opts.target.unwrap_or(ScheduleTarget::Root),
        },
        when,
    })
}

fn project_named(client: &Client, named: &str) -> Result<ProjectId> {
    let projects = &client.tree.projects;
    if let Some(p) = projects.iter().find(|p| p.id.0 == named) {
        return Ok(p.id.clone());
    }
    let by: Vec<_> = projects
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(named))
        .collect();
    match by.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => bail!("no project is {named}: `nebula sessions` names them"),
        _ => bail!("{named} names more than one project: give its id"),
    }
}

/// The project whose checkout the current directory is in.
fn project_here(client: &Client) -> Result<ProjectId> {
    let here = std::env::current_dir()?;
    let here = std::fs::canonicalize(&here).unwrap_or(here);
    client
        .tree
        .worktrees
        .iter()
        .filter(|w| {
            let path = std::fs::canonicalize(&w.path).unwrap_or_else(|_| w.path.clone());
            here.starts_with(path)
        })
        .max_by_key(|w| w.path.as_os_str().len())
        .map(|w| w.project_id.clone())
        .context("the current directory is in no project's checkout: name one with --project")
}

/// `HH:MM`, or `H:MM`.
fn parse_time(text: &str) -> Result<(u8, u8)> {
    let bad = || anyhow::anyhow!("{text} is no time of day: give it as HH:MM, 07:30");
    let (h, m) = text.trim().split_once(':').ok_or_else(bad)?;
    let (hour, minute): (u8, u8) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    if hour > 23 || minute > 59 || m.len() != 2 {
        return Err(bad());
    }
    Ok((hour, minute))
}

/// `YYYY-MM-DD HH:MM` (or with a `T`) on this Mac's clock, epoch ms.
fn parse_once(text: &str) -> Result<i64> {
    let bad = || anyhow::anyhow!("{text} is no date and time: give it as \"2026-11-20 15:00\"");
    let (date, time) = text.trim().split_once([' ', 'T']).ok_or_else(bad)?;
    let mut parts = date.split('-').map(str::parse::<i64>);
    let (Some(Ok(year)), Some(Ok(month)), Some(Ok(day)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(bad());
    };
    let (hour, minute) = parse_time(time.trim())?;
    let (month, day) = (u32::try_from(month)?, u32::try_from(day)?);
    when::at_civil(year, month, day, hour.into(), minute.into(), &Local).ok_or_else(bad)
}

/// What a schedule runs, on one line.
fn what(s: &Schedule, client: &Client) -> String {
    match &s.spec.task {
        ScheduleTask::Rank => "the orchestrator's rank of the focus picks".into(),
        ScheduleTask::Prompt {
            project,
            prompt,
            kind,
            model,
            effort,
            permission_mode,
            target,
            ..
        } => {
            let project = client
                .tree
                .projects
                .iter()
                .find(|p| &p.id == project)
                .map_or(project.0.as_str(), |p| p.name.as_str());
            let mut on = kind.as_str().to_string();
            for knob in [model, effort, permission_mode].into_iter().flatten() {
                on.push(' ');
                on.push_str(knob);
            }
            let line: String = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
            let line = if line.chars().count() > 60 {
                format!("{}…", line.chars().take(59).collect::<String>())
            } else {
                line
            };
            format!("{project}, {}, on {on}: {line}", target.as_str())
        }
    }
}

/// When a schedule runs, and next.
fn said_when(s: &Schedule) -> String {
    let form = match s.spec.when {
        ScheduleWhen::Every { minutes } => format!("every {minutes} min"),
        ScheduleWhen::At { hour, minute, days } => {
            format!("{} at {hour:02}:{minute:02}", nebula_core::days_label(days))
        }
        ScheduleWhen::Once { at } => format!("once at {}", when::label(at, &Local)),
    };
    match s.next_at {
        Some(next) => format!("{form}, next {}", when::label(next, &Local)),
        None => format!("{form}, done"),
    }
}

/// What a run did, on one line.
fn said_run(run: &ScheduleRun) -> String {
    let at = when::label(run.at, &Local);
    let did = match &run.outcome {
        RunOutcome::Started { session } => format!("started session {session}"),
        RunOutcome::Held { session } => {
            format!("held for {session} until it is at rest with an empty input box")
        }
        RunOutcome::Typed { session } => format!("typed into {session}"),
        RunOutcome::Restarted { session } => {
            format!("started {session} again on a new conversation")
        }
        RunOutcome::Skipped { why } => format!("skipped: {why}"),
        RunOutcome::Failed { why } => format!("failed: {why}"),
    };
    match &run.note {
        Some(note) => format!("{at}, {did}. {note}"),
        None => format!("{at}, {did}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_as_a_clock_shows_them() {
        assert_eq!(parse_time("07:30").unwrap(), (7, 30));
        assert_eq!(parse_time("7:05").unwrap(), (7, 5));
        for bad in ["24:00", "7:5", "noon", "07:60"] {
            assert!(parse_time(bad).is_err(), "{bad}");
        }
        let at = parse_once("2026-11-20 15:00").unwrap();
        assert_eq!(when::label(at, &Local), "Fri 2026-11-20 15:00");
        assert_eq!(parse_once("2026-11-20T15:00").unwrap(), at);
        assert!(parse_once("2026-02-30 15:00").is_err());
        assert!(parse_once("tomorrow").is_err());
    }
}
