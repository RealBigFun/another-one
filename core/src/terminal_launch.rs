use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context};
use portable_pty::{native_pty_system, CommandBuilder};
use serde_json::Value;
use uuid::Uuid;

use crate::agents::{
    harness, HarnessEnv, TerminalLaunchConfig, TerminalLaunchMode, TerminalSessionKind,
    TerminalSessionRef,
};
use crate::terminal_types::{PreparedTerminalRuntime, TerminalGridSize, TerminalRuntimeKey};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);
const CODEX_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const DISCOVERY_POLL_INTERVAL: Duration = Duration::from_millis(400);
pub(crate) const CODEX_HOME_ENV: &str = "CODEX_HOME";
const ANOTHER_ONE_PI_SESSION_CAPTURE_ENV: &str = "ANOTHER_ONE_PI_SESSION_CAPTURE";

pub enum TerminalLaunchReply {
    Launched {
        key: TerminalRuntimeKey,
        runtime: PreparedTerminalRuntime,
        launch_config: TerminalLaunchConfig,
        process_id: Option<u32>,
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
        details: String,
    },
}

pub enum WarmTerminalLaunchReply {
    Launched {
        launch_id: u64,
        runtime: PreparedTerminalRuntime,
        launch_config: TerminalLaunchConfig,
        process_id: Option<u32>,
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
        details: String,
    },
}

pub fn spawn_terminal_launch(
    sender: mpsc::Sender<TerminalLaunchReply>,
    key: TerminalRuntimeKey,
    cwd: Option<PathBuf>,
    launch_config: TerminalLaunchConfig,
    agent_launch_args: Vec<String>,
    size: TerminalGridSize,
) {
    thread::spawn(move || {
        if let Err(error) = launch_terminal(
            sender.clone(),
            key.clone(),
            cwd,
            launch_config,
            agent_launch_args,
            size,
        ) {
            let _ = sender.send(TerminalLaunchReply::Failed {
                key,
                message: format_launch_error(&error),
                details: format_launch_error_details(&error),
            });
        }
    });
}

pub fn spawn_warm_terminal_launch(
    sender: mpsc::Sender<WarmTerminalLaunchReply>,
    launch_id: u64,
    cwd: Option<PathBuf>,
    launch_config: TerminalLaunchConfig,
    agent_launch_args: Vec<String>,
    size: TerminalGridSize,
) {
    thread::spawn(move || {
        if let Err(error) = launch_warm_terminal(
            sender.clone(),
            launch_id,
            cwd,
            launch_config,
            agent_launch_args,
            size,
        ) {
            let _ = sender.send(WarmTerminalLaunchReply::Failed {
                launch_id,
                message: format_launch_error(&error),
                details: format_launch_error_details(&error),
            });
        }
    });
}

fn format_launch_error(error: &anyhow::Error) -> String {
    format!("{error:#}")
}

fn format_launch_error_details(error: &anyhow::Error) -> String {
    format!("{error:?}")
}

fn launch_terminal(
    sender: mpsc::Sender<TerminalLaunchReply>,
    key: TerminalRuntimeKey,
    cwd: Option<PathBuf>,
    launch_config: TerminalLaunchConfig,
    agent_launch_args: Vec<String>,
    size: TerminalGridSize,
) -> anyhow::Result<()> {
    let launch_started_at = SystemTime::now();
    let cwd = cwd.unwrap_or(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let env = HarnessEnv::from_os();
    let (mut builder, launch_config, discovery_kind) =
        build_command(&env, &cwd, launch_config, &agent_launch_args)?;

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
    let process_id = child.process_id();
    let child_killer = child.clone_killer();

    // Broadcast tee — see `PreparedTerminalRuntime::output_broadcast`.
    // Capacity 512 absorbs a burst of ~4 MB (8 KiB reads × 512) —
    // roughly one full alt-screen repaint from Claude Code's
    // status-panel rewrite storms without the subscriber hitting
    // `RecvError::Lagged` and skipping rows.
    let (output_broadcast, _initial_rx) = tokio::sync::broadcast::channel::<Vec<u8>>(512);
    let broadcast_for_reader = output_broadcast.clone();

    // Only clone the 8 KiB chunk when there's actually a subscriber —
    // zero-subscriber `send()` returns Err and we were cloning for
    // nothing before. `receiver_count()` is atomic, so this is cheap.
    let has_subscribers = move || broadcast_for_reader.receiver_count() > 0;
    let broadcast_for_reader_send = output_broadcast.clone();

    sender
        .send(TerminalLaunchReply::Launched {
            key: key.clone(),
            runtime: PreparedTerminalRuntime {
                size,
                master: pair.master,
                writer,
                child_killer,
                output_broadcast,
            },
            launch_config,
            process_id,
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
                    let bytes = buf[..count].to_vec();
                    // Only clone + broadcast when a mobile viewer is
                    // actually subscribed. Zero-subscriber send()
                    // would return Err, but the `.clone()` of the
                    // Vec runs unconditionally — wasted at ~hundreds
                    // of chunks/sec under chatty agents.
                    if has_subscribers() {
                        let _ = broadcast_for_reader_send.send(bytes.clone());
                    }
                    let _ = output_sender.send(TerminalLaunchReply::Output {
                        key: output_key.clone(),
                        bytes,
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
    agent_launch_args: Vec<String>,
    size: TerminalGridSize,
) -> anyhow::Result<()> {
    let launch_started_at = SystemTime::now();
    let cwd = cwd.unwrap_or(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let env = HarnessEnv::from_os();
    let (mut builder, launch_config, discovery_kind) =
        build_command(&env, &cwd, launch_config, &agent_launch_args)?;

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
    let process_id = child.process_id();
    let child_killer = child.clone_killer();

    // Warm launches get a broadcast tee too. Today the warm-launch
    // flow is desktop-only (it never reaches mobile because warm
    // tabs aren't promoted onto a task's tab row until committed),
    // but constructing the sender here keeps `PreparedTerminalRuntime`
    // non-optional and lets future code subscribe without a branch.
    let (output_broadcast, _initial_rx) = tokio::sync::broadcast::channel::<Vec<u8>>(512);
    let broadcast_for_reader = output_broadcast.clone();

    sender
        .send(WarmTerminalLaunchReply::Launched {
            launch_id,
            runtime: PreparedTerminalRuntime {
                size,
                master: pair.master,
                writer,
                child_killer,
                output_broadcast,
            },
            launch_config,
            process_id,
        })
        .map_err(|_| anyhow!("warm terminal launch receiver dropped"))?;

    let output_sender = sender.clone();
    thread::spawn(move || {
        let mut buf = [0_u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(count) => {
                    let bytes = buf[..count].to_vec();
                    let _ = broadcast_for_reader.send(bytes.clone());
                    let _ =
                        output_sender.send(WarmTerminalLaunchReply::Output { launch_id, bytes });
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

#[derive(Clone, Debug)]
pub(crate) enum DiscoveryKind {
    Codex { root: PathBuf },
    Pi { capture: PiSessionCapture },
}

#[derive(Clone, Debug)]
pub(crate) struct PiSessionCapture {
    path: PathBuf,
}

fn build_command(
    env: &HarnessEnv,
    cwd: &Path,
    launch_config: TerminalLaunchConfig,
    agent_launch_args: &[String],
) -> anyhow::Result<(CommandBuilder, TerminalLaunchConfig, Option<DiscoveryKind>)> {
    if launch_config.mode == TerminalLaunchMode::RawShell {
        return Ok((CommandBuilder::new_default_prog(), launch_config, None));
    }

    let Some(provider) = launch_config.provider else {
        return Ok((CommandBuilder::new_default_prog(), launch_config, None));
    };

    let mut combined_agent_launch_args = agent_launch_args.to_vec();
    combined_agent_launch_args.extend(launch_config.extra_args.iter().cloned());

    harness(provider).build_launch(env, cwd, launch_config, &combined_agent_launch_args)
}

pub(crate) fn resolve_pi_session(
    cwd: &Path,
    requested_session: Option<&TerminalSessionRef>,
    sessions_root: Option<&Path>,
) -> Option<TerminalSessionRef> {
    let session = requested_session?;
    pi_session_exists(cwd, &session.id, sessions_root).then(|| session.clone())
}

fn pi_session_exists(cwd: &Path, session_id: &str, sessions_root: Option<&Path>) -> bool {
    let session_path = Path::new(session_id);
    if session_path.is_file() {
        return true;
    }

    let sessions_root = sessions_root
        .map(Path::to_path_buf)
        .or_else(|| dirs::home_dir().map(|home| home.join(".pi/agent/sessions")));
    let Some(sessions_root) = sessions_root else {
        return false;
    };

    let mut stack = vec![sessions_root];
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
            let Ok(file) = fs::File::open(&path) else {
                continue;
            };
            let mut first_line = String::new();
            if BufReader::new(file).read_line(&mut first_line).is_err() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&first_line) else {
                continue;
            };
            let matches_id = value.get("id").and_then(Value::as_str) == Some(session_id);
            if !matches_id {
                continue;
            }
            let matches_cwd = value
                .get("cwd")
                .and_then(Value::as_str)
                .map(|value| Path::new(value) == cwd)
                .unwrap_or(true);
            if matches_cwd {
                return true;
            }
        }
    }

    false
}

pub(crate) fn resolve_claude_session(
    cwd: &Path,
    requested_session: Option<&TerminalSessionRef>,
    projects_root: Option<&Path>,
) -> (TerminalSessionRef, bool) {
    if let Some(session) = requested_session {
        if claude_session_exists(cwd, &session.id, projects_root) {
            return (session.clone(), true);
        }
    }

    (
        TerminalSessionRef {
            kind: TerminalSessionKind::ClaudeSession,
            id: Uuid::new_v4().to_string(),
        },
        false,
    )
}

fn claude_session_exists(cwd: &Path, session_id: &str, projects_root: Option<&Path>) -> bool {
    let projects_root = projects_root
        .map(Path::to_path_buf)
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude/projects")));
    let Some(projects_root) = projects_root else {
        return false;
    };

    let session_file_name = format!("{session_id}.jsonl");
    let expected_project_path = projects_root
        .join(claude_project_dir_name(cwd))
        .join(&session_file_name);
    if expected_project_path.is_file() {
        return true;
    }

    let Ok(entries) = fs::read_dir(projects_root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_dir() && path.join(&session_file_name).is_file()
    })
}

fn claude_project_dir_name(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

fn apply_terminal_environment(builder: &mut CommandBuilder) {
    builder.env("TERM", "xterm-256color");
    builder.env("COLORTERM", "truecolor");
    builder.env("COLORTERM_BCE", "1");
    builder.env("TERM_PROGRAM", "WezTerm");
    builder.env("TERM_PROGRAM_VERSION", "20240203");
    apply_agent_command_path(builder);
}

fn apply_agent_command_path(builder: &mut CommandBuilder) {
    builder.env(
        "PATH",
        agent_command_path_env().to_string_lossy().into_owned(),
    );
}

fn agent_command_path_env() -> OsString {
    std::env::join_paths(agent_command_path_dirs()).unwrap_or_else(|_| {
        std::env::var_os("PATH").unwrap_or_else(|| OsString::from(default_agent_command_path()))
    })
}

fn agent_command_path_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }

    dirs.extend(shell_initialized_path_dirs());

    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".cargo/bin"));
    }

    dirs.extend(default_agent_command_path().split(':').map(PathBuf::from));

    let mut unique = Vec::new();
    for dir in dirs {
        if !unique.iter().any(|existing| existing == &dir) {
            unique.push(dir);
        }
    }
    unique
}

fn shell_initialized_path_dirs() -> Vec<PathBuf> {
    static PATH_DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    PATH_DIRS
        .get_or_init(read_shell_initialized_path_dirs)
        .clone()
}

fn read_shell_initialized_path_dirs() -> Vec<PathBuf> {
    let Some(shell) = user_shell_path() else {
        return Vec::new();
    };

    let Ok(output) = std::process::Command::new(shell)
        .args(["-lic", "printf '\\n__ANOTHER_ONE_PATH__%s\\n' \"$PATH\""])
        .output()
    else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(path) = stdout
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("__ANOTHER_ONE_PATH__"))
    else {
        return Vec::new();
    };

    std::env::split_paths(path).collect()
}

fn user_shell_path() -> Option<OsString> {
    if let Some(shell) = std::env::var_os("SHELL").filter(|shell| !shell.is_empty()) {
        return Some(shell);
    }

    #[cfg(target_os = "macos")]
    {
        Some(OsString::from("/bin/zsh"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Some(OsString::from("/bin/bash"))
    }

    #[cfg(not(unix))]
    {
        None
    }
}

fn default_agent_command_path() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
    }

    #[cfg(not(target_os = "macos"))]
    {
        "/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin:/snap/bin"
    }
}

impl PiSessionCapture {
    pub(crate) fn attach_to_command(&self, builder: &mut CommandBuilder) {
        builder.env(
            ANOTHER_ONE_PI_SESSION_CAPTURE_ENV,
            self.path.to_string_lossy().into_owned(),
        );
    }
}

pub(crate) fn resolve_codex_home_override(
    env: &HarnessEnv,
    launch_config: TerminalLaunchConfig,
) -> anyhow::Result<(TerminalLaunchConfig, Option<PathBuf>)> {
    if let Some(home_override) = launch_config.home_override.clone() {
        prepare_codex_home_override(env, &home_override)?;
        return Ok((launch_config, Some(home_override)));
    }

    if launch_config.session.is_some() {
        return Ok((launch_config, None));
    }

    let home_override = create_codex_home_override_path(env)?;
    prepare_codex_home_override(env, &home_override)?;
    Ok((
        launch_config.with_home_override(Some(home_override.clone())),
        Some(home_override),
    ))
}

fn create_codex_home_override_path(env: &HarnessEnv) -> anyhow::Result<PathBuf> {
    let codex_homes_dir = env
        .codex_isolated_homes_root
        .clone()
        .ok_or_else(|| anyhow!("no codex isolated home root configured"))?;
    fs::create_dir_all(&codex_homes_dir)
        .with_context(|| format!("failed to create {}", codex_homes_dir.display()))?;
    Ok(codex_homes_dir.join(Uuid::new_v4().to_string()))
}

fn prepare_codex_home_override(env: &HarnessEnv, home_override: &Path) -> anyhow::Result<()> {
    prepare_codex_home_override_from(env.codex_root.as_deref(), home_override)
}

fn prepare_codex_home_override_from(
    source_home: Option<&Path>,
    home_override: &Path,
) -> anyhow::Result<()> {
    fs::create_dir_all(home_override)
        .with_context(|| format!("failed to create {}", home_override.display()))?;

    let Some(source_home) = source_home else {
        return Ok(());
    };
    if source_home == home_override {
        return Ok(());
    }

    copy_file_if_exists(
        &source_home.join("config.toml"),
        &home_override.join("config.toml"),
    )?;
    copy_file_if_exists(
        &source_home.join("installation_id"),
        &home_override.join("installation_id"),
    )?;
    copy_file_if_exists(
        &source_home.join("version.json"),
        &home_override.join("version.json"),
    )?;
    ensure_symlink_if_exists(
        &source_home.join("auth.json"),
        &home_override.join("auth.json"),
    )?;
    ensure_symlink_if_exists(&source_home.join("plugins"), &home_override.join("plugins"))?;
    ensure_symlink_if_exists(&source_home.join("skills"), &home_override.join("skills"))?;
    ensure_symlink_if_exists(
        &source_home.join("vendor_imports"),
        &home_override.join("vendor_imports"),
    )?;

    Ok(())
}

fn copy_file_if_exists(source: &Path, target: &Path) -> anyhow::Result<()> {
    if !source.is_file() {
        return Ok(());
    }
    fs::copy(source, target).with_context(|| {
        format!(
            "failed to copy shared codex file from {} to {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(())
}

fn ensure_symlink_if_exists(source: &Path, target: &Path) -> anyhow::Result<()> {
    if !source.exists() {
        return Ok(());
    }

    if let Ok(existing_target) = fs::read_link(target) {
        if existing_target == source {
            return Ok(());
        }
        fs::remove_file(target)
            .or_else(|_| fs::remove_dir_all(target))
            .ok();
    } else if target.exists() {
        fs::remove_file(target)
            .or_else(|_| fs::remove_dir_all(target))
            .ok();
    }

    unix_fs::symlink(source, target).with_context(|| {
        format!(
            "failed to symlink shared codex path from {} to {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(())
}

pub(crate) fn prepare_pi_session_capture(env: &HarnessEnv) -> anyhow::Result<PiSessionCapture> {
    let capture_dir = env
        .pi_session_captures_root
        .clone()
        .ok_or_else(|| anyhow!("no pi session captures root configured"))?;
    fs::create_dir_all(&capture_dir)
        .with_context(|| format!("failed to create {}", capture_dir.display()))?;
    Ok(PiSessionCapture {
        path: capture_dir.join(format!("{}.json", Uuid::new_v4())),
    })
}

pub(crate) fn pi_session_capture_extension_path() -> PathBuf {
    // `terminal_launch.rs` lives in the `core` crate, but the Pi extension is
    // checked into the workspace-level `scripts/` directory.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")))
        .join("scripts")
        .join("pi-session-start-extension.ts")
}

pub(crate) fn create_cursor_chat(env: &HarnessEnv, cwd: &Path) -> anyhow::Result<String> {
    let output = env
        .command_runner
        .run("agent", &["create-chat"], cwd)
        .context("failed to create Cursor Agent chat")?;
    if !output.success {
        return Err(anyhow!(
            "agent create-chat failed: {}",
            output.stderr.trim()
        ));
    }
    let chat_id = output
        .stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| anyhow!("agent create-chat returned no chat id"))?;
    Ok(chat_id.to_string())
}

fn discover_session(
    kind: DiscoveryKind,
    launch_started_at: SystemTime,
    cwd: &Path,
) -> Option<TerminalSessionRef> {
    let deadline = SystemTime::now() + discovery_timeout_for_kind(&kind);
    loop {
        let discovered = match &kind {
            DiscoveryKind::Codex { root } => {
                discover_codex_session(launch_started_at, cwd, Some(root))
            }
            DiscoveryKind::Pi { capture } => {
                discover_pi_session(launch_started_at, cwd, Some(&capture.path))
            }
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

fn discovery_timeout_for_kind(kind: &DiscoveryKind) -> Duration {
    match kind {
        // Codex can delay creation of the resumable rollout file until after
        // the first real turn, so keep discovery alive well past startup.
        DiscoveryKind::Codex { .. } => CODEX_DISCOVERY_TIMEOUT,
        DiscoveryKind::Pi { .. } => DISCOVERY_TIMEOUT,
    }
}

fn discover_pi_session(
    launch_started_at: SystemTime,
    cwd: &Path,
    capture_path: Option<&Path>,
) -> Option<TerminalSessionRef> {
    match capture_path.map(|path| read_session_capture(path, TerminalSessionKind::PiSession)) {
        Some(SessionCaptureState::Ready(session)) => {
            let _ = fs::remove_file(capture_path.expect("capture path should exist"));
            return Some(session);
        }
        Some(SessionCaptureState::Pending) => return None,
        Some(SessionCaptureState::Missing) | None => {}
    }

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

fn discover_codex_session(
    launch_started_at: SystemTime,
    cwd: &Path,
    codex_root: Option<&Path>,
) -> Option<TerminalSessionRef> {
    discover_codex_session_from_saved_sessions(launch_started_at, cwd, codex_root)
        .or_else(|| discover_codex_session_from_index(launch_started_at, codex_root))
}

fn discover_codex_session_from_saved_sessions(
    launch_started_at: SystemTime,
    cwd: &Path,
    codex_root: Option<&Path>,
) -> Option<TerminalSessionRef> {
    let codex_root = codex_root
        .map(Path::to_path_buf)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))?;
    let mut newest: Option<(SystemTime, TerminalSessionRef)> = None;

    for root in [
        codex_root.join("sessions"),
        codex_root.join("archived_sessions"),
    ] {
        let Some((discovered_at, session)) =
            newest_matching_codex_session(&root, launch_started_at, cwd)
        else {
            continue;
        };
        let replace = newest
            .as_ref()
            .map(|(current, _)| discovered_at > *current)
            .unwrap_or(true);
        if replace {
            newest = Some((discovered_at, session));
        }
    }

    newest.map(|(_, session)| session)
}

fn discover_codex_session_from_index(
    launch_started_at: SystemTime,
    codex_root: Option<&Path>,
) -> Option<TerminalSessionRef> {
    let index_path = codex_root
        .map(Path::to_path_buf)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))?
        .join("session_index.jsonl");
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

fn newest_matching_codex_session(
    root: &Path,
    launch_started_at: SystemTime,
    cwd: &Path,
) -> Option<(SystemTime, TerminalSessionRef)> {
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

            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let Some(created_at) = codex_session_file_time(&metadata) else {
                continue;
            };
            if created_at < recent_cutoff {
                continue;
            }

            let Some(session) = parse_codex_session_file(&path, cwd) else {
                continue;
            };
            let replace = newest
                .as_ref()
                .map(|(current, _)| created_at > *current)
                .unwrap_or(true);
            if replace {
                newest = Some((created_at, session));
            }
        }
    }

    newest
}

fn codex_session_file_time(metadata: &fs::Metadata) -> Option<SystemTime> {
    metadata.created().ok().or_else(|| metadata.modified().ok())
}

fn parse_codex_session_file(path: &Path, cwd: &Path) -> Option<TerminalSessionRef> {
    let file = fs::File::open(path).ok()?;
    let mut first_line = String::new();
    BufReader::new(file).read_line(&mut first_line).ok()?;
    let value = serde_json::from_str::<Value>(&first_line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }

    let payload = value.get("payload")?;
    let matches_cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(|value| Path::new(value) == cwd)
        .unwrap_or(false);
    if !matches_cwd {
        return None;
    }

    Some(TerminalSessionRef {
        kind: TerminalSessionKind::CodexSession,
        id: payload.get("id")?.as_str()?.to_string(),
    })
}

#[derive(Debug, PartialEq, Eq)]
enum SessionCaptureState {
    Missing,
    Pending,
    Ready(TerminalSessionRef),
}

fn read_session_capture(path: &Path, kind: TerminalSessionKind) -> SessionCaptureState {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return SessionCaptureState::Missing;
        }
        Err(_) => return SessionCaptureState::Pending,
    };
    let Some(id) = serde_json::from_str::<Value>(&contents)
        .ok()
        .and_then(|value| {
            value
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
    else {
        return SessionCaptureState::Pending;
    };

    SessionCaptureState::Ready(TerminalSessionRef { kind, id })
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

#[cfg(test)]
mod tests {
    use super::{
        build_command, claude_project_dir_name, claude_session_exists, discover_codex_session,
        discover_codex_session_from_index, discover_codex_session_from_saved_sessions,
        discover_pi_session, discovery_timeout_for_kind, pi_session_capture_extension_path,
        pi_session_exists, prepare_codex_home_override_from, read_session_capture,
        resolve_claude_session, resolve_pi_session, DiscoveryKind, PiSessionCapture,
        SessionCaptureState, TerminalSessionKind, TerminalSessionRef,
    };
    use crate::agents::{AgentProviderKind, HarnessEnv, TerminalLaunchConfig, TerminalLaunchMode};
    use std::env;
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};
    use uuid::Uuid;

    fn temp_capture_path() -> PathBuf {
        env::temp_dir().join(format!("another-one-codex-capture-{}.json", Uuid::new_v4()))
    }

    fn temp_claude_projects_root() -> PathBuf {
        env::temp_dir().join(format!("another-one-claude-projects-{}", Uuid::new_v4()))
    }

    fn temp_pi_sessions_root() -> PathBuf {
        env::temp_dir().join(format!("another-one-pi-sessions-{}", Uuid::new_v4()))
    }

    fn temp_codex_root() -> PathBuf {
        env::temp_dir().join(format!("another-one-codex-root-{}", Uuid::new_v4()))
    }

    fn argv(builder: &portable_pty::CommandBuilder) -> Vec<String> {
        builder
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn assert_command_argv(builder: &portable_pty::CommandBuilder, expected: &[&str]) {
        let argv = argv(builder);
        let command_name = Path::new(&argv[0])
            .file_name()
            .and_then(|name| name.to_str())
            .expect("command should have a file name");
        assert_eq!(command_name, expected[0]);
        assert_eq!(&argv[1..], &expected[1..]);
    }

    #[test]
    fn codex_discovery_timeout_is_long_lived() {
        assert_eq!(
            discovery_timeout_for_kind(&DiscoveryKind::Codex {
                root: PathBuf::from("/tmp/codex-home"),
            }),
            Duration::from_secs(60 * 60)
        );
        assert_eq!(
            discovery_timeout_for_kind(&DiscoveryKind::Pi {
                capture: PiSessionCapture {
                    path: PathBuf::from("/tmp/pi-capture.json"),
                },
            }),
            Duration::from_secs(20)
        );
    }

    #[test]
    fn session_capture_reads_session_id_from_hook_payload() {
        let path = temp_capture_path();
        fs::write(&path, r#"{"session_id":"session-42","source":"startup"}"#)
            .expect("capture file should be writable");

        let state = read_session_capture(&path, TerminalSessionKind::CodexSession);

        assert_eq!(
            state,
            SessionCaptureState::Ready(TerminalSessionRef {
                kind: TerminalSessionKind::CodexSession,
                id: "session-42".to_string(),
            })
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn session_capture_waits_for_complete_hook_payload() {
        let path = temp_capture_path();
        fs::write(&path, r#"{"source":"startup"}"#).expect("capture file should be writable");

        let state = read_session_capture(&path, TerminalSessionKind::CodexSession);

        assert_eq!(state, SessionCaptureState::Pending);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn codex_discovery_prefers_saved_session_with_matching_cwd() {
        let codex_root = temp_codex_root();
        let sessions_dir = codex_root.join("sessions/2026/04/21");
        fs::create_dir_all(&sessions_dir).expect("sessions dir should be created");
        fs::write(
            sessions_dir.join("rollout-2026-04-21T16-19-30-match.jsonl"),
            r#"{"timestamp":"2026-04-21T23:19:37.594Z","type":"session_meta","payload":{"id":"match-session","cwd":"/tmp/project"}}"#,
        )
        .expect("matching session file should be created");
        fs::write(
            sessions_dir.join("rollout-2026-04-21T16-19-30-other.jsonl"),
            r#"{"timestamp":"2026-04-21T23:19:37.594Z","type":"session_meta","payload":{"id":"other-session","cwd":"/tmp/other"}}"#,
        )
        .expect("non-matching session file should be created");

        let session = discover_codex_session_from_saved_sessions(
            SystemTime::now(),
            Path::new("/tmp/project"),
            Some(&codex_root),
        );

        assert_eq!(
            session,
            Some(TerminalSessionRef {
                kind: TerminalSessionKind::CodexSession,
                id: "match-session".to_string(),
            })
        );

        let _ = fs::remove_dir_all(codex_root);
    }

    #[test]
    fn codex_home_override_copies_shared_files_without_hooks() {
        let source_home = temp_codex_root();
        let target_home = temp_codex_root();
        fs::create_dir_all(source_home.join("plugins")).expect("plugins dir should be created");
        fs::create_dir_all(source_home.join("skills")).expect("skills dir should be created");
        fs::create_dir_all(source_home.join("vendor_imports"))
            .expect("vendor imports dir should be created");
        fs::write(source_home.join("config.toml"), "model = \"gpt-5.4\"\n")
            .expect("config should be written");
        fs::write(source_home.join("installation_id"), "install-123")
            .expect("installation id should be written");
        fs::write(source_home.join("version.json"), "{\"version\":\"1\"}")
            .expect("version should be written");
        fs::write(source_home.join("auth.json"), "{\"access_token\":\"abc\"}")
            .expect("auth should be written");
        fs::write(source_home.join("hooks.json"), "{\"hooks\":{}}")
            .expect("hooks should be written");

        prepare_codex_home_override_from(Some(&source_home), &target_home)
            .expect("codex home override should be prepared");

        assert_eq!(
            fs::read_to_string(target_home.join("config.toml")).expect("config should exist"),
            "model = \"gpt-5.4\"\n"
        );
        assert_eq!(
            fs::read_to_string(target_home.join("installation_id"))
                .expect("installation id should exist"),
            "install-123"
        );
        assert_eq!(
            fs::read_to_string(target_home.join("version.json")).expect("version should exist"),
            "{\"version\":\"1\"}"
        );
        assert!(fs::symlink_metadata(target_home.join("auth.json"))
            .expect("auth symlink should exist")
            .file_type()
            .is_symlink());
        assert!(fs::symlink_metadata(target_home.join("plugins"))
            .expect("plugins symlink should exist")
            .file_type()
            .is_symlink());
        assert!(fs::symlink_metadata(target_home.join("skills"))
            .expect("skills symlink should exist")
            .file_type()
            .is_symlink());
        assert!(fs::symlink_metadata(target_home.join("vendor_imports"))
            .expect("vendor imports symlink should exist")
            .file_type()
            .is_symlink());
        assert!(
            !target_home.join("hooks.json").exists(),
            "isolated codex homes should not inherit hooks"
        );

        let _ = fs::remove_dir_all(source_home);
        let _ = fs::remove_dir_all(target_home);
    }

    #[test]
    fn codex_discovery_returns_none_when_isolated_home_has_no_sessions() {
        let codex_root = temp_codex_root();
        fs::create_dir_all(&codex_root).expect("codex root should be created");

        let session = discover_codex_session(
            SystemTime::now(),
            Path::new("/tmp/project"),
            Some(&codex_root),
        );

        assert_eq!(session, None);

        let _ = fs::remove_dir_all(codex_root);
    }

    #[test]
    fn codex_discovery_falls_back_to_session_index_when_sessions_are_missing() {
        let codex_root = temp_codex_root();
        fs::create_dir_all(&codex_root).expect("codex root should be created");
        fs::write(
            codex_root.join("session_index.jsonl"),
            r#"{"id":"older-session","updated_at":"2026-04-21T23:18:00.000Z"}
{"id":"index-session","updated_at":"2026-04-21T23:19:30.000Z"}"#,
        )
        .expect("session index should be created");

        let session = discover_codex_session_from_index(SystemTime::now(), Some(&codex_root));

        assert_eq!(
            session,
            Some(TerminalSessionRef {
                kind: TerminalSessionKind::CodexSession,
                id: "index-session".to_string(),
            })
        );

        let _ = fs::remove_dir_all(codex_root);
    }

    #[test]
    fn pi_discovery_prefers_extension_capture_when_available() {
        let path = temp_capture_path();
        fs::write(&path, r#"{"session_id":"pi-session"}"#)
            .expect("capture file should be writable");

        let session = discover_pi_session(SystemTime::now(), Path::new("/tmp"), Some(&path));

        assert_eq!(
            session,
            Some(TerminalSessionRef {
                kind: TerminalSessionKind::PiSession,
                id: "pi-session".to_string(),
            })
        );
        assert!(
            !path.exists(),
            "discovery should clean up consumed capture files"
        );
    }

    #[test]
    fn pi_session_capture_extension_path_points_to_workspace_script() {
        assert!(pi_session_capture_extension_path().is_file());
    }

    #[test]
    fn pi_session_exists_checks_saved_session_index() {
        let sessions_root = temp_pi_sessions_root();
        let session_dir = sessions_root.join("project");
        fs::create_dir_all(&session_dir).expect("session dir should be created");
        fs::write(
            session_dir.join("session.jsonl"),
            r#"{"type":"session","id":"pi-session","cwd":"/tmp/project"}"#,
        )
        .expect("session file should be created");

        assert!(pi_session_exists(
            Path::new("/tmp/project"),
            "pi-session",
            Some(&sessions_root),
        ));
        assert!(!pi_session_exists(
            Path::new("/tmp/project"),
            "missing-session",
            Some(&sessions_root),
        ));

        let _ = fs::remove_dir_all(sessions_root);
    }

    #[test]
    fn resolve_pi_session_reuses_existing_saved_session() {
        let sessions_root = temp_pi_sessions_root();
        let session_dir = sessions_root.join("project");
        fs::create_dir_all(&session_dir).expect("session dir should be created");
        fs::write(
            session_dir.join("session.jsonl"),
            r#"{"type":"session","id":"pi-session","cwd":"/tmp/project"}"#,
        )
        .expect("session file should be created");
        let requested_session = TerminalSessionRef {
            kind: TerminalSessionKind::PiSession,
            id: "pi-session".to_string(),
        };

        let session = resolve_pi_session(
            Path::new("/tmp/project"),
            Some(&requested_session),
            Some(&sessions_root),
        );

        assert_eq!(session, Some(requested_session));

        let _ = fs::remove_dir_all(sessions_root);
    }

    #[test]
    fn resolve_pi_session_drops_missing_saved_session() {
        let sessions_root = temp_pi_sessions_root();
        fs::create_dir_all(&sessions_root).expect("sessions root should be created");
        let requested_session = TerminalSessionRef {
            kind: TerminalSessionKind::PiSession,
            id: "missing-session".to_string(),
        };

        let session = resolve_pi_session(
            Path::new("/tmp/project"),
            Some(&requested_session),
            Some(&sessions_root),
        );

        assert_eq!(session, None);

        let _ = fs::remove_dir_all(sessions_root);
    }

    #[test]
    fn claude_project_dir_name_replaces_non_alphanumeric_characters() {
        assert_eq!(
            claude_project_dir_name(Path::new("/Users/jeff.f/webz/another-one")),
            "-Users-jeff-f-webz-another-one"
        );
    }

    #[test]
    fn claude_session_exists_checks_expected_project_directory_first() {
        let projects_root = temp_claude_projects_root();
        let cwd = Path::new("/tmp/my.repo");
        let project_dir = projects_root.join(claude_project_dir_name(cwd));
        fs::create_dir_all(&project_dir).expect("project dir should be created");
        fs::write(project_dir.join("session-42.jsonl"), "{}")
            .expect("session file should be created");

        assert!(claude_session_exists(
            cwd,
            "session-42",
            Some(&projects_root)
        ));
        assert!(!claude_session_exists(cwd, "missing", Some(&projects_root)));

        let _ = fs::remove_dir_all(projects_root);
    }

    #[test]
    fn claude_session_exists_falls_back_to_scanning_other_project_directories() {
        let projects_root = temp_claude_projects_root();
        let other_project_dir = projects_root.join("other-project");
        fs::create_dir_all(&other_project_dir).expect("project dir should be created");
        fs::write(other_project_dir.join("session-99.jsonl"), "{}")
            .expect("session file should be created");

        assert!(claude_session_exists(
            Path::new("/tmp/current-project"),
            "session-99",
            Some(&projects_root),
        ));

        let _ = fs::remove_dir_all(projects_root);
    }

    #[test]
    fn resolve_claude_session_reuses_existing_session_when_file_exists() {
        let projects_root = temp_claude_projects_root();
        let cwd = Path::new("/tmp/project");
        let requested_session = TerminalSessionRef {
            kind: TerminalSessionKind::ClaudeSession,
            id: "session-123".to_string(),
        };
        let project_dir = projects_root.join(claude_project_dir_name(cwd));
        fs::create_dir_all(&project_dir).expect("project dir should be created");
        fs::write(project_dir.join("session-123.jsonl"), "{}")
            .expect("session file should be created");

        let (session, should_resume) =
            resolve_claude_session(cwd, Some(&requested_session), Some(&projects_root));

        assert!(should_resume);
        assert_eq!(session, requested_session);

        let _ = fs::remove_dir_all(projects_root);
    }

    #[test]
    fn resolve_claude_session_generates_new_session_when_saved_one_is_missing() {
        let projects_root = temp_claude_projects_root();
        fs::create_dir_all(&projects_root).expect("projects root should be created");
        let requested_session = TerminalSessionRef {
            kind: TerminalSessionKind::ClaudeSession,
            id: "missing-session".to_string(),
        };

        let (session, should_resume) = resolve_claude_session(
            Path::new("/tmp/project"),
            Some(&requested_session),
            Some(&projects_root),
        );

        assert!(!should_resume);
        assert_eq!(session.kind, TerminalSessionKind::ClaudeSession);
        assert_ne!(session.id, requested_session.id);

        let _ = fs::remove_dir_all(projects_root);
    }

    #[test]
    fn build_command_injects_codex_args_before_fresh_session_setup() {
        let cwd = Path::new("/tmp/project");
        let codex_root = temp_codex_root();
        fs::create_dir_all(&codex_root).expect("codex root should be created");
        let launch_config = TerminalLaunchConfig::for_provider(AgentProviderKind::Codex)
            .with_home_override(Some(codex_root.clone()));
        let agent_launch_args = vec!["--yolo".to_string(), "--profile".to_string()];

        let (builder, launch_config, discovery) = build_command(
            &HarnessEnv::for_test(),
            cwd,
            launch_config,
            &agent_launch_args,
        )
        .expect("build should succeed");

        assert_command_argv(&builder, &["codex", "--yolo", "--profile"]);
        assert_eq!(launch_config.home_override, Some(codex_root.clone()));
        assert!(launch_config.session.is_none());
        assert!(matches!(
            discovery,
            Some(DiscoveryKind::Codex { root }) if root == codex_root
        ));

        let _ = fs::remove_dir_all(codex_root);
    }

    #[test]
    fn build_command_injects_codex_args_before_resume_args() {
        let cwd = Path::new("/tmp/project");
        let session = TerminalSessionRef {
            kind: TerminalSessionKind::CodexSession,
            id: "codex-session".to_string(),
        };
        let launch_config = TerminalLaunchConfig::for_provider(AgentProviderKind::Codex)
            .with_session(Some(session.clone()));
        let agent_launch_args = vec!["--yolo".to_string()];

        let (builder, launch_config, discovery) = build_command(
            &HarnessEnv::for_test(),
            cwd,
            launch_config,
            &agent_launch_args,
        )
        .expect("build should succeed");

        assert_command_argv(&builder, &["codex", "--yolo", "resume", "codex-session"]);
        assert_eq!(launch_config.session, Some(session));
        assert!(discovery.is_none());
    }

    #[test]
    fn build_command_injects_claude_args_for_new_session() {
        let cwd = Path::new("/tmp/project");
        let launch_config = TerminalLaunchConfig::for_provider(AgentProviderKind::ClaudeCode);
        let agent_launch_args = vec!["--dangerously-skip-permissions".to_string()];

        let (builder, launch_config, discovery) = build_command(
            &HarnessEnv::for_test(),
            cwd,
            launch_config,
            &agent_launch_args,
        )
        .expect("build should succeed");

        let argv = argv(&builder);
        assert_eq!(
            Path::new(&argv[0])
                .file_name()
                .and_then(|name| name.to_str()),
            Some("claude")
        );
        assert_eq!(argv[1], "--dangerously-skip-permissions");
        assert_eq!(argv[2], "--session-id");
        assert_eq!(argv.len(), 4);
        assert!(launch_config.session.is_some());
        assert!(discovery.is_none());
    }

    #[test]
    fn build_command_injects_claude_args_for_resume() {
        let projects_root = temp_claude_projects_root();
        let cwd = Path::new("/tmp/project");
        let session = TerminalSessionRef {
            kind: TerminalSessionKind::ClaudeSession,
            id: "session-123".to_string(),
        };
        let project_dir = projects_root.join(claude_project_dir_name(cwd));
        fs::create_dir_all(&project_dir).expect("project dir should be created");
        fs::write(project_dir.join("session-123.jsonl"), "{}")
            .expect("session file should be created");

        let launch_config = TerminalLaunchConfig::for_provider(AgentProviderKind::ClaudeCode)
            .with_session(Some(session));
        let agent_launch_args = vec!["--dangerously-skip-permissions".to_string()];
        let env = HarnessEnv {
            claude_projects_root: Some(projects_root.clone()),
            ..HarnessEnv::for_test()
        };

        let (builder, _, discovery) = build_command(&env, cwd, launch_config, &agent_launch_args)
            .expect("build should succeed");

        assert_command_argv(
            &builder,
            &[
                "claude",
                "--dangerously-skip-permissions",
                "--resume",
                "session-123",
            ],
        );
        assert!(discovery.is_none());

        let _ = fs::remove_dir_all(projects_root);
    }

    #[test]
    fn build_command_injects_cursor_args_before_resume() {
        let cwd = Path::new("/tmp/project");
        let session = TerminalSessionRef {
            kind: TerminalSessionKind::CursorChat,
            id: "cursor-chat".to_string(),
        };
        let launch_config = TerminalLaunchConfig::for_provider(AgentProviderKind::CursorAgent)
            .with_session(Some(session.clone()));
        let agent_launch_args = vec!["--model".to_string(), "sonnet".to_string()];

        let (builder, launch_config, discovery) = build_command(
            &HarnessEnv::for_test(),
            cwd,
            launch_config,
            &agent_launch_args,
        )
        .expect("build should succeed");

        assert_command_argv(
            &builder,
            &["agent", "--model", "sonnet", "--resume", "cursor-chat"],
        );
        assert_eq!(launch_config.session, Some(session));
        assert!(discovery.is_none());
    }

    #[test]
    fn build_command_injects_generic_provider_args_before_session() {
        let cwd = Path::new("/tmp/project");
        let session = TerminalSessionRef {
            kind: TerminalSessionKind::PiSession,
            id: "resume-me".to_string(),
        };
        let launch_config = TerminalLaunchConfig::for_provider(AgentProviderKind::Forge)
            .with_session(Some(session));
        let agent_launch_args = vec!["--log-level".to_string(), "debug".to_string()];

        let (builder, _, discovery) = build_command(
            &HarnessEnv::for_test(),
            cwd,
            launch_config,
            &agent_launch_args,
        )
        .expect("build should succeed");

        assert_command_argv(&builder, &["forge", "--log-level", "debug", "resume-me"]);
        assert!(discovery.is_none());
    }

    #[test]
    fn build_command_ignores_agent_args_for_raw_shell() {
        let cwd = Path::new("/tmp/project");
        let launch_config = TerminalLaunchConfig {
            mode: TerminalLaunchMode::RawShell,
            ..TerminalLaunchConfig::default()
        };
        let agent_launch_args = vec!["--ignored".to_string()];

        let (builder, _, discovery) = build_command(
            &HarnessEnv::for_test(),
            cwd,
            launch_config,
            &agent_launch_args,
        )
        .expect("build should succeed");

        assert!(builder.is_default_prog());
        assert!(builder.get_argv().is_empty());
        assert!(discovery.is_none());
    }
}
