//! The answers a reply offers on its `NEXT: a | b | c` line, read by the
//! rule Claude Code's `quick-answers` mod draws its pills by
//! (`claude/mods/quick-answers/hooks/register.tsx` in the dotfiles), so the
//! web app offers exactly the answers the terminal does. The line counts as
//! the reply's last written line or the one before it, a report line being
//! allowed to close the reply, and it splits on the bars outside a code
//! span, at most four answers, the recommended one first.

const MAX_ANSWERS: usize = 4;

/// What JavaScript's `\s` matches: Unicode's white space, NEL aside, and
/// the byte order mark.
fn is_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

/// The answers part of a `NEXT:` line, the mod's
/// `/^[\s*_`>-]*NEXT:[\s*_`]*(.+)$/`: `None` when `line` is not one.
fn next_line(line: &str) -> Option<&str> {
    let rest = line
        .trim_start_matches(|c| is_space(c) || matches!(c, '*' | '_' | '`' | '>' | '-'))
        .strip_prefix("NEXT:")?;
    // `.` matches no line terminator, and `$` is the end of the line.
    if rest.is_empty() || rest.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return None;
    }
    let body = rest.trim_start_matches(|c| is_space(c) || matches!(c, '*' | '_' | '`'));
    // The greedy run gives one character back for `(.+)` to match.
    Some(match body {
        "" => rest.char_indices().last().map_or(rest, |(i, _)| &rest[i..]),
        body => body,
    })
}

/// Where a reply's `NEXT:` line is among its `lines`: its last line with
/// anything on it, or the one before that.
fn next_line_at(lines: &[&str]) -> Option<usize> {
    let written: Vec<usize> = (0..lines.len())
        .filter(|&i| !lines[i].trim_matches(is_space).is_empty())
        .collect();
    written
        .iter()
        .rev()
        .take(2)
        .copied()
        .find(|&i| next_line(lines[i]).is_some())
}

/// A `NEXT:` line's answers: split on the bars that stand outside a code
/// span, so `a || b` in backticks stays one answer, at most four.
fn answers_in(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut answer = String::new();
    let mut in_code = false;
    for c in next_line(line).unwrap_or_default().chars() {
        if c == '`' {
            in_code = !in_code;
        }
        if c == '|' && !in_code {
            found.push(std::mem::take(&mut answer));
        } else {
            answer.push(c);
        }
    }
    found.push(answer);
    found
        .iter()
        .map(|a| a.trim_matches(|c| is_space(c) || matches!(c, '*' | '_')))
        .filter(|a| !a.is_empty())
        .take(MAX_ANSWERS)
        .map(str::to_string)
        .collect()
}

/// The answers `reply` offers on its `NEXT: a | b | c` line, none when it
/// has no such line where one counts.
pub fn answers_of(reply: &str) -> Vec<String> {
    let lines: Vec<&str> = reply.split('\n').collect();
    next_line_at(&lines).map_or_else(Vec::new, |at| answers_in(lines[at]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers(reply: &str) -> Vec<String> {
        answers_of(reply)
    }

    /// The replies the mod's own tests (`register.test.tsx`) offer answers
    /// for, and the answers they expect.
    #[test]
    fn a_reply_ending_on_a_next_line_offers_its_answers() {
        assert_eq!(
            answers(
                "Done with the middleware.\n\nNEXT: Per token | Per IP | Per token, admins exempt"
            ),
            ["Per token", "Per IP", "Per token, admins exempt"]
        );
        assert_eq!(
            answers("Which one?\nNEXT: Per token | Per IP"),
            ["Per token", "Per IP"]
        );
        assert_eq!(
            answers(
                "Which store?\nNEXT: SQLite, one file with zero setup | PostgreSQL, full SQL, run locally or hosted"
            ),
            [
                "SQLite, one file with zero setup",
                "PostgreSQL, full SQL, run locally or hosted"
            ]
        );
        assert_eq!(
            answers("Which one?\n**NEXT:** Per token | Per IP\n"),
            ["Per token", "Per IP"]
        );
    }

    #[test]
    fn a_reply_with_no_next_line_offers_none() {
        assert!(answers("All tests pass.").is_empty());
        assert!(answers("").is_empty());
    }

    /// `the NEXT line counts last or just before a closing line, and bars
    /// in code stay put`, case for case.
    #[test]
    fn the_next_line_counts_last_or_just_before_a_closing_line() {
        assert_eq!(answers("Done.\nNEXT: Yes | No"), ["Yes", "No"]);
        assert_eq!(
            answers("Which port?\nNEXT: 8080 | 3000\nQUESTION which port should it use?"),
            ["8080", "3000"]
        );
        assert!(answers("NEXT: a | b\nmore\nand more").is_empty());
        assert_eq!(
            answers("Pick one.\nNEXT: Use `a || b` | Use `a ?? b`"),
            ["Use `a || b`", "Use `a ?? b`"]
        );
        assert_eq!(
            answers("- NEXT: one | two | three | four | five"),
            ["one", "two", "three", "four"]
        );
        assert!(answers("Hm.\nNEXT: | |").is_empty());
    }

    /// Where the regex decides: a line of nothing but emphasis is still
    /// the `NEXT:` line, so an earlier one is not read in its place, and a
    /// line the regex cannot match to its end (a carriage return) is none.
    #[test]
    fn the_line_is_matched_as_the_mods_regex_matches_it() {
        assert!(answers("NEXT: a | b\nNEXT: **").is_empty());
        assert_eq!(answers("NEXT: a | b\nNEXT: x\r"), ["a", "b"]);
        assert_eq!(answers("> `NEXT:` _a_ | *b*"), ["a", "b"]);
        assert!(answers("NEXT:").is_empty());
    }
}
