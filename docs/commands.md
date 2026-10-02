# Commands

<sub>[← README](../README.md) · [Keys](keys.md) · [Commands](commands.md) · [Sessions](sessions.md) · [Configuration](configuration.md) · [How it works](how-it-works.md)</sub>

The `nebula` CLI. Every command carries its own help — `nebula <command> --help` is the full page,
flags and examples included, and `-h` is the one-screen reminder. `nebula --version` (short `-V`)
prints the version of the binary you're running (`nebula 0.44.0`) — the same version the TUI's
FOOTER carries at its left edge (with `⇡ vX.Y.Z` beside it once a newer release is published), and
what to check after `nebula upgrade`. This page is the same surface in one place. Commands marked *(agents run this)* are the ones a coding agent invokes on
your behalf — see [How it works](how-it-works.md).

```
nebula                      open the TUI (auto-starts the daemon)
nebula add <dir>            register a git checkout as a project
nebula daemon               run the daemon that owns every session
nebula kill                 shut the running daemon down (stops all sessions)
nebula reload               move the daemon onto the installed binary, keeping every session
nebula rename <title>       title the session this runs inside          (agents run this)
nebula worktree [name]      move this session into a worktree           (agents run this)
nebula spawn <task>         start another agent session beside it       (agents run this)
nebula open <file>…         show files in this nebula's file tabs       (agents run this)
nebula tree                 print the projects, worktrees and sessions
nebula session <cmd>        start a session from a shell and follow it
nebula config <cmd>         back up, restore or locate this machine's settings
nebula browser              serve this TUI in a web browser via ttyd
nebula ssh <host>           open nebula on a remote host over ssh
nebula tunnel <host>        open a remote host's nebula in a tab here
nebula upgrade              install the latest published nebula
```

## The TUI

```sh
nebula                    # launch the TUI (auto-starts the daemon). With no project yet, a
                          # launch from inside a git repo offers it on the splash — Enter
                          # opens it; anywhere else, `o` browses for one
```

## Projects and the daemon

```sh
nebula add <dir>          # add a repo as a project, named after its root directory
nebula add .              # same, for the repo you're in (bare `nebula <dir>` / `nebula .` also work)
                          # — but a directory whose name collides with a subcommand needs the long
                          # form (`nebula add browser`) or a `./` prefix, or bare `nebula browser`
                          # serves the TUI over ttyd instead of adding the directory
nebula daemon             # run the daemon (normally auto-spawned)
nebula daemon --foreground  # daemon with logs to stdout, for debugging
nebula kill               # stop the daemon and all sessions cleanly
nebula reload             # restart the daemon in place onto the binary this runs from: every
                          # session keeps its process, scrollback and status, and agents' hooks
                          # keep reaching it. Open TUIs drop their connection — relaunch `nebula`
                          # to pick the sessions back up. The cut-over after `make install`; a
                          # daemon from before reload existed can only be restarted by `kill`
```

## What agents run for you

```sh
nebula rename <title>     # title the current session (agents run this; --force to retitle)
nebula worktree [name] [--base <ref>]  # move the current session into a worktree of its project,
                          # creating the branch if it's new (agents run this when you ask for a
                          # worktree; no name invents one; --base picks a new branch's start point,
                          # a branch name meaning origin's fetched copy — main is origin/main;
                          # without it the worktree_base_branch setting, else origin's default)
nebula spawn <task> [--kind <claude|codex|cursor|pi|muse|grok|opencode>]  # start a new agent session beside the current
                          # one, in the same worktree, opening on <task> (agents run this when you
                          # ask for a new nebula session; --kind defaults to this session's harness;
                          # custom harnesses launch from the TUI picker and presets, not --kind)
nebula open <file>…       # show the files in this nebula's FILE TABS — a modal with one tab per
                          # file, the focused one previewed, Enter editing it (agents run this only
                          # when you ask to see a file; text files only — an image or any other
                          # binary is refused, and the agent names the path instead)
```

## Sessions from a shell

A script of your own can put sessions on the grid: it reads the tree, starts a session on a task
in a checkout, and follows that session by its id. It waits until it stops, reads its screen, types
its next turn, and deletes it once the work has landed. The session is an ordinary one, with a card
on the grid like any other. None of these commands starts a daemon, and each fails with a line
saying so when none is running. Unlike the four above, `nebula tree` and `nebula session` are not
among the commands nebula pre-approves for the agents it runs.

```sh
nebula tree               # print the tree the daemon holds: a line per project, per worktree under
                          # it, and per session in that. A session's line is its id, its status
                          # (fresh, running, finished, needs_feedback, terminated or disconnected),
                          # its harness, its model and its name. Archived sessions are left out
nebula tree --json        # the same as one JSON object: projects, each with its worktrees (root is
                          # true on the project's main checkout), each with its sessions. Archived
                          # sessions are included, and a session carries alive, unseen, archived
                          # and session_id, the id its own CLI resumes the conversation by (Claude
                          # names the transcript after it)
nebula session start [--in <dir>] [--name <title>] [--kind <harness>] [--model <m>] [--effort <e>]
                          [--json] <task>…
                          # start a session on <task>: the QUICK PROMPT, from a shell. It starts at
                          # the root of the checkout <dir> is in (default: the current directory),
                          # a project's main checkout or one of its worktrees. A repository nebula
                          # does not know is refused, naming `nebula add`, and so is one nested
                          # inside a checkout it does know: the session never lands in the
                          # checkout around it. A worktree `git worktree add` made a moment ago is
                          # waited for (up to 5 s) until WORKTREE SYNC has adopted it, so a script
                          # that wants a checkout of its own makes one with git and names it.
                          # --name titles the session and that name stays; without one it starts
                          # as agent-N and AUTO-TITLE names it from its first prompt.
                          # --kind, --model and --effort say what runs: `nebula config harnesses`
                          # lists what can be launched. Left out, they are what the QUICK PROMPT
                          # launches: the quick_prompt_kind setting's harness with its model and
                          # effort from Settings → Agents. Inside a session it does the same.
                          # It prints the new session's id, name and checkout; --json prints them
                          # as {"id","name","worktree","path"}, worktree being the worktree's id.
                          # A session on a harness that reports no status to nebula (muse, grok, a
                          # custom one with no hooks) stays running from its first task for as
                          # long as its process lives, and `start` says so in a warning on stderr
nebula session wait <id> [--timeout <secs>]
                          # block until the session <id> (the id `nebula tree` prints) is no longer
                          # running, then print the status it stopped in, alone on stdout:
                          # finished, needs_feedback, terminated, disconnected or fresh. One that
                          # is not running when asked returns at once. --timeout gives up after
                          # that many seconds with exit code 124, as timeout(1) does. A session
                          # that is archived, or deleted while it is waited for, is an error.
                          # The status is the one the session stopped in at that moment: a turn
                          # that ended while a subagent was starting can go back to running. A
                          # session on a harness that reports no status to nebula stays running,
                          # so a wait on it needs --timeout
nebula session read <id>  # print the session's screen as plain text, the blank rows at its end
                          # dropped. It never attaches, so a TUI showing the session is not
                          # resized. This is one screen, not the conversation: a long answer has
                          # scrolled off it. A script that needs a session's full last message
                          # reads the CLI's own transcript, found by the session_id
                          # `nebula tree --json` prints. A session with no live PTY (the IDLE
                          # REAPER took it, or the daemon restarted) is refused: reading boots
                          # nothing. The IDLE REAPER takes a finished session's PTY once its
                          # worktree has been out of every TUI's view for session_idle_timeout
                          # (5 minutes by default). For a session nobody has open that can be
                          # right after it finishes, so read straight after `wait`, or read the
                          # transcript
nebula session send <id> <text>…
                          # type <text> into a session at rest as its next turn and submit it, as
                          # the FOLLOW-UP COMPOSER does: its next task, or the answer to a
                          # question the agent asked in plain words at the end of its turn. It is
                          # not how a permission dialog is answered: text typed into a dialog,
                          # with its Enter, would answer it blindly. So a session that is
                          # needs_feedback is refused (answer it in the TUI), as is one still
                          # running (wait for it first) and an archived one. A session on a
                          # harness that reports no status to nebula stays running, so it is
                          # refused too.
                          # Text with line breaks goes in as one paste; control characters, a tab
                          # in text of one line, and text over 16 KiB are refused. A session with
                          # no live PTY is brought back first, resumed on its conversation where
                          # its harness resumes one: a harness that does not boots fresh. The text
                          # is typed once the CLI's input box is up and its output has been quiet
                          # for 700 ms, and the Enter 150 ms after the text. When the box is not
                          # up within 30 s nothing is typed and the command fails.
                          # It returns once the session reports it is working on the turn, so a
                          # `session wait` straight after it waits for that turn and not the last
                          # one. When 5 s pass with no such report the text was typed all the
                          # same, and the exit code is 3
nebula session delete <id>
                          # remove the session's row and stop its process, as `d` on its card
                          # does, without the confirm. A worktree whose checkout is gone keeps its
                          # row while a session is filed under it: delete the session and the row
                          # follows the checkout
```

## Settings

```sh
nebula config path              # where config.json, config.local.json, agent_presets.json and
                                # ssh_hosts.json live
nebula config export [path]     # one JSON file of this machine's settings: stdout, a file, or
                                # nebula-settings.json inside a folder. Never config.local.json
nebula config import <source>   # merge a backup in: an export, a bare config.json /
                                # agent_presets.json / ssh_hosts.json, a folder holding any of
                                # them, or - for stdin. Keys it sets replace this machine's, keys
                                # it lacks stay, and config.local.json is never written
nebula config harnesses         # print the effective harness registry: every harness with
                                # the program, flags, resume shape, hook dialect and defaults a
                                # launch uses. Copy a row into config.json `harnesses` to override it
```

See [Configuration](configuration.md#backup-restore-and-other-machines).

## Other machines, other screens

```sh
nebula ssh <host> [dir]   # open nebula on a remote machine over ssh (installs it there if
                          # missing); destinations are remembered for the TUI's HOSTS PICKER
                          # (`Shift+H`). Needs the OpenSSH client (`ssh`) on PATH here. This
                          # machine's config.json and agent presets ride along, and the remote
                          # nebula merges them into its own settings (its config.local.json
                          # still wins); --no-sync-config or the ssh_sync_config setting leaves
                          # them here
nebula tunnel <host> [dir] [--port N] [--remote-port N]
                          # that host's nebula in a browser tab here, over one ssh tunnel: installs
                          # nebula there if missing, runs `nebula browser` on its loopback, forwards
                          # the port, and opens the local URL. Nothing is exposed on the remote's
                          # network — the tunnel is the only way in — so it needs no --credential.
                          # If that host already has a `nebula browser` on the port, the tunnel
                          # reuses it instead of failing on the clash (a --credential one will ask
                          # for it in the tab).
                          # Needs the OpenSSH client (`ssh`) on PATH here and ttyd on the remote;
                          # Ctrl+C takes both ends down. --port is the local end (same rules as
                          # `nebula browser`), --remote-port the far end when something there
                          # already holds that number. Settings ride along as they do for
                          # `nebula ssh` (--no-sync-config leaves them here)
nebula browser [--port N] [--bind ADDR | --public] [--credential USER:PASSWORD] [--no-open]
                          # serve this TUI in a browser tab via ttyd and open it; needs ttyd on
                          # PATH. With no --port it takes 7681 when that's free and a free port
                          # otherwise, saying which — so one per checkout can serve at once.
                          # --port 0 always picks a free one; --port N is that port or an error,
                          # which is what you want behind an ssh tunnel. Listens on 127.0.0.1
                          # unless --bind names an interface address or --public takes them all
                          # (0.0.0.0) — for a nebula on a remote box, where the access control
                          # is the firewall/security group in front of the port. That serves a
                          # live, writable terminal, so put something in front of it and use
                          # --credential to add ttyd's HTTP basic auth on top. --no-open serves
                          # without launching a desktop browser, for a box that has none
nebula upgrade            # install the latest release (--force on a dev build). Swapping the
                          # binary doesn't touch a running daemon, so afterwards it shuts an idle
                          # one (no live sessions) down for you and the next launch starts on the
                          # new binary. With sessions live it restarts the daemon in place onto
                          # the new binary (what `nebula reload` does), and they keep running.
                          # A daemon from before in-place restarts is left up — its sessions
                          # would die with it — and it says to run `nebula kill` when you're
                          # ready. When the new build speaks another protocol, and so can't
                          # attach to that daemon, it says that too and offers the restart then
                          # and there
```
