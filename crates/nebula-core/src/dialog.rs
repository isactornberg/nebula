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
//! is up. An unnumbered option is one only with more options under it and
//! nothing else but a key legend, as a past prompt has the conversation
//! going on below it.

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
        below.iter().any(|r| option(r.as_ref()))
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
        let codex = rows("› Explain this codebase\n\n  100% context left");
        assert!(dialog_in(&codex).is_none(), "Codex's composer");
        assert!(dialog_in::<&str>(&[]).is_none());
        assert!(marks_in(&codex) && !marks_in(&rows("$ ls\nREADME.md")));
    }
}
