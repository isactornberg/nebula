# The ORCHESTRATOR VIEW's BOARD: two projects (the demo and `api-server`), the PREWARM POOL off, and
# five sessions started from the shell before the TUI is up, each a story the stand-in agent below
# tells: one stopped on a permission prompt (needs you), one finished unread with a `DONE` report
# line (done), two still running, and one that finishes at once (the board's `1 idle`, once the
# keys have read it). F10 then starts the orchestrator, whose launch the agent tells apart by its
# folder. The preset is NEBULA_SHOT_THEME (`default` when unset):
# `NEBULA_SHOT_THEME=light COLS=120 make shot SCENE=orchestrator-board`.
mkdir -p "$WORK/data"
printf '{"theme":"%s","prewarm_agents":false,"prewarm_sessions":false}\n' "${NEBULA_SHOT_THEME:-default}" \
  > "$WORK/data/config.json"
API="$WORK/api-server"
mkdir -p "$API"
git -C "$API" init -q -b main
git -C "$API" -c user.name=shot -c user.email=shot@example.invalid commit -q --allow-empty -m "api-server"
# shot.sh registers the demo with `$BIN add`; this wrapper registers api-server first and, once the
# demo is in, starts the sessions. Each stand-in tells the story its session's name picks, so the
# order the CLIs come up in, which a loaded machine shuffles, does not matter.
REAL_BIN="$BIN"
cat > "$RUNTIME/nebula" <<WRAP
#!/bin/sh
if [ "\$1" = add ]; then
  "$REAL_BIN" add "$API" >/dev/null
  "$REAL_BIN" "\$@" || exit
  start() { "$REAL_BIN" session start --in "\$1" --name "\$2" "\$3" >/dev/null; }
  start "$DEMO" "Daemon startup profile" "Profile the daemon's startup and find what takes 400ms"
  start "$API" "JSON flag for lists" "Add a --json flag to every list command"
  start "$API" "Token store on sqlite" "Move the token store from a JSON file to sqlite"
  start "$DEMO" "Login redirect loop" "Fix the login redirect loop when the session cookie has expired"
  start "$DEMO" "Changelog by date" "Sort the changelog entries by date, newest first"
  exit 0
fi
exec "$REAL_BIN" "\$@"
WRAP
chmod +x "$RUNTIME/nebula"
BIN="$RUNTIME/nebula"
export NEBULA_SHOT_BIN="$REAL_BIN"
cat > "$RUNTIME/agent" <<'AGENT'
#!/bin/sh
post() {
  curl -sS -m 3 -X POST -H "Authorization: Bearer $NEBULA_API_TOKEN" -H 'Content-Type: application/json' \
    -d "$2" "$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=$1" >/dev/null 2>&1
}
say() { printf '%s\r\n' "$@"; }
if [ "$(basename "$PWD")" = orchestrator ]; then
  post UserPromptSubmit '{"session_id":"shot-o","prompt":"What needs me?"}'
  # Lines short enough for the narrowest board's chat, as a stand-in never reflows.
  say "> What needs me?" "" \
      "⏺ Bash(nebula orchestrator list --json)" "  ⎿  5 sessions across demo and api-server" "" \
      "⏺ Two things want you:" "" \
      "  - demo · Daemon startup profile is waiting on a" \
      "    permission to run cargo build --release --timings." \
      "  - api-server · JSON flag for lists finished: every" \
      "    list command takes --json now, with a test each." "" \
      "  Two more are running. Want me to bring the" \
      "  permission prompt up?"
  sleep 0.3; post Stop '{"session_id":"shot-o"}'
  exec /bin/cat
fi
case "$("$NEBULA_SHOT_BIN" sessions 2>/dev/null | grep "^$NEBULA_AGENT_ID ")" in
  *'name="Daemon startup profile"'*) post UserPromptSubmit '{"session_id":"shot-1","prompt":"Profile the daemon startup"}'
     # Claude Code 2.1's permission dialog, its rules as wide as the narrowest board's chat.
     rule=$(printf '─%.0s' $(seq 56)); dashes=$(printf '╌%.0s' $(seq 56))
     say "> Profile the daemon's startup and find what takes 400ms" "" \
         "⏺ Building release with timing report" "  ⎿  \$ cargo build --release --timings" "" \
         "$rule" " Bash command" " Build release with timing report" "$dashes" " cargo build --release --timings" \
         "$dashes" " This command requires approval" "" " Do you want to proceed?" " ❯ 1. Yes" \
         "   2. Yes, and don’t ask again for: cargo build *" "   3. No" "" " Esc to cancel · Tab to amend"
     sleep 0.3; post PermissionRequest '{"session_id":"shot-1","tool_name":"Bash"}' ;;
  *'name="JSON flag for lists"'*) post UserPromptSubmit '{"session_id":"shot-2","prompt":"Add a --json flag to every list command"}'
     say "> Add a --json flag to every list command" "" \
         "⏺ Update(src/cli/list.rs)" "  ⎿  Updated with 64 additions and 12 removals" "" \
         "⏺ Bash(cargo test cli::)" "  ⎿  test result: ok. 31 passed; 0 failed" "" \
         "⏺ Every list command takes --json now, one test each." \
         "  DONE every list command takes --json, with a test per command"
     sleep 0.3; post Stop '{"session_id":"shot-2"}' ;;
  *'name="Token store on sqlite"'*) post UserPromptSubmit '{"session_id":"shot-3","prompt":"Move the token store to sqlite"}'
     say "> Move the token store from a JSON file to sqlite" "" \
         "⏺ Write(migrations/003_tokens.sql)" "  ⎿  Wrote 1 line" "" \
         "✻ Migrating… (1m 12s · esc to interrupt)" ;;
  *'name="Login redirect loop"'*) post UserPromptSubmit '{"session_id":"shot-4","prompt":"Fix the login redirect loop"}'
     say "> Fix the login redirect loop when the session cookie has expired" "" \
         "⏺ Read(src/auth/session.rs)" "  ⎿  Read 214 lines" "" \
         "✻ Thinking… (34s · esc to interrupt)" ;;
  *) post UserPromptSubmit '{"session_id":"shot-5","prompt":"Sort the changelog by date"}'
     say "> Sort the changelog entries by date, newest first" "" "⏺ Entries are newest first."
     sleep 0.3; post Stop '{"session_id":"shot-5"}' ;;
esac
exec /bin/cat
AGENT
chmod +x "$RUNTIME/agent"
export NEBULA_AGENT_CMD="$RUNTIME/agent"
