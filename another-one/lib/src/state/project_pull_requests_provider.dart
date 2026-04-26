// Riverpod surface for the project page's Open PRs section.
//
// `projectPullRequestsProvider((projectId, filterIndex, query))` is
// a one-shot fetch keyed by the active filter + query. Riverpod
// caches per-key for the session, so flipping back to a previous
// filter doesn't re-shell-out.
//
// Mirrors GPUI's `project_page_pull_requests` HashMap that's keyed
// by `(project_id, filter_index, query)` and lives until the panel
// is dismissed.

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show ProjectPagePullRequestDto;
import 'local_connection_provider.dart';

class ProjectPullRequestsKey {
  const ProjectPullRequestsKey({
    required this.projectId,
    required this.filterIndex,
    required this.query,
  });

  final String projectId;
  final int filterIndex;
  final String query;

  @override
  bool operator ==(Object other) =>
      other is ProjectPullRequestsKey &&
      other.projectId == projectId &&
      other.filterIndex == filterIndex &&
      other.query == query;

  @override
  int get hashCode => Object.hash(projectId, filterIndex, query);
}

final projectPullRequestsProvider = FutureProvider.family<
    List<ProjectPagePullRequestDto>?,
    ProjectPullRequestsKey>((ref, key) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.findProjectPullRequests(
      projectId: key.projectId,
      filterIndex: key.filterIndex,
      query: key.query,
    );
  } on UnimplementedError {
    return null;
  }
});

/// Active filter index + applied query for a project, plus the
/// in-flight draft text in the search input. Mirrors GPUI's
/// `project_page_pr_filter` / `project_page_pr_query` /
/// `project_page_pr_query_draft` triple.
class ProjectPrSearchState {
  const ProjectPrSearchState({
    this.filterIndex = 0,
    this.appliedQuery = '',
    this.draftQuery = '',
  });

  final int filterIndex;
  final String appliedQuery;
  final String draftQuery;

  ProjectPrSearchState copyWith({
    int? filterIndex,
    String? appliedQuery,
    String? draftQuery,
  }) =>
      ProjectPrSearchState(
        filterIndex: filterIndex ?? this.filterIndex,
        appliedQuery: appliedQuery ?? this.appliedQuery,
        draftQuery: draftQuery ?? this.draftQuery,
      );
}

class _ProjectPrSearchNotifier extends StateNotifier<ProjectPrSearchState> {
  _ProjectPrSearchNotifier() : super(const ProjectPrSearchState());

  void setFilter(int index) =>
      state = state.copyWith(filterIndex: index);
  void setDraft(String value) =>
      state = state.copyWith(draftQuery: value);
  void apply() => state = state.copyWith(appliedQuery: state.draftQuery);
  void clear() => state = state.copyWith(appliedQuery: '', draftQuery: '');
}

final projectPrSearchProvider = StateNotifierProvider.family<
    _ProjectPrSearchNotifier, ProjectPrSearchState, String>(
  (_, projectId) => _ProjectPrSearchNotifier(),
);

/// Whether the Open PRs section's collapsible header is expanded.
/// Mirrors GPUI's `project_page_prs_collapsed` (note: GPUI tracks
/// the *collapsed* flag; we invert to "expanded" since that's the
/// default-true convention everywhere else in the codebase).
final openPrsExpandedProvider = StateProvider<bool>((_) => true);
