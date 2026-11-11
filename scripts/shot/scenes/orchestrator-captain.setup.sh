# The ORCHESTRATOR VIEW's BOARD with a CAPTAIN: two projects (the demo and `api-server`), the
# PREWARM POOL off. Five sessions start from the shell before the TUI is up: in the demo one asking
# for a permission, one running and one that finishes at once (the board's `1 idle`, once the keys
# have read it); in api-server two workers, one running and one asking for a permission. F10 starts
# the orchestrator, whose stand-in gets api-server's captain with `nebula orchestrator captain`; the
# captain's stand-in ends its turn on a QUESTION. The board shows api-server as the captain's row,
# `captain · 2 workers` under `needs you`, as a finish that asks waits on the user, beside its asking
# worker, a row of its own. The stand-ins' replies end on `NEXT:` lines, the answers a reply may
# offer the user, which are the chat's and never on the board. The preset
# is NEBULA_SHOT_THEME (`default` when unset):
# `NEBULA_SHOT_THEME=light COLS=120 make shot SCENE=orchestrator-captain`.
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
  start "$API" "Token store on sqlite" "Move the token store from a JSON file to sqlite"
  start "$API" "Rate limit middleware" "Add a rate limit middleware to the API"
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
# Claude Code 2.1's permission dialog for `$1`, its rules as wide as the narrowest board's chat.
dialog() {
  rule=$(printf '─%.0s' $(seq 56)); dashes=$(printf '╌%.0s' $(seq 56))
  say "$rule" " Bash command" " $2" "$dashes" " $1" "$dashes" " This command requires approval" "" \
      " Do you want to proceed?" " ❯ 1. Yes" "   2. Yes, and don’t ask again for: ${1%% *} *" "   3. No" "" \
      " Esc to cancel · Tab to amend"
}
if [ "$(basename "$PWD")" = orchestrator ]; then
  post UserPromptSubmit '{"session_id":"shot-o","prompt":"Add rate limiting to api-server"}'
  "$NEBULA_SHOT_BIN" orchestrator captain api-server >/dev/null
  # Lines short enough for the narrowest board's chat, as a stand-in never reflows.
  say "> Add rate limiting to api-server" "" \
      "⏺ Bash(nebula orchestrator captain api-server)" "  ⎿  the captain of api-server is session 01K…" "" \
      "⏺ Bash(nebula orchestrator send 01K… \"Add rate limiting\")" "  ⎿  sent; it is working on it." "" \
      "⏺ It's with api-server's captain. It started a worker," \
      "  which waits on you to run cargo test, and asks" \
      "  whether the limit is per token or per IP." "" \
      "  NEXT: Show me the worker | Per token"
  sleep 0.3; post Stop '{"session_id":"shot-o"}'
  exec /bin/cat
fi
case "$("$NEBULA_SHOT_BIN" sessions 2>/dev/null | grep "^$NEBULA_AGENT_ID ")" in
  *'name=captain '*) post UserPromptSubmit '{"session_id":"shot-c","prompt":"Add rate limiting"}'
     say "> Add rate limiting" "" \
         "⏺ Bash(nebula spawn --worktree rate-limit --json \"Add a rate limit…\")" \
         "  ⎿  {\"id\": \"01K…\", \"name\": \"Rate limit middleware\"}" "" \
         "⏺ Rate limit middleware is on it in rate-limit; it waits on" \
         "  you to run cargo test. Token store on sqlite is still going." "" \
         "  QUESTION should the limit be per token or per IP?" \
         "  NEXT: Per token | Per IP | Per token, admins exempt"
     sleep 0.3; post Stop '{"session_id":"shot-c"}' ;;
  *'name="Daemon startup profile"'*) post UserPromptSubmit '{"session_id":"shot-1","prompt":"Profile the daemon startup"}'
     say "> Profile the daemon's startup and find what takes 400ms" "" \
         "⏺ Building release with timing report" "  ⎿  \$ cargo build --release --timings" ""
     dialog "cargo build --release --timings" "Build release with timing report"
     sleep 0.3; post PermissionRequest '{"session_id":"shot-1","tool_name":"Bash"}' ;;
  *'name="Rate limit middleware"'*) post UserPromptSubmit '{"session_id":"shot-2","prompt":"Add a rate limit middleware"}'
     say "> Add a rate limit middleware to the API" "" \
         "⏺ Update(src/middleware/rate_limit.rs)" "  ⎿  Wrote 48 lines" "" \
         "⏺ Running the middleware tests" ""
     dialog "cargo test middleware::" "Run the middleware tests"
     sleep 0.3; post PermissionRequest '{"session_id":"shot-2","tool_name":"Bash"}' ;;
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
