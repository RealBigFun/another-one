// App-level logging helper.
//
// Wraps `dart:developer.log()` so:
//   * DevTools' Logging tab gets a structured event with our
//     `aone.<module>` prefix (filterable + leveled).
//   * `flutter run`'s stdout gets a single one-line summary
//     (`<HH:MM:SS> [aone.<module>] LEVEL: message`).
//   * A side-channel writes the same events to
//     `/tmp/aone-debug.log` (overwritten at boot) so an outside
//     observer can `tail -F` an app-only stream — much cleaner
//     than wading through GTK init noise + Material warnings in
//     the full `flutter run` log.
//
// Use:
//   const log = Log('aone.titlebar');   // top of file
//   log.info('built chip', {'profile': info.profile});
//   log.warn('missing branch', {'project': projectId});
//   log.error('save failed', error: e, stackTrace: s);
//
// `error` and `stackTrace` are passed through to `developer.log`,
// so DevTools renders them in the structured-error panel. The
// stdout / file lines stay one line for grep-ability.

import 'dart:async';
import 'dart:convert';
import 'dart:developer' as developer;
import 'dart:io';

const _logFilePath = '/tmp/aone-debug.log';

enum LogLevel {
  debug(500, 'DEBUG'),
  info(800, 'INFO'),
  warn(900, 'WARN'),
  error(1000, 'ERROR');

  const LogLevel(this.severity, this.label);

  /// `dart:developer.log()` severity. Mirrors the Java logger
  /// constants the Dart team standardised on (FINE=500, INFO=800,
  /// WARNING=900, SEVERE=1000).
  final int severity;
  final String label;
}

/// Per-module logger. Cheap to construct; safe to keep at file
/// scope as `const`.
class Log {
  const Log(this.name);

  final String name;

  void debug(
    String message, [
    Map<String, Object?>? fields,
  ]) =>
      _emit(LogLevel.debug, message, fields, null, null);

  void info(
    String message, [
    Map<String, Object?>? fields,
  ]) =>
      _emit(LogLevel.info, message, fields, null, null);

  void warn(
    String message, [
    Map<String, Object?>? fields,
  ]) =>
      _emit(LogLevel.warn, message, fields, null, null);

  void error(
    String message, {
    Map<String, Object?>? fields,
    Object? error,
    StackTrace? stackTrace,
  }) =>
      _emit(LogLevel.error, message, fields, error, stackTrace);

  void _emit(
    LogLevel level,
    String message,
    Map<String, Object?>? fields,
    Object? error,
    StackTrace? stackTrace,
  ) {
    final fieldsStr = (fields == null || fields.isEmpty)
        ? ''
        : ' ${jsonEncode(fields)}';
    final line =
        '${_timestamp()} [$name] ${level.label}: $message$fieldsStr';
    // 1. DevTools (rich, structured)
    developer.log(
      message + fieldsStr,
      name: name,
      level: level.severity,
      error: error,
      stackTrace: stackTrace,
    );
    // 2. stderr — appears in `flutter run` stdout
    stderr.writeln(line);
    if (error != null) {
      stderr.writeln('  error: $error');
    }
    if (stackTrace != null) {
      stderr.writeln(stackTrace.toString().trimRight());
    }
    // 3. File tee
    _appendToFile(line, error: error, stackTrace: stackTrace);
  }
}

String _timestamp() {
  final n = DateTime.now();
  String two(int x) => x.toString().padLeft(2, '0');
  String three(int x) => x.toString().padLeft(3, '0');
  return '${two(n.hour)}:${two(n.minute)}:${two(n.second)}.${three(n.millisecond)}';
}

IOSink? _logFileSink;
bool _logFileInitFailed = false;

void _appendToFile(
  String line, {
  Object? error,
  StackTrace? stackTrace,
}) {
  if (_logFileInitFailed) return;
  try {
    final sink = _logFileSink ??= File(_logFilePath).openWrite(
      mode: FileMode.writeOnlyAppend,
    );
    sink.writeln(line);
    if (error != null) {
      sink.writeln('  error: $error');
    }
    if (stackTrace != null) {
      sink.writeln(stackTrace.toString().trimRight());
    }
  } on FileSystemException {
    // Don't let a broken /tmp kill the app — flip the kill-switch
    // so we stop trying to tee on every event.
    _logFileInitFailed = true;
  }
}

/// Truncate the log file at boot so each `flutter run` session
/// gets a clean view. Call once from `main()` before any other
/// log emission.
Future<void> initLogFile() async {
  try {
    final f = File(_logFilePath);
    await f.writeAsString(
      '# aone log — opened ${DateTime.now().toIso8601String()}\n',
    );
  } on FileSystemException {
    _logFileInitFailed = true;
  }
}

/// Flush + close the log file sink. Call from a shutdown hook
/// so the last few buffered lines actually hit disk.
Future<void> closeLogFile() async {
  final sink = _logFileSink;
  _logFileSink = null;
  if (sink == null) return;
  try {
    await sink.flush();
    await sink.close();
  } on Object {
    // Best-effort — swallow.
  }
}

/// Convenience: wraps `runZonedGuarded` so unhandled async
/// errors land in the same sink as everything else. Use as:
///   runGuardedApp(() => runApp(MyApp()));
void runGuardedApp(void Function() body) {
  const log = Log('aone.uncaught');
  runZonedGuarded(
    body,
    (error, stack) {
      log.error(
        'uncaught error',
        error: error,
        stackTrace: stack,
      );
    },
  );
}
