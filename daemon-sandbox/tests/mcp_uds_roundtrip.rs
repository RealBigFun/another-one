//! End-to-end smoke test: daemon starts an MCP UDS listener,
//! a test client connects, drives the initialize/tools/list/
//! tools/call handshake, and gets sensible responses back.
//!
//! Exercises the real transport glue (UDS bind, accept loop,
//! spawn_blocking into the sync `serve`) not just the protocol
//! layer — that one is covered by core's in-memory tests.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use another_one_core::mcp::orchestrator::{
    McpOrchestrator, ProjectInfo, RunCommandRequest, RunCommandResponse, SpawnTaskRequest,
    SpawnTaskResponse, SpawnTerminalRequest, SpawnTerminalResponse, TabInfo, TaskInfo, TaskStatus,
    TerminalSnapshot,
};
use daemon_sandbox::transport_mcp;

#[derive(Default)]
struct FakeOrch {
    spawned_tasks: Mutex<u32>,
}

impl McpOrchestrator for FakeOrch {
    fn list_projects(&self) -> Vec<ProjectInfo> {
        vec![ProjectInfo {
            id: "proj-a".into(),
            path: "/tmp/proj-a".into(),
            label: "A".into(),
        }]
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
        *self.spawned_tasks.lock().unwrap() += 1;
        Ok(SpawnTaskResponse {
            project_id: "proj-a".into(),
            task_id: "t-new".into(),
            worktree_path: None,
            tab_id: "tab-new".into(),
        })
    }
    fn spawn_terminal(&self, _: SpawnTerminalRequest) -> anyhow::Result<SpawnTerminalResponse> {
        Ok(SpawnTerminalResponse {
            tab_id: "tab-terminal".into(),
        })
    }
    fn send_input(&self, _: &str, _: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
    fn run_command(&self, _: RunCommandRequest) -> anyhow::Result<RunCommandResponse> {
        Ok(RunCommandResponse {
            output: b"done\n".to_vec(),
            timed_out: false,
        })
    }
    fn close_tab(&self, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

fn send_line(stream: &mut UnixStream, payload: &str) {
    stream.write_all(payload.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    stream.flush().unwrap();
}

fn read_line(reader: &mut BufReader<UnixStream>) -> String {
    let mut buf = String::new();
    reader.read_line(&mut buf).unwrap();
    buf
}

#[test]
fn uds_end_to_end_initialize_and_call() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("mcp.sock");
    let orch: Arc<FakeOrch> = Arc::new(FakeOrch::default());
    let orch_trait: Arc<dyn McpOrchestrator> = orch.clone();

    let _listener =
        rt.block_on(async { transport_mcp::spawn(socket_path.clone(), orch_trait).unwrap() });

    // Tiny retry loop: `spawn` returns before the accept loop
    // actually starts polling the listener, so a too-eager
    // connect can race. Poll the path + a connect attempt for
    // up to ~1s.
    let mut client = None;
    for _ in 0..50 {
        if let Ok(c) = UnixStream::connect(&socket_path) {
            client = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut client = client.expect("failed to connect to MCP UDS within 1s");

    let mut reader = BufReader::new(client.try_clone().unwrap());

    send_line(
        &mut client,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    );
    let init_resp = read_line(&mut reader);
    assert!(init_resp.contains("another-one-daemon"), "{init_resp}");
    assert!(init_resp.contains("protocolVersion"), "{init_resp}");

    send_line(
        &mut client,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
    );
    let list_resp = read_line(&mut reader);
    assert!(list_resp.contains("spawn_task"), "{list_resp}");
    assert!(list_resp.contains("read_terminal_output"), "{list_resp}");

    send_line(
        &mut client,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_projects","arguments":{}}}"#,
    );
    let proj_resp = read_line(&mut reader);
    assert!(proj_resp.contains("proj-a"), "{proj_resp}");

    send_line(
        &mut client,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"spawn_task","arguments":{"project_id":"proj-a","harness":"claude-code"}}}"#,
    );
    let spawn_resp = read_line(&mut reader);
    assert!(spawn_resp.contains("t-new"), "{spawn_resp}");

    assert_eq!(*orch.spawned_tasks.lock().unwrap(), 1);
}
