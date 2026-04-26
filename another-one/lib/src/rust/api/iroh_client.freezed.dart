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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( WorkerReply_ProjectList value)?  projectList,TResult Function( WorkerReply_Err value)?  err,TResult Function( WorkerReply_GitActionScriptsAck value)?  gitActionScriptsAck,TResult Function( WorkerReply_SetGitCommitScriptAck value)?  setGitCommitScriptAck,TResult Function( WorkerReply_ResetGitCommitScriptAck value)?  resetGitCommitScriptAck,TResult Function( WorkerReply_SetGitPrScriptAck value)?  setGitPrScriptAck,TResult Function( WorkerReply_ResetGitPrScriptAck value)?  resetGitPrScriptAck,TResult Function( WorkerReply_ShortcutSettingsAck value)?  shortcutSettingsAck,TResult Function( WorkerReply_SetShortcutBindingAck value)?  setShortcutBindingAck,TResult Function( WorkerReply_ResetShortcutBindingAck value)?  resetShortcutBindingAck,TResult Function( WorkerReply_McpSettingsAck value)?  mcpSettingsAck,TResult Function( WorkerReply_McpAddFromCatalogAck value)?  mcpAddFromCatalogAck,TResult Function( WorkerReply_McpToggleAck value)?  mcpToggleAck,TResult Function( WorkerReply_McpRemoveAck value)?  mcpRemoveAck,required TResult orElse(),}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that);case WorkerReply_Err() when err != null:
return err(_that);case WorkerReply_GitActionScriptsAck() when gitActionScriptsAck != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( WorkerReply_ProjectList value)  projectList,required TResult Function( WorkerReply_Err value)  err,required TResult Function( WorkerReply_GitActionScriptsAck value)  gitActionScriptsAck,required TResult Function( WorkerReply_SetGitCommitScriptAck value)  setGitCommitScriptAck,required TResult Function( WorkerReply_ResetGitCommitScriptAck value)  resetGitCommitScriptAck,required TResult Function( WorkerReply_SetGitPrScriptAck value)  setGitPrScriptAck,required TResult Function( WorkerReply_ResetGitPrScriptAck value)  resetGitPrScriptAck,required TResult Function( WorkerReply_ShortcutSettingsAck value)  shortcutSettingsAck,required TResult Function( WorkerReply_SetShortcutBindingAck value)  setShortcutBindingAck,required TResult Function( WorkerReply_ResetShortcutBindingAck value)  resetShortcutBindingAck,required TResult Function( WorkerReply_McpSettingsAck value)  mcpSettingsAck,required TResult Function( WorkerReply_McpAddFromCatalogAck value)  mcpAddFromCatalogAck,required TResult Function( WorkerReply_McpToggleAck value)  mcpToggleAck,required TResult Function( WorkerReply_McpRemoveAck value)  mcpRemoveAck,}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList():
return projectList(_that);case WorkerReply_Err():
return err(_that);case WorkerReply_GitActionScriptsAck():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( WorkerReply_ProjectList value)?  projectList,TResult? Function( WorkerReply_Err value)?  err,TResult? Function( WorkerReply_GitActionScriptsAck value)?  gitActionScriptsAck,TResult? Function( WorkerReply_SetGitCommitScriptAck value)?  setGitCommitScriptAck,TResult? Function( WorkerReply_ResetGitCommitScriptAck value)?  resetGitCommitScriptAck,TResult? Function( WorkerReply_SetGitPrScriptAck value)?  setGitPrScriptAck,TResult? Function( WorkerReply_ResetGitPrScriptAck value)?  resetGitPrScriptAck,TResult? Function( WorkerReply_ShortcutSettingsAck value)?  shortcutSettingsAck,TResult? Function( WorkerReply_SetShortcutBindingAck value)?  setShortcutBindingAck,TResult? Function( WorkerReply_ResetShortcutBindingAck value)?  resetShortcutBindingAck,TResult? Function( WorkerReply_McpSettingsAck value)?  mcpSettingsAck,TResult? Function( WorkerReply_McpAddFromCatalogAck value)?  mcpAddFromCatalogAck,TResult? Function( WorkerReply_McpToggleAck value)?  mcpToggleAck,TResult? Function( WorkerReply_McpRemoveAck value)?  mcpRemoveAck,}){
final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that);case WorkerReply_Err() when err != null:
return err(_that);case WorkerReply_GitActionScriptsAck() when gitActionScriptsAck != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( List<ProjectSummary> projects)?  projectList,TResult Function( String message,  ErrKind kind)?  err,TResult Function( GitActionScriptsView view)?  gitActionScriptsAck,TResult Function( bool changed)?  setGitCommitScriptAck,TResult Function( bool changed)?  resetGitCommitScriptAck,TResult Function( bool changed)?  setGitPrScriptAck,TResult Function( bool changed)?  resetGitPrScriptAck,TResult Function( ShortcutSettingsView view)?  shortcutSettingsAck,TResult Function()?  setShortcutBindingAck,TResult Function()?  resetShortcutBindingAck,TResult Function( McpSettingsView view)?  mcpSettingsAck,TResult Function()?  mcpAddFromCatalogAck,TResult Function()?  mcpToggleAck,TResult Function()?  mcpRemoveAck,required TResult orElse(),}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case WorkerReply_Err() when err != null:
return err(_that.message,_that.kind);case WorkerReply_GitActionScriptsAck() when gitActionScriptsAck != null:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( List<ProjectSummary> projects)  projectList,required TResult Function( String message,  ErrKind kind)  err,required TResult Function( GitActionScriptsView view)  gitActionScriptsAck,required TResult Function( bool changed)  setGitCommitScriptAck,required TResult Function( bool changed)  resetGitCommitScriptAck,required TResult Function( bool changed)  setGitPrScriptAck,required TResult Function( bool changed)  resetGitPrScriptAck,required TResult Function( ShortcutSettingsView view)  shortcutSettingsAck,required TResult Function()  setShortcutBindingAck,required TResult Function()  resetShortcutBindingAck,required TResult Function( McpSettingsView view)  mcpSettingsAck,required TResult Function()  mcpAddFromCatalogAck,required TResult Function()  mcpToggleAck,required TResult Function()  mcpRemoveAck,}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList():
return projectList(_that.projects);case WorkerReply_Err():
return err(_that.message,_that.kind);case WorkerReply_GitActionScriptsAck():
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( List<ProjectSummary> projects)?  projectList,TResult? Function( String message,  ErrKind kind)?  err,TResult? Function( GitActionScriptsView view)?  gitActionScriptsAck,TResult? Function( bool changed)?  setGitCommitScriptAck,TResult? Function( bool changed)?  resetGitCommitScriptAck,TResult? Function( bool changed)?  setGitPrScriptAck,TResult? Function( bool changed)?  resetGitPrScriptAck,TResult? Function( ShortcutSettingsView view)?  shortcutSettingsAck,TResult? Function()?  setShortcutBindingAck,TResult? Function()?  resetShortcutBindingAck,TResult? Function( McpSettingsView view)?  mcpSettingsAck,TResult? Function()?  mcpAddFromCatalogAck,TResult? Function()?  mcpToggleAck,TResult? Function()?  mcpRemoveAck,}) {final _that = this;
switch (_that) {
case WorkerReply_ProjectList() when projectList != null:
return projectList(_that.projects);case WorkerReply_Err() when err != null:
return err(_that.message,_that.kind);case WorkerReply_GitActionScriptsAck() when gitActionScriptsAck != null:
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
