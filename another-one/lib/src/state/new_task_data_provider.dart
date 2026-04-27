// Riverpod surface for the new-task modal's read-side data:
// available branches per project + the user's enabled agents.
//
// Both are cached per FutureProvider key so re-opening the modal
// is instant on the second click. Invalidate after a workspace
// switch (rare) or never (the bridge holds the same store the
// modal is reading from in-process).

import '../rust/api/local_session.dart'
    show AgentSettingsView, EnabledAgentsView;
import 'connection_future_provider.dart';
import 'local_connection_provider.dart';

final projectBranchesProvider =
    makeConnectionFutureProviderFamily<List<String>, String>(
      read: (_, connection, projectId) =>
          connection.readProjectBranches(projectId),
      fallback: const [],
    );

final primaryBranchProvider =
    makeConnectionFutureProviderFamily<String?, String>(
      read: (_, connection, projectId) =>
          connection.primaryBranchForProject(projectId),
      fallback: null,
    );

final enabledAgentsProvider = makeConnectionFutureProvider<EnabledAgentsView>(
  read: (_, connection) => connection.readEnabledAgents(),
  fallback: const EnabledAgentsView(agents: []),
);

/// Full agent registry — Settings → Agents page reads from here.
/// Refresh after every mutation (set_agent_enabled, etc.) by
/// invalidating this provider.
final agentSettingsProvider = makeConnectionFutureProvider<AgentSettingsView?>(
  read: (_, connection) => connection.readAgentSettings(),
  fallback: null,
  prepare: (_, connection) => waitForConnectedDaemon(connection),
);
