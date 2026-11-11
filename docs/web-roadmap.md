# Web UI roadmap

<sub>[← Web](web.md) · [README](../README.md) · [How it works](how-it-works.md)</sub>

What comes next for `nebula serve` and its page, in the order to build it, and how to carry on the work.
Slices 1 to 3 are on `main`: the shared client code, the read-only page, and acting from it (send, answer, seen, Open in terminal, Follow TUI, `Shift+W`), with Focus by rule.
Slice 4's Skills page is built in full: when each skill was last used, a switch for each CLI, the fixes it can make, each with an Undo, and editing a `SKILL.md`.
The plan is to use those for about a week before slice 5, Linear.

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
| 5 | Linear issues and pull requests in the lanes | A rule for which project a Linear issue belongs to |
| 6 | The agentos Today rail | A `--json` output from agentos's `home.ts` |
| 7 | The phone: `tailscale serve`, pairing, PWA, push | Slice 5's daemon-side polling, for push |
| 8 | The orchestrator's Focus picks | Slice 5's joined candidates; slice 9 for the morning run |
| 9 | Schedules in the daemon | A login item, so the daemon is up after a reboot |

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
With that, slice 4 is built in full; what it leaves is under the known gaps.

- Read from disk, since no CLI lists its resolved skills as JSON: Claude Code's `~/.claude/skills`, each project's `.claude/skills` and its plugins' `skills/`; Codex's `~/.codex/skills` and `.agents/skills`; Cursor's own folders, and the Claude and Codex ones it also reads.
- For each skill: its name, description and frontmatter, its `SKILL.md` rendered, where it comes from (a symlink's target and the git repository it is in, the skills installer's source, a plugin and its marketplace, a project), which CLIs load it, and whether one has it switched off (Claude's `skillOverrides`, Codex's `skills.config`).
- Problems it points out: a broken symlink, two skills with one name, a frontmatter `name` that does not match its folder, a skill only one CLI can see.
- Read-only first, so the page shows which file is the real one before anything writes through it.
- Then editing, now built: a `SKILL.md` is written at its symlink's target, refused when the file changed since it was read, with that repository's git status shown after, as the switches are already.
  Nothing is committed for the user.

### 5. Linear issues and pull requests in the lanes

The unit of the board becomes a thing to look at: an issue joined to its sessions, its pull request and its reply, keyed by `ABC-123`, or by the session or PR when there is no issue.

- Move the Linear and `gh` data layer out of `nebula-tui` (`linear.rs`, the data half of `issues.rs`, `pull_request.rs`'s lookup) into shared code, and poll from one place.
  The daemon is the better home: merge detection, merge notes and the PR line under a board row then work with no TUI open, and slice 7's push needs it.
- Fetch the fields the lanes need: priority, state and its type, assignee, `branchName`, updated time.
  Today `linear.rs` lists issues without them.
- Join an issue to a session by the session's `issue_url`, then by the worktree branch matching the issue's `branchName`.
- New lanes for issues with no session: "In flight, no session" (started in Linear, nothing running) and "Todo".
- Actions on an issue: Start via the project's captain, Open in Linear.
  Starting moves the issue to the team's first started state.
- Fill `prUrl` in the state; it is always null today.
- Open question: several projects can share one Linear team (`linear:<TEAM>`), so an issue needs a rule for its project, from its Linear project or a label, or the captain chooses.
- Open question: how far ordering by rule can go on a team whose issues mostly lack priorities, due dates and cycles; the orchestrator's picks (slice 8) carry that judgement.

### 6. The agentos Today rail

- agentos writes a small JSON file each time it rebuilds its Home page: the count of todos due with the top three, the day's plan, projects quiet for 30 days or more, and the state of its nightly job and its kill switch.
- `nebula serve` reads the file from a path in `config.json`, re-reads it when it changes, and fills the state's `today`, which is null today.
- The page shows it as the bottom band of the idle view and as a chip in the top bar when a session is selected, as the merged design draws it.
- Capture from the page, as a call to agentos's `capture`, is a later step: the rail starts read-only.
- Show a count and the top three todos, never the whole overdue list.

### 7. The phone

- `tailscale serve` in front of `nebula serve`, with its real HTTPS certificate.
  The Host check then accepts the machine's tailnet name, and the server reads the `Tailscale-User-Login` header.
- Pairing: a one-time link (or QR code) sets a device cookie, and every request needs it as well as the origin check.
- A web app manifest and a service worker, so the page installs to the iPhone home screen.
- Web Push straight from the Mac to Apple's push service, with no relay of our own, when a row turns red.
  The trigger has to live where the board lives without a page open, which slice 5 moves into the daemon.

### 8. The orchestrator's Focus picks

- `nebula orchestrator attention --json` prints the top candidates in compact form: key, lane, Linear fields, sessions with their why line, PR state, and recent feedback.
- `nebula orchestrator picks set --json -` takes at most three `{key, reason}` picks.
  The daemon refuses a key that is not a current candidate, stamps the picks with the time, and broadcasts them; `Focus.source` becomes `"orchestrator"`.
- Triggers: a 07:30 schedule (slice 9), the Re-rank button, and opening the page when the picks are more than three hours old and the candidates changed, at most four a day, only while the orchestrator is at rest with an empty input box.
- Feedback: Not today and Wrong, with an optional reason, ride along in the next `attention --json`; a repeated correction becomes a standing rule in the orchestrator home's `CLAUDE.md` once the user says yes.
- Cost, estimated: about 0.2 to 0.3 million cache-read tokens a run.
- When the orchestrator has not run, the picks fall back to the rule and say so.
- Open question: with the orchestrator as the source, "Nothing needs you." must not show while needs-you rows sit in the lanes.

### 9. Schedules in the daemon

- A schedule is a prompt in a project, with its harness, model, effort and permission mode, and where each run goes: a new session (in a new worktree or the root checkout), the same session, or the same session with fresh context.
- Forms: every N minutes, a time of day with weekdays, and once; no cron syntax at first.
- A loop in the daemon compares a due time with the wall clock on each tick, so laptop sleep cannot stall it, and keeps every run with its outcome and why a run was skipped.
- Missed runs: a time-of-day run more than ten minutes late is skipped, with one catch-up run and a note for a daily schedule; a late interval run fires once.
- `nebula schedule add | list | rm | run` for agents and scripts, and a Schedules tab on the page.
- A login item so the daemon starts at login; today the first TUI starts it, so schedules would stop after a reboot.
- An unattended run that stops on a permission dialog turns its row red like any other session.
- Store it apart from the daemon's migrations, or number it with care: migration numbers are positional and collide with upstream's, as migration 29 did.

### Later

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
- A fix's or a save's Undo lives in the `nebula serve` that made it, so it is gone once that server stops; the files are then put back by hand, or by `git restore` for a command or a `SKILL.md` git holds.
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
- Protocol 59 is a fork gate above upstream's numbers, renumbered on every rebase.

## How to carry on the work

- Work in a worktree, not the main checkout: dev-watch rebuilds and reloads the dev daemon on every saved change there.
  Land each finished step on `main` as one commit and fast-forward.
- The API contract between `nebula serve` and the page is `crates/nebula-web/app/src/api/contract.ts`; the Rust side in `crates/nebula-web/src/api.rs` mirrors it, changed in the same commit.
- The gate is `make ci` plus `pnpm typecheck && pnpm test && pnpm build` in `crates/nebula-web/app`.
- Look at the page in a real browser at 1440x1000 and 390x844 before landing, with `chrome-devtools-axi` and a session name of its own.
  `pnpm dev` serves the page against the mock in `app/src/mock/`, which must implement every endpoint.
- After landing on `main`: `make web` in the main checkout, wait for dev-watch to rebuild (a dev-watch started before it watched the app's build needs a `touch crates/nebula-web/build.rs`), then restart the `nebula serve` on port 7682.
