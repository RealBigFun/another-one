// Currently-focused project on the desktop's main pane.
//
// Mirror of GPUI's `WorkspacePane::active_project_page`. Either this
// or `selectedTabProvider` is set at any moment — never both. The
// main area renders the project page when this is `Some`, the
// terminal when `selectedTabProvider` is `Some`, and the welcome
// placeholder when both are `null`.
//
// Coordination lives at call sites: activating a project clears the
// tab; activating a tab clears the project. Centralising the
// discriminated-union into one notifier would be cleaner long-term
// but requires touching every existing read of `selectedTabProvider`
// — leaving as a follow-up so this lands without churn.

import 'package:flutter_riverpod/flutter_riverpod.dart';

final activeProjectPageProvider = StateProvider<String?>((_) => null);
