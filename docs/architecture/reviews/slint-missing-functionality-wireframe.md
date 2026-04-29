# Slint Missing Functionality Wireframe

Current branch: `slint-daemon-poc-clean`

Purpose: define the remaining Slint implementation gaps against the GPUI baseline before closing any migration bead. This is not a new design. GPUI remains the source of truth for section relationships, assets, colors, state, and behavior.

## Scope Rules

- Slint/Rust only. Do not introduce Flutter or Dart work.
- Wire functionality only when the daemon protocol and `desktop/src/daemon_host.rs` can satisfy the request.
- Keep placeholders visible as implementation gaps when a daemon-host adapter is missing.
- Route user-facing failures through the Slint toast surface.
- Reuse GPUI assets from `desktop/assets` unless a source review proves the asset is not applicable.

## Remaining Surface Map

| Surface | GPUI Source | Current Slint State | Required Functionality | Daemon Status | bd Owner |
| --- | --- | --- | --- | --- | --- |
| Titlebar Open In | `desktop/src/titlebar.rs`, `desktop/src/open_in.rs`, `desktop/src/daemon_host.rs` | Visual button/menu exists but reads fixture entries and only shows a toast. | Load enabled apps, mark preferred app, open the active project through the selected app, refresh preferred app after launch. | Ready: `Control::OpenInState`, `Control::OpenProjectInApp`, `WorkerReply::OpenInStateAck`, and `OpenProjectInAppAck` are implemented by `desktop/src/daemon_host.rs`. | `another-one-dn3.10` |
| Titlebar custom Actions | `desktop/src/titlebar.rs`, `desktop/src/custom_actions_modal.rs`, `desktop/src/app.rs` | Menu and action modal are present but still fixture-driven. | Load project/global actions, run selected action, persist add/edit/delete action, attach resulting tab. | Partial: protocol has `ListProjectActions`, `RunProjectAction`, `SaveProjectAction`, `DeleteProjectAction`; `desktop/src/daemon_host.rs` does not implement the registry adapter yet. | `another-one-dn3.10`, `another-one-y4n.7` |
| Titlebar git Push/Commit menu | `desktop/src/titlebar.rs`, `desktop/src/app.rs`, `desktop/src/git.rs` | Push split button is a placeholder toast. | Run GPUI-equivalent toolbar git actions, surface progress/errors, refresh project tree and right inspector after mutation. | Partial: protocol has `RunToolbarGitAction`; `desktop/src/daemon_host.rs` does not implement the registry adapter yet. | `another-one-dn3.10` |
| Pair mobile QR | `desktop/src/pair_mobile.rs`, `desktop/src/app.rs` | QR button exists but only shows a placeholder toast. | Read pairing URL/QR bytes, show QR modal/popover, support reset/refresh. | Missing Slint-facing daemon read/reset control. GPUI reads from local endpoint state. | `another-one-rbv` |
| Footer add project | `desktop/src/left_sidebar.rs`, `desktop/src/app.rs` | Footer icon exists but is a placeholder toast. | Open file picker or path prompt, call add-project, refresh project tree, toast failures. | Partial: protocol has `AddProject`; `desktop/src/daemon_host.rs` does not implement `add_project`. | `another-one-dn3` |
| Project row menu | `desktop/src/left_sidebar.rs`, `desktop/src/app.rs` | Anchored menu exists; it must be a source-backed sorting menu, not a generic project action menu. | Show `Sort tasks by` choices: Recent activity, Most activity, Manual. Row click still owns project activation; project remove is a separate destructive confirmation flow, not this menu. | Missing Slint sort-state persistence/control. Do not fake project open/remove actions inside this menu. | `another-one-dn3` |
| Task row menu | `desktop/src/left_sidebar.rs`, `desktop/src/app.rs` | Anchored menu exists; it must expose task actions in GPUI order. | Pin/Unpin, New task from current branch, Rename, Delete. Row click still owns task activation. Delete and rename require GPUI-equivalent confirmation/inline edit flows. | Partial: `SetTaskPinned` is safe to wire now; `RenameTask` and `RemoveTask` still need full Slint UI/confirmation integration before use. | `another-one-dn3` |
| New Task modal | `desktop/src/new_task_modal.rs`, `desktop/src/app.rs` | Basic modal submits a task through daemon. | Match GPUI project/source branch selection, validation, enabled agents, existing/new branch mode, keyboard flow. | Partial: `SubmitNewTask` works; enabled-agent data exists; branch mode and agent selection UI remain incomplete. | `another-one-y4n.7`, `another-one-dn3` |
| Right inspector | `desktop/src/right_sidebar.rs`, `desktop/src/app.rs` | Changes, commits, checks, compare have real daemon-backed reads/mutations. | Continue visual parity loops, add any missing row states, keep stage/unstage/discard confirmation behavior aligned. | Mostly ready for current scope. | `another-one-p4r.6`, `another-one-dn3` |
| Resource indicator/popover | `desktop/src/resource_indicator.rs`, `desktop/src/resource_usage.rs` | Visual component exists; summary is mostly static. | Track app-window CPU/memory only, list subprocesses separately, refresh without causing idle spikes. | Missing Slint-facing daemon/resource snapshot model. | `another-one-y4n.8`, terminal readiness epic |
| Toast copy | `desktop/src/app.rs` toast helpers | Toast renders copy action but copy callback is a placeholder. | Copy current toast message/detail to clipboard and report success/failure through toast. | UI-local; no daemon needed. | `another-one-y4n.8` |
| Settings | `desktop/src/settings_page.rs`, `desktop/src/app.rs` | Daemon-backed settings pages exist and read the daemon location. | Keep visual parity and ensure titlebar/open-in state refreshes after Open In settings toggles. | Ready for current pages. | `another-one-dn3`, `another-one-p4r.6` |

## Immediate Wiring Order

1. Wire titlebar Open In because both the daemon protocol and desktop daemon-host adapter are complete.
2. Wire toast copy because it is UI-local and unblocks reusable toast behavior.
3. Keep action/git/project/task mutation placeholders open until the daemon-host registry methods exist.
4. Continue visual parity loops after each functional slice; do not close visual-fidelity beads from protocol-only work.

## Source Review Requirements Per Slice

- Titlebar/menu work must review `desktop/src/titlebar.rs`, the relevant daemon `Control` and `WorkerReply` variants, and the current Slint overlay behavior.
- Sidebar work must review `desktop/src/left_sidebar.rs` and preserve the project-to-task nesting model before any row action changes.
- Modal/form work must review `desktop/src/new_task_modal.rs` or `desktop/src/custom_actions_modal.rs` and update `docs/architecture/slint-base-component-api.md` if shared primitives change.
- Resource work must review `desktop/src/resource_indicator.rs` and `desktop/src/resource_usage.rs` before changing polling or refresh cadence.

## Verification Gate

- Add Rust tests for every source contract that can regress without launching the app.
- Run `cargo fmt -p slint-poc`.
- Run `cargo check -p slint-poc`.
- Run `cargo test -p slint-poc --lib`.
- Add bd notes before closing or deferring any bead.
