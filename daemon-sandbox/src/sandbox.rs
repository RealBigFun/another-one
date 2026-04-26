//! `TerminalRegistry` impl used only by the standalone
//! `daemon-sandbox` binary. Fakes a single project with one task and
//! one tab; the first `attach_tab` call spawns a bash PTY, subsequent
//! attaches return a fresh receiver on the same broadcast. Useful
//! for smoke-testing the iroh endpoint + mobile UI without running
//! the full desktop app.
//!
//! The desktop crate supplies its *own* `TerminalRegistry` impl that
//! wraps the running `AnotherOneApp`. This module is not linked into
//! that path.

use std::sync::{Arc, Mutex};

use portable_pty::MasterPty;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::frame::{AgentProvider, ProjectKind, ProjectSummary, TabSummary, TaskSummary};
use crate::pty::PtySession;
use crate::registry::TerminalRegistry;

const SANDBOX_PROJECT_ID: &str = "sandbox";
const SANDBOX_TASK_ID: &str = "sandbox-task";
const SANDBOX_SECTION_ID: &str = "sandbox-section";
const SANDBOX_TAB_ID: &str = "sandbox-tab";

struct Shell {
    tx: broadcast::Sender<Vec<u8>>,
    writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
}

pub struct SandboxRegistry {
    shell: Mutex<Option<Shell>>,
}

impl SandboxRegistry {
    pub fn new() -> Self {
        Self {
            shell: Mutex::new(None),
        }
    }

    /// Lazily spawn the singleton PTY on first attach.
    fn ensure_shell(&self) -> anyhow::Result<broadcast::Sender<Vec<u8>>> {
        let mut guard = self.shell.lock().unwrap();
        if let Some(shell) = guard.as_ref() {
            return Ok(shell.tx.clone());
        }

        let session = PtySession::spawn(80, 24)?;
        let (tx, _rx0) = broadcast::channel::<Vec<u8>>(64);
        let tx_for_pump = tx.clone();
        let mut output_rx = session.output_rx;
        // Pump the PTY's mpsc output into the broadcast so all
        // mobile subscribers see the same bytes.
        tokio::spawn(async move {
            while let Some(bytes) = output_rx.recv().await {
                let _ = tx_for_pump.send(bytes);
            }
        });

        *guard = Some(Shell {
            tx: tx.clone(),
            writer: Arc::new(Mutex::new(session.master_writer)),
            master: Arc::new(Mutex::new(session.master)),
        });
        Ok(tx)
    }
}

impl Default for SandboxRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalRegistry for SandboxRegistry {
    fn list_projects(&self) -> Vec<ProjectSummary> {
        vec![ProjectSummary {
            id: SANDBOX_PROJECT_ID.to_string(),
            name: "Sandbox".to_string(),
            path: "/".to_string(),
            kind: ProjectKind::Root,
            current_branch: None,
            tasks: vec![TaskSummary {
                id: SANDBOX_TASK_ID.to_string(),
                name: "shell".to_string(),
                section_id: SANDBOX_SECTION_ID.to_string(),
                branch_name: String::new(),
                active_tab_id: SANDBOX_TAB_ID.to_string(),
                tabs: vec![TabSummary {
                    id: SANDBOX_TAB_ID.to_string(),
                    title: "bash".to_string(),
                    provider: Some(AgentProvider::Shell),
                    running: true,
                    pinned: false,
                    fixed_title: None,
                }],
                pinned: false,
                last_commit_relative: String::new(),
                lines_added: 0,
                lines_removed: 0,
                target_project_id: SANDBOX_PROJECT_ID.to_string(),
            }],
        }]
    }

    fn attach_tab(&self, section_id: &str, tab_id: &str) -> Option<broadcast::Receiver<Vec<u8>>> {
        if section_id != SANDBOX_SECTION_ID || tab_id != SANDBOX_TAB_ID {
            debug!(section_id, tab_id, "sandbox: unknown section/tab");
            return None;
        }
        match self.ensure_shell() {
            Ok(tx) => Some(tx.subscribe()),
            Err(e) => {
                warn!(error = %e, "sandbox: shell spawn failed");
                None
            }
        }
    }

    fn tab_input(&self, section_id: &str, tab_id: &str, bytes: &[u8]) {
        if section_id != SANDBOX_SECTION_ID || tab_id != SANDBOX_TAB_ID {
            return;
        }
        let guard = self.shell.lock().unwrap();
        let Some(shell) = guard.as_ref() else { return };
        let mut writer = shell.writer.lock().unwrap();
        let _ = writer.write_all(bytes);
        let _ = writer.flush();
    }

    fn tab_resize(&self, _viewer_id: &str, section_id: &str, tab_id: &str, cols: u16, rows: u16) {
        // Sandbox has a single shell and a single viewer in practice;
        // min-across-viewers collapses to "whatever arrived last".
        if section_id != SANDBOX_SECTION_ID || tab_id != SANDBOX_TAB_ID {
            return;
        }
        let guard = self.shell.lock().unwrap();
        let Some(shell) = guard.as_ref() else { return };
        let master = shell.master.lock().unwrap();
        let _ = master.resize(portable_pty::PtySize {
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        });
    }
}
