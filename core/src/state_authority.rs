//! Typed mutation API for the durable app state owned by the daemon.
//!
//! See `docs/architecture/daemon-owned-state-authority.md` and
//! `docs/architecture/sqlite-persistence.md` for the design.
//!
//! This module is **additive**. Today every external mutator of
//! `ProjectStore` either calls a `pub fn` on the store directly or
//! reaches into a `pub` field. Migrating those call sites to
//! [`apply`] is the next step (tracked in the audit at
//! `docs/architecture/sqlite-persistence-audit.md`). Once every
//! caller goes through `apply`, the `pub` field surface and the
//! whole-store clone seam in `app/src/app.rs` (`commit_local_mutation`
//! / `sync_registry_project_store`) get deleted, and the
//! [`Mutation`] / [`MutationOutcome`] pair becomes the only way to
//! change state.
//!
//! The dispatcher is intentionally a thin shim over the existing
//! `ProjectStore::*` methods. PR1 of the SQLite persistence move
//! is a behavior-neutral refactor — every variant should produce
//! the same on-disk JSON it does today. PR2 swaps the persistence
//! adapter and lights up row-level writes.

use std::collections::HashSet;

use daemon_proto::{ProjectSummary, RepoSummary, UiSnapshot};

use crate::git_actions::GitActionLlmSettings;
use crate::open_in::OpenInAppKind;
use crate::project_store::{
    InvalidProjectBranchSetting, PersistedSectionState, PreparedProject, ProjectAction,
    ProjectCheckoutState, ProjectStore, RepoDefaultCommitAction, ThemeMode,
};
use crate::shortcuts::ShortcutAction;

/// Every durable mutation flows through this enum. Variants are
/// derived from the audit at
/// `docs/architecture/sqlite-persistence-audit.md` — there is one
/// variant per real call site, no speculative ones.
///
/// Outcomes are returned as [`MutationOutcome`] so callers that care
/// about idempotence (`changed: bool`) or post-mutation projections
/// (e.g. the `TaskSummary` used by `Control::SetTaskName` /
/// `SetTaskPinned`) can stay pure-functional.
#[derive(Debug, Clone)]
pub enum Mutation {
    // ── Project catalog ─────────────────────────────────────────
    /// `Control::CreateProject` reply / GUI "Add Project…" /
    /// worktree-branch creation completion.
    InsertPreparedProject(PreparedProject),

    /// `Control::DeleteProject`.
    RemoveProject { project_id: String },

    /// `Control::SetBranchSetting{field:"default-branch"}`.
    SetDefaultBranch {
        project_id: String,
        branch_name: Option<String>,
    },

    /// `Control::SetBranchSetting{field:"default-target-branch"}`.
    SetDefaultTargetBranch {
        project_id: String,
        branch_name: Option<String>,
    },

    /// Post git-refresh sweep. Returns `Vec<InvalidProjectBranchSetting>`.
    ClearMissingBranchSettings { project_id: String },

    /// `Control::SetRepoDefaultCommitAction`.
    SetRepoDefaultCommitAction {
        repo_id: String,
        action: RepoDefaultCommitAction,
    },

    /// `Control::SetProjectAction`.
    UpsertProjectAction {
        project_id: String,
        action: ProjectAction,
        save_global_copy: bool,
    },

    /// `Control::DeleteProjectAction`.
    DeleteProjectAction {
        project_id: String,
        action_id: String,
    },

    /// Per-worktree checkout-state push from the git-refresh poll.
    UpdateWorktreeCheckout {
        worktree_id: String,
        checkout: ProjectCheckoutState,
    },

    // ── Task catalog ────────────────────────────────────────────
    /// `client_open_task` + `Control::CreateWorktreeTask` reply path.
    InsertTask { task: crate::project_store::Task },

    /// `Control::DeleteTask`.
    RemoveTask {
        root_project_id: String,
        task_id: String,
    },

    /// `Control::SetTaskName`.
    RenameTask { task_id: String, new_name: String },

    /// `Control::SetTaskPinned`.
    SetTaskPinned { task_id: String, pinned: bool },

    /// `Control::SetTaskBranch`.
    UpdateTaskBranch {
        task_id: String,
        target_project_id: String,
        branch_name: String,
    },

    // ── Section / tab state (the hot path) ──────────────────────
    /// `Control::SetSectionState` for project-page / standalone
    /// shells. Task-bound sections use [`Self::UpdateTaskTabs`].
    SetTerminalSection {
        section_id: String,
        state: PersistedSectionState,
    },

    /// `Control::SetSectionState` for task-bound sections — syncs the
    /// per-task tab list and the matching `terminal_sections` rows.
    UpdateTaskTabs {
        task_id: String,
        state: PersistedSectionState,
    },

    /// GUI tab-close cleanup. Outcome is `changed: bool`.
    RemoveTerminalSections { section_ids: HashSet<String> },

    // ── Persistent UI prefs ─────────────────────────────────────
    /// `Control::SetThemeMode`.
    SetThemeMode { mode: ThemeMode },

    /// `Control::SetExpandedRepos`.
    SetExpandedRepos { repo_ids: HashSet<String> },

    /// `Control::SetSidebarGitMetadataVisible`.
    SetSidebarGitMetadataVisible { visible: bool },

    /// `Control::SetLastActiveSection`.
    SetLastActiveSection { section_id: Option<String> },

    /// GUI sidebar splitter. Currently bypasses `Control::*` —
    /// classified as desktop-only-but-persisted ephemera; see
    /// `daemon-owned-state-authority.md`.
    SetLeftSidebarOpen { is_open: bool },

    // ── Host settings ───────────────────────────────────────────
    SetShortcutBinding {
        action: ShortcutAction,
        binding: String,
    },
    ClearShortcutBinding {
        action: ShortcutAction,
    },
    ResetShortcutBinding {
        action: ShortcutAction,
    },

    SetDefaultAgent {
        agent_id: String,
    },
    SetAgentEnabled {
        agent_id: String,
        enabled: bool,
    },
    SetAgentLaunchArgs {
        agent_id: String,
        args: Vec<String>,
    },

    SetOpenInAppEnabled {
        app: OpenInAppKind,
        enabled: bool,
        available: Vec<OpenInAppKind>,
    },
    SetPreferredOpenInApp {
        app: OpenInAppKind,
        available: Vec<OpenInAppKind>,
    },

    SetGitCommitScript {
        script: String,
    },
    ResetGitCommitScript,
    SetGitPrScript {
        script: String,
    },
    ResetGitPrScript,
    SetGitCommitLlm {
        settings: GitActionLlmSettings,
    },
    SetGitPrLlm {
        settings: GitActionLlmSettings,
    },

    // ── Projection ingestion (client-side authority only) ───────
    /// `WorkerReply::ProjectList` arriving on a paired session.
    /// On the desktop host this is a no-op (the daemon's store
    /// already *is* the desktop's store); on a pure-client binary
    /// it is the only writer. Not persisted by SQLite — it's an
    /// in-memory mirror of the host's projection.
    AbsorbProjection {
        projects: Vec<ProjectSummary>,
        repos: Vec<RepoSummary>,
        ui: UiSnapshot,
    },
}

/// Typed mutation outcomes. Variants only carry data callers
/// actually inspect today — anything else is `Unit`.
///
/// We intentionally don't use `Result<...>` here: validation errors
/// (e.g. `upsert_project_action` returning `Err("Could not find the
/// project group...")`) are bubbled up as
/// [`MutationOutcome::Failed`] so callers can `match` on success/
/// failure without unwinding. Crash-class invariant violations
/// stay `panic!` / `expect` like they are today.
#[derive(Debug, Clone)]
pub enum MutationOutcome {
    /// No data to return; mutation either succeeded or was a
    /// no-op. Disambiguate with [`Self::Changed`] when the caller
    /// needs idempotence info.
    Unit,

    /// `changed: bool`. Used by set/reset variants where the caller
    /// distinguishes "wrote something" from "already had this value".
    Changed(bool),

    /// `Result<bool, String>`-shaped. Used by branch-setting
    /// updates that today return that exact type.
    BranchSettingApplied(Result<bool, String>),

    /// Stale branch references swept out of `branch_settings`.
    InvalidBranchSettings(Vec<InvalidProjectBranchSetting>),

    /// `Option<Task>` returned by `remove_task` (caller uses it to
    /// tear down the worktree directory if any).
    RemovedTask(Option<crate::project_store::Task>),

    /// `Option<(old_section_id, new_section_id)>` returned by
    /// `update_task_branch` for tab-state migration.
    TaskBranchUpdated(Option<(String, String)>),

    /// Validation failure surfaced from a mutation that today
    /// returns `Result<_, String>` (`upsert_project_action`).
    Failed(String),
}

impl MutationOutcome {
    /// Convenience: `true` if the variant carries `changed = true`
    /// or any other "something happened" signal.
    pub fn is_changed(&self) -> bool {
        match self {
            MutationOutcome::Unit => true,
            MutationOutcome::Changed(c) => *c,
            MutationOutcome::BranchSettingApplied(Ok(c)) => *c,
            MutationOutcome::BranchSettingApplied(Err(_)) => false,
            MutationOutcome::InvalidBranchSettings(v) => !v.is_empty(),
            MutationOutcome::RemovedTask(t) => t.is_some(),
            MutationOutcome::TaskBranchUpdated(t) => t.is_some(),
            MutationOutcome::Failed(_) => false,
        }
    }
}

/// Apply one mutation against the store.
///
/// This is a free function rather than a method so callers that
/// already hold `&mut ProjectStore` (e.g. inside `with_store_mut`)
/// don't need to wrap it in a `StateAuthority` value first. When
/// the refactor reaches the point of removing the GPUI app's
/// owned `ProjectStore` field, this will graduate to a method on
/// a `StateAuthority` struct that owns the store + the persistence
/// adapter.
///
/// Persistence: every variant that previously called `self.save()`
/// inside `ProjectStore` continues to do so via the underlying
/// `pub fn`. Variants that historically did *not* call `save()`
/// (`rename_task`, `set_task_pinned`, the raw `*_mut` handle paths)
/// keep their old contract for now — the audit calls these out
/// explicitly. PR2 normalises that: every successful mutation
/// commits exactly one persistence write.
pub fn apply(store: &mut ProjectStore, mutation: Mutation) -> MutationOutcome {
    match mutation {
        // ── Project catalog ─────────────────────────────────────
        Mutation::InsertPreparedProject(prepared) => {
            MutationOutcome::Changed(store.insert_prepared_project(prepared))
        }
        Mutation::RemoveProject { project_id } => {
            store.remove_project(&project_id);
            MutationOutcome::Unit
        }
        Mutation::SetDefaultBranch {
            project_id,
            branch_name,
        } => MutationOutcome::BranchSettingApplied(
            store.update_default_branch(&project_id, branch_name),
        ),
        Mutation::SetDefaultTargetBranch {
            project_id,
            branch_name,
        } => MutationOutcome::BranchSettingApplied(
            store.update_default_target_branch(&project_id, branch_name),
        ),
        Mutation::ClearMissingBranchSettings { project_id } => {
            MutationOutcome::InvalidBranchSettings(store.clear_missing_branch_settings(&project_id))
        }
        Mutation::SetRepoDefaultCommitAction { repo_id, action } => {
            store.set_repo_default_commit_action(repo_id, action);
            MutationOutcome::Unit
        }
        Mutation::UpsertProjectAction {
            project_id,
            action,
            save_global_copy,
        } => match store.upsert_project_action(&project_id, action, save_global_copy) {
            Ok(()) => MutationOutcome::Unit,
            Err(e) => MutationOutcome::Failed(e),
        },
        Mutation::DeleteProjectAction {
            project_id,
            action_id,
        } => MutationOutcome::Changed(store.delete_project_action(&project_id, &action_id)),
        Mutation::UpdateWorktreeCheckout {
            worktree_id,
            checkout,
        } => MutationOutcome::Changed(store.update_worktree_checkout(&worktree_id, checkout)),

        // ── Task catalog ────────────────────────────────────────
        Mutation::InsertTask { task } => {
            store.insert_task(task);
            MutationOutcome::Unit
        }
        Mutation::RemoveTask {
            root_project_id,
            task_id,
        } => MutationOutcome::RemovedTask(store.remove_task(&root_project_id, &task_id)),
        Mutation::RenameTask { task_id, new_name } => {
            // `rename_task` does not call `save()` itself today —
            // every external caller follows it with `save()`. The
            // authority normalises that: rename + save in one step.
            let changed = store.rename_task(&task_id, &new_name);
            if changed {
                store.save();
            }
            MutationOutcome::Changed(changed)
        }
        Mutation::SetTaskPinned { task_id, pinned } => {
            // Same as RenameTask: `set_task_pinned` doesn't save
            // on its own. Normalise here.
            let changed = store.set_task_pinned(&task_id, pinned);
            if changed {
                store.save();
            }
            MutationOutcome::Changed(changed)
        }
        Mutation::UpdateTaskBranch {
            task_id,
            target_project_id,
            branch_name,
        } => MutationOutcome::TaskBranchUpdated(store.update_task_branch(
            &task_id,
            &target_project_id,
            &branch_name,
        )),

        // ── Section / tab state ─────────────────────────────────
        Mutation::SetTerminalSection { section_id, state } => {
            store.set_terminal_section(section_id, state);
            MutationOutcome::Unit
        }
        Mutation::UpdateTaskTabs { task_id, state } => {
            store.update_task_tabs(&task_id, &state);
            MutationOutcome::Unit
        }
        Mutation::RemoveTerminalSections { section_ids } => {
            MutationOutcome::Changed(store.remove_terminal_sections(&section_ids))
        }

        // ── Persistent UI prefs ─────────────────────────────────
        Mutation::SetThemeMode { mode } => {
            store.set_theme_mode(mode);
            MutationOutcome::Unit
        }
        Mutation::SetExpandedRepos { repo_ids } => {
            store.set_expanded_repos(&repo_ids);
            MutationOutcome::Unit
        }
        Mutation::SetSidebarGitMetadataVisible { visible } => {
            store.set_sidebar_git_metadata_visible(visible);
            MutationOutcome::Unit
        }
        Mutation::SetLastActiveSection { section_id } => {
            store.set_last_active_section_key(section_id);
            MutationOutcome::Unit
        }
        Mutation::SetLeftSidebarOpen { is_open } => {
            store.set_left_sidebar_open(is_open);
            MutationOutcome::Unit
        }

        // ── Host settings ───────────────────────────────────────
        Mutation::SetShortcutBinding { action, binding } => {
            store.set_shortcut_binding(action, binding);
            MutationOutcome::Unit
        }
        Mutation::ClearShortcutBinding { action } => {
            store.clear_shortcut_binding(action);
            MutationOutcome::Unit
        }
        Mutation::ResetShortcutBinding { action } => {
            store.reset_shortcut_binding(action);
            MutationOutcome::Unit
        }

        Mutation::SetDefaultAgent { agent_id } => {
            MutationOutcome::Changed(store.set_default_agent(&agent_id))
        }
        Mutation::SetAgentEnabled { agent_id, enabled } => {
            MutationOutcome::Changed(store.set_agent_enabled(&agent_id, enabled))
        }
        Mutation::SetAgentLaunchArgs { agent_id, args } => {
            MutationOutcome::Changed(store.set_agent_launch_args(agent_id, args))
        }

        Mutation::SetOpenInAppEnabled {
            app,
            enabled,
            available,
        } => {
            store.set_open_in_app_enabled(app, enabled, &available);
            MutationOutcome::Unit
        }
        Mutation::SetPreferredOpenInApp { app, available } => {
            store.set_preferred_open_in_app(app, &available);
            MutationOutcome::Unit
        }

        Mutation::SetGitCommitScript { script } => {
            MutationOutcome::Changed(store.set_git_commit_generation_script(script))
        }
        Mutation::ResetGitCommitScript => {
            MutationOutcome::Changed(store.reset_git_commit_generation_script())
        }
        Mutation::SetGitPrScript { script } => {
            MutationOutcome::Changed(store.set_git_pr_generation_script(script))
        }
        Mutation::ResetGitPrScript => {
            MutationOutcome::Changed(store.reset_git_pr_generation_script())
        }
        Mutation::SetGitCommitLlm { settings } => {
            MutationOutcome::Changed(store.set_git_commit_generation_llm(settings))
        }
        Mutation::SetGitPrLlm { settings } => {
            MutationOutcome::Changed(store.set_git_pr_generation_llm(settings))
        }

        // ── Projection ingestion ────────────────────────────────
        Mutation::AbsorbProjection {
            projects,
            repos,
            ui,
        } => {
            store.absorb_projection(projects, repos, ui);
            MutationOutcome::Unit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_store::ProjectStore;

    /// Sanity check: the dispatcher routes a representative variant
    /// to the underlying mutator and the outcome matches. Heavier
    /// behaviour tests live next to `ProjectStore`'s own methods —
    /// the authority is a thin shim, so over-testing it just
    /// duplicates `project_store::tests` for no extra coverage.
    #[test]
    fn set_theme_mode_routes_through_apply() {
        let mut store = ProjectStore::from_projects_for_test(Vec::new(), Vec::new());
        let outcome = apply(
            &mut store,
            Mutation::SetThemeMode {
                mode: ThemeMode::Dark,
            },
        );
        assert!(matches!(outcome, MutationOutcome::Unit));
        assert_eq!(store.ui.theme_mode, ThemeMode::Dark);
    }

    #[test]
    fn rename_task_no_op_returns_unchanged() {
        let mut store = ProjectStore::from_projects_for_test(Vec::new(), Vec::new());
        let outcome = apply(
            &mut store,
            Mutation::RenameTask {
                task_id: "does-not-exist".to_string(),
                new_name: "anything".to_string(),
            },
        );
        assert!(matches!(outcome, MutationOutcome::Changed(false)));
    }
}
