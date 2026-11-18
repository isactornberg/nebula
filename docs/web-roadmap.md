# Web UI roadmap

<sub>[← Web](web.md) · [README](../README.md) · [How it works](how-it-works.md)</sub>

What comes next for `nebula serve` and its page, in the order to build it, and how to carry on the work.
Slices 1 to 3 are on `main`: the shared client code, the read-only page, and acting from it (send, answer, seen, Open in terminal, Follow TUI, `Shift+W`), with Focus by rule.
Slice 4's Skills page is built in full: when each skill was last used, a switch for each CLI, the fixes it can make, each with an Undo, editing a `SKILL.md`, and deleting a skill to the Trash.
Slice 5 is built in full: the issue lanes, each session's issue and pull request, Refresh, and Start, which hands an issue to a project's captain.
Slice 6 is built, read-only: the Today band at the foot of the idle view and its chip in the top bar, from the file `today_file` names.
Slice 7 is built in full: a phone reaches the page through `tailscale serve` once it is paired, installs it to its Home Screen, and is pushed a notification when a session turns red.
Slice 8 is built: the orchestrator chooses and explains the focus picks, on Re-rank and when the page opens on picks that are due, with Not today and Wrong on each, and on a schedule.
Slice 9 is built: schedules in the daemon, `nebula schedule` and the Schedules page, the morning rank among them, and a login item that starts the daemon at login.
Slice 10 is built: the Deck, a page of session tiles for a phone on a stand beside the keyboard.
Slice 11 is built: images in a follow-up, pasted or picked from a phone's photos, which Claude Code is sent as images.

## Decisions already made

- The page also shows the day from agentos, a personal toolbox: one app, with a read-only Today rail fed by a file agentos writes.
  The notes and the todo list stay in their own editors.
- The layout is the merged design: Focus picks and lanes on the left, a detail pane on the right, and the Focus cards when nothing is selected.
- Focus is live lanes built by code, with the orchestrator choosing and explaining at most three picks on top.
  Code gathers status for free; the orchestrator only ranks.
- Schedules live in the daemon, not in launchd jobs.
- A reply's answers come from its `NEXT:` line, read by the quick-answers mod's own rule, in the web detail pane only.
  The board never reads the line.
- Plain CSS in Catppuccin Macchiato, no component library, to keep the JavaScript dependency tree small.
- The TUI stays the coding surface and gets no new views: `Shift+W` opens the page, and the page's Open in terminal brings the TUI forward.

## Next slices

| Slice | What | Needs first |
| --- | --- | --- |
| 4 | A Skills page, read-only, then editing | Nothing: no daemon or protocol change |
| 5 | Linear issues and pull requests in the lanes | Built |
| 6 | The agentos Today rail | Built, read-only; capture from the page later |
| 7 | The phone: `tailscale serve`, pairing, PWA, push | Built |
| 8 | The orchestrator's Focus picks | Built |
| 9 | Schedules in the daemon | Built, with the login item, which the user turns on |
| 10 | The Deck, a phone on a stand | Built |

### 4. A Skills page

A page for every skill the agent CLIs on this machine load, beside Attention in the page's nav, in the same list and detail layout.

Built: the page, as [Web](web.md#skills) describes it, resolved in `crates/nebula-web/src/skills/`.
Its list is the design picked as "Global and projects": a tree of folds, what loads everywhere by where it comes from and what loads in one project by project, filtered by CLI, with one row for each fix when no skill is open.
The resolver groups the problems into fixes (`Skills.fixes`, in `skills/fixes.rs`), and says which skill Claude loads instead of one it passes over (`SkillLoad.shadowedBy`).
When each skill was last used and how often is read from Claude Code's transcripts (`skills/usage.rs`, served apart at `/api/skills/usage`), in the list's rows, on a skill's page and on the overview.
Its first writes are switches (`skills/write.rs`, the edits in `skills/edit.rs`, `Skill.controls` saying what each CLI's can set): on a skill's page Claude's `skillOverrides` (On, Name only, Slash menu only, Off) and Codex's `[[skills.config]]` (On, Off); on each plugin's row in the list, `enabledPlugins`; on the overview, **Turn off Codex's built-in** for a name found twice, and the **Tidy up** band under **Not used**, with its Undo.
`POST /api/skills/switch` takes one CLI and skills by id, `POST /api/skills/plugin` a plugin by its `enabledPlugins` key; the server writes the file its scan resolved, the user's settings or a project's `.claude/settings.local.json` for Claude and `~/.codex/config.toml` for Codex.
The fixes it makes (`skills/acts.rs`, `SkillFix.acts` saying which each fix offers) are **Remove N links** for a folder's broken links, **Remove the command** for an older command a skill hides, only while git holds it as it is, **Keep both** (or **Keep as is**) for a fix whose cure the user may decline, and **Compare** for two copies of one skill.
`POST /api/skills/fix` takes a fix by its id and one of its acts, and answers with a token that `POST /api/skills/undo` takes once; `GET /api/skills/compare/:fix` reads the two copies.
A removal looks at each path again right before, and Undo puts a thing back only where nothing is; a kept fix is stored in `skills-kept.json` in nebula's data dir, by the fix's id and a hash of its files' text.
Last, editing a `SKILL.md` (`skills/save.rs`): `SkillDetail.edit` holds the file's text and its version, a hash of its bytes, for a file the page edits, and `SkillDetail.uneditable` says why not for the rest, such as a plugin's cached copy or a CLI's built-in.
`POST /api/skills/check` runs the scan's own checks of a file (`own_file` in `skills/mod.rs`) on a text not yet saved, and `POST /api/skills/save` takes a skill by its id, the version and the new text, writes it at the real file as the switches write theirs, refuses a file not at that version with a 409, and answers with a token for `POST /api/skills/undo`.
`SkillDetail.changes` is what the skill's repository has not committed, for the Repo row.
Then deleting a skill (`skills/delete.rs`, the Trash in `skills/trash.rs`): `SkillDetail.delete` lists what goes, its real folder or command file and each link the scan found to it, what git will show and the installer's lock entry, with a version, a fingerprint of those paths, and `SkillDetail.undeletable` says why not, by the rules that keep the page from editing a file.
`POST /api/skills/delete` takes a skill by its id and that version, refuses with a 409 when anything is not as read, moves each path to the Trash (`NSFileManager` on macOS, the freedesktop.org Trash elsewhere), drops the skill's entry from `~/.agents/.skill-lock.json`, and answers with a token for `POST /api/skills/undo`, which moves each back only where nothing is.
With that, slice 4 is built in full; what it leaves is under the known gaps.

- Read from disk, since no CLI lists its resolved skills as JSON: Claude Code's `~/.claude/skills`, each project's `.claude/skills` and its plugins' `skills/`; Codex's `~/.codex/skills` and `.agents/skills`; Cursor's own folders, and the Claude and Codex ones it also reads.
- For each skill: its name, description and frontmatter, its `SKILL.md` rendered, where it comes from (a symlink's target and the git repository it is in, the skills installer's source, a plugin and its marketplace, a project), which CLIs load it, and whether one has it switched off (Claude's `skillOverrides`, Codex's `skills.config`).
- Problems it points out: a broken symlink, two skills with one name, a frontmatter `name` that does not match its folder, a skill only one CLI can see.
- Read-only first, so the page shows which file is the real one before anything writes through it.
- Then editing, now built: a `SKILL.md` is written at its symlink's target, refused when the file changed since it was read, with that repository's git status shown after, as the switches are already.
  Nothing is committed for the user.

### 5. Linear issues and pull requests in the lanes

The unit of the board becomes a thing to look at: an issue joined to its sessions, its pull request and its reply, keyed by `ABC-123`, or by the session or PR when there is no issue.

Decided:

- `nebula serve` reads Linear and the pull requests itself, through `nebula-forge`, in two cases only: when the page opens with nothing read yet, and on its Refresh button.
  No timer, no daemon change, no protocol change, and the TUI keeps its own reads as they are.
- The issue lanes show only the issues assigned to you.
- Linear only for now; a project whose issues are GitHub's gives none.
- Dropped: polling in the daemon, a broadcast event for it, and merge detection and merge notes in the daemon.
  Slice 7's push needs none of them: it is for a session turning red, which the daemon knows from its status.

Built, as [Web](web.md#issues-and-pull-requests) describes it:

- `POST /api/refresh` starts a refresh, or joins the one under way, and the State carries it (`State.refresh`: whether one runs, when the last landed, and why each part that failed did).
  The refresh lives in `crates/nebula-web/src/refresh.rs`: Linear and every project's `gh` at once, each part that fails keeping what the refresh before read of it.
- Linear: `nebula_forge::linear::assigned`, one paged query (100 a page) for the open issues assigned to you in every team the projects name, with their priority, state and its type, assignee, `branchName`, updated time, team and Linear project.
  The teams come from each project's `issues` setting by `nebula_forge::issues::project_setting`, the rule the TUI's ISSUES MODAL reads it by, a space's over its projects'.
- Pull requests: `nebula_forge::pull_request::checkout_prs`, one `gh api graphql` query per project for every checkout its sessions work in, each branch asked as a variable, open, draft, merged and closed, held to `matches_checkout`'s rules as `lookup` is.
  `Session.prUrl` and `Session.prState` carry it.
- The join (`refresh::joined`): the session's issue URL, else the issue key as a whole word in its branch, else Linear's `branchName`; `Session.issue` carries the issue.
- The lanes (`Board.lanes`): the assigned issues no session works on, `inFlight` (started) and `todo` (unstarted), by priority and then latest changed, each row keyed by the issue's key; an issue opens at `/i/<key>`.

Then Start on an issue in a lane, as [Web](web.md#starting-an-issue) describes it:

- Decided: it asks which project the issue goes to, of every project whose resolved `issues` setting names the issue's Linear team, not one space's alone, since several projects can share a team; the one last used for the issue's Linear project is picked first.
  It hands the issue to that project's captain, not to a worker, and moves the issue to the team's first started state only when it is not started already.
- `POST /api/issues/:key/start` takes `{project, again}` and answers `IssueStarted`; `start.rs` in `crates/nebula-web/src/` checks the project reads the issue's team and that the issue was not handed over already, unless `again`, and lets one Start of an issue run at a time.
  `Project.team` says which team each project reads, `Issue.lastProject` the one to pick, and `Issue.started` what Start did: the project and captain, when, whether the brief is held, and why the move failed.
  `issue-starts.json` in the data dir keeps the last two, written renamed into place.
- The hand-off is `ClientRequest::StartIssue`, a request any socket client may send, behind protocol 60 (`START_ISSUE_PROTOCOL`), answered with `ServerEvent::IssueStarted`; every client is told the held briefs (`ServerEvent::HeldBriefs`), after its Snapshot and on every change, which `Tree::held_briefs` keeps.
  It lives in `crates/nebula-daemon/src/start_issue.rs`: the captain from `Daemon::captain_for`, the gate `orchestrator captain` goes through, and the brief a HELD TURN (`crates/nebula-daemon/src/held.rs`), typed by `Daemon::when_ready`, the wait the orchestrator's done note has, made one for any session in a role.
  Held turns are kept in `held-turns.json` in the data dir, so a restart delivers them, booting each one's captain.
- The TUI's merge notes go through the daemon too, so one place types into a captain: the TUI still decides which go, by its pull-request polling and `merge-notes.json`, and hands each over with `ClientRequest::MergeNote`, behind protocol 61 (`MERGE_NOTE_PROTOCOL`), and the daemon holds it as a held turn beside the briefs.
  A held note is for its project and goes to whichever captain the project has when it goes, waiting while it has none, where a brief stays with the captain Start got or started.
  A captain takes its held turns one at a time, oldest first, each once it has taken the last turn the daemon typed into it, and `write_prompt` locks the session from a turn's text until a pause after its Enter, so no two of the daemon's turns share an input box.
  `when_ready` also waits out a turn whose report line is a `QUESTION` or `BLOCKED` (`nebula_core::why::turn_asks_user`, the board's rule for `needs you`), so nothing is typed in place of the user's answer.
- `nebula spawn --issue <url>` (`SpawnSiblingAgent.issue_url`) gives the worker its issue, and the captain's guidance says to pass it; the brief names the branch by `nebula_core::issue_branch`, the rule the TUI cuts an issue's branch by, moved out of the TUI.
- The move is `nebula_forge::linear::start`: it reads the issue's state and its team's states in one query, and moves it with `issueUpdate` to the `started` state lowest in position, by the issue's key.
  It runs once the daemon has taken the hand-off, and the serve patches the issue's state in what it keeps instead of reading Linear again.

Open question: how far ordering by rule can go on a team whose issues mostly lack priorities, due dates and cycles; the orchestrator's picks (slice 8) carry that judgement.

### 6. The agentos Today rail

- agentos writes a small JSON file each time it rebuilds its Home page: the count of todos due with the top three, the day's plan, projects quiet for 30 days or more, and the state of its nightly job and its kill switch.
- `nebula serve` reads the file from a path in `config.json`, re-reads it when it changes, and fills the state's `today`, which is null today.
- The page shows it as the bottom band of the idle view and as a chip in the top bar when a session is selected, as the merged design draws it.
- Capture from the page, as a call to agentos's `capture`, is a later step: the rail starts read-only.
- Show a count and the top three todos, never the whole overdue list.

Built, as [Web](web.md#today) describes it:

- agentos's `home.ts` writes `today.json` to its state dir (`~/.local/state/agentos`, or `$AGENTOS_STATE`) on every run, renamed into place, and `brain-nightly` records each run's time and problems, or `ok`, in `brain-nightly-last` there for it to carry.
- Decided: agentos's Home has no day plan, so "the day's plan" is today's training, as Home's training section has it and as the merged design's band draws it.
- The file is the contract's `Today` beside `version: 1`, with epoch milliseconds for times, so `nebula serve` serves it as it reads it: `Today` in `contract.ts`, mirrored in `api.rs`, and `State.today` with `State.todayProblem` for a file that is not a day.
- `today_file` in `config.json` names it, from `/` or `~/`, with no default, since the fork knows no toolbox's paths.
- `crates/nebula-web/src/today.rs` keeps it: the hub looks at the setting and the file's size, time and inode on the board's one-second beat and reads the file again when one changed, with no file watcher.
- The page: `Today.tsx`, the band at the foot of the idle view and, on a phone, at the foot of the list, and the chip in the top bar wherever the band is not on screen.
  `useChipFit` measures the bar before it is drawn, on each change of what it holds or of its width, and has the chip give way, the training first and then all but `Today`, and then the session the TUI shows cut short, so the bar keeps to one line on a desktop.
  A link the file gives goes through markdown-it's own check, so a `javascript:` link is text; a web link opens a new tab and any other hands off to its app in place.
- The words are the fork's own, `automation on` for the kill switch and `Todos` for the list, so the page names no toolbox, todo app or notes app.

### 7. The phone

- `tailscale serve` in front of `nebula serve`, with its real HTTPS certificate.
  The Host check then accepts the machine's tailnet name, and the server reads the `Tailscale-User-Login` header.
- Pairing: a one-time link (or QR code) sets a device cookie, and every request needs it as well as the origin check.
- A web app manifest and a service worker, so the page installs to the iPhone home screen.
- Web Push straight from the Mac to Apple's push service, with no relay of our own, when a row turns red.
  The trigger has to live where the board lives without a page open: the daemon, which knows when a session turns red from its status alone, with no Linear or pull request reads.

Step 1, reaching the page, pairing and installing it, is built, as [Web](web.md#from-a-phone) describes it:

- Decided: the server learns the Mac's tailnet name from `tailscale status --json` (`Self.DNSName`), when it starts and again when a request through `tailscale serve` names a host it does not know, at most once every ten seconds, rather than from a setting, so there is nothing to set and a rename needs no restart.
  `crates/nebula-web/src/tailnet.rs` holds the rule for who asks: this machine, a loopback host at the bound port with none of the headers a proxy adds, or a device on the tailnet, the tailnet name as its host over HTTPS with `Tailscale-User-Login`, which `tailscale serve` sets and takes off any request that brings its own.
  A request through the proxy that names the loopback host is refused, since `tailscale serve` passes on the Host a device sends.
- Every request over the tailnet needs a paired device but `/pair`, `/pair/<code>`, and the manifest and its icons, which are the same for every nebula; `lib.rs`'s guard checks the device cookie, and the device's user against the one that paired it, before any route.
  The rest of the page and the API are as on the Mac, but `/api/devices`, which the Mac's page alone may use.
- Pairing (`crates/nebula-web/src/devices.rs`): `POST /api/devices/pair` opens a pairing with an eight-letter code, valid five minutes and once, which ten wrong tries void, a look at `/pair/<code>` counting as a try; `GET /api/devices` lists the devices and the open pairing with its link and QR code (`qrcodegen`, a one-file crate with no dependencies), and `POST /api/devices/:id/remove` removes one.
  `devices.json` in the data dir keeps each device's id, name, user, a SHA-256 hash of its token, and when it was paired and last used, written renamed into place with mode 0600; the last use is written at most once a minute.
  A removed device's open event stream ends, and the page reloads onto the pairing page.
- The pages before pairing are the server's own (`crates/nebula-web/src/pair.rs`), with no script: one form for the code and the device's name, posting to `POST /pair`, which sets the `HttpOnly; Secure; SameSite=Strict` cookie and goes to `/`.
- Installing: `app/public/manifest.webmanifest` (`display: standalone`, mantle as the theme colour, base as the background), icons drawn from the mark at 180, 192 and 512 px, `apple-touch-icon`, and `black-translucent` for the status bar, with the top bar's padding taking the safe area's top and the body its sides and bottom.
  Step 1 had no service worker, as iOS installs a page without one; step 2's push brought one.
- The Mac's page has **Phones** in its top bar (`app/src/Phones.tsx`), only where the page's host is a loopback one.

Step 2, push, is built, as [Web](web.md#notifications) describes it:

- Decided: `nebula serve` owns the subscriptions and the VAPID key, and the daemon learns of them from the files, with no protocol request.
  The page subscribes through the server, which knows which paired device asks, and the daemon reads `push/` and `vapid.json` in the data dir on each beat; the daemon would otherwise need the device list too, to never push to a removed device while it was down when the server removed it, so a request behind a gate of its own bought only a second path to the same files.
  Each subscription is a file of its own, `push/<device>-<hash of its endpoint>.json`, written renamed into place with mode 0600: the server writes and removes them under the lock its devices are kept under, so none outlives its device, and the daemon only ever deletes the one file a 404 or 410 named, so the two never write the same file.
  A server that starts drops a file for a device it does not keep.
- Decided: RustCrypto (`p256` with `ecdh` and `ecdsa`, `aes-gcm`, `hkdf`, and `sha2` 0.11, which `nebula-web` moved to as well) over `ring`.
  `ring`'s ECDH takes only an ephemeral key it makes itself, so RFC 8291 Appendix A's fixed keys, now a unit test, could not be fed to it, and it builds C and assembly where these are pure Rust; nothing is hand-rolled.
- The code is `nebula-forge/src/push.rs`, the outbound HTTP crate, which the daemon now depends on: one record of `aes128gcm`, an ES256 JWT for the endpoint's origin good for 12 hours with `sub` `mailto:push@nebula.invalid`, `TTL: 3600` and `Urgency: high`, sent through `curl` limited to HTTPS with the endpoint and the JWT in a config on stdin and the encrypted body from a 0600 file in the temp dir, removed once sent.
  A subscription is taken only with an `https://` endpoint and keys that read as a P-256 point and 16 bytes.
- The trigger (`nebula-daemon/src/push.rs`) runs on a one-second beat, and does nothing while no subscription is kept.
  A red episode is a session's red state, its status and when it moved there: a session reads red a second into a state, as the board reads it, by `why::why_line` and `report_word`; leaving red, or moving to another red state, starts a new one.
  Episodes wait until `ioreg -r -c IOHIDSystem -d 1` says `HIDIdleTime` is a minute, read only while one waits and no sooner than it could have reached the minute; where it does not read, they go at once.
  `push-sent.json` keeps the state each red session was sent in, so an in-place restart, which dev-watch makes on every change, sends nothing twice.
  The trigger is a plain struct stepped with the clock, the screen reads and the idle time handed in, unit-tested on each of those rules.
- The service worker (`app/public/sw.js`) handles `push` and `notificationclick` alone, with no caching: it is registered only where the page is not the Mac's own, and served before pairing as the manifest is, since it is the same for every nebula.
  A tap focuses the open page and posts it the session's path, `/s/<id>`, which the app opens as any route; with no page open it opens one there.
- On the phone a bell in the top bar opens a panel that turns notifications on from a tap (the permission is asked before anything else is awaited, as iOS asks), off, says they are blocked and how to allow them, or, in Safari on iOS, to add the page to the Home Screen first.
  Each load keeps the server to what the browser holds: a subscription it has is kept again, one it lost or may no longer use is dropped.
- The Mac's phones panel says for each device whether its notifications are on, and **Send a test** for one that has them.

### 8. The orchestrator's Focus picks

- `nebula orchestrator attention --json` prints the top candidates in compact form: key, lane, Linear fields, sessions with their why line, PR state, and recent feedback.
- `nebula orchestrator picks set --json -` takes at most three `{key, reason}` picks; `Focus.source` becomes `"orchestrator"`.
- Triggers: a 07:30 schedule (slice 9), the Re-rank button, and opening the page when the picks are more than three hours old and the candidates changed, at most four a day, only while the orchestrator is at rest with an empty input box.
- Feedback: Not today and Wrong, with an optional reason, ride along in the next `attention --json`; a repeated correction becomes a standing rule in the orchestrator home's `CLAUDE.md` once the user says yes.
- Cost, estimated: about 0.2 to 0.3 million cache-read tokens a run.
- When the orchestrator has not run, or its picks are stale or invalid, the picks fall back to the rule and say so.

Decided, after the first plan, which had the daemon hold the candidates:

- `nebula serve` holds the candidates and the picks, not the daemon.
  Linear and the pull requests are read by the serve alone, on the page's asking (slice 5), so the candidates are the serve's; a daemon that refused keys would need a copy of them.
  The serve holds each pick to its candidates, stamps the picks, keeps them in `focus.json` in the data dir, and the State carries them to every page, with no broadcast of the daemon's.
- `attention` and `picks set` reach the serve's loopback API, found by its port file, as the orchestrator's session alone: the command names it in an `X-Nebula-Agent` header, held to the State's orchestrator as the daemon holds its other commands, and the serve takes neither from a device on the tailnet.
  With no serve running the CLI says so and how to start one.
- `attention` reads Linear and the pull requests again first when the last read is over 10 minutes old, so a morning run never ranks stale data.
- The rank goes through the daemon's held turns: `RankFocus`, behind protocol 62, the first request that holds a turn for the orchestrator, as `StartIssue` and `MergeNote` hold a captain's.
  The daemon writes the turn's text itself, the fixed RANK TURN beside the orchestrator's guidance, so the request carries none: the daemon stays the one place that types into a session, a client can type nothing but that, and slice 9's schedule in the daemon holds the same turn by calling `Daemon::rank_focus` with no serve in the loop.
  `ServerEvent::RankTurn` tells every client when a rank was held and last typed, which the page reads held, ranking and ended by.
- The page's own ask, on opening, is sent without `wait`: the daemon refuses it unless the orchestrator is ready for a turn that moment, so an automatic rank never waits to land in the middle of a conversation; Re-rank waits.
- A candidate is keyed by the issue joined to it, else its session's id, and the issues in the lanes are candidates too, so the orchestrator weighs the work no session has started.
- Stale is over three hours old with the candidates changed since, a key come or gone or moved to another lane; the same test makes a rank due on opening, with no picks ever also due.
- Not today keeps a key off the candidates until the day ends on the Mac's clock, so it can be picked neither by the orchestrator nor shown; Wrong puts the pick aside and leaves the key a candidate.
- The rule stands in, saying why, when the orchestrator has not ranked, its picks are stale, none is a candidate now, the user put each aside, or Linear has not been read yet since the serve started, as the picks are keyed by its issues.
- The open question: with the orchestrator as the source, the page never says nothing needs you while a row does.
  Under its picks, **Also needs you** lists the rows that need you that it left out, under **Needs you** when it picked nothing, and the list keeps them in its own lane as it does every row the focus leaves out.

Built, as [Web](web.md#focus) and [Orchestrator](orchestrator.md#ranking-the-focus-picks) describe it:

- `crates/nebula-web/src/focus.rs`: the candidates (`candidates`), what is shown (`shown`), when a rank is due (`due`), where one stands (`rank`), the attention it prints, the picks it takes (`validate`), what it keeps (`Kept`), the routes `/api/attention`, `/api/focus/picks`, `/api/focus/rank` and `/api/focus/feedback`, and the CLI's two commands, over `nebula_client::serve::call`, a plain HTTP/1.0 request.
- `Focus` in the contract gains `fallback` and `rank`, and a `FocusPick` its `key` and a `sessionId` that is null for an issue no session works on.
- The daemon: `About::Rank` among the held turns (`nebula-daemon/src/held.rs`), typed into the orchestrator by `when_ready` as a done note is, and the orchestrator's guidance (`CLAUDE_ORCHESTRATOR_GUIDANCE`) says how to rank, to pass the picks in a quoted heredoc, and to propose a standing rule for its `CLAUDE.md` only on a repeated correction, written once the user says yes.
- The hub publishes the State again when time alone moves the focus: picks turning three hours old, a new day, a rank asked for going unheard of.
- The page: Re-rank and a line on where a rank stands or why the rule stands in, under Focus in the list; an issue's card; Not today and Wrong on each card and beside `Why focus` on a session's or an issue's page, each asking for a reason on a row of its own; and **Also needs you**.
- Measured on one real Claude orchestrator (Opus 5.5 at xhigh effort, Claude Code 2.1.325), three candidates: a rank is three API calls, `attention`, `picks set` and a one-line reply.
  The first rank in a new orchestrator read 162,000 tokens from the cache, wrote 38,000 to it and wrote 640 out: its context was about 67,000 tokens, Claude Code's own system prompt and tools with the guidance.
  The next, a minute later with a Wrong said in between, read 203,000 from the cache, wrote 1,000 to it and 433 out, and left the pick marked wrong out, as the reason said.
  So a rank costs about three times the orchestrator's context in cache reads: within the estimate's 0.2 to 0.3 million while its conversation is short, and more as it grows, since each rank is a turn in that conversation.

### 9. Schedules in the daemon

- A schedule is a prompt in a project, with its harness, model, effort and permission mode, and where each run goes: a new session (in a new worktree or the root checkout), the same session, or the same session with fresh context.
- Forms: every N minutes, a time of day with weekdays, and once; no cron syntax at first.
- A loop in the daemon compares a due time with the wall clock on each tick, so laptop sleep cannot stall it, and keeps every run with its outcome and why a run was skipped.
- Missed runs: a time-of-day run more than ten minutes late is skipped, with one catch-up run and a note for a daily schedule; a late interval run fires once.
- `nebula schedule add | list | rm | run` for agents and scripts, and a Schedules tab on the page.
- A login item so the daemon starts at login; until it is on, the first TUI starts the daemon, so schedules stop after a reboot until a TUI is opened.
- An unattended run that stops on a permission dialog turns its row red like any other session.
- Store it apart from the daemon's migrations, or number it with care: migration numbers are positional and collide with upstream's, as migration 29 did.

Built, as [Web](web.md#schedules) and [Commands](commands.md#schedules) describe it:

- `nebula-core/src/schedule.rs` holds the shapes, `nebula-daemon/src/schedule.rs` the book, the beat and the requests, `schedule/when.rs` the timing and the rules for a late run, and `schedule/fire.rs` a run.
  `AddSchedule`, `RemoveSchedule` and `RunSchedule` are requests any socket client may send, behind protocol 63 (`SCHEDULE_PROTOCOL`), answered with `ScheduleAdded`, an `Ack` and `ScheduleRan`; every client is told the schedules and the run log after its Snapshot and on every change (`ServerEvent::Schedules`), which the `Tree` holds as `schedules`.
- Decided, storage: `schedules.json` in the data dir, written renamed into place as `held-turns.json` is, holding the schedules, the run log (each schedule's newest 50) and the permission mode of each session a schedule started.
  No SQL migration, so nothing can collide with upstream's numbering; a file that does not read is a book with nothing in it, said once in the log.
- Decided, the forms: every N minutes (the first N minutes after it was added), a time of day on a set of weekdays, and once at an instant.
  Time of day is the Mac's clock through `localtime_r`'s offset, so a daylight saving change moves it with the clock: a time the clock skips runs as late as it skipped, one it shows twice runs once, the first time.
  The tests hand the timing a zone of their own, Central European time across both 2026 changes, and a clock of their own.
- Decided, missed runs: ten minutes late or less is on time; a time-of-day run later than that is skipped with why, and "a daily schedule" is one that runs on every day of the week, which runs once late on the day it was due, with a note, a run missed on an earlier day skipped; a once run that late is skipped; an interval run past its period runs once for all it missed, and the next is a period after it.
  So a weekday schedule, the morning rank among them, is skipped when the Mac sleeps through 07:30, and the page's own rank on opening stands in.
- Decided, where a run goes: a new session in a worktree cut for the run, named for the schedule and when it ran (`nightly-triage-1116-0730`), or in the root checkout; or one session, which the first run starts in the root checkout and every later run reaches as a HELD TURN (`About::Schedule`), typed by `when_ready` once it is at rest with an empty input box and asks the user nothing, a run while one is held skipped rather than held beside it, a reaped session booted first, and an archived or deleted one replaced on the next run, which says so.
- Decided, fresh context: the daemon starts the session's CLI again on a new conversation, the prompt its first (`Daemon::restart_fresh`), once it is at rest, rather than typing a harness's own command such as Claude's `/clear`, which nebula cannot see take and each harness spells differently.
  So it is the same for every harness that reports its status; one that does not (Muse, Grok Build, a custom harness with no hooks) is refused both same-session targets, as its session never reads as at rest, and one that resumes no conversation is refused the one that keeps its context, as a session the idle reaper took would come back without it.
- Decided, permission mode: Claude's `--permission-mode`, `manual`, `acceptEdits`, `auto`, `plan`, `dontAsk` or `bypassPermissions`, put on every start of the session's CLI, a resume included; none is the mode the user's Claude settings give, and a mode for another harness is refused, as it launches as nebula launches it.
  `manual` is in the list because the real run showed a default of `auto` from the user's settings: a schedule that must ask needs to say so.
- Decided, the rank: a schedule's task is a prompt or the rank, and a rank calls `Daemon::rank_focus` with `wait`, as Re-rank does, after `nebula serve` is up: started, when none answers, by `nebula_client::serve::serve_url`, the code `Shift+W` now shares, through the user's `$SHELL -l -i -c`, so the serve has the `LINEAR_API_KEY` and the `gh` a terminal has even in a daemon the login item started with launchd's bare environment.
  The run says held, then typed, as `schedule_turn_went` marks it from the held turn.
- Decided, the login item: `nebula login-item on | off | status` writes or removes `~/Library/LaunchAgents/dev.nebula.daemon.plist`, which runs the installed binary's `daemon` through the user's login shell (`$SHELL -l -c`), with `SHELL` set, `RunAtLoad` and no `KeepAlive`, so a `nebula kill` or an idle `nebula upgrade` stays a stop, and no `NEBULA_*` variable, so it is the installed instance's daemon on its default dirs.
  It never runs `launchctl`: launchd loads the file at the next login, and the daemon running now is left alone, as a `bootout` would stop it and every session with it.
  A cargo build, and an instance on a `NEBULA_DATA_DIR`, `NEBULA_RUNTIME_DIR` or `NEBULA_DEV_WATCH` of its own, is refused, so the dev build can never claim it; one label means one item, and `status` says whose binary it starts.
- Decided, the phone: the Schedules tab is the phone's too, reading and acting, as a paired device can already type into every session and answer its dialogs, so adding a schedule gives it nothing it lacks, and checking last night's runs from a phone is the point.
  On a phone the top bar's first line is the mark, the three sections and the bell; Follow TUI moved to the end of the line that says what the TUI shows, as the four no longer fit one line at 390 px.
- The page: a list of the schedules with their last run's dot, red while that run's session waits on the user, the one-click **Add the morning rank** while none is scheduled, a schedule's page with **Run now**, **Remove** asked on its row and every run, the add form, and the recent runs of every schedule, with a note while the login item is off (`Schedules.loginItem`).
  `Schedules` in the contract, mirrored in `api.rs`, and the mock's scene of a triage run red on a dialog, a skip, a catch-up, a held digest and a failed run (`app/src/mock/schedules.ts`).
- Seen end to end: stand-ins through the CLI and the page under isolated daemons with a test-only clock skew (`NEBULA_SCHEDULE_SKEW_FILE`), each run concurrently five times over; and one real Claude schedule on Haiku in an isolated daemon with launchd's bare `PATH`, whose first run waited red on Claude's question whether to trust the folder, whose later runs were typed into its session unattended, `--permission-mode=acceptEdits` put on its resume too, a once run in a new session that ran with no dialog, and a `manual` one that waited red on its Bash permission.

### 10. The Deck

A page of its own at `/deck` for a phone on its side on a stand beside the keyboard, as [Web](web.md#deck) describes it, from the design in which the phone comes first and a Stream Deck + only if the phone falls short.

Built, with no daemon, protocol or API change:

- `app/src/DeckPage.tsx` is the page, chosen in `main.tsx` by its path ahead of the app's routes, and `app/src/deck.ts` its logic: the tiles (`deckIds`), the grid a screen gets (`deckGrid`), how they hold still (`holdOrder`), a touch on the tiles that is a tap, a hold or a swipe and never two (`press`), and what the bar offers (`offered`), each tested in `deck.test.ts`.
- Decided, the grid: eight tiles on a phone and 12 to 16 on a screen whose short side is 600 px or more, of the columns and rows whose tiles come closest to half again as wide as they are tall, the shape of the phone's on its side, read from the window's size alone so the bar never moves a tile.
  The dots go at the end of the top row on a screen on its side and under the tiles upright, so the grid keeps the width of the top row and the bar.
- Decided, the order: the board's rows section by section, then the sessions the board leaves off in `State.sessions`' order, which the contract now says is the palette's attention order.
  The palette's order alone put a finish that reports a `QUESTION` among the finishes, after every running session, so a red tile could land on the second page; the board puts every red row first.
- It acts through the calls the detail pane makes: `/show` on a tap, `/seen` when the reader opens on a finish nobody has read, and the bar's answers through `pickFor` and `api.pick`, which the detail pane's keys and pills now share, so a dialog's option goes to `/answer` and a `NEXT:` answer to `/send`.
- Decided, the hold: it opens the reader, the detail pane's `ReplyPanel` and `Composer` with the bar's answers, over the tiles, rather than marking the session seen alone, which reading it does anyway; tapping the bar's name opens it too.
  A hold acts as the finger lifts (`press` arms at half a second, and the tile lights up): opened while the finger still pressed, the reader's text was under it when Chrome's own long press landed, and that menu was no longer the tile's to prevent.
- **Deck** in the page's nav is a plain link, since the Deck is a page outside the app's routes, and nebula's mark at the start of the Deck's top row leads back, so neither Home Screen app is a dead end; at 390 px the four sections and the bell still keep to one line, the sections' padding 6 px.
  On the Mac's own page at 390 px, **Phones** with its label had dropped onto a line of its own, so on a phone it is its icon alone, the bell's 32 px square, as the mark is, and Disconnected takes a line of its own under the first rather than push either off it.
- Past four pages, the dots are `‹ 1/7 ›`: seven dots crowded the day's pills to an ellipsis on a phone with 50 sessions.
- Decided, the swipe: one `press` for the whole grid, not one a tile, so a swipe may start on a tile, an empty place or the gap between two, and the tile a touch went down on is the one it taps or holds.
  Every pointer goes through it, so a mouse dragged across the tiles turns the page too, and a mouse press that strays past the slop or leaves the tiles is nothing, where it was one that left its tile.
  A finger may stray 10 px (`SLOP_PX`) and still tap or hold, between Android's touch slop of 8 dp and the 15 px Chrome allows on a desktop; past that before the hold arms, more across than up or down, it is a swipe in the making, and let go at least 40 px (`SWIPE_PX`) to one side, still more across than up or down, it turns the page; past it any other way, or once held, it is nothing.
  The grid is `touch-action: pan-y pinch-zoom`, where the rest of the Deck stays `manipulation`: under it Chrome cancelled every sideways move past its slop as a pan of its own, so no swipe could reach the page, and now only an up or down move or a pinch is the browser's, cancelled as before, which also keeps a hold that drifts up or down within the browser's own slop a hold.
  A swipe stops at the first and last page, while the arrows wrap, as a page that wraps under a finger reads as nothing happening; it is a touch, so the page it turns to holds still for `STILL_MS`.
  Each turn, by swipe, dot or arrow, brings the new page in from 24 px to its side and 30% opacity over 180 ms, and none with reduced motion; the grid is keyed by the turn so each one plays afresh, and opening the reader clears it, so the tiles come back from the reader still.
  The OS's and browser's own swipe back from the screen's edge are left alone, as a page cannot take them: Safari's on iOS outside a Home Screen app, and Android's gesture navigation, take a swipe that starts at the edge before the page sees it, and the page then gets a `pointercancel` and does nothing.
- Seen in Chrome under Android phone and tablet emulation, on its side and upright, with 40 sessions (`NEBULA_MOCK_SESSIONS=40 pnpm dev`): a finger's touches sent through `Input.dispatchTouchEvent`, which Chrome's gesture pipeline handles as a finger's, cancelling a pan by `touch-action`, and taps and holds through `Input.synthesizeTapGesture`.
  A swipe left and right turns the page and back, from a tile or the gap between two, and Chrome's own `Input.synthesizeScrollGesture` sideways does too; none calls `/show` or `/seen`, opens a menu or brings a `pointercancel`, and a swipe right on the first page stays there.
  A drag down is nothing of the page's, the browser cancelling it; a tap calls `/show` alone; a hold opens the reader on release with Chrome's `contextmenu` prevented and nothing selected; a hold that drifts 6 px down or 7 px sideways still opens it; one held that then moves away lights the tile and puts the light out, turning no page and opening nothing; and a swipe 1.5 s after a tap, once a session from the last page had started running, turned to the second page as it stood at the tap, held it 2.5 s on, past the tap's own three seconds, and settled 3.7 s on.
  By mouse at 1440 × 1000 and 1024 × 768, a drag either way turns the page, a click calls `/show` alone, a long press opens the reader, closing it plays no turn, and a press dragged off the tiles is nothing.
  A touch sent with `Input.dispatchTouchEvent` sometimes leaves the next `Input.synthesizeScrollGesture` with a stale touch action in Chrome's emulation, so that Chrome takes it as a pan, on a bare page with one `pan-y` box as on the Deck: run the synthetic gestures first.
- The screen stays on through the Wake Lock API, asked for again on `visibilitychange`.
- Decided, the Home Screen: a second manifest, `public/deck.webmanifest`, with `id` and `start_url` `/deck`, which a line in `index.html` names in place of the app's on `/deck`.
  iOS has opened a Home Screen app at its manifest's `start_url` since 11.3, not at the page it was added from, so the app's own manifest would open the Deck phone on the board; a remembered last page would hold only once the app had been opened on the Deck, and would follow a phone that is not on the stand.
  The second manifest is one of the install files an unpaired device is served (`INSTALL_FILES` in `pair.rs`).
  Chrome on Android installs by the same manifest: under its Android emulation, Chrome's own reading of the page on `/deck` (`Page.getAppManifest`) is `deck.webmanifest`, the app id `/deck`, its start `/deck`, with no installability errors.
- Seen in Chrome under Android tablet emulation, with puppeteer's long-press through Chrome's gesture pipeline (`Input.synthesizeTapGesture`): on plain text the same press opens the context menu and selects a word; on a tile Chrome's `contextmenu` arrives already prevented, nothing is selected, no `pointercancel` comes, and only `/seen` is called; a short tap calls only `/show`.
- Seen end to end against an isolated dev instance, its sessions played by a stand-in agent through Claude's hooks: a tap, a hold that marks a finish seen and taps nothing, the tiles settling three seconds after a touch, a permission dialog answered from the bar, the second page, a disconnect and the way back, at 844 × 390, 390 × 844, 1280 × 800 and 800 × 1280.

Not built: a way to capture a todo from the Deck, and anything that presses the kill switch; serve has no call for `agentos off`, and should not get one for this.
A do-not-disturb or presence state is still being weighed.

### 11. Images in a follow-up

A follow-up from the detail pane or the Deck's reader carries images, pasted into its box or picked with **Image**, as [Web](web.md#images-in-a-follow-up) describes it.

Built, behind protocol gate 64:

- `POST /api/images` takes an image's bytes as the whole body and answers its id; `POST /api/sessions/:id/send` takes `images`, those ids, beside `text`.
  The route is behind `lib.rs`'s guard like every other, so a device not paired gets the pairing page's 401 and nothing is kept, and the Mac's own page uses it as it is.
  `crates/nebula-web/src/images.rs` keeps the file, and a send resolves only an id it gave, `web-` and 32 hex digits and an extension, to a file in its folder, so no other path on the Mac can be named.
- Decided, how Claude Code gets the image: the follow-up's text with each image's path on a line of its own under it.
  A line of a paste that is an image's path alone Claude Code takes for the image, as it takes a file dropped on its box: its paste handler reads the file itself, at the person's own permission, which only a deny rule refuses, and the model gets an image block, with the prompt reporting `[Image #1]` where the line was (2.1.327, seen in its bundle and in a live session).
  So there is no `Read` tool call, and no permission question, wherever the file lives; `@<path>` was tried and dropped, as the Enter after it picked a file from Claude's mention list and left the prompt unsent.
  The daemon types any turn that is an image's path alone as a bracketed paste too (`nebula_core::turn::is_image_path`), since only a paste makes a path an image, and its receipt counts a prompt that holds the rest of the text, the image lines aside, as taken whole: before it, the same send said the prompt was cut short.
  Gate 64, `IMAGE_TURN_PROTOCOL`, is that receipt: the server sends no image through an older daemon, which would say so and invite the images again.
- Seen for Codex 0.163: the paste is text, and the model opens the path with its `view_image` tool, which asks for no approval.
  Cursor, Pi and OpenCode get the path as text, which they read with their own tools, as their permission modes allow; none of them has been tried.
- Decided, where: the data dir's `attachments/`, the TUI's `^V` and dropped-file folder, now `nebula_core::attachments`, outside every checkout, so no image is ever committed, and since Claude reads a pasted path without its file tools, a folder outside the session's checkout costs no permission question.
  Each file is `web-<128 random bits in hex>.<ext>`, written renamed into place with mode 0600 by the same `write_private` the push files use.
- Decided, cleanup: a file there older than a week goes when `nebula serve` starts and whenever it keeps another, the TUI's rule for the same folder, now one function both call.
  A prompt is sent within minutes, and Claude's transcript holds the image itself, so the file matters only to a session that reads it again; a week covers one resumed days later.
- Decided, the limit: 20 MiB, set on the route with `DefaultBodyLimit` and checked by the page before it uploads.
  A phone's photo is 2 to 8 MB as a JPEG and up to about 15 MB at 48 megapixels, a screenshot a few MB as a PNG, and Claude Code scales what it reads down to what the model takes, so 20 MiB holds every photo a phone hands over while bounding what one request holds in memory; axum's own 2 MB default would have refused most photos.
- Decided, the types: PNG, JPEG, GIF and WebP, the four Claude Code reads a pasted path as, told by their magic bytes and never by the Content-Type, and HEIC or HEIF by its `ftyp` brand.
- Decided, HEIC: kept as the JPEG `/usr/bin/sips -s format jpeg` makes of it on macOS, refused elsewhere.
  iOS Safari's photo picker for `<input type="file" accept="image/*">` converts a HEIC photo to a JPEG under its default **Automatic** format, by its reported behaviour; with **Current** chosen in the picker's options, or a file picked from Files, the HEIC comes as it is, and so it can from any other browser.
  A page cannot draw a HEIC in Chrome, so its thumbnail is its extension, `HEIC`, in place of the picture.
- The page (`app/src/Attach.tsx`): each image uploads the moment it is added, over an `XMLHttpRequest` for its progress, as `fetch` reports none for an upload; a thumbnail with a bar, a red one that says why it failed, ✕ to remove one, which also stops its upload, and **Send** held until every image is up.
  A paste takes the clipboard's image files and leaves its text, and **Image** is a hidden `<input type="file" accept="image/*" multiple>`, which on iOS offers the photo library, the camera and Files.
- Seen end to end against an isolated daemon and serve, with a real Claude Code 2.1.327 session on Haiku at low effort, from desktop Chrome as an emulated iPhone, paired through a stand-in for `tailscale serve`: an HTTPS proxy on 127.0.0.1 with its own certificate, setting `X-Forwarded-*` and `Tailscale-User-Login` and taking off any the browser sent, with Chrome resolving the tailnet name to it.
  An upload before pairing was refused with 401 and kept nothing; once paired, a 4.9 MB, 12-megapixel JPEG uploaded over emulated Fast 4G with its bar moving, was sent with a question, and Claude described it rightly, and a HEIC photo came back a JPEG and was described rightly too.
  The transcript holds an image block for each, and no tool call in the whole session.
- While a session runs, its last reply's `NEXT:` line, which no pills stand for then, is a line of its own after the report, where it had run on into the report line's paragraph.

Not built: images in the orchestrator's **Ask** field, and dropping a file onto the box.

### Later

- The Deck on a Stream Deck +, maybe, if the phone falls short: the same layout, its eight keys the session tiles (press to show, hold to mark seen), and its four dials Session (turn through every session, push to show), Answer (turn through the options, push to pick or send), Today (turn through the day, push to open it on the Mac) and agentos, whose push opens the nightly log and whose 2 s hold is `agentos off`, or on again.
  It needs `nebula session show`, `seen` and `answer` in the CLI beside `send`, so a plugin on 127.0.0.1 can act without faking the page's origin to get past serve's write guard.
  The plugin would live in this repo, in TypeScript on Elgato's SDK, read `/api/events` with the types in `contract.ts`, start `nebula serve --no-open` when none runs, and draw its keys and strip as SVG in the TUI's status colours.

- Roles: launch presets that carry instructions, permission mode and tools, compiled to each CLI's flags (Claude `--agent` or `--append-system-prompt`, a Codex profile, a Cursor rule file).
- A Tauri window around the page, only if a global shortcut or `nebula://` links prove necessary.

## Known gaps

- Open in terminal raising the right WezTerm window and tab with several windows open has not been checked by hand.
- A long dialog option loses its wrapped tail: Claude's "2. Yes, and always allow access to <path>" reads as "Yes, and always allow access to".
- Codex dialogs are only partly checked: arrows move the selection, digits behaved unpredictably.
- A reply's time (`Reply.at`) is always null, since the report read carries no timestamp.
- The Screen tab polls once a second while it is open.
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
- Protocols 59 to 64 are fork gates above upstream's numbers, renumbered on every rebase.
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
- The Today band is read-only: capture from the page is still to come.
- Whether Obsidian opens agentos's `obsidian://open?path=` links to a sheet and a project note has not been clicked by hand.
- `Quiet for 30 days or more` is the page's own label, while the 30 days are agentos's: a writer that lists projects quiet for another span is labelled wrongly.
- A todo due at a time of day is counted late or due by its date alone.
- The day file is looked at only while `nebula serve` is connected to a daemon, since the page has no state without one.
- Until the agentos change is on its `main` and the nightly job has run once, `today.json` has no nightly run, and the band shows none.
- Pairing has been run end to end only in desktop Chrome, as an emulated iPhone with its safe areas, through a stand-in for `tailscale serve` that sets its headers as its source does; a real iPhone through real `tailscale serve` has not been tried.
  That Safari keeps the Secure, SameSite=Strict cookie, that iOS copies it into the Home Screen app when the page is added after pairing, and how `black-translucent` draws the status bar there are read from WebKit's and others' notes, not seen.
- The pairing link names the tailnet name with no port, so it is right only for `tailscale serve` on port 443; on another port, type the code.
- The tailnet name is `Self.DNSName` alone, not every name `CertDomains` holds.
- A device's cookie lasts 400 days from pairing and is not renewed by use; after that it is paired again.
- Ten wrong codes void a pairing, but nothing counts across pairings: a device on the tailnet gets ten tries at each code the Mac opens.
- On a phone, Enter in a follow-up sends it, and a phone's keyboard has no Shift+Enter, so a follow-up typed there is one line.
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
- A Re-rank pressed while you talk to the orchestrator is held and lands once it is at rest with its box empty, as a turn of nebula's between yours.
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
- The API contract between `nebula serve` and the page is `crates/nebula-web/app/src/api/contract.ts`; the Rust side in `crates/nebula-web/src/api.rs` mirrors it, changed in the same commit.
- The gate is `make ci` plus `pnpm typecheck && pnpm test && pnpm build` in `crates/nebula-web/app`.
- Look at the page in a real browser at 1440x1000 and 390x844 before landing, with `chrome-devtools-axi` and a session name of its own.
  `pnpm dev` serves the page against the mock in `app/src/mock/`, which must implement every endpoint.
- After landing on `main`: `make web` in the main checkout, wait for dev-watch to rebuild (a dev-watch started before it watched the app's build needs a `touch crates/nebula-web/build.rs`), then restart the `nebula serve` on port 7682.
