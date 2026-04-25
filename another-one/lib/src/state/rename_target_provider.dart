// Tracks which task row is currently in inline-rename mode.
//
// At most one task can be edited at a time. The sidebar's task
// row watches this; if its `task.id` matches, it swaps the static
// name `Text` for an inline TextField (mirrors GPUI's
// `SidebarTaskRenameState` row-id check in left_sidebar.rs).

import 'package:flutter_riverpod/flutter_riverpod.dart';

final renameTargetTaskIdProvider = StateProvider<String?>((_) => null);
