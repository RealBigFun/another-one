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

 String get projectId; String? get currentBranch; BigInt get changedFileCount; BigInt get ahead; BigInt get behind;
/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$WorkerReplyCopyWith<WorkerReply> get copyWith => _$WorkerReplyCopyWithImpl<WorkerReply>(this as WorkerReply, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is WorkerReply&&(identical(other.projectId, projectId) || other.projectId == projectId)&&(identical(other.currentBranch, currentBranch) || other.currentBranch == currentBranch)&&(identical(other.changedFileCount, changedFileCount) || other.changedFileCount == changedFileCount)&&(identical(other.ahead, ahead) || other.ahead == ahead)&&(identical(other.behind, behind) || other.behind == behind));
}


@override
int get hashCode => Object.hash(runtimeType,projectId,currentBranch,changedFileCount,ahead,behind);

@override
String toString() {
  return 'WorkerReply(projectId: $projectId, currentBranch: $currentBranch, changedFileCount: $changedFileCount, ahead: $ahead, behind: $behind)';
}


}

/// @nodoc
abstract mixin class $WorkerReplyCopyWith<$Res>  {
  factory $WorkerReplyCopyWith(WorkerReply value, $Res Function(WorkerReply) _then) = _$WorkerReplyCopyWithImpl;
@useResult
$Res call({
 String projectId, String? currentBranch, BigInt changedFileCount, BigInt ahead, BigInt behind
});




}
/// @nodoc
class _$WorkerReplyCopyWithImpl<$Res>
    implements $WorkerReplyCopyWith<$Res> {
  _$WorkerReplyCopyWithImpl(this._self, this._then);

  final WorkerReply _self;
  final $Res Function(WorkerReply) _then;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? projectId = null,Object? currentBranch = freezed,Object? changedFileCount = null,Object? ahead = null,Object? behind = null,}) {
  return _then(_self.copyWith(
projectId: null == projectId ? _self.projectId : projectId // ignore: cast_nullable_to_non_nullable
as String,currentBranch: freezed == currentBranch ? _self.currentBranch : currentBranch // ignore: cast_nullable_to_non_nullable
as String?,changedFileCount: null == changedFileCount ? _self.changedFileCount : changedFileCount // ignore: cast_nullable_to_non_nullable
as BigInt,ahead: null == ahead ? _self.ahead : ahead // ignore: cast_nullable_to_non_nullable
as BigInt,behind: null == behind ? _self.behind : behind // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}

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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( WorkerReply_GitRefresh value)?  gitRefresh,required TResult orElse(),}){
final _that = this;
switch (_that) {
case WorkerReply_GitRefresh() when gitRefresh != null:
return gitRefresh(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( WorkerReply_GitRefresh value)  gitRefresh,}){
final _that = this;
switch (_that) {
case WorkerReply_GitRefresh():
return gitRefresh(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( WorkerReply_GitRefresh value)?  gitRefresh,}){
final _that = this;
switch (_that) {
case WorkerReply_GitRefresh() when gitRefresh != null:
return gitRefresh(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String projectId,  String? currentBranch,  BigInt changedFileCount,  BigInt ahead,  BigInt behind)?  gitRefresh,required TResult orElse(),}) {final _that = this;
switch (_that) {
case WorkerReply_GitRefresh() when gitRefresh != null:
return gitRefresh(_that.projectId,_that.currentBranch,_that.changedFileCount,_that.ahead,_that.behind);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String projectId,  String? currentBranch,  BigInt changedFileCount,  BigInt ahead,  BigInt behind)  gitRefresh,}) {final _that = this;
switch (_that) {
case WorkerReply_GitRefresh():
return gitRefresh(_that.projectId,_that.currentBranch,_that.changedFileCount,_that.ahead,_that.behind);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String projectId,  String? currentBranch,  BigInt changedFileCount,  BigInt ahead,  BigInt behind)?  gitRefresh,}) {final _that = this;
switch (_that) {
case WorkerReply_GitRefresh() when gitRefresh != null:
return gitRefresh(_that.projectId,_that.currentBranch,_that.changedFileCount,_that.ahead,_that.behind);case _:
  return null;

}
}

}

/// @nodoc


class WorkerReply_GitRefresh extends WorkerReply {
  const WorkerReply_GitRefresh({required this.projectId, this.currentBranch, required this.changedFileCount, required this.ahead, required this.behind}): super._();
  

@override final  String projectId;
@override final  String? currentBranch;
@override final  BigInt changedFileCount;
@override final  BigInt ahead;
@override final  BigInt behind;

/// Create a copy of WorkerReply
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
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
@override @useResult
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
@override @pragma('vm:prefer-inline') $Res call({Object? projectId = null,Object? currentBranch = freezed,Object? changedFileCount = null,Object? ahead = null,Object? behind = null,}) {
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

// dart format on
