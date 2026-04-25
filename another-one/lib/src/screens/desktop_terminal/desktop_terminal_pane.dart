// Desktop terminal pane — embeds an xterm.dart Terminal bound to
// the local daemon's PTY for the currently-selected tab.
//
// Lifts the proven timing tricks from mobile's `task_page.dart`:
//   - 200ms "ignore window" after attach so in-flight bytes from
//     the previous tab don't splat into the new Terminal.
//   - 400ms attach retry — first AttachTab can race a still-spawning
//     LaunchTab; the retry catches up. Cancels itself on first byte.
//   - 1.5s spinner failsafe so an idle agent (no pending output)
//     doesn't leave the spinner up forever.
//
// No tab strip: the sidebar is the tab navigator. Selection changes
// re-key the widget so xterm state wipes cleanly between tabs.
//
// Reads `localConnectionProvider` for the transport. The pane is
// always mounted under a non-null `selectedTabProvider` value —
// the parent gates rendering with a null-check.

import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:xterm/xterm.dart';

import '../../connection.dart';
import '../../state/local_connection_provider.dart';
import '../../state/tab_selection_provider.dart';
import '../../tokens.dart';
import '../../transport.dart';

class DesktopTerminalPane extends ConsumerWidget {
  const DesktopTerminalPane({super.key, required this.selection});

  final TabSelection selection;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final transport = ref.watch(localConnectionProvider);
    return _AttachedTerminal(
      key: ValueKey('${selection.sectionId}::${selection.tabId}'),
      transport: transport,
      selection: selection,
    );
  }
}

class _AttachedTerminal extends StatefulWidget {
  const _AttachedTerminal({
    super.key,
    required this.transport,
    required this.selection,
  });

  final DaemonConnection transport;
  final TabSelection selection;

  @override
  State<_AttachedTerminal> createState() => _AttachedTerminalState();
}

class _AttachedTerminalState extends State<_AttachedTerminal> {
  final Terminal _terminal = Terminal(maxLines: 10000);
  final TerminalController _terminalController = TerminalController();
  final FocusNode _terminalFocus = FocusNode();

  StreamSubscription<Uint8List>? _bytesSub;
  StreamSubscription<TransportStatus>? _statusSub;

  /// Drop bytes that land before this timestamp — covers the
  /// detach→attach swap window where the daemon's outbound queue
  /// might still hold bytes from a previously-attached tab. Set
  /// once at mount; selection changes re-key the widget so a
  /// fresh ignore window starts with the new instance.
  final DateTime _ignoreBytesUntil =
      DateTime.now().add(const Duration(milliseconds: 200));

  bool _awaitingFirstByte = true;
  Timer? _spinnerTimeout;
  Timer? _attachRetry;
  bool _gotFirstByte = false;
  String? _errorDetail;

  @override
  void initState() {
    super.initState();
    _wireTerminal();
    _armBytesListener();
    _statusSub = widget.transport.status.listen(_onStatus);
    _onStatus(widget.transport.currentStatus);
    _armSpinnerTimeout();
    unawaited(_openTab());
    // Grab focus once the first frame settles so keystrokes flow
    // immediately without needing a click.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _terminalFocus.requestFocus();
    });
  }

  @override
  void dispose() {
    _spinnerTimeout?.cancel();
    _attachRetry?.cancel();
    _bytesSub?.cancel();
    _statusSub?.cancel();
    // Best-effort detach — the next attach (or the transport close)
    // will clean up anyway, but releasing eagerly stops bytes flowing
    // for an unmounted pane.
    unawaited(widget.transport.detachTab());
    _terminalFocus.dispose();
    super.dispose();
  }

  void _wireTerminal() {
    _terminal.onOutput = (data) {
      // Same CR/LF normalisation as mobile — IME-delivered Enter
      // arrives as `\n` while agent CLIs need `\r`. Hardware Enter
      // in xterm.dart already sends `\r`, so this only touches IME
      // input.
      final normalized = data.replaceAll('\n', '\r');
      widget.transport.sendBytes(utf8.encode(normalized));
    };
    _terminal.onResize = (w, h, _, _) {
      // tabResize throws if no tab is attached yet (xterm's first
      // onResize can fire before _openTab finishes). Suppress —
      // the next onResize after attach will carry the same
      // dimensions. The mobile path silently drops these on the
      // iroh wire too.
      widget.transport.tabResize(cols: w, rows: h).catchError((_) {});
    };
  }

  void _armBytesListener() {
    _bytesSub?.cancel();
    _bytesSub = widget.transport.incoming.listen((bytes) {
      if (DateTime.now().isBefore(_ignoreBytesUntil)) return;
      _gotFirstByte = true;
      _attachRetry?.cancel();
      _attachRetry = null;
      _clearSpinner();
      _terminal.write(utf8.decode(bytes, allowMalformed: true));
    });
  }

  void _onStatus(TransportStatus status) {
    if (!mounted) return;
    switch (status.state) {
      case TransportState.connected:
        if (_errorDetail != null) {
          setState(() => _errorDetail = null);
        }
      case TransportState.error:
      case TransportState.unpaired:
        setState(() => _errorDetail = status.label);
      case TransportState.connecting:
      case TransportState.disconnected:
        break;
    }
  }

  void _clearSpinner() {
    if (_awaitingFirstByte) {
      setState(() => _awaitingFirstByte = false);
    }
    _spinnerTimeout?.cancel();
    _spinnerTimeout = null;
  }

  void _armSpinnerTimeout() {
    _spinnerTimeout?.cancel();
    _spinnerTimeout = Timer(const Duration(milliseconds: 1500), () {
      if (!mounted || !_awaitingFirstByte) return;
      setState(() => _awaitingFirstByte = false);
    });
  }

  Future<void> _openTab() async {
    _gotFirstByte = false;
    unawaited(
      widget.transport.launchTab(
        sectionId: widget.selection.sectionId,
        tabId: widget.selection.tabId,
      ),
    );
    // First attach: tolerate "tab not running yet" — LaunchTab
    // populates the broadcast asynchronously, and on the local FFI
    // path the race window is real (no QUIC RTT to absorb it). The
    // 400ms retry below catches up. iroh's wire-level attach drops
    // the race silently, so this matches mobile's effective
    // behaviour.
    try {
      await widget.transport.attachTab(
        sectionId: widget.selection.sectionId,
        tabId: widget.selection.tabId,
      );
    } catch (_) {}
    _attachRetry?.cancel();
    _attachRetry = Timer(const Duration(milliseconds: 400), () async {
      if (!mounted || _gotFirstByte) return;
      try {
        await widget.transport.attachTab(
          sectionId: widget.selection.sectionId,
          tabId: widget.selection.tabId,
        );
      } catch (_) {}
    });
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        if (_errorDetail != null) _ErrorBanner(detail: _errorDetail!),
        Expanded(
          child: Stack(
            children: [
              Padding(
                padding: const EdgeInsets.all(AppTokens.terminalViewPadding),
                child: TerminalView(
                  _terminal,
                  controller: _terminalController,
                  focusNode: _terminalFocus,
                  autofocus: true,
                  shortcuts: _desktopShortcuts,
                  textStyle: const TerminalStyle(
                    fontSize: AppTokens.fontBodyLg,
                    fontFamily: AppTokens.fontFamilyMono,
                  ),
                ),
              ),
              if (_awaitingFirstByte)
                const Center(child: CircularProgressIndicator()),
            ],
          ),
        ),
      ],
    );
  }
}

/// Empty for now — xterm.dart's defaults handle copy/paste fine on
/// desktop. Holds the seam for future custom shortcuts (find-in-
/// buffer, font size adjust, etc.). Specifying an empty map is a
/// no-op against xterm's defaults.
final Map<ShortcutActivator, Intent> _desktopShortcuts = {};

class _ErrorBanner extends StatelessWidget {
  const _ErrorBanner({required this.detail});

  final String detail;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      color: AppTokens.errorBg,
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space5,
        vertical: AppTokens.space2,
      ),
      child: Text(
        detail,
        style: const TextStyle(
          fontSize: AppTokens.fontBody,
          color: AppTokens.errorText,
        ),
      ),
    );
  }
}
