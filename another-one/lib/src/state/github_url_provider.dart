// Per-project GitHub remote URL cache.
//
// Mirrors GPUI's `project_github_links` map: at boot, the desktop
// looks up each project's `origin` remote, normalises any github.com
// URL, and stores the result keyed by project id. The sidebar's
// hover-only GitHub icon and the (future) titlebar GitHub button
// both read from this cache.
//
// Wrapped as `FutureProvider.family` so each project's lookup runs
// on first read and is cached for the rest of the session. The
// underlying `read_project_github_url` shells out to git in a
// `spawn_blocking` task — cheap (~one syscall) but not free, so we
// don't repoll. If the user changes the remote at runtime the
// stale value persists; matches GPUI's "look up once at boot"
// behaviour.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'local_connection_provider.dart';

final projectGithubUrlProvider =
    FutureProvider.family<String?, String>((ref, projectId) async {
  final transport = ref.watch(localConnectionProvider);
  return transport.readProjectGithubUrl(projectId);
});
