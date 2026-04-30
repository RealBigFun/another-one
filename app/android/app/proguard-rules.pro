# No Java/Kotlin app code lives in this module — the only class
# referenced from the manifest is `android.app.NativeActivity`, which is
# part of the framework and must be preserved by name.
-keep class android.app.NativeActivity { *; }
