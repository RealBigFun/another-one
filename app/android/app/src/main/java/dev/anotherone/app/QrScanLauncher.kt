package dev.anotherone.app

import android.app.Activity
import android.util.Log
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning

/**
 * Tiny Kotlin shim around Google's ML Kit "1-tap" code scanner.
 *
 * Rust calls `launch(activity)` from `desktop/src/mobile.rs` via JNI.
 * The Play Services scanner UI takes over the screen, the user aims at
 * a QR code, and on success the scanned string is forwarded back into
 * Rust through `onScanResult` — a native fn defined in Rust as
 * `Java_dev_anotherone_app_QrScanLauncher_onScanResult`. The Rust side
 * pushes that URL onto a process-wide queue (`mobile::QR_SCAN_QUEUE`)
 * which `AnotherOneApp::drain_qr_scan_queue` empties on the next render
 * tick.
 *
 * No Manifest entry needed — the scanner activity is hosted by Play
 * Services itself.
 */
object QrScanLauncher {
    private const val TAG = "QrScanLauncher"

    init {
        // `NativeActivity` already loaded `libanother_one.so` to run our
        // Rust `android_main`, but the JVM's `external fun` resolver only
        // searches libraries that were registered with it via
        // `System.loadLibrary` inside a class loader that owns this
        // class. Without this explicit call, dispatching `onScanResult`
        // back through JNI fails with `UnsatisfiedLinkError: No
        // implementation found for ... (tried
        // Java_dev_anotherone_app_QrScanLauncher_onScanResult ...)`
        // even though the symbol IS exported by the `.so`. Loading the
        // library here is idempotent — Android dedupes by `dlopen`
        // refcount, so the second load is effectively free.
        System.loadLibrary("another_one")
    }

    @JvmStatic
    external fun onScanResult(result: String?)

    @JvmStatic
    fun launch(activity: Activity) {
        val options = GmsBarcodeScannerOptions.Builder()
            .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
            .build()
        val scanner = GmsBarcodeScanning.getClient(activity, options)
        scanner.startScan()
            .addOnSuccessListener { barcode ->
                val raw = barcode.rawValue
                Log.i(TAG, "scan success: ${raw?.length ?: 0} chars")
                onScanResult(raw)
            }
            .addOnCanceledListener {
                Log.i(TAG, "scan canceled")
                onScanResult(null)
            }
            .addOnFailureListener { e ->
                Log.w(TAG, "scan failed", e)
                onScanResult(null)
            }
    }
}
