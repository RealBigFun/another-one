//! Centralized `alacritty_terminal::Term` construction.
//!
//! Single source of truth for the per-tab `Config` (scrollback
//! history, cursor defaults, semantic escape detection, …) and the
//! `Term::new(...)` invocation. Adding a knob to terminal behaviour
//! goes here so production paths and tests pick up the change in
//! lockstep.
//!
//! Today the only production caller is
//! `terminal::task::TerminalTask::new`. Step 9 of `rc/terminal-hardening`
//! collapsed `app/src/terminal_runtime.rs`'s constructors after
//! Step 7 retired the renderer-side parser; this module is the
//! survivor.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::{Config, Term};
use another_one_core::terminal_types::TerminalGridSize;

/// Build the `Config` every per-tab `Term` is constructed with.
/// Currently `Config::default()` (which carries alacritty's 10_000-
/// line scrollback default and the steady-block cursor we want);
/// future tunables (custom scrollback depth, semantic escape rules,
/// …) thread through here so callers stay one-line.
pub fn default_term_config() -> Config {
    Config::default()
}

/// Construct a per-tab `Term` with the daemon's standard config.
/// `listener` is forwarded to alacritty so the caller can plug in
/// its preferred event proxy (the per-tab task pushes events into
/// a `VecDeque`; tests use `VoidListener`).
pub fn make_term<L: EventListener>(size: TerminalGridSize, listener: L) -> Term<L> {
    Term::new(default_term_config(), &size, listener)
}
