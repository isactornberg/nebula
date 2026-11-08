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
  The header row is `‹ sessions /` followed by a strip of every live session on the machine, the orchestrator first and the rest in the attention order `.` and `,` walk: needs feedback, running, unseen finishes, then the rest by recency.
  The session on screen is the lit one, and what the row has no room for is counted at its end.
- **Showing a session.**
  A click on a session in the strip brings it up full-screen in place of the chat, still inside the view.
  So does `F9` (`Shift+F9` going back), which walks the strip from the session on screen from anywhere, a session included, and the orchestrator's own `nebula orchestrator review <session-id>`, when the user asks to see something.
  `^q` (or `^F`, or `` ^` ``) goes back one step: from that session to the orchestrator, and from the orchestrator out to the grid.
- **Leaving.**
  `F10` from anywhere in the view, or a click on `‹ sessions`, goes back to the grid.
  The view never moves the grid's cursor, so the grid is where it was.
- **Lasting.**
  A dev-watch relaunch brings the view back on the session it showed, as it brings back any session left full-screen.
  A closed window does not yet: nothing saves the layout when the TUI is killed rather than quit.

The view is a full-screen session underneath, so scrollback, selection, links and the input lock behave exactly as they do in any full-screen session.
While it is up, nothing else swaps the session on screen: a row leaving the grid under its cursor moves the cursor, not the view.

## Agent-facing commands

The orchestrator receives extra guidance in its system prompt.
These commands use the same CLI-over-socket pattern as `nebula spawn`, `nebula worktree` and `nebula open`, but they are daemon-gated to the persisted orchestrator session:

```sh
nebula orchestrator list [--json]
nebula orchestrator read <session-id> [--bytes N]
nebula orchestrator send <session-id> <prompt>
nebula orchestrator spawn --project <project-id-or-name> [--worktree <branch-or-id>] <prompt>
nebula orchestrator review <session-id>
```

The text format is compact and model-friendly.
`--json` prints stable IDs and status fields so the orchestrator can call a later command without fuzzy matching a label.
`read` prints a Claude session's recent conversation from its transcript, as `nebula read` does, with its screen under it while a dialog is open there, and any other session's screen as `nebula session read` lays it out; `--bytes` keeps its end.

The orchestrator never adopts a PREWARM POOL spare: a spare booted bare has no way to be handed the guidance, so the orchestrator always starts cold.

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

- A startup setting that opens nebula in the view, for phones and other narrow terminals.
- The protocol still carries `tabs` on `OrchestratorOpenReview` and `ReviewOpened`, ignored, from the review modal `review` used to open: it goes at the next breaking protocol bump.
- First-class transcript summaries per harness instead of terminal-tail history.
- Optional idle-note injection for the orchestrator, behind an autonomy setting.
- A richer spawn target grammar for project aliases and worktree creation bases.
