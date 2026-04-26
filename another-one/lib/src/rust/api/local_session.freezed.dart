// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'local_session.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$ProjectActionKindDto {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ProjectActionKindDto);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'ProjectActionKindDto()';
}


}

/// @nodoc
class $ProjectActionKindDtoCopyWith<$Res>  {
$ProjectActionKindDtoCopyWith(ProjectActionKindDto _, $Res Function(ProjectActionKindDto) __);
}


/// Adds pattern-matching-related methods to [ProjectActionKindDto].
extension ProjectActionKindDtoPatterns on ProjectActionKindDto {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( ProjectActionKindDto_Shell value)?  shell,TResult Function( ProjectActionKindDto_Agent value)?  agent,required TResult orElse(),}){
final _that = this;
switch (_that) {
case ProjectActionKindDto_Shell() when shell != null:
return shell(_that);case ProjectActionKindDto_Agent() when agent != null:
return agent(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( ProjectActionKindDto_Shell value)  shell,required TResult Function( ProjectActionKindDto_Agent value)  agent,}){
final _that = this;
switch (_that) {
case ProjectActionKindDto_Shell():
return shell(_that);case ProjectActionKindDto_Agent():
return agent(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( ProjectActionKindDto_Shell value)?  shell,TResult? Function( ProjectActionKindDto_Agent value)?  agent,}){
final _that = this;
switch (_that) {
case ProjectActionKindDto_Shell() when shell != null:
return shell(_that);case ProjectActionKindDto_Agent() when agent != null:
return agent(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String command)?  shell,TResult Function( String prompt,  AgentProvider provider,  String? model,  String? traits,  String? mode,  ProjectActionAccessDto access)?  agent,required TResult orElse(),}) {final _that = this;
switch (_that) {
case ProjectActionKindDto_Shell() when shell != null:
return shell(_that.command);case ProjectActionKindDto_Agent() when agent != null:
return agent(_that.prompt,_that.provider,_that.model,_that.traits,_that.mode,_that.access);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String command)  shell,required TResult Function( String prompt,  AgentProvider provider,  String? model,  String? traits,  String? mode,  ProjectActionAccessDto access)  agent,}) {final _that = this;
switch (_that) {
case ProjectActionKindDto_Shell():
return shell(_that.command);case ProjectActionKindDto_Agent():
return agent(_that.prompt,_that.provider,_that.model,_that.traits,_that.mode,_that.access);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String command)?  shell,TResult? Function( String prompt,  AgentProvider provider,  String? model,  String? traits,  String? mode,  ProjectActionAccessDto access)?  agent,}) {final _that = this;
switch (_that) {
case ProjectActionKindDto_Shell() when shell != null:
return shell(_that.command);case ProjectActionKindDto_Agent() when agent != null:
return agent(_that.prompt,_that.provider,_that.model,_that.traits,_that.mode,_that.access);case _:
  return null;

}
}

}

/// @nodoc


class ProjectActionKindDto_Shell extends ProjectActionKindDto {
  const ProjectActionKindDto_Shell({required this.command}): super._();
  

 final  String command;

/// Create a copy of ProjectActionKindDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$ProjectActionKindDto_ShellCopyWith<ProjectActionKindDto_Shell> get copyWith => _$ProjectActionKindDto_ShellCopyWithImpl<ProjectActionKindDto_Shell>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ProjectActionKindDto_Shell&&(identical(other.command, command) || other.command == command));
}


@override
int get hashCode => Object.hash(runtimeType,command);

@override
String toString() {
  return 'ProjectActionKindDto.shell(command: $command)';
}


}

/// @nodoc
abstract mixin class $ProjectActionKindDto_ShellCopyWith<$Res> implements $ProjectActionKindDtoCopyWith<$Res> {
  factory $ProjectActionKindDto_ShellCopyWith(ProjectActionKindDto_Shell value, $Res Function(ProjectActionKindDto_Shell) _then) = _$ProjectActionKindDto_ShellCopyWithImpl;
@useResult
$Res call({
 String command
});




}
/// @nodoc
class _$ProjectActionKindDto_ShellCopyWithImpl<$Res>
    implements $ProjectActionKindDto_ShellCopyWith<$Res> {
  _$ProjectActionKindDto_ShellCopyWithImpl(this._self, this._then);

  final ProjectActionKindDto_Shell _self;
  final $Res Function(ProjectActionKindDto_Shell) _then;

/// Create a copy of ProjectActionKindDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? command = null,}) {
  return _then(ProjectActionKindDto_Shell(
command: null == command ? _self.command : command // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class ProjectActionKindDto_Agent extends ProjectActionKindDto {
  const ProjectActionKindDto_Agent({required this.prompt, required this.provider, this.model, this.traits, this.mode, required this.access}): super._();
  

 final  String prompt;
 final  AgentProvider provider;
 final  String? model;
 final  String? traits;
 final  String? mode;
 final  ProjectActionAccessDto access;

/// Create a copy of ProjectActionKindDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$ProjectActionKindDto_AgentCopyWith<ProjectActionKindDto_Agent> get copyWith => _$ProjectActionKindDto_AgentCopyWithImpl<ProjectActionKindDto_Agent>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ProjectActionKindDto_Agent&&(identical(other.prompt, prompt) || other.prompt == prompt)&&(identical(other.provider, provider) || other.provider == provider)&&(identical(other.model, model) || other.model == model)&&(identical(other.traits, traits) || other.traits == traits)&&(identical(other.mode, mode) || other.mode == mode)&&(identical(other.access, access) || other.access == access));
}


@override
int get hashCode => Object.hash(runtimeType,prompt,provider,model,traits,mode,access);

@override
String toString() {
  return 'ProjectActionKindDto.agent(prompt: $prompt, provider: $provider, model: $model, traits: $traits, mode: $mode, access: $access)';
}


}

/// @nodoc
abstract mixin class $ProjectActionKindDto_AgentCopyWith<$Res> implements $ProjectActionKindDtoCopyWith<$Res> {
  factory $ProjectActionKindDto_AgentCopyWith(ProjectActionKindDto_Agent value, $Res Function(ProjectActionKindDto_Agent) _then) = _$ProjectActionKindDto_AgentCopyWithImpl;
@useResult
$Res call({
 String prompt, AgentProvider provider, String? model, String? traits, String? mode, ProjectActionAccessDto access
});




}
/// @nodoc
class _$ProjectActionKindDto_AgentCopyWithImpl<$Res>
    implements $ProjectActionKindDto_AgentCopyWith<$Res> {
  _$ProjectActionKindDto_AgentCopyWithImpl(this._self, this._then);

  final ProjectActionKindDto_Agent _self;
  final $Res Function(ProjectActionKindDto_Agent) _then;

/// Create a copy of ProjectActionKindDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? prompt = null,Object? provider = null,Object? model = freezed,Object? traits = freezed,Object? mode = freezed,Object? access = null,}) {
  return _then(ProjectActionKindDto_Agent(
prompt: null == prompt ? _self.prompt : prompt // ignore: cast_nullable_to_non_nullable
as String,provider: null == provider ? _self.provider : provider // ignore: cast_nullable_to_non_nullable
as AgentProvider,model: freezed == model ? _self.model : model // ignore: cast_nullable_to_non_nullable
as String?,traits: freezed == traits ? _self.traits : traits // ignore: cast_nullable_to_non_nullable
as String?,mode: freezed == mode ? _self.mode : mode // ignore: cast_nullable_to_non_nullable
as String?,access: null == access ? _self.access : access // ignore: cast_nullable_to_non_nullable
as ProjectActionAccessDto,
  ));
}


}

// dart format on
