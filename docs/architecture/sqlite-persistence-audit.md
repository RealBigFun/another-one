# SQLite persistence audit — `ProjectStore` mutators

> Pre-PR audit for the [sqlite-persistence](sqlite-persistence.md) refactor.
> Goal: enumerate every external caller that mutates
> `core::project_store::ProjectStore` so we can replace them with a
> typed `Mutation` enum applied via `StateAuthority::apply`.

Scope of search: `app/`, `daemon/`, `daemon-client/`, `mcp-shim/`,
`core/`. `daemon-client/`, `mcp-shim/`, `daemon-proto/`,
`daemon-transport/`, and the `daemon/` crate itself contain **zero**
direct mutation of `ProjectStore` — every external mutation lives in
the desktop binary (`app/`).

Total external mutation call sites: **~50**, concentrated in two
files: `app/src/daemon_host.rs` (the `DesktopTerminalRegistry` impl,
already routes through `with_store_mut`) and `app/src/app.rs` (the
GPUI app — the legacy direct `self.project_store.<mutator>()` paths
that still rely on `commit_local_mutation`).

---

## 1. Public API surface of `ProjectStore`

### Public fields (`core/src/project_store.rs:1525`)

| Field | Type | What it represents |
|---|---|---|
| `repos` | `HashMap<String, RepoRecord>` | Repo catalog (one per common-dir) |
| `projects` | `Vec<Project>` | Runtime view: ordered, joined w/ tasks |
| `project_order` | `Vec<String>` | Canonical project ordering |
| `tasks` | `HashMap<String, Vec<Task>>` | Runtime view keyed by root project id |
| `task_ids_by_root_project` | `HashMap<String, Vec<String>>` | Canonical task ordering |
| `terminal_sections` | `HashMap<String, PersistedSectionState>` | Project-page / standalone shell sections (hot path) |
| `ui` | `UiState` | All UI/host settings (theme, expanded, shortcuts, agents, open-in, git-actions, pinned, …) |

Private (canonical) fields: `projects_by_id`, `tasks_by_id`,
`file_path`.

### `&mut self` public methods (grouped roughly by domain)

```
// ── Project catalog ─────────────────────────────────────────────
project_mut                      L1681   raw mutable handle
repo_mut                         L1689   raw mutable handle
task_mut                         L1702   raw mutable handle (no external callers)
remove_project                   L2111   archive root project + drop tasks
remove_worktree_project          L2140   no external callers
insert_prepared_project          L2377   add or unarchive project + repo
update_default_branch            L1865   branch_settings.default_branch
update_default_target_branch     L1877   branch_settings.default_target_branch
clear_missing_branch_settings    L1889   clean stale branch refs after refresh
upsert_project_action            L2027   project-scoped action
upsert_global_action             L2061   no external callers
delete_project_action            L2067
set_repo_default_commit_action   L2096   ui.repo_default_commit_actions[id]

// ── Task catalog ────────────────────────────────────────────────
insert_task                      L2266
remove_task                      L2237
update_task_branch               L2275   target_project_id + branch_name
update_worktree_checkout         L2304   embedded checkout block on Task
rename_task                      L2357
set_task_pinned                  L2369   ui.pinned_task_ids

// ── Section / tab state ─────────────────────────────────────────
set_section_state                L2776   no external callers (daemon_host calls the two below instead)
set_terminal_section             L3384   terminal_sections[key] = state  (project-page sections)
update_task_tabs                 L3400   per-task tabs, syncs into terminal_sections
remove_sections                  L3060   no external callers
remove_terminal_sections         L3392   bulk delete by section_id set
set_tab_session                  L3417   no external callers
set_tab_restore_status           L3440   no external callers
find_task_mut                    L3396   no external callers

// ── UI / persistent prefs ───────────────────────────────────────
set_left_sidebar_open            L2737   ui.left_sidebar_open
set_sidebar_git_metadata_visible L2745
set_theme_mode                   L2753
set_expanded_repos               L2762
set_expanded_projects            L3376   legacy alias, no external callers
set_last_active_section_id       L2767   no external callers
set_last_active_section_key      L3380   legacy alias (used by daemon_host)

// ── Host settings ───────────────────────────────────────────────
set_shortcut_binding             L2786
clear_shortcut_binding           L3045
reset_shortcut_binding           L3050
reset_shortcuts                  L3055   no external callers (settings_page loops bindings)
set_default_agent                L2955
set_agent_enabled                L2969
set_agent_launch_args            L2799
remove_agent_launch_args         L2818   no external callers (set_agent_launch_args delegates)
set_open_in_app_enabled          L3009
set_preferred_open_in_app        L3036
set_git_commit_generation_script L2833
reset_git_commit_generation_script L2852
set_git_pr_generation_script     L2868
reset_git_pr_generation_script   L2886
set_git_commit_generation_llm    L2899
set_git_pr_generation_llm        L2912

// ── Projection ingestion ────────────────────────────────────────
absorb_projection                L2619   merges remote ProjectList into local store
absorb_ui_snapshot               L2513   merges remote UiSnapshot
set_remote_snapshot              L2588   no external callers (used internally by absorb_projection)

// ── Persistence / projection internals ──────────────────────────
save                             L2452   coalesced JSON write
rebuild_runtime_views            L3252   internal projection rebuild
refresh_runtime_views            L3300   subset of rebuild
load                             L1612   constructor
```

Methods marked **no external callers** are private-by-convention
already and can stay internal to the future `StateAuthority` without
any caller migration.

---

## 2. External callers grouped by mutation domain

### 2.1 Project catalog

| Site | Domain method | Trigger |
|---|---|---|
| `app/src/daemon_host.rs:626` `store.remove_project(&project_id)` (inside `DesktopTerminalRegistry::remove_project`, via `with_store_mut`) | `remove_project` | `Control::DeleteProject` |
| `app/src/daemon_host.rs:678` `store.update_default_branch(...)` (in `set_branch_setting`, via `with_store_mut`) | `update_default_branch` | `Control::SetBranchSetting{field:"default-branch"}` |
| `app/src/daemon_host.rs:685` `store.update_default_target_branch(...)` | `update_default_target_branch` | `Control::SetBranchSetting{field:"default-target-branch"}` |
| `app/src/daemon_host.rs:754` `store.set_repo_default_commit_action(...)` | `set_repo_default_commit_action` | `Control::SetRepoDefaultCommitAction` |
| `app/src/app.rs:10258` `self.project_store.insert_prepared_project(prepared.clone())` (`finish_worktree_branch_creation`) — pub field path, followed by `commit_local_mutation()` | `insert_prepared_project` | GUI: create-branch-modal completion |
| `app/src/app.rs:11370` `self.project_store.insert_prepared_project(project.clone())` (drain of pending project-add reply) | `insert_prepared_project` | GUI: "Add Project…" reply |
| `app/src/app.rs:9495,9551` `self.project_store.repo_mut(&repo_id) → repo.{branch_order, branches_by_name, common_dir, branches_by_name[…]ahead/behind} = …` (`apply_project_git_state`) | direct `pub` field via `repo_mut` | git-status refresh tick (poll loop) |
| `app/src/app.rs:9510,9542` `self.project_store.project_mut(project_id) → project.{kind, checkout, checkout.current_branch, …} = …` (`apply_project_git_state`) | direct `pub` field via `project_mut` | git-status refresh tick |
| `app/src/app.rs:9531` `self.project_store.update_worktree_checkout(project_id, …)` (`apply_project_git_state`) | `update_worktree_checkout` | git-status refresh tick |
| `app/src/app.rs:9583` `self.project_store.refresh_runtime_views()` (after the above) | projection rebuild | git-status refresh tick |
| `app/src/app.rs:9636,9696,10991` `self.project_store.clear_missing_branch_settings(&project_id)` (post git refresh) | `clear_missing_branch_settings` | git refresh / toolbar git action completion |
| `app/src/custom_actions_modal.rs:1720` `this.project_store.delete_project_action(&project_id, &action_id)` | `delete_project_action` | GUI: custom-action modal "Delete" |
| `app/src/custom_actions_modal.rs:2074` `self.project_store.upsert_project_action(&project_id, action, save_global_copy)` | `upsert_project_action` | GUI: custom-action modal "Save" |

Notes:
- Every project-catalog site in `app.rs` (rows 5–11 above) is a
  *legacy direct mutation* not yet routed through a `Control::*` verb.
  These are the call sites that currently rely on
  `self.commit_local_mutation()` / `sync_registry_project_store()`.
- `apply_project_git_state` (app/src/app.rs:9477-9590) is the single
  largest direct-field caller: 4 mutable handle calls + 1 method +
  1 manual `refresh_runtime_views()` per project per git-poll tick.
  See "Risks" below.

### 2.2 Task catalog

| Site | Domain method | Trigger |
|---|---|---|
| `app/src/daemon_host.rs:636` `state.project_store.rename_task(...)` (`rename_task`, **bypasses `with_store_mut`** — calls `save()` + `notify_state_changed()` manually) | `rename_task` | `Control::SetTaskName` |
| `app/src/daemon_host.rs:649` `state.project_store.set_task_pinned(...)` (same pattern as above) | `set_task_pinned` | `Control::SetTaskPinned` |
| `app/src/daemon_host.rs:662` `store.remove_task(&project_id, &task_id)` | `remove_task` | `Control::DeleteTask` |
| `app/src/daemon_host.rs:763` `store.update_task_branch(...)` | `update_task_branch` | `Control::SetTaskBranch` |
| `app/src/app.rs:10016` `self.project_store.insert_task(Task { … })` (`insert_and_open_task`) | `insert_task` | GUI / MCP / mobile via `client_open_task` |
| `app/src/app.rs:11206` `self.project_store.insert_task(Task { … })` (worktree-task creation reply) | `insert_task` | `Control::CreateWorktreeTask` reply |

### 2.3 Section / tab state (the hot path)

| Site | Domain method | Trigger |
|---|---|---|
| `app/src/daemon_host.rs:707` `store.update_task_tabs(task_id, &persisted)` (in `set_section_state`) | `update_task_tabs` | `Control::SetSectionState` (task-bound) |
| `app/src/daemon_host.rs:709` `store.set_terminal_section(section_id, persisted)` | `set_terminal_section` | `Control::SetSectionState` (project-page) |
| `app/src/daemon_host.rs:716` `store.set_last_active_section_key(section_id)` | `set_last_active_section_key` | `Control::SetLastActiveSection` |
| `app/src/app.rs:13429` `project_store.update_task_tabs(task_id, &persisted)` (helper `apply_persisted_section_state_to_project_store`) | `update_task_tabs` | called from `persist_section_state` (app.rs:5148) **and** `sync_workspace_section_to_project_store` (app.rs:6904) |
| `app/src/app.rs:13431` `project_store.set_terminal_section(section_id.store_key(), persisted)` (same helper) | `set_terminal_section` | same |
| `app/src/app.rs:9031` `self.project_store.remove_terminal_sections(&bare_section_keys)` (`remove_persisted_sections`) | `remove_terminal_sections` | GUI: tab-close cleanup |
| `app/src/app.rs:5198` `state.project_store.terminal_sections.get_mut(&section_key) → tab.title = …` (`apply_pty_title_update`) — **direct `pub` field mutation** | `pub terminal_sections` | every PTY title escape (OSC 0/1/2) |
| `app/src/app.rs:5223` `state.project_store.rebuild_runtime_views()` (after the above) | projection rebuild | every PTY title change |

### 2.4 UI / persistent prefs

| Site | Domain method | Trigger |
|---|---|---|
| `app/src/daemon_host.rs:722` `store.set_sidebar_git_metadata_visible(visible)` | `set_sidebar_git_metadata_visible` | `Control::SetSidebarGitMetadataVisible` |
| `app/src/daemon_host.rs:737` `store.set_theme_mode(mode)` | `set_theme_mode` | `Control::SetThemeMode` |
| `app/src/daemon_host.rs:770` `store.set_expanded_repos(&set)` | `set_expanded_repos` | `Control::SetExpandedRepos` |
| `app/src/app.rs:11938,11967,12117` `self.project_store.set_left_sidebar_open(...)` (sidebar-resize animation) — **direct mutation, no Control verb** | `set_left_sidebar_open` | GUI: drag/click sidebar splitter |
| `app/src/app.rs:16624` `self.project_store.absorb_projection(summaries, repo_summaries, ui)` (`drain_remote_worker_replies`) | `absorb_projection` | every `WorkerReply::ProjectList` push from a paired daemon |

### 2.5 Host settings (shortcuts, agents, open-in, git-actions)

All of these flow through `with_store_mut` and a `Control::*` verb;
they are the easiest domain to convert.

| Site | Domain method | Trigger |
|---|---|---|
| `daemon_host.rs:1202` `store.set_agent_enabled(agent_id, enabled)` | `set_agent_enabled` | `Control::SetAgentEnabled` |
| `daemon_host.rs:1208` `store.set_default_agent(agent_id)` | `set_default_agent` | `Control::SetDefaultAgent` |
| `daemon_host.rs:1214` `store.set_agent_launch_args(agent_id, args)` | `set_agent_launch_args` | `Control::SetAgentLaunchArgs` |
| `daemon_host.rs:1240` `store.set_open_in_app_enabled(app, enabled, &available)` | `set_open_in_app_enabled` | `Control::SetOpenInAppEnabled` |
| `daemon_host.rs:1257` `store.set_preferred_open_in_app(app, &available)` | `set_preferred_open_in_app` | `Control::OpenProjectInApp` |
| `daemon_host.rs:1291` `store.set_git_commit_generation_script(script)` | `set_git_commit_generation_script` | `Control::SetGitCommitScript` |
| `daemon_host.rs:1296` `store.reset_git_commit_generation_script()` | `reset_git_commit_generation_script` | `Control::ResetGitCommitScript` |
| `daemon_host.rs:1301` `store.set_git_pr_generation_script(script)` | `set_git_pr_generation_script` | `Control::SetGitPrScript` |
| `daemon_host.rs:1306` `store.reset_git_pr_generation_script()` | `reset_git_pr_generation_script` | `Control::ResetGitPrScript` |
| `daemon_host.rs:782` `store.set_git_commit_generation_llm(settings)` | `set_git_commit_generation_llm` | `Control::SetGitCommitLlm` |
| `daemon_host.rs:794` `store.set_git_pr_generation_llm(settings)` | `set_git_pr_generation_llm` | `Control::SetGitPrLlm` |
| `daemon_host.rs:1337` `store.clear_shortcut_binding(action)` | `clear_shortcut_binding` | `Control::SetShortcutBinding{binding=""}` |
| `daemon_host.rs:1339` `store.set_shortcut_binding(action, binding)` | `set_shortcut_binding` | `Control::SetShortcutBinding` |
| `daemon_host.rs:1348` `store.reset_shortcut_binding(action)` | `reset_shortcut_binding` | `Control::ResetShortcutBinding` |

### 2.6 Whole-store assignment

| Site | What | Trigger |
|---|---|---|
| `app/src/app.rs:6900` `state.project_store = self.project_store.clone()` (`sync_registry_project_store`) | clones the entire store from the GPUI app's copy into `RegistryState` after a direct GUI mutation | every `commit_local_mutation()` |

This is **the** seam the refactor eliminates. After PR1 there is one
copy of the store, owned by `StateAuthority`, and the GPUI app reads
from a projection.

### 2.7 Domains with zero external callers

These mutators are only invoked from inside `core/src/project_store.rs`
itself and can stay private under `StateAuthority`:

- `set_section_state`, `set_last_active_section_id`,
  `set_expanded_projects` (legacy aliases, daemon_host calls the
  current-named ones).
- `upsert_global_action`, `remove_worktree_project`,
  `remove_agent_launch_args`, `reset_shortcuts`.
- `set_tab_session`, `set_tab_restore_status`, `find_task_mut`,
  `task_mut`.
- `set_remote_snapshot` (only called from inside `absorb_projection`).

---

## 3. Draft `Mutation` enum

Variant set is derived strictly from the call-site evidence above —
no speculative variants.

```rust
pub enum Mutation {
    // ── Project catalog ─────────────────────────────────────────
    /// `Control::CreateProject` reply / GUI "Add Project" /
    /// worktree-branch creation.
    InsertPreparedProject { prepared: PreparedProject },

    /// `Control::DeleteProject`.
    RemoveProject { project_id: String },

    /// `Control::SetBranchSetting{field:"default-branch"}`.
    SetDefaultBranch { project_id: String, branch_name: Option<String> },

    /// `Control::SetBranchSetting{field:"default-target-branch"}`.
    SetDefaultTargetBranch { project_id: String, branch_name: Option<String> },

    /// Post git-refresh sweep. Returns `Vec<InvalidBranchSetting>`
    /// today; outcome must carry that.
    ClearMissingBranchSettings { project_id: String },

    /// `Control::SetRepoDefaultCommitAction`.
    SetRepoDefaultCommitAction { repo_id: String, action: RepoDefaultCommitAction },

    /// Compound git-refresh write (see §4 — currently 4 raw `*_mut`
    /// handles + `update_worktree_checkout` + manual
    /// `refresh_runtime_views`). Modeled as one mutation so all the
    /// scattered field assignments commit atomically.
    ApplyProjectGitState {
        project_id: String,
        state: ProjectGitState, // ahead/behind, current_branch, repo metadata, checkout
    },

    /// `Control::SetProjectAction`.
    UpsertProjectAction {
        project_id: String,
        action: ProjectAction,
        save_global_copy: bool,
    },

    /// `Control::DeleteProjectAction`.
    DeleteProjectAction { project_id: String, action_id: String },

    // ── Task catalog ────────────────────────────────────────────
    /// `client_open_task` + `Control::CreateWorktreeTask` reply path.
    InsertTask { task: Task },

    /// `Control::DeleteTask`.
    RemoveTask { root_project_id: String, task_id: String },

    /// `Control::SetTaskName`. Outcome: (changed, Option<TaskSummary>).
    RenameTask { task_id: String, new_name: String },

    /// `Control::SetTaskPinned`. Outcome: (changed, Option<TaskSummary>).
    SetTaskPinned { task_id: String, pinned: bool },

    /// `Control::SetTaskBranch`.
    UpdateTaskBranch {
        task_id: String,
        target_project_id: String,
        branch_name: String,
    },

    // ── Section / tab state (hot path) ──────────────────────────
    /// `Control::SetSectionState` for task-bound sections (the daemon-
    /// host helper dispatches this when `section_id.task_id` is set).
    SetTaskSectionState { task_id: String, persisted: PersistedSectionState },

    /// `Control::SetSectionState` for project-page / standalone shells.
    SetTerminalSection { section_id: String, persisted: PersistedSectionState },

    /// GUI tab-close cleanup.
    RemoveTerminalSections { section_ids: HashSet<String> },

    /// PTY-driven OSC-0/1/2 title update. Currently a raw
    /// `terminal_sections.get_mut(...).tab.title = …` (app.rs:5198).
    /// CAS-style: set only if `fixed_title.is_none()` and the title
    /// differs.
    ApplyPtyTabTitle {
        section_key: String,
        tab_id: String,
        kind: PtyTitleUpdate, // Reset { fallback } | Set { title }
    },

    // ── Persistent UI prefs ─────────────────────────────────────
    /// `Control::SetThemeMode`.
    SetThemeMode { mode: ThemeMode },

    /// `Control::SetExpandedRepos`.
    SetExpandedRepos { repo_ids: HashSet<String> },

    /// `Control::SetSidebarGitMetadataVisible`.
    SetSidebarGitMetadataVisible { visible: bool },

    /// `Control::SetLastActiveSection`.
    SetLastActiveSection { section_id: Option<String> },

    /// GUI sidebar splitter — currently bypasses Control. Either add a
    /// verb or classify as desktop-only ephemera per
    /// daemon-owned-state-authority.md (it *is* persisted today).
    SetLeftSidebarOpen { is_open: bool },

    // ── Host settings ───────────────────────────────────────────
    SetShortcutBinding   { action: ShortcutAction, binding: String },
    ClearShortcutBinding { action: ShortcutAction },
    ResetShortcutBinding { action: ShortcutAction },

    SetDefaultAgent     { agent_id: String },
    SetAgentEnabled     { agent_id: String, enabled: bool },
    SetAgentLaunchArgs  { agent_id: String, args: Vec<String> },

    SetOpenInAppEnabled { app: OpenInAppKind, enabled: bool, available: Vec<OpenInAppKind> },
    SetPreferredOpenInApp { app: OpenInAppKind, available: Vec<OpenInAppKind> },

    SetGitCommitScript   { script: String },
    ResetGitCommitScript,
    SetGitPrScript       { script: String },
    ResetGitPrScript,
    SetGitCommitLlm      { settings: GitActionLlmSettings },
    SetGitPrLlm          { settings: GitActionLlmSettings },

    // ── Projection ingestion (client side) ──────────────────────
    /// `WorkerReply::ProjectList` arriving on a paired session.
    /// Lives in the same enum but applies to the *client-side*
    /// authority — see §4 risks.
    AbsorbProjection {
        projects: Vec<ProjectSummary>,
        repos: Vec<RepoSummary>,
        ui: UiSnapshot,
    },
}
```

`Mutation` outcomes (a sibling enum) need to carry at minimum:
`changed: bool` for idempotent set/reset variants;
`Option<TaskSummary>` for `RenameTask` / `SetTaskPinned`;
`Vec<InvalidBranchSetting>` for `ClearMissingBranchSettings`.

---

## 4. Risks / surprises

### 4.1 Compound mutation: `apply_project_git_state` (app/src/app.rs:9477)

Single git-refresh tick mutates **5 logical things atomically**:

```text
repo_mut → branch_order, branches_by_name, common_dir   // L9495–9508
project_mut → kind, checkout                            // L9510–9521
update_worktree_checkout(...)                            // L9531
project_mut → checkout.current_branch                   // L9542
repo_mut → branches_by_name[branch].ahead/behind        // L9551–9572
+ refresh_runtime_views()                                // L9583
```

This cannot decompose into 5 `Mutation`s without exposing
intermediate inconsistent states (e.g. `branch_order` updated but
`branches_by_name` not, projection rebuilt mid-way). Model as
**one** `ApplyProjectGitState` variant carrying the whole
`ProjectGitState` and let the authority do the field-level diff
internally. This is also a CAS site: every assignment is gated on
`old != new`.

### 4.2 PTY title hot path (app/src/app.rs:5172 `apply_pty_title_update`)

Reads `terminal_sections[key].tabs[id].fixed_title` then conditionally
mutates `.title`. Runs on every OSC 0/1/2 escape, which under a CDP /
sub-agent burst can be hundreds per second. Cannot be a blind
`SetTabTitle` — it has to honour `fixed_title` and skip when the
title hasn't actually changed. Model as `ApplyPtyTabTitle` (CAS
inside the authority) and make sure the SQLite path is *one
single-row UPDATE* (the `sections` table per the schema sketch).
This is the single biggest perf motivator for the whole refactor.

### 4.3 `persist_section_state` (app/src/app.rs:5148)

Called every time the GPUI workspace mutates section state (cursor,
scroll, viewport, tab list). Already routes through
`Control::SetSectionState` — but **also** mutates the local
`self.project_store` synchronously *before* dispatching, via
`apply_persisted_section_state_to_project_store` (app.rs:13422).
After the refactor the local mutation goes away (the GPUI app reads
from a projection); it only needs to keep the optimistic-render copy
the workspace pane uses for layout.

### 4.4 `rename_task` / `set_task_pinned` bypass `with_store_mut`

`daemon_host.rs:632–657` calls `state.project_store.rename_task(...)`
and `state.project_store.save()` + `notify_state_changed()` manually
(rather than `with_store_mut`) because the helper needs a
`TaskSummary` that requires `&state` *after* the mutation. Mutation
outcome on the new authority needs to carry that summary so we can
drop the bespoke path.

### 4.5 Cloning the whole store: `sync_registry_project_store`

`app/src/app.rs:6900` does `state.project_store = self.project_store.clone()`
after every direct GUI mutation. This is roughly 14 call sites of
`commit_local_mutation` / `sync_registry_project_store` chained
through app.rs. After the refactor:
- The GPUI `AnotherOneApp.project_store` field disappears (or
  becomes a read-only projection cached for render).
- `commit_local_mutation` and `sync_registry_project_store` are
  deleted.
- All 14 call sites become `Mutation::* + apply()`.

This is the single biggest reviewer-visible change in the diff.

### 4.6 `absorb_projection` blurs roles

On the desktop host `absorb_projection` is a no-op (the daemon's
store *is* the desktop's store). On a pure-client binary it's the
only writer. Per `daemon-owned-state-authority.md` this should
become *client-side projection application*, not a `Mutation` on
the authority. Either:
- keep it on `ProjectStore` as a separate "client read-model
  applicator" surface and leave `Mutation` host-only, or
- include it but tag the variant client-only and route it through
  a different `apply()` entry point.

Either way it is **not** persisted by SQLite — it's an in-memory
mirror.

### 4.7 Multi-thread exposure

`registry_state: Arc<Mutex<RegistryState>>` is shared between:
- the GPUI render thread (reads in
  `app/src/{left_sidebar,right_sidebar,settings_page,…}.rs`),
- the daemon dispatch tokio runtime (reads + mutates via
  `with_store_mut`),
- the iroh server task (reads via `list_projects`/`ui_snapshot`),
- the PTY title applier (mutates `terminal_sections` directly,
  app.rs:5197 with a held `state` lock).

After the refactor the GPUI reads must come from a lock-free
projection (already structured this way for `ProjectSummary` /
`UiSnapshot` over the wire) so the authority's mutation lock is not
contended on render. The PTY title path moves into
`Mutation::ApplyPtyTabTitle` and goes through the same lock as
every other mutation.

### 4.8 `Drop for ProjectStore` calls `flush_pending_save()`

A pending blob-write must drain on shutdown (project_store.rs:1539).
The new SQLite adapter doesn't need this — every mutation is durable
already. Keep the `Drop` impl as a safety net during PR1 (when JSON
adapter still exists), drop it in PR2.

---

## 5. Estimated PR1 scope

**Files changed: ~10**

| File | Approx lines touched | Why |
|---|---|---|
| `core/src/project_store.rs` | -200 / +800 | New `Mutation` + `MutationOutcome` enums; visibility flips on existing mutators; new `StateAuthority`; persistence trait widening to per-mutation `apply()`; existing pub fields → `pub(crate)` |
| `app/src/app.rs` | ~14 call-site rewrites | Every `commit_local_mutation` / `sync_registry_project_store` / direct `project_store.foo()` becomes `authority.apply(Mutation::Foo { … })` |
| `app/src/daemon_host.rs` | ~25 call-site rewrites | Every `with_store_mut(|store| store.foo(...))` becomes `authority.apply(Mutation::Foo { … })`; `commit_project_store_mutation` deleted |
| `app/src/custom_actions_modal.rs` | 2 call sites | Upsert/delete project action |
| `app/tests/daemon_dispatch_harness.rs` | ~30 lines | Test harness now seeds via authority instead of `from_projects_for_test` |
| `daemon/src/registry.rs` | ~5 lines | Trait surface unchanged; default-impl bodies referencing `ProjectStore::*` go away |
| `app/src/settings_page.rs` | 0 (already routes through Control) | — |
| `app/src/left_sidebar.rs` | 0 (already routes through Control) | — |
| `core/src/project_store.rs` tests | minor | Tests asserting on private fields keep working; tests asserting on `pub` fields move to projection reads |

**External call sites converted: ~50**, broken down:
- Project catalog: 13
- Task catalog: 6
- Section/tab state: 8 (incl. the 2 hot paths)
- UI prefs: 5
- Host settings: 14
- Whole-store clone seam: 1 (deleted)
- Projection ingestion: 1 (special-cased)

**Largest single-file impact: `core/src/project_store.rs`** (the
authority, mutation enum, and persistence-trait reshape all land
here). `app/src/daemon_host.rs` is second by call-site count.
`app/src/app.rs` is the trickiest because the conversion is
intertwined with deleting `self.project_store` as an owned field on
`AnotherOneApp` — this might warrant a sub-PR or a feature flag.

PR1 is large but mechanical (per the design doc's framing). The two
non-mechanical pieces are:
1. `apply_project_git_state` → `ApplyProjectGitState` mutation
   (must preserve the cross-field atomicity).
2. The PTY title hot path (`apply_pty_title_update` →
   `ApplyPtyTabTitle`) — this is what unlocks PR2's SQLite win, so
   it must land cleanly in PR1.
