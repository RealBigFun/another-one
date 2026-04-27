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
      _setStatus('connecting…');
      final transport = _buildTransport();
      _transport = transport;
      await _waitForConnected(transport);

      // Pull the project tree so we can pick a real (sectionId, tabId).
      _setStatus('discovering tab…');
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
      _log.info('benchmark picked tab', {
        'section_id': picked.sectionId,
        'tab_id': picked.tabId,
        'is_shell': picked.isShell,
      });
      if (!picked.isShell) {
        setState(() {
          _running = false;
          _status = 'no shell tab found — every existing tab is an '
              'agent (Claude / Cursor / Codex / etc). Open a plain '
              'shell tab in the regular UI and re-run the benchmark.';
        });
        return;
      }

      _setStatus('launching tab…');
      await transport.launchTab(
        sectionId: picked.sectionId,
        tabId: picked.tabId,
      );
      // The daemon's launch_tab just queues the request — the actual
      // PTY spawn happens async on the pty-drain task, so the broadcast
      // for this (section_id, tab_id) doesn't exist immediately. Poll
      // listProjects until the tab reports `running: true`, then
      // attach. Without this gate, attachTab fires too early, the
      // daemon emits "attach_tab: no such live runtime", and the data
      // stream stays silent — exactly the symptom seen in the first
      // benchmark attempt.
      _setStatus('waiting for PTY…');
      final live = await _waitForRunning(transport, picked,
          Duration(seconds: 10));
      if (!live) {
        setState(() {
          _running = false;
          _status = 'gave up waiting for the daemon to mark the tab '
              'running — check daemon logs';
        });
        return;
      }
      _setStatus('attaching…');
      await transport.attachTab(
        sectionId: picked.sectionId,
        tabId: picked.tabId,
      );
      _setStatus('streaming…');

      // Drive a high-bandwidth producer through the tab's stdin.
      // `head -c <N>` bounds the run; `base64` keeps bytes printable
      // so xterm doesn't crash on stray control sequences. We don't
      // try to disable the shell's command-line echo — the previous
      // attempt with `stty -echo + sentinel` fired off the command's
      // own echo (the shell prints the line back as you "type" it),
      // ending the run in 20 ms. Instead, measure throughput from
      // the FIRST inbound byte (skipping prompt + command echo) until
      // the byte stream stalls for [stallWindow] (= producer ran out).
      final cmd =
          "cat /dev/urandom | base64 | head -c $kBenchmarkBytes\n";
      transport.sendBytes(cmd.codeUnits);

      const stallWindow = Duration(milliseconds: 1500);
      final completer = Completer<void>();
      DateTime? firstByteAt;
      DateTime lastByteAt = DateTime.now();
      int totalBytes = 0;
      Timer stallTimer = Timer.periodic(
        const Duration(milliseconds: 250),
        (timer) {
          if (firstByteAt == null) return;
          if (DateTime.now().difference(lastByteAt) >= stallWindow &&
              !completer.isCompleted) {
            timer.cancel();
            completer.complete();
          }
        },
      );
      final deadlineTimer = Timer(
        Duration(seconds: kBenchmarkTimeoutS),
        () {
          if (!completer.isCompleted) completer.complete();
        },
      );
      _bytesSub = transport.incoming.listen((chunk) {
        lastByteAt = DateTime.now();
        firstByteAt ??= lastByteAt;
        totalBytes += chunk.lengthInBytes;
      });

      await completer.future;
      stallTimer.cancel();
      deadlineTimer.cancel();
      final start = firstByteAt ?? DateTime.now();
      final elapsed = lastByteAt.difference(start);

      final mbps = elapsed.inMilliseconds == 0
          ? 0.0
          : totalBytes / 1e6 / elapsed.inMilliseconds * 1000;
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
        _details = '${_formatBytes(totalBytes)} in '
            '${elapsed.inMilliseconds} ms '
            '(target ${kBenchmarkBytes ~/ 1000000} MB; '
            '${kBenchmarkTimeoutS}s timeout). '
            '${totalBytes < 100000 ? "tiny — daemon broadcast probably "
                "bailed on lag (terminal-correctness policy in "
                "daemon-sandbox/src/transport_iroh.rs); a release-mode "
                "client should saturate the broadcast cleanly. " : ""}'
            'measured from first inbound byte to last; stall '
            'window 1.5s.';
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

  void _setStatus(String text) {
    if (!mounted) return;
    setState(() => _status = text);
    _log.info('benchmark status', {'status': text});
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

  /// Poll [DaemonConnection.listProjects] until [_TabRef] surfaces as
  /// `running: true`, or [maxWait] elapses. The daemon publishes
  /// `running` flips on every project-list snapshot so this is the
  /// only signal a remote client has that the PTY actually came up.
  Future<bool> _waitForRunning(
    DaemonConnection transport,
    _TabRef target,
    Duration maxWait,
  ) async {
    final deadline = DateTime.now().add(maxWait);
    while (DateTime.now().isBefore(deadline)) {
      final completer = Completer<List<ProjectSummary>>();
      final sub = transport.workerReplies.listen((reply) {
        if (reply is WorkerReply_ProjectList && !completer.isCompleted) {
          completer.complete(reply.projects);
        }
      });
      await transport.listProjects();
      try {
        final projects = await completer.future
            .timeout(const Duration(seconds: 2));
        await sub.cancel();
        for (final p in projects) {
          for (final task in p.tasks) {
            if (task.sectionId != target.sectionId) continue;
            for (final tab in task.tabs) {
              if (tab.id == target.tabId && tab.running) return true;
            }
          }
        }
      } on TimeoutException {
        await sub.cancel();
      }
      await Future.delayed(const Duration(milliseconds: 200));
    }
    return false;
  }

  /// Prefer the first tab whose `provider == null` (plain shell);
  /// fall back to the first tab of any kind if no shell tab exists.
  /// Skipping agent tabs matters for the benchmark because the
  /// producer command (`cat /dev/urandom | base64 | head -c …`) is
  /// shell-only — sent to Claude Code / Cursor / Codex / Gemini /
  /// etc. it parses as a prompt, generates a short response, and
  /// stalls waiting for more input. (Observed: 42 KB total before
  /// stall, mistaken for a daemon throughput limit.)
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
    _TabRef? fallback;
    for (final p in projects) {
      for (final task in p.tasks) {
        for (final tab in task.tabs) {
          fallback ??= _TabRef(
              sectionId: task.sectionId,
              tabId: tab.id,
              isShell: tab.provider == null);
          if (tab.provider == null) {
            return _TabRef(
                sectionId: task.sectionId, tabId: tab.id, isShell: true);
          }
        }
      }
    }
    return fallback;
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

String _formatBytes(int n) {
  if (n < 1000) return '$n B';
  if (n < 1000 * 1000) return '${(n / 1000).toStringAsFixed(1)} KB';
  if (n < 1000 * 1000 * 1000) {
    return '${(n / 1e6).toStringAsFixed(1)} MB';
  }
  return '${(n / 1e9).toStringAsFixed(2)} GB';
}

class _TabRef {
  final String sectionId;
  final String tabId;
  final bool isShell;
  _TabRef({
    required this.sectionId,
    required this.tabId,
    required this.isShell,
  });
}
