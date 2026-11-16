# Web

<sub>[← README](../README.md) · [Keys](keys.md) · [Commands](commands.md) · [Sessions](sessions.md) · [Configuration](configuration.md) · [How it works](how-it-works.md)</sub>

`nebula serve` puts this machine's sessions in a browser tab: the board, each session's last reply, the answers it offers and the dialog it shows, and its screen.
Under the board are the Linear issues assigned to you that no session works on, and each session shows the issue it works on and the pull request on its checkout.
**Start** on one of those issues hands it to a project's captain, which starts a worker on it.
At the foot of the board is **Today**, a day another program writes to a file: today's training, the todos due, the projects gone quiet, and how its nightly job and its automation stand.
The page answers them too: a follow-up, one of the answers a reply offers, an option of the dialog a session shows.
Its **Skills** page lists every skill the agent CLIs on this machine load, switches one on or off for a CLI, and fixes what it can: it removes broken links and the old command a skill hides, and keeps a fix you decline off the list.
It also edits a skill's `SKILL.md`, and deletes a skill to the Trash.
Its **Schedules** page lists what nebula's daemon runs on its own, a prompt in a project or the orchestrator's morning rank, adds one, runs one now and removes one, with every run kept and why one was skipped ([Schedules](#schedules)).
A phone on your tailnet reaches the page too, through `tailscale serve`, once it is paired, and installs it to its Home Screen ([From a phone](#from-a-phone)).

## Running it

```sh
nebula serve              # serve on http://127.0.0.1:7682 and open a tab on it
nebula serve --port 8080  # that port, or an error when it is taken
nebula serve --no-open    # serve, print the URL, open nothing
```

With no `--port` it takes 7682 when that is free and a free port otherwise, saying which, so it never clashes with `nebula browser`, whose ttyd starts at 7681.
`--port 0` always picks a free one.
It prints the URL it serves on, and Ctrl+C stops it.

One server serves each nebula.
While it runs it keeps its port in `serve.port` in the runtime dir, and a second `nebula serve` finds it there, says where it serves and opens a tab on it instead of serving again.

It is a client of the daemon like the TUI, and it never starts one.
With no daemon running, the page says it is disconnected; once a daemon is up, it connects on its own and the page comes back.
The same happens across `nebula reload` and `nebula upgrade`, which drop every client while the daemon restarts in place: the page is disconnected for a moment and then current again, sessions and all.

## What the page shows

The board is the TUI's: `needs you`, `done`, `running` and the captains at rest, in attention order, with the line each row says under it read off the session's screen the way the TUI reads it.
The focus at the top of the page is at most three things worth your time, the orchestrator's picks, each with why it chose it, and the answers a session's reply offers; the board under it leaves them out.
Until the orchestrator has picked, the focus is by rule: the first three rows that need you or are done, the ones that need you first, each with that line as its reason ([Focus](#focus) says how the two meet).
Running sessions and captains at rest are never in focus, so with nothing that needs you or is done the page says nothing needs you.

A session's reply is the last one it wrote: for Claude, the newest reply in its transcript, as Markdown; for every other CLI, the text of its screen.
When a Claude reply ends on a `NEXT: <a> | <b> | <c>` line, the page offers those answers, read by the same rule Claude Code's `quick-answers` mod draws its pills by.
A dialog open in the session is shown with its question and its numbered options.
A captain lists the other sessions of its project, in attention order.
The screen is the session's terminal as text, read again every second while it is shown.

A deep link opens the page on a session: `http://127.0.0.1:7682/s/<session-id>`.
Opening a finished session nobody has looked at marks it seen, as putting it up in the TUI does.

## Focus

The orchestrator chooses the focus, and says why, from what nebula gathers for free: the rows that need you or are done, and the issues in the lanes ([Issues and pull requests](#issues-and-pull-requests)).
Code reads the status, Linear and the pull requests; the orchestrator only ranks.
Each pick is a card: a session's, with the answers its reply offers, or an issue's no session works on, with its state and priority, which opens the issue.
A pick of an issue a session works on opens that session.
The list says `Focus 09:12`, when it picked, and the page `The orchestrator picked three at 09:12.`

**Re-rank**, beside Focus at the top of the list, asks it to pick again.
nebula types the orchestrator one turn for it, `[nebula] Rank the focus picks …`, only once it is at rest with its input box empty and its last turn asking you nothing, as it types a done note ([Orchestrator](orchestrator.md#ranking-the-focus-picks)), so a rank never lands in the middle of a conversation with you.
Until then the list says `Ranking once the orchestrator is at rest with an empty input box.`, then `The orchestrator is ranking.` while it works, and Re-rank reads **Ranking…** and waits; pressing it again joins the rank under way.
A rank whose turn ends with no picks set says `The orchestrator's last rank ended without setting picks.`
With no orchestrator, Re-rank is off: `F10` in the TUI starts one.

Opening the page, or coming back to it, asks for a rank too, but only when the picks are due: the orchestrator has never picked, or its picks are over three hours old and the candidates changed since, a row or an issue come or gone or moved to another section.
Such a rank goes only while the orchestrator is at rest with its input box empty right then, never held for later, and at most four times a day on this Mac's clock.
A rank on a schedule, such as one every weekday at 07:30, is a schedule of its own ([Schedules](#schedules)).

Each of the orchestrator's picks has **Not today** and **Wrong**, on its card and beside `Why focus #2` on its page, which is where a phone has them.
Either asks first why, a reason you may leave empty, and then puts the pick aside: it leaves the focus at once, and the page says so.
**Not today** keeps it out of the candidates until the day ends, so the orchestrator cannot pick it again today; **Wrong** says the pick was a mistake, and it may be picked again later.
The next rank reads what you said of picks in the last week, with your reasons, and when you correct the same kind of pick for the same reason again, the orchestrator proposes a standing rule for the `CLAUDE.md` in its folder, and writes it there only once you say yes.

The rule stands in for the orchestrator's picks, and the list says why under Focus:

- it has not ranked yet: `The orchestrator has not ranked yet.`;
- its picks are over three hours old and the candidates changed since: `The orchestrator's picks from 06:10 are over three hours old, and the board has changed since.`;
- none of them is a candidate now, each answered, read or gone: `None of the orchestrator's picks from 09:12 is on the board now.`;
- you put each one aside: `You put the orchestrator's picks from 09:12 aside.`;
- Linear and the pull requests, which the candidates are keyed by, are not read yet, as right after `nebula serve` starts: `The orchestrator's picks from 09:12 show once Linear and the pull requests are read.`

A pick that is no candidate any more, a question you answered or a finish you read, leaves the focus on its own, and the rest stay.
The orchestrator may leave out rows that need you, judging something else more worth your time, so the page never says nothing needs you while one does: under its picks, **Also needs you** lists the rows that need you and that it did not pick, and when it picked nothing at all they are listed under **Needs you** in the cards' place.
In the list they stay under **Needs you**, as every row the focus leaves out does.

`nebula serve` keeps the picks, what you said of them and when the page asked for a rank on its own in `focus.json` in nebula's data directory, so every browser shows the same and a restart keeps them.

## Issues and pull requests

`nebula serve` reads two things the daemon does not keep: the open Linear issues assigned to you, and the pull request on each checkout a session works in.
It reads them when the page opens and nothing has been read yet, and when you press **Refresh** at the top of the list, or `r`.
It never reads them on a timer.
Beside the button the page says when it last read them, `Updated 4 min ago`, kept current while the page is open.
A part that could not be read says why in a line under it, `LINEAR_API_KEY is not set in nebula serve's environment.` or `Pull requests in api: gh timed out.`, and keeps what the read before had of it.
Pressing Refresh while a read is under way joins that read; there is never a second.

The issues are Linear's, read with the `LINEAR_API_KEY` in `nebula serve`'s own environment, so a serve the TUI starts (`Shift+W`) has the TUI's.
They are the issues assigned to you that are not done or canceled, in each Linear team a project reads its issues from: its **Issues** setting on the Project tab (`linear:REL`), or its space's, which wins, as the TUI's issues list reads it.
A project whose issues are GitHub's gives none; that is still to come.
One query reads every team, a page of 100 issues at a time until there are no more.

A session works on an issue by the first of these that holds:

- its issue URL names it, as a session started from an issue has;
- the issue's key is in its checkout's branch as a whole word, as nebula cuts a worktree for one (`rel-123-fix-login` is REL-123's, and `rel-1234-x` is not);
- its branch is the one Linear suggests for the issue.

A session's detail then names the issue with its title and state under **Linked**, and its key opens it in Linear.

The pull request is GitHub's, read with `gh` in each project: one query per project with a session in one of its checkouts, however many checkouts.
A checkout's pull request is its branch's open one, a draft included, else its newest while the checkout has nothing past that one's head, merged or closed, by the rule the TUI's PR row keeps.
It shows under **Linked** and on a focus card as `ready for review`, `draft`, `merged` or `closed`, the TUI's words.
A project whose remotes name no GitHub repository has no pull requests, and says nothing.

Under the board, the assigned issues no session works on are two lanes: **In flight, no session**, started in Linear with nothing running, and **Todo**, not started yet.
Each is in priority order, urgent first and no priority last, then the latest changed first, with Linear's mark for its priority at the end of the row.
An issue in the backlog or triage is in neither.
Opening one shows its title, key, state, priority, Linear project, when it last changed and the branch Linear suggests, with **Linear** to open it there; `/i/<key>` links to it.
Focus by rule stays the sessions', and the orchestrator may pick an issue too ([Focus](#focus)).

### Starting an issue

An issue in a lane has **Start** at the top of its page, or `s`.
It only asks: a row under it names the issue and the project it goes to, says what the hand-off does there and in Linear, and has a **Start** of its own and **Cancel**.
Nothing is handed over until that second **Start**; Esc or **Cancel** leaves the row, and while it asks the keys are on **Cancel**.
It offers every project whose **Issues** setting, or its space's, names the issue's Linear team, the one Start last sent an issue of the same Linear project to picked first; with one project there is nothing to pick.

The second **Start** hands the issue to that project's captain, started as `nebula orchestrator captain` starts one when the project has none, and brought back when it has no live process.
The captain is typed a brief: start one worker on the issue, on the branch nebula cuts for it (`rel-123-fix-login`), with `nebula spawn --issue`, so the worker carries the issue and the board joins the two.
The brief is typed only once the captain is at rest with its input box empty and its last turn asking you nothing, never into a turn under way, a prompt of yours or in place of your answer to a `QUESTION` it asked: at once when it is, and otherwise held until it is, across a daemon restart too, one turn at a time with the merge notes the captain is owed.
[Orchestrator](orchestrator.md#start-from-the-web-app) says how the daemon holds it.

Once the daemon has the hand-off, the page moves the issue to its team's first started state in Linear, the `started` state first in the team's order, with the same `LINEAR_API_KEY` it reads with.
It asks Linear the issue's state first, and moves nothing that is started already, done or canceled, so Start never moves an issue back or sideways.
A move that fails leaves the hand-off as it is, and the page says why under the issue's title: `It was not moved to its started state in Linear: Linear timed out.`
The page shows the new state at once, with no refresh, and the issue goes to **In flight, no session**.

A handed-over issue's row says so under its title, `handed to api · 2m`, or `handed to api · brief held` while the brief waits, and its page says which project's captain has it, since when, and whether the brief waits.
`nebula serve` keeps that in `issue-starts.json` in nebula's data directory, with the project each Linear project's issues last went to, so every browser shows the same and a restart keeps it; it keeps the 100 issues handed over last.
**Start** on an issue handed over already asks too, and says so: `REL-123 was handed to the captain of api already. Start it again in api?`
Once the captain's worker is up, the session joins the issue by its URL, the issue leaves its lane, and a page open on `/i/<key>` goes to that session.
The TUI shows none of this, and reads nothing more for it.

## Today

The bottom band of the idle view is **Today**, a day another program writes to a file, such as the `today.json` agentos writes each time it rebuilds its Home page.
The page reads it and never writes it, and shows it only while `today_file` names it ([The day file](#the-day-file)).

- **Training**: the block the week is in (`Week 3, build`), what today holds, linked to its sheet, and how many lifts the week has had.
- **Todos**: how many are overdue or due within a week and how many of those are late, then the first three, each with how many days late it is (`12d`), `today`, or how soon it is due (`in 3d`), and `+4 more` to the rest.
  The page never lists more than three.
- **Quiet for 30 days or more**: the projects with no update in that long, the quietest first, each with how many days.
- Under them, how the nightly job last ran, `nightly ok` or `nightly failed` with how long ago once that is a day or more, and `automation on` or `automation off`, the program's kill switch.
  A failed run says why on a line of its own.

Each thing links to where it is edited: a web link opens in a new tab, and any other, such as an `obsidian://` link, hands off to its app.
A part the program could not read says why in its place, as the todos do when the list could not be reached and the training when its log could not, and the chip says it is unread.

While a session, an issue or the Skills page is open, the band is not on screen, and the day is a chip in the top bar instead: `Today`, today's training up to its first comma, the todos late or due, the nightly run's word, and `automation off` while it is.
Where the top bar is short of room, the chip leaves out the training first, then says `Today` alone in the colour of the worst of it, red for something that failed, peach for todos late or automation off, and green otherwise, so the bar keeps to one line; hovering it says all of it.
Short of room still, the session the TUI shows is cut short, `TUI on api › Fix the lo…`, and hovering it says all of it.
The chip goes back to the idle view.
On a phone, where the list is the idle view, the band is at the foot of the list, and the chip takes a line of its own under the nav.
On a phone the top bar's first line is the mark, the sections and the bell; what the TUI shows is a line of its own under it, with **Follow TUI** at its end.

Capturing a todo from the page is still to come.

### The day file

`today_file` in `config.json` names the file, from `/` or `~/`:

```json
{ "today_file": "~/.local/state/agentos/today.json" }
```

There is no default: with nothing set there is no band.
`nebula serve` looks at the setting and the file every second, and reads the file again whenever its size, time or inode changed, so a write shows within a second, with no restart.
A writer should write the whole file beside it and rename it into place, so the page never reads half of one.
A file that is missing or cannot be read is no band, and one that is not a day this build reads shows why in the band, ``The day in /home/you/today.json could not be read: missing field `todos`.``, with the rest of the page as it was.

The file is one JSON object:

```json
{
  "version": 1,
  "at": 1794536224000,
  "todos": {
    "count": 7,
    "overdue": 5,
    "top": [{ "title": "Renew the car insurance", "due": "2026-11-01", "url": "https://todo.example.com/task/1" }],
    "url": "https://todo.example.com/upcoming",
    "problem": null
  },
  "training": {
    "block": "Week 3, build",
    "today": "Day 2 Lower, squat and hinge",
    "url": "obsidian://open?path=%2Fnotes%2FNext%20session.md",
    "lifts": 1,
    "liftsPerWeek": 4,
    "problem": null
  },
  "quiet": [{ "name": "Garden shed", "brain": "home", "days": 47, "url": "obsidian://open?path=%2Fnotes%2FGarden%20shed.md" }],
  "nightly": { "at": 1794536220000, "ok": true, "why": null },
  "off": false
}
```

- `version` is 1, the one this build reads; any other is refused with why.
- `at`, `nightly.at`: when it was written and when the nightly job last ran, in epoch milliseconds.
- `todos`: `count` and `overdue` of the todos overdue or due within a week, `top` the first few soonest due, each with its `due` date, `url` where the rest are, and `problem` why they could not be read, with nothing counted then.
- `training`: the week's `block`, what `today` holds, the `url` of its sheet, `lifts` of `liftsPerWeek`, and `problem` why the training could not be read, with no `block`, `today` or `url` and no lifts then; null when there is no training week.
  A file without `problem` is read as one whose training could be read.
- `quiet`: the projects with no update for 30 days or more, the quietest first, each with the notebook it is in (`brain`) and `days`.
- `nightly`: its last run, `ok` or not and `why`; null before its first.
- `off`: whether the program's kill switch is on, so its automation is off.

Every `url` is optional, a field this build does not know is ignored, and a link markdown-it would drop, such as `javascript:`, is shown as text.

## Answering a session

A follow-up is typed into the session the way `nebula session send` types one, and the page waits until the session's CLI reports taking the whole of it.
It is refused while the session is working, and while it has a dialog open, which the text and its Enter would answer blindly.
A session whose terminal is gone, reaped while idle or stopped by a daemon restart, is brought back first.
When the CLI takes only part of the text, or reports nothing within 5 s, the page says so: the text may not have arrived whole.

An answer pill sends its answer as a follow-up, in the detail or on a focus card.
`1` to `9` pick the same: the open session's, or on the idle page the card the list's cursor is on.

A dialog's option is picked with the keys its CLI takes, right after the page reads the screen again to check the dialog is still open and offers that option.
In one of Claude Code's numbered dialogs, a permission question or a menu it asks you to choose from, that is the option's digit; in every other dialog, the question whether to trust a folder and every one of Codex's, it is the arrow keys and then Enter, once the screen shows the option selected.

## With the TUI

The page and the TUI open each other's view of a session.
**Open in terminal** on a session asks every TUI that is open to put it up.
On the grid, the TUI lands on the session's card, in its project's tab, with the pane on it and the keys in it, as a `/` jump does; in the orchestrator view, the session takes the view, as `nebula orchestrator review` does.
The TUI you touched last also brings its terminal's window to the front: in WezTerm, its window and tab with that pane active; in Terminal, iTerm2 and Ghostty on macOS, the app.
Over ssh, or in another terminal, the window stays where it is.
The page says under the button whether the TUI put the session up or had it on screen already, or why it could not.
With no TUI open there is nothing to show it in, and the page offers neither this nor **Follow TUI**.

**Follow TUI** keeps the page on the session the TUI shows: the one in the pane of the TUI you touched last, with a key, a click or its window coming to the front.
While it is on, the button reads **Following TUI**, and when you switch it on with the page already where the TUI is, the page says so.
While no TUI is open, there is nothing to follow.

The other way, `Shift+W` in the TUI, or **Open in web app** on a card's right-click menu, opens the page on the session the pane reads, or on the board with none.
With no `nebula serve` running, the TUI starts one in the background, `nebula serve --no-open`, which outlives the TUI, and opens the page once it is up.
Over ssh there is no browser on that machine to open, and the TUI says so instead.

## Skills

**Skills** in the page's nav, or `http://127.0.0.1:7682/skills`, lists every skill the agent CLIs on this machine load, read off disk where each CLI looks for them, since none of them lists what it found.

- Claude Code's: its managed settings folder's `.claude/skills`, `~/.claude/skills` and `~/.claude/commands`, each project's `.claude/skills` and `.claude/commands`, a `.claude` nested in a project's folders, the `.claude` of each folder a space's `add_dirs` names, its plugins' skills (named `plugin:skill`), and the skills synced from your claude.ai account.
- Codex's: each project's `.agents/skills`, `~/.agents/skills`, `~/.codex/skills` and the system skills it bundles there, and `/etc/codex/skills`.
- Cursor's: each project's and your home's `.agents/skills`, `.cursor/skills`, `.claude/skills` and `.codex/skills`, and its built-ins in `~/.cursor/skills-cursor`.

A CLI with no home folder on this machine is not read.
The projects are the daemon's, so with no daemon up the page lists every skill but the projects' own.

The list is a tree of folds, each one folded until you open it, so the whole of it fits on one screen.
**Global** holds what loads in every project: your own skills by where they come from (each git repository by name, plain folders, the `npx skills` installer), then **Plugins** with a fold for each plugin, its version and whether it is off, then the skills synced from claude.ai, and what each CLI has built in.
**Projects** holds what loads in one project only, a fold for each project under its space, saying how many of its skills a space's added directory gives every project in that space as well, and in how many spaces; open, it names them.
A skill in a space's added directory that is in no project has a fold of its own under that space.
A skill one CLI loads everywhere and another in one project only is in both places, under each with the CLIs that load it there, and a skill two projects load is under each of them.
A fold says how many skills it holds, where it is, and in red how many of them have a problem; open, a project's fold says how many skills a session there gets from each CLI.
A skill's row is its name, the CLIs that load it when they are not the ones most of its fold has, and a red dot when something is wrong with it.
At the end of a row Claude loads its skill on is when Claude last used it, `today` or how many days ago, and a fold's is that of its skill used last; a skill not used in the last 30 days has none.
**All**, **Claude**, **Codex** and **Cursor** under the filter keep only what that CLI loads.
`/` goes to the filter, which keeps the skills whose name, description or fold holds every word typed, opens the folds it leaves, and puts the cursor on the best match.
`j` and `k` move (in the filter, the arrow keys), `←` and `→` fold and unfold, Enter opens a skill or a fold, Esc goes back, and a click does what the key does.
The browser remembers which folds are open.

With no skill open, the page says what needs fixing, one row for each fix however many problems it causes.
The broken links in one folder are one row, and so are the skills one name is found under, saying what each CLI does about it; a skill's own file, and the skills only one CLI loads, are one more each.
Each row says what fixes it, and **Show them** lists the skills it is about.
A row the page can fix has a button for it, the first one drawn as the button to press, and [Fixing from the page](#fixing-from-the-page) says what each does; a fix it cannot make, such as renaming a skill, is said and left to you.
Under them, **Use, last 30 days** lists the skills Claude used most, each with where it loads and how many times, and **All by use** lists the rest.
Beside it, **Not used** says how many of the skills Claude loads it did not use: of your own, in each plugin, from claude.ai and in the projects.
It says what it does not count: skills switched off, built in, broken links, a skill another of its name hides, and those only Codex or Cursor load.
On a phone, where the list takes the screen, a row at its top says how many things need fixing and opens them as a screen of their own, `/skills/fixes`, with the use under them in one column.

A skill is one real folder, however many links lead to it.
Its page says, for each CLI, where its sessions get it, everywhere or in which projects, or why they do not: another skill of the same name that it loads instead, or the folders it reads.
Beside that it shows the path the CLI finds it at, and its switch, or the setting that switches it off where no switch can: Claude's `skillOverrides`, `enabledPlugins` and `syncClaudeAiSkills`, in whichever settings file wins, and Codex's `[[skills.config]]` and `[skills.bundled]`.
Cursor has no such setting.
It shows the real path, the repository and branch the folder is in with what that repository has not committed (`4 changed files`), the installer's source or the plugin and its marketplace, how often Claude used it in the last 30 days, when last and in which project, and in tabs the `SKILL.md` itself, the frontmatter as a table, and the files beside it.
**Edit SKILL.md**, or `e`, edits the file there, as [Editing a skill's file](#editing-a-skills-file) says.
**Delete** beside it moves the skill to the Trash, as [Deleting a skill](#deleting-a-skill) says.
A deep link opens it: `/skills/<id>`, the id a hash of the skill's real path.

It points out what is wrong with a skill:

- a broken link, or a `SKILL.md` that links out of its folder, which the page never reads;
- frontmatter that does not parse, which Claude Code loads as no fields at all;
- a `name` other than its folder's, or one the Agent Skills spec does not allow, and a description over its 1,024 characters;
- no `name` or `description`, which Codex needs both of;
- two skills one CLI finds under one name, saying which one it uses;
- a skill of your own that only one CLI loads.

Use is read from Claude Code's transcripts, in `projects` under its config folder (`~/.claude`, or `$CLAUDE_CONFIG_DIR`), its subagents' included.
A use is a `Skill` tool call, or a slash command whose name is one Claude loads a skill under, so `/model` and `/clear` are not counted, and a call a resumed session repeats counts once.
A use is in the project whose root checkout, or one of whose worktrees, its session ran in.
Codex's and Cursor's use is not read.
A transcript last written more than 30 days ago is not opened, and `nebula serve` keeps what each one held until its size or time changes, so after the first read a page load opens only the transcripts that changed.
The page asks for the use apart from the list, at `/api/skills/usage`, so the list never waits for it.

### Switching a skill on or off

On a skill's page, each CLI that loads it has a switch beside where it loads.
Claude's is **On**, **Name only**, **Slash menu only** or **Off**: its `skillOverrides` entry for the name Claude lists the skill under, `name-only` listing it to the model without its description, `user-invocable-only` hiding it from the model and keeping it in the `/` menu, and `off` hiding it from both; On is no entry.
A skill Claude loads everywhere is switched in your own settings, `~/.claude/settings.json` (under `$CLAUDE_CONFIG_DIR` when that is set).
One it loads in one project only is switched in that project's `.claude/settings.local.json`, which is not committed and which Claude Code also reads for the project's worktrees; Claude reads it before the project's `.claude/settings.json` and your own, so On writes `on` there when one of those says otherwise.
A plugin's skills have the plugin's switch, `enabledPlugins` in the same file, which turns the whole plugin on or off.
Codex's switch is **On** or **Off**: a `[[skills.config]]` entry in `~/.codex/config.toml` (under `$CODEX_HOME` when that is set) naming the skill's `SKILL.md` by its path, one for each path when a link leads to it.
An entry already there is flipped where it is, and a new one goes last, since the last entry that selects a skill wins.
A skill Codex has built in is switched the same way, on its own; `[skills.bundled]`, which turns every built-in off at once, is read and never written.
Cursor has no switch.
A skill another of its name hides, one Claude loads through a space's added directory or in several projects, and one your managed settings or `syncClaudeAiSkills` decide has no switch, and says what switches it.

Claude Code watches its settings files, so a running Claude session goes by a changed `skillOverrides` from its next turn: it no longer offers the model a skill switched off or to the slash menu only, though one it listed earlier in that conversation stays in what the model has read.
A plugin goes on or off for new Claude sessions, and for a running one after `/reload-plugins`.
Codex reads its config when it starts, so a running Codex session goes by a change once restarted.
The page says so under the switches.

In the list, each plugin under **Plugins** has a switch on its row.
On the overview, a fix that one switch cures has a button for it: a skill of yours under the name of one Codex has built in gets **Turn off Codex's built-in**, the same write as that skill's switch, and the fix goes once Codex lists one of them.
Under **Not used**, **Tidy up** names every skill of your own that Claude loads everywhere and did not use in 30 days, and one button makes them all Slash menu only in one write; **Undo** then puts each back as it was.

Every write is the same.
The page names a skill or a plugin by the id the list gave it, never a file, and the server writes the file its own scan says that CLI reads for it.
It reads the file, changes the one key in its text, writes the result to a file beside it, and renames that over the file only once the file reads as it did, so a change Claude Code or Codex made meanwhile is not lost: the page says the file changed, and writes nothing.
A file that is a link is written at its target, and stays a link.
In JSON every other byte stays as it was, every key in its order and indent; TOML keeps every comment and line the change does not touch.
Before anything is written, the changed text is parsed again and held to the one change: the JSON must be the file as read with only that key set or taken out, and the TOML must differ only in that skill's `[[skills.config]]` entries; anything else is refused as a change that could not be made safely, and nothing is written.
A file that is not there yet gets only that key, and one that does not parse is never written: the page says why, where the switch is.
After a write the page reads the list again and says, under the switch, which file it wrote and, when that file is in a git repository, how the repository stands: `dotfiles: 1 changed file, nothing committed`.
Nothing is ever committed.

### Fixing from the page

On the overview, a fix the page can make has a button on its row, and on a phone the buttons go under the row's text.
Every act names the fix by the id the list gave it, never a file, and the server works out each path from its own latest scan, looking at each one again right before it acts.
After an act the page reads the list again and says, above what needs fixing, what it did, which files, and how each repository it touched stands: `acme-api: 28 deleted files, nothing committed`.
Nothing is ever committed.
Each act has an **Undo** there, which the server carries: it puts back only where nothing is now, and says what it left alone.
An Undo is held by the `nebula serve` that made the act, so it goes when that server stops.

**Remove N links**, on a folder's broken links, first asks on its row, naming every link it removes and saying what git will show: the removals as deleted files when git tracks the links, or nothing when it ignores them or does not track them.
It removes only the links the scan found dangling in that folder, each one only while it is still a link, still points where it did, and still points at nothing.
One that changed since the list was read is left as it is, and the page says so.
It never follows a link, and never removes a file or a folder.
Undo makes each link again, with the target it had.

**Remove the command**, on a skill of a name an older command also has, removes the command file only when git can bring it back: when git has it committed, with no change to it, and no link leads to it.
It is checked again right before, and asked on its row first, saying what git will show.
Undo writes it back from git, with the bytes and mode it had.
A command git does not hold as it is, or that Claude loads somewhere no skill of its name does, is never removed: the row says why, and leaves it to you.

**Keep both**, on a name two skills share, a name a skill shares with a built-in, or **Keep as is** on a skill only one CLI loads, takes the row off the list until one of its files changes.
It is kept in `skills-kept.json` in nebula's data directory, by the fix and a hash of the text of each file it is about, so touching a file without changing it keeps it off and any change brings it back.
Its problems are then not counted in the list's red, and its skills' pages say it is kept.
Under what needs fixing, **N kept · show** lists what is kept, each with **Show again**.

**Compare**, on two copies of one skill, and **Compare with** the other copy under a skill's own problem, shows the two `SKILL.md` files side by side, read-only, with their paths and the lines that are not in the other marked; on a phone they stack.
When the two are the same byte for byte, it says so in one line.

### Editing a skill's file

**Edit SKILL.md** at the top of a skill's page, or `e`, opens its file in place of the tabs: the whole text, frontmatter included, in a plain textarea as tall as the text, with the file's real path and the links that lead to it.
A legacy command is edited the same way, under its own file name.
The page edits your own files: a skill in a git repository or a plain folder of yours, a project's, a plugin's when its marketplace is a folder on this machine, and one the `npx skills` installer put there, which says that the installer's next update replaces it.
It does not edit what something else keeps and would write over, and says why in one line where the button would be: a plugin's copy in Claude's plugin cache, a skill synced from claude.ai, a CLI's built-in, the managed settings folder's and Codex's admin folder's, a broken link, and a file it cannot read or that is 1 MiB or more.

As you type, the server checks the text the way it checks a file on disk, and the page lists what it finds: frontmatter that does not parse, a `name` other than its folder's or one the spec does not allow, a description over 1,024 characters, and a missing `name` or `description` where Codex loads the skill.
A problem does not stop a save, but Save then reads **Save with N problems**, and a problem the page had not shown yet when Save was pressed is shown first, so saving with it takes a second press.
**Preview** shows the text after the frontmatter as the page renders a `SKILL.md`.
`Cmd+S` (or `Ctrl+S`) saves, and Esc or **Cancel** leaves at once when nothing changed and asks first when something did.
Text typed and not saved stays with the page while you look at other skills, and coming back opens it again; closing the tab asks first.

A save names the skill by the id the list gave it and sends the version its page read the file at, a hash of the file's bytes, never a path.
The server writes the file its own scan found, at a link's target, the link left a link and the file's mode kept, and writes the bytes as they were typed: a file whose line breaks were all CRLF keeps them, and a final newline is there only when you left one.
It reads the file right before, and writes nothing when the file is not at that version, or changed while it was being written.
The page then keeps your text, says the file changed on disk, offers to show the file as it is now beside yours with the lines that differ marked, and Save becomes **Save over it**, which writes yours over the new file.
After a save the page reads the skill again and says, where the editor was, which file it wrote and how its repository stands: `toolbox: 1 changed file, nothing committed`.
Nothing is committed.
**Undo** puts back the text the file had before the save, only while the file still holds what was saved, and like an act's Undo it is held by the `nebula serve` that saved.

### Deleting a skill

**Delete** at the top of a skill's page only asks: a row under it names the skill, lists each path that goes, and says what git will show, with a **Delete** of its own and **Cancel**.
Nothing is deleted until that second **Delete**; no key skips the step, Esc or **Cancel** leaves it, and while it asks the keys are on **Cancel**.
What goes is the skill everywhere: its real folder, or a command's file, and every link the list found leading to it, in each CLI's folders and in each project.
A plain folder with no link to it is only that folder, and a broken link only its link.
The row says what git will show for each repository: `In toolbox, this shows as 4 deleted files, and nothing is committed.`, that git ignores or does not track it, or, for a folder in no repository, that the Trash holds its only copy.
For a skill the `npx skills` installer put in `~/.agents/skills`, its entry in `~/.agents/.skill-lock.json` goes too, every other byte of the file kept, since `npx skills update` installs again a skill its lock still names.

Everything goes to the Trash, so it outlasts the server: on macOS the one Finder shows, through macOS's own call for it, and elsewhere the freedesktop.org Trash in `~/.local/share/Trash` (or `$XDG_DATA_HOME/Trash`).
The real folder goes first, so nothing goes when that cannot.
The page does not delete what something else keeps, as it does not edit it, and says why in one line where the button would be: a plugin's cached copy, a skill synced from claude.ai, a CLI's built-in, the managed settings folder's and Codex's admin folder's, and a plugin's own folder when the plugin is its one skill.
A skill the page neither edits nor deletes for one reason says so in one line.

A delete names the skill by the id the list gave it and sends the version its page read what goes at, a fingerprint of each path, each link's target and a hash of its `SKILL.md`.
The server looks at each path again right before, and deletes nothing when a link leads somewhere else now, the `SKILL.md` changed, or anything is not as the page read it; the page then says why where the button is, and reads the skill again.
After a delete the page goes back to the list and says, above it, what went and how each repository stands: `toolbox: 2 deleted files, nothing committed`.
Nothing is committed.
**Undo** there moves each thing back from the Trash only where nothing is now, and the installer's entry once the folder is back; like an act's Undo it is held by the `nebula serve` that deleted, and once that server stops, the Trash still has it all.

## Schedules

**Schedules** in the page's nav, or `http://127.0.0.1:7682/schedules`, lists what nebula's daemon runs on its own, with no page or TUI open: a prompt in a project, or the orchestrator's rank of the focus picks.
`nebula schedule` does the same from a shell ([Commands](commands.md#schedules)).

Each row is a schedule's name, its project, when it runs and when next: `api · weekdays 07:30 · next tomorrow 07:30`.
Its dot is its last run's: green for one that ran, yellow for one whose turn still waits, red for one that failed, hollow for one skipped or none yet, and red too while the session its last run went to waits on you, as that session's row on the board is.
With no rank scheduled, a band under the list offers the morning rank, each weekday at 07:30, in one click: **Add the morning rank**.
**Add** at the top of the list opens the form.

A schedule's page, `/schedules/<id>`, says what it runs and where, its harness, model, effort and permission mode, when it runs on this Mac's clock, the session a same-session schedule keeps, and every run it kept, newest first.
**Run now** runs it as if it were due, and leaves when it runs next as it was.
**Remove** asks on a row of its own first, and removes the schedule with its runs; the sessions it started stay.
With no schedule open, the page lists the recent runs of every schedule.

A run says when it was due, what it did, and the session it went to, linked, with that session's dot:

- **Started**: a new session on the prompt.
- **Held**: the prompt waits for the schedule's one session to be at rest with an empty input box.
- **Typed**: the prompt was typed into that session.
- **Restarted**: that session's CLI was started again on a new conversation, the prompt its first.
- **Skipped**, saying why: a run the Mac slept through more than ten minutes, or one held while the last still waits.
- **Failed**, saying why: a project gone, a CLI not found, a session that went before it took its turn.

A run that ran late says so under it: `Caught up 2 h 12 min late, as the Mac slept or nebula's daemon was down`.

### What a schedule runs

A prompt runs in a project, on a harness, a model and an effort, the CLI's own when none is named, and for Claude a permission mode, Claude's `--permission-mode`: `manual`, `acceptEdits`, `auto`, `plan`, `dontAsk` or `bypassPermissions`, or none, the mode your own Claude settings give it.
Each start of a session's CLI has the mode, a resume included, so a session a schedule started stays in it.
Another harness launches as nebula launches it, and a mode for one is refused.

Where each run goes:

- **A new session in a new worktree**: each run cuts a worktree of its own, named for the schedule and when it ran, `nightly-triage-1116-0730`.
- **A new session in the root checkout**.
- **One session, each run typed into it**: the first run starts it in the root checkout, and each later run's prompt is typed into it once it is at rest with its input box empty and its last turn asking you nothing, as a held turn is ([Orchestrator](orchestrator.md#start-from-the-web-app)).
  A session the idle reaper took is brought back first, on its conversation.
- **One session, each run on a new conversation**: the same, but its CLI is started again on a new conversation, the prompt its first, so each run starts with fresh context; the old conversation stays where the CLI keeps it.

A run of the one session while the last run's turn still waits is skipped, saying so, never held beside it.
A session archived or deleted is not typed into: the next run starts another, and says so.
The one session needs a harness that reports its status to nebula, as a held turn waits for it to be at rest, and to keep its context one that resumes a conversation: Muse, Grok Build and a custom harness with no hooks are refused it, with why, and run each time in a new session instead.

The morning rank is the orchestrator's RANK TURN, held until it is at rest with an empty input box, as **Re-rank** holds it ([Focus](#focus)).
`nebula serve` holds the candidates it ranks, so a rank due with no serve running starts one first, as `Shift+W` does, through your login shell, so it reads Linear and GitHub with your keys.

### When

- **Every N minutes**, the first N minutes after it was added.
- **At a time of day**, on the days picked, on this Mac's clock: a daylight saving change moves it with the clock, a time the clock skips as it springs forward runs as late as the clock skipped (02:30 at 03:30), and one it shows twice as it falls back runs once.
- **Once**, at a day and time.

No cron syntax.
The daemon compares each schedule's due time with the wall clock every second, so a Mac that slept through a due time finds it due the moment it wakes:

- a run ten minutes late or less is on time;
- a time-of-day run later than that is skipped, saying why, but a daily one, which runs every day of the week, runs once late on the day it was due, saying so, and one missed on a day before is skipped;
- a once run later than that is skipped;
- an interval run that missed several runs runs once, for all of them, and the next is a period after it.

### Kept, and the login item

The daemon keeps the schedules and their runs, the newest 50 of each, in `schedules.json` in nebula's data directory, written renamed into place, with the permission mode of each session a schedule started.
Every page and CLI is told of a change at once.

Schedules run while the daemon does.
After a reboot that is once a TUI starts it, unless the login item starts it when you log in: `nebula login-item on` ([Commands](commands.md#schedules)).
While it is off the page says so, over the recent runs.

A paired phone has the page too: it can read the schedules, and add, run and remove them, as it can already type into every session and answer its dialogs.

## From a phone

A phone, or any device on your tailnet, reaches the page through `tailscale serve`, which serves it at the Mac's tailnet name over HTTPS with Tailscale's own certificate and proxies it to `nebula serve` on 127.0.0.1.
It needs Tailscale running on the Mac with HTTPS certificates on for the tailnet (MagicDNS and HTTPS, on the admin console's DNS page).

### Serving it on the tailnet

Run this once on the Mac, with the port `nebula serve` serves on:

```sh
tailscale serve --bg 7682
```

That serves `https://<the Mac's tailnet name>/`, such as `https://my-mac.tailnet-name.ts.net/`, to the tailnet alone, and keeps serving it across restarts of Tailscale and of the Mac.
It takes the place of anything else `tailscale serve` served at `/` on port 443.
`tailscale serve status` shows what it serves, and `tailscale serve --https=443 off` stops it.
Never point `tailscale funnel` at it: a request from the internet names no Tailscale user, and the page refuses it anyway.

`nebula serve` learns the Mac's tailnet name from `tailscale status --json`, the `tailscale` on `PATH` or the macOS app's, when it starts, and again when a request through `tailscale serve` names a host it does not know, so there is nothing to set.
With no Tailscale running, no phone can reach it, and the page on the Mac says so.

### Pairing a phone

Nothing reaches the page over the tailnet until it is paired, the board, the API, the live stream and the app's own files alike.
A device that is not paired gets one plain page in place of every other: **Pair this device from nebula on your Mac**, with a field for a code.

1. On the Mac's page, **Phones** in the top bar opens the phones panel, and **Pair a phone** shows a one-time link, its QR code, and its code, `K7QX-M2PD`.
2. Scan the QR code with the phone's camera and press **Pair this device** on the page it opens, or type the code on the phone's own pairing page.
   The phone's name comes from its browser, `iPhone`, and can be changed there.
3. The phone is in, and the panel on the Mac says `Paired iPhone.`

A code pairs one device, once, for five minutes; a new **Pair a phone** takes the place of the code before it, and ten wrong codes void it.
A device paired again, with its old cookie, takes the place of the one it was.

To have the page on the phone's Home Screen, pair it in Safari first, then choose **Add to Home Screen** in Safari's Share menu.
It opens as an app of its own, full screen under the status bar, with nebula's mark as its icon.
A Home Screen app keeps cookies apart from Safari's, so one that opens unpaired shows the pairing page: type the code there.

### Removing a device

The phones panel lists every paired device: its name, whether its notifications are on, the Tailscale user who paired it, when it was paired and when it was last used.
**Remove** lets it in no more: its next request gets the pairing page, and a page it has open goes to that page at once.
It also drops the device's push subscription, so it is sent no more notifications, and so does pairing the device again.

### Notifications

A paired phone can be told when a session needs you, as a notification on its lock screen, with no page open on it or on the Mac.

To turn them on, tap the bell at the right of the top bar on the phone, then **Turn on notifications**, and allow them when the phone asks.
On an iPhone or iPad this works only in the Home Screen app, as iOS shows notifications from no other page: in Safari the panel says to add nebula to the Home Screen first, and to turn them on there.
A Home Screen app that opens on the pairing page is paired there with a typed code ([Pairing a phone](#pairing-a-phone)).

The bell says how they stand: green while they are on, struck through while they are off, and red, struck through, while the phone blocks them.
Blocked notifications are allowed again in the phone's settings, which the panel names: on iOS, **Settings**, then **Notifications**, then **nebula**, and **Allow Notifications**.
Coming back to the page reads them again.
**Turn off** stops them for that phone alone.

A notification goes when a session turns red, the board's **needs you**: a dialog opens in it, such as a permission prompt or the question whether to trust a folder, or a turn ends on a `QUESTION` or `BLOCKED` report line.
Its title is the session's project and name, `shopfront › Rotate the staging API key`, and its text is the line the board says under the row, what the dialog asks about or the report line.
Tapping it opens the page on that session.
A session that turns red again after you answered it, or that asks another question, sends another, which takes the place of the one before for that session.
The orchestrator sends none, as it is not on the board.

None goes while you are at the Mac: a session that turns red is held until the Mac has had no keyboard or mouse input for a minute, and goes then only if the session still needs you.
So a question you answer at the Mac never reaches the phone, and one you leave there does, a minute after you step away.
On a machine whose idle time cannot be read, it goes at once.

The daemon sends them, straight from the Mac to the phone's push service, Apple's for an iPhone, with no relay of nebula's own, so `nebula serve` need not be running, and a daemon from before this build sends none until `nebula reload`.
Each is encrypted for that phone alone and signed with a key nebula makes once, `vapid.json` in its data directory, readable by you alone; the push service is told no more about the sender than `mailto:push@nebula.invalid`.
The subscriptions are kept beside `devices.json`, a file for each device under `push/`, and a subscription the push service says is gone is dropped.
`push-sent.json` holds the sessions already sent, so a daemon that restarts does not send them again.

**Send a test**, beside each device whose notifications are on in the Mac's phones panel, sends it one now, and says whether its push service took it.

### What a paired device can do

A paired device does all the page does on the Mac, but for pairing, removing and testing devices, which the Mac's page alone does, and turns its own notifications on and off, which the Mac's page does not.
It is let in only as the Tailscale user who paired it: `tailscale serve` names the user behind each request in its `Tailscale-User-Login` header, and a device whose user is not the one that paired it gets the pairing page, saying so.
A device of no user, such as a tagged server, is refused.

The device keeps a token in a cookie only the page's HTTPS host gets, one no script reads, for 400 days, the longest a browser keeps one.
`nebula serve` keeps only a SHA-256 hash of each token, in `devices.json` in nebula's data directory, readable by you alone, with the device's name, user, and when it was paired and last used.

## Who can reach it

It listens on 127.0.0.1 only, so nothing off this machine can connect but through `tailscale serve`.
A request must name the server as its host: `127.0.0.1` or `localhost` at the port it serves on, through no proxy, or the Mac's tailnet name through `tailscale serve`, over HTTPS, with a Tailscale user.
A web page that resolves some other name to this machine (DNS rebinding) is refused, and so is a request through a proxy that names this machine's own loopback.
A request over the tailnet must also come from a paired device ([From a phone](#from-a-phone)).
A request that would act on a session, write a setting or change a file must also come from the page's own origin, and no response carries a CORS header, so another site's page can neither act through the server nor read what it answers.

## Building it

The page is a React app in `crates/nebula-web/app`, built into the binary.
`make build` and `make install` build it first (`make web`, which needs Node and pnpm), and released binaries carry it.
A plain `cargo build` needs no Node: without a build of the app, the binary serves a page that says how to make one, and the API under `/api` works as it is.
`pnpm dev` in the app's folder serves the page against fixture data, with no daemon at all.

## What comes next

[Web UI roadmap](web-roadmap.md) lists the next slices, the gaps known today, and how to carry on the work.
