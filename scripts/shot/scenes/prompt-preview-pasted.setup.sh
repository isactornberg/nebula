# A card whose last prompt came in pasted: its stand-in reports the prompt the way Claude Code hands a
# paste to its hooks, wrapped in `<pasted_content id="…">`, and the card's preview reads what was asked.
mkdir -p "$WORK/data"
printf '{"prewarm_agents":false,"prewarm_sessions":false}\n' > "$WORK/data/config.json"
REAL_BIN="$BIN"
cat > "$RUNTIME/nebula" <<WRAP
#!/bin/sh
if [ "\$1" = add ]; then
  "$REAL_BIN" "\$@" || exit
  "$REAL_BIN" session start --in "$DEMO" --name "Docstring for total" "Add a docstring to total()" >/dev/null
  exit 0
fi
exec "$REAL_BIN" "\$@"
WRAP
chmod +x "$RUNTIME/nebula"
BIN="$RUNTIME/nebula"
cat > "$RUNTIME/agent" <<'AGENT'
#!/bin/sh
curl -sS -m 3 -X POST -H "Authorization: Bearer $NEBULA_API_TOKEN" -H 'Content-Type: application/json' \
  -d '{"session_id":"p","prompt":"<pasted_content id=\"50cb\">\n[question from session \"captain\"]\nIs the add-docstring branch pushed, and does its test pass?\n</pasted_content>"}' \
  "$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=UserPromptSubmit" >/dev/null 2>&1
printf '%s\r\n' '> [question from session "captain"]' '  Is the add-docstring branch pushed, and does its test pass?'
exec /bin/cat
AGENT
chmod +x "$RUNTIME/agent"
export NEBULA_AGENT_CMD="$RUNTIME/agent"
