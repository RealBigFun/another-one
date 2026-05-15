//! Typed scope tree: System → Project → Task → Tab.
//!
//! A capability is resolved against the layer of context the caller
//! holds. Every level carries an `Arc` to its parent so a call site
//! at `Tab` depth can still ask for system info; conversely, the
//! capability registry rejects (returns no matches) when a capability
//! needs a deeper layer than the caller supplied.
//!
//! Fields here are factual context only (paths, ids, probe state) —
//! no behavior. Behavior belongs on capability traits in
//! `crate::capability` and the per-domain modules (`crate::git`,
//! `crate::git_remote`, …).

use std::path::PathBuf;
use std::sync::Arc;

mod tool_probe;

pub use tool_probe::ToolProbe;

/// Static facts about the host process. Today this is just an OS
/// identifier and the tool-PATH probe; it grows as capabilities need
/// more system-level inputs.
pub struct SystemScope {
    pub os: &'static str,
    pub tool_probe: Arc<ToolProbe>,
}

impl SystemScope {
    pub fn new(tool_probe: Arc<ToolProbe>) -> Arc<Self> {
        Arc::new(Self {
            os: std::env::consts::OS,
            tool_probe,
        })
    }
}

/// A project: an on-disk repo root the user has opened. Wraps the
/// `PathBuf` that PR #185 already threads as `project_path`.
pub struct ProjectScope {
    pub system: Arc<SystemScope>,
    pub project_id: String,
    pub root: PathBuf,
}

impl ProjectScope {
    pub fn new(system: Arc<SystemScope>, project_id: String, root: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            system,
            project_id,
            root,
        })
    }
}

/// A task inside a project (a worktree + branch). Optional fields
/// reflect the existing `Task` shape in `project_store`: a task may
/// not yet have a branch or a worktree path.
pub struct TaskScope {
    pub project: Arc<ProjectScope>,
    pub task_id: String,
    pub branch: Option<String>,
    pub worktree: Option<PathBuf>,
}

impl TaskScope {
    pub fn new(
        project: Arc<ProjectScope>,
        task_id: String,
        branch: Option<String>,
        worktree: Option<PathBuf>,
    ) -> Arc<Self> {
        Arc::new(Self {
            project,
            task_id,
            branch,
            worktree,
        })
    }
}

/// A terminal tab inside a task. `cwd` is the tab's working
/// directory (the "pwd/harness" layer of context).
pub struct TabScope {
    pub task: Arc<TaskScope>,
    pub tab_id: String,
    pub cwd: PathBuf,
}

impl TabScope {
    pub fn new(task: Arc<TaskScope>, tab_id: String, cwd: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            task,
            tab_id,
            cwd,
        })
    }
}

/// A scope-layer reference. Capability impls call `scope.system()`,
/// `scope.project()`, etc. to read context at the depth they need.
/// `applies()` returns `false` (so the registry filters the impl
/// out) when the scope is shallower than the capability requires.
#[derive(Clone)]
pub enum Scope {
    System(Arc<SystemScope>),
    Project(Arc<ProjectScope>),
    Task(Arc<TaskScope>),
    Tab(Arc<TabScope>),
}

impl Scope {
    pub fn system(&self) -> &SystemScope {
        match self {
            Scope::System(s) => s,
            Scope::Project(p) => &p.system,
            Scope::Task(t) => &t.project.system,
            Scope::Tab(t) => &t.task.project.system,
        }
    }

    pub fn project(&self) -> Option<&ProjectScope> {
        match self {
            Scope::System(_) => None,
            Scope::Project(p) => Some(p),
            Scope::Task(t) => Some(&t.project),
            Scope::Tab(t) => Some(&t.task.project),
        }
    }

    pub fn task(&self) -> Option<&TaskScope> {
        match self {
            Scope::System(_) | Scope::Project(_) => None,
            Scope::Task(t) => Some(t),
            Scope::Tab(t) => Some(&t.task),
        }
    }

    pub fn tab(&self) -> Option<&TabScope> {
        match self {
            Scope::Tab(t) => Some(t),
            _ => None,
        }
    }
}

impl From<Arc<SystemScope>> for Scope {
    fn from(s: Arc<SystemScope>) -> Self {
        Scope::System(s)
    }
}
impl From<Arc<ProjectScope>> for Scope {
    fn from(s: Arc<ProjectScope>) -> Self {
        Scope::Project(s)
    }
}
impl From<Arc<TaskScope>> for Scope {
    fn from(s: Arc<TaskScope>) -> Self {
        Scope::Task(s)
    }
}
impl From<Arc<TabScope>> for Scope {
    fn from(s: Arc<TabScope>) -> Self {
        Scope::Tab(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_up_from_deepest_layer() {
        let probe = Arc::new(ToolProbe::new());
        let sys = SystemScope::new(probe);
        let proj = ProjectScope::new(sys.clone(), "p".into(), PathBuf::from("/tmp"));
        let task = TaskScope::new(proj.clone(), "t".into(), Some("main".into()), None);
        let tab = TabScope::new(task.clone(), "tab".into(), PathBuf::from("/tmp"));
        let scope: Scope = tab.into();
        assert!(scope.system().os == std::env::consts::OS);
        assert_eq!(scope.project().unwrap().project_id, "p");
        assert_eq!(scope.task().unwrap().task_id, "t");
        assert_eq!(scope.tab().unwrap().tab_id, "tab");
    }

    #[test]
    fn shallow_scope_yields_none_for_deeper_layers() {
        let probe = Arc::new(ToolProbe::new());
        let sys = SystemScope::new(probe);
        let scope: Scope = sys.into();
        assert!(scope.project().is_none());
        assert!(scope.task().is_none());
        assert!(scope.tab().is_none());
    }
}
