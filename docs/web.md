# Web

<sub>[← README](../README.md) · [Keys](keys.md) · [Commands](commands.md) · [Sessions](sessions.md) · [Configuration](configuration.md) · [How it works](how-it-works.md)</sub>

`nebula serve` puts this machine's sessions in a browser tab: the board, each session's conversation as a thread of its turns, the answers it offers and the dialog it shows, and its live screen.
Under the board are the Linear issues assigned to you that no session works on, and each session shows the issue it works on and the pull request on its checkout.
**Start** on one of those issues hands it to a project's captain, which starts a worker on it.
Home, the page with nothing open, has **Today**, a day another program writes to a file: today's training, the todos due, and how its nightly job and its automation stand.
The page answers them too: a follow-up, with images pasted into it or picked from a phone's photos, one of the answers a reply offers, an option of the dialog a session shows.
**New** starts a Claude session in one of your projects on a prompt of yours ([Starting a session](#starting-a-session)).
**⌘K** finds any session, issue, project or action from one field ([Search or jump](#search-or-jump)).
Its **Skills** page lists every skill the agent CLIs on this machine load, switches one on or off for a CLI, and fixes what it can: it removes broken links and the old command a skill hides, and keeps a fix you decline off the list.
It also edits a skill's `SKILL.md`, and deletes a skill to the Trash.
Its **Schedules** page lists what nebula's daemon runs on its own, a prompt in a project or the orchestrator's morning rank, adds one, runs one now and removes one, with every run kept and why one was skipped ([Schedules](#schedules)).
A phone on your tailnet reaches the page too, through `tailscale serve`, once it is paired, and installs it to its Home Screen ([From a phone](#from-a-phone)).
Its **Deck**, `/deck` or **Deck** in the page's rail, is a page of its own for a phone on its side on a stand beside the keyboard: a tile for each session, tapped to put it up in the TUI and held to read and answer it there ([Deck](#deck)).

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

Down the left edge is the rail: nebula's mark, which goes Home, **New**, **⌘K**, the list's views, **Schedules**, **Skills** and **Deck**, and at its foot **Phones** (or, on a paired device, **Alerts**, its notifications' bell) and the theme.
Beside it is the list, 360 px wide, narrower below 1,064 px, and beside the list the pane it opens, or Home while nothing is open.
Skills and Schedules have lists of their own, up to 440 px wide, beside the page each opens.
The mark, the list's head and the first row of every pane sit on one line across the top.
Below 932 px, where Home has no room beside the list, the list is the page while nothing is open, Home's focus heads its Inbox, and the day is a view of its own, **Today**.
On a phone, 640 px wide and less, the rail gives way to a top bar, the mark, the orchestrator's thread ([The orchestrator's thread](#the-orchestrators-thread)), search, which opens the palette, and the bell, and a tab bar at the foot: **Inbox**, **Running**, **Issues**, **Today** and **More**, which holds **Orchestrator**, **Projects**, **Schedules**, **Skills**, **Deck**, the theme and **Refresh**.
There the list's head ends in **+**, which opens New, on **Inbox**, **Running** and **Projects**.

The list's views are **Inbox**, the board, with how many rows need you on its badge; **Running**, every session at work, a captain's workers too; **Issues**, the assigned issues no session works on ([Issues and pull requests](#issues-and-pull-requests)); and **Projects**, every session under its project's name.
**All projects** at the list's top keeps the rows of one project, and for Issues the issues of the Linear team it reads.
The list's foot holds what the TUI shows, with **Follow TUI** ([With the TUI](#with-the-tui)), the day's line while Home is away ([Today](#today)), when Linear and the pull requests were read, with **Refresh** ([Issues and pull requests](#issues-and-pull-requests)), and **Orchestrator**, with how the orchestrator stands, which opens its thread ([The orchestrator's thread](#the-orchestrators-thread)).

Each row is a session's status dot, its name and how long it has been in that state, and under them its project, the issue's key when its branch does not name it, its branch, and the line the board says of it, with what it asks, `Question`, `Blocked` or `Dialog`, or its pull request's state in a pill.

The board is the TUI's, `needs you`, `done`, `running` and the captains at rest, in attention order, with the line each row says under it read off the session's screen the way the TUI reads it, except that a project's captain never stands for its other sessions: each one running or done has its own row beside the captain's.
Home is the day: the date and what the board holds, `3 need you · 2 done · 4 running`, the focus, **Today**, **Shipped today**, the workers that finished today with each one's pull request, and **Coming up**.
The focus is at most three things worth your time, the orchestrator's picks, each a card with why it chose it and the answers a session's reply offers, or the options of the dialog it shows; the list keeps them in their sections too.
Until the orchestrator has picked, the focus is by rule: the first three rows that need you or are done, the ones that need you first, each with that line as its reason ([Focus](#focus) says how the two meet).
Running sessions and captains at rest are never in focus, so with nothing that needs you or is done the page says nothing needs you.

The page is dark, Catppuccin Macchiato, or light, Catppuccin Latte, as the system is, until the theme at the rail's foot, in More on a phone, or in the palette picks one, which that browser keeps.
Latte's quiet text and its tones are darkened a step where they are text, so every word on the page reads at 4.5:1 or more in both themes.
The live terminal, the Deck and the pairing page follow the same theme.
The page is set in Inter, with JetBrains Mono for keys, branches, paths and code, and for a skill's file as it is edited, both served from the binary.

A session's reply is the last one it wrote: for Claude, the newest reply in its transcript, as Markdown; for every other CLI, the text of its screen.
When a Claude reply ends on a `NEXT: <a> | <b> | <c>` line, the page offers those answers, read by the same rule the `quick-answers` mod for Claude Code draws its pills by.

A Claude session's conversation is read from its transcript too, turn by turn, for the page to show as a thread.
Each prompt is as it was typed, a slash command as its name and arguments and an image as the `[Image #3]` Claude Code shows, numbered across the session, and each reply is the newest message of its turn, by the rule the last reply is read by, with when each was written.
A prompt typed while Claude works is a turn where Claude took it in, and the reply after it answers it.
Only the transcript's prompts and replies are read, never the copies of each request's messages Claude Code writes beside them.
What the CLI writes in your place is left out: system reminders, a command's output, a tool's call and its result, a background task's notice, and a compaction's summary.
A turn nebula typed, a Start's brief or a merge note, is kept, as the reply after it answers it.
The page is given the newest 40 turns and 256 KiB of their text at most, and told when the transcript holds older ones.
The transcript is read back from its end until it has them, 64 MiB of it at most, as for the last reply, and a transcript unchanged since the last read is not read again.
It asks for them apart from the session's detail, at `/api/sessions/<id>/turns`, so neither the detail nor the Deck waits on a long transcript.
A daemon from before this build reads no turns, and the page shows the last reply alone until `nebula reload`.
A dialog open in the session is shown with its question and its numbered options.
A captain lists the other sessions of its project, in attention order.
The screen is the session's terminal as text, read again every second while it is shown.

### A session's page

A session opens as a thread: your prompts as bubbles on the right, its replies as Markdown messages on the left, each with how long ago it was written, and the newest at the foot.
It opens at the newest turn and stays there as turns come, until you scroll up, and again once you scroll back down.
Over the first turn shown it says what the session is, `Captain for ACME-900 in rover-firmware, on main`, or that earlier turns are not shown, when the transcript holds older ones.
A reply's closing `QUESTION`, `BLOCKED` or `DONE` line is shown as its report, and its `NEXT:` line goes where its answers are drawn under the thread.
A captain's workers are quiet lines in the thread, by when each came to where it stands: started, running, finished and the pull request it opened.
A dialog the session shows is its newest message, with what it asks to run, and its options are under the thread.
While the session works, a quiet line at the foot says `Claude is at work`.
A session in the focus says why at the top of its thread, `Focus #1 · …`, with **Not today** and **Wrong** ([Focus](#focus)).
A session with no turns, not Claude's or on an older daemon, shows its last reply alone, read off the screen for every CLI but Claude.

The head names the session, its state and, for a captain, that it is one; under the title, its project, and the branch of its worktree when it has one of its own (`api · fix-login`), on a phone too.
The line under that reads issue, session and pull request from left to right: the issue's key, title and Linear state, the session with its root checkout's branch or its captain, and its pull request with its state, or a captain's workers with theirs.
A captain called `captain`, as every project's captain is, is named by its project wherever nothing beside it names one: `worker of api's captain`, `Workers of api's captain`, the composer's `Reply to api's captain…`, Archive's confirm and the browser tab.
A worker's captain there opens the captain, and on a captain's page **Workers**, or the `+N` past the three it names, lists every one of its workers, a row each that opens it.
**Show in TUI** puts the session up in a TUI that is open ([With the TUI](#with-the-tui)), **Mark seen** marks a finish nobody has looked at, and ⋯ holds **Follow TUI**, the issue and the pull request, **Archive**, and the branch.

**Archive** closes a session as the TUI's Archive does, once a confirm, a sheet along a phone's foot, says what it does: the agent stops and the session leaves the list, and its worktree and branch stay.
On a captain's page the confirm says its workers keep running, and only the captain goes.
The page goes back to the list, whose foot says `Archived <name>.` with **Undo** for 8 seconds, which brings it back as the TUI's Unarchive does; after that, Unarchive in the TUI's archived view does.
A session brought back from the archive has no process until something starts it again, so its page says it stopped rather than that it is at work.
The orchestrator's page has no Archive: `F10` in the TUI is where it starts.
The web app deletes no session, worktree or branch.
**Terminal**, or `t`, shows the session's live screen, read every second while it is shown and never while it is not: on a wide screen it takes the list's room beside the thread, and on a phone or a narrow window it takes the thread's place.
It is drawn in the theme's colours, a crust well in dark and a mantle one in light, as large as its columns fit across, and **Open in TUI** over it puts the session up in the TUI.
Each browser remembers whether it is shown.

Under the thread are the answers the reply offers, or the dialog's options, which `1` to `9` pick, and the composer.
However many there are, the composer keeps its room: past 30% of the screen's height they scroll.
On a phone the page is a screen of its own, its head sticky at the top with a way back named for the list it came from, `‹ Inbox`, the terminal and ⋯ as icons, and the line under its project as chips: a worker's captain, the issue, the session's state, a captain's workers, which list them all, and the pull request.
The composer stays above the on-screen keyboard there, and while the keyboard is up the answers or options step aside, so the box and the thread have the room it leaves; they are back once it is down.

An issue opened from **Issues** or a focus card has a page in the same look: its title, state and priority, the same line across its top with no session yet, what Linear says of it, **Linear** to open it there, and **Start** ([Starting an issue](#starting-an-issue)).

A deep link opens the page on a session: `http://127.0.0.1:7682/s/<session-id>`.
Opening a finished session nobody has looked at marks it seen, as putting it up in the TUI does.
A seen finish leaves `done`, but the Inbox keeps the open one's row where it was, as the TUI's board keeps the session on screen, until another session opens or the list does; then the row folds away.

### The orchestrator's thread

**Orchestrator**, at the list's foot, `o` on every page, or on a phone the ✦ in the top bar and **Orchestrator** in **More**, opens the orchestrator's conversation, the one `F10` puts up in the TUI, as a session's page: its turns from its Claude transcript, the turns nebula types into it among yours, such as `[nebula] Rank the focus picks …`, and the composer at the foot, where Enter sends, with images and dictation as in a session's thread.
Its head names it **Orchestrator**, with its state and what it does, and has no issue, session and pull request line; **Show in TUI** puts up the TUI's orchestrator view, and **Terminal** its live screen.
It lives at `/orchestrator`, which stays the orchestrator's when it is started again as a new session, and the session's own `/s/<id>` goes there.
The top bar's ✦ carries a dot while the orchestrator's reply waits unread or it shows a dialog, as a row's dot does.
While no orchestrator runs, the link says `Not running`, the ✦ is faint, and the page says `No orchestrator runs: F10 in the TUI starts one.`

## Keys

Outside a text field, on Home and on a session's or an issue's page:

| Key | Does |
| --- | --- |
| `j` / `k`, `↓` / `↑` | move the list's cursor |
| Enter | open the row the cursor is on |
| Esc | go back Home, then clear the find |
| `/` | go to **Find a session** |
| `1` to `9` | pick that answer or dialog option: the open session's, or on Home the one on the focus card the cursor is on |
| `t` | show or hide the live terminal |
| `r` | read Linear and the pull requests again |
| `s` | ask **Start** on an issue's page |
| `n` | open **New**, on every page |
| `o` | open the orchestrator's thread, on every page |
| ⌘K, or Ctrl+K off a Mac | open or close the palette, on every page and in a text field too |

In the composer, Enter sends and Shift+Enter adds a line; on a touch screen Return adds a line and the button sends.
The Skills page has keys of its own, listed at its list's foot ([Skills](#skills)), Esc goes back to the list on a schedule's page, and the Deck takes Enter, Space and `s` ([Deck](#deck)).
A key never acts while a menu, a dialog or an IME's composition has it.

## Finding a session

**Find a session**, the field at the top of the list, finds any session on this machine, the ones the board leaves off too: those at rest and stopped that it counts under it, `28 idle · 1 stopped`.
It keeps the sessions whose name, project, branch or issue key holds every word typed, in any order and case aside, so `shop cart` finds `Persist the cart across devices` in shopfront, and `acme-940` the session on `acme-940-persist-cart`.
While it has a word in it, the list is its matches alone, in place of the view it shows: the board's rows first, in its order, then the sessions it leaves off, each with its status, project, name and age, and its branch under the name, with the issue's key when the branch does not hold it.
The orchestrator comes last, as `orchestrator`, and opens its thread.
When nothing matches, the list says so: `No session has “gps” in its name, project, branch or issue.`
A tap or a click on a match opens it as any row does, and the find stays, so going back shows the same matches; ✕ in the field, or Esc, clears it, and the board is back.

`/` goes to the field, as it goes to the Skills page's filter.
In it the arrow keys move through the matches, the cursor on the first as you type; Enter opens the one the cursor is on and leaves the field, and Esc clears the field, then leaves it.
Out of it, `j` and `k` move through the matches and Enter opens one, as on the board, and Esc goes back Home, then clears the find.

## Search or jump

⌘K, or Ctrl+K off a Mac, opens the palette on every page, in a text field too, and pressing it again closes it; so do **⌘K** in the rail and, on a phone, the search in the top bar.
It is one field over the page, the page dimmed and out of reach behind it, and on a phone the whole screen, with **Cancel**.
Until you type, it offers the five sessions that changed last and every action.
What you type finds, group by group:

- **Sessions**: every session on the machine, by the rule of [Finding a session](#finding-a-session), in its order.
- **Issues**: the lanes' issues, by key or title.
- **Projects**: by name; picking one shows **Projects** filtered to it.
- **Actions**: by name.

Each wants every word typed, in any order and case aside, and marks where they are.
The actions are **New session**, **Rank again** while an orchestrator runs, **Refresh Linear and pull requests**, **Follow TUI** while a TUI is open, the theme, **Schedules**, **Skills**, **Deck** and, on this Mac, **Phones**.
With a session open, its own come first: **Reply**, which puts the cursor in its box, the live terminal, **Show in TUI** while a TUI is open, and **Mark seen** for a finish nobody has looked at.
The arrow keys move through the results, Enter opens the one lit, a click or a tap opens any, and Esc or a click beside it closes it.
Once it closes, the focus is back where it was before it opened.

## Focus

The orchestrator chooses the focus, and says why, from what nebula gathers for free: the rows that need you or are done, and the issues in the lanes ([Issues and pull requests](#issues-and-pull-requests)).
Code reads the status, Linear and the pull requests; the orchestrator only ranks.
Each pick is a card: a session's, with the answers its reply offers or the options of the dialog it shows, or an issue's no session works on, with its state and priority, **Start**, which opens the issue with Start's question asked ([Starting an issue](#starting-an-issue)), and **Not now**, which is Not today.
A pick of an issue a session works on opens that session.
Over the cards the page says who picked and when: `The orchestrator picked three at 09:12.`

**Rank again**, at the top of Home or beside Focus where the focus heads the list, asks it to pick again.
nebula types the orchestrator one turn for it, `[nebula] Rank the focus picks …`, only once it is at rest with its input box empty and its last turn asking you nothing, as it types a done note ([Orchestrator](orchestrator.md#ranking-the-focus-picks)), so a rank never lands in the middle of a conversation with you.
Until then the page says `Ranking once the orchestrator is at rest with an empty input box.`, then `The orchestrator is ranking.` while it works, and Rank again reads **Ranking…** and waits; pressing it again joins the rank under way.
A rank whose turn ends with no picks set says `The orchestrator's last rank ended without setting picks.`
With no orchestrator, Rank again is off: `F10` in the TUI starts one.

Opening the page, or coming back to it, asks for a rank too, but only when the picks are due: the orchestrator has never picked, or its picks are over three hours old and the candidates changed since, a row or an issue come or gone or moved to another section.
Such a rank goes only while the orchestrator is at rest with its input box empty right then, never held for later, and at most four times a day on this Mac's clock.
A rank on a schedule, such as one every weekday at 07:30, is a schedule of its own ([Schedules](#schedules)).

Each of the orchestrator's picks has **Not today** and **Wrong**, behind the ⋯ at its card's top right and beside `Focus #2` at the top of its page.
Either asks first why, a reason you may leave empty, and then puts the pick aside: it leaves the focus at once, and the page says so.
**Not today** keeps it out of the candidates until the day ends, so the orchestrator cannot pick it again today; **Wrong** says the pick was a mistake, and it may be picked again later.
The next rank reads what you said of picks in the last week, with your reasons, and when you correct the same kind of pick for the same reason again, the orchestrator proposes a standing rule for the `CLAUDE.md` in its folder, and writes it there only once you say yes.

The rule stands in for the orchestrator's picks, and the page says why under Focus:

- it has not ranked yet: `The orchestrator has not ranked yet.`;
- its picks are over three hours old and the candidates changed since: `The orchestrator's picks from 06:10 are over three hours old, and the board has changed since.`;
- none of them is a candidate now, each answered, read or gone: `None of the orchestrator's picks from 09:12 is on the board now.`;
- you put each one aside: `You put the orchestrator's picks from 09:12 aside.`;
- Linear and the pull requests, which the candidates are keyed by, are not read yet, as right after `nebula serve` starts: `The orchestrator's picks from 09:12 show once Linear and the pull requests are read.`

A pick that is no candidate any more, a question you answered or a finish you read, leaves the focus on its own, and the rest stay.
The orchestrator may leave out rows that need you, judging something else more worth your time, so the page never says nothing needs you while one does: under its picks, **Also needs you** lists the rows that need you and that it did not pick, and when it picked nothing at all they are listed under **Needs you** in the cards' place.
In the list every row that needs you stays under **Needs you**, picked or not.

`nebula serve` keeps the picks, what you said of them and when the page asked for a rank on its own in `focus.json` in nebula's data directory, so every browser shows the same and a restart keeps them.

## Issues and pull requests

`nebula serve` reads two things the daemon does not keep: the open Linear issues assigned to you, and the pull request on each checkout a session works in.
It reads them when the page opens and nothing has been read yet, and when you press **Refresh** at the foot of the list, in More on a phone, or `r`.
It never reads them on a timer.
Beside the button the page says when it last read them, `Updated 4 min ago`, kept current while the page is open.
A part that could not be read says why in a line under it, `LINEAR_API_KEY is not set in nebula serve's environment.` or `Pull requests in api: gh timed out.`, and keeps what the read before had of it.
Pressing Refresh while a read is under way joins that read; there is never a second.

The issues are Linear's, read with the `LINEAR_API_KEY` in `nebula serve`'s own environment, so a serve the TUI starts (`Shift+W`) has the TUI's.
They are the issues assigned to you that are not done or canceled, in each Linear team a project reads its issues from: its **Issues** setting on the Project tab (`linear:ACME`), or its space's, which wins, as the TUI's issues list reads it.
A project whose issues are GitHub's gives none; that is still to come.
One query reads every team, a page of 100 issues at a time until there are no more.

A session works on an issue by the first of these that holds:

- its issue URL names it, as a session started from an issue has;
- the issue's key is in its checkout's branch as a whole word, as nebula cuts a worktree for one (`acme-123-fix-login` is ACME-123's, and `acme-1234-x` is not);
- its branch is the one Linear suggests for the issue.

A session's page then names the issue with its title and state across its top, and its key opens it in Linear.

The pull request is GitHub's, read with `gh` in each project: one query per project with a session in one of its checkouts, however many checkouts.
A checkout's pull request is its branch's open one, a draft included, else its newest while the checkout has nothing past that one's head, merged or closed, by the rule the TUI's PR row keeps.
It shows across the top of a session's page and on a focus card as `ready for review`, `draft`, `merged` or `closed`, the TUI's words.
A project whose remotes name no GitHub repository has no pull requests, and says nothing.

**Issues**, a view of the list, is the assigned issues no session works on, in two lanes: **In flight, no session**, started in Linear with nothing running, and **Todo**, not started yet.
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
The captain is typed a brief: start one worker on the issue, on the branch nebula cuts for it (`acme-123-fix-login`), with `nebula spawn --issue`, so the worker carries the issue and the board joins the two.
The brief is typed only once the captain is at rest with its input box empty and its last turn asking you nothing, never into a turn under way, a prompt of yours or in place of your answer to a `QUESTION` it asked: at once when it is, and otherwise held until it is, across a daemon restart too, one turn at a time with the merge notes the captain is owed.
[Orchestrator](orchestrator.md#start-from-the-web-app) says how the daemon holds it.

Once the daemon has the hand-off, the page moves the issue to its team's first started state in Linear, the `started` state first in the team's order, with the same `LINEAR_API_KEY` it reads with.
It asks Linear the issue's state first, and moves nothing that is started already, done or canceled, so Start never moves an issue back or sideways.
A move that fails leaves the hand-off as it is, and the page says why under the issue's title: `It was not moved to its started state in Linear: Linear timed out.`
The page shows the new state at once, with no refresh, and the issue goes to **In flight, no session**.

A handed-over issue's row says so under its title, `handed to api · 2m`, or `handed to api · brief held` while the brief waits, and its page says which project's captain has it, since when, and whether the brief waits.
`nebula serve` keeps that in `issue-starts.json` in nebula's data directory, with the project each Linear project's issues last went to, so every browser shows the same and a restart keeps it; it keeps the 100 issues handed over last.
**Start** on an issue handed over already asks too, and says so: `ACME-123 was handed to the captain of api already. Start it again in api?`
Once the captain's worker is up, the session joins the issue by its URL, the issue leaves its lane, and a page open on `/i/<key>` goes to that session.
The TUI shows none of this, and reads nothing more for it.

## Today

**Today** on Home is a day another program writes to a file, such as the `today.json` agentos writes each time it rebuilds its Home page.
The page reads it and never writes it, and shows it only while `today_file` names it ([The day file](#the-day-file)).

- **Training**: the block the week is in (`Week 3, build`), what today holds, linked to its sheet, and how many lifts the week has had.
- **Todos**: how many are overdue or due within a week and how many of those are late, then the first three, each with how many days late it is (`12d`), `today`, or how soon it is due (`in 3d`), and `+4 more` to the rest.
  The page never lists more than three.
- Under **Coming up** at Home's foot, how the nightly job last ran, `Nightly ok · 03:17` or `Nightly failed`, with its day when it was not today, and `Automation on` or `Automation off`, the program's kill switch, then the schedules that run next, soonest first, each linked to its page.
  A failed run says why on a line of its own.

Each thing links to where it is edited: a web link opens in a new tab, and any other, such as an `obsidian://` link, hands off to its app.
A part the program could not read says why in its place, as the todos do when the list could not be reached and the training when its log could not, and the chip says it is unread.

While a session or an issue is open, Home is not on screen, and the day is a line at the foot of the list instead: `Today`, today's training up to its first comma, the todos late or due, the nightly run's word, and `automation off` while it is.
Its dot is the colour of the worst of it, red for something that failed, peach for todos late or automation off, and green otherwise; where the line is short of room it is cut short, and hovering it says all of it.
It goes back Home.
Below 932 px the day is **Today**, a view of the list, in the rail or the tab bar, whose tab on a phone has the same dot while something is late or failed.
On a phone the top bar is the mark, search, which opens the palette, and the bell, or **Phones** on the Mac's own page, each a round button with its icon alone; what the TUI shows is a line of its own under it, with **Follow TUI** at its end.
Disconnected takes a line of its own under the first; on a wider screen it is **Offline** at the rail's foot.

Capturing a todo from the page is still to come.

### The day file

`today_file` in `config.json` names the file, from `/` or `~/`:

```json
{ "today_file": "~/.local/state/agentos/today.json" }
```

There is no default: with nothing set there is no Today.
`nebula serve` looks at the setting and the file every second, and reads the file again whenever its size, time or inode changed, so a write shows within a second, with no restart.
A writer should write the whole file beside it and rename it into place, so the page never reads half of one.
A file that is missing or cannot be read is no Today, and one that is not a day this build reads shows why in its place, ``The day in /home/you/today.json could not be read: missing field `todos`.``, with the rest of the page as it was.

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
  "nightly": { "at": 1794536220000, "ok": true, "why": null },
  "off": false
}
```

- `version` is 1, the one this build reads; any other is refused with why.
- `at`, `nightly.at`: when it was written and when the nightly job last ran, in epoch milliseconds.
- `todos`: `count` and `overdue` of the todos overdue or due within a week, `top` the first few soonest due, each with its `due` date, `url` where the rest are, and `problem` why they could not be read, with nothing counted then.
- `training`: the week's `block`, what `today` holds, the `url` of its sheet, `lifts` of `liftsPerWeek`, and `problem` why the training could not be read, with no `block`, `today` or `url` and no lifts then; null when there is no training week.
  A file without `problem` is read as one whose training could be read.
- `nightly`: its last run, `ok` or not and `why`; null before its first.
- `off`: whether the program's kill switch is on, so its automation is off.

Every `url` is optional, a field this build does not know is ignored, and a link markdown-it would drop, such as `javascript:`, is shown as text.

## Answering a session

A follow-up is typed into the session the way `nebula session send` types one, and the page waits until the session's CLI reports taking the whole of it.
It is refused while the session is working, and while it has a dialog open, which the text and its Enter would answer blindly.
A session whose terminal is gone, reaped while idle or stopped by a daemon restart, is brought back first.
When the CLI takes only part of the text, or reports nothing within 5 s, the page says so: the text may not have arrived whole.

An answer pill sends its answer as a follow-up, on a session's page or on a focus card.
`1` to `9` pick the same: the open session's, or on the idle page the card the list's cursor is on.
In the composer, Enter or **Send** sends and Shift+Enter adds a line.
On a touch screen, a phone or a tablet, Return adds a line and **Send** sends, since its keyboard has no Shift+Return; ⌘↩ (Ctrl+↩ off a Mac) sends on any keyboard.

A dialog's option is picked with the keys its CLI takes, right after the page reads the screen again to check the dialog is still open and offers that option.
In one of Claude Code's numbered dialogs, a permission question or a menu it asks you to choose from, that is the option's digit; in every other dialog, the question whether to trust a folder and every one of Codex's, it is the arrow keys and then Enter, once the screen shows the option selected.

### Dictating a follow-up

The mic in the composer dictates a follow-up with the browser's own speech recognition, the Web Speech API, in English or Svenska.
The language is the browser's own when it is one of those, else English, until you pick one, which that browser remembers.
While it listens, the words come as you speak, those it still guesses at faint with a dotted line under them, beside how long it has listened and the language, which you can change as it listens.
**Stop** puts the words in the box, after what it held, to fix before you send; **Send** sends them as heard; **Cancel** drops them.
A word still being guessed when you stop is kept, and when the browser stops listening on its own, what it heard goes in the box.
On a phone it listens in a sheet over the thread, and while the box is empty the mic is the composer's main button, with **Say it** after the answers; on a wider screen it listens in the composer's place.
Where the browser has no speech recognition, or the page may not use the mic, the mic puts the cursor in the box and says to use the keyboard's mic.
When it hears nothing, or the network it needs does not answer, it says so, and keeps what it heard.

### Images in a follow-up

A follow-up can carry images, on a session's page and in the Deck's reader alike.
Paste one into the box, a screenshot or a photo copied on the phone or the Mac, drop one on it, or press **Image**, which on a phone offers its photos, its camera and its files.
Each image shows by the box with its name and size and a bar while it uploads, and ✕ removes it.
**Send** waits until every image is up, and a follow-up of images alone, with no words, goes too.
An image that cannot be sent is marked red and says why, and the follow-up waits until it is removed.

The images the session is sent are PNG, JPEG, GIF and WebP, the ones the agent CLIs read, of 20 MB at most.
A HEIC or HEIF photo, which an iPhone hands over when asked for its photo as it is, is sent as a JPEG that macOS's own `sips` makes of it; off a Mac it is refused, saying to share it as a JPEG.
Anything else is refused, whatever the browser calls it: the server reads what a file is from its first bytes.

`nebula serve` keeps each image as a file in `attachments/` in nebula's data directory, outside every checkout, readable by you alone, under a name no one can guess, and the follow-up names it by its path on a line of its own under your words.
Claude Code takes that line for the image itself, as it takes a file dropped on its box: the model is sent the picture, with no tool call and so no permission question, and the session's prompt shows `[Image #1]` in its place.
Codex is sent the path as text and opens the image with its own `view_image` tool, which it asks no permission for; every other CLI gets the path and reads the file with its own tools, as its permission settings allow.
The images in `attachments/` go once they are a week old, when `nebula serve` starts and whenever it keeps another, as the files the TUI's `^V` keeps there do.
A daemon from before this build sends no image, as it would say the follow-up arrived cut short: the page says so, and `nebula reload` moves it onto this build.

## Starting a session

**New** starts a Claude session in one of the daemon's projects, on a prompt of yours: pick the project, type the prompt, and paste or pick images as a follow-up takes them ([Images in a follow-up](#images-in-a-follow-up)).
**New** at the top of the rail, `n` outside a text field, **New session** in the palette and, on a phone, **+** at the end of the list's head open it at `/new`: a thread with nothing in it yet, the project across its head and the composer at its foot, which dictates as a reply's does and starts with **Start** or Enter.
The project it picks first is the one the list is filtered to, else the one that browser started a session in last, else the first.
nebula starts it in the project's root checkout, on the model and effort Settings → Agents sets for new sessions, as the quick prompt and `nebula session start` start one, named `agent-1` or the next `agent-N` free there until its first turn titles it, as `nebula spawn` names one.
Two started at once in one project take two names: the daemon gives the second the next one free.
The prompt is typed into it once its input box is up, as a follow-up is, and the page opens the session once its CLI reports taking the whole of it.
When it reports nothing within 5 s, the session asks something before it takes input, such as whether to trust its folder, or nebula serve loses the daemon meanwhile, as it does for a moment on `nebula reload`, the session stands, the page opens it all the same, and says over its thread that it started and that the prompt may not have arrived whole, or why it was not typed.
When the server refuses or does not answer, New says why under the composer and keeps the prompt and its images to try again; it keeps the prompt while the page is open, too, when you leave New and come back.
The page asks for it with `POST /api/sessions`, which a paired phone may send too, and a device that is not paired or another site's page may not ([Who can reach it](#who-can-reach-it)).

## With the TUI

The page and the TUI open each other's view of a session.
**Show in TUI** on a session's page, or **Open in TUI** over its live terminal, asks every TUI that is open to put it up.
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

**Skills** in the page's rail, in More on a phone, or `http://127.0.0.1:7682/skills`, lists every skill the agent CLIs on this machine load, read off disk where each CLI looks for them, since none of them lists what it found.

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

**Schedules** in the page's rail, in More on a phone, or `http://127.0.0.1:7682/schedules`, lists what nebula's daemon runs on its own, with no page or TUI open: a prompt in a project, or the orchestrator's rank of the focus picks.
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

The morning rank is the orchestrator's RANK TURN, held until it is at rest with an empty input box, as **Rank again** holds it ([Focus](#focus)).
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

## Deck

**Deck** in the page's rail, in More on a phone, `http://127.0.0.1:7682/deck`, or the same path on a paired phone or tablet, is the Deck: a page for a phone on its side on a stand beside the keyboard, with no rail and no top bar.
**‹ Inbox** at the start of its top row goes back to the rest of the page, the one way back in an app opened full screen, as one installed on Android is.
It is a grid of session tiles, a bar under them for the selected session, and the day along the top.

The screen sets how many tiles a page holds: eight on a phone, four by two on its side and two by four upright, and 12 to 16 on a tablet or a larger screen, such as four by three on a tablet of 1280 × 800 on its side and three by five upright, each about half again as wide as it is tall.
On a tablet the type on the tiles is larger too.
It is read again as the screen turns, and never from what the bar holds, so selecting another session never moves a tile.

Each tile is a session: its project, its name, its state and how long it has been in it, `Question · 4m`, in the list's colours.
A tile that needs you pulses, a running one has a thin bar moving along its foot, and one at rest is dimmed; with reduced motion on, the pulse is a steady ring and the bar stands still.
The tiles are the board's rows in its order, `needs you` first, then `done`, `running` and the captains at rest, and after them the sessions the board leaves off, idle or stopped, in the palette's attention order.
The orchestrator has no tile.
With more sessions than a page holds, dots page through the rest, or past four pages `‹ 1/7 ›`, which steps through them and never crowds out the day: at the right end of the top row on a screen on its side, under the tiles on one upright, so the tiles keep the width of the day and the bar.

- **Tap** a tile to put its session up in the TUI, as **Show in TUI** does ([With the TUI](#with-the-tui)), and to select it.
  The bar says for a moment whether the TUI put it up, or that no TUI is open to show it in.
- **Hold** a tile for half a second, until it lights up, and let go to read its session, as [The reader](#the-reader) says.
  A hold is never a tap as well, and opens no menu and selects no text, in Safari on iOS and in Chrome on Android alike; what it opens comes up as you let go, never under a finger still pressing.
- **Swipe** the tiles left for the next page and right for the one before, as the dots and arrows page, on a phone or a tablet.
  A swipe stops at the first page and the last, and the page it turns to comes in from its side, or is simply there with reduced motion on.
  A swipe is never a tap or a hold, and a hold never a swipe: a finger that goes more than 10 px from where it went down before it is held taps nothing and holds nothing, one held that then goes as far is nothing at all, and a swipe is at least 40 px across and more across than up or down, so a finger that drifts a few pixels still taps or holds.
  Up and down and a pinch stay the browser's, and so does the swipe in from the screen's edge that goes back, in Safari on iOS and on Android alike: a swipe that starts at the very edge goes back rather than turn the page.
- From a keyboard, Enter or Space taps the tile in focus and `s` holds it.

Tiles keep their places for three seconds after any touch, then settle into the new order, so nothing moves under a finger.
A swipe is a touch too: the page it turns to shows the tiles as they stood, and they hold still for three seconds after it.
A session gone meanwhile leaves its place empty until then, and a new one comes last.

The bar under the tiles is the selected session: its project and name, its line, and its answers as buttons, the recommended one first.
They are an open dialog's options, picked as a session's page picks them, or the answers the reply's `NEXT:` line offers, sent as a follow-up ([Answering a session](#answering-a-session)); with nothing to answer, it says so.
An answer is never cut short: the answers wrap onto another row, and a long one onto another line.
The first tile is selected when the page opens.
Tapping the session's name in the bar, with its `^`, reads it too.

### The reader

Holding a tile, or tapping the name in the bar, opens the reader over the tiles: the session's project, name and state, its last reply and the dialog it shows as its page's thread draws them, its dialog's options or its reply's answers, and its page's composer for a follow-up of your own, with images and dictation, sent as the session's page sends one ([Answering a session](#answering-a-session)).
On a screen on its side the reply is on the left and what answers it on the right; upright, one under the other.
Its text can be selected and copied.
**Show in TUI** puts the session up in the TUI while one is open, and ✕ or Esc goes back to the tiles.
Opening a finish nobody has looked at marks it seen, as opening it on the rest of the page does: a `Done` tile turns `Finished`.

Along the top, while there is a day file ([Today](#today)), pills for the todos due and today's training, each opening its own link, and how the nightly job last ran, with `automation off` while the kill switch is on, read-only.
With no day file there are none.

While the page is visible it keeps the phone's screen on, asking again each time it comes back, as iOS lets go of the lock when the page is hidden; a phone that refuses is left to sleep.
Disconnected says so at the top right, and a phone that is not paired gets the pairing page, as everywhere else.

On a phone, **Add to Home Screen** from `/deck` gives an app of its own, **Deck**, that opens on the Deck: the page names `deck.webmanifest` in place of the app's manifest, with `/deck` its start, since iOS opens a Home Screen app at its manifest's `start_url`, not at the page it was added from.
On Android, Chrome's **Install app** from `/deck` installs the Deck only where the nebula app is not installed: the app's scope, `/`, covers `/deck`, so with it installed Chrome offers only to open the app or add a plain shortcut.
Installed, the Deck opens full screen, as the nebula app does on Android ([Pairing a phone](#pairing-a-phone)); iOS has no full screen for a Home Screen app and opens it as before.
It can sit beside the nebula app on the same device.

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

Nothing reaches the page over the tailnet until it is paired, the board, the API, the live stream and the app's own files alike, but for Inter, the font every nebula serves alike.
A device that is not paired gets one plain page in place of every other, set in Inter: **Pair this device from nebula on your Mac**, with a field for a code.
It is dark or light as the page is on that device, the theme it picked there or else the system's.

1. On the Mac's page, **Phones** at the rail's foot opens the phones panel, and **Pair a phone** shows a one-time link, its QR code, and its code, `K7QX-M2PD`.
2. Scan the QR code with the phone's camera and press **Pair this device** on the page it opens, or type the code on the phone's own pairing page.
   The phone's name comes from its browser, `iPhone`, and can be changed there.
3. The phone is in, and the panel on the Mac says `Paired iPhone.`

A code pairs one device, once, for five minutes; a new **Pair a phone** takes the place of the code before it, and ten wrong codes void it.
A device paired again, with its old cookie, takes the place of the one it was.

To have the page on the phone's Home Screen, pair it in Safari first, then choose **Add to Home Screen** in Safari's Share menu.
It opens as an app of its own, full screen under a status bar in the top bar's colour, with nebula's mark as its icon.
A Home Screen app keeps cookies apart from Safari's, so one that opens unpaired shows the pairing page: type the code there.
On Android, pair it in Chrome, then choose **Install app** in Chrome's menu.
It opens full screen, with no status bar or navigation bar, for a tablet on a stand; Chrome on a Mac opens the same app in a window of its own.
An app installed before this build picks that up when Chrome next updates it, or at once when it is installed again.

### Removing a device

The phones panel lists every paired device: its name, whether its notifications are on, the Tailscale user who paired it, when it was paired and when it was last used.
**Remove** lets it in no more: its next request gets the pairing page, and a page it has open goes to that page at once.
It also drops the device's push subscription, so it is sent no more notifications, and so does pairing the device again.

### Notifications

A paired phone can be told when a session needs you, as a notification on its lock screen, with no page open on it or on the Mac.

To turn them on, tap the bell at the right of the top bar on the phone, or **Alerts** at the rail's foot on a tablet, then **Turn on notifications**, and allow them when the phone asks.
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
It sends images in a follow-up too ([Images in a follow-up](#images-in-a-follow-up)), and a device that is not paired can send none: its upload gets the pairing page's refusal, and nothing is kept.
It is let in only as the Tailscale user who paired it: `tailscale serve` names the user behind each request in its `Tailscale-User-Login` header, and a device whose user is not the one that paired it gets the pairing page, saying so.
A device of no user, such as a tagged server, is refused.

The device keeps a token in a cookie only the page's HTTPS host gets, one no script reads, for 400 days, the longest a browser keeps one.
The cookie is `SameSite=Lax`, not `Strict`, so it goes with a page opened from another app too: Chrome on Android counts an installed app's launch from its icon as one, and would open it unpaired with a `Strict` cookie.
Another site's page still cannot act with it, as anything but a read must come from the page's own origin ([Who can reach it](#who-can-reach-it)).
Each page load of a paired device sets the cookie again, so a device paired with a `Strict` cookie, before this build, takes the `Lax` one the next time it opens the page in its browser, without pairing again.
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

## Known gaps

Not yet tried on a real iPhone or iPad:

- Dictation, which has run only against a stand-in recognizer in desktop Chrome; in Safari and its Home Screen app, Apple's recognizer may refuse the page.
- The composer staying above the on-screen keyboard, read from the visual viewport.
- The blur iOS draws under the status bar of a Home Screen app, on an iPad, where the rail stands at the top in place of the top bar; on a real iPhone it is gone.

[Web UI roadmap](web-roadmap.md) lists every other gap known today, what comes next, and how to carry on the work.
