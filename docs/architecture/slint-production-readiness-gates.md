# Slint Production Readiness Gates

The GPUI app is the absolute product baseline. Slint work is production-ready
only when each gate below has concrete source inventory, implementation evidence,
and verification artifacts. Missing GPUI facets are gaps, not optional polish.

## Gate 1: Baseline Inventory

Required evidence:

- GPUI source inventory for layout, views, style, base components, platform
  integrations, and terminal behavior.
- Owner mapping from each GPUI facet to a Slint epic or task.
- Explicit bd decision for any accepted deviation.

Blocking beads:

- `another-one-p9w`
- `another-one-p4r`
- `another-one-6n5`
- `another-one-dn3`
- `another-one-y4n`
- `another-one-rbv`
- `another-one-4vk`

## Gate 2: Visual Fidelity

Required evidence:

- GPUI capture corpus or documented unavailable-evidence record.
- Slint capture corpus using matching names.
- Automated diff output for geometry/color-sensitive surfaces.
- Manual review notes for text quality, icons, shadows, borders, terminal cells,
  and platform chrome.

Protocol:

- `docs/architecture/slint-visual-fidelity-gate.md`

## Gate 3: Terminal Production Readiness

Required evidence:

- typed terminal input protocol;
- attach/detach/reattach lifecycle proof;
- grapheme and wide-cell rendering proof;
- cursor/selection/link/copy/focus/mouse proof;
- renderer batching and throughput proof;
- idle CPU/memory readings for app-window usage, separate from subprocess
  tracking.

Protocol:

- `docs/architecture/terminal-production-readiness.md`

## Gate 4: Platform Trait Boundary

Platform-specific behavior must be selected through narrow Rust trait/profile
seams instead of ad hoc branches in shared Slint view/layout/component code.

Required evidence:

- build profile selection for Linux, macOS, Android, and iOS;
- system appearance resolution through platform profile support;
- input policy differences for desktop keyboard/mouse and mobile touch/IME;
- window/chrome/orientation behavior documented per target.

## Gate 5: Layout And View Parity

Required evidence:

- desktop, compact desktop, tablet, mobile portrait, and mobile landscape layout
  contracts;
- every GPUI screen, panel, modal, popover, toast, and terminal state mapped to a
  Slint view contract;
- resize and drawer behavior verified without UI-thread stalls.

## Gate 6: Component Readiness

Required evidence:

- GPUI component state matrix;
- Slint component API catalog;
- component screenshot fixtures or documented reference crops;
- hover, active, focus, disabled, loading, selected, and error states covered
  where applicable;
- all interactive controls expose labels/tooltips unless explicitly decorative.

## Gate 7: Build And Runtime Proof

Required evidence:

- Linux and macOS desktop builds;
- Android install/run proof;
- iOS simulator build proof on macOS;
- hot-reload/dev workflow proof for active Slint development;
- no unsupported platform is implied by shared scripts or docs.

## Closure Rule

An epic can close only when its child gates are closed or explicitly waived by a
bd decision. A task can close with unavailable evidence only when the unavailable
state is documented, scoped to that task, and linked to a follow-up blocker or
decision.
