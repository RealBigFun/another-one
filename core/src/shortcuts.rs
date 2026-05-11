//! Shortcut configuration: pure enums, defaults, and persistence.
//!
//! Data-only. The counterpart in `desktop/src/shortcuts.rs` holds the
//! GPUI `KeyDownEvent`-facing side (event capture, key matching,
//! token-label rendering) — those live on the UI crate because the
//! `KeyDownEvent` type is a GPUI concept.
//!
//! `ShortcutAction::default_binding` does select a platform-specific
//! default for `CloseCurrentTab` (cmd-w vs control-w), but it does
//! that via inline `cfg!(target_os = ...)` here rather than routing
//! through desktop's GPUI-coupled `PlatformServices` trait — that
//! keeps the core crate free of the platform shim.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    CycleProjects,
    NewTabInCurrentTask,
    NewTask,
    CloseCurrentTab,
    NextTab,
    PreviousTab,
    NextTask,
    PreviousTask,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

pub const ALL_SHORTCUT_ACTIONS: [ShortcutAction; 11] = [
    ShortcutAction::CycleProjects,
    ShortcutAction::NewTabInCurrentTask,
    ShortcutAction::NewTask,
    ShortcutAction::CloseCurrentTab,
    ShortcutAction::NextTab,
    ShortcutAction::PreviousTab,
    ShortcutAction::NextTask,
    ShortcutAction::PreviousTask,
    ShortcutAction::ZoomIn,
    ShortcutAction::ZoomOut,
    ShortcutAction::ZoomReset,
];

impl ShortcutAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::CycleProjects => "Cycle Projects",
            Self::NewTabInCurrentTask => "New Tab in Current Task",
            Self::NewTask => "New Task",
            Self::CloseCurrentTab => "Close Current Tab",
            Self::NextTab => "Next Tab",
            Self::PreviousTab => "Previous Tab",
            Self::NextTask => "Next Task",
            Self::PreviousTask => "Previous Task",
            Self::ZoomIn => "Zoom In",
            Self::ZoomOut => "Zoom Out",
            Self::ZoomReset => "Reset Zoom",
        }
    }

    pub fn default_binding(self) -> &'static str {
        match self {
            Self::CycleProjects => "cmd-o",
            Self::NewTabInCurrentTask => "cmd-n",
            Self::NewTask => "cmd-t",
            Self::CloseCurrentTab => close_current_tab_default_binding(),
            Self::NextTab => "cmd-shift-]",
            Self::PreviousTab => "cmd-shift-[",
            Self::NextTask => "cmd-alt-down",
            Self::PreviousTask => "cmd-alt-up",
            Self::ZoomIn => zoom_in_default_binding(),
            Self::ZoomOut => zoom_out_default_binding(),
            Self::ZoomReset => zoom_reset_default_binding(),
        }
    }
}

const fn close_current_tab_default_binding() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "cmd-w"
    }
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "android"))]
    {
        "control-w"
    }
    // iOS doesn't surface a hardware-keyboard shortcut for tab close
    // in the GPUI build; mobile drives this via the on-screen tab
    // menu instead. Return an empty string so the const-fn remains
    // total — callers that depend on a real binding gate on
    // `Platform::is_keyboard_capable()` already.
    #[cfg(target_os = "ios")]
    {
        ""
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows",
        target_os = "android",
        target_os = "ios",
    )))]
    {
        compile_error!(
            "another-one-core: add a `close_current_tab` default binding \
             for this target."
        );
        ""
    }
}

/// Zoom shortcuts used the GPUI `KeyBinding::new("cmd-=", …)`
/// plumbing before #62. On Linux/Windows the `cmd-` token maps to
/// the Super/Meta key, not Ctrl, so the bindings silently did
/// nothing — which is the bug report #62 is tracking. Promoting
/// zoom to `ShortcutAction` lets each platform advertise a sane
/// default and lets users remap through the same settings page as
/// the other shortcuts.
const fn zoom_in_default_binding() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "cmd-="
    }
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "android"))]
    {
        "control-="
    }
    #[cfg(target_os = "ios")]
    {
        ""
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows",
        target_os = "android",
        target_os = "ios",
    )))]
    {
        compile_error!("another-one-core: add a `zoom_in` default binding for this target.");
        ""
    }
}

const fn zoom_out_default_binding() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "cmd--"
    }
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "android"))]
    {
        "control--"
    }
    #[cfg(target_os = "ios")]
    {
        ""
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows",
        target_os = "android",
        target_os = "ios",
    )))]
    {
        compile_error!("another-one-core: add a `zoom_out` default binding for this target.");
        ""
    }
}

const fn zoom_reset_default_binding() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "cmd-0"
    }
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "android"))]
    {
        "control-0"
    }
    #[cfg(target_os = "ios")]
    {
        ""
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows",
        target_os = "android",
        target_os = "ios",
    )))]
    {
        compile_error!("another-one-core: add a `zoom_reset` default binding for this target.");
        ""
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutSettings {
    #[serde(default = "default_cycle_projects_shortcut")]
    pub cycle_projects: String,
    #[serde(default = "default_new_tab_in_current_task_shortcut")]
    pub new_tab_in_current_task: String,
    #[serde(default = "default_new_task_shortcut")]
    pub new_task: String,
    #[serde(default = "default_close_current_tab_shortcut")]
    pub close_current_tab: String,
    #[serde(default = "default_next_tab_shortcut")]
    pub next_tab: String,
    #[serde(default = "default_previous_tab_shortcut")]
    pub previous_tab: String,
    #[serde(default = "default_next_task_shortcut")]
    pub next_task: String,
    #[serde(default = "default_previous_task_shortcut")]
    pub previous_task: String,
    #[serde(default = "default_zoom_in_shortcut")]
    pub zoom_in: String,
    #[serde(default = "default_zoom_out_shortcut")]
    pub zoom_out: String,
    #[serde(default = "default_zoom_reset_shortcut")]
    pub zoom_reset: String,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            cycle_projects: default_cycle_projects_shortcut(),
            new_tab_in_current_task: default_new_tab_in_current_task_shortcut(),
            new_task: default_new_task_shortcut(),
            close_current_tab: default_close_current_tab_shortcut(),
            next_tab: default_next_tab_shortcut(),
            previous_tab: default_previous_tab_shortcut(),
            next_task: default_next_task_shortcut(),
            previous_task: default_previous_task_shortcut(),
            zoom_in: default_zoom_in_shortcut(),
            zoom_out: default_zoom_out_shortcut(),
            zoom_reset: default_zoom_reset_shortcut(),
        }
    }
}

impl ShortcutSettings {
    pub fn binding_for(&self, action: ShortcutAction) -> &str {
        match action {
            ShortcutAction::CycleProjects => &self.cycle_projects,
            ShortcutAction::NewTabInCurrentTask => &self.new_tab_in_current_task,
            ShortcutAction::NewTask => &self.new_task,
            ShortcutAction::CloseCurrentTab => &self.close_current_tab,
            ShortcutAction::NextTab => &self.next_tab,
            ShortcutAction::PreviousTab => &self.previous_tab,
            ShortcutAction::NextTask => &self.next_task,
            ShortcutAction::PreviousTask => &self.previous_task,
            ShortcutAction::ZoomIn => &self.zoom_in,
            ShortcutAction::ZoomOut => &self.zoom_out,
            ShortcutAction::ZoomReset => &self.zoom_reset,
        }
    }

    pub fn set_binding(&mut self, action: ShortcutAction, binding: impl Into<String>) {
        let binding = binding.into();
        match action {
            ShortcutAction::CycleProjects => self.cycle_projects = binding,
            ShortcutAction::NewTabInCurrentTask => self.new_tab_in_current_task = binding,
            ShortcutAction::NewTask => self.new_task = binding,
            ShortcutAction::CloseCurrentTab => self.close_current_tab = binding,
            ShortcutAction::NextTab => self.next_tab = binding,
            ShortcutAction::PreviousTab => self.previous_tab = binding,
            ShortcutAction::NextTask => self.next_task = binding,
            ShortcutAction::PreviousTask => self.previous_task = binding,
            ShortcutAction::ZoomIn => self.zoom_in = binding,
            ShortcutAction::ZoomOut => self.zoom_out = binding,
            ShortcutAction::ZoomReset => self.zoom_reset = binding,
        }
    }

    pub fn clear_binding(&mut self, action: ShortcutAction) {
        self.set_binding(action, String::new());
    }

    pub fn reset_binding(&mut self, action: ShortcutAction) {
        self.set_binding(action, action.default_binding().to_string());
    }

    pub fn reset_all(&mut self) {
        *self = Self::default();
    }

    pub fn action_for_binding(
        &self,
        ignored_action: ShortcutAction,
        binding: &str,
    ) -> Option<ShortcutAction> {
        ALL_SHORTCUT_ACTIONS.into_iter().find(|action| {
            *action != ignored_action
                && !self.binding_for(*action).is_empty()
                && self.binding_for(*action) == binding
        })
    }
}

fn default_cycle_projects_shortcut() -> String {
    ShortcutAction::CycleProjects.default_binding().to_string()
}

fn default_new_tab_in_current_task_shortcut() -> String {
    ShortcutAction::NewTabInCurrentTask
        .default_binding()
        .to_string()
}

fn default_new_task_shortcut() -> String {
    ShortcutAction::NewTask.default_binding().to_string()
}

fn default_close_current_tab_shortcut() -> String {
    ShortcutAction::CloseCurrentTab
        .default_binding()
        .to_string()
}

fn default_next_tab_shortcut() -> String {
    ShortcutAction::NextTab.default_binding().to_string()
}

fn default_previous_tab_shortcut() -> String {
    ShortcutAction::PreviousTab.default_binding().to_string()
}

fn default_next_task_shortcut() -> String {
    ShortcutAction::NextTask.default_binding().to_string()
}

fn default_previous_task_shortcut() -> String {
    ShortcutAction::PreviousTask.default_binding().to_string()
}

fn default_zoom_in_shortcut() -> String {
    ShortcutAction::ZoomIn.default_binding().to_string()
}

fn default_zoom_out_shortcut() -> String {
    ShortcutAction::ZoomOut.default_binding().to_string()
}

fn default_zoom_reset_shortcut() -> String {
    ShortcutAction::ZoomReset.default_binding().to_string()
}
