// AnotherOne Android shell — minimal Gradle project that wraps the
// pre-built `libanother_one.so` (produced by cargo-ndk from the
// workspace `another-one` crate) into an APK using `NativeActivity`.
// All UI lives in Rust; there is no Java/Kotlin app code.

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "AnotherOne"
include(":app")
