// Root Gradle build for the AnotherOne Android shell.
//
// Pinned versions match what `gpui-mobile`'s example builds against —
// AGP 9.1.0 + Kotlin 1.9.22 + Gradle 9.4.1 (set in the wrapper). The
// versions are tightly coupled; bump only in lockstep.

buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:9.1.0")
        // Kotlin support — required for the QR-scan shim
        // (`QrScanLauncher.kt`) which wraps Google's ML Kit code-scanner
        // module so Rust can launch it via JNI.
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:1.9.22")
    }
}

tasks.register("clean", Delete::class) {
    delete(rootProject.layout.buildDirectory)
}
