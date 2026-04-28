use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{point_to_viewport, viewport_to_point, Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{self, Color, CursorShape, NamedColor, Rgb};
use anyhow::Context;
use daemon_sandbox::frame::{
    self, Control, ControlEnvelope, TerminalInputEvent, WorkerReply, WorkerReplyEnvelope,
};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use tokio::sync::mpsc;
use tokio::time::Instant;

slint::include_modules!();

mod platform;
mod style;

const TERMINAL_COLS: u16 = 100;
const TERMINAL_ROWS: u16 = 34;
const RETRY_DELAY: Duration = Duration::from_secs(1);
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const TERMINAL_FRAME_INTERVAL: Duration = Duration::from_millis(33);
const DEFAULT_TERMINAL_BACKGROUND_RGB: u32 = 0x17191d;
const DEFAULT_TERMINAL_FOREGROUND_RGB: u32 = 0xd7dae0;
const PROJECT_ACCENTS: [u32; 8] = [
    0x5b4a9e, 0x2e7d6f, 0xb85c38, 0x3a6ea5, 0x8b5e3c, 0x7b2d5f, 0x4a7c4b, 0x9c5151,
];
const SHELL_COLOR_SMOKE_PROBE: &[u8] =
    b"printf '\\033[31mRED \\033[32mGREEN \\033[34mBLUE\\033[0m DEFAULT\\n'\nprintf 'ANOTHERONE_SLINT_READY\\n'\r";
const SHELL_READINESS_PROBE: &[u8] = b"printf 'ANOTHERONE_SLINT_READY\\n'\r";

pub fn run_app() -> Result<(), slint::PlatformError> {
    let app = AppWindow::new()?;
    let platform_profile = platform::current_platform_profile();
    #[cfg(not(target_os = "android"))]
    slint::set_xdg_app_id(platform_profile.app_id)?;
    style::apply_theme(&app);
    app.set_platform_label(platform_profile.label().into());
    seed_shell_model(&app);

    let (client_event_tx, client_event_rx) = mpsc::unbounded_channel::<SlintClientEvent>();
    let terminal_event_tx = client_event_tx.clone();
    app.on_terminal_key(move |text, control, alt, _shift| {
        let _ = terminal_event_tx.send(SlintClientEvent::TerminalKey(SlintKeyEvent {
            text: text.to_string(),
            control,
            alt,
        }));
    });
    let project_event_tx = client_event_tx.clone();
    app.on_project_selected(move |project_id| {
        let _ = project_event_tx.send(SlintClientEvent::SelectProject(project_id.to_string()));
    });
    let task_event_tx = client_event_tx.clone();
    app.on_task_selected(move |task_id| {
        let _ = task_event_tx.send(SlintClientEvent::SelectTask(task_id.to_string()));
    });
    let tab_event_tx = client_event_tx.clone();
    app.on_tab_selected(move |tab_id| {
        let _ = tab_event_tx.send(SlintClientEvent::SelectTab(tab_id.to_string()));
    });
    let submit_event_tx = client_event_tx.clone();
    app.on_submit_new_task(move |task_name, source_branch| {
        let _ = submit_event_tx.send(SlintClientEvent::SubmitNewTask {
            task_name: task_name.to_string(),
            source_branch: source_branch.to_string(),
        });
    });
    let dismiss_toast_app = app.as_weak();
    app.on_toast_dismissed(move || {
        clear_toast(&dismiss_toast_app);
    });
    let copy_toast_app = app.as_weak();
    app.on_toast_copy_requested(move || {
        set_toast(
            &copy_toast_app,
            "info",
            "Notification copy is not wired yet",
            "Toast details remain visible for manual copy.",
        );
    });

    spawn_terminal_worker(app.as_weak(), client_event_rx);
    app.on_close_requested(|| std::process::exit(0));
    app.run()
}

#[cfg(target_os = "android")]
#[no_mangle]
pub fn android_main(app: slint::android::AndroidApp) {
    if let Err(error) = slint::android::init(app) {
        eprintln!("AnotherOne Slint android backend init failed: {error}");
        return;
    }

    if let Err(error) = run_app() {
        eprintln!("AnotherOne Slint android startup failed: {error}");
    }
}

fn seed_shell_model(app: &AppWindow) {
    app.set_project_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        ProjectSidebarEntry {
            id: "another-one".into(),
            name: "another-one".into(),
            path: "~/.another-one/worktrees/another-one".into(),
            branch: "slint-daemon-poc-clean".into(),
            initials: "A".into(),
            accent: project_accent_color("another-one"),
            active: true,
            task_count_label: "3".into(),
        },
        ProjectSidebarEntry {
            id: "daemon-sandbox".into(),
            name: "daemon-sandbox".into(),
            path: "daemon-sandbox".into(),
            branch: "daemon transport".into(),
            initials: "D".into(),
            accent: project_accent_color("daemon-sandbox"),
            active: false,
            task_count_label: "1".into(),
        },
        ProjectSidebarEntry {
            id: "slint-platform".into(),
            name: "slint-platform".into(),
            path: "slint-platform".into(),
            branch: "platform traits".into(),
            initials: "S".into(),
            accent: project_accent_color("slint-platform"),
            active: false,
            task_count_label: "2".into(),
        },
    ])));
    app.set_task_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        TaskSidebarEntry {
            id: "slint-build".into(),
            title: "Slint build".into(),
            branch: "slint-daemon-poc-clean".into(),
            metadata: "active | +0 -0".into(),
            initials: "S".into(),
            accent: project_accent_color("slint-build"),
            active: true,
            pinned: true,
            running: true,
        },
        TaskSidebarEntry {
            id: "terminal-ready".into(),
            title: "Terminal readiness".into(),
            branch: "terminal-production".into(),
            metadata: "in progress | renderer".into(),
            initials: "T".into(),
            accent: project_accent_color("terminal-ready"),
            active: false,
            pinned: false,
            running: false,
        },
        TaskSidebarEntry {
            id: "style-system".into(),
            title: "Style system".into(),
            branch: "gpui baseline".into(),
            metadata: "blocked | visual corpus".into(),
            initials: "G".into(),
            accent: project_accent_color("style-system"),
            active: false,
            pinned: false,
            running: false,
        },
    ])));
    app.set_tab_chips(slint::ModelRc::new(slint::VecModel::from(vec![
        TerminalTabChip {
            id: "main".into(),
            title: "Codex".into(),
            provider: "codex".into(),
            active: true,
            running: true,
            pinned: false,
        },
        TerminalTabChip {
            id: "shell".into(),
            title: "Shell".into(),
            provider: "shell".into(),
            active: false,
            running: false,
            pinned: false,
        },
    ])));
}

#[derive(Default)]
struct WorkspaceShellModel {
    project_rows: Vec<ProjectSidebarEntry>,
    task_rows: Vec<TaskSidebarEntry>,
    tab_chips: Vec<TerminalTabChip>,
    active_project_name: String,
    active_task_name: String,
    active_branch_name: String,
    active_worktree_name: String,
    active_project_path: String,
    project_summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalTarget {
    section_id: String,
    tab_id: String,
}

fn set_workspace_tree(
    app_weak: &slint::Weak<AppWindow>,
    projects: &[frame::ProjectSummary],
    active_section_id: &str,
    active_tab_id: &str,
) {
    let model = workspace_shell_model(projects, active_section_id, active_tab_id);
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_project_rows(slint::ModelRc::new(slint::VecModel::from(
                model.project_rows,
            )));
            app.set_task_rows(slint::ModelRc::new(slint::VecModel::from(model.task_rows)));
            app.set_tab_chips(slint::ModelRc::new(slint::VecModel::from(model.tab_chips)));
            app.set_active_project_name(model.active_project_name.into());
            app.set_active_task_name(model.active_task_name.into());
            app.set_active_branch_name(model.active_branch_name.into());
            app.set_active_worktree_name(model.active_worktree_name.into());
            app.set_active_project_path(model.active_project_path.into());
            app.set_project_summary(model.project_summary.into());
        }
    });
}

fn set_toast(
    app_weak: &slint::Weak<AppWindow>,
    kind: impl Into<String>,
    message: impl Into<String>,
    detail: impl Into<String>,
) {
    let app_weak = app_weak.clone();
    let kind = kind.into();
    let message = message.into();
    let detail = detail.into();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_toast_kind(kind.into());
            app.set_toast_message(message.into());
            app.set_toast_detail(detail.into());
        }
    });
}

fn clear_toast(app_weak: &slint::Weak<AppWindow>) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_toast_kind("info".into());
            app.set_toast_message("".into());
            app.set_toast_detail("".into());
        }
    });
}

fn workspace_shell_model(
    projects: &[frame::ProjectSummary],
    active_section_id: &str,
    active_tab_id: &str,
) -> WorkspaceShellModel {
    let active_project = projects
        .iter()
        .find(|project| {
            project
                .tasks
                .iter()
                .any(|task| task.section_id == active_section_id)
        })
        .or_else(|| projects.first());
    let active_task = active_project.and_then(|project| {
        project
            .tasks
            .iter()
            .find(|task| task.section_id == active_section_id)
            .or_else(|| project.tasks.first())
    });

    let project_rows = projects
        .iter()
        .take(3)
        .map(|project| {
            let active = project
                .tasks
                .iter()
                .any(|task| task.section_id == active_section_id);
            ProjectSidebarEntry {
                id: project.id.clone().into(),
                name: project.name.clone().into(),
                path: compact_path(&project.path).into(),
                branch: project
                    .current_branch
                    .as_deref()
                    .unwrap_or_else(|| project_kind_label(project.kind))
                    .into(),
                initials: initials(&project.name).into(),
                accent: project_accent_color(&project.id),
                active,
                task_count_label: project.tasks.len().to_string().into(),
            }
        })
        .collect::<Vec<_>>();

    let mut task_entries = projects
        .iter()
        .flat_map(|project| {
            project.tasks.iter().map(move |task| {
                let running = task.tabs.iter().any(|tab| tab.running);
                TaskSidebarEntry {
                    id: task.id.clone().into(),
                    title: task.name.clone().into(),
                    branch: task.branch_name.clone().into(),
                    metadata: task_metadata(task, running).into(),
                    initials: initials(&task.name).into(),
                    accent: project_accent_color(&project.id),
                    active: task.section_id == active_section_id,
                    pinned: task.pinned,
                    running,
                }
            })
        })
        .collect::<Vec<_>>();
    task_entries.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| right.pinned.cmp(&left.pinned))
            .then_with(|| left.title.cmp(&right.title))
    });
    task_entries.truncate(7);

    let tab_chips = active_task
        .map(|task| {
            task.tabs
                .iter()
                .take(5)
                .map(|tab| TerminalTabChip {
                    id: tab.id.clone().into(),
                    title: tab
                        .fixed_title
                        .as_deref()
                        .unwrap_or(tab.title.as_str())
                        .to_string()
                        .into(),
                    provider: tab.provider.map(provider_label).unwrap_or("shell").into(),
                    active: tab.id == active_tab_id,
                    running: tab.running,
                    pinned: tab.pinned,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    WorkspaceShellModel {
        project_rows,
        task_rows: task_entries,
        tab_chips,
        active_project_name: active_project
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "another-one".to_string()),
        active_task_name: active_task
            .map(|task| task.name.clone())
            .unwrap_or_else(|| "No active task".to_string()),
        active_branch_name: active_task
            .map(|task| task.branch_name.clone())
            .or_else(|| active_project.and_then(|project| project.current_branch.clone()))
            .unwrap_or_else(|| "detached".to_string()),
        active_worktree_name: active_project
            .map(|project| worktree_name(&project.path))
            .unwrap_or_else(|| "workspace".to_string()),
        active_project_path: active_project
            .map(|project| project.path.clone())
            .unwrap_or_default(),
        project_summary: format!("{} projects", projects.len()),
    }
}

fn first_attachable_target(projects: &[frame::ProjectSummary]) -> Option<TerminalTarget> {
    projects
        .iter()
        .find_map(|project| project.tasks.iter().find_map(target_for_task))
}

fn target_for_project_id(
    projects: &[frame::ProjectSummary],
    project_id: &str,
) -> Option<TerminalTarget> {
    projects
        .iter()
        .find(|project| project.id == project_id)
        .and_then(|project| project.tasks.iter().find_map(target_for_task))
}

fn target_for_task_id(projects: &[frame::ProjectSummary], task_id: &str) -> Option<TerminalTarget> {
    projects
        .iter()
        .flat_map(|project| &project.tasks)
        .find(|task| task.id == task_id)
        .and_then(target_for_task)
}

fn target_for_tab_id(
    projects: &[frame::ProjectSummary],
    active_section_id: &str,
    tab_id: &str,
) -> Option<TerminalTarget> {
    projects
        .iter()
        .flat_map(|project| &project.tasks)
        .find(|task| {
            task.section_id == active_section_id && task.tabs.iter().any(|tab| tab.id == tab_id)
        })
        .or_else(|| {
            projects
                .iter()
                .flat_map(|project| &project.tasks)
                .find(|task| task.tabs.iter().any(|tab| tab.id == tab_id))
        })
        .map(|task| TerminalTarget {
            section_id: task.section_id.clone(),
            tab_id: tab_id.to_string(),
        })
}

fn target_still_exists(projects: &[frame::ProjectSummary], target: &TerminalTarget) -> bool {
    projects.iter().any(|project| {
        project.tasks.iter().any(|task| {
            task.section_id == target.section_id
                && task.tabs.iter().any(|tab| tab.id == target.tab_id)
        })
    })
}

fn project_id_for_target(
    projects: &[frame::ProjectSummary],
    target: &TerminalTarget,
) -> Option<String> {
    task_project_for_target(projects, target).map(|(project, task)| {
        if task.target_project_id.is_empty() {
            project.id.clone()
        } else {
            task.target_project_id.clone()
        }
    })
}

fn normalized_source_branch(
    projects: &[frame::ProjectSummary],
    target: &TerminalTarget,
    requested_branch: &str,
) -> Option<String> {
    let requested_branch = requested_branch.trim();
    if !requested_branch.is_empty() {
        return Some(requested_branch.to_string());
    }

    task_project_for_target(projects, target).and_then(|(project, task)| {
        if !task.branch_name.is_empty() {
            Some(task.branch_name.clone())
        } else {
            project.current_branch.clone()
        }
    })
}

fn task_project_for_target<'a>(
    projects: &'a [frame::ProjectSummary],
    target: &TerminalTarget,
) -> Option<(&'a frame::ProjectSummary, &'a frame::TaskSummary)> {
    projects.iter().find_map(|project| {
        project
            .tasks
            .iter()
            .find(|task| task.section_id == target.section_id)
            .map(|task| (project, task))
    })
}

fn target_for_task(task: &frame::TaskSummary) -> Option<TerminalTarget> {
    let tab = task
        .tabs
        .iter()
        .find(|tab| tab.id == task.active_tab_id)
        .or_else(|| task.tabs.first())?;
    Some(TerminalTarget {
        section_id: task.section_id.clone(),
        tab_id: tab.id.clone(),
    })
}

fn project_kind_label(kind: frame::ProjectKind) -> &'static str {
    match kind {
        frame::ProjectKind::Root => "root",
        frame::ProjectKind::Worktree => "worktree",
    }
}

fn task_metadata(task: &frame::TaskSummary, running: bool) -> String {
    let mut parts = Vec::new();
    if !task.last_commit_relative.is_empty() {
        parts.push(task.last_commit_relative.clone());
    }
    if task.lines_added != 0 || task.lines_removed != 0 {
        parts.push(format!("+{} -{}", task.lines_added, task.lines_removed));
    }
    parts.push(if running { "running" } else { "idle" }.to_string());
    parts.join(" | ")
}

fn provider_label(provider: frame::AgentProvider) -> &'static str {
    match provider {
        frame::AgentProvider::ClaudeCode => "claude-code",
        frame::AgentProvider::CursorAgent => "cursor-agent",
        frame::AgentProvider::Codex => "codex",
        frame::AgentProvider::Pi => "pi",
        frame::AgentProvider::Gemini => "gemini",
        frame::AgentProvider::OpenCode => "opencode",
        frame::AgentProvider::Amp => "amp",
        frame::AgentProvider::RovoDev => "rovo-dev",
        frame::AgentProvider::Forge => "forge",
        frame::AgentProvider::Shell => "shell",
    }
}

fn compact_path(path: &str) -> String {
    let mut parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .rev()
        .take(3)
        .collect::<Vec<_>>();
    parts.reverse();
    if parts.is_empty() {
        path.to_string()
    } else {
        format!(".../{}", parts.join("/"))
    }
}

fn worktree_name(path: &str) -> String {
    path.rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("workspace")
        .to_string()
}

fn initials(label: &str) -> String {
    label
        .chars()
        .find(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase().to_string())
        .unwrap_or_else(|| "A".to_string())
}

fn project_accent_color(id: &str) -> slint::Color {
    let hash = id.bytes().fold(0_u32, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(byte as u32)
    });
    let color = PROJECT_ACCENTS[(hash as usize) % PROJECT_ACCENTS.len()];
    slint::Color::from_argb_encoded(0xff000000 | color)
}

fn spawn_terminal_worker(
    app_weak: slint::Weak<AppWindow>,
    mut client_event_rx: mpsc::UnboundedReceiver<SlintClientEvent>,
) {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                set_terminal_status(
                    &app_weak,
                    format!("terminal: tokio runtime failed: {error}"),
                );
                return;
            }
        };

        runtime.block_on(async move {
            loop {
                if client_event_rx.is_closed() {
                    break;
                }

                if let Err(error) = run_terminal_session(&app_weak, &mut client_event_rx).await {
                    set_terminal_status(&app_weak, format!("terminal: {error:#}; retrying"));
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        });
    });
}

async fn run_terminal_session(
    app_weak: &slint::Weak<AppWindow>,
    client_event_rx: &mut mpsc::UnboundedReceiver<SlintClientEvent>,
) -> anyhow::Result<()> {
    set_terminal_status(app_weak, "terminal: loading /tmp/daemon-sandbox.ticket");
    let (endpoint_id, direct_addrs) = wait_for_ticket(app_weak).await?;

    set_terminal_status(app_weak, "terminal: binding local iroh endpoint");
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("bind client endpoint")?;

    pre_authorize_local_client(endpoint.id())?;

    let mut addr = EndpointAddr::new(endpoint_id);
    for direct_addr in direct_addrs {
        addr = addr.with_ip_addr(direct_addr);
    }

    set_terminal_status(app_weak, "terminal: dialing daemon-sandbox over iroh");
    let conn = endpoint
        .connect(addr, daemon_sandbox::transport_iroh::ALPN)
        .await
        .context("connect to daemon-sandbox")?;
    let (mut send, mut recv) = conn.open_bi().await.context("open bidi stream")?;

    let mut next_request_id = 1_u64;
    send_control(&mut send, &mut next_request_id, Control::ListProjects).await?;
    set_terminal_status(app_weak, "terminal: requesting sandbox project tree");
    let (mut projects, mut attached_target) = loop {
        let Some((ty, payload)) = tokio::time::timeout(FRAME_TIMEOUT, frame::read_frame(&mut recv))
            .await
            .context("timed out waiting for project list")??
        else {
            anyhow::bail!("daemon closed before project list");
        };

        if ty != frame::TY_WORKER_REPLY {
            continue;
        }

        let envelope: WorkerReplyEnvelope =
            serde_json::from_slice(&payload).context("decode worker reply")?;
        match envelope.reply {
            WorkerReply::ProjectList { projects } => {
                let Some(target) = first_attachable_target(&projects) else {
                    anyhow::bail!("daemon-sandbox returned no attachable task tabs");
                };
                set_workspace_tree(app_weak, &projects, &target.section_id, &target.tab_id);
                break (projects, target);
            }
            WorkerReply::Err { message, .. } => anyhow::bail!("list_projects failed: {message}"),
            _ => {}
        }
    };

    attach_terminal_target(&mut send, &mut next_request_id, &attached_target, true).await?;
    if let Some(probe) = startup_probe() {
        send_terminal_input(
            &mut send,
            &mut next_request_id,
            TerminalInputEvent::Key {
                bytes: probe.to_vec(),
            },
        )
        .await
        .context("send startup probe")?;
    }

    set_terminal_status(
        app_weak,
        format!(
            "terminal: attached {}/{} at {TERMINAL_COLS}x{TERMINAL_ROWS}",
            attached_target.section_id, attached_target.tab_id
        ),
    );

    let mut terminal = AlacrittySnapshot::new(TERMINAL_COLS, TERMINAL_ROWS);
    let mut dirty = true;
    let mut last_flush = Instant::now()
        .checked_sub(TERMINAL_FRAME_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut pending_flush_at = Some(Instant::now());

    loop {
        tokio::select! {
            maybe_event = client_event_rx.recv() => {
                let Some(event) = maybe_event else {
                    anyhow::bail!("client event channel closed");
                };
                match event {
                    SlintClientEvent::TerminalKey(input) => {
                        if let Some(event) = encode_terminal_key(&input, terminal.input_modes()) {
                            send_terminal_input(&mut send, &mut next_request_id, event)
                                .await
                                .context("send terminal input")?;
                        }
                    }
                    SlintClientEvent::SelectProject(project_id) => {
                        if let Some(target) = target_for_project_id(&projects, &project_id) {
                            switch_terminal_target(
                                app_weak,
                                &mut send,
                                &mut next_request_id,
                                &projects,
                                &mut attached_target,
                                target,
                                &mut terminal,
                                &mut dirty,
                                &mut pending_flush_at,
                            )
                            .await?;
                        } else {
                            set_terminal_status(app_weak, format!("terminal: project has no attachable tab: {project_id}"));
                        }
                    }
                    SlintClientEvent::SelectTask(task_id) => {
                        if let Some(target) = target_for_task_id(&projects, &task_id) {
                            switch_terminal_target(
                                app_weak,
                                &mut send,
                                &mut next_request_id,
                                &projects,
                                &mut attached_target,
                                target,
                                &mut terminal,
                                &mut dirty,
                                &mut pending_flush_at,
                            )
                            .await?;
                        } else {
                            set_terminal_status(app_weak, format!("terminal: task has no attachable tab: {task_id}"));
                        }
                    }
                    SlintClientEvent::SelectTab(tab_id) => {
                        if let Some(target) = target_for_tab_id(&projects, &attached_target.section_id, &tab_id) {
                            switch_terminal_target(
                                app_weak,
                                &mut send,
                                &mut next_request_id,
                                &projects,
                                &mut attached_target,
                                target,
                                &mut terminal,
                                &mut dirty,
                                &mut pending_flush_at,
                            )
                            .await?;
                        } else {
                            set_terminal_status(app_weak, format!("terminal: unknown tab: {tab_id}"));
                        }
                    }
                    SlintClientEvent::SubmitNewTask { task_name, source_branch } => {
                        let task_name = task_name.trim().to_string();
                        if task_name.is_empty() {
                            set_toast(app_weak, "error", "Task name is required", "Enter a task name before creating a task.");
                            continue;
                        }
                        let Some(project_id) = project_id_for_target(&projects, &attached_target) else {
                            set_toast(app_weak, "error", "No active project", "Select a daemon-backed project before creating a task.");
                            continue;
                        };
                        let Some(source_branch) = normalized_source_branch(&projects, &attached_target, &source_branch) else {
                            set_toast(app_weak, "error", "No source branch", "Enter a source branch before creating a worktree task.");
                            continue;
                        };
                        send_control(
                            &mut send,
                            &mut next_request_id,
                            Control::SubmitNewTask {
                                project_id,
                                task_name: task_name.clone(),
                                source_branch: source_branch.clone(),
                                agent_ids: Vec::new(),
                                branch_mode_existing: false,
                                worktree_mode: true,
                            },
                        )
                        .await
                        .context("submit new task")?;
                        set_toast(
                            app_weak,
                            "info",
                            format!("Creating task {task_name}"),
                            format!("Source branch: {source_branch}"),
                        );
                    }
                }
            }
            frame = frame::read_frame(&mut recv) => {
                let Some((ty, payload)) = frame.context("read daemon frame")? else {
                    anyhow::bail!("daemon closed terminal stream");
                };
                match ty {
                    frame::TY_DATA => {
                        let replies = terminal.apply_output(&payload);
                        for reply in replies {
                            send_terminal_input(
                                &mut send,
                                &mut next_request_id,
                                TerminalInputEvent::PtyReply { bytes: reply },
                            )
                                .await
                                .context("send terminal protocol reply")?;
                        }
                        dirty = true;
                        if pending_flush_at.is_none() {
                            pending_flush_at = Some(next_terminal_flush_deadline(Instant::now(), last_flush));
                        }
                    }
                    frame::TY_WORKER_REPLY => {
                        if let Ok(envelope) = serde_json::from_slice::<WorkerReplyEnvelope>(&payload) {
                            match envelope.reply {
                                WorkerReply::ProjectList { projects: latest_projects } => {
                                    projects = latest_projects;
                                    if target_still_exists(&projects, &attached_target) {
                                        set_workspace_tree(
                                            app_weak,
                                            &projects,
                                            &attached_target.section_id,
                                            &attached_target.tab_id,
                                        );
                                    } else if let Some(target) = first_attachable_target(&projects) {
                                        switch_terminal_target(
                                            app_weak,
                                            &mut send,
                                            &mut next_request_id,
                                            &projects,
                                            &mut attached_target,
                                            target,
                                            &mut terminal,
                                            &mut dirty,
                                            &mut pending_flush_at,
                                        )
                                        .await?;
                                    } else {
                                        set_terminal_status(app_weak, "terminal: project tree has no attachable tabs");
                                    }
                                }
                                WorkerReply::Err { message, .. } => {
                                    set_terminal_status(app_weak, format!("terminal worker error: {message}"));
                                    set_toast(app_weak, "error", "Daemon request failed", message);
                                }
                                WorkerReply::SubmitNewTaskAck { section_id } => {
                                    let target = TerminalTarget {
                                        section_id,
                                        tab_id: "0".to_string(),
                                    };
                                    attach_terminal_target(&mut send, &mut next_request_id, &target, true).await?;
                                    attached_target = target;
                                    terminal = AlacrittySnapshot::new(TERMINAL_COLS, TERMINAL_ROWS);
                                    set_terminal_surface(app_weak, terminal.snapshot_surface());
                                    dirty = false;
                                    pending_flush_at = None;
                                    send_control(&mut send, &mut next_request_id, Control::ListProjects).await?;
                                    set_toast(app_weak, "success", "Task created", "Attached to the new task terminal.");
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ = wait_for_terminal_flush(pending_flush_at), if pending_flush_at.is_some() => {
                if dirty {
                    set_terminal_surface(app_weak, terminal.snapshot_surface());
                    dirty = false;
                }
                last_flush = Instant::now();
                pending_flush_at = None;
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SlintKeyEvent {
    text: String,
    control: bool,
    alt: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SlintClientEvent {
    TerminalKey(SlintKeyEvent),
    SelectProject(String),
    SelectTask(String),
    SelectTab(String),
    SubmitNewTask {
        task_name: String,
        source_branch: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TerminalInputModeState {
    app_cursor: bool,
    bracketed_paste: bool,
}

async fn wait_for_terminal_flush(deadline: Option<Instant>) {
    let Some(deadline) = deadline else {
        std::future::pending::<()>().await;
        return;
    };

    tokio::time::sleep_until(deadline).await;
}

fn next_terminal_flush_deadline(now: Instant, last_flush: Instant) -> Instant {
    let earliest = last_flush + TERMINAL_FRAME_INTERVAL;
    if earliest > now {
        earliest
    } else {
        now
    }
}

async fn switch_terminal_target<W>(
    app_weak: &slint::Weak<AppWindow>,
    send: &mut W,
    next_request_id: &mut u64,
    projects: &[frame::ProjectSummary],
    current_target: &mut TerminalTarget,
    next_target: TerminalTarget,
    terminal: &mut AlacrittySnapshot,
    dirty: &mut bool,
    pending_flush_at: &mut Option<Instant>,
) -> anyhow::Result<()>
where
    W: frame::WriteAllAsync + Unpin,
{
    set_workspace_tree(
        app_weak,
        projects,
        &next_target.section_id,
        &next_target.tab_id,
    );

    if *current_target == next_target {
        return Ok(());
    }

    attach_terminal_target(send, next_request_id, &next_target, true).await?;
    *current_target = next_target;
    *terminal = AlacrittySnapshot::new(TERMINAL_COLS, TERMINAL_ROWS);
    set_terminal_surface(app_weak, terminal.snapshot_surface());
    *dirty = false;
    *pending_flush_at = None;
    set_terminal_status(
        app_weak,
        format!(
            "terminal: attached {}/{} at {TERMINAL_COLS}x{TERMINAL_ROWS}",
            current_target.section_id, current_target.tab_id
        ),
    );

    Ok(())
}

async fn attach_terminal_target<W>(
    send: &mut W,
    next_request_id: &mut u64,
    target: &TerminalTarget,
    persist_active: bool,
) -> anyhow::Result<()>
where
    W: frame::WriteAllAsync + Unpin,
{
    if persist_active {
        send_control(
            send,
            next_request_id,
            Control::ActivateSectionTab {
                section_id: target.section_id.clone(),
                tab_id: target.tab_id.clone(),
            },
        )
        .await?;
    }
    send_control(
        send,
        next_request_id,
        Control::LaunchTab {
            section_id: target.section_id.clone(),
            tab_id: target.tab_id.clone(),
        },
    )
    .await?;
    send_control(
        send,
        next_request_id,
        Control::AttachTab {
            section_id: target.section_id.clone(),
            tab_id: target.tab_id.clone(),
        },
    )
    .await?;
    send_control(
        send,
        next_request_id,
        Control::TabResize {
            cols: TERMINAL_COLS,
            rows: TERMINAL_ROWS,
        },
    )
    .await
}

async fn send_terminal_input<W>(
    send: &mut W,
    next_request_id: &mut u64,
    event: TerminalInputEvent,
) -> anyhow::Result<()>
where
    W: frame::WriteAllAsync + Unpin,
{
    send_control(send, next_request_id, Control::TabInput { event }).await
}

async fn send_control<W>(
    send: &mut W,
    next_request_id: &mut u64,
    control: Control,
) -> anyhow::Result<()>
where
    W: frame::WriteAllAsync + Unpin,
{
    let request_id = *next_request_id;
    *next_request_id = next_request_id.wrapping_add(1);
    let payload = serde_json::to_vec(&ControlEnvelope {
        request_id,
        control,
    })?;
    frame::write_frame(send, frame::TY_CONTROL, &payload).await?;
    Ok(())
}

async fn wait_for_ticket(
    app_weak: &slint::Weak<AppWindow>,
) -> anyhow::Result<(EndpointId, Vec<SocketAddr>)> {
    loop {
        match load_ticket() {
            Ok(Some(ticket)) => return Ok(ticket),
            Ok(None) => {
                set_terminal_status(app_weak, "terminal: waiting for /tmp/daemon-sandbox.ticket");
                tokio::time::sleep(RETRY_DELAY).await;
            }
            Err(error) => return Err(error),
        }
    }
}

fn load_ticket() -> anyhow::Result<Option<(EndpointId, Vec<SocketAddr>)>> {
    let path = std::env::temp_dir().join("daemon-sandbox.ticket");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };

    let mut id = None;
    let mut addrs = Vec::new();
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("id=") {
            id = Some(rest.trim().parse().context("parse EndpointId in ticket")?);
        } else if let Some(rest) = line.strip_prefix("addr=") {
            addrs.push(rest.trim().parse().context("parse addr in ticket")?);
        }
    }

    Ok(id.map(|id| (id, addrs)))
}

fn pre_authorize_local_client(endpoint_id: EndpointId) -> anyhow::Result<()> {
    daemon_sandbox::persist_pairing(&endpoint_id.to_string(), &sandbox_paired_peers_path())
}

fn sandbox_paired_peers_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("another-one-sandbox").join("paired_peers")
}

fn set_terminal_status(app_weak: &slint::Weak<AppWindow>, status: impl Into<String>) {
    let app_weak = app_weak.clone();
    let status = status.into();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_terminal_status(status.into());
        }
    });
}

fn set_terminal_surface(app_weak: &slint::Weak<AppWindow>, surface: TerminalSurface) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_terminal_background_spans(slint::ModelRc::new(slint::VecModel::from(
                surface.background_spans,
            )));
            app.set_terminal_cursor_spans(slint::ModelRc::new(slint::VecModel::from(
                surface.cursor_spans,
            )));
            app.set_terminal_link_spans(slint::ModelRc::new(slint::VecModel::from(
                surface.link_spans,
            )));
            app.set_terminal_runs(slint::ModelRc::new(slint::VecModel::from(
                surface.text_runs,
            )));
        }
    });
}

fn encode_terminal_key(
    input: &SlintKeyEvent,
    modes: TerminalInputModeState,
) -> Option<TerminalInputEvent> {
    let text = input.text.as_str();
    let mut bytes = match text {
        "\u{0008}" => vec![0x7f],
        "\u{0009}" => b"\t".to_vec(),
        "\u{000a}" => b"\r".to_vec(),
        "\u{001b}" => b"\x1b".to_vec(),
        "\u{007f}" => b"\x1b[3~".to_vec(),
        "\u{f700}" => cursor_key_sequence(b'A', modes.app_cursor),
        "\u{f701}" => cursor_key_sequence(b'B', modes.app_cursor),
        "\u{f702}" => cursor_key_sequence(b'D', modes.app_cursor),
        "\u{f703}" => cursor_key_sequence(b'C', modes.app_cursor),
        "\u{f729}" => cursor_key_sequence(b'H', modes.app_cursor),
        "\u{f72b}" => cursor_key_sequence(b'F', modes.app_cursor),
        "\u{f72c}" => b"\x1b[5~".to_vec(),
        "\u{f72d}" => b"\x1b[6~".to_vec(),
        value if value.chars().count() == 1 => {
            let ch = value.chars().next()?;
            if input.control {
                control_key_byte(ch)?
            } else if input.alt {
                value.as_bytes().to_vec()
            } else {
                return Some(TerminalInputEvent::Text {
                    text: value.to_string(),
                });
            }
        }
        value if !input.control && input.alt => value.as_bytes().to_vec(),
        value if !input.control => {
            return Some(TerminalInputEvent::Paste {
                text: value.to_string(),
                bracketed: modes.bracketed_paste,
            });
        }
        _ => return None,
    };

    if input.alt {
        bytes.insert(0, 0x1b);
    }

    Some(TerminalInputEvent::Key { bytes })
}

fn cursor_key_sequence(final_byte: u8, app_cursor: bool) -> Vec<u8> {
    if app_cursor {
        vec![0x1b, b'O', final_byte]
    } else {
        vec![0x1b, b'[', final_byte]
    }
}

fn control_key_byte(ch: char) -> Option<Vec<u8>> {
    let lower = ch.to_ascii_lowercase();
    if lower.is_ascii_lowercase() {
        Some(vec![(lower as u8) - b'a' + 1])
    } else if ch == ' ' {
        Some(vec![0])
    } else {
        None
    }
}

fn startup_probe() -> Option<&'static [u8]> {
    match std::env::var("ANOTHERONE_SLINT_STARTUP_PROBE").as_deref() {
        Ok("shell-color") => Some(SHELL_COLOR_SMOKE_PROBE),
        Ok("shell-ready") => Some(SHELL_READINESS_PROBE),
        _ => None,
    }
}

#[derive(Clone)]
struct RuntimeEventProxy {
    queue: Arc<Mutex<VecDeque<Event>>>,
}

impl EventListener for RuntimeEventProxy {
    fn send_event(&self, event: Event) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push_back(event);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TerminalSize {
    cols: u16,
    rows: u16,
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }

    fn screen_lines(&self) -> usize {
        self.rows as usize
    }

    fn columns(&self) -> usize {
        self.cols as usize
    }
}

struct AlacrittySnapshot {
    term: Term<RuntimeEventProxy>,
    parser: ansi::Processor<ansi::StdSyncHandler>,
    event_queue: Arc<Mutex<VecDeque<Event>>>,
    size: TerminalSize,
}

#[derive(Default)]
struct TerminalSurface {
    text_runs: Vec<TerminalTextRun>,
    background_spans: Vec<TerminalBackgroundSpan>,
    cursor_spans: Vec<TerminalCursorSpan>,
    link_spans: Vec<TerminalLinkSpan>,
}

#[derive(Clone, PartialEq)]
struct ResolvedCellStyle {
    foreground: u32,
    background: u32,
    bold: bool,
}

struct PendingTerminalRun {
    line: usize,
    column: usize,
    cell_count: usize,
    text: String,
    style: ResolvedCellStyle,
}

impl AlacrittySnapshot {
    fn new(cols: u16, rows: u16) -> Self {
        let size = TerminalSize { cols, rows };
        let event_queue = Arc::new(Mutex::new(VecDeque::new()));
        let event_proxy = RuntimeEventProxy {
            queue: event_queue.clone(),
        };
        Self {
            term: Term::new(Config::default(), &size, event_proxy),
            parser: ansi::Processor::default(),
            event_queue,
            size,
        }
    }

    fn apply_output(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.parser.advance(&mut self.term, bytes);
        self.pending_pty_writes()
    }

    fn input_modes(&self) -> TerminalInputModeState {
        let mode = self.term.mode();
        TerminalInputModeState {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        }
    }

    fn pending_pty_writes(&self) -> Vec<Vec<u8>> {
        let mut writes = Vec::new();
        let Ok(mut queue) = self.event_queue.lock() else {
            return writes;
        };
        while let Some(event) = queue.pop_front() {
            match event {
                Event::PtyWrite(text) => writes.push(text.into_bytes()),
                Event::ColorRequest(_, formatter) => {
                    writes.push(formatter(Default::default()).into_bytes());
                }
                Event::TextAreaSizeRequest(formatter) => {
                    writes.push(formatter(window_size_from_grid(self.size)).into_bytes());
                }
                _ => {}
            }
        }
        writes
    }

    fn snapshot_surface(&self) -> TerminalSurface {
        let renderable = self.term.renderable_content();
        let display_offset = renderable.display_offset;
        let cursor = (renderable.cursor.shape != CursorShape::Hidden)
            .then(|| point_to_viewport(display_offset, renderable.cursor.point))
            .flatten();
        let mut surface = TerminalSurface::default();

        for viewport_line in 0..self.size.rows as usize {
            let point = viewport_to_point(display_offset, Point::new(viewport_line, Column(0)));
            let grid_line = &self.term.grid()[point.line];
            let mut pending_run = None;
            let mut visual_column = 0;

            for column in 0..self.size.cols as usize {
                let cell = &grid_line[Column(column)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }

                let is_cursor = cursor.is_some_and(|cursor| {
                    cursor.line == viewport_line && cursor.column.0 == column
                });
                let mut style = resolve_cell_style(cell, renderable.colors);
                let cell_count = terminal_cell_width(cell);
                let text = visible_cell_text(cell);

                if text.as_deref().is_some_and(|text| {
                    joins_previous_terminal_grapheme(&pending_run, viewport_line, text, &style)
                }) {
                    if let Some(run) = pending_run.as_mut() {
                        if let Some(text) = text {
                            run.text.push_str(&text);
                        }
                    }
                    continue;
                }

                if is_cursor && renderable.cursor.shape == CursorShape::Block {
                    maybe_push_background_span(
                        &mut surface.background_spans,
                        viewport_line,
                        visual_column,
                        cell_count,
                        style.foreground,
                        true,
                    );
                    style.foreground = style.background;
                } else {
                    maybe_push_background_span(
                        &mut surface.background_spans,
                        viewport_line,
                        visual_column,
                        cell_count,
                        style.background,
                        false,
                    );
                }

                if let Some(hyperlink) = cell.hyperlink() {
                    maybe_push_link_span(
                        &mut surface.link_spans,
                        viewport_line,
                        visual_column,
                        cell_count,
                        hyperlink.uri(),
                    );
                }

                if is_cursor {
                    maybe_push_cursor_span(
                        &mut surface.cursor_spans,
                        viewport_line,
                        visual_column,
                        cell_count,
                        renderable.cursor.shape,
                        style.foreground,
                    );
                }

                let Some(text) = text else {
                    if let Some(run) = pending_run.take() {
                        push_terminal_run(&mut surface.text_runs, run);
                    }
                    visual_column += cell_count;
                    continue;
                };

                append_terminal_run(
                    &mut pending_run,
                    &mut surface.text_runs,
                    viewport_line,
                    visual_column,
                    cell_count,
                    text,
                    style,
                );
                visual_column += cell_count;
            }

            if let Some(run) = pending_run.take() {
                push_terminal_run(&mut surface.text_runs, run);
            }
        }

        surface
    }
}

fn joins_previous_terminal_grapheme(
    pending_run: &Option<PendingTerminalRun>,
    line: usize,
    text: &str,
    style: &ResolvedCellStyle,
) -> bool {
    let Some(run) = pending_run else {
        return false;
    };

    run.line == line && run.style == *style && !text.is_empty() && run.text.ends_with('\u{200d}')
}

fn append_terminal_run(
    pending_run: &mut Option<PendingTerminalRun>,
    runs: &mut Vec<TerminalTextRun>,
    line: usize,
    column: usize,
    cell_count: usize,
    text: String,
    style: ResolvedCellStyle,
) {
    if let Some(run) = pending_run {
        if run.line == line && run.column + run.cell_count == column && run.style == style {
            run.cell_count += cell_count;
            run.text.push_str(&text);
            return;
        }

        if let Some(finished) = pending_run.take() {
            push_terminal_run(runs, finished);
        }
    }

    *pending_run = Some(PendingTerminalRun {
        line,
        column,
        cell_count,
        text,
        style,
    });
}

fn push_terminal_run(runs: &mut Vec<TerminalTextRun>, run: PendingTerminalRun) {
    runs.push(TerminalTextRun {
        line: to_i32(run.line),
        column: to_i32(run.column),
        cell_count: to_i32(run.cell_count),
        text: run.text.into(),
        color: slint::Color::from_argb_encoded(run.style.foreground),
        bold: run.style.bold,
    });
}

fn maybe_push_background_span(
    spans: &mut Vec<TerminalBackgroundSpan>,
    line: usize,
    column: usize,
    cell_count: usize,
    color: u32,
    force: bool,
) {
    if !force && color == default_background_color() {
        return;
    }

    let line = to_i32(line);
    let column = to_i32(column);
    let cell_count = to_i32(cell_count);
    if let Some(last) = spans.last_mut() {
        if last.line == line
            && last.column + last.cell_count == column
            && last.color.as_argb_encoded() == color
        {
            last.cell_count += cell_count;
            return;
        }
    }

    spans.push(TerminalBackgroundSpan {
        line,
        column,
        cell_count,
        color: slint::Color::from_argb_encoded(color),
    });
}

fn maybe_push_cursor_span(
    spans: &mut Vec<TerminalCursorSpan>,
    line: usize,
    column: usize,
    cell_count: usize,
    shape: CursorShape,
    color: u32,
) {
    let Some(shape) = cursor_shape_name(shape) else {
        return;
    };

    spans.push(TerminalCursorSpan {
        line: to_i32(line),
        column: to_i32(column),
        cell_count: to_i32(cell_count),
        shape: shape.into(),
        color: slint::Color::from_argb_encoded(color),
    });
}

fn cursor_shape_name(shape: CursorShape) -> Option<&'static str> {
    match shape {
        CursorShape::Block | CursorShape::Hidden => None,
        CursorShape::Underline => Some("underline"),
        CursorShape::Beam => Some("beam"),
        CursorShape::HollowBlock => Some("hollow-block"),
    }
}

fn maybe_push_link_span(
    spans: &mut Vec<TerminalLinkSpan>,
    line: usize,
    column: usize,
    cell_count: usize,
    uri: &str,
) {
    let line = to_i32(line);
    let column = to_i32(column);
    let cell_count = to_i32(cell_count);
    if let Some(last) = spans.last_mut() {
        if last.line == line && last.column + last.cell_count == column && last.uri.as_str() == uri
        {
            last.cell_count += cell_count;
            return;
        }
    }

    spans.push(TerminalLinkSpan {
        line,
        column,
        cell_count,
        uri: uri.into(),
    });
}

fn visible_cell_text(cell: &alacritty_terminal::term::cell::Cell) -> Option<String> {
    if cell.flags.contains(Flags::HIDDEN) || cell_is_render_blank(cell) {
        return None;
    }

    let mut text = String::new();
    text.push(if cell.c == ' ' { '\u{00a0}' } else { cell.c });
    for zero_width in cell.zerowidth().into_iter().flatten() {
        text.push(*zero_width);
    }

    Some(text)
}

fn cell_is_render_blank(cell: &alacritty_terminal::term::cell::Cell) -> bool {
    if cell.c != ' ' {
        return false;
    }

    if cell.bg != Color::Named(NamedColor::Background) {
        return false;
    }

    !cell
        .flags
        .intersects(Flags::ALL_UNDERLINES | Flags::INVERSE | Flags::STRIKEOUT)
}

fn terminal_cell_width(cell: &alacritty_terminal::term::cell::Cell) -> usize {
    if cell.flags.contains(Flags::WIDE_CHAR) {
        2
    } else {
        1
    }
}

fn resolve_cell_style(
    cell: &alacritty_terminal::term::cell::Cell,
    colors: &Colors,
) -> ResolvedCellStyle {
    let mut foreground = resolve_color(cell.fg, cell.flags, true, colors);
    let mut background = resolve_color(cell.bg, cell.flags, false, colors);

    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut foreground, &mut background);
    }

    if cell.flags.contains(Flags::HIDDEN) {
        foreground = background;
    }

    ResolvedCellStyle {
        foreground,
        background,
        bold: cell.flags.contains(Flags::BOLD),
    }
}

fn resolve_color(mut color: Color, flags: Flags, is_foreground: bool, colors: &Colors) -> u32 {
    if is_foreground {
        if flags.contains(Flags::DIM) {
            if let Color::Named(named) = color {
                color = Color::Named(named.to_dim());
            }
        } else if flags.contains(Flags::BOLD) {
            if let Color::Named(named) = color {
                color = Color::Named(named.to_bright());
            }
        }
    }

    let rgb = match color {
        Color::Named(named) => resolve_named_color(named, colors),
        Color::Spec(rgb) => rgb,
        Color::Indexed(index) => resolve_indexed_color(index, colors),
    };

    rgb_to_argb(rgb)
}

fn resolve_named_color(named: NamedColor, colors: &Colors) -> Rgb {
    colors[named].unwrap_or_else(|| default_named_color(named))
}

fn resolve_indexed_color(index: u8, colors: &Colors) -> Rgb {
    colors[index as usize].unwrap_or_else(|| default_indexed_color(index))
}

fn default_named_color(named: NamedColor) -> Rgb {
    match named {
        NamedColor::Black => rgb_to_vte(0x1f242d),
        NamedColor::Red => rgb_to_vte(0xe06c75),
        NamedColor::Green => rgb_to_vte(0x98c379),
        NamedColor::Yellow => rgb_to_vte(0xe5c07b),
        NamedColor::Blue => rgb_to_vte(0x61afef),
        NamedColor::Magenta => rgb_to_vte(0xc678dd),
        NamedColor::Cyan => rgb_to_vte(0x56b6c2),
        NamedColor::White => rgb_to_vte(0xd7dae0),
        NamedColor::BrightBlack => rgb_to_vte(0x5c6370),
        NamedColor::BrightRed => rgb_to_vte(0xf28b95),
        NamedColor::BrightGreen => rgb_to_vte(0xb8db87),
        NamedColor::BrightYellow => rgb_to_vte(0xf2d48f),
        NamedColor::BrightBlue => rgb_to_vte(0x8fc7ff),
        NamedColor::BrightMagenta => rgb_to_vte(0xd7a8ff),
        NamedColor::BrightCyan => rgb_to_vte(0x7fd7e6),
        NamedColor::BrightWhite => rgb_to_vte(0xffffff),
        NamedColor::Foreground => rgb_to_vte(DEFAULT_TERMINAL_FOREGROUND_RGB),
        NamedColor::Background => rgb_to_vte(DEFAULT_TERMINAL_BACKGROUND_RGB),
        NamedColor::Cursor => rgb_to_vte(DEFAULT_TERMINAL_FOREGROUND_RGB),
        NamedColor::DimBlack => scale_rgb(default_named_color(NamedColor::Black), 0.72),
        NamedColor::DimRed => scale_rgb(default_named_color(NamedColor::Red), 0.72),
        NamedColor::DimGreen => scale_rgb(default_named_color(NamedColor::Green), 0.72),
        NamedColor::DimYellow => scale_rgb(default_named_color(NamedColor::Yellow), 0.72),
        NamedColor::DimBlue => scale_rgb(default_named_color(NamedColor::Blue), 0.72),
        NamedColor::DimMagenta => scale_rgb(default_named_color(NamedColor::Magenta), 0.72),
        NamedColor::DimCyan => scale_rgb(default_named_color(NamedColor::Cyan), 0.72),
        NamedColor::DimWhite => scale_rgb(default_named_color(NamedColor::White), 0.72),
        NamedColor::BrightForeground => rgb_to_vte(0xffffff),
        NamedColor::DimForeground => scale_rgb(rgb_to_vte(DEFAULT_TERMINAL_FOREGROUND_RGB), 0.72),
    }
}

fn default_indexed_color(index: u8) -> Rgb {
    match index {
        0 => default_named_color(NamedColor::Black),
        1 => default_named_color(NamedColor::Red),
        2 => default_named_color(NamedColor::Green),
        3 => default_named_color(NamedColor::Yellow),
        4 => default_named_color(NamedColor::Blue),
        5 => default_named_color(NamedColor::Magenta),
        6 => default_named_color(NamedColor::Cyan),
        7 => default_named_color(NamedColor::White),
        8 => default_named_color(NamedColor::BrightBlack),
        9 => default_named_color(NamedColor::BrightRed),
        10 => default_named_color(NamedColor::BrightGreen),
        11 => default_named_color(NamedColor::BrightYellow),
        12 => default_named_color(NamedColor::BrightBlue),
        13 => default_named_color(NamedColor::BrightMagenta),
        14 => default_named_color(NamedColor::BrightCyan),
        15 => default_named_color(NamedColor::BrightWhite),
        16..=231 => {
            let index = index - 16;
            let red = index / 36;
            let green = (index % 36) / 6;
            let blue = index % 6;
            let cube = [0, 95, 135, 175, 215, 255];
            Rgb {
                r: cube[red as usize],
                g: cube[green as usize],
                b: cube[blue as usize],
            }
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            Rgb {
                r: value,
                g: value,
                b: value,
            }
        }
    }
}

fn default_background_color() -> u32 {
    0xff000000 | DEFAULT_TERMINAL_BACKGROUND_RGB
}

fn scale_rgb(rgb: Rgb, factor: f32) -> Rgb {
    Rgb {
        r: (f32::from(rgb.r) * factor).round().clamp(0.0, 255.0) as u8,
        g: (f32::from(rgb.g) * factor).round().clamp(0.0, 255.0) as u8,
        b: (f32::from(rgb.b) * factor).round().clamp(0.0, 255.0) as u8,
    }
}

fn rgb_to_argb(rgb: Rgb) -> u32 {
    0xff000000 | ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32
}

fn rgb_to_vte(color: u32) -> Rgb {
    Rgb {
        r: ((color >> 16) & 0xff) as u8,
        g: ((color >> 8) & 0xff) as u8,
        b: (color & 0xff) as u8,
    }
}

fn to_i32(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}

fn window_size_from_grid(size: TerminalSize) -> WindowSize {
    WindowSize {
        num_lines: size.rows,
        num_cols: size.cols,
        cell_width: 8,
        cell_height: 16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_surface_preserves_ansi_foreground_colors() {
        let mut terminal = AlacrittySnapshot::new(40, 4);
        let _ = terminal.apply_output(b"\x1b[31mRED \x1b[32mGREEN \x1b[34mBLUE\x1b[0m DEFAULT");

        let surface = terminal.snapshot_surface();

        assert_run_color(&surface, "RED", 0xffe06c75);
        assert_run_color(&surface, "GREEN", 0xff98c379);
        assert_run_color(&surface, "BLUE", 0xff61afef);
    }

    #[test]
    fn snapshot_surface_preserves_combining_marks_in_text_runs() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        let _ = terminal.apply_output("e\u{0301}clair".as_bytes());

        let surface = terminal.snapshot_surface();

        let run = find_run_containing(&surface, "e\u{0301}clair");
        assert_eq!(run.line, 0);
        assert_eq!(run.column, 0);
        assert_eq!(run.cell_count, 6);
    }

    #[test]
    fn snapshot_surface_preserves_wide_cjk_cell_occupancy() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        let _ = terminal.apply_output("界X".as_bytes());

        let surface = terminal.snapshot_surface();

        let run = find_run_containing(&surface, "界X");
        assert_eq!(run.line, 0);
        assert_eq!(run.column, 0);
        assert_eq!(run.cell_count, 3);
    }

    #[test]
    fn snapshot_surface_preserves_wide_emoji_cell_occupancy() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        let _ = terminal.apply_output("🙂Z".as_bytes());

        let surface = terminal.snapshot_surface();

        let run = find_run_containing(&surface, "🙂Z");
        assert_eq!(run.line, 0);
        assert_eq!(run.column, 0);
        assert_eq!(run.cell_count, 3);
    }

    #[test]
    fn snapshot_surface_preserves_emoji_zwj_graphemes() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        let _ = terminal.apply_output("👩\u{200d}💻Z".as_bytes());

        let surface = terminal.snapshot_surface();

        let run = find_run_containing(&surface, "👩\u{200d}💻Z");
        assert_eq!(run.line, 0);
        assert_eq!(run.column, 0);
        assert_eq!(run.cell_count, 3);
    }

    #[test]
    fn snapshot_surface_splits_styled_runs_after_wide_cells() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        let _ = terminal.apply_output("\x1b[31m界\x1b[32mX".as_bytes());

        let surface = terminal.snapshot_surface();

        let wide = find_run_containing(&surface, "界");
        assert_eq!(wide.column, 0);
        assert_eq!(wide.cell_count, 2);
        assert_eq!(wide.color.as_argb_encoded(), 0xffe06c75);

        let narrow = find_run_containing(&surface, "X");
        assert_eq!(narrow.column, 2);
        assert_eq!(narrow.cell_count, 1);
        assert_eq!(narrow.color.as_argb_encoded(), 0xff98c379);
    }

    #[test]
    fn snapshot_surface_emits_beam_cursor_span() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        let _ = terminal.apply_output(b"\x1b[6 q");

        let surface = terminal.snapshot_surface();

        let cursor = single_cursor_span(&surface);
        assert_eq!(cursor.shape.as_str(), "beam");
        assert_eq!(cursor.line, 0);
        assert_eq!(cursor.column, 0);
        assert_eq!(cursor.cell_count, 1);
    }

    #[test]
    fn snapshot_surface_emits_underline_cursor_span() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        let _ = terminal.apply_output(b"\x1b[4 q");

        let surface = terminal.snapshot_surface();

        let cursor = single_cursor_span(&surface);
        assert_eq!(cursor.shape.as_str(), "underline");
        assert_eq!(cursor.line, 0);
        assert_eq!(cursor.column, 0);
        assert_eq!(cursor.cell_count, 1);
    }

    #[test]
    fn snapshot_surface_hides_hidden_cursor() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        let _ = terminal.apply_output(b"\x1b[?25l");

        let surface = terminal.snapshot_surface();

        assert!(surface.cursor_spans.is_empty());
    }

    #[test]
    fn snapshot_surface_preserves_osc8_hyperlink_spans() {
        let mut terminal = AlacrittySnapshot::new(40, 4);
        let _ = terminal.apply_output(b"\x1b]8;;https://example.test\x1b\\link\x1b]8;;\x1b\\ tail");

        let surface = terminal.snapshot_surface();

        let link = single_link_span(&surface);
        assert_eq!(link.line, 0);
        assert_eq!(link.column, 0);
        assert_eq!(link.cell_count, 4);
        assert_eq!(link.uri.as_str(), "https://example.test");
    }

    #[test]
    fn cursor_shape_name_maps_hollow_block() {
        assert_eq!(
            cursor_shape_name(CursorShape::HollowBlock),
            Some("hollow-block")
        );
    }

    #[test]
    fn next_terminal_flush_deadline_keeps_frame_budget() {
        let last_flush = Instant::now();
        let now = last_flush + Duration::from_millis(10);

        let deadline = next_terminal_flush_deadline(now, last_flush);

        assert_eq!(deadline, last_flush + TERMINAL_FRAME_INTERVAL);
    }

    #[test]
    fn next_terminal_flush_deadline_flushes_immediately_when_budget_elapsed() {
        let last_flush = Instant::now();
        let now = last_flush + TERMINAL_FRAME_INTERVAL + Duration::from_millis(1);

        let deadline = next_terminal_flush_deadline(now, last_flush);

        assert_eq!(deadline, now);
    }

    #[test]
    fn input_modes_track_application_cursor_mode() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        assert!(!terminal.input_modes().app_cursor);

        let _ = terminal.apply_output(b"\x1b[?1h");

        assert!(terminal.input_modes().app_cursor);
    }

    #[test]
    fn input_modes_track_bracketed_paste_mode() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        assert!(!terminal.input_modes().bracketed_paste);

        let _ = terminal.apply_output(b"\x1b[?2004h");

        assert!(terminal.input_modes().bracketed_paste);
    }

    #[test]
    fn encode_terminal_key_uses_normal_cursor_sequences() {
        let bytes = encode_key_bytes("\u{f700}", TerminalInputModeState::default());

        assert_eq!(bytes, b"\x1b[A");
    }

    #[test]
    fn encode_terminal_key_uses_application_cursor_sequences() {
        let bytes = encode_key_bytes(
            "\u{f700}",
            TerminalInputModeState {
                app_cursor: true,
                bracketed_paste: false,
            },
        );

        assert_eq!(bytes, b"\x1bOA");
    }

    #[test]
    fn encode_terminal_key_brackets_multi_character_paste_when_enabled() {
        let bytes = encode_key_bytes(
            "alpha\nbeta",
            TerminalInputModeState {
                app_cursor: false,
                bracketed_paste: true,
            },
        );

        assert_eq!(bytes, b"\x1b[200~alpha\nbeta\x1b[201~");
    }

    #[test]
    fn encode_terminal_key_preserves_alt_prefix_for_text() {
        let event = SlintKeyEvent {
            text: "x".to_string(),
            control: false,
            alt: true,
        };

        let bytes = encode_terminal_key(&event, TerminalInputModeState::default())
            .expect("alt key should encode")
            .pty_bytes();

        assert_eq!(bytes, b"\x1bx");
    }

    #[test]
    fn project_id_for_target_prefers_task_target_project_id() {
        let projects = project_tree_for_submit_tests("worktree-project", "feature/a");
        let target = TerminalTarget {
            section_id: "section-1".to_string(),
            tab_id: "0".to_string(),
        };

        let project_id = project_id_for_target(&projects, &target);

        assert_eq!(project_id.as_deref(), Some("worktree-project"));
    }

    #[test]
    fn project_id_for_target_falls_back_to_root_project_id() {
        let projects = project_tree_for_submit_tests("", "feature/a");
        let target = TerminalTarget {
            section_id: "section-1".to_string(),
            tab_id: "0".to_string(),
        };

        let project_id = project_id_for_target(&projects, &target);

        assert_eq!(project_id.as_deref(), Some("root-project"));
    }

    #[test]
    fn normalized_source_branch_uses_requested_branch_first() {
        let projects = project_tree_for_submit_tests("worktree-project", "feature/a");
        let target = TerminalTarget {
            section_id: "section-1".to_string(),
            tab_id: "0".to_string(),
        };

        let source_branch = normalized_source_branch(&projects, &target, "  release/b  ");

        assert_eq!(source_branch.as_deref(), Some("release/b"));
    }

    #[test]
    fn normalized_source_branch_falls_back_to_task_branch() {
        let projects = project_tree_for_submit_tests("worktree-project", "feature/a");
        let target = TerminalTarget {
            section_id: "section-1".to_string(),
            tab_id: "0".to_string(),
        };

        let source_branch = normalized_source_branch(&projects, &target, "");

        assert_eq!(source_branch.as_deref(), Some("feature/a"));
    }

    fn assert_run_color(surface: &TerminalSurface, text: &str, expected: u32) {
        let run = find_run_containing(surface, text);
        assert_eq!(run.color.as_argb_encoded(), expected);
    }

    fn find_run_containing<'a>(surface: &'a TerminalSurface, text: &str) -> &'a TerminalTextRun {
        surface
            .text_runs
            .iter()
            .find(|run| run.text.as_str().contains(text))
            .unwrap_or_else(|| panic!("missing terminal run containing {text:?}"))
    }

    fn single_cursor_span(surface: &TerminalSurface) -> &TerminalCursorSpan {
        assert_eq!(surface.cursor_spans.len(), 1);
        &surface.cursor_spans[0]
    }

    fn single_link_span(surface: &TerminalSurface) -> &TerminalLinkSpan {
        assert_eq!(surface.link_spans.len(), 1);
        &surface.link_spans[0]
    }

    fn encode_key_bytes(text: &str, modes: TerminalInputModeState) -> Vec<u8> {
        let event = SlintKeyEvent {
            text: text.to_string(),
            control: false,
            alt: false,
        };

        encode_terminal_key(&event, modes)
            .expect("key should encode")
            .pty_bytes()
    }

    fn project_tree_for_submit_tests(
        task_target_project_id: &str,
        task_branch_name: &str,
    ) -> Vec<frame::ProjectSummary> {
        vec![frame::ProjectSummary {
            id: "root-project".to_string(),
            name: "Root".to_string(),
            path: "/repo/root".to_string(),
            kind: frame::ProjectKind::Root,
            current_branch: Some("main".to_string()),
            tasks: vec![frame::TaskSummary {
                id: "task-1".to_string(),
                name: "Task".to_string(),
                section_id: "section-1".to_string(),
                branch_name: task_branch_name.to_string(),
                active_tab_id: "0".to_string(),
                tabs: Vec::new(),
                pinned: false,
                last_commit_relative: String::new(),
                lines_added: 0,
                lines_removed: 0,
                target_project_id: task_target_project_id.to_string(),
            }],
        }]
    }
}
