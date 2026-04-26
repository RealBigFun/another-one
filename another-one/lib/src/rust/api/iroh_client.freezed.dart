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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( WorkerReply_ProjectList value)?  projectList,TResult Function( WorkerReply_ProjectAdded value)?  projectAdded,TResult Function( WorkerReply_ProjectRemoved value)?  projectRemoved,TResult Function( WorkerReply_Err value)?  err,TResult Function( WorkerReply_TaskCreated value)?  taskCreated,TResult Function( WorkerReply_TaskRenamed value)?  taskRenamed,TResult Function( WorkerReply_TaskPinned value)?  taskPinned,TResult Function( WorkerReply_TaskRemoved value)?  taskRemoved,required TResult orElse(),}){
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
return taskRemoved(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( WorkerReply_ProjectList value)  projectList,required TResult Function( WorkerReply_ProjectAdded value)  projectAdded,required TResult Function( WorkerReply_ProjectRemoved value)  projectRemoved,required TResult Function( WorkerReply_Err value)  err,required TResult Function( WorkerReply_TaskCreated value)  taskCreated,required TResult Function( WorkerReply_TaskRenamed value)  taskRenamed,required TResult Function( WorkerReply_TaskPinned value)  taskPinned,required TResult Function( WorkerReply_TaskRemoved value)  taskRemoved,}){
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
return taskRemoved(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( WorkerReply_ProjectList value)?  projectList,TResult? Function( WorkerReply_ProjectAdded value)?  projectAdded,TResult? Function( WorkerReply_ProjectRemoved value)?  projectRemoved,TResult? Function( WorkerReply_Err value)?  err,TResult? Function( WorkerReply_TaskCreated value)?  taskCreated,TResult? Function( WorkerReply_TaskRenamed value)?  taskRenamed,TResult? Function( WorkerReply_TaskPinned value)?  taskPinned,TResult? Function( WorkerReply_TaskRemoved value)?  taskRemoved,}){
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
return taskRemoved(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( List<ProjectSummary> projects)?  projectList,TResult Function( ProjectSummary project)?  projectAdded,TResult Function( String projectId)?  projectRemoved,TResult Function( String message,  ErrKind kind)?  err,TResult Function( String projectId,  TaskSummary task)?  taskCreated,TResult Function( bool changed,  TaskSummary? task)?  taskRenamed,TResult Function( bool changed,  TaskSummary? task)?  taskPinned,TResult Function( String projectId,  String taskId,  bool removed)?  taskRemoved,required TResult orElse(),}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case WorkerReply_ProjectAdded() when projectAdded != null:
return projectAdded(_that.project);case WorkerReply_ProjectRemoved() when projectRemoved != null:
return projectRemoved(_that.projectId);case WorkerReply_Err() when err != null:
return err(_that.message,_that.kind);case WorkerReply_TaskCreated() when taskCreated != null:
return taskCreated(_that.projectId,_that.task);case WorkerReply_TaskRenamed() when taskRenamed != null:
return taskRenamed(_that.changed,_that.task);case WorkerReply_TaskPinned() when taskPinned != null:
return taskPinned(_that.changed,_that.task);case WorkerReply_TaskRemoved() when taskRemoved != null:
return taskRemoved(_that.projectId,_that.taskId,_that.removed);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( List<ProjectSummary> projects)  projectList,required TResult Function( ProjectSummary project)  projectAdded,required TResult Function( String projectId)  projectRemoved,required TResult Function( String message,  ErrKind kind)  err,required TResult Function( String projectId,  TaskSummary task)  taskCreated,required TResult Function( bool changed,  TaskSummary? task)  taskRenamed,required TResult Function( bool changed,  TaskSummary? task)  taskPinned,required TResult Function( String projectId,  String taskId,  bool removed)  taskRemoved,}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList():
return projectList(_that.projects);case WorkerReply_ProjectAdded():
return projectAdded(_that.project);case WorkerReply_ProjectRemoved():
return projectRemoved(_that.projectId);case WorkerReply_Err():
return err(_that.message,_that.kind);case WorkerReply_TaskCreated():
return taskCreated(_that.projectId,_that.task);case WorkerReply_TaskRenamed():
return taskRenamed(_that.changed,_that.task);case WorkerReply_TaskPinned():
return taskPinned(_that.changed,_that.task);case WorkerReply_TaskRemoved():
return taskRemoved(_that.projectId,_that.taskId,_that.removed);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( List<ProjectSummary> projects)?  projectList,TResult? Function( ProjectSummary project)?  projectAdded,TResult? Function( String projectId)?  projectRemoved,TResult? Function( String message,  ErrKind kind)?  err,TResult? Function( String projectId,  TaskSummary task)?  taskCreated,TResult? Function( bool changed,  TaskSummary? task)?  taskRenamed,TResult? Function( bool changed,  TaskSummary? task)?  taskPinned,TResult? Function( String projectId,  String taskId,  bool removed)?  taskRemoved,}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case WorkerReply_ProjectAdded() when projectAdded != null:
return projectAdded(_that.project);case WorkerReply_ProjectRemoved() when projectRemoved != null:
return projectRemoved(_that.projectId);case WorkerReply_Err() when err != null:
return err(_that.message,_that.kind);case WorkerReply_TaskCreated() when taskCreated != null:
return taskCreated(_that.projectId,_that.task);case WorkerReply_TaskRenamed() when taskRenamed != null:
return taskRenamed(_that.changed,_that.task);case WorkerReply_TaskPinned() when taskPinned != null:
return taskPinned(_that.changed,_that.task);case WorkerReply_TaskRemoved() when taskRemoved != null:
return taskRemoved(_that.projectId,_that.taskId,_that.removed);case _:
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

// dart format on
