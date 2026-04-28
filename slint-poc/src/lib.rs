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
use alacritty_terminal::term::{point_to_viewport, viewport_to_point, Config, Term};
use alacritty_terminal::vte::ansi::{self, Color, CursorShape, NamedColor, Rgb};
use anyhow::Context;
use daemon_sandbox::frame::{
    self, Control, ControlEnvelope, TerminalInputEvent, WorkerReply, WorkerReplyEnvelope,
};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use tokio::sync::mpsc;

slint::include_modules!();

const TERMINAL_COLS: u16 = 100;
const TERMINAL_ROWS: u16 = 34;
const RETRY_DELAY: Duration = Duration::from_secs(1);
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_TERMINAL_BACKGROUND_RGB: u32 = 0x17191d;
const DEFAULT_TERMINAL_FOREGROUND_RGB: u32 = 0xd7dae0;
const SHELL_COLOR_SMOKE_PROBE: &[u8] =
    b"printf '\\033[31mRED \\033[32mGREEN \\033[34mBLUE\\033[0m DEFAULT\\n'\nprintf 'SLINT_POC_IROH_ALACRITTY_READY\\n'\r";
const SHELL_READINESS_PROBE: &[u8] = b"printf 'SLINT_POC_IROH_ALACRITTY_READY\\n'\r";

pub fn run_app() -> Result<(), slint::PlatformError> {
    let app = AppWindow::new()?;
    #[cfg(not(target_os = "android"))]
    slint::set_xdg_app_id("com.anotherone.SlintPoc")?;
    apply_tokens(&app);

    let (input_tx, input_rx) = mpsc::unbounded_channel::<TerminalInputEvent>();
    app.on_terminal_key(move |text, control, alt, _shift| {
        if let Some(bytes) = encode_terminal_key(text.as_str(), control, alt) {
            let _ = input_tx.send(TerminalInputEvent::Key { bytes });
        }
    });

    spawn_terminal_worker(app.as_weak(), input_rx);
    app.on_close_requested(|| std::process::exit(0));
    app.run()
}

#[cfg(target_os = "android")]
#[no_mangle]
pub fn android_main(app: slint::android::AndroidApp) {
    if let Err(error) = slint::android::init(app) {
        eprintln!("slint-poc android backend init failed: {error}");
        return;
    }

    if let Err(error) = run_app() {
        eprintln!("slint-poc android startup failed: {error}");
    }
}

fn apply_tokens(app: &AppWindow) {
    app.set_chrome_bg(argb(0xff, 0x27, 0x29, 0x2e));
    app.set_card_bg(argb(0xff, 0x2b, 0x2d, 0x31));
    app.set_terminal_bg(argb(0xff, 0x17, 0x19, 0x1d));
    app.set_overlay_hover(argb(0x0f, 0xff, 0xff, 0xff));
    app.set_overlay_active(argb(0x1a, 0xff, 0xff, 0xff));
    app.set_focus_ring(argb(0xff, 0x5d, 0x7a, 0xd5));
    app.set_text_primary(argb(0xff, 0xeb, 0xeb, 0xeb));
    app.set_text_secondary(argb(0xff, 0xc7, 0xc7, 0xc7));
    app.set_text_muted(argb(0xff, 0x94, 0x94, 0x94));
}

fn argb(a: u8, r: u8, g: u8, b: u8) -> slint::Color {
    slint::Color::from_argb_u8(a, r, g, b)
}

fn spawn_terminal_worker(
    app_weak: slint::Weak<AppWindow>,
    mut input_rx: mpsc::UnboundedReceiver<TerminalInputEvent>,
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
                if input_rx.is_closed() {
                    break;
                }

                if let Err(error) = run_terminal_session(&app_weak, &mut input_rx).await {
                    set_terminal_status(&app_weak, format!("terminal: {error:#}; retrying"));
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        });
    });
}

async fn run_terminal_session(
    app_weak: &slint::Weak<AppWindow>,
    input_rx: &mut mpsc::UnboundedReceiver<TerminalInputEvent>,
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
    let (section_id, tab_id) = loop {
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
                let Some(project) = projects.first() else {
                    anyhow::bail!("daemon-sandbox returned no projects");
                };
                let Some(task) = project.tasks.first() else {
                    anyhow::bail!("daemon-sandbox project returned no tasks");
                };
                let Some(tab) = task.tabs.first() else {
                    anyhow::bail!("daemon-sandbox task returned no tabs");
                };
                break (task.section_id.clone(), tab.id.clone());
            }
            WorkerReply::Err { message, .. } => anyhow::bail!("list_projects failed: {message}"),
            _ => {}
        }
    };

    send_control(
        &mut send,
        &mut next_request_id,
        Control::LaunchTab {
            section_id: section_id.clone(),
            tab_id: tab_id.clone(),
        },
    )
    .await?;
    send_control(
        &mut send,
        &mut next_request_id,
        Control::AttachTab {
            section_id: section_id.clone(),
            tab_id: tab_id.clone(),
        },
    )
    .await?;
    send_control(
        &mut send,
        &mut next_request_id,
        Control::TabResize {
            cols: TERMINAL_COLS,
            rows: TERMINAL_ROWS,
        },
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
        format!("terminal: attached {section_id}/{tab_id} at {TERMINAL_COLS}x{TERMINAL_ROWS}"),
    );

    let mut terminal = AlacrittySnapshot::new(TERMINAL_COLS, TERMINAL_ROWS);
    let mut frame_tick = tokio::time::interval(Duration::from_millis(33));
    let mut dirty = true;

    loop {
        tokio::select! {
            maybe_input = input_rx.recv() => {
                let Some(input) = maybe_input else {
                    anyhow::bail!("input channel closed");
                };
                send_terminal_input(&mut send, &mut next_request_id, input)
                    .await
                    .context("send terminal input")?;
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
                    }
                    frame::TY_WORKER_REPLY => {
                        if let Ok(envelope) = serde_json::from_slice::<WorkerReplyEnvelope>(&payload) {
                            if let WorkerReply::Err { message, .. } = envelope.reply {
                                set_terminal_status(app_weak, format!("terminal worker error: {message}"));
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ = frame_tick.tick() => {
                if dirty {
                    set_terminal_surface(app_weak, terminal.snapshot_surface());
                    dirty = false;
                }
            }
        }
    }
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
            app.set_terminal_runs(slint::ModelRc::new(slint::VecModel::from(
                surface.text_runs,
            )));
        }
    });
}

fn encode_terminal_key(text: &str, control: bool, alt: bool) -> Option<Vec<u8>> {
    let mut bytes = match text {
        "\u{0008}" => vec![0x7f],
        "\u{0009}" => b"\t".to_vec(),
        "\u{000a}" => b"\r".to_vec(),
        "\u{001b}" => b"\x1b".to_vec(),
        "\u{007f}" => b"\x1b[3~".to_vec(),
        "\u{f700}" => b"\x1b[A".to_vec(),
        "\u{f701}" => b"\x1b[B".to_vec(),
        "\u{f702}" => b"\x1b[D".to_vec(),
        "\u{f703}" => b"\x1b[C".to_vec(),
        "\u{f729}" => b"\x1b[H".to_vec(),
        "\u{f72b}" => b"\x1b[F".to_vec(),
        "\u{f72c}" => b"\x1b[5~".to_vec(),
        "\u{f72d}" => b"\x1b[6~".to_vec(),
        value if value.chars().count() == 1 => {
            let ch = value.chars().next()?;
            if control {
                control_key_byte(ch)?
            } else {
                value.as_bytes().to_vec()
            }
        }
        value if !control => value.as_bytes().to_vec(),
        _ => return None,
    };

    if alt {
        bytes.insert(0, 0x1b);
    }

    Some(bytes)
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
    match std::env::var("SLINT_POC_STARTUP_PROBE").as_deref() {
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

    run.line == line
        && run.style == *style
        && !text.is_empty()
        && run.text.ends_with('\u{200d}')
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
}
