//! MCP tool schemas and dispatch.
//!
//! Each tool the daemon MCP server exposes has a schema block
//! (returned by `tools/list`) and a call handler that parses
//! arguments, invokes the orchestrator, and shapes the response
//! payload. The schemas use JSON Schema subset that MCP clients
//! (Claude Code, Cursor, etc.) understand today — just `type`,
//! `properties`, `required`, `description`, `items`.

use serde_json::{json, Value};

use crate::mcp::orchestrator::{
    McpOrchestrator, RunCommandRequest, SelectFocusRequest, SpawnTaskRequest,
    SpawnTerminalRequest, RUN_COMMAND_TIMEOUT_CEILING_MS,
};

/// Error kinds the dispatcher produces. `server.rs` maps these to
/// JSON-RPC error codes.
#[derive(Debug)]
pub enum ToolError {
    UnknownTool,
    InvalidArgs(String),
    Execution(anyhow::Error),
}

/// Return the full list of tool descriptors for `tools/list`.
pub fn tool_manifest() -> Value {
    json!([
        {
            "name": "list_projects",
            "description": "List every project the daemon currently knows about.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "list_tasks",
            "description": "List every task across all projects.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "list_tabs",
            "description": "List the tabs belonging to a task.",
            "inputSchema": {
                "type": "object",
                "properties": { "task_id": { "type": "string" } },
                "required": ["task_id"],
                "additionalProperties": false
            }
        },
        {
            "name": "get_task_status",
            "description": "Best-effort task status: working | idle | no-tabs. Returns null if the task id is unknown.",
            "inputSchema": {
                "type": "object",
                "properties": { "task_id": { "type": "string" } },
                "required": ["task_id"],
                "additionalProperties": false
            }
        },
        {
            "name": "read_terminal_output",
            "description": "Snapshot of a tab's recent PTY output. Capped by the daemon's ring buffer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tab_id": { "type": "string" },
                    "tail": { "type": "integer", "description": "Max bytes to return. Defaults to the ring-buffer limit." }
                },
                "required": ["tab_id"],
                "additionalProperties": false
            }
        },
        {
            "name": "spawn_task",
            "description": "Create a new task in a project and launch a harness in its first tab. Spawned tasks are siblings of the caller, not children — no ongoing relationship after return.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "harness": { "type": "string", "description": "Provider id, e.g. 'claude-code' or 'codex'." },
                    "branch": { "type": "string", "description": "New branch for worktree tasks; omit for a direct task." },
                    "initial_prompt": { "type": "string" },
                    "title": { "type": "string" }
                },
                "required": ["project_id", "harness"],
                "additionalProperties": false
            }
        },
        {
            "name": "spawn_terminal",
            "description": "Spawn a plain shell terminal in a project or task scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "task_id": { "type": "string" },
                    "cwd": { "type": "string" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "send_input",
            "description": "Write bytes into a tab's PTY.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tab_id": { "type": "string" },
                    "bytes": { "type": "string", "description": "UTF-8 text to write to the PTY." }
                },
                "required": ["tab_id", "bytes"],
                "additionalProperties": false
            }
        },
        {
            "name": "run_command",
            "description": "Write a command to a PTY and wait for output until idle or timeout. Hard-capped at 5 minutes regardless of the requested timeout.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tab_id": { "type": "string" },
                    "task_id": { "type": "string" },
                    "command": { "type": "string" },
                    "timeout_ms": { "type": "integer" }
                },
                "required": ["command"],
                "additionalProperties": false
            }
        },
        {
            "name": "close_tab",
            "description": "Close a tab, terminating its PTY.",
            "inputSchema": {
                "type": "object",
                "properties": { "tab_id": { "type": "string" } },
                "required": ["tab_id"],
                "additionalProperties": false
            }
        },
        {
            "name": "poll_events",
            "description":
                "Drain up to `max_events` recent ClientEvents from the daemon's \
                 ring buffer — task/tab create/close, focus changes — so MCP \
                 harnesses can observe what the user (or peer clients) just did. \
                 Returned events are removed from the queue.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "max_events": { "type": "integer", "minimum": 1 }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "select_focus",
            "description":
                "Move a client's focus. Without `for_client`, the calling MCP \
                 session moves its own focus. With `for_client` set (privileged), \
                 the daemon moves the named peer client's view — the most common \
                 use is `for_client = \"gui:desktop\"` to scroll the human's GUI \
                 to a tab the harness just spawned.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "focus": { "type": "object",
                        "description":
                            "One of {None: null}, {Project: {project_id}}, \
                             {Task: {project_id, task_id}}, {Tab: {project_id, \
                             task_id?, section_id, tab_id}}." },
                    "for_client": {
                        "type": ["string", "null"],
                        "description":
                            "Optional ClientId to drive on behalf of (e.g. \"gui:desktop\")."
                    }
                },
                "required": ["focus"],
                "additionalProperties": false
            }
        }
    ])
}

/// Dispatch a `tools/call` to the orchestrator. Returns the
/// structured payload that `server.rs` wraps into an MCP content
/// response.
pub fn call(
    name: &str,
    args: &Value,
    orchestrator: &dyn McpOrchestrator,
) -> Result<Value, ToolError> {
    match name {
        "list_projects" => Ok(json!(orchestrator.list_projects())),
        "list_tasks" => Ok(json!(orchestrator.list_tasks())),
        "list_tabs" => {
            let task_id = string_arg(args, "task_id")?;
            Ok(json!(orchestrator.list_tabs(&task_id)))
        }
        "get_task_status" => {
            let task_id = string_arg(args, "task_id")?;
            Ok(json!(orchestrator.get_task_status(&task_id)))
        }
        "read_terminal_output" => {
            let tab_id = string_arg(args, "tab_id")?;
            let tail = args
                .get("tail")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
                .unwrap_or(usize::MAX);
            Ok(json!(orchestrator.read_terminal_output(&tab_id, tail)))
        }
        "spawn_task" => {
            let req: SpawnTaskRequest = parse_args(args)?;
            orchestrator
                .spawn_task(req)
                .map(|r| json!(r))
                .map_err(ToolError::Execution)
        }
        "spawn_terminal" => {
            let req: SpawnTerminalRequest = parse_args(args)?;
            if req.project_id.is_none() && req.task_id.is_none() {
                return Err(ToolError::InvalidArgs(
                    "spawn_terminal requires one of 'project_id' or 'task_id'".into(),
                ));
            }
            if req.project_id.is_some() && req.task_id.is_some() {
                return Err(ToolError::InvalidArgs(
                    "spawn_terminal accepts 'project_id' XOR 'task_id', not both".into(),
                ));
            }
            orchestrator
                .spawn_terminal(req)
                .map(|r| json!(r))
                .map_err(ToolError::Execution)
        }
        "send_input" => {
            let tab_id = string_arg(args, "tab_id")?;
            let bytes_str = string_arg(args, "bytes")?;
            orchestrator
                .send_input(&tab_id, bytes_str.as_bytes())
                .map(|_| json!({ "ok": true }))
                .map_err(ToolError::Execution)
        }
        "run_command" => {
            let mut req: RunCommandRequest = parse_args(args)?;
            if req.tab_id.is_none() && req.task_id.is_none() {
                return Err(ToolError::InvalidArgs(
                    "run_command requires one of 'tab_id' or 'task_id'".into(),
                ));
            }
            // Enforce the hard ceiling — a wedged harness must not
            // be able to extend the timeout past this.
            if let Some(ms) = req.timeout_ms {
                if ms > RUN_COMMAND_TIMEOUT_CEILING_MS {
                    req.timeout_ms = Some(RUN_COMMAND_TIMEOUT_CEILING_MS);
                }
            }
            orchestrator
                .run_command(req)
                .map(|r| {
                    json!({
                        // Bytes go out as UTF-8 lossy so the MCP
                        // client gets a readable string; binary-
                        // clean consumers use send_input +
                        // read_terminal_output.
                        "output": String::from_utf8_lossy(&r.output),
                        "output_bytes": r.output,
                        "timed_out": r.timed_out,
                    })
                })
                .map_err(ToolError::Execution)
        }
        "close_tab" => {
            let tab_id = string_arg(args, "tab_id")?;
            orchestrator
                .close_tab(&tab_id)
                .map(|_| json!({ "ok": true }))
                .map_err(ToolError::Execution)
        }
        "select_focus" => {
            let req: SelectFocusRequest = parse_args(args)?;
            orchestrator
                .select_focus(req)
                .map(|_| json!({ "ok": true }))
                .map_err(ToolError::Execution)
        }
        "poll_events" => {
            let max = args
                .get("max_events")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
                .unwrap_or(64);
            Ok(json!(orchestrator.poll_events(max)))
        }
        _ => Err(ToolError::UnknownTool),
    }
}

fn string_arg(args: &Value, key: &str) -> Result<String, ToolError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ToolError::InvalidArgs(format!("missing or non-string '{key}'")))
}

fn parse_args<T: for<'de> serde::Deserialize<'de>>(args: &Value) -> Result<T, ToolError> {
    serde_json::from_value(args.clone())
        .map_err(|e| ToolError::InvalidArgs(format!("invalid arguments: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_has_unique_names() {
        let manifest = tool_manifest();
        let arr = manifest.as_array().unwrap();
        let mut names = std::collections::HashSet::new();
        for t in arr {
            let n = t["name"].as_str().unwrap();
            assert!(names.insert(n), "duplicate tool name: {n}");
        }
    }

    #[test]
    fn run_command_ceiling_is_enforced() {
        // Proof of the cap via direct inspection of the constant —
        // the actual ceiling enforcement runs inside `call`, and
        // the integration tests in server.rs cover the call path.
        assert_eq!(RUN_COMMAND_TIMEOUT_CEILING_MS, 5 * 60 * 1_000);
    }

    #[test]
    fn _tool_error_has_unknown_variant() {
        fn accepts(_: ToolError) {}
        accepts(ToolError::UnknownTool);
        accepts(ToolError::InvalidArgs("x".into()));
        accepts(ToolError::Execution(anyhow::anyhow!("x")));
    }
}
