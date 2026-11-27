package dev.livelydesk.ferric

import android.app.Application
import dev.livelydesk.ferric.core.CoreStateStore
import dev.livelydesk.ferric.core.NativeBridge

/**
 * Application entry point. Registers JNI `ndk_context` and creates the lazy [CoreStateStore]
 * without starting Android audio streams during cold background launch.
 */
class FerricApp : Application() {
    lateinit var store: CoreStateStore
        private set

    override fun onCreate() {
        super.onCreate()
        NativeBridge.ensureInitialized(this)
        store = CoreStateStore(applicationContext)
    }

    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (::store.isInitialized && level >= TRIM_MEMORY_BACKGROUND) {
            store.onTrimMemory()
        }
    }
}
