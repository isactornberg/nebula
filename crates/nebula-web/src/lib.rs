//! `nebula serve`: the web app and the JSON it reads, on 127.0.0.1. A
//! client of the DAEMON like the TUI: it subscribes over the daemon's
//! socket ([`hub`]), keeps the entity tree and the BOARD's reads, and
//! answers the app's contract (`app/src/api/contract.ts`, mirrored in
//! [`api`]) from what it holds. It types a session's next turn as `nebula
//! session send` does ([`send`]), with the images the page sent for it
//! ([`images`]), and a new Claude session's first in a project on the
//! page's New, picks an option of the dialog open in one ([`pick`]), and
//! marks a finish seen. The open Linear issues assigned
//! to you and the pull requests on the sessions' checkouts it reads itself
//! when the page asks it to ([`refresh`]), and it hands an issue to a
//! project's captain on the page's Start ([`start`]). The orchestrator's
//! focus picks it holds to its candidates and keeps ([`focus`]). The day a program
//! such as agentos writes to the file `today_file` names it reads again
//! whenever that file changes ([`today`]). The skills the agent CLIs on
//! this machine load it reads off disk itself ([`skills`]), daemon or none,
//! and it switches one on or off in that CLI's own settings file.
//!
//! What it serves is live agent sessions, so every request must name this
//! server as its host, `127.0.0.1` or `localhost` at the bound port, which
//! a page another site serves cannot (DNS rebinding), or come through
//! `tailscale serve` for this machine's tailnet name from a device paired
//! to it ([`tailnet`], [`devices`], [`pair`]), which may turn notifications
//! on: its page subscribes, and the DAEMON pushes to it when a session turns
//! red ([`nebula_forge::push`]); every request but a read
//! must come from the app's own origin; and no response carries a CORS
//! header, so no other origin may read one.
//!
//! One server serves each daemon: it writes its port to
//! [`paths::serve_port_path`] while it runs, which is how a second one
//! ([`serving`]) and the TUI find it.

pub mod answers;
pub mod api;
mod assets;
mod devices;
pub mod focus;
mod hub;
mod images;
mod pair;
mod pick;
mod refresh;
mod schedules;
mod send;
mod skills;
mod start;
mod tailnet;
mod today;

pub use assets::index_html;

use anyhow::{Context, Result};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Extension, Json, Router};
use hub::{Hub, Unanswered};
use nebula_core::{paths, AgentId};
use nebula_forge::push;
use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

/// Where `nebula serve` listens unless told otherwise: the port after
/// `nebula browser`'s (ttyd's 7681), so the two serve side by side.
pub const DEFAULT_PORT: u16 = 7682;

/// How soon a browser whose stream dropped asks again.
const STREAM_RETRY: Duration = Duration::from_secs(1);

/// How often the port file is written again if it went: macOS's tmp
/// cleaner takes a file in `/tmp` nobody read for three days.
const PORT_FILE_CHECK: Duration = Duration::from_secs(60);

/// Serve on 127.0.0.1 until the process is stopped: on `port`, or with
/// none [`DEFAULT_PORT`] when it is free and a free port otherwise
/// (`Some(0)` is any free port). `ready` is handed the URL once it is
/// listening.
pub fn run(port: Option<u16>, ready: impl FnOnce(&str)) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let listener = bind(port).await?;
        let port = listener.local_addr()?.port();
        let file = PortFile::write(port)?;
        let refresher = refresh::Refresher::new();
        let starts = paths::data_dir().join("issue-starts.json");
        let picks = paths::data_dir().join("focus.json");
        let attachments = nebula_core::attachments::dir();
        nebula_core::attachments::prune(&attachments);
        refresher.patch(|f| {
            f.starts = start::Starts::load(&starts);
            f.focus = focus::Kept::load(&picks);
        });
        let hub = Hub::spawn(refresher.clone());
        let reach = Reach {
            tailnet: tailnet::Tailnet::spawn(),
            devices: devices::Devices::load(paths::data_dir().join("devices.json")),
        };
        ready(&format!("http://127.0.0.1:{port}"));
        let mut check = tokio::time::interval(PORT_FILE_CHECK);
        let serve = std::future::IntoFuture::into_future(axum::serve(
            listener,
            router(
                hub,
                refresher,
                port,
                skills::Cache::here(),
                [starts, picks, attachments],
                reach,
            ),
        ));
        tokio::pin!(serve);
        let stop = stopped();
        tokio::pin!(stop);
        loop {
            tokio::select! {
                served = &mut serve => break served?,
                stop = &mut stop => break stop?,
                _ = check.tick() => file.keep(),
            }
        }
        Ok(())
    })
}

/// Until the process is asked to stop: Ctrl+C, a `kill`, or its terminal
/// going away. The open event streams never end on their own, so the
/// server stops without waiting on them.
async fn stopped() -> Result<()> {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    tokio::select! {
        got = tokio::signal::ctrl_c() => got?,
        _ = term.recv() => {}
        _ = hangup.recv() => {}
    }
    Ok(())
}

async fn bind(port: Option<u16>) -> Result<tokio::net::TcpListener> {
    let at = |port| tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    match port {
        Some(port) => at(port)
            .await
            .with_context(|| format!("port {port} is not free")),
        None => match at(DEFAULT_PORT).await {
            Ok(listener) => Ok(listener),
            Err(_) => {
                let listener = at(0).await.context("no free port on 127.0.0.1")?;
                println!(
                    "nebula serve: {DEFAULT_PORT} is busy, so serving on {} instead",
                    listener.local_addr()?.port()
                );
                Ok(listener)
            }
        },
    }
}

/// This server's port file: written with its port once it has bound, and
/// removed when it stops, while it still names that port.
struct PortFile(u16);

impl PortFile {
    fn write(port: u16) -> Result<PortFile> {
        write_port(port)?;
        Ok(PortFile(port))
    }

    /// Write it again when it went, and no other server's took its place.
    fn keep(&self) {
        if !paths::serve_port_path().exists() {
            let _ = write_port(self.0);
        }
    }
}

fn write_port(port: u16) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let path = paths::serve_port_path();
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .with_context(|| format!("create {}", dir.display()))?;
    }
    // Renamed into place, so a reader never sees half of it.
    let tmp = path.with_extension(format!("port.{}", std::process::id()));
    std::fs::write(&tmp, format!("{port}\n"))
        .and_then(|()| std::fs::rename(&tmp, &path))
        .with_context(|| format!("write {}", path.display()))
}

impl Drop for PortFile {
    fn drop(&mut self) {
        let path = paths::serve_port_path();
        if port_in(&path) == Some(self.0) {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn port_in(path: &std::path::Path) -> Option<u16> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// The URL of the server already serving this daemon, by its port file,
/// once it answers there as `nebula serve` does.
pub fn serving() -> Option<String> {
    nebula_client::serve::serving_port(&paths::serve_port_path())
        .map(|port| format!("http://127.0.0.1:{port}"))
}

#[derive(Clone)]
pub(crate) struct App {
    hub: Hub,
    refresher: refresh::Refresher,
    port: u16,
    skills: skills::Cache,
    /// Where Start keeps what it did ([`start::Starts`]).
    starts: std::sync::Arc<std::path::PathBuf>,
    starting: start::Starting,
    /// Where the focus keeps its picks ([`focus::Kept`]), and the one rank
    /// asked for at a time.
    focus: std::sync::Arc<std::path::PathBuf>,
    ranking: std::sync::Arc<tokio::sync::Mutex<()>>,
    /// Where the images a follow-up carries are kept ([`images`]).
    attachments: std::sync::Arc<std::path::PathBuf>,
    tailnet: tailnet::Tailnet,
    devices: devices::Devices,
}

/// Who may reach the server from off this machine: this machine's tailnet
/// name, and the devices paired to it.
struct Reach {
    tailnet: tailnet::Tailnet,
    devices: devices::Devices,
}

/// The server's routes, with `files` where Start and the focus keep what
/// they do, and the folder images are kept in.
fn router(
    hub: Hub,
    refresher: refresh::Refresher,
    port: u16,
    skills: skills::Cache,
    files: [std::path::PathBuf; 3],
    reach: Reach,
) -> Router {
    let [starts, picks, attachments] = files;
    let app = App {
        hub,
        refresher,
        port,
        skills,
        starts: std::sync::Arc::new(starts),
        starting: start::Starting::default(),
        focus: std::sync::Arc::new(picks),
        ranking: std::sync::Arc::default(),
        attachments: std::sync::Arc::new(attachments),
        tailnet: reach.tailnet,
        devices: reach.devices,
    };
    Router::new()
        .route("/pair", post(pair::pair))
        .route("/pair/{code}", get(pair::link))
        .route("/api/devices", get(device_list))
        .route("/api/devices/pair", post(pair_device))
        .route("/api/devices/{id}/remove", post(remove_device))
        .route("/api/devices/{id}/test", post(test_device))
        .route("/api/push", get(push_state))
        .route("/api/push/subscribe", post(push_subscribe))
        .route("/api/push/unsubscribe", post(push_unsubscribe))
        .route("/api/state", get(state))
        .route("/api/events", get(events))
        .route("/api/refresh", post(refresh))
        .route("/api/issues/{key}/start", post(start_issue))
        .route("/api/schedules", post(schedules::add))
        .route("/api/schedules/{id}/remove", post(schedules::remove))
        .route("/api/schedules/{id}/run", post(schedules::run))
        .route("/api/attention", get(focus::attention_route))
        .route("/api/focus/picks", post(focus::picks_route))
        .route("/api/focus/rank", post(focus::rank_route))
        .route("/api/focus/feedback", post(focus::feedback_route))
        .route(
            "/api/images",
            post(keep_image).layer(DefaultBodyLimit::max(images::MAX_BYTES)),
        )
        .route("/api/sessions", post(start_session))
        .route("/api/sessions/{id}", get(detail))
        .route("/api/sessions/{id}/turns", get(turns))
        .route("/api/sessions/{id}/screen", get(screen))
        .route("/api/sessions/{id}/send", post(send_to_session))
        .route("/api/sessions/{id}/answer", post(answer))
        .route("/api/sessions/{id}/show", post(show))
        .route("/api/sessions/{id}/seen", post(seen))
        .route("/api/orchestrator/send", post(send_to_orchestrator))
        .route("/api/skills", get(skill_list))
        .route("/api/skills/usage", get(skill_usage))
        .route("/api/skills/switch", post(switch_skills))
        .route("/api/skills/plugin", post(switch_plugin))
        .route("/api/skills/fix", post(fix_skills))
        .route("/api/skills/undo", post(undo_fix))
        .route("/api/skills/compare/{fix}", get(compare_copies))
        .route(
            "/api/skills/check",
            post(check_skill).layer(DefaultBodyLimit::max(skills::BODY_MAX)),
        )
        .route(
            "/api/skills/save",
            post(save_skill).layer(DefaultBodyLimit::max(skills::BODY_MAX)),
        )
        .route("/api/skills/delete", post(delete_skill))
        .route("/api/skills/{id}", get(skill))
        .route("/api/{*rest}", any(no_endpoint))
        .method_not_allowed_fallback(not_allowed)
        .fallback(get(assets::serve))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    let body = api::ApiError {
        error: message.into(),
    };
    (status, Json(body)).into_response()
}

const DOWN: &str = "nebula serve is not connected to a daemon.";

/// Why a request was not done: the status it is answered with, and what
/// to tell the user.
#[derive(Debug)]
pub(crate) struct Refusal {
    status: StatusCode,
    message: String,
}

impl Refusal {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Refusal {
            status,
            message: message.into(),
        }
    }

    fn down() -> Self {
        Refusal::new(StatusCode::SERVICE_UNAVAILABLE, DOWN)
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        error(self.status, self.message)
    }
}

impl From<Unanswered> for Refusal {
    fn from(unanswered: Unanswered) -> Self {
        match unanswered {
            Unanswered::Down => Refusal::down(),
            Unanswered::Late => {
                Refusal::new(StatusCode::GATEWAY_TIMEOUT, "The daemon did not answer.")
            }
            Unanswered::Refused(why) => Refusal::new(StatusCode::BAD_GATEWAY, sentence(&why)),
        }
    }
}

impl IntoResponse for Unanswered {
    fn into_response(self) -> Response {
        Refusal::from(self).into_response()
    }
}

/// `text`, a message of the daemon's, as a sentence: capitalized and
/// ending on a stop.
pub(crate) fn sentence(text: &str) -> String {
    let mut chars = text.trim().chars();
    let mut out: String = chars
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    out.extend(chars);
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

/// Refuse what did not come from this server's own page: a request that
/// comes neither from this machine nor through `tailscale serve`
/// ([`tailnet::visitor`]), anything but a read whose `Origin` is not the
/// page's, and a device on the tailnet that is not paired, but for the page
/// that pairs it ([`pair`]). Every response, a refusal included, names the
/// server it came from, which is how a second server and the TUI tell it
/// from another program that took the port its file names
/// ([`nebula_client::serve`]).
async fn guard(State(app): State<App>, mut request: Request, next: Next) -> Response {
    let mut response = match admit(&app, &mut request).await {
        Ok(cookie) => {
            let mut response = next.run(request).await;
            if let Some(cookie) = cookie.and_then(|c| HeaderValue::try_from(c).ok()) {
                response.headers_mut().append(header::SET_COOKIE, cookie);
            }
            response
        }
        Err(refused) => *refused,
    };
    response.headers_mut().insert(
        header::SERVER,
        HeaderValue::from_static(nebula_client::serve::SERVER),
    );
    response
}

/// The device on the tailnet a request comes from, by its id.
#[derive(Clone)]
struct Paired(String);

/// Let `request` through, with who it comes from, or the response that
/// refuses it. A paired device's page load is let through with its cookie
/// to set again, so one paired with an older cookie, such as a `Strict`
/// one, takes the current one without pairing again.
async fn admit(app: &App, request: &mut Request) -> Result<Option<String>, Box<Response>> {
    let read = matches!(*request.method(), Method::GET | Method::HEAD);
    let headers = request.headers();
    let refuse = |why: &str| Box::new(error(StatusCode::FORBIDDEN, why));
    let visitor = app
        .tailnet
        .visitor(headers, app.port)
        .await
        .map_err(|why| refuse(&why))?;
    if !read && !visitor.own_origin(headers, app.port) {
        return Err(refuse("only nebula's own page may ask this server to act"));
    }
    let mut cookie = None;
    if let tailnet::Visitor::Tailnet { login, .. } = &visitor {
        let path = request.uri().path();
        if !pair::open_to_all(path) {
            let token = devices::token_in(headers);
            let id = app
                .devices
                .check(token, login)
                .map_err(|why| Box::new(pair::refused(path, why, login, headers)))?;
            if path.starts_with("/api/devices") {
                return Err(refuse(
                    "Only nebula on the Mac itself pairs a device or removes one.",
                ));
            }
            let page_load = headers
                .get("sec-fetch-dest")
                .is_some_and(|dest| dest == "document");
            cookie = token.filter(|_| page_load).map(devices::set_cookie);
            request.extensions_mut().insert(Paired(id));
        }
    }
    request.extensions_mut().insert(visitor);
    Ok(cookie)
}

/// The paired devices and the open pairing, for this machine's page alone.
async fn device_list(State(app): State<App>) -> Json<api::Devices> {
    Json(app.devices.list(app.tailnet.name()))
}

/// Open a pairing for a new device, in place of any open one, with this
/// machine's tailnet name read again for its link.
async fn pair_device(State(app): State<App>) -> Result<Json<api::Devices>, Refusal> {
    app.tailnet.read_again(true).await;
    if app.tailnet.name().is_none() {
        return Err(Refusal::new(
            StatusCode::CONFLICT,
            "Tailscale is not running on this Mac, so no phone can reach this page: `tailscale status` names no tailnet for it.",
        ));
    }
    app.devices.open_pairing();
    Ok(Json(app.devices.list(app.tailnet.name())))
}

/// Remove a paired device: its cookie lets nothing in from then on, and a
/// stream open to it ends.
async fn remove_device(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<api::Devices>, Refusal> {
    if !app.devices.remove(&id) {
        return Err(Refusal::new(
            StatusCode::NOT_FOUND,
            "There is no such paired device.",
        ));
    }
    Ok(Json(app.devices.list(app.tailnet.name())))
}

/// Send the paired device `id` a test notification, as the daemon sends
/// one when a session turns red.
async fn test_device(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<api::PushTested>, Refusal> {
    let name = app
        .devices
        .name(&id)
        .ok_or_else(|| Refusal::new(StatusCode::NOT_FOUND, "There is no such paired device."))?;
    let Some(kept) = app.devices.subscriptions(&id).into_iter().next() else {
        return Err(Refusal::new(
            StatusCode::CONFLICT,
            format!("{name} has not turned notifications on: turn them on in nebula's page on it."),
        ));
    };
    let notice = push::Notice {
        title: "nebula".into(),
        body: "A test from nebula on your Mac: notifications reach this device.".into(),
        tag: "nebula-test".into(),
        url: "/".into(),
    };
    match push::send(&vapid(&app)?, &kept.subscription, &notice).await {
        push::Sent::Delivered => Ok(Json(api::PushTested {
            said: format!("Sent a test to {name}."),
        })),
        push::Sent::Gone => {
            app.devices.gone(&kept);
            Err(Refusal::new(
                StatusCode::CONFLICT,
                format!("{name}'s push service no longer knows it, so its notifications are off: turn them on again on it."),
            ))
        }
        push::Sent::Failed(why) => Err(Refusal::new(
            StatusCode::BAD_GATEWAY,
            sentence(&format!("the test did not reach {name}: {why}")),
        )),
    }
}

/// The paired device a request comes from: notifications are for one alone.
fn paired_device(paired: Option<Extension<Paired>>) -> Result<String, Refusal> {
    paired.map(|Extension(Paired(id))| id).ok_or_else(|| {
        Refusal::new(
            StatusCode::NOT_FOUND,
            "Notifications are for a phone paired to this Mac: turn them on in nebula's page on it.",
        )
    })
}

fn vapid(app: &App) -> Result<push::Vapid, Refusal> {
    app.devices.vapid().map_err(|err| {
        eprintln!("nebula serve: the VAPID key was not kept: {err}");
        Refusal::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The key notifications are signed with could not be kept.",
        )
    })
}

/// What the device `id` turns notifications on with, and whether they are.
fn push_of(app: &App, id: &str) -> Result<api::Push, Refusal> {
    Ok(api::Push {
        key: vapid(app)?.public(),
        on: !app.devices.subscriptions(id).is_empty(),
    })
}

async fn push_state(
    State(app): State<App>,
    paired: Option<Extension<Paired>>,
) -> Result<Json<api::Push>, Refusal> {
    Ok(Json(push_of(&app, &paired_device(paired)?)?))
}

/// Keep the subscription the device's browser made, in place of any it had.
async fn push_subscribe(
    State(app): State<App>,
    paired: Option<Extension<Paired>>,
    got: Result<Json<api::PushSubscription>, JsonRejection>,
) -> Result<Json<api::Push>, Refusal> {
    let id = paired_device(paired)?;
    let got = body(got)?;
    let subscription = push::Subscription {
        endpoint: got.endpoint,
        p256dh: got.keys.p256dh,
        auth: got.keys.auth,
    };
    app.devices
        .subscribe(&id, &subscription)
        .map_err(|why| Refusal::new(StatusCode::BAD_REQUEST, why))?;
    Ok(Json(push_of(&app, &id)?))
}

async fn push_unsubscribe(
    State(app): State<App>,
    paired: Option<Extension<Paired>>,
) -> Result<Json<api::Push>, Refusal> {
    let id = paired_device(paired)?;
    app.devices.unsubscribe(&id);
    Ok(Json(push_of(&app, &id)?))
}

async fn state(State(app): State<App>) -> Response {
    match app.hub.live() {
        Some(live) => (
            [(header::CONTENT_TYPE, "application/json")],
            live.json.clone(),
        )
            .into_response(),
        None => Refusal::down().into_response(),
    }
}

/// A `state` event carrying the whole STATE now, and another each time it
/// is published. The stream ends when the daemon goes away, and a browser
/// asking again meanwhile is answered `503`: what the app shows as
/// disconnected, until the daemon is back and a stream opens again. A
/// paired device's stream also ends once the device is removed.
async fn events(State(app): State<App>, paired: Option<Extension<Paired>>) -> Response {
    let live = app.hub.subscribe();
    if live.borrow().is_none() {
        return Refusal::down().into_response();
    }
    // Marked changed, so a removal just before the stream began is seen
    // at its first turn.
    let gone = paired.map(|Extension(Paired(id))| {
        let mut removals = app.devices.removals();
        removals.mark_changed();
        (app.devices.clone(), removals, id)
    });
    let stream = futures_util::stream::unfold(
        (live, gone, true),
        |(mut live, mut gone, first)| async move {
            if !first {
                loop {
                    let removed = async {
                        match &mut gone {
                            Some((_, removals, _)) => removals.changed().await,
                            None => std::future::pending().await,
                        }
                    };
                    tokio::select! {
                        changed = live.changed() => {
                            changed.ok()?;
                            break;
                        }
                        _ = removed => {}
                    }
                    if gone
                        .as_ref()
                        .is_some_and(|(devices, _, id)| !devices.has(id))
                    {
                        return None;
                    }
                }
            }
            let state = live.borrow_and_update().clone()?;
            let event = Event::default().event("state").data(&state.json);
            let event = if first {
                event.retry(STREAM_RETRY)
            } else {
                event
            };
            Some((Ok::<_, Infallible>(event), (live, gone, false)))
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Read Linear's issues and the sessions' pull requests again, or join the
/// refresh under way; the STATE says when it lands.
async fn refresh(State(app): State<App>) -> Result<StatusCode, Refusal> {
    let live = app.hub.live().ok_or_else(Refusal::down)?;
    app.refresher.start(refresh::Ask::of(&live));
    Ok(StatusCode::ACCEPTED)
}

/// Hand the issue `key` to the captain of the project the body names, and
/// move it to its started state in Linear.
async fn start_issue(
    State(app): State<App>,
    Path(key): Path<String>,
    got: Result<Json<api::StartIssue>, JsonRejection>,
) -> Result<Json<api::IssueStarted>, Refusal> {
    let to = body(got)?;
    let started = start::start(
        &app.hub,
        &app.refresher,
        &app.starts,
        &app.starting,
        &key,
        to,
    )
    .await?;
    Ok(Json(started))
}

async fn detail(State(app): State<App>, Path(id): Path<String>) -> Response {
    let Some(live) = app.hub.live() else {
        return Refusal::down().into_response();
    };
    let Some(session) = live.state.sessions.iter().find(|s| s.id == id).cloned() else {
        return no_session(&id).into_response();
    };
    let agent = AgentId(id);
    let (reply, tail) = tokio::join!(app.hub.reply(agent.clone()), app.hub.screen(agent));
    let (reply, tail) = match (reply, tail) {
        (Ok(reply), Ok(tail)) => (reply, tail),
        (Err(e), _) | (_, Err(e)) => return e.into_response(),
    };
    let answers = match &reply {
        Some(text) if session.harness == api::Harness::Claude => answers::answers_of(text),
        _ => Vec::new(),
    };
    let rows = tail.map(|t| nebula_client::screen::screen_text(&t.data, t.cols, t.rows));
    let dialog = rows
        .as_deref()
        .and_then(nebula_core::dialog::choice_in)
        .map(api::Dialog::from);
    let reply = match (reply, rows) {
        (Some(text), _) => Some(api::Reply {
            text,
            source: api::ReplySource::Transcript,
            at: None,
        }),
        (None, Some(rows)) => Some(api::Reply {
            text: rows.join("\n"),
            source: api::ReplySource::Screen,
            at: None,
        }),
        (None, None) => None,
    };
    let worker_ids = match session.role {
        api::Role::Captain => live
            .state
            .sessions
            .iter()
            .filter(|s| s.project_id == session.project_id && s.id != session.id)
            .map(|s| s.id.clone())
            .collect(),
        _ => Vec::new(),
    };
    Json(api::SessionDetail {
        session,
        reply,
        answers,
        dialog,
        worker_ids,
    })
    .into_response()
}

/// A session's conversation, turn by turn: apart from its detail, so the
/// detail, which the Deck reads too, never waits on a long transcript, and
/// the page asks for the turns only where it shows them. A session not
/// Claude's has none, as its transcript is no Claude transcript.
async fn turns(State(app): State<App>, Path(id): Path<String>) -> Response {
    let Some(live) = app.hub.live() else {
        return Refusal::down().into_response();
    };
    let Some(session) = live.state.sessions.iter().find(|s| s.id == id) else {
        return no_session(&id).into_response();
    };
    let got = match session.harness {
        api::Harness::Claude => match app.hub.turns(AgentId(id)).await {
            Ok(got) => got,
            Err(e) => return e.into_response(),
        },
        _ => None,
    };
    let (turns, more) = got.map_or((None, false), |(turns, more)| {
        (Some(turns.into_iter().map(api::Turn::from).collect()), more)
    });
    Json(api::Turns { turns, more }).into_response()
}

async fn screen(State(app): State<App>, Path(id): Path<String>) -> Response {
    let Some(live) = app.hub.live() else {
        return Refusal::down().into_response();
    };
    if !live.state.sessions.iter().any(|s| s.id == id) {
        return no_session(&id).into_response();
    }
    let screen = match app.hub.screen(AgentId(id)).await {
        Ok(Some(tail)) => api::Screen {
            rows: nebula_client::screen::screen_text(&tail.data, tail.cols, tail.rows),
            cols: tail.cols,
            height: tail.rows,
        },
        Ok(None) => api::Screen {
            rows: Vec::new(),
            cols: 0,
            height: 0,
        },
        Err(e) => return e.into_response(),
    };
    Json(screen).into_response()
}

fn no_session(id: &str) -> Refusal {
    Refusal::new(
        StatusCode::NOT_FOUND,
        format!("There is no live session {id}."),
    )
}

/// A request's JSON body, or why it is not one this endpoint takes.
pub(crate) fn body<T>(got: Result<Json<T>, JsonRejection>) -> Result<T, Refusal> {
    got.map(|Json(body)| body).map_err(|rejection| {
        Refusal::new(StatusCode::BAD_REQUEST, sentence(&rejection.body_text()))
    })
}

#[derive(serde::Deserialize)]
struct Text {
    text: String,
}

/// A follow-up: its text, and the images [`keep_image`] kept for it.
#[derive(serde::Deserialize)]
struct FollowUp {
    text: String,
    #[serde(default)]
    images: Vec<String>,
}

#[derive(serde::Deserialize)]
struct Pick {
    option: u32,
}

async fn send_to_session(
    State(app): State<App>,
    Path(id): Path<String>,
    got: Result<Json<FollowUp>, JsonRejection>,
) -> Result<StatusCode, Refusal> {
    let FollowUp { text, images } = body(got)?;
    let paths = images
        .iter()
        .map(|image| images::path_of(&app.attachments, image))
        .collect::<Result<Vec<_>, _>>()?;
    let text = images::with_paths(&text, &paths);
    send::send(&app.hub, send::To::Session(&id), &text, !paths.is_empty()).await?;
    Ok(StatusCode::ACCEPTED)
}

/// The page's New: a Claude session started in the project the body names,
/// its first turn the body's text and images ([`send::start`]).
async fn start_session(
    State(app): State<App>,
    got: Result<Json<api::NewSession>, JsonRejection>,
) -> Result<(StatusCode, Json<api::SessionStarted>), Refusal> {
    let new = body(got)?;
    let paths = new
        .images
        .iter()
        .map(|image| images::path_of(&app.attachments, image))
        .collect::<Result<Vec<_>, _>>()?;
    let text = images::with_paths(&new.text, &paths);
    let images = !paths.is_empty();
    // On a task of its own, so a page that goes away while the session
    // boots does not leave it started with nothing typed.
    let hub = app.hub.clone();
    let started =
        tokio::spawn(async move { send::start(&hub, &new.project_id, &text, images).await })
            .await
            .map_err(|_| {
                Refusal::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "The start did not finish.",
                )
            })??;
    Ok((StatusCode::CREATED, Json(started)))
}

/// An image for a follow-up, its bytes the request's whole body: kept
/// ([`images::keep`]), and answered with the id the follow-up names it by.
async fn keep_image(
    State(app): State<App>,
    got: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Result<(StatusCode, Json<api::Image>), Refusal> {
    let bytes = got.map_err(|rejection| match rejection.status() {
        StatusCode::PAYLOAD_TOO_LARGE => images::too_large(),
        status => Refusal::new(status, sentence(&rejection.body_text())),
    })?;
    let id = images::keep(&app.attachments, &bytes).await?;
    Ok((StatusCode::CREATED, Json(api::Image { id })))
}

async fn send_to_orchestrator(
    State(app): State<App>,
    got: Result<Json<Text>, JsonRejection>,
) -> Result<StatusCode, Refusal> {
    send::send(&app.hub, send::To::Orchestrator, &body(got)?.text, false).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn answer(
    State(app): State<App>,
    Path(id): Path<String>,
    got: Result<Json<Pick>, JsonRejection>,
) -> Result<StatusCode, Refusal> {
    let option = body(got)?.option;
    let live = app.hub.live().ok_or_else(Refusal::down)?;
    let session = live
        .state
        .sessions
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| no_session(&id))?;
    pick::pick(&app.hub, &AgentId(id.clone()), session.harness, option).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn seen(State(app): State<App>, Path(id): Path<String>) -> Result<StatusCode, Refusal> {
    let live = app.hub.live().ok_or_else(Refusal::down)?;
    if !live.state.sessions.iter().any(|s| s.id == id) {
        return Err(no_session(&id));
    }
    app.hub.seen(AgentId(id)).await?;
    Ok(StatusCode::ACCEPTED)
}

/// Every TUI puts the session up ("Open in terminal"), and the one touched
/// last brings its window to the front: refused while no TUI is open,
/// which a daemon too old to say counts as. The `202` says whether that
/// TUI had the session up already.
async fn show(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<api::Shown>), Refusal> {
    let live = app.hub.live().ok_or_else(Refusal::down)?;
    if !live.state.sessions.iter().any(|s| s.id == id) {
        return Err(no_session(&id));
    }
    let conflict = |message: String| Refusal::new(StatusCode::CONFLICT, message);
    if live.protocol < nebula_core::SHOW_PROTOCOL {
        return Err(conflict(
            "The running daemon cannot show a session in the TUI: `nebula reload` moves it \
             onto this build, its sessions kept."
                .into(),
        ));
    }
    let Some(tui) = &live.state.tui else {
        return Err(conflict("No TUI is open to show the session in.".into()));
    };
    let shown = api::Shown::of(tui, &id);
    let mut client = app.hub.connect("nothing was shown").await?;
    client.show_session(&AgentId(id)).await.map_err(|err| {
        if err.is::<nebula_client::Closed>() {
            Refusal::down()
        } else {
            conflict(sentence(&format!("{err:#}")))
        }
    })?;
    Ok((StatusCode::ACCEPTED, Json(shown)))
}

/// The latest scan of the skills, with the projects of the daemon's tree
/// when one is connected.
async fn skill_scan(app: &App) -> Result<std::sync::Arc<skills::Scan>, Refusal> {
    let repos = app.hub.live().map(|l| l.repos.clone()).unwrap_or_default();
    app.skills.scan(repos).await.ok_or_else(|| {
        Refusal::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Reading the skills failed.",
        )
    })
}

async fn skill_list(State(app): State<App>) -> Response {
    match skill_scan(&app).await {
        Ok(scan) => (
            [(header::CONTENT_TYPE, "application/json")],
            scan.json.clone(),
        )
            .into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

/// How often Claude used each skill lately, from its transcripts.
async fn skill_usage(State(app): State<App>) -> Result<Json<api::SkillUsage>, Refusal> {
    let scan = skill_scan(&app).await?;
    let usage = app.skills.usage(scan).await.ok_or_else(|| {
        Refusal::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Reading Claude's transcripts failed.",
        )
    })?;
    Ok(Json(usage))
}

/// A skill's detail, its file read now.
async fn skill(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<api::SkillDetail>, Refusal> {
    let scan = skill_scan(&app).await?;
    let detail = tokio::task::spawn_blocking(move || {
        scan.get(&id)
            .map(|found| skills::detail(found, scan.places()))
            .ok_or_else(|| Refusal::new(StatusCode::NOT_FOUND, format!("There is no skill {id}.")))
    })
    .await
    .map_err(|_| {
        Refusal::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Reading the skill failed.",
        )
    })??;
    Ok(Json(detail))
}

/// What the scan would say of a skill's file holding a text not yet saved.
async fn check_skill(
    State(app): State<App>,
    got: Result<Json<api::CheckSkill>, JsonRejection>,
) -> Result<Json<api::SkillCheck>, Refusal> {
    let to = body(got)?;
    let scan = skill_scan(&app).await?;
    let checked = tokio::task::spawn_blocking(move || skills::check(&scan, &to.id, &to.text))
        .await
        .map_err(|_| {
            Refusal::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Checking the text failed.",
            )
        })??;
    Ok(Json(checked))
}

/// Save a skill's file, by the id the list gave the skill and the version
/// its detail gave the text: the answer carries the token its Undo takes.
async fn save_skill(
    State(app): State<App>,
    got: Result<Json<api::SaveSkill>, JsonRejection>,
) -> Result<Json<api::SkillDone>, Refusal> {
    let to = body(got)?;
    let scan = skill_scan(&app).await?;
    let done = app
        .skills
        .act(move || skills::save(&scan, &to))
        .await
        .ok_or_else(act_failed)??;
    Ok(Json(done))
}

/// Move a skill to the Trash, by the id the list gave it and the version
/// its detail gave what goes: the answer carries the token its Undo takes.
async fn delete_skill(
    State(app): State<App>,
    got: Result<Json<api::DeleteSkill>, JsonRejection>,
) -> Result<Json<api::SkillDone>, Refusal> {
    let to = body(got)?;
    let scan = skill_scan(&app).await?;
    let done = app
        .skills
        .act(move || skills::delete(&scan, &to))
        .await
        .ok_or_else(act_failed)??;
    Ok(Json(done))
}

/// Set a CLI's switch for skills, by the ids the list gave them.
async fn switch_skills(
    State(app): State<App>,
    got: Result<Json<api::SwitchSkills>, JsonRejection>,
) -> Result<Json<api::SkillWrite>, Refusal> {
    let to = body(got)?;
    let scan = skill_scan(&app).await?;
    let written = app
        .skills
        .write(move || skills::switch(&scan, to.cli, &to.set))
        .await
        .ok_or_else(write_failed)??;
    Ok(Json(written))
}

/// Turn a Claude plugin on or off, by the id the list gave it.
async fn switch_plugin(
    State(app): State<App>,
    got: Result<Json<api::SwitchPlugin>, JsonRejection>,
) -> Result<Json<api::SkillWrite>, Refusal> {
    let to = body(got)?;
    let scan = skill_scan(&app).await?;
    let written = app
        .skills
        .write(move || skills::switch_plugin(&scan, &to.plugin, to.on))
        .await
        .ok_or_else(write_failed)??;
    Ok(Json(written))
}

/// Act on a fix, by the id the list gave it: the answer carries the token
/// its Undo takes.
async fn fix_skills(
    State(app): State<App>,
    got: Result<Json<api::FixSkills>, JsonRejection>,
) -> Result<Json<api::SkillDone>, Refusal> {
    let to = body(got)?;
    let scan = skill_scan(&app).await?;
    let done = app
        .skills
        .act(move || skills::act(&scan, &to.fix, to.act))
        .await
        .ok_or_else(act_failed)??;
    Ok(Json(done))
}

/// Undo an act, by the token its answer carried, once.
async fn undo_fix(
    State(app): State<App>,
    got: Result<Json<api::UndoFix>, JsonRejection>,
) -> Result<Json<api::SkillDone>, Refusal> {
    let to = body(got)?;
    let undo = app.skills.take_undo(&to.undo).ok_or_else(|| {
        Refusal::new(
            StatusCode::NOT_FOUND,
            "There is nothing to undo: it was undone already, or nebula serve started again since.",
        )
    })?;
    let scan = skill_scan(&app).await?;
    let done = app
        .skills
        .write(move || skills::undo(scan.places(), &undo))
        .await
        .ok_or_else(act_failed)??;
    Ok(Json(done))
}

/// The two copies of one skill a fix is about, read now.
async fn compare_copies(
    State(app): State<App>,
    Path(fix): Path<String>,
) -> Result<Json<api::SkillCompare>, Refusal> {
    let scan = skill_scan(&app).await?;
    let compared = tokio::task::spawn_blocking(move || skills::compare(&scan, &fix))
        .await
        .map_err(|_| {
            Refusal::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Reading the two files failed.",
            )
        })??;
    Ok(Json(compared))
}

fn act_failed() -> Refusal {
    Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, "The act did not finish.")
}

fn write_failed() -> Refusal {
    Refusal::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Writing the setting failed.",
    )
}

async fn no_endpoint() -> Response {
    error(StatusCode::NOT_FOUND, "no such endpoint")
}

async fn not_allowed() -> Response {
    error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A server with no daemon behind it, and its port.
    async fn server() -> u16 {
        server_reading(std::path::Path::new("/nonexistent")).await
    }

    /// [`server`], reading the skills of a machine whose home is `home`.
    async fn server_reading(home: &std::path::Path) -> u16 {
        server_with(home, None).await
    }

    /// The tailnet name of the machine [`server_on_tailnet`] serves.
    const TAILNET: &str = "my-mac.tailnet-name.ts.net";

    /// [`server_reading`] on a machine whose tailnet name is [`TAILNET`],
    /// keeping its devices in `home`'s `data/devices.json`.
    async fn server_on_tailnet(home: &std::path::Path) -> u16 {
        server_with(home, Some(TAILNET)).await
    }

    async fn server_with(home: &std::path::Path, tailnet: Option<&str>) -> u16 {
        let listener = bind(Some(0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let home = home.to_path_buf();
        let files = [
            home.join("data/issue-starts.json"),
            home.join("data/focus.json"),
            home.join("data/attachments"),
        ];
        let reach = Reach {
            tailnet: tailnet::Tailnet::fixed(tailnet),
            devices: devices::Devices::load(home.join("data/devices.json")),
        };
        let skills = skills::Cache::of(move |_| skills::Places {
            claude: home.join(".claude"),
            claude_managed: home.join("managed"),
            codex: home.join(".codex"),
            codex_admin: home.join("etc-codex"),
            cursor: home.join(".cursor"),
            projects: Vec::new(),
            added: Vec::new(),
            kept: home.join("data/skills-kept.json"),
            trash: Some(home.join("Trash")),
            home: home.clone(),
        });
        let app = router(
            Hub::down(),
            refresh::Refresher::new(),
            port,
            skills,
            files,
            reach,
        );
        tokio::spawn(async move { axum::serve(listener, app).await });
        port
    }

    /// The raw response to a request of `head` (its request line and
    /// headers, `\r\n`-separated, without the blank line ending them).
    async fn send(port: u16, head: &str) -> String {
        send_with(port, head, "").await
    }

    /// [`send`] with `body` after the head.
    async fn send_with(port: u16, head: &str, body: &str) -> String {
        send_bytes(port, head, body.as_bytes()).await
    }

    async fn send_bytes(port: u16, head: &str, body: &[u8]) -> String {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let request = format!(
            "{head}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        s.write_all(request.as_bytes()).await.unwrap();
        // A refusal may come, and the socket close, before the whole of a
        // large body is read.
        let _ = s.write_all(body).await;
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    fn status(response: &str) -> u16 {
        response.split_whitespace().nth(1).unwrap().parse().unwrap()
    }

    /// DNS rebinding: a page served by another name is refused, whatever
    /// it asks for; this server's own names at its own port are not.
    #[tokio::test]
    async fn a_request_for_another_host_is_refused() {
        let port = server().await;
        for host in [
            "evil.example".to_string(),
            format!("evil.example:{port}"),
            format!("127.0.0.1:{}", port.wrapping_add(1)),
            "127.0.0.1".to_string(),
            format!("192.168.1.10:{port}"),
        ] {
            let got = send(port, &format!("GET /api/state HTTP/1.1\r\nHost: {host}")).await;
            assert_eq!(status(&got), 403, "Host: {host}\n{got}");
            assert!(got.contains(r#"{"error":"#), "an ApiError: {got}");
        }
        for path in [
            "/api/skills",
            "/api/skills/usage",
            "/api/skills/0123456789abcdef",
            "/api/skills/compare/0123456789abcdef",
        ] {
            let got = send(
                port,
                &format!("GET {path} HTTP/1.1\r\nHost: evil.example:{port}"),
            )
            .await;
            assert_eq!(status(&got), 403, "{path}: {got}");
        }
        let got = send(port, "GET / HTTP/1.0").await;
        assert_eq!(status(&got), 403, "no Host at all: {got}");
        for host in [format!("127.0.0.1:{port}"), format!("LOCALHOST:{port}")] {
            let got = send(port, &format!("GET /api/state HTTP/1.1\r\nHost: {host}")).await;
            assert_eq!(status(&got), 503, "Host: {host}, past the guard: {got}");
        }
    }

    /// Anything but a read must come from this server's own page: every
    /// act it takes.
    #[tokio::test]
    async fn an_act_from_another_origin_is_refused() {
        let port = server().await;
        let post = |path: &str, origin: Option<String>| {
            let origin = origin.map_or(String::new(), |o| format!("\r\nOrigin: {o}"));
            format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}{origin}")
        };
        let acts = [
            "/api/sessions",
            "/api/sessions/a1/send",
            "/api/sessions/a1/answer",
            "/api/sessions/a1/show",
            "/api/sessions/a1/seen",
            "/api/orchestrator/send",
            "/api/skills/switch",
            "/api/skills/plugin",
            "/api/skills/fix",
            "/api/skills/undo",
            "/api/skills/check",
            "/api/skills/save",
            "/api/skills/delete",
        ];
        for path in acts {
            for origin in [
                None,
                Some("null".to_string()),
                Some("https://evil.example".to_string()),
                Some(format!("http://evil.example:{port}")),
                Some(format!("https://127.0.0.1:{port}")),
            ] {
                let got = send(port, &post(path, origin.clone())).await;
                assert_eq!(status(&got), 403, "{path}, Origin: {origin:?}\n{got}");
            }
        }
        for origin in [
            format!("http://127.0.0.1:{port}"),
            format!("http://localhost:{port}"),
        ] {
            let got = send(port, &post("/api/sessions/a1/seen", Some(origin.clone()))).await;
            assert_eq!(status(&got), 503, "Origin: {origin}, past the guard: {got}");
            assert!(got.ends_with(&format!(r#"{{"error":"{DOWN}"}}"#)), "{got}");
        }
        let read = format!(
            "GET /api/state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: https://evil.example"
        );
        assert_eq!(
            status(&send(port, &read).await),
            503,
            "a read is not refused"
        );
    }

    /// A body an act cannot take is refused with what is wrong with it,
    /// as an ApiError, before anything is asked of the daemon.
    #[tokio::test]
    async fn an_act_with_a_body_it_cannot_take_is_refused_saying_why() {
        let port = server().await;
        let head = |path: &str| {
            format!(
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: \
                 http://127.0.0.1:{port}\r\nContent-Type: application/json"
            )
        };
        for (path, body) in [
            ("/api/sessions", r#"{"text":"fix the login redirect"}"#),
            ("/api/sessions", r#"{"projectId":7,"text":"fix it"}"#),
            (
                "/api/sessions",
                r#"{"projectId":"p1","text":"look","images":["../../etc/passwd"]}"#,
            ),
            ("/api/sessions/a1/send", ""),
            ("/api/sessions/a1/send", r#"{"words":"hi"}"#),
            ("/api/orchestrator/send", r#"{"text":7}"#),
            ("/api/sessions/a1/answer", r#"{"option":-1}"#),
            ("/api/sessions/a1/answer", r#"{"option":"2"}"#),
            ("/api/skills/switch", r#"{"cli":"claude"}"#),
            ("/api/skills/switch", r#"{"cli":"vim","set":[]}"#),
            (
                "/api/skills/switch",
                r#"{"cli":"claude","set":[{"id":"a","state":"maybe"}]}"#,
            ),
            (
                "/api/skills/switch",
                r#"{"cli":"claude","set":[{"path":"/etc/passwd","state":"off"}]}"#,
            ),
            ("/api/skills/plugin", r#"{"plugin":"kit@market"}"#),
            ("/api/skills/fix", r#"{"fix":"0123456789abcdef"}"#),
            (
                "/api/skills/fix",
                r#"{"fix":"0123456789abcdef","act":"delete"}"#,
            ),
            (
                "/api/skills/fix",
                r#"{"path":"/etc/passwd","act":"removeLinks"}"#,
            ),
            ("/api/skills/undo", "{}"),
            ("/api/skills/check", r#"{"id":"0123456789abcdef"}"#),
            (
                "/api/skills/save",
                r#"{"id":"0123456789abcdef","text":"x"}"#,
            ),
            (
                "/api/skills/save",
                r#"{"path":"/etc/passwd","version":"0","text":"x"}"#,
            ),
            (
                "/api/skills/save",
                r#"{"id":"0123456789abcdef","version":"0","text":"\ud800"}"#,
            ),
        ] {
            let got = send_with(port, &head(path), body).await;
            assert_eq!(status(&got), 400, "{path} {body}\n{got}");
            let (_, json) = got.split_once("\r\n\r\n").unwrap();
            let error: serde_json::Value = serde_json::from_str(json).unwrap();
            let error = error["error"].as_str().unwrap();
            assert!(
                error.ends_with('.') && error.starts_with(char::is_uppercase),
                "{error}"
            );
        }
        for (path, body) in [
            ("/api/sessions/a1/send", r#"{"text":" \n "}"#),
            ("/api/sessions", r#"{"projectId":"p1","text":" \n "}"#),
        ] {
            let got = send_with(port, &head(path), body).await;
            assert_eq!(status(&got), 400, "{path}: {got}");
            assert!(
                got.ends_with(r#"{"error":"There is nothing to send."}"#),
                "{path}: {got}"
            );
        }
        // Text that can be typed goes as far as the daemon, which is down.
        let got = send_with(
            port,
            &head("/api/sessions"),
            r#"{"projectId":"p1","text":"fix it"}"#,
        )
        .await;
        assert_eq!(status(&got), 503, "{got}");
        let read = format!("GET /api/sessions/a1/turns HTTP/1.1\r\nHost: 127.0.0.1:{port}");
        let got = send(port, &read).await;
        assert_eq!(status(&got), 503, "{got}");
    }

    fn json_of(response: &str) -> serde_json::Value {
        let (_, body) = response.split_once("\r\n\r\n").unwrap();
        serde_json::from_str(body).unwrap()
    }

    /// The skills are read off disk, with no daemon up too: the list, and
    /// a skill's detail by the id the list gives it, its file read. An id
    /// the scan did not find names nothing, a path least of all. How often
    /// each was used is apart from them.
    #[tokio::test]
    async fn the_skills_answer_without_a_daemon_and_only_by_their_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(tmp.path()).unwrap();
        let folder = home.join(".claude/skills/deploy");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            "---\nname: deploy\ndescription: Ships it.\n---\nRun the deploy.\n",
        )
        .unwrap();
        let port = server_reading(&home).await;
        let get = |path: &str| format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}");

        let got = send(port, &get("/api/skills")).await;
        assert_eq!(status(&got), 200, "{got}");
        assert!(got.contains("application/json"), "{got}");
        let list = json_of(&got);
        assert_eq!(list["home"], home.display().to_string());
        assert_eq!(list["clis"], serde_json::json!(["claude"]));
        let skill = &list["skills"][0];
        assert_eq!(skill["name"], "deploy");
        assert_eq!(skill["kind"], "skill");
        assert_eq!(skill["source"], serde_json::json!({"kind": "folder"}));
        assert_eq!(skill["loads"][0]["scope"], "personal");

        let id = skill["id"].as_str().unwrap();
        let got = send(port, &get(&format!("/api/skills/{id}"))).await;
        assert_eq!(status(&got), 200, "{got}");
        let detail = json_of(&got);
        assert_eq!(detail["body"], "Run the deploy.\n");
        assert_eq!(
            detail["frontmatter"][1],
            serde_json::json!({"key": "description", "value": "Ships it."})
        );
        assert_eq!(detail["frontmatterError"], serde_json::json!(null));
        assert_eq!(
            detail["file"],
            folder.join("SKILL.md").display().to_string()
        );

        for id in ["nope", "..%2F..%2Fetc%2Fpasswd", "%2Fetc%2Fpasswd"] {
            let got = send(port, &get(&format!("/api/skills/{id}"))).await;
            assert_eq!(status(&got), 404, "{id}: {got}");
            assert!(
                json_of(&got)["error"]
                    .as_str()
                    .unwrap()
                    .starts_with("There is no skill"),
                "{got}"
            );
        }
        let got = send(port, &get("/api/skills/a/../../etc/passwd")).await;
        assert_ne!(status(&got), 200, "{got}");
        assert!(!got.contains("root:"), "{got}");

        let got = send(port, &get("/api/skills/usage")).await;
        assert_eq!(status(&got), 200, "{got}");
        assert_eq!(json_of(&got), serde_json::json!({"days": 30, "used": []}));
    }

    /// A switch is set through the server by the id the list gave the
    /// skill, in the file the scan says, and an id it did not give names
    /// nothing, so nothing is written.
    #[tokio::test]
    async fn a_switch_writes_the_setting_the_scan_found_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(tmp.path()).unwrap();
        let folder = home.join(".claude/skills/deploy");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            "---\nname: deploy\ndescription: Ships it.\n---\n",
        )
        .unwrap();
        let settings = home.join(".claude/settings.json");
        std::fs::write(&settings, "{\n  \"model\": \"x\"\n}\n").unwrap();
        let port = server_reading(&home).await;
        let list = json_of(
            &send(
                port,
                &format!("GET /api/skills HTTP/1.1\r\nHost: 127.0.0.1:{port}"),
            )
            .await,
        );
        let id = list["skills"][0]["id"].as_str().unwrap().to_string();
        assert_eq!(list["skills"][0]["controls"][0]["state"], "on");
        let post = |body: String| {
            let head = format!(
                "POST /api/skills/switch HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: \
                 http://127.0.0.1:{port}\r\nContent-Type: application/json"
            );
            async move { send_with(port, &head, &body).await }
        };

        let got = post(format!(
            r#"{{"cli":"claude","set":[{{"id":"{id}","state":"off"}}]}}"#
        ))
        .await;
        assert_eq!(status(&got), 200, "{got}");
        let written = json_of(&got);
        assert_eq!(written["file"], "~/.claude/settings.json");
        assert_eq!(written["changed"], true);
        assert_eq!(
            written["before"],
            serde_json::json!([{"id": id, "state": "on"}])
        );
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            "{\n  \"model\": \"x\",\n  \"skillOverrides\": {\n    \"deploy\": \"off\"\n  }\n}\n"
        );
        let list = json_of(
            &send(
                port,
                &format!("GET /api/skills HTTP/1.1\r\nHost: 127.0.0.1:{port}"),
            )
            .await,
        );
        assert_eq!(
            list["skills"][0]["controls"][0]["state"], "off",
            "the list is read again after a write"
        );

        let before = std::fs::read_to_string(&settings).unwrap();
        for id in ["0123456789abcdef", "../../etc/passwd"] {
            let got = post(format!(
                r#"{{"cli":"claude","set":[{{"id":"{id}","state":"on"}}]}}"#
            ))
            .await;
            assert_eq!(status(&got), 404, "{got}");
        }
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), before);
    }

    /// A fix is acted on through the server by the id the list gave it, and
    /// undone by the token the answer carried, once; an id or a token it did
    /// not give names nothing.
    #[tokio::test]
    async fn a_fix_is_made_and_undone_by_the_ids_the_server_gave() {
        let tmp = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(tmp.path()).unwrap();
        let link = home.join(".claude/skills/old");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(home.join("gone"), &link).unwrap();
        let port = server_reading(&home).await;
        let list = json_of(
            &send(
                port,
                &format!("GET /api/skills HTTP/1.1\r\nHost: 127.0.0.1:{port}"),
            )
            .await,
        );
        let fix = &list["fixes"][0];
        assert_eq!(fix["acts"][0]["act"], "removeLinks");
        assert_eq!(list["kept"], serde_json::json!([]));
        let id = fix["id"].as_str().unwrap().to_string();
        let post = |path: &str, body: String| {
            let head = format!(
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: \
                 http://127.0.0.1:{port}\r\nContent-Type: application/json"
            );
            async move { send_with(port, &head, &body).await }
        };

        let got = post(
            "/api/skills/fix",
            r#"{"fix":"0123456789abcdef","act":"removeLinks"}"#.into(),
        )
        .await;
        assert_eq!(status(&got), 404, "{got}");
        assert!(link.symlink_metadata().is_ok());

        let got = post(
            "/api/skills/fix",
            format!(r#"{{"fix":"{id}","act":"removeLinks"}}"#),
        )
        .await;
        assert_eq!(status(&got), 200, "{got}");
        let done = json_of(&got);
        assert_eq!(done["said"], "Removed 1 link from ~/.claude/skills.");
        assert!(link.symlink_metadata().is_err());
        let list = json_of(
            &send(
                port,
                &format!("GET /api/skills HTTP/1.1\r\nHost: 127.0.0.1:{port}"),
            )
            .await,
        );
        assert_eq!(
            list["fixes"],
            serde_json::json!([]),
            "read again after an act"
        );

        let undo = done["undo"].as_str().unwrap().to_string();
        let again = || post("/api/skills/undo", format!(r#"{{"undo":"{undo}"}}"#));
        let got = again().await;
        assert_eq!(status(&got), 200, "{got}");
        assert_eq!(std::fs::read_link(&link).unwrap(), home.join("gone"));
        assert_eq!(json_of(&got)["undo"], serde_json::json!(null));
        let got = again().await;
        assert_eq!(status(&got), 404, "only once: {got}");
        let got = post("/api/skills/undo", r#"{"undo":"0.1"}"#.into()).await;
        assert_eq!(status(&got), 404, "{got}");
    }

    /// A skill's file is saved through the server by the id the list gave
    /// the skill and the version its detail gave the text, and put back by
    /// the token the answer carried; an id it did not give, or a version
    /// the file is not at now, writes nothing.
    #[tokio::test]
    async fn a_save_writes_the_file_the_scan_found_at_the_version_read() {
        let tmp = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(tmp.path()).unwrap();
        let file = home.join(".claude/skills/deploy/SKILL.md");
        let before = "---\nname: deploy\ndescription: Ships it.\n---\n";
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, before).unwrap();
        let port = server_reading(&home).await;
        let get = |path: String| format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}");
        let list = json_of(&send(port, &get("/api/skills".into())).await);
        let id = list["skills"][0]["id"].as_str().unwrap().to_string();
        let detail = json_of(&send(port, &get(format!("/api/skills/{id}"))).await);
        assert_eq!(detail["edit"]["text"], before);
        assert_eq!(detail["uneditable"], serde_json::json!(null));
        let version = detail["edit"]["version"].as_str().unwrap().to_string();
        let post = |path: &str, body: serde_json::Value| {
            let head = format!(
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: \
                 http://127.0.0.1:{port}\r\nContent-Type: application/json"
            );
            async move { send_with(port, &head, &body.to_string()).await }
        };

        let got = post(
            "/api/skills/check",
            serde_json::json!({"id": id, "text": "---\nname: [x\n---\nHi.\n"}),
        )
        .await;
        assert_eq!(status(&got), 200, "{got}");
        let checked = json_of(&got);
        assert_eq!(checked["problems"][0]["kind"], "badFrontmatter");
        assert_eq!(checked["body"], "Hi.\n");

        for (to, want) in [
            (
                serde_json::json!({"id": "0123456789abcdef", "version": version, "text": "x"}),
                404,
            ),
            (
                serde_json::json!({"id": id, "version": "0000000000000000", "text": "x"}),
                409,
            ),
        ] {
            let got = post("/api/skills/save", to).await;
            assert_eq!(status(&got), want, "{got}");
            assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
        }
        let got = post(
            "/api/skills/save",
            serde_json::json!({"id": id, "version": version, "text": "Mine.\r\n"}),
        )
        .await;
        assert_eq!(status(&got), 200, "{got}");
        let done = json_of(&got);
        assert_eq!(done["said"], "Saved ~/.claude/skills/deploy/SKILL.md.");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "Mine.\r\n");
        let detail = json_of(&send(port, &get(format!("/api/skills/{id}"))).await);
        assert_eq!(detail["body"], "Mine.\r\n", "read again after a save");

        let undo = done["undo"].as_str().unwrap().to_string();
        let got = post("/api/skills/undo", serde_json::json!({"undo": undo})).await;
        assert_eq!(status(&got), 200, "{got}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    }

    /// A skill is deleted through the server by the id the list gave it and
    /// the version its detail gave what goes, into the Trash, and put back
    /// by the token the answer carried; a version that is not now's moves
    /// nothing.
    #[tokio::test]
    async fn a_delete_moves_the_skill_to_the_trash_at_the_version_read() {
        let tmp = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(tmp.path()).unwrap();
        let folder = home.join(".claude/skills/deploy");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("SKILL.md"), "---\nname: deploy\n---\n").unwrap();
        let port = server_reading(&home).await;
        let get = |path: String| format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}");
        let list = json_of(&send(port, &get("/api/skills".into())).await);
        let id = list["skills"][0]["id"].as_str().unwrap().to_string();
        let detail = json_of(&send(port, &get(format!("/api/skills/{id}"))).await);
        assert_eq!(
            detail["delete"]["goes"][0]["path"],
            folder.display().to_string()
        );
        assert_eq!(detail["undeletable"], serde_json::json!(null));
        let version = detail["delete"]["version"].as_str().unwrap().to_string();
        let post = |path: &str, body: serde_json::Value| {
            let head = format!(
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: \
                 http://127.0.0.1:{port}\r\nContent-Type: application/json"
            );
            async move { send_with(port, &head, &body.to_string()).await }
        };

        let got = post(
            "/api/skills/delete",
            serde_json::json!({"id": id, "version": "0000000000000000"}),
        )
        .await;
        assert_eq!(status(&got), 409, "{got}");
        assert!(folder.join("SKILL.md").is_file());
        let got = post(
            "/api/skills/delete",
            serde_json::json!({"id": id, "version": version}),
        )
        .await;
        assert_eq!(status(&got), 200, "{got}");
        let done = json_of(&got);
        assert_eq!(done["said"], "Deleted deploy: its folder is in the Trash.");
        assert!(!folder.exists());
        assert!(home.join("Trash/files/deploy/SKILL.md").is_file());
        let list = json_of(&send(port, &get("/api/skills".into())).await);
        assert_eq!(
            list["skills"],
            serde_json::json!([]),
            "read again after a delete"
        );

        let undo = done["undo"].as_str().unwrap().to_string();
        let got = post("/api/skills/undo", serde_json::json!({"undo": undo})).await;
        assert_eq!(status(&got), 200, "{got}");
        assert!(folder.join("SKILL.md").is_file());
    }

    /// Reading the skills is a read: another origin is refused anything
    /// else, and the page's own has nothing else to ask of the list.
    #[tokio::test]
    async fn the_skills_take_only_reads() {
        let port = server().await;
        let post = |origin: &str| {
            format!("POST /api/skills HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}")
        };
        let got = send(port, &post("https://evil.example")).await;
        assert_eq!(status(&got), 403, "{got}");
        let got = send(port, &post(&format!("http://127.0.0.1:{port}"))).await;
        assert_eq!(status(&got), 405, "{got}");
        let got = send(
            port,
            &format!("GET /api/skills HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: https://evil.example"),
        )
        .await;
        assert_eq!(
            status(&got),
            200,
            "a read from anywhere is answered, with no CORS header: {got}"
        );
        let headers = got.split("\r\n\r\n").next().unwrap().to_ascii_lowercase();
        assert!(!headers.contains("access-control-"), "{got}");
    }

    #[test]
    fn a_daemon_message_reads_as_a_sentence() {
        assert_eq!(
            sentence("session a1 took only part"),
            "Session a1 took only part."
        );
        assert_eq!(sentence(" it quit? "), "It quit?");
    }

    /// Every response names the server, a refusal's too, so a port file
    /// naming this one's port finds it, and not another program's.
    #[tokio::test]
    async fn every_response_names_the_server() {
        let port = server().await;
        let got = send(port, "GET /api/state HTTP/1.1\r\nHost: evil.example").await;
        assert!(
            got.to_ascii_lowercase()
                .contains("\r\nserver: nebula serve\r\n"),
            "{got}"
        );
        let dir = std::env::temp_dir().join(format!("nebula-web-port-{port}"));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("serve.port");
        std::fs::write(&file, format!("{port}\n")).unwrap();
        let found = tokio::task::spawn_blocking(move || nebula_client::serve::serving_port(&file));
        assert_eq!(found.await.unwrap(), Some(port));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// No response says another origin may read it, a preflight's least of
    /// all.
    #[tokio::test]
    async fn no_response_carries_a_cors_header() {
        let port = server().await;
        for head in [
            "OPTIONS /api/state HTTP/1.1\r\nOrigin: https://evil.example\r\n\
             Access-Control-Request-Method: GET",
            "GET /api/state HTTP/1.1\r\nOrigin: https://evil.example",
            "GET /s/a1 HTTP/1.1\r\nOrigin: https://evil.example",
        ] {
            let got = send(port, &format!("{head}\r\nHost: 127.0.0.1:{port}")).await;
            let headers = got.split("\r\n\r\n").next().unwrap().to_ascii_lowercase();
            assert!(!headers.contains("access-control-"), "{head}\n{got}");
        }
    }

    /// Every path the API does not have serves the app's page, a deep link
    /// included, and an API path it does not have is a JSON 404.
    #[tokio::test]
    async fn a_deep_link_serves_the_app_and_a_missing_endpoint_is_json() {
        let port = server().await;
        let host = format!("\r\nHost: 127.0.0.1:{port}");
        let got = send(port, &format!("GET /s/01ABC HTTP/1.1{host}")).await;
        assert_eq!(status(&got), 200, "{got}");
        assert!(got.contains("text/html"), "{got}");
        assert!(
            got.ends_with(std::str::from_utf8(index_html()).unwrap()),
            "{got}"
        );
        let got = send(port, &format!("GET /api/nothing HTTP/1.1{host}")).await;
        assert_eq!(status(&got), 404, "{got}");
        assert!(got.ends_with(r#"{"error":"no such endpoint"}"#), "{got}");
        let got = send(port, &format!("GET /api/sessions/a1/send HTTP/1.1{host}")).await;
        assert_eq!(status(&got), 405, "{got}");
    }

    /// The head of a request `tailscale serve` passes on from a device of
    /// the Tailscale user `login`: `line`, then `more` headers.
    fn served(line: &str, login: &str, more: &str) -> String {
        format!(
            "{line}\r\nHost: {TAILNET}\r\nX-Forwarded-For: 100.101.102.103\r\n\
             X-Forwarded-Host: {TAILNET}\r\nX-Forwarded-Proto: https\r\n\
             Tailscale-User-Login: {login}{more}"
        )
    }

    fn header_of<'a>(response: &'a str, name: &str) -> Option<&'a str> {
        let head = response.split("\r\n\r\n").next().unwrap();
        head.split("\r\n").find_map(|line| {
            let (k, v) = line.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    /// A device on the tailnet gets the page that pairs it and nothing
    /// else, until the code the Mac's page shows pairs it, once; its cookie
    /// then lets it in as the user who paired it and no other, until the
    /// Mac removes it. This machine's own page needs none of it.
    #[tokio::test]
    async fn a_device_on_the_tailnet_is_let_in_once_paired_and_only_as_its_user() {
        let home = tempfile::tempdir().unwrap();
        let port = server_on_tailnet(home.path()).await;
        let (ada, eve) = ("ada@example.com", "eve@example.com");
        let origin = format!("\r\nOrigin: https://{TAILNET}");
        let get = |path: &str, login: &str, more: &str| {
            served(&format!("GET {path} HTTP/1.1"), login, more)
        };

        for path in ["/", "/s/a1", "/i/ACME-1", "/skills", "/assets/index.js"] {
            let got = send(port, &get(path, ada, "")).await;
            assert_eq!(status(&got), 401, "{path}: {got}");
            assert!(got.contains("<h1>Pair this device</h1>"), "{path}: {got}");
            assert!(
                got.contains("Signed in to Tailscale as ada@example.com."),
                "{got}"
            );
            assert!(
                !got.ends_with(std::str::from_utf8(index_html()).unwrap()),
                "{path}"
            );
        }
        for path in [
            "/api/state",
            "/api/events",
            "/api/skills",
            "/api/devices",
            "/api/sessions/a1",
        ] {
            let got = send(port, &get(path, ada, "")).await;
            assert_eq!(status(&got), 401, "{path}: {got}");
            assert_eq!(
                json_of(&got)["error"],
                "Pair this device from nebula on your Mac."
            );
        }
        for path in ["/api/sessions/a1/seen", "/api/devices/pair", "/api/refresh"] {
            let got = send(
                port,
                &served(&format!("POST {path} HTTP/1.1"), ada, &origin),
            )
            .await;
            assert_eq!(status(&got), 401, "{path}: {got}");
        }
        let got = send(port, &get("/manifest.webmanifest", ada, "")).await;
        assert_eq!(status(&got), 200, "the files that install the app: {got}");

        // The Mac's page opens a pairing; a device cannot.
        let mac = format!("\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}");
        let opened = send(port, &format!("POST /api/devices/pair HTTP/1.1{mac}")).await;
        assert_eq!(status(&opened), 200, "{opened}");
        let pairing = json_of(&opened)["pairing"].clone();
        let url = pairing["url"].as_str().unwrap();
        let code = url
            .strip_prefix(&format!("https://{TAILNET}/pair/"))
            .unwrap()
            .to_string();
        assert_eq!(pairing["code"].as_str().unwrap().replace('-', ""), code);

        let link = send(port, &get(&format!("/pair/{code}"), ada, "")).await;
        assert_eq!(status(&link), 200, "{link}");
        assert!(
            link.contains(&format!(
                r#"<input type="hidden" name="code" value="{code}">"#
            )),
            "{link}"
        );
        assert_eq!(header_of(&link, "cache-control"), Some("no-store"));

        let pair = |code: &str, login: &str, origin: &str, more: &str| {
            let body = format!("code={code}&name=Ada%27s+iPhone");
            let head = served(
                "POST /pair HTTP/1.1",
                login,
                &format!("{origin}\r\nContent-Type: application/x-www-form-urlencoded{more}"),
            );
            (head, body)
        };
        let (head, body) = pair(&code, ada, "\r\nOrigin: https://evil.example", "");
        assert_eq!(
            status(&send_with(port, &head, &body).await),
            403,
            "another origin"
        );
        let (head, body) = pair("2222-2222", ada, &origin, "");
        let wrong = send_with(port, &head, &body).await;
        assert_eq!(status(&wrong), 403, "{wrong}");
        assert!(
            wrong.contains("That is not the code your Mac shows."),
            "{wrong}"
        );

        let (head, body) = pair(&code, ada, &origin, "");
        let paired = send_with(port, &head, &body).await;
        assert_eq!(status(&paired), 303, "{paired}");
        assert_eq!(header_of(&paired, "location"), Some("/"));
        let cookie = header_of(&paired, "set-cookie").unwrap().to_string();
        for part in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
            assert!(cookie.contains(part), "{cookie}");
        }
        let token = cookie
            .strip_prefix("nebula_device=")
            .and_then(|c| c.split(';').next())
            .unwrap()
            .to_string();

        let (head, body) = pair(&code, ada, &origin, "");
        let again = send_with(port, &head, &body).await;
        assert_eq!(status(&again), 403, "the code pairs once: {again}");
        assert_eq!(
            status(&send(port, &get(&format!("/pair/{code}"), ada, "")).await),
            410
        );

        let with = format!("\r\nCookie: nebula_device={token}");
        let got = send(port, &get("/api/state", ada, &with)).await;
        assert_eq!(status(&got), 503, "past the guard, no daemon: {got}");
        let got = send(port, &get("/s/a1", ada, &with)).await;
        assert_eq!(status(&got), 200, "{got}");
        assert!(
            got.ends_with(std::str::from_utf8(index_html()).unwrap()),
            "the app"
        );
        assert_eq!(header_of(&got, "set-cookie"), None, "not a page load");
        let load = format!("{with}\r\nSec-Fetch-Dest: document");
        let got = send(port, &get("/s/a1", ada, &load)).await;
        assert_eq!(status(&got), 200, "{got}");
        assert_eq!(
            header_of(&got, "set-cookie"),
            Some(cookie.as_str()),
            "a page load sets the cookie again, as it is now"
        );
        let seen = served(
            "POST /api/sessions/a1/seen HTTP/1.1",
            ada,
            &format!("{origin}{with}"),
        );
        assert_eq!(status(&send(port, &seen).await), 503);
        for act in [
            "POST /api/sessions HTTP/1.1",
            "POST /api/sessions/a1/seen HTTP/1.1",
        ] {
            let from_elsewhere =
                served(act, ada, &format!("\r\nOrigin: https://evil.example{with}"));
            assert_eq!(
                status(&send(port, &from_elsewhere).await),
                403,
                "the Lax cookie goes with another site's form, the guard refuses it: {act}"
            );
        }
        let got = send(port, &get("/api/devices", ada, &with)).await;
        assert_eq!(
            status(&got),
            403,
            "the Mac alone lists and pairs devices: {got}"
        );

        let got = send(port, &get("/api/state", eve, &with)).await;
        assert_eq!(status(&got), 403, "{got}");
        assert!(json_of(&got)["error"]
            .as_str()
            .unwrap()
            .contains("another Tailscale user"));
        let got = send(port, &get("/", eve, &with)).await;
        assert_eq!(status(&got), 403, "{got}");
        assert!(got.contains("<h1>Pair this device</h1>"), "{got}");

        let listed = send(port, &format!("GET /api/devices HTTP/1.1{mac}")).await;
        let listed = json_of(&listed);
        assert!(listed["pairing"].is_null(), "{listed}");
        let device = &listed["devices"][0];
        assert_eq!(
            (device["name"].as_str(), device["login"].as_str()),
            (Some("Ada's iPhone"), Some(ada))
        );
        let file = std::fs::read_to_string(home.path().join("data/devices.json")).unwrap();
        assert!(!file.contains(&token), "only a hash on disk: {file}");

        let id = device["id"].as_str().unwrap();
        let removed = send(
            port,
            &format!("POST /api/devices/{id}/remove HTTP/1.1{mac}"),
        )
        .await;
        assert_eq!(status(&removed), 200, "{removed}");
        assert_eq!(json_of(&removed)["devices"], serde_json::json!([]));
        let got = send(port, &get("/api/state", ada, &with)).await;
        assert_eq!(status(&got), 401, "removed: {got}");
    }

    /// Pair a device of `login` through the server on `port`, as the Mac's
    /// page and the device's would: the cookie it is let in with, and its id.
    async fn paired(port: u16, login: &str) -> (String, String) {
        let mac = format!("\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}");
        let opened = json_of(&send(port, &format!("POST /api/devices/pair HTTP/1.1{mac}")).await);
        let code = opened["pairing"]["code"].as_str().unwrap().to_string();
        let body = format!("code={code}&name=iPhone");
        let head = served(
            "POST /pair HTTP/1.1",
            login,
            &format!(
                "\r\nOrigin: https://{TAILNET}\r\nContent-Type: application/x-www-form-urlencoded"
            ),
        );
        let paired = send_with(port, &head, &body).await;
        let cookie = header_of(&paired, "set-cookie").unwrap();
        let cookie = cookie.split(';').next().unwrap().to_string();
        let listed = json_of(&send(port, &format!("GET /api/devices HTTP/1.1{mac}")).await);
        let id = listed["devices"][0]["id"].as_str().unwrap().to_string();
        (format!("\r\nCookie: {cookie}"), id)
    }

    /// A paired device turns notifications on with the key the server
    /// makes once, keeps one subscription beside it, readable by the user
    /// alone, and turns them off; the Mac's page sees which devices have
    /// them on and sends a test, and removing a device drops its
    /// subscription. None of it is for the Mac's own page or for a device
    /// not paired.
    #[tokio::test]
    async fn a_paired_device_turns_notifications_on_and_the_mac_tests_them() {
        let home = tempfile::tempdir().unwrap();
        let port = server_on_tailnet(home.path()).await;
        let ada = "ada@example.com";
        let mac = format!("\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}");
        let origin = format!("\r\nOrigin: https://{TAILNET}");
        let json = "\r\nContent-Type: application/json";

        let got = send(port, &format!("GET /api/push HTTP/1.1{mac}")).await;
        assert_eq!(status(&got), 404, "not for the Mac's own page: {got}");
        let got = send(port, &served("GET /api/push HTTP/1.1", ada, "")).await;
        assert_eq!(status(&got), 401, "not before pairing: {got}");

        let (with, id) = paired(port, ada).await;
        let got = send(port, &served("GET /api/push HTTP/1.1", ada, &with)).await;
        assert_eq!(status(&got), 200, "{got}");
        let push = json_of(&got);
        assert_eq!(push["on"], false);
        let key = push["key"].as_str().unwrap().to_string();
        assert_eq!(key.len(), 87, "65 bytes, base64url: {key}");
        let again = json_of(&send(port, &served("GET /api/push HTTP/1.1", ada, &with)).await);
        assert_eq!(again["key"], key.as_str(), "made once");

        let subscribe = |body: String| {
            let head = served(
                "POST /api/push/subscribe HTTP/1.1",
                ada,
                &format!("{origin}{json}{with}"),
            );
            async move { send_with(port, &head, &body).await }
        };
        let p256dh = "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
        let auth = "BTBZMqHH6r4Tts7J_aSIgg";
        let sub = |endpoint: &str| {
            serde_json::json!({"endpoint": endpoint, "expirationTime": null, "keys": {"p256dh": p256dh, "auth": auth}})
                .to_string()
        };
        for bad in [
            sub("http://127.0.0.1:7682/api/state"),
            sub("file:///etc/passwd"),
            serde_json::json!({"endpoint": "https://fcm.googleapis.com/x", "keys": {"p256dh": "AAAA", "auth": auth}}).to_string(),
            "{}".into(),
        ] {
            let got = subscribe(bad.clone()).await;
            assert_eq!(status(&got), 400, "{bad}\n{got}");
        }
        // An endpoint nothing listens on, so a test cannot reach it.
        let got = subscribe(sub("https://127.0.0.1:9/push/abc")).await;
        assert_eq!(status(&got), 200, "{got}");
        assert_eq!(json_of(&got)["on"], true);
        let kept: Vec<_> = std::fs::read_dir(home.path().join("data/push"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(kept.len(), 1, "{kept:?}");
        use std::os::unix::fs::PermissionsExt;
        for file in [&kept[0], &home.path().join("data/vapid.json")] {
            let mode = std::fs::metadata(file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{}", file.display());
        }
        let got = send(
            port,
            &served(
                "POST /api/push/subscribe HTTP/1.1",
                ada,
                &format!("\r\nOrigin: https://evil.example{json}{with}"),
            ),
        )
        .await;
        assert_eq!(status(&got), 403, "another origin: {got}");

        let listed = json_of(&send(port, &format!("GET /api/devices HTTP/1.1{mac}")).await);
        assert_eq!(listed["devices"][0]["push"], true);
        let test = |id: &str| format!("POST /api/devices/{id}/test HTTP/1.1{mac}");
        let got = send(port, &test(&id)).await;
        assert_eq!(status(&got), 502, "{got}");
        assert!(
            json_of(&got)["error"]
                .as_str()
                .unwrap()
                .starts_with("The test did not reach iPhone: "),
            "{got}"
        );
        let got = send(port, &test("0000000000000000")).await;
        assert_eq!(status(&got), 404, "{got}");
        let got = send(
            port,
            &served(
                &format!("POST /api/devices/{id}/test HTTP/1.1"),
                ada,
                &format!("{origin}{with}"),
            ),
        )
        .await;
        assert_eq!(status(&got), 403, "the Mac alone sends a test: {got}");

        let off = served(
            "POST /api/push/unsubscribe HTTP/1.1",
            ada,
            &format!("{origin}{with}"),
        );
        let got = send(port, &off).await;
        assert_eq!(json_of(&got)["on"], false, "{got}");
        let got = send(port, &test(&id)).await;
        assert_eq!(status(&got), 409, "{got}");
        let listed = json_of(&send(port, &format!("GET /api/devices HTTP/1.1{mac}")).await);
        assert_eq!(listed["devices"][0]["push"], false);

        assert_eq!(
            status(&subscribe(sub("https://127.0.0.1:9/push/abc")).await),
            200
        );
        let removed = send(
            port,
            &format!("POST /api/devices/{id}/remove HTTP/1.1{mac}"),
        )
        .await;
        assert_eq!(status(&removed), 200, "{removed}");
        let left = std::fs::read_dir(home.path().join("data/push"))
            .unwrap()
            .count();
        assert_eq!(left, 0, "removing a device drops its subscription");
    }

    /// An image for a follow-up comes from a paired device or the Mac's own
    /// page, never from a device not paired: it is kept in the data dir's
    /// `attachments/` for the user alone, and only an id the server gave
    /// names it in a send.
    #[tokio::test]
    async fn only_a_paired_device_or_the_mac_keeps_an_image() {
        const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR pixels";
        let home = tempfile::tempdir().unwrap();
        let port = server_on_tailnet(home.path()).await;
        let kept = home.path().join("data/attachments");
        let post = |login: &str, more: &str| {
            served(
                "POST /api/images HTTP/1.1",
                login,
                &format!("\r\nOrigin: https://{TAILNET}\r\nContent-Type: image/png{more}"),
            )
        };
        let ada = "ada@example.com";
        let got = send_bytes(port, &post(ada, ""), PNG).await;
        assert_eq!(status(&got), 401, "not paired: {got}");
        assert!(!kept.exists(), "nothing is kept for a device not paired");

        let (cookie, _) = paired(port, ada).await;
        let got = send_bytes(port, &post(ada, &cookie), PNG).await;
        assert_eq!(status(&got), 201, "{got}");
        let id = json_of(&got)["id"].as_str().unwrap().to_string();
        let file = kept.join(&id);
        assert_eq!(std::fs::read(&file).unwrap(), PNG);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let got = send_bytes(port, &post("eve@example.com", &cookie), PNG).await;
        assert_eq!(status(&got), 403, "another user's: {got}");

        // The type is read from the bytes, never the Content-Type.
        let got = send_bytes(port, &post(ada, &cookie), b"<svg onload=alert(1)/>").await;
        assert_eq!(status(&got), 415, "{got}");
        assert!(json_of(&got)["error"]
            .as_str()
            .unwrap()
            .contains("not an image"));
        let big = vec![0u8; images::MAX_BYTES + 1];
        let got = send_bytes(port, &post(ada, &cookie), &big).await;
        assert_eq!(status(&got), 413, "{got}");
        assert!(
            json_of(&got)["error"].as_str().unwrap().contains("20 MB"),
            "{got}"
        );

        let mac = format!("\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}");
        let got = send_bytes(port, &format!("POST /api/images HTTP/1.1{mac}"), PNG).await;
        assert_eq!(status(&got), 201, "the Mac's own page: {got}");
        let got = send_bytes(
            port,
            &format!("POST /api/images HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://evil.example"),
            PNG,
        )
        .await;
        assert_eq!(status(&got), 403, "another site's page: {got}");

        // A send names an image by the id it was given, and nothing else.
        let follow =
            |images: &str| format!(r#"{{"text":"What does this show?","images":{images}}}"#);
        let head =
            format!("POST /api/sessions/a1/send HTTP/1.1{mac}\r\nContent-Type: application/json");
        let got = send_with(port, &head, &follow(r#"["../devices.json"]"#)).await;
        assert_eq!(status(&got), 400, "{got}");
        let got = send_with(port, &head, &follow(&format!(r#"["{id}"]"#))).await;
        assert_eq!(
            status(&got),
            503,
            "past the images, on to the daemon: {got}"
        );
    }

    /// The Mac's own page is as it was with a tailnet name too: no cookie,
    /// no pairing; and a pairing's page is not for it.
    #[tokio::test]
    async fn this_machine_needs_no_pairing() {
        let home = tempfile::tempdir().unwrap();
        let port = server_on_tailnet(home.path()).await;
        let host = format!("\r\nHost: 127.0.0.1:{port}");
        assert_eq!(
            status(&send(port, &format!("GET /api/state HTTP/1.1{host}")).await),
            503
        );
        let got = send(port, &format!("GET / HTTP/1.1{host}")).await;
        assert!(
            got.ends_with(std::str::from_utf8(index_html()).unwrap()),
            "{got}"
        );
        let got = send(port, &format!("GET /api/devices HTTP/1.1{host}")).await;
        assert_eq!(json_of(&got)["tailnet"], TAILNET);
        let got = send(port, &format!("GET /pair/ABCDEFGH HTTP/1.1{host}")).await;
        assert_eq!(status(&got), 404, "{got}");
        // The Mac with no tailnet name opens no pairing.
        let port = server().await;
        let mac = format!("\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}");
        let got = send(port, &format!("POST /api/devices/pair HTTP/1.1{mac}")).await;
        assert_eq!(status(&got), 409, "{got}");
    }
}
