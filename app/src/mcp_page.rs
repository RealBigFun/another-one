//! MCP registry page — rendered as a section of the settings page.
//!
//! Layout: one row per catalog entry + one row per custom registry
//! entry. Each row shows a per-provider toggle for every harness
//! that declares `supports_mcp_client()`. Toggling fires a sync_all
//! pass; per-provider adapter errors surface as toasts (following
//! the AGENTS.md UI rule).
//!
//! Intentionally minimal: there's no inline entry editor in this
//! first cut. Catalog entries copy into the registry on first "Add";
//! custom entries are added via the "Custom entry (JSON)" form at
//! the bottom of the page. Deeper editing (transport, env, headers)
//! is file-only today — users edit
//! `{config_dir}/another-one/mcp.json` directly. Inline editor is
//! tracked as a follow-up.

use gpui::{
    div, hsla, prelude::*, px, rems, AnyElement, Context, IntoElement, MouseButton, MouseDownEvent,
};

use another_one_core::agents::AgentProviderKind;
use another_one_core::mcp::{catalog, McpServer, McpSource, McpTransport};

use crate::app::AnotherOneApp;
use crate::project_store::ThemeMode;

fn mcp_theme(mode: ThemeMode) -> crate::theme::AppTheme {
    crate::theme::app_theme_for_preference(mode)
}

/// Providers with `supports_mcp_client() == true`. Kept in sync
/// manually with `AgentHarness::supports_mcp_client` — a future
/// capability-registry refactor (#37) should collapse this.
const MCP_PROVIDERS: &[(AgentProviderKind, &str)] = &[
    (AgentProviderKind::ClaudeCode, "Claude"),
    (AgentProviderKind::CursorAgent, "Cursor"),
    (AgentProviderKind::Codex, "Codex"),
    (AgentProviderKind::Gemini, "Gemini"),
    (AgentProviderKind::OpenCode, "OpenCode"),
    (AgentProviderKind::Amp, "Amp"),
];

impl AnotherOneApp {
    pub(crate) fn settings_mcp_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        let mode = self.project_store.ui.theme_mode;
        let theme = mcp_theme(mode);
        let panel_bg = theme.card_bg;
        let row_bg = theme.sunken_bg;

        let mut rows = div().flex().flex_col();
        rows = rows.child(mcp_header_row(mode));

        // Catalog entries first. If already in the registry, render
        // as a registry row (with toggles); otherwise render as an
        // "Add" prompt that copies the catalog default into the
        // registry.
        for (index, entry) in catalog::entries().iter().enumerate() {
            let in_registry = self.mcp_registry.entries.iter().any(|e| e.id == entry.id);
            if in_registry {
                let server = self
                    .mcp_registry
                    .entries
                    .iter()
                    .find(|e| e.id == entry.id)
                    .expect("just checked");
                rows = rows.child(self.render_mcp_registry_row(server, row_bg, index, cx));
            } else {
                rows = rows.child(
                    self.render_mcp_catalog_prompt_row(entry, row_bg, index, cx)
                        .into_any_element(),
                );
            };
        }

        // Custom entries (anything in the registry whose source
        // isn't Catalog — catalog rows above already cover those).
        for (index, server) in self
            .mcp_registry
            .entries
            .iter()
            .filter(|e| !matches!(e.source, McpSource::Catalog))
            .enumerate()
        {
            let row =
                self.render_mcp_registry_row(server, row_bg, index + catalog::entries().len(), cx);
            rows = rows.child(row);
        }

        div()
            .flex()
            .flex_col()
            .bg(panel_bg)
            .rounded(px(10.))
            .overflow_hidden()
            .child(rows)
            .child(mcp_footer_note(mode))
    }

    fn render_mcp_catalog_prompt_row(
        &self,
        entry: &'static catalog::CatalogEntry,
        bg: gpui::Hsla,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mode = self.project_store.ui.theme_mode;
        let theme = mcp_theme(mode);
        let mut row = div()
            .id(("mcp-row-catalog", index))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(16.))
            .px(px(18.))
            .py(px(14.))
            .bg(bg);
        if index > 0 {
            row = row.border_t_1().border_color(theme.border);
        }
        let label = entry.label;
        let description = entry.description;
        let catalog_id = entry.id;
        row.child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .child(
                    div()
                        .text_size(rems(13. / 16.))
                        .text_color(theme.text_primary)
                        .child(label),
                )
                .child(
                    div()
                        .text_size(rems(12. / 16.))
                        .text_color(theme.text_secondary)
                        .child(description),
                ),
        )
        .child(
            div()
                .id(("mcp-add", index))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(6.))
                .bg(theme.overlay_hover)
                .cursor_pointer()
                .hover(|s| s.bg(theme.overlay_active))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                        this.mcp_add_from_catalog(catalog_id, cx);
                    }),
                )
                .child(
                    div()
                        .text_size(rems(12. / 16.))
                        .text_color(theme.text_primary)
                        .child("Add"),
                ),
        )
    }

    fn render_mcp_registry_row(
        &self,
        server: &McpServer,
        bg: gpui::Hsla,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mode = self.project_store.ui.theme_mode;
        let theme = mcp_theme(mode);
        let id = server.id.clone();
        let label = server.label.clone();
        let source_label = match server.source {
            McpSource::Catalog => "catalog",
            McpSource::Custom => "custom",
            McpSource::BuiltInDaemon => "daemon",
        };

        let mut row = div()
            .id(("mcp-row-reg", index))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .px(px(18.))
            .py(px(14.))
            .bg(bg);
        if index > 0 {
            row = row.border_t_1().border_color(theme.border);
        }

        let mut toggles = div().flex().flex_row().gap(px(6.));
        for (pindex, (provider, short)) in MCP_PROVIDERS.iter().enumerate() {
            let is_on = server.enabled_for.contains(provider);
            let provider = *provider;
            let toggle_id = id.clone();
            let provider_errored = self.mcp_last_sync_errors.contains(&provider);
            // Codex is stdio-only. Rather than letting the user
            // flip a toggle that silently does nothing, gate it
            // off entirely for HTTP entries.
            let unsupported_transport = matches!(provider, AgentProviderKind::Codex)
                && matches!(server.transport, McpTransport::Http { .. });

            let bg_color = if unsupported_transport {
                theme.overlay_rest
            } else if provider_errored {
                hsla(0. / 360., 0.70, 0.40, 1.)
            } else if is_on {
                hsla(215. / 360., 0.60, 0.45, 1.)
            } else {
                theme.overlay_hover
            };
            let hover_bg = if unsupported_transport {
                theme.overlay_rest
            } else if is_on {
                hsla(215. / 360., 0.60, 0.55, 1.)
            } else {
                theme.overlay_active
            };
            let text_color = if unsupported_transport {
                theme.text_secondary
            } else if provider_errored || is_on {
                gpui::white()
            } else {
                theme.text_primary
            };
            let mut cell = div()
                .id(("mcp-toggle", index * 16 + pindex))
                .px(px(8.))
                .py(px(4.))
                .rounded(px(5.))
                .bg(bg_color);
            if !unsupported_transport {
                cell = cell
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            this.mcp_toggle(&toggle_id, provider, cx);
                        }),
                    );
            }
            toggles = toggles.child(
                cell.child(
                    div()
                        .text_size(rems(11. / 16.))
                        .text_color(text_color)
                        .child(*short),
                ),
            );
        }

        let remove_id = id.clone();
        let can_remove = !matches!(server.source, McpSource::BuiltInDaemon);
        row.child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .child(
                    div()
                        .text_size(rems(13. / 16.))
                        .text_color(theme.text_primary)
                        .child(label),
                )
                .child(
                    div()
                        .text_size(rems(11. / 16.))
                        .text_color(theme.text_secondary)
                        .child(format!("{}  ·  {}", source_label, id)),
                ),
        )
        .child(toggles)
        .when(can_remove, move |row| {
            let remove_id = remove_id.clone();
            row.child(
                div()
                    .id(("mcp-remove", index))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(5.))
                    .bg(theme.overlay_rest)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.overlay_active))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            this.mcp_remove(&remove_id, cx);
                        }),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .text_color(theme.text_secondary)
                            .child("Remove"),
                    ),
            )
        })
        .into_any_element()
    }

    // ---- Registry mutations ----

    pub(crate) fn mcp_add_from_catalog(
        &mut self,
        catalog_id: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = catalog::find(catalog_id) else {
            return;
        };
        self.mcp_registry.upsert(catalog::instantiate(entry));
        self.persist_mcp_registry();
        cx.notify();
    }

    pub(crate) fn mcp_toggle(
        &mut self,
        id: &str,
        provider: AgentProviderKind,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.mcp_registry.entries.iter().find(|e| e.id == id) else {
            return;
        };
        let enabled = !entry.enabled_for.contains(&provider);
        if !self.mcp_registry.toggle(id, provider, enabled) {
            return;
        }
        self.mcp_sync_and_persist(cx);
        cx.notify();
    }

    pub(crate) fn mcp_remove(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.mcp_registry.remove(id) {
            return;
        }
        self.mcp_sync_and_persist(cx);
        cx.notify();
    }

    fn mcp_sync_and_persist(&mut self, cx: &mut Context<Self>) {
        let report = self.mcp_registry.sync_all();
        self.mcp_last_sync_errors.clear();
        for (provider, result) in report {
            if let Err(err) = result {
                self.mcp_last_sync_errors.insert(provider);
                self.show_error_toast(format!("MCP sync failed for {:?}: {err}", provider), cx);
            }
        }
        self.persist_mcp_registry();
    }

    fn persist_mcp_registry(&self) {
        if let Err(err) = self.mcp_registry.save() {
            log::warn!("failed to persist MCP registry: {err}");
        }
    }
}

fn mcp_header_row(mode: ThemeMode) -> impl IntoElement {
    let theme = mcp_theme(mode);
    div()
        .flex()
        .flex_row()
        .items_center()
        .px(px(18.))
        .py(px(12.))
        .child(
            div()
                .text_size(rems(12. / 16.))
                .text_color(theme.text_secondary)
                .child(
                    "Toggle MCP servers on per harness. Toggling syncs the registry into each \
                     agent's native config (preserving entries AnotherOne doesn't own).",
                ),
        )
}

fn mcp_footer_note(mode: ThemeMode) -> impl IntoElement {
    let theme = mcp_theme(mode);
    div()
        .px(px(18.))
        .py(px(12.))
        .border_t_1()
        .border_color(theme.border)
        .child(
            div()
                .text_size(rems(11. / 16.))
                .text_color(theme.text_secondary)
                .child(
                    "Custom transports, env, and headers: edit \
                     ~/.config/another-one/mcp.json. Inline editor is a follow-up.",
                ),
        )
}
