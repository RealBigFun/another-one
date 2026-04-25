// Phase 0 throughput benchmark for xterm.dart. Standalone screen
// that pumps a synthetic ANSI-laced byte stream through a Terminal
// widget and reports sustained MB/s. No PTY, no daemon, no iroh —
// the goal is a ceiling on what xterm can absorb regardless of
// upstream backpressure.
//
// Activated via `--dart-define=BENCHMARK=true`; off otherwise so
// regular debug runs keep the normal AppRoot flow.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:xterm/xterm.dart';

class BenchmarkPage extends StatefulWidget {
  const BenchmarkPage({super.key});

  @override
  State<BenchmarkPage> createState() => _BenchmarkPageState();
}

class _BenchmarkPageState extends State<BenchmarkPage> {
  final Terminal _terminal = Terminal(maxLines: 10000);
  final TerminalController _controller = TerminalController();

  @override
  void initState() {
    super.initState();
    // Auto-run on first frame so the run is reproducible from a
    // launch-and-wait CLI invocation. Manual play/stop buttons stay
    // wired for re-runs from the same window.
    WidgetsBinding.instance.addPostFrameCallback((_) => _start());
  }

  Timer? _pumpTimer;
  int _totalBytes = 0;
  final Stopwatch _stopwatch = Stopwatch();
  String _statusLine = 'idle — press play to run a 10s benchmark';
  double _peakRateMBps = 0.0;
  double _finalRateMBps = 0.0;

  // ~75% printable text + 25% ANSI escapes (cursor moves + colors),
  // modeled after typical agent output (Claude Code, Cursor, Codex
  // logs). The chunk is pre-built once and reused so we measure
  // xterm's render rate, not Dart string allocation.
  static final String _chunk = _buildChunk();
  static final int _chunkBytes = _chunk.length;

  static String _buildChunk() {
    final sb = StringBuffer();
    for (int i = 0; i < 100; i++) {
      sb.write('\x1b[32m[INFO]\x1b[0m ');
      sb.write('handler #$i processed request_id=req-$i-abc123 in ');
      sb.write('${(i * 17) % 1000}ms with status=ok bytes=${i * 1024}\n');
      if (i % 7 == 0) {
        sb.write('\x1b[1;33mwarning:\x1b[0m approaching the rate limit '
            'on a downstream call (${100 - i}% headroom)\n');
      }
      if (i % 13 == 0) {
        sb.write('\x1b[31merror:\x1b[0m transient failure '
            '— retrying in ${i % 5}s\n');
      }
    }
    return sb.toString();
  }

  void _start() {
    if (_pumpTimer?.isActive ?? false) return;
    _totalBytes = 0;
    _peakRateMBps = 0.0;
    _finalRateMBps = 0.0;
    _stopwatch.reset();
    _stopwatch.start();

    // Pump chunks every 1ms; each tick writes 10 chunks. Adjust if
    // we need a different shape — this is throughput, not jitter.
    _pumpTimer = Timer.periodic(const Duration(milliseconds: 1), (timer) {
      for (int i = 0; i < 10; i++) {
        _terminal.write(_chunk);
        _totalBytes += _chunkBytes;
      }

      final elapsedMs = _stopwatch.elapsedMilliseconds;
      if (elapsedMs > 0) {
        final mbps = (_totalBytes / 1024.0 / 1024.0) /
            (elapsedMs / 1000.0);
        if (mbps > _peakRateMBps) _peakRateMBps = mbps;
        setState(() {
          _statusLine = '${(_totalBytes / 1024 / 1024).toStringAsFixed(1)} MiB '
              'in ${(elapsedMs / 1000).toStringAsFixed(1)}s · '
              '${mbps.toStringAsFixed(1)} MiB/s '
              '(peak ${_peakRateMBps.toStringAsFixed(1)})';
        });
      }

      if (elapsedMs >= 10_000) {
        timer.cancel();
        _stopwatch.stop();
        _finalRateMBps = (_totalBytes / 1024.0 / 1024.0) /
            (_stopwatch.elapsedMilliseconds / 1000.0);
        // ignore: avoid_print
        print('XTERM_BENCH_RESULT: '
            'sustained=${_finalRateMBps.toStringAsFixed(2)}MiB/s '
            'peak=${_peakRateMBps.toStringAsFixed(2)}MiB/s '
            'totalBytes=$_totalBytes '
            'elapsedMs=${_stopwatch.elapsedMilliseconds}');
        setState(() {
          _statusLine = 'done · ${_finalRateMBps.toStringAsFixed(1)} MiB/s '
              'sustained · peak ${_peakRateMBps.toStringAsFixed(1)} MiB/s';
        });
      }
    });
  }

  void _stop() {
    _pumpTimer?.cancel();
    _stopwatch.stop();
  }

  @override
  void dispose() {
    _pumpTimer?.cancel();
    _stopwatch.stop();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(
          'xterm benchmark · $_statusLine',
          style: const TextStyle(fontSize: 14),
          overflow: TextOverflow.ellipsis,
        ),
        actions: [
          IconButton(
            icon: const Icon(Icons.play_arrow),
            onPressed: _start,
            tooltip: 'Run 10s benchmark',
          ),
          IconButton(
            icon: const Icon(Icons.stop),
            onPressed: _stop,
            tooltip: 'Abort',
          ),
        ],
      ),
      body: TerminalView(_terminal, controller: _controller),
    );
  }
}
