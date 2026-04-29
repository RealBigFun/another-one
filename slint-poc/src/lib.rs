use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
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
const TERMINAL_RESIZE_DEBOUNCE: Duration = Duration::from_millis(80);
const SIDEBAR_TREE_TOP: i32 = 40;
const SIDEBAR_PROJECT_ROW_HEIGHT: i32 = 36;
const SIDEBAR_TASK_ROW_HEIGHT: i32 = 46;
const RIGHT_INSPECTOR_SECTION_ROW_HEIGHT: i32 = 44;
const RIGHT_INSPECTOR_FILE_ROW_HEIGHT: i32 = 34;
const RIGHT_INSPECTOR_COMMIT_ROW_HEIGHT: i32 = 42;
const RIGHT_INSPECTOR_CHECK_ROW_HEIGHT: i32 = 46;
const DEFAULT_TERMINAL_BACKGROUND_RGB: u32 = 0x1e1f22;
const DEFAULT_TERMINAL_FOREGROUND_RGB: u32 = 0xd7dae0;
const PROJECT_ACCENTS: [u32; 8] = [
    0x5b4a9e, 0x2e7d6f, 0xb85c38, 0x3a6ea5, 0x8b5e3c, 0x7b2d5f, 0x4a7c4b, 0x9c5151,
];
const SHELL_COLOR_SMOKE_PROBE: &[u8] =
    b"printf '\\033[31mRED \\033[32mGREEN \\033[34mBLUE\\033[0m DEFAULT\\n'\nprintf 'ANOTHERONE_SLINT_READY\\n'\r";
const SHELL_READINESS_PROBE: &[u8] = b"printf 'ANOTHERONE_SLINT_READY\\n'\r";
const TERMINAL_FIDELITY_FIXTURE: &[u8] = concat!(
    "\x1b[31mRED \x1b[32mGREEN \x1b[34mBLUE\x1b[0m DEFAULT\r\n",
    "\x1b[38;5;208mINDEXED_208\x1b[0m \x1b[38;2;125;90;255mTRUECOLOR_RGB\x1b[0m\r\n",
    "Combining: e\u{301} CJK: \u{754c} Emoji: \u{1f469}\u{200d}\u{1f4bb}\r\n",
    "\x1b]8;;https://example.test\x1b\\OSC8_LINK\x1b]8;;\x1b\\ plain text\r\n",
    "\x1b[4 qUnderline cursor fixture"
)
.as_bytes();
const TERMINAL_RENDER_PROBE_CHUNK_SIZE: usize = 8192;
const TERMINAL_RENDER_PROBE_LINE: &[u8] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz+/ terminal flood line\n";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalRenderProbeReport {
    pub target_bytes: usize,
    pub applied_bytes: usize,
    pub data_frames: usize,
    pub snapshots: usize,
    pub elapsed_ms: u128,
    pub throughput_mib_s: u64,
    pub snapshot_p50_us: u128,
    pub snapshot_p95_us: u128,
    pub snapshot_p99_us: u128,
    pub snapshot_max_us: u128,
    pub max_text_runs: usize,
    pub max_background_spans: usize,
    pub max_cursor_spans: usize,
    pub rss_kib: Option<u64>,
}

#[derive(Clone, Debug)]
enum InspectorCommitFileChangesState {
    Loading,
    Loaded(Vec<frame::BranchCompareFileWire>),
    Failed,
}

impl TerminalRenderProbeReport {
    pub fn to_lines(&self) -> Vec<String> {
        vec![
            format!("target_bytes={}", self.target_bytes),
            format!("applied_bytes={}", self.applied_bytes),
            format!("data_frames={}", self.data_frames),
            format!("snapshots={}", self.snapshots),
            format!("elapsed_ms={}", self.elapsed_ms),
            format!("throughput_mib_s={}", self.throughput_mib_s),
            format!("snapshot_p50_us={}", self.snapshot_p50_us),
            format!("snapshot_p95_us={}", self.snapshot_p95_us),
            format!("snapshot_p99_us={}", self.snapshot_p99_us),
            format!("snapshot_max_us={}", self.snapshot_max_us),
            format!("max_text_runs={}", self.max_text_runs),
            format!("max_background_spans={}", self.max_background_spans),
            format!("max_cursor_spans={}", self.max_cursor_spans),
            format!(
                "rss_kib={}",
                self.rss_kib
                    .map(|rss| rss.to_string())
                    .unwrap_or_else(|| "unavailable".to_string())
            ),
        ]
    }
}

pub fn run_terminal_render_probe(target_bytes: usize) -> TerminalRenderProbeReport {
    let mut terminal = AlacrittySnapshot::new(TERMINAL_COLS, TERMINAL_ROWS);
    let mut chunk = Vec::with_capacity(TERMINAL_RENDER_PROBE_CHUNK_SIZE);
    let mut applied_bytes = 0;
    let mut data_frames = 0;
    let mut snapshot_durations = Vec::new();
    let mut max_text_runs = 0;
    let mut max_background_spans = 0;
    let mut max_cursor_spans = 0;
    let started = std::time::Instant::now();

    while applied_bytes < target_bytes {
        chunk.clear();
        while chunk.len() < TERMINAL_RENDER_PROBE_CHUNK_SIZE
            && applied_bytes + chunk.len() < target_bytes
        {
            let remaining = target_bytes - applied_bytes - chunk.len();
            let take = remaining.min(TERMINAL_RENDER_PROBE_LINE.len());
            chunk.extend_from_slice(&TERMINAL_RENDER_PROBE_LINE[..take]);
        }

        let _ = terminal.apply_output(&chunk);
        applied_bytes += chunk.len();
        data_frames += 1;

        let snapshot_started = std::time::Instant::now();
        let surface = terminal.snapshot_surface();
        snapshot_durations.push(snapshot_started.elapsed().as_micros());
        max_text_runs = max_text_runs.max(surface.text_runs.len());
        max_background_spans = max_background_spans.max(surface.background_spans.len());
        max_cursor_spans = max_cursor_spans.max(surface.cursor_spans.len());
    }

    snapshot_durations.sort_unstable();
    let elapsed = started.elapsed();
    TerminalRenderProbeReport {
        target_bytes,
        applied_bytes,
        data_frames,
        snapshots: snapshot_durations.len(),
        elapsed_ms: elapsed.as_millis(),
        throughput_mib_s: mib_per_second(applied_bytes, elapsed),
        snapshot_p50_us: percentile(&snapshot_durations, 50),
        snapshot_p95_us: percentile(&snapshot_durations, 95),
        snapshot_p99_us: percentile(&snapshot_durations, 99),
        snapshot_max_us: snapshot_durations.last().copied().unwrap_or_default(),
        max_text_runs,
        max_background_spans,
        max_cursor_spans,
        rss_kib: current_rss_kib(),
    }
}

fn percentile(sorted_values: &[u128], percentile: usize) -> u128 {
    if sorted_values.is_empty() {
        return 0;
    }

    let index = ((sorted_values.len() - 1) * percentile) / 100;
    sorted_values[index]
}

fn mib_per_second(bytes: usize, elapsed: Duration) -> u64 {
    let nanos = elapsed.as_nanos();
    if nanos == 0 {
        return 0;
    }

    let bytes_per_second = (bytes as u128).saturating_mul(1_000_000_000) / nanos;
    (bytes_per_second / (1024 * 1024)) as u64
}

fn current_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        let rest = line.strip_prefix("VmRSS:")?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

fn empty_string_to_none(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

pub fn run_app() -> Result<(), slint::PlatformError> {
    let app = AppWindow::new()?;
    let platform_profile = platform::current_platform_profile();
    #[cfg(not(target_os = "android"))]
    slint::set_xdg_app_id(platform_profile.app_id)?;
    style::apply_theme(&app);
    app.set_platform_label(platform_profile.label().into());
    seed_shell_model(&app);
    seed_visual_state_fixture(&app);
    if std::env::var("ANOTHERONE_SLINT_FIXTURE").as_deref() == Ok("terminal-fidelity") {
        seed_terminal_fidelity_fixture(&app);
        app.on_close_requested(|| std::process::exit(0));
        return app.run();
    }
    if std::env::var("ANOTHERONE_SLINT_FIXTURE").as_deref() == Ok("component-states") {
        seed_component_state_fixture(&app);
        app.on_close_requested(|| std::process::exit(0));
        return app.run();
    }

    let (client_event_tx, client_event_rx) = mpsc::unbounded_channel::<SlintClientEvent>();
    let terminal_event_tx = client_event_tx.clone();
    app.on_terminal_key(move |text, control, alt, _shift| {
        let _ = terminal_event_tx.send(SlintClientEvent::TerminalKey(SlintKeyEvent {
            text: text.to_string(),
            control,
            alt,
        }));
    });
    let focus_event_tx = client_event_tx.clone();
    app.on_terminal_focus_changed(move |focused| {
        let _ = focus_event_tx.send(SlintClientEvent::TerminalFocus(focused));
    });
    let pointer_event_tx = client_event_tx.clone();
    app.on_terminal_pointer_event(move |kind, button, column, line, control, alt, shift| {
        let _ = pointer_event_tx.send(SlintClientEvent::TerminalPointer(SlintPointerEvent {
            kind: kind.to_string(),
            button: button.to_string(),
            column,
            line,
            control,
            alt,
            shift,
        }));
    });
    let resize_event_tx = client_event_tx.clone();
    app.on_terminal_resized(move |cols, rows| {
        let _ = resize_event_tx.send(SlintClientEvent::TerminalResize { cols, rows });
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
    let tab_close_event_tx = client_event_tx.clone();
    app.on_tab_close_requested(move |tab_id| {
        let _ = tab_close_event_tx.send(SlintClientEvent::CloseTerminalTab(tab_id.to_string()));
    });
    let tab_pin_event_tx = client_event_tx.clone();
    app.on_tab_pin_toggled(move |tab_id| {
        let _ = tab_pin_event_tx.send(SlintClientEvent::ToggleTerminalTabPinned(
            tab_id.to_string(),
        ));
    });
    let add_tab_event_tx = client_event_tx.clone();
    app.on_terminal_add_tab_requested(move || {
        let _ = add_tab_event_tx.send(SlintClientEvent::AddTerminalTab);
    });
    let terminal_error_copy_app = app.as_weak();
    app.on_terminal_error_copy_requested(move || {
        let Some(app) = terminal_error_copy_app.upgrade() else {
            return;
        };
        let details = app.get_terminal_error_details().to_string();
        if details.trim().is_empty() {
            set_toast(
                &terminal_error_copy_app,
                "warning",
                "No terminal error details",
                "The failed tab did not include daemon failure details.",
            );
            return;
        }
        match platform::copy_text(&details) {
            Ok(()) => set_toast(
                &terminal_error_copy_app,
                "success",
                "Terminal error details copied",
                "Failure details are on the clipboard.",
            ),
            Err(error) => set_toast(
                &terminal_error_copy_app,
                "error",
                "Could not copy terminal error details",
                error,
            ),
        }
    });
    let inspector_mode_event_tx = client_event_tx.clone();
    app.on_right_inspector_mode_selected(move |mode| {
        let _ =
            inspector_mode_event_tx.send(SlintClientEvent::RightInspectorMode(mode.to_string()));
    });
    let inspector_stage_event_tx = client_event_tx.clone();
    app.on_inspector_stage_file_requested(move |path, original_path, _untracked| {
        let _ = inspector_stage_event_tx.send(SlintClientEvent::StageChangedFile {
            path: path.to_string(),
            original_path: empty_string_to_none(original_path.as_str()),
        });
    });
    let inspector_unstage_event_tx = client_event_tx.clone();
    app.on_inspector_unstage_file_requested(move |path, original_path, _untracked| {
        let _ = inspector_unstage_event_tx.send(SlintClientEvent::UnstageChangedFile {
            path: path.to_string(),
            original_path: empty_string_to_none(original_path.as_str()),
        });
    });
    let inspector_discard_event_tx = client_event_tx.clone();
    app.on_inspector_discard_file_requested(move |path, original_path, untracked| {
        let _ = inspector_discard_event_tx.send(SlintClientEvent::DiscardChangedFile {
            path: path.to_string(),
            original_path: empty_string_to_none(original_path.as_str()),
            untracked,
        });
    });
    let inspector_commit_event_tx = client_event_tx.clone();
    app.on_inspector_commit_toggled(move |project_id, commit_id| {
        let _ = inspector_commit_event_tx.send(SlintClientEvent::ToggleInspectorCommit {
            project_id: project_id.to_string(),
            commit_id: commit_id.to_string(),
        });
    });
    let inspector_check_open_event_tx = client_event_tx.clone();
    app.on_inspector_check_open_requested(move |uri| {
        let _ = inspector_check_open_event_tx
            .send(SlintClientEvent::OpenInspectorCheckLink(uri.to_string()));
    });
    let inspector_section_event_tx = client_event_tx.clone();
    app.on_inspector_section_toggled(move |group| {
        let _ = inspector_section_event_tx
            .send(SlintClientEvent::ToggleInspectorSection(group.to_string()));
    });
    let inspector_stage_all_event_tx = client_event_tx.clone();
    app.on_inspector_stage_all_requested(move || {
        let _ = inspector_stage_all_event_tx.send(SlintClientEvent::StageAllChanges);
    });
    let inspector_unstage_all_event_tx = client_event_tx.clone();
    app.on_inspector_unstage_all_requested(move || {
        let _ = inspector_unstage_all_event_tx.send(SlintClientEvent::UnstageAllChanges);
    });
    let submit_event_tx = client_event_tx.clone();
    app.on_submit_new_task(move |task_name, source_branch, project_id| {
        let _ = submit_event_tx.send(SlintClientEvent::SubmitNewTask {
            task_name: task_name.to_string(),
            source_branch: source_branch.to_string(),
            project_id: project_id.to_string(),
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
    app.set_sidebar_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        SidebarTreeEntry {
            kind: "project".into(),
            id: "project:another-one".into(),
            group_id: "another-one".into(),
            project_id: "another-one".into(),
            task_id: "".into(),
            row_y: 40,
            row_height: 36,
            name: "another-one".into(),
            branch: "slint-daemon-poc-clean".into(),
            metadata: "".into(),
            path: "~/.another-one/worktrees/another-one".into(),
            initials: "A".into(),
            accent: project_accent_color("another-one"),
            active: false,
            expanded: true,
            has_children: true,
            task_count_label: "3".into(),
            pinned: false,
            worktree: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        SidebarTreeEntry {
            kind: "task".into(),
            id: "task:slint-build".into(),
            group_id: "another-one".into(),
            project_id: "another-one".into(),
            task_id: "slint-build".into(),
            row_y: 76,
            row_height: 46,
            name: "Slint build".into(),
            branch: "slint-daemon-poc-clean".into(),
            metadata: "active | +0 -0".into(),
            path: "".into(),
            initials: "S".into(),
            accent: project_accent_color("slint-build"),
            active: true,
            expanded: false,
            has_children: false,
            task_count_label: "".into(),
            pinned: true,
            worktree: true,
            running: true,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        SidebarTreeEntry {
            kind: "task".into(),
            id: "task:terminal-ready".into(),
            group_id: "another-one".into(),
            project_id: "another-one".into(),
            task_id: "terminal-ready".into(),
            row_y: 122,
            row_height: 46,
            name: "Terminal readiness".into(),
            branch: "terminal-production".into(),
            metadata: "in progress | renderer".into(),
            path: "".into(),
            initials: "T".into(),
            accent: project_accent_color("terminal-ready"),
            active: false,
            expanded: false,
            has_children: false,
            task_count_label: "".into(),
            pinned: false,
            worktree: true,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        SidebarTreeEntry {
            kind: "task".into(),
            id: "task:style-system".into(),
            group_id: "another-one".into(),
            project_id: "another-one".into(),
            task_id: "style-system".into(),
            row_y: 168,
            row_height: 46,
            name: "Style system".into(),
            branch: "gpui baseline".into(),
            metadata: "blocked | visual corpus".into(),
            path: "".into(),
            initials: "G".into(),
            accent: project_accent_color("style-system"),
            active: false,
            expanded: false,
            has_children: false,
            task_count_label: "".into(),
            pinned: false,
            worktree: true,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        SidebarTreeEntry {
            kind: "project".into(),
            id: "project:daemon-sandbox".into(),
            group_id: "daemon-sandbox".into(),
            project_id: "daemon-sandbox".into(),
            task_id: "".into(),
            row_y: 214,
            row_height: 36,
            name: "daemon-sandbox".into(),
            branch: "daemon transport".into(),
            metadata: "".into(),
            path: "daemon-sandbox".into(),
            initials: "D".into(),
            accent: project_accent_color("daemon-sandbox"),
            active: false,
            expanded: true,
            has_children: false,
            task_count_label: "0".into(),
            pinned: false,
            worktree: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        SidebarTreeEntry {
            kind: "project".into(),
            id: "project:slint-platform".into(),
            group_id: "slint-platform".into(),
            project_id: "slint-platform".into(),
            task_id: "".into(),
            row_y: 250,
            row_height: 36,
            name: "slint-platform".into(),
            branch: "platform traits".into(),
            metadata: "".into(),
            path: "slint-platform".into(),
            initials: "S".into(),
            accent: project_accent_color("slint-platform"),
            active: false,
            expanded: false,
            has_children: true,
            task_count_label: "2".into(),
            pinned: false,
            worktree: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
    ])));
    app.set_project_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        ProjectSidebarEntry {
            id: "another-one".into(),
            name: "another-one".into(),
            path: "~/.another-one/worktrees/another-one".into(),
            branch: "slint-daemon-poc-clean".into(),
            initials: "A".into(),
            accent: project_accent_color("another-one"),
            active: true,
            loading: false,
            error: false,
            expanded: true,
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
            loading: false,
            error: false,
            expanded: true,
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
            loading: false,
            error: false,
            expanded: false,
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
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
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
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
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
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
    ])));
    app.set_tab_chips(slint::ModelRc::new(slint::VecModel::from(vec![
        TerminalTabChip {
            id: "main".into(),
            title: "Codex".into(),
            provider: "codex".into(),
            restore_status: "ready".into(),
            failure_message: "".into(),
            failure_details: "".into(),
            active: true,
            running: true,
            pinned: false,
        },
        TerminalTabChip {
            id: "shell".into(),
            title: "Shell".into(),
            provider: "shell".into(),
            restore_status: "not-started".into(),
            failure_message: "".into(),
            failure_details: "".into(),
            active: false,
            running: false,
            pinned: false,
        },
    ])));
}

fn seed_visual_state_fixture(app: &AppWindow) {
    let Ok(state) = std::env::var("ANOTHERONE_SLINT_VISUAL_STATE") else {
        return;
    };

    match state.as_str() {
        "new-task-modal" => {
            app.set_modal_open(true);
            app.set_new_task_name("Review GPUI parity".into());
            app.set_new_task_branch("slint-visual-fixture".into());
        }
        "toast-error" => {
            app.set_toast_kind("error".into());
            app.set_toast_message("Could not open terminal link".into());
            app.set_toast_detail(
                "https://example.test returned an unsupported platform action".into(),
            );
        }
        "layout-collapsed" => {
            app.set_left_sidebar_open(false);
            app.set_right_inspector_open(false);
            app.set_resource_popover_open(true);
        }
        _ => {}
    }
}

fn seed_terminal_fidelity_fixture(app: &AppWindow) {
    let mut terminal = AlacrittySnapshot::new(100, 34);
    let _ = terminal.apply_output(TERMINAL_FIDELITY_FIXTURE);
    apply_terminal_surface(app, terminal.snapshot_surface());
    app.set_terminal_selection_spans(slint::ModelRc::new(slint::VecModel::from(
        selection_spans_for_points(
            TerminalCellPoint {
                line: 1,
                column: 11,
            },
            TerminalCellPoint {
                line: 1,
                column: 24,
            },
            100,
            34,
        ),
    )));
    app.set_active_project_name("terminal-fidelity-fixture".into());
    app.set_active_task_name("Slint terminal visual gate".into());
    app.set_active_branch_name("slint-daemon-poc-clean".into());
    app.set_active_worktree_name("fixture-mode".into());
    app.set_project_summary("fixture".into());
    app.set_tab_chips(slint::ModelRc::new(slint::VecModel::from(vec![
        TerminalTabChip {
            id: "terminal-fidelity".into(),
            title: "Fidelity".into(),
            provider: "fixture".into(),
            restore_status: "ready".into(),
            failure_message: "".into(),
            failure_details: "".into(),
            active: true,
            running: false,
            pinned: true,
        },
        TerminalTabChip {
            id: "cursor-selection-link".into(),
            title: "Cursor/Link".into(),
            provider: "fixture".into(),
            restore_status: "ready".into(),
            failure_message: "".into(),
            failure_details: "".into(),
            active: false,
            running: false,
            pinned: false,
        },
    ])));
    app.set_terminal_status(
        "terminal fidelity fixture: ANSI/indexed/truecolor, graphemes, wide cells, OSC8 link, selection, cursor"
            .into(),
    );
}

fn seed_component_state_fixture(app: &AppWindow) {
    app.set_component_fixture_mode(true);
    app.set_active_project_name("component-fixture".into());
    app.set_active_task_name("Base component states".into());
    app.set_active_branch_name("slint-component-fixture".into());
    app.set_active_worktree_name("fixture-mode".into());
    app.set_project_summary("fixture rows".into());
    app.set_project_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        ProjectSidebarEntry {
            id: "project-active".into(),
            name: "active project".into(),
            path: "~/another-one".into(),
            branch: "slint-daemon-poc-clean".into(),
            initials: "A".into(),
            accent: project_accent_color("project-active"),
            active: true,
            loading: false,
            error: false,
            expanded: true,
            task_count_label: "5".into(),
        },
        ProjectSidebarEntry {
            id: "project-loading".into(),
            name: "loading project".into(),
            path: "~/daemon".into(),
            branch: "waiting".into(),
            initials: "L".into(),
            accent: project_accent_color("project-loading"),
            active: false,
            loading: true,
            error: false,
            expanded: true,
            task_count_label: "...".into(),
        },
        ProjectSidebarEntry {
            id: "project-error".into(),
            name: "errored project".into(),
            path: "~/missing".into(),
            branch: "unavailable".into(),
            initials: "E".into(),
            accent: project_accent_color("project-error"),
            active: false,
            loading: false,
            error: true,
            expanded: false,
            task_count_label: "0".into(),
        },
    ])));
    app.set_task_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        TaskSidebarEntry {
            id: "task-active".into(),
            title: "Active running task".into(),
            branch: "feature/slint".into(),
            metadata: "active | +12 -3".into(),
            initials: "R".into(),
            accent: project_accent_color("task-active"),
            active: true,
            pinned: true,
            running: true,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        TaskSidebarEntry {
            id: "task-editing".into(),
            title: "Rename in progress".into(),
            branch: "feature/edit".into(),
            metadata: "editing".into(),
            initials: "E".into(),
            accent: project_accent_color("task-editing"),
            active: false,
            pinned: false,
            running: false,
            loading: false,
            error: false,
            editing: true,
            delete_confirm: false,
        },
        TaskSidebarEntry {
            id: "task-loading".into(),
            title: "Loading daemon state".into(),
            branch: "feature/loading".into(),
            metadata: "loading".into(),
            initials: "L".into(),
            accent: project_accent_color("task-loading"),
            active: false,
            pinned: false,
            running: false,
            loading: true,
            error: false,
            editing: false,
            delete_confirm: false,
        },
        TaskSidebarEntry {
            id: "task-delete".into(),
            title: "Delete confirmation".into(),
            branch: "feature/delete".into(),
            metadata: "confirm delete".into(),
            initials: "D".into(),
            accent: project_accent_color("task-delete"),
            active: false,
            pinned: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: true,
        },
        TaskSidebarEntry {
            id: "task-error".into(),
            title: "Errored task".into(),
            branch: "feature/error".into(),
            metadata: "failed".into(),
            initials: "X".into(),
            accent: project_accent_color("task-error"),
            active: false,
            pinned: false,
            running: false,
            loading: false,
            error: true,
            editing: false,
            delete_confirm: false,
        },
    ])));
    app.set_fixture_segments(slint::ModelRc::new(slint::VecModel::from(vec![
        SegmentEntry {
            id: "light".into(),
            label: "Light".into(),
            selected: false,
            disabled: false,
        },
        SegmentEntry {
            id: "dark".into(),
            label: "Dark".into(),
            selected: true,
            disabled: false,
        },
        SegmentEntry {
            id: "system".into(),
            label: "System".into(),
            selected: false,
            disabled: true,
        },
    ])));
    app.set_fixture_menu_entries(slint::ModelRc::new(slint::VecModel::from(vec![
        MenuEntry {
            id: "open".into(),
            label: "Open task".into(),
            shortcut: "Enter".into(),
            selected: false,
            disabled: false,
            destructive: false,
        },
        MenuEntry {
            id: "pin".into(),
            label: "Pin task".into(),
            shortcut: "P".into(),
            selected: true,
            disabled: false,
            destructive: false,
        },
        MenuEntry {
            id: "disabled".into(),
            label: "Unavailable".into(),
            shortcut: "".into(),
            selected: false,
            disabled: true,
            destructive: false,
        },
        MenuEntry {
            id: "delete".into(),
            label: "Delete task".into(),
            shortcut: "Del".into(),
            selected: false,
            disabled: false,
            destructive: true,
        },
    ])));
}

fn apply_terminal_surface(app: &AppWindow, surface: TerminalSurface) {
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

#[derive(Default)]
struct WorkspaceShellModel {
    sidebar_rows: Vec<SidebarTreeEntry>,
    project_rows: Vec<ProjectSidebarEntry>,
    task_rows: Vec<TaskSidebarEntry>,
    tab_chips: Vec<TerminalTabChip>,
    active_project_name: String,
    active_task_name: String,
    active_branch_name: String,
    active_worktree_name: String,
    active_project_path: String,
    terminal_panel_state: String,
    terminal_panel_title: String,
    terminal_panel_body: String,
    terminal_panel_project: String,
    terminal_panel_branch: String,
    terminal_panel_task: String,
    terminal_panel_tab: String,
    terminal_panel_cwd: String,
    terminal_error_details: String,
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
            app.set_sidebar_rows(slint::ModelRc::new(slint::VecModel::from(
                model.sidebar_rows,
            )));
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
            app.set_terminal_panel_state(model.terminal_panel_state.into());
            app.set_terminal_panel_title(model.terminal_panel_title.into());
            app.set_terminal_panel_body(model.terminal_panel_body.into());
            app.set_terminal_panel_project(model.terminal_panel_project.into());
            app.set_terminal_panel_branch(model.terminal_panel_branch.into());
            app.set_terminal_panel_task(model.terminal_panel_task.into());
            app.set_terminal_panel_tab(model.terminal_panel_tab.into());
            app.set_terminal_panel_cwd(model.terminal_panel_cwd.into());
            app.set_terminal_error_details(model.terminal_error_details.into());
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

fn set_right_inspector_loading(app_weak: &slint::Weak<AppWindow>, mode: &str, project_id: &str) {
    let title = match mode {
        "commits" => "Loading commits",
        "checks" => "Loading checks",
        _ => "Loading changes",
    };
    let summary = match mode {
        "commits" => "Recent commit data is requested from the daemon.",
        "checks" => "Pull request check data is requested from the daemon.",
        _ => "Working-tree changes are requested from the daemon.",
    };
    set_right_inspector_state(
        app_weak,
        mode,
        "loading",
        title,
        summary,
        format!("Project: {project_id}"),
        Vec::new(),
    );
}

fn set_right_inspector_deferred(app_weak: &slint::Weak<AppWindow>, mode: &str, project_id: &str) {
    let (title, summary) = match mode {
        "commits" => (
            "Commits pending",
            "The Slint toolbar mode is in place; commit rows are the next right-inspector slice.",
        ),
        "checks" => (
            "Checks pending",
            "The Slint toolbar mode is in place; check rows are the next right-inspector slice.",
        ),
        _ => (
            "Compare pending",
            "Compare mode is tracked separately from the current Changes slice.",
        ),
    };
    set_right_inspector_state(
        app_weak,
        mode,
        "deferred",
        title,
        summary,
        format!("Project: {project_id}"),
        Vec::new(),
    );
}

fn set_right_inspector_changes_with_collapsed(
    app_weak: &slint::Weak<AppWindow>,
    mode: &str,
    project_id: &str,
    files: Option<Vec<frame::ChangedFileWire>>,
    collapsed_sections: &HashSet<String>,
) {
    match files {
        None => set_right_inspector_state(
            app_weak,
            mode,
            "unavailable",
            "Changes unavailable",
            "The daemon did not recognize the active project for changed-file lookup.",
            format!("Project: {project_id}"),
            Vec::new(),
        ),
        Some(files) if files.is_empty() => set_right_inspector_state(
            app_weak,
            mode,
            "clean",
            "Working tree clean",
            "No staged or unstaged changes were reported by the daemon.",
            format!("Project: {project_id}"),
            Vec::new(),
        ),
        Some(files) => {
            let (rows, summary) = right_inspector_rows_for_changed_files_with_collapsed(
                project_id,
                &files,
                collapsed_sections,
            );
            set_right_inspector_state(
                app_weak,
                mode,
                "dirty",
                "Working tree changes",
                summary,
                format!("Project: {project_id}"),
                rows,
            );
        }
    }
}

fn set_right_inspector_commits(
    app_weak: &slint::Weak<AppWindow>,
    project_id: &str,
    view: &frame::RecentCommitsWire,
    expanded_commits: &HashSet<String>,
    file_change_states: &HashMap<String, InspectorCommitFileChangesState>,
) {
    if view.commits.is_empty() {
        set_right_inspector_state(
            app_weak,
            "commits",
            "clean",
            "No commits yet",
            "No commits were found on this branch.",
            view.current_branch
                .clone()
                .map(|branch| format!("Branch: {branch}"))
                .unwrap_or_else(|| format!("Project: {project_id}")),
            Vec::new(),
        );
        return;
    }

    let summary = if view.has_more {
        format!("{} commits shown; more are available.", view.commits.len())
    } else {
        format!("{} recent commits.", view.commits.len())
    };
    let detail = view
        .current_branch
        .clone()
        .map(|branch| format!("Branch: {branch}"))
        .unwrap_or_else(|| format!("Project: {project_id}"));
    let rows = right_inspector_rows_for_commits_with_expansions(
        project_id,
        view,
        expanded_commits,
        file_change_states,
    );
    set_right_inspector_state(
        app_weak,
        "commits",
        "dirty",
        "Recent commits",
        summary,
        detail,
        rows,
    );
}

fn set_right_inspector_error(
    app_weak: &slint::Weak<AppWindow>,
    mode: &str,
    project_id: &str,
    message: impl Into<String>,
) {
    set_right_inspector_state(
        app_weak,
        mode,
        "error",
        "Inspector request failed",
        message.into(),
        format!("Project: {project_id}"),
        Vec::new(),
    );
}

fn set_right_inspector_state(
    app_weak: &slint::Weak<AppWindow>,
    mode: impl Into<String>,
    state: impl Into<String>,
    title: impl Into<String>,
    summary: impl Into<String>,
    detail: impl Into<String>,
    rows: Vec<RightInspectorRow>,
) {
    let app_weak = app_weak.clone();
    let mode = mode.into();
    let state = state.into();
    let title = title.into();
    let summary = summary.into();
    let detail = detail.into();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_right_inspector_mode(mode.into());
            app.set_right_inspector_state(state.into());
            app.set_right_inspector_title(title.into());
            app.set_right_inspector_summary(summary.into());
            app.set_right_inspector_detail(detail.into());
            app.set_right_inspector_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
        }
    });
}

fn set_right_inspector_compare_target(
    app_weak: &slint::Weak<AppWindow>,
    target_branch: Option<String>,
) {
    let app_weak = app_weak.clone();
    let available = target_branch
        .as_deref()
        .is_some_and(|target| !target.trim().is_empty());
    let target_branch = target_branch.unwrap_or_default();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_right_inspector_compare_available(available);
            app.set_right_inspector_compare_target(target_branch.into());
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
                loading: false,
                error: false,
                expanded: active,
                task_count_label: project.tasks.len().to_string().into(),
            }
        })
        .collect::<Vec<_>>();
    let sidebar_rows = sidebar_tree_rows(projects, active_section_id);

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
                    loading: false,
                    error: false,
                    editing: false,
                    delete_confirm: false,
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
                    restore_status: restore_status_label(&tab.restore_status).into(),
                    failure_message: tab.failure_message.clone().unwrap_or_default().into(),
                    failure_details: tab.failure_details.clone().unwrap_or_default().into(),
                    active: tab.id == active_tab_id,
                    running: tab.running,
                    pinned: tab.pinned,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let active_tab = active_task.and_then(|task| active_tab_for_task(task, active_tab_id));
    let terminal_panel = terminal_panel_model(active_project, active_task, active_tab);

    WorkspaceShellModel {
        sidebar_rows,
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
        terminal_panel_state: terminal_panel.state,
        terminal_panel_title: terminal_panel.title,
        terminal_panel_body: terminal_panel.body,
        terminal_panel_project: terminal_panel.project,
        terminal_panel_branch: terminal_panel.branch,
        terminal_panel_task: terminal_panel.task,
        terminal_panel_tab: terminal_panel.tab,
        terminal_panel_cwd: terminal_panel.cwd,
        terminal_error_details: terminal_panel.error_details,
        project_summary: format!("{} projects", projects.len()),
    }
}

struct TerminalPanelModel {
    state: String,
    title: String,
    body: String,
    project: String,
    branch: String,
    task: String,
    tab: String,
    cwd: String,
    error_details: String,
}

fn terminal_panel_model(
    project: Option<&frame::ProjectSummary>,
    task: Option<&frame::TaskSummary>,
    tab: Option<&frame::TabSummary>,
) -> TerminalPanelModel {
    let project_label = project
        .map(|project| project.id.clone())
        .unwrap_or_else(|| "Not available".to_string());
    let branch_label = task
        .map(|task| task.branch_name.clone())
        .or_else(|| project.and_then(|project| project.current_branch.clone()))
        .unwrap_or_else(|| "Not available".to_string());
    let task_label = task
        .map(|task| task.id.clone())
        .unwrap_or_else(|| "Not available".to_string());
    let tab_label = tab
        .map(|tab| tab.id.clone())
        .unwrap_or_else(|| "Not available".to_string());
    let cwd_label = project
        .map(|project| project.path.clone())
        .unwrap_or_else(|| "Not available".to_string());

    let (state, title, body, error_details) = match (task, tab) {
        (None, _) => (
            "empty",
            "Select a branch to get started",
            "Open a task from the project tree to attach a terminal.",
            "",
        ),
        (Some(_), None) => (
            "empty",
            "No active tabs",
            "This task has no open tabs. Add an agent tab to start working.",
            "",
        ),
        (Some(_), Some(tab)) => match restore_status_label(&tab.restore_status) {
            "launching" => (
                "launching",
                "Launching terminal",
                "The tab was created immediately and its PTY is launching in the background.",
                "",
            ),
            "failed" => (
                "failed",
                "Terminal launch failed",
                tab.failure_message
                    .as_deref()
                    .unwrap_or("The daemon reported a terminal launch failure."),
                tab.failure_details.as_deref().unwrap_or_default(),
            ),
            "not-started" => (
                "lazy",
                "Lazy restore",
                "This restored tab has metadata only. Opening it triggers launch or resume on demand.",
                "",
            ),
            _ => ("ready", "", "", ""),
        },
    };

    TerminalPanelModel {
        state: state.to_string(),
        title: title.to_string(),
        body: body.to_string(),
        project: project_label,
        branch: branch_label,
        task: task_label,
        tab: tab_label,
        cwd: cwd_label,
        error_details: error_details.to_string(),
    }
}

fn active_tab_for_task<'a>(
    task: &'a frame::TaskSummary,
    active_tab_id: &str,
) -> Option<&'a frame::TabSummary> {
    task.tabs
        .iter()
        .find(|tab| tab.id == active_tab_id)
        .or_else(|| task.tabs.iter().find(|tab| tab.id == task.active_tab_id))
        .or_else(|| task.tabs.first())
}

fn restore_status_label(status: &impl std::fmt::Debug) -> &'static str {
    match format!("{status:?}").as_str() {
        "NotStarted" => "not-started",
        "Launching" => "launching",
        "Failed" => "failed",
        _ => "ready",
    }
}

fn sidebar_tree_rows(
    projects: &[frame::ProjectSummary],
    active_section_id: &str,
) -> Vec<SidebarTreeEntry> {
    let mut rows = Vec::new();
    let mut row_y = SIDEBAR_TREE_TOP;

    for project in projects {
        let mut tasks = project.tasks.iter().collect::<Vec<_>>();
        tasks.sort_by(|left, right| {
            right
                .pinned
                .cmp(&left.pinned)
                .then_with(|| left.name.cmp(&right.name))
        });
        let has_children = !tasks.is_empty();
        // The desktop stores explicit expansion state. The daemon projection
        // does not carry it yet, so Slint keeps groups expanded to preserve the
        // source relationship instead of hiding child rows behind fake state.
        let expanded = has_children;

        rows.push(SidebarTreeEntry {
            kind: "project".into(),
            id: format!("project:{}", project.id).into(),
            group_id: project.id.clone().into(),
            project_id: project.id.clone().into(),
            task_id: "".into(),
            row_y,
            row_height: SIDEBAR_PROJECT_ROW_HEIGHT,
            name: project.name.clone().into(),
            branch: project
                .current_branch
                .as_deref()
                .unwrap_or_else(|| project_kind_label(project.kind))
                .into(),
            metadata: "".into(),
            path: compact_path(&project.path).into(),
            initials: initials(&project.name).into(),
            accent: project_accent_color(&project.id),
            active: false,
            expanded,
            has_children,
            task_count_label: tasks.len().to_string().into(),
            pinned: false,
            worktree: false,
            running: false,
            loading: false,
            error: false,
            editing: false,
            delete_confirm: false,
        });
        row_y += SIDEBAR_PROJECT_ROW_HEIGHT;

        if expanded {
            for task in tasks {
                let running = task.tabs.iter().any(|tab| tab.running);
                let worktree =
                    !task.target_project_id.is_empty() && task.target_project_id != project.id;
                rows.push(SidebarTreeEntry {
                    kind: "task".into(),
                    id: format!("task:{}", task.id).into(),
                    group_id: project.id.clone().into(),
                    project_id: project.id.clone().into(),
                    task_id: task.id.clone().into(),
                    row_y,
                    row_height: SIDEBAR_TASK_ROW_HEIGHT,
                    name: task.name.clone().into(),
                    branch: task.branch_name.clone().into(),
                    metadata: task_metadata(task, running).into(),
                    path: "".into(),
                    initials: initials(&task.name).into(),
                    accent: project_accent_color(&project.id),
                    active: task.section_id == active_section_id,
                    expanded: false,
                    has_children: false,
                    task_count_label: "".into(),
                    pinned: task.pinned,
                    worktree,
                    running,
                    loading: false,
                    error: false,
                    editing: false,
                    delete_confirm: false,
                });
                row_y += SIDEBAR_TASK_ROW_HEIGHT;
            }
        }
    }

    rows
}

fn right_inspector_rows_for_changed_files_with_collapsed(
    project_id: &str,
    files: &[frame::ChangedFileWire],
    collapsed_sections: &HashSet<String>,
) -> (Vec<RightInspectorRow>, String) {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut staged_additions = 0;
    let mut staged_deletions = 0;
    let mut unstaged_additions = 0;
    let mut unstaged_deletions = 0;

    for file in files {
        if changed_file_has_staged_changes(file) {
            staged_additions += file.staged_additions.max(0);
            staged_deletions += file.staged_deletions.max(0);
            staged.push(file);
        }
        if changed_file_has_unstaged_changes(file) {
            unstaged_additions += file.unstaged_additions.max(0);
            unstaged_deletions += file.unstaged_deletions.max(0);
            unstaged.push(file);
        }
    }

    let mut rows = Vec::new();
    let mut row_y = 0;
    if !staged.is_empty() {
        let expanded = !collapsed_sections.contains("staged");
        rows.push(right_inspector_section_row_with_expanded(
            project_id,
            "staged",
            "Staged Changes",
            staged.len(),
            staged_additions,
            staged_deletions,
            row_y,
            expanded,
        ));
        row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;
        if expanded {
            for file in &staged {
                rows.push(right_inspector_file_row(project_id, "staged", file, row_y));
                row_y += RIGHT_INSPECTOR_FILE_ROW_HEIGHT;
            }
        }
    }
    if !unstaged.is_empty() {
        let expanded = !collapsed_sections.contains("unstaged");
        rows.push(right_inspector_section_row_with_expanded(
            project_id,
            "unstaged",
            "Changes",
            unstaged.len(),
            unstaged_additions,
            unstaged_deletions,
            row_y,
            expanded,
        ));
        row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;
        if expanded {
            for file in &unstaged {
                rows.push(right_inspector_file_row(
                    project_id, "unstaged", file, row_y,
                ));
                row_y += RIGHT_INSPECTOR_FILE_ROW_HEIGHT;
            }
        }
    }

    let summary = format!("{} staged, {} unstaged", staged.len(), unstaged.len());
    (rows, summary)
}

fn right_inspector_section_row(
    project_id: &str,
    group: &str,
    title: &str,
    file_count: usize,
    additions: i32,
    deletions: i32,
    row_y: i32,
) -> RightInspectorRow {
    right_inspector_section_row_with_expanded(
        project_id, group, title, file_count, additions, deletions, row_y, true,
    )
}

fn right_inspector_section_row_with_expanded(
    project_id: &str,
    group: &str,
    title: &str,
    file_count: usize,
    additions: i32,
    deletions: i32,
    row_y: i32,
    expanded: bool,
) -> RightInspectorRow {
    RightInspectorRow {
        kind: "section".into(),
        group: group.into(),
        id: format!("section:{group}").into(),
        project_id: project_id.into(),
        path: "".into(),
        original_path: "".into(),
        row_y,
        row_height: RIGHT_INSPECTOR_SECTION_ROW_HEIGHT,
        title: title.into(),
        parent_dir: "".into(),
        status: "".into(),
        status_color: slint::Color::from_argb_encoded(0xff949494),
        additions_label: diff_label(additions, true).into(),
        deletions_label: diff_label(deletions, false).into(),
        file_count_label: file_count.to_string().into(),
        can_stage: group == "unstaged",
        can_unstage: group == "staged",
        untracked: false,
        expanded,
    }
}

fn right_inspector_file_row(
    project_id: &str,
    group: &str,
    file: &frame::ChangedFileWire,
    row_y: i32,
) -> RightInspectorRow {
    let status = changed_file_status_char(file, group);
    let (file_name, parent_dir) = file_name_and_parent(&file.path);
    let (additions, deletions) = if group == "staged" {
        (file.staged_additions, file.staged_deletions)
    } else {
        (file.unstaged_additions, file.unstaged_deletions)
    };

    RightInspectorRow {
        kind: "file".into(),
        group: group.into(),
        id: format!(
            "{group}:{}:{}",
            file.original_path.as_deref().unwrap_or(""),
            file.path
        )
        .into(),
        project_id: project_id.into(),
        path: file.path.clone().into(),
        original_path: file.original_path.clone().unwrap_or_default().into(),
        row_y,
        row_height: RIGHT_INSPECTOR_FILE_ROW_HEIGHT,
        title: file_name.into(),
        parent_dir: parent_dir.into(),
        status: status.to_string().into(),
        status_color: changed_file_status_color(status),
        additions_label: diff_label(additions, true).into(),
        deletions_label: diff_label(deletions, false).into(),
        file_count_label: "".into(),
        can_stage: changed_file_has_unstaged_changes(file),
        can_unstage: changed_file_has_staged_changes(file),
        untracked: file.untracked,
        expanded: false,
    }
}

fn right_inspector_rows_for_commits_with_expansions(
    project_id: &str,
    view: &frame::RecentCommitsWire,
    expanded_commits: &HashSet<String>,
    file_change_states: &HashMap<String, InspectorCommitFileChangesState>,
) -> Vec<RightInspectorRow> {
    let mut rows = Vec::new();
    let mut row_y = 0;
    let branch = view.current_branch.as_deref().unwrap_or("current branch");
    rows.push(right_inspector_section_row(
        project_id,
        "commits",
        &format!("Recent commits on {branch}"),
        view.commits.len(),
        0,
        0,
        row_y,
    ));
    row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;

    for commit in &view.commits {
        let commit_key = right_inspector_commit_key(project_id, &commit.id);
        let expanded = expanded_commits.contains(&commit_key);
        rows.push(RightInspectorRow {
            kind: "commit".into(),
            group: "commits".into(),
            id: format!("commit:{}", commit.id).into(),
            project_id: project_id.into(),
            path: commit.id.clone().into(),
            original_path: "".into(),
            row_y,
            row_height: RIGHT_INSPECTOR_COMMIT_ROW_HEIGHT,
            title: commit.subject.clone().into(),
            parent_dir: format!("{} - {}", commit.author_name, commit.authored_relative).into(),
            status: commit.short_id.clone().into(),
            status_color: slint::Color::from_argb_encoded(0xff949494),
            additions_label: "".into(),
            deletions_label: "".into(),
            file_count_label: "".into(),
            can_stage: false,
            can_unstage: false,
            untracked: false,
            expanded,
        });
        row_y += RIGHT_INSPECTOR_COMMIT_ROW_HEIGHT;

        if expanded {
            let state = file_change_states.get(&commit_key);
            rows.extend(right_inspector_commit_detail_rows(
                project_id, commit, state, &mut row_y,
            ));
        }
    }

    rows
}

fn right_inspector_commit_detail_rows(
    project_id: &str,
    commit: &frame::CommitWire,
    state: Option<&InspectorCommitFileChangesState>,
    row_y: &mut i32,
) -> Vec<RightInspectorRow> {
    let mut rows = Vec::new();
    let (title, count, additions, deletions) = match state {
        Some(InspectorCommitFileChangesState::Loaded(files)) if !files.is_empty() => {
            let additions = files.iter().map(|file| file.additions.max(0)).sum();
            let deletions = files.iter().map(|file| file.deletions.max(0)).sum();
            let title = if files.len() == 1 {
                "1 file changed".to_string()
            } else {
                format!("{} files changed", files.len())
            };
            (title, files.len(), additions, deletions)
        }
        Some(InspectorCommitFileChangesState::Loaded(_)) => {
            ("No file changes in this commit.".to_string(), 0, 0, 0)
        }
        Some(InspectorCommitFileChangesState::Failed) => {
            ("Couldn't load file changes.".to_string(), 0, 0, 0)
        }
        _ => ("Loading file changes...".to_string(), 0, 0, 0),
    };

    rows.push(right_inspector_section_row(
        project_id,
        "commit-files",
        &title,
        count,
        additions,
        deletions,
        *row_y,
    ));
    *row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;

    if let Some(InspectorCommitFileChangesState::Loaded(files)) = state {
        for file in files {
            rows.push(right_inspector_commit_file_row(
                project_id, &commit.id, file, *row_y,
            ));
            *row_y += RIGHT_INSPECTOR_FILE_ROW_HEIGHT;
        }
    }

    rows
}

fn right_inspector_commit_file_row(
    project_id: &str,
    commit_id: &str,
    file: &frame::BranchCompareFileWire,
    row_y: i32,
) -> RightInspectorRow {
    let mut row = right_inspector_compare_file_row(project_id, file, row_y);
    row.group = "commit-file".into();
    row.id = format!("commit-file:{commit_id}:{}", file.path).into();
    row
}

fn right_inspector_commit_key(project_id: &str, commit_id: &str) -> String {
    format!("{project_id}:{commit_id}")
}

fn right_inspector_rows_for_checks(
    project_id: &str,
    checks: &[frame::Check],
) -> Vec<RightInspectorRow> {
    let mut checks = checks.iter().collect::<Vec<_>>();
    checks.sort_by(|left, right| {
        check_bucket_priority(&left.bucket)
            .cmp(&check_bucket_priority(&right.bucket))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    let mut rows = Vec::new();
    let mut row_y = 0;
    rows.push(right_inspector_section_row(
        project_id,
        "checks",
        "Pull request checks",
        checks.len(),
        0,
        0,
        row_y,
    ));
    row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;

    for check in checks {
        let (bucket, color) = check_bucket_visual(&check.bucket);
        rows.push(RightInspectorRow {
            kind: "check".into(),
            group: bucket.into(),
            id: format!("check:{}", check.name).into(),
            project_id: project_id.into(),
            path: check.link.clone().unwrap_or_default().into(),
            original_path: "".into(),
            row_y,
            row_height: RIGHT_INSPECTOR_CHECK_ROW_HEIGHT,
            title: check.name.clone().into(),
            parent_dir: check
                .description
                .clone()
                .unwrap_or_else(|| check.state.clone())
                .into(),
            status: check.state.clone().into(),
            status_color: color,
            additions_label: check.duration_text.clone().unwrap_or_default().into(),
            deletions_label: "".into(),
            file_count_label: "".into(),
            can_stage: false,
            can_unstage: false,
            untracked: false,
            expanded: false,
        });
        row_y += RIGHT_INSPECTOR_CHECK_ROW_HEIGHT;
    }

    rows
}

fn right_inspector_rows_for_compare(
    project_id: &str,
    view: &frame::BranchCompareWire,
) -> Vec<RightInspectorRow> {
    let mut rows = Vec::new();
    let mut row_y = 0;
    let additions = view.files.iter().map(|file| file.additions.max(0)).sum();
    let deletions = view.files.iter().map(|file| file.deletions.max(0)).sum();
    let current_branch = view.current_branch.as_deref().unwrap_or("current branch");
    rows.push(right_inspector_section_row(
        project_id,
        "compare",
        &format!("Comparing {current_branch} against {}", view.target_branch),
        view.files.len(),
        additions,
        deletions,
        row_y,
    ));
    row_y += RIGHT_INSPECTOR_SECTION_ROW_HEIGHT;

    for file in &view.files {
        rows.push(right_inspector_compare_file_row(project_id, file, row_y));
        row_y += RIGHT_INSPECTOR_FILE_ROW_HEIGHT;
    }

    rows
}

fn right_inspector_compare_file_row(
    project_id: &str,
    file: &frame::BranchCompareFileWire,
    row_y: i32,
) -> RightInspectorRow {
    let status = status_char(&file.status);
    let (file_name, parent_dir) = file_name_and_parent(&file.path);
    let parent_label = file
        .original_path
        .as_ref()
        .map(|original_path| format!("Renamed from {original_path}"))
        .unwrap_or(parent_dir);

    RightInspectorRow {
        kind: "file".into(),
        group: "compare".into(),
        id: format!(
            "compare:{}:{}",
            file.original_path.as_deref().unwrap_or(""),
            file.path
        )
        .into(),
        project_id: project_id.into(),
        path: file.path.clone().into(),
        original_path: file.original_path.clone().unwrap_or_default().into(),
        row_y,
        row_height: RIGHT_INSPECTOR_FILE_ROW_HEIGHT,
        title: file_name.into(),
        parent_dir: parent_label.into(),
        status: status.to_string().into(),
        status_color: changed_file_status_color(status),
        additions_label: diff_label(file.additions, true).into(),
        deletions_label: diff_label(file.deletions, false).into(),
        file_count_label: "".into(),
        can_stage: false,
        can_unstage: false,
        untracked: false,
        expanded: false,
    }
}

fn check_bucket_priority(bucket: &impl std::fmt::Debug) -> u8 {
    match format!("{bucket:?}").as_str() {
        "Fail" => 0,
        "Pending" => 1,
        "Pass" => 2,
        _ => 3,
    }
}

fn check_bucket_visual(bucket: &impl std::fmt::Debug) -> (&'static str, slint::Color) {
    match format!("{bucket:?}").as_str() {
        "Pass" => ("pass", slint::Color::from_argb_encoded(0xff8bd99c)),
        "Fail" => ("fail", slint::Color::from_argb_encoded(0xffe58b95)),
        "Pending" => ("pending", slint::Color::from_argb_encoded(0xffe6c36d)),
        _ => ("skipped", slint::Color::from_argb_encoded(0xff8f8f8f)),
    }
}

fn changed_file_has_staged_changes(file: &frame::ChangedFileWire) -> bool {
    let status = status_char(&file.index_status);
    status != ' ' && status != '?'
}

fn changed_file_has_unstaged_changes(file: &frame::ChangedFileWire) -> bool {
    let status = status_char(&file.worktree_status);
    file.untracked || (status != ' ' && status != '?')
}

fn changed_file_status_char(file: &frame::ChangedFileWire, group: &str) -> char {
    let raw = if group == "staged" {
        status_char(&file.index_status)
    } else if file.untracked {
        'A'
    } else {
        status_char(&file.worktree_status)
    };

    match raw {
        '?' | ' ' => 'A',
        other => other,
    }
}

fn changed_file_status_color(status: char) -> slint::Color {
    let rgb = match status {
        'A' => 0x85dda0,
        'D' => 0xe47777,
        'R' | 'C' => 0x86c6e9,
        _ => 0xe6cc4d,
    };
    slint::Color::from_argb_encoded(0xff000000 | rgb)
}

fn status_char(status: &str) -> char {
    status.chars().next().unwrap_or(' ')
}

fn diff_label(value: i32, positive: bool) -> String {
    if value <= 0 {
        String::new()
    } else if positive {
        format!("+{value}")
    } else {
        format!("-{value}")
    }
}

fn file_name_and_parent(path: &str) -> (String, String) {
    let path_ref = Path::new(path);
    let file_name = path_ref
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string();
    let parent = path_ref
        .parent()
        .and_then(|parent| parent.to_str())
        .filter(|parent| !parent.is_empty() && *parent != ".")
        .unwrap_or_default()
        .to_string();
    (file_name, parent)
}

fn first_attachable_target(projects: &[frame::ProjectSummary]) -> Option<TerminalTarget> {
    projects
        .iter()
        .find_map(|project| project.tasks.iter().find_map(target_for_task))
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

async fn request_right_inspector_data(
    app_weak: &slint::Weak<AppWindow>,
    send: &mut (impl frame::WriteAllAsync + Unpin),
    next_request_id: &mut u64,
    mode: &str,
    project_id: &str,
) -> anyhow::Result<()> {
    if project_id.trim().is_empty() {
        set_right_inspector_compare_target(app_weak, None);
        set_right_inspector_state(
            app_weak,
            mode,
            "unavailable",
            "No active project",
            "Select a daemon-backed project or task to inspect git state.",
            "",
            Vec::new(),
        );
        return Ok(());
    }

    send_control(
        send,
        next_request_id,
        Control::ReadBranchSettings {
            project_id: project_id.to_string(),
        },
    )
    .await
    .context("read branch settings")?;

    match mode {
        "changes" => {
            set_right_inspector_loading(app_weak, mode, project_id);
            send_control(
                send,
                next_request_id,
                Control::ReadChangedFiles {
                    project_id: project_id.to_string(),
                },
            )
            .await
            .context("read changed files")?;
        }
        "commits" => {
            set_right_inspector_loading(app_weak, mode, project_id);
            send_control(
                send,
                next_request_id,
                Control::ReadRecentCommits {
                    project_id: project_id.to_string(),
                    limit: 20,
                },
            )
            .await
            .context("read recent commits")?;
        }
        "checks" => {
            set_right_inspector_loading(app_weak, mode, project_id);
            send_control(
                send,
                next_request_id,
                Control::ReadPullRequestChecks {
                    project_id: project_id.to_string(),
                },
            )
            .await
            .context("read pull request checks")?;
        }
        "compare" => {
            set_right_inspector_state(
                app_weak,
                mode,
                "loading",
                "Loading compare view",
                "Resolving the configured target branch from daemon settings.",
                format!("Project: {project_id}"),
                Vec::new(),
            );
        }
        other => set_right_inspector_deferred(app_weak, other, project_id),
    }

    Ok(())
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

    let mut right_inspector_mode = "changes".to_string();
    let mut right_inspector_project_id =
        project_id_for_target(&projects, &attached_target).unwrap_or_default();
    let mut right_inspector_changed_files: Option<Vec<frame::ChangedFileWire>> = None;
    let mut collapsed_inspector_sections = HashSet::new();
    let mut right_inspector_recent_commits: Option<frame::RecentCommitsWire> = None;
    let mut expanded_inspector_commits = HashSet::new();
    let mut inspector_commit_file_change_states = HashMap::new();
    let mut pending_inspector_commit_file_requests = HashMap::new();
    request_right_inspector_data(
        app_weak,
        &mut send,
        &mut next_request_id,
        &right_inspector_mode,
        &right_inspector_project_id,
    )
    .await?;

    let mut terminal = AlacrittySnapshot::new(TERMINAL_COLS, TERMINAL_ROWS);
    attach_terminal_target(
        &mut send,
        &mut next_request_id,
        &attached_target,
        terminal.size,
        true,
    )
    .await?;
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
            "terminal: attached {}/{} at {}x{}",
            attached_target.section_id,
            attached_target.tab_id,
            terminal.size.cols,
            terminal.size.rows
        ),
    );

    let mut dirty = true;
    let mut last_flush = Instant::now()
        .checked_sub(TERMINAL_FRAME_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut pending_flush_at = Some(Instant::now());
    let mut pending_resize = None;
    let mut pending_resize_at = None;
    let mut selection_drag_anchor = None;
    let mut selection_range = None;

    loop {
        tokio::select! {
            maybe_event = client_event_rx.recv() => {
                let Some(event) = maybe_event else {
                    anyhow::bail!("client event channel closed");
                };
                match event {
                    SlintClientEvent::TerminalKey(input) => {
                        if is_copy_shortcut(&input) {
                            if let Some((anchor, focus)) = selection_range {
                                let selected_text = terminal.selected_text(anchor, focus);
                                if !selected_text.is_empty() {
                                    match platform::copy_text(&selected_text) {
                                        Ok(()) => set_toast(app_weak, "info", "Copied terminal selection", format!("{} bytes", selected_text.len())),
                                        Err(error) => set_toast(app_weak, "error", "Could not copy terminal selection", error),
                                    }
                                    continue;
                                }
                            }
                        }
                        if let Some(event) = encode_terminal_key(&input, terminal.input_modes()) {
                            send_terminal_input(&mut send, &mut next_request_id, event)
                                .await
                                .context("send terminal input")?;
                        }
                    }
                    SlintClientEvent::TerminalFocus(focused) => {
                        if terminal.input_modes().focus_in_out {
                            send_terminal_input(
                                &mut send,
                                &mut next_request_id,
                                TerminalInputEvent::Focus { focused },
                            )
                            .await
                            .context("send terminal focus input")?;
                        }
                    }
                    SlintClientEvent::TerminalPointer(input) => {
                        let input_modes = terminal.input_modes();
                        if let Some(event) = encode_terminal_pointer_event(&input, input_modes) {
                            send_terminal_input(&mut send, &mut next_request_id, event)
                                .await
                                .context("send terminal pointer input")?;
                        } else if input.is_primary_down() {
                            let surface = terminal.snapshot_surface();
                            if let Some(uri) = link_uri_at(&surface.link_spans, input.line, input.column) {
                                match platform::open_uri(&uri) {
                                    Ok(()) => set_toast(app_weak, "info", "Opened terminal link", uri),
                                    Err(error) => set_toast(app_weak, "error", "Could not open terminal link", error),
                                }
                            } else if let Some(point) = input.terminal_point() {
                                selection_drag_anchor = Some(point);
                                selection_range = None;
                                set_terminal_selection(app_weak, Vec::new());
                            }
                        } else if input.is_primary_move() {
                            if let (Some(anchor), Some(focus)) = (selection_drag_anchor, input.terminal_point()) {
                                selection_range = normalized_selection_points(anchor, focus);
                                set_terminal_selection(
                                    app_weak,
                                    selection_spans_for_points(anchor, focus, terminal.size.cols, terminal.size.rows),
                                );
                            }
                        } else if input.is_primary_up() {
                            if let Some(anchor) = selection_drag_anchor.take() {
                                if let Some(focus) = input.terminal_point() {
                                    selection_range = normalized_selection_points(anchor, focus);
                                    set_terminal_selection(
                                        app_weak,
                                        selection_spans_for_points(anchor, focus, terminal.size.cols, terminal.size.rows),
                                    );
                                }
                            }
                        }
                    }
                    SlintClientEvent::TerminalResize { cols, rows } => {
                        let next_size = TerminalSize {
                            cols: clamp_terminal_dimension(cols, 20, 240),
                            rows: clamp_terminal_dimension(rows, 8, 120),
                        };
                        if terminal.size != next_size {
                            pending_resize = Some(next_size);
                            pending_resize_at = Some(Instant::now() + TERMINAL_RESIZE_DEBOUNCE);
                        } else {
                            pending_resize = None;
                            pending_resize_at = None;
                        }
                    }
                    SlintClientEvent::SelectProject(project_id) => {
                        if let Some(project) = projects.iter().find(|project| project.id == project_id) {
                            set_project_overview_placeholder(app_weak, project);
                            right_inspector_project_id = project_id;
                            set_right_inspector_compare_target(app_weak, None);
                            right_inspector_recent_commits = None;
                            expanded_inspector_commits.clear();
                            inspector_commit_file_change_states.clear();
                            pending_inspector_commit_file_requests.clear();
                            request_right_inspector_data(
                                app_weak,
                                &mut send,
                                &mut next_request_id,
                                &right_inspector_mode,
                                &right_inspector_project_id,
                            )
                            .await?;
                            selection_drag_anchor = None;
                            selection_range = None;
                            set_terminal_selection(app_weak, Vec::new());
                        } else {
                            set_terminal_status(
                                app_weak,
                                format!("project unavailable: {project_id}"),
                            );
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
                            right_inspector_project_id =
                                project_id_for_target(&projects, &attached_target)
                                    .unwrap_or_default();
                            set_right_inspector_compare_target(app_weak, None);
                            right_inspector_recent_commits = None;
                            expanded_inspector_commits.clear();
                            inspector_commit_file_change_states.clear();
                            pending_inspector_commit_file_requests.clear();
                            request_right_inspector_data(
                                app_weak,
                                &mut send,
                                &mut next_request_id,
                                &right_inspector_mode,
                                &right_inspector_project_id,
                            )
                            .await?;
                            selection_drag_anchor = None;
                            selection_range = None;
                            set_terminal_selection(app_weak, Vec::new());
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
                            right_inspector_project_id =
                                project_id_for_target(&projects, &attached_target)
                                    .unwrap_or_default();
                            set_right_inspector_compare_target(app_weak, None);
                            right_inspector_recent_commits = None;
                            expanded_inspector_commits.clear();
                            inspector_commit_file_change_states.clear();
                            pending_inspector_commit_file_requests.clear();
                            request_right_inspector_data(
                                app_weak,
                                &mut send,
                                &mut next_request_id,
                                &right_inspector_mode,
                                &right_inspector_project_id,
                            )
                            .await?;
                            selection_drag_anchor = None;
                            selection_range = None;
                            set_terminal_selection(app_weak, Vec::new());
                        } else {
                            set_terminal_status(app_weak, format!("terminal: unknown tab: {tab_id}"));
                        }
                    }
                    SlintClientEvent::AddTerminalTab => {
                        send_control(
                            &mut send,
                            &mut next_request_id,
                            Control::AddAgentToSection {
                                section_id: attached_target.section_id.clone(),
                                agent_id: String::new(),
                            },
                        )
                        .await
                        .context("add terminal tab")?;
                    }
                    SlintClientEvent::CloseTerminalTab(tab_id) => {
                        send_control(
                            &mut send,
                            &mut next_request_id,
                            Control::CloseSectionTab {
                                section_id: attached_target.section_id.clone(),
                                tab_id,
                            },
                        )
                        .await
                        .context("close terminal tab")?;
                    }
                    SlintClientEvent::ToggleTerminalTabPinned(tab_id) => {
                        send_control(
                            &mut send,
                            &mut next_request_id,
                            Control::ToggleSectionTabPinned {
                                section_id: attached_target.section_id.clone(),
                                tab_id,
                            },
                        )
                        .await
                        .context("toggle terminal tab pin")?;
                    }
                    SlintClientEvent::RightInspectorMode(mode) => {
                        right_inspector_mode = mode;
                        request_right_inspector_data(
                            app_weak,
                            &mut send,
                            &mut next_request_id,
                            &right_inspector_mode,
                            &right_inspector_project_id,
                        )
                        .await?;
                    }
                    SlintClientEvent::StageChangedFile { path, original_path } => {
                        send_control(
                            &mut send,
                            &mut next_request_id,
                            Control::StageChangedFile {
                                project_id: right_inspector_project_id.clone(),
                                path,
                                original_path,
                            },
                        )
                        .await
                        .context("stage changed file")?;
                    }
                    SlintClientEvent::UnstageChangedFile { path, original_path } => {
                        send_control(
                            &mut send,
                            &mut next_request_id,
                            Control::UnstageChangedFile {
                                project_id: right_inspector_project_id.clone(),
                                path,
                                original_path,
                            },
                        )
                        .await
                        .context("unstage changed file")?;
                    }
                    SlintClientEvent::DiscardChangedFile {
                        path,
                        original_path,
                        untracked,
                    } => {
                        send_control(
                            &mut send,
                            &mut next_request_id,
                            Control::DiscardChangedFile {
                                project_id: right_inspector_project_id.clone(),
                                path,
                                untracked,
                                original_path,
                            },
                        )
                        .await
                        .context("discard changed file")?;
                    }
                    SlintClientEvent::ToggleInspectorCommit {
                        project_id,
                        commit_id,
                    } => {
                        if project_id != right_inspector_project_id {
                            continue;
                        }
                        let commit_key = right_inspector_commit_key(&project_id, &commit_id);
                        if !expanded_inspector_commits.insert(commit_key.clone()) {
                            expanded_inspector_commits.remove(&commit_key);
                        } else if !matches!(
                            inspector_commit_file_change_states.get(&commit_key),
                            Some(InspectorCommitFileChangesState::Loading)
                                | Some(InspectorCommitFileChangesState::Loaded(_))
                        ) {
                            inspector_commit_file_change_states
                                .insert(commit_key.clone(), InspectorCommitFileChangesState::Loading);
                            let request_id = next_request_id;
                            pending_inspector_commit_file_requests.insert(request_id, commit_key);
                            send_control(
                                &mut send,
                                &mut next_request_id,
                                Control::ReadCommitFileChanges {
                                    project_id,
                                    commit_id,
                                },
                            )
                            .await
                            .context("read commit file changes")?;
                        }

                        if let Some(view) = right_inspector_recent_commits.as_ref() {
                            set_right_inspector_commits(
                                app_weak,
                                &right_inspector_project_id,
                                view,
                                &expanded_inspector_commits,
                                &inspector_commit_file_change_states,
                            );
                        }
                    }
                    SlintClientEvent::OpenInspectorCheckLink(uri) => {
                        match platform::open_uri(&uri) {
                            Ok(()) => set_toast(app_weak, "info", "Opened check", uri),
                            Err(error) => {
                                set_toast(app_weak, "error", "Could not open check", error)
                            }
                        }
                    }
                    SlintClientEvent::ToggleInspectorSection(group) => {
                        if !collapsed_inspector_sections.insert(group.clone()) {
                            collapsed_inspector_sections.remove(&group);
                        }
                        if right_inspector_mode == "changes" {
                            set_right_inspector_changes_with_collapsed(
                                app_weak,
                                "changes",
                                &right_inspector_project_id,
                                right_inspector_changed_files.clone(),
                                &collapsed_inspector_sections,
                            );
                        }
                    }
                    SlintClientEvent::StageAllChanges => {
                        send_control(
                            &mut send,
                            &mut next_request_id,
                            Control::StageAllChanges {
                                project_id: right_inspector_project_id.clone(),
                            },
                        )
                        .await
                        .context("stage all changes")?;
                    }
                    SlintClientEvent::UnstageAllChanges => {
                        send_control(
                            &mut send,
                            &mut next_request_id,
                            Control::UnstageAllChanges {
                                project_id: right_inspector_project_id.clone(),
                            },
                        )
                        .await
                        .context("unstage all changes")?;
                    }
                    SlintClientEvent::SubmitNewTask {
                        task_name,
                        source_branch,
                        project_id,
                    } => {
                        let task_name = task_name.trim().to_string();
                        if task_name.is_empty() {
                            set_toast(app_weak, "error", "Task name is required", "Enter a task name before creating a task.");
                            continue;
                        }
                        let project_id = if project_id.trim().is_empty() {
                            let Some(project_id) = project_id_for_target(&projects, &attached_target) else {
                                set_toast(app_weak, "error", "No active project", "Select a daemon-backed project before creating a task.");
                                continue;
                            };
                            project_id
                        } else {
                            project_id.trim().to_string()
                        };
                        let requested_source_branch = source_branch.trim();
                        let source_branch = if !requested_source_branch.is_empty() {
                            Some(requested_source_branch.to_string())
                        } else {
                            projects
                                .iter()
                                .find(|project| project.id == project_id)
                                .and_then(|project| project.current_branch.clone())
                                .or_else(|| {
                                    normalized_source_branch(
                                        &projects,
                                        &attached_target,
                                        requested_source_branch,
                                    )
                                })
                        };
                        let Some(source_branch) = source_branch else {
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
                            let request_id = envelope.request_id;
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
                                        if right_inspector_project_id.is_empty() {
                                            right_inspector_project_id =
                                                project_id_for_target(&projects, &attached_target)
                                                    .unwrap_or_default();
                                            set_right_inspector_compare_target(app_weak, None);
                                            right_inspector_recent_commits = None;
                                            expanded_inspector_commits.clear();
                                            inspector_commit_file_change_states.clear();
                                            pending_inspector_commit_file_requests.clear();
                                        }
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
                                        right_inspector_project_id =
                                            project_id_for_target(&projects, &attached_target)
                                                .unwrap_or_default();
                                        set_right_inspector_compare_target(app_weak, None);
                                        right_inspector_recent_commits = None;
                                        expanded_inspector_commits.clear();
                                        inspector_commit_file_change_states.clear();
                                        pending_inspector_commit_file_requests.clear();
                                    } else {
                                        set_terminal_status(app_weak, "terminal: project tree has no attachable tabs");
                                    }
                                }
                                WorkerReply::Err { message, .. } => {
                                    if let Some(commit_key) =
                                        pending_inspector_commit_file_requests.remove(&request_id)
                                    {
                                        inspector_commit_file_change_states.insert(
                                            commit_key,
                                            InspectorCommitFileChangesState::Failed,
                                        );
                                        if let Some(view) = right_inspector_recent_commits.as_ref()
                                        {
                                            set_right_inspector_commits(
                                                app_weak,
                                                &right_inspector_project_id,
                                                view,
                                                &expanded_inspector_commits,
                                                &inspector_commit_file_change_states,
                                            );
                                        }
                                        set_toast(
                                            app_weak,
                                            "warning",
                                            "Could not load commit files",
                                            message,
                                        );
                                        continue;
                                    }
                                    set_terminal_status(app_weak, format!("terminal worker error: {message}"));
                                    set_right_inspector_error(
                                        app_weak,
                                        &right_inspector_mode,
                                        &right_inspector_project_id,
                                        message.clone(),
                                    );
                                    set_toast(app_weak, "error", "Daemon request failed", message);
                                }
                                WorkerReply::ChangedFilesAck { files } => {
                                    right_inspector_changed_files = files.clone();
                                    set_right_inspector_changes_with_collapsed(
                                        app_weak,
                                        &right_inspector_mode,
                                        &right_inspector_project_id,
                                        files,
                                        &collapsed_inspector_sections,
                                    );
                                }
                                WorkerReply::StageChangedFileAck { changed_files }
                                | WorkerReply::UnstageChangedFileAck { changed_files }
                                | WorkerReply::StageAllChangesAck { changed_files }
                                | WorkerReply::UnstageAllChangesAck { changed_files } => {
                                    right_inspector_changed_files = Some(changed_files.clone());
                                    set_right_inspector_changes_with_collapsed(
                                        app_weak,
                                        "changes",
                                        &right_inspector_project_id,
                                        Some(changed_files),
                                        &collapsed_inspector_sections,
                                    );
                                    set_toast(
                                        app_weak,
                                        "success",
                                        "Changed files updated",
                                        "The daemon returned the refreshed working-tree snapshot.",
                                    );
                                }
                                WorkerReply::DiscardChangedFileAck { changed_files } => {
                                    right_inspector_changed_files = Some(changed_files.clone());
                                    set_right_inspector_changes_with_collapsed(
                                        app_weak,
                                        "changes",
                                        &right_inspector_project_id,
                                        Some(changed_files),
                                        &collapsed_inspector_sections,
                                    );
                                    set_toast(
                                        app_weak,
                                        "success",
                                        "File changes discarded",
                                        "The daemon returned the refreshed working-tree snapshot.",
                                    );
                                }
                                WorkerReply::DiscardAllChangesAck {
                                    changed_files,
                                    failures,
                                } => {
                                    right_inspector_changed_files = Some(changed_files.clone());
                                    set_right_inspector_changes_with_collapsed(
                                        app_weak,
                                        "changes",
                                        &right_inspector_project_id,
                                        Some(changed_files),
                                        &collapsed_inspector_sections,
                                    );
                                    if failures.is_empty() {
                                        set_toast(
                                            app_weak,
                                            "success",
                                            "Changes discarded",
                                            "The daemon returned the refreshed working-tree snapshot.",
                                        );
                                    } else {
                                        set_toast(
                                            app_weak,
                                            "warning",
                                            "Some changes could not be discarded",
                                            failures.join("; "),
                                        );
                                    }
                                }
                                WorkerReply::RecentCommitsAck { view } => {
                                    match view {
                                        None => {
                                            right_inspector_recent_commits = None;
                                            set_right_inspector_state(
                                                app_weak,
                                                "commits",
                                                "unavailable",
                                                "Commits unavailable",
                                                "The daemon did not recognize the active project.",
                                                format!("Project: {right_inspector_project_id}"),
                                                Vec::new(),
                                            );
                                        }
                                        Some(view) => {
                                            right_inspector_recent_commits = Some(view.clone());
                                            set_right_inspector_commits(
                                                app_weak,
                                                &right_inspector_project_id,
                                                &view,
                                                &expanded_inspector_commits,
                                                &inspector_commit_file_change_states,
                                            );
                                        }
                                    }
                                }
                                WorkerReply::CommitFileChangesAck { files } => {
                                    if let Some(commit_key) =
                                        pending_inspector_commit_file_requests.remove(&request_id)
                                    {
                                        let state = match files {
                                            Some(files) => {
                                                InspectorCommitFileChangesState::Loaded(files)
                                            }
                                            None => InspectorCommitFileChangesState::Failed,
                                        };
                                        inspector_commit_file_change_states.insert(commit_key, state);
                                        if let Some(view) = right_inspector_recent_commits.as_ref()
                                        {
                                            set_right_inspector_commits(
                                                app_weak,
                                                &right_inspector_project_id,
                                                view,
                                                &expanded_inspector_commits,
                                                &inspector_commit_file_change_states,
                                            );
                                        }
                                    }
                                }
                                WorkerReply::PullRequestChecksAck { checks } => {
                                    match checks {
                                        None => set_right_inspector_state(
                                            app_weak,
                                            "checks",
                                            "clean",
                                            "No pull request",
                                            "No pull request exists for this branch.",
                                            format!("Project: {right_inspector_project_id}"),
                                            Vec::new(),
                                        ),
                                        Some(checks) if checks.is_empty() => {
                                            set_right_inspector_state(
                                                app_weak,
                                                "checks",
                                                "clean",
                                                "No checks",
                                                "No CI checks found for this pull request.",
                                                format!("Project: {right_inspector_project_id}"),
                                                Vec::new(),
                                            );
                                        }
                                        Some(checks) => {
                                            let rows = right_inspector_rows_for_checks(
                                                &right_inspector_project_id,
                                                &checks,
                                            );
                                            set_right_inspector_state(
                                                app_weak,
                                                "checks",
                                                "dirty",
                                                "Pull request checks",
                                                format!("{} checks returned by daemon.", checks.len()),
                                                format!("Project: {right_inspector_project_id}"),
                                                rows,
                                            );
                                        }
                                    }
                                }
                                WorkerReply::BranchSettingsAck { settings } => {
                                    let target_branch = settings
                                        .and_then(|settings| settings.effective_default_target_branch)
                                        .filter(|branch| !branch.trim().is_empty());
                                    set_right_inspector_compare_target(app_weak, target_branch.clone());
                                    if right_inspector_mode == "compare" {
                                        if let Some(target_branch) = target_branch {
                                            send_control(
                                                &mut send,
                                                &mut next_request_id,
                                                Control::ReadBranchCompareState {
                                                    project_id: right_inspector_project_id.clone(),
                                                    target_branch,
                                                },
                                            )
                                            .await
                                            .context("read branch compare state")?;
                                        } else {
                                            set_right_inspector_state(
                                                app_weak,
                                                "compare",
                                                "unavailable",
                                                "Compare unavailable",
                                                "No default target branch is configured for this project.",
                                                format!("Project: {right_inspector_project_id}"),
                                                Vec::new(),
                                            );
                                        }
                                    }
                                }
                                WorkerReply::BranchCompareAck { view } => match view {
                                    None => set_right_inspector_state(
                                        app_weak,
                                        "compare",
                                        "unavailable",
                                        "Compare unavailable",
                                        "The daemon did not recognize the active project for branch compare.",
                                        format!("Project: {right_inspector_project_id}"),
                                        Vec::new(),
                                    ),
                                    Some(view) if view.files.is_empty() => {
                                        let target_branch = view.target_branch.clone();
                                        set_right_inspector_state(
                                            app_weak,
                                            "compare",
                                            "clean",
                                            "No branch differences",
                                            format!("No differences from {target_branch}."),
                                            view.current_branch
                                                .map(|branch| format!("Branch: {branch}"))
                                                .unwrap_or_else(|| {
                                                    format!("Project: {right_inspector_project_id}")
                                                }),
                                            Vec::new(),
                                        );
                                    }
                                    Some(view) => {
                                        let rows = right_inspector_rows_for_compare(
                                            &right_inspector_project_id,
                                            &view,
                                        );
                                        set_right_inspector_state(
                                            app_weak,
                                            "compare",
                                            "dirty",
                                            "Branch compare",
                                            format!(
                                                "{} files differ from {}.",
                                                view.files.len(),
                                                view.target_branch
                                            ),
                                            "Read-only branch diff. Stage, unstage, and discard actions are unavailable in compare mode.",
                                            rows,
                                        );
                                    }
                                },
                                WorkerReply::SubmitNewTaskAck { section_id } => {
                                    let target = TerminalTarget {
                                        section_id,
                                        tab_id: "0".to_string(),
                                    };
                                    attach_terminal_target(
                                        &mut send,
                                        &mut next_request_id,
                                        &target,
                                        terminal.size,
                                        true,
                                    )
                                    .await?;
                                    attached_target = target;
                                    terminal =
                                        AlacrittySnapshot::new(terminal.size.cols, terminal.size.rows);
                                    set_terminal_surface(app_weak, terminal.snapshot_surface());
                                    selection_drag_anchor = None;
                                    selection_range = None;
                                    set_terminal_selection(app_weak, Vec::new());
                                    dirty = false;
                                    pending_flush_at = None;
                                    send_control(&mut send, &mut next_request_id, Control::ListProjects).await?;
                                    set_toast(app_weak, "success", "Task created", "Attached to the new task terminal.");
                                }
                                WorkerReply::AddAgentToSectionAck { tab_id } => {
                                    let target = TerminalTarget {
                                        section_id: attached_target.section_id.clone(),
                                        tab_id,
                                    };
                                    attach_terminal_target(
                                        &mut send,
                                        &mut next_request_id,
                                        &target,
                                        terminal.size,
                                        true,
                                    )
                                    .await?;
                                    attached_target = target;
                                    terminal = AlacrittySnapshot::new(
                                        terminal.size.cols,
                                        terminal.size.rows,
                                    );
                                    set_terminal_surface(app_weak, terminal.snapshot_surface());
                                    selection_drag_anchor = None;
                                    selection_range = None;
                                    set_terminal_selection(app_weak, Vec::new());
                                    dirty = false;
                                    pending_flush_at = None;
                                    send_control(
                                        &mut send,
                                        &mut next_request_id,
                                        Control::ListProjects,
                                    )
                                    .await?;
                                }
                                WorkerReply::CloseSectionTabAck { active_tab_id } => {
                                    if active_tab_id.is_empty() {
                                        terminal = AlacrittySnapshot::new(
                                            terminal.size.cols,
                                            terminal.size.rows,
                                        );
                                        set_terminal_surface(app_weak, terminal.snapshot_surface());
                                        selection_drag_anchor = None;
                                        selection_range = None;
                                        set_terminal_selection(app_weak, Vec::new());
                                        dirty = false;
                                        pending_flush_at = None;
                                        set_terminal_status(
                                            app_weak,
                                            "terminal: section has no active tabs",
                                        );
                                    } else {
                                        let target = TerminalTarget {
                                            section_id: attached_target.section_id.clone(),
                                            tab_id: active_tab_id,
                                        };
                                        attach_terminal_target(
                                            &mut send,
                                            &mut next_request_id,
                                            &target,
                                            terminal.size,
                                            true,
                                        )
                                        .await?;
                                        attached_target = target;
                                        terminal = AlacrittySnapshot::new(
                                            terminal.size.cols,
                                            terminal.size.rows,
                                        );
                                        set_terminal_surface(app_weak, terminal.snapshot_surface());
                                        selection_drag_anchor = None;
                                        selection_range = None;
                                        set_terminal_selection(app_weak, Vec::new());
                                        dirty = false;
                                        pending_flush_at = None;
                                    }
                                    send_control(
                                        &mut send,
                                        &mut next_request_id,
                                        Control::ListProjects,
                                    )
                                    .await?;
                                }
                                WorkerReply::ToggleSectionTabPinnedAck { .. } => {
                                    send_control(
                                        &mut send,
                                        &mut next_request_id,
                                        Control::ListProjects,
                                    )
                                    .await?;
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
            _ = wait_for_terminal_resize(pending_resize_at), if pending_resize_at.is_some() => {
                let Some(next_size) = pending_resize.take() else {
                    pending_resize_at = None;
                    continue;
                };
                pending_resize_at = None;
                if terminal.size != next_size {
                    terminal.resize(next_size);
                    send_terminal_resize(&mut send, &mut next_request_id, next_size)
                        .await
                        .context("send terminal resize")?;
                    set_terminal_surface(app_weak, terminal.snapshot_surface());
                    selection_drag_anchor = None;
                    selection_range = None;
                    set_terminal_selection(app_weak, Vec::new());
                    dirty = false;
                    pending_flush_at = None;
                    set_terminal_status(
                        app_weak,
                        format!(
                            "terminal: attached {}/{} at {}x{}",
                            attached_target.section_id,
                            attached_target.tab_id,
                            next_size.cols,
                            next_size.rows
                        ),
                    );
                }
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
struct SlintPointerEvent {
    kind: String,
    button: String,
    column: i32,
    line: i32,
    control: bool,
    alt: bool,
    shift: bool,
}

impl SlintPointerEvent {
    fn is_primary_down(&self) -> bool {
        self.kind == "down" && self.button == "left"
    }

    fn is_primary_move(&self) -> bool {
        self.kind == "move"
    }

    fn is_primary_up(&self) -> bool {
        self.kind == "up" && self.button == "left"
    }

    fn terminal_point(&self) -> Option<TerminalCellPoint> {
        Some(TerminalCellPoint {
            line: self.line,
            column: self.column,
        })
        .filter(|point| point.line >= 0 && point.column >= 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TerminalCellPoint {
    line: i32,
    column: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SlintClientEvent {
    TerminalKey(SlintKeyEvent),
    TerminalFocus(bool),
    TerminalPointer(SlintPointerEvent),
    TerminalResize {
        cols: i32,
        rows: i32,
    },
    SelectProject(String),
    SelectTask(String),
    SelectTab(String),
    AddTerminalTab,
    CloseTerminalTab(String),
    ToggleTerminalTabPinned(String),
    RightInspectorMode(String),
    StageChangedFile {
        path: String,
        original_path: Option<String>,
    },
    UnstageChangedFile {
        path: String,
        original_path: Option<String>,
    },
    DiscardChangedFile {
        path: String,
        original_path: Option<String>,
        untracked: bool,
    },
    ToggleInspectorCommit {
        project_id: String,
        commit_id: String,
    },
    OpenInspectorCheckLink(String),
    ToggleInspectorSection(String),
    StageAllChanges,
    UnstageAllChanges,
    SubmitNewTask {
        task_name: String,
        source_branch: String,
        project_id: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TerminalInputModeState {
    app_cursor: bool,
    bracketed_paste: bool,
    focus_in_out: bool,
    mouse_report_click: bool,
    mouse_drag: bool,
    mouse_motion: bool,
    sgr_mouse: bool,
}

async fn wait_for_terminal_flush(deadline: Option<Instant>) {
    let Some(deadline) = deadline else {
        std::future::pending::<()>().await;
        return;
    };

    tokio::time::sleep_until(deadline).await;
}

async fn wait_for_terminal_resize(deadline: Option<Instant>) {
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

fn clamp_terminal_dimension(value: i32, min: u16, max: u16) -> u16 {
    value.clamp(i32::from(min), i32::from(max)) as u16
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

    attach_terminal_target(send, next_request_id, &next_target, terminal.size, true).await?;
    *current_target = next_target;
    *terminal = AlacrittySnapshot::new(terminal.size.cols, terminal.size.rows);
    set_terminal_surface(app_weak, terminal.snapshot_surface());
    *dirty = false;
    *pending_flush_at = None;
    set_terminal_status(
        app_weak,
        format!(
            "terminal: attached {}/{} at {}x{}",
            current_target.section_id,
            current_target.tab_id,
            terminal.size.cols,
            terminal.size.rows
        ),
    );

    Ok(())
}

async fn attach_terminal_target<W>(
    send: &mut W,
    next_request_id: &mut u64,
    target: &TerminalTarget,
    size: TerminalSize,
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
    send_terminal_resize(send, next_request_id, size).await
}

async fn send_terminal_resize<W>(
    send: &mut W,
    next_request_id: &mut u64,
    size: TerminalSize,
) -> anyhow::Result<()>
where
    W: frame::WriteAllAsync + Unpin,
{
    send_control(
        send,
        next_request_id,
        Control::TabResize {
            cols: size.cols,
            rows: size.rows,
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

fn set_project_overview_placeholder(
    app_weak: &slint::Weak<AppWindow>,
    project: &frame::ProjectSummary,
) {
    let app_weak = app_weak.clone();
    let project_name = project.name.clone();
    let branch_name = project
        .current_branch
        .as_deref()
        .unwrap_or_else(|| project_kind_label(project.kind))
        .to_string();
    let worktree_name = worktree_name(&project.path);
    let project_path = project.path.clone();
    let status = format!("project overview: {project_name} (Slint project page parity pending)");
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_active_project_name(project_name.into());
            app.set_active_task_name("Project overview".into());
            app.set_active_branch_name(branch_name.into());
            app.set_active_worktree_name(worktree_name.into());
            app.set_active_project_path(project_path.into());
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

fn set_terminal_selection(app_weak: &slint::Weak<AppWindow>, spans: Vec<TerminalSelectionSpan>) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_terminal_selection_spans(slint::ModelRc::new(slint::VecModel::from(spans)));
        }
    });
}

fn is_copy_shortcut(input: &SlintKeyEvent) -> bool {
    input.control && !input.alt && input.text.eq_ignore_ascii_case("c")
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

fn encode_terminal_pointer_event(
    input: &SlintPointerEvent,
    modes: TerminalInputModeState,
) -> Option<TerminalInputEvent> {
    if !mouse_reporting_enabled(modes) {
        return None;
    }

    let column = u16::try_from(input.column).ok()?;
    let line = u16::try_from(input.line).ok()?;
    let modifiers = mouse_modifier_bits(input);
    let release = input.kind == "up";
    let code = match input.kind.as_str() {
        "down" | "up" => mouse_button_code(&input.button)?.checked_add(modifiers)?,
        "move" => {
            if !modes.mouse_motion && !modes.mouse_drag {
                return None;
            }
            let button = if input.button == "other" {
                if modes.mouse_motion {
                    3
                } else {
                    return None;
                }
            } else {
                mouse_button_code(&input.button)?
            };
            32u8.checked_add(button)?.checked_add(modifiers)?
        }
        _ => return None,
    };

    let bytes = if modes.sgr_mouse {
        encode_sgr_mouse_button(code, column, line, release)
    } else {
        let legacy_code = if release {
            3u8.checked_add(modifiers)?
        } else {
            code
        };
        encode_legacy_mouse_button(legacy_code, column, line)?
    };

    Some(TerminalInputEvent::Mouse { bytes })
}

fn mouse_reporting_enabled(modes: TerminalInputModeState) -> bool {
    modes.mouse_report_click || modes.mouse_drag || modes.mouse_motion
}

fn mouse_button_code(button: &str) -> Option<u8> {
    match button {
        "left" => Some(0),
        "middle" => Some(1),
        "right" => Some(2),
        _ => None,
    }
}

fn mouse_modifier_bits(input: &SlintPointerEvent) -> u8 {
    let mut bits = 0;
    if input.shift {
        bits |= 4;
    }
    if input.alt {
        bits |= 8;
    }
    if input.control {
        bits |= 16;
    }
    bits
}

fn encode_sgr_mouse_button(code: u8, column: u16, line: u16, release: bool) -> Vec<u8> {
    let suffix = if release { 'm' } else { 'M' };
    format!(
        "\x1b[<{code};{};{}{suffix}",
        u32::from(column) + 1,
        u32::from(line) + 1
    )
    .into_bytes()
}

fn encode_legacy_mouse_button(code: u8, column: u16, line: u16) -> Option<Vec<u8>> {
    Some(vec![
        0x1b,
        b'[',
        b'M',
        code.checked_add(32)?,
        legacy_mouse_coord(column)?,
        legacy_mouse_coord(line)?,
    ])
}

fn legacy_mouse_coord(coord: u16) -> Option<u8> {
    u8::try_from(u32::from(coord) + 33).ok()
}

fn link_uri_at(spans: &[TerminalLinkSpan], line: i32, column: i32) -> Option<String> {
    spans
        .iter()
        .find(|span| {
            span.line == line && column >= span.column && column < span.column + span.cell_count
        })
        .map(|span| span.uri.to_string())
}

fn selection_spans_for_points(
    anchor: TerminalCellPoint,
    focus: TerminalCellPoint,
    columns: u16,
    rows: u16,
) -> Vec<TerminalSelectionSpan> {
    let Some((start, end)) = normalized_selection_points(anchor, focus) else {
        return Vec::new();
    };
    let columns = i32::from(columns);
    let rows = i32::from(rows);
    if columns <= 0 || rows <= 0 {
        return Vec::new();
    }

    let first_line = start.line.clamp(0, rows.saturating_sub(1));
    let last_line = end.line.clamp(0, rows.saturating_sub(1));
    let mut spans = Vec::new();

    for line in first_line..=last_line {
        let column_start = if line == start.line { start.column } else { 0 }.clamp(0, columns);
        let column_end = if line == end.line {
            end.column.saturating_add(1)
        } else {
            columns
        }
        .clamp(0, columns);

        if column_end > column_start {
            spans.push(TerminalSelectionSpan {
                line,
                column: column_start,
                cell_count: column_end - column_start,
            });
        }
    }

    spans
}

fn normalized_selection_points(
    anchor: TerminalCellPoint,
    focus: TerminalCellPoint,
) -> Option<(TerminalCellPoint, TerminalCellPoint)> {
    (anchor != focus).then(|| {
        if anchor <= focus {
            (anchor, focus)
        } else {
            (focus, anchor)
        }
    })
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    fn resize(&mut self, size: TerminalSize) {
        if self.size == size {
            return;
        }

        self.size = size;
        self.term.resize(size);
    }

    fn input_modes(&self) -> TerminalInputModeState {
        let mode = self.term.mode();
        TerminalInputModeState {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            focus_in_out: mode.contains(TermMode::FOCUS_IN_OUT),
            mouse_report_click: mode.contains(TermMode::MOUSE_REPORT_CLICK),
            mouse_drag: mode.contains(TermMode::MOUSE_DRAG),
            mouse_motion: mode.contains(TermMode::MOUSE_MOTION),
            sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
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

    fn selected_text(&self, anchor: TerminalCellPoint, focus: TerminalCellPoint) -> String {
        let spans = selection_spans_for_points(anchor, focus, self.size.cols, self.size.rows);
        if spans.is_empty() {
            return String::new();
        }

        let renderable = self.term.renderable_content();
        let display_offset = renderable.display_offset;
        let mut lines = Vec::new();

        for span in spans {
            let Some(line) = usize::try_from(span.line).ok() else {
                continue;
            };
            let point = viewport_to_point(display_offset, Point::new(line, Column(0)));
            let grid_line = &self.term.grid()[point.line];
            let span_start = span.column;
            let span_end = span.column + span.cell_count;
            let mut text = String::new();
            let mut visual_column = 0;

            for column in 0..self.size.cols as usize {
                let cell = &grid_line[Column(column)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }

                let cell_count = terminal_cell_width(cell);
                let cell_start = to_i32(visual_column);
                let cell_end = to_i32(visual_column + cell_count);
                if span_start < cell_end && span_end > cell_start {
                    text.push_str(&selected_cell_text(cell));
                }
                visual_column += cell_count;
            }

            lines.push(text.trim_end().to_string());
        }

        lines.join("\n")
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

fn selected_cell_text(cell: &alacritty_terminal::term::cell::Cell) -> String {
    if cell.flags.contains(Flags::HIDDEN) {
        return String::new();
    }

    let mut text = String::new();
    text.push(cell.c);
    for zero_width in cell.zerowidth().into_iter().flatten() {
        text.push(*zero_width);
    }

    text
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
    fn slint_ui_uses_gpui_typography_contract() {
        for source in [
            include_str!("../ui/app.slint"),
            include_str!("../ui/components.slint"),
        ] {
            assert!(source.contains("Lilex Nerd Font Mono"));
            assert!(!source.contains("font-family: \"monospace\""));
            assert!(!source.contains("font-weight: 800"));
            assert!(!source.contains("font-weight: 900"));
            assert!(!source.contains("font-size: 19px"));
        }
    }

    #[test]
    fn slint_sidebar_uses_gpui_project_tree_contract() {
        let app_source = include_str!("../ui/app.slint");

        assert!(app_source.contains("sidebar_rows"));
        assert!(app_source.contains("AoSidebarProjectTreeRow"));
        assert!(app_source.contains("AoSidebarTaskTreeRow"));
        assert!(!app_source.contains("OPEN TASKS"));
    }

    #[test]
    fn slint_terminal_workspace_uses_gpui_asset_and_color_contract() {
        let app_source = include_str!("../ui/app.slint");
        let components_source = include_str!("../ui/components.slint");

        for asset in [
            "icons__terminal.svg",
            "icons__pin-off.svg",
            "icons__close.svg",
            "icons__plus.svg",
            "icons__copy.svg",
            "icons__alert-triangle.svg",
            "agent-icons/claude.png",
            "agent-icons/openai.svg",
            "agent-icons/cursor.svg",
            "agent-icons/gemini.png",
        ] {
            assert!(
                app_source.contains(asset) || components_source.contains(asset),
                "missing GPUI asset reference: {asset}"
            );
        }
        assert_eq!(DEFAULT_TERMINAL_BACKGROUND_RGB, 0x1e1f22);
        assert!(app_source.contains("background: #1e1f22"));
        assert!(components_source.contains("#1e1f22"));
        assert!(components_source.contains("#2b2d31"));
        assert!(components_source.contains("#2f3136"));
        assert!(!app_source.contains("icon: \"C\""));
        assert!(!components_source.contains("icon: \"C\""));
    }

    #[test]
    fn slint_right_inspector_uses_gpui_asset_and_color_contract() {
        let app_source = include_str!("../ui/app.slint");
        let components_source = include_str!("../ui/components.slint");

        for asset in [
            "icons__file_icons__changes.svg",
            "icons__git-commit.svg",
            "icons__tool-check.svg",
            "icons__git-split.svg",
            "icons__plus.svg",
            "icons__minus.svg",
            "icons__discard.svg",
            "icons__chevron-down.svg",
            "icons__chevron-right.svg",
            "icons__badge-check.svg",
            "icons__badge-x.svg",
            "icons__badge-clock.svg",
            "icons__external-link.svg",
        ] {
            assert!(
                app_source.contains(asset) || components_source.contains(asset),
                "missing right-inspector GPUI asset reference: {asset}"
            );
        }
        assert!(app_source.contains("#262a30"));
        assert!(app_source.contains("right_inspector_compare_available"));
        assert!(app_source.contains("inspector_commit_toggled"));
        assert!(app_source.contains("inspector_check_open_requested"));
        assert!(app_source.contains("inspector_section_toggled"));
        assert!(app_source.contains("inspector_discard_confirm_open"));
        assert!(app_source.contains("Confirm Discard"));
        assert!(app_source.contains("This action cannot be undone."));
        assert!(!app_source.contains("Discard confirmation pending"));
        assert!(components_source.contains("#ffffff14"));
        assert!(components_source.contains("#8bd99c"));
        assert!(components_source.contains("#e58b95"));
    }

    #[test]
    fn workspace_shell_model_nests_tasks_under_each_project() {
        let projects = vec![
            sidebar_project(
                "project-a",
                "Project A",
                vec![
                    sidebar_task(
                        "task-low",
                        "Later task",
                        "feature/later",
                        false,
                        "section-low",
                    ),
                    sidebar_task(
                        "task-pin",
                        "Pinned task",
                        "feature/pinned",
                        true,
                        "section-pin",
                    ),
                ],
            ),
            sidebar_project(
                "project-b",
                "Project B",
                vec![sidebar_task(
                    "task-b",
                    "Other task",
                    "feature/other",
                    false,
                    "section-b",
                )],
            ),
        ];

        let model = workspace_shell_model(&projects, "section-pin", "0");
        let rows = model
            .sidebar_rows
            .iter()
            .map(|row| {
                (
                    row.kind.as_str().to_string(),
                    row.group_id.as_str().to_string(),
                    row.project_id.as_str().to_string(),
                    row.task_id.as_str().to_string(),
                    row.active,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            rows,
            vec![
                (
                    "project".to_string(),
                    "project-a".to_string(),
                    "project-a".to_string(),
                    String::new(),
                    false,
                ),
                (
                    "task".to_string(),
                    "project-a".to_string(),
                    "project-a".to_string(),
                    "task-pin".to_string(),
                    true,
                ),
                (
                    "task".to_string(),
                    "project-a".to_string(),
                    "project-a".to_string(),
                    "task-low".to_string(),
                    false,
                ),
                (
                    "project".to_string(),
                    "project-b".to_string(),
                    "project-b".to_string(),
                    String::new(),
                    false,
                ),
                (
                    "task".to_string(),
                    "project-b".to_string(),
                    "project-b".to_string(),
                    "task-b".to_string(),
                    false,
                ),
            ]
        );
    }

    #[test]
    fn workspace_shell_model_exposes_failed_terminal_panel() {
        let mut task = sidebar_task(
            "task-fail",
            "Failed task",
            "feature/fail",
            false,
            "section-fail",
        );
        task.tabs[0] = serde_json::from_value(serde_json::json!({
            "id": "0",
            "title": "Shell",
            "provider": "shell",
            "running": false,
            "pinned": false,
            "fixed_title": null,
            "restore_status": "failed",
            "failure_message": "PTY spawn failed",
            "failure_details": "permission denied"
        }))
        .expect("failed tab summary fixture should deserialize");
        let projects = vec![sidebar_project("project-a", "Project A", vec![task])];

        let model = workspace_shell_model(&projects, "section-fail", "0");

        assert_eq!(model.terminal_panel_state, "failed");
        assert_eq!(model.terminal_panel_title, "Terminal launch failed");
        assert_eq!(model.terminal_panel_body, "PTY spawn failed");
        assert_eq!(model.terminal_error_details, "permission denied");
        assert_eq!(model.tab_chips[0].restore_status.as_str(), "failed");
    }

    #[test]
    fn right_inspector_rows_partition_staged_and_unstaged_changes() {
        let files = vec![
            changed_file_wire("src/lib.rs", "M", "M", 2, 1, 3, 0, false),
            changed_file_wire("docs/new.md", "?", "?", 0, 0, 5, 0, true),
        ];

        let (rows, summary) = right_inspector_rows_for_changed_files_with_collapsed(
            "project-a",
            &files,
            &HashSet::new(),
        );

        assert_eq!(summary, "1 staged, 2 unstaged");
        assert_eq!(rows[0].kind.as_str(), "section");
        assert_eq!(rows[0].title.as_str(), "Staged Changes");
        assert_eq!(rows[0].file_count_label.as_str(), "1");
        assert_eq!(rows[1].kind.as_str(), "file");
        assert_eq!(rows[1].group.as_str(), "staged");
        assert_eq!(rows[1].status.as_str(), "M");
        assert_eq!(rows[2].title.as_str(), "Changes");
        assert_eq!(rows[2].file_count_label.as_str(), "2");
        assert_eq!(rows[3].group.as_str(), "unstaged");
        assert_eq!(rows[3].additions_label.as_str(), "+3");
        assert_eq!(rows[4].title.as_str(), "new.md");
        assert_eq!(rows[4].parent_dir.as_str(), "docs");
        assert_eq!(rows[4].status.as_str(), "A");
    }

    #[test]
    fn right_inspector_rows_omit_collapsed_change_section_children() {
        let files = vec![changed_file_wire("src/lib.rs", "M", "M", 2, 1, 3, 0, false)];
        let collapsed = HashSet::from(["unstaged".to_string()]);

        let (rows, summary) =
            right_inspector_rows_for_changed_files_with_collapsed("project-a", &files, &collapsed);

        assert_eq!(summary, "1 staged, 1 unstaged");
        assert!(rows
            .iter()
            .any(|row| row.group.as_str() == "staged" && row.expanded));
        assert!(rows
            .iter()
            .any(|row| row.group.as_str() == "unstaged" && !row.expanded));
        assert_eq!(
            rows.iter()
                .filter(|row| row.kind.as_str() == "file" && row.group.as_str() == "unstaged")
                .count(),
            0
        );
    }

    #[test]
    fn right_inspector_rows_render_recent_commits() {
        let view = frame::RecentCommitsWire {
            current_branch: Some("feature/right-inspector".to_string()),
            has_more: false,
            commits: vec![frame::CommitWire {
                id: "abcdef012345".to_string(),
                short_id: "abcdef0".to_string(),
                subject: "feat: wire inspector".to_string(),
                author_name: "Mason".to_string(),
                authored_relative: "2 minutes ago".to_string(),
            }],
        };

        let rows = right_inspector_rows_for_commits_with_expansions(
            "project-a",
            &view,
            &HashSet::new(),
            &HashMap::new(),
        );

        assert_eq!(rows[0].kind.as_str(), "section");
        assert_eq!(
            rows[0].title.as_str(),
            "Recent commits on feature/right-inspector"
        );
        assert_eq!(rows[1].kind.as_str(), "commit");
        assert_eq!(rows[1].title.as_str(), "feat: wire inspector");
        assert_eq!(rows[1].parent_dir.as_str(), "Mason - 2 minutes ago");
        assert_eq!(rows[1].status.as_str(), "abcdef0");
    }

    #[test]
    fn right_inspector_rows_render_expanded_commit_files() {
        let view = frame::RecentCommitsWire {
            current_branch: Some("feature/right-inspector".to_string()),
            has_more: false,
            commits: vec![frame::CommitWire {
                id: "abcdef012345".to_string(),
                short_id: "abcdef0".to_string(),
                subject: "feat: wire inspector".to_string(),
                author_name: "Mason".to_string(),
                authored_relative: "2 minutes ago".to_string(),
            }],
        };
        let commit_key = right_inspector_commit_key("project-a", "abcdef012345");
        let expanded = HashSet::from([commit_key.clone()]);
        let states = HashMap::from([(
            commit_key,
            InspectorCommitFileChangesState::Loaded(vec![frame::BranchCompareFileWire {
                path: "slint-poc/src/lib.rs".to_string(),
                original_path: None,
                status: "M".to_string(),
                additions: 4,
                deletions: 2,
            }]),
        )]);

        let rows = right_inspector_rows_for_commits_with_expansions(
            "project-a",
            &view,
            &expanded,
            &states,
        );

        assert!(rows[1].expanded);
        assert_eq!(rows[2].title.as_str(), "1 file changed");
        assert_eq!(rows[3].group.as_str(), "commit-file");
        assert_eq!(rows[3].additions_label.as_str(), "+4");
    }

    #[test]
    fn right_inspector_rows_sort_checks_by_gpui_priority() {
        let checks = vec![
            check_wire("Unit", frame::CheckBucket::Pass),
            check_wire("Build", frame::CheckBucket::Fail),
            check_wire("Lint", frame::CheckBucket::Pending),
        ];

        let rows = right_inspector_rows_for_checks("project-a", &checks);

        assert_eq!(rows[0].title.as_str(), "Pull request checks");
        assert_eq!(rows[1].title.as_str(), "Build");
        assert_eq!(rows[1].group.as_str(), "fail");
        assert_eq!(rows[2].title.as_str(), "Lint");
        assert_eq!(rows[2].group.as_str(), "pending");
        assert_eq!(rows[3].title.as_str(), "Unit");
        assert_eq!(rows[3].group.as_str(), "pass");
    }

    #[test]
    fn right_inspector_check_rows_preserve_open_link() {
        let mut check = check_wire("Build", frame::CheckBucket::Fail);
        check.link = Some("https://github.test/checks/1".to_string());

        let rows = right_inspector_rows_for_checks("project-a", &[check]);

        assert_eq!(rows[1].path.as_str(), "https://github.test/checks/1");
    }

    #[test]
    fn right_inspector_rows_render_read_only_branch_compare() {
        let view = frame::BranchCompareWire {
            current_branch: Some("feature/slint".to_string()),
            target_branch: "main".to_string(),
            files: vec![
                frame::BranchCompareFileWire {
                    path: "slint-poc/src/lib.rs".to_string(),
                    original_path: None,
                    status: "M".to_string(),
                    additions: 12,
                    deletions: 3,
                },
                frame::BranchCompareFileWire {
                    path: "desktop/src/old.rs".to_string(),
                    original_path: Some("desktop/src/new.rs".to_string()),
                    status: "R".to_string(),
                    additions: 1,
                    deletions: 1,
                },
            ],
        };

        let rows = right_inspector_rows_for_compare("project-a", &view);

        assert_eq!(
            rows[0].title.as_str(),
            "Comparing feature/slint against main"
        );
        assert_eq!(rows[0].additions_label.as_str(), "+13");
        assert_eq!(rows[0].deletions_label.as_str(), "-4");
        assert_eq!(rows[1].group.as_str(), "compare");
        assert!(!rows[1].can_stage);
        assert_eq!(rows[2].status.as_str(), "R");
        assert_eq!(
            rows[2].parent_dir.as_str(),
            "Renamed from desktop/src/new.rs"
        );
    }

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
    fn snapshot_surface_preserves_indexed_and_truecolor_foreground_colors() {
        let mut terminal = AlacrittySnapshot::new(60, 4);
        let _ =
            terminal.apply_output(b"\x1b[38;5;208mINDEXED\x1b[0m \x1b[38;2;125;90;255mRGB\x1b[0m");

        let surface = terminal.snapshot_surface();

        assert_run_color(&surface, "INDEXED", 0xffff8700);
        assert_run_color(&surface, "RGB", 0xff7d5aff);
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
    fn clamp_terminal_dimension_respects_bounds() {
        assert_eq!(clamp_terminal_dimension(5, 20, 240), 20);
        assert_eq!(clamp_terminal_dimension(120, 20, 240), 120);
        assert_eq!(clamp_terminal_dimension(400, 20, 240), 240);
    }

    #[test]
    fn terminal_resize_updates_snapshot_dimensions() {
        let mut terminal = AlacrittySnapshot::new(20, 4);

        terminal.resize(TerminalSize { cols: 40, rows: 8 });

        assert_eq!(terminal.size, TerminalSize { cols: 40, rows: 8 });
        assert_eq!(terminal.term.columns(), 40);
        assert_eq!(terminal.term.screen_lines(), 8);
    }

    #[test]
    fn terminal_render_probe_reaches_target_bytes() {
        let report = run_terminal_render_probe(16 * 1024);

        assert_eq!(report.applied_bytes, report.target_bytes);
        assert!(report.snapshots > 0);
        assert!(report.max_text_runs > 0);
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
    fn input_modes_track_focus_reporting_mode() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        assert!(!terminal.input_modes().focus_in_out);

        let _ = terminal.apply_output(b"\x1b[?1004h");

        assert!(terminal.input_modes().focus_in_out);
    }

    #[test]
    fn input_modes_track_sgr_mouse_reporting_mode() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        assert!(!terminal.input_modes().mouse_report_click);
        assert!(!terminal.input_modes().sgr_mouse);

        let _ = terminal.apply_output(b"\x1b[?1000h\x1b[?1006h");

        let modes = terminal.input_modes();
        assert!(modes.mouse_report_click);
        assert!(modes.sgr_mouse);
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
                ..TerminalInputModeState::default()
            },
        );

        assert_eq!(bytes, b"\x1bOA");
    }

    #[test]
    fn encode_terminal_key_brackets_multi_character_paste_when_enabled() {
        let bytes = encode_key_bytes(
            "alpha\nbeta",
            TerminalInputModeState {
                bracketed_paste: true,
                ..TerminalInputModeState::default()
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
    fn encode_terminal_pointer_event_uses_sgr_mouse_press() {
        let event = pointer_event("down", "left", 11, 33);
        let modes = TerminalInputModeState {
            mouse_report_click: true,
            sgr_mouse: true,
            ..TerminalInputModeState::default()
        };

        let bytes = encode_terminal_pointer_event(&event, modes)
            .expect("pointer should encode")
            .pty_bytes();

        assert_eq!(bytes, b"\x1b[<0;12;34M");
    }

    #[test]
    fn encode_terminal_pointer_event_uses_sgr_mouse_release() {
        let event = pointer_event("up", "left", 11, 33);
        let modes = TerminalInputModeState {
            mouse_report_click: true,
            sgr_mouse: true,
            ..TerminalInputModeState::default()
        };

        let bytes = encode_terminal_pointer_event(&event, modes)
            .expect("pointer should encode")
            .pty_bytes();

        assert_eq!(bytes, b"\x1b[<0;12;34m");
    }

    #[test]
    fn encode_terminal_pointer_event_uses_legacy_mouse_when_sgr_is_disabled() {
        let event = pointer_event("down", "left", 0, 0);
        let modes = TerminalInputModeState {
            mouse_report_click: true,
            ..TerminalInputModeState::default()
        };

        let bytes = encode_terminal_pointer_event(&event, modes)
            .expect("pointer should encode")
            .pty_bytes();

        assert_eq!(bytes, b"\x1b[M !!");
    }

    #[test]
    fn encode_terminal_pointer_event_reports_motion_only_when_enabled() {
        let event = pointer_event("move", "other", 2, 4);
        let modes = TerminalInputModeState {
            mouse_motion: true,
            sgr_mouse: true,
            ..TerminalInputModeState::default()
        };

        let bytes = encode_terminal_pointer_event(&event, modes)
            .expect("motion should encode")
            .pty_bytes();

        assert_eq!(bytes, b"\x1b[<35;3;5M");
    }

    #[test]
    fn encode_terminal_pointer_event_ignores_mouse_when_reporting_disabled() {
        let event = pointer_event("down", "left", 11, 33);

        let encoded = encode_terminal_pointer_event(&event, TerminalInputModeState::default());

        assert!(encoded.is_none());
    }

    #[test]
    fn link_uri_at_returns_uri_for_cell_range() {
        let spans = vec![TerminalLinkSpan {
            line: 2,
            column: 8,
            cell_count: 4,
            uri: "https://example.test".into(),
        }];

        let uri = link_uri_at(&spans, 2, 10);

        assert_eq!(uri.as_deref(), Some("https://example.test"));
    }

    #[test]
    fn selection_spans_cover_multi_line_ranges() {
        let spans = selection_spans_for_points(
            TerminalCellPoint { line: 1, column: 6 },
            TerminalCellPoint { line: 3, column: 2 },
            10,
            5,
        );

        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].line, 1);
        assert_eq!(spans[0].column, 6);
        assert_eq!(spans[0].cell_count, 4);
        assert_eq!(spans[1].line, 2);
        assert_eq!(spans[1].column, 0);
        assert_eq!(spans[1].cell_count, 10);
        assert_eq!(spans[2].line, 3);
        assert_eq!(spans[2].column, 0);
        assert_eq!(spans[2].cell_count, 3);
    }

    #[test]
    fn selected_text_preserves_wide_and_combining_terminal_text() {
        let mut terminal = AlacrittySnapshot::new(20, 4);
        let _ = terminal.apply_output("A界e\u{301}Z".as_bytes());

        let selected = terminal.selected_text(
            TerminalCellPoint { line: 0, column: 1 },
            TerminalCellPoint { line: 0, column: 3 },
        );

        assert_eq!(selected, "界e\u{301}");
    }

    #[test]
    fn is_copy_shortcut_requires_control_c_without_alt() {
        let event = SlintKeyEvent {
            text: "c".to_string(),
            control: true,
            alt: false,
        };

        assert!(is_copy_shortcut(&event));
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

    fn pointer_event(kind: &str, button: &str, column: i32, line: i32) -> SlintPointerEvent {
        SlintPointerEvent {
            kind: kind.to_string(),
            button: button.to_string(),
            column,
            line,
            control: false,
            alt: false,
            shift: false,
        }
    }

    fn sidebar_project(
        id: &str,
        name: &str,
        tasks: Vec<frame::TaskSummary>,
    ) -> frame::ProjectSummary {
        frame::ProjectSummary {
            id: id.to_string(),
            name: name.to_string(),
            path: format!("/repo/{id}"),
            kind: frame::ProjectKind::Root,
            current_branch: Some("main".to_string()),
            tasks,
        }
    }

    fn sidebar_task(
        id: &str,
        name: &str,
        branch_name: &str,
        pinned: bool,
        section_id: &str,
    ) -> frame::TaskSummary {
        frame::TaskSummary {
            id: id.to_string(),
            name: name.to_string(),
            section_id: section_id.to_string(),
            branch_name: branch_name.to_string(),
            active_tab_id: "0".to_string(),
            tabs: vec![frame::TabSummary {
                id: "0".to_string(),
                title: "Shell".to_string(),
                provider: Some(frame::AgentProvider::Shell),
                running: true,
                pinned: false,
                fixed_title: None,
                restore_status: Default::default(),
                failure_message: None,
                failure_details: None,
            }],
            pinned,
            last_commit_relative: String::new(),
            lines_added: 0,
            lines_removed: 0,
            target_project_id: String::new(),
        }
    }

    fn changed_file_wire(
        path: &str,
        index_status: &str,
        worktree_status: &str,
        staged_additions: i32,
        staged_deletions: i32,
        unstaged_additions: i32,
        unstaged_deletions: i32,
        untracked: bool,
    ) -> frame::ChangedFileWire {
        frame::ChangedFileWire {
            path: path.to_string(),
            original_path: None,
            staged_additions,
            staged_deletions,
            unstaged_additions,
            unstaged_deletions,
            index_status: index_status.to_string(),
            worktree_status: worktree_status.to_string(),
            untracked,
        }
    }

    fn check_wire(name: &str, bucket: frame::CheckBucket) -> frame::Check {
        frame::Check {
            name: name.to_string(),
            state: format!("{bucket:?}").to_lowercase(),
            bucket,
            description: Some(format!("{name} description")),
            link: None,
            duration_text: Some("1m".to_string()),
        }
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
