// Helper that wraps a `DaemonConnection` mutation in the
// try/catch/snackbar pattern repeated across the sidebar's
// `_addProject`, project remove, task pin, rename, and delete
// flows. Each call site was the same five-or-so lines:
//
//   try { await fn(); } catch (e) {
//     if (!context.mounted) return;
//     ScaffoldMessenger.of(context).showSnackBar(SnackBar(
//       content: Text('$prefix: $e'), backgroundColor: ...));
//   }
//
// The helper centralises the error path so the call sites become
// `final result = await runMutation(...)`. Success-case snackbars
// (e.g. "Project already added at that path") stay at call sites
// — the helper deliberately doesn't try to encode every
// notification rule.

import 'package:flutter/material.dart';

import '../tokens.dart';

/// Awaits `fn()`, surfaces any thrown error as a red snackbar, and
/// returns the result on success. Returns `null` on error so the
/// caller can branch on `if (result == null) return`.
///
/// `errorPrefix` is concatenated as `"<prefix>: <error>"`. Pass
/// the imperative form ("Failed to add project", not "Add project
/// failed").
Future<T?> runMutation<T>(
  BuildContext context,
  Future<T> Function() fn, {
  required String errorPrefix,
}) async {
  try {
    return await fn();
  } catch (e) {
    if (!context.mounted) return null;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text('$errorPrefix: $e'),
        backgroundColor: AppTokens.errorBg,
      ),
    );
    return null;
  }
}
