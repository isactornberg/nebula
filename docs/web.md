# Web

<sub>[← README](../README.md) · [Keys](keys.md) · [Commands](commands.md) · [Sessions](sessions.md) · [Configuration](configuration.md) · [How it works](how-it-works.md)</sub>

`nebula serve` puts this machine's sessions in a browser tab: the board, each session's last reply, the answers it offers and the dialog it shows, and its screen.
The page answers them too: a follow-up, one of the answers a reply offers, an option of the dialog a session shows.
Its **Skills** page lists every skill the agent CLIs on this machine load.

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
The focus at the top of the page is the first three rows that need you or are done, the ones that need you first, each with that line as its reason and the answers its reply offers; the board under it leaves them out.
Running sessions and captains at rest are never in focus, so with nothing that needs you or is done the page says nothing needs you.

A session's reply is the last one it wrote: for Claude, the newest reply in its transcript, as Markdown; for every other CLI, the text of its screen.
When a Claude reply ends on a `NEXT: <a> | <b> | <c>` line, the page offers those answers, read by the same rule Claude Code's `quick-answers` mod draws its pills by.
A dialog open in the session is shown with its question and its numbered options.
A captain lists the other sessions of its project, in attention order.
The screen is the session's terminal as text, read again every second while it is shown.

A deep link opens the page on a session: `http://127.0.0.1:7682/s/<session-id>`.
Opening a finished session nobody has looked at marks it seen, as putting it up in the TUI does.

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
**Projects** holds what loads in one project only, a fold for each project under its space, saying how many of its skills a space's added directory gives every project in that space as well.
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
Under them, **Use, last 30 days** lists the skills Claude used most, each with where it loads and how many times, and **All by use** lists the rest.
Beside it, **Not used** says how many of the skills Claude loads it did not use: of your own, in each plugin, from claude.ai and in the projects.
It says what it does not count: skills switched off, built in, broken links, a skill another of its name hides, and those only Codex or Cursor load.
On a phone, where the list takes the screen, a row at its top says how many things need fixing and opens them as a screen of their own, `/skills/fixes`, with the use under them in one column.

A skill is one real folder, however many links lead to it.
Its page says, for each CLI, where its sessions get it, everywhere or in which projects, or why they do not: another skill of the same name that it loads instead, or the folders it reads.
Beside that it shows the path the CLI finds it at, and whether a setting switches it off: Claude's `skillOverrides`, `enabledPlugins` and `syncClaudeAiSkills`, in whichever settings file wins, and Codex's `[[skills.config]]` and `[skills.bundled]`.
Cursor has no such setting.
It shows the real path, the repository and branch the folder is in, the installer's source or the plugin and its marketplace, how often Claude used it in the last 30 days, when last and in which project, and in tabs the `SKILL.md` itself, the frontmatter as a table, and the files beside it.
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

The page writes nothing: the fixes as buttons, and a switch for each CLI on a skill's page, are the next step.

## Who can reach it

It listens on 127.0.0.1 only, so nothing off this machine can connect.
A request must name the server as its host, `127.0.0.1` or `localhost` at the port it serves on, so a web page that resolves some other name to this machine (DNS rebinding) is refused.
A request that would act on a session must also come from the page's own origin, and no response carries a CORS header, so another site's page can neither act through the server nor read what it answers.

## Building it

The page is a React app in `crates/nebula-web/app`, built into the binary.
`make build` and `make install` build it first (`make web`, which needs Node and pnpm), and released binaries carry it.
A plain `cargo build` needs no Node: without a build of the app, the binary serves a page that says how to make one, and the API under `/api` works as it is.
`pnpm dev` in the app's folder serves the page against fixture data, with no daemon at all.

## What comes next

[Web UI roadmap](web-roadmap.md) lists the next slices, the gaps known today, and how to carry on the work.
