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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( WorkerReply_ProjectList value)?  projectList,TResult Function( WorkerReply_Err value)?  err,TResult Function( WorkerReply_SlugifyBranchNameAck value)?  slugifyBranchNameAck,TResult Function( WorkerReply_ProjectBranchesAck value)?  projectBranchesAck,TResult Function( WorkerReply_PrimaryBranchAck value)?  primaryBranchAck,TResult Function( WorkerReply_RepoDefaultCommitActionAck value)?  repoDefaultCommitActionAck,TResult Function( WorkerReply_ActiveGitStateAck value)?  activeGitStateAck,TResult Function( WorkerReply_ChangedFilesAck value)?  changedFilesAck,TResult Function( WorkerReply_ProjectGithubUrlAck value)?  projectGithubUrlAck,TResult Function( WorkerReply_RecentCommitsAck value)?  recentCommitsAck,TResult Function( WorkerReply_CommitFileChangesAck value)?  commitFileChangesAck,TResult Function( WorkerReply_BranchCompareAck value)?  branchCompareAck,required TResult orElse(),}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that);case WorkerReply_Err() when err != null:
return err(_that);case WorkerReply_SlugifyBranchNameAck() when slugifyBranchNameAck != null:
return slugifyBranchNameAck(_that);case WorkerReply_ProjectBranchesAck() when projectBranchesAck != null:
return projectBranchesAck(_that);case WorkerReply_PrimaryBranchAck() when primaryBranchAck != null:
return primaryBranchAck(_that);case WorkerReply_RepoDefaultCommitActionAck() when repoDefaultCommitActionAck != null:
return repoDefaultCommitActionAck(_that);case WorkerReply_ActiveGitStateAck() when activeGitStateAck != null:
return activeGitStateAck(_that);case WorkerReply_ChangedFilesAck() when changedFilesAck != null:
return changedFilesAck(_that);case WorkerReply_ProjectGithubUrlAck() when projectGithubUrlAck != null:
return projectGithubUrlAck(_that);case WorkerReply_RecentCommitsAck() when recentCommitsAck != null:
return recentCommitsAck(_that);case WorkerReply_CommitFileChangesAck() when commitFileChangesAck != null:
return commitFileChangesAck(_that);case WorkerReply_BranchCompareAck() when branchCompareAck != null:
return branchCompareAck(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( WorkerReply_ProjectList value)  projectList,required TResult Function( WorkerReply_Err value)  err,required TResult Function( WorkerReply_SlugifyBranchNameAck value)  slugifyBranchNameAck,required TResult Function( WorkerReply_ProjectBranchesAck value)  projectBranchesAck,required TResult Function( WorkerReply_PrimaryBranchAck value)  primaryBranchAck,required TResult Function( WorkerReply_RepoDefaultCommitActionAck value)  repoDefaultCommitActionAck,required TResult Function( WorkerReply_ActiveGitStateAck value)  activeGitStateAck,required TResult Function( WorkerReply_ChangedFilesAck value)  changedFilesAck,required TResult Function( WorkerReply_ProjectGithubUrlAck value)  projectGithubUrlAck,required TResult Function( WorkerReply_RecentCommitsAck value)  recentCommitsAck,required TResult Function( WorkerReply_CommitFileChangesAck value)  commitFileChangesAck,required TResult Function( WorkerReply_BranchCompareAck value)  branchCompareAck,}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList():
return projectList(_that);case WorkerReply_Err():
return err(_that);case WorkerReply_SlugifyBranchNameAck():
return slugifyBranchNameAck(_that);case WorkerReply_ProjectBranchesAck():
return projectBranchesAck(_that);case WorkerReply_PrimaryBranchAck():
return primaryBranchAck(_that);case WorkerReply_RepoDefaultCommitActionAck():
return repoDefaultCommitActionAck(_that);case WorkerReply_ActiveGitStateAck():
return activeGitStateAck(_that);case WorkerReply_ChangedFilesAck():
return changedFilesAck(_that);case WorkerReply_ProjectGithubUrlAck():
return projectGithubUrlAck(_that);case WorkerReply_RecentCommitsAck():
return recentCommitsAck(_that);case WorkerReply_CommitFileChangesAck():
return commitFileChangesAck(_that);case WorkerReply_BranchCompareAck():
return branchCompareAck(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( WorkerReply_ProjectList value)?  projectList,TResult? Function( WorkerReply_Err value)?  err,TResult? Function( WorkerReply_SlugifyBranchNameAck value)?  slugifyBranchNameAck,TResult? Function( WorkerReply_ProjectBranchesAck value)?  projectBranchesAck,TResult? Function( WorkerReply_PrimaryBranchAck value)?  primaryBranchAck,TResult? Function( WorkerReply_RepoDefaultCommitActionAck value)?  repoDefaultCommitActionAck,TResult? Function( WorkerReply_ActiveGitStateAck value)?  activeGitStateAck,TResult? Function( WorkerReply_ChangedFilesAck value)?  changedFilesAck,TResult? Function( WorkerReply_ProjectGithubUrlAck value)?  projectGithubUrlAck,TResult? Function( WorkerReply_RecentCommitsAck value)?  recentCommitsAck,TResult? Function( WorkerReply_CommitFileChangesAck value)?  commitFileChangesAck,TResult? Function( WorkerReply_BranchCompareAck value)?  branchCompareAck,}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that);case WorkerReply_Err() when err != null:
return err(_that);case WorkerReply_SlugifyBranchNameAck() when slugifyBranchNameAck != null:
return slugifyBranchNameAck(_that);case WorkerReply_ProjectBranchesAck() when projectBranchesAck != null:
return projectBranchesAck(_that);case WorkerReply_PrimaryBranchAck() when primaryBranchAck != null:
return primaryBranchAck(_that);case WorkerReply_RepoDefaultCommitActionAck() when repoDefaultCommitActionAck != null:
return repoDefaultCommitActionAck(_that);case WorkerReply_ActiveGitStateAck() when activeGitStateAck != null:
return activeGitStateAck(_that);case WorkerReply_ChangedFilesAck() when changedFilesAck != null:
return changedFilesAck(_that);case WorkerReply_ProjectGithubUrlAck() when projectGithubUrlAck != null:
return projectGithubUrlAck(_that);case WorkerReply_RecentCommitsAck() when recentCommitsAck != null:
return recentCommitsAck(_that);case WorkerReply_CommitFileChangesAck() when commitFileChangesAck != null:
return commitFileChangesAck(_that);case WorkerReply_BranchCompareAck() when branchCompareAck != null:
return branchCompareAck(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( List<ProjectSummary> projects)?  projectList,TResult Function( String message,  ErrKind kind)?  err,TResult Function( String slug)?  slugifyBranchNameAck,TResult Function( List<String> branches)?  projectBranchesAck,TResult Function( String? branch)?  primaryBranchAck,TResult Function( String? action)?  repoDefaultCommitActionAck,TResult Function( ActiveGitStateWire? state)?  activeGitStateAck,TResult Function( List<ChangedFileWire>? files)?  changedFilesAck,TResult Function( String? url)?  projectGithubUrlAck,TResult Function( RecentCommitsWire? view)?  recentCommitsAck,TResult Function( List<BranchCompareFileWire>? files)?  commitFileChangesAck,TResult Function( BranchCompareWire? view)?  branchCompareAck,required TResult orElse(),}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case WorkerReply_Err() when err != null:
return err(_that.message,_that.kind);case WorkerReply_SlugifyBranchNameAck() when slugifyBranchNameAck != null:
return slugifyBranchNameAck(_that.slug);case WorkerReply_ProjectBranchesAck() when projectBranchesAck != null:
return projectBranchesAck(_that.branches);case WorkerReply_PrimaryBranchAck() when primaryBranchAck != null:
return primaryBranchAck(_that.branch);case WorkerReply_RepoDefaultCommitActionAck() when repoDefaultCommitActionAck != null:
return repoDefaultCommitActionAck(_that.action);case WorkerReply_ActiveGitStateAck() when activeGitStateAck != null:
return activeGitStateAck(_that.state);case WorkerReply_ChangedFilesAck() when changedFilesAck != null:
return changedFilesAck(_that.files);case WorkerReply_ProjectGithubUrlAck() when projectGithubUrlAck != null:
return projectGithubUrlAck(_that.url);case WorkerReply_RecentCommitsAck() when recentCommitsAck != null:
return recentCommitsAck(_that.view);case WorkerReply_CommitFileChangesAck() when commitFileChangesAck != null:
return commitFileChangesAck(_that.files);case WorkerReply_BranchCompareAck() when branchCompareAck != null:
return branchCompareAck(_that.view);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( List<ProjectSummary> projects)  projectList,required TResult Function( String message,  ErrKind kind)  err,required TResult Function( String slug)  slugifyBranchNameAck,required TResult Function( List<String> branches)  projectBranchesAck,required TResult Function( String? branch)  primaryBranchAck,required TResult Function( String? action)  repoDefaultCommitActionAck,required TResult Function( ActiveGitStateWire? state)  activeGitStateAck,required TResult Function( List<ChangedFileWire>? files)  changedFilesAck,required TResult Function( String? url)  projectGithubUrlAck,required TResult Function( RecentCommitsWire? view)  recentCommitsAck,required TResult Function( List<BranchCompareFileWire>? files)  commitFileChangesAck,required TResult Function( BranchCompareWire? view)  branchCompareAck,}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList():
return projectList(_that.projects);case WorkerReply_Err():
return err(_that.message,_that.kind);case WorkerReply_SlugifyBranchNameAck():
return slugifyBranchNameAck(_that.slug);case WorkerReply_ProjectBranchesAck():
return projectBranchesAck(_that.branches);case WorkerReply_PrimaryBranchAck():
return primaryBranchAck(_that.branch);case WorkerReply_RepoDefaultCommitActionAck():
return repoDefaultCommitActionAck(_that.action);case WorkerReply_ActiveGitStateAck():
return activeGitStateAck(_that.state);case WorkerReply_ChangedFilesAck():
return changedFilesAck(_that.files);case WorkerReply_ProjectGithubUrlAck():
return projectGithubUrlAck(_that.url);case WorkerReply_RecentCommitsAck():
return recentCommitsAck(_that.view);case WorkerReply_CommitFileChangesAck():
return commitFileChangesAck(_that.files);case WorkerReply_BranchCompareAck():
return branchCompareAck(_that.view);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( List<ProjectSummary> projects)?  projectList,TResult? Function( String message,  ErrKind kind)?  err,TResult? Function( String slug)?  slugifyBranchNameAck,TResult? Function( List<String> branches)?  projectBranchesAck,TResult? Function( String? branch)?  primaryBranchAck,TResult? Function( String? action)?  repoDefaultCommitActionAck,TResult? Function( ActiveGitStateWire? state)?  activeGitStateAck,TResult? Function( List<ChangedFileWire>? files)?  changedFilesAck,TResult? Function( String? url)?  projectGithubUrlAck,TResult? Function( RecentCommitsWire? view)?  recentCommitsAck,TResult? Function( List<BranchCompareFileWire>? files)?  commitFileChangesAck,TResult? Function( BranchCompareWire? view)?  branchCompareAck,}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case WorkerReply_Err() when err != null:
return err(_that.message,_that.kind);case WorkerReply_SlugifyBranchNameAck() when slugifyBranchNameAck != null:
return slugifyBranchNameAck(_that.slug);case WorkerReply_ProjectBranchesAck() when projectBranchesAck != null:
return projectBranchesAck(_that.branches);case WorkerReply_PrimaryBranchAck() when primaryBranchAck != null:
return primaryBranchAck(_that.branch);case WorkerReply_RepoDefaultCommitActionAck() when repoDefaultCommitActionAck != null:
return repoDefaultCommitActionAck(_that.action);case WorkerReply_ActiveGitStateAck() when activeGitStateAck != null:
return activeGitStateAck(_that.state);case WorkerReply_ChangedFilesAck() when changedFilesAck != null:
return changedFilesAck(_that.files);case WorkerReply_ProjectGithubUrlAck() when projectGithubUrlAck != null:
return projectGithubUrlAck(_that.url);case WorkerReply_RecentCommitsAck() when recentCommitsAck != null:
return recentCommitsAck(_that.view);case WorkerReply_CommitFileChangesAck() when commitFileChangesAck != null:
return commitFileChangesAck(_that.files);case WorkerReply_BranchCompareAck() when branchCompareAck != null:
return branchCompareAck(_that.view);case _:
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
  

 final  ActiveGitStateWire? state;

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
 ActiveGitStateWire? state
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
as ActiveGitStateWire?,
  ));
}


}

/// @nodoc


class WorkerReply_ChangedFilesAck extends WorkerReply {
  const WorkerReply_ChangedFilesAck({final  List<ChangedFileWire>? files}): _files = files,super._();
  

 final  List<ChangedFileWire>? _files;
 List<ChangedFileWire>? get files {
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
 List<ChangedFileWire>? files
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
as List<ChangedFileWire>?,
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
  

 final  RecentCommitsWire? view;

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
 RecentCommitsWire? view
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
as RecentCommitsWire?,
  ));
}


}

/// @nodoc


class WorkerReply_CommitFileChangesAck extends WorkerReply {
  const WorkerReply_CommitFileChangesAck({final  List<BranchCompareFileWire>? files}): _files = files,super._();
  

 final  List<BranchCompareFileWire>? _files;
 List<BranchCompareFileWire>? get files {
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
 List<BranchCompareFileWire>? files
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
as List<BranchCompareFileWire>?,
  ));
}


}

/// @nodoc


class WorkerReply_BranchCompareAck extends WorkerReply {
  const WorkerReply_BranchCompareAck({this.view}): super._();
  

 final  BranchCompareWire? view;

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
 BranchCompareWire? view
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
as BranchCompareWire?,
  ));
}


}

// dart format on
