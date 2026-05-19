//! Stable identifiers, metadata, and a per-render registry for UI
//! controls (buttons, toggles, …).
//!
//! ## Purpose
//!
//! Every interactive element that registers itself here gains a stable
//! [`ControlId`] and a compact [`ControlEntry`] in the current-frame
//! [`ControlRegistry`]. The registry is cleared at the top of each
//! render and re-populated during element construction, so its contents
//! always reflect the live frame.
//!
//! The `test-harness` feature exposes `simulate_click` / `simulate_toggle`
//! on `AnotherOneApp`, enabling interaction tests that do not require a
//! real window. See the `test-harness` gate in `app/src/lib.rs`.
//!
//! ## Migration order (from issue #198)
//!
//! 1. ✅ Types defined here (no call sites yet).
//! 2. `control_registry` field added to `AnotherOneApp`, cleared in `render`.
//! 3. Convert `sidebar_task_menu_item`, `settings_general_button`,
//!    `settings_theme_button` one at a time.
//! 4. Add `simulate_click` / `simulate_toggle` behind `test-harness`.
//! 5. Expand to left sidebar → settings → header → project panes.

use gpui::SharedString;

// ---- ControlKind --------------------------------------------------------

/// The interactive behaviour of a registered control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ControlKind {
    Button,
    Toggle { selected: bool },
}

// ---- ControlId ----------------------------------------------------------

/// Stable identifier for a control within a single rendered frame.
///
/// `Static` covers buttons with a constant id string (most settings page
/// buttons). `Task` and future variants handle per-row or per-item ids
/// where the same builder function is called N times with different data.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ControlId {
    /// A button whose id is a compile-time string (e.g. `"theme-light"`).
    Static(&'static str),
    /// A per-task control — the task id disambiguates within a kind bucket.
    Task {
        task_id: SharedString,
        kind: TaskControl,
    },
}

/// Discriminator for task-scoped controls so ids stay collision-free even
/// when multiple control kinds exist per task row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum TaskControl {
    Rename,
    Delete,
    Pin,
    Open,
}

// ---- ControlEntry -------------------------------------------------------

/// A single registered control for the current frame.
pub(crate) struct ControlEntry {
    pub(crate) id: ControlId,
    pub(crate) label: SharedString,
    pub(crate) kind: ControlKind,
    pub(crate) enabled: bool,
    /// Stored action invoked by `simulate_click` / `simulate_toggle`.
    /// `None` for controls that drive mutations directly via closure rather
    /// than a GPUI `Action`.
    pub(crate) handler: Option<Box<dyn FnMut(&mut crate::app::AnotherOneApp, &mut gpui::Context<crate::app::AnotherOneApp>)>>,
}

// ---- ControlRegistry ----------------------------------------------------

/// Per-frame registry of all interactive controls. Cleared at the top of
/// `AnotherOneApp::render` and re-populated during element construction.
#[derive(Default)]
pub(crate) struct ControlRegistry {
    entries: Vec<ControlEntry>,
}

impl ControlRegistry {
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    /// Register a control for the current frame.
    pub(crate) fn register(&mut self, entry: ControlEntry) {
        self.entries.push(entry);
    }

    /// Look up a control by id. Returns the first matching entry.
    pub(crate) fn get(&self, id: &ControlId) -> Option<&ControlEntry> {
        self.entries.iter().find(|e| &e.id == id)
    }

    /// Mutable lookup — needed to invoke the stored handler.
    pub(crate) fn get_mut(&mut self, id: &ControlId) -> Option<&mut ControlEntry> {
        self.entries.iter_mut().find(|e| &e.id == id)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &ControlEntry> {
        self.entries.iter()
    }
}

// ---- ControlError -------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ControlError {
    NotFound,
    Disabled,
    WrongKind,
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ControlError::NotFound => write!(f, "control not found in current frame"),
            ControlError::Disabled => write!(f, "control is disabled"),
            ControlError::WrongKind => write!(f, "control kind mismatch"),
        }
    }
}
