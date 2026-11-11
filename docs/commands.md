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
nebula open <file>…         show files/images/diagrams in file tabs     (agents run this)
nebula sessions             list every session across every project     (agents run this)
nebula read <session>       read recent context from another session    (agents run this)
nebula ask <session> <q>    ask another session and print its answer    (agents run this)
nebula session <cmd>        start a session from a shell and follow it
nebula config <cmd>         back up, restore or locate this machine's settings
nebula browser              serve this TUI in a web browser via ttyd
nebula ssh <host>           open nebula on a remote host over ssh
nebula tunnel <host>        open a remote host's nebula in a tab here
nebula sbx [name]           open nebula inside a local Docker sandbox
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
                          # If an existing project's old folder has gone missing, use the TUI's
                          # Locate project prompt on that project to relink it; `nebula add` still
                          # registers the path as a project and does not guess which stranded row
                          # should move.
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
                          # a branch name meaning origin's fetched copy — main is origin/main —
                          # but is ignored when the branch already exists; without it the
                          # worktree_base_branch setting applies, else origin's default)
nebula spawn <task> [--kind <claude|codex|cursor|pi|muse|grok|opencode>] [--worktree <branch> [--base <ref>]] [--name <title>] [--json]
                          # start a new agent session beside the current one, in the same worktree,
                          # opening on <task> (agents run this when you ask for a new nebula session;
                          # --kind defaults to this session's harness; custom harnesses launch from
                          # the TUI picker and presets, not --kind); --worktree starts it in the
                          # project's worktree on <branch> instead, creating it when the branch has
                          # none (--base picks a new branch's start point and is refused when the
                          # branch already exists), and leaves this session where it is. --name
                          # gives the session a title it keeps instead of one it gives itself. It prints
                          # the new session's id; --json prints {"id","name","worktree","path"} as
                          # `nebula session start --json` does. Once the session has started, the
                          # command never fails: should reading the new row back fail, --json
                          # prints the id with the rest null and says why on stderr, so a caller
                          # never retries into a second session. A --kind whose harness reports no
                          # status to nebula gets the warning `session start` gives
nebula open <file>…       # show the files in this nebula's FILE TABS — a modal with one tab per
                          # file, the focused one previewed, Enter editing it (agents run this only
                          # when you ask to see a file, image, mockup or diagram; text files,
                          # supported local images and Mermaid diagrams render inline, other
                          # binaries are refused and the agent names the path instead)
nebula sessions [--json]  # list every session across every project: id, name, project, worktree
                          # branch/path, harness, status and one-line recent summary, and the role
                          # of a session with one of its own (role=orchestrator, role=captain;
                          # --json gives every agent its role). Agents run this when you name
                          # another session and they need an exact id.
nebula read <session> [--lines N | --report]
                          # print clean recent context from another session, resolving by id or
                          # unambiguous fuzzy name. Claude transcript text is preferred; otherwise
                          # the screen the session's terminal shows now. An archived Claude
                          # session, one whose worktree was deleted included, is read by its
                          # exact id (a deleted session only until the daemon restarts).
                          # --report prints only a Claude session's last reply, every line of
                          # it: a finished turn's report, ending in its DONE, BLOCKED or
                          # QUESTION line, without the tool results, task notifications and
                          # reminders around it
nebula ask <session> "<question>" [--timeout S] [--no-wait]
                          # send a labelled question into another local agent, wait for that turn
                          # to finish, and print the answer: a Claude session's reply as its
                          # transcript holds it, else the screen the turn left. A busy target is
                          # queued until idle by default; --no-wait fails fast instead.
```

## Sessions from a shell

A script of your own can put sessions on the grid: it starts a session on a task in a checkout, and
follows that session by its id. It waits until it stops, reads its screen, types
its next turn, and deletes it once the work has landed. The session is an ordinary one, with a card
on the grid like any other. None of these commands starts a daemon, and each fails with a line
saying so when none is running. Of these, nebula pre-approves only `session wait` and `session read`
for the agents it runs, the two that change nothing: an agent waits on a session it spawned to hear
when it is done. The rest are for your own scripts.

```sh
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
nebula session wait <id> [--timeout <secs>] [--until-done]
                          # block until the session <id> (the id `nebula sessions` prints) is no longer
                          # running, then print the status it stopped in, alone on stdout:
                          # finished, needs_feedback, terminated, disconnected or fresh. One that
                          # is not running when asked returns at once. --timeout gives up after
                          # that many seconds with exit code 124, as timeout(1) does, saying on
                          # stderr the status the session is still in. A session
                          # that is archived, or deleted while it is waited for, is an error.
                          # A daemon restart (`nebula reload`, `nebula upgrade`) drops the wait's
                          # connection, and the wait connects again for up to 30 s and carries on.
                          # --until-done waits through needs_feedback too, and returns only once
                          # the session is done: for an agent that wants to hear the end of the
                          # work. Done is finished, terminated, disconnected or fresh, held for 5 s
                          # by the time the status changed, so a Stop that a subagent starting just
                          # after it sends back to running is waited through (a session done long
                          # ago returns at once), and with no job the session started in the
                          # background (Claude's run_in_background Bash, a Monitor watch) still
                          # running, since the agent wakes when that ends. A job left running for
                          # good (a dev server) holds the wait for as long, so give that one
                          # --timeout. Against a daemon older than this CLI the 5 s still hold, but
                          # the background jobs are out of sight. A session on a harness that
                          # reports no status to nebula stays running, so a wait on it needs
                          # --timeout
nebula session read <id>  # print the session's screen as plain text, the blank rows at its end
                          # dropped. It never attaches, so a TUI showing the session is not
                          # resized. This is one screen, not the conversation: a long answer has
                          # scrolled off it. A script that needs a session's full last message
                          # runs `nebula read --report`, which prints a Claude session's last
                          # reply from its transcript. A session with no live PTY (the IDLE
                          # REAPER took it, or the daemon restarted) is refused: reading boots
                          # nothing. The IDLE REAPER takes a finished session's PTY once its
                          # worktree has been out of every TUI's view for session_idle_timeout
                          # (5 minutes by default). For a session nobody has open that can be
                          # right after it finishes, so read straight after `wait`, or use
                          # `nebula read`
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
                          # its harness resumes one: a harness that does not boots fresh. A Claude
                          # session is first waited on to come up, past any question Claude asks
                          # before it starts: once it has reported a hook since it booted. One on
                          # its question whether to trust the folder, whose default answer quits,
                          # fails at once saying it is waiting on a dialog in its pane, and one
                          # not up within 30 s fails the same way; nothing is typed either way.
                          # The text is typed once the CLI's input box is up and its output has
                          # been quiet for 700 ms, and the Enter 150 ms after the text. When the
                          # box is not up within 30 s nothing is typed and the command fails.
                          # It returns once the session reports it is working on the turn, so a
                          # `session wait` straight after it waits for that turn and not the last
                          # one. When 5 s pass with no such report the text was typed all the
                          # same, and the exit code is 3.
                          # A screen showing a choice dialog, such as a one-time notice Claude
                          # opens once it is up, is never typed into: one still up after 30 s
                          # fails the command, saying it is waiting on a dialog in its pane
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
nebula sbx [name] [--image IMAGE]
                          # nebula inside a local Docker container, so agents never see this
                          # machine's files: creates `nebula-sbx` (or `nebula-sbx-<name>`) from
                          # buildpack-deps:bookworm-scm on first use, starts it when stopped, and
                          # gets in the way `nebula ssh` does — the same self-installing script,
                          # over `docker exec` instead of ssh. The container is kept, daemon,
                          # sessions and clones included; `docker rm -f nebula-sbx` discards it.
                          # --image only applies when the container is created. Needs `docker`
                          # on PATH; settings ride along as they do for `nebula ssh`
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
