//! Project/task/worktree workflow helpers that keep persistence-adjacent
//! decisions out of UI rendering code.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::project_store::{Project, ProjectStore};
use another_one_core::section::SectionId;

pub(crate) fn removed_repo_ids_without_remaining_projects(
    projects: &[Project],
    removed_project_ids: &HashSet<String>,
) -> HashSet<String> {
    let removed_repo_ids = projects
        .iter()
        .filter(|project| removed_project_ids.contains(&project.id))
        .map(|project| project.repo_id.clone())
        .collect::<HashSet<_>>();

    removed_repo_ids
        .into_iter()
        .filter(|repo_id| {
            !projects.iter().any(|project| {
                project.repo_id == *repo_id && !removed_project_ids.contains(&project.id)
            })
        })
        .collect()
}

pub(crate) fn fallback_section_after_project_removal(
    store: &ProjectStore,
) -> Option<(SectionId, PathBuf)> {
    let project = store.projects.first()?;
    let branch_name = store.current_branch_name(&project.id)?;
    Some((
        SectionId::new(&project.id, &branch_name),
        project.path.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(id: &str, repo_id: &str) -> Project {
        Project {
            id: id.into(),
            repo_id: repo_id.into(),
            name: id.into(),
            path: PathBuf::from(format!("/tmp/{id}")),
            kind: crate::project_store::ProjectKind::Root,
            checkout: crate::project_store::ProjectCheckoutState::default(),
            branch_settings: Default::default(),
            actions: Vec::new(),
            worktree_name: None,
            repo_common_dir: None,
        }
    }

    #[test]
    fn removal_keeps_repo_when_sibling_project_remains() {
        let projects = vec![project("root", "repo"), project("worktree", "repo")];
        let removed = HashSet::from(["worktree".to_string()]);

        assert!(removed_repo_ids_without_remaining_projects(&projects, &removed).is_empty());
    }

    #[test]
    fn removal_returns_repo_when_last_project_is_removed() {
        let projects = vec![project("root", "repo")];
        let removed = HashSet::from(["root".to_string()]);

        assert!(removed_repo_ids_without_remaining_projects(&projects, &removed).contains("repo"));
    }
}
