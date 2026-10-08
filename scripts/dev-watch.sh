#!/usr/bin/env bash
# DEV WATCH — the rebuild half of `make dev-watch`. The TUI half is
# crates/nebula-tui/src/event_loop/dev_watch.rs.
#
# Polls the sources; on a save it rebuilds, moves the dev daemon onto the new
# binary when the change can reach it, and reports to the status file the dev
# TUI polls: `building`, then `ready` (the TUI relaunches itself), `idle` (the
# build relinked nothing) or `failed <why>` (the TUI flashes it and stays). The
# TUI owns the terminal, so everything this prints goes to the log.
#
# Run by the Makefile with the dev instance's environment ($(DEV_ENV)), so the
# `nebula reload` below reaches the dev daemon and never the real one.
set -uo pipefail
STATUS="$1"
LOG="$2"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$REPO/target/debug/nebula"
MARK="$STATUS.mark"
cd "$REPO" || exit 1
exec >>"$LOG" 2>&1

# Sub-second, so a build that relinks inside the second it started still
# reads as new.
mtime() { stat -c %.9Y "$1" 2>/dev/null || stat -f %Fm "$1" 2>/dev/null; }
report() { printf '%s\n' "$*" >"$STATUS.tmp" && mv "$STATUS.tmp" "$STATUS"; }

# What a build reads: the crates' sources and manifests, the workspace's, and
# the vendored crates. Integration tests never reach the binary.
changed_since() {
	find crates vendor Cargo.toml Cargo.lock -newer "$1" -type f \
		\( -name '*.rs' -o -name Cargo.toml -o -name Cargo.lock -o -name '*.ts' \) \
		-not -path 'crates/*/tests/*' 2>/dev/null
}

touch "$MARK"
while sleep 0.3; do
	[ -n "$(changed_since "$MARK" | head -1)" ] || continue
	# An editor's save is often several writes; let the burst settle before
	# building, so one save is one build.
	sleep 0.2
	changed="$(changed_since "$MARK")"
	touch "$MARK"
	echo "--- $(date '+%H:%M:%S') changed:"
	printf '  %s\n' $changed
	report building
	before="$(mtime "$BIN")"
	if ! cargo build 2>&1 | tee "$STATUS.build"; then
		why="$(grep -m1 -E '^error' "$STATUS.build")"
		report "failed ${why:-the build failed}"
		continue
	fi
	if [ "$(mtime "$BIN")" = "$before" ]; then
		report idle
		continue
	fi
	# The first exec of a freshly linked binary can stall for seconds on
	# macOS signature validation; pay it here, not inside the daemon's
	# restart or the TUI's connect deadline.
	"$BIN" --version >/dev/null
	# Only TUI code changed: the daemon already runs everything it needs, and
	# leaving it alone is what keeps a half-written daemon change from ever
	# touching the sessions.
	if printf '%s\n' $changed | grep -qvE '^crates/nebula-(tui|fuzzy)/'; then
		if ! "$BIN" reload; then
			report "failed the dev daemon didn't take the new build, see dev-watch.log"
			continue
		fi
	fi
	report ready
done
