// MCP settings sub-page — port of
// `desktop/src/mcp_page.rs::settings_mcp_content`.
//
// Lists every catalog entry: when in the registry the row renders
// with per-provider toggles + a Remove button; otherwise as an
// "Add" prompt that copies the catalog default into the registry
// on click. Custom entries (not from the catalog) follow.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../rust/api/local_session.dart'
    show
        McpCatalogEntryDto,
        McpServerDto,
        McpSourceDto,
        McpSettingsView,
        McpTransportKindDto;
import '../../../state/local_connection_provider.dart';
import '../../../tokens.dart';
import '../../../widgets/app_toast.dart';
import 'settings_async_state.dart';

final _mcpSettingsProvider = FutureProvider.autoDispose<McpSettingsView?>((
  ref,
) async {
  final connection = ref.watch(localConnectionProvider);
  await waitForConnectedDaemon(connection);
  try {
    return await connection.readMcpSettings();
  } on UnimplementedError {
    return null;
  }
});

class SettingsMcpSection extends ConsumerStatefulWidget {
  const SettingsMcpSection({super.key});

  @override
  ConsumerState<SettingsMcpSection> createState() => _SettingsMcpSectionState();
}

class _SettingsMcpSectionState extends ConsumerState<SettingsMcpSection> {
  McpSettingsView? _providerView;
  McpSettingsView? _currentView;

  static const Color _panelBg = Color(0xFF23252A);
  static const Color _rowBg = Color(0xFF1F2125);
  static const Color _activeBg = Color(0xFF2E67B8);
  static const Color _danger = Color(0xFFCC4646);

  // Mirrors `desktop/src/mcp_page.rs::MCP_PROVIDERS`.
  static const List<({String id, String short})> _providers = [
    (id: 'claude-code', short: 'Claude'),
    (id: 'cursor-agent', short: 'Cursor'),
    (id: 'codex', short: 'Codex'),
    (id: 'gemini', short: 'Gemini'),
    (id: 'opencode', short: 'OpenCode'),
    (id: 'amp', short: 'Amp'),
  ];

  void _syncFromProvider(McpSettingsView view) {
    if (identical(_providerView, view)) {
      return;
    }
    _providerView = view;
    _currentView = view;
  }

  void _updateView(McpSettingsView Function(McpSettingsView current) update) {
    final current = _currentView;
    if (current == null) {
      return;
    }
    setState(() {
      _currentView = update(current);
    });
  }

  McpTransportKindDto _catalogTransportKind(String catalogId) {
    return switch (catalogId) {
      'playwright' || 'github' => McpTransportKindDto.stdio,
      _ => McpTransportKindDto.http,
    };
  }

  void _handleCatalogAdded(McpCatalogEntryDto entry) {
    _updateView(
      (current) => McpSettingsView(
        catalogEntries: current.catalogEntries,
        registryEntries: [
          ...current.registryEntries,
          McpServerDto(
            id: entry.id,
            label: entry.label,
            source: McpSourceDto.catalog,
            transportKind: _catalogTransportKind(entry.id),
            enabledFor: const [],
          ),
        ],
        syncErrorProviderIds: current.syncErrorProviderIds,
      ),
    );
  }

  void _handleToggle({
    required String entryId,
    required String providerId,
    required bool enabled,
  }) {
    _updateView((current) {
      final nextEntries = current.registryEntries
          .map((entry) {
            if (entry.id != entryId) {
              return entry;
            }
            final enabledFor = [...entry.enabledFor];
            if (enabled) {
              if (!enabledFor.contains(providerId)) {
                enabledFor.add(providerId);
              }
            } else {
              enabledFor.remove(providerId);
            }
            return McpServerDto(
              id: entry.id,
              label: entry.label,
              source: entry.source,
              transportKind: entry.transportKind,
              enabledFor: enabledFor,
            );
          })
          .toList(growable: false);
      return McpSettingsView(
        catalogEntries: current.catalogEntries,
        registryEntries: nextEntries,
        syncErrorProviderIds: const [],
      );
    });
  }

  void _handleRemoved(String entryId) {
    _updateView(
      (current) => McpSettingsView(
        catalogEntries: current.catalogEntries,
        registryEntries: current.registryEntries
            .where((entry) => entry.id != entryId)
            .toList(growable: false),
        syncErrorProviderIds: const [],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final viewAsync = ref.watch(_mcpSettingsProvider);
    final body = viewAsync.when<Widget>(
      data: (view) {
        if (view == null) {
          return ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 860),
            child: const SettingsSectionStatePanel(
              panelBg: _panelBg,
              title: 'Not available on this connection',
              message: 'This daemon does not expose MCP settings yet.',
            ),
          );
        }
        return ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 860),
          child: Container(
            decoration: BoxDecoration(
              color: _panelBg,
              borderRadius: BorderRadius.circular(10),
              border: Border.all(color: AppTokens.border),
            ),
            clipBehavior: Clip.antiAlias,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                ...() {
                  _syncFromProvider(view);
                  return _buildRows(_currentView!);
                }(),
                const _FooterNote(),
              ],
            ),
          ),
        );
      },
      error: (error, _) => ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 860),
        child: SettingsSectionStatePanel(
          panelBg: _panelBg,
          title: 'Could not load MCP settings',
          message: '$error',
          error: true,
        ),
      ),
      loading: SettingsSectionLoading.new,
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text(
          'MCP',
          style: TextStyle(
            fontSize: 18,
            fontWeight: FontWeight.w600,
            color: AppTokens.textPrimary,
          ),
        ),
        const SizedBox(height: 4),
        ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 760),
          child: const Text(
            "Toggle MCP servers on per harness. Toggling syncs the registry into each agent's native config (preserving entries AnotherOne doesn't own).",
            style: TextStyle(
              fontSize: 12,
              height: 1.5,
              color: AppTokens.textSecondary,
            ),
          ),
        ),
        const SizedBox(height: 24),
        body,
      ],
    );
  }

  List<Widget> _buildRows(McpSettingsView view) {
    final rows = <Widget>[];
    rows.add(const _HeaderRow());
    final registryById = {
      for (final entry in view.registryEntries) entry.id: entry,
    };
    var index = 0;
    for (final catalog in view.catalogEntries) {
      final inRegistry = registryById[catalog.id];
      if (inRegistry != null) {
        rows.add(
          _RegistryRow(
            server: inRegistry,
            isFirst: index == 0,
            providers: _providers,
            syncErrors: view.syncErrorProviderIds,
            rowBg: _rowBg,
            activeBg: _activeBg,
            danger: _danger,
            onToggled: (providerId, enabled) => _handleToggle(
              entryId: inRegistry.id,
              providerId: providerId,
              enabled: enabled,
            ),
            onRemoved: () => _handleRemoved(inRegistry.id),
            onReload: () => ref.invalidate(_mcpSettingsProvider),
          ),
        );
      } else {
        rows.add(
          _CatalogPromptRow(
            entry: catalog,
            isFirst: index == 0,
            rowBg: _rowBg,
            onAdded: () => _handleCatalogAdded(catalog),
          ),
        );
      }
      index++;
    }
    final customs = view.registryEntries.where(
      (e) => e.source != McpSourceDto.catalog,
    );
    for (final custom in customs) {
      rows.add(
        _RegistryRow(
          server: custom,
          isFirst: index == 0,
          providers: _providers,
          syncErrors: view.syncErrorProviderIds,
          rowBg: _rowBg,
          activeBg: _activeBg,
          danger: _danger,
          onToggled: (providerId, enabled) => _handleToggle(
            entryId: custom.id,
            providerId: providerId,
            enabled: enabled,
          ),
          onRemoved: () => _handleRemoved(custom.id),
          onReload: () => ref.invalidate(_mcpSettingsProvider),
        ),
      );
      index++;
    }
    return rows;
  }
}

class _HeaderRow extends StatelessWidget {
  const _HeaderRow();

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 12),
      child: const Text(
        "Toggle MCP servers on per harness. Toggling syncs the registry into each agent's native config (preserving entries AnotherOne doesn't own).",
        style: TextStyle(fontSize: 12, color: AppTokens.textSecondary),
      ),
    );
  }
}

class _CatalogPromptRow extends ConsumerStatefulWidget {
  const _CatalogPromptRow({
    required this.entry,
    required this.isFirst,
    required this.rowBg,
    required this.onAdded,
  });

  final McpCatalogEntryDto entry;
  final bool isFirst;
  final Color rowBg;
  final VoidCallback onAdded;

  @override
  ConsumerState<_CatalogPromptRow> createState() => _CatalogPromptRowState();
}

class _CatalogPromptRowState extends ConsumerState<_CatalogPromptRow> {
  bool _busy = false;

  Future<void> _add() async {
    if (_busy) return;
    setState(() => _busy = true);
    try {
      await ref
          .read(localConnectionProvider)
          .mcpAddFromCatalog(widget.entry.id);
      widget.onAdded();
    } catch (e) {
      if (!mounted) return;
      showAppToast(context, message: 'Could not add MCP entry: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: BoxDecoration(
        color: widget.rowBg,
        border: widget.isFirst
            ? null
            : const Border(top: BorderSide(color: AppTokens.border)),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 14),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  widget.entry.label,
                  style: const TextStyle(
                    fontSize: 13,
                    color: AppTokens.textPrimary,
                  ),
                ),
                const SizedBox(height: 2),
                Text(
                  widget.entry.description,
                  style: const TextStyle(
                    fontSize: 12,
                    color: AppTokens.textSecondary,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(width: 16),
          InkWell(
            borderRadius: BorderRadius.circular(6),
            onTap: _busy ? null : _add,
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
              decoration: BoxDecoration(
                color: AppTokens.overlayHover,
                borderRadius: BorderRadius.circular(6),
              ),
              child: _busy
                  ? const SizedBox(
                      width: 12,
                      height: 12,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        valueColor: AlwaysStoppedAnimation<Color>(
                          AppTokens.textPrimary,
                        ),
                      ),
                    )
                  : const Text(
                      'Add',
                      style: TextStyle(
                        fontSize: 12,
                        color: AppTokens.textPrimary,
                      ),
                    ),
            ),
          ),
        ],
      ),
    );
  }
}

class _RegistryRow extends ConsumerStatefulWidget {
  const _RegistryRow({
    required this.server,
    required this.isFirst,
    required this.providers,
    required this.syncErrors,
    required this.rowBg,
    required this.activeBg,
    required this.danger,
    required this.onToggled,
    required this.onRemoved,
    required this.onReload,
  });

  final McpServerDto server;
  final bool isFirst;
  final List<({String id, String short})> providers;
  final List<String> syncErrors;
  final Color rowBg;
  final Color activeBg;
  final Color danger;
  final void Function(String providerId, bool enabled) onToggled;
  final VoidCallback onRemoved;
  final VoidCallback onReload;

  @override
  ConsumerState<_RegistryRow> createState() => _RegistryRowState();
}

class _RegistryRowState extends ConsumerState<_RegistryRow> {
  bool _busy = false;

  Future<void> _toggle(String providerId, bool enabled) async {
    if (_busy) return;
    setState(() => _busy = true);
    try {
      await ref
          .read(localConnectionProvider)
          .mcpToggle(
            entryId: widget.server.id,
            providerId: providerId,
            enabled: enabled,
          );
      widget.onToggled(providerId, enabled);
    } catch (e) {
      if (!mounted) return;
      widget.onReload();
      showAppToast(context, message: 'Could not toggle: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _remove() async {
    if (_busy) return;
    setState(() => _busy = true);
    try {
      await ref.read(localConnectionProvider).mcpRemove(widget.server.id);
      widget.onRemoved();
    } catch (e) {
      if (!mounted) return;
      widget.onReload();
      showAppToast(context, message: 'Could not remove: $e');
    }
    if (mounted) setState(() => _busy = false);
  }

  String _sourceLabel(McpSourceDto source) {
    return switch (source) {
      McpSourceDto.catalog => 'catalog',
      McpSourceDto.custom => 'custom',
      McpSourceDto.builtInDaemon => 'daemon',
    };
  }

  @override
  Widget build(BuildContext context) {
    final server = widget.server;
    final canRemove = server.source != McpSourceDto.builtInDaemon;
    return Container(
      decoration: BoxDecoration(
        color: widget.rowBg,
        border: widget.isFirst
            ? null
            : const Border(top: BorderSide(color: AppTokens.border)),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 14),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  server.label,
                  style: const TextStyle(
                    fontSize: 13,
                    color: AppTokens.textPrimary,
                  ),
                ),
                const SizedBox(height: 2),
                Text(
                  '${_sourceLabel(server.source)}  ·  ${server.id}',
                  style: const TextStyle(
                    fontSize: 11,
                    color: AppTokens.textSecondary,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(width: 12),
          Wrap(
            spacing: 6,
            children: [
              for (final p in widget.providers)
                _ProviderToggle(
                  label: p.short,
                  providerId: p.id,
                  enabled: server.enabledFor.contains(p.id),
                  errored: widget.syncErrors.contains(p.id),
                  // Codex is stdio-only — gate Codex off for HTTP
                  // entries. Mirrors the GPUI logic.
                  unsupported:
                      p.id == 'codex' &&
                      server.transportKind == McpTransportKindDto.http,
                  activeBg: widget.activeBg,
                  danger: widget.danger,
                  onTap: (enabled) => _toggle(p.id, enabled),
                ),
            ],
          ),
          if (canRemove) ...[
            const SizedBox(width: 12),
            InkWell(
              borderRadius: BorderRadius.circular(5),
              onTap: _busy ? null : _remove,
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                decoration: BoxDecoration(
                  color: AppTokens.overlayRest,
                  borderRadius: BorderRadius.circular(5),
                ),
                child: const Text(
                  'Remove',
                  style: TextStyle(
                    fontSize: 11,
                    color: AppTokens.textSecondary,
                  ),
                ),
              ),
            ),
          ],
        ],
      ),
    );
  }
}

class _ProviderToggle extends StatelessWidget {
  const _ProviderToggle({
    required this.label,
    required this.providerId,
    required this.enabled,
    required this.errored,
    required this.unsupported,
    required this.activeBg,
    required this.danger,
    required this.onTap,
  });

  final String label;
  final String providerId;
  final bool enabled;
  final bool errored;
  final bool unsupported;
  final Color activeBg;
  final Color danger;
  final ValueChanged<bool> onTap;

  @override
  Widget build(BuildContext context) {
    final Color bg;
    if (unsupported) {
      bg = const Color(0x08FFFFFF);
    } else if (errored) {
      bg = danger;
    } else if (enabled) {
      bg = activeBg;
    } else {
      bg = AppTokens.overlayHover;
    }
    final textColor = unsupported
        ? AppTokens.textSecondary
        : AppTokens.textPrimary;
    final inner = Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(5),
      ),
      child: Text(label, style: TextStyle(fontSize: 11, color: textColor)),
    );
    if (unsupported) {
      return Tooltip(
        message: '$label only supports stdio transports today.',
        child: inner,
      );
    }
    return InkWell(
      borderRadius: BorderRadius.circular(5),
      onTap: () => onTap(!enabled),
      child: inner,
    );
  }
}

class _FooterNote extends StatelessWidget {
  const _FooterNote();

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 12),
      decoration: const BoxDecoration(
        border: Border(top: BorderSide(color: AppTokens.border)),
      ),
      child: const Text(
        'Custom transports, env, and headers: edit ~/.config/another-one/mcp.json. Inline editor is a follow-up.',
        style: TextStyle(fontSize: 11, color: AppTokens.textSecondary),
      ),
    );
  }
}
