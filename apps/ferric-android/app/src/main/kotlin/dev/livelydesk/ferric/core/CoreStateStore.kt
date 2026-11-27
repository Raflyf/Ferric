package dev.livelydesk.ferric.core

import android.content.Context
import dev.livelydesk.ferric.playback.OutputRouteHelper
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uniffi.ferric_mobile_ffi.FerricMobileCore
import uniffi.ferric_mobile_ffi.FfiEventSink
import uniffi.ferric_mobile_ffi.FfiRepeatMode
import uniffi.ferric_mobile_ffi.FfiTelemetry
import uniffi.ferric_mobile_ffi.FfiTrack

data class PlaybackUiState(
    val tracks: List<FfiTrack> = emptyList(),
    val currentTrack: FfiTrack? = null,
    val upNext: List<FfiTrack> = emptyList(),
    val isPlaying: Boolean = false,
    val positionMs: Long = 0L,
    val durationMs: Long = 0L,
    val sampleRate: Int = 0,
    val outputRate: Int = 0,
    val isCrossfading: Boolean = false,
    val spectrumBins: List<Int> = List(8) { 0 },
    val shuffle: Boolean = false,
    val repeatMode: FfiRepeatMode = FfiRepeatMode.OFF,
    val activeOutputRoute: String = "Phone Speaker",
    val statusMessage: String? = null,
    val isReady: Boolean = false,
)

/**
 * Pure state reducer kept JVM-testable without native `.so` loading.
 */
object PlaybackStateReducer {
    fun applyTelemetry(state: PlaybackUiState, t: FfiTelemetry): PlaybackUiState = state.copy(
        isPlaying = t.isPlaying,
        positionMs = t.positionMs.toLong().coerceAtLeast(0L),
        durationMs = t.durationMs.toLong().coerceAtLeast(0L),
        sampleRate = t.sampleRate.toInt(),
        outputRate = t.outputRate.toInt(),
        isCrossfading = t.isCrossfading,
        spectrumBins = if (t.spectrumBins.size == 8) {
            t.spectrumBins.map { it.toInt().coerceIn(0, 1000) }
        } else {
            state.spectrumBins
        },
    )

    fun applyNowPlaying(state: PlaybackUiState, track: FfiTrack?): PlaybackUiState = state.copy(
        currentTrack = track,
        durationMs = track?.durationMs?.toLong()?.coerceAtLeast(0L) ?: state.durationMs,
    )
}

/**
 * Singleton runtime bridge owning the Rust [FerricMobileCore] handle and exposing Compose-friendly
 * [StateFlow] snapshots. All FFI mutations execute off the Main Thread via [Dispatchers.IO].
 */
class CoreStateStore(private val appContext: Context) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val coreLock = Any()

    @Volatile
    private var core: FerricMobileCore? = null

    private val _uiState = MutableStateFlow(
        PlaybackUiState(
            activeOutputRoute = OutputRouteHelper.activeOutputLabel(appContext),
        ),
    )
    val uiState: StateFlow<PlaybackUiState> = _uiState.asStateFlow()

    private val eventSink = object : FfiEventSink {
        override fun onPlaybackTelemetry(telemetry: FfiTelemetry) {
            _uiState.update { PlaybackStateReducer.applyTelemetry(it, telemetry) }
        }

        override fun onNowPlayingChanged(track: FfiTrack?) {
            _uiState.update { PlaybackStateReducer.applyNowPlaying(it, track) }
        }

        override fun onQueueChanged(upNext: List<FfiTrack>) {
            _uiState.update { it.copy(upNext = upNext) }
        }

        override fun onLibraryChanged() {
            scope.launch { refreshLibraryInternal() }
        }

        override fun onToast(message: String) {
            _uiState.update { it.copy(statusMessage = message) }
        }
    }

    /**
     * Initialize Rust SQLite + library lazily without opening an Android audio stream yet.
     */
    fun bootstrapLibrary() {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            try {
                handle.seedDemoIfEmpty()
                refreshLibraryInternal()
                _uiState.update { it.copy(isReady = true) }
            } catch (err: Exception) {
                _uiState.update { it.copy(statusMessage = "Init failed: ${err.message}") }
            }
        }
    }

    fun getOrCreateCore(): FerricMobileCore? {
        core?.let { return it }
        return synchronized(coreLock) {
            core?.let { return it }
            try {
                NativeBridge.ensureInitialized(appContext)
                val dataDir = File(appContext.filesDir, "ferric").apply { mkdirs() }
                val cacheDir = File(appContext.cacheDir, "ferric").apply { mkdirs() }
                val created = FerricMobileCore(
                    dataDir = dataDir.absolutePath,
                    cacheDir = cacheDir.absolutePath,
                    cipher = KeystoreVault(),
                    contentOpener = AndroidContentOpener(appContext.contentResolver),
                    sink = eventSink,
                )
                core = created
                created
            } catch (err: Exception) {
                _uiState.update { it.copy(statusMessage = "Rust core failed: ${err.message}") }
                null
            }
        }
    }

    fun ensureAudioStartedSync(): Boolean {
        val handle = getOrCreateCore() ?: return false
        return try {
            handle.ensureAudioStarted()
            true
        } catch (err: Exception) {
            _uiState.update { it.copy(statusMessage = "Audio start failed: ${err.message}") }
            false
        }
    }

    fun playTrack(trackId: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.playTrack(trackId) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun togglePlayPause() {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            val state = _uiState.value
            if (state.currentTrack == null && state.tracks.isNotEmpty()) {
                runCatching { handle.playTrack(state.tracks.first().id) }
            } else {
                runCatching { handle.togglePlayPause() }
            }
        }
    }

    fun play() {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            val state = _uiState.value
            if (state.currentTrack == null && state.tracks.isNotEmpty()) {
                runCatching { handle.playTrack(state.tracks.first().id) }
            } else {
                runCatching { handle.play() }
            }
        }
    }

    fun pause() {
        scope.launch {
            core?.pause()
        }
    }

    fun next() {
        scope.launch {
            runCatching { getOrCreateCore()?.next() }
        }
    }

    fun prev() {
        scope.launch {
            runCatching { getOrCreateCore()?.prev() }
        }
    }

    fun seekTo(positionMs: Long) {
        val clamped = positionMs.coerceAtLeast(0L)
        _uiState.update { it.copy(positionMs = clamped) }
        scope.launch {
            core?.seekMs(clamped.toULong())
        }
    }

    fun setDucking(duck: Boolean) {
        scope.launch {
            core?.setDucking(duck)
        }
    }

    fun toggleShuffle() {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            val enabled = handle.toggleShuffle()
            _uiState.update { it.copy(shuffle = enabled) }
        }
    }

    fun cycleRepeat() {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            val nextMode = handle.cycleRepeat()
            _uiState.update { it.copy(repeatMode = nextMode) }
        }
    }

    fun refreshOutputRoute() {
        val label = OutputRouteHelper.activeOutputLabel(appContext)
        _uiState.update { it.copy(activeOutputRoute = label) }
    }

    fun clearStatusMessage() {
        _uiState.update { it.copy(statusMessage = null) }
    }

    fun onTrimMemory() {
        scope.launch {
            core?.trimMemory()
        }
    }

    fun shutdown() {
        synchronized(coreLock) {
            core?.shutdown()
            core = null
        }
    }

    private fun refreshLibraryInternal() {
        val handle = core ?: return
        val songs = runCatching { handle.listSongs("") }.getOrDefault(emptyList())
        val current = handle.getCurrentTrack()
        val upNext = handle.getUpNext(50u)
        _uiState.update {
            it.copy(
                tracks = songs,
                currentTrack = current ?: it.currentTrack,
                upNext = upNext,
            )
        }
    }
}
