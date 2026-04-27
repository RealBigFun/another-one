import 'package:flutter/material.dart';

import '../tokens.dart';

void showAppToast(
  BuildContext context, {
  required String message,
  bool warning = true,
}) {
  final messenger = ScaffoldMessenger.maybeOf(context);
  if (messenger == null) {
    return;
  }
  messenger.showSnackBar(
    SnackBar(
      content: Text(message),
      backgroundColor: warning ? AppTokens.errorBg : null,
    ),
  );
}
