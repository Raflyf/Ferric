package dev.livelydesk.ferric

import android.app.Application
import android.content.Context
import coil3.ImageLoader
import coil3.SingletonImageLoader
import coil3.disk.DiskCache
import coil3.memory.MemoryCache
import coil3.request.allowHardware
import coil3.request.crossfade
import dev.livelydesk.ferric.core.CoreStateStore
import dev.livelydesk.ferric.core.NativeBridge
import okio.Path.Companion.toOkioPath

/**
 * Application entry point. Registers JNI `ndk_context`, configures Coil 3 with strict RAM/disk caps
 * (32 MB memory LRU + 80 MB disk cache), and creates the lazy [CoreStateStore] without starting
 * Android audio streams during cold background launch.
 */
class FerricApp : Application(), SingletonImageLoader.Factory {
    lateinit var store: CoreStateStore
        private set

    override fun onCreate() {
        super.onCreate()
        NativeBridge.ensureInitialized(this)
        store = CoreStateStore(applicationContext)
    }

    override fun newImageLoader(context: Context): ImageLoader =
        ImageLoader.Builder(context)
            .memoryCache {
                MemoryCache.Builder()
                    .maxSizeBytes(32L * 1024L * 1024L)
                    .build()
            }
            .diskCache {
                DiskCache.Builder()
                    .directory(context.cacheDir.resolve("artwork_cache").toOkioPath())
                    .maxSizeBytes(80L * 1024L * 1024L)
                    .build()
            }
            .allowHardware(true)
            .crossfade(180)
            .build()

    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (::store.isInitialized && level >= TRIM_MEMORY_BACKGROUND) {
            store.onTrimMemory()
        }
    }
}
