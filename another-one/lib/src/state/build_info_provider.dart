// One-shot read of `read_build_info` — values come from `env!()`
// at compile time and never change for the running process, so a
// FutureProvider that resolves once at startup is the right shape.
// The titlebar chip just `.watch`es it and renders an em-dash
// placeholder while the future is in flight (a few ms at most).

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/build_info.dart' as build_info_api;

final buildInfoProvider = FutureProvider<build_info_api.BuildInfo>((_) {
  return build_info_api.readBuildInfo();
});
