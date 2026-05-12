//! Master/detail navigation state for the narrow (phone) layout.
//!
//! Wide mode (desktop three-column) doesn't use any of this — its visible
//! panes are controlled by `sidebar_w`, `right_w`, and the per-page flags
//! (`settings_open`, `pair_mobile_modal_open`, …). When the viewport is
//! narrower than `layout::NARROW_BREAKPOINT`, `app::Render::render` forks
//! into a single-pane stack whose contents are selected by
//! `AnotherOneApp::mobile_view` and a small history stack used by the
//! phone header's back chevron.
//!
//! Also hosts the QR-scan-to-pair plumbing — a process-wide queue that
//! the JNI native callback (`Java_dev_anotherone_app_QrScanLauncher_onScanResult`,
//! defined under cfg(target_os = "android")) writes into and the render
//! tick drains.

#[cfg(target_os = "android")]
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, OnceLock};

use gpui::Context;

use crate::app::AnotherOneApp;

/// Raw pointer to the Java `NativeActivity` jobject, captured by
/// `android_main` and used by the JNI helpers below. We can't trust
/// `ndk_context::android_context().context()` here — `android-activity`'s
/// glue passes the *Application* global to `ndk_context`, not the
/// Activity, so any JNI call that needs an Activity-typed argument
/// (ML Kit's barcode scanner is one) gets rejected with `JNI ERROR
/// (app bug): attempt to pass an instance of android.app.Application
/// as argument 1 to ...`. Storing the activity pointer ourselves
/// sidesteps that. Lifetime is fine — `android_app.activity_as_ptr()`
/// returns a JNI global ref that lives as long as the NativeActivity
/// itself, which equals the process lifetime for this app.
#[cfg(target_os = "android")]
static ACTIVITY_PTR: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Called once from `android_main` to stash the activity pointer so
/// later JNI helpers can hand it to Java APIs that demand an Activity.
#[cfg(target_os = "android")]
pub fn set_activity_ptr(ptr: *mut std::ffi::c_void) {
    ACTIVITY_PTR.store(ptr, Ordering::Release);
}

/// Internal (app-private, survives `adb install -r`) storage path
/// stashed at `android_main` entry from
/// `AndroidApp::internal_data_path`. Other modules that want to
/// persist small per-install state (iroh secret key, future
/// pairing metadata) read it via [`internal_data_path`] instead of
/// re-deriving from JNI.
#[cfg(target_os = "android")]
static INTERNAL_DATA_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Latched OS dark-mode preference read once at `android_main`
/// entry from `AndroidApp::config().ui_mode_night()`. GPUI
/// (via gpui-mobile) only updates its own window-appearance state
/// on `ConfigChanged`, which doesn't fire at app start — so the
/// first render sees the default `Light` appearance regardless of
/// the actual OS theme. Reading the config at init + exposing it
/// to our theme resolver (`theme::resolve_theme`) lets
/// `ThemeMode::System` render correctly from the first frame. See
/// #TODO (system theme on android).
#[cfg(target_os = "android")]
static SYSTEM_PREFERS_DARK: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Sentinel values inside `SYSTEM_PREFERS_DARK`: 0 = unknown (no
/// activity-lifecycle hook has run yet, fall through to
/// `window.appearance()`), 1 = light, 2 = dark.
#[cfg(target_os = "android")]
const SYSTEM_THEME_UNKNOWN: u8 = 0;
#[cfg(target_os = "android")]
const SYSTEM_THEME_LIGHT: u8 = 1;
#[cfg(target_os = "android")]
const SYSTEM_THEME_DARK: u8 = 2;

/// Stash the app's internal data directory so non-android-activity
/// callers (notably `daemon-client`'s dial helper) can persist
/// state that should outlive a single process but stay within the
/// app-private storage sandbox. Idempotent — first call wins. See
/// #TODO(secret-key-persist) for the iroh-client-side follow-up.
#[cfg(target_os = "android")]
pub fn set_internal_data_path(path: std::path::PathBuf) {
    let _ = INTERNAL_DATA_PATH.set(path);
}

/// Record the OS dark-mode preference from
/// `AndroidApp::config().ui_mode_night()`. Called once at
/// `android_main` entry and again on any future `ConfigChanged`
/// event that we choose to route through here. Later reads pick
/// this up via [`system_prefers_dark`].
#[cfg(target_os = "android")]
pub fn set_system_prefers_dark(prefers_dark: bool) {
    let sentinel = if prefers_dark {
        SYSTEM_THEME_DARK
    } else {
        SYSTEM_THEME_LIGHT
    };
    SYSTEM_PREFERS_DARK.store(sentinel, std::sync::atomic::Ordering::Release);
}

/// The OS dark-mode preference previously stashed by
/// [`set_system_prefers_dark`], or `None` when nothing's recorded
/// (pre-`android_main` call paths, host-target tests). The theme
/// resolver uses this to shortcut `ThemeMode::System` on Android
/// — otherwise the first render sees gpui-mobile's default
/// `WindowAppearance::Light` regardless of the phone's real
/// setting.
#[cfg(target_os = "android")]
pub fn system_prefers_dark() -> Option<bool> {
    match SYSTEM_PREFERS_DARK.load(std::sync::atomic::Ordering::Acquire) {
        SYSTEM_THEME_LIGHT => Some(false),
        SYSTEM_THEME_DARK => Some(true),
        _ => None,
    }
}

/// Show the soft keyboard. Called when the user taps a terminal
/// pane on Android — the terminal surface isn't a GPUI `TextInput`
/// and wouldn't otherwise trigger the IME to rise, so without this
/// the phone paired fine but users couldn't type. Idempotent: if
/// the IME is already visible, the NDK call is a no-op. Desktop
/// gets a stub that compiles to nothing so the shared panel code
/// stays target-agnostic.
#[cfg(target_os = "android")]
pub fn show_soft_keyboard() {
    gpui_mobile::android::jni::show_keyboard_android(gpui_mobile::KeyboardType::Default);
}

/// Host-target stub — no OS-theme plumbing on desktop (GPUI's
/// `window.appearance()` already reads the real value there).
#[cfg(not(target_os = "android"))]
pub fn system_prefers_dark() -> Option<bool> {
    None
}

/// Host-target stub — desktop has hardware keyboards, no IME to
/// raise. Compiles to nothing so the shared panel click handlers
/// can call unconditionally.
#[cfg(not(target_os = "android"))]
pub fn show_soft_keyboard() {}

/// The path [`set_internal_data_path`] stashed, or `None` if the
/// activity glue never reported one (shouldn't happen under
/// `android-activity` 0.6, but keep the probe total so host-target
/// tests that don't enter `android_main` can still link).
#[cfg(target_os = "android")]
pub fn internal_data_path() -> Option<&'static std::path::Path> {
    INTERNAL_DATA_PATH.get().map(|p| p.as_path())
}

/// File inside the app-private internal storage where the iroh
/// client's secret key persists across reconnects and `adb
/// install -r` cycles. Ephemeral key-per-dial (the pre-#TODO
/// behaviour) meant every reconnect presented a fresh viewer_id,
/// which the daemon rejected with "no outstanding pair nonce"
/// because the allowlist entry was keyed on the first-pair
/// identity.
#[cfg(target_os = "android")]
pub fn iroh_secret_key_path() -> Option<std::path::PathBuf> {
    Some(internal_data_path()?.join("iroh-client.key"))
}

/// Development-only pair-URL trigger file. When present inside the
/// app's internal-data directory, [`drain_qr_scan_results`] reads
/// its contents as if they were a scanned QR, deletes the file,
/// and returns the URL to the render tick. This is what lets the
/// `scripts/test-mobile-pair.sh` harness drive the pair flow
/// without a human holding the phone to a camera.
///
/// `adb shell run-as dev.anotherone.app sh -c "printf %s '<url>' >
/// files/pair-trigger"` is the canonical producer. Anything that
/// can write into the app-private storage namespace works;
/// nothing else on the device (including the user via the system
/// file picker) has access without `run-as`, so leaving the path
/// wired in release builds is a non-issue.
#[cfg(target_os = "android")]
fn pair_trigger_file_path() -> Option<std::path::PathBuf> {
    Some(internal_data_path()?.join("pair-trigger"))
}

#[cfg(target_os = "android")]
fn absorb_pair_trigger_file() {
    let Some(path) = pair_trigger_file_path() else {
        return;
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            log::warn!("pair-trigger read failed: {e}");
            return;
        }
    };
    // Remove first so a failed push doesn't re-fire on every tick.
    // The canonical producer writes → delete-after-consume, but a
    // crashed producer could leave a stale file; the unlink here
    // keeps the fire-once semantics regardless.
    if let Err(e) = std::fs::remove_file(&path) {
        log::warn!("pair-trigger unlink failed: {e}");
    }
    let url = contents.trim().to_string();
    if url.is_empty() {
        return;
    }
    log::info!("pair-trigger: injecting URL len={} into QR queue", url.len());
    push_qr_scan_result(url);
}

/// Host-target stub — mobile's persistent-identity path concept
/// doesn't apply on the desktop iroh client path (desktop runs the
/// iroh *server*, persisting its own key via the daemon's
/// `load_or_create_secret_key`). Returning `None` lets shared
/// callers fall through to ephemeral-key semantics.
#[cfg(not(target_os = "android"))]
pub fn iroh_secret_key_path() -> Option<std::path::PathBuf> {
    None
}

/// Queue of pairing URLs delivered by the QR scanner. The JNI callback
/// pushes; the render tick drains. Wrapped in `OnceLock` so it can be
/// referenced from both the JNI thread and the GPUI thread without
/// initialization races.
static QR_SCAN_QUEUE: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn qr_scan_queue() -> &'static Mutex<Vec<String>> {
    QR_SCAN_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Push a scanned URL into the queue. Invoked by the JNI callback and
/// (in tests) by host-side code that wants to simulate a scan.
#[cfg(target_os = "android")]
pub fn push_qr_scan_result(url: String) {
    if let Ok(mut q) = qr_scan_queue().lock() {
        q.push(url);
    }
}

/// Take all pending scan results. Called from the render tick.
pub fn drain_qr_scan_results() -> Vec<String> {
    // On Android, check the pair-trigger file first so the
    // automation harness (scripts/test-mobile-pair.sh) can feed a
    // URL into the same queue the camera callback uses. No-op
    // when no trigger file is present (common path).
    #[cfg(target_os = "android")]
    absorb_pair_trigger_file();
    qr_scan_queue()
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

/// Trigger the camera-based QR scanner. On Android this calls into
/// the Kotlin `QrScanLauncher` activity via JNI; on other targets it
/// returns an error so callers can post a "not on this platform" toast.
///
/// Class lookup goes through the *activity's* class loader, not
/// `JNIEnv::find_class`. When `attach_current_thread` runs on a thread
/// the JVM didn't itself launch (which is the case for every GPUI
/// thread on Android), the JNI call stack contains no Java frames, so
/// `find_class` falls back to the system class loader — which only
/// knows about `/system/lib64` and `/system_ext/lib64` and aborts with
/// `ClassNotFoundException` for anything in our APK. Reflecting through
/// `Activity.getClassLoader().loadClass(...)` uses the dex-aware loader
/// the platform set up at app launch.
#[cfg(target_os = "android")]
pub fn launch_qr_scanner() -> Result<(), String> {
    use jni::objects::{JClass, JObject, JValue};

    let ctx = ndk_context::android_context();
    let activity_ptr = ACTIVITY_PTR.load(Ordering::Acquire);
    if activity_ptr.is_null() {
        return Err("activity pointer not yet stashed by android_main".into());
    }
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }
        .map_err(|e| format!("JavaVM::from_raw: {e}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| format!("attach_current_thread: {e}"))?;
    // NB: NOT `ctx.context()` — that's the `Application` global on
    // `android-activity` 0.6, but ML Kit's `GmsBarcodeScanning` (and
    // anything else taking `Activity`) demands the actual activity.
    let activity = unsafe { JObject::from_raw(activity_ptr.cast()) };

    // `activity.getClassLoader()` — the Context instance method —
    // returns the *app's* classloader, which has visibility into all
    // of the APK's dex files (classes.dex, classes2.dex, classes3.dex —
    // our `QrScanLauncher` happens to be in classes3.dex). Going via
    // `activity.getClass().getClassLoader()` instead would return the
    // *framework* classloader (which loaded the `android.app.Activity`
    // class itself), and that loader knows nothing about our app's
    // dex archive — `loadClass("dev.anotherone.app.QrScanLauncher")`
    // would `ClassNotFoundException` despite the class being right
    // there in classes3.dex.
    let class_loader = env
        .call_method(
            &activity,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("Activity.getClassLoader: {e}"))?;
    let class_name = env
        .new_string("dev.anotherone.app.QrScanLauncher")
        .map_err(|e| format!("new_string: {e}"))?;
    let class_obj = env
        .call_method(
            &class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name)],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("ClassLoader.loadClass(QrScanLauncher): {e}"))?;
    let class: JClass = class_obj.into();
    env.call_static_method(
        class,
        "launch",
        "(Landroid/app/Activity;)V",
        &[JValue::Object(&activity)],
    )
    .map_err(|e| format!("QrScanLauncher.launch: {e}"))?;
    Ok(())
}

#[cfg(not(target_os = "android"))]
pub fn launch_qr_scanner() -> Result<(), String> {
    Err("QR scanning is only available on the mobile build".into())
}

/// JNI bridge: Kotlin's `QrScanLauncher` calls this when the user
/// completes (or cancels) a scan. A null jstring means cancellation.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_anotherone_app_QrScanLauncher_onScanResult(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    result: jni::objects::JString,
) {
    if result.is_null() {
        return;
    }
    let Ok(rust_str) = env.get_string(&result) else {
        return;
    };
    let url: String = rust_str.into();
    if !url.is_empty() {
        push_qr_scan_result(url);
    }
}

/// Which pane is currently full-bleed on a narrow viewport.
///
/// `Project` carries the project id (a `String` everywhere else in the app)
/// so the workspace pane can render the right thing without us having to
/// also mutate `active_project_page`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MobileView {
    /// Projects list — the home screen on a phone. Reuses the same
    /// `sidebar_content` builder the desktop three-column layout calls
    /// for its left column.
    Home,
    /// Workspace for a specific project (terminals + project page).
    /// Reuses the existing `WorkspacePane` entity.
    Project(String),
    /// Right-sidebar git panel (working tree / commits / checks / compare),
    /// reachable from the phone header's git icon when a project is active.
    ChangedFiles,
}

impl AnotherOneApp {
    /// Push the current `mobile_view` onto the back-stack and switch to
    /// `next`. Use from tap handlers in the sidebar / phone header.
    pub fn mobile_push(&mut self, next: MobileView, cx: &mut Context<Self>) {
        if self.mobile_view == next {
            return;
        }
        self.mobile_nav_stack.push(self.mobile_view.clone());
        self.mobile_view = next;
        cx.notify();
    }

    /// Pop the back-stack, restoring the previous `mobile_view`. Returns
    /// `false` when the stack was already empty so callers wired to the
    /// Android hardware back button can let the OS handle it (i.e.
    /// background the activity).
    pub fn mobile_back(&mut self, cx: &mut Context<Self>) -> bool {
        match self.mobile_nav_stack.pop() {
            Some(prev) => {
                self.mobile_view = prev;
                cx.notify();
                true
            }
            None => false,
        }
    }
}
