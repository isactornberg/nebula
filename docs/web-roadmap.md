# Web UI roadmap

<sub>[← Web](web.md) · [README](../README.md) · [How it works](how-it-works.md)</sub>

What comes next for `nebula serve` and its page, the gaps known today, and how to carry on the work.
Everything [Web](web.md) describes is built.

## Decisions

- The page is the "Threads, refined" design: a rail, a list of sessions, issues or projects, and a session as a thread of its turns, with Home the day and the focus picks when nothing is open.
  Across the top of a session or an issue is one line, issue, session and pull request; the live terminal is a toggle beside the thread.
- Dark is Catppuccin Macchiato and light Catppuccin Latte, following the system until a device picks one; Latte's quiet text and tones are darkened where they are text, for 4.5:1.
- Plain CSS on the tokens in `app/src/tokens.css`, no component library, and Inter and JetBrains Mono served from the binary, so the page asks no font host for anything.
- The page also shows the day from agentos, a personal toolbox, read-only, from a file agentos writes; the notes and the todo list stay in their own editors.
- Focus is the board's rows by rule, with the orchestrator choosing and explaining at most three picks on top.
  Code gathers status for free; the orchestrator only ranks.
- Schedules live in the daemon, not in launchd jobs.
- A reply's answers come from its `NEXT:` line, read by the quick-answers mod's own rule, on the page only; the board never reads the line.
- The TUI stays the coding surface and gets no new views: `Shift+W` opens the page, and **Show in TUI** brings the TUI forward.

## Where it lives

- The API contract is `app/src/api/contract.ts`, mirrored in `crates/nebula-web/src/api.rs`, and the page's calls are `app/src/api/client.ts`.
- The shell, `app/src/Shell.tsx`: the rail, a phone's top bar and tab bar, and More; `App.tsx` holds the routes, the keys and what is open.
- The list, `Side.tsx`, its rows and views read by `model.ts`; Home and the focus cards, `Idle.tsx`; the day, `Today.tsx`, from the file `crates/nebula-web/src/today.rs` reads.
- A session's page, `Detail.tsx`, with its thread in `Thread.tsx`, the composer in `Compose.tsx`, its images in `Attach.tsx` and dictation in `Dictate.tsx`; an issue's page and Start are in `Detail.tsx` too.
- The palette, `PaletteDialog.tsx` and `palette.ts`; New, `NewSession.tsx`, which posts to `POST /api/sessions` (`crates/nebula-web/src/send.rs`).
- Skills, `SkillsPage.tsx` and `skills.ts`, resolved in `crates/nebula-web/src/skills/`; Schedules, `SchedulesPage.tsx` and `schedules.ts`, kept by the daemon in `crates/nebula-daemon/src/schedule.rs` behind protocol 63.
- Phones and pairing, `Phones.tsx`, `crates/nebula-web/src/pair.rs`, `devices.rs` and `tailnet.rs`; notifications, `Notify.tsx`, `app/public/sw.js` and `crates/nebula-daemon/src/push.rs`.
- The Deck, `DeckPage.tsx` and `deck.ts`, at `/deck` with its own `deck.webmanifest`.
- The theme, `theme.ts`, which keeps each device's choice where `index.html` and the pairing page read it before the first paint; the styles, `tokens.css`, `app.css`, and `new.css` and `palette.css` beside their components.
- Turn history is the daemon's `ReadTurns` behind protocol 65 (`crates/nebula-daemon/src/turns.rs`), served at `GET /api/sessions/:id/turns`.
- Linear and the pull requests are read by `crates/nebula-web/src/refresh.rs`; the focus picks are `focus.rs`, the rank a held turn for the orchestrator behind protocol 62; Start is `start.rs`, a held turn for a captain behind protocol 60.
- `pnpm dev` serves the page against the mock in `app/src/mock/`, every endpoint of the contract; its flags are listed at the top of `mock/plugin.ts`.

## Next

- Capture a todo from the page.
- GitHub issues in the lanes, beside Linear's.
- Images in the orchestrator's field.
- The Deck on a Stream Deck +, maybe, if the phone falls short: the same layout, its eight keys the session tiles (press to show, hold to mark seen), and its four dials Session (turn through every session, push to show), Answer (turn through the options, push to pick or send), Today (turn through the day, push to open it on the Mac) and agentos, whose push opens the nightly log and whose 2 s hold is `agentos off`, or on again.
  It needs `nebula session show`, `seen` and `answer` in the CLI beside `send`, so a plugin on 127.0.0.1 can act without faking the page's origin to get past serve's write guard.
  The plugin would live in this repo, in TypeScript on Elgato's SDK, read `/api/events` with the types in `contract.ts`, start `nebula serve --no-open` when none runs, and draw its keys and strip as SVG in the TUI's status colours.
- Roles: launch presets that carry instructions, permission mode and tools, compiled to each CLI's flags (Claude `--agent` or `--append-system-prompt`, a Codex profile, a Cursor rule file).
- A Tauri window around the page, only if a global shortcut or `nebula://` links prove necessary.

## Known gaps

- **Show in TUI** raising the right WezTerm window and tab with several windows open has not been checked by hand.
- A long dialog option loses its wrapped tail: Claude's "2. Yes, and always allow access to <path>" reads as "Yes, and always allow access to".
- Codex dialogs are only partly checked: arrows move the selection, digits behaved unpredictably.
- A reply's time (`Reply.at`) is always null, since the report read carries no timestamp.
- The live terminal polls the screen once a second while it is shown.
- Dictation has run only against a stand-in recognizer in desktop Chrome: a real iPhone's Safari and its Home Screen app, where Apple's recognizer may refuse the page, have not tried it.
- That the composer stays above the iOS keyboard is read from the visual viewport, and has not been seen on a real iPhone.
- The blur iOS draws under the status bar of a Home Screen app is gone on a real iPhone, by the status bar's `default` style and a sticky top bar with a solid fill; on a real iPad, where the rail stands at the top in place of the top bar, it has not been looked at.
- A running `nebula serve` keeps serving the app it was built with: restart it after a rebuild.
- The Skills page reads each project's root checkout only, not its worktrees' `.claude/skills`, and not Codex's `$CWD/../.agents/skills`.
- Skill use is read from Claude Code's transcripts only.
  Codex's session logs record a skill the user names (`$name`) as a `skill` item in the user's message, but a skill Codex picks itself only as a shell read of its `SKILL.md`, so Codex's use is not counted.
- A use in a checkout nebula does not know of, outside a project's root, is in no project.
- It lists skills only: a plugin's `commands/` are not listed, and neither are Cursor's own switches, which are not known.
- A plugin enabled in a project's settings only is shown as off.
- Synced claude.ai skills are named `anthropic-skills:<name>`, as Claude Code 2.1 lists them; the prefix is not documented.
- A switch's check that the file is as it was read and its rename are two steps, so a write another program makes between them is lost; neither CLI locks its settings files.
- That a running Claude session goes by a changed `skillOverrides` from its next turn is read from Claude Code 2.1's bundle (its settings watcher, and the skill listing it builds each turn), not seen in a live session.
- Which of a linked skill's paths Codex matches `[[skills.config]]` against is not documented, so a switch writes an entry for each.
- A skill that a space's added directory gives its projects, or that several projects load, has no Claude switch: no one file switches it for those sessions alone.
- A fix's, a save's or a delete's Undo lives in the `nebula serve` that made it, so it is gone once that server stops; the files are then put back by hand, by `git restore` for a command or a `SKILL.md` git holds, or from the Trash for a deleted skill.
- A save's last look at the file and its rename are two steps, as a switch's are, so a write another program makes between them is lost.
- A save puts a new file in the old one's place, with the old one's mode: a hard link to the old file keeps the old text, the file's owner becomes the user `nebula serve` runs as, and its extended attributes are not carried over.
- The editor checks a text for what its own file gets wrong; that its new `name` is one another skill has too shows once it is saved, and renaming the folder to match a new `name` is left to the user.
- Only the `SKILL.md` or command file is edited, not the scripts and references beside it.
- A file whose line breaks are mixed, or that has a lone CR, is saved with LF breaks, since a browser's textarea holds no other; the editor says so first.
- Text typed and not saved lives in the open page: closing or reloading the tab asks first, and a browser that quits loses it.
- A plugin's skill is edited only when Claude Code loads the plugin from its marketplace's folder, a marketplace added from a local path; one it loads from its cache is not, even when that marketplace is the user's own repository.
- A removal's last look at a link or a command and the removal itself are two steps, so a file put at that path between them would go: no call removes a path only while it is still a link, and git holds no lock to take.
- Compare marks the lines that differ only while the two files have at most about a million pairs of lines between them; longer ones are shown side by side unmarked.
- A kept fix's entry stays in `skills-kept.json` while that fix is not on the list, so a fix fixed by hand and made again with the same files is kept off again.
- A delete's last look at each path and its move to the Trash are two steps, so a change made between them goes to the Trash with the rest.
- A delete takes the links the scan found: a link in a checkout nebula does not know of is left pointing at nothing, and shows as a broken link once the page reads its folder; a link that leads to the skill through another link takes only the first.
- The installer's lock is read at `~/.agents/.skill-lock.json`, as the scan reads it, not under `$XDG_STATE_HOME/skills`, where the installer keeps it when that is set.
- Whether Finder's **Put Back** works for what a delete moved to the macOS Trash has not been checked; Undo moves it back either way.
- Outside macOS, the Trash is the one in the user's data dir, so a skill on another disk than it is refused rather than moved to that disk's own Trash.
- Protocols 59 to 65 are fork gates above upstream's numbers, renumbered on every rebase.
- The issue lanes and a session's issue know only the open issues assigned to you: a session on someone else's issue, or on one done or canceled since, shows its key from its URL and no title or state.
- A project whose issues are GitHub's gives no issues to the lanes.
- A refresh reads the pull requests of the checkouts sessions work in only; a checkout with no session has none on the page.
- `checkout_prs` reads a branch's 10 newest pull requests, so a branch of ours whose name more than 10 forks' pull requests share (`main`, say) may miss its own.
- The TUI reads its pull requests and issues on its own, so a page and a TUI open together ask `gh` and Linear twice.
- A refresh runs the git reads of a project's checkouts one after another; only the projects run at once.
- Start has been run end to end only against stand-ins: a captain that types back what it is typed, and a `curl` that answers as Linear does.
  How a real Claude captain follows the brief, and Linear's own answers to the states query and the move, have not been seen.
- The page knows a brief was typed, not that the captain started the worker: a captain that ignores it, or whose worker fails to start, leaves the issue saying it was handed over, with no worker coming, until Start again.
- A held brief goes to the captain it was held for: one archived before it takes the brief drops it, and the page still says the issue was handed to that project.
- A brief the captain takes only part of is said in the daemon's log, not typed again, as typing it again could start two workers.
- A turn counts as asking the user only by its report line, a `QUESTION` or `BLOCKED` as the board reads it: the orchestrator ends its replies with no such line, so a question it ends a turn on does not hold a done note back, and a captain's question with no `QUESTION` line does not hold a held turn.
- A held turn comes off the held list, and out of its file, just before it is typed, so a daemon that stops in that moment loses it rather than type it twice.
- A TUI older than protocol 61 still types its own merge notes, beside a newer daemon's turns, until it is restarted onto the new build.
- A `send` waits for no held turn: one typed in the moment a held turn has passed its last look goes in first, and the held turn follows its Enter as a turn the CLI queues, never in one box with it.
- `held-turns.json` replaced `held-briefs.json`, which no build that shipped wrote: a brief a development daemon held in the old file when it moved onto this build is not typed.
- A refresh that runs while Start moves an issue can land the state it read before the move; the next refresh puts it right.
- `issue-starts.json` keeps the 100 issues Start handed over last; an older one shows no record.
- Today is read-only: capture from the page is still to come.
- Whether Obsidian opens agentos's `obsidian://open?path=` links to a sheet and a project note has not been clicked by hand.
- `Quiet for 30 days or more` is the page's own label, while the 30 days are agentos's: a writer that lists projects quiet for another span is labelled wrongly.
- A todo due at a time of day is counted late or due by its date alone.
- The day file is looked at only while `nebula serve` is connected to a daemon, since the page has no state without one.
- Pairing has been run end to end only in desktop Chrome, as an emulated iPhone with its safe areas, through a stand-in for `tailscale serve` that sets its headers as its source does; a real iPhone through real `tailscale serve` has not been tried.
  That Safari keeps the Secure, SameSite=Strict cookie, that iOS copies it into the Home Screen app when the page is added after pairing, and how iOS draws the status bar there are read from WebKit's and others' notes, not seen.
- The pairing link names the tailnet name with no port, so it is right only for `tailscale serve` on port 443; on another port, type the code.
- The tailnet name is `Self.DNSName` alone, not every name `CertDomains` holds.
- A device's cookie lasts 400 days from pairing and is not renewed by use; after that it is paired again.
- Ten wrong codes void a pairing, but nothing counts across pairings: a device on the tailnet gets ten tries at each code the Mac opens.
- An image in a follow-up has been sent only from desktop Chrome as an emulated iPhone, through the `tailscale serve` stand-in: a real iPhone's paste into the box, its photo picker, its camera, and what iOS Safari hands over for a HEIC photo under each of the picker's formats have not been tried.
- That Claude Code takes a pasted line that is an image's path for the image is its paste handler's behaviour in 2.1.327, not a documented interface, and a later Claude may change it; the receipt would then still hold, since the prompt holds the path.
- Codex 0.163's `view_image` has been seen opening a sent image; Cursor, Pi and OpenCode have not been sent one.
- Every Codex send in the isolated instance that tried images, with images or without, reported no receipt within 5 s, so the page said the text may not have arrived whole though it had.
- An image uploaded and never sent stays in `attachments/` for its week, as does one whose follow-up was refused.
- The Deck's swipe has been seen only under Chrome's Android emulation, whose touch slop is 15 px, not on a real phone or tablet: that Safari on iOS hands a sideways move under `pan-y` to the page, and how far a real Android Chrome, whose slop is 8 dp, lets a hold drift up or down before it takes the move as a pan of its own, are read from their documentation and Chrome's source, not seen.
  Neither iOS's edge swipe back nor Android's back gesture can be made in an emulator, so that each leaves a swipe that starts off the edge to the page is not seen either.
- Push has been seen end to end against a stand-in push service, over HTTPS, that checks each JWT against the key the phone subscribed with and decrypts each message with the phone's own key, from stand-in sessions turning red under an isolated daemon, and in desktop Chrome, subscribed through the page and the `tailscale serve` stand-in, with real pushes through Google's FCM shown by its service worker.
  A real iPhone has not been tried, so Apple's push service has taken none yet: the first thing that test checks is that Apple accepts the JWT's `sub`, `mailto:push@nebula.invalid`, which is generic on purpose, and its 12-hour `exp`.
  Whether iOS keeps the subscription of a Home Screen app paired in it, how its lock screen shows the title and text, and that a tap opens the session have not been seen either.
- A tap on a notification has been checked only from its message on: headless Chrome shows no notification to tap, so the service worker's `notificationclick` has not run on a real tap.
- The idle hold reads the Mac's input alone: a session that turns red while you watch the page on the phone, away from the Mac, is pushed to that phone all the same.
- A push service that keeps a message for an offline phone keeps it an hour; a session that is no longer red when the phone comes back still shows its notification.
- Every paired phone with notifications on gets every push: there is no choosing which sessions, or which phones, beyond turning a phone's off.
- A subscription's endpoint may be any `https://` host, not only a known push service's, so a paired device can have the Mac post to a host of its choosing; that is left open on purpose, since a paired device can already type into every session.
- `attention` and `picks set` need `nebula serve` running, since it holds the candidates: a scheduled rank starts one, and leaves it running, as `Shift+W` does.
- An orchestrator launched before this build has the old guidance in its system prompt until it is launched again: the rank turn names both commands, so it ranks by the turn alone, without the guidance on reasons, the heredoc or feedback.
- Each rank is a turn in the orchestrator's conversation, so it costs more as that conversation grows, and it grows the conversation by a few thousand tokens.
- A **Rank again** pressed while you talk to the orchestrator is held and lands once it is at rest with its box empty, as a turn of nebula's between yours.
- Ended is read from the orchestrator's status: a turn of yours typed the moment its rank turn ended is taken for the rank's, and the page says the rank is under way until that one ends.
- What a rank asked for is lives in the serve, and when the daemon last typed one in the daemon, so a restart of either forgets it: the page then shows no rank under way, and the picks still land.
- Attention prints the 25 most urgent candidates; the rest are counted, not listed, so a long Todo lane past them is never picked.
- A pick keyed by a session's id goes once an issue joins the session, which keys it by the issue from then on, and the rule or the next rank takes its place.
- Not today ends with the day on the Mac's clock, whatever the clock of the phone it was said on.
- The login item has been written, linted and read back against a temporary HOME only: no real login has started a daemon from it yet.
  That launchd's login shell finds `nebula`'s own needs, and how macOS shows the new background item, are still to see.
- The daemon a login item starts runs `zsh -l -c`, so its own environment is the login profile's; agents and a rank's serve start through `-l -i`, as from a TUI's daemon, but a worktree hook the daemon runs gets only what `.zprofile` sets.
- A schedule's first run in a folder Claude does not trust yet waits red on its question whether to trust it, as any new session there does: answer it once, and the runs after it go unattended.
- Weekday schedules do not catch up: a laptop asleep at 07:30 skips that day's morning rank, and the page's own rank on opening stands in.
- A schedule cannot be edited or paused: remove it and add it again.
- Removing a schedule removes its runs; each schedule's log keeps its newest 50.
- Each new-worktree run leaves its worktree behind, one a run, for the user or **Auto-clean merged worktrees** to remove.
- A same-session schedule's session is always in the root checkout.
- Fresh context costs a CLI boot each run, and the old conversation stays only in the CLI's own history.
- A once schedule's time is an instant, picked on the clock of the browser or the shell that added it; a time of day is the Mac's clock, so a phone in another zone sees the next run of one in its own zone's time.
- `manual` is Claude Code 2.1.325's mode; an older Claude refuses the flag, and the session's start fails.
- Only Claude has been run for real on a schedule; the same session on Codex, Cursor, Pi and OpenCode has been checked by its rules, not run.
- Daylight saving has been crossed in the unit tests' zone, not on the Mac's clock.
- A run under way when the daemon stops is not run again: a held turn is kept in `held-turns.json` and typed after the restart, but a new session that was being created when the daemon stopped is lost, and the run says nothing more of it.
- A correction repeated across runs is the orchestrator's to notice, by the guidance, from `feedback`: nebula counts nothing, and the standing rule has been seen proposed by no real run yet.

## How to carry on the work

- Work in a worktree, not the main checkout: dev-watch rebuilds and reloads the dev daemon on every saved change there.
  Land each finished step on `main` as one commit and fast-forward.
- Change the API contract in `contract.ts` and `api.rs` in the same commit.
- The gate is `make ci` plus `pnpm typecheck && pnpm test && pnpm build` in `crates/nebula-web/app`.
- Look at every screen in a real browser at 1440 × 900, 1024 × 768 and 390 × 844, in both themes, and the Deck at 1180 × 820 and 844 × 390 too, with `chrome-devtools-axi` and a session name of its own, before landing.
- After landing on `main`: `make web` in the main checkout, wait for dev-watch to rebuild (a dev-watch started before it watched the app's build needs a `touch crates/nebula-web/build.rs`), then restart the `nebula serve` on port 7682.
