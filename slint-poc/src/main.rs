use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{viewport_to_point, Config, Term};
use alacritty_terminal::vte::ansi;
use anyhow::Context;
use daemon_sandbox::frame::{self, Control, ControlEnvelope, WorkerReply, WorkerReplyEnvelope};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use tokio::sync::mpsc;

slint::include_modules!();

const TERMINAL_COLS: u16 = 100;
const TERMINAL_ROWS: u16 = 34;
const RETRY_DELAY: Duration = Duration::from_secs(1);
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> Result<(), slint::PlatformError> {
    let app = AppWindow::new()?;
    slint::set_xdg_app_id("com.anotherone.SlintPoc")?;
    apply_tokens(&app);

    let (input_tx, input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    app.on_terminal_key(move |text, control, alt, _shift| {
        if let Some(bytes) = encode_terminal_key(text.as_str(), control, alt) {
            let _ = input_tx.send(bytes);
        }
    });

    spawn_terminal_worker(app.as_weak(), input_rx);
    app.on_close_requested(|| std::process::exit(0));
    app.run()
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
    mut input_rx: mpsc::UnboundedReceiver<Vec<u8>>,
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
    input_rx: &mut mpsc::UnboundedReceiver<Vec<u8>>,
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
    frame::write_frame(
        &mut send,
        frame::TY_DATA,
        b"printf 'SLINT_POC_IROH_ALACRITTY_READY\\n'\r",
    )
    .await
    .context("send readiness probe")?;

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
                frame::write_frame(&mut send, frame::TY_DATA, &input)
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
                            frame::write_frame(&mut send, frame::TY_DATA, &reply)
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
                    set_terminal_text(app_weak, terminal.snapshot_text());
                    dirty = false;
                }
            }
        }
    }
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

fn set_terminal_text(app_weak: &slint::Weak<AppWindow>, text: String) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_terminal_text(text.into());
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

    fn snapshot_text(&self) -> String {
        let display_offset = self.term.renderable_content().display_offset;
        let mut lines = Vec::with_capacity(self.size.rows as usize);

        for viewport_line in 0..self.size.rows as usize {
            let point = viewport_to_point(display_offset, Point::new(viewport_line, Column(0)));
            let grid_line = &self.term.grid()[point.line];
            let mut line = String::with_capacity(self.size.cols as usize);

            for column in 0..self.size.cols as usize {
                let cell = &grid_line[Column(column)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                if cell.flags.contains(Flags::HIDDEN) {
                    line.push(' ');
                } else {
                    line.push(cell.c);
                    for zero_width in cell.zerowidth().into_iter().flatten() {
                        line.push(*zero_width);
                    }
                }
            }

            lines.push(line.trim_end().to_string());
        }

        lines.join("\n")
    }
}

fn window_size_from_grid(size: TerminalSize) -> WindowSize {
    WindowSize {
        num_lines: size.rows,
        num_cols: size.cols,
        cell_width: 8,
        cell_height: 16,
    }
}
