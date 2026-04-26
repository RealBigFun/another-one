// Riverpod surface for the new-task modal's read-side data:
// available branches per project + the user's enabled agents.
//
// Both are cached per FutureProvider key so re-opening the modal
// is instant on the second click. Invalidate after a workspace
// switch (rare) or never (the bridge holds the same store the
// modal is reading from in-process).

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../rust/api/local_session.dart' show EnabledAgentsView;
import 'local_connection_provider.dart';

final projectBranchesProvider =
    FutureProvider.family<List<String>, String>((ref, projectId) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.readProjectBranches(projectId);
  } on UnimplementedError {
    return const [];
  }
});

final primaryBranchProvider =
    FutureProvider.family<String?, String>((ref, projectId) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.primaryBranchForProject(projectId);
  } on UnimplementedError {
    return null;
  }
});

final enabledAgentsProvider =
    FutureProvider<EnabledAgentsView>((ref) async {
  final connection = ref.watch(localConnectionProvider);
  try {
    return await connection.readEnabledAgents();
  } on UnimplementedError {
    return const EnabledAgentsView(agents: []);
  }
});
