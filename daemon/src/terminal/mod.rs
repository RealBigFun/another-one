//! Daemon-canonical terminal state.
//!
//! Per-tab `alacritty_terminal::Term` ownership lives here. Each
//! tab is a tokio task ([`task::TerminalTask`]) the daemon spawns
//! when its PTY launches and aborts when the PTY exits. The task
//! owns the Term + Processor and feeds bytes through
//! `Processor::advance` off any viewer's render thread.
//!
//! See [`docs/designs/01-daemon-canonical-terminal.md`] for the
//! design and phasing. Phase 2 lands the per-tab task scaffolding
//! in isolation; Phase 3 wires per-viewer pacers; Phase 4 moves
//! PTY spawn here. Refs #158.

pub mod task;

pub use task::{spawn_terminal_task, TerminalCommand, TerminalTaskHandle};
