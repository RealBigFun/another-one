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
//! (`daemon/src/transport_mcp.rs`) and the stdio shim
//! both call this. Session ends when the reader EOFs or a write
//! fails.

use std::io::{BufReader, Read, Write};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Deserializer, Value};

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

/// JSON-RPC error codes we use. Tool *execution* failures go
/// inside `result.isError`, not as JSON-RPC errors — see
/// `handle_tool_call` for the rationale.
mod err_code {
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
}

/// Hard cap on a single session's incoming bytes. A hostile peer
/// sending an unterminated stream can't run us out of memory.
/// 16 MiB is well above any plausible legitimate payload (MCP
/// tool requests are tens of KB at most) and well below
/// comfortable address space.
const MAX_SESSION_BYTES: u64 = 16 * 1024 * 1024;

/// Drive one MCP session to completion. Returns `Ok(())` on clean
/// EOF, `Err` only on I/O errors the transport can't recover from.
///
/// We parse the incoming byte stream with
/// [`serde_json::Deserializer::into_iter`] rather than
/// line-splitting so the server handles both newline-delimited
/// and concatenated JSON-RPC — some MCP clients pack back-to-back
/// requests without a separator. Whitespace between messages
/// (spaces, tabs, LF, CRLF) is consumed by the deserialiser.
/// Per-session state held by `serve` for the lifetime of one MCP
/// connection. Today this is just the per-session
/// `broadcast::Receiver` used by `poll_events`; it gives sessions
/// independent views into the daemon's `ClientEvent` stream so two
/// connected harnesses don't drain each other.
pub struct SessionState {
    events_rx: Option<tokio::sync::broadcast::Receiver<crate::clients::ClientEvent>>,
}

impl SessionState {
    pub fn new(events_rx: Option<tokio::sync::broadcast::Receiver<crate::clients::ClientEvent>>) -> Self {
        Self { events_rx }
    }

    /// Drain up to `max` events from the session's receiver.
    /// `tokio::sync::broadcast::error::TryRecvError::Lagged(n)` is
    /// translated into a synthetic `ClientEvent::Lagged { skipped: n }`
    /// so subscribers see the gap honestly and can resync via
    /// `list_tasks` / `list_tabs`. The receiver is still positioned
    /// at the oldest still-buffered event afterwards, so the next
    /// drain continues normally.
    pub fn drain_events(&mut self, max: usize) -> Vec<crate::clients::ClientEvent> {
        let Some(rx) = self.events_rx.as_mut() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for _ in 0..max {
            use tokio::sync::broadcast::error::TryRecvError;
            match rx.try_recv() {
                Ok(ev) => out.push(ev),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => break,
                Err(TryRecvError::Lagged(skipped)) => {
                    out.push(crate::clients::ClientEvent::Lagged { skipped });
                }
            }
        }
        out
    }
}

pub fn serve<R: Read, W: Write>(
    reader: R,
    mut writer: W,
    orchestrator: Arc<dyn McpOrchestrator>,
) -> std::io::Result<()> {
    let mut session = SessionState::new(orchestrator.subscribe_events());
    let capped = reader.take(MAX_SESSION_BYTES);
    let buffered = BufReader::new(capped);
    let stream = Deserializer::from_reader(buffered).into_iter::<Value>();
    for item in stream {
        let value = match item {
            Ok(v) => v,
            Err(err) if err.is_eof() => return Ok(()),
            Err(err) if err.is_io() => return Err(err.into()),
            Err(err) => {
                // Malformed JSON — can't produce a spec-compliant
                // response without an id. Log and close the
                // session; the peer needs to reconnect to sync up
                // the framing.
                eprintln!("mcp: malformed request: {err}");
                return Ok(());
            }
        };
        let request: Request = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("mcp: not a JSON-RPC request: {err}");
                continue;
            }
        };

        let response = dispatch(&request, orchestrator.as_ref(), &mut session);
        // Notifications (no id) don't get a response.
        let Some(response) = response else {
            continue;
        };
        writer.write_all(response.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    Ok(())
}

fn dispatch(
    req: &Request,
    orchestrator: &dyn McpOrchestrator,
    session: &mut SessionState,
) -> Option<String> {
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
            Some(handle_tool_call(req, orchestrator, session))
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

fn handle_tool_call(
    req: &Request,
    orchestrator: &dyn McpOrchestrator,
    session: &mut SessionState,
) -> String {
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

    match tools::call(name, &args, orchestrator, session) {
        Ok(result) => {
            // MCP tools/call success shape:
            //   { content: [ { type: "text", text: "..." } ], isError?: false, structuredContent? }
            let text = serde_json::to_string(&result).unwrap_or_else(|_| "{}".into());
            let wrapped = json!({
                "content": [ { "type": "text", "text": text } ],
                "structuredContent": result,
            });
            success(req, wrapped)
        }
        Err(tools::ToolError::UnknownTool) => {
            // Unknown tool is a protocol-level problem — the
            // client asked for a name we don't expose. JSON-RPC
            // error is correct here.
            error(
                req,
                err_code::METHOD_NOT_FOUND,
                format!("unknown tool: {name}"),
            )
        }
        Err(tools::ToolError::InvalidArgs(msg)) => {
            // Arg validation is likewise a protocol-level
            // problem — JSON-RPC error is correct.
            error(req, err_code::INVALID_PARAMS, msg)
        }
        Err(tools::ToolError::Execution(err)) => {
            // MCP spec: tool *execution* errors live inside
            // `result` with `isError: true` so the model can
            // observe them and react. Mapping these to JSON-RPC
            // -32000 breaks MCP clients' error-recovery path.
            let msg = format!("{err:#}");
            let wrapped = json!({
                "content": [ { "type": "text", "text": msg } ],
                "isError": true,
            });
            success(req, wrapped)
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

    /// Orchestrator wired to a real `broadcast::Sender` so tests can
    /// drive end-to-end ClientEvent flow: tools/call from one
    /// session, observe via `poll_events` from another, etc.
    ///
    /// Capacity is intentionally small (4) so the lag test below
    /// can overflow it without producing thousands of events.
    struct BusOrch {
        bus: tokio::sync::broadcast::Sender<crate::clients::ClientEvent>,
    }
    impl BusOrch {
        fn new(capacity: usize) -> Arc<Self> {
            Arc::new(Self {
                bus: tokio::sync::broadcast::channel(capacity).0,
            })
        }
    }
    impl McpOrchestrator for BusOrch {
        fn list_projects(&self) -> Vec<ProjectInfo> { Vec::new() }
        fn list_tasks(&self) -> Vec<TaskInfo> { Vec::new() }
        fn list_tabs(&self, _: &str) -> Vec<TabInfo> { Vec::new() }
        fn get_task_status(&self, _: &str) -> Option<TaskStatus> { None }
        fn read_terminal_output(&self, _: &str, _: usize) -> Option<TerminalSnapshot> { None }
        fn spawn_task(&self, _: SpawnTaskRequest) -> anyhow::Result<SpawnTaskResponse> {
            anyhow::bail!("not used in this test")
        }
        fn spawn_terminal(
            &self,
            _: SpawnTerminalRequest,
        ) -> anyhow::Result<SpawnTerminalResponse> {
            // Simulate the daemon firing a TaskOpened on the bus
            // when a tab spawns. Tests assert this reaches the
            // peer subscriber.
            let _ = self.bus.send(crate::clients::ClientEvent::TaskOpened {
                originator: crate::clients::ClientId::mcp("test"),
                task_id: "task-x".into(),
                section_id: crate::section::SectionId::for_task("p", "main", "task-x"),
                tab_id: Some("tab-x".into()),
            });
            Ok(SpawnTerminalResponse { tab_id: "tab-x".into() })
        }
        fn send_input(&self, _: &str, _: &[u8]) -> anyhow::Result<()> { Ok(()) }
        fn run_command(&self, _: RunCommandRequest) -> anyhow::Result<RunCommandResponse> {
            Ok(RunCommandResponse { output: Vec::new(), timed_out: false })
        }
        fn close_tab(&self, _: &str) -> anyhow::Result<()> { Ok(()) }
        fn subscribe_events(
            &self,
        ) -> Option<tokio::sync::broadcast::Receiver<crate::clients::ClientEvent>> {
            Some(self.bus.subscribe())
        }
    }

    fn drive_with(
        orch: Arc<dyn McpOrchestrator>,
        script: &str,
    ) -> String {
        let reader = Cursor::new(script.to_string());
        let mut writer = Vec::new();
        serve(reader, &mut writer, orch).unwrap();
        String::from_utf8(writer).unwrap()
    }

    #[test]
    fn poll_events_returns_bus_events_to_subscribed_session() {
        // One orchestrator, one session that initialises before the
        // event fires, calls spawn_terminal (which pushes onto the
        // bus), then poll_events to drain.
        let orch = BusOrch::new(64);
        let trait_arc: Arc<dyn McpOrchestrator> = orch.clone();
        let script = String::new()
            + &req(1, "initialize", json!({}))
            + &line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            + &req(2, "tools/call",
                json!({"name":"spawn_terminal","arguments":{"project_id":"p"}}))
            + &req(3, "tools/call",
                json!({"name":"poll_events","arguments":{"max_events":10}}));
        let out = drive_with(trait_arc, &script);
        // Last response is poll_events. Parse the structuredContent
        // and assert it contains a TaskOpened from the spawn we
        // just made.
        let last_line = out.lines().last().expect("response");
        let parsed: Value = serde_json::from_str(last_line).expect("json");
        let events = parsed
            .pointer("/result/structuredContent")
            .and_then(|v| v.as_array())
            .expect("event list");
        assert!(
            events.iter().any(|e| e.get("TaskOpened").is_some()),
            "expected TaskOpened in poll_events output: {events:?}"
        );
    }

    #[test]
    fn two_sessions_each_get_their_own_bus_events() {
        // Drive session A that fires the spawn, then session B
        // that subscribes after-the-fact and polls. B should see
        // ZERO events because subscription is per-session and the
        // event fired before B connected.
        let orch = BusOrch::new(64);

        // Session A — emits via spawn_terminal.
        let trait_a: Arc<dyn McpOrchestrator> = orch.clone();
        let script_a = String::new()
            + &req(1, "initialize", json!({}))
            + &line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            + &req(2, "tools/call",
                json!({"name":"spawn_terminal","arguments":{"project_id":"p"}}))
            + &req(3, "tools/call",
                json!({"name":"poll_events","arguments":{"max_events":10}}));
        let out_a = drive_with(trait_a, &script_a);
        let last_a: Value = serde_json::from_str(out_a.lines().last().unwrap()).unwrap();
        let events_a = last_a
            .pointer("/result/structuredContent")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(events_a.len(), 1, "A should see its own spawn event");

        // Session B — subscribes after the spawn already fired,
        // immediately polls.
        let trait_b: Arc<dyn McpOrchestrator> = orch.clone();
        let script_b = String::new()
            + &req(1, "initialize", json!({}))
            + &line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            + &req(2, "tools/call",
                json!({"name":"poll_events","arguments":{"max_events":10}}));
        let out_b = drive_with(trait_b, &script_b);
        let last_b: Value = serde_json::from_str(out_b.lines().last().unwrap()).unwrap();
        let events_b = last_b
            .pointer("/result/structuredContent")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(
            events_b.len(),
            0,
            "B subscribed after the event — must not see it (per-session semantics)"
        );
    }

    #[test]
    fn slow_consumer_surfaces_lagged_event_with_skipped_count() {
        // Orchestrator with capacity=4. Subscribe a session, fire
        // 6 events, then poll. The receiver dropped 2 events; the
        // first event we see should be `Lagged{skipped:2}`,
        // followed by the 4 events the buffer still held.
        let orch = BusOrch::new(4);
        // Pre-subscribe by running an initialize-only session and
        // capturing the bus separately. The trick: drive() consumes
        // the orchestrator, so we run a session that subscribes,
        // pushes events directly via the orch handle, then polls.
        let trait_arc: Arc<dyn McpOrchestrator> = orch.clone();
        let bus_handle = orch.bus.clone();
        // Custom drive that lets us push events between the
        // initialize and the poll.
        use std::io::Read;
        struct DualReader {
            init: Vec<u8>,
            poll: Vec<u8>,
            served_init: bool,
            push_done: Box<dyn Fn() + Send>,
        }
        impl Read for DualReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if !self.served_init && !self.init.is_empty() {
                    let n = self.init.len().min(buf.len());
                    buf[..n].copy_from_slice(&self.init[..n]);
                    self.init.drain(..n);
                    if self.init.is_empty() {
                        self.served_init = true;
                        (self.push_done)();
                    }
                    return Ok(n);
                }
                let n = self.poll.len().min(buf.len());
                if n == 0 { return Ok(0); }
                buf[..n].copy_from_slice(&self.poll[..n]);
                self.poll.drain(..n);
                Ok(n)
            }
        }
        let init_script = String::new()
            + &req(1, "initialize", json!({}))
            + &line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        let poll_script = req(2, "tools/call",
            json!({"name":"poll_events","arguments":{"max_events":20}}));
        let bus_for_push = bus_handle.clone();
        let push_done = Box::new(move || {
            // Fire 6 events into a 4-cap channel; the receiver
            // (subscribed during initialize) lags by 2.
            for i in 0..6u64 {
                let _ = bus_for_push.send(crate::clients::ClientEvent::TabClosed {
                    originator: crate::clients::ClientId::mcp("test"),
                    tab_id: format!("t{i}"),
                });
            }
        });
        let reader = DualReader {
            init: init_script.into_bytes(),
            poll: poll_script.into_bytes(),
            served_init: false,
            push_done,
        };
        let mut writer: Vec<u8> = Vec::new();
        serve(reader, &mut writer, trait_arc).unwrap();
        let out = String::from_utf8(writer).unwrap();
        let last_line = out.lines().last().unwrap();
        let parsed: Value = serde_json::from_str(last_line).unwrap();
        let events = parsed
            .pointer("/result/structuredContent")
            .and_then(|v| v.as_array())
            .unwrap();
        // First entry should be Lagged with skipped >= 1.
        assert!(
            matches!(
                events.first().and_then(|e| e.get("Lagged")),
                Some(_)
            ),
            "expected Lagged first; got {events:?}"
        );
        // The Lagged entry carries the skipped count as a u64.
        let skipped = events[0].pointer("/Lagged/skipped").and_then(|v| v.as_u64()).unwrap();
        assert!(skipped >= 1, "skipped should be at least 1, got {skipped}");
        // Plus we should still see some surviving TabClosed events.
        assert!(
            events.iter().skip(1).any(|e| e.get("TabClosed").is_some()),
            "expected surviving TabClosed events after lag: {events:?}"
        );
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
        assert!(
            out.is_empty(),
            "expected no response to notification, got: {out}"
        );
    }

    #[test]
    fn tools_list_returns_all_ten_tools() {
        let (out, _orch) = drive(&req(2, "tools/list", json!({})));
        let resp: Value = serde_json::from_str(out.trim()).unwrap();
        let tools = resp["result"]["tools"].as_array().expect("tools array");
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
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
        let script = req(
            3,
            "tools/call",
            json!({ "name": "list_projects", "arguments": {} }),
        );
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
        let script = req(
            4,
            "tools/call",
            json!({ "name": "not_a_tool", "arguments": {} }),
        );
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
    fn malformed_json_closes_session_cleanly() {
        // Malformed framing is unrecoverable (we can't re-sync
        // without a delimiter contract), so the session ends —
        // but it doesn't panic or propagate an I/O error.
        let script = "this is not json\n";
        let (out, _orch) = drive(script);
        assert!(out.is_empty());
    }

    #[test]
    fn concatenated_requests_are_parsed_independently() {
        // Two JSON-RPC messages back-to-back with no newline
        // between them. Must both dispatch.
        let mut script = String::new();
        script.push_str(
            &serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list",
                "params": {},
            }))
            .unwrap(),
        );
        script.push_str(
            &serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list",
                "params": {},
            }))
            .unwrap(),
        );
        let (out, _orch) = drive(&script);
        let lines: Vec<&str> = out.trim().split('\n').collect();
        assert_eq!(lines.len(), 2, "got {out}");
    }

    /// Orchestrator that fails every write tool.
    struct ErrOrch;
    impl McpOrchestrator for ErrOrch {
        fn list_projects(&self) -> Vec<ProjectInfo> {
            vec![]
        }
        fn list_tasks(&self) -> Vec<TaskInfo> {
            vec![]
        }
        fn list_tabs(&self, _: &str) -> Vec<TabInfo> {
            vec![]
        }
        fn get_task_status(&self, _: &str) -> Option<TaskStatus> {
            None
        }
        fn read_terminal_output(&self, _: &str, _: usize) -> Option<TerminalSnapshot> {
            None
        }
        fn spawn_task(&self, _: SpawnTaskRequest) -> anyhow::Result<SpawnTaskResponse> {
            anyhow::bail!("synthetic failure from test")
        }
        fn spawn_terminal(&self, _: SpawnTerminalRequest) -> anyhow::Result<SpawnTerminalResponse> {
            anyhow::bail!("no")
        }
        fn send_input(&self, _: &str, _: &[u8]) -> anyhow::Result<()> {
            anyhow::bail!("no")
        }
        fn run_command(&self, _: RunCommandRequest) -> anyhow::Result<RunCommandResponse> {
            anyhow::bail!("no")
        }
        fn close_tab(&self, _: &str) -> anyhow::Result<()> {
            anyhow::bail!("no")
        }
    }

    #[test]
    fn tool_execution_error_surfaces_as_is_error_in_result() {
        // MCP spec: tool execution failures are NOT JSON-RPC
        // errors — they're successful responses with
        // `result.isError = true` so the model can observe and
        // react. Mapping these to -32000 (as a prior version did)
        // breaks Claude Code's error-recovery path.
        let reader = Cursor::new(
            r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"spawn_task","arguments":{"project_id":"p","harness":"claude-code"}}}"#
                .to_string(),
        );
        let mut writer = Vec::new();
        let orch: Arc<dyn McpOrchestrator> = Arc::new(ErrOrch);
        serve(reader, &mut writer, orch).unwrap();
        let out = String::from_utf8(writer).unwrap();
        let resp: Value = serde_json::from_str(out.trim()).unwrap();
        assert!(
            resp.get("error").is_none(),
            "expected success response, got error: {out}"
        );
        assert_eq!(resp["result"]["isError"], true);
        assert!(resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("synthetic failure"));
    }
}
