# Slint Visual Fidelity Gate

The GPUI app is the visual baseline for Slint productization. Slint surfaces do
not pass review because they are "close enough"; every visible difference is
either fixed or recorded as a bd decision that explicitly accepts the deviation.

## Artifact Roots

- GPUI baseline captures: `docs/reference/gpui-baseline/<baseline-sha>/`
- Slint captures: `docs/reference/slint/<branch-or-sha>/`
- Pair manifests: `manifest.md` in each capture root
- Metrics: `metrics/*.json`
- Review notes: `notes/*.md`

## Screenshot Pair Naming

Every comparison uses the same relative name in both roots:

- `window/desktop-main-dark.png`
- `window/desktop-main-light.png`
- `window/compact-main-dark.png`
- `titlebar/default.png`
- `titlebar/dirty-branch.png`
- `sidebar/project-list-default.png`
- `sidebar/task-list-hover-active.png`
- `terminal/color-smoke.png`
- `terminal/text-quality.png`
- `terminal/selection-cursor-links.png`
- `right-sidebar/changes.png`
- `right-sidebar/commits.png`
- `settings/agents.png`
- `modal/new-task.png`
- `toast/error.png`

If a state cannot be captured automatically, add a note with:

- the state name;
- why automated capture failed;
- exact manual reproduction steps;
- the bd task or decision that owns resolving the missing artifact.

## Owner Mapping

- Style owns color roles, typography, density, radius, shadows, icon treatment,
  and light/dark/system appearance comparisons.
- Components own reusable control states: normal, hover, active, focus,
  disabled, loading, selected, error.
- Views own per-view composition and viewport-specific section behavior.
- Layout owns shell geometry, drawer/top-strip behavior, resize behavior, and
  mobile/desktop layout differences.
- Terminal owns cell metrics, glyph quality, ANSI/indexed/truecolor color,
  cursor/selection/link/copy/focus/mouse behavior, and throughput evidence.
- Platform owns system appearance detection, window/chrome behavior, mobile
  orientation, and target-specific build/runtime differences.

## Automated Diff Gate

Automated image diff is advisory for text-heavy surfaces and mandatory for
geometry/color regressions.

Pass thresholds:

- Full-window shell: no more than 0.5% changed pixels after ignoring known
  dynamic content regions.
- Component fixture: no more than 0.25% changed pixels.
- Color swatches and chrome regions: exact sampled token match unless an
  accepted decision says otherwise.
- Terminal cell background spans: exact cell-position and color match.

Dynamic masks may cover:

- terminal cursor blink;
- live resource values;
- timestamps;
- git SHA/build chip text;
- subprocess output content when the fixture does not pin command output.

Masks must be declared in the pair manifest.

## Manual Review Gate

Manual review is required even when automated diff passes.

Review checklist:

- Text rendering: weight, size, line-height, antialiasing quality, truncation,
  terminal monospace metrics, and fallback glyph behavior match the GPUI
  baseline closely enough for side-by-side use.
- Icons: stroke/fill, alignment, hit target, disabled tint, hover tint, and
  selected tint match the baseline.
- Borders/shadows: card edges, focus rings, divider lines, modal scrims, and
  popover elevation match the baseline.
- Motion: transitions do not introduce layout lag or obscure state changes.
- Terminal: ANSI/indexed/truecolor colors, cursor shape, selection, links,
  copy text, mouse/focus reporting, and wide/combining glyph handling pass the
  terminal readiness contract.
- Platform: Linux and macOS desktop windows preserve expected chrome behavior;
  mobile surfaces document intentional adaptations separately from desktop
  fidelity.

## Light Dark System Review

Dark mode is the GPUI fidelity baseline. Light mode is a new extension and must
still use the same role model.

Required checks:

- force dark resolved appearance and capture all required dark surfaces;
- force light resolved appearance and capture representative shell, modal,
  terminal, and settings surfaces;
- force system mode to dark and light through the platform trait/profile seam;
- record whether the platform supports live system appearance changes or only
  startup-time resolution.

## Deviation Handling

Visual deviations have only three valid outcomes:

- Fix the Slint implementation.
- Link an existing bd decision that accepts the deviation.
- Create a proposed bd decision and wait for approval before treating the
  deviation as accepted.

Deviation notes must include:

- affected screenshot pair;
- observed difference;
- suspected source file/component;
- risk to GPUI parity;
- linked fix task or decision.

## Minimum Pass Set

Before broad Slint view implementation can be accepted, these must pass:

- desktop main shell dark;
- titlebar default and dirty branch;
- left project/task sidebar hover and active states;
- terminal color smoke and text-quality fixtures;
- new-task modal;
- one right-sidebar mode;
- one settings surface;
- one toast/error surface;
- compact/mobile layout smoke.

Terminal-specific pass criteria remain stricter and are defined in
`docs/architecture/terminal-production-readiness.md`.
