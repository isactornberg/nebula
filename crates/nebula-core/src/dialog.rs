//! A choice dialog on an agent CLI's screen: a list of options, one marked
//! as selected, that the CLI draws in place of its input box. Text typed
//! into one, with its Enter, picks whichever option is selected, so
//! nothing types a turn into a session showing one. Claude marks the
//! selected option `❯`, numbered (`❯ 1. Yes`, a permission dialog) or not
//! (`❯ No, exit`, the question whether to trust the folder, or a one-time
//! notice such as `Make auto mode your default permission mode?`); Codex
//! marks a numbered one `›`.
//!
//! Claude marks its input box's prompt `❯` too, and every prompt the
//! conversation shows above it, so the mark that counts is the screen's
//! last one: the input box, under the rule it hangs from, means no dialog
//! is up. An unnumbered option is one only with another option level with
//! it right above it or anywhere under it, and nothing else under it but a
//! key legend, as a past prompt has the conversation going on below it.

/// The marks a selected option opens with.
const MARKS: [char; 2] = ['❯', '›'];

/// The rows of the choice dialog the screen `rows` (top to bottom) ends
/// on, from the rule over it, or the screen's top, to the screen's end;
/// `None` when it ends on none.
pub fn dialog_in<S: AsRef<str>>(rows: &[S]) -> Option<&[S]> {
    let (at, mark, text, column) = rows.iter().enumerate().rev().find_map(|(i, row)| {
        let (mark, text, column) = marked(row.as_ref())?;
        Some((i, mark, text, column))
    })?;
    let numbered = text
        .split_once(". ")
        .is_some_and(|(n, _)| n.parse::<u32>().is_ok());
    let listed = || {
        let below = &rows[at + 1..];
        let option =
            |row: &str| !row.trim().is_empty() && body_column(row) >= column && !legend(row);
        // The selection on the last option has the others above it.
        let above = at > 0 && option_at(rows[at - 1].as_ref(), column).is_some();
        (above || below.iter().any(|r| option(r.as_ref())))
            && below.iter().all(|r| {
                let row = r.as_ref();
                row.trim().is_empty() || option(row) || legend(row)
            })
    };
    let under_rule = at > 0 && is_rule(rows[at - 1].as_ref());
    let dialog = numbered || (mark == '❯' && !text.is_empty() && !under_rule && listed());
    if !dialog {
        return None;
    }
    let start = rows[..at]
        .iter()
        .rposition(|r| is_rule(r.as_ref()))
        .map_or(0, |i| i + 1);
    Some(&rows[start..])
}

/// What a choice dialog asks, and the options it offers in the order it
/// draws them, each with the number that picks it: the number it is drawn
/// with, or, in a dialog that numbers none (the question whether to trust
/// the folder), its place in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub question: String,
    pub options: Vec<(u32, String)>,
    /// The number of the option the CLI has selected, which an Enter picks.
    pub selected: u32,
    /// Whether the CLI draws the options' numbers, rather than their
    /// numbers being their places.
    pub numbered: bool,
}

/// A permission dialog's question when it asks only for a yes.
const STOCK_QUESTIONS: [&str; 2] = ["Do you want ", "Would you like "];

/// The [`Choice`] the dialog the screen `rows` ends on ([`dialog_in`])
/// offers. Its options are the rows level with the selected one: a
/// numbered dialog's numbered rows, the description under each left out,
/// or the run of rows around an unnumbered selection. Its question is the
/// question whether to trust the folder, as the CLI words it
/// ([`crate::trust`]), else the last stock question above the options
/// (`Do you want to proceed?`), else the last question there, else the
/// dialog's first line.
pub fn choice_in<S: AsRef<str>>(rows: &[S]) -> Option<Choice> {
    let dialog = dialog_in(rows)?;
    let at = dialog.iter().rposition(|r| marked(r.as_ref()).is_some())?;
    let (_, _, column) = marked(dialog[at].as_ref())?;
    let option_text = |row| option_at(row, column);
    let mut options = Vec::new();
    let mut first = at;
    let drawn = option_text(dialog[at].as_ref()).and_then(numbered);
    let selected = if let Some((n, _)) = drawn {
        for (i, row) in dialog.iter().enumerate() {
            if let Some((n, label)) = option_text(row.as_ref()).and_then(numbered) {
                first = first.min(i);
                options.push((n, option_label(label)));
            }
        }
        n
    } else {
        let level = |i: usize| option_text(dialog[i].as_ref()).is_some();
        while first > 0 && level(first - 1) {
            first -= 1;
        }
        let mut n = 0;
        for row in dialog[first..].iter().map(AsRef::as_ref) {
            let Some(text) = option_text(row) else { break };
            n += 1;
            options.push((n, option_label(text)));
        }
        (at - first + 1) as u32
    };
    let question = match crate::trust::question_on(rows) {
        Some(q) => q.to_string(),
        None => asked(&dialog[..first])?,
    };
    Some(Choice {
        question,
        options,
        selected,
        numbered: drawn.is_some(),
    })
}

/// The text of `row` when it is level with options whose text starts at
/// `column`: marked there, or indented to it.
fn option_at(row: &str, column: usize) -> Option<&str> {
    let level = match marked(row) {
        Some((_, text, c)) => (c == column).then_some(text),
        None => (body_column(row) == column).then(|| row.trim()),
    };
    level.filter(|t| !t.is_empty() && !legend(row) && !is_rule(row))
}

/// An option's number and words: `2. No` is `(2, "No")`.
fn numbered(text: &str) -> Option<(u32, &str)> {
    let (n, label) = text.split_once(". ")?;
    Some((n.parse().ok()?, label))
}

/// An option's words, a key hint the CLI adds (Codex's `(y)`, `(esc)`)
/// taken off.
fn option_label(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match text.rsplit_once(" (") {
        Some((label, hint))
            if hint.strip_suffix(')').is_some_and(|k| {
                (1..=3).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_lowercase())
            }) =>
        {
            label.to_string()
        }
        _ => text,
    }
}

/// What the dialog rows above its options (`rows`) ask: the last stock
/// question among them, else the last question, each from the start of its
/// sentence, else the dialog's first line.
fn asked<S: AsRef<str>>(rows: &[S]) -> Option<String> {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut open = false;
    for row in rows.iter().map(AsRef::as_ref) {
        let body = row.trim_start_matches(lead);
        let separator =
            body.is_empty() || body.starts_with(|c| ('\u{2500}'..='\u{257F}').contains(&c));
        if separator {
            open = false;
            continue;
        }
        let text = body
            .trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '│' | '┃'))
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        match paragraphs.last_mut() {
            Some(last) if open => {
                last.push(' ');
                last.push_str(&text);
            }
            _ => paragraphs.push(text),
        }
        open = true;
    }
    let stock = paragraphs.iter().rev().find_map(|p| {
        let at = STOCK_QUESTIONS.iter().filter_map(|q| p.rfind(q)).max()?;
        let rest = &p[at..];
        Some(rest.find('?').map_or(rest, |end| &rest[..=end]).to_string())
    });
    let question = || {
        let p = paragraphs.iter().rev().find(|p| p.ends_with('?'))?;
        let body = p.trim_end_matches('?');
        let start = [". ", "! ", "? "]
            .iter()
            .filter_map(|end| body.rfind(end).map(|i| i + end.len()))
            .max()
            .unwrap_or(0);
        Some(p[start..].to_string())
    };
    stock
        .or_else(question)
        .or_else(|| paragraphs.first().cloned())
}

/// Whether the screen `rows` shows one of Claude's or Codex's marks at all,
/// on an option or a prompt.
pub fn marks_in<S: AsRef<str>>(rows: &[S]) -> bool {
    rows.iter().any(|row| marked(row.as_ref()).is_some())
}

/// A row that opens with a mark: the mark, the text after it, and the
/// column that text starts at.
fn marked(row: &str) -> Option<(char, &str, usize)> {
    let at = body_column(row);
    let body = row.trim_start_matches(lead);
    let mark = body.chars().next().filter(|c| MARKS.contains(c))?;
    let rest = &body[mark.len_utf8()..];
    if !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    Some((mark, rest.trim(), at + 2))
}

/// What a row's text is indented by: whitespace and a frame's side.
fn lead(c: char) -> bool {
    c.is_whitespace() || matches!(c, '│' | '┃')
}

/// The column a row's text starts at, past its indent.
fn body_column(row: &str) -> usize {
    row.chars().take_while(|c| lead(*c)).count()
}

/// A rule across the screen, or a frame's top or bottom: solid, never the
/// dashed one round a dialog's preview.
fn is_rule(row: &str) -> bool {
    let body = row.trim_start_matches(lead);
    body.chars()
        .next()
        .is_some_and(|c| ('\u{2500}'..='\u{257F}').contains(&c))
        && !body.contains('╌')
}

/// A dialog's key legend (`Enter to confirm · Esc to cancel`).
fn legend(row: &str) -> bool {
    ["Enter to ", "Esc to "].iter().any(|key| row.contains(key))
}

/// Does the input box on the screen in `tail` hold nothing the user typed?
/// Its row is the last one that opens on a prompt mark, past any frame,
/// and everything after the mark must be blank or drawn dim: Claude draws
/// the suggestion it offers in an empty box dim. A screen with no such row
/// has no box to type into.
pub fn box_is_empty(tail: &crate::OutputTail) -> bool {
    let mut parser = vt100::Parser::new(tail.rows.max(2), tail.cols.max(2), 0);
    parser.process(&tail.data);
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let blank = |c: &vt100::Cell| {
        c.contents()
            .trim_matches(|ch: char| ch.is_whitespace() || ch == '│' || ch == '┃')
            .is_empty()
    };
    for row in (0..rows).rev() {
        let Some(at) = (0..cols).find(|&c| screen.cell(row, c).is_some_and(|c| !blank(c))) else {
            continue;
        };
        let mark = screen.cell(row, at).map(vt100::Cell::contents);
        if !matches!(mark, Some("❯" | "›" | ">")) {
            continue;
        }
        return (at + 1..cols).all(|c| screen.cell(row, c).is_none_or(|c| blank(c) || c.dim()));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str) -> Vec<&str> {
        text.lines().collect()
    }

    fn screen(name: &str) -> Vec<&'static str> {
        match name {
            "claude-bash" => include_str!("why/screens/claude-bash.txt"),
            "claude-edit" => include_str!("why/screens/claude-edit.txt"),
            "claude-ask" => include_str!("why/screens/claude-ask.txt"),
            "claude-trust" => include_str!("why/screens/claude-trust.txt"),
            "codex-run" => include_str!("why/screens/codex-run.txt"),
            _ => unreachable!(),
        }
        .lines()
        .collect()
    }

    /// A numbered dialog with its first option selected.
    fn choice(question: &str, options: &[&str]) -> Choice {
        Choice {
            question: question.into(),
            options: (1..)
                .zip(options)
                .map(|(n, o)| (n, o.to_string()))
                .collect(),
            selected: 1,
            numbered: true,
        }
    }

    /// The dialogs real CLIs draw (Claude Code 2.1.319, Codex 0.163): what
    /// each asks, and its options, without a description under one or a
    /// key hint after it, and never a diff line of an edit's preview.
    #[test]
    fn a_dialog_says_what_it_asks_and_what_it_offers() {
        assert_eq!(
            choice_in(&screen("claude-bash")),
            Some(choice(
                "Do you want to proceed?",
                &[
                    "Yes",
                    "Yes, and don’t ask again for: cargo build *",
                    "Yes, and switch to auto mode · auto mode handles these prompts for you",
                    "No",
                ]
            ))
        );
        assert_eq!(
            choice_in(&screen("claude-edit")),
            Some(choice(
                "Do you want to make this edit to session.rs?",
                &[
                    "Yes",
                    "Yes, and switch to accept edits (auto-approve file edits and common file",
                    "No",
                ]
            ))
        );
        assert_eq!(
            choice_in(&screen("claude-ask")),
            Some(choice(
                "Which log format should the daemon write?",
                &[
                    "JSON lines",
                    "Plain text",
                    "Type something.",
                    "Chat about this"
                ]
            ))
        );
        assert_eq!(
            choice_in(&screen("codex-run")),
            Some(choice(
                "Would you like to run the following command?",
                &[
                    "Yes, proceed",
                    "Yes, and don't ask again for commands that start with `cargo build`",
                    "No, and tell Codex what to do differently",
                ]
            ))
        );
    }

    /// An unnumbered dialog's options are numbered by their place, the
    /// selection wherever it is among them.
    #[test]
    fn an_unnumbered_dialog_numbers_its_options_in_order() {
        assert_eq!(
            choice_in(&screen("claude-trust")),
            Some(Choice {
                numbered: false,
                ..choice(
                    "Is this a project you created or one you trust?",
                    &["No, exit", "Yes, I trust this folder"],
                )
            })
        );
        let notice = rows(
            " ───────────────\n  \
             Make auto mode your default permission mode?\n\n      \
             Yes, set auto mode as my default permission mode\n    \
             ❯ No, keep manual mode\n      \
             Ask me later\n\n\n",
        );
        assert_eq!(
            choice_in(&notice),
            Some(Choice {
                selected: 2,
                numbered: false,
                ..choice(
                    "Make auto mode your default permission mode?",
                    &[
                        "Yes, set auto mode as my default permission mode",
                        "No, keep manual mode",
                        "Ask me later",
                    ],
                )
            })
        );
        assert_eq!(
            choice_in(&rows("──────────\n ❯ now add a test\n──────────")),
            None
        );
    }

    /// Moved to the last option, a dialog has its other options above the
    /// selection and none under it: Claude Code 2.1.322's question
    /// whether to trust the folder, once Down chose `Yes`.
    #[test]
    fn an_unnumbered_dialog_selected_on_its_last_option_is_one() {
        let trust = rows(
            " Quick safety check: Is this a project you created or one you trust?\n\n \
             Security guide\n\n   \
             No, exit\n \
             ❯ Yes, I trust this folder\n\n \
             Enter to confirm · Esc to cancel",
        );
        let choice = choice_in(&trust).expect("a dialog");
        assert_eq!(
            choice.options,
            [
                (1, "No, exit".into()),
                (2, "Yes, I trust this folder".into())
            ]
        );
        assert_eq!((choice.selected, choice.numbered), (2, false));
        let notice = rows(
            "  Make auto mode your default permission mode?\n\n      \
             Yes, set auto mode as my default permission mode\n    \
             ❯ No, keep manual mode",
        );
        assert_eq!(choice_in(&notice).map(|c| c.selected), Some(2));
    }

    #[test]
    fn claudes_trust_question_and_a_notice_are_dialogs() {
        let trust = rows(
            " Claude Code v2.1\n\
             ──────────────────────\n \
             Accessing workspace:\n\n \
             Quick safety check: Is this a project you created or one you trust?\n\n \
             ❯ No, exit\n   \
             Yes, I trust this folder\n\n \
             Enter to confirm · Esc to cancel\n",
        );
        let dialog = dialog_in(&trust).unwrap();
        assert_eq!(dialog[0], " Accessing workspace:", "from the rule over it");
        assert_eq!(dialog.len(), trust.len() - 2);
        let notice = rows(
            " ───────────────\n  \
             Make auto mode your default permission mode?\n\n    \
             ❯ Yes, set auto mode as my default permission mode\n      \
             No, keep manual mode\n\n\n",
        );
        assert!(dialog_in(&notice).is_some());
    }

    #[test]
    fn a_numbered_dialog_is_one_for_claude_and_codex() {
        let claude = rows(
            " Do you want to proceed?\n \
             ❯ 1. Yes\n   \
             2. No\n\n \
             Esc to cancel · Tab to amend",
        );
        assert!(dialog_in(&claude).is_some());
        let codex = rows("  Allow command?\n  › 1. Yes, proceed\n    2. No");
        assert!(dialog_in(&codex).is_some());
        let moved = rows(" Do you want to proceed?\n   1. Yes\n ❯ 2. No\n\n Esc to cancel");
        let moved = choice_in(&moved).unwrap();
        assert_eq!((moved.selected, moved.numbered), (2, true));
    }

    /// The input box is up: its prompt is the screen's last mark, under
    /// its rule, whatever prompts the conversation shows above it, one
    /// with an option-like line under it included.
    #[test]
    fn the_input_box_and_past_prompts_are_no_dialog() {
        let at_rest = rows(
            " ❯ fix the login loop\n   \
             and the redirect\n\n \
             ⏺ Done.\n\n\
             ─────────────────────\n \
             ❯\n\
             ─────────────────────\n  \
             ? for shortcuts",
        );
        assert!(dialog_in(&at_rest).is_none());
        let typed = rows("──────────\n ❯ now add a test\n──────────");
        assert!(dialog_in(&typed).is_none());
        let past = rows(" ❯ fix the login loop\n\n ⏺ Looking at it.\n   Ran 2 commands");
        assert!(
            dialog_in(&past).is_none(),
            "a past prompt, the reply under it"
        );
        let wrapped =
            rows("  It said so.\n ❯ fix the login loop\n   and the redirect\n\n ⏺ On it.");
        assert!(
            dialog_in(&wrapped).is_none(),
            "a reply over it and under it"
        );
        let codex = rows("› Explain this codebase\n\n  100% context left");
        assert!(dialog_in(&codex).is_none(), "Codex's composer");
        assert!(dialog_in::<&str>(&[]).is_none());
        assert!(marks_in(&codex) && !marks_in(&rows("$ ls\nREADME.md")));
    }
}
