# Slint Layout Contract

The Slint shell consumes the GPUI layout baseline from `desktop/src/layout.rs` while adding explicit mobile and tablet modes.

## Breakpoints

| Mode | Width | Shell behavior |
| --- | --- | --- |
| `mobile` | `< 760px` | Single terminal-first column. Persistent sidebars and footer are hidden. Active task context appears in a compact terminal header. |
| `tablet` | `760px..1179px` | Left project/task sidebar remains visible at reduced width. Right inspector collapses. Footer remains visible. |
| `desktop` | `>= 1180px` | Left sidebar, terminal center, and footer are visible. Right inspector appears when width is at least `1280px`. |

## GPUI Geometry Mapping

| GPUI token | Value | Slint mapping |
| --- | --- | --- |
| `GUTTER` | `0px visible split in captured shell; 4px collapsed rail token remains GPUI source context` | desktop `gutter-width` is `0px` so the center pane starts at the GPUI-captured sidebar edge |
| `SIDEBAR_COLLAPSED` | `4px` | mobile sidebar hidden, tablet/desktop persistent |
| `TITLEBAR_CHROME_H` | `42px captured desktop strip` | desktop/tablet `titlebar-height`; mobile uses `52px` for touch context |
| `FOOTER_H` | `38px` | desktop/tablet `footer-height`; mobile hides footer |
| `TERMINAL_TAB_BAR_H` | `36px` | Slint tab bar area is `38px` including divider |
| `MIN_MAIN` | `200px` | terminal surface wins width before right inspector opens |
| `DESKTOP_LEFT_RAIL` | `282px captured at 1902px window width` | desktop `sidebar-width`; tablet keeps reduced `252px` rail |
| `DESKTOP_RIGHT_RAIL` | `464px captured at 1902px window width` | desktop `right-width` when the inspector is visible |

## Current Implementation

`slint-poc/ui/app.slint` defines:

- `mobile-layout`
- `tablet-layout`
- `layout_label`
- responsive `titlebar-height`
- responsive `footer-height`
- responsive `sidebar-width`
- responsive `right-width`
- mobile terminal context header

The shell no longer scales the desktop view into phone widths. Mobile layout is terminal-first and hides persistent sidebars until drawer/sheet navigation is implemented.

The first matched GPUI/Slint desktop captures use a `1902x1023` client region. At that geometry the Slint shell now matches the GPUI macro regions: `282px` left rail, `1156px` center workspace, and `464px` right inspector.

Linux viewport captures now cover desktop (`1902x1023`), tablet (`1024x768`), mobile portrait (`390x844`), and mobile landscape (`844x390`) under `docs/reference/slint/slint-daemon-poc-clean/captures/window/`. These prove breakpoint behavior in the desktop compositor; Android rotation/runtime proof remains a device-gated platform item.

## Remaining Work

- Add actual drawer/sheet navigation for mobile project/task lists.
- Add touch-sized titlebar/menu controls for tablet/mobile.
- Move layout constants into a generated/shared artifact once Slint supports the final production crate structure.
- Capture Android runtime rotation screenshots once an `adb` target is available.
