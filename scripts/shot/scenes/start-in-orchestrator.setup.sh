# The orchestrator-board scene's sessions with START IN THE ORCHESTRATOR on: the launch itself
# opens on the ORCHESTRATOR VIEW, starting the orchestrator, with no F10 pressed.
# `COLS=120 make shot SCENE=start-in-orchestrator`.
. "$HERE/scenes/orchestrator-board.setup.sh"
printf '{"theme":"%s","prewarm_agents":false,"prewarm_sessions":false,"start_in_orchestrator":true}\n' \
  "${NEBULA_SHOT_THEME:-default}" > "$WORK/data/config.json"
