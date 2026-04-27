// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'iroh_client.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$WorkerReply {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'WorkerReply()';
}


}

/// @nodoc
class $WorkerReplyCopyWith<$Res>  {
$WorkerReplyCopyWith(WorkerReply _, $Res Function(WorkerReply) __);
}


/// Adds pattern-matching-related methods to [WorkerReply].
extension WorkerReplyPatterns on WorkerReply {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( WorkerReply_ProjectList value)?  projectList,TResult Function( WorkerReply_ProjectAdded value)?  projectAdded,TResult Function( WorkerReply_ProjectRemoved value)?  projectRemoved,TResult Function( WorkerReply_Err value)?  err,TResult Function( WorkerReply_TaskCreated value)?  taskCreated,TResult Function( WorkerReply_TaskRenamed value)?  taskRenamed,TResult Function( WorkerReply_TaskPinned value)?  taskPinned,TResult Function( WorkerReply_TaskRemoved value)?  taskRemoved,TResult Function( WorkerReply_SlugifyBranchNameAck value)?  slugifyBranchNameAck,TResult Function( WorkerReply_ProjectBranchesAck value)?  projectBranchesAck,TResult Function( WorkerReply_PrimaryBranchAck value)?  primaryBranchAck,TResult Function( WorkerReply_RepoDefaultCommitActionAck value)?  repoDefaultCommitActionAck,TResult Function( WorkerReply_ActiveGitStateAck value)?  activeGitStateAck,TResult Function( WorkerReply_ChangedFilesAck value)?  changedFilesAck,TResult Function( WorkerReply_ProjectGithubUrlAck value)?  projectGithubUrlAck,TResult Function( WorkerReply_RecentCommitsAck value)?  recentCommitsAck,TResult Function( WorkerReply_CommitFileChangesAck value)?  commitFileChangesAck,TResult Function( WorkerReply_BranchCompareAck value)?  branchCompareAck,TResult Function( WorkerReply_BranchSettingsAck value)?  branchSettingsAck,TResult Function( WorkerReply_SetBranchSettingAck value)?  setBranchSettingAck,TResult Function( WorkerReply_StageChangedFileAck value)?  stageChangedFileAck,TResult Function( WorkerReply_UnstageChangedFileAck value)?  unstageChangedFileAck,TResult Function( WorkerReply_StageAllChangesAck value)?  stageAllChangesAck,TResult Function( WorkerReply_UnstageAllChangesAck value)?  unstageAllChangesAck,TResult Function( WorkerReply_DiscardChangedFileAck value)?  discardChangedFileAck,TResult Function( WorkerReply_DiscardAllChangesAck value)?  discardAllChangesAck,TResult Function( WorkerReply_ToolbarActionOutcomeAck value)?  toolbarActionOutcomeAck,TResult Function( WorkerReply_CreateBranchAck value)?  createBranchAck,TResult Function( WorkerReply_CreateReviewTaskAck value)?  createReviewTaskAck,TResult Function( WorkerReply_PullRequestStatusAck value)?  pullRequestStatusAck,TResult Function( WorkerReply_PullRequestChecksAck value)?  pullRequestChecksAck,TResult Function( WorkerReply_ProjectPullRequestsAck value)?  projectPullRequestsAck,TResult Function( WorkerReply_OpenInStateAck value)?  openInStateAck,TResult Function( WorkerReply_ProjectActionsAck value)?  projectActionsAck,TResult Function( WorkerReply_EnabledAgentsAck value)?  enabledAgentsAck,TResult Function( WorkerReply_SubmitNewTaskAck value)?  submitNewTaskAck,TResult Function( WorkerReply_AddAgentToSectionAck value)?  addAgentToSectionAck,TResult Function( WorkerReply_ActivateSectionTabAck value)?  activateSectionTabAck,TResult Function( WorkerReply_CloseSectionTabAck value)?  closeSectionTabAck,TResult Function( WorkerReply_ToggleSectionTabPinnedAck value)?  toggleSectionTabPinnedAck,TResult Function( WorkerReply_AgentSettingsAck value)?  agentSettingsAck,TResult Function( WorkerReply_SetAgentEnabledAck value)?  setAgentEnabledAck,TResult Function( WorkerReply_SetDefaultAgentAck value)?  setDefaultAgentAck,TResult Function( WorkerReply_SetAgentLaunchArgsAck value)?  setAgentLaunchArgsAck,TResult Function( WorkerReply_OpenInSettingsAck value)?  openInSettingsAck,TResult Function( WorkerReply_SetOpenInAppEnabledAck value)?  setOpenInAppEnabledAck,TResult Function( WorkerReply_OpenProjectInAppAck value)?  openProjectInAppAck,TResult Function( WorkerReply_RunProjectActionAck value)?  runProjectActionAck,TResult Function( WorkerReply_SaveProjectActionAck value)?  saveProjectActionAck,TResult Function( WorkerReply_DeleteProjectActionAck value)?  deleteProjectActionAck,TResult Function( WorkerReply_GitActionScriptsAck value)?  gitActionScriptsAck,TResult Function( WorkerReply_SetGitCommitScriptAck value)?  setGitCommitScriptAck,TResult Function( WorkerReply_ResetGitCommitScriptAck value)?  resetGitCommitScriptAck,TResult Function( WorkerReply_SetGitPrScriptAck value)?  setGitPrScriptAck,TResult Function( WorkerReply_ResetGitPrScriptAck value)?  resetGitPrScriptAck,TResult Function( WorkerReply_ShortcutSettingsAck value)?  shortcutSettingsAck,TResult Function( WorkerReply_SetShortcutBindingAck value)?  setShortcutBindingAck,TResult Function( WorkerReply_ResetShortcutBindingAck value)?  resetShortcutBindingAck,TResult Function( WorkerReply_McpSettingsAck value)?  mcpSettingsAck,TResult Function( WorkerReply_McpAddFromCatalogAck value)?  mcpAddFromCatalogAck,TResult Function( WorkerReply_McpToggleAck value)?  mcpToggleAck,TResult Function( WorkerReply_McpRemoveAck value)?  mcpRemoveAck,required TResult orElse(),}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that);case WorkerReply_ProjectAdded() when projectAdded != null:
return projectAdded(_that);case WorkerReply_ProjectRemoved() when projectRemoved != null:
return projectRemoved(_that);case WorkerReply_Err() when err != null:
return err(_that);case WorkerReply_TaskCreated() when taskCreated != null:
return taskCreated(_that);case WorkerReply_TaskRenamed() when taskRenamed != null:
return taskRenamed(_that);case WorkerReply_TaskPinned() when taskPinned != null:
return taskPinned(_that);case WorkerReply_TaskRemoved() when taskRemoved != null:
return taskRemoved(_that);case WorkerReply_SlugifyBranchNameAck() when slugifyBranchNameAck != null:
return slugifyBranchNameAck(_that);case WorkerReply_ProjectBranchesAck() when projectBranchesAck != null:
return projectBranchesAck(_that);case WorkerReply_PrimaryBranchAck() when primaryBranchAck != null:
return primaryBranchAck(_that);case WorkerReply_RepoDefaultCommitActionAck() when repoDefaultCommitActionAck != null:
return repoDefaultCommitActionAck(_that);case WorkerReply_ActiveGitStateAck() when activeGitStateAck != null:
return activeGitStateAck(_that);case WorkerReply_ChangedFilesAck() when changedFilesAck != null:
return changedFilesAck(_that);case WorkerReply_ProjectGithubUrlAck() when projectGithubUrlAck != null:
return projectGithubUrlAck(_that);case WorkerReply_RecentCommitsAck() when recentCommitsAck != null:
return recentCommitsAck(_that);case WorkerReply_CommitFileChangesAck() when commitFileChangesAck != null:
return commitFileChangesAck(_that);case WorkerReply_BranchCompareAck() when branchCompareAck != null:
return branchCompareAck(_that);case WorkerReply_BranchSettingsAck() when branchSettingsAck != null:
return branchSettingsAck(_that);case WorkerReply_SetBranchSettingAck() when setBranchSettingAck != null:
return setBranchSettingAck(_that);case WorkerReply_StageChangedFileAck() when stageChangedFileAck != null:
return stageChangedFileAck(_that);case WorkerReply_UnstageChangedFileAck() when unstageChangedFileAck != null:
return unstageChangedFileAck(_that);case WorkerReply_StageAllChangesAck() when stageAllChangesAck != null:
return stageAllChangesAck(_that);case WorkerReply_UnstageAllChangesAck() when unstageAllChangesAck != null:
return unstageAllChangesAck(_that);case WorkerReply_DiscardChangedFileAck() when discardChangedFileAck != null:
return discardChangedFileAck(_that);case WorkerReply_DiscardAllChangesAck() when discardAllChangesAck != null:
return discardAllChangesAck(_that);case WorkerReply_ToolbarActionOutcomeAck() when toolbarActionOutcomeAck != null:
return toolbarActionOutcomeAck(_that);case WorkerReply_CreateBranchAck() when createBranchAck != null:
return createBranchAck(_that);case WorkerReply_CreateReviewTaskAck() when createReviewTaskAck != null:
return createReviewTaskAck(_that);case WorkerReply_PullRequestStatusAck() when pullRequestStatusAck != null:
return pullRequestStatusAck(_that);case WorkerReply_PullRequestChecksAck() when pullRequestChecksAck != null:
return pullRequestChecksAck(_that);case WorkerReply_ProjectPullRequestsAck() when projectPullRequestsAck != null:
return projectPullRequestsAck(_that);case WorkerReply_OpenInStateAck() when openInStateAck != null:
return openInStateAck(_that);case WorkerReply_ProjectActionsAck() when projectActionsAck != null:
return projectActionsAck(_that);case WorkerReply_EnabledAgentsAck() when enabledAgentsAck != null:
return enabledAgentsAck(_that);case WorkerReply_SubmitNewTaskAck() when submitNewTaskAck != null:
return submitNewTaskAck(_that);case WorkerReply_AddAgentToSectionAck() when addAgentToSectionAck != null:
return addAgentToSectionAck(_that);case WorkerReply_ActivateSectionTabAck() when activateSectionTabAck != null:
return activateSectionTabAck(_that);case WorkerReply_CloseSectionTabAck() when closeSectionTabAck != null:
return closeSectionTabAck(_that);case WorkerReply_ToggleSectionTabPinnedAck() when toggleSectionTabPinnedAck != null:
return toggleSectionTabPinnedAck(_that);case WorkerReply_AgentSettingsAck() when agentSettingsAck != null:
return agentSettingsAck(_that);case WorkerReply_SetAgentEnabledAck() when setAgentEnabledAck != null:
return setAgentEnabledAck(_that);case WorkerReply_SetDefaultAgentAck() when setDefaultAgentAck != null:
return setDefaultAgentAck(_that);case WorkerReply_SetAgentLaunchArgsAck() when setAgentLaunchArgsAck != null:
return setAgentLaunchArgsAck(_that);case WorkerReply_OpenInSettingsAck() when openInSettingsAck != null:
return openInSettingsAck(_that);case WorkerReply_SetOpenInAppEnabledAck() when setOpenInAppEnabledAck != null:
return setOpenInAppEnabledAck(_that);case WorkerReply_OpenProjectInAppAck() when openProjectInAppAck != null:
return openProjectInAppAck(_that);case WorkerReply_RunProjectActionAck() when runProjectActionAck != null:
return runProjectActionAck(_that);case WorkerReply_SaveProjectActionAck() when saveProjectActionAck != null:
return saveProjectActionAck(_that);case WorkerReply_DeleteProjectActionAck() when deleteProjectActionAck != null:
return deleteProjectActionAck(_that);case WorkerReply_GitActionScriptsAck() when gitActionScriptsAck != null:
return gitActionScriptsAck(_that);case WorkerReply_SetGitCommitScriptAck() when setGitCommitScriptAck != null:
return setGitCommitScriptAck(_that);case WorkerReply_ResetGitCommitScriptAck() when resetGitCommitScriptAck != null:
return resetGitCommitScriptAck(_that);case WorkerReply_SetGitPrScriptAck() when setGitPrScriptAck != null:
return setGitPrScriptAck(_that);case WorkerReply_ResetGitPrScriptAck() when resetGitPrScriptAck != null:
return resetGitPrScriptAck(_that);case WorkerReply_ShortcutSettingsAck() when shortcutSettingsAck != null:
return shortcutSettingsAck(_that);case WorkerReply_SetShortcutBindingAck() when setShortcutBindingAck != null:
return setShortcutBindingAck(_that);case WorkerReply_ResetShortcutBindingAck() when resetShortcutBindingAck != null:
return resetShortcutBindingAck(_that);case WorkerReply_McpSettingsAck() when mcpSettingsAck != null:
return mcpSettingsAck(_that);case WorkerReply_McpAddFromCatalogAck() when mcpAddFromCatalogAck != null:
return mcpAddFromCatalogAck(_that);case WorkerReply_McpToggleAck() when mcpToggleAck != null:
return mcpToggleAck(_that);case WorkerReply_McpRemoveAck() when mcpRemoveAck != null:
return mcpRemoveAck(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( WorkerReply_ProjectList value)  projectList,required TResult Function( WorkerReply_ProjectAdded value)  projectAdded,required TResult Function( WorkerReply_ProjectRemoved value)  projectRemoved,required TResult Function( WorkerReply_Err value)  err,required TResult Function( WorkerReply_TaskCreated value)  taskCreated,required TResult Function( WorkerReply_TaskRenamed value)  taskRenamed,required TResult Function( WorkerReply_TaskPinned value)  taskPinned,required TResult Function( WorkerReply_TaskRemoved value)  taskRemoved,required TResult Function( WorkerReply_SlugifyBranchNameAck value)  slugifyBranchNameAck,required TResult Function( WorkerReply_ProjectBranchesAck value)  projectBranchesAck,required TResult Function( WorkerReply_PrimaryBranchAck value)  primaryBranchAck,required TResult Function( WorkerReply_RepoDefaultCommitActionAck value)  repoDefaultCommitActionAck,required TResult Function( WorkerReply_ActiveGitStateAck value)  activeGitStateAck,required TResult Function( WorkerReply_ChangedFilesAck value)  changedFilesAck,required TResult Function( WorkerReply_ProjectGithubUrlAck value)  projectGithubUrlAck,required TResult Function( WorkerReply_RecentCommitsAck value)  recentCommitsAck,required TResult Function( WorkerReply_CommitFileChangesAck value)  commitFileChangesAck,required TResult Function( WorkerReply_BranchCompareAck value)  branchCompareAck,required TResult Function( WorkerReply_BranchSettingsAck value)  branchSettingsAck,required TResult Function( WorkerReply_SetBranchSettingAck value)  setBranchSettingAck,required TResult Function( WorkerReply_StageChangedFileAck value)  stageChangedFileAck,required TResult Function( WorkerReply_UnstageChangedFileAck value)  unstageChangedFileAck,required TResult Function( WorkerReply_StageAllChangesAck value)  stageAllChangesAck,required TResult Function( WorkerReply_UnstageAllChangesAck value)  unstageAllChangesAck,required TResult Function( WorkerReply_DiscardChangedFileAck value)  discardChangedFileAck,required TResult Function( WorkerReply_DiscardAllChangesAck value)  discardAllChangesAck,required TResult Function( WorkerReply_ToolbarActionOutcomeAck value)  toolbarActionOutcomeAck,required TResult Function( WorkerReply_CreateBranchAck value)  createBranchAck,required TResult Function( WorkerReply_CreateReviewTaskAck value)  createReviewTaskAck,required TResult Function( WorkerReply_PullRequestStatusAck value)  pullRequestStatusAck,required TResult Function( WorkerReply_PullRequestChecksAck value)  pullRequestChecksAck,required TResult Function( WorkerReply_ProjectPullRequestsAck value)  projectPullRequestsAck,required TResult Function( WorkerReply_OpenInStateAck value)  openInStateAck,required TResult Function( WorkerReply_ProjectActionsAck value)  projectActionsAck,required TResult Function( WorkerReply_EnabledAgentsAck value)  enabledAgentsAck,required TResult Function( WorkerReply_SubmitNewTaskAck value)  submitNewTaskAck,required TResult Function( WorkerReply_AddAgentToSectionAck value)  addAgentToSectionAck,required TResult Function( WorkerReply_ActivateSectionTabAck value)  activateSectionTabAck,required TResult Function( WorkerReply_CloseSectionTabAck value)  closeSectionTabAck,required TResult Function( WorkerReply_ToggleSectionTabPinnedAck value)  toggleSectionTabPinnedAck,required TResult Function( WorkerReply_AgentSettingsAck value)  agentSettingsAck,required TResult Function( WorkerReply_SetAgentEnabledAck value)  setAgentEnabledAck,required TResult Function( WorkerReply_SetDefaultAgentAck value)  setDefaultAgentAck,required TResult Function( WorkerReply_SetAgentLaunchArgsAck value)  setAgentLaunchArgsAck,required TResult Function( WorkerReply_OpenInSettingsAck value)  openInSettingsAck,required TResult Function( WorkerReply_SetOpenInAppEnabledAck value)  setOpenInAppEnabledAck,required TResult Function( WorkerReply_OpenProjectInAppAck value)  openProjectInAppAck,required TResult Function( WorkerReply_RunProjectActionAck value)  runProjectActionAck,required TResult Function( WorkerReply_SaveProjectActionAck value)  saveProjectActionAck,required TResult Function( WorkerReply_DeleteProjectActionAck value)  deleteProjectActionAck,required TResult Function( WorkerReply_GitActionScriptsAck value)  gitActionScriptsAck,required TResult Function( WorkerReply_SetGitCommitScriptAck value)  setGitCommitScriptAck,required TResult Function( WorkerReply_ResetGitCommitScriptAck value)  resetGitCommitScriptAck,required TResult Function( WorkerReply_SetGitPrScriptAck value)  setGitPrScriptAck,required TResult Function( WorkerReply_ResetGitPrScriptAck value)  resetGitPrScriptAck,required TResult Function( WorkerReply_ShortcutSettingsAck value)  shortcutSettingsAck,required TResult Function( WorkerReply_SetShortcutBindingAck value)  setShortcutBindingAck,required TResult Function( WorkerReply_ResetShortcutBindingAck value)  resetShortcutBindingAck,required TResult Function( WorkerReply_McpSettingsAck value)  mcpSettingsAck,required TResult Function( WorkerReply_McpAddFromCatalogAck value)  mcpAddFromCatalogAck,required TResult Function( WorkerReply_McpToggleAck value)  mcpToggleAck,required TResult Function( WorkerReply_McpRemoveAck value)  mcpRemoveAck,}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList():
return projectList(_that);case WorkerReply_ProjectAdded():
return projectAdded(_that);case WorkerReply_ProjectRemoved():
return projectRemoved(_that);case WorkerReply_Err():
return err(_that);case WorkerReply_TaskCreated():
return taskCreated(_that);case WorkerReply_TaskRenamed():
return taskRenamed(_that);case WorkerReply_TaskPinned():
return taskPinned(_that);case WorkerReply_TaskRemoved():
return taskRemoved(_that);case WorkerReply_SlugifyBranchNameAck():
return slugifyBranchNameAck(_that);case WorkerReply_ProjectBranchesAck():
return projectBranchesAck(_that);case WorkerReply_PrimaryBranchAck():
return primaryBranchAck(_that);case WorkerReply_RepoDefaultCommitActionAck():
return repoDefaultCommitActionAck(_that);case WorkerReply_ActiveGitStateAck():
return activeGitStateAck(_that);case WorkerReply_ChangedFilesAck():
return changedFilesAck(_that);case WorkerReply_ProjectGithubUrlAck():
return projectGithubUrlAck(_that);case WorkerReply_RecentCommitsAck():
return recentCommitsAck(_that);case WorkerReply_CommitFileChangesAck():
return commitFileChangesAck(_that);case WorkerReply_BranchCompareAck():
return branchCompareAck(_that);case WorkerReply_BranchSettingsAck():
return branchSettingsAck(_that);case WorkerReply_SetBranchSettingAck():
return setBranchSettingAck(_that);case WorkerReply_StageChangedFileAck():
return stageChangedFileAck(_that);case WorkerReply_UnstageChangedFileAck():
return unstageChangedFileAck(_that);case WorkerReply_StageAllChangesAck():
return stageAllChangesAck(_that);case WorkerReply_UnstageAllChangesAck():
return unstageAllChangesAck(_that);case WorkerReply_DiscardChangedFileAck():
return discardChangedFileAck(_that);case WorkerReply_DiscardAllChangesAck():
return discardAllChangesAck(_that);case WorkerReply_ToolbarActionOutcomeAck():
return toolbarActionOutcomeAck(_that);case WorkerReply_CreateBranchAck():
return createBranchAck(_that);case WorkerReply_CreateReviewTaskAck():
return createReviewTaskAck(_that);case WorkerReply_PullRequestStatusAck():
return pullRequestStatusAck(_that);case WorkerReply_PullRequestChecksAck():
return pullRequestChecksAck(_that);case WorkerReply_ProjectPullRequestsAck():
return projectPullRequestsAck(_that);case WorkerReply_OpenInStateAck():
return openInStateAck(_that);case WorkerReply_ProjectActionsAck():
return projectActionsAck(_that);case WorkerReply_EnabledAgentsAck():
return enabledAgentsAck(_that);case WorkerReply_SubmitNewTaskAck():
return submitNewTaskAck(_that);case WorkerReply_AddAgentToSectionAck():
return addAgentToSectionAck(_that);case WorkerReply_ActivateSectionTabAck():
return activateSectionTabAck(_that);case WorkerReply_CloseSectionTabAck():
return closeSectionTabAck(_that);case WorkerReply_ToggleSectionTabPinnedAck():
return toggleSectionTabPinnedAck(_that);case WorkerReply_AgentSettingsAck():
return agentSettingsAck(_that);case WorkerReply_SetAgentEnabledAck():
return setAgentEnabledAck(_that);case WorkerReply_SetDefaultAgentAck():
return setDefaultAgentAck(_that);case WorkerReply_SetAgentLaunchArgsAck():
return setAgentLaunchArgsAck(_that);case WorkerReply_OpenInSettingsAck():
return openInSettingsAck(_that);case WorkerReply_SetOpenInAppEnabledAck():
return setOpenInAppEnabledAck(_that);case WorkerReply_OpenProjectInAppAck():
return openProjectInAppAck(_that);case WorkerReply_RunProjectActionAck():
return runProjectActionAck(_that);case WorkerReply_SaveProjectActionAck():
return saveProjectActionAck(_that);case WorkerReply_DeleteProjectActionAck():
return deleteProjectActionAck(_that);case WorkerReply_GitActionScriptsAck():
return gitActionScriptsAck(_that);case WorkerReply_SetGitCommitScriptAck():
return setGitCommitScriptAck(_that);case WorkerReply_ResetGitCommitScriptAck():
return resetGitCommitScriptAck(_that);case WorkerReply_SetGitPrScriptAck():
return setGitPrScriptAck(_that);case WorkerReply_ResetGitPrScriptAck():
return resetGitPrScriptAck(_that);case WorkerReply_ShortcutSettingsAck():
return shortcutSettingsAck(_that);case WorkerReply_SetShortcutBindingAck():
return setShortcutBindingAck(_that);case WorkerReply_ResetShortcutBindingAck():
return resetShortcutBindingAck(_that);case WorkerReply_McpSettingsAck():
return mcpSettingsAck(_that);case WorkerReply_McpAddFromCatalogAck():
return mcpAddFromCatalogAck(_that);case WorkerReply_McpToggleAck():
return mcpToggleAck(_that);case WorkerReply_McpRemoveAck():
return mcpRemoveAck(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( WorkerReply_ProjectList value)?  projectList,TResult? Function( WorkerReply_ProjectAdded value)?  projectAdded,TResult? Function( WorkerReply_ProjectRemoved value)?  projectRemoved,TResult? Function( WorkerReply_Err value)?  err,TResult? Function( WorkerReply_TaskCreated value)?  taskCreated,TResult? Function( WorkerReply_TaskRenamed value)?  taskRenamed,TResult? Function( WorkerReply_TaskPinned value)?  taskPinned,TResult? Function( WorkerReply_TaskRemoved value)?  taskRemoved,TResult? Function( WorkerReply_SlugifyBranchNameAck value)?  slugifyBranchNameAck,TResult? Function( WorkerReply_ProjectBranchesAck value)?  projectBranchesAck,TResult? Function( WorkerReply_PrimaryBranchAck value)?  primaryBranchAck,TResult? Function( WorkerReply_RepoDefaultCommitActionAck value)?  repoDefaultCommitActionAck,TResult? Function( WorkerReply_ActiveGitStateAck value)?  activeGitStateAck,TResult? Function( WorkerReply_ChangedFilesAck value)?  changedFilesAck,TResult? Function( WorkerReply_ProjectGithubUrlAck value)?  projectGithubUrlAck,TResult? Function( WorkerReply_RecentCommitsAck value)?  recentCommitsAck,TResult? Function( WorkerReply_CommitFileChangesAck value)?  commitFileChangesAck,TResult? Function( WorkerReply_BranchCompareAck value)?  branchCompareAck,TResult? Function( WorkerReply_BranchSettingsAck value)?  branchSettingsAck,TResult? Function( WorkerReply_SetBranchSettingAck value)?  setBranchSettingAck,TResult? Function( WorkerReply_StageChangedFileAck value)?  stageChangedFileAck,TResult? Function( WorkerReply_UnstageChangedFileAck value)?  unstageChangedFileAck,TResult? Function( WorkerReply_StageAllChangesAck value)?  stageAllChangesAck,TResult? Function( WorkerReply_UnstageAllChangesAck value)?  unstageAllChangesAck,TResult? Function( WorkerReply_DiscardChangedFileAck value)?  discardChangedFileAck,TResult? Function( WorkerReply_DiscardAllChangesAck value)?  discardAllChangesAck,TResult? Function( WorkerReply_ToolbarActionOutcomeAck value)?  toolbarActionOutcomeAck,TResult? Function( WorkerReply_CreateBranchAck value)?  createBranchAck,TResult? Function( WorkerReply_CreateReviewTaskAck value)?  createReviewTaskAck,TResult? Function( WorkerReply_PullRequestStatusAck value)?  pullRequestStatusAck,TResult? Function( WorkerReply_PullRequestChecksAck value)?  pullRequestChecksAck,TResult? Function( WorkerReply_ProjectPullRequestsAck value)?  projectPullRequestsAck,TResult? Function( WorkerReply_OpenInStateAck value)?  openInStateAck,TResult? Function( WorkerReply_ProjectActionsAck value)?  projectActionsAck,TResult? Function( WorkerReply_EnabledAgentsAck value)?  enabledAgentsAck,TResult? Function( WorkerReply_SubmitNewTaskAck value)?  submitNewTaskAck,TResult? Function( WorkerReply_AddAgentToSectionAck value)?  addAgentToSectionAck,TResult? Function( WorkerReply_ActivateSectionTabAck value)?  activateSectionTabAck,TResult? Function( WorkerReply_CloseSectionTabAck value)?  closeSectionTabAck,TResult? Function( WorkerReply_ToggleSectionTabPinnedAck value)?  toggleSectionTabPinnedAck,TResult? Function( WorkerReply_AgentSettingsAck value)?  agentSettingsAck,TResult? Function( WorkerReply_SetAgentEnabledAck value)?  setAgentEnabledAck,TResult? Function( WorkerReply_SetDefaultAgentAck value)?  setDefaultAgentAck,TResult? Function( WorkerReply_SetAgentLaunchArgsAck value)?  setAgentLaunchArgsAck,TResult? Function( WorkerReply_OpenInSettingsAck value)?  openInSettingsAck,TResult? Function( WorkerReply_SetOpenInAppEnabledAck value)?  setOpenInAppEnabledAck,TResult? Function( WorkerReply_OpenProjectInAppAck value)?  openProjectInAppAck,TResult? Function( WorkerReply_RunProjectActionAck value)?  runProjectActionAck,TResult? Function( WorkerReply_SaveProjectActionAck value)?  saveProjectActionAck,TResult? Function( WorkerReply_DeleteProjectActionAck value)?  deleteProjectActionAck,TResult? Function( WorkerReply_GitActionScriptsAck value)?  gitActionScriptsAck,TResult? Function( WorkerReply_SetGitCommitScriptAck value)?  setGitCommitScriptAck,TResult? Function( WorkerReply_ResetGitCommitScriptAck value)?  resetGitCommitScriptAck,TResult? Function( WorkerReply_SetGitPrScriptAck value)?  setGitPrScriptAck,TResult? Function( WorkerReply_ResetGitPrScriptAck value)?  resetGitPrScriptAck,TResult? Function( WorkerReply_ShortcutSettingsAck value)?  shortcutSettingsAck,TResult? Function( WorkerReply_SetShortcutBindingAck value)?  setShortcutBindingAck,TResult? Function( WorkerReply_ResetShortcutBindingAck value)?  resetShortcutBindingAck,TResult? Function( WorkerReply_McpSettingsAck value)?  mcpSettingsAck,TResult? Function( WorkerReply_McpAddFromCatalogAck value)?  mcpAddFromCatalogAck,TResult? Function( WorkerReply_McpToggleAck value)?  mcpToggleAck,TResult? Function( WorkerReply_McpRemoveAck value)?  mcpRemoveAck,}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that);case WorkerReply_ProjectAdded() when projectAdded != null:
return projectAdded(_that);case WorkerReply_ProjectRemoved() when projectRemoved != null:
return projectRemoved(_that);case WorkerReply_Err() when err != null:
return err(_that);case WorkerReply_TaskCreated() when taskCreated != null:
return taskCreated(_that);case WorkerReply_TaskRenamed() when taskRenamed != null:
return taskRenamed(_that);case WorkerReply_TaskPinned() when taskPinned != null:
return taskPinned(_that);case WorkerReply_TaskRemoved() when taskRemoved != null:
return taskRemoved(_that);case WorkerReply_SlugifyBranchNameAck() when slugifyBranchNameAck != null:
return slugifyBranchNameAck(_that);case WorkerReply_ProjectBranchesAck() when projectBranchesAck != null:
return projectBranchesAck(_that);case WorkerReply_PrimaryBranchAck() when primaryBranchAck != null:
return primaryBranchAck(_that);case WorkerReply_RepoDefaultCommitActionAck() when repoDefaultCommitActionAck != null:
return repoDefaultCommitActionAck(_that);case WorkerReply_ActiveGitStateAck() when activeGitStateAck != null:
return activeGitStateAck(_that);case WorkerReply_ChangedFilesAck() when changedFilesAck != null:
return changedFilesAck(_that);case WorkerReply_ProjectGithubUrlAck() when projectGithubUrlAck != null:
return projectGithubUrlAck(_that);case WorkerReply_RecentCommitsAck() when recentCommitsAck != null:
return recentCommitsAck(_that);case WorkerReply_CommitFileChangesAck() when commitFileChangesAck != null:
return commitFileChangesAck(_that);case WorkerReply_BranchCompareAck() when branchCompareAck != null:
return branchCompareAck(_that);case WorkerReply_BranchSettingsAck() when branchSettingsAck != null:
return branchSettingsAck(_that);case WorkerReply_SetBranchSettingAck() when setBranchSettingAck != null:
return setBranchSettingAck(_that);case WorkerReply_StageChangedFileAck() when stageChangedFileAck != null:
return stageChangedFileAck(_that);case WorkerReply_UnstageChangedFileAck() when unstageChangedFileAck != null:
return unstageChangedFileAck(_that);case WorkerReply_StageAllChangesAck() when stageAllChangesAck != null:
return stageAllChangesAck(_that);case WorkerReply_UnstageAllChangesAck() when unstageAllChangesAck != null:
return unstageAllChangesAck(_that);case WorkerReply_DiscardChangedFileAck() when discardChangedFileAck != null:
return discardChangedFileAck(_that);case WorkerReply_DiscardAllChangesAck() when discardAllChangesAck != null:
return discardAllChangesAck(_that);case WorkerReply_ToolbarActionOutcomeAck() when toolbarActionOutcomeAck != null:
return toolbarActionOutcomeAck(_that);case WorkerReply_CreateBranchAck() when createBranchAck != null:
return createBranchAck(_that);case WorkerReply_CreateReviewTaskAck() when createReviewTaskAck != null:
return createReviewTaskAck(_that);case WorkerReply_PullRequestStatusAck() when pullRequestStatusAck != null:
return pullRequestStatusAck(_that);case WorkerReply_PullRequestChecksAck() when pullRequestChecksAck != null:
return pullRequestChecksAck(_that);case WorkerReply_ProjectPullRequestsAck() when projectPullRequestsAck != null:
return projectPullRequestsAck(_that);case WorkerReply_OpenInStateAck() when openInStateAck != null:
return openInStateAck(_that);case WorkerReply_ProjectActionsAck() when projectActionsAck != null:
return projectActionsAck(_that);case WorkerReply_EnabledAgentsAck() when enabledAgentsAck != null:
return enabledAgentsAck(_that);case WorkerReply_SubmitNewTaskAck() when submitNewTaskAck != null:
return submitNewTaskAck(_that);case WorkerReply_AddAgentToSectionAck() when addAgentToSectionAck != null:
return addAgentToSectionAck(_that);case WorkerReply_ActivateSectionTabAck() when activateSectionTabAck != null:
return activateSectionTabAck(_that);case WorkerReply_CloseSectionTabAck() when closeSectionTabAck != null:
return closeSectionTabAck(_that);case WorkerReply_ToggleSectionTabPinnedAck() when toggleSectionTabPinnedAck != null:
return toggleSectionTabPinnedAck(_that);case WorkerReply_AgentSettingsAck() when agentSettingsAck != null:
return agentSettingsAck(_that);case WorkerReply_SetAgentEnabledAck() when setAgentEnabledAck != null:
return setAgentEnabledAck(_that);case WorkerReply_SetDefaultAgentAck() when setDefaultAgentAck != null:
return setDefaultAgentAck(_that);case WorkerReply_SetAgentLaunchArgsAck() when setAgentLaunchArgsAck != null:
return setAgentLaunchArgsAck(_that);case WorkerReply_OpenInSettingsAck() when openInSettingsAck != null:
return openInSettingsAck(_that);case WorkerReply_SetOpenInAppEnabledAck() when setOpenInAppEnabledAck != null:
return setOpenInAppEnabledAck(_that);case WorkerReply_OpenProjectInAppAck() when openProjectInAppAck != null:
return openProjectInAppAck(_that);case WorkerReply_RunProjectActionAck() when runProjectActionAck != null:
return runProjectActionAck(_that);case WorkerReply_SaveProjectActionAck() when saveProjectActionAck != null:
return saveProjectActionAck(_that);case WorkerReply_DeleteProjectActionAck() when deleteProjectActionAck != null:
return deleteProjectActionAck(_that);case WorkerReply_GitActionScriptsAck() when gitActionScriptsAck != null:
return gitActionScriptsAck(_that);case WorkerReply_SetGitCommitScriptAck() when setGitCommitScriptAck != null:
return setGitCommitScriptAck(_that);case WorkerReply_ResetGitCommitScriptAck() when resetGitCommitScriptAck != null:
return resetGitCommitScriptAck(_that);case WorkerReply_SetGitPrScriptAck() when setGitPrScriptAck != null:
return setGitPrScriptAck(_that);case WorkerReply_ResetGitPrScriptAck() when resetGitPrScriptAck != null:
return resetGitPrScriptAck(_that);case WorkerReply_ShortcutSettingsAck() when shortcutSettingsAck != null:
return shortcutSettingsAck(_that);case WorkerReply_SetShortcutBindingAck() when setShortcutBindingAck != null:
return setShortcutBindingAck(_that);case WorkerReply_ResetShortcutBindingAck() when resetShortcutBindingAck != null:
return resetShortcutBindingAck(_that);case WorkerReply_McpSettingsAck() when mcpSettingsAck != null:
return mcpSettingsAck(_that);case WorkerReply_McpAddFromCatalogAck() when mcpAddFromCatalogAck != null:
return mcpAddFromCatalogAck(_that);case WorkerReply_McpToggleAck() when mcpToggleAck != null:
return mcpToggleAck(_that);case WorkerReply_McpRemoveAck() when mcpRemoveAck != null:
return mcpRemoveAck(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( List<ProjectSummary> projects)?  projectList,TResult Function( ProjectSummary project)?  projectAdded,TResult Function( String projectId)?  projectRemoved,TResult Function( String message,  ErrKind kind)?  err,TResult Function( String projectId,  TaskSummary task)?  taskCreated,TResult Function( bool changed,  TaskSummary? task)?  taskRenamed,TResult Function( bool changed,  TaskSummary? task)?  taskPinned,TResult Function( String projectId,  String taskId,  bool removed)?  taskRemoved,TResult Function( String slug)?  slugifyBranchNameAck,TResult Function( List<String> branches)?  projectBranchesAck,TResult Function( String? branch)?  primaryBranchAck,TResult Function( String? action)?  repoDefaultCommitActionAck,TResult Function( ActiveGitStateDto? state)?  activeGitStateAck,TResult Function( List<ChangedFileDto>? files)?  changedFilesAck,TResult Function( String? url)?  projectGithubUrlAck,TResult Function( RecentCommitsView? view)?  recentCommitsAck,TResult Function( List<BranchCompareFileDto>? files)?  commitFileChangesAck,TResult Function( BranchCompareView? view)?  branchCompareAck,TResult Function( ResolvedProjectBranchSettingsDto? settings)?  branchSettingsAck,TResult Function( bool changed)?  setBranchSettingAck,TResult Function( List<ChangedFileDto> changedFiles)?  stageChangedFileAck,TResult Function( List<ChangedFileDto> changedFiles)?  unstageChangedFileAck,TResult Function( List<ChangedFileDto> changedFiles)?  stageAllChangesAck,TResult Function( List<ChangedFileDto> changedFiles)?  unstageAllChangesAck,TResult Function( List<ChangedFileDto> changedFiles)?  discardChangedFileAck,TResult Function( List<ChangedFileDto> changedFiles,  List<String> failures)?  discardAllChangesAck,TResult Function( ToolbarActionOutcomeDto outcome)?  toolbarActionOutcomeAck,TResult Function( String sectionId,  List<ProjectSummary> projects)?  createBranchAck,TResult Function( String sectionId,  List<ProjectSummary> projects)?  createReviewTaskAck,TResult Function( PullRequestStatusDto? status)?  pullRequestStatusAck,TResult Function( List<CheckDto>? checks)?  pullRequestChecksAck,TResult Function( List<ProjectPagePullRequestDto>? prs)?  projectPullRequestsAck,TResult Function( OpenInState state)?  openInStateAck,TResult Function( List<ProjectActionDto> actions)?  projectActionsAck,TResult Function( EnabledAgentsView view)?  enabledAgentsAck,TResult Function( String sectionId)?  submitNewTaskAck,TResult Function( String tabId)?  addAgentToSectionAck,TResult Function()?  activateSectionTabAck,TResult Function( String activeTabId)?  closeSectionTabAck,TResult Function( bool pinned)?  toggleSectionTabPinnedAck,TResult Function( AgentSettingsView view)?  agentSettingsAck,TResult Function( bool changed)?  setAgentEnabledAck,TResult Function( bool changed)?  setDefaultAgentAck,TResult Function( bool changed)?  setAgentLaunchArgsAck,TResult Function( OpenInSettingsView view)?  openInSettingsAck,TResult Function()?  setOpenInAppEnabledAck,TResult Function()?  openProjectInAppAck,TResult Function( String tabId)?  runProjectActionAck,TResult Function()?  saveProjectActionAck,TResult Function( bool deleted)?  deleteProjectActionAck,TResult Function( GitActionScriptsView view)?  gitActionScriptsAck,TResult Function( bool changed)?  setGitCommitScriptAck,TResult Function( bool changed)?  resetGitCommitScriptAck,TResult Function( bool changed)?  setGitPrScriptAck,TResult Function( bool changed)?  resetGitPrScriptAck,TResult Function( ShortcutSettingsView view)?  shortcutSettingsAck,TResult Function()?  setShortcutBindingAck,TResult Function()?  resetShortcutBindingAck,TResult Function( McpSettingsView view)?  mcpSettingsAck,TResult Function()?  mcpAddFromCatalogAck,TResult Function()?  mcpToggleAck,TResult Function()?  mcpRemoveAck,required TResult orElse(),}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case WorkerReply_ProjectAdded() when projectAdded != null:
return projectAdded(_that.project);case WorkerReply_ProjectRemoved() when projectRemoved != null:
return projectRemoved(_that.projectId);case WorkerReply_Err() when err != null:
return err(_that.message,_that.kind);case WorkerReply_TaskCreated() when taskCreated != null:
return taskCreated(_that.projectId,_that.task);case WorkerReply_TaskRenamed() when taskRenamed != null:
return taskRenamed(_that.changed,_that.task);case WorkerReply_TaskPinned() when taskPinned != null:
return taskPinned(_that.changed,_that.task);case WorkerReply_TaskRemoved() when taskRemoved != null:
return taskRemoved(_that.projectId,_that.taskId,_that.removed);case WorkerReply_SlugifyBranchNameAck() when slugifyBranchNameAck != null:
return slugifyBranchNameAck(_that.slug);case WorkerReply_ProjectBranchesAck() when projectBranchesAck != null:
return projectBranchesAck(_that.branches);case WorkerReply_PrimaryBranchAck() when primaryBranchAck != null:
return primaryBranchAck(_that.branch);case WorkerReply_RepoDefaultCommitActionAck() when repoDefaultCommitActionAck != null:
return repoDefaultCommitActionAck(_that.action);case WorkerReply_ActiveGitStateAck() when activeGitStateAck != null:
return activeGitStateAck(_that.state);case WorkerReply_ChangedFilesAck() when changedFilesAck != null:
return changedFilesAck(_that.files);case WorkerReply_ProjectGithubUrlAck() when projectGithubUrlAck != null:
return projectGithubUrlAck(_that.url);case WorkerReply_RecentCommitsAck() when recentCommitsAck != null:
return recentCommitsAck(_that.view);case WorkerReply_CommitFileChangesAck() when commitFileChangesAck != null:
return commitFileChangesAck(_that.files);case WorkerReply_BranchCompareAck() when branchCompareAck != null:
return branchCompareAck(_that.view);case WorkerReply_BranchSettingsAck() when branchSettingsAck != null:
return branchSettingsAck(_that.settings);case WorkerReply_SetBranchSettingAck() when setBranchSettingAck != null:
return setBranchSettingAck(_that.changed);case WorkerReply_StageChangedFileAck() when stageChangedFileAck != null:
return stageChangedFileAck(_that.changedFiles);case WorkerReply_UnstageChangedFileAck() when unstageChangedFileAck != null:
return unstageChangedFileAck(_that.changedFiles);case WorkerReply_StageAllChangesAck() when stageAllChangesAck != null:
return stageAllChangesAck(_that.changedFiles);case WorkerReply_UnstageAllChangesAck() when unstageAllChangesAck != null:
return unstageAllChangesAck(_that.changedFiles);case WorkerReply_DiscardChangedFileAck() when discardChangedFileAck != null:
return discardChangedFileAck(_that.changedFiles);case WorkerReply_DiscardAllChangesAck() when discardAllChangesAck != null:
return discardAllChangesAck(_that.changedFiles,_that.failures);case WorkerReply_ToolbarActionOutcomeAck() when toolbarActionOutcomeAck != null:
return toolbarActionOutcomeAck(_that.outcome);case WorkerReply_CreateBranchAck() when createBranchAck != null:
return createBranchAck(_that.sectionId,_that.projects);case WorkerReply_CreateReviewTaskAck() when createReviewTaskAck != null:
return createReviewTaskAck(_that.sectionId,_that.projects);case WorkerReply_PullRequestStatusAck() when pullRequestStatusAck != null:
return pullRequestStatusAck(_that.status);case WorkerReply_PullRequestChecksAck() when pullRequestChecksAck != null:
return pullRequestChecksAck(_that.checks);case WorkerReply_ProjectPullRequestsAck() when projectPullRequestsAck != null:
return projectPullRequestsAck(_that.prs);case WorkerReply_OpenInStateAck() when openInStateAck != null:
return openInStateAck(_that.state);case WorkerReply_ProjectActionsAck() when projectActionsAck != null:
return projectActionsAck(_that.actions);case WorkerReply_EnabledAgentsAck() when enabledAgentsAck != null:
return enabledAgentsAck(_that.view);case WorkerReply_SubmitNewTaskAck() when submitNewTaskAck != null:
return submitNewTaskAck(_that.sectionId);case WorkerReply_AddAgentToSectionAck() when addAgentToSectionAck != null:
return addAgentToSectionAck(_that.tabId);case WorkerReply_ActivateSectionTabAck() when activateSectionTabAck != null:
return activateSectionTabAck();case WorkerReply_CloseSectionTabAck() when closeSectionTabAck != null:
return closeSectionTabAck(_that.activeTabId);case WorkerReply_ToggleSectionTabPinnedAck() when toggleSectionTabPinnedAck != null:
return toggleSectionTabPinnedAck(_that.pinned);case WorkerReply_AgentSettingsAck() when agentSettingsAck != null:
return agentSettingsAck(_that.view);case WorkerReply_SetAgentEnabledAck() when setAgentEnabledAck != null:
return setAgentEnabledAck(_that.changed);case WorkerReply_SetDefaultAgentAck() when setDefaultAgentAck != null:
return setDefaultAgentAck(_that.changed);case WorkerReply_SetAgentLaunchArgsAck() when setAgentLaunchArgsAck != null:
return setAgentLaunchArgsAck(_that.changed);case WorkerReply_OpenInSettingsAck() when openInSettingsAck != null:
return openInSettingsAck(_that.view);case WorkerReply_SetOpenInAppEnabledAck() when setOpenInAppEnabledAck != null:
return setOpenInAppEnabledAck();case WorkerReply_OpenProjectInAppAck() when openProjectInAppAck != null:
return openProjectInAppAck();case WorkerReply_RunProjectActionAck() when runProjectActionAck != null:
return runProjectActionAck(_that.tabId);case WorkerReply_SaveProjectActionAck() when saveProjectActionAck != null:
return saveProjectActionAck();case WorkerReply_DeleteProjectActionAck() when deleteProjectActionAck != null:
return deleteProjectActionAck(_that.deleted);case WorkerReply_GitActionScriptsAck() when gitActionScriptsAck != null:
return gitActionScriptsAck(_that.view);case WorkerReply_SetGitCommitScriptAck() when setGitCommitScriptAck != null:
return setGitCommitScriptAck(_that.changed);case WorkerReply_ResetGitCommitScriptAck() when resetGitCommitScriptAck != null:
return resetGitCommitScriptAck(_that.changed);case WorkerReply_SetGitPrScriptAck() when setGitPrScriptAck != null:
return setGitPrScriptAck(_that.changed);case WorkerReply_ResetGitPrScriptAck() when resetGitPrScriptAck != null:
return resetGitPrScriptAck(_that.changed);case WorkerReply_ShortcutSettingsAck() when shortcutSettingsAck != null:
return shortcutSettingsAck(_that.view);case WorkerReply_SetShortcutBindingAck() when setShortcutBindingAck != null:
return setShortcutBindingAck();case WorkerReply_ResetShortcutBindingAck() when resetShortcutBindingAck != null:
return resetShortcutBindingAck();case WorkerReply_McpSettingsAck() when mcpSettingsAck != null:
return mcpSettingsAck(_that.view);case WorkerReply_McpAddFromCatalogAck() when mcpAddFromCatalogAck != null:
return mcpAddFromCatalogAck();case WorkerReply_McpToggleAck() when mcpToggleAck != null:
return mcpToggleAck();case WorkerReply_McpRemoveAck() when mcpRemoveAck != null:
return mcpRemoveAck();case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( List<ProjectSummary> projects)  projectList,required TResult Function( ProjectSummary project)  projectAdded,required TResult Function( String projectId)  projectRemoved,required TResult Function( String message,  ErrKind kind)  err,required TResult Function( String projectId,  TaskSummary task)  taskCreated,required TResult Function( bool changed,  TaskSummary? task)  taskRenamed,required TResult Function( bool changed,  TaskSummary? task)  taskPinned,required TResult Function( String projectId,  String taskId,  bool removed)  taskRemoved,required TResult Function( String slug)  slugifyBranchNameAck,required TResult Function( List<String> branches)  projectBranchesAck,required TResult Function( String? branch)  primaryBranchAck,required TResult Function( String? action)  repoDefaultCommitActionAck,required TResult Function( ActiveGitStateDto? state)  activeGitStateAck,required TResult Function( List<ChangedFileDto>? files)  changedFilesAck,required TResult Function( String? url)  projectGithubUrlAck,required TResult Function( RecentCommitsView? view)  recentCommitsAck,required TResult Function( List<BranchCompareFileDto>? files)  commitFileChangesAck,required TResult Function( BranchCompareView? view)  branchCompareAck,required TResult Function( ResolvedProjectBranchSettingsDto? settings)  branchSettingsAck,required TResult Function( bool changed)  setBranchSettingAck,required TResult Function( List<ChangedFileDto> changedFiles)  stageChangedFileAck,required TResult Function( List<ChangedFileDto> changedFiles)  unstageChangedFileAck,required TResult Function( List<ChangedFileDto> changedFiles)  stageAllChangesAck,required TResult Function( List<ChangedFileDto> changedFiles)  unstageAllChangesAck,required TResult Function( List<ChangedFileDto> changedFiles)  discardChangedFileAck,required TResult Function( List<ChangedFileDto> changedFiles,  List<String> failures)  discardAllChangesAck,required TResult Function( ToolbarActionOutcomeDto outcome)  toolbarActionOutcomeAck,required TResult Function( String sectionId,  List<ProjectSummary> projects)  createBranchAck,required TResult Function( String sectionId,  List<ProjectSummary> projects)  createReviewTaskAck,required TResult Function( PullRequestStatusDto? status)  pullRequestStatusAck,required TResult Function( List<CheckDto>? checks)  pullRequestChecksAck,required TResult Function( List<ProjectPagePullRequestDto>? prs)  projectPullRequestsAck,required TResult Function( OpenInState state)  openInStateAck,required TResult Function( List<ProjectActionDto> actions)  projectActionsAck,required TResult Function( EnabledAgentsView view)  enabledAgentsAck,required TResult Function( String sectionId)  submitNewTaskAck,required TResult Function( String tabId)  addAgentToSectionAck,required TResult Function()  activateSectionTabAck,required TResult Function( String activeTabId)  closeSectionTabAck,required TResult Function( bool pinned)  toggleSectionTabPinnedAck,required TResult Function( AgentSettingsView view)  agentSettingsAck,required TResult Function( bool changed)  setAgentEnabledAck,required TResult Function( bool changed)  setDefaultAgentAck,required TResult Function( bool changed)  setAgentLaunchArgsAck,required TResult Function( OpenInSettingsView view)  openInSettingsAck,required TResult Function()  setOpenInAppEnabledAck,required TResult Function()  openProjectInAppAck,required TResult Function( String tabId)  runProjectActionAck,required TResult Function()  saveProjectActionAck,required TResult Function( bool deleted)  deleteProjectActionAck,required TResult Function( GitActionScriptsView view)  gitActionScriptsAck,required TResult Function( bool changed)  setGitCommitScriptAck,required TResult Function( bool changed)  resetGitCommitScriptAck,required TResult Function( bool changed)  setGitPrScriptAck,required TResult Function( bool changed)  resetGitPrScriptAck,required TResult Function( ShortcutSettingsView view)  shortcutSettingsAck,required TResult Function()  setShortcutBindingAck,required TResult Function()  resetShortcutBindingAck,required TResult Function( McpSettingsView view)  mcpSettingsAck,required TResult Function()  mcpAddFromCatalogAck,required TResult Function()  mcpToggleAck,required TResult Function()  mcpRemoveAck,}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList():
return projectList(_that.projects);case WorkerReply_ProjectAdded():
return projectAdded(_that.project);case WorkerReply_ProjectRemoved():
return projectRemoved(_that.projectId);case WorkerReply_Err():
return err(_that.message,_that.kind);case WorkerReply_TaskCreated():
return taskCreated(_that.projectId,_that.task);case WorkerReply_TaskRenamed():
return taskRenamed(_that.changed,_that.task);case WorkerReply_TaskPinned():
return taskPinned(_that.changed,_that.task);case WorkerReply_TaskRemoved():
return taskRemoved(_that.projectId,_that.taskId,_that.removed);case WorkerReply_SlugifyBranchNameAck():
return slugifyBranchNameAck(_that.slug);case WorkerReply_ProjectBranchesAck():
return projectBranchesAck(_that.branches);case WorkerReply_PrimaryBranchAck():
return primaryBranchAck(_that.branch);case WorkerReply_RepoDefaultCommitActionAck():
return repoDefaultCommitActionAck(_that.action);case WorkerReply_ActiveGitStateAck():
return activeGitStateAck(_that.state);case WorkerReply_ChangedFilesAck():
return changedFilesAck(_that.files);case WorkerReply_ProjectGithubUrlAck():
return projectGithubUrlAck(_that.url);case WorkerReply_RecentCommitsAck():
return recentCommitsAck(_that.view);case WorkerReply_CommitFileChangesAck():
return commitFileChangesAck(_that.files);case WorkerReply_BranchCompareAck():
return branchCompareAck(_that.view);case WorkerReply_BranchSettingsAck():
return branchSettingsAck(_that.settings);case WorkerReply_SetBranchSettingAck():
return setBranchSettingAck(_that.changed);case WorkerReply_StageChangedFileAck():
return stageChangedFileAck(_that.changedFiles);case WorkerReply_UnstageChangedFileAck():
return unstageChangedFileAck(_that.changedFiles);case WorkerReply_StageAllChangesAck():
return stageAllChangesAck(_that.changedFiles);case WorkerReply_UnstageAllChangesAck():
return unstageAllChangesAck(_that.changedFiles);case WorkerReply_DiscardChangedFileAck():
return discardChangedFileAck(_that.changedFiles);case WorkerReply_DiscardAllChangesAck():
return discardAllChangesAck(_that.changedFiles,_that.failures);case WorkerReply_ToolbarActionOutcomeAck():
return toolbarActionOutcomeAck(_that.outcome);case WorkerReply_CreateBranchAck():
return createBranchAck(_that.sectionId,_that.projects);case WorkerReply_CreateReviewTaskAck():
return createReviewTaskAck(_that.sectionId,_that.projects);case WorkerReply_PullRequestStatusAck():
return pullRequestStatusAck(_that.status);case WorkerReply_PullRequestChecksAck():
return pullRequestChecksAck(_that.checks);case WorkerReply_ProjectPullRequestsAck():
return projectPullRequestsAck(_that.prs);case WorkerReply_OpenInStateAck():
return openInStateAck(_that.state);case WorkerReply_ProjectActionsAck():
return projectActionsAck(_that.actions);case WorkerReply_EnabledAgentsAck():
return enabledAgentsAck(_that.view);case WorkerReply_SubmitNewTaskAck():
return submitNewTaskAck(_that.sectionId);case WorkerReply_AddAgentToSectionAck():
return addAgentToSectionAck(_that.tabId);case WorkerReply_ActivateSectionTabAck():
return activateSectionTabAck();case WorkerReply_CloseSectionTabAck():
return closeSectionTabAck(_that.activeTabId);case WorkerReply_ToggleSectionTabPinnedAck():
return toggleSectionTabPinnedAck(_that.pinned);case WorkerReply_AgentSettingsAck():
return agentSettingsAck(_that.view);case WorkerReply_SetAgentEnabledAck():
return setAgentEnabledAck(_that.changed);case WorkerReply_SetDefaultAgentAck():
return setDefaultAgentAck(_that.changed);case WorkerReply_SetAgentLaunchArgsAck():
return setAgentLaunchArgsAck(_that.changed);case WorkerReply_OpenInSettingsAck():
return openInSettingsAck(_that.view);case WorkerReply_SetOpenInAppEnabledAck():
return setOpenInAppEnabledAck();case WorkerReply_OpenProjectInAppAck():
return openProjectInAppAck();case WorkerReply_RunProjectActionAck():
return runProjectActionAck(_that.tabId);case WorkerReply_SaveProjectActionAck():
return saveProjectActionAck();case WorkerReply_DeleteProjectActionAck():
return deleteProjectActionAck(_that.deleted);case WorkerReply_GitActionScriptsAck():
return gitActionScriptsAck(_that.view);case WorkerReply_SetGitCommitScriptAck():
return setGitCommitScriptAck(_that.changed);case WorkerReply_ResetGitCommitScriptAck():
return resetGitCommitScriptAck(_that.changed);case WorkerReply_SetGitPrScriptAck():
return setGitPrScriptAck(_that.changed);case WorkerReply_ResetGitPrScriptAck():
return resetGitPrScriptAck(_that.changed);case WorkerReply_ShortcutSettingsAck():
return shortcutSettingsAck(_that.view);case WorkerReply_SetShortcutBindingAck():
return setShortcutBindingAck();case WorkerReply_ResetShortcutBindingAck():
return resetShortcutBindingAck();case WorkerReply_McpSettingsAck():
return mcpSettingsAck(_that.view);case WorkerReply_McpAddFromCatalogAck():
return mcpAddFromCatalogAck();case WorkerReply_McpToggleAck():
return mcpToggleAck();case WorkerReply_McpRemoveAck():
return mcpRemoveAck();}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( List<ProjectSummary> projects)?  projectList,TResult? Function( ProjectSummary project)?  projectAdded,TResult? Function( String projectId)?  projectRemoved,TResult? Function( String message,  ErrKind kind)?  err,TResult? Function( String projectId,  TaskSummary task)?  taskCreated,TResult? Function( bool changed,  TaskSummary? task)?  taskRenamed,TResult? Function( bool changed,  TaskSummary? task)?  taskPinned,TResult? Function( String projectId,  String taskId,  bool removed)?  taskRemoved,TResult? Function( String slug)?  slugifyBranchNameAck,TResult? Function( List<String> branches)?  projectBranchesAck,TResult? Function( String? branch)?  primaryBranchAck,TResult? Function( String? action)?  repoDefaultCommitActionAck,TResult? Function( ActiveGitStateDto? state)?  activeGitStateAck,TResult? Function( List<ChangedFileDto>? files)?  changedFilesAck,TResult? Function( String? url)?  projectGithubUrlAck,TResult? Function( RecentCommitsView? view)?  recentCommitsAck,TResult? Function( List<BranchCompareFileDto>? files)?  commitFileChangesAck,TResult? Function( BranchCompareView? view)?  branchCompareAck,TResult? Function( ResolvedProjectBranchSettingsDto? settings)?  branchSettingsAck,TResult? Function( bool changed)?  setBranchSettingAck,TResult? Function( List<ChangedFileDto> changedFiles)?  stageChangedFileAck,TResult? Function( List<ChangedFileDto> changedFiles)?  unstageChangedFileAck,TResult? Function( List<ChangedFileDto> changedFiles)?  stageAllChangesAck,TResult? Function( List<ChangedFileDto> changedFiles)?  unstageAllChangesAck,TResult? Function( List<ChangedFileDto> changedFiles)?  discardChangedFileAck,TResult? Function( List<ChangedFileDto> changedFiles,  List<String> failures)?  discardAllChangesAck,TResult? Function( ToolbarActionOutcomeDto outcome)?  toolbarActionOutcomeAck,TResult? Function( String sectionId,  List<ProjectSummary> projects)?  createBranchAck,TResult? Function( String sectionId,  List<ProjectSummary> projects)?  createReviewTaskAck,TResult? Function( PullRequestStatusDto? status)?  pullRequestStatusAck,TResult? Function( List<CheckDto>? checks)?  pullRequestChecksAck,TResult? Function( List<ProjectPagePullRequestDto>? prs)?  projectPullRequestsAck,TResult? Function( OpenInState state)?  openInStateAck,TResult? Function( List<ProjectActionDto> actions)?  projectActionsAck,TResult? Function( EnabledAgentsView view)?  enabledAgentsAck,TResult? Function( String sectionId)?  submitNewTaskAck,TResult? Function( String tabId)?  addAgentToSectionAck,TResult? Function()?  activateSectionTabAck,TResult? Function( String activeTabId)?  closeSectionTabAck,TResult? Function( bool pinned)?  toggleSectionTabPinnedAck,TResult? Function( AgentSettingsView view)?  agentSettingsAck,TResult? Function( bool changed)?  setAgentEnabledAck,TResult? Function( bool changed)?  setDefaultAgentAck,TResult? Function( bool changed)?  setAgentLaunchArgsAck,TResult? Function( OpenInSettingsView view)?  openInSettingsAck,TResult? Function()?  setOpenInAppEnabledAck,TResult? Function()?  openProjectInAppAck,TResult? Function( String tabId)?  runProjectActionAck,TResult? Function()?  saveProjectActionAck,TResult? Function( bool deleted)?  deleteProjectActionAck,TResult? Function( GitActionScriptsView view)?  gitActionScriptsAck,TResult? Function( bool changed)?  setGitCommitScriptAck,TResult? Function( bool changed)?  resetGitCommitScriptAck,TResult? Function( bool changed)?  setGitPrScriptAck,TResult? Function( bool changed)?  resetGitPrScriptAck,TResult? Function( ShortcutSettingsView view)?  shortcutSettingsAck,TResult? Function()?  setShortcutBindingAck,TResult? Function()?  resetShortcutBindingAck,TResult? Function( McpSettingsView view)?  mcpSettingsAck,TResult? Function()?  mcpAddFromCatalogAck,TResult? Function()?  mcpToggleAck,TResult? Function()?  mcpRemoveAck,}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case WorkerReply_ProjectAdded() when projectAdded != null:
return projectAdded(_that.project);case WorkerReply_ProjectRemoved() when projectRemoved != null:
return projectRemoved(_that.projectId);case WorkerReply_Err() when err != null:
return err(_that.message,_that.kind);case WorkerReply_TaskCreated() when taskCreated != null:
return taskCreated(_that.projectId,_that.task);case WorkerReply_TaskRenamed() when taskRenamed != null:
return taskRenamed(_that.changed,_that.task);case WorkerReply_TaskPinned() when taskPinned != null:
return taskPinned(_that.changed,_that.task);case WorkerReply_TaskRemoved() when taskRemoved != null:
return taskRemoved(_that.projectId,_that.taskId,_that.removed);case WorkerReply_SlugifyBranchNameAck() when slugifyBranchNameAck != null:
return slugifyBranchNameAck(_that.slug);case WorkerReply_ProjectBranchesAck() when projectBranchesAck != null:
return projectBranchesAck(_that.branches);case WorkerReply_PrimaryBranchAck() when primaryBranchAck != null:
return primaryBranchAck(_that.branch);case WorkerReply_RepoDefaultCommitActionAck() when repoDefaultCommitActionAck != null:
return repoDefaultCommitActionAck(_that.action);case WorkerReply_ActiveGitStateAck() when activeGitStateAck != null:
return activeGitStateAck(_that.state);case WorkerReply_ChangedFilesAck() when changedFilesAck != null:
return changedFilesAck(_that.files);case WorkerReply_ProjectGithubUrlAck() when projectGithubUrlAck != null:
return projectGithubUrlAck(_that.url);case WorkerReply_RecentCommitsAck() when recentCommitsAck != null:
return recentCommitsAck(_that.view);case WorkerReply_CommitFileChangesAck() when commitFileChangesAck != null:
return commitFileChangesAck(_that.files);case WorkerReply_BranchCompareAck() when branchCompareAck != null:
return branchCompareAck(_that.view);case WorkerReply_BranchSettingsAck() when branchSettingsAck != null:
return branchSettingsAck(_that.settings);case WorkerReply_SetBranchSettingAck() when setBranchSettingAck != null:
return setBranchSettingAck(_that.changed);case WorkerReply_StageChangedFileAck() when stageChangedFileAck != null:
return stageChangedFileAck(_that.changedFiles);case WorkerReply_UnstageChangedFileAck() when unstageChangedFileAck != null:
return unstageChangedFileAck(_that.changedFiles);case WorkerReply_StageAllChangesAck() when stageAllChangesAck != null:
return stageAllChangesAck(_that.changedFiles);case WorkerReply_UnstageAllChangesAck() when unstageAllChangesAck != null:
return unstageAllChangesAck(_that.changedFiles);case WorkerReply_DiscardChangedFileAck() when discardChangedFileAck != null:
return discardChangedFileAck(_that.changedFiles);case WorkerReply_DiscardAllChangesAck() when discardAllChangesAck != null:
return discardAllChangesAck(_that.changedFiles,_that.failures);case WorkerReply_ToolbarActionOutcomeAck() when toolbarActionOutcomeAck != null:
return toolbarActionOutcomeAck(_that.outcome);case WorkerReply_CreateBranchAck() when createBranchAck != null:
return createBranchAck(_that.sectionId,_that.projects);case WorkerReply_CreateReviewTaskAck() when createReviewTaskAck != null:
return createReviewTaskAck(_that.sectionId,_that.projects);case WorkerReply_PullRequestStatusAck() when pullRequestStatusAck != null:
return pullRequestStatusAck(_that.status);case WorkerReply_PullRequestChecksAck() when pullRequestChecksAck != null:
return pullRequestChecksAck(_that.checks);case WorkerReply_ProjectPullRequestsAck() when projectPullRequestsAck != null:
return projectPullRequestsAck(_that.prs);case WorkerReply_OpenInStateAck() when openInStateAck != null:
return openInStateAck(_that.state);case WorkerReply_ProjectActionsAck() when projectActionsAck != null:
return projectActionsAck(_that.actions);case WorkerReply_EnabledAgentsAck() when enabledAgentsAck != null:
return enabledAgentsAck(_that.view);case WorkerReply_SubmitNewTaskAck() when submitNewTaskAck != null:
return submitNewTaskAck(_that.sectionId);case WorkerReply_AddAgentToSectionAck() when addAgentToSectionAck != null:
return addAgentToSectionAck(_that.tabId);case WorkerReply_ActivateSectionTabAck() when activateSectionTabAck != null:
return activateSectionTabAck();case WorkerReply_CloseSectionTabAck() when closeSectionTabAck != null:
return closeSectionTabAck(_that.activeTabId);case WorkerReply_ToggleSectionTabPinnedAck() when toggleSectionTabPinnedAck != null:
return toggleSectionTabPinnedAck(_that.pinned);case WorkerReply_AgentSettingsAck() when agentSettingsAck != null:
return agentSettingsAck(_that.view);case WorkerReply_SetAgentEnabledAck() when setAgentEnabledAck != null:
return setAgentEnabledAck(_that.changed);case WorkerReply_SetDefaultAgentAck() when setDefaultAgentAck != null:
return setDefaultAgentAck(_that.changed);case WorkerReply_SetAgentLaunchArgsAck() when setAgentLaunchArgsAck != null:
return setAgentLaunchArgsAck(_that.changed);case WorkerReply_OpenInSettingsAck() when openInSettingsAck != null:
return openInSettingsAck(_that.view);case WorkerReply_SetOpenInAppEnabledAck() when setOpenInAppEnabledAck != null:
return setOpenInAppEnabledAck();case WorkerReply_OpenProjectInAppAck() when openProjectInAppAck != null:
return openProjectInAppAck();case WorkerReply_RunProjectActionAck() when runProjectActionAck != null:
return runProjectActionAck(_that.tabId);case WorkerReply_SaveProjectActionAck() when saveProjectActionAck != null:
return saveProjectActionAck();case WorkerReply_DeleteProjectActionAck() when deleteProjectActionAck != null:
return deleteProjectActionAck(_that.deleted);case WorkerReply_GitActionScriptsAck() when gitActionScriptsAck != null:
return gitActionScriptsAck(_that.view);case WorkerReply_SetGitCommitScriptAck() when setGitCommitScriptAck != null:
return setGitCommitScriptAck(_that.changed);case WorkerReply_ResetGitCommitScriptAck() when resetGitCommitScriptAck != null:
return resetGitCommitScriptAck(_that.changed);case WorkerReply_SetGitPrScriptAck() when setGitPrScriptAck != null:
return setGitPrScriptAck(_that.changed);case WorkerReply_ResetGitPrScriptAck() when resetGitPrScriptAck != null:
return resetGitPrScriptAck(_that.changed);case WorkerReply_ShortcutSettingsAck() when shortcutSettingsAck != null:
return shortcutSettingsAck(_that.view);case WorkerReply_SetShortcutBindingAck() when setShortcutBindingAck != null:
return setShortcutBindingAck();case WorkerReply_ResetShortcutBindingAck() when resetShortcutBindingAck != null:
return resetShortcutBindingAck();case WorkerReply_McpSettingsAck() when mcpSettingsAck != null:
return mcpSettingsAck(_that.view);case WorkerReply_McpAddFromCatalogAck() when mcpAddFromCatalogAck != null:
return mcpAddFromCatalogAck();case WorkerReply_McpToggleAck() when mcpToggleAck != null:
return mcpToggleAck();case WorkerReply_McpRemoveAck() when mcpRemoveAck != null:
return mcpRemoveAck();case _:
  return null;

}
}

}

/// @nodoc


class WorkerReply_ProjectList extends WorkerReply {
  const WorkerReply_ProjectList({required final  List<ProjectSummary> projects}): _projects = projects,super._();


 final  List<ProjectSummary> _projects;
 List<ProjectSummary> get projects {
  if (_projects is EqualUnmodifiableListView) return _projects;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_projects);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ProjectListCopyWith<WorkerReply_ProjectList> get copyWith => _$WorkerReply_ProjectListCopyWithImpl<WorkerReply_ProjectList>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ProjectList&&const DeepCollectionEquality().equals(other._projects, _projects));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_projects));

@override
String toString() {
  return 'WorkerReply.projectList(projects: $projects)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ProjectListCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ProjectListCopyWith(WorkerReply_ProjectList value, $Res Function(WorkerReply_ProjectList) _then) = _$WorkerReply_ProjectListCopyWithImpl;
@useResult
$Res call({
 List<ProjectSummary> projects
});




}
/// @nodoc
class _$WorkerReply_ProjectListCopyWithImpl<$Res>
    implements $WorkerReply_ProjectListCopyWith<$Res> {
  _$WorkerReply_ProjectListCopyWithImpl(this._self, this._then);

  final WorkerReply_ProjectList _self;
  final $Res Function(WorkerReply_ProjectList) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? projects = null,}) {
  return _then(WorkerReply_ProjectList(
projects: null == projects ? _self._projects : projects // ignore: cast_nullable_to_non_nullable
as List<ProjectSummary>,
  ));
}


}

/// @nodoc


class WorkerReply_ProjectAdded extends WorkerReply {
  const WorkerReply_ProjectAdded({required this.project}): super._();


 final  ProjectSummary project;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ProjectAddedCopyWith<WorkerReply_ProjectAdded> get copyWith => _$WorkerReply_ProjectAddedCopyWithImpl<WorkerReply_ProjectAdded>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ProjectAdded&&(identical(other.project, project) || other.project == project));
}


@override
int get hashCode => Object.hash(runtimeType,project);

@override
String toString() {
  return 'WorkerReply.projectAdded(project: $project)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ProjectAddedCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ProjectAddedCopyWith(WorkerReply_ProjectAdded value, $Res Function(WorkerReply_ProjectAdded) _then) = _$WorkerReply_ProjectAddedCopyWithImpl;
@useResult
$Res call({
 ProjectSummary project
});




}
/// @nodoc
class _$WorkerReply_ProjectAddedCopyWithImpl<$Res>
    implements $WorkerReply_ProjectAddedCopyWith<$Res> {
  _$WorkerReply_ProjectAddedCopyWithImpl(this._self, this._then);

  final WorkerReply_ProjectAdded _self;
  final $Res Function(WorkerReply_ProjectAdded) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? project = null,}) {
  return _then(WorkerReply_ProjectAdded(
project: null == project ? _self.project : project // ignore: cast_nullable_to_non_nullable
as ProjectSummary,
  ));
}


}

/// @nodoc


class WorkerReply_ProjectRemoved extends WorkerReply {
  const WorkerReply_ProjectRemoved({required this.projectId}): super._();


 final  String projectId;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ProjectRemovedCopyWith<WorkerReply_ProjectRemoved> get copyWith => _$WorkerReply_ProjectRemovedCopyWithImpl<WorkerReply_ProjectRemoved>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ProjectRemoved&&(identical(other.projectId, projectId) || other.projectId == projectId));
}


@override
int get hashCode => Object.hash(runtimeType,projectId);

@override
String toString() {
  return 'WorkerReply.projectRemoved(projectId: $projectId)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ProjectRemovedCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ProjectRemovedCopyWith(WorkerReply_ProjectRemoved value, $Res Function(WorkerReply_ProjectRemoved) _then) = _$WorkerReply_ProjectRemovedCopyWithImpl;
@useResult
$Res call({
 String projectId
});




}
/// @nodoc
class _$WorkerReply_ProjectRemovedCopyWithImpl<$Res>
    implements $WorkerReply_ProjectRemovedCopyWith<$Res> {
  _$WorkerReply_ProjectRemovedCopyWithImpl(this._self, this._then);

  final WorkerReply_ProjectRemoved _self;
  final $Res Function(WorkerReply_ProjectRemoved) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? projectId = null,}) {
  return _then(WorkerReply_ProjectRemoved(
projectId: null == projectId ? _self.projectId : projectId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class WorkerReply_Err extends WorkerReply {
  const WorkerReply_Err({required this.message, required this.kind}): super._();


 final  String message;
 final  ErrKind kind;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ErrCopyWith<WorkerReply_Err> get copyWith => _$WorkerReply_ErrCopyWithImpl<WorkerReply_Err>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_Err&&(identical(other.message, message) || other.message == message)&&(identical(other.kind, kind) || other.kind == kind));
}


@override
int get hashCode => Object.hash(runtimeType,message,kind);

@override
String toString() {
  return 'WorkerReply.err(message: $message, kind: $kind)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ErrCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ErrCopyWith(WorkerReply_Err value, $Res Function(WorkerReply_Err) _then) = _$WorkerReply_ErrCopyWithImpl;
@useResult
$Res call({
 String message, ErrKind kind
});




}
/// @nodoc
class _$WorkerReply_ErrCopyWithImpl<$Res>
    implements $WorkerReply_ErrCopyWith<$Res> {
  _$WorkerReply_ErrCopyWithImpl(this._self, this._then);

  final WorkerReply_Err _self;
  final $Res Function(WorkerReply_Err) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,Object? kind = null,}) {
  return _then(WorkerReply_Err(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,kind: null == kind ? _self.kind : kind // ignore: cast_nullable_to_non_nullable
as ErrKind,
  ));
}


}

/// @nodoc


class WorkerReply_TaskCreated extends WorkerReply {
  const WorkerReply_TaskCreated({required this.projectId, required this.task}): super._();


 final  String projectId;
 final  TaskSummary task;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_TaskCreatedCopyWith<WorkerReply_TaskCreated> get copyWith => _$WorkerReply_TaskCreatedCopyWithImpl<WorkerReply_TaskCreated>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_TaskCreated&&(identical(other.projectId, projectId) || other.projectId == projectId)&&(identical(other.task, task) || other.task == task));
}


@override
int get hashCode => Object.hash(runtimeType,projectId,task);

@override
String toString() {
  return 'WorkerReply.taskCreated(projectId: $projectId, task: $task)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_TaskCreatedCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_TaskCreatedCopyWith(WorkerReply_TaskCreated value, $Res Function(WorkerReply_TaskCreated) _then) = _$WorkerReply_TaskCreatedCopyWithImpl;
@useResult
$Res call({
 String projectId, TaskSummary task
});




}
/// @nodoc
class _$WorkerReply_TaskCreatedCopyWithImpl<$Res>
    implements $WorkerReply_TaskCreatedCopyWith<$Res> {
  _$WorkerReply_TaskCreatedCopyWithImpl(this._self, this._then);

  final WorkerReply_TaskCreated _self;
  final $Res Function(WorkerReply_TaskCreated) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? projectId = null,Object? task = null,}) {
  return _then(WorkerReply_TaskCreated(
projectId: null == projectId ? _self.projectId : projectId // ignore: cast_nullable_to_non_nullable
as String,task: null == task ? _self.task : task // ignore: cast_nullable_to_non_nullable
as TaskSummary,
  ));
}


}

/// @nodoc


class WorkerReply_TaskRenamed extends WorkerReply {
  const WorkerReply_TaskRenamed({required this.changed, this.task}): super._();


 final  bool changed;
 final  TaskSummary? task;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_TaskRenamedCopyWith<WorkerReply_TaskRenamed> get copyWith => _$WorkerReply_TaskRenamedCopyWithImpl<WorkerReply_TaskRenamed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_TaskRenamed&&(identical(other.changed, changed) || other.changed == changed)&&(identical(other.task, task) || other.task == task));
}


@override
int get hashCode => Object.hash(runtimeType,changed,task);

@override
String toString() {
  return 'WorkerReply.taskRenamed(changed: $changed, task: $task)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_TaskRenamedCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_TaskRenamedCopyWith(WorkerReply_TaskRenamed value, $Res Function(WorkerReply_TaskRenamed) _then) = _$WorkerReply_TaskRenamedCopyWithImpl;
@useResult
$Res call({
 bool changed, TaskSummary? task
});




}
/// @nodoc
class _$WorkerReply_TaskRenamedCopyWithImpl<$Res>
    implements $WorkerReply_TaskRenamedCopyWith<$Res> {
  _$WorkerReply_TaskRenamedCopyWithImpl(this._self, this._then);

  final WorkerReply_TaskRenamed _self;
  final $Res Function(WorkerReply_TaskRenamed) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changed = null,Object? task = freezed,}) {
  return _then(WorkerReply_TaskRenamed(
changed: null == changed ? _self.changed : changed // ignore: cast_nullable_to_non_nullable
as bool,task: freezed == task ? _self.task : task // ignore: cast_nullable_to_non_nullable
as TaskSummary?,
  ));
}


}

/// @nodoc


class WorkerReply_TaskPinned extends WorkerReply {
  const WorkerReply_TaskPinned({required this.changed, this.task}): super._();


 final  bool changed;
 final  TaskSummary? task;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_TaskPinnedCopyWith<WorkerReply_TaskPinned> get copyWith => _$WorkerReply_TaskPinnedCopyWithImpl<WorkerReply_TaskPinned>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_TaskPinned&&(identical(other.changed, changed) || other.changed == changed)&&(identical(other.task, task) || other.task == task));
}


@override
int get hashCode => Object.hash(runtimeType,changed,task);

@override
String toString() {
  return 'WorkerReply.taskPinned(changed: $changed, task: $task)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_TaskPinnedCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_TaskPinnedCopyWith(WorkerReply_TaskPinned value, $Res Function(WorkerReply_TaskPinned) _then) = _$WorkerReply_TaskPinnedCopyWithImpl;
@useResult
$Res call({
 bool changed, TaskSummary? task
});




}
/// @nodoc
class _$WorkerReply_TaskPinnedCopyWithImpl<$Res>
    implements $WorkerReply_TaskPinnedCopyWith<$Res> {
  _$WorkerReply_TaskPinnedCopyWithImpl(this._self, this._then);

  final WorkerReply_TaskPinned _self;
  final $Res Function(WorkerReply_TaskPinned) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changed = null,Object? task = freezed,}) {
  return _then(WorkerReply_TaskPinned(
changed: null == changed ? _self.changed : changed // ignore: cast_nullable_to_non_nullable
as bool,task: freezed == task ? _self.task : task // ignore: cast_nullable_to_non_nullable
as TaskSummary?,
  ));
}


}

/// @nodoc


class WorkerReply_TaskRemoved extends WorkerReply {
  const WorkerReply_TaskRemoved({required this.projectId, required this.taskId, required this.removed}): super._();


 final  String projectId;
 final  String taskId;
 final  bool removed;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_TaskRemovedCopyWith<WorkerReply_TaskRemoved> get copyWith => _$WorkerReply_TaskRemovedCopyWithImpl<WorkerReply_TaskRemoved>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_TaskRemoved&&(identical(other.projectId, projectId) || other.projectId == projectId)&&(identical(other.taskId, taskId) || other.taskId == taskId)&&(identical(other.removed, removed) || other.removed == removed));
}


@override
int get hashCode => Object.hash(runtimeType,projectId,taskId,removed);

@override
String toString() {
  return 'WorkerReply.taskRemoved(projectId: $projectId, taskId: $taskId, removed: $removed)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_TaskRemovedCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_TaskRemovedCopyWith(WorkerReply_TaskRemoved value, $Res Function(WorkerReply_TaskRemoved) _then) = _$WorkerReply_TaskRemovedCopyWithImpl;
@useResult
$Res call({
 String projectId, String taskId, bool removed
});




}
/// @nodoc
class _$WorkerReply_TaskRemovedCopyWithImpl<$Res>
    implements $WorkerReply_TaskRemovedCopyWith<$Res> {
  _$WorkerReply_TaskRemovedCopyWithImpl(this._self, this._then);

  final WorkerReply_TaskRemoved _self;
  final $Res Function(WorkerReply_TaskRemoved) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? projectId = null,Object? taskId = null,Object? removed = null,}) {
  return _then(WorkerReply_TaskRemoved(
projectId: null == projectId ? _self.projectId : projectId // ignore: cast_nullable_to_non_nullable
as String,taskId: null == taskId ? _self.taskId : taskId // ignore: cast_nullable_to_non_nullable
as String,removed: null == removed ? _self.removed : removed // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class WorkerReply_SlugifyBranchNameAck extends WorkerReply {
  const WorkerReply_SlugifyBranchNameAck({required this.slug}): super._();


 final  String slug;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_SlugifyBranchNameAckCopyWith<WorkerReply_SlugifyBranchNameAck> get copyWith => _$WorkerReply_SlugifyBranchNameAckCopyWithImpl<WorkerReply_SlugifyBranchNameAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_SlugifyBranchNameAck&&(identical(other.slug, slug) || other.slug == slug));
}


@override
int get hashCode => Object.hash(runtimeType,slug);

@override
String toString() {
  return 'WorkerReply.slugifyBranchNameAck(slug: $slug)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_SlugifyBranchNameAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_SlugifyBranchNameAckCopyWith(WorkerReply_SlugifyBranchNameAck value, $Res Function(WorkerReply_SlugifyBranchNameAck) _then) = _$WorkerReply_SlugifyBranchNameAckCopyWithImpl;
@useResult
$Res call({
 String slug
});




}
/// @nodoc
class _$WorkerReply_SlugifyBranchNameAckCopyWithImpl<$Res>
    implements $WorkerReply_SlugifyBranchNameAckCopyWith<$Res> {
  _$WorkerReply_SlugifyBranchNameAckCopyWithImpl(this._self, this._then);

  final WorkerReply_SlugifyBranchNameAck _self;
  final $Res Function(WorkerReply_SlugifyBranchNameAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? slug = null,}) {
  return _then(WorkerReply_SlugifyBranchNameAck(
slug: null == slug ? _self.slug : slug // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class WorkerReply_ProjectBranchesAck extends WorkerReply {
  const WorkerReply_ProjectBranchesAck({required final  List<String> branches}): _branches = branches,super._();


 final  List<String> _branches;
 List<String> get branches {
  if (_branches is EqualUnmodifiableListView) return _branches;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_branches);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ProjectBranchesAckCopyWith<WorkerReply_ProjectBranchesAck> get copyWith => _$WorkerReply_ProjectBranchesAckCopyWithImpl<WorkerReply_ProjectBranchesAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ProjectBranchesAck&&const DeepCollectionEquality().equals(other._branches, _branches));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_branches));

@override
String toString() {
  return 'WorkerReply.projectBranchesAck(branches: $branches)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ProjectBranchesAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ProjectBranchesAckCopyWith(WorkerReply_ProjectBranchesAck value, $Res Function(WorkerReply_ProjectBranchesAck) _then) = _$WorkerReply_ProjectBranchesAckCopyWithImpl;
@useResult
$Res call({
 List<String> branches
});




}
/// @nodoc
class _$WorkerReply_ProjectBranchesAckCopyWithImpl<$Res>
    implements $WorkerReply_ProjectBranchesAckCopyWith<$Res> {
  _$WorkerReply_ProjectBranchesAckCopyWithImpl(this._self, this._then);

  final WorkerReply_ProjectBranchesAck _self;
  final $Res Function(WorkerReply_ProjectBranchesAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? branches = null,}) {
  return _then(WorkerReply_ProjectBranchesAck(
branches: null == branches ? _self._branches : branches // ignore: cast_nullable_to_non_nullable
as List<String>,
  ));
}


}

/// @nodoc


class WorkerReply_PrimaryBranchAck extends WorkerReply {
  const WorkerReply_PrimaryBranchAck({this.branch}): super._();


 final  String? branch;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_PrimaryBranchAckCopyWith<WorkerReply_PrimaryBranchAck> get copyWith => _$WorkerReply_PrimaryBranchAckCopyWithImpl<WorkerReply_PrimaryBranchAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_PrimaryBranchAck&&(identical(other.branch, branch) || other.branch == branch));
}


@override
int get hashCode => Object.hash(runtimeType,branch);

@override
String toString() {
  return 'WorkerReply.primaryBranchAck(branch: $branch)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_PrimaryBranchAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_PrimaryBranchAckCopyWith(WorkerReply_PrimaryBranchAck value, $Res Function(WorkerReply_PrimaryBranchAck) _then) = _$WorkerReply_PrimaryBranchAckCopyWithImpl;
@useResult
$Res call({
 String? branch
});




}
/// @nodoc
class _$WorkerReply_PrimaryBranchAckCopyWithImpl<$Res>
    implements $WorkerReply_PrimaryBranchAckCopyWith<$Res> {
  _$WorkerReply_PrimaryBranchAckCopyWithImpl(this._self, this._then);

  final WorkerReply_PrimaryBranchAck _self;
  final $Res Function(WorkerReply_PrimaryBranchAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? branch = freezed,}) {
  return _then(WorkerReply_PrimaryBranchAck(
branch: freezed == branch ? _self.branch : branch // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class WorkerReply_RepoDefaultCommitActionAck extends WorkerReply {
  const WorkerReply_RepoDefaultCommitActionAck({this.action}): super._();


 final  String? action;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_RepoDefaultCommitActionAckCopyWith<WorkerReply_RepoDefaultCommitActionAck> get copyWith => _$WorkerReply_RepoDefaultCommitActionAckCopyWithImpl<WorkerReply_RepoDefaultCommitActionAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_RepoDefaultCommitActionAck&&(identical(other.action, action) || other.action == action));
}


@override
int get hashCode => Object.hash(runtimeType,action);

@override
String toString() {
  return 'WorkerReply.repoDefaultCommitActionAck(action: $action)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_RepoDefaultCommitActionAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_RepoDefaultCommitActionAckCopyWith(WorkerReply_RepoDefaultCommitActionAck value, $Res Function(WorkerReply_RepoDefaultCommitActionAck) _then) = _$WorkerReply_RepoDefaultCommitActionAckCopyWithImpl;
@useResult
$Res call({
 String? action
});




}
/// @nodoc
class _$WorkerReply_RepoDefaultCommitActionAckCopyWithImpl<$Res>
    implements $WorkerReply_RepoDefaultCommitActionAckCopyWith<$Res> {
  _$WorkerReply_RepoDefaultCommitActionAckCopyWithImpl(this._self, this._then);

  final WorkerReply_RepoDefaultCommitActionAck _self;
  final $Res Function(WorkerReply_RepoDefaultCommitActionAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? action = freezed,}) {
  return _then(WorkerReply_RepoDefaultCommitActionAck(
action: freezed == action ? _self.action : action // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class WorkerReply_ActiveGitStateAck extends WorkerReply {
  const WorkerReply_ActiveGitStateAck({this.state}): super._();


 final  ActiveGitStateDto? state;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ActiveGitStateAckCopyWith<WorkerReply_ActiveGitStateAck> get copyWith => _$WorkerReply_ActiveGitStateAckCopyWithImpl<WorkerReply_ActiveGitStateAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ActiveGitStateAck&&(identical(other.state, state) || other.state == state));
}


@override
int get hashCode => Object.hash(runtimeType,state);

@override
String toString() {
  return 'WorkerReply.activeGitStateAck(state: $state)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ActiveGitStateAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ActiveGitStateAckCopyWith(WorkerReply_ActiveGitStateAck value, $Res Function(WorkerReply_ActiveGitStateAck) _then) = _$WorkerReply_ActiveGitStateAckCopyWithImpl;
@useResult
$Res call({
 ActiveGitStateDto? state
});




}
/// @nodoc
class _$WorkerReply_ActiveGitStateAckCopyWithImpl<$Res>
    implements $WorkerReply_ActiveGitStateAckCopyWith<$Res> {
  _$WorkerReply_ActiveGitStateAckCopyWithImpl(this._self, this._then);

  final WorkerReply_ActiveGitStateAck _self;
  final $Res Function(WorkerReply_ActiveGitStateAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? state = freezed,}) {
  return _then(WorkerReply_ActiveGitStateAck(
state: freezed == state ? _self.state : state // ignore: cast_nullable_to_non_nullable
as ActiveGitStateDto?,
  ));
}


}

/// @nodoc


class WorkerReply_ChangedFilesAck extends WorkerReply {
  const WorkerReply_ChangedFilesAck({final  List<ChangedFileDto>? files}): _files = files,super._();


 final  List<ChangedFileDto>? _files;
 List<ChangedFileDto>? get files {
  final value = _files;
  if (value == null) return null;
  if (_files is EqualUnmodifiableListView) return _files;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(value);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ChangedFilesAckCopyWith<WorkerReply_ChangedFilesAck> get copyWith => _$WorkerReply_ChangedFilesAckCopyWithImpl<WorkerReply_ChangedFilesAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ChangedFilesAck&&const DeepCollectionEquality().equals(other._files, _files));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_files));

@override
String toString() {
  return 'WorkerReply.changedFilesAck(files: $files)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ChangedFilesAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ChangedFilesAckCopyWith(WorkerReply_ChangedFilesAck value, $Res Function(WorkerReply_ChangedFilesAck) _then) = _$WorkerReply_ChangedFilesAckCopyWithImpl;
@useResult
$Res call({
 List<ChangedFileDto>? files
});




}
/// @nodoc
class _$WorkerReply_ChangedFilesAckCopyWithImpl<$Res>
    implements $WorkerReply_ChangedFilesAckCopyWith<$Res> {
  _$WorkerReply_ChangedFilesAckCopyWithImpl(this._self, this._then);

  final WorkerReply_ChangedFilesAck _self;
  final $Res Function(WorkerReply_ChangedFilesAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? files = freezed,}) {
  return _then(WorkerReply_ChangedFilesAck(
files: freezed == files ? _self._files : files // ignore: cast_nullable_to_non_nullable
as List<ChangedFileDto>?,
  ));
}


}

/// @nodoc


class WorkerReply_ProjectGithubUrlAck extends WorkerReply {
  const WorkerReply_ProjectGithubUrlAck({this.url}): super._();


 final  String? url;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ProjectGithubUrlAckCopyWith<WorkerReply_ProjectGithubUrlAck> get copyWith => _$WorkerReply_ProjectGithubUrlAckCopyWithImpl<WorkerReply_ProjectGithubUrlAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ProjectGithubUrlAck&&(identical(other.url, url) || other.url == url));
}


@override
int get hashCode => Object.hash(runtimeType,url);

@override
String toString() {
  return 'WorkerReply.projectGithubUrlAck(url: $url)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ProjectGithubUrlAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ProjectGithubUrlAckCopyWith(WorkerReply_ProjectGithubUrlAck value, $Res Function(WorkerReply_ProjectGithubUrlAck) _then) = _$WorkerReply_ProjectGithubUrlAckCopyWithImpl;
@useResult
$Res call({
 String? url
});




}
/// @nodoc
class _$WorkerReply_ProjectGithubUrlAckCopyWithImpl<$Res>
    implements $WorkerReply_ProjectGithubUrlAckCopyWith<$Res> {
  _$WorkerReply_ProjectGithubUrlAckCopyWithImpl(this._self, this._then);

  final WorkerReply_ProjectGithubUrlAck _self;
  final $Res Function(WorkerReply_ProjectGithubUrlAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? url = freezed,}) {
  return _then(WorkerReply_ProjectGithubUrlAck(
url: freezed == url ? _self.url : url // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class WorkerReply_RecentCommitsAck extends WorkerReply {
  const WorkerReply_RecentCommitsAck({this.view}): super._();


 final  RecentCommitsView? view;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_RecentCommitsAckCopyWith<WorkerReply_RecentCommitsAck> get copyWith => _$WorkerReply_RecentCommitsAckCopyWithImpl<WorkerReply_RecentCommitsAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_RecentCommitsAck&&(identical(other.view, view) || other.view == view));
}


@override
int get hashCode => Object.hash(runtimeType,view);

@override
String toString() {
  return 'WorkerReply.recentCommitsAck(view: $view)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_RecentCommitsAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_RecentCommitsAckCopyWith(WorkerReply_RecentCommitsAck value, $Res Function(WorkerReply_RecentCommitsAck) _then) = _$WorkerReply_RecentCommitsAckCopyWithImpl;
@useResult
$Res call({
 RecentCommitsView? view
});




}
/// @nodoc
class _$WorkerReply_RecentCommitsAckCopyWithImpl<$Res>
    implements $WorkerReply_RecentCommitsAckCopyWith<$Res> {
  _$WorkerReply_RecentCommitsAckCopyWithImpl(this._self, this._then);

  final WorkerReply_RecentCommitsAck _self;
  final $Res Function(WorkerReply_RecentCommitsAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? view = freezed,}) {
  return _then(WorkerReply_RecentCommitsAck(
view: freezed == view ? _self.view : view // ignore: cast_nullable_to_non_nullable
as RecentCommitsView?,
  ));
}


}

/// @nodoc


class WorkerReply_CommitFileChangesAck extends WorkerReply {
  const WorkerReply_CommitFileChangesAck({final  List<BranchCompareFileDto>? files}): _files = files,super._();


 final  List<BranchCompareFileDto>? _files;
 List<BranchCompareFileDto>? get files {
  final value = _files;
  if (value == null) return null;
  if (_files is EqualUnmodifiableListView) return _files;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(value);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_CommitFileChangesAckCopyWith<WorkerReply_CommitFileChangesAck> get copyWith => _$WorkerReply_CommitFileChangesAckCopyWithImpl<WorkerReply_CommitFileChangesAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_CommitFileChangesAck&&const DeepCollectionEquality().equals(other._files, _files));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_files));

@override
String toString() {
  return 'WorkerReply.commitFileChangesAck(files: $files)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_CommitFileChangesAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_CommitFileChangesAckCopyWith(WorkerReply_CommitFileChangesAck value, $Res Function(WorkerReply_CommitFileChangesAck) _then) = _$WorkerReply_CommitFileChangesAckCopyWithImpl;
@useResult
$Res call({
 List<BranchCompareFileDto>? files
});




}
/// @nodoc
class _$WorkerReply_CommitFileChangesAckCopyWithImpl<$Res>
    implements $WorkerReply_CommitFileChangesAckCopyWith<$Res> {
  _$WorkerReply_CommitFileChangesAckCopyWithImpl(this._self, this._then);

  final WorkerReply_CommitFileChangesAck _self;
  final $Res Function(WorkerReply_CommitFileChangesAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? files = freezed,}) {
  return _then(WorkerReply_CommitFileChangesAck(
files: freezed == files ? _self._files : files // ignore: cast_nullable_to_non_nullable
as List<BranchCompareFileDto>?,
  ));
}


}

/// @nodoc


class WorkerReply_BranchCompareAck extends WorkerReply {
  const WorkerReply_BranchCompareAck({this.view}): super._();


 final  BranchCompareView? view;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_BranchCompareAckCopyWith<WorkerReply_BranchCompareAck> get copyWith => _$WorkerReply_BranchCompareAckCopyWithImpl<WorkerReply_BranchCompareAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_BranchCompareAck&&(identical(other.view, view) || other.view == view));
}


@override
int get hashCode => Object.hash(runtimeType,view);

@override
String toString() {
  return 'WorkerReply.branchCompareAck(view: $view)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_BranchCompareAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_BranchCompareAckCopyWith(WorkerReply_BranchCompareAck value, $Res Function(WorkerReply_BranchCompareAck) _then) = _$WorkerReply_BranchCompareAckCopyWithImpl;
@useResult
$Res call({
 BranchCompareView? view
});




}
/// @nodoc
class _$WorkerReply_BranchCompareAckCopyWithImpl<$Res>
    implements $WorkerReply_BranchCompareAckCopyWith<$Res> {
  _$WorkerReply_BranchCompareAckCopyWithImpl(this._self, this._then);

  final WorkerReply_BranchCompareAck _self;
  final $Res Function(WorkerReply_BranchCompareAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? view = freezed,}) {
  return _then(WorkerReply_BranchCompareAck(
view: freezed == view ? _self.view : view // ignore: cast_nullable_to_non_nullable
as BranchCompareView?,
  ));
}


}

/// @nodoc


class WorkerReply_BranchSettingsAck extends WorkerReply {
  const WorkerReply_BranchSettingsAck({this.settings}): super._();


 final  ResolvedProjectBranchSettingsDto? settings;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_BranchSettingsAckCopyWith<WorkerReply_BranchSettingsAck> get copyWith => _$WorkerReply_BranchSettingsAckCopyWithImpl<WorkerReply_BranchSettingsAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_BranchSettingsAck&&(identical(other.settings, settings) || other.settings == settings));
}


@override
int get hashCode => Object.hash(runtimeType,settings);

@override
String toString() {
  return 'WorkerReply.branchSettingsAck(settings: $settings)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_BranchSettingsAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_BranchSettingsAckCopyWith(WorkerReply_BranchSettingsAck value, $Res Function(WorkerReply_BranchSettingsAck) _then) = _$WorkerReply_BranchSettingsAckCopyWithImpl;
@useResult
$Res call({
 ResolvedProjectBranchSettingsDto? settings
});




}
/// @nodoc
class _$WorkerReply_BranchSettingsAckCopyWithImpl<$Res>
    implements $WorkerReply_BranchSettingsAckCopyWith<$Res> {
  _$WorkerReply_BranchSettingsAckCopyWithImpl(this._self, this._then);

  final WorkerReply_BranchSettingsAck _self;
  final $Res Function(WorkerReply_BranchSettingsAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? settings = freezed,}) {
  return _then(WorkerReply_BranchSettingsAck(
settings: freezed == settings ? _self.settings : settings // ignore: cast_nullable_to_non_nullable
as ResolvedProjectBranchSettingsDto?,
  ));
}


}

/// @nodoc


class WorkerReply_SetBranchSettingAck extends WorkerReply {
  const WorkerReply_SetBranchSettingAck({required this.changed}): super._();


 final  bool changed;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_SetBranchSettingAckCopyWith<WorkerReply_SetBranchSettingAck> get copyWith => _$WorkerReply_SetBranchSettingAckCopyWithImpl<WorkerReply_SetBranchSettingAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_SetBranchSettingAck&&(identical(other.changed, changed) || other.changed == changed));
}


@override
int get hashCode => Object.hash(runtimeType,changed);

@override
String toString() {
  return 'WorkerReply.setBranchSettingAck(changed: $changed)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_SetBranchSettingAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_SetBranchSettingAckCopyWith(WorkerReply_SetBranchSettingAck value, $Res Function(WorkerReply_SetBranchSettingAck) _then) = _$WorkerReply_SetBranchSettingAckCopyWithImpl;
@useResult
$Res call({
 bool changed
});




}
/// @nodoc
class _$WorkerReply_SetBranchSettingAckCopyWithImpl<$Res>
    implements $WorkerReply_SetBranchSettingAckCopyWith<$Res> {
  _$WorkerReply_SetBranchSettingAckCopyWithImpl(this._self, this._then);

  final WorkerReply_SetBranchSettingAck _self;
  final $Res Function(WorkerReply_SetBranchSettingAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changed = null,}) {
  return _then(WorkerReply_SetBranchSettingAck(
changed: null == changed ? _self.changed : changed // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class WorkerReply_StageChangedFileAck extends WorkerReply {
  const WorkerReply_StageChangedFileAck({required final  List<ChangedFileDto> changedFiles}): _changedFiles = changedFiles,super._();


 final  List<ChangedFileDto> _changedFiles;
 List<ChangedFileDto> get changedFiles {
  if (_changedFiles is EqualUnmodifiableListView) return _changedFiles;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_changedFiles);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_StageChangedFileAckCopyWith<WorkerReply_StageChangedFileAck> get copyWith => _$WorkerReply_StageChangedFileAckCopyWithImpl<WorkerReply_StageChangedFileAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_StageChangedFileAck&&const DeepCollectionEquality().equals(other._changedFiles, _changedFiles));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_changedFiles));

@override
String toString() {
  return 'WorkerReply.stageChangedFileAck(changedFiles: $changedFiles)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_StageChangedFileAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_StageChangedFileAckCopyWith(WorkerReply_StageChangedFileAck value, $Res Function(WorkerReply_StageChangedFileAck) _then) = _$WorkerReply_StageChangedFileAckCopyWithImpl;
@useResult
$Res call({
 List<ChangedFileDto> changedFiles
});




}
/// @nodoc
class _$WorkerReply_StageChangedFileAckCopyWithImpl<$Res>
    implements $WorkerReply_StageChangedFileAckCopyWith<$Res> {
  _$WorkerReply_StageChangedFileAckCopyWithImpl(this._self, this._then);

  final WorkerReply_StageChangedFileAck _self;
  final $Res Function(WorkerReply_StageChangedFileAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changedFiles = null,}) {
  return _then(WorkerReply_StageChangedFileAck(
changedFiles: null == changedFiles ? _self._changedFiles : changedFiles // ignore: cast_nullable_to_non_nullable
as List<ChangedFileDto>,
  ));
}


}

/// @nodoc


class WorkerReply_UnstageChangedFileAck extends WorkerReply {
  const WorkerReply_UnstageChangedFileAck({required final  List<ChangedFileDto> changedFiles}): _changedFiles = changedFiles,super._();


 final  List<ChangedFileDto> _changedFiles;
 List<ChangedFileDto> get changedFiles {
  if (_changedFiles is EqualUnmodifiableListView) return _changedFiles;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_changedFiles);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_UnstageChangedFileAckCopyWith<WorkerReply_UnstageChangedFileAck> get copyWith => _$WorkerReply_UnstageChangedFileAckCopyWithImpl<WorkerReply_UnstageChangedFileAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_UnstageChangedFileAck&&const DeepCollectionEquality().equals(other._changedFiles, _changedFiles));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_changedFiles));

@override
String toString() {
  return 'WorkerReply.unstageChangedFileAck(changedFiles: $changedFiles)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_UnstageChangedFileAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_UnstageChangedFileAckCopyWith(WorkerReply_UnstageChangedFileAck value, $Res Function(WorkerReply_UnstageChangedFileAck) _then) = _$WorkerReply_UnstageChangedFileAckCopyWithImpl;
@useResult
$Res call({
 List<ChangedFileDto> changedFiles
});




}
/// @nodoc
class _$WorkerReply_UnstageChangedFileAckCopyWithImpl<$Res>
    implements $WorkerReply_UnstageChangedFileAckCopyWith<$Res> {
  _$WorkerReply_UnstageChangedFileAckCopyWithImpl(this._self, this._then);

  final WorkerReply_UnstageChangedFileAck _self;
  final $Res Function(WorkerReply_UnstageChangedFileAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changedFiles = null,}) {
  return _then(WorkerReply_UnstageChangedFileAck(
changedFiles: null == changedFiles ? _self._changedFiles : changedFiles // ignore: cast_nullable_to_non_nullable
as List<ChangedFileDto>,
  ));
}


}

/// @nodoc


class WorkerReply_StageAllChangesAck extends WorkerReply {
  const WorkerReply_StageAllChangesAck({required final  List<ChangedFileDto> changedFiles}): _changedFiles = changedFiles,super._();


 final  List<ChangedFileDto> _changedFiles;
 List<ChangedFileDto> get changedFiles {
  if (_changedFiles is EqualUnmodifiableListView) return _changedFiles;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_changedFiles);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_StageAllChangesAckCopyWith<WorkerReply_StageAllChangesAck> get copyWith => _$WorkerReply_StageAllChangesAckCopyWithImpl<WorkerReply_StageAllChangesAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_StageAllChangesAck&&const DeepCollectionEquality().equals(other._changedFiles, _changedFiles));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_changedFiles));

@override
String toString() {
  return 'WorkerReply.stageAllChangesAck(changedFiles: $changedFiles)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_StageAllChangesAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_StageAllChangesAckCopyWith(WorkerReply_StageAllChangesAck value, $Res Function(WorkerReply_StageAllChangesAck) _then) = _$WorkerReply_StageAllChangesAckCopyWithImpl;
@useResult
$Res call({
 List<ChangedFileDto> changedFiles
});




}
/// @nodoc
class _$WorkerReply_StageAllChangesAckCopyWithImpl<$Res>
    implements $WorkerReply_StageAllChangesAckCopyWith<$Res> {
  _$WorkerReply_StageAllChangesAckCopyWithImpl(this._self, this._then);

  final WorkerReply_StageAllChangesAck _self;
  final $Res Function(WorkerReply_StageAllChangesAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changedFiles = null,}) {
  return _then(WorkerReply_StageAllChangesAck(
changedFiles: null == changedFiles ? _self._changedFiles : changedFiles // ignore: cast_nullable_to_non_nullable
as List<ChangedFileDto>,
  ));
}


}

/// @nodoc


class WorkerReply_UnstageAllChangesAck extends WorkerReply {
  const WorkerReply_UnstageAllChangesAck({required final  List<ChangedFileDto> changedFiles}): _changedFiles = changedFiles,super._();


 final  List<ChangedFileDto> _changedFiles;
 List<ChangedFileDto> get changedFiles {
  if (_changedFiles is EqualUnmodifiableListView) return _changedFiles;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_changedFiles);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_UnstageAllChangesAckCopyWith<WorkerReply_UnstageAllChangesAck> get copyWith => _$WorkerReply_UnstageAllChangesAckCopyWithImpl<WorkerReply_UnstageAllChangesAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_UnstageAllChangesAck&&const DeepCollectionEquality().equals(other._changedFiles, _changedFiles));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_changedFiles));

@override
String toString() {
  return 'WorkerReply.unstageAllChangesAck(changedFiles: $changedFiles)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_UnstageAllChangesAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_UnstageAllChangesAckCopyWith(WorkerReply_UnstageAllChangesAck value, $Res Function(WorkerReply_UnstageAllChangesAck) _then) = _$WorkerReply_UnstageAllChangesAckCopyWithImpl;
@useResult
$Res call({
 List<ChangedFileDto> changedFiles
});




}
/// @nodoc
class _$WorkerReply_UnstageAllChangesAckCopyWithImpl<$Res>
    implements $WorkerReply_UnstageAllChangesAckCopyWith<$Res> {
  _$WorkerReply_UnstageAllChangesAckCopyWithImpl(this._self, this._then);

  final WorkerReply_UnstageAllChangesAck _self;
  final $Res Function(WorkerReply_UnstageAllChangesAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changedFiles = null,}) {
  return _then(WorkerReply_UnstageAllChangesAck(
changedFiles: null == changedFiles ? _self._changedFiles : changedFiles // ignore: cast_nullable_to_non_nullable
as List<ChangedFileDto>,
  ));
}


}

/// @nodoc


class WorkerReply_DiscardChangedFileAck extends WorkerReply {
  const WorkerReply_DiscardChangedFileAck({required final  List<ChangedFileDto> changedFiles}): _changedFiles = changedFiles,super._();


 final  List<ChangedFileDto> _changedFiles;
 List<ChangedFileDto> get changedFiles {
  if (_changedFiles is EqualUnmodifiableListView) return _changedFiles;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_changedFiles);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_DiscardChangedFileAckCopyWith<WorkerReply_DiscardChangedFileAck> get copyWith => _$WorkerReply_DiscardChangedFileAckCopyWithImpl<WorkerReply_DiscardChangedFileAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_DiscardChangedFileAck&&const DeepCollectionEquality().equals(other._changedFiles, _changedFiles));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_changedFiles));

@override
String toString() {
  return 'WorkerReply.discardChangedFileAck(changedFiles: $changedFiles)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_DiscardChangedFileAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_DiscardChangedFileAckCopyWith(WorkerReply_DiscardChangedFileAck value, $Res Function(WorkerReply_DiscardChangedFileAck) _then) = _$WorkerReply_DiscardChangedFileAckCopyWithImpl;
@useResult
$Res call({
 List<ChangedFileDto> changedFiles
});




}
/// @nodoc
class _$WorkerReply_DiscardChangedFileAckCopyWithImpl<$Res>
    implements $WorkerReply_DiscardChangedFileAckCopyWith<$Res> {
  _$WorkerReply_DiscardChangedFileAckCopyWithImpl(this._self, this._then);

  final WorkerReply_DiscardChangedFileAck _self;
  final $Res Function(WorkerReply_DiscardChangedFileAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changedFiles = null,}) {
  return _then(WorkerReply_DiscardChangedFileAck(
changedFiles: null == changedFiles ? _self._changedFiles : changedFiles // ignore: cast_nullable_to_non_nullable
as List<ChangedFileDto>,
  ));
}


}

/// @nodoc


class WorkerReply_DiscardAllChangesAck extends WorkerReply {
  const WorkerReply_DiscardAllChangesAck({required final  List<ChangedFileDto> changedFiles, required final  List<String> failures}): _changedFiles = changedFiles,_failures = failures,super._();


 final  List<ChangedFileDto> _changedFiles;
 List<ChangedFileDto> get changedFiles {
  if (_changedFiles is EqualUnmodifiableListView) return _changedFiles;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_changedFiles);
}

 final  List<String> _failures;
 List<String> get failures {
  if (_failures is EqualUnmodifiableListView) return _failures;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_failures);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_DiscardAllChangesAckCopyWith<WorkerReply_DiscardAllChangesAck> get copyWith => _$WorkerReply_DiscardAllChangesAckCopyWithImpl<WorkerReply_DiscardAllChangesAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_DiscardAllChangesAck&&const DeepCollectionEquality().equals(other._changedFiles, _changedFiles)&&const DeepCollectionEquality().equals(other._failures, _failures));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_changedFiles),const DeepCollectionEquality().hash(_failures));

@override
String toString() {
  return 'WorkerReply.discardAllChangesAck(changedFiles: $changedFiles, failures: $failures)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_DiscardAllChangesAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_DiscardAllChangesAckCopyWith(WorkerReply_DiscardAllChangesAck value, $Res Function(WorkerReply_DiscardAllChangesAck) _then) = _$WorkerReply_DiscardAllChangesAckCopyWithImpl;
@useResult
$Res call({
 List<ChangedFileDto> changedFiles, List<String> failures
});




}
/// @nodoc
class _$WorkerReply_DiscardAllChangesAckCopyWithImpl<$Res>
    implements $WorkerReply_DiscardAllChangesAckCopyWith<$Res> {
  _$WorkerReply_DiscardAllChangesAckCopyWithImpl(this._self, this._then);

  final WorkerReply_DiscardAllChangesAck _self;
  final $Res Function(WorkerReply_DiscardAllChangesAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changedFiles = null,Object? failures = null,}) {
  return _then(WorkerReply_DiscardAllChangesAck(
changedFiles: null == changedFiles ? _self._changedFiles : changedFiles // ignore: cast_nullable_to_non_nullable
as List<ChangedFileDto>,failures: null == failures ? _self._failures : failures // ignore: cast_nullable_to_non_nullable
as List<String>,
  ));
}


}

/// @nodoc


class WorkerReply_ToolbarActionOutcomeAck extends WorkerReply {
  const WorkerReply_ToolbarActionOutcomeAck({required this.outcome}): super._();


 final  ToolbarActionOutcomeDto outcome;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ToolbarActionOutcomeAckCopyWith<WorkerReply_ToolbarActionOutcomeAck> get copyWith => _$WorkerReply_ToolbarActionOutcomeAckCopyWithImpl<WorkerReply_ToolbarActionOutcomeAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ToolbarActionOutcomeAck&&(identical(other.outcome, outcome) || other.outcome == outcome));
}


@override
int get hashCode => Object.hash(runtimeType,outcome);

@override
String toString() {
  return 'WorkerReply.toolbarActionOutcomeAck(outcome: $outcome)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ToolbarActionOutcomeAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ToolbarActionOutcomeAckCopyWith(WorkerReply_ToolbarActionOutcomeAck value, $Res Function(WorkerReply_ToolbarActionOutcomeAck) _then) = _$WorkerReply_ToolbarActionOutcomeAckCopyWithImpl;
@useResult
$Res call({
 ToolbarActionOutcomeDto outcome
});




}
/// @nodoc
class _$WorkerReply_ToolbarActionOutcomeAckCopyWithImpl<$Res>
    implements $WorkerReply_ToolbarActionOutcomeAckCopyWith<$Res> {
  _$WorkerReply_ToolbarActionOutcomeAckCopyWithImpl(this._self, this._then);

  final WorkerReply_ToolbarActionOutcomeAck _self;
  final $Res Function(WorkerReply_ToolbarActionOutcomeAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? outcome = null,}) {
  return _then(WorkerReply_ToolbarActionOutcomeAck(
outcome: null == outcome ? _self.outcome : outcome // ignore: cast_nullable_to_non_nullable
as ToolbarActionOutcomeDto,
  ));
}


}

/// @nodoc


class WorkerReply_CreateBranchAck extends WorkerReply {
  const WorkerReply_CreateBranchAck({required this.sectionId, required final  List<ProjectSummary> projects}): _projects = projects,super._();


 final  String sectionId;
 final  List<ProjectSummary> _projects;
 List<ProjectSummary> get projects {
  if (_projects is EqualUnmodifiableListView) return _projects;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_projects);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_CreateBranchAckCopyWith<WorkerReply_CreateBranchAck> get copyWith => _$WorkerReply_CreateBranchAckCopyWithImpl<WorkerReply_CreateBranchAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_CreateBranchAck&&(identical(other.sectionId, sectionId) || other.sectionId == sectionId)&&const DeepCollectionEquality().equals(other._projects, _projects));
}


@override
int get hashCode => Object.hash(runtimeType,sectionId,const DeepCollectionEquality().hash(_projects));

@override
String toString() {
  return 'WorkerReply.createBranchAck(sectionId: $sectionId, projects: $projects)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_CreateBranchAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_CreateBranchAckCopyWith(WorkerReply_CreateBranchAck value, $Res Function(WorkerReply_CreateBranchAck) _then) = _$WorkerReply_CreateBranchAckCopyWithImpl;
@useResult
$Res call({
 String sectionId, List<ProjectSummary> projects
});




}
/// @nodoc
class _$WorkerReply_CreateBranchAckCopyWithImpl<$Res>
    implements $WorkerReply_CreateBranchAckCopyWith<$Res> {
  _$WorkerReply_CreateBranchAckCopyWithImpl(this._self, this._then);

  final WorkerReply_CreateBranchAck _self;
  final $Res Function(WorkerReply_CreateBranchAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? sectionId = null,Object? projects = null,}) {
  return _then(WorkerReply_CreateBranchAck(
sectionId: null == sectionId ? _self.sectionId : sectionId // ignore: cast_nullable_to_non_nullable
as String,projects: null == projects ? _self._projects : projects // ignore: cast_nullable_to_non_nullable
as List<ProjectSummary>,
  ));
}


}

/// @nodoc


class WorkerReply_CreateReviewTaskAck extends WorkerReply {
  const WorkerReply_CreateReviewTaskAck({required this.sectionId, required final  List<ProjectSummary> projects}): _projects = projects,super._();


 final  String sectionId;
 final  List<ProjectSummary> _projects;
 List<ProjectSummary> get projects {
  if (_projects is EqualUnmodifiableListView) return _projects;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_projects);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_CreateReviewTaskAckCopyWith<WorkerReply_CreateReviewTaskAck> get copyWith => _$WorkerReply_CreateReviewTaskAckCopyWithImpl<WorkerReply_CreateReviewTaskAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_CreateReviewTaskAck&&(identical(other.sectionId, sectionId) || other.sectionId == sectionId)&&const DeepCollectionEquality().equals(other._projects, _projects));
}


@override
int get hashCode => Object.hash(runtimeType,sectionId,const DeepCollectionEquality().hash(_projects));

@override
String toString() {
  return 'WorkerReply.createReviewTaskAck(sectionId: $sectionId, projects: $projects)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_CreateReviewTaskAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_CreateReviewTaskAckCopyWith(WorkerReply_CreateReviewTaskAck value, $Res Function(WorkerReply_CreateReviewTaskAck) _then) = _$WorkerReply_CreateReviewTaskAckCopyWithImpl;
@useResult
$Res call({
 String sectionId, List<ProjectSummary> projects
});




}
/// @nodoc
class _$WorkerReply_CreateReviewTaskAckCopyWithImpl<$Res>
    implements $WorkerReply_CreateReviewTaskAckCopyWith<$Res> {
  _$WorkerReply_CreateReviewTaskAckCopyWithImpl(this._self, this._then);

  final WorkerReply_CreateReviewTaskAck _self;
  final $Res Function(WorkerReply_CreateReviewTaskAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? sectionId = null,Object? projects = null,}) {
  return _then(WorkerReply_CreateReviewTaskAck(
sectionId: null == sectionId ? _self.sectionId : sectionId // ignore: cast_nullable_to_non_nullable
as String,projects: null == projects ? _self._projects : projects // ignore: cast_nullable_to_non_nullable
as List<ProjectSummary>,
  ));
}


}

/// @nodoc


class WorkerReply_PullRequestStatusAck extends WorkerReply {
  const WorkerReply_PullRequestStatusAck({this.status}): super._();


 final  PullRequestStatusDto? status;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_PullRequestStatusAckCopyWith<WorkerReply_PullRequestStatusAck> get copyWith => _$WorkerReply_PullRequestStatusAckCopyWithImpl<WorkerReply_PullRequestStatusAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_PullRequestStatusAck&&(identical(other.status, status) || other.status == status));
}


@override
int get hashCode => Object.hash(runtimeType,status);

@override
String toString() {
  return 'WorkerReply.pullRequestStatusAck(status: $status)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_PullRequestStatusAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_PullRequestStatusAckCopyWith(WorkerReply_PullRequestStatusAck value, $Res Function(WorkerReply_PullRequestStatusAck) _then) = _$WorkerReply_PullRequestStatusAckCopyWithImpl;
@useResult
$Res call({
 PullRequestStatusDto? status
});




}
/// @nodoc
class _$WorkerReply_PullRequestStatusAckCopyWithImpl<$Res>
    implements $WorkerReply_PullRequestStatusAckCopyWith<$Res> {
  _$WorkerReply_PullRequestStatusAckCopyWithImpl(this._self, this._then);

  final WorkerReply_PullRequestStatusAck _self;
  final $Res Function(WorkerReply_PullRequestStatusAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? status = freezed,}) {
  return _then(WorkerReply_PullRequestStatusAck(
status: freezed == status ? _self.status : status // ignore: cast_nullable_to_non_nullable
as PullRequestStatusDto?,
  ));
}


}

/// @nodoc


class WorkerReply_PullRequestChecksAck extends WorkerReply {
  const WorkerReply_PullRequestChecksAck({final  List<CheckDto>? checks}): _checks = checks,super._();


 final  List<CheckDto>? _checks;
 List<CheckDto>? get checks {
  final value = _checks;
  if (value == null) return null;
  if (_checks is EqualUnmodifiableListView) return _checks;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(value);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_PullRequestChecksAckCopyWith<WorkerReply_PullRequestChecksAck> get copyWith => _$WorkerReply_PullRequestChecksAckCopyWithImpl<WorkerReply_PullRequestChecksAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_PullRequestChecksAck&&const DeepCollectionEquality().equals(other._checks, _checks));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_checks));

@override
String toString() {
  return 'WorkerReply.pullRequestChecksAck(checks: $checks)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_PullRequestChecksAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_PullRequestChecksAckCopyWith(WorkerReply_PullRequestChecksAck value, $Res Function(WorkerReply_PullRequestChecksAck) _then) = _$WorkerReply_PullRequestChecksAckCopyWithImpl;
@useResult
$Res call({
 List<CheckDto>? checks
});




}
/// @nodoc
class _$WorkerReply_PullRequestChecksAckCopyWithImpl<$Res>
    implements $WorkerReply_PullRequestChecksAckCopyWith<$Res> {
  _$WorkerReply_PullRequestChecksAckCopyWithImpl(this._self, this._then);

  final WorkerReply_PullRequestChecksAck _self;
  final $Res Function(WorkerReply_PullRequestChecksAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? checks = freezed,}) {
  return _then(WorkerReply_PullRequestChecksAck(
checks: freezed == checks ? _self._checks : checks // ignore: cast_nullable_to_non_nullable
as List<CheckDto>?,
  ));
}


}

/// @nodoc


class WorkerReply_ProjectPullRequestsAck extends WorkerReply {
  const WorkerReply_ProjectPullRequestsAck({final  List<ProjectPagePullRequestDto>? prs}): _prs = prs,super._();


 final  List<ProjectPagePullRequestDto>? _prs;
 List<ProjectPagePullRequestDto>? get prs {
  final value = _prs;
  if (value == null) return null;
  if (_prs is EqualUnmodifiableListView) return _prs;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(value);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ProjectPullRequestsAckCopyWith<WorkerReply_ProjectPullRequestsAck> get copyWith => _$WorkerReply_ProjectPullRequestsAckCopyWithImpl<WorkerReply_ProjectPullRequestsAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ProjectPullRequestsAck&&const DeepCollectionEquality().equals(other._prs, _prs));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_prs));

@override
String toString() {
  return 'WorkerReply.projectPullRequestsAck(prs: $prs)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ProjectPullRequestsAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ProjectPullRequestsAckCopyWith(WorkerReply_ProjectPullRequestsAck value, $Res Function(WorkerReply_ProjectPullRequestsAck) _then) = _$WorkerReply_ProjectPullRequestsAckCopyWithImpl;
@useResult
$Res call({
 List<ProjectPagePullRequestDto>? prs
});




}
/// @nodoc
class _$WorkerReply_ProjectPullRequestsAckCopyWithImpl<$Res>
    implements $WorkerReply_ProjectPullRequestsAckCopyWith<$Res> {
  _$WorkerReply_ProjectPullRequestsAckCopyWithImpl(this._self, this._then);

  final WorkerReply_ProjectPullRequestsAck _self;
  final $Res Function(WorkerReply_ProjectPullRequestsAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? prs = freezed,}) {
  return _then(WorkerReply_ProjectPullRequestsAck(
prs: freezed == prs ? _self._prs : prs // ignore: cast_nullable_to_non_nullable
as List<ProjectPagePullRequestDto>?,
  ));
}


}

/// @nodoc


class WorkerReply_OpenInStateAck extends WorkerReply {
  const WorkerReply_OpenInStateAck({required this.state}): super._();


 final  OpenInState state;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_OpenInStateAckCopyWith<WorkerReply_OpenInStateAck> get copyWith => _$WorkerReply_OpenInStateAckCopyWithImpl<WorkerReply_OpenInStateAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_OpenInStateAck&&(identical(other.state, state) || other.state == state));
}


@override
int get hashCode => Object.hash(runtimeType,state);

@override
String toString() {
  return 'WorkerReply.openInStateAck(state: $state)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_OpenInStateAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_OpenInStateAckCopyWith(WorkerReply_OpenInStateAck value, $Res Function(WorkerReply_OpenInStateAck) _then) = _$WorkerReply_OpenInStateAckCopyWithImpl;
@useResult
$Res call({
 OpenInState state
});




}
/// @nodoc
class _$WorkerReply_OpenInStateAckCopyWithImpl<$Res>
    implements $WorkerReply_OpenInStateAckCopyWith<$Res> {
  _$WorkerReply_OpenInStateAckCopyWithImpl(this._self, this._then);

  final WorkerReply_OpenInStateAck _self;
  final $Res Function(WorkerReply_OpenInStateAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? state = null,}) {
  return _then(WorkerReply_OpenInStateAck(
state: null == state ? _self.state : state // ignore: cast_nullable_to_non_nullable
as OpenInState,
  ));
}


}

/// @nodoc


class WorkerReply_ProjectActionsAck extends WorkerReply {
  const WorkerReply_ProjectActionsAck({required final  List<ProjectActionDto> actions}): _actions = actions,super._();


 final  List<ProjectActionDto> _actions;
 List<ProjectActionDto> get actions {
  if (_actions is EqualUnmodifiableListView) return _actions;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_actions);
}


/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ProjectActionsAckCopyWith<WorkerReply_ProjectActionsAck> get copyWith => _$WorkerReply_ProjectActionsAckCopyWithImpl<WorkerReply_ProjectActionsAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ProjectActionsAck&&const DeepCollectionEquality().equals(other._actions, _actions));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_actions));

@override
String toString() {
  return 'WorkerReply.projectActionsAck(actions: $actions)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ProjectActionsAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ProjectActionsAckCopyWith(WorkerReply_ProjectActionsAck value, $Res Function(WorkerReply_ProjectActionsAck) _then) = _$WorkerReply_ProjectActionsAckCopyWithImpl;
@useResult
$Res call({
 List<ProjectActionDto> actions
});




}
/// @nodoc
class _$WorkerReply_ProjectActionsAckCopyWithImpl<$Res>
    implements $WorkerReply_ProjectActionsAckCopyWith<$Res> {
  _$WorkerReply_ProjectActionsAckCopyWithImpl(this._self, this._then);

  final WorkerReply_ProjectActionsAck _self;
  final $Res Function(WorkerReply_ProjectActionsAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? actions = null,}) {
  return _then(WorkerReply_ProjectActionsAck(
actions: null == actions ? _self._actions : actions // ignore: cast_nullable_to_non_nullable
as List<ProjectActionDto>,
  ));
}


}

/// @nodoc


class WorkerReply_EnabledAgentsAck extends WorkerReply {
  const WorkerReply_EnabledAgentsAck({required this.view}): super._();


 final  EnabledAgentsView view;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_EnabledAgentsAckCopyWith<WorkerReply_EnabledAgentsAck> get copyWith => _$WorkerReply_EnabledAgentsAckCopyWithImpl<WorkerReply_EnabledAgentsAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_EnabledAgentsAck&&(identical(other.view, view) || other.view == view));
}


@override
int get hashCode => Object.hash(runtimeType,view);

@override
String toString() {
  return 'WorkerReply.enabledAgentsAck(view: $view)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_EnabledAgentsAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_EnabledAgentsAckCopyWith(WorkerReply_EnabledAgentsAck value, $Res Function(WorkerReply_EnabledAgentsAck) _then) = _$WorkerReply_EnabledAgentsAckCopyWithImpl;
@useResult
$Res call({
 EnabledAgentsView view
});




}
/// @nodoc
class _$WorkerReply_EnabledAgentsAckCopyWithImpl<$Res>
    implements $WorkerReply_EnabledAgentsAckCopyWith<$Res> {
  _$WorkerReply_EnabledAgentsAckCopyWithImpl(this._self, this._then);

  final WorkerReply_EnabledAgentsAck _self;
  final $Res Function(WorkerReply_EnabledAgentsAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? view = null,}) {
  return _then(WorkerReply_EnabledAgentsAck(
view: null == view ? _self.view : view // ignore: cast_nullable_to_non_nullable
as EnabledAgentsView,
  ));
}


}

/// @nodoc


class WorkerReply_SubmitNewTaskAck extends WorkerReply {
  const WorkerReply_SubmitNewTaskAck({required this.sectionId}): super._();


 final  String sectionId;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_SubmitNewTaskAckCopyWith<WorkerReply_SubmitNewTaskAck> get copyWith => _$WorkerReply_SubmitNewTaskAckCopyWithImpl<WorkerReply_SubmitNewTaskAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_SubmitNewTaskAck&&(identical(other.sectionId, sectionId) || other.sectionId == sectionId));
}


@override
int get hashCode => Object.hash(runtimeType,sectionId);

@override
String toString() {
  return 'WorkerReply.submitNewTaskAck(sectionId: $sectionId)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_SubmitNewTaskAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_SubmitNewTaskAckCopyWith(WorkerReply_SubmitNewTaskAck value, $Res Function(WorkerReply_SubmitNewTaskAck) _then) = _$WorkerReply_SubmitNewTaskAckCopyWithImpl;
@useResult
$Res call({
 String sectionId
});




}
/// @nodoc
class _$WorkerReply_SubmitNewTaskAckCopyWithImpl<$Res>
    implements $WorkerReply_SubmitNewTaskAckCopyWith<$Res> {
  _$WorkerReply_SubmitNewTaskAckCopyWithImpl(this._self, this._then);

  final WorkerReply_SubmitNewTaskAck _self;
  final $Res Function(WorkerReply_SubmitNewTaskAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? sectionId = null,}) {
  return _then(WorkerReply_SubmitNewTaskAck(
sectionId: null == sectionId ? _self.sectionId : sectionId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class WorkerReply_AddAgentToSectionAck extends WorkerReply {
  const WorkerReply_AddAgentToSectionAck({required this.tabId}): super._();


 final  String tabId;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_AddAgentToSectionAckCopyWith<WorkerReply_AddAgentToSectionAck> get copyWith => _$WorkerReply_AddAgentToSectionAckCopyWithImpl<WorkerReply_AddAgentToSectionAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_AddAgentToSectionAck&&(identical(other.tabId, tabId) || other.tabId == tabId));
}


@override
int get hashCode => Object.hash(runtimeType,tabId);

@override
String toString() {
  return 'WorkerReply.addAgentToSectionAck(tabId: $tabId)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_AddAgentToSectionAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_AddAgentToSectionAckCopyWith(WorkerReply_AddAgentToSectionAck value, $Res Function(WorkerReply_AddAgentToSectionAck) _then) = _$WorkerReply_AddAgentToSectionAckCopyWithImpl;
@useResult
$Res call({
 String tabId
});




}
/// @nodoc
class _$WorkerReply_AddAgentToSectionAckCopyWithImpl<$Res>
    implements $WorkerReply_AddAgentToSectionAckCopyWith<$Res> {
  _$WorkerReply_AddAgentToSectionAckCopyWithImpl(this._self, this._then);

  final WorkerReply_AddAgentToSectionAck _self;
  final $Res Function(WorkerReply_AddAgentToSectionAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? tabId = null,}) {
  return _then(WorkerReply_AddAgentToSectionAck(
tabId: null == tabId ? _self.tabId : tabId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class WorkerReply_ActivateSectionTabAck extends WorkerReply {
  const WorkerReply_ActivateSectionTabAck(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ActivateSectionTabAck);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'WorkerReply.activateSectionTabAck()';
}


}




/// @nodoc


class WorkerReply_CloseSectionTabAck extends WorkerReply {
  const WorkerReply_CloseSectionTabAck({required this.activeTabId}): super._();


 final  String activeTabId;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_CloseSectionTabAckCopyWith<WorkerReply_CloseSectionTabAck> get copyWith => _$WorkerReply_CloseSectionTabAckCopyWithImpl<WorkerReply_CloseSectionTabAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_CloseSectionTabAck&&(identical(other.activeTabId, activeTabId) || other.activeTabId == activeTabId));
}


@override
int get hashCode => Object.hash(runtimeType,activeTabId);

@override
String toString() {
  return 'WorkerReply.closeSectionTabAck(activeTabId: $activeTabId)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_CloseSectionTabAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_CloseSectionTabAckCopyWith(WorkerReply_CloseSectionTabAck value, $Res Function(WorkerReply_CloseSectionTabAck) _then) = _$WorkerReply_CloseSectionTabAckCopyWithImpl;
@useResult
$Res call({
 String activeTabId
});




}
/// @nodoc
class _$WorkerReply_CloseSectionTabAckCopyWithImpl<$Res>
    implements $WorkerReply_CloseSectionTabAckCopyWith<$Res> {
  _$WorkerReply_CloseSectionTabAckCopyWithImpl(this._self, this._then);

  final WorkerReply_CloseSectionTabAck _self;
  final $Res Function(WorkerReply_CloseSectionTabAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? activeTabId = null,}) {
  return _then(WorkerReply_CloseSectionTabAck(
activeTabId: null == activeTabId ? _self.activeTabId : activeTabId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class WorkerReply_ToggleSectionTabPinnedAck extends WorkerReply {
  const WorkerReply_ToggleSectionTabPinnedAck({required this.pinned}): super._();


 final  bool pinned;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ToggleSectionTabPinnedAckCopyWith<WorkerReply_ToggleSectionTabPinnedAck> get copyWith => _$WorkerReply_ToggleSectionTabPinnedAckCopyWithImpl<WorkerReply_ToggleSectionTabPinnedAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ToggleSectionTabPinnedAck&&(identical(other.pinned, pinned) || other.pinned == pinned));
}


@override
int get hashCode => Object.hash(runtimeType,pinned);

@override
String toString() {
  return 'WorkerReply.toggleSectionTabPinnedAck(pinned: $pinned)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ToggleSectionTabPinnedAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ToggleSectionTabPinnedAckCopyWith(WorkerReply_ToggleSectionTabPinnedAck value, $Res Function(WorkerReply_ToggleSectionTabPinnedAck) _then) = _$WorkerReply_ToggleSectionTabPinnedAckCopyWithImpl;
@useResult
$Res call({
 bool pinned
});




}
/// @nodoc
class _$WorkerReply_ToggleSectionTabPinnedAckCopyWithImpl<$Res>
    implements $WorkerReply_ToggleSectionTabPinnedAckCopyWith<$Res> {
  _$WorkerReply_ToggleSectionTabPinnedAckCopyWithImpl(this._self, this._then);

  final WorkerReply_ToggleSectionTabPinnedAck _self;
  final $Res Function(WorkerReply_ToggleSectionTabPinnedAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? pinned = null,}) {
  return _then(WorkerReply_ToggleSectionTabPinnedAck(
pinned: null == pinned ? _self.pinned : pinned // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class WorkerReply_AgentSettingsAck extends WorkerReply {
  const WorkerReply_AgentSettingsAck({required this.view}): super._();


 final  AgentSettingsView view;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_AgentSettingsAckCopyWith<WorkerReply_AgentSettingsAck> get copyWith => _$WorkerReply_AgentSettingsAckCopyWithImpl<WorkerReply_AgentSettingsAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_AgentSettingsAck&&(identical(other.view, view) || other.view == view));
}


@override
int get hashCode => Object.hash(runtimeType,view);

@override
String toString() {
  return 'WorkerReply.agentSettingsAck(view: $view)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_AgentSettingsAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_AgentSettingsAckCopyWith(WorkerReply_AgentSettingsAck value, $Res Function(WorkerReply_AgentSettingsAck) _then) = _$WorkerReply_AgentSettingsAckCopyWithImpl;
@useResult
$Res call({
 AgentSettingsView view
});




}
/// @nodoc
class _$WorkerReply_AgentSettingsAckCopyWithImpl<$Res>
    implements $WorkerReply_AgentSettingsAckCopyWith<$Res> {
  _$WorkerReply_AgentSettingsAckCopyWithImpl(this._self, this._then);

  final WorkerReply_AgentSettingsAck _self;
  final $Res Function(WorkerReply_AgentSettingsAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? view = null,}) {
  return _then(WorkerReply_AgentSettingsAck(
view: null == view ? _self.view : view // ignore: cast_nullable_to_non_nullable
as AgentSettingsView,
  ));
}


}

/// @nodoc


class WorkerReply_SetAgentEnabledAck extends WorkerReply {
  const WorkerReply_SetAgentEnabledAck({required this.changed}): super._();


 final  bool changed;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_SetAgentEnabledAckCopyWith<WorkerReply_SetAgentEnabledAck> get copyWith => _$WorkerReply_SetAgentEnabledAckCopyWithImpl<WorkerReply_SetAgentEnabledAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_SetAgentEnabledAck&&(identical(other.changed, changed) || other.changed == changed));
}


@override
int get hashCode => Object.hash(runtimeType,changed);

@override
String toString() {
  return 'WorkerReply.setAgentEnabledAck(changed: $changed)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_SetAgentEnabledAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_SetAgentEnabledAckCopyWith(WorkerReply_SetAgentEnabledAck value, $Res Function(WorkerReply_SetAgentEnabledAck) _then) = _$WorkerReply_SetAgentEnabledAckCopyWithImpl;
@useResult
$Res call({
 bool changed
});




}
/// @nodoc
class _$WorkerReply_SetAgentEnabledAckCopyWithImpl<$Res>
    implements $WorkerReply_SetAgentEnabledAckCopyWith<$Res> {
  _$WorkerReply_SetAgentEnabledAckCopyWithImpl(this._self, this._then);

  final WorkerReply_SetAgentEnabledAck _self;
  final $Res Function(WorkerReply_SetAgentEnabledAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changed = null,}) {
  return _then(WorkerReply_SetAgentEnabledAck(
changed: null == changed ? _self.changed : changed // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class WorkerReply_SetDefaultAgentAck extends WorkerReply {
  const WorkerReply_SetDefaultAgentAck({required this.changed}): super._();


 final  bool changed;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_SetDefaultAgentAckCopyWith<WorkerReply_SetDefaultAgentAck> get copyWith => _$WorkerReply_SetDefaultAgentAckCopyWithImpl<WorkerReply_SetDefaultAgentAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_SetDefaultAgentAck&&(identical(other.changed, changed) || other.changed == changed));
}


@override
int get hashCode => Object.hash(runtimeType,changed);

@override
String toString() {
  return 'WorkerReply.setDefaultAgentAck(changed: $changed)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_SetDefaultAgentAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_SetDefaultAgentAckCopyWith(WorkerReply_SetDefaultAgentAck value, $Res Function(WorkerReply_SetDefaultAgentAck) _then) = _$WorkerReply_SetDefaultAgentAckCopyWithImpl;
@useResult
$Res call({
 bool changed
});




}
/// @nodoc
class _$WorkerReply_SetDefaultAgentAckCopyWithImpl<$Res>
    implements $WorkerReply_SetDefaultAgentAckCopyWith<$Res> {
  _$WorkerReply_SetDefaultAgentAckCopyWithImpl(this._self, this._then);

  final WorkerReply_SetDefaultAgentAck _self;
  final $Res Function(WorkerReply_SetDefaultAgentAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changed = null,}) {
  return _then(WorkerReply_SetDefaultAgentAck(
changed: null == changed ? _self.changed : changed // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class WorkerReply_SetAgentLaunchArgsAck extends WorkerReply {
  const WorkerReply_SetAgentLaunchArgsAck({required this.changed}): super._();


 final  bool changed;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_SetAgentLaunchArgsAckCopyWith<WorkerReply_SetAgentLaunchArgsAck> get copyWith => _$WorkerReply_SetAgentLaunchArgsAckCopyWithImpl<WorkerReply_SetAgentLaunchArgsAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_SetAgentLaunchArgsAck&&(identical(other.changed, changed) || other.changed == changed));
}


@override
int get hashCode => Object.hash(runtimeType,changed);

@override
String toString() {
  return 'WorkerReply.setAgentLaunchArgsAck(changed: $changed)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_SetAgentLaunchArgsAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_SetAgentLaunchArgsAckCopyWith(WorkerReply_SetAgentLaunchArgsAck value, $Res Function(WorkerReply_SetAgentLaunchArgsAck) _then) = _$WorkerReply_SetAgentLaunchArgsAckCopyWithImpl;
@useResult
$Res call({
 bool changed
});




}
/// @nodoc
class _$WorkerReply_SetAgentLaunchArgsAckCopyWithImpl<$Res>
    implements $WorkerReply_SetAgentLaunchArgsAckCopyWith<$Res> {
  _$WorkerReply_SetAgentLaunchArgsAckCopyWithImpl(this._self, this._then);

  final WorkerReply_SetAgentLaunchArgsAck _self;
  final $Res Function(WorkerReply_SetAgentLaunchArgsAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changed = null,}) {
  return _then(WorkerReply_SetAgentLaunchArgsAck(
changed: null == changed ? _self.changed : changed // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class WorkerReply_OpenInSettingsAck extends WorkerReply {
  const WorkerReply_OpenInSettingsAck({required this.view}): super._();


 final  OpenInSettingsView view;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_OpenInSettingsAckCopyWith<WorkerReply_OpenInSettingsAck> get copyWith => _$WorkerReply_OpenInSettingsAckCopyWithImpl<WorkerReply_OpenInSettingsAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_OpenInSettingsAck&&(identical(other.view, view) || other.view == view));
}


@override
int get hashCode => Object.hash(runtimeType,view);

@override
String toString() {
  return 'WorkerReply.openInSettingsAck(view: $view)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_OpenInSettingsAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_OpenInSettingsAckCopyWith(WorkerReply_OpenInSettingsAck value, $Res Function(WorkerReply_OpenInSettingsAck) _then) = _$WorkerReply_OpenInSettingsAckCopyWithImpl;
@useResult
$Res call({
 OpenInSettingsView view
});




}
/// @nodoc
class _$WorkerReply_OpenInSettingsAckCopyWithImpl<$Res>
    implements $WorkerReply_OpenInSettingsAckCopyWith<$Res> {
  _$WorkerReply_OpenInSettingsAckCopyWithImpl(this._self, this._then);

  final WorkerReply_OpenInSettingsAck _self;
  final $Res Function(WorkerReply_OpenInSettingsAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? view = null,}) {
  return _then(WorkerReply_OpenInSettingsAck(
view: null == view ? _self.view : view // ignore: cast_nullable_to_non_nullable
as OpenInSettingsView,
  ));
}


}

/// @nodoc


class WorkerReply_SetOpenInAppEnabledAck extends WorkerReply {
  const WorkerReply_SetOpenInAppEnabledAck(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_SetOpenInAppEnabledAck);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'WorkerReply.setOpenInAppEnabledAck()';
}


}




/// @nodoc


class WorkerReply_OpenProjectInAppAck extends WorkerReply {
  const WorkerReply_OpenProjectInAppAck(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_OpenProjectInAppAck);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'WorkerReply.openProjectInAppAck()';
}


}




/// @nodoc


class WorkerReply_RunProjectActionAck extends WorkerReply {
  const WorkerReply_RunProjectActionAck({required this.tabId}): super._();


 final  String tabId;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_RunProjectActionAckCopyWith<WorkerReply_RunProjectActionAck> get copyWith => _$WorkerReply_RunProjectActionAckCopyWithImpl<WorkerReply_RunProjectActionAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_RunProjectActionAck&&(identical(other.tabId, tabId) || other.tabId == tabId));
}


@override
int get hashCode => Object.hash(runtimeType,tabId);

@override
String toString() {
  return 'WorkerReply.runProjectActionAck(tabId: $tabId)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_RunProjectActionAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_RunProjectActionAckCopyWith(WorkerReply_RunProjectActionAck value, $Res Function(WorkerReply_RunProjectActionAck) _then) = _$WorkerReply_RunProjectActionAckCopyWithImpl;
@useResult
$Res call({
 String tabId
});




}
/// @nodoc
class _$WorkerReply_RunProjectActionAckCopyWithImpl<$Res>
    implements $WorkerReply_RunProjectActionAckCopyWith<$Res> {
  _$WorkerReply_RunProjectActionAckCopyWithImpl(this._self, this._then);

  final WorkerReply_RunProjectActionAck _self;
  final $Res Function(WorkerReply_RunProjectActionAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? tabId = null,}) {
  return _then(WorkerReply_RunProjectActionAck(
tabId: null == tabId ? _self.tabId : tabId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class WorkerReply_SaveProjectActionAck extends WorkerReply {
  const WorkerReply_SaveProjectActionAck(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_SaveProjectActionAck);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'WorkerReply.saveProjectActionAck()';
}


}




/// @nodoc


class WorkerReply_DeleteProjectActionAck extends WorkerReply {
  const WorkerReply_DeleteProjectActionAck({required this.deleted}): super._();


 final  bool deleted;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_DeleteProjectActionAckCopyWith<WorkerReply_DeleteProjectActionAck> get copyWith => _$WorkerReply_DeleteProjectActionAckCopyWithImpl<WorkerReply_DeleteProjectActionAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_DeleteProjectActionAck&&(identical(other.deleted, deleted) || other.deleted == deleted));
}


@override
int get hashCode => Object.hash(runtimeType,deleted);

@override
String toString() {
  return 'WorkerReply.deleteProjectActionAck(deleted: $deleted)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_DeleteProjectActionAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_DeleteProjectActionAckCopyWith(WorkerReply_DeleteProjectActionAck value, $Res Function(WorkerReply_DeleteProjectActionAck) _then) = _$WorkerReply_DeleteProjectActionAckCopyWithImpl;
@useResult
$Res call({
 bool deleted
});




}
/// @nodoc
class _$WorkerReply_DeleteProjectActionAckCopyWithImpl<$Res>
    implements $WorkerReply_DeleteProjectActionAckCopyWith<$Res> {
  _$WorkerReply_DeleteProjectActionAckCopyWithImpl(this._self, this._then);

  final WorkerReply_DeleteProjectActionAck _self;
  final $Res Function(WorkerReply_DeleteProjectActionAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? deleted = null,}) {
  return _then(WorkerReply_DeleteProjectActionAck(
deleted: null == deleted ? _self.deleted : deleted // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class WorkerReply_GitActionScriptsAck extends WorkerReply {
  const WorkerReply_GitActionScriptsAck({required this.view}): super._();


 final  GitActionScriptsView view;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_GitActionScriptsAckCopyWith<WorkerReply_GitActionScriptsAck> get copyWith => _$WorkerReply_GitActionScriptsAckCopyWithImpl<WorkerReply_GitActionScriptsAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_GitActionScriptsAck&&(identical(other.view, view) || other.view == view));
}


@override
int get hashCode => Object.hash(runtimeType,view);

@override
String toString() {
  return 'WorkerReply.gitActionScriptsAck(view: $view)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_GitActionScriptsAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_GitActionScriptsAckCopyWith(WorkerReply_GitActionScriptsAck value, $Res Function(WorkerReply_GitActionScriptsAck) _then) = _$WorkerReply_GitActionScriptsAckCopyWithImpl;
@useResult
$Res call({
 GitActionScriptsView view
});




}
/// @nodoc
class _$WorkerReply_GitActionScriptsAckCopyWithImpl<$Res>
    implements $WorkerReply_GitActionScriptsAckCopyWith<$Res> {
  _$WorkerReply_GitActionScriptsAckCopyWithImpl(this._self, this._then);

  final WorkerReply_GitActionScriptsAck _self;
  final $Res Function(WorkerReply_GitActionScriptsAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? view = null,}) {
  return _then(WorkerReply_GitActionScriptsAck(
view: null == view ? _self.view : view // ignore: cast_nullable_to_non_nullable
as GitActionScriptsView,
  ));
}


}

/// @nodoc


class WorkerReply_SetGitCommitScriptAck extends WorkerReply {
  const WorkerReply_SetGitCommitScriptAck({required this.changed}): super._();


 final  bool changed;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_SetGitCommitScriptAckCopyWith<WorkerReply_SetGitCommitScriptAck> get copyWith => _$WorkerReply_SetGitCommitScriptAckCopyWithImpl<WorkerReply_SetGitCommitScriptAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_SetGitCommitScriptAck&&(identical(other.changed, changed) || other.changed == changed));
}


@override
int get hashCode => Object.hash(runtimeType,changed);

@override
String toString() {
  return 'WorkerReply.setGitCommitScriptAck(changed: $changed)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_SetGitCommitScriptAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_SetGitCommitScriptAckCopyWith(WorkerReply_SetGitCommitScriptAck value, $Res Function(WorkerReply_SetGitCommitScriptAck) _then) = _$WorkerReply_SetGitCommitScriptAckCopyWithImpl;
@useResult
$Res call({
 bool changed
});




}
/// @nodoc
class _$WorkerReply_SetGitCommitScriptAckCopyWithImpl<$Res>
    implements $WorkerReply_SetGitCommitScriptAckCopyWith<$Res> {
  _$WorkerReply_SetGitCommitScriptAckCopyWithImpl(this._self, this._then);

  final WorkerReply_SetGitCommitScriptAck _self;
  final $Res Function(WorkerReply_SetGitCommitScriptAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changed = null,}) {
  return _then(WorkerReply_SetGitCommitScriptAck(
changed: null == changed ? _self.changed : changed // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class WorkerReply_ResetGitCommitScriptAck extends WorkerReply {
  const WorkerReply_ResetGitCommitScriptAck({required this.changed}): super._();


 final  bool changed;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ResetGitCommitScriptAckCopyWith<WorkerReply_ResetGitCommitScriptAck> get copyWith => _$WorkerReply_ResetGitCommitScriptAckCopyWithImpl<WorkerReply_ResetGitCommitScriptAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ResetGitCommitScriptAck&&(identical(other.changed, changed) || other.changed == changed));
}


@override
int get hashCode => Object.hash(runtimeType,changed);

@override
String toString() {
  return 'WorkerReply.resetGitCommitScriptAck(changed: $changed)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ResetGitCommitScriptAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ResetGitCommitScriptAckCopyWith(WorkerReply_ResetGitCommitScriptAck value, $Res Function(WorkerReply_ResetGitCommitScriptAck) _then) = _$WorkerReply_ResetGitCommitScriptAckCopyWithImpl;
@useResult
$Res call({
 bool changed
});




}
/// @nodoc
class _$WorkerReply_ResetGitCommitScriptAckCopyWithImpl<$Res>
    implements $WorkerReply_ResetGitCommitScriptAckCopyWith<$Res> {
  _$WorkerReply_ResetGitCommitScriptAckCopyWithImpl(this._self, this._then);

  final WorkerReply_ResetGitCommitScriptAck _self;
  final $Res Function(WorkerReply_ResetGitCommitScriptAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changed = null,}) {
  return _then(WorkerReply_ResetGitCommitScriptAck(
changed: null == changed ? _self.changed : changed // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class WorkerReply_SetGitPrScriptAck extends WorkerReply {
  const WorkerReply_SetGitPrScriptAck({required this.changed}): super._();


 final  bool changed;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_SetGitPrScriptAckCopyWith<WorkerReply_SetGitPrScriptAck> get copyWith => _$WorkerReply_SetGitPrScriptAckCopyWithImpl<WorkerReply_SetGitPrScriptAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_SetGitPrScriptAck&&(identical(other.changed, changed) || other.changed == changed));
}


@override
int get hashCode => Object.hash(runtimeType,changed);

@override
String toString() {
  return 'WorkerReply.setGitPrScriptAck(changed: $changed)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_SetGitPrScriptAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_SetGitPrScriptAckCopyWith(WorkerReply_SetGitPrScriptAck value, $Res Function(WorkerReply_SetGitPrScriptAck) _then) = _$WorkerReply_SetGitPrScriptAckCopyWithImpl;
@useResult
$Res call({
 bool changed
});




}
/// @nodoc
class _$WorkerReply_SetGitPrScriptAckCopyWithImpl<$Res>
    implements $WorkerReply_SetGitPrScriptAckCopyWith<$Res> {
  _$WorkerReply_SetGitPrScriptAckCopyWithImpl(this._self, this._then);

  final WorkerReply_SetGitPrScriptAck _self;
  final $Res Function(WorkerReply_SetGitPrScriptAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changed = null,}) {
  return _then(WorkerReply_SetGitPrScriptAck(
changed: null == changed ? _self.changed : changed // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class WorkerReply_ResetGitPrScriptAck extends WorkerReply {
  const WorkerReply_ResetGitPrScriptAck({required this.changed}): super._();


 final  bool changed;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ResetGitPrScriptAckCopyWith<WorkerReply_ResetGitPrScriptAck> get copyWith => _$WorkerReply_ResetGitPrScriptAckCopyWithImpl<WorkerReply_ResetGitPrScriptAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ResetGitPrScriptAck&&(identical(other.changed, changed) || other.changed == changed));
}


@override
int get hashCode => Object.hash(runtimeType,changed);

@override
String toString() {
  return 'WorkerReply.resetGitPrScriptAck(changed: $changed)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ResetGitPrScriptAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ResetGitPrScriptAckCopyWith(WorkerReply_ResetGitPrScriptAck value, $Res Function(WorkerReply_ResetGitPrScriptAck) _then) = _$WorkerReply_ResetGitPrScriptAckCopyWithImpl;
@useResult
$Res call({
 bool changed
});




}
/// @nodoc
class _$WorkerReply_ResetGitPrScriptAckCopyWithImpl<$Res>
    implements $WorkerReply_ResetGitPrScriptAckCopyWith<$Res> {
  _$WorkerReply_ResetGitPrScriptAckCopyWithImpl(this._self, this._then);

  final WorkerReply_ResetGitPrScriptAck _self;
  final $Res Function(WorkerReply_ResetGitPrScriptAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? changed = null,}) {
  return _then(WorkerReply_ResetGitPrScriptAck(
changed: null == changed ? _self.changed : changed // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class WorkerReply_ShortcutSettingsAck extends WorkerReply {
  const WorkerReply_ShortcutSettingsAck({required this.view}): super._();


 final  ShortcutSettingsView view;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_ShortcutSettingsAckCopyWith<WorkerReply_ShortcutSettingsAck> get copyWith => _$WorkerReply_ShortcutSettingsAckCopyWithImpl<WorkerReply_ShortcutSettingsAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ShortcutSettingsAck&&(identical(other.view, view) || other.view == view));
}


@override
int get hashCode => Object.hash(runtimeType,view);

@override
String toString() {
  return 'WorkerReply.shortcutSettingsAck(view: $view)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_ShortcutSettingsAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_ShortcutSettingsAckCopyWith(WorkerReply_ShortcutSettingsAck value, $Res Function(WorkerReply_ShortcutSettingsAck) _then) = _$WorkerReply_ShortcutSettingsAckCopyWithImpl;
@useResult
$Res call({
 ShortcutSettingsView view
});




}
/// @nodoc
class _$WorkerReply_ShortcutSettingsAckCopyWithImpl<$Res>
    implements $WorkerReply_ShortcutSettingsAckCopyWith<$Res> {
  _$WorkerReply_ShortcutSettingsAckCopyWithImpl(this._self, this._then);

  final WorkerReply_ShortcutSettingsAck _self;
  final $Res Function(WorkerReply_ShortcutSettingsAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? view = null,}) {
  return _then(WorkerReply_ShortcutSettingsAck(
view: null == view ? _self.view : view // ignore: cast_nullable_to_non_nullable
as ShortcutSettingsView,
  ));
}


}

/// @nodoc


class WorkerReply_SetShortcutBindingAck extends WorkerReply {
  const WorkerReply_SetShortcutBindingAck(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_SetShortcutBindingAck);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'WorkerReply.setShortcutBindingAck()';
}


}




/// @nodoc


class WorkerReply_ResetShortcutBindingAck extends WorkerReply {
  const WorkerReply_ResetShortcutBindingAck(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_ResetShortcutBindingAck);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'WorkerReply.resetShortcutBindingAck()';
}


}




/// @nodoc


class WorkerReply_McpSettingsAck extends WorkerReply {
  const WorkerReply_McpSettingsAck({required this.view}): super._();


 final  McpSettingsView view;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_McpSettingsAckCopyWith<WorkerReply_McpSettingsAck> get copyWith => _$WorkerReply_McpSettingsAckCopyWithImpl<WorkerReply_McpSettingsAck>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_McpSettingsAck&&(identical(other.view, view) || other.view == view));
}


@override
int get hashCode => Object.hash(runtimeType,view);

@override
String toString() {
  return 'WorkerReply.mcpSettingsAck(view: $view)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_McpSettingsAckCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_McpSettingsAckCopyWith(WorkerReply_McpSettingsAck value, $Res Function(WorkerReply_McpSettingsAck) _then) = _$WorkerReply_McpSettingsAckCopyWithImpl;
@useResult
$Res call({
 McpSettingsView view
});




}
/// @nodoc
class _$WorkerReply_McpSettingsAckCopyWithImpl<$Res>
    implements $WorkerReply_McpSettingsAckCopyWith<$Res> {
  _$WorkerReply_McpSettingsAckCopyWithImpl(this._self, this._then);

  final WorkerReply_McpSettingsAck _self;
  final $Res Function(WorkerReply_McpSettingsAck) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? view = null,}) {
  return _then(WorkerReply_McpSettingsAck(
view: null == view ? _self.view : view // ignore: cast_nullable_to_non_nullable
as McpSettingsView,
  ));
}


}

/// @nodoc


class WorkerReply_McpAddFromCatalogAck extends WorkerReply {
  const WorkerReply_McpAddFromCatalogAck(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_McpAddFromCatalogAck);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'WorkerReply.mcpAddFromCatalogAck()';
}


}




/// @nodoc


class WorkerReply_McpToggleAck extends WorkerReply {
  const WorkerReply_McpToggleAck(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_McpToggleAck);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'WorkerReply.mcpToggleAck()';
}


}




/// @nodoc


class WorkerReply_McpRemoveAck extends WorkerReply {
  const WorkerReply_McpRemoveAck(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_McpRemoveAck);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'WorkerReply.mcpRemoveAck()';
}


}




// dart format on
