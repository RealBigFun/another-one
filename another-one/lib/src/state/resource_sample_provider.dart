// Polls cheap app-only resource samples frequently and refreshes the
// heavier terminal/session tree more slowly while the popover is open.
// This keeps the monitor responsive without making the act of opening
// the monitor dominate the app's own CPU reading.

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/resources.dart' as resources_api;
import '../rust/api/resources.dart'
    show ResourceSample, ResourceUsageSnapshotDto;

const _closedAppRefreshInterval = Duration(seconds: 5);
const _openAppRefreshInterval = Duration(seconds: 1);
const _openDetailRefreshInterval = Duration(seconds: 10);
const _initialDetailRefreshDelay = Duration(milliseconds: 750);
const _maxCpuSampleWindow = Duration(seconds: 15);

class ResourceUsage {
  const ResourceUsage({
    required this.cpuPercent,
    required this.memoryMib,
    this.snapshot,
  });

  final double? cpuPercent;
  final double memoryMib;
  final ResourceUsageSnapshotDto? snapshot;
}

final resourceUsagePopoverOpenProvider = StateProvider<bool>((_) => false);

class _ResourceUsageNotifier extends StateNotifier<ResourceUsage?> {
  _ResourceUsageNotifier() : super(null);

  Timer? _appTimer;
  Timer? _detailTimer;
  Timer? _detailRefreshDelay;
  bool _popoverOpen = false;
  ResourceSample? _previousAppSample;
  Future<void>? _appInflight;
  Future<void>? _detailInflight;

  void setPopoverOpen(bool open, {bool immediate = true}) {
    _popoverOpen = open;
    _restartAppTimer();
    _restartDetailTimer();
    if (immediate) {
      unawaited(_readAppSample());
    }
    if (open) {
      _scheduleInitialDetailRefresh();
    }
  }

  void refreshNow() {
    unawaited(_readAppSample());
    if (_popoverOpen) {
      unawaited(_readDetailSnapshot());
    }
    _restartAppTimer();
    _restartDetailTimer();
  }

  void _restartAppTimer() {
    _appTimer?.cancel();
    final interval = _popoverOpen
        ? _openAppRefreshInterval
        : _closedAppRefreshInterval;
    _appTimer = Timer.periodic(interval, (_) {
      unawaited(_readAppSample());
    });
  }

  void _restartDetailTimer() {
    _detailTimer?.cancel();
    _detailRefreshDelay?.cancel();
    if (!_popoverOpen) return;
    _detailTimer = Timer.periodic(_openDetailRefreshInterval, (_) {
      unawaited(_readDetailSnapshot());
    });
  }

  void _scheduleInitialDetailRefresh() {
    _detailRefreshDelay?.cancel();
    _detailRefreshDelay = Timer(_initialDetailRefreshDelay, () {
      _detailRefreshDelay = null;
      if (_popoverOpen) {
        unawaited(_readDetailSnapshot());
      }
    });
  }

  Future<void> _readAppSample() async {
    final inflight = _appInflight;
    if (inflight != null) {
      return inflight;
    }

    final future = _readAppSampleInner();
    _appInflight = future;
    try {
      await future;
    } finally {
      if (identical(_appInflight, future)) {
        _appInflight = null;
      }
    }
  }

  Future<void> _readAppSampleInner() async {
    try {
      final sample = await resources_api.readAppResourceSample();
      if (!mounted) return;

      final previous = state;
      final cpuPercent = _cpuPercentBetween(_previousAppSample, sample);
      _previousAppSample = sample;
      state = ResourceUsage(
        cpuPercent: cpuPercent ?? previous?.cpuPercent,
        memoryMib: sample.memoryBytes.toDouble() / 1024 / 1024,
        snapshot: previous?.snapshot,
      );
    } catch (_) {
      // Keep the previous state; resource sampling is best-effort.
    }
  }

  Future<void> _readDetailSnapshot() async {
    final inflight = _detailInflight;
    if (inflight != null) {
      return inflight;
    }

    final future = _readDetailSnapshotInner();
    _detailInflight = future;
    try {
      await future;
    } finally {
      if (identical(_detailInflight, future)) {
        _detailInflight = null;
      }
    }
  }

  Future<void> _readDetailSnapshotInner() async {
    try {
      final snapshot = await resources_api.readResourceUsageSnapshot();
      if (!mounted) return;

      if (snapshot == null) {
        final previous = state;
        state = ResourceUsage(
          cpuPercent: previous?.cpuPercent,
          memoryMib: previous?.memoryMib ?? 0,
          snapshot: null,
        );
        return;
      }

      final previous = state;
      state = ResourceUsage(
        cpuPercent: previous?.cpuPercent ?? snapshot.appCpuPercent,
        memoryMib:
            previous?.memoryMib ??
            snapshot.appMemoryBytes.toDouble() / 1024 / 1024,
        snapshot: snapshot,
      );
    } catch (_) {
      // Keep the previous state; resource sampling is best-effort.
    }
  }

  @override
  void dispose() {
    _appTimer?.cancel();
    _detailTimer?.cancel();
    _detailRefreshDelay?.cancel();
    super.dispose();
  }
}

double? _cpuPercentBetween(ResourceSample? previous, ResourceSample current) {
  if (previous == null) return null;

  final elapsedMs = current.timestampMs - previous.timestampMs;
  if (elapsedMs <= BigInt.zero) return 0;
  if (elapsedMs > BigInt.from(_maxCpuSampleWindow.inMilliseconds)) {
    return null;
  }

  final cpuDeltaNs = current.cpuTimeNs - previous.cpuTimeNs;
  if (cpuDeltaNs <= BigInt.zero) return 0;
  final elapsedNs = elapsedMs.toDouble() * 1000000.0;
  return cpuDeltaNs.toDouble() / elapsedNs * 100.0;
}

final resourceUsageProvider =
    StateNotifierProvider<_ResourceUsageNotifier, ResourceUsage?>((ref) {
      final notifier = _ResourceUsageNotifier();
      notifier.setPopoverOpen(
        ref.read(resourceUsagePopoverOpenProvider),
        immediate: true,
      );
      ref.listen<bool>(resourceUsagePopoverOpenProvider, (_, next) {
        notifier.setPopoverOpen(next);
      });
      return notifier;
    });
