// Per-task screen. Full-screen scaffold with a tab strip across the
// top and an xterm.dart Terminal below, bound to the
// currently-attached tab on the daemon.
//
// Tab switching is a detach→attach pair on the wire. The local
// Terminal instance is recreated per switch: cheap and avoids state
// leaking between tabs. Per-tab scrollback preservation is explicitly
// deferred (acknowledged in the spec).

import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:xterm/xterm.dart';

import 'rust/api/iroh_client.dart';
import 'tokens.dart';
import 'transport_iroh.dart';

class TaskPage extends StatefulWidget {
  const TaskPage({
    super.key,
    required this.transport,
    required this.project,
    required this.task,
  });

  final IrohTransport transport;
  final ProjectSummary project;
  final TaskSummary task;

  @override
  State<TaskPage> createState() => _TaskPageState();
}

class _TaskPageState extends State<TaskPage> {
  /// Id of the tab whose PTY is currently attached to this session.
  late String _activeTabId;

  /// Rebuilt whenever `_activeTabId` changes. We re-key the terminal
  /// widget on this so xterm state is wiped cleanly on tab switch.
  Terminal _terminal = Terminal(maxLines: 10000);
  final TerminalController _terminalController = TerminalController();
  final FocusNode _terminalFocus = FocusNode();

  StreamSubscription<Uint8List>? _bytesSub;
  int _instanceKey = 0;

  @override
  void initState() {
    super.initState();
    _activeTabId = widget.task.activeTabId;
    _wireTerminal();
    _bytesSub = widget.transport.incoming.listen((bytes) {
      _terminal.write(utf8.decode(bytes, allowMalformed: true));
    });
    // Initial attach + size sync.
    unawaited(
      widget.transport.attachTab(
        sectionId: widget.task.sectionId,
        tabId: _activeTabId,
      ),
    );
    unawaited(
      widget.transport.tabResize(
        cols: _terminal.viewWidth,
        rows: _terminal.viewHeight,
      ),
    );
  }

  void _wireTerminal() {
    _terminal.onOutput = (data) {
      widget.transport.sendBytes(utf8.encode(data));
    };
    _terminal.onResize = (w, h, _, _) {
      widget.transport.tabResize(cols: w, rows: h);
    };
  }

  Future<void> _switchTab(TabSummary tab) async {
    if (tab.id == _activeTabId) return;
    // Wipe Dart-side state first so no bytes from the previous tab
    // sneak in after detach/attach.
    await _bytesSub?.cancel();
    await widget.transport.detachTab();

    setState(() {
      _activeTabId = tab.id;
      _instanceKey++;
      _terminal = Terminal(maxLines: 10000);
      _wireTerminal();
    });

    _bytesSub = widget.transport.incoming.listen((bytes) {
      _terminal.write(utf8.decode(bytes, allowMalformed: true));
    });

    await widget.transport.attachTab(
      sectionId: widget.task.sectionId,
      tabId: tab.id,
    );
    await widget.transport.tabResize(
      cols: _terminal.viewWidth,
      rows: _terminal.viewHeight,
    );
    _terminalFocus.requestFocus();
  }

  @override
  void dispose() {
    _bytesSub?.cancel();
    // Best-effort detach — if the session's already closed this is a
    // no-op on the daemon side.
    unawaited(widget.transport.detachTab());
    _terminalFocus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              widget.task.name,
              style: const TextStyle(
                fontSize: AppTokens.fontHeadingSm,
                fontWeight: FontWeight.w600,
              ),
            ),
            Text(
              '⎇ ${widget.task.branchName}',
              style: const TextStyle(
                fontSize: AppTokens.fontSmall,
                fontFamily: AppTokens.fontFamilyMono,
                color: AppTokens.textMuted,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ],
        ),
      ),
      body: SafeArea(
        child: Column(
          children: [
            _TabStrip(
              tabs: widget.task.tabs,
              activeTabId: _activeTabId,
              onTap: _switchTab,
            ),
            Expanded(
              child: Container(
                color: AppTokens.terminalBg,
                child: GestureDetector(
                  onTap: _terminalFocus.requestFocus,
                  child: TerminalView(
                    _terminal,
                    key: ValueKey(_instanceKey),
                    controller: _terminalController,
                    focusNode: _terminalFocus,
                    autofocus: true,
                    backgroundOpacity: 1.0,
                    padding: const EdgeInsets.all(AppTokens.space2),
                    textStyle: const TerminalStyle(
                      fontFamily: AppTokens.fontFamilyMono,
                      fontSize: AppTokens.fontBody,
                    ),
                  ),
                ),
              ),
            ),
            _ChordBar(
              onSend: (bytes) {
                widget.transport.sendBytes(bytes);
                _terminalFocus.requestFocus();
              },
            ),
          ],
        ),
      ),
    );
  }
}

/// Horizontal, scrollable tab strip mirroring the shape in
/// `desktop/src/panels.rs` (active = darker surface, inactive = card
/// surface + muted text, rest hovers + close button omitted on mobile
/// — creation and close are desktop-only per the spec).
class _TabStrip extends StatelessWidget {
  const _TabStrip({
    required this.tabs,
    required this.activeTabId,
    required this.onTap,
  });

  final List<TabSummary> tabs;
  final String activeTabId;
  final void Function(TabSummary) onTap;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: AppTokens.tabStripHeight,
      decoration: const BoxDecoration(
        color: AppTokens.chromeBg,
        border: Border(
          bottom: BorderSide(color: AppTokens.border, width: 1),
        ),
      ),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            for (var i = 0; i < tabs.length; i++)
              _TabChip(
                tab: tabs[i],
                index: i,
                total: tabs.length,
                active: tabs[i].id == activeTabId,
                onTap: () => onTap(tabs[i]),
              ),
          ],
        ),
      ),
    );
  }
}

class _TabChip extends StatelessWidget {
  const _TabChip({
    required this.tab,
    required this.index,
    required this.total,
    required this.active,
    required this.onTap,
  });

  final TabSummary tab;
  final int index;
  final int total;
  final bool active;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final bg = active ? AppTokens.terminalBg : AppTokens.cardBg;
    final textColor =
        active ? AppTokens.textPrimary : AppTokens.textMuted;

    // When there's more than one tab, desktop suffixes the index (e.g.
    // "claude 1"); mirror that here.
    final title = total > 1 ? '${tab.title} ${index + 1}' : tab.title;

    return InkWell(
      onTap: onTap,
      child: Container(
        height: AppTokens.tabStripHeight,
        padding: const EdgeInsets.symmetric(horizontal: AppTokens.space5),
        decoration: BoxDecoration(color: bg),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              tab.running ? Icons.terminal : Icons.check_circle_outline,
              size: AppTokens.iconSizeSm,
              color: textColor,
            ),
            const SizedBox(width: AppTokens.space2),
            Text(
              title,
              style: TextStyle(
                color: textColor,
                fontSize: AppTokens.fontBody,
                fontFamily: AppTokens.fontFamilyMono,
                fontWeight: active ? FontWeight.w600 : FontWeight.w400,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Ported verbatim from the deleted `project_detail_page.dart` — a
/// row of on-screen chord buttons for keys that don't fit on a mobile
/// keyboard (Esc, Ctrl-*, arrows).
class _ChordBar extends StatelessWidget {
  const _ChordBar({required this.onSend});

  final void Function(List<int> bytes) onSend;

  static const List<(String, List<int>)> _chords = [
    ('Esc', [0x1B]),
    ('Tab', [0x09]),
    ('Ctrl-C', [0x03]),
    ('Ctrl-D', [0x04]),
    ('Ctrl-L', [0x0C]),
    ('Ctrl-R', [0x12]),
    ('↑', [0x1B, 0x5B, 0x41]),
    ('↓', [0x1B, 0x5B, 0x42]),
    ('←', [0x1B, 0x5B, 0x44]),
    ('→', [0x1B, 0x5B, 0x43]),
  ];

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: const BoxDecoration(
        color: AppTokens.chromeBg,
        border: Border(top: BorderSide(color: AppTokens.border)),
      ),
      padding: const EdgeInsets.symmetric(
        vertical: AppTokens.space2,
        horizontal: AppTokens.space1,
      ),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            for (final (label, bytes) in _chords)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 2),
                child: OutlinedButton(
                  onPressed: () => onSend(bytes),
                  style: OutlinedButton.styleFrom(
                    padding: const EdgeInsets.symmetric(
                      horizontal: AppTokens.space4,
                      vertical: AppTokens.space1,
                    ),
                    minimumSize: Size.zero,
                    tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                  ),
                  child: Text(
                    label,
                    style: const TextStyle(
                      fontSize: AppTokens.fontBody,
                      fontFamily: AppTokens.fontFamilyMono,
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
