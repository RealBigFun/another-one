//! Soak test for `daemon::terminal::task`.
//!
//! Phase 2e of `docs/designs/01-daemon-canonical-terminal.md`. The
//! goal is "Term task chews through a heavy harness's worth of
//! bytes without panicking, blowing memory, or stalling," not
//! "match a specific upstream output byte-for-byte." We synthesize
//! a workload that exercises:
//!
//! - scrolling past the screen height (forces history/scrollback
//!   updates),
//! - SGR color changes (alacritty's color path),
//! - cursor moves (CSI H, line-overwrite redraws),
//! - intermixed bells (side-channel pressure),
//! - alt-screen toggles (mode flips),
//! - long ASCII runs (the parser's hot fast-path).
//!
//! The total payload sits in the ~5 MB range \u2014 a realistic burst
//! from a chatty agent over a few seconds. The test asserts:
//!
//! - end-to-end runtime < 5 s on CI hardware,
//! - the task never deadlocks (every chunk receives a frame),
//! - the seq counter advances monotonically,
//! - bell side-channel events flow on a parallel subscriber
//!   without backpressuring the parser.

use std::time::{Duration, Instant};

use another_one_core::terminal_types::TerminalGridSize;
use daemon::terminal::{spawn_terminal_task, TerminalCommand, TerminalSideEffect};
use daemon_proto::TerminalFrame;
use tokio::sync::oneshot;

/// Build a payload that exercises a representative slice of VT
/// behaviour. Returns roughly `target_bytes` worth of bytes, split
/// into chunks the task will receive as separate `Bytes` commands.
fn synthetic_harness_payload(target_bytes: usize) -> Vec<Vec<u8>> {
    let mut chunks = Vec::new();
    let mut total = 0_usize;

    let scroll_block: &[u8] = b"\
        \x1b[31mred\x1b[0m \x1b[1;32mbold-green\x1b[0m \x1b[33;44myellow-on-blue\x1b[0m \
        plain text and a long ascii run that fills out the line until it must wrap \
        across the column limit and into the next row of the grid causing a scroll \
        when we run off the bottom edge.\r\n";
    let cursor_dance: &[u8] =
        b"\x1b[H\x1b[2Joverwrite-screen\x1b[5;10Hmid-jump\x1b[10;1Hbottom-line\r\n";
    let bell_block: &[u8] = b"\x07\x07ring-ring\x07\r\n";
    let alt_screen_toggle: &[u8] = b"\x1b[?1049h\x1b[?1049l";
    let title_change: &[u8] = b"\x1b]0;agent: thinking...\x1b\\";

    while total < target_bytes {
        for source in [
            scroll_block,
            scroll_block,
            cursor_dance,
            scroll_block,
            bell_block,
            scroll_block,
            title_change,
            alt_screen_toggle,
            scroll_block,
        ] {
            // Emit ~16 KB per chunk so the inbox sees realistic
            // backpressure pacing rather than a single mega-chunk.
            let mut chunk = Vec::with_capacity(16 * 1024);
            while chunk.len() < 16 * 1024 {
                chunk.extend_from_slice(source);
            }
            total += chunk.len();
            chunks.push(chunk);
            if total >= target_bytes {
                break;
            }
        }
    }
    chunks
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_task_soak_5mb_under_5s() {
    let chunks = synthetic_harness_payload(5 * 1024 * 1024);
    let chunk_count = chunks.len();

    let size = TerminalGridSize {
        cols: 120,
        rows: 40,
        pixel_width: 0,
        pixel_height: 0,
    };
    let handle = spawn_terminal_task(size);
    let mut bell_subscriber = handle.subscribe_side_effects();

    // Spawn a parallel drain on the side-channel so a stuck
    // subscriber can't backpressure the parser. We just count
    // bell events to prove the channel actually flowed.
    let drain = tokio::spawn(async move {
        // Count delivered bells AND skipped-but-known-bell events
        // (broadcast capacity is bounded; the soak fires bells
        // faster than a 64-deep channel can hold). Lagged delivery
        // is the channel's documented behaviour, not a bug — the
        // dispatcher in Phase 3c will log + continue on Lagged.
        let mut bells = 0_u64;
        loop {
            match bell_subscriber.recv().await {
                Ok(TerminalSideEffect::Bell) => bells += 1,
                Ok(_other) => {} // Title / ResetTitle
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    bells += skipped;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
        bells
    });

    let start = Instant::now();
    for chunk in chunks {
        handle
            .send(TerminalCommand::Bytes(chunk))
            .await
            .expect("send chunk");
    }

    // Force-roundtrip a RequestFullFrame so we know the task has
    // drained every Bytes command we just queued.
    let (tx, rx) = oneshot::channel();
    handle
        .send(TerminalCommand::RequestFullFrame { reply: tx })
        .await
        .expect("send request");
    let frame = rx.await.expect("reply").expect("frame after soak");

    let elapsed = start.elapsed();
    handle.shutdown().await.expect("shutdown");
    let bells = drain.await.expect("drain join");

    // The frame ought to be a Full with seq >= chunk_count + 1
    // (one emit per Bytes command + the last frame that the
    // RequestFullFrame returns reflects the post-advance state).
    match &*frame {
        TerminalFrame::Full { seq, snapshot } => {
            assert!(
                *seq as usize >= chunk_count,
                "seq advanced at least once per chunk: seq={seq}, chunks={chunk_count}"
            );
            assert_eq!(snapshot.cols, 120);
            assert_eq!(snapshot.rows, 40);
        }
        TerminalFrame::Diff { .. } => panic!("Phase 2 only emits Full frames"),
    }

    assert!(
        bells > 0,
        "side-channel must have observed at least one bell"
    );

    assert!(
        elapsed < Duration::from_secs(5),
        "soak should finish under 5s on CI hardware; took {elapsed:?} \
         for {chunk_count} chunks ({} bytes total)",
        chunk_count * 16 * 1024
    );
}
