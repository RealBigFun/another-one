//! `SectionId`: the stable key for a specific branch/task within a
//! project.
//!
//! Extracted from `desktop/src/app.rs` so core-side types (terminal
//! launch, terminal runtime data, the upcoming `TerminalManager`) can
//! reference it without the binary-only `app` module coming along.

/// Identifies a section: a specific branch within a specific project,
/// optionally narrowed to a specific task.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SectionId {
    pub project_id: String,
    pub branch_name: String,
    pub task_id: Option<String>,
}

impl SectionId {
    pub fn new(project_id: &str, branch_name: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
            branch_name: branch_name.to_string(),
            task_id: None,
        }
    }

    pub fn for_task(project_id: &str, branch_name: &str, task_id: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
            branch_name: branch_name.to_string(),
            task_id: Some(task_id.to_string()),
        }
    }

    pub fn store_key(&self) -> String {
        format!(
            "{}::{}::{}",
            self.project_id,
            self.branch_name,
            self.task_id.as_deref().unwrap_or("")
        )
    }

    pub fn from_store_key(key: &str) -> Option<Self> {
        let mut parts = key.splitn(3, "::");
        let project_id = parts.next()?.to_string();
        let branch_name = parts.next()?.to_string();
        let task_id = parts.next()?.to_string();

        Some(Self {
            project_id,
            branch_name,
            task_id: (!task_id.is_empty()).then_some(task_id),
        })
    }
}
