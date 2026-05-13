# Daemon-owned state authority

> Durable app state belongs to the daemon. Desktop, mobile, and MCP submit mutations; they do not each own a private `ProjectStore` truth.

#architecture · #state-sync · #daemon

## Decision

Introduce a daemon-side state authority as the only commit point for durable app state. The first implementation can wrap today's `ProjectStore`, but callers should target an explicit mutation API instead of mutating the store and remembering to `save()` / broadcast.

```text
client UI / MCP / mobile
        │
        ▼
daemon_proto::Control mutation
        │
        ▼
StateAuthority::apply(mutation)
        │
        ├─ mutate canonical state
        ├─ rebuild read models / projections
        ├─ persist through adapter
        └─ broadcast state-changed tick
        │
        ▼
WorkerReply ack + ProjectList / UiSnapshot projections
```

Desktop remains special only because it embeds the daemon in-process. Its UI may keep local view state and a projected copy for rendering, but persistent changes still flow through the same Control → authority path used by mobile and MCP.

## Interface shape

The authority owns four concepts:

### Mutations

A closed enum of durable commands, grouped by domain:

- Project catalog: add prepared project, remove root project, remove worktree project, update branch settings, update repo metadata.
- Task catalog: create task, create worktree task, rename task, pin task, remove task, update task branch.
- Section/tab state: set section state, create tab, select active tab, delete tab, set tab pinned, update tab title/restore status/session metadata.
- Persistent UI settings: theme preference, expanded repos/projects, sidebar git metadata visibility, last active section, repo default commit action.
- Host settings: shortcuts, agent enabled/default/launch args, Open In enabled/preferred app, git action scripts and LLM settings, custom actions.

Each mutation returns a typed outcome that can be converted into the existing `WorkerReply` ack variants. Idempotent commands return `changed: bool` where clients need to distinguish no-op from change.

### Projections

The authority exposes read models instead of public mutable fields:

- `project_list()` → `Vec<ProjectSummary>` plus `Vec<RepoSummary>`.
- `ui_snapshot()` → `UiSnapshot`.
- targeted reads such as branch settings, project actions, agent settings, Open In settings.
- desktop-only render helpers may live beside the app, but they consume projections or read-only snapshots.

Projection construction must be one path shared by initial load, in-process daemon replies, iroh replies, and tests. `ProjectStore::{projects,tasks,project_order,task_ids_by_root_project}` are treated as read models, not authority.

### Persistence

The authority depends on a persistence adapter, not on `projects.json` directly:

```rust
trait AppStatePersistence {
    fn load(&self) -> Result<CanonicalAppState, LoadError>;
    fn save(&self, state: &CanonicalAppState) -> Result<(), SaveError>;
}
```

The first adapter preserves today's JSON format, migrations, backups, and coalesced writer behavior. Tests use an in-memory adapter so mutation tests do not touch the user's config directory.

### Broadcast effects

Every successful durable mutation has one commit point:

1. validate and mutate canonical state;
2. rebuild affected projections;
3. persist via adapter;
4. publish exactly one state-change notification;
5. return the mutation outcome / projection ack.

This replaces scattered `ProjectStore::save()`, `AnotherOneApp::sync_registry_project_store()`, and manual `RegistryState::notify_state_changed()` calls.

## Existing direct mutation paths

### Daemon-owned state

These must move behind the authority seam (most already have `Control` verbs, but still call `ProjectStore` directly):

- `ProjectStore::insert_prepared_project`, `remove_project`, `remove_worktree_project`.
- `insert_task`, `remove_task`, `rename_task`, `set_task_pinned`, `update_task_branch`.
- `set_section_state`, `set_terminal_section`, `update_task_tabs`, `set_tab_session`, `set_tab_restore_status`, `remove_sections`, `remove_terminal_sections`.
- `set_expanded_repos` / legacy `set_expanded_projects`, `set_last_active_section_id` / legacy `set_last_active_section_key`, `set_sidebar_git_metadata_visible`, `set_theme_mode`, `set_repo_default_commit_action`.
- branch settings: `update_default_branch`, `update_default_target_branch`, `clear_missing_branch_settings`.
- custom actions: `upsert_project_action`, `upsert_global_action`, `delete_project_action`.
- shortcuts and settings: `set_shortcut_binding`, `clear_shortcut_binding`, `reset_shortcut_binding`, `reset_shortcuts`.
- agent settings: `set_default_agent`, `set_agent_enabled`, `set_agent_launch_args`, `remove_agent_launch_args`.
- Open In settings: `set_open_in_app_enabled`, `set_preferred_open_in_app`.
- git action settings: `set_git_commit_generation_script`, `reset_git_commit_generation_script`, `set_git_pr_generation_script`, `reset_git_pr_generation_script`, `set_git_commit_generation_llm`, `set_git_pr_generation_llm`.
- projection ingestion that currently mutates local stores: `absorb_projection`, `absorb_ui_snapshot`, `set_remote_snapshot` should become client-side projection application, not authority mutation.

### Desktop-only ephemera

These stay local to the GPUI app/workspace and should not be persisted through daemon state:

- modal open/close state, form drafts, validation text, search text, hover/pressed state, context menus.
- panel/sidebar current pixel sizes and transient open/close animation state unless explicitly promoted to persistent preference.
- active keyboard focus, scroll positions, drag state, selected rows, transient diff selection.
- live PTY handles, writer/broadcast maps, viewport claims, in-flight launch bookkeeping, resize queues.
- toast queue and user-facing notification lifecycle.

If a value affects what another paired client should render after reconnect, classify it as daemon-owned instead.

## Current `ProjectStore` methods that move behind the seam

`ProjectStore` can remain as an internal canonical-state implementation while callers migrate. Its public mutators should stop being called from app code directly and become private helpers under `StateAuthority`:

- catalog: `insert_prepared_project`, `remove_project`, `remove_worktree_project`, `remove_exact_project`.
- task: `insert_task`, `remove_task`, `remove_task_by_id`, `rename_task`, `set_task_pinned`, `update_task_branch`.
- sections/tabs: `set_section_state`, `set_terminal_section`, `update_task_tabs`, `set_tab_session`, `set_tab_restore_status`, `remove_sections`, `remove_terminal_sections`.
- UI/settings: all `set_*`, `reset_*`, `clear_*`, `upsert_*`, and `delete_*` methods that currently mutate `ui`, projects, repos, actions, or tasks.
- persistence/projection internals: `load`, `save`, `snapshot_for_save`, `read_from_disk`, `rebuild_runtime_views`, `refresh_runtime_views`, `sanitize` become authority/persistence/projection internals rather than app-callable operations.

Read-only helpers such as `project`, `task`, `repo_for_workspace`, `branch_names`, and `workspace_path` can stay as query helpers initially, but long-term callers should prefer explicit projection/query methods.

## Test plan for the authority interface

- Mutation commit order: one mutation updates canonical state, rebuilds projections, calls persistence once, and emits one broadcast tick.
- Idempotent mutations: repeated set/rename/remove commands report `changed = false` and do not emit duplicate persistence/broadcast effects when nothing changed.
- JSON adapter compatibility: existing v3/v4 fixtures load, migrate, sanitize, and save with the same shape as today.
- In-memory adapter: tests can seed canonical state and assert outcomes without filesystem access.
- Projection fixtures: canonical projects/tasks/repos/sections project into `ProjectSummary`, `TaskSummary`, `TabSummary`, `RepoSummary`, and `UiSnapshot` consistently.
- Client ingestion: desktop applies remote `ProjectList` snapshots through the same projection path for in-process and iroh sessions; newest queued projection wins.
- Settings sync: theme, expanded repos, sidebar git metadata, pinned tasks, and last active section mutated through Control are visible in the next `UiSnapshot`.
- Section/tab reconciliation: open/close/select/title/restore-status mutations preserve active tab rules and ignore stale projections.

## Migration sequence

1. Add `StateAuthority` around `ProjectStore` inside `RegistryState`; route `DesktopTerminalRegistry::with_store_mut` through `apply`.
2. Add `AppStatePersistence` with a JSON adapter that reuses the existing store-file structs and coalesced writer.
3. Split canonical state from projection builders so `ProjectStore` public vectors/maps are no longer treated as authority.
4. Move persistent desktop settings from direct `commit_local_mutation()` sites to `Control` verbs.
5. Collapse client `ProjectList` ingestion into one projection applicator.

This sequence lets each step preserve behavior while shrinking the set of code allowed to mutate durable state.
