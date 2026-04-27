// Riverpod surface for custom Project Actions.
//
// `projectActionsProvider(projectId)` returns the merged
// per-project + global action list, in the same order GPUI's
// titlebar dropdown renders.
//
// `lastUsedCustomActionIdProvider` mirrors GPUI's in-memory
// `last_used_custom_action_id`: it picks which action the
// titlebar primary half runs by default. State only — never
// persisted to disk; resets on app restart, same as GPUI.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show ProjectActionDto;
import 'connection_future_provider.dart';

final projectActionsProvider =
    makeConnectionFutureProviderFamily<List<ProjectActionDto>, String>(
      read: (_, connection, projectId) =>
          connection.listProjectActions(projectId),
      fallback: const [],
    );

/// In-memory id of the last action the user ran from the
/// titlebar split-button. The button uses this to pick a
/// "selected action" in the same way GPUI does — most recent
/// click wins, with a fallback to the first available action.
final lastUsedCustomActionIdProvider = StateProvider<String?>((_) => null);
