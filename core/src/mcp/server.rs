//! Minimal Model Context Protocol server.
//!
//! Hand-rolled rather than pulling in an MCP SDK. Rationale: the
//! surface we expose is tiny (5 read tools + 5 write tools, plus
//! the `initialize` / `tools/list` / `tools/call` handshake) and
//! the existing Rust MCP ecosystem is young and churn-prone. A
//! ~500-line implementation keeps the dep graph and audit surface
//! small.
//!
//! ## Protocol summary
//!
//! - Framing: newline-delimited JSON-RPC 2.0 (one request or
//!   response per line). This is the streamable-line transport
//!   MCP uses over stdio/UDS; we don't implement the newer
//!   Streamable HTTP transport here.
//! - Handshake: client sends `initialize` with its
//!   protocol/client info. Server responds with server info and
//!   declared capabilities (only `tools`). Client then sends
//!   `notifications/initialized`. After that, `tools/list` and
//!   `tools/call` are fair game.
//! - Errors: JSON-RPC error codes — `-32601` method not found,
//!   `-32602` invalid params, `-32000` server error (tool call
//!   failure). Tool-call errors are surfaced either via the
//!   outer JSON-RPC error channel or via `{ isError: true }` on
//!   the result payload; we use the former for orchestrator
//!   `Err` and the latter is not used today.
//!
//! ## Session model
//!
//! `serve(reader, writer, orchestrator)` drives exactly one MCP
//! session. Per-connection tasks in the UDS listener
//! (`daemon-sandbox/src/transport_mcp.rs`) and the stdio shim
//! both call this. Session ends when the reader EOFs or a write
//! fails.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::mcp::orchestrator::McpOrchestrator;
use crate::mcp::tools;

const PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "another-one-daemon";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct SuccessResponse<'a> {
    jsonrpc: &'a str,
    id: Value,
    result: Value,
}

#[derive(Debug, Serialize)]
struct ErrorResponse<'a> {
    jsonrpc: &'a str,
    id: Value,
    error: ErrorObject,
}

#[derive(Debug, Serialize)]
struct ErrorObject {
    code: i32,
    message: String,
}

/// JSON-RPC error codes we use.
mod err_code {
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const SERVER_ERROR: i32 = -32000;
}

/// Drive one MCP session to completion. Returns `Ok(())` on clean
/// EOF, `Err` only on I/O errors the transport can't recover from.
pub fn serve<R: Read, W: Write>(
    reader: R,
    mut writer: W,
    orchestrator: Arc<dyn McpOrchestrator>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            // EOF. Clean end-of-session.
            return Ok(());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(err) => {
                // Can't even parse — can't produce a valid JSON-RPC
                // error response (no id). Log to stderr, skip.
                eprintln!("mcp: malformed request: {err} ({trimmed})");
                continue;
            }
        };

        let response = dispatch(&request, orchestrator.as_ref());
        // Notifications (no id) don't get a response.
        let Some(response) = response else {
            continue;
        };
        writer.write_all(response.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
}

fn dispatch(req: &Request, orchestrator: &dyn McpOrchestrator) -> Option<String> {
    let is_notification = req.id.is_none();

    match req.method.as_str() {
        "initialize" => Some(success(req, initialize_result())),
        "notifications/initialized" | "initialized" => None,
        "ping" => Some(success(req, json!({}))),
        "tools/list" => Some(success(req, json!({ "tools": tools::tool_manifest() }))),
        "tools/call" => {
            if is_notification {
                return None;
            }
            Some(handle_tool_call(req, orchestrator))
        }
        "shutdown" => {
            if is_notification {
                return None;
            }
            Some(success(req, json!(null)))
        }
        _ => {
            if is_notification {
                return None;
            }
            Some(error(
                req,
                err_code::METHOD_NOT_FOUND,
                format!("unknown method: {}", req.method),
            ))
        }
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION
        }
    })
}

fn handle_tool_call(req: &Request, orchestrator: &dyn McpOrchestrator) -> String {
    let name = match req.params.get("name").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return error(
                req,
                err_code::INVALID_PARAMS,
                "missing 'name' in tools/call params".into(),
            );
        }
    };
    let args = req.params.get("arguments").cloned().unwrap_or(json!({}));

    match tools::call(name, &args, orchestrator) {
        Ok(result) => {
            // MCP tools/call result shape:
            //   { content: [ { type: "text", text: "..." } ], isError?: bool }
            let text = serde_json::to_string(&result).unwrap_or_else(|_| "{}".into());
            let wrapped = json!({
                "content": [ { "type": "text", "text": text } ],
                "structuredContent": result,
            });
            success(req, wrapped)
        }
        Err(tools::ToolError::UnknownTool) => {
            error(req, err_code::METHOD_NOT_FOUND, format!("unknown tool: {name}"))
        }
        Err(tools::ToolError::InvalidArgs(msg)) => {
            error(req, err_code::INVALID_PARAMS, msg)
        }
        Err(tools::ToolError::Execution(err)) => {
            error(req, err_code::SERVER_ERROR, format!("{err:#}"))
        }
    }
}

fn success(req: &Request, result: Value) -> String {
    let id = req.id.clone().unwrap_or(Value::Null);
    serde_json::to_string(&SuccessResponse {
        jsonrpc: "2.0",
        id,
        result,
    })
    .expect("serialise success response")
}

fn error(req: &Request, code: i32, message: String) -> String {
    let id = req.id.clone().unwrap_or(Value::Null);
    serde_json::to_string(&ErrorResponse {
        jsonrpc: "2.0",
        id,
        error: ErrorObject { code, message },
    })
    .expect("serialise error response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::orchestrator::{
        ProjectInfo, RunCommandRequest, RunCommandResponse, SpawnTaskRequest, SpawnTaskResponse,
        SpawnTerminalRequest, SpawnTerminalResponse, TabInfo, TaskInfo, TaskStatus,
        TerminalSnapshot,
    };
    use std::io::Cursor;
    use std::sync::Mutex;

    /// A mock orchestrator recording every call for test
    /// assertions.
    #[derive(Default)]
    struct MockOrch {
        pub calls: Mutex<Vec<String>>,
    }

    impl MockOrch {
        fn record(&self, s: &str) {
            self.calls.lock().unwrap().push(s.to_string());
        }
    }

    impl McpOrchestrator for MockOrch {
        fn list_projects(&self) -> Vec<ProjectInfo> {
            self.record("list_projects");
            vec![ProjectInfo {
                id: "p1".into(),
                path: "/tmp/p1".into(),
                label: "Proj One".into(),
            }]
        }
        fn list_tasks(&self) -> Vec<TaskInfo> {
            self.record("list_tasks");
            vec![]
        }
        fn list_tabs(&self, _task_id: &str) -> Vec<TabInfo> {
            self.record("list_tabs");
            vec![]
        }
        fn get_task_status(&self, _task_id: &str) -> Option<TaskStatus> {
            self.record("get_task_status");
            None
        }
        fn read_terminal_output(&self, _tab_id: &str, _tail: usize) -> Option<TerminalSnapshot> {
            self.record("read_terminal_output");
            Some(TerminalSnapshot {
                bytes: b"hello".to_vec(),
                truncated_head: false,
            })
        }
        fn spawn_task(&self, _req: SpawnTaskRequest) -> anyhow::Result<SpawnTaskResponse> {
            self.record("spawn_task");
            Ok(SpawnTaskResponse {
                project_id: "p1".into(),
                task_id: "t1".into(),
                worktree_path: None,
                tab_id: "tab1".into(),
            })
        }
        fn spawn_terminal(
            &self,
            _req: SpawnTerminalRequest,
        ) -> anyhow::Result<SpawnTerminalResponse> {
            self.record("spawn_terminal");
            Ok(SpawnTerminalResponse {
                tab_id: "tab2".into(),
            })
        }
        fn send_input(&self, _tab_id: &str, _bytes: &[u8]) -> anyhow::Result<()> {
            self.record("send_input");
            Ok(())
        }
        fn run_command(&self, _req: RunCommandRequest) -> anyhow::Result<RunCommandResponse> {
            self.record("run_command");
            Ok(RunCommandResponse {
                output: b"ok\n".to_vec(),
                timed_out: false,
            })
        }
        fn close_tab(&self, _tab_id: &str) -> anyhow::Result<()> {
            self.record("close_tab");
            Ok(())
        }
    }

    fn drive(script: &str) -> (String, Arc<MockOrch>) {
        let reader = Cursor::new(script.to_string());
        let mut writer = Vec::new();
        let orch: Arc<MockOrch> = Arc::new(MockOrch::default());
        let orch_trait: Arc<dyn McpOrchestrator> = orch.clone();
        serve(reader, &mut writer, orch_trait).unwrap();
        (String::from_utf8(writer).unwrap(), orch)
    }

    fn line(s: &str) -> String {
        format!("{s}\n")
    }

    fn req(id: u64, method: &str, params: Value) -> String {
        line(
            &serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }))
            .unwrap(),
        )
    }

    #[test]
    fn initialize_returns_server_info_and_capabilities() {
        let (out, _orch) = drive(&req(1, "initialize", json!({})));
        let resp: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(resp["result"]["serverInfo"]["name"], SERVER_NAME);
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn notifications_initialized_is_silent() {
        let script = line(
            &serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
            }))
            .unwrap(),
        );
        let (out, _orch) = drive(&script);
        assert!(out.is_empty(), "expected no response to notification, got: {out}");
    }

    #[test]
    fn tools_list_returns_all_ten_tools() {
        let (out, _orch) = drive(&req(2, "tools/list", json!({})));
        let resp: Value = serde_json::from_str(out.trim()).unwrap();
        let tools = resp["result"]["tools"].as_array().expect("tools array");
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for expected in &[
            "list_projects",
            "list_tasks",
            "list_tabs",
            "get_task_status",
            "read_terminal_output",
            "spawn_task",
            "spawn_terminal",
            "send_input",
            "run_command",
            "close_tab",
        ] {
            assert!(
                names.contains(expected),
                "missing tool {expected}; had {names:?}"
            );
        }
    }

    #[test]
    fn tools_call_list_projects_hits_orchestrator() {
        let script = req(3, "tools/call", json!({ "name": "list_projects", "arguments": {} }));
        let (out, orch) = drive(&script);
        assert_eq!(orch.calls.lock().unwrap().as_slice(), &["list_projects"]);
        let resp: Value = serde_json::from_str(out.trim()).unwrap();
        // structuredContent carries the actual payload.
        let projects = resp["result"]["structuredContent"].as_array().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0]["id"], "p1");
    }

    #[test]
    fn tools_call_unknown_tool_returns_method_not_found() {
        let script = req(4, "tools/call", json!({ "name": "not_a_tool", "arguments": {} }));
        let (out, _orch) = drive(&script);
        let resp: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(resp["error"]["code"], err_code::METHOD_NOT_FOUND);
    }

    #[test]
    fn tools_call_missing_name_returns_invalid_params() {
        let script = req(5, "tools/call", json!({}));
        let (out, _orch) = drive(&script);
        let resp: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(resp["error"]["code"], err_code::INVALID_PARAMS);
    }

    #[test]
    fn unknown_method_returns_method_not_found() {
        let (out, _orch) = drive(&req(6, "mystery/method", json!({})));
        let resp: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(resp["error"]["code"], err_code::METHOD_NOT_FOUND);
    }

    #[test]
    fn spawn_task_arguments_are_deserialised_and_response_roundtrips() {
        let script = req(
            7,
            "tools/call",
            json!({
                "name": "spawn_task",
                "arguments": {
                    "project_id": "p1",
                    "harness": "claude-code",
                    "branch": "feat/x",
                }
            }),
        );
        let (out, orch) = drive(&script);
        assert_eq!(orch.calls.lock().unwrap().as_slice(), &["spawn_task"]);
        let resp: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(resp["result"]["structuredContent"]["task_id"], "t1");
    }

    #[test]
    fn multiple_requests_on_one_session() {
        let mut script = String::new();
        script.push_str(&req(1, "initialize", json!({})));
        script.push_str(&req(2, "tools/list", json!({})));
        script.push_str(&req(
            3,
            "tools/call",
            json!({ "name": "list_projects", "arguments": {} }),
        ));
        let (out, orch) = drive(&script);
        let lines: Vec<&str> = out.trim().split('\n').collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(orch.calls.lock().unwrap().as_slice(), &["list_projects"]);
    }

    #[test]
    fn malformed_json_is_skipped_without_killing_session() {
        let mut script = String::new();
        script.push_str("this is not json\n");
        script.push_str(&req(9, "tools/list", json!({})));
        let (out, _orch) = drive(&script);
        // Exactly one response — the tools/list one.
        assert_eq!(out.trim().split('\n').count(), 1);
    }
}
