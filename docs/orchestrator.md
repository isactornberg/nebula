# Orchestrator

<sub>[← README](../README.md) · [Keys](keys.md) · [Commands](commands.md) · [Sessions](sessions.md) · [Configuration](configuration.md) · [How it works](how-it-works.md)</sub>

The ORCHESTRATOR view is a chat-first way to run nebula.
Instead of moving from project to project and opening each worktree by hand, the user keeps one agent session in front of them and asks it to coordinate the rest.

## Shape of the feature

- **One orchestrator session, in a home of its own.**
  The daemon persists a session role on agent rows, and only a row with the orchestrator role may call the orchestrator-only socket commands.
  `F10` starts one when there is none: a Claude session named `orchestrator` in the ORCHESTRATOR HOME, the `orchestrator` folder in nebula's data directory.
  The daemon starts an orchestrator for a client only there, and only while there is none; a captain it starts only for `nebula orchestrator captain`, never for a client asking for a session in that role.
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
  A `done` row says how the turn ended: its last line that starts with `DONE`, `BLOCKED` or `QUESTION`, the report line agents end a turn with, else the start of the last paragraph the session wrote above its input box.
  Only the turn after the last prompt counts, and a tool's output in it does not (a captain's `nebula read` of a worker's `DONE` is not its own); the line leaves `DONE` off, as the section says it, and shows `BLOCKED` and `QUESTION` in the section's color.
  A finish whose report line is a `QUESTION` or `BLOCKED` waits on the user as a dialog does: once the board has read it, its row moves up under `needs you`, counted there and walked by `F9` with the others, its line as it was, and it stays there once you have read it, until the session starts another turn.
  A reply may end on a line of answers for the user to pick from, `NEXT: <a> | <b> | <c>`, which Claude Code's `quick-answers` mod asks for and draws as pills over the input box: that is the chat's business, and nebula neither asks for the line nor sends its answers.
  All nebula does with it is leave it out: the line, and the mod's row of pills, are never read as why a session is on the board.
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
The user may also talk to a captain directly, so work a captain reports that the orchestrator did not ask for is the user's own, and it reports it as theirs.
Work across several projects goes to each captain in its part, a part that depends on another only once the earlier captain reports `DONE`, and a captain's reply that only reports status needs no comment.
It uses `spawn` only when the user asks for a session of their own, passing `--kind`, `--model`, `--effort` and `--base` only when the user names one, and waits on it the same way when the user will want to hear about it; it waits only when there is a next step to take on the result.
A wait goes on through a session's dialogs, so whenever it reports while the user may be away, it also checks `nebula orchestrator list --json` for sessions waiting on the user (`needs_feedback`), a captain's workers included, and names them.
It keeps its replies short, since the user may be reading them on a phone, and knows `review` only helps when the user is at the TUI.
These commands use the same CLI-over-socket pattern as `nebula spawn`, `nebula worktree` and `nebula open`, but they are daemon-gated to the persisted orchestrator session:

```sh
nebula orchestrator list [--json]
nebula orchestrator captain <project-id-or-name> [--json]
nebula orchestrator read <session-id> [--bytes N | --report]
nebula orchestrator send <session-id> <prompt>
nebula orchestrator spawn --project <project-id-or-name> [--worktree <branch-or-id> [--base <ref>]]
                          [--kind <harness>] [--name <title>] [--model <m>] [--effort <e>] [--json] <prompt>
nebula orchestrator review <session-id>
nebula orchestrator attention --json
nebula orchestrator picks set --json <file|->
```

The text format is compact and model-friendly.
`--json` prints stable IDs and status fields so the orchestrator can call a later command without fuzzy matching a label.
`spawn` prints the new session's id, and with `--json` the object `nebula session start --json` prints: `{"id","name","worktree","path"}`.
Once the session has started the command never fails: should reading the new row back fail, `--json` prints the id with the rest null and says why on stderr, so the orchestrator never retries into a second session.
A `--name` stays the session's name, as `session start --name` does: AUTO-TITLE leaves it alone.
`--model` and `--effort` are what the harness launches with; left out, it launches on its own defaults.
A `--kind` whose harness reports no status to nebula (`muse`, `grok`, a custom one with no hooks) starts all the same, with a warning on stderr that the session stays running, so a `session wait` on it needs `--timeout`.
`captain` prints the id of the project's CAPTAIN (below), starting one when the project has none, and with `--json` the same object `spawn --json` prints.
`attention` and `picks set` rank the web app's focus picks, through `nebula serve` rather than the daemon ([Ranking the focus picks](#ranking-the-focus-picks)).
`read` prints a Claude session's recent conversation from its transcript, as `nebula read` does, with its screen under it while a dialog is open there, and any other session's screen as `nebula session read` lays it out; `--bytes` keeps its end.
`read --report` prints only a Claude session's last reply, every line of it, as `nebula read --report` does: once a captain's turn is done that is its report, ending in its `DONE`, `BLOCKED` or `QUESTION` line, without the tool results, task notifications and reminders a plain `read` shows around it.
`send` types the prompt as the session's next turn and returns once the session reports the prompt it took and that holds the whole of it, as `nebula session send` does, so a `session wait` run right after it waits for that turn and not the one before.
A session that took only part of it fails the command, naming how its prompt starts against the one sent, since it is working on that part.
When 5 s pass with no such report the prompt was typed all the same, nothing says it arrived whole, and the exit code is 3.
A daemon older than this check is refused with nothing sent: `nebula reload` moves it onto the installed build.
A session already working takes the prompt when it next reads input, and one with a dialog open (`needs_feedback`) is refused: the prompt and its Enter would answer the dialog blindly.
A `send` whose command is gone while it waits, killed or timed out, types nothing later.
A session with no live process is booted first, and a Claude one is typed into only once it is up, as `captain` waits for below: one that comes up on its question whether to trust the folder fails the command at once, saying it is waiting on a dialog in its pane, and the orchestrator tells the user it waits on them there instead.

A result the orchestrator never waited for still reaches it.
Once a session it sent work to is done with it, as `session wait --until-done` counts done, the daemon types a DONE NOTE into the orchestrator as its next turn: `[nebula] The captain of api-server, 01K…, is done with what you sent it. Read what it did with nebula orchestrator read 01K… --report.`
A `session wait --until-done` on that session that answers instead tells the orchestrator itself, and no note follows; one still waiting holds the note back, and one killed before it answers lets it go.
The note waits for the orchestrator to be at rest, its last turn finished for 5 s, with its input box empty (the suggestion Claude draws dim in an empty box does not count), no key sent to it from any client for 5 s, and its last turn asking you nothing, and a second note waits for it to take the first, or any other turn the daemon typed into it.
A turn asks you something when its report line is a `QUESTION` or `BLOCKED`, a finish the board has under `needs you`: a note typed then would be read as your answer, so it waits until you have answered and the turn on your answer has ended.
Each session gets one note, for the latest `send` to it; notes still owed when the daemon restarts are dropped.

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
It reads a worker's work by the branch from its own checkout (`git show <branch>:<file>`), as `git -C <worktree>` matches none of the git rules it is allowed.
The guidance has it answer questions about the code from the checkout and turn anything that edits, builds or runs code into a WORKER: `nebula spawn --worktree <branch> --name <short title> --json "$(cat <<'EOF' … EOF)"`, a fresh branch for each independent piece with a title of a few words naming it, at most 4 at once, and its reports name each worker by that title, never by its id.
A worker on an issue carries it too, `--issue <url>` before `--worktree`, and a BRIEF from the web app's Start (below) names the one worker to start and its command.
The task, and a follow-up's text, go in that quoted heredoc and never in quotes: Claude Code denies a quoted argument holding a line that starts with `#`, such as a Markdown heading, and in `dontAsk` there is no prompt to approve it, while the shell expands `$` and backticks inside double quotes.
Every Claude session's guidance says the same for text that spans lines in any `nebula` command, so the orchestrator's `send` and `spawn` take it that way too.
Each task asks the worker to end its final reply with one line `DONE <what>`, `BLOCKED <why>` or `QUESTION <what>`, and to list what a future agent in the repo should know as `Notes:` lines.
A follow-up to work a worker already did goes to that worker with `nebula ask <id> "<follow-up>" --no-wait`, run in the background with a long `--timeout`, so it prints the worker's reply when that turn ends.
After starting a worker the captain runs `nebula session wait <id> --until-done` as a background command, and when it returns reads the worker's report with `nebula read <id> --report`, decides the next step in the project and reports.
Every report ends with one `DONE`, `BLOCKED` or `QUESTION` line, the line the board shows under a finished row: `QUESTION` only for an answer it needs from the user to go on, `BLOCKED` only for work that cannot go on, and `DONE started <what>` for a reply that says what it started.
It never merges a pull request, never closes or deletes sessions or worktrees, has a pull request opened only when the user asks, by the worker that did the work, passes the user's words on exactly and never widens a task.

When nebula sees a pull request merge in a project with a captain, it tells the captain in one MERGE NOTE, typed in as its next turn: `[nebula] PR #42 "Fix the login loop" merged from fix-login. Sessions that worked on it: 01K…, 01K….`
The sign is the one **Auto-clean merged worktrees** acts on, the checkout's pull request seen turning merged, and the note goes whether that setting is on or not.
A pull request already merged when nebula first looks at its checkout, merged while no TUI was looking or between two looks, gets its note too, as long as a live worker of the captain's project is still in that checkout.
The pull request sweep that looks for it covers the checkouts a captain's project has live sessions in whichever project is on screen, one `gh` call a tick at most, each checkout asked again within five minutes.
A worker's turn finishing brings its checkout's next look forward to that moment, since a turn is what opens a pull request, and the wait before the one after counts from it, so this costs no extra `gh` call.
It names the checkout's sessions, archived ones too, taken as the merge is seen: a cleanup archives them with the checkout, and `nebula read <id>` reads such a session's transcript by its id.
The TUI that sees the merge hands the note to the daemon (`MergeNote`), and the daemon types it, as it types a Start's brief (below): a HELD TURN, typed once the captain is up and at rest, its last turn finished for 5 s, its input box empty (the suggestion Claude draws dim in an empty box does not count), no key sent to it from any client for 5 s, and its last turn asking you nothing.
The captain is typed one held turn at a time, briefs and merge notes alike, oldest first, each once it has taken the last turn the daemon typed into it, so a note and a brief that come at once are two turns and never share its input box, however many TUIs are open.
A held note shows nothing while it waits: a captain whose turn asks you something is under `needs you` on the board, which is why the note waits, and the note goes once you have answered and the turn on your answer has ended.
Each pull request gets one note, ever: the TUIs keep every note in `merge-notes.json` in the data directory, marked sent once one hands it over, so neither a restart nor a second TUI on the same daemon hands one over twice, and a note still held when the TUI quits goes from the next one; the daemon holds a note for one pull request once, should two TUIs hand it over in the same moment.
The file keeps 100 notes, dropping its oldest sent ones first and a held one only once no sent one is left.
A note is for its project, not for one captain: it waits in the file while its project has no captain, and once handed over goes to whichever captain the project has when it goes, the one `captain` would find, so a captain archived while a note is held for it hands the note to the next captain the project gets, the note waiting meanwhile; only a project removed from nebula takes a held note with it, which the daemon's log says.
A daemon older than protocol 61 types no merge note itself, and a TUI never types one beside it, since a note it typed could land in the captain's input box with the daemon's own brief: the TUI hands it none, says `merge note held: the daemon is older than this nebula, restart it first (nebula reload)` once, and the notes wait in the file until `nebula reload` moves the daemon onto the installed build.
The captain's guidance has it read those sessions' final replies, collect their `Notes:` lines and keep what a future agent in the repository would otherwise get wrong in its Claude memory, short, with duplicates merged and stale entries dropped.
A doc in the repository that is now out of date goes to a worker to fix in a pull request, and a decision worth keeping in the user's own notes comes back as one `To record: …` line, which the captain never writes there itself.
Its reply only reports status, ending `DONE …`.

Like the orchestrator, a captain never adopts a PREWARM POOL spare, and the IDLE REAPER never takes it.
An archived captain is retired: the next `captain` call starts a new one.
Should a retired one be unarchived beside it, `captain`, the board and the merge notes all go by the more recently created of the two.
A project whose root checkout is gone from disk gets no captain, new or resumed, until the project is pointed at its new folder; removing a project from nebula forgets its captain with its other sessions.
A client older than captains (before protocol 53) is sent a captain as a plain worker, since it could not read the role.

### Start from the web app

The web app's **Start** on an issue ([Web](web.md#starting-an-issue)) hands it to a project's captain through `StartIssue`, a request any socket client may send, since the page cannot send the orchestrator's: it names a project and an issue, by its URL, key and title, and the task for its worker.
The daemon refuses an issue URL it would not take into a session's launch, a key the URL does not name, and a project it does not have.
It gets the project's captain as `captain` does, through the same gate, so however many Starts race, one project gets one captain, and boots it when it has no live process.
Then it holds the BRIEF for that captain, one turn naming the issue and the command for its worker, on the branch nebula cuts for the issue, the task in a quoted heredoc:

```text
[nebula] Start was pressed on the Linear issue REL-123, "Fix login" in nebula's web app: it is yours to start. Start one worker on it, on its own branch and carrying the issue, so nebula ties the session to it:

  nebula spawn --issue https://linear.app/acme/issue/REL-123/fix-login --worktree rel-123-fix-login --name <short title> --json "$(cat <<'EOF'
Fix Linear issue REL-123: Fix login (https://linear.app/acme/issue/REL-123/fix-login)
EOF
)"

with the task ended as every task you give a worker ends. Then follow it as you follow every worker.
```

A brief is never typed into a captain at work, into an input box with text in it, or in place of the answer to a question the captain asked you.
It goes once the captain is up, at rest (a finished turn settled for 5 s), left alone by every client's keys for 5 s, its input box empty, the suggestion Claude draws dim in an empty box aside, and its last turn asking you nothing (its report line no `QUESTION` or `BLOCKED`): the wait a DONE NOTE has for the orchestrator, from the same code.
A brief is a HELD TURN, as a MERGE NOTE is: the captain takes them one at a time, oldest first, each once it has taken the last turn the daemon typed into it, a `send` included, and is typed each as `send` types a turn, its hook reporting the prompt it took.
No two turns the daemon types into one session ever share its input box: each one's text and its Enter go in before the next one's text does.
`StartIssue` answers once the captain is up with whether the brief went at once, given 3 s for it, or is held; one that comes up on its question whether to trust its folder is answered held at once, and takes the brief once the user has answered it.
Every client is told which briefs are held, after its Snapshot and on every change (`HeldBriefs`), and the web app says so on the issue.

Held turns, briefs and merge notes, are kept in `held-turns.json` in the data directory, written as each is held and as each is typed.
A daemon that starts again, an IN-PLACE RESTART or after a stop, reads them back, boots each one's captain when it has no live process, and types each once that captain is ready for it, as before the restart.
A brief whose captain is archived or deleted before it takes it goes with it, and a captain started in its place is not handed it; a merge note goes on to the project's next captain instead.

`nebula spawn --issue <url>` gives the session it starts that issue, as a session started from the TUI's issues list has it: the issue is on its row, which the board and the web app join it by, and its Claude reads it first.
A daemon older than protocol 60 is never asked either: the web app and `spawn --issue` say that `nebula reload` moves it onto the installed build.
A TUI hands merge notes only to a daemon at protocol 61 or newer, as [Captains](#captains) says.

### Ranking the focus picks

The web app's focus is at most three picks the orchestrator chooses and explains ([Web](web.md#focus)).
The candidates are what `nebula serve` holds and the daemon does not, Linear's issues and the pull requests, so both commands go to the serve on this machine, which finds it by its port file, and fail when none runs: `nebula serve is not running, and it holds the candidates the focus picks are ranked from: start it with nebula serve --no-open, or Shift+W in the TUI`.
The serve takes them from the orchestrator's session alone, the one whose id the command's `NEBULA_AGENT_ID` names, as the daemon takes its other commands, and from no device on the tailnet.

`attention --json` prints the candidates on one line of JSON, compact, since every rank reads it: the most urgent 25, the rows that need you, then the ones done, then the issues in flight and to do, each with its `key` (an issue's, `REL-123`, when one is joined to it, else the session's id), its `lane` (`needsYou`, `done`, `inFlight`, `todo`), its Linear issue (title, state, priority, Linear project, how long since it changed, and the project Start handed it to) and its sessions (id, project, name, status, the line the board reads under it, how long in that status, and its pull request's state), and `more` for the count past those.
Times are ages, `5m`, `3h`, `2d`, and a field with nothing in it is left out.
`feedback` is what you said of picks in the last week, newest first: the key, `notToday` or `wrong`, your reason, the pick's reason and what it was.
`read` says how long ago Linear and the pull requests were read, and `problems` why a part could not be; when they are over 10 minutes old, the serve reads them again first, and waits up to a minute for it.
A key you said not today to is left out until the day ends.

`picks set --json -` takes a JSON array of at most three `{"key", "reason"}` on stdin (or from a file), the first to do first, and an empty one picks nothing.
The serve refuses more than three, a key picked twice, a reason missing or over 300 characters, and a key that is neither a candidate now nor one the last `attention` printed, and says which; it puts each reason on one line, stamps the picks with the time, keeps them in `focus.json` in the data directory, and every page shows them at once.

The page's **Re-rank** has the daemon type the orchestrator the RANK TURN:

```text
[nebula] Rank the focus picks for nebula's web app: run nebula orchestrator attention --json, choose at most three, and set them with nebula orchestrator picks set --json -, as your guidance says. Reply in one line.
```

The orchestrator's guidance says the rest: to rank from `attention` alone, with no `list`, `read` or captain, weighing what waits on you and what unblocks other work over what can wait, to give each pick a reason of one short sentence saying why now, to pass the picks in a quoted heredoc, to go by your feedback, and, when you have corrected the same kind of pick for the same reason more than once, to propose a standing rule for the `CLAUDE.md` in its folder, quoting it, written there only once you say yes.
It replies in one line.

The rank is `RankFocus`, a request any socket client may send, behind protocol 62 (`RANK_PROTOCOL`), since the text is nebula's own.
It is a HELD TURN for the orchestrator, beside the captains' briefs and merge notes: typed only once the orchestrator is up, at rest, its input box empty, left alone by every key for 5 s and its last turn asking you nothing, the wait a done note has, from the same code, booted first when it has no live process, kept in `held-turns.json` across a restart, and gone with an orchestrator archived before it takes it.
One is held at a time, and a second joins it.
A rank the page asks for on its own as it opens is sent without `wait`: the daemon refuses it unless the orchestrator is ready for a turn that moment and owed no other, so it is never held for later.
Every client is told where it stands, right after its Snapshot and on every change (`RankTurn`): when the one held was asked for, and when one was last typed, which the page reads `held`, `ranking` and `ended` by.
A daemon older than protocol 62 is never asked, and the page says that `nebula reload` moves it onto the installed build.

A SCHEDULE ranks too, with no page open: the morning rank, `nebula schedule add --rank --at 07:30 --days weekdays`, or one click on the web app's Schedules page ([Web](web.md#schedules)).
When it is due, the daemon starts `nebula serve` first when none runs, as `Shift+W` does, through your login shell, since the serve holds the candidates `attention` prints, and then holds the RANK TURN as a Re-rank does, through the same `Daemon::rank_focus`, so it lands only once the orchestrator is at rest with its input box empty.
The schedule's run says it is held, then typed; with no orchestrator, or a serve that does not come up, the run fails and says why.

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
