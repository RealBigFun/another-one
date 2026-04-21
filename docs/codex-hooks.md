# Codex Session Capture Hook

Install the sample hook by copying [codex-hooks.json.example](/Users/jeff.f/webz/another-one/docs/codex-hooks.json.example) to `~/.codex/hooks.json`.

The hook runs [codex-session-start-hook.sh](/Users/jeff.f/webz/another-one/scripts/codex-session-start-hook.sh), which writes the first Codex `SessionStart` hook payload to the app-provided capture path. `another-one` then reads `session_id` from that payload and stores it in the tab launch config.
