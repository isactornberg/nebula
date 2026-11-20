//! A Claude session's conversation, turn by turn, from its transcript: the
//! thread the web app shows (`ClientRequest::ReadTurns`).
//!
//! A user turn is the prompt as its user typed it: a slash command as its
//! name and arguments, an image the short `[Image #n]` Claude Code shows,
//! and none of what the CLI wraps around a prompt or writes as one (system
//! reminders, a command's output, a tool's result, a background task's
//! notification). A prompt typed while Claude works is a turn where Claude
//! took it in. An assistant turn is what its user read for that turn, by
//! the rule a report is read by: the text of the turn's newest assistant
//! message, all its text blocks, never a tool call. Only the transcript's
//! user and assistant lines are read for them, never the copies of a
//! request's messages Claude Code also writes there.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use nebula_core::{AgentId, Turn, TurnRole};
use parking_lot::Mutex;
use serde_json::Value;

use crate::registry::Daemon;
use crate::session_context::{
    assistant_texts, last_bytes, lines_back, ASSISTANT_LINE, QUEUED_LINE, USER_LINE,
};

/// The most turns a read gives.
pub const MAX_TURNS: usize = 40;
/// The most bytes of text the turns of a read hold. The newest is always
/// given, its end alone when it is longer, as a report is.
pub const MAX_TURN_BYTES: usize = 256 * 1024;
/// How many transcripts' last reads are kept ([`READS`]).
const READS_KEPT: usize = 8;

/// A read's turns, and whether the transcript holds older ones.
type TurnsRead = (Vec<Turn>, bool);
/// A transcript as it stood: its path, length and modification time.
type Stamp = (PathBuf, u64, SystemTime);

/// The last reads of transcripts' turns, newest last, each with how its
/// file stood: one unchanged since is not read again.
static READS: Mutex<Vec<(Stamp, TurnsRead)>> = Mutex::new(Vec::new());

/// What is cut out of a prompt's text: what the CLI wraps around it, and
/// what it writes in the user's place.
const NOT_TYPED: &[&str] = &[
    "system-reminder",
    "local-command-stdout",
    "local-command-stderr",
    "local-command-caveat",
    "bash-stdout",
    "bash-stderr",
    "user-prompt-submit-hook",
];

impl Daemon {
    /// `ClientRequest::ReadTurns`: Claude session `id`'s last turns
    /// ([`turns_in`]), and whether its transcript holds older ones.
    pub fn read_turns(&self, id: &AgentId) -> Result<(Vec<Turn>, bool)> {
        let path = self
            .agent_transcript_path(id)
            .context("only a Claude session has turns to read")?;
        turns_in(&path).context("its transcript could not be read")
    }
}

/// The last turns of the Claude transcript at `path`, at most
/// [`MAX_TURNS`] and [`MAX_TURN_BYTES`] of them, oldest first, and whether
/// it holds older ones than those, read or not. `None` when it cannot be
/// read.
fn turns_in(path: &Path) -> Option<TurnsRead> {
    let meta = std::fs::metadata(path).ok()?;
    let stamp = (path.to_path_buf(), meta.len(), meta.modified().ok()?);
    let known = READS
        .lock()
        .iter()
        .find(|(stood, _)| *stood == stamp)
        .map(|(_, read)| read.clone());
    if known.is_some() {
        return known;
    }
    let mut thread = Thread::default();
    let left = lines_back(path, 0, &[USER_LINE, ASSISTANT_LINE, QUEUED_LINE], |line| {
        thread.take(line)
    })?;
    let (turns, dropped) = newest(thread.done());
    let read = (turns, left || dropped);
    let mut reads = READS.lock();
    reads.retain(|((stood, ..), _)| stood != path);
    reads.push((stamp, read.clone()));
    if reads.len() > READS_KEPT {
        reads.remove(0);
    }
    Some(read)
}

/// The newest of `all` that fit, and whether any were left out.
fn newest(mut all: Vec<Turn>) -> (Vec<Turn>, bool) {
    let mut bytes = 0;
    let fit = all
        .iter()
        .rev()
        .take(MAX_TURNS)
        .take_while(|t| {
            bytes += t.text.len();
            bytes <= MAX_TURN_BYTES
        })
        .count()
        .max(all.len().min(1));
    let more = fit < all.len();
    let mut turns = all.split_off(all.len() - fit);
    if let Some(last) = turns.last_mut() {
        last.text = last_bytes(&last.text, MAX_TURN_BYTES).to_string();
    }
    (turns, more)
}

/// What a user line of a transcript is to the thread.
#[derive(Debug, PartialEq)]
enum Said {
    /// A prompt its user typed, which starts a turn.
    Prompt(String),
    /// One the CLI injected into its own turn queue: it starts a turn of
    /// the model's, but nobody typed it.
    Injected,
    /// Nothing anyone typed, within the turn under way.
    Nothing,
}

/// What a line of a transcript is to the thread.
enum Line {
    /// What its user said, when, and whether as a prompt queued while
    /// Claude worked.
    Said(Said, Option<i64>, bool),
    /// An assistant message's text blocks on this line: its id and when.
    Reply(Option<String>, Vec<String>, Option<i64>),
}

/// What the transcript line `raw` is to the thread, if anything: a user
/// or an assistant line, or a prompt queued while Claude worked, never a
/// subagent's, a skill's body or a compaction's summary.
fn line_of(raw: &str) -> Option<Line> {
    let value = serde_json::from_str::<Value>(raw).ok()?;
    let aside = ["isSidechain", "isMeta", "isCompactSummary"]
        .iter()
        .any(|key| value.get(key).and_then(Value::as_bool) == Some(true));
    if aside {
        return None;
    }
    let at = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(nebula_core::clock::utc_ms);
    let said = match value.get("type")?.as_str()? {
        "assistant" => {
            let message = value.get("message")?;
            let texts = assistant_texts(message);
            if texts.iter().all(|t| t.trim().is_empty()) {
                return None;
            }
            let id = message.get("id").and_then(Value::as_str).map(String::from);
            return Some(Line::Reply(id, texts, at));
        }
        "user" => Line::Said(said(value.get("message")?.get("content")), at, false),
        "attachment" => {
            let queued = value
                .get("attachment")
                .filter(|a| a.get("type").and_then(Value::as_str) == Some("queued_command"))?;
            let said = match queued.get("commandMode").and_then(Value::as_str) {
                Some("task-notification") => Said::Injected,
                _ => said(queued.get("prompt")),
            };
            Line::Said(said, at, true)
        }
        _ => return None,
    };
    Some(said)
}

/// A turn's newest assistant message, as read back to it: its id, its
/// text blocks and when the last of them was written, and whether an older
/// message of the turn has been met, which leaves it whole.
struct Reply {
    id: Option<String>,
    parts: Vec<String>,
    at: Option<i64>,
    whole: bool,
}

/// The thread a transcript holds, built from its lines newest first.
#[derive(Default)]
struct Thread {
    /// Newest first.
    turns: Vec<Turn>,
    /// The reply of the turn being read back over.
    reply: Option<Reply>,
    /// The prompt the turn after opened on, when typed on a user line: a
    /// queued copy of it just before is that prompt once more.
    typed: Option<String>,
    bytes: usize,
}

impl Thread {
    /// Take the line `raw`, older than every line taken so far, and break
    /// once the thread holds more than a read gives.
    fn take(&mut self, raw: &str) -> ControlFlow<()> {
        match line_of(raw) {
            None | Some(Line::Said(Said::Nothing, ..)) => {}
            Some(Line::Reply(id, texts, at)) => match &mut self.reply {
                None => {
                    self.reply = Some(Reply {
                        id,
                        parts: texts,
                        at,
                        whole: false,
                    })
                }
                Some(r) if !r.whole && r.id.is_some() && r.id == id => {
                    r.parts.splice(0..0, texts);
                    r.at = r.at.or(at);
                }
                Some(r) => r.whole = true,
            },
            Some(Line::Said(Said::Injected, ..)) => {
                self.end_turn();
                self.typed = None;
            }
            Some(Line::Said(Said::Prompt(text), at, queued)) => {
                if queued && self.typed.as_ref() == Some(&text) {
                    self.typed = None;
                    return ControlFlow::Continue(());
                }
                self.end_turn();
                self.typed = (!queued).then(|| text.clone());
                self.push(TurnRole::User, text, at);
            }
        }
        if self.turns.len() > MAX_TURNS || self.bytes > MAX_TURN_BYTES {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    fn push(&mut self, role: TurnRole, text: String, at: Option<i64>) {
        self.bytes += text.len();
        self.turns.push(Turn { role, text, at });
    }

    /// The turn being read back over starts here: its reply, if it gave
    /// one, is a turn.
    fn end_turn(&mut self) {
        if let Some(reply) = self.reply.take() {
            let text = reply.parts.join("\n").trim().to_string();
            self.push(TurnRole::Assistant, text, reply.at);
        }
    }

    /// Every turn taken, oldest first.
    fn done(mut self) -> Vec<Turn> {
        self.end_turn();
        self.turns.reverse();
        self.turns
    }
}

/// What a user message's `content` says: its text blocks, and each image
/// the text does not name already as the `[Image #n]` Claude Code shows in
/// its place. Claude Code numbers a session's images across all of it, and
/// writes the marks into the text, so an image past the marks the text has
/// is numbered on from the highest of them. A tool's result is nothing
/// anyone typed.
fn said(content: Option<&Value>) -> Said {
    let blocks = match content {
        Some(Value::String(text)) => return typed(text),
        Some(Value::Array(blocks)) => blocks,
        _ => return Said::Nothing,
    };
    fn kind(block: &Value) -> &str {
        block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }
    let words: Vec<&str> = blocks
        .iter()
        .filter(|b| kind(b) == "text")
        .filter_map(|b| b.get("text")?.as_str())
        .collect();
    let named: Vec<usize> = words
        .iter()
        .flat_map(|w| w.split("[Image #").skip(1))
        .filter_map(|after| after.split_once(']')?.0.parse().ok())
        .collect();
    let mut next = named.iter().max().map_or(1, |n| n + 1);
    let mut marked = named.len();
    let mut parts = Vec::new();
    for block in blocks {
        match kind(block) {
            "text" => parts.extend(block.get("text").and_then(Value::as_str).map(String::from)),
            "image" if marked > 0 => marked -= 1,
            "image" => {
                parts.push(format!("[Image #{next}]"));
                next += 1;
            }
            _ => {}
        }
    }
    typed(&parts.join("\n"))
}

/// The prompt a user typed, from the text of their message.
fn typed(raw: &str) -> Said {
    let text = NOT_TYPED
        .iter()
        .fold(raw.to_string(), |t, tag| without(&t, tag));
    let text = crate::prompt_history::without_paste_marks(&text);
    let text = text.trim();
    // nebula's own turns, a brief or a merge note, stay: the reply after
    // one answers it.
    if crate::prompt_history::is_injected(text) && !text.starts_with("[nebula]") {
        return Said::Injected;
    }
    if text.starts_with("<command-") {
        if let Some(name) = inside(text, "command-name") {
            let args = inside(text, "command-args").unwrap_or_default();
            return Said::Prompt(format!("{name} {args}").trim().to_string());
        }
    }
    if let Some(line) = text
        .strip_prefix("<bash-input>")
        .and(inside(text, "bash-input"))
    {
        return Said::Prompt(format!("!{line}"));
    }
    if text.is_empty() || text.starts_with("[Request interrupted by user") {
        return Said::Nothing;
    }
    Said::Prompt(text.to_string())
}

/// What is between the first `<tag>` and its `</tag>` in `text`, trimmed.
fn inside<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let from = text.find(&open)? + open.len();
    let to = text[from..].find(&format!("</{tag}>"))?;
    Some(text[from..from + to].trim())
}

/// `text` without each `<tag …>…</tag>` in it, and without the rest of it
/// after one that is never closed.
fn without(text: &str, tag: &str) -> String {
    let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(&open) {
        out.push_str(&rest[..at]);
        rest = rest[at..]
            .find(&close)
            .map_or("", |end| &rest[at + end + close.len()..]);
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_context::SCAN_CHUNK;
    use serde_json::json;

    fn line(value: Value) -> String {
        value.to_string() + "\n"
    }

    fn user(at: &str, content: Value) -> String {
        line(json!({
            "type": "user",
            "timestamp": at,
            "message": {"role": "user", "content": content},
        }))
    }

    fn assistant(at: &str, id: &str, block: Value) -> String {
        line(json!({
            "type": "assistant",
            "timestamp": at,
            "message": {"id": id, "role": "assistant", "content": [block]},
        }))
    }

    fn text(t: &str) -> Value {
        json!({"type": "text", "text": t})
    }

    /// A prompt typed while Claude worked, as Claude Code records it.
    fn queued(at: &str, mode: &str, prompt: Value) -> String {
        line(json!({
            "type": "attachment",
            "timestamp": at,
            "attachment": {"type": "queued_command", "commandMode": mode, "prompt": prompt},
        }))
    }

    /// The copy of a message Claude Code writes for the request it went in.
    fn blob(role: &str, content: Value) -> String {
        line(json!({
            "type": "api-request-blob",
            "hash": "h1",
            "message": {"role": role, "content": content},
        }))
    }

    /// The turns in `transcript`, as a read of it builds them.
    fn turns_of(transcript: &str) -> Vec<Turn> {
        let mut thread = Thread::default();
        for line in transcript.lines().rev() {
            if thread.take(line).is_break() {
                break;
            }
        }
        thread.done()
    }

    fn turn(role: TurnRole, text: &str, at: &str) -> Turn {
        Turn {
            role,
            text: text.into(),
            at: nebula_core::clock::utc_ms(at),
        }
    }

    const T1: &str = "2026-11-19T09:00:00.000Z";
    const T2: &str = "2026-11-19T09:00:05.250Z";
    const T3: &str = "2026-11-19T09:01:00.000Z";
    const T4: &str = "2026-11-19T09:02:00.000Z";

    /// A turn is the prompt as typed and the text of the newest assistant
    /// message after it, every text block of it: an earlier message's text,
    /// a tool call, a tool's result and the reminders around them are no
    /// turn of their own.
    #[test]
    fn a_turn_is_the_prompt_and_the_reply_its_user_read() {
        let transcript = [
            user(
                T1,
                json!("<system-reminder>The date is Thursday.</system-reminder>\nFix the login redirect."),
            ),
            assistant(T1, "m1", text("Let me look at the router.")),
            assistant(T1, "m1", json!({"type": "tool_use", "id": "t1", "name": "Read", "input": {}})),
            user(
                T1,
                json!([{"type": "tool_result", "tool_use_id": "t1", "content": "fn route() {}"}]),
            ),
            user(
                T1,
                json!([{"type": "text", "text": "<system-reminder>A hook said hello.</system-reminder>"}]),
            ),
            assistant(T2, "m2", text("The redirect keeps its query string now.")),
            assistant(T2, "m2", json!({"type": "thinking", "thinking": "done?"})),
            assistant(T3, "m2", text("DONE fixed the login redirect\nNEXT: Ship it | Wait")),
        ]
        .concat();
        assert_eq!(
            turns_of(&transcript),
            [
                turn(TurnRole::User, "Fix the login redirect.", T1),
                turn(
                    TurnRole::Assistant,
                    "The redirect keeps its query string now.\nDONE fixed the login redirect\nNEXT: Ship it | Wait",
                    T3,
                ),
            ]
        );
    }

    /// An image in a prompt is the `[Image #n]` Claude Code shows in its
    /// place, numbered across the session: the mark the text has, and for
    /// an image it does not name, one on from the highest it does; a prompt
    /// of an image alone is that.
    #[test]
    fn an_image_in_a_prompt_stays_a_short_mark() {
        let image = json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="}});
        let transcript = [
            user(
                T1,
                json!([text("[Image #3] What is wrong here?"), image.clone()]),
            ),
            user(
                T2,
                json!([
                    text("[Image #4][Image #5] And these two?"),
                    image.clone(),
                    image.clone()
                ]),
            ),
            user(
                T2,
                json!([
                    text("[Image #6] and one unnamed"),
                    image.clone(),
                    image.clone()
                ]),
            ),
            user(
                T3,
                json!([image.clone(), text("Older ones?"), image.clone()]),
            ),
            user(T3, json!([image])),
        ]
        .concat();
        let said: Vec<String> = turns_of(&transcript).into_iter().map(|t| t.text).collect();
        assert_eq!(
            said,
            [
                "[Image #3] What is wrong here?",
                "[Image #4][Image #5] And these two?",
                "[Image #6] and one unnamed\n[Image #7]",
                "[Image #1]\nOlder ones?\n[Image #2]",
                "[Image #1]"
            ]
        );
    }

    /// The copies of a request's messages Claude Code writes beside the
    /// conversation, with no time or message id, are no turns: no prompt
    /// twice, none the CLI wrote, and no reply of a turn but its newest.
    #[test]
    fn a_requests_copy_of_a_message_is_no_turn() {
        let transcript = [
            user(T1, json!("Fix the login redirect.")),
            blob("system", json!([text("You are a coding agent.")])),
            blob("user", json!([text("Fix the login redirect.")])),
            assistant(T1, "m1", text("Let me look at the router.")),
            assistant(
                T1,
                "m1",
                json!({"type": "tool_use", "id": "t1", "name": "Read", "input": {}}),
            ),
            user(
                T1,
                json!([{"type": "tool_result", "tool_use_id": "t1", "content": "fn route() {}"}]),
            ),
            blob("assistant", json!([text("Let me look at the router.")])),
            blob("user", json!([text("Tool loaded.")])),
            assistant(T2, "m2", text("Fixed.")),
        ]
        .concat();
        assert_eq!(
            turns_of(&transcript),
            [
                turn(TurnRole::User, "Fix the login redirect.", T1),
                turn(TurnRole::Assistant, "Fixed.", T2),
            ]
        );
    }

    /// A prompt typed while Claude works is a turn where Claude took it in,
    /// and the reply after it answers it; a background task's notice queued
    /// the same way, or a sibling session's message, is nobody's prompt,
    /// and a queued prompt Claude takes as its next turn is one prompt.
    #[test]
    fn a_prompt_typed_while_claude_works_is_a_turn_where_it_was_taken() {
        let transcript = [
            user(T1, json!("Fix the header.")),
            assistant(T1, "m1", text("Looking at the header.")),
            assistant(
                T1,
                "m1",
                json!({"type": "tool_use", "id": "t1", "name": "Read", "input": {}}),
            ),
            user(
                T1,
                json!([{"type": "tool_result", "tool_use_id": "t1", "content": "<header/>"}]),
            ),
            queued(T2, "prompt", json!("Also check the footer.")),
            assistant(T2, "m2", text("Fixed both.")),
            queued(
                T3,
                "task-notification",
                json!("A background build finished."),
            ),
            assistant(T3, "m3", text("The build passed.")),
            queued(
                T3,
                "prompt",
                json!("<agent-message from=\"a1\">Done on my side.</agent-message>"),
            ),
            assistant(T3, "m4", text("Noted.")),
            queued(T4, "prompt", json!("Then ship it.")),
            user(T4, json!("Then ship it.")),
            assistant(T4, "m5", text("Shipped.")),
        ]
        .concat();
        let said: Vec<(TurnRole, String)> = turns_of(&transcript)
            .into_iter()
            .map(|t| (t.role, t.text))
            .collect();
        assert_eq!(
            said,
            [
                (TurnRole::User, "Fix the header.".into()),
                (TurnRole::Assistant, "Looking at the header.".into()),
                (TurnRole::User, "Also check the footer.".into()),
                (TurnRole::Assistant, "Fixed both.".into()),
                (TurnRole::Assistant, "The build passed.".into()),
                (TurnRole::Assistant, "Noted.".into()),
                (TurnRole::User, "Then ship it.".into()),
                (TurnRole::Assistant, "Shipped.".into()),
            ]
        );
    }

    /// A slash command is its name and arguments as typed, with no wrapper
    /// or output; a skill's body, the CLI's caveat and a bash command's
    /// output are nothing anyone typed, and `!` keeps its command.
    #[test]
    fn a_command_is_what_was_typed_without_its_wrappers() {
        let transcript = [
            user(
                T1,
                json!("<command-message>model</command-message>\n<command-name>/model</command-name>\n<command-args>opus</command-args>"),
            ),
            user(T1, json!("<local-command-stdout>Set model to opus</local-command-stdout>")),
            line(json!({
                "type": "user",
                "isMeta": true,
                "message": {"role": "user", "content": "<local-command-caveat>Caveat: the messages below were generated by the user.</local-command-caveat>"},
            })),
            user(T2, json!("<command-name>/review</command-name>\n<command-args></command-args>")),
            line(json!({
                "type": "user",
                "isMeta": true,
                "message": {"role": "user", "content": [text("# Review\nRead the diff and list what is wrong.")]},
            })),
            user(T3, json!("<bash-input>git status</bash-input>")),
            user(T3, json!("<bash-stdout>nothing to commit</bash-stdout><bash-stderr></bash-stderr>")),
        ]
        .concat();
        let said: Vec<String> = turns_of(&transcript).into_iter().map(|t| t.text).collect();
        assert_eq!(said, ["/model opus", "/review", "!git status"]);
    }

    /// A turn interrupted keeps the text its user read before the stop, and
    /// the CLI's note of the stop is no prompt: the next prompt is the next
    /// turn.
    #[test]
    fn an_interrupted_turn_keeps_what_it_said_before_the_stop() {
        let transcript = [
            user(T1, json!("Rename the config keys.")),
            assistant(T1, "m1", text("Renaming `port` first.")),
            assistant(T1, "m1", json!({"type": "tool_use", "id": "t1", "name": "Edit", "input": {}})),
            user(
                T2,
                json!([{"type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": "[Request interrupted by user for tool use]"}]),
            ),
            user(T2, json!([text("[Request interrupted by user for tool use]")])),
            user(T3, json!("Leave `port` as it is.")),
            assistant(T4, "m2", text("Left it, and renamed the rest.")),
        ]
        .concat();
        let said: Vec<(TurnRole, String)> = turns_of(&transcript)
            .into_iter()
            .map(|t| (t.role, t.text))
            .collect();
        assert_eq!(
            said,
            [
                (TurnRole::User, "Rename the config keys.".into()),
                (TurnRole::Assistant, "Renaming `port` first.".into()),
                (TurnRole::User, "Leave `port` as it is.".into()),
                (TurnRole::Assistant, "Left it, and renamed the rest.".into()),
            ]
        );
    }

    /// A compaction's summary, a subagent's turns and a background task's
    /// notification are nobody's prompt; the reply to a notification is a
    /// turn of its own, and a brief nebula typed stays, as the reply after
    /// it answers it.
    #[test]
    fn what_the_cli_wrote_in_the_users_place_is_no_prompt() {
        let transcript = [
            user(T1, json!("Run the e2e tests in the background.")),
            assistant(T1, "m1", text("Started them; I will report.")),
            line(json!({"type": "system", "subtype": "compact_boundary", "content": "Conversation compacted"})),
            line(json!({
                "type": "user",
                "isCompactSummary": true,
                "isVisibleInTranscriptOnly": true,
                "message": {"role": "user", "content": "This session is being continued from a previous conversation that ran out of context."},
            })),
            line(json!({"type": "summary", "summary": "Run the e2e tests", "leafUuid": "u1"})),
            line(json!({
                "type": "user",
                "isSidechain": true,
                "message": {"role": "user", "content": "Find the flaky test."},
            })),
            user(
                T2,
                json!("<system-reminder>A background task ended.</system-reminder>\n<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>"),
            ),
            assistant(T3, "m2", text("All 212 pass.\n\nDONE ran the e2e tests")),
            user(T4, json!("[nebula] MERGE NOTE: pull request #12 merged.")),
        ]
        .concat();
        let said: Vec<(TurnRole, String)> = turns_of(&transcript)
            .into_iter()
            .map(|t| (t.role, t.text))
            .collect();
        assert_eq!(
            said,
            [
                (
                    TurnRole::User,
                    "Run the e2e tests in the background.".into()
                ),
                (TurnRole::Assistant, "Started them; I will report.".into()),
                (
                    TurnRole::Assistant,
                    "All 212 pass.\n\nDONE ran the e2e tests".into()
                ),
                (
                    TurnRole::User,
                    "[nebula] MERGE NOTE: pull request #12 merged.".into()
                ),
            ]
        );
    }

    /// A read gives the newest [`MAX_TURNS`], and within
    /// [`MAX_TURN_BYTES`], saying older ones are left; the newest is given
    /// whatever its size, its end when it alone is too long.
    #[test]
    fn a_read_gives_the_newest_turns_that_fit() {
        let many: Vec<Turn> = (0..MAX_TURNS + 5)
            .map(|n| turn(TurnRole::User, &format!("turn {n}"), T1))
            .collect();
        let (turns, more) = newest(many);
        assert_eq!(turns.len(), MAX_TURNS);
        assert_eq!(turns[0].text, "turn 5");
        assert!(more);

        let big = "x".repeat(MAX_TURN_BYTES / 2 + 1);
        let (turns, more) = newest(vec![
            turn(TurnRole::User, &big, T1),
            turn(TurnRole::Assistant, &big, T2),
            turn(TurnRole::User, "short", T3),
        ]);
        assert_eq!(turns.len(), 2);
        assert!(more);

        let huge = format!("start{}end", "y".repeat(MAX_TURN_BYTES));
        let (turns, more) = newest(vec![turn(TurnRole::Assistant, &huge, T1)]);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].text.len(), MAX_TURN_BYTES);
        assert!(turns[0].text.ends_with("end"));
        assert!(!more);
        assert_eq!(newest(Vec::new()), (Vec::new(), false));
    }

    /// The daemon reads a session's turns from the transcript its hooks
    /// named; one with no transcript has none to read.
    #[test]
    fn the_daemon_reads_turns_from_the_sessions_transcript() {
        let daemon = Daemon::new(
            std::sync::Arc::new(crate::store::Store::open_in_memory().unwrap()),
            crate::hooks::HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sid.jsonl");
        let transcript = [
            user(T1, json!("Which parser?")),
            assistant(T2, "m1", text("serde_json.")),
        ]
        .concat();
        std::fs::write(&path, transcript).unwrap();
        let id = AgentId("a1".into());
        daemon.note_transcript(
            &id,
            crate::session_title::TranscriptRef {
                transcript_path: path,
                session_id: "sid".into(),
            },
        );
        let (turns, more) = daemon.read_turns(&id).unwrap();
        assert_eq!(
            turns,
            [
                turn(TurnRole::User, "Which parser?", T1),
                turn(TurnRole::Assistant, "serde_json.", T2),
            ]
        );
        assert!(!more);
        let err = daemon.read_turns(&AgentId("none".into())).unwrap_err();
        assert!(err.to_string().contains("only a Claude session"), "{err}");
    }

    /// A long session's transcript is mostly what Claude Code writes beside
    /// the conversation: a read goes back past all of it, a chunk at a time
    /// and over a line longer than one, until it has the newest
    /// [`MAX_TURNS`], and reads a transcript again once it has grown.
    #[test]
    fn a_long_transcript_is_read_back_until_it_has_the_turns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sid.jsonl");
        let filler = line(json!({
            "type": "api-request",
            "timestamp": T1,
            "body": "x".repeat(300 * 1024),
        }));
        let mut transcript = String::new();
        for n in 0..MAX_TURNS + 5 {
            transcript += &user(T1, json!(format!("prompt {n}")));
            transcript += &filler;
            let said = text(&format!("reply {n}"));
            transcript += &if n == MAX_TURNS {
                let write = json!({"type": "tool_use", "id": "t1", "name": "Write", "input": {"body": "y".repeat(3 * SCAN_CHUNK)}});
                line(json!({
                    "type": "assistant",
                    "timestamp": T2,
                    "message": {"id": format!("m{n}"), "role": "assistant", "content": [said, write]},
                }))
            } else {
                assistant(T2, &format!("m{n}"), said)
            };
        }
        assert!(transcript.len() > 12 * 1024 * 1024);
        std::fs::write(&path, &transcript).unwrap();
        let (turns, more) = turns_in(&path).unwrap();
        assert!(more);
        assert_eq!(turns.len(), MAX_TURNS);
        assert_eq!(turns[0].text, "prompt 25");
        assert_eq!(
            turns[MAX_TURNS - 1].text,
            format!("reply {}", MAX_TURNS + 4)
        );
        assert!(turns.iter().any(|t| t.text == format!("reply {MAX_TURNS}")));

        transcript += &user(T3, json!("One more."));
        std::fs::write(&path, &transcript).unwrap();
        let (turns, _) = turns_in(&path).unwrap();
        assert_eq!(turns.last().unwrap().text, "One more.");
    }
}
