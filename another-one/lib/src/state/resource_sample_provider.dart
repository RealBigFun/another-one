// Polls the FRB-exposed `read_app_resource_sample` every 1.5s and
// computes CPU% from cumulative-time deltas. Stays in Dart on
// purpose: the Rust layer is meant to be runnable as a headless
// daemon, and the delta math + label rounding are display-layer
// concerns that don't need to cross the interop boundary on every
// tick.

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/resources.dart' as resources_api;
import '../rust/api/resources.dart' show ResourceUsageSnapshotDto;

class ResourceUsage {
  const ResourceUsage({
    required this.cpuPercent,
    required this.memoryMib,
    required this.totalMemoryMib,
    this.snapshot,
  });

  /// CPU% over the last sampling window — `null` until we've taken
  /// at least two samples (one delta is required).
  final double? cpuPercent;

  /// Resident set size in MiB.
  final double memoryMib;

  /// Total system memory in MiB if the platform reports it
  /// (Linux/macOS yes, Windows no).
  final double? totalMemoryMib;

  /// Hierarchical snapshot — APP SHELL row + project → task → session
  /// tree. `null` until the embedded daemon has booted and the first
  /// `read_resource_usage_snapshot` returns. Populated on every tick
  /// once available.
  final ResourceUsageSnapshotDto? snapshot;
}

class _ResourceUsageNotifier extends StateNotifier<ResourceUsage?> {
  _ResourceUsageNotifier() : super(null) {
    _start();
  }

  Timer? _timer;
  BigInt? _lastCpuTimeNs;
  BigInt? _lastTimestampMs;

  void _start() {
    // Take an initial sample, then poll every 1.5s — matches the
    // GPUI desktop's `RESOURCE_USAGE_REFRESH_INTERVAL`.
    unawaited(_tick());
    _timer = Timer.periodic(const Duration(milliseconds: 1500), (_) {
      unawaited(_tick());
    });
  }

  Future<void> _tick() async {
    try {
      final sample = await resources_api.readAppResourceSample();
      final memMib = sample.memoryBytes.toDouble() / 1024 / 1024;
      final totalMib = sample.totalMemoryBytes == null
          ? null
          : sample.totalMemoryBytes!.toDouble() / 1024 / 1024;
      double? cpuPct;
      final lastCpu = _lastCpuTimeNs;
      final lastTs = _lastTimestampMs;
      if (lastCpu != null && lastTs != null) {
        final dCpu = (sample.cpuTimeNs - lastCpu).toDouble();
        final dTimeMs = (sample.timestampMs - lastTs).toDouble();
        if (dTimeMs > 0 && dCpu >= 0) {
          // ns / (ms * 1e6) = fraction; * 100 = percent.
          cpuPct = (dCpu / (dTimeMs * 1e6)) * 100;
        }
      }
      _lastCpuTimeNs = sample.cpuTimeNs;
      _lastTimestampMs = sample.timestampMs;
      // Pull the hierarchical tree on the same tick — it has its own
      // delta-state on the Rust side, so calling it once per 1.5s
      // matches the GPUI desktop's `RESOURCE_SAMPLE_INTERVAL`.
      ResourceUsageSnapshotDto? snapshot;
      try {
        snapshot = await resources_api.readResourceUsageSnapshot();
      } catch (_) {
        snapshot = null;
      }
      if (!mounted) return;
      state = ResourceUsage(
        cpuPercent: cpuPct,
        memoryMib: memMib,
        totalMemoryMib: totalMib,
        snapshot: snapshot,
      );
    } catch (_) {
      // Don't surface — the indicator just shows the previous value
      // (or em-dashes on first failure). Sampling errors are noisy
      // on platforms where `read_process_samples` returns nothing.
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }
}

final resourceUsageProvider =
    StateNotifierProvider<_ResourceUsageNotifier, ResourceUsage?>(
  (_) => _ResourceUsageNotifier(),
);
