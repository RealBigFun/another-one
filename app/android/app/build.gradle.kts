// AnotherOne Android app module.
//
// Packages the pre-built `libanother_one.so` (produced by `cargo ndk`)
// into an APK whose only Activity is the system `NativeActivity`. There
// is no Java/Kotlin app code — the GPUI app drives the entire UI from
// Rust via gpui-mobile.

plugins {
    id("com.android.application")
    // AGP 9 ships built-in Kotlin support; applying the standalone
    // `org.jetbrains.kotlin.android` plugin clashes with it. The Kotlin
    // sources under `src/main/java/` are picked up automatically.
}

android {
    namespace = "dev.anotherone.app"
    compileSdk = 34

    defaultConfig {
        applicationId = "dev.anotherone.app"
        // Vulkan 1.0 is mandatory from API 26; matches gpui-mobile's
        // minimum supported API level.
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

        ndk {
            abiFilters += listOf("arm64-v8a")
        }

        // Forwarded into AndroidManifest.xml to tell `NativeActivity`
        // which `.so` to dlopen. Must match the cdylib `name` in
        // `desktop/Cargo.toml` — without the `lib` prefix or `.so`.
        manifestPlaceholders["nativeLibraryName"] = "another_one"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
        debug {
            isDebuggable = true
            isJniDebuggable = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
            // Kotlin shim for the QR-scan launcher lives alongside any
            // future Java helper code under `src/main/java/`.
            java.srcDirs("src/main/java")
        }
    }

    packaging {
        // Cargo's release strip already happens; don't double-strip.
        jniLibs {
            keepDebugSymbols += listOf("*/arm64-v8a/libanother_one.so")
        }
    }

    lint {
        abortOnError = false
        checkReleaseBuilds = false
    }
}

dependencies {
    // Google's "1-tap" ML Kit code scanner: hosts its own scanner UI
    // inside Play Services, so we don't need to embed CameraX or write
    // our own QR detector. Returns a `Barcode` via Task; on success the
    // Kotlin shim calls a JNI native fn that pushes the URL to a
    // process-wide queue the GPUI app drains every render tick.
    implementation("com.google.android.gms:play-services-code-scanner:16.1.0")
    implementation("org.jetbrains.kotlin:kotlin-stdlib:1.9.22")
}
