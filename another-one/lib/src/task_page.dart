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
import 'transport.dart';
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
  StreamSubscription<TransportStatus>? _statusSub;
  int _instanceKey = 0;

  /// Timestamp before which we drop incoming bytes instead of writing
  /// them to the Terminal. Set 200ms into the future whenever we
  /// switch tabs so any in-flight bytes from the *previous* tab —
  /// already queued in the daemon's outbound mpsc (cap 64) when we
  /// sent DetachTab — get discarded instead of splatting into the
  /// new Terminal. A cleaner fix would be an explicit "attach
  /// complete for tab X" sentinel in the daemon → client protocol;
  /// until that exists, a short grace period handles the common
  /// case (LAN RTT ≪ 200ms, cellular usually < 200ms). Initial open
  /// uses the same mechanism so any bytes from a still-detaching
  /// prior session also get dropped.
  DateTime _ignoreBytesUntil = DateTime.fromMillisecondsSinceEpoch(0);

  /// `true` until the first byte of the attached tab's PTY has
  /// arrived *or* [_spinnerTimeout] fires — whichever is first.
  /// Without the timeout, an idle shell (already at a prompt with
  /// no pending output) would leave the spinner up indefinitely
  /// since there are no new bytes to prove the attach succeeded.
  bool _awaitingFirstByte = true;
  Timer? _spinnerTimeout;

  /// The "did the daemon actually spawn yet?" retry, fired 400ms
  /// after the initial AttachTab. Kept as a Timer (not Future.delayed)
  /// so the first inbound byte can cancel it — otherwise the delayed
  /// closure would fire a redundant second AttachTab that the daemon
  /// processes fine but wastes a round-trip.
  Timer? _attachRetry;

  /// Set when the user triggers a tab switch or first open. The
  /// inbound-bytes listener clears this on first byte, which lets
  /// [_attachRetry] cancel itself.
  bool _gotFirstByte = false;

  /// Any in-flight transport error surfaced via [TransportStatus].
  /// Drives a banner above the terminal; null means "no error to
  /// show". Cleared on any subsequent connected status.
  String? _errorDetail;

  @override
  void initState() {
    super.initState();
    _activeTabId = widget.task.activeTabId;
    _wireTerminal();
    _armBytesListener();
    _statusSub = widget.transport.status.listen(_onStatus);
    // Seed with the current status — the stream only fires on changes,
    // so if we arrived already in `.error` state we'd show nothing.
    _onStatus(widget.transport.currentStatus);
    _armSpinnerTimeout();
    _armIgnoreWindow();
    unawaited(_openTab(_activeTabId));
  }

  /// (Re)subscribe the PTY-bytes listener. Pulled into a helper so
  /// initState + _switchTab can't drift apart. The listener does not
  /// rely on `setState` — xterm's Terminal internally schedules a
  /// rebuild when `write` is called, so flipping spinner state via
  /// setState on every byte would be needless churn.
  void _armBytesListener() {
    _bytesSub?.cancel();
    _bytesSub = widget.transport.incoming.listen((bytes) {
      if (DateTime.now().isBefore(_ignoreBytesUntil)) {
        // In-flight bytes from a prior attachment — drop instead of
        // splatting into the current tab's Terminal.
        return;
      }
      _gotFirstByte = true;
      _attachRetry?.cancel();
      _attachRetry = null;
      _clearSpinner();
      _terminal.write(utf8.decode(bytes, allowMalformed: true));
    });
  }

  /// Open the ignore window. Call before sending DetachTab so bytes
  /// already queued by the daemon for the old tab don't land in the
  /// new Terminal once the listener re-arms. 200ms empirically
  /// covers LAN round-trip (~20-50ms) plus the daemon's outbound
  /// mpsc flush; cellular usually fits too.
  void _armIgnoreWindow() {
    _ignoreBytesUntil = DateTime.now().add(const Duration(milliseconds: 200));
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
        // status.toString() returns "Instance of 'TransportStatus'" —
        // TransportStatus doesn't override toString. status.label is
        // the human-readable form defined in transport.dart.
        setState(() => _errorDetail = status.label);
      case TransportState.connecting:
      case TransportState.disconnected:
        // Transient — don't surface as an error banner; the spinner
        // + retry already communicate "working on it" clearly enough.
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

  /// Failsafe: if no bytes arrive within 1.5s, hide the spinner
  /// anyway. Idle agents (sitting at a prompt with no pending
  /// output) legitimately emit nothing after attach; the spinner
  /// would stay up forever without this.
  void _armSpinnerTimeout() {
    _spinnerTimeout?.cancel();
    _spinnerTimeout = Timer(const Duration(milliseconds: 1500), () {
      if (!mounted || !_awaitingFirstByte) return;
      setState(() => _awaitingFirstByte = false);
    });
  }

  /// LaunchTab + AttachTab in that order. LaunchTab is a no-op on
  /// the daemon if the tab's already running, so it's cheap to send
  /// unconditionally. We AttachTab immediately and arm a Timer-based
  /// retry — if the first AttachTab landed before the daemon's
  /// LaunchTab had populated a broadcast, the retry catches up. The
  /// retry cancels itself the instant any byte arrives (see the
  /// `_bytesSub` listener) so the common case doesn't pay a
  /// redundant second AttachTab round-trip.
  ///
  /// The initial PTY size is *not* sent here. xterm.dart's Terminal
  /// starts out with defaults (80×25) and only learns its real
  /// dimensions after TerminalView has laid out — which happens
  /// asynchronously during the build phase after we return. The
  /// real dimensions arrive via `onResize` (wired in
  /// [_wireTerminal]), which fires on first layout and whenever
  /// the widget reflows. If we sent a resize here we'd carry
  /// xterm's defaults onto the wire and clamp the desktop's PTY to
  /// 80×25 until the real onResize landed — visibly squeezing the
  /// desktop window for the first few hundred ms on every mobile
  /// open.
  Future<void> _openTab(String tabId) async {
    _gotFirstByte = false;
    unawaited(
      widget.transport.launchTab(
        sectionId: widget.task.sectionId,
        tabId: tabId,
      ),
    );
    // First attach: tolerate "tab not running yet" — LaunchTab
    // populates the broadcast asynchronously, and on the local FFI
    // path the race window is real (no QUIC RTT to absorb it). The
    // 400ms retry below catches up. iroh's wire-level attach drops
    // the race silently, so this matches desktop's effective
    // behaviour.
    try {
      await widget.transport.attachTab(
        sectionId: widget.task.sectionId,
        tabId: tabId,
      );
    } catch (_) {}
    _attachRetry?.cancel();
    _attachRetry = Timer(const Duration(milliseconds: 400), () async {
      if (!mounted || tabId != _activeTabId || _gotFirstByte) return;
      try {
        await widget.transport.attachTab(
          sectionId: widget.task.sectionId,
          tabId: tabId,
        );
      } catch (_) {}
    });
  }

  void _wireTerminal() {
    _terminal.onOutput = (data) {
      // Soft keyboards on iOS/Android emit LF (\n) when the user taps
      // Enter, but agent CLIs (Claude Code, shells, REPLs) treat CR
      // (\r) as "submit line" — bare LF lands in the prompt buffer as
      // a continuation. Hardware Enter in xterm.dart already encodes
      // as CR, so translating here affects only IME-delivered text.
      final normalized = data.replaceAll('\n', '\r');
      widget.transport.sendBytes(utf8.encode(normalized));
    };
    _terminal.onResize = (w, h, _, _) {
      widget.transport.tabResize(cols: w, rows: h);
    };
  }

  Future<void> _switchTab(TabSummary tab) async {
    if (tab.id == _activeTabId) return;
    // Open the ignore window BEFORE sending DetachTab so the
    // listener is already set to drop anything that arrives after
    // this point until the window expires.
    _armIgnoreWindow();
    // Detach the old Terminal's callbacks before dropping our ref
    // to it. TerminalView doesn't dispose synchronously and its
    // onResize can fire one more time during the key-swap —
    // without these nulls, that stray onResize would call
    // `widget.transport.tabResize` with the OLD tab still in the
    // active_viewers map (we've already moved on conceptually),
    // clamping the newly-attached tab's PTY to a bogus size.
    _terminal.onOutput = null;
    _terminal.onResize = null;
    await _bytesSub?.cancel();
    await widget.transport.detachTab();

    setState(() {
      _activeTabId = tab.id;
      _instanceKey++;
      _terminal = Terminal(maxLines: 10000);
      _wireTerminal();
      _awaitingFirstByte = true;
    });

    _armBytesListener();
    _armSpinnerTimeout();

    await _openTab(tab.id);
    _terminalFocus.requestFocus();
  }

  @override
  void dispose() {
    _spinnerTimeout?.cancel();
    _attachRetry?.cancel();
    _bytesSub?.cancel();
    _statusSub?.cancel();
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
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                // `Icons.call_split` substitutes for the `⎇` glyph
                // (U+2387) used on desktop — that codepoint isn't in
                // Android's default monospace fallback and renders
                // as tofu.
                const Icon(
                  Icons.call_split,
                  size: AppTokens.iconSizeSm,
                  color: AppTokens.textMuted,
                ),
                const SizedBox(width: 4),
                Flexible(
                  child: Text(
                    widget.task.branchName,
                    style: const TextStyle(
                      fontSize: AppTokens.fontSmall,
                      fontFamily: AppTokens.fontFamilyMono,
                      color: AppTokens.textMuted,
                    ),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
      body: SafeArea(
        child: Column(
          children: [
            if (_errorDetail != null) _ErrorBanner(message: _errorDetail!),
            _TabStrip(
              tabs: widget.task.tabs,
              activeTabId: _activeTabId,
              onTap: _switchTab,
            ),
            Expanded(
              child: Container(
                color: AppTokens.terminalBg,
                child: Stack(
                  children: [
                    GestureDetector(
                      onTap: _terminalFocus.requestFocus,
                      child: TerminalView(
                        _terminal,
                        key: ValueKey(_instanceKey),
                        controller: _terminalController,
                        focusNode: _terminalFocus,
                        autofocus: true,
                        backgroundOpacity: 1.0,
                        padding: const EdgeInsets.all(AppTokens.space2),
                        // Recommended by xterm.dart for mobile: many soft
                        // keyboards report backspace as a text-diff rather
                        // than a hardware key event. Without this, tapping
                        // delete on an empty-looking prompt sends nothing.
                        deleteDetection: true,
                        textStyle: const TerminalStyle(
                          fontFamily: AppTokens.fontFamilyMono,
                          fontSize: AppTokens.fontBody,
                        ),
                      ),
                    ),
                    if (_awaitingFirstByte)
                      const Positioned.fill(child: _TabLoadingOverlay()),
                  ],
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
        border: Border(bottom: BorderSide(color: AppTokens.border, width: 1)),
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
    final textColor = active ? AppTokens.textPrimary : AppTokens.textMuted;

    // Prefer the user-set `fixed_title` (mirrors desktop: `fixed_title`
    // on `PersistedTerminalTab` overrides the agent-provided label).
    // Otherwise: when there's more than one tab, desktop suffixes the
    // index — mirror that here.
    final baseTitle = tab.fixedTitle ?? tab.title;
    final title = (tab.fixedTitle == null && total > 1)
        ? '$baseTitle ${index + 1}'
        : baseTitle;

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
            if (tab.pinned) ...[
              const SizedBox(width: AppTokens.space2),
              const Icon(
                Icons.push_pin,
                size: AppTokens.iconSizeXs,
                color: AppTokens.accent,
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// Ported verbatim from the deleted `project_detail_page.dart` — a
/// row of on-screen chord buttons for keys that don't fit on a mobile
/// keyboard (Esc, Ctrl-*, arrows).
/// Shown over the terminal while we're waiting for the first PTY
/// byte after an attach. Cold launches (spawning Claude Code, etc.)
/// can take 1–2s; without this the user stares at a black rectangle.
/// Drawn as a semi-transparent scrim so any output that started
/// streaming during the transition fades through.
class _TabLoadingOverlay extends StatelessWidget {
  const _TabLoadingOverlay();

  @override
  Widget build(BuildContext context) {
    return ColoredBox(
      color: AppTokens.terminalBg.withValues(alpha: 0.85),
      child: const Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            SizedBox(
              width: 28,
              height: 28,
              child: CircularProgressIndicator(
                strokeWidth: 2.5,
                color: AppTokens.accent,
              ),
            ),
            SizedBox(height: AppTokens.space4),
            Text(
              'attaching…',
              style: TextStyle(
                fontSize: AppTokens.fontSmall,
                fontFamily: AppTokens.fontFamilyMono,
                color: AppTokens.textMuted,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// One-line banner surfaced above the terminal when the transport
/// reports `.error` or `.unpaired`. Previously we swallowed these
/// silently, so a dropped session (or a reset-pairings flow on the
/// desktop) showed up only as "the spinner eventually disappears
/// and nothing ever arrives." Keep it terse — the AppBar is the
/// only other place for truncation.
class _ErrorBanner extends StatelessWidget {
  const _ErrorBanner({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      color: AppTokens.errorBg,
      padding: const EdgeInsets.symmetric(
        horizontal: AppTokens.space4,
        vertical: AppTokens.space2,
      ),
      child: Text(
        message,
        style: const TextStyle(
          color: AppTokens.errorText,
          fontSize: AppTokens.fontSmall,
          fontFamily: AppTokens.fontFamilyMono,
        ),
        overflow: TextOverflow.ellipsis,
        maxLines: 2,
      ),
    );
  }
}

class _ChordBar extends StatelessWidget {
  const _ChordBar({required this.onSend});

  final void Function(List<int> bytes) onSend;

  static const List<(String, List<int>)> _chords = [
    ('Esc', [0x1B]),
    ('Tab', [0x09]),
    // Explicit CR submit — backup for the LF→CR translation in
    // _wireTerminal; also useful when the IME is mid-composition and
    // swallows the Enter tap.
    ('⏎', [0x0D]),
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
                // Chord keys are the most-tapped interactive element
                // in the app (Esc / Ctrl-C / arrow keys on the agent
                // terminal). Previous sizing was ~22×24 — well below
                // Material's 48×48 recommendation and Apple's 44×44.
                // Bumped to 44×40 with a wider tap zone via
                // `tapTargetSize: padded`.
                child: SizedBox(
                  height: 40,
                  child: OutlinedButton(
                    onPressed: () => onSend(bytes),
                    style: OutlinedButton.styleFrom(
                      padding: const EdgeInsets.symmetric(
                        horizontal: AppTokens.space5,
                        vertical: AppTokens.space2,
                      ),
                      minimumSize: const Size(44, 40),
                      tapTargetSize: MaterialTapTargetSize.padded,
                    ),
                    child: Text(
                      label,
                      style: const TextStyle(
                        fontSize: AppTokens.fontBodyLg,
                        fontFamily: AppTokens.fontFamilyMono,
                      ),
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
