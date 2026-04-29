# Slint Parallel-Work Ownership Map

This is the authoritative table for which agent (or person) owns which files
during the GPUI → Slint port. Before any change, agents read this doc and the
matching port-review under `docs/architecture/reviews/`.

## Why this exists

Several Slint surfaces are scoped per `bd` ticket but share a small set of
monolithic files (`slint-poc/src/lib.rs`, `slint-poc/ui/app.slint`,
`slint-poc/ui/components.slint`, `desktop/src/daemon_host.rs`). Without a
contract, two parallel agents working on different surfaces still merge-
conflict on those four files. The Phase A substrate split (see
`/home/mason/.claude/plans/in-this-branch-i-m-snazzy-pie.md`) carves those
monoliths into per-surface modules so each agent's diff stays in its own
column. This doc records the resulting ownership.

After Phase A lands, this doc moves from "target layout" to "shipped layout"
and the parallel fan-out reads it as the source of truth.

## Status

- **Phase A in progress.** Target paths are listed below; extractions land
  one commit at a time. Until A.6 ships, the "current location" column
  reflects pre-split reality for un-extracted surfaces.

### Extractions landed so far

| # | Module | Lines | Highlights |
|---|---|---|---|
| 1 | `slint-poc/src/daemon_ticket.rs` | ~170 | DaemonTicket discovery + parsing + pre-auth. Source-contract pin to GPUI writer paths. |
| 2 | `slint-poc/src/toast.rs` | ~95 | set_toast / clear_toast / toast_clipboard_text. Pin to GPUI show_*_toast. |
| 3 | `slint-poc/src/titlebar.rs` | ~180 | build_chip_label, debug_banner_text, Open In menu projection. Pin to titlebar overlay symbols. |
| 4 | `slint-poc/src/right_inspector.rs` | ~820 | All Changes/Commits/Checks/Compare row builders + AppWindow mutators + InspectorCommitFileChangesState + four row-height constants. |
| 5 | `slint-poc/src/visual_fixtures.rs` | ~790 | seed_shell_model + seed_visual_state_fixture + seed_component_state_fixture + per-mode right-inspector fixtures. |
| 6 | `slint-poc/src/util.rs` | ~145 | Cross-cutting frame formatters (compact_path, initials, project_accent_color, project_kind_label, provider_label, restore_status_label, task_metadata, worktree_name) + PROJECT_ACCENTS palette. |
| 7 | `slint-poc/src/terminal_target.rs` | ~150 | TerminalTarget + 9 resolution helpers shared by sidebar and terminal. |
| 8 | `slint-poc/src/resource.rs` | ~145 | set_resource_usage AppWindow mutator + resource_sessions_summary + resource_usage_rows. |
| 9 | `slint-poc/src/workspace_shell.rs` | ~500 | WorkspaceShellModel + ProjectGithubUrls + set_workspace_tree + workspace_shell_model + sidebar_tree_rows + sidebar_task_menu_entries + TerminalPanelModel + active_tab_for_task. |
| 10 | `slint-poc/src/terminal_input.rs` | ~335 | Slint key/pointer event types + pure terminal protocol encoders. |
| 11 | `slint-poc/src/terminal_view.rs` | ~115 | TerminalSurface POD + AppWindow mutators (set_terminal_status, set_project_overview_placeholder, set_terminal_surface, set_terminal_selection, apply_terminal_surface). |
| 12 | `slint-poc/src/terminal_colors.rs` | ~150 | Pure ANSI/indexed/named color resolution. |
| 13 | `slint-poc/src/terminal_runs.rs` | ~250 | ResolvedCellStyle + run accumulator + cell helpers + cursor/background/link span coalescers. |

`slint-poc/src/lib.rs` shrunk from 7,554 → ~4,290 lines (~43% reduction).
Test count rose from 111 → 124 (13 new source-contract / behavior tests
added across the extractions). All extractions verified with `cargo fmt
-p slint-poc && cargo test -p slint-poc --lib`.

### Extractions still pending

The biggest remaining chunk is the terminal renderer itself —
`spawn_terminal_worker` (~1,900 lines) plus `AlacrittySnapshot` impl
(~700 lines) plus their `RuntimeEventProxy` / `TerminalSize` /
`window_size_from_grid` support and `seed_terminal_fidelity_fixture`.
These are tightly coupled; they should extract together as
`slint-poc/src/terminal_renderer.rs` in a single focused slice.

Smaller follow-ups:

1. `terminal_render_probe.rs` — the public `TerminalRenderProbeReport`
   struct + `run_terminal_render_probe` plus `percentile`,
   `mib_per_second`, `current_rss_kib` helpers. Crate-root `pub use`
   re-export must stay because `scripts/slint/terminal_render_probe.rs`
   binary consumes them.
2. `daemon.rs` — outbound `Control` senders (`switch_terminal_target`
   etc.) + the inbound `WorkerReply` dispatcher.
3. `app.rs` — `run_app` bootstrap + `android_main` + remaining callback
   wiring.

UI / Slint (`app.slint` split) and `desktop/src/daemon_host.rs` splits
(A.3, A.4) remain unstarted. They land after the lib.rs split is fully
done.

## Surface ownership

Each row: target Slint Rust module → target Slint UI file → GPUI source of
truth → owning bd ticket(s) → required daemon-host adapters → port-review
doc. Agents working a row may edit only those files (plus tests under the
same module). Anything else is a coordination point.

| Surface | Target Rust | Target UI | GPUI source | bd ticket | Daemon-host adapter | Port review |
|---|---|---|---|---|---|---|
| App shell / bootstrap | `slint-poc/src/app.rs` | `slint-poc/ui/app.slint` (thin) | `desktop/src/app.rs` | `another-one-dn3` (epic) | n/a | (cross-cutting) |
| Daemon dispatcher | `slint-poc/src/daemon.rs` | n/a | `desktop/src/daemon_host.rs` | `another-one-rbv` | `daemon_host/mod.rs` | (none yet) |
| Toast stack | `slint-poc/src/toast.rs` | `slint-poc/ui/components.slint` (toast primitives) | `desktop/src/app.rs` (`show_*_toast`, `toast_layer`) | `another-one-y4n.8` | n/a | `slint-overlays-port-review.md` |
| Titlebar | `slint-poc/src/titlebar.rs` | `slint-poc/ui/titlebar.slint` | `desktop/src/titlebar.rs`, `desktop/src/open_in.rs`, `desktop/src/custom_actions_modal.rs` | `another-one-dn3.10`, `another-one-dn3.7` (chrome) | `daemon_host/open_in.rs`, `daemon_host/git.rs`, `daemon_host/actions.rs` | `slint-overlays-port-review.md` (titlebar menus) |
| Left sidebar | `slint-poc/src/left_sidebar.rs` | `slint-poc/ui/left_sidebar.slint` | `desktop/src/left_sidebar.rs` | `another-one-dn3.6` | `daemon_host/projects.rs` | `slint-sidebar-port-review.md` |
| Right inspector | `slint-poc/src/right_inspector.rs` | `slint-poc/ui/right_inspector.slint` | `desktop/src/right_sidebar.rs` | `another-one-dn3.8` | `daemon_host/changes.rs`, `daemon_host/commits.rs`, `daemon_host/checks.rs`, `daemon_host/git.rs` | `slint-right-inspector-port-review.md` |
| Terminal workspace | `slint-poc/src/terminal_workspace.rs` | `slint-poc/ui/terminal_workspace.slint` | `desktop/src/panels.rs`, `desktop/src/terminal_runtime.rs` | `another-one-dn3.7` | `daemon_host/tabs.rs` | `slint-terminal-workspace-port-review.md` |
| Footer | (folded into `left_sidebar.rs`) | `slint-poc/ui/footer.slint` | `desktop/src/left_sidebar.rs` (footer region) | `another-one-dn3.6` | `daemon_host/projects.rs` (`AddProject`) | `slint-sidebar-port-review.md` |
| Modals (new task, action, create branch, pair mobile) | `slint-poc/src/modals.rs` | `slint-poc/ui/overlays.slint` (existing) | `desktop/src/new_task_modal.rs`, `desktop/src/custom_actions_modal.rs`, `desktop/src/create_branch_modal.rs`, `desktop/src/pair_mobile.rs`, `desktop/src/add_agent_modal.rs` | `another-one-dn3.10`, `another-one-y4n.7` | `daemon_host/actions.rs`, `daemon_host/pair.rs` | `slint-overlays-port-review.md` |
| Resource | `slint-poc/src/resource.rs` | `slint-poc/ui/overlays.slint` (resource popover) | `desktop/src/resource_indicator.rs`, `core/src/resource_usage.rs` | `another-one-y4n.8` | `daemon_host/resource.rs` | `slint-overlays-port-review.md` (resource popover) |
| Settings | `slint-poc/src/settings.rs` (existing) | `slint-poc/ui/settings.slint` (existing) | `desktop/src/settings_page.rs`, `desktop/src/mcp_page.rs` | `another-one-dn3.9` | `daemon_host/settings.rs` | `slint-settings-port-review.md` |
| Visual fixtures | `slint-poc/src/visual_fixtures.rs` | n/a | `docs/architecture/slint-visual-fidelity-gate.md` | `another-one-p4r.6` | n/a | `slint-visual-gap-report.md` |
| Style tokens | `slint-poc/src/style.rs` (existing) | `slint-poc/ui/components.slint` (token defs) | `desktop/src/tokens.rs`, `desktop/src/theme.rs` | `another-one-p4r` (epic) | n/a | (none yet) |
| Platform | `slint-poc/src/platform.rs` (existing) | n/a | `desktop/src/platform/{linux,macos,windows}.rs` | `another-one-rbv` | n/a | `slint-platform-traits.md` |
| Overlays internals (contract tests) | `slint-poc/src/overlays.rs` (existing) | `slint-poc/ui/overlays.slint` (existing) | (multiple) | `another-one-y4n.7`, `another-one-y4n.8` | n/a | `slint-overlays-port-review.md` |

## Shared substrate (coordinate before editing)

These files belong to nobody by surface — they are shared substrate. Edits
require a PR comment + user approval before merging. After Phase A, each
file has a top-of-file comment repeating this rule.

- `slint-poc/src/app.rs` — bootstrap only. Per-surface code goes in the
  surface module; only orchestration, channel wiring, and global toast
  dispatch live here.
- `slint-poc/ui/app.slint` — thin top-level composing the per-surface UI
  files. No surface-specific layout here.
- `slint-poc/ui/components.slint` — the base-component library
  (`another-one-y4n.7` ownership). New primitives must be reused by ≥ 2
  surfaces or they belong in the surface's own `.slint`.
- `desktop/src/daemon_host/mod.rs` — public dispatcher only. Per-control
  registry adapters live in submodules.
- `desktop/src/app.rs` — GPUI app. Slint work does not edit this file
  except when adding a method that the embedded daemon needs to call.
- `core/` — shared between GPUI and Slint. Edits require both apps to
  build.

## Drive-by-finding rule (overrides AGENTS.md)

`AGENTS.md` describes a GitHub-issue-first workflow. **For this branch we
file in `bd`, not GitHub.** When you see a bug, smell, or follow-up that
isn't in your column:

1. Confirm the title + parent epic with the user.
2. `bd create --title "..." --parent <epic> --priority <P>`.
3. Reference the new bd id in your PR description (`Closes
   another-one-xyz.N` or `See another-one-xyz.N` for follow-ups).

## "Stay in your column" enforcement

If your slice would edit a file outside your column:

- File a `bd` dependency on the ticket that owns that file.
- Or coordinate via PR comment with the file's owning agent/ticket.
- Never edit silently — every cross-column edit must be a deliberate
  hand-off, not a side effect.

## Closed-ticket suspicion

`another-one-dn3.6` was closed and reopened the same day during exact-parity
review (Codex marked it done; the user pulled it back). Treat closed bd
tickets in this branch as **suspect until independently verified**. Phase
A.5b runs an audit pass; until that ships, do not assume any closed surface
is parity-complete.

## Verification gate (every slice, every agent)

```sh
cargo fmt -p slint-poc
cargo check -p slint-poc
cargo test -p slint-poc --lib
# When daemon-host changed:
cargo check -p another-one
cargo test -p another-one daemon_host
```

End-to-end visual proof for visible slices: launch via
`./scripts/slint/linux-dev.sh --run` (or `macos-build.sh` on Darwin) and
capture under `docs/reference/slint/slint-daemon-poc-clean/captures/`.

## Cross-references

- Plan: `/home/mason/.claude/plans/in-this-branch-i-m-snazzy-pie.md`.
- Production gates: `docs/architecture/slint-production-readiness-gates.md`.
- Visual fidelity gate: `docs/architecture/slint-visual-fidelity-gate.md`.
- Per-surface port reviews: `docs/architecture/reviews/slint-*-port-review.md`.
- Missing functionality wireframe:
  `docs/architecture/reviews/slint-missing-functionality-wireframe.md`.
- Visual gap report:
  `docs/architecture/reviews/slint-visual-gap-report.md`.
