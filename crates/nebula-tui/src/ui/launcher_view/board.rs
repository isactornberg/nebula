//! The ORCHESTRATOR VIEW's BOARD (`crate::board` is its model): a column
//! left of the session on screen, its header counting what is on it, then
//! a section each for `needs you`, `done` and `running`, and the rest
//! counted on one dim line. A row is the session's status dot, its project
//! and title, and how long it has been in that state; a session in a
//! WORKTREE of its own, or on a branch with a pull request, names them on a
//! dim line under that (`rel-913-distributor  ↗ #426`); a `needs you` or
//! `done` row says why on a dim line under it. A CAPTAIN's row names its
//! project and the workers it has at work instead (`● api captain · 2
//! workers`), and captains at rest have a section of their own after
//! `running`. A finished row's NEXT STEPS are listed under it, and a click
//! on one sends it to that session. Only the steps of the session on
//! screen are numbered, as `F1`–`F4` send those: the lit row's, or, with
//! the orchestrator's chat on screen, its own, in an `orchestrator
//! suggests` block at the top. The session on screen is the lit row, and a
//! click on any row shows that one.

use super::nested::{fill_selected, thread_age};
use super::status_dot;
use crate::app::{App, HitTarget};
use crate::board::{Board, BoardRow, RowKind, Section};
use crate::launcher::PAD_X;
use crate::theme::Theme;
use crate::ui::fit_to_width;
use nebula_core::SessionRef;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::ops::Range;

/// The age column, right-aligned: `now`, `6m`, `2h`.
const AGE_W: usize = 3;
/// The air before the age column, as between a row's project and its
/// title.
const AGE_GAP: usize = 2;
/// The air between a row's project and its title.
const PROJECT_GAP: usize = 2;
/// Fewest cells a title keeps before its project goes.
const MIN_TITLE: usize = 8;
/// Fewest cells a project clips to before the title gives way.
const MIN_PROJECT: usize = 4;
/// Fewest cells a cut of a project is worth: a shorter one hardly saves
/// room and makes the name harder to read, so the title gives way instead.
const MIN_CUT: usize = 3;
/// What the header says with no row on the board.
const QUIET: &str = "nothing needs you";
/// The heading over the orchestrator's own NEXT STEPS.
const SUGGESTS: &str = "orchestrator suggests";
/// What an unnumbered step is marked with: `F1`–`F4` do not send it.
const STEP_MARK: &str = "› ";

/// One line or block of the board, top to bottom.
enum Entry<'a> {
    Counts,
    Gap,
    Heading(Section),
    /// The orchestrator's NEXT STEPS while its chat is on screen.
    Suggests,
    Step(usize, &'a str),
    Row(&'a BoardRow),
    /// The rows the board had no room for, scrolled off above it and
    /// left off below, and the rest.
    More(usize, usize, String),
    Rest(String),
}

impl Entry<'_> {
    fn height(&self) -> u16 {
        match self {
            Entry::Row(row) => said(row) + row.next.len() as u16,
            _ => 1,
        }
    }
}

/// The lines a row says of itself, over its NEXT STEPS: its title, its
/// checkout and why it is on the board.
fn said(row: &BoardRow) -> u16 {
    1 + u16::from(row.checkout) + u16::from(row.section.reads())
}

/// Which of the board's rows it draws in `height` lines, `top` of them
/// taken by the lines over the first section: from `scroll`, the row the
/// wheel scrolled it to, as many as fit; else as many of the first as fit,
/// or, when that would leave the lit row (`lit`) off, the run of rows
/// ending at it, so `F9` never lands on a row not drawn. The rows left off
/// are counted on the last line.
fn shown(
    board: &Board,
    lit: Option<usize>,
    scroll: Option<usize>,
    top: u16,
    height: u16,
) -> Range<usize> {
    let rows = &board.rows;
    if let Some(from) = scroll {
        // No further than the run that ends at the last row.
        let last = shown(board, rows.len().checked_sub(1), None, top, height);
        return run_from(board, from.min(last.start), top, height);
    }
    let first = run_from(board, 0, top, height);
    let Some(lit) = lit.filter(|&l| l >= first.end) else {
        return first;
    };
    let lines = |i: usize, opens: bool| Entry::Row(&rows[i]).height() + 2 * u16::from(opens);
    // Back from the lit row, under the top lines and over the `+N more`
    // line.
    let mut used = top + 1;
    let mut from = lit + 1;
    while from > 0 {
        let i = from - 1;
        let h = lines(i, i == lit || rows[i + 1].section != rows[i].section);
        if used + h > height && i != lit {
            break;
        }
        used += h;
        from = i;
    }
    from..lit + 1
}

/// The rows from `from` on that fit in `height` lines under `top` ones:
/// each with the gap and heading of a section it opens, and room after it
/// for the `+N more` line, or, after the last row with none scrolled off,
/// the rest's gap and line.
fn run_from(board: &Board, from: usize, top: u16, height: u16) -> Range<usize> {
    let rows = &board.rows;
    let mut used = top;
    let mut k = from;
    for i in from..rows.len() {
        let opens = i == from || rows[i - 1].section != rows[i].section;
        let h = Entry::Row(&rows[i]).height() + 2 * u16::from(opens);
        let after = if i + 1 < rows.len() || from > 0 {
            1
        } else {
            2 * u16::from(board.idle + board.stopped > 0)
        };
        if used + h + after > height {
            break;
        }
        used += h;
        k = i + 1;
    }
    from..k
}

/// The lines over the board's first section: its counts, and the
/// orchestrator's `suggests` block over its `steps`.
fn top_entries(steps: &[String]) -> Vec<Entry<'_>> {
    let mut out = vec![Entry::Counts];
    if !steps.is_empty() {
        out.extend([Entry::Gap, Entry::Suggests]);
        out.extend(steps.iter().enumerate().map(|(i, s)| Entry::Step(i, s)));
    }
    out
}

/// The board's lines under `top` with the rows in `shown`, and what it
/// leaves off.
fn entries<'a>(board: &'a Board, top: Vec<Entry<'a>>, shown: Range<usize>) -> Vec<Entry<'a>> {
    let mut out = top;
    let (above, below) = (shown.start, board.rows.len() - shown.end);
    let rows = &board.rows[shown];
    for section in Section::ALL {
        let mut rows = rows.iter().filter(|r| r.section == section).peekable();
        if rows.peek().is_none() {
            continue;
        }
        out.push(Entry::Gap);
        out.push(Entry::Heading(section));
        out.extend(rows.map(Entry::Row));
    }
    let rest = board.rest();
    if above + below > 0 {
        out.push(Entry::More(above, below, rest));
    } else if !rest.is_empty() {
        out.push(Entry::Gap);
        out.push(Entry::Rest(rest));
    }
    out
}

/// The board down the left of `body`, the body under the view's header.
/// Returns what it leaves: where the session on screen draws.
pub(super) fn draw(f: &mut Frame, app: &mut App, body: Rect) -> Rect {
    let th = app.chrome.theme;
    let w = crate::board::width(body.width).min(body.width);
    let area = Rect { width: w, ..body };
    let rest = Rect {
        x: area.right(),
        width: body.width - w,
        ..body
    };
    if area.width <= PAD_X + 2 || area.height == 0 {
        return rest;
    }
    // The keys are the session's, not the board's: the FOCUSED PANEL TINT
    // washes the session beside it, and the board keeps the canvas.
    if app.chrome.black_background {
        f.buffer_mut()
            .set_style(area, Style::default().bg(th.canvas));
    }
    let seam_x = area.right() - 1;
    for y in area.y..area.bottom() {
        if let Some(cell) = f.buffer_mut().cell_mut((seam_x, y)) {
            cell.set_symbol("│").set_fg(th.edge);
        }
    }
    // One column of air either side of the seam, as the session has.
    let inner = Rect {
        x: area.x + PAD_X,
        width: seam_x.saturating_sub(area.x + PAD_X + 1),
        ..area
    };

    let board = crate::board::board(app);
    let showing = app.pane.term.as_ref().map(|t| t.sref.clone());
    let lit = board
        .rows
        .iter()
        .position(|r| showing.as_ref() == Some(&SessionRef::Agent(r.id.clone())));
    // The orchestrator's own steps, while its chat is the session on screen.
    let orchestrator = app
        .orchestrator_agent()
        .filter(|o| showing == Some(SessionRef::Agent(o.id.clone())))
        .map(|o| (o.id.clone(), crate::board::next_of(app, o)));
    let suggested = orchestrator.as_ref().map_or(&[][..], |(_, s)| s.as_slice());
    let top = top_entries(suggested);
    let top_h = top.iter().map(Entry::height).sum();
    let range = shown(&board, lit, app.launcher.board_scroll, top_h, inner.height);
    app.launcher.board_top = range.start;
    let mut y = inner.y;
    for entry in entries(&board, top, range) {
        let h = entry.height();
        if y + h > inner.bottom() {
            break;
        }
        let r = Rect {
            y,
            height: h,
            ..inner
        };
        match entry {
            Entry::Counts => line(f, r, counts(&board, r.width, th)),
            Entry::Gap => {}
            Entry::Heading(section) => line(
                f,
                r,
                Line::from(Span::styled(
                    section.title(),
                    Style::default()
                        .fg(section_color(section, th))
                        .add_modifier(Modifier::BOLD),
                )),
            ),
            Entry::Suggests => line(
                f,
                r,
                Line::from(Span::styled(
                    SUGGESTS,
                    Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
                )),
            ),
            Entry::Step(i, step) => {
                if let Some((id, _)) = &orchestrator {
                    let hit = HitTarget::BoardNextStep(id.clone(), i);
                    app.chrome.hits.push((
                        Rect {
                            x: area.x,
                            width: seam_x - area.x,
                            ..r
                        },
                        hit.clone(),
                    ));
                    line(f, r, step_line(app, &hit, i, step, true, r.width, true));
                }
            }
            Entry::Row(row) => {
                let sref = SessionRef::Agent(row.id.clone());
                let lit = showing.as_ref() == Some(&sref);
                if lit {
                    fill_selected(
                        f.buffer_mut(),
                        Rect {
                            width: seam_x - r.x,
                            ..r
                        },
                        th,
                    );
                }
                draw_row(f, app, r, row, lit);
                let said = said(row);
                for i in 0..row.next.len() {
                    app.chrome.hits.push((
                        Rect {
                            x: area.x,
                            y: r.y + said + i as u16,
                            width: seam_x - area.x,
                            height: 1,
                        },
                        HitTarget::BoardNextStep(row.id.clone(), i),
                    ));
                }
                app.chrome.hits.push((
                    Rect {
                        x: area.x,
                        width: seam_x - area.x,
                        height: said,
                        ..r
                    },
                    HitTarget::OrchestratorChip(sref),
                ));
                if row.section.reads() {
                    app.pane.board_cards.push(row.id.clone());
                }
            }
            Entry::More(above, below, rest) => {
                let text = [(above, "above"), (below, "more")]
                    .into_iter()
                    .filter(|(n, _)| *n > 0)
                    .map(|(n, word)| format!("+{n} {word}"))
                    .chain((!rest.is_empty()).then_some(rest))
                    .collect::<Vec<_>>()
                    .join(" · ");
                line(f, r, dim_line(text, th));
            }
            Entry::Rest(rest) => line(f, r, dim_line(rest, th)),
        }
        y += h;
    }
    // A finish read before this client came up has no row, and may have
    // asked: each is read once, and one that did comes back under `needs
    // you`.
    let unread: Vec<_> = app
        .strip_sessions()
        .into_iter()
        .filter(|id| {
            app.tree.agents.iter().any(|a| {
                a.id == *id
                    && a.status == nebula_core::AgentStatus::Finished
                    && !a.unseen
                    && crate::board::read_of(app, a).is_none()
            })
        })
        .collect();
    app.pane.board_cards.extend(unread);
    // Under the rows, the wheel scrolls the board, never the chat.
    app.chrome.hits.push((
        Rect {
            width: seam_x - area.x,
            ..area
        },
        HitTarget::Board,
    ));
    rest
}

/// The cells `s` takes on screen.
fn cells(s: &str) -> usize {
    Span::raw(s).width()
}

fn line(f: &mut Frame, r: Rect, line: Line<'static>) {
    f.render_widget(Paragraph::new(line), Rect { height: 1, ..r });
}

fn dim_line(text: String, th: Theme) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(th.dim)))
}

fn section_color(section: Section, th: Theme) -> Color {
    match section {
        Section::NeedsYou => th.err,
        Section::Done => th.done,
        Section::Running => th.warn,
        Section::Captains => th.muted,
    }
}

/// The header: `2 need you · 1 done · 3 running`, each number in its
/// section's color; in a board too narrow for that, the numbers alone,
/// the sections under it saying which is which.
fn counts(board: &Board, width: u16, th: Theme) -> Line<'static> {
    let counted: Vec<(Section, &str, usize)> = Section::ALL
        .into_iter()
        .filter_map(|s| {
            let n = board.count(s);
            (n > 0).then_some((s, s.count_word(n)?, n))
        })
        .collect();
    if counted.is_empty() {
        return dim_line(QUIET.into(), th);
    }
    let line = |words: bool| {
        let mut spans = Vec::new();
        for (section, word, n) in &counted {
            if !spans.is_empty() {
                spans.push(Span::styled(" · ", Style::default().fg(th.dim)));
            }
            spans.push(Span::styled(
                n.to_string(),
                Style::default()
                    .fg(section_color(*section, th))
                    .add_modifier(Modifier::BOLD),
            ));
            if words {
                spans.push(Span::styled(
                    format!(" {word}"),
                    Style::default().fg(th.muted),
                ));
            }
        }
        Line::from(spans)
    };
    let full = line(true);
    if full.width() <= usize::from(width) {
        full
    } else {
        line(false)
    }
}

/// One row: `● project  Title   6m`, a CAPTAIN's `● project captain · 2
/// workers   6m`, the checkout's line under a row that has one, under a
/// `needs you` or `done` row the dim line saying why, blank until the
/// board has read it, and a line for each NEXT STEP, numbered.
fn draw_row(f: &mut Frame, app: &App, r: Rect, row: &BoardRow, lit: bool) {
    let th = app.chrome.theme;
    let Some(a) = app.tree.agents.iter().find(|a| a.id == row.id) else {
        return;
    };
    let width = usize::from(r.width);
    let project = crate::launcher::row(app, &row.id)
        .map(|l| l.project)
        .unwrap_or_default();
    let age = thread_age(a.status_changed_at);
    let age_w = cells(&age).max(AGE_W);
    // The dot, the age and the air before it.
    let room = width.saturating_sub(2 + AGE_GAP + age_w);
    let mut title_style = Style::default().fg(th.text);
    // On the lit row's fill the dim parts brighten a step, as on any
    // selection bar.
    let quiet = Style::default().fg(if lit { th.muted } else { th.dim });
    if lit {
        title_style = title_style.add_modifier(Modifier::BOLD);
    }
    let sref = SessionRef::Agent(a.id.clone());
    if app.launcher.hover_crumb.as_ref() == Some(&HitTarget::OrchestratorChip(sref)) {
        title_style = title_style.add_modifier(Modifier::UNDERLINED);
    }
    let mut spans = vec![status_dot(Some(a.status), a.unseen, th)];
    let named = match row.kind {
        RowKind::Session => session_name(&project, &a.name, room, title_style, quiet),
        RowKind::Captain { workers } => captain_name(&project, workers, room, title_style, quiet),
    };
    let used = 2 + named.iter().map(|s| s.width()).sum::<usize>();
    spans.extend(named);
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used + age_w))));
    spans.push(Span::styled(format!("{age:>age_w$}"), quiet));
    line(f, r, Line::from(spans));
    let checkout = u16::from(row.checkout);
    if let Some((branch, pr)) = crate::board::checkout_of(app, a).filter(|_| row.checkout) {
        let spans = checkout_line(&branch, pr.as_ref(), width.saturating_sub(2), quiet, th);
        line(f, Rect { y: r.y + 1, ..r }, Line::from(spans));
    }
    let reads = row.section.reads();
    if reads && r.height >= 2 + checkout {
        let why = crate::board::read_of(app, a)
            .and_then(|read| read.line.as_deref())
            .unwrap_or_default();
        let why = fit_to_width(why, width.saturating_sub(2));
        let mut spans = vec![Span::raw("  ")];
        // `BLOCKED` and `QUESTION` change what the finish means: the word is
        // in the section's color.
        let word = crate::board::report_word(&why).unwrap_or_default();
        if !word.is_empty() {
            spans.push(Span::styled(
                word,
                Style::default().fg(section_color(row.section, th)),
            ));
        }
        spans.push(Span::styled(why[word.len()..].to_string(), quiet));
        let y = r.y + 1 + checkout;
        line(f, Rect { y, ..r }, Line::from(spans));
    }
    for (i, step) in row.next.iter().enumerate() {
        let y = r.y + 1 + checkout + u16::from(reads) + i as u16;
        if y >= r.bottom() {
            break;
        }
        let hit = HitTarget::BoardNextStep(row.id.clone(), i);
        // Only the session on screen's are `F1`–`F4`'s, so only its are
        // numbered.
        let spans = step_line(app, &hit, i, step, lit, r.width, lit);
        line(f, Rect { y, ..r }, spans);
    }
}

/// A row's checkout in `room` cells, under its title: the branch, cut
/// before the pull request is, then `↗ #426` in the color every pull
/// request row wears (`pr_row::look`).
fn checkout_line(
    branch: &str,
    pr: Option<&crate::launcher::RowPr>,
    room: usize,
    quiet: Style,
    th: Theme,
) -> Vec<Span<'static>> {
    let number = pr.map(|pr| format!("↗ #{}", pr.number)).unwrap_or_default();
    let gap = if number.is_empty() { 0 } else { PROJECT_GAP };
    let branch = fit_to_width(branch, room.saturating_sub(cells(&number) + gap));
    let mut spans = vec![Span::raw("  "), Span::styled(branch, quiet)];
    if let Some(pr) = pr {
        let look = crate::pr_row::look(pr.standing, pr.trouble, th);
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(Span::styled(number, Style::default().fg(look.glyph)));
    }
    spans
}

/// NEXT STEP `i`, `step`, on a line `width` wide under its row or the
/// `suggests` heading: numbered when it is `F1`–`F4`'s to send, else
/// marked, and underlined under the pointer. `bright` is a lit row's.
fn step_line(
    app: &App,
    hit: &HitTarget,
    i: usize,
    step: &str,
    numbered: bool,
    width: u16,
    bright: bool,
) -> Line<'static> {
    let th = app.chrome.theme;
    let (mark, mark_style) = if numbered {
        (format!("{} ", i + 1), Style::default().fg(th.accent))
    } else {
        (STEP_MARK.to_string(), Style::default().fg(th.dim))
    };
    let mut style = Style::default().fg(if bright { th.text } else { th.muted });
    if app.launcher.hover_crumb.as_ref() == Some(hit) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    let room = usize::from(width).saturating_sub(2 + cells(&mark));
    Line::from(vec![
        Span::raw("  "),
        Span::styled(mark, mark_style),
        Span::styled(fit_to_width(step, room), style),
    ])
}

/// A session's project and title in `room` cells. The project gives way to
/// the title first, down to a few letters, unless the cut would save less
/// than [`MIN_CUT`] cells; then the title does; and in a row with no room
/// for both, the project goes.
fn session_name(
    project: &str,
    title: &str,
    room: usize,
    title_style: Style,
    quiet: Style,
) -> Vec<Span<'static>> {
    let mut project_room = room
        .saturating_sub(cells(title) + PROJECT_GAP)
        .max(MIN_PROJECT);
    let whole = cells(project);
    if whole > project_room
        && whole - project_room < MIN_CUT
        && whole + PROJECT_GAP + MIN_TITLE <= room
    {
        project_room = whole;
    }
    let mut spans = Vec::new();
    let mut lead = 0;
    if !project.is_empty() && MIN_PROJECT + PROJECT_GAP + MIN_TITLE <= room {
        let project = fit_to_width(project, project_room);
        lead = cells(&project) + PROJECT_GAP;
        spans.push(Span::styled(project, quiet));
        spans.push(Span::raw(" ".repeat(PROJECT_GAP)));
    }
    spans.push(Span::styled(
        fit_to_width(title, room.saturating_sub(lead)),
        title_style,
    ));
    spans
}

/// A CAPTAIN's project, the title its row goes by, then ` captain` and
/// the workers it has at work, in `room` cells: the project gives way
/// first, down to [`MIN_TITLE`] cells, then the count, whole, then
/// ` captain`.
fn captain_name(
    project: &str,
    workers: usize,
    room: usize,
    title_style: Style,
    quiet: Style,
) -> Vec<Span<'static>> {
    const ROLE: &str = " captain";
    let mut role = ROLE.to_string();
    match workers {
        0 => {}
        1 => role.push_str(" · 1 worker"),
        n => role.push_str(&format!(" · {n} workers")),
    }
    if room.saturating_sub(cells(&role)) < MIN_TITLE.min(cells(project)) {
        role = ROLE.to_string();
    }
    let project_room = room.saturating_sub(cells(&role)).max(MIN_TITLE.min(room));
    let project = fit_to_width(project, project_room);
    let role = fit_to_width(&role, room.saturating_sub(cells(&project)));
    vec![
        Span::styled(project, title_style),
        Span::styled(role, quiet),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(project: &str, title: &str, room: usize) -> String {
        session_name(project, title, room, Style::default(), Style::default())
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    /// A long project is cut before its captain's count goes: the count
    /// leaves only once the project would keep fewer than a few letters.
    #[test]
    fn a_captains_count_outlasts_a_long_project() {
        let captain = |project: &str, room: usize| -> String {
            captain_name(project, 2, room, Style::default(), Style::default())
                .iter()
                .map(|s| s.content.as_ref())
                .collect()
        };
        assert_eq!(
            captain("relox-dashboard", 40),
            "relox-dashboard captain · 2 workers"
        );
        assert_eq!(
            captain("relox-dashboard", 30),
            "relox-das… captain · 2 workers"
        );
        assert_eq!(captain("relox-dashboard", 27), "relox-dashboard captain");
        assert_eq!(captain("api", 23), "api captain · 2 workers");
    }

    /// A project is cut only when the cut is worth it: `delta` stays whole
    /// beside a title that gives up the cell or two instead, where a long
    /// project gives way first.
    #[test]
    fn a_project_is_cut_only_when_the_cut_saves_some_room() {
        assert_eq!(
            name("delta", "Split the monolith", 24),
            "delta  Split the monoli…"
        );
        assert_eq!(
            name("payments-reconciliation-service", "Currency rounding", 30),
            "payments-r…  Currency rounding"
        );
        assert_eq!(
            name("demo", "Login redirect loop", 25),
            "demo  Login redirect loop"
        );
    }
}
