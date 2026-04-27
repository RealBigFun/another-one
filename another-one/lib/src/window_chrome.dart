import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

const _windowChromeChannel = MethodChannel('another_one/window_chrome');

bool get _supportsWindowChrome {
  if (kIsWeb) {
    return false;
  }
  switch (defaultTargetPlatform) {
    case TargetPlatform.linux:
    case TargetPlatform.macOS:
      return true;
    default:
      return false;
  }
}

Future<void> startNativeWindowDrag(Offset globalPosition) async {
  if (!_supportsWindowChrome) {
    return;
  }
  try {
    await _windowChromeChannel.invokeMethod<void>('startWindowDrag', {
      'x': globalPosition.dx.round(),
      'y': globalPosition.dy.round(),
    });
  } on MissingPluginException {
    // Desktop-only best-effort affordance.
  } on PlatformException {
    // Desktop-only best-effort affordance.
  }
}

Future<void> toggleNativeWindowMaximize() async {
  if (!_supportsWindowChrome) {
    return;
  }
  try {
    await _windowChromeChannel.invokeMethod<void>('toggleMaximize');
  } on MissingPluginException {
    // Desktop-only best-effort affordance.
  } on PlatformException {
    // Desktop-only best-effort affordance.
  }
}
