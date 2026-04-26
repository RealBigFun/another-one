// End-to-end PTY throughput benchmark for `another-one-ojm.10`.
//
// Spawns a one-off `DaemonConnection` (iroh-loopback OR FFI direct),
// finds the first project's first task's first tab in the daemon's
// in-memory store, attaches, blasts a high-bandwidth producer
// command into stdin, counts bytes received over a configurable
// window, and renders sustained MB/s + total bytes + duration.
//
// The point is to verify that the post-ojm.9 iroh path can sustain
// the burst rate the legacy FFI path delivered, before the ADR's
// step 4 (`ojm.11`) deletes LocalSession. Run both modes and
// compare numbers — there is no machine assertion here, the
// reviewer is the assertion.
//
// Activation:
//   * `--dart-define=ANOTHER_ONE_SURFACE=throughput-benchmark`
//   * Optional `--dart-define=THROUGHPUT_BENCHMARK_MODE=iroh|local`
//     (defaults to `iroh`; the path the desktop now ships with).
//   * Optional `--dart-define=THROUGHPUT_BENCHMARK_BYTES=200000000`
//     — total bytes the producer emits before quitting (default 200 MB).
//   * Optional `--dart-define=THROUGHPUT_BENCHMARK_TIMEOUT_S=120`
//     — wall-clock cap regardless of producer progress.
//
// Manual usage: launch the build with the surface flag, click
// "Run benchmark", watch the readout. Re-run by clicking again
// (each click constructs a fresh transport so the comparison is
// not skewed by warmed caches).

import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/foundation.dart' show kDebugMode;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'connection.dart';
import 'log.dart';
import 'rust/api/iroh_client.dart';
import 'state/local_connection_provider.dart' show loopbackSessionAddrProvider;
import 'tokens.dart';
import 'transport.dart';
import 'transport_iroh.dart';
import 'transport_local.dart';

/// Build-time mode toggle: `iroh` exercises the loopback iroh wire
/// (the post-ojm.9 path the desktop ships with); `local` constructs
/// a `LocalTransport` against the same daemon over FFI for the
/// ojm.11-deletion baseline. Both share the same daemon process.
const String kBenchmarkMode = String.fromEnvironment(
  'THROUGHPUT_BENCHMARK_MODE',
  defaultValue: 'iroh',
);

/// Total bytes the producer emits per run. 200 MB by default —
/// enough to amortise startup + the first few `block_in_place`
/// hand-offs, short enough that the reviewer doesn't have to wait
/// 30 minutes per run. The ADR's full-rigor 30-minute / 10 MB/s
/// burst is a manual re-trigger of this same harness with a
/// matching dart-define override.
const int kBenchmarkBytes = int.fromEnvironment(
  'THROUGHPUT_BENCHMARK_BYTES',
  defaultValue: 200 * 1000 * 1000,
);

/// Wall-clock cap regardless of producer progress.
const int kBenchmarkTimeoutS = int.fromEnvironment(
  'THROUGHPUT_BENCHMARK_TIMEOUT_S',
  defaultValue: 120,
);

const _log = Log('aone.throughput-bench');

class ThroughputBenchmarkPage extends ConsumerStatefulWidget {
  const ThroughputBenchmarkPage({super.key});

  @override
  ConsumerState<ThroughputBenchmarkPage> createState() =>
      _ThroughputBenchmarkPageState();
}

class _ThroughputBenchmarkPageState
    extends ConsumerState<ThroughputBenchmarkPage> {
  DaemonConnection? _transport;
  StreamSubscription<Uint8List>? _bytesSub;
  StreamSubscription<TransportStatus>? _statusSub;

  String _status = 'idle — press Run';
  String _details = '';
  bool _running = false;

  Future<void> _run() async {
    if (_running) return;
    setState(() {
      _running = true;
      _status = 'starting…';
      _details = '';
    });
    await _teardown();
    try {
      final transport = _buildTransport();
      _transport = transport;
      await _waitForConnected(transport);

      // Pull the project tree so we can pick a real (sectionId, tabId).
      final picked = await _pickFirstTab(transport);
      if (picked == null) {
        setState(() {
          _running = false;
          _status = 'no project / task / tab found in the daemon — '
              'create at least one task with a tab before running '
              'the benchmark';
        });
        return;
      }

      await transport.launchTab(
        sectionId: picked.sectionId,
        tabId: picked.tabId,
      );
      await transport.attachTab(
        sectionId: picked.sectionId,
        tabId: picked.tabId,
      );

      final start = DateTime.now();
      int totalBytes = 0;
      final completer = Completer<void>();
      _bytesSub = transport.incoming.listen((chunk) {
        totalBytes += chunk.lengthInBytes;
        if (!completer.isCompleted &&
            DateTime.now().difference(start).inSeconds >= kBenchmarkTimeoutS) {
          completer.complete();
        }
      });

      // Drive a high-bandwidth producer through the tab's stdin.
      // `head -c <N>` bounds the run; `base64` keeps bytes printable
      // so xterm doesn't crash on stray control sequences. The exact
      // command is shell-portable enough for bash / zsh / fish.
      final cmd = "stty -echo; cat /dev/urandom | base64 | head -c "
          "$kBenchmarkBytes; stty echo; printf '\\\\nbench-done\\\\n'\n";
      transport.sendBytes(cmd.codeUnits);

      // Wait for either the timeout or the EOF token; we look for the
      // sentinel inline because the daemon's `attachTab` stream stays
      // alive after the producer exits (the shell is still running).
      Timer? deadlineTimer = Timer(
        Duration(seconds: kBenchmarkTimeoutS),
        () {
          if (!completer.isCompleted) completer.complete();
        },
      );
      // Sentinel detection: a small ring-buffer of recent bytes lets us
      // catch `bench-done` without retaining the full stream.
      final tail = <int>[];
      _bytesSub?.cancel();
      _bytesSub = transport.incoming.listen((chunk) {
        totalBytes += chunk.lengthInBytes;
        for (final b in chunk) {
          tail.add(b);
          if (tail.length > 32) tail.removeAt(0);
        }
        if (!completer.isCompleted) {
          final tailStr = String.fromCharCodes(tail);
          if (tailStr.contains('bench-done')) {
            completer.complete();
          }
        }
      });

      await completer.future;
      deadlineTimer.cancel();
      final elapsed = DateTime.now().difference(start);

      final mbps = totalBytes / 1e6 / elapsed.inMilliseconds * 1000;
      _log.info('throughput benchmark complete', {
        'mode': kBenchmarkMode,
        'total_bytes': totalBytes,
        'elapsed_ms': elapsed.inMilliseconds,
        'mbps': mbps.toStringAsFixed(2),
        'section_id': picked.sectionId,
        'tab_id': picked.tabId,
      });
      setState(() {
        _running = false;
        _status = '$kBenchmarkMode mode: ${mbps.toStringAsFixed(1)} MB/s';
        _details = '${(totalBytes / 1e6).toStringAsFixed(1)} MB in '
            '${elapsed.inMilliseconds} ms '
            '(target ${kBenchmarkBytes ~/ 1000000} MB; '
            '${kBenchmarkTimeoutS}s timeout)';
      });
    } catch (e, s) {
      _log.error('throughput benchmark failed',
          error: e, stackTrace: s, fields: {'mode': kBenchmarkMode});
      setState(() {
        _running = false;
        _status = 'failed: $e';
        _details = '';
      });
    }
  }

  Future<void> _teardown() async {
    await _bytesSub?.cancel();
    _bytesSub = null;
    await _statusSub?.cancel();
    _statusSub = null;
    final t = _transport;
    _transport = null;
    if (t != null) {
      try {
        await t.detachTab();
      } catch (_) {/* best effort */}
      try {
        await t.close();
      } catch (_) {/* best effort */}
    }
  }

  DaemonConnection _buildTransport() {
    if (kBenchmarkMode == 'local') {
      return LocalTransport();
    }
    final addr = ref.read(loopbackSessionAddrProvider);
    return IrohTransport(
      addr.endpointId,
      directAddrs: addr.directAddrs,
      relayUrls: addr.relayUrls,
      pairToken: null,
      displayNameOverride: 'BenchLoopback',
    );
  }

  Future<void> _waitForConnected(DaemonConnection transport) async {
    if (transport.currentStatus.state == TransportState.connected) return;
    final connected = Completer<void>();
    _statusSub = transport.status.listen((s) {
      if (s.state == TransportState.connected && !connected.isCompleted) {
        connected.complete();
      }
      if (s.state == TransportState.error && !connected.isCompleted) {
        connected.completeError(StateError('transport error: ${s.detail}'));
      }
    });
    transport.connect();
    await connected.future.timeout(const Duration(seconds: 15));
    await _statusSub?.cancel();
    _statusSub = null;
  }

  Future<_TabRef?> _pickFirstTab(DaemonConnection transport) async {
    final replies = transport.workerReplies;
    final completer = Completer<List<ProjectSummary>>();
    final sub = replies.listen((reply) {
      if (reply is WorkerReply_ProjectList && !completer.isCompleted) {
        completer.complete(reply.projects);
      }
    });
    await transport.listProjects();
    final projects =
        await completer.future.timeout(const Duration(seconds: 5));
    await sub.cancel();
    for (final p in projects) {
      for (final task in p.tasks) {
        for (final tab in task.tabs) {
          return _TabRef(sectionId: task.sectionId, tabId: tab.id);
        }
      }
    }
    return null;
  }

  @override
  void dispose() {
    unawaited(_teardown());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: AppTokens.chromeBg,
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Throughput benchmark ($kBenchmarkMode)',
              style: const TextStyle(
                fontFamily: AppTokens.fontFamilyMono,
                color: AppTokens.textPrimary,
                fontSize: 24,
                fontWeight: FontWeight.w600,
              ),
            ),
            const SizedBox(height: 8),
            Text(
              'Pump $kBenchmarkBytes bytes from a daemon-side producer '
              'into the attached tab and measure incoming MB/s. '
              'Mode toggled via --dart-define=THROUGHPUT_BENCHMARK_MODE.',
              style: const TextStyle(
                fontFamily: AppTokens.fontFamilyMono,
                color: AppTokens.textSecondary,
                fontSize: 13,
              ),
            ),
            const SizedBox(height: 24),
            Row(children: [
              FilledButton(
                onPressed: _running ? null : _run,
                child: Text(_running ? 'Running…' : 'Run benchmark'),
              ),
              const SizedBox(width: 16),
              if (_running)
                const SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                ),
            ]),
            const SizedBox(height: 24),
            Text(
              _status,
              style: const TextStyle(
                fontFamily: AppTokens.fontFamilyMono,
                color: AppTokens.textPrimary,
                fontSize: 18,
              ),
            ),
            if (_details.isNotEmpty) ...[
              const SizedBox(height: 6),
              Text(
                _details,
                style: const TextStyle(
                  fontFamily: AppTokens.fontFamilyMono,
                  color: AppTokens.textSecondary,
                  fontSize: 13,
                ),
              ),
            ],
            if (kDebugMode) ...[
              const Spacer(),
              const Text(
                'debug build — release-mode runs are ≥3× faster',
                style: TextStyle(
                  fontFamily: AppTokens.fontFamilyMono,
                  color: AppTokens.textMuted,
                  fontSize: 11,
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

class _TabRef {
  final String sectionId;
  final String tabId;
  _TabRef({required this.sectionId, required this.tabId});
}
