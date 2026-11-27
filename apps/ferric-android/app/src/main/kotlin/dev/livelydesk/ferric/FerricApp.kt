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
import java.io.File

/**
 * Application entry point. Registers JNI `ndk_context`, configures Coil 3 with tight mobile RAM/disk
 * budgets (20 MB memory LRU + 32 MB disk cache), trims bloaty Chromium WebView cache dirs without
 * touching login cookies, and creates [CoreStateStore] lazily.
 */
class FerricApp : Application(), SingletonImageLoader.Factory {
    lateinit var store: CoreStateStore
        private set

    override fun onCreate() {
        super.onCreate()
        NativeBridge.ensureInitialized(this)
        store = CoreStateStore(applicationContext)
        trimDisposableWebViewCachesAsync(applicationContext)
    }

    override fun newImageLoader(context: Context): ImageLoader =
        ImageLoader.Builder(context)
            .memoryCache {
                MemoryCache.Builder()
                    .maxSizeBytes(20L * 1024L * 1024L)
                    .build()
            }
            .diskCache {
                DiskCache.Builder()
                    .directory(context.cacheDir.resolve("coil_artwork").toOkioPath())
                    .maxSizeBytes(32L * 1024L * 1024L)
                    .build()
            }
            .allowHardware(true)
            .crossfade(160)
            .build()

    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (::store.isInitialized && level >= TRIM_MEMORY_BACKGROUND) {
            store.onTrimMemory()
        }
    }

    private fun trimDisposableWebViewCachesAsync(ctx: Context) {
        Thread(
            {
                runCatching {
                    // Prune legacy double-cached artwork_cache under cacheDir if migrated to coil_artwork.
                    pruneDirectoryIfOversized(File(ctx.cacheDir, "WebView"), maxKeepBytes = 2L * 1024L * 1024L)
                    val appWebView = ctx.getDir("webview", MODE_PRIVATE)
                    pruneDirectoryIfOversized(File(appWebView, "Default/Cache"), maxKeepBytes = 2L * 1024L * 1024L)
                    pruneDirectoryIfOversized(File(appWebView, "Default/Code Cache"), maxKeepBytes = 2L * 1024L * 1024L)
                    pruneDirectoryIfOversized(File(appWebView, "Default/GPUCache"), maxKeepBytes = 2L * 1024L * 1024L)
                    val chromiumDir = File(ctx.applicationInfo.dataDir, "app_webview")
                    pruneDirectoryIfOversized(File(chromiumDir, "Default/Cache"), maxKeepBytes = 2L * 1024L * 1024L)
                    pruneDirectoryIfOversized(File(chromiumDir, "Default/Code Cache"), maxKeepBytes = 2L * 1024L * 1024L)
                    pruneDirectoryIfOversized(File(chromiumDir, "Default/GPUCache"), maxKeepBytes = 2L * 1024L * 1024L)
                    pruneDirectoryIfOversized(File(chromiumDir, "Default/Service Worker"), maxKeepBytes = 2L * 1024L * 1024L)
                }
            },
            "ferric-cache-hygiene",
        ).start()
    }

    private fun pruneDirectoryIfOversized(dir: File, maxKeepBytes: Long) {
        if (!dir.exists() || !dir.isDirectory) return
        val files = dir.walkTopDown().filter { it.isFile }.toList()
        var total = files.sumOf { it.length() }
        if (total <= maxKeepBytes) return
        for (f in files.sortedBy { it.lastModified() }) {
            if (total <= maxKeepBytes) break
            val len = f.length()
            if (f.delete()) {
                total -= len
            }
        }
    }
}
