# Slint View Viewport Contracts

Views own content structure and viewport-specific behavior. The layout epic owns
breakpoint values, shell geometry, drawer mechanics, and resize math. A view may
name viewport states, but it must not duplicate breakpoint definitions.

## Viewport States

- `desktop`: full desktop shell with titlebar, left project/task sidebar,
  terminal workspace, and optional right inspector.
- `compact_desktop`: narrow desktop window where sidebars may collapse but
  desktop keyboard/mouse semantics remain.
- `tablet`: touch-first large viewport with reduced chrome density and explicit
  modal sizing constraints.
- `mobile_portrait`: single-column phone viewport with navigation surfaces
  replacing persistent desktop sidebars.
- `mobile_landscape`: short-height phone/tablet viewport prioritizing terminal
  height and compact top/bottom chrome.

## Shell

- `desktop`: persistent titlebar, left sidebar, terminal workspace, optional
  right sidebar.
- `compact_desktop`: titlebar remains; left/right sidebars may collapse into
  toggles; terminal keeps focus and resize state.
- `tablet`: sidebars become overlay or split surfaces selected by layout.
- `mobile_portrait`: one primary content surface at a time; navigation moves to
  compact top strip or bottom affordance.
- `mobile_landscape`: terminal-first; secondary panels must not consume vertical
  space unless explicitly opened.

Unsupported states: none. Shell must define behavior for every viewport.

## Titlebar

- `desktop`: mirrors GPUI titlebar actions: build/branch state, Actions, Open In,
  GitHub/PR, resource monitor, commit controls, side toggles.
- `compact_desktop`: actions may collapse into menus, but primary branch/resource
  state remains visible.
- `tablet`: titlebar can become top app bar with touch-sized controls.
- `mobile_portrait`: titlebar becomes compact context header; overflow actions
  move into menu/sheet.
- `mobile_landscape`: shortest possible context strip; terminal content gets
  priority.

Unsupported states: none.

## Project Task Sidebar

- `desktop`: persistent project list with nested task rows and hover/active
  states.
- `compact_desktop`: collapsible drawer preserving active project/task context.
- `tablet`: drawer or split list depending on available width.
- `mobile_portrait`: navigation list is a full-screen or sheet view; terminal is
  not shown behind an active list unless layout explicitly chooses overlay.
- `mobile_landscape`: compact project/task selector, preferably popover/sheet,
  not a persistent sidebar.

Unsupported states: none.

## Terminal Workspace

- `desktop`: primary center view with task tabs, terminal pane, footer/status
  line, selection/link/copy affordances, and resize reporting.
- `compact_desktop`: terminal remains primary; sidebars collapse before terminal
  loses required minimum dimensions.
- `tablet`: terminal remains primary but controls use touch-friendly hit targets.
- `mobile_portrait`: one terminal session at a time; tabs become switcher/sheet.
- `mobile_landscape`: terminal consumes most vertical space; nonessential chrome
  collapses.

Unsupported states: none.

## Right Inspector

- `desktop`: optional persistent right sidebar for changes, commits, compare,
  checks, pull requests, and related project detail panels.
- `compact_desktop`: collapses into drawer or popover.
- `tablet`: overlay/sheet; persistent split only when layout provides enough
  width.
- `mobile_portrait`: separate view/sheet, never persistent beside terminal.
- `mobile_landscape`: separate view/sheet, optimized for short height.

Unsupported states: none, but mobile persistent side-by-side inspector is not
supported.

## Project Page

- `desktop`: can use full content width with sections for project metadata,
  pull requests, actions, and settings-like surfaces.
- `compact_desktop`: stacks sections vertically.
- `tablet`: stacks sections with touch spacing.
- `mobile_portrait`: single-column section list.
- `mobile_landscape`: compact section list; avoid tall modals.

Unsupported states: none.

## Settings

- `desktop`: left category list plus right content panel can be persistent.
- `compact_desktop`: category list collapses.
- `tablet`: category navigation can be split or sheet.
- `mobile_portrait`: category list and detail are separate navigation states.
- `mobile_landscape`: category list should not permanently reduce terminal or
  detail height unless explicitly in settings view.

Unsupported states: none.

## MCP Views

- `desktop`: catalog/registry/provider state can render as panels inside
  settings or dedicated right/detail surfaces.
- `compact_desktop`: panels stack.
- `tablet`: panels stack with touch-friendly toggles.
- `mobile_portrait`: one MCP section at a time.
- `mobile_landscape`: compact section switching; avoid persistent multi-column
  density.

Unsupported states: none.

## Modals

- `desktop`: centered card over scrim, matching GPUI modal sizing and focus
  behavior.
- `compact_desktop`: centered card with clamped width.
- `tablet`: card or sheet depending on layout policy.
- `mobile_portrait`: bottom sheet or full-screen form for large forms.
- `mobile_landscape`: short-height sheet/card with scroll when needed.

Unsupported states: none.

## Menus And Popovers

- `desktop`: anchored popovers/menus with GPUI hover/focus behavior.
- `compact_desktop`: anchored when space exists, otherwise sheet.
- `tablet`: touch menu or sheet.
- `mobile_portrait`: sheet/action list.
- `mobile_landscape`: compact sheet/action list.

Unsupported states: none.

## Toast Stack

- `desktop`: non-blocking toast stack, positioned to avoid titlebar/sidebars.
- `compact_desktop`: reflows away from collapsed drawer toggles.
- `tablet`: touch-safe position.
- `mobile_portrait`: top or bottom stack based on active keyboard/sheet state.
- `mobile_landscape`: short-height safe position, never covering terminal input
  footer when focused.

Unsupported states: none.

## Routing Rules

- View contracts consume layout-provided viewport state; views do not compute
  breakpoints directly.
- View-specific unsupported behavior must be explicit in this document.
- If a view requires a layout deviation from GPUI, create or link a bd decision
  before implementation.
- Component composition details belong in the view section/component tasks, not
  this viewport contract.
