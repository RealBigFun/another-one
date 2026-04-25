//! Headless core for AnotherOne.
//!
//! What belongs here: pure domain types, persistence formats, and
//! shell-out helpers that neither depend on GPUI nor care about which
//! UI shell calls them. The desktop (`desktop/` crate, GPUI + Rust) and
//! the future sandbox/mobile daemon both consume this.
//!
//! What does *not* belong here: anything that imports `gpui::*`,
//! anything that holds an `Entity` handle, anything that mutates UI
//! state directly. Event types from GPUI (`KeyDownEvent`, etc.) are
//! the domain of the UI crate; core describes data and operations in
//! UI-agnostic terms.
//!
//! Extraction is proceeding file-by-file from `desktop/src/` per the
//! Phase 1 plan in `docs/`. This is PR 1 — the two smallest,
//! zero-intra-crate-dep files.

pub mod agents;
pub mod git_actions;
pub mod git_service;
pub mod leakscope;
pub mod mcp;
pub mod open_in;
pub mod platform;
pub mod process;
pub mod project_service;
pub mod project_store;
pub mod section;
pub mod shortcuts;
pub mod terminal_launch;
pub mod terminal_manager;
pub mod terminal_types;
