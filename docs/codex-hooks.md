# Codex Session Restore

`another-one` no longer requires a Codex startup hook to restore sessions.

When launching a fresh Codex tab, the app now discovers the new session by scanning `~/.codex/sessions` (and `~/.codex/archived_sessions` as a fallback) for the newest rollout whose creation time is close to the terminal launch time and whose recorded `cwd` matches the tab.

`~/.codex/session_index.jsonl` is still used as a last-resort fallback when no matching session file is available.

The sample files [codex-hooks.json.example](/Users/jeff.f/webz/another-one/docs/codex-hooks.json.example) and [codex-session-start-hook.sh](/Users/jeff.f/webz/another-one/scripts/codex-session-start-hook.sh) are kept only as legacy/debugging references.
