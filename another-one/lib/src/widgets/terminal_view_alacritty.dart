// Alacritty-engine terminal view (Phase 0 spike).
//
// Replaces xterm.dart's parser with `alacritty_terminal` behind the
// `engine_*` FRB surface. This is the default desktop renderer on
// macOS/Linux; `ANOTHER_ONE_ALACRITTY=0` forces the legacy xterm pane
// for comparison.
//
// Pipeline:
//   1. transport.incoming  → engineWritePty
//   2. Incoming bytes / resizes schedule a coalesced engineSnapshot
//      pull; idle terminals do no frame polling.
//   3. CustomPaint draws the cell grid + cursor.
//   4. Keystrokes → engineEncodeInput → transport.sendBytes.
//
// What this widget DOES NOT do (Phase 0 scope):
//   * Selection painting / clipboard interaction.
//   * Scrollback rendering — viewport only.
//   * IME marked text.
//   * Bracketed paste, mouse modes, app-cursor-keys.
//   * Resize-to-fit fonts. Cell metrics are computed once from
//     AppTokens; window resize reflows columns / rows but the
//     cell size is fixed.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../connection.dart';
import '../rust/api/terminal_engine.dart' as engine;
import '../tokens.dart';

class TerminalViewAlacritty extends StatefulWidget {
  const TerminalViewAlacritty({
    super.key,
    required this.transport,
    required this.sectionId,
    required this.tabId,
  });

  final DaemonConnection transport;
  final String sectionId;
  final String tabId;

  @override
  State<TerminalViewAlacritty> createState() => _TerminalViewAlacrittyState();
}

class _TerminalViewAlacrittyState extends State<TerminalViewAlacritty> {
  static const double _fontSize = AppTokens.fontBodyLg;
  static const String _fontFamily = AppTokens.fontFamilyMono;
  static const Duration _snapshotCadence = Duration(milliseconds: 33);

  StreamSubscription<Uint8List>? _bytesSub;
  Timer? _snapshotTimer;
  Timer? _attachRetry;
  final FocusNode _focus = FocusNode();

  /// Last revision we pulled — skip repaint when alacritty hasn't
  /// produced new output since the previous tick.
  BigInt _lastRevision = BigInt.zero;
  engine.SnapshotDto? _snapshot;
  Size? _viewportSize;
  int _cols = 80;
  int _rows = 24;
  bool _opened = false;
  bool _snapshotInFlight = false;
  bool _snapshotRequested = false;
  bool _gotFirstByte = false;

  @override
  void initState() {
    super.initState();
    _wireBytes();
  }

  @override
  void dispose() {
    _snapshotTimer?.cancel();
    _attachRetry?.cancel();
    _bytesSub?.cancel();
    _focus.dispose();
    // Keep the engine registry entry alive across tab switches so
    // returning to an idle tab can repaint its last known grid without
    // waiting for fresh PTY bytes. Tab close owns engine cleanup.
    super.dispose();
  }

  Future<void> _ensureOpenedAndAttached() async {
    if (_opened) return;
    _opened = true;
    await engine.engineOpen(
      sectionId: widget.sectionId,
      tabId: widget.tabId,
      cols: _cols,
      rows: _rows,
    );
    await _attachAndResize();
    unawaited(
      widget.transport.launchTab(
        sectionId: widget.sectionId,
        tabId: widget.tabId,
      ),
    );
    _scheduleAttachRetry();
    _scheduleSnapshot();
  }

  Future<void> _attachAndResize() async {
    try {
      await widget.transport.attachTab(
        sectionId: widget.sectionId,
        tabId: widget.tabId,
      );
    } catch (_) {}
    try {
      await widget.transport.tabResize(cols: _cols, rows: _rows);
    } catch (_) {}
  }

  void _scheduleAttachRetry([int attempt = 1]) {
    _attachRetry?.cancel();
    if (_gotFirstByte || attempt > 30) return;
    final delay = attempt < 5
        ? const Duration(milliseconds: 250)
        : const Duration(milliseconds: 500);
    _attachRetry = Timer(delay, () async {
      if (!mounted || _gotFirstByte) return;
      await _attachAndResize();
      _scheduleSnapshot();
      _scheduleAttachRetry(attempt + 1);
    });
  }

  void _wireBytes() {
    _bytesSub?.cancel();
    _bytesSub = widget.transport.incoming.listen((bytes) async {
      try {
        _gotFirstByte = true;
        _attachRetry?.cancel();
        _attachRetry = null;
        await engine.engineWritePty(
          sectionId: widget.sectionId,
          tabId: widget.tabId,
          bytes: bytes,
        );
        _scheduleSnapshot();
      } catch (_) {}
    });
  }

  void _scheduleSnapshot() {
    _snapshotRequested = true;
    if (_snapshotTimer != null || _snapshotInFlight) return;
    _snapshotTimer = Timer(_snapshotCadence, () {
      _snapshotTimer = null;
      unawaited(_pullSnapshot());
    });
  }

  Future<void> _pullSnapshot() async {
    if (_snapshotInFlight || !mounted) return;
    _snapshotInFlight = true;
    _snapshotRequested = false;
    try {
      final revision = await engine.engineRevision(
        sectionId: widget.sectionId,
        tabId: widget.tabId,
      );
      if (revision != _lastRevision || _snapshot == null) {
        final snap = await engine.engineSnapshot(
          sectionId: widget.sectionId,
          tabId: widget.tabId,
          scrollbackOffset: 0,
          maxRows: _rows,
        );
        _lastRevision = snap.revision;
        if (mounted) setState(() => _snapshot = snap);
      }
    } catch (_) {
      // Engine not yet open — _ensureOpenedAndAttached lazily seeds it.
    } finally {
      _snapshotInFlight = false;
    }
    if (mounted && _snapshotRequested) {
      _scheduleSnapshot();
    }
  }

  void _maybeResize(Size size) {
    if (_viewportSize == size) return;
    _viewportSize = size;
    final metrics = _CellMetrics.measure(_fontSize, _fontFamily);
    final padding = AppTokens.terminalViewPadding * 2;
    final cols = ((size.width - padding) / metrics.width).floor().clamp(
      2,
      1024,
    );
    final rows = ((size.height - padding) / metrics.height).floor().clamp(
      1,
      1024,
    );
    if (cols == _cols && rows == _rows && _opened) return;
    _cols = cols;
    _rows = rows;
    if (!_opened) {
      unawaited(_ensureOpenedAndAttached());
      return;
    }
    unawaited(
      engine.engineResize(
        sectionId: widget.sectionId,
        tabId: widget.tabId,
        cols: cols,
        rows: rows,
      ),
    );
    unawaited(widget.transport.tabResize(cols: cols, rows: rows));
    _scheduleSnapshot();
  }

  Future<void> _sendInput(engine.InputEventDto event) async {
    try {
      final encoded = await engine.engineEncodeInput(
        sectionId: widget.sectionId,
        tabId: widget.tabId,
        event: event,
      );
      if (encoded.isEmpty) return;
      widget.transport.sendBytes(encoded);
    } catch (_) {}
  }

  KeyEventResult _handleKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent && event is! KeyRepeatEvent) {
      return KeyEventResult.ignored;
    }
    final logical = event.logicalKey;
    engine.InputEventDto? mapped;
    if (logical == LogicalKeyboardKey.enter ||
        logical == LogicalKeyboardKey.numpadEnter) {
      mapped = const engine.InputEventDto(kind: 1, code: 0);
    } else if (logical == LogicalKeyboardKey.backspace) {
      mapped = const engine.InputEventDto(kind: 2, code: 0);
    } else if (logical == LogicalKeyboardKey.tab) {
      mapped = const engine.InputEventDto(kind: 3, code: 0);
    } else if (logical == LogicalKeyboardKey.escape) {
      mapped = const engine.InputEventDto(kind: 4, code: 0);
    } else if (logical == LogicalKeyboardKey.arrowUp) {
      mapped = const engine.InputEventDto(kind: 5, code: 0);
    } else if (logical == LogicalKeyboardKey.arrowDown) {
      mapped = const engine.InputEventDto(kind: 6, code: 0);
    } else if (logical == LogicalKeyboardKey.arrowLeft) {
      mapped = const engine.InputEventDto(kind: 7, code: 0);
    } else if (logical == LogicalKeyboardKey.arrowRight) {
      mapped = const engine.InputEventDto(kind: 8, code: 0);
    } else {
      final ch = event.character;
      if (ch != null && ch.isNotEmpty) {
        mapped = engine.InputEventDto(kind: 0, code: ch.runes.first);
      }
    }
    if (mapped != null) {
      unawaited(_sendInput(mapped));
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          _maybeResize(constraints.biggest);
        });
        return Padding(
          padding: const EdgeInsets.all(AppTokens.terminalViewPadding),
          child: Focus(
            focusNode: _focus,
            autofocus: true,
            onKeyEvent: _handleKey,
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: _focus.requestFocus,
              child: CustomPaint(
                painter: _TerminalPainter(
                  snapshot: _snapshot,
                  metrics: _CellMetrics.measure(_fontSize, _fontFamily),
                ),
                size: Size.infinite,
              ),
            ),
          ),
        );
      },
    );
  }
}

class _CellMetrics {
  final double width;
  final double height;

  const _CellMetrics(this.width, this.height);

  static _CellMetrics? _cached;

  static _CellMetrics measure(double fontSize, String fontFamily) {
    final cached = _cached;
    if (cached != null) return cached;
    final tp = TextPainter(
      text: TextSpan(
        text: 'M',
        style: TextStyle(
          fontFamily: fontFamily,
          fontSize: fontSize,
          color: const Color(0xFFFFFFFF),
          height: 1.2,
        ),
      ),
      textDirection: TextDirection.ltr,
    )..layout();
    final m = _CellMetrics(tp.width, tp.height);
    _cached = m;
    return m;
  }
}

class _TerminalPainter extends CustomPainter {
  _TerminalPainter({required this.snapshot, required this.metrics});

  final engine.SnapshotDto? snapshot;
  final _CellMetrics metrics;

  @override
  void paint(Canvas canvas, Size size) {
    final bgPaint = Paint()..color = AppTokens.terminalBg;
    canvas.drawRect(Offset.zero & size, bgPaint);
    final snap = snapshot;
    if (snap == null) return;

    final cellW = metrics.width;
    final cellH = metrics.height;
    final cells = snap.cells;
    for (int row = 0; row < snap.rows; row++) {
      for (int col = 0; col < snap.cols; col++) {
        final idx = row * snap.cols + col;
        if (idx >= cells.length) continue;
        final cell = cells[idx];
        final dx = col * cellW;
        final dy = row * cellH;
        if ((cell.bg & 0xFF) != 0) {
          final bgRect = Rect.fromLTWH(dx, dy, cellW, cellH);
          canvas.drawRect(bgRect, Paint()..color = _argb(cell.bg));
        }
        if (cell.ch == 0) continue;
        final fg = _argb(cell.fg);
        final flags = cell.flags;
        final tp = TextPainter(
          text: TextSpan(
            text: String.fromCharCode(cell.ch),
            style: TextStyle(
              color: fg,
              fontFamily: AppTokens.fontFamilyMono,
              fontSize: AppTokens.fontBodyLg,
              fontWeight: (flags & 0x1) != 0
                  ? FontWeight.bold
                  : FontWeight.normal,
              fontStyle: (flags & 0x2) != 0
                  ? FontStyle.italic
                  : FontStyle.normal,
              decoration: (flags & 0x4) != 0
                  ? TextDecoration.underline
                  : ((flags & 0x10) != 0 ? TextDecoration.lineThrough : null),
              decorationColor: fg,
              height: 1.2,
            ),
          ),
          textDirection: TextDirection.ltr,
        )..layout();
        tp.paint(canvas, Offset(dx, dy));
      }
    }

    if (snap.cursorVisible) {
      final cx = snap.cursorCol * cellW;
      final cy = snap.cursorRow * cellH;
      final cursorPaint = Paint()..color = const Color(0x80EAEAEA);
      canvas.drawRect(Rect.fromLTWH(cx, cy, cellW, cellH), cursorPaint);
    }
  }

  /// Convert packed `RRGGBBAA` (alacritty side) to Flutter `ARGB`.
  Color _argb(int rgba) {
    final r = (rgba >> 24) & 0xFF;
    final g = (rgba >> 16) & 0xFF;
    final b = (rgba >> 8) & 0xFF;
    final a = rgba & 0xFF;
    return Color.fromARGB(a, r, g, b);
  }

  @override
  bool shouldRepaint(covariant _TerminalPainter old) {
    return old.snapshot?.revision != snapshot?.revision ||
        old.snapshot?.cols != snapshot?.cols ||
        old.snapshot?.rows != snapshot?.rows;
  }
}
