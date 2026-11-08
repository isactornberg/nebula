//! The question an agent CLI asks before it starts in a folder it has no
//! trust for yet. It comes before the CLI runs a single hook, its answer is
//! a keypress, and the answer it defaults to can quit: Claude Code 2.1
//! defaults to `No, exit` in a folder whose settings pre-approve tools, as
//! nebula's hook install makes every checkout's. The daemon reads it off a
//! starting session's screen, and the board says it under the row.

/// Each CLI's wording of the question, as it prints it.
const QUESTIONS: [&str; 3] = [
    // Claude Code 2.1.
    "Is this a project you created or one you trust?",
    // Claude Code before it.
    "Do you trust the files in this folder?",
    // cursor-agent.
    "Do you trust the contents of this directory?",
];

/// The trust question `text`, a screen's text however it wraps, shows, as
/// the CLI words it.
pub fn question_in(text: &str) -> Option<&'static str> {
    let text = folded(text);
    QUESTIONS
        .into_iter()
        .find(|q| text.contains(&folded(q.trim_end_matches('?'))))
}

/// The trust question the screen `rows` (top to bottom) asks: in the
/// choice dialog it ends on (`crate::dialog`), or, on a screen with none of
/// the marks Claude and Codex draw a prompt or an option with, anywhere on
/// it, as another CLI may draw its question. A conversation that only
/// quotes the question, above the input box, asks nothing.
pub fn question_on<S: AsRef<str>>(rows: &[S]) -> Option<&'static str> {
    let joined = |rows: &[S]| {
        rows.iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join("\n")
    };
    match crate::dialog::dialog_in(rows) {
        Some(dialog) => question_in(&joined(dialog)),
        None if crate::dialog::marks_in(rows) => None,
        None => question_in(&joined(rows)),
    }
}

/// `text` lowercased, with every run of whitespace and frame sides one
/// space, so a question a narrow pane wraps, or a box borders, still reads
/// as one line.
fn folded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text
        .split(|c: char| c.is_whitespace() || c == '│' || c == '┃')
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(word.chars().flat_map(char::to_lowercase));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_clis_question_is_found_however_it_wraps() {
        let claude =
            " Quick safety check: Is this a project you created or one\n │ you trust? (Like\n";
        assert_eq!(question_in(claude), Some(QUESTIONS[0]));
        assert_eq!(
            question_in("│ Do you trust the files │\n│ in this folder? │"),
            Some(QUESTIONS[1])
        );
        assert_eq!(
            question_in("  Do you trust the contents of this directory?"),
            Some(QUESTIONS[2])
        );
        assert_eq!(question_in("Do you trust me?"), None);
    }

    /// Only a dialog asks it: a conversation quoting it above Claude's
    /// input box, or above a permission dialog, does not.
    #[test]
    fn the_question_is_asked_only_in_the_dialog_the_screen_ends_on() {
        let asked = [
            "──────────",
            " Quick safety check: Is this a project you created or one you trust?",
            "",
            " ❯ No, exit",
            "   Yes, I trust this folder",
        ];
        assert_eq!(question_on(&asked), Some(QUESTIONS[0]));
        let quoted = [
            " ⏺ Claude asks \"Is this a project you created or one you trust?\" first.",
            "──────────",
            " ❯",
            "──────────",
        ];
        assert_eq!(question_on(&quoted), None, "the input box is up");
        let permission = [
            " ⏺ It asks: Is this a project you created or one you trust?",
            "──────────",
            " Bash command",
            "   rm -rf build",
            " Do you want to proceed?",
            " ❯ 1. Yes",
            "   2. No",
        ];
        assert_eq!(question_on(&permission), None, "another dialog");
        let unmarked = [
            "  Do you trust the contents of this directory?",
            "  [a] Trust",
        ];
        assert_eq!(question_on(&unmarked), Some(QUESTIONS[2]));
    }
}
