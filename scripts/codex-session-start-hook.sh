#!/bin/sh

set -eu

capture_path="${ANOTHER_ONE_CODEX_SESSION_CAPTURE:-}"
if [ -z "$capture_path" ]; then
  exit 0
fi

capture_dir=$(dirname "$capture_path")
mkdir -p "$capture_dir"

# SessionStart also fires for subagents. Keep the first payload so the parent
# session wins and later inherited hook invocations do not overwrite it.
( set -C; cat > "$capture_path" ) 2>/dev/null || cat >/dev/null
