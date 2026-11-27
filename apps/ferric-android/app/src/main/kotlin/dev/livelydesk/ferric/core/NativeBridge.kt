package dev.livelydesk.ferric.core

import android.content.Context

/**
 * Loads `libferric_mobile_ffi.so` and registers Android's `Application` [Context] with Rust's
 * `ndk_context` before any Android audio (`oboe`) stream is opened.
 */
object NativeBridge {
    @Volatile
    private var loaded = false

    fun ensureInitialized(appContext: Context) {
        if (!loaded) {
            synchronized(this) {
                if (!loaded) {
                    System.loadLibrary("ferric_mobile_ffi")
                    initAndroidContext(appContext.applicationContext)
                    loaded = true
                }
            }
        }
    }

    @JvmStatic
    private external fun initAndroidContext(appContext: Context)
}
