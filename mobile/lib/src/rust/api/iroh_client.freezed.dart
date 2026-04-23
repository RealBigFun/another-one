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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( WorkerReply_GitRefresh value)?  gitRefresh,TResult Function( WorkerReply_PullRequestStatus value)?  pullRequestStatus,TResult Function( WorkerReply_ProjectList value)?  projectList,required TResult orElse(),}){
final _that = this;
switch (_that) {
case WorkerReply_GitRefresh() when gitRefresh != null:
return gitRefresh(_that);case WorkerReply_PullRequestStatus() when pullRequestStatus != null:
return pullRequestStatus(_that);case WorkerReply_ProjectList() when projectList != null:
return projectList(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( WorkerReply_GitRefresh value)  gitRefresh,required TResult Function( WorkerReply_PullRequestStatus value)  pullRequestStatus,required TResult Function( WorkerReply_ProjectList value)  projectList,}){
final _that = this;
switch (_that) {
case WorkerReply_GitRefresh():
return gitRefresh(_that);case WorkerReply_PullRequestStatus():
return pullRequestStatus(_that);case WorkerReply_ProjectList():
return projectList(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( WorkerReply_GitRefresh value)?  gitRefresh,TResult? Function( WorkerReply_PullRequestStatus value)?  pullRequestStatus,TResult? Function( WorkerReply_ProjectList value)?  projectList,}){
final _that = this;
switch (_that) {
case WorkerReply_GitRefresh() when gitRefresh != null:
return gitRefresh(_that);case WorkerReply_PullRequestStatus() when pullRequestStatus != null:
return pullRequestStatus(_that);case WorkerReply_ProjectList() when projectList != null:
return projectList(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String projectId,  String? currentBranch,  BigInt changedFileCount,  BigInt ahead,  BigInt behind)?  gitRefresh,TResult Function( String projectId,  String branchName,  PullRequestInfo? pr)?  pullRequestStatus,TResult Function( List<ProjectSummary> projects)?  projectList,required TResult orElse(),}) {final _that = this;
switch (_that) {
case WorkerReply_GitRefresh() when gitRefresh != null:
return gitRefresh(_that.projectId,_that.currentBranch,_that.changedFileCount,_that.ahead,_that.behind);case WorkerReply_PullRequestStatus() when pullRequestStatus != null:
return pullRequestStatus(_that.projectId,_that.branchName,_that.pr);case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String projectId,  String? currentBranch,  BigInt changedFileCount,  BigInt ahead,  BigInt behind)  gitRefresh,required TResult Function( String projectId,  String branchName,  PullRequestInfo? pr)  pullRequestStatus,required TResult Function( List<ProjectSummary> projects)  projectList,}) {final _that = this;
switch (_that) {
case WorkerReply_GitRefresh():
return gitRefresh(_that.projectId,_that.currentBranch,_that.changedFileCount,_that.ahead,_that.behind);case WorkerReply_PullRequestStatus():
return pullRequestStatus(_that.projectId,_that.branchName,_that.pr);case WorkerReply_ProjectList():
return projectList(_that.projects);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String projectId,  String? currentBranch,  BigInt changedFileCount,  BigInt ahead,  BigInt behind)?  gitRefresh,TResult? Function( String projectId,  String branchName,  PullRequestInfo? pr)?  pullRequestStatus,TResult? Function( List<ProjectSummary> projects)?  projectList,}) {final _that = this;
switch (_that) {
case WorkerReply_GitRefresh() when gitRefresh != null:
return gitRefresh(_that.projectId,_that.currentBranch,_that.changedFileCount,_that.ahead,_that.behind);case WorkerReply_PullRequestStatus() when pullRequestStatus != null:
return pullRequestStatus(_that.projectId,_that.branchName,_that.pr);case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case _:
  return null;

}
}

}

/// @nodoc


class WorkerReply_GitRefresh extends WorkerReply {
  const WorkerReply_GitRefresh({required this.projectId, this.currentBranch, required this.changedFileCount, required this.ahead, required this.behind}): super._();
  

 final  String projectId;
 final  String? currentBranch;
 final  BigInt changedFileCount;
 final  BigInt ahead;
 final  BigInt behind;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_GitRefreshCopyWith<WorkerReply_GitRefresh> get copyWith => _$WorkerReply_GitRefreshCopyWithImpl<WorkerReply_GitRefresh>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_GitRefresh&&(identical(other.projectId, projectId) || other.projectId == projectId)&&(identical(other.currentBranch, currentBranch) || other.currentBranch == currentBranch)&&(identical(other.changedFileCount, changedFileCount) || other.changedFileCount == changedFileCount)&&(identical(other.ahead, ahead) || other.ahead == ahead)&&(identical(other.behind, behind) || other.behind == behind));
}


@override
int get hashCode => Object.hash(runtimeType,projectId,currentBranch,changedFileCount,ahead,behind);

@override
String toString() {
  return 'WorkerReply.gitRefresh(projectId: $projectId, currentBranch: $currentBranch, changedFileCount: $changedFileCount, ahead: $ahead, behind: $behind)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_GitRefreshCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_GitRefreshCopyWith(WorkerReply_GitRefresh value, $Res Function(WorkerReply_GitRefresh) _then) = _$WorkerReply_GitRefreshCopyWithImpl;
@useResult
$Res call({
 String projectId, String? currentBranch, BigInt changedFileCount, BigInt ahead, BigInt behind
});




}
/// @nodoc
class _$WorkerReply_GitRefreshCopyWithImpl<$Res>
    implements $WorkerReply_GitRefreshCopyWith<$Res> {
  _$WorkerReply_GitRefreshCopyWithImpl(this._self, this._then);

  final WorkerReply_GitRefresh _self;
  final $Res Function(WorkerReply_GitRefresh) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? projectId = null,Object? currentBranch = freezed,Object? changedFileCount = null,Object? ahead = null,Object? behind = null,}) {
  return _then(WorkerReply_GitRefresh(
projectId: null == projectId ? _self.projectId : projectId // ignore: cast_nullable_to_non_nullable
as String,currentBranch: freezed == currentBranch ? _self.currentBranch : currentBranch // ignore: cast_nullable_to_non_nullable
as String?,changedFileCount: null == changedFileCount ? _self.changedFileCount : changedFileCount // ignore: cast_nullable_to_non_nullable
as BigInt,ahead: null == ahead ? _self.ahead : ahead // ignore: cast_nullable_to_non_nullable
as BigInt,behind: null == behind ? _self.behind : behind // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class WorkerReply_PullRequestStatus extends WorkerReply {
  const WorkerReply_PullRequestStatus({required this.projectId, required this.branchName, this.pr}): super._();
  

 final  String projectId;
 final  String branchName;
 final  PullRequestInfo? pr;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReply_PullRequestStatusCopyWith<WorkerReply_PullRequestStatus> get copyWith => _$WorkerReply_PullRequestStatusCopyWithImpl<WorkerReply_PullRequestStatus>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply_PullRequestStatus&&(identical(other.projectId, projectId) || other.projectId == projectId)&&(identical(other.branchName, branchName) || other.branchName == branchName)&&(identical(other.pr, pr) || other.pr == pr));
}


@override
int get hashCode => Object.hash(runtimeType,projectId,branchName,pr);

@override
String toString() {
  return 'WorkerReply.pullRequestStatus(projectId: $projectId, branchName: $branchName, pr: $pr)';
}


}

/// @nodoc
abstract mixin class $WorkerReply_PullRequestStatusCopyWith<$Res> implements $WorkerReplyCopyWith<$Res> {
  factory $WorkerReply_PullRequestStatusCopyWith(WorkerReply_PullRequestStatus value, $Res Function(WorkerReply_PullRequestStatus) _then) = _$WorkerReply_PullRequestStatusCopyWithImpl;
@useResult
$Res call({
 String projectId, String branchName, PullRequestInfo? pr
});




}
/// @nodoc
class _$WorkerReply_PullRequestStatusCopyWithImpl<$Res>
    implements $WorkerReply_PullRequestStatusCopyWith<$Res> {
  _$WorkerReply_PullRequestStatusCopyWithImpl(this._self, this._then);

  final WorkerReply_PullRequestStatus _self;
  final $Res Function(WorkerReply_PullRequestStatus) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? projectId = null,Object? branchName = null,Object? pr = freezed,}) {
  return _then(WorkerReply_PullRequestStatus(
projectId: null == projectId ? _self.projectId : projectId // ignore: cast_nullable_to_non_nullable
as String,branchName: null == branchName ? _self.branchName : branchName // ignore: cast_nullable_to_non_nullable
as String,pr: freezed == pr ? _self.pr : pr // ignore: cast_nullable_to_non_nullable
as PullRequestInfo?,
  ));
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

// dart format on
