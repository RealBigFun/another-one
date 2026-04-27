import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../connection.dart';
import 'local_connection_provider.dart';

typedef ConnectionFutureRead<T> =
    Future<T> Function(Ref ref, DaemonConnection connection);
typedef ConnectionFuturePrepare =
    Future<void> Function(Ref ref, DaemonConnection connection);
typedef ConnectionFamilyFutureRead<T, K extends Object> =
    Future<T> Function(Ref ref, DaemonConnection connection, K key);
typedef ConnectionFamilyFuturePrepare<K extends Object> =
    Future<void> Function(Ref ref, DaemonConnection connection, K key);

/// Build a one-shot provider that reads through the active daemon
/// connection and falls back to a caller-supplied default when that
/// connection type has not implemented the verb yet.
FutureProvider<T> makeConnectionFutureProvider<T>({
  required ConnectionFutureRead<T> read,
  required T fallback,
  ConnectionFuturePrepare? prepare,
}) {
  return FutureProvider<T>((ref) async {
    final connection = ref.watch(localConnectionProvider);
    if (prepare != null) {
      await prepare(ref, connection);
    }
    try {
      return await read(ref, connection);
    } on UnimplementedError {
      return fallback;
    }
  });
}

/// `family` variant of [makeConnectionFutureProvider] for project-
/// scoped or composite-key reads.
FutureProviderFamily<T, K>
makeConnectionFutureProviderFamily<T, K extends Object>({
  required ConnectionFamilyFutureRead<T, K> read,
  required T fallback,
  ConnectionFamilyFuturePrepare<K>? prepare,
}) {
  return FutureProvider.family<T, K>((ref, key) async {
    final connection = ref.watch(localConnectionProvider);
    if (prepare != null) {
      await prepare(ref, connection, key);
    }
    try {
      return await read(ref, connection, key);
    } on UnimplementedError {
      return fallback;
    }
  });
}
