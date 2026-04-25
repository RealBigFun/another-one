// Polls the FRB-exposed `read_app_resource_sample` every 1.5s and
// computes CPU% from cumulative-time deltas — same shape the GPUI
// desktop's titlebar resource indicator uses.

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/resources.dart' as resources_api;

class ResourceUsage {
  const ResourceUsage({
    required this.cpuPercent,
    required this.memoryMib,
    required this.totalMemoryMib,
  });

  /// CPU% over the last sampling window — `null` until we've taken
  /// at least two samples (one delta is required).
  final double? cpuPercent;

  /// Resident set size in MiB.
  final double memoryMib;

  /// Total system memory in MiB if the platform reports it
  /// (Linux/macOS yes, Windows no).
  final double? totalMemoryMib;
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
      if (!mounted) return;
      state = ResourceUsage(
        cpuPercent: cpuPct,
        memoryMib: memMib,
        totalMemoryMib: totalMib,
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
