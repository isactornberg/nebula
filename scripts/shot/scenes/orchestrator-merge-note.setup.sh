# A MERGE NOTE reaching a CAPTAIN: the demo's feature-x checkout has an open pull request (#39) and a
# worker that finished it, the PREWARM POOL off. F10 starts the orchestrator, whose stand-in gets the
# demo's captain with `nebula orchestrator captain`; the captain's stand-in comes up at rest on a turn
# that started the worker. Some 14 s in, the stand-in gh starts answering #39 as merged, the TUI sees
# it on the selected checkout's next PR beat and types the note into the captain, whose stand-in
# reads the worker, saves what it learned and reports with a To record line and a DONE. The keys then
# bring the captain up. The preset is NEBULA_SHOT_THEME (`default` when unset):
# `COLS=120 make shot SCENE=orchestrator-merge-note`.
mkdir -p "$WORK/data"
printf '{"theme":"%s","prewarm_agents":false,"prewarm_sessions":false}\n' "${NEBULA_SHOT_THEME:-default}" \
  > "$WORK/data/config.json"
FIX="$WORK/fixtures"
cp -R "$HERE/fixtures" "$FIX"
pr() {
  printf '{"number": 39, "url": "https://github.com/AgentSystemLabs/nebula/pull/39", "title": "Draft PRs sink to the bottom", "state": "%s", "isDraft": false, "comments": [], "reviews": []}\n' "$1" \
    > "$FIX/pr-view-feature-x.json"
}
pr OPEN
( sleep "${NEBULA_SHOT_MERGE_SECS:-14}"; pr MERGED ) &
export NEBULA_GH_FIXTURES="$FIX"
# shot.sh registers the demo with `$BIN add`; this wrapper then starts the worker in feature-x.
REAL_BIN="$BIN"
cat > "$RUNTIME/nebula" <<WRAP
#!/bin/sh
if [ "\$1" = add ]; then
  "$REAL_BIN" "\$@" || exit
  "$REAL_BIN" session start --in "$WORK/demo-worktrees/feature-x" --name "Draft PRs sink" \
    "Sort draft pull requests below the ready ones in the open list" >/dev/null
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
  post UserPromptSubmit '{"session_id":"shot-o","prompt":"Who looks after the demo?"}'
  "$NEBULA_SHOT_BIN" orchestrator captain demo >/dev/null
  say "> Who looks after the demo?" "" \
      "⏺ Bash(nebula orchestrator captain demo)" "  ⎿  the captain of demo is session 01K…" "" \
      "⏺ The demo's captain. It hears of every merge there."
  sleep 0.3; post Stop '{"session_id":"shot-o"}'
  exec /bin/cat
fi
case "$("$NEBULA_SHOT_BIN" sessions 2>/dev/null | grep "^$NEBULA_AGENT_ID ")" in
  *'name=captain '*)
    stty -echo 2>/dev/null
    post UserPromptSubmit '{"session_id":"shot-c","prompt":"Sort draft PRs to the bottom"}'
    say "> Sort draft PRs to the bottom" "" \
        "⏺ Bash(nebula spawn --worktree feature-x --json \"Sort draft…\")" \
        "  ⎿  {\"id\": \"01K…\", \"name\": \"Draft PRs sink\"}" "" \
        "⏺ Draft PRs sink is on it in feature-x." "" "  DONE started Draft PRs sink"
    sleep 0.3; post Stop '{"session_id":"shot-c"}'
    while IFS= read -r note; do
      post UserPromptSubmit '{"session_id":"shot-c","prompt":"merge note"}'
      # The note folded the way Claude wraps a prompt, as the stand-in never reflows.
      say ""; printf '%s\n' "$note" | fold -s -w 54 | sed '1s/^/> /; 2,$s/^/  /' | while IFS= read -r l; do say "$l"; done
      say "" \
          "⏺ Bash(nebula read 01K…)" "  ⎿  Notes: the list sorts by isDraft, then by date" \
          "     DONE drafts sink below the ready ones" "" \
          "⏺ Saved to memory for this repo: the open list sorts" \
          "  by isDraft before date, and a fixture needs its" \
          "  headRefName to match a checkout." "" \
          "  To record: drafts sit below ready PRs in every list" \
          "  DONE saved 2 notes on PR sorting"
      sleep 0.3; post Stop '{"session_id":"shot-c"}'
    done ;;
  *) post UserPromptSubmit '{"session_id":"shot-1","prompt":"Sort draft pull requests"}'
     say "> Sort draft pull requests below the ready ones in the open list" "" \
         "⏺ Update(crates/nebula-tui/src/pull_request.rs)" "  ⎿  Wrote 12 lines" "" \
         "⏺ Drafts sort below the ready ones; PR #39 is up." "" \
         "  Notes: the list sorts by isDraft, then by date" \
         "  DONE drafts sink below the ready ones"
     sleep 0.3; post Stop '{"session_id":"shot-1"}' ;;
esac
exec /bin/cat
AGENT
chmod +x "$RUNTIME/agent"
export NEBULA_AGENT_CMD="$RUNTIME/agent"
