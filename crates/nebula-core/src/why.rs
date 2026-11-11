//! What a session is about, read off its screen: the WHY LINE a board row
//! says under it. A session waiting on the user says what it asks (a
//! permission dialog what it is about, any other question as it is); a
//! finished one says how its turn ended, from its REPORT LINE (`DONE`,
//! `BLOCKED`, `QUESTION`) or else the last paragraph it wrote. A `NEXT:`
//! line of answers a reply may end on, and the pills Claude Code's
//! `quick-answers` mod draws in its place, are the chat's and never why.
//! Every client that draws a board reads the same lines this way.

use crate::AgentStatus;

/// How many of the screen's last lines a `needs you` row with no dialog
/// looks through for a question.
const ASK_WINDOW: usize = 8;
/// What it runs together when none of them is a question.
const ASK_TAIL: usize = 3;
/// A finished turn's report line starts with one of these. `DONE` is
/// what the `done` section says already, so the line drops it.
const REPORT_WORDS: [&str; 3] = ["DONE", "BLOCKED", "QUESTION"];
/// What opens the line of answers a reply may end on for the user to pick
/// from, which Claude Code's `quick-answers` mod asks for: the chat's to
/// show, and never why a session is on the board.
const NEXT: &str = "NEXT:";
/// A permission dialog's question when it asks only for a yes: the board
/// says what it is about instead.
const STOCK_QUESTIONS: [&str; 2] = ["Do you want to ", "Would you like to "];

/// The line under a row in the state `status`, from the session's screen
/// (`rows`, top to bottom).
pub fn why_line(status: AgentStatus, rows: &[String]) -> Option<String> {
    // The question whether to trust the folder, asked before the CLI runs:
    // its options say nothing, and its question does not end its line.
    // Only its dialog asks it, not a conversation quoting it.
    if status == AgentStatus::NeedsFeedback {
        if let Some(question) = crate::trust::question_on(rows) {
            return Some(question.to_string());
        }
    }
    let lines: Vec<Line> = rows.iter().map(|r| Line::parse(r)).collect();
    match status {
        AgentStatus::NeedsFeedback => asking_line(&lines),
        _ => report_line(&lines),
    }
}

/// Under a `needs you` row: what the session asks, never its options. A
/// permission dialog's stock question gives way to what it is about
/// (`Bash: cargo build`, `Edit: src/lib.rs`, Codex's `$ cargo build`);
/// any other question is said as it is. With no dialog on screen, the
/// last question among the last lines, else those lines run together.
fn asking_line(lines: &[Line]) -> Option<String> {
    if let Some(first) = lines.iter().rposition(|l| option_number(l) == Some(1)) {
        let start = lines[..first]
            .iter()
            .rposition(Line::opens_dialog)
            .map_or(0, |i| i + 1);
        let dialog = &lines[start..first];
        if let Some(q) = dialog.iter().rposition(|l| {
            l.text()
                .is_some_and(|t| STOCK_QUESTIONS.iter().any(|s| t.starts_with(s)))
        }) {
            return Some(subject(dialog, q).unwrap_or_else(|| question(dialog, q)));
        }
        if let Some(q) = dialog.iter().rposition(Line::asks) {
            return Some(question(dialog, q));
        }
    }
    let said = &lines[..input_box(lines).unwrap_or(lines.len())];
    let texts: Vec<usize> = (0..said.len())
        .filter(|&i| {
            said[i].text().is_some() && !said[i].is_status() && option_number(&said[i]).is_none()
        })
        .collect();
    let window = &texts[texts.len().saturating_sub(ASK_WINDOW)..];
    if let Some(&q) = window.iter().rev().find(|&&i| said[i].asks()) {
        return Some(question(said, q));
    }
    let line = window[window.len().saturating_sub(ASK_TAIL)..]
        .iter()
        .filter_map(|&i| said[i].shown())
        .collect::<Vec<_>>()
        .join(" ");
    (!line.is_empty()).then_some(line)
}

/// What a permission dialog asks about: Codex's `$ …` command line, or a
/// Claude dialog's title word and what follows it, `Bash: cargo build`
/// from a `Bash command` dialog's dashed preview of the command, and
/// `Edit: src/lib.rs` from the path under an `Edit file` title (whose
/// preview is the diff).
fn subject(dialog: &[Line], q: usize) -> Option<String> {
    if let Some(at) = dialog
        .iter()
        .position(|l| l.text().is_some_and(|t| t.starts_with("$ ")))
    {
        let end = dialog[at..]
            .iter()
            .position(|l| l.text().is_none())
            .map_or(dialog.len(), |n| at + n);
        return Some(joined(&dialog[at..end]));
    }
    let title_at = dialog.iter().position(|l| l.text().is_some())?;
    if title_at == q {
        return None;
    }
    let title = dialog[title_at].text()?;
    let dashes: Vec<usize> = (title_at..q)
        .filter(|&i| dialog[i] == Line::Rule { dashed: true })
        .collect();
    let (from, to) = match dashes.as_slice() {
        [open, close, ..] if title.ends_with(" command") => (open + 1, *close),
        [open, ..] => (title_at + 1, *open),
        [] => (title_at + 1, q),
    };
    let what = joined(&dialog[from..to]);
    let tool = title.split_whitespace().next()?;
    (!what.is_empty()).then(|| format!("{tool}: {what}"))
}

/// The question on line `q`: its paragraph run into one line, from the
/// sentence the question mark ends.
fn question(lines: &[Line], q: usize) -> String {
    let at = lines[q].at();
    let from = lines[..q]
        .iter()
        .rposition(|l| l.text().is_none() || l.at() != at || option_number(l).is_some())
        .map_or(0, |i| i + 1);
    let text = joined(&lines[from..=q]);
    let body = text.trim_end_matches('?');
    let start = [". ", "! ", "? "]
        .iter()
        .filter_map(|p| body.rfind(p).map(|i| i + p.len()))
        .max()
        .unwrap_or(0);
    text[start..].to_string()
}

/// Under a `done` row: the last report line of the turn on screen
/// (`BLOCKED …`, `QUESTION …`, or what follows `DONE`), else the start of
/// the last paragraph it said. The turn is what follows the last prompt it
/// was given and ends at its input box; a tool's output in it (indented
/// under `⎿`, or Codex's `└`), and the CLI's own status lines, are not
/// what it said.
fn report_line(lines: &[Line]) -> Option<String> {
    let said = turn_said(lines);
    if let Some(report) = said.iter().rev().flatten().find_map(|l| report(l.shown()?)) {
        return Some(report);
    }
    // The last paragraph before the `NEXT:` line of answers it may end
    // on, which is the chat's to show: the text since the last gap, or since the reply bullet
    // that opened it.
    let said = &said[..next_at(&said).unwrap_or(said.len())];
    let last = said.iter().rposition(Option::is_some)?;
    let first = said[..=last]
        .iter()
        .rposition(|l| l.is_none_or(Line::is_reply))
        .map_or(0, |i| if said[i].is_some() { i } else { i + 1 });
    let text: Vec<&str> = said[first..=last]
        .iter()
        .flatten()
        .filter_map(|l| l.shown())
        .collect();
    Some(text.join(" "))
}

/// What the latest turn on screen said: the lines after the last prompt it
/// was given, up to its input box, each `None` where it said nothing (a
/// gap, a tool's output, the CLI's own status lines). The input box, and
/// any suggestion the CLI types into it, is not the turn.
fn turn_said(lines: &[Line]) -> Vec<Option<&Line>> {
    let end = input_box(lines).unwrap_or(lines.len());
    let start = lines[..end]
        .iter()
        .rposition(Line::is_prompt)
        .map_or(0, |i| i + 1);
    // The pills sit between the reply and the input box, and a long one
    // wraps onto rows with no cap of their own: the turn ends where the
    // first of them is drawn.
    let end = lines[start..end]
        .iter()
        .position(Line::is_pills)
        .map_or(end, |i| start + i);
    let mut said = Vec::new();
    let mut output_at = None;
    for line in &lines[start..end] {
        // A tool's output runs on through blanks, indented past its mark.
        if let Some(mark_at) = output_at {
            match line {
                Line::Blank => continue,
                Line::Text { at, .. } if *at > mark_at => continue,
                _ => output_at = None,
            }
        }
        if line.is_output() {
            output_at = Some(line.at());
        }
        let text = line.text().is_some() && !line.is_output() && !line.is_status();
        said.push(text.then_some(line));
    }
    said
}

/// The left end of a pill in the row Claude Code's `quick-answers` mod draws
/// over its input box, a Nerd Font's half circle: a row of them is the
/// CLI's own line, never what the turn said.
const PILL_LEFT: char = '\u{e0b6}';

/// Where the last `NEXT:` line of what a turn said is.
fn next_at(said: &[Option<&Line>]) -> Option<usize> {
    said.iter()
        .rposition(|l| l.and_then(Line::shown).is_some_and(|t| t.starts_with(NEXT)))
}

/// `line` as a report line, or `None` when it is not one.
fn report(line: &str) -> Option<String> {
    let word = opening_word(line)?;
    // Answers a reply offers on the same line are the chat's, not the report.
    let line = line.split(NEXT).next().unwrap_or(line).trim_end();
    let rest = line[word.len()..].trim_start_matches([':', '-', ' ']);
    Some(if word == "DONE" && !rest.is_empty() {
        rest.to_string()
    } else {
        line.to_string()
    })
}

/// The report word `line` opens with.
fn opening_word(line: &str) -> Option<&'static str> {
    REPORT_WORDS.into_iter().find(|w| {
        line.strip_prefix(w)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
    })
}

/// The word a `done` row's line opens with when it is one the board
/// keeps (`BLOCKED`, `QUESTION`): it changes what the finish means.
pub fn report_word(line: &str) -> Option<&'static str> {
    opening_word(line).filter(|w| *w != "DONE")
}

/// The lines' text run into one.
fn joined(lines: &[Line]) -> String {
    lines
        .iter()
        .filter_map(Line::shown)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Where the input box is, when the screen ends in one: the last prompt
/// on screen, when no reply comes after it (an empty one is a box
/// wherever it is).
fn input_box(lines: &[Line]) -> Option<usize> {
    let at = lines
        .iter()
        .rposition(|l| l.is_prompt() || l.text().is_some_and(|t| matches!(t, "❯" | "›" | ">")))?;
    (!lines[at..].iter().any(Line::is_reply)).then_some(at)
}

/// The number of a dialog's option line (`❯ 1. Yes`, `2. No`).
fn option_number(line: &Line) -> Option<u32> {
    let text = line.text()?;
    let text = text.strip_prefix(PROMPTS).map_or(text, str::trim_start);
    text.split_once(". ")?.0.parse().ok()
}

/// A prompt's mark: Claude's, Codex's, and the one older CLIs draw.
const PROMPTS: [char; 3] = ['❯', '›', '>'];
/// What opens a reply or a tool call at the left edge.
const BULLETS: [char; 3] = ['⏺', '●', '•'];
/// What a tool's output hangs from: Claude's mark and Codex's corner.
const OUTPUT: [char; 2] = ['⎿', '└'];

/// One row of a session's screen, read for the line under its board row.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Blank,
    /// A frame's top or bottom, or a rule across the screen: solid, with a
    /// label in it or not, or the dashed one round a dialog's preview.
    Rule {
        dashed: bool,
    },
    /// Text, its frame and runs of blanks taken off, and the column it
    /// starts in.
    Text {
        at: usize,
        text: String,
    },
}

impl Line {
    fn parse(row: &str) -> Line {
        let lead = |c: char| c.is_whitespace() || matches!(c, '│' | '┃');
        let at = row.chars().take_while(|c| lead(*c)).count();
        let body = row.trim_start_matches(lead);
        match body.chars().next() {
            None => return Line::Blank,
            Some(c) if is_frame(c) && !OUTPUT.contains(&c) => {
                return Line::Rule {
                    dashed: body.contains('╌'),
                }
            }
            _ => {}
        }
        let body = body.trim_end_matches(|c: char| c.is_whitespace() || is_frame(c));
        if body.is_empty() {
            return Line::Blank;
        }
        Line::Text {
            at,
            text: body.split_whitespace().collect::<Vec<_>>().join(" "),
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Line::Text { text, .. } => Some(text),
            _ => None,
        }
    }

    fn at(&self) -> usize {
        match self {
            Line::Text { at, .. } => *at,
            _ => 0,
        }
    }

    /// The text as the board shows it: a reply's bullet and a tool
    /// output's mark taken off.
    fn shown(&self) -> Option<&str> {
        let text = self.text()?;
        Some(
            text.strip_prefix(BULLETS)
                .or_else(|| text.strip_prefix(OUTPUT))
                .map_or(text, str::trim_start),
        )
    }

    /// A prompt the session was given, at the left edge, or the input box.
    fn is_prompt(&self) -> bool {
        self.at() == 0
            && self.text().is_some_and(|t| {
                t.strip_prefix(PROMPTS)
                    .is_some_and(|r| r.is_empty() || r.starts_with(' '))
            })
            && option_number(self).is_none()
    }

    fn is_output(&self) -> bool {
        self.text().is_some_and(|t| t.starts_with(OUTPUT))
    }

    /// A reply or a tool call, opened by its bullet at the left edge.
    fn is_reply(&self) -> bool {
        self.at() == 0 && self.text().is_some_and(|t| t.starts_with(BULLETS))
    }

    /// One of the CLI's own lines rather than the agent's: a spinner or
    /// summary glyph before it (Claude's `✻ Worked for 4s`, `◉ xhigh`,
    /// Codex's `■ Conversation interrupted`), or Codex's `Worked for 5s`.
    fn is_status(&self) -> bool {
        self.text().is_some_and(|t| {
            let mut chars = t.chars();
            let glyph = chars.next().is_some_and(|c| {
                !c.is_ascii()
                    && !c.is_alphanumeric()
                    && !BULLETS.contains(&c)
                    && !OUTPUT.contains(&c)
                    && !PROMPTS.contains(&c)
            });
            glyph && chars.next() == Some(' ') || t.starts_with("Worked for ")
        })
    }

    /// A row of the pills Claude Code's `quick-answers` mod draws over its
    /// input box.
    fn is_pills(&self) -> bool {
        self.text().is_some_and(|t| t.contains(PILL_LEFT))
    }

    /// A line that ends in a question.
    fn asks(&self) -> bool {
        self.text().is_some_and(|t| t.ends_with('?')) && option_number(self).is_none()
    }

    /// Where a dialog begins below: a solid rule over it, or the reply or
    /// prompt it follows.
    fn opens_dialog(&self) -> bool {
        *self == Line::Rule { dashed: false } || self.is_reply() || self.is_prompt()
    }
}

/// A box-drawing character: a frame's side or corner, or a rule.
fn is_frame(c: char) -> bool {
    ('\u{2500}'..='\u{257F}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(s: &[&str]) -> Vec<String> {
        s.iter().map(|r| r.to_string()).collect()
    }

    /// A screen `nebula session read` printed off a real CLI (Claude Code
    /// 2.1.319 and Codex 0.163, 80 columns), its paths made up.
    fn screen(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    fn asking(text: &str) -> Option<String> {
        why_line(AgentStatus::NeedsFeedback, &screen(text))
    }

    fn finished(text: &str) -> Option<String> {
        why_line(AgentStatus::Finished, &screen(text))
    }

    #[test]
    fn a_permission_dialog_says_what_it_is_about_not_its_options() {
        assert_eq!(
            asking(include_str!("why/screens/claude-bash.txt")).as_deref(),
            Some("Bash: cargo build --release --timings")
        );
        assert_eq!(
            asking(include_str!("why/screens/claude-bash-long.txt")).as_deref(),
            Some(
                "Bash: RUSTFLAGS='-C target-cpu=native' cargo build --release --timings \
                 --features tracing,metrics --target-dir /tmp/cap-target-dir-for-profiling"
            ),
            "a wrapped command runs into one line"
        );
        assert_eq!(
            asking(include_str!("why/screens/claude-edit.txt")).as_deref(),
            Some("Edit: src/auth/session.rs")
        );
        assert_eq!(
            asking(include_str!("why/screens/claude-write.txt")).as_deref(),
            Some("Create: src/lib.rs")
        );
        assert_eq!(
            asking(include_str!("why/screens/codex-run.txt")).as_deref(),
            Some("$ cargo build --release --timings"),
            "Codex's question is stock too, and its reason ends in a question mark"
        );
        assert_eq!(
            asking(include_str!("why/screens/codex-run-long.txt")).as_deref(),
            Some(
                "$ RUSTFLAGS='-C target-cpu=native' cargo build --release --timings \
                 --features tracing,metrics --target-dir /tmp/cap-target-dir-for-profiling"
            )
        );
    }

    /// Claude's question whether to trust the folder, which it asks before
    /// it starts: the question, not its options or the legend under them.
    #[test]
    fn a_trust_question_is_said_as_the_cli_words_it() {
        assert_eq!(
            asking(include_str!("why/screens/claude-trust.txt")).as_deref(),
            Some("Is this a project you created or one you trust?")
        );
        let quoted = format!(
            "⏺ Claude first asks: Is this a project you created or one you trust?\n{}",
            include_str!("why/screens/claude-bash.txt")
        );
        assert_eq!(
            asking(&quoted).as_deref(),
            Some("Bash: cargo build --release --timings"),
            "a conversation quoting it above another dialog"
        );
    }

    #[test]
    fn a_question_is_said_as_it_is() {
        assert_eq!(
            asking(include_str!("why/screens/claude-ask.txt")).as_deref(),
            Some("Which log format should the daemon write?")
        );
        let plain = rows(&[
            "⏺ The package has no crate root. The smallest fix is a one-line",
            "  src/lib.rs. Should I add it and run the build",
            "  again?",
            "",
            "─────────────────────────────────",
            "❯",
            "─────────────────────────────────",
            "  ? for shortcuts",
        ]);
        assert_eq!(
            why_line(AgentStatus::NeedsFeedback, &plain).as_deref(),
            Some("Should I add it and run the build again?"),
            "with no dialog, the last question's sentence"
        );
    }

    #[test]
    fn a_boxed_dialog_drops_its_frame() {
        let screen = rows(&[
            "╭──────────────────────────────╮",
            "│ Bash command                 │",
            "│   rm -rf target              │",
            "│ Do you want to proceed?      │",
            "│ ❯ 1. Yes                     │",
            "│   2. No                      │",
            "╰──────────────────────────────╯",
        ]);
        assert_eq!(
            why_line(AgentStatus::NeedsFeedback, &screen).as_deref(),
            Some("Bash: rm -rf target")
        );
    }

    #[test]
    fn with_no_question_the_last_lines_run_together() {
        let screen = rows(&["one", "two", "three", "four"]);
        assert_eq!(
            why_line(AgentStatus::NeedsFeedback, &screen).as_deref(),
            Some("two three four")
        );
        assert_eq!(
            why_line(AgentStatus::NeedsFeedback, &rows(&["", " "])),
            None
        );
    }

    #[test]
    fn a_finished_turn_reads_its_report_line_without_done() {
        assert_eq!(
            finished(include_str!("why/screens/claude-done.txt")).as_deref(),
            Some("explained release build speed")
        );
        assert_eq!(
            finished(include_str!("why/screens/codex-done.txt")).as_deref(),
            Some("explained release build speed")
        );
        let screen = rows(&[
            "❯ sort the changelog",
            "⏺ Sorted every entry by date.",
            "  DONE: CHANGELOG.md is newest first",
            "  Notes: Unreleased stays on top",
            "────────────────────",
            "> ",
            "────────────────────",
            "  ? for shortcuts",
        ]);
        assert_eq!(
            why_line(AgentStatus::Finished, &screen).as_deref(),
            Some("CHANGELOG.md is newest first")
        );
        let blocked = rows(&[
            "⏺ BLOCKED the tests need a database",
            "DONEGAL is no report",
        ]);
        assert_eq!(
            why_line(AgentStatus::Finished, &blocked).as_deref(),
            Some("BLOCKED the tests need a database"),
            "BLOCKED and QUESTION keep their word"
        );
        assert_eq!(
            report_word("BLOCKED the tests need a database"),
            Some("BLOCKED")
        );
        assert_eq!(report_word("QUESTION: which base?"), Some("QUESTION"));
        assert_eq!(report_word("DONE it"), None);
        assert_eq!(report_word("BLOCKEDNESS"), None);
    }

    #[test]
    fn only_the_last_turn_reports_and_never_from_a_tools_output() {
        let screen = rows(&[
            "⏺ DONE an old turn",
            "",
            "❯ check on the worker",
            "",
            "⏺ Bash(nebula read 01ABC)",
            "  ⎿  DONE fixed the redirect loop",
            "     Notes: none",
            "",
            "      DONE in a long output",
            "",
            "⏺ The worker fixed the redirect loop; it",
            "  opened no pull request.",
            "",
            "✻ Worked for 4s",
            "",
            "────────────────────",
            "❯",
            "────────────────────",
        ]);
        assert_eq!(
            why_line(AgentStatus::Finished, &screen).as_deref(),
            Some("The worker fixed the redirect loop; it opened no pull request."),
            "the start of the last paragraph, run into one line"
        );
    }

    #[test]
    fn a_finished_turn_with_no_report_reads_its_last_paragraph() {
        let screen = rows(&[
            "⏺ Entries are newest first.",
            "",
            "  Unreleased stays on top, and the",
            "  rest is sorted by date.",
            "╭──────────────────╮",
            "│ >                │",
            "╰──────────────────╯",
            "  ? for shortcuts",
        ]);
        assert_eq!(
            why_line(AgentStatus::Finished, &screen).as_deref(),
            Some("Unreleased stays on top, and the rest is sorted by date.")
        );
        let codex = rows(&[
            "› sort it",
            "",
            "• Sorted.",
            "",
            "  Newest first now.",
            "  └ Tip: Press f3 to search this conversation.",
            "",
            "  Worked for 5s • 07:04",
            "",
            "",
            "› Ask Codex to do anything",
            "",
            "  GPT-6.1-Sol default · ~/code/api-server",
            "  ← for agents · ? for shortcuts",
        ]);
        assert_eq!(
            why_line(AgentStatus::Finished, &codex).as_deref(),
            Some("Newest first now."),
            "Codex's box, tip and timing are not what it said"
        );
        assert_eq!(
            why_line(AgentStatus::Finished, &rows(&["just text"])).as_deref(),
            Some("just text")
        );
    }

    /// Claude Code's `quick-answers` mod hides the `NEXT:` line and draws
    /// its answers as pills over the input box: the pills are the CLI's own
    /// row, never what the turn said.
    #[test]
    fn a_row_of_pills_is_never_the_why_line() {
        let screen = rows(&[
            "❯ add rate limiting",
            "",
            "⏺ Rate limit middleware is on it in rate-limit.",
            "  Which should the limit be keyed on?",
            "",
            "✻ Worked for 4s",
            "  \u{e0b6}1 Per token\u{e0b4}  \u{e0b6}2 Per IP\u{e0b4}  \u{e0b6}3 Per token, admins exempt\u{e0b4}     [-]",
            "────────────────────",
            "❯ ",
            "────────────────────",
        ]);
        assert_eq!(
            finished(&screen.join("\n")).as_deref(),
            Some(
                "Rate limit middleware is on it in rate-limit. Which should the limit be keyed on?"
            ),
        );
        // Long answers stack and wrap onto rows with no cap of their own.
        let wrapped = rows(&[
            "❯ pick a store",
            "",
            "⏺ Three would do.",
            "",
            "✻ Worked for 4s",
            "",
            "  \u{e0b6}1 SQLite, one file with zero\u{e0b4}",
            "  setup",
            "  \u{e0b6}2 PostgreSQL, full SQL\u{e0b4}",
            "────────────────────",
            "❯ ",
            "────────────────────",
        ]);
        assert_eq!(
            why_line(AgentStatus::Finished, &wrapped).as_deref(),
            Some("Three would do.")
        );
        // Answers written on the report line itself are not part of why.
        let inline = rows(&["❯ go", "", "⏺ QUESTION which base? NEXT: main | dev"]);
        assert_eq!(
            why_line(AgentStatus::Finished, &inline).as_deref(),
            Some("QUESTION which base?")
        );
    }

    /// A reply with no report line says why with its last paragraph
    /// before the `NEXT:` line of answers it ends on, never that line.
    #[test]
    fn the_next_line_is_never_the_why_line() {
        let screen = rows(&[
            "❯ add a docstring",
            "",
            "⏺ The worker added the docstring on add-docstring.",
            "",
            "  NEXT: Read the worker's result | Open a PR for it",
            "  later",
        ]);
        assert_eq!(
            why_line(AgentStatus::Finished, &screen).as_deref(),
            Some("The worker added the docstring on add-docstring.")
        );
        let only_steps = rows(&["❯ go", "", "⏺ NEXT: Open a PR"]);
        assert_eq!(why_line(AgentStatus::Finished, &only_steps), None);
    }
}
