//! Reusable panel helper and main content-row assembly.
//!
//! The centre panel now renders a real terminal grid from alacritty_terminal,
//! fed by a PTY spawned via portable-pty.

use gpui::{
    canvas, div, fill, hsla, point, prelude::*, px, rgb, size, svg, AnyElement, App, Bounds,
    ClipboardItem, Context, Element, ElementId, ElementInputHandler, Entity, Font, FontFallbacks,
    FontFeatures, FontStyle, FontWeight as GpuiFontWeight, GlobalElementId, InspectorElementId,
    KeyDownEvent, Keystroke, LayoutId, MouseButton, MouseDownEvent, Pixels, ScrollWheelEvent,
    SharedString, TextRun, Window,
};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::selection::SelectionType;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

use crate::app::ThreeColumnApp;
use crate::layout::*;

// ── Colour helpers ───────────────────────────────────────────────────

/// Convert an alacritty ANSI colour to a GPUI Hsla.
fn ansi_color_to_hsla(c: AnsiColor) -> gpui::Hsla {
    match c {
        AnsiColor::Named(n) => named_to_hsla(n),
        AnsiColor::Spec(rgb) => gpui::Hsla::from(gpui::Rgba {
            r: rgb.r as f32 / 255.,
            g: rgb.g as f32 / 255.,
            b: rgb.b as f32 / 255.,
            a: 1.,
        }),
        AnsiColor::Indexed(idx) => indexed_to_hsla(idx),
    }
}

fn named_to_hsla(n: NamedColor) -> gpui::Hsla {
    let rgb_val = match n {
        NamedColor::Black => 0x1e1f22,
        NamedColor::Red => 0xf07178,
        NamedColor::Green => 0xc3e88d,
        NamedColor::Yellow => 0xffcb6b,
        NamedColor::Blue => 0x82aaff,
        NamedColor::Magenta => 0xc792ea,
        NamedColor::Cyan => 0x89ddff,
        NamedColor::White => 0xd0d0d0,
        NamedColor::BrightBlack => 0x545454,
        NamedColor::BrightRed => 0xff5370,
        NamedColor::BrightGreen => 0xc3e88d,
        NamedColor::BrightYellow => 0xffcb6b,
        NamedColor::BrightBlue => 0x82aaff,
        NamedColor::BrightMagenta => 0xc792ea,
        NamedColor::BrightCyan => 0x89ddff,
        NamedColor::BrightWhite => 0xffffff,
        NamedColor::Foreground => 0xcccccc,
        NamedColor::Background => 0x1e1f22,
        _ => 0xcccccc,
    };
    gpui::rgb(rgb_val).into()
}

fn indexed_to_hsla(idx: u8) -> gpui::Hsla {
    if idx < 16 {
        // Standard 16 colours → map to named.
        let named = match idx {
            0 => NamedColor::Black,
            1 => NamedColor::Red,
            2 => NamedColor::Green,
            3 => NamedColor::Yellow,
            4 => NamedColor::Blue,
            5 => NamedColor::Magenta,
            6 => NamedColor::Cyan,
            7 => NamedColor::White,
            8 => NamedColor::BrightBlack,
            9 => NamedColor::BrightRed,
            10 => NamedColor::BrightGreen,
            11 => NamedColor::BrightYellow,
            12 => NamedColor::BrightBlue,
            13 => NamedColor::BrightMagenta,
            14 => NamedColor::BrightCyan,
            15 => NamedColor::BrightWhite,
            _ => NamedColor::Foreground,
        };
        named_to_hsla(named)
    } else if idx < 232 {
        // 6×6×6 colour cube (indices 16–231).
        let idx = idx - 16;
        let r = (idx / 36) % 6;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        let to_val = |v: u8| if v == 0 { 0u8 } else { 55 + 40 * v };
        gpui::Hsla::from(gpui::Rgba {
            r: to_val(r) as f32 / 255.,
            g: to_val(g) as f32 / 255.,
            b: to_val(b) as f32 / 255.,
            a: 1.,
        })
    } else {
        // Greyscale ramp (indices 232–255).
        let level = 8 + 10 * (idx - 232);
        gpui::Hsla::from(gpui::Rgba {
            r: level as f32 / 255.,
            g: level as f32 / 255.,
            b: level as f32 / 255.,
            a: 1.,
        })
    }
}

// ── Panel rendering ──────────────────────────────────────────────────

#[derive(Clone)]
struct RenderCell {
    text: String,
    fg: gpui::Hsla,
    bg: gpui::Hsla,
    bold: bool,
    italic: bool,
    width: usize,
}

fn cell_covers_cursor(
    cursor_pos: Option<(usize, usize)>,
    line_idx: usize,
    col_idx: usize,
    width: usize,
) -> bool {
    cursor_pos.is_some_and(|(cursor_line, cursor_col)| {
        cursor_line == line_idx && cursor_col >= col_idx && cursor_col < col_idx + width
    })
}

fn platform_ctrl_equivalent(keystroke: &Keystroke) -> Option<Keystroke> {
    if !keystroke.modifiers.platform
        || keystroke.modifiers.alt
        || keystroke.modifiers.control
        || keystroke.modifiers.function
    {
        return None;
    }

    let mut modifiers = keystroke.modifiers;
    modifiers.platform = false;
    modifiers.control = true;

    Some(Keystroke {
        modifiers,
        key: keystroke.key.clone(),
        key_char: keystroke.key_char.clone(),
    })
}

fn selection_type_for_click_count(click_count: usize) -> SelectionType {
    match click_count {
        2 => SelectionType::Semantic,
        3.. => SelectionType::Lines,
        _ => SelectionType::Simple,
    }
}

#[derive(Clone)]
struct PaintRect {
    line: usize,
    column: usize,
    cell_count: usize,
    color: gpui::Hsla,
}

impl PaintRect {
    fn paint(&self, origin: gpui::Point<Pixels>, cell_w: f32, cell_h: f32, window: &mut Window) {
        let position = point(
            origin.x + px(self.column as f32 * cell_w),
            origin.y + px(self.line as f32 * cell_h),
        );
        let rect_size = size(px(cell_w * self.cell_count as f32), px(cell_h));
        window.paint_quad(fill(Bounds::new(position, rect_size), self.color));
    }
}

#[derive(Clone)]
struct PaintTextRun {
    line: usize,
    column: usize,
    text: String,
    cell_count: usize,
    fg: gpui::Hsla,
    bold: bool,
    italic: bool,
}

impl PaintTextRun {
    fn new(line: usize, column: usize, cell: &RenderCell) -> Self {
        Self {
            line,
            column,
            text: cell.text.clone(),
            cell_count: cell.width,
            fg: cell.fg,
            bold: cell.bold,
            italic: cell.italic,
        }
    }

    fn can_append(&self, line: usize, column: usize, cell: &RenderCell) -> bool {
        self.line == line
            && self.column + self.cell_count == column
            && self.fg == cell.fg
            && self.bold == cell.bold
            && self.italic == cell.italic
    }

    fn append(&mut self, cell: &RenderCell) {
        self.text.push_str(&cell.text);
        self.cell_count += cell.width;
    }

    fn paint(
        &self,
        origin: gpui::Point<Pixels>,
        cell_w: f32,
        cell_h: f32,
        font_size_val: f32,
        window: &mut Window,
        cx: &mut App,
    ) {
        let position = point(
            origin.x + px(self.column as f32 * cell_w),
            origin.y + px(self.line as f32 * cell_h),
        );
        let font = terminal_font();
        let run = TextRun {
            len: self.text.len(),
            font: Font {
                family: font.family,
                features: FontFeatures::default(),
                fallbacks: font.fallbacks,
                weight: if self.bold {
                    GpuiFontWeight::BOLD
                } else {
                    GpuiFontWeight::NORMAL
                },
                style: if self.italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                },
            },
            color: self.fg,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(
            SharedString::from(self.text.clone()),
            px(font_size_val),
            &[run],
            Some(px(cell_w)),
        );
        let _ = shaped.paint(position, px(cell_h), window, cx);
    }
}

fn terminal_font() -> Font {
    Font {
        family: "Lilex Nerd Font Mono".into(),
        features: FontFeatures::default(),
        fallbacks: Some(FontFallbacks::from_fonts(vec![
            "MesloLGS Nerd Font Mono".into(),
            "Hack Nerd Font Mono".into(),
            "Symbols Nerd Font Mono".into(),
            "Symbols Nerd Font".into(),
            "JetBrains Mono".into(),
            "Menlo".into(),
        ])),
        weight: GpuiFontWeight::NORMAL,
        style: FontStyle::Normal,
    }
}

fn fit_cells(available: f32, cell: f32, min: u16) -> u16 {
    (((available / cell).next_up().floor()) as u16).max(min)
}

fn terminal_cell_metrics(font_size_val: f32, window: &mut Window) -> (f32, f32) {
    let font = terminal_font();
    let font_size = px(font_size_val);
    let text_system = window.text_system();
    let font_id = text_system.resolve_font(&font);
    let cell_w = text_system
        .advance(font_id, font_size, 'm')
        .map(|size| f32::from(size.width))
        .unwrap_or(8.4);
    let ascent = f32::from(text_system.ascent(font_id, font_size));
    let descent = f32::from(text_system.descent(font_id, font_size));
    let line_h = (font_size_val * 1.35).max(ascent + descent).max(18.0);
    (cell_w, line_h)
}

struct TerminalInputHost {
    child: AnyElement,
    focus_handle: gpui::FocusHandle,
    view: Entity<ThreeColumnApp>,
}

impl TerminalInputHost {
    fn new(
        child: impl IntoElement,
        focus_handle: gpui::FocusHandle,
        view: Entity<ThreeColumnApp>,
    ) -> Self {
        Self {
            child: child.into_any_element(),
            focus_handle,
            view,
        }
    }
}

impl IntoElement for TerminalInputHost {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TerminalInputHost {
    type RequestLayoutState = ();
    type PrepaintState = Bounds<Pixels>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.child.prepaint(window, cx);
        bounds
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        input_bounds: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
        window.handle_input(
            &self.focus_handle,
            ElementInputHandler::new(input_bounds.clone(), self.view.clone()),
            cx,
        );
    }
}

impl ThreeColumnApp {
    /// Generic bordered panel with a title strip and body text.
    pub fn panel(
        title: &'static str,
        body: &'static str,
        bg: gpui::Hsla,
        dark: bool,
    ) -> impl IntoElement {
        let title_col = if dark {
            hsla(0., 0., 0.85, 1.)
        } else {
            gpui::rgb(0x1a1a1a).into()
        };
        let body_col = if dark {
            hsla(0., 0., 0.55, 1.)
        } else {
            gpui::rgb(0x333333).into()
        };
        let border = if dark {
            gpui::white().opacity(0.06)
        } else {
            gpui::black().opacity(0.08)
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(bg)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(36.))
                    .px_3()
                    .border_b_1()
                    .border_color(border)
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col)
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .p_3()
                    .text_sm()
                    .text_color(body_col)
                    .child(body),
            )
    }

    /// Build the terminal tab bar + REAL terminal content for the active section.
    fn section_main_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Project page takes priority over terminal view.
        if let Some(ref project_id) = self.active_project_page.clone() {
            self.clear_terminal_viewport();
            return self.render_project_page(&project_id, window, cx);
        }

        let Some(ref section_id) = self.active_section.clone() else {
            self.clear_terminal_viewport();
            return div().flex().flex_col().size_full().bg(rgb(0x1e1f22)).child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(hsla(0., 0., 0.40, 1.))
                    .child("Select a branch to get started"),
            );
        };

        self.ensure_active_terminal_spawned(section_id);

        let tab_bar_bg = rgb(0x27292e);
        let tab_bg_active = rgb(0x1e1f22);
        let tab_bg_inactive = rgb(0x2b2d31);
        let tab_text_active = hsla(0., 0., 0.92, 1.);
        let tab_text_inactive = hsla(0., 0., 0.55, 1.);
        let tab_hover = rgb(0x2f3136);
        let close_col = hsla(0., 0., 0.45, 1.);
        let close_hover = hsla(0., 0., 0.80, 1.);
        let border_col = gpui::white().opacity(0.06);
        let plus_col = hsla(0., 0., 0.50, 1.);
        let terminal_bg = rgb(0x1e1f22);

        let sid_for_add = section_id.clone();

        // ── Tab bar ──────────────────────────────────────────────

        let mut tab_bar = div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(36.))
            .bg(tab_bar_bg)
            .border_b_1()
            .border_color(border_col)
            .overflow_hidden();

        let section_state = self.section_states.get(section_id);

        if let Some(state) = section_state {
            for (i, tab) in state.tabs.iter().enumerate() {
                let is_active = i == state.active_tab;
                let tab_title: SharedString = if state.tabs.len() > 1 {
                    format!("{} {}", tab.title, tab.id + 1).into()
                } else {
                    tab.title.clone().into()
                };

                let sid_click = section_id.clone();
                let tab_index = i;
                let sid_close = section_id.clone();
                let close_index = i;
                let can_close = state.tabs.len() > 1;
                let tab_id_val = tab.id;

                tab_bar = tab_bar.child(
                    div()
                        .id(SharedString::from(format!("tab-{}", tab_id_val)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .h_full()
                        .px(px(12.))
                        .cursor_pointer()
                        .bg(if is_active {
                            tab_bg_active
                        } else {
                            tab_bg_inactive
                        })
                        .hover(move |s| s.bg(if is_active { tab_bg_active } else { tab_hover }))
                        .tooltip(move |_window, cx| {
                            Self::action_tooltip_view("Switch to this terminal tab", cx)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                if let Some(state) = this.section_states.get_mut(&sid_click) {
                                    state.active_tab = tab_index;
                                }
                                this.ensure_active_terminal_spawned(&sid_click);
                                cx.notify();
                            }),
                        )
                        .child(
                            svg()
                                .path("assets/icons/icons__terminal.svg")
                                .size(px(14.))
                                .text_color(if is_active {
                                    tab_text_active
                                } else {
                                    tab_text_inactive
                                }),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(if is_active {
                                    tab_text_active
                                } else {
                                    tab_text_inactive
                                })
                                .child(tab_title),
                        )
                        .when(can_close, |d| {
                            d.child(
                                div()
                                    .id(SharedString::from(format!("tab-close-{}", tab_id_val)))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(18.))
                                    .h(px(18.))
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .text_color(close_col)
                                    .hover(move |s| {
                                        s.bg(gpui::white().opacity(0.08)).text_color(close_hover)
                                    })
                                    .tooltip(move |_window, cx| {
                                        Self::action_tooltip_view("Close this terminal tab", cx)
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, _ev: &MouseDownEvent, _window, cx| {
                                                cx.stop_propagation();
                                                if let Some(state) =
                                                    this.section_states.get_mut(&sid_close)
                                                {
                                                    state.close_tab(close_index);
                                                }
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .child(
                                        svg().path("assets/icons/icons__close.svg").size(px(12.)),
                                    ),
                            )
                        }),
                );
            }
        }

        // "+" add tab button.
        tab_bar = tab_bar.child(
            div()
                .id("add-terminal-tab")
                .flex()
                .items_center()
                .justify_center()
                .w(px(28.))
                .h(px(28.))
                .ml(px(4.))
                .rounded(px(5.))
                .cursor_pointer()
                .hover(move |s| s.bg(tab_hover))
                .tooltip(move |_window, cx| Self::action_tooltip_view("New terminal tab", cx))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                        if let Some(state) = this.section_states.get_mut(&sid_for_add) {
                            state.add_tab();
                        }
                        this.ensure_active_terminal_spawned(&sid_for_add);
                        cx.notify();
                    }),
                )
                .child(
                    svg()
                        .path("assets/icons/icons__plus.svg")
                        .size(px(14.))
                        .text_color(plus_col),
                ),
        );

        // ── Dynamic terminal resize based on available space ─────
        let (cell_w, cell_h) = terminal_cell_metrics(self.font_size, window);
        {
            let ww = f32::from(window.bounds().size.width);
            let wh = f32::from(window.bounds().size.height);
            let top_chrome_h = if cfg!(target_os = "macos") {
                TITLEBAR_CHROME_H
            } else {
                0.0
            };
            let main_w = (ww - self.sidebar_w - 2.0 * GUTTER - self.right_w).max(cell_w * 2.0);
            let main_h = (wh - top_chrome_h - FOOTER_H).max(cell_h);
            let grid_w = main_w;
            let grid_h = (main_h - 36.0).max(cell_h);
            let new_cols = fit_cells(grid_w, cell_w, 2);
            let new_rows = fit_cells(grid_h, cell_h, 1);

            if let Some(state) = self.section_states.get_mut(section_id) {
                if let Some(tab) = state.tabs.get_mut(state.active_tab) {
                    if let Some(ref mut terminal) = tab.terminal {
                        terminal.queue_resize(new_cols, new_rows);
                    }
                }
            }
        }

        // ── Terminal grid content ────────────────────────────────

        let terminal_content = self.render_terminal_grid(section_id, cell_w, cell_h, window, cx);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(terminal_bg)
            .child(tab_bar)
            .child(terminal_content)
    }

    /// Render the actual terminal grid from the alacritty Term, and handle keyboard input.
    fn render_terminal_grid(
        &mut self,
        section_id: &crate::app::SectionId,
        cell_w: f32,
        cell_h: f32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let terminal_bg = rgb(0x1e1f22);
        let default_fg = hsla(0., 0., 0.80, 1.);
        let cursor_color = hsla(0., 0., 0.92, 1.);
        let selection_bg = rgb(0x345b7d);
        let selection_fg = hsla(0., 0., 0.96, 1.);

        let section_state = self.section_states.get(section_id);
        let active_tab = section_state.and_then(|s| s.tabs.get(s.active_tab));
        let term_arc = active_tab
            .and_then(|t| t.terminal.as_ref())
            .map(|t| t.term.clone());

        let Some(term_arc) = term_arc else {
            self.clear_terminal_viewport();
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .bg(terminal_bg)
                .child(
                    div()
                        .text_sm()
                        .text_color(hsla(0., 0., 0.52, 1.))
                        .child("Starting terminal..."),
                )
                .into_any_element();
        };

        let mut cursor_pos: Option<(usize, usize)> = None;
        let mut num_cols = 80usize;
        let mut num_lines = 24usize;
        let mut rects = Vec::new();
        let mut text_runs = Vec::new();
        let mut cursor_cell_emitted = false;

        if let Ok(term) = term_arc.lock() {
            num_cols = term.columns();
            num_lines = term.screen_lines();

            let content = term.renderable_content();
            let display_offset = content.display_offset;
            let selection = content.selection;
            let cursor_shape = content.cursor.shape;
            let mut previous_line: Option<i32> = None;
            let mut previous_cell_had_extras = false;
            let mut current_rect: Option<PaintRect> = None;
            let mut current_run: Option<PaintTextRun> = None;

            let cursor = content.cursor.point;
            let cursor_line = cursor.line.0 + display_offset as i32;
            if cursor_line >= 0 && (cursor_line as usize) < num_lines {
                cursor_pos = Some((cursor_line as usize, cursor.column.0));
            }

            for indexed in content.display_iter {
                let point = indexed.point;
                let row_idx = point.line.0 + display_offset as i32;
                if row_idx < 0 || row_idx as usize >= num_lines {
                    continue;
                }
                let col_idx = point.column.0;
                if col_idx >= num_cols {
                    continue;
                }

                if previous_line != Some(point.line.0) {
                    previous_line = Some(point.line.0);
                    previous_cell_had_extras = false;
                }

                let cell = indexed.cell;
                if cell
                    .flags
                    .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }

                if cell.c == ' ' && previous_cell_had_extras {
                    previous_cell_had_extras = false;
                    continue;
                }
                previous_cell_had_extras =
                    matches!(cell.zerowidth(), Some(chars) if !chars.is_empty());

                let mut fg_color = cell.fg;
                let mut bg_color = cell.bg;
                if cell.flags.contains(CellFlags::INVERSE) {
                    std::mem::swap(&mut fg_color, &mut bg_color);
                }

                let fg = match fg_color {
                    AnsiColor::Named(NamedColor::Foreground) => default_fg,
                    other => ansi_color_to_hsla(other),
                };
                let bg = match bg_color {
                    AnsiColor::Named(NamedColor::Background) => gpui::transparent_black(),
                    other => ansi_color_to_hsla(other),
                };
                let bold = cell.flags.contains(CellFlags::BOLD);
                let italic = cell.flags.contains(CellFlags::ITALIC);
                let is_selected = selection.is_some_and(|selection| {
                    selection.contains_cell(&indexed, point, cursor_shape)
                });

                let mut text = String::new();
                text.push(if cell.c == '\0' { ' ' } else { cell.c });
                if let Some(chars) = cell.zerowidth() {
                    for ch in chars {
                        text.push(*ch);
                    }
                }

                let width = if cell.flags.contains(CellFlags::WIDE_CHAR) {
                    2
                } else {
                    1
                };
                let rendered_cell = RenderCell {
                    text,
                    fg,
                    bg,
                    bold,
                    italic,
                    width,
                };

                let line_idx = row_idx as usize;
                let is_cursor =
                    cell_covers_cursor(cursor_pos, line_idx, col_idx, rendered_cell.width);
                let mut effective_bg = rendered_cell.bg;
                let mut effective_fg = rendered_cell.fg;

                if is_selected {
                    effective_bg = selection_bg.into();
                    effective_fg = selection_fg;
                }

                if is_cursor {
                    effective_bg = cursor_color;
                    effective_fg = terminal_bg.into();
                }

                if is_cursor {
                    cursor_cell_emitted = true;
                }
                let is_blank = rendered_cell.text == " ";

                if effective_bg != gpui::transparent_black() {
                    match current_rect.as_mut() {
                        Some(rect)
                            if rect.line == line_idx
                                && rect.column + rect.cell_count == col_idx
                                && rect.color == effective_bg =>
                        {
                            rect.cell_count += rendered_cell.width;
                        }
                        Some(_) => {
                            rects.push(current_rect.take().unwrap());
                            current_rect = Some(PaintRect {
                                line: line_idx,
                                column: col_idx,
                                cell_count: rendered_cell.width,
                                color: effective_bg,
                            });
                        }
                        None => {
                            current_rect = Some(PaintRect {
                                line: line_idx,
                                column: col_idx,
                                cell_count: rendered_cell.width,
                                color: effective_bg,
                            });
                        }
                    }
                } else if let Some(rect) = current_rect.take() {
                    rects.push(rect);
                }

                if is_blank {
                    if let Some(run) = current_run.take() {
                        text_runs.push(run);
                    }
                    continue;
                }

                let effective_cell = RenderCell {
                    text: rendered_cell.text,
                    fg: effective_fg,
                    bg: effective_bg,
                    bold: rendered_cell.bold,
                    italic: rendered_cell.italic,
                    width: rendered_cell.width,
                };

                match current_run.as_mut() {
                    Some(run) if run.can_append(line_idx, col_idx, &effective_cell) => {
                        run.append(&effective_cell);
                    }
                    Some(_) => {
                        text_runs.push(current_run.take().unwrap());
                        current_run = Some(PaintTextRun::new(line_idx, col_idx, &effective_cell));
                    }
                    None => {
                        current_run = Some(PaintTextRun::new(line_idx, col_idx, &effective_cell));
                    }
                }

                if let Some(rect) = current_rect.take() {
                    rects.push(rect);
                }
                if let Some(run) = current_run.take() {
                    text_runs.push(run);
                }
            }
        }

        self.update_terminal_viewport(section_id, cell_w, cell_h, num_cols, num_lines);

        if let Some((cursor_line, cursor_col)) = cursor_pos.filter(|_| !cursor_cell_emitted) {
            rects.push(PaintRect {
                line: cursor_line,
                column: cursor_col,
                cell_count: 1,
                color: cursor_color,
            });
        }

        // Build grid element.
        let sid_for_mouse = section_id.clone();
        let sid_for_keys = section_id.clone();
        let sid_for_scroll = section_id.clone();

        // Calculate grid pixel dimensions for centering.
        let grid_pixel_w = cell_w * num_cols as f32;

        let mut grid_div = div()
            .id("terminal-grid")
            .flex_1()
            .min_h_0()
            .w_full()
            .bg(terminal_bg)
            .overflow_hidden()
            .flex()
            .flex_col()
            .track_focus(&self.focus_handle)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    this.focus_handle.focus(window);
                    Self::handle_terminal_mouse_down(this, &sid_for_mouse, ev, cx);
                }),
            )
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, _window, cx| {
                Self::handle_terminal_key(this, &sid_for_keys, ev, cx);
            }))
            // Scroll wheel → scroll the terminal scrollback buffer.
            .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, window, cx| {
                Self::handle_terminal_scroll(this, &sid_for_scroll, ev, window, cx);
            }));

        let canvas_w = grid_pixel_w + 16.0;
        let canvas_h = (cell_h * num_lines as f32).max(cell_h);
        let font_size_val = self.font_size;
        let terminal_canvas = canvas(
            move |_bounds, _window, _cx| (rects, text_runs),
            move |bounds, (rects, text_runs), window, cx| {
                let origin = point(bounds.origin.x + px(8.0), bounds.origin.y + px(6.0));
                for rect in rects {
                    rect.paint(origin, cell_w, cell_h, window);
                }
                for run in text_runs {
                    run.paint(origin, cell_w, cell_h, font_size_val, window, cx);
                }
            },
        )
        .w(px(canvas_w))
        .h(px(canvas_h))
        .flex_none();

        let inner = div()
            .w(px(grid_pixel_w))
            .flex_1()
            .min_h_0()
            .child(terminal_canvas);

        grid_div = grid_div.child(inner);

        // NOTE: refresh timer is started once in ThreeColumnApp::new()

        TerminalInputHost::new(grid_div, self.focus_handle.clone(), cx.entity().clone())
            .into_any_element()
    }

    fn handle_terminal_mouse_down(
        this: &mut Self,
        section_id: &crate::app::SectionId,
        ev: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some((viewport_section_id, row, col)) = this.terminal_cell_at_position(ev.position)
        else {
            return;
        };
        if &viewport_section_id != section_id {
            return;
        }

        let Some(state) = this.section_states.get(&viewport_section_id) else {
            return;
        };
        let Some(tab) = state.tabs.get(state.active_tab) else {
            return;
        };
        let Some(terminal) = tab.terminal.as_ref() else {
            return;
        };

        terminal.clear_selection();
        terminal.begin_selection(row, col, selection_type_for_click_count(ev.click_count));
        this.terminal_selection_drag = Some(crate::app::TerminalSelectionDrag {
            section_id: viewport_section_id,
        });
        cx.stop_propagation();
        cx.notify();
    }

    /// Handle keyboard input → write to PTY using Zed-style key mappings.
    fn handle_terminal_key(
        this: &mut Self,
        section_id: &crate::app::SectionId,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        if this.new_task_modal.is_some() {
            return;
        }

        if this.sidebar_task_rename.is_some() {
            return;
        }

        let ks = &ev.keystroke;
        let shortcut_key = ks.key.to_ascii_lowercase();

        let Some(state) = this.section_states.get(section_id) else {
            return;
        };
        let Some(tab) = state.tabs.get(state.active_tab) else {
            return;
        };
        let Some(ref terminal) = tab.terminal else {
            return;
        };

        if ks.modifiers.platform && !ks.modifiers.control && !ks.modifiers.alt {
            match shortcut_key.as_str() {
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        this.note_terminal_input_activity();
                        terminal.paste_text(&text);
                    }
                    cx.stop_propagation();
                    return;
                }
                "c" => {
                    if let Some(selection) = terminal.selection_text() {
                        cx.write_to_clipboard(ClipboardItem::new_string(selection));
                    } else {
                        this.note_terminal_input_activity();
                        terminal.write_to_pty(b"\x03");
                    }
                    cx.stop_propagation();
                    return;
                }
                "a" => {
                    terminal.select_all();
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                "backspace" => {
                    this.note_terminal_input_activity();
                    terminal.write_to_pty(b"\x15");
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }

        // Get the terminal mode for APP_CURSOR-aware key mapping.
        let term_mode = terminal.term.lock().map(|t| *t.mode()).unwrap_or_default();

        // Zed-style input split:
        // - special/control/navigation keys are handled on keydown
        // - normal text is delivered through the platform input handler / IME path
        let translated_esc = platform_ctrl_equivalent(ks)
            .and_then(|translated| crate::keys::to_esc_str(&translated, &term_mode));

        if let Some(esc) = crate::keys::to_esc_str(ks, &term_mode).or(translated_esc) {
            this.note_terminal_input_activity();
            terminal.write_to_pty(esc.as_bytes());
            cx.stop_propagation();
        }
    }

    /// Handle scroll wheel events on the terminal grid.
    fn handle_terminal_scroll(
        this: &mut Self,
        section_id: &crate::app::SectionId,
        ev: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (_, cell_h) = terminal_cell_metrics(this.font_size, window);
        let Some(state) = this.section_states.get_mut(section_id) else {
            return;
        };
        let Some(tab) = state.tabs.get_mut(state.active_tab) else {
            return;
        };
        let Some(ref mut terminal) = tab.terminal else {
            return;
        };

        terminal.scroll_wheel(ev, cell_h);
        cx.stop_propagation();
        cx.notify();
    }

    /// Assemble the three-column main row (sidebar | gutter | main | gutter | right).
    pub fn main_row(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        sw: f32,
        rw: f32,
        open: bool,
        busy: bool,
    ) -> impl IntoElement {
        let section_panel = self.section_main_panel(window, cx);
        let right_open = self.right_sidebar_is_open();

        div()
            .relative()
            .flex()
            .flex_row()
            .flex_1()
            .min_h_0()
            .size_full()
            .bg(rgb(0x27292e))
            // --- Sidebar column ---
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(sw))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .when(cfg!(not(target_os = "macos")), |col| {
                        col.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .h(px(SIDEBAR_TOOLBAR_H))
                                .pl(px(TRAFFIC_LIGHT_PAD))
                                .pr_2()
                                .border_b_1()
                                .border_color(gpui::black().opacity(0.06))
                                .bg(rgb(0xfafafa))
                                .child(
                                    div()
                                        .id("sidebar-toggle")
                                        .cursor_pointer()
                                        .when(!busy, |d| {
                                            d.tooltip(move |_window, cx| {
                                                Self::action_tooltip_view(
                                                    "Show or hide the projects sidebar",
                                                    cx,
                                                )
                                            })
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(Self::titlebar_toggle_mouse),
                                            )
                                        })
                                        .when(busy, |d| d.opacity(0.45))
                                        .hover(|s| s.bg(rgb(0xe4e4e7)))
                                        .rounded_md()
                                        .child(Self::sidebar_toggle_svg(window)),
                                ),
                        )
                    })
                    .child({
                        let content = div().flex_1().min_h_0();
                        if open {
                            content.child(self.sidebar_content(window, cx))
                        } else {
                            content
                        }
                    }),
            )
            // --- Left gutter ---
            .child(
                div()
                    .id("gutter-left")
                    .w(px(GUTTER))
                    .flex_shrink_0()
                    .h_full()
                    .cursor_col_resize()
                    .hover(|s| s.bg(gpui::blue().opacity(0.15)))
                    .tooltip(move |_window, cx| {
                        Self::action_tooltip_view("Drag to resize the projects sidebar", cx)
                    })
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::left_gutter_down)),
            )
            // --- Main centre panel ---
            .child(
                div()
                    .flex_1()
                    .min_w(px(MIN_MAIN))
                    .min_h_0()
                    .bg(rgb(0x1e1f22))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .child(section_panel),
            )
            // --- Right gutter ---
            .child(
                div()
                    .id("gutter-right")
                    .w(px(GUTTER))
                    .flex_shrink_0()
                    .h_full()
                    .cursor_col_resize()
                    .hover(|s| s.bg(gpui::blue().opacity(0.15)))
                    .tooltip(move |_window, cx| {
                        Self::action_tooltip_view("Drag to resize the changed files sidebar", cx)
                    })
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::right_gutter_down)),
            )
            // --- Right panel ---
            .child(
                div()
                    .w(px(rw))
                    .flex_shrink_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child({
                        let content = div().flex_1().min_h_0().size_full();
                        if right_open {
                            content.child(self.changed_files_panel(window, cx))
                        } else {
                            content
                        }
                    }),
            )
            .child(self.project_menu_overlay(sw, cx))
    }
}
