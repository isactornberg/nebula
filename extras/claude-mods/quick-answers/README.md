# quick-answers

A Claude Code mod that turns the answers a reply offers into pills above the prompt box.

When a reply's last line is `NEXT: a | b | c`, the answers show as one row between the reply and the input box, each with its digit on a pill.
Pick one with a click, or with `1` to `4` while the input box is empty; the pick is sent as the next prompt and the row goes away.
A digit typed after any other text is just text, and a reply with no `NEXT:` line shows nothing.

The mod asks for the line itself: in a session a terminal or the desktop app draws, it adds one section to the system prompt telling Claude to end a reply that leaves something to decide on such a line, the recommended answer first.
A headless run (`claude -p`, an SDK host) is told nothing, as nothing there would draw the pills.
The reply is drawn without the line, since the pills stand in for it; the stored message keeps it, and `ctrl+o` shows it.
The first answer, the one Claude recommends, is the filled pill.
A session picks the instruction up when it starts, so one already running needs a restart.

nebula's web app reads the same line from a Claude session's transcript and offers its answers as buttons under the thread, by the same rule ([Answering a session](../../../docs/web.md#answering-a-session)).
Without this mod, nothing asks Claude for the line, so the web app has no answers to offer.

## What it needs

- A Claude Code build with mod support: plugins whose `hooks/hooks.json` names a hooks module under `modules`, which `claude plugin validate` checks.
  It was built and tested on Claude Code 2.1.330.
- A [Nerd Font](https://www.nerdfonts.com) in your terminal, for the pills' rounded ends (the powerline half circles `U+E0B6` and `U+E0B4`); without one they draw as boxes or blanks.
- A truecolor terminal.
  The pills use Catppuccin Macchiato: the recommended answer filled mauve, the others on surface0.
  Change the four colours at the top of `hooks/register.tsx` to suit your theme.

## Install

From your nebula clone:

```sh
claude plugin marketplace add "$PWD/extras/claude-mods"
claude plugin install quick-answers@nebula-extras
```

Claude Code reads the mod from the clone itself, so a `git pull` that changes it takes effect after `/reload-plugins` in a running session, or in the next session.
Keep the clone where it is: the marketplace is that folder.

## Check

From your nebula clone:

```sh
claude plugin validate extras/claude-mods/quick-answers
claude plugin test extras/claude-mods/quick-answers
```

## Remove

```sh
claude plugin uninstall quick-answers@nebula-extras
claude plugin marketplace remove nebula-extras
```
