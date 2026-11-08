# The orchestrator-board scene with its Login redirect loop session started in the demo's
# `feature-x` worktree, whose pull request the stand-in gh answers (#39, a draft): its row names
# the branch and the pull request on a line under its title, and no root checkout's row does.
# `COLS=120 make shot SCENE=orchestrator-board-checkout`.
. "$HERE/scenes/orchestrator-board.setup.sh"
sed -i.bak 's|start "'"$DEMO"'" "Login redirect loop"|start "'"$WORK"'/demo-worktrees/feature-x" "Login redirect loop"|' "$RUNTIME/nebula"
rm -f "$RUNTIME/nebula.bak"
