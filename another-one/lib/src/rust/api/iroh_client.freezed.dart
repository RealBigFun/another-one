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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( WorkerReply_ProjectList value)?  projectList,TResult Function( WorkerReply_ProjectAdded value)?  projectAdded,TResult Function( WorkerReply_ProjectRemoved value)?  projectRemoved,TResult Function( WorkerReply_Err value)?  err,required TResult orElse(),}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that);case WorkerReply_ProjectAdded() when projectAdded != null:
return projectAdded(_that);case WorkerReply_ProjectRemoved() when projectRemoved != null:
return projectRemoved(_that);case WorkerReply_Err() when err != null:
return err(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( WorkerReply_ProjectList value)  projectList,required TResult Function( WorkerReply_ProjectAdded value)  projectAdded,required TResult Function( WorkerReply_ProjectRemoved value)  projectRemoved,required TResult Function( WorkerReply_Err value)  err,}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList():
return projectList(_that);case WorkerReply_ProjectAdded():
return projectAdded(_that);case WorkerReply_ProjectRemoved():
return projectRemoved(_that);case WorkerReply_Err():
return err(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( WorkerReply_ProjectList value)?  projectList,TResult? Function( WorkerReply_ProjectAdded value)?  projectAdded,TResult? Function( WorkerReply_ProjectRemoved value)?  projectRemoved,TResult? Function( WorkerReply_Err value)?  err,}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that);case WorkerReply_ProjectAdded() when projectAdded != null:
return projectAdded(_that);case WorkerReply_ProjectRemoved() when projectRemoved != null:
return projectRemoved(_that);case WorkerReply_Err() when err != null:
return err(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( List<ProjectSummary> projects)?  projectList,TResult Function( ProjectSummary project)?  projectAdded,TResult Function( String projectId)?  projectRemoved,TResult Function( String message,  ErrKind kind)?  err,required TResult orElse(),}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case WorkerReply_ProjectAdded() when projectAdded != null:
return projectAdded(_that.project);case WorkerReply_ProjectRemoved() when projectRemoved != null:
return projectRemoved(_that.projectId);case WorkerReply_Err() when err != null:
return err(_that.message,_that.kind);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( List<ProjectSummary> projects)  projectList,required TResult Function( ProjectSummary project)  projectAdded,required TResult Function( String projectId)  projectRemoved,required TResult Function( String message,  ErrKind kind)  err,}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList():
return projectList(_that.projects);case WorkerReply_ProjectAdded():
return projectAdded(_that.project);case WorkerReply_ProjectRemoved():
return projectRemoved(_that.projectId);case WorkerReply_Err():
return err(_that.message,_that.kind);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( List<ProjectSummary> projects)?  projectList,TResult? Function( ProjectSummary project)?  projectAdded,TResult? Function( String projectId)?  projectRemoved,TResult? Function( String message,  ErrKind kind)?  err,}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case WorkerReply_ProjectAdded() when projectAdded != null:
return projectAdded(_that.project);case WorkerReply_ProjectRemoved() when projectRemoved != null:
return projectRemoved(_that.projectId);case WorkerReply_Err() when err != null:
return err(_that.message,_that.kind);case _:
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

// dart format on
