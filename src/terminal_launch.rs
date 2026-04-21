use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context};
use portable_pty::{native_pty_system, CommandBuilder};
use serde_json::Value;
use uuid::Uuid;

use crate::agents::{
    AgentProviderKind, TerminalLaunchConfig, TerminalLaunchMode, TerminalSessionKind,
    TerminalSessionRef,
};
use crate::terminal_runtime::{PreparedTerminalRuntime, TerminalGridSize, TerminalRuntimeKey};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);
const DISCOVERY_POLL_INTERVAL: Duration = Duration::from_millis(400);

pub(crate) enum TerminalLaunchReply {
    Launched {
        key: TerminalRuntimeKey,
        runtime: PreparedTerminalRuntime,
        launch_config: TerminalLaunchConfig,
    },
    Output {
        key: TerminalRuntimeKey,
        bytes: Vec<u8>,
    },
    SessionDiscovered {
        key: TerminalRuntimeKey,
        session: TerminalSessionRef,
    },
    Exited {
        key: TerminalRuntimeKey,
        status: String,
    },
    Failed {
        key: TerminalRuntimeKey,
        message: String,
    },
}

pub(crate) enum WarmTerminalLaunchReply {
    Launched {
        launch_id: u64,
        runtime: PreparedTerminalRuntime,
        launch_config: TerminalLaunchConfig,
    },
    Output {
        launch_id: u64,
        bytes: Vec<u8>,
    },
    SessionDiscovered {
        launch_id: u64,
        session: TerminalSessionRef,
    },
    Exited {
        launch_id: u64,
        status: String,
    },
    Failed {
        launch_id: u64,
        message: String,
    },
}

pub(crate) fn spawn_terminal_launch(
    sender: mpsc::Sender<TerminalLaunchReply>,
    key: TerminalRuntimeKey,
    cwd: Option<PathBuf>,
    launch_config: TerminalLaunchConfig,
    size: TerminalGridSize,
) {
    thread::spawn(move || {
        if let Err(error) = launch_terminal(sender.clone(), key.clone(), cwd, launch_config, size) {
            let _ = sender.send(TerminalLaunchReply::Failed {
                key,
                message: error.to_string(),
            });
        }
    });
}

pub(crate) fn spawn_warm_terminal_launch(
    sender: mpsc::Sender<WarmTerminalLaunchReply>,
    launch_id: u64,
    cwd: Option<PathBuf>,
    launch_config: TerminalLaunchConfig,
    size: TerminalGridSize,
) {
    thread::spawn(move || {
        if let Err(error) =
            launch_warm_terminal(sender.clone(), launch_id, cwd, launch_config, size)
        {
            let _ = sender.send(WarmTerminalLaunchReply::Failed {
                launch_id,
                message: error.to_string(),
            });
        }
    });
}

fn launch_terminal(
    sender: mpsc::Sender<TerminalLaunchReply>,
    key: TerminalRuntimeKey,
    cwd: Option<PathBuf>,
    launch_config: TerminalLaunchConfig,
    size: TerminalGridSize,
) -> anyhow::Result<()> {
    let launch_started_at = SystemTime::now();
    let cwd = cwd.unwrap_or(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let (mut builder, launch_config, discovery_kind) =
        build_command(&cwd, launch_config, launch_started_at)?;

    builder.cwd(&cwd);
    apply_terminal_environment(&mut builder);

    let pty_system = native_pty_system();
    let pair = pty_system.openpty(size.as_pty_size())?;
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let child = pair
        .slave
        .spawn_command(builder)
        .with_context(|| format!("failed to launch terminal in {}", cwd.display()))?;
    let child_killer = child.clone_killer();

    sender
        .send(TerminalLaunchReply::Launched {
            key: key.clone(),
            runtime: PreparedTerminalRuntime {
                size,
                master: pair.master,
                writer,
                child_killer,
            },
            launch_config,
        })
        .map_err(|_| anyhow!("terminal launch receiver dropped"))?;

    let output_sender = sender.clone();
    let output_key = key.clone();
    thread::spawn(move || {
        let mut buf = [0_u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(count) => {
                    let _ = output_sender.send(TerminalLaunchReply::Output {
                        key: output_key.clone(),
                        bytes: buf[..count].to_vec(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });

    let exit_sender = sender.clone();
    let exit_key = key.clone();
    thread::spawn(move || {
        let mut child = child;
        let status = child
            .wait()
            .map(|status| status.to_string())
            .unwrap_or_else(|error| format!("terminal exited: {error}"));
        let _ = exit_sender.send(TerminalLaunchReply::Exited {
            key: exit_key,
            status,
        });
    });

    if let Some(discovery_kind) = discovery_kind {
        let discovery_sender = sender.clone();
        let discovery_key = key.clone();
        let discovery_cwd = cwd.clone();
        thread::spawn(move || {
            if let Some(session) =
                discover_session(discovery_kind, launch_started_at, &discovery_cwd)
            {
                let _ = discovery_sender.send(TerminalLaunchReply::SessionDiscovered {
                    key: discovery_key,
                    session,
                });
            }
        });
    }

    Ok(())
}

fn launch_warm_terminal(
    sender: mpsc::Sender<WarmTerminalLaunchReply>,
    launch_id: u64,
    cwd: Option<PathBuf>,
    launch_config: TerminalLaunchConfig,
    size: TerminalGridSize,
) -> anyhow::Result<()> {
    let launch_started_at = SystemTime::now();
    let cwd = cwd.unwrap_or(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let (mut builder, launch_config, discovery_kind) =
        build_command(&cwd, launch_config, launch_started_at)?;

    builder.cwd(&cwd);
    apply_terminal_environment(&mut builder);

    let pty_system = native_pty_system();
    let pair = pty_system.openpty(size.as_pty_size())?;
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let child = pair
        .slave
        .spawn_command(builder)
        .with_context(|| format!("failed to launch terminal in {}", cwd.display()))?;
    let child_killer = child.clone_killer();

    sender
        .send(WarmTerminalLaunchReply::Launched {
            launch_id,
            runtime: PreparedTerminalRuntime {
                size,
                master: pair.master,
                writer,
                child_killer,
            },
            launch_config,
        })
        .map_err(|_| anyhow!("warm terminal launch receiver dropped"))?;

    let output_sender = sender.clone();
    thread::spawn(move || {
        let mut buf = [0_u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(count) => {
                    let _ = output_sender.send(WarmTerminalLaunchReply::Output {
                        launch_id,
                        bytes: buf[..count].to_vec(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });

    let exit_sender = sender.clone();
    thread::spawn(move || {
        let mut child = child;
        let status = child
            .wait()
            .map(|status| status.to_string())
            .unwrap_or_else(|error| format!("terminal exited: {error}"));
        let _ = exit_sender.send(WarmTerminalLaunchReply::Exited { launch_id, status });
    });

    if let Some(discovery_kind) = discovery_kind {
        let discovery_sender = sender.clone();
        let discovery_cwd = cwd.clone();
        thread::spawn(move || {
            if let Some(session) =
                discover_session(discovery_kind, launch_started_at, &discovery_cwd)
            {
                let _ = discovery_sender
                    .send(WarmTerminalLaunchReply::SessionDiscovered { launch_id, session });
            }
        });
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum DiscoveryKind {
    Pi,
    Codex,
}

fn build_command(
    cwd: &Path,
    launch_config: TerminalLaunchConfig,
    launch_started_at: SystemTime,
) -> anyhow::Result<(CommandBuilder, TerminalLaunchConfig, Option<DiscoveryKind>)> {
    if launch_config.mode == TerminalLaunchMode::RawShell {
        return Ok((CommandBuilder::new_default_prog(), launch_config, None));
    }

    let Some(provider) = launch_config.provider else {
        return Ok((CommandBuilder::new_default_prog(), launch_config, None));
    };

    match provider {
        AgentProviderKind::ClaudeCode => {
            let session = launch_config.session.clone().unwrap_or(TerminalSessionRef {
                kind: TerminalSessionKind::ClaudeSession,
                id: Uuid::new_v4().to_string(),
            });
            let mut builder = CommandBuilder::new("claude");
            if launch_config.session.is_some() {
                builder.args(["--resume", session.id.as_str()]);
            } else {
                builder.args(["--session-id", session.id.as_str()]);
            }
            Ok((builder, launch_config.with_session(Some(session)), None))
        }
        AgentProviderKind::CursorAgent => {
            let session = if let Some(session) = launch_config.session.clone() {
                session
            } else {
                TerminalSessionRef {
                    kind: TerminalSessionKind::CursorChat,
                    id: create_cursor_chat(cwd)?,
                }
            };
            let mut builder = CommandBuilder::new("agent");
            builder.args(["--resume", session.id.as_str()]);
            Ok((builder, launch_config.with_session(Some(session)), None))
        }
        AgentProviderKind::Codex => {
            let mut builder = CommandBuilder::new("codex");
            let discovery = if let Some(session) = launch_config.session.clone() {
                builder.args(["resume", session.id.as_str()]);
                None
            } else {
                Some(DiscoveryKind::Codex)
            };
            let _ = launch_started_at;
            Ok((builder, launch_config, discovery))
        }
        AgentProviderKind::Pi => {
            let mut builder = CommandBuilder::new("pi");
            let discovery = if let Some(session) = launch_config.session.clone() {
                builder.args(["--session", session.id.as_str()]);
                None
            } else {
                Some(DiscoveryKind::Pi)
            };
            Ok((builder, launch_config, discovery))
        }
        provider => {
            let mut builder = CommandBuilder::new(provider_command(provider));
            if let Some(session) = launch_config.session.clone() {
                builder.arg(session.id);
            }
            Ok((builder, launch_config, None))
        }
    }
}

fn apply_terminal_environment(builder: &mut CommandBuilder) {
    builder.env("TERM", "xterm-256color");
    builder.env("COLORTERM", "truecolor");
    builder.env("COLORTERM_BCE", "1");
    builder.env("TERM_PROGRAM", "WezTerm");
    builder.env("TERM_PROGRAM_VERSION", "20240203");
}

fn create_cursor_chat(cwd: &Path) -> anyhow::Result<String> {
    let output = Command::new("agent")
        .arg("create-chat")
        .current_dir(cwd)
        .output()
        .context("failed to create Cursor Agent chat")?;
    if !output.status.success() {
        return Err(anyhow!(
            "agent create-chat failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let chat_id = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| anyhow!("agent create-chat returned no chat id"))?;
    Ok(chat_id.to_string())
}

fn provider_command(provider: AgentProviderKind) -> &'static str {
    match provider {
        AgentProviderKind::ClaudeCode => "claude",
        AgentProviderKind::CursorAgent => "agent",
        AgentProviderKind::Codex => "codex",
        AgentProviderKind::Pi => "pi",
        AgentProviderKind::Gemini => "gemini",
        AgentProviderKind::OpenCode => "opencode",
        AgentProviderKind::Amp => "amp",
        AgentProviderKind::RovoDev => "rovo-dev",
        AgentProviderKind::Forge => "forge",
    }
}

fn discover_session(
    kind: DiscoveryKind,
    launch_started_at: SystemTime,
    cwd: &Path,
) -> Option<TerminalSessionRef> {
    let deadline = SystemTime::now() + DISCOVERY_TIMEOUT;
    loop {
        let discovered = match kind {
            DiscoveryKind::Pi => discover_pi_session(launch_started_at, cwd),
            DiscoveryKind::Codex => discover_codex_session(launch_started_at),
        };
        if discovered.is_some() {
            return discovered;
        }
        if SystemTime::now() >= deadline {
            return None;
        }
        thread::sleep(DISCOVERY_POLL_INTERVAL);
    }
}

fn discover_pi_session(launch_started_at: SystemTime, cwd: &Path) -> Option<TerminalSessionRef> {
    let sessions_root = dirs::home_dir()?.join(".pi/agent/sessions");
    newest_matching_jsonl(&sessions_root, launch_started_at, |path| {
        let file = fs::File::open(path).ok()?;
        let mut first_line = String::new();
        BufReader::new(file).read_line(&mut first_line).ok()?;
        let value = serde_json::from_str::<Value>(&first_line).ok()?;
        let id = value.get("id")?.as_str()?.to_string();
        let matches_cwd = value
            .get("cwd")
            .and_then(Value::as_str)
            .map(|value| Path::new(value) == cwd)
            .unwrap_or(true);
        matches_cwd.then_some(TerminalSessionRef {
            kind: TerminalSessionKind::PiSession,
            id,
        })
    })
}

fn discover_codex_session(launch_started_at: SystemTime) -> Option<TerminalSessionRef> {
    let index_path = dirs::home_dir()?.join(".codex/session_index.jsonl");
    let recent_cutoff = launch_started_at
        .checked_sub(Duration::from_secs(5))
        .unwrap_or(launch_started_at);
    if fs::metadata(&index_path).ok()?.modified().ok()? < recent_cutoff {
        return None;
    }
    let file = fs::File::open(index_path).ok()?;
    let reader = BufReader::new(file);

    reader
        .lines()
        .filter_map(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .filter_map(|value| value.get("id")?.as_str().map(str::to_string))
        .last()
        .map(|id| TerminalSessionRef {
            kind: TerminalSessionKind::CodexSession,
            id,
        })
}

fn newest_matching_jsonl(
    root: &Path,
    launch_started_at: SystemTime,
    mut parse: impl FnMut(&Path) -> Option<TerminalSessionRef>,
) -> Option<TerminalSessionRef> {
    let mut stack = vec![root.to_path_buf()];
    let recent_cutoff = launch_started_at
        .checked_sub(Duration::from_secs(5))
        .unwrap_or(launch_started_at);
    let mut newest: Option<(SystemTime, TerminalSessionRef)> = None;

    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let modified_at = entry.metadata().ok()?.modified().ok()?;
            if modified_at < recent_cutoff {
                continue;
            }
            let Some(session) = parse(&path) else {
                continue;
            };
            let replace = newest
                .as_ref()
                .map(|(current, _)| modified_at > *current)
                .unwrap_or(true);
            if replace {
                newest = Some((modified_at, session));
            }
        }
    }

    newest.map(|(_, session)| session)
}
