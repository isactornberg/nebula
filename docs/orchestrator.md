# Orchestrator

<sub>[← README](../README.md) · [Keys](keys.md) · [Commands](commands.md) · [Sessions](sessions.md) · [Configuration](configuration.md) · [How it works](how-it-works.md)</sub>

The ORCHESTRATOR view is a chat-first way to run nebula.
Instead of moving from project to project and opening each worktree by hand, the user keeps one agent session in front of them and asks it to coordinate the rest.

## Shape of the feature

- **One orchestrator session, in a home of its own.**
  The daemon persists a session role on agent rows, and only a row with the orchestrator role may call the orchestrator-only socket commands.
  `F10` starts one when there is none: a Claude session named `orchestrator` in the ORCHESTRATOR HOME, the `orchestrator` folder in nebula's data directory.
  The first `F10` makes that folder a project (created and `git init`ed, the way `nebula add` does for a folder that is not there yet), and no list of projects shows it: no tab, no card, no row in the palette or the move picker.
  It coordinates every project from outside all of them, and a `CLAUDE.md` in that folder gives it standing context of your own.
- **Chat-first layout.**
  `F10`, or a click on `orchestrator F10` at the right end of the grid's header, brings the orchestrator up full-screen with its input locked, so typing goes straight to it.
  With the `start_in_orchestrator` setting on (Settings → Sessions, off by default), every launch of nebula opens here instead of on the grid.
  In a terminal at least 100 columns wide, the BOARD runs down the left of the view, a third of its width, and the session on screen takes the rest.
  In a narrower one, the header row is `‹ sessions /` followed by a strip of every live session on the machine instead, the orchestrator first and the rest in the attention order `.` and `,` walk: needs feedback, running, unseen finishes, then the rest by recency.
  A captain's chip names its project (`● api-server captain`).
  The session on screen is the lit one, and what the row has no room for is counted at its end; when the session on screen would be among those, the strip scrolls until it is drawn, counting the sessions it passed after the orchestrator (`● orchestrator  +3  ● Dark mode toggle`).
- **The board.**
  It lists what needs attention across every project, in sections: `needs you` (sessions waiting on a permission or a question, red, and finishes whose report line asks a `QUESTION` or says `BLOCKED`), `done` (finishes nobody has read yet, blue), `running` (yellow), and then one dim line counting the rest: `idle` for the ones up and waiting, `stopped` for the ones whose process is gone, died while working or lost to a daemon restart (`3 idle · 1 stopped`).
  An empty section is left out, and the board's first line counts the others (`2 need you · 1 done · 4 running`).
  A row is the session's status dot, its project, its title and how long it has been in that state, the age in a column of its own two cells clear of the title; in a row too narrow for all of it, the project is cut short before the title is, unless the cut would save less than three cells.
  A session in a worktree of its own, or on a branch with a pull request, names them on a dim line under its title (`token-store  ↗ #57`), the pull request in the color its row wears on the grid: red when it conflicts or a check fails, purple once merged.
  A session in the root checkout with no pull request has no such line.
  A `needs you` row says what the session is asking on a dim line under it, read off its screen and never its options.
  A permission dialog that only asks for a yes says what it is about: the tool and what it runs on for Claude (`Bash: cargo build --release`, `Edit: src/auth/session.rs`), the `$ …` command line for Codex.
  Any other question, an `AskUserQuestion` or one at the end of a reply, is said as it is, and so is a CLI's question whether to trust the folder it starts in, which puts a session that has not started yet under `needs you`.
  A `done` row says how the turn ended: its last line that starts with `DONE`, `BLOCKED` or `QUESTION`, the report line agents end a turn with, else the start of the last paragraph the session wrote above its input box and its `NEXT:` line.
  Only the turn after the last prompt counts, and a tool's output in it does not (a captain's `nebula read` of a worker's `DONE` is not its own); the line leaves `DONE` off, as the section says it, and shows `BLOCKED` and `QUESTION` in the section's color.
  A finish whose report line is a `QUESTION` or `BLOCKED` waits on the user as a dialog does: once the board has read it, its row moves up under `needs you`, counted there and walked by `F9` with the others, its line as it was, and it stays there once you have read it, until the session starts another turn.
  A reply's NEXT STEPS are the answers it offers: every session nebula starts is told to end a reply that leaves the user something to answer or decide with one line `NEXT: <a> | <b> | <c>`, two to four answers of a few words each, the one it recommends first, and the orchestrator's and the captains' guidance says the same of an obvious follow-up.
  It is read off the latest turn as the report line is, wrapped or not, and never off a tool's output or the suggestion Claude types into the input box.
  The board never lists them: they belong to the chat, where the reply's own `NEXT:` line shows them, and where Claude Code's `quick-answers` mod draws them as a row over its input box, a digit or a click away.
  `F1` to `F4` send the session on screen its first to fourth step, from inside it too, as the follow-up box sends a turn.
  The footer names the keys, and whose steps they send (`F1-F2: orchestrator's steps`, `F1: api-server captain's step`), while the session on screen has any.
  The board reads a session's screen once each time its status changes, never on every frame: a beat after the change, so the screen has painted it, or at once for a change older than that, as when the board comes back after a while away.
  What it read stays for as long as the status does, so a return to the view shows every line on its first frame.
  The orchestrator is not a row: it is the chat.
  A project with a CAPTAIN is one row, the captain's: `● api-server captain · 2 workers`, counting the project's other sessions that are running or waiting on the user; in a board too narrow for all of it the project is cut short first, and the count is left out only once the project would keep fewer than 8 cells.
  It sits in the section the captain's own state puts it, its `DONE` report under it in `done`, a `QUESTION` or `BLOCKED` one under `needs you`, and while the captain is at rest in a `captains` section after `running`, which the first line does not count.
  The project's other sessions fold into it, all but the ones that need you, which stay rows of their own under `needs you`, and one put on screen, which is a row again for as long as it is there.
  A board taller than the screen keeps its first rows and counts the rest on its last line (`+4 more · 3 idle`); with the session on screen further down, it shows the run of rows that ends at that one instead, so the lit row is always drawn.
  `F8` hands the keys to the board: a highlight starts on the row of the session on screen, `j`/`k` or the arrows move it, `Enter` shows that row's session with the keys in it, and `Esc` or `F8` again gives the keys back to the session on screen.
  The row under the highlight is the one filled and kept drawn while the board has the keys, and nothing typed then reaches a session.
  The mouse wheel over the board scrolls it a row a notch, no further than its last rows, its last line counting the rows above and below it (`+2 above · +1 more · 3 idle`), and `F9` brings it back to the lit row.
- **Showing a session.**
  A click on a session in the strip, or on a row of the board, brings it up full-screen in place of the chat, still inside the view.
  So does `F9` (`Shift+F9` going back), which walks the board's rows from the session on screen from anywhere, a session included, or the strip in a narrow terminal, and the orchestrator's own `nebula orchestrator review <session-id>`, when the user asks to see something.
  With the board up, the header names the session on screen under the orchestrator (`‹ sessions / orchestrator / ● Fix the login loop`), and a click on `orchestrator` goes back to the chat.
  While any session needs you or has an unread finish, `F9` walks only those rows, triage first, and the `running` ones join the walk once none is left.
  A session answered or read on screen leaves the walk, and `F9` goes on from the place it had, so the rest of the queue is not skipped.
  Putting a finished turn up reads it, but its row stays under `done` for as long as it is on screen; it leaves the board once something else is shown.
  A captain is the exception, since it always has a row: on screen it stays in the section its own state puts it, under `captains` while at rest.
  `^q` (or `^F`, or `` ^` ``) goes back one step: from that session to the orchestrator, and from the orchestrator out to the grid.
- **Leaving.**
  `F10` from anywhere in the view, or a click on `‹ sessions`, goes back to the grid.
  The view never moves the grid's cursor, so the grid is where it was, its pane on the card under the cursor, or empty in a project with no session in it.
- **Lasting.**
  A dev-watch relaunch brings the view back on the session it showed, as it brings back any session left full-screen.
  So does a window that was closed, or a TUI that was killed: the layout is saved a moment after it changes, not only on a quit.

The view is a full-screen session underneath, so scrollback, selection, links and the input lock behave exactly as they do in any full-screen session.
While it is up, nothing else swaps the session on screen: a row leaving the grid under its cursor moves the cursor, not the view.

## Agent-facing commands

The orchestrator receives extra guidance in its system prompt.
It names every command below with its flags.
Project work goes to the project's CAPTAIN (below): the orchestrator gets it with `nebula orchestrator captain <project>`, `send`s it the request in the user's words, naming by id the sessions it is about, and runs `nebula session wait <captain-id> --until-done` as a background command, which returns once the captain is done and its report line can be read.
It never changes a project's files, or builds or runs anything in one, itself, however small the change: when `captain` or `send` fails, it tells the user why instead of doing the work another way.
A quick question about where things stand it answers itself from `orchestrator list`, without a captain hop.
It knows that text `read` shows in a session's input box is a suggestion the CLI offers, not a prompt anyone sent.
The user may also talk to a captain directly or pick one of its next steps, so work a captain reports that the orchestrator did not ask for is the user's own, and it reports it as theirs.
Work across several projects goes to each captain in its part, a part that depends on another only once the earlier captain reports `DONE`, and a captain's reply that only reports status needs no comment.
It uses `spawn` only when the user asks for a session of their own, passing `--kind`, `--model`, `--effort` and `--base` only when the user names one, and waits on it the same way when the user will want to hear about it; it waits only when there is a next step to take on the result.
A wait goes on through a session's dialogs, so whenever it reports while the user may be away, it also checks `nebula orchestrator list --json` for sessions waiting on the user (`needs_feedback`), a captain's workers included, and names them.
It keeps its replies short, since the user may be reading them on a phone, and knows `review` only helps when the user is at the TUI.
When a follow-up the user may want next is obvious, its reply ends with a `NEXT: <a> | <b>` line, at most four steps of a few words each that a pick sends back to it as they are, which `F1` to `F4` send while its chat is on screen.
These commands use the same CLI-over-socket pattern as `nebula spawn`, `nebula worktree` and `nebula open`, but they are daemon-gated to the persisted orchestrator session:

```sh
nebula orchestrator list [--json]
nebula orchestrator captain <project-id-or-name> [--json]
nebula orchestrator read <session-id> [--bytes N]
nebula orchestrator send <session-id> <prompt>
nebula orchestrator spawn --project <project-id-or-name> [--worktree <branch-or-id> [--base <ref>]]
                          [--kind <harness>] [--name <title>] [--model <m>] [--effort <e>] [--json] <prompt>
nebula orchestrator review <session-id>
```

The text format is compact and model-friendly.
`--json` prints stable IDs and status fields so the orchestrator can call a later command without fuzzy matching a label.
`spawn` prints the new session's id, and with `--json` the object `nebula session start --json` prints: `{"id","name","worktree","path"}`.
Once the session has started the command never fails: should reading the new row back fail, `--json` prints the id with the rest null and says why on stderr, so the orchestrator never retries into a second session.
A `--name` stays the session's name, as `session start --name` does: AUTO-TITLE leaves it alone.
`--model` and `--effort` are what the harness launches with; left out, it launches on its own defaults.
A `--kind` whose harness reports no status to nebula (`muse`, `grok`, a custom one with no hooks) starts all the same, with a warning on stderr that the session stays running, so a `session wait` on it needs `--timeout`.
`captain` prints the id of the project's CAPTAIN (below), starting one when the project has none, and with `--json` the same object `spawn --json` prints.
`read` prints a Claude session's recent conversation from its transcript, as `nebula read` does, with its screen under it while a dialog is open there, and any other session's screen as `nebula session read` lays it out; `--bytes` keeps its end.
`send` types the prompt as the session's next turn and returns once the session reports working on it, as `nebula session send` does, so a `session wait` run right after it waits for that turn and not the one before.
When 5 s pass with no such report the prompt was typed all the same, and the exit code is 3.
A session already working takes the prompt when it next reads input, and one with a dialog open (`needs_feedback`) is refused: the prompt and its Enter would answer the dialog blindly.
A `send` whose command is gone while it waits, killed or timed out, types nothing later.
A session with no live process is booted first, and a Claude one is typed into only once it is up, as `captain` waits for below: one that comes up on its question whether to trust the folder fails the command at once, saying it is waiting on a dialog in its pane, and the orchestrator tells the user it waits on them there instead.

The orchestrator never adopts a PREWARM POOL spare: a spare booted bare has no way to be handed the guidance, so the orchestrator always starts cold.
The IDLE REAPER never takes it either, however long it sits unwatched: it keeps its process, so the next message reaches it at once.
With the `orchestrator_remote_control` setting on (Settings → Sessions, off by default), its Claude launches and resumes with `--remote-control orchestrator`, so the Claude app on a phone can reach it.
The flag is part of the launch, so toggling the setting restarts a running orchestrator that is at rest, resumed on its conversation; one mid-turn keeps what it started with until its next start, and the Settings overlay says so.
A `make dev` instance's goes by `orchestrator-dev-<checkout>` there instead, named for the checkout it runs from, so a phone that reaches both tells it from the release one.

## Captains

A CAPTAIN is one long-lived Claude session per project that the orchestrator routes the project's work to.
It is a session named `captain` in the project's root checkout, with the role `captain`.
`nebula orchestrator captain <project>` finds the project's captain, or starts one on no prompt; the orchestrator's `send` gives it work, and the user may talk to it directly as to any session.
The daemon starts one captain per project however many calls race for it, and calls for different projects never wait on each other: a captain slow to come up holds up only the calls for its own project.
`captain` returns once the captain's Claude is up, booting one with no live process first: once it has reported a hook since it started, which Claude does only past any question it asks before that.
The one that matters is whether to trust the folder, asked in a project Claude has no trust for yet, and its default answer quits: a prompt typed into it would answer it.
So `captain` waits while a captain shows that question, which the board lists under needs you and `F9` brings up, for up to 10 minutes, and returns once the user has answered it; one that has not come up within 30 s otherwise, or that shows another dialog, fails the command, saying it is waiting on a dialog in its pane.
The orchestrator runs `captain` as a background command and checks `orchestrator list --json` a moment later: a new captain on that question is `needs_feedback`, and it tells the user plainly `Claude asks whether to trust <folder>; F9 brings it up`.
It never answers a dialog for the user or offers to, and never asks the user to run a `nebula` command.
The orchestrator's own first start asks the same about nebula's orchestrator folder, in the view, its keys going to it, and the footer says to pick `Yes, I trust this folder` to start it.

A captain reads the code but never changes it.
Its Claude launches and resumes with the captain's guidance in its system prompt, as the orchestrator's does, so the guidance survives a compaction and a `/clear`, and with `--permission-mode dontAsk`, whatever permission mode your own Claude uses: anything not allowed up front is denied without a prompt.
What it is allowed is reading: the file tools that only read (`Read`, `Grep`, `Glob`), the shell commands Claude itself knows only read (`ls`, `cat`), the `nebula` commands it runs its workers with (`spawn`, `ask`, `read`, `sessions`, `session wait`, `session read`), git's `log`, `status`, `diff`, `show` and branch listing, and `gh pr view`, `gh pr list`, `gh issue view` and `gh issue list`.
`--disallowedTools` denies, even where your own settings allow them, `NotebookEdit`, every edit in the project's root checkout, the folder its worktrees go in (`<repo>-worktrees` beside it) and the directories its space shares, git's `commit`, `push`, `merge`, `pull`, `reset`, `rebase`, `checkout`, `switch`, `restore`, `stash` and `clean`, `gh pr merge` and `gh pr create`, and `nebula worktree`, which would move it out of its checkout.
Its Claude memory for the repository, which Claude keeps under its own config folder and shares with every worktree of the repository, is the one place it writes; Claude saves there without asking.
So an edit, a `sed -i`, a build or a commit in your main checkout never runs from a captain, and its guidance says its Bash is for reading and `nebula` only and hands that work to a worker.
The guidance has it answer questions about the code from the checkout and turn anything that edits, builds or runs code into a WORKER: `nebula spawn --worktree <branch> --name <short title> --json "<task>"`, a fresh branch for each independent piece with a title of a few words naming it, at most 4 at once, and its reports name each worker by that title, never by its id.
Each task asks the worker to end its final reply with one line `DONE <what>`, `BLOCKED <why>` or `QUESTION <what>`, and to list what a future agent in the repo should know as `Notes:` lines.
A follow-up to work a worker already did goes to that worker with `nebula ask <id> "<follow-up>" --no-wait`, run in the background with a long `--timeout`, so it prints the worker's reply when that turn ends.
After starting a worker the captain runs `nebula session wait <id> --until-done` as a background command, and when it returns reads the result with `nebula read <id>`, decides the next step in the project and reports.
Every report ends with one `DONE`, `BLOCKED` or `QUESTION` line, the line the board shows under a finished row: `QUESTION` only for an answer it needs from the user to go on, `BLOCKED` only for work that cannot go on, and `DONE started <what>` for a reply that says what it started.
When a follow-up is obvious, a `NEXT:` line comes after it, the reply's last, with its NEXT STEPS: the answers a `QUESTION` can take, say.
It never merges a pull request, never closes or deletes sessions or worktrees, has a pull request opened only when the user asks, by the worker that did the work, passes the user's words on exactly and never widens a task.

When nebula sees a pull request merge in a project with a captain, it tells the captain in one MERGE NOTE, typed in as its next turn: `[nebula] PR #42 "Fix the login loop" merged from fix-login. Sessions that worked on it: 01K…, 01K….`
The sign is the one **Auto-clean merged worktrees** acts on, the checkout's pull request seen turning merged, and the note goes whether that setting is on or not.
A pull request already merged when nebula first looks at its checkout, merged while no TUI was looking or between two looks, gets its note too, as long as a live worker of the captain's project is still in that checkout.
The pull request sweep that looks for it covers the checkouts a captain's project has live sessions in whichever project is on screen, one `gh` call a tick at most, each checkout asked again within five minutes.
A worker's turn finishing brings its checkout's next look forward to that moment, since a turn is what opens a pull request, and the wait before the one after counts from it, so this costs no extra `gh` call.
It names the checkout's sessions, archived ones too, taken as the merge is seen, before a cleanup takes their rows: `nebula read <id>` still reads such a session's transcript by its id.
The note waits for the captain to be up and at rest, live with its last turn finished, and a second waits for the captain to take the first, whichever TUI sent it.
It is never typed into a prompt of yours: it waits until the captain's input box is empty, read off its screen just before (the suggestion Claude draws dim in an empty box does not count), and until no key has gone to the captain from this TUI for 5 seconds.
Each pull request gets one note, ever: the notes are kept in `merge-notes.json` in the data directory with the captain and turn each went to, so neither a restart nor a second TUI on the same daemon sends one twice, and a note still held when the TUI quits goes out from the next one.
The file keeps 100 notes, dropping its oldest sent ones first and a held one only once no sent one is left.
The captain's guidance has it read those sessions' final replies, collect their `Notes:` lines and keep what a future agent in the repository would otherwise get wrong in its Claude memory, short, with duplicates merged and stale entries dropped.
A doc in the repository that is now out of date goes to a worker to fix in a pull request, and a decision worth keeping in the user's own notes comes back as one `To record: …` line, which the captain never writes there itself.
Its reply only reports status, ending `DONE …`.

Like the orchestrator, a captain never adopts a PREWARM POOL spare, and the IDLE REAPER never takes it.
An archived captain is retired: the next `captain` call starts a new one.
Should a retired one be unarchived beside it, `captain`, the board and the merge notes all go by the more recently created of the two.
A project whose root checkout is gone from disk gets no captain, new or resumed, until the project is pointed at its new folder; removing a project from nebula forgets its captain with its other sessions.
A client older than captains (before protocol 53) is sent a captain as a plain worker, since it could not read the role.

## Permission model

The socket is already same-user only, but orchestrator commands are still privileged: they can message other agents and change what every client shows.
Every orchestrator request carries the caller's `NEBULA_AGENT_ID`, and the daemon checks that:

1. the caller exists,
2. it is not archived, and
3. its persisted role is `orchestrator`.

Prompt text alone is never trusted.
A normal agent can read the guidance only if it was accidentally pasted into its context, and the daemon still refuses the request because its row is not marked as the orchestrator.
That gate is also why `Bash(nebula orchestrator:*)` sits in the permission rules nebula installs for every Claude session: allowing the command costs nothing in a session the daemon refuses it from.

The orchestrator also does not approve permission prompts for other agents.
The default loop is tell-and-ask: report that a session is waiting and offer to bring it up.
Higher-autonomy policy can be added later once the UI has a clear way to show what was approved and why.

## Attention feed

The robust first design is **poll-on-demand plus a visible strip**, not injected prompts.
The daemon already broadcasts status changes to every TUI, and the orchestrator can call `nebula orchestrator list --json` whenever it needs a fresh state read.
The strip updates immediately from the same events.
This avoids writing unsolicited text into an idle chat box, avoids races with a model mid-turn, and still gives the user an always-visible feed of what needs attention.

Future work may add rate-limited, clearly marked notes to the orchestrator only when it is idle.
That should be opt-in because injected notes become model input, while the current list command is explicit and auditable.

## Follow-ups

- The protocol still carries `tabs` on `OrchestratorOpenReview` and `ReviewOpened`, ignored, from the review modal `review` used to open: it goes at the next breaking protocol bump.
- First-class transcript summaries per harness instead of terminal-tail history.
- Optional idle-note injection for the orchestrator, behind an autonomy setting.
- A richer spawn target grammar for project aliases and worktree creation bases.
