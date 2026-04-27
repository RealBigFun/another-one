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
  return _invokeWindowChrome('startWindowDrag', {
    'x': globalPosition.dx.round(),
    'y': globalPosition.dy.round(),
  });
}

Future<void> toggleNativeWindowMaximize() async {
  return _invokeWindowChrome('toggleMaximize');
}

Future<void> minimizeNativeWindow() async {
  return _invokeWindowChrome('minimizeWindow');
}

Future<void> closeNativeWindow() async {
  return _invokeWindowChrome('closeWindow');
}

Future<void> _invokeWindowChrome(String method, [Object? arguments]) async {
  if (!_supportsWindowChrome) {
    return;
  }
  try {
    await _windowChromeChannel.invokeMethod<void>(method, arguments);
  } on MissingPluginException {
    // Desktop-only best-effort affordance.
  } on PlatformException {
    // Desktop-only best-effort affordance.
  }
}
