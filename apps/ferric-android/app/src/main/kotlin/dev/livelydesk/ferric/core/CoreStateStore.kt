package dev.livelydesk.ferric.core

import android.content.Context
import android.net.Uri
import android.os.Build
import androidx.compose.runtime.Immutable
import coil3.SingletonImageLoader
import dev.livelydesk.ferric.auth.SilentSpotifyRefresher
import dev.livelydesk.ferric.auth.WebCookieCleaner
import dev.livelydesk.ferric.connect.ConnectNsdHelper
import dev.livelydesk.ferric.local.LocalMediaScanner
import dev.livelydesk.ferric.playback.OutputRouteHelper
import dev.livelydesk.ferric.playback.PlaybackFocusCoordinator
import dev.livelydesk.ferric.ui.theme.CoverShadeColors
import dev.livelydesk.ferric.ui.theme.CoverShadeExtractor
import dev.livelydesk.ferric.widget.FerricWidgetUpdater
import java.io.File
import java.util.TimeZone
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.ferric_mobile_ffi.FerricMobileCore
import uniffi.ferric_mobile_ffi.FfiAccountStatus
import uniffi.ferric_mobile_ffi.FfiAlbumSummary
import uniffi.ferric_mobile_ffi.FfiArtistSummary
import uniffi.ferric_mobile_ffi.FfiAutoEqProfile
import uniffi.ferric_mobile_ffi.FfiCardKind
import uniffi.ferric_mobile_ffi.FfiCollectionDetail
import uniffi.ferric_mobile_ffi.FfiConnectState
import uniffi.ferric_mobile_ffi.FfiDetailSort
import uniffi.ferric_mobile_ffi.FfiDeviceKind
import uniffi.ferric_mobile_ffi.FfiDspSettings
import uniffi.ferric_mobile_ffi.FfiEqConfig
import uniffi.ferric_mobile_ffi.FfiEventSink
import uniffi.ferric_mobile_ffi.FfiHomeState
import uniffi.ferric_mobile_ffi.FfiListeningStats
import uniffi.ferric_mobile_ffi.FfiLyricsState
import uniffi.ferric_mobile_ffi.FfiPlaylist
import uniffi.ferric_mobile_ffi.FfiPlaylistSort
import uniffi.ferric_mobile_ffi.FfiPlaylistSourceFilter
import uniffi.ferric_mobile_ffi.FfiQrCode
import uniffi.ferric_mobile_ffi.FfiRepeatMode
import uniffi.ferric_mobile_ffi.FfiSearchResults
import uniffi.ferric_mobile_ffi.FfiStorageStats
import uniffi.ferric_mobile_ffi.FfiTelemetry
import uniffi.ferric_mobile_ffi.FfiThemeConfig
import uniffi.ferric_mobile_ffi.FfiTrack
import uniffi.ferric_mobile_ffi.FfiTrackSource
import uniffi.ferric_mobile_ffi.FfiVideoStreamsState

/**
 * High-frequency (5 Hz) playback progress stream separated from [PlaybackUiState] so only
 * seekbars, timestamps, lyrics highlighter, and spectrum meters recompose during playback.
 */
@Immutable
data class PlaybackProgress(
    val positionMs: Long = 0L,
    val durationMs: Long = 0L,
    val spectrumBins: List<Int> = List(8) { 0 },
)

data class PlaybackUiState(
    val tracks: List<FfiTrack> = emptyList(),
    val likedTracks: List<FfiTrack> = emptyList(),
    val recentTracks: List<FfiTrack> = emptyList(),
    val localTracks: List<FfiTrack> = emptyList(),
    val offlineTracks: List<FfiTrack> = emptyList(),
    val offlineTrackIds: Set<String> = emptySet(),
    val downloadProgress: Map<String, Int> = emptyMap(),
    val storageStats: FfiStorageStats? = null,
    val listeningStats: FfiListeningStats? = null,
    val playlists: List<FfiPlaylist> = emptyList(),
    val albums: List<FfiAlbumSummary> = emptyList(),
    val artists: List<FfiArtistSummary> = emptyList(),
    val playlistSourceFilter: FfiPlaylistSourceFilter = FfiPlaylistSourceFilter.ALL,
    val playlistSort: FfiPlaylistSort = FfiPlaylistSort.NEWEST,
    val homeState: FfiHomeState = FfiHomeState(
        chips = emptyList(),
        selectedMoodIdx = null,
        quickPicks = emptyList(),
        stations = emptyList(),
        shelves = emptyList(),
        isLoading = false,
        isLoadingMore = false,
    ),
    val searchResults: FfiSearchResults = FfiSearchResults(
        query = "",
        localTracks = emptyList(),
        onlineTracks = emptyList(),
        onlineCards = emptyList(),
        isLoading = false,
    ),
    val recentSearches: List<String> = emptyList(),
    val activeDetail: FfiCollectionDetail? = null,
    val originalDetailTracks: List<FfiTrack> = emptyList(),
    val detailSort: FfiDetailSort = FfiDetailSort.ORIGINAL,
    val accountStatus: FfiAccountStatus = FfiAccountStatus(
        spotifyConnected = false,
        spotifyUserName = "",
        spotifyUserId = "",
        spotifyPlaylistCount = 0u,
        spotifyLikedCount = 0u,
        youtubeConnected = false,
        youtubeChannelName = "",
        youtubeAccountId = "",
        youtubePlaylistCount = 0u,
        subsonicConnected = false,
        subsonicUrl = "",
        subsonicUsername = "",
    ),
    val themeConfig: FfiThemeConfig? = null,
    val coverShade: CoverShadeColors = CoverShadeColors(),
    val eqConfig: FfiEqConfig? = null,
    val eqPresets: List<String> = emptyList(),
    val autoEqProfiles: List<FfiAutoEqProfile> = emptyList(),
    val dspSettings: FfiDspSettings? = null,
    val lyricsState: FfiLyricsState = FfiLyricsState(
        trackId = "",
        lines = emptyList(),
        sourceName = "",
        isSynced = false,
        offsetMs = 0L,
        isLoading = false,
    ),
    val videoStreams: FfiVideoStreamsState = FfiVideoStreamsState(
        trackId = "",
        videoId = "",
        isOfficialMv = false,
        options = emptyList(),
        isLoading = false,
    ),
    val currentTrack: FfiTrack? = null,
    val upNext: List<FfiTrack> = emptyList(),
    val isExtendingQueue: Boolean = false,
    val miniPlayerDismissed: Boolean = false,
    val isPlaying: Boolean = false,
    val positionMs: Long = 0L,
    val durationMs: Long = 0L,
    val sampleRate: Int = 0,
    val outputRate: Int = 0,
    val isCrossfading: Boolean = false,
    val spectrumBins: List<Int> = List(8) { 0 },
    val seekSyncNonce: Long = 0L,
    val sleepTimerRemainingSecs: Long = 0L,
    val shuffle: Boolean = false,
    val repeatMode: FfiRepeatMode = FfiRepeatMode.OFF,
    val localVolume: Float = 1.0f,
    val activeOutputRoute: String = "Phone Speaker",
    val connectState: FfiConnectState = FfiConnectState(
        localDeviceId = "",
        localDeviceName = "This phone",
        localDeviceKind = FfiDeviceKind.PHONE,
        localIp = "127.0.0.1",
        localTcpPort = 0u,
        roomTagsHex = ",",
        activeSpeakerId = "",
        isLocalActiveSpeaker = true,
        isAccountReady = false,
        relayConnected = false,
        peers = emptyList(),
        activeRemoteQueue = emptyList(),
        activeRemoteQueueIndex = 0u,
    ),
    val connectQrCode: FfiQrCode? = null,
    val statusMessage: String? = null,
    val isReady: Boolean = false,
)

/**
 * Pure state reducer kept JVM-testable without native `.so` loading.
 */
object PlaybackStateReducer {
    fun applyProgress(prev: PlaybackProgress, t: FfiTelemetry): PlaybackProgress {
        val reportedDur = t.durationMs.toLong().coerceAtLeast(0L)
        val safeDur = if (reportedDur > 0L) reportedDur else prev.durationMs.coerceAtLeast(0L)
        val rawPos = t.positionMs.toLong().coerceAtLeast(0L)
        val safePos = if (safeDur > 0L) rawPos.coerceIn(0L, safeDur) else rawPos
        return PlaybackProgress(
            positionMs = safePos,
            durationMs = safeDur,
            spectrumBins = if (t.spectrumBins.size == 8) {
                t.spectrumBins.map { it.toInt().coerceIn(0, 1000) }
            } else {
                prev.spectrumBins
            },
        )
    }

    fun applyLowFreqTelemetry(state: PlaybackUiState, t: FfiTelemetry): PlaybackUiState {
        val nextDur = t.durationMs.toLong().coerceAtLeast(0L)
        val nextSr = t.sampleRate.toInt()
        val nextOr = t.outputRate.toInt()
        if (state.isPlaying == t.isPlaying &&
            state.durationMs == nextDur &&
            state.sampleRate == nextSr &&
            state.outputRate == nextOr &&
            state.isCrossfading == t.isCrossfading
        ) {
            return state
        }
        return state.copy(
            isPlaying = t.isPlaying,
            durationMs = nextDur,
            sampleRate = nextSr,
            outputRate = nextOr,
            isCrossfading = t.isCrossfading,
        )
    }

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

    @Volatile
    private var lastExtractedArtUrl: String? = null

    private var localSearchJob: Job? = null
    private var onlineSearchJob: Job? = null
    private var autoEqSearchJob: Job? = null
    private var sleepTimerPollJob: Job? = null
    private var remoteProgressJob: Job? = null

    private val _uiState = MutableStateFlow(
        PlaybackUiState(
            activeOutputRoute = OutputRouteHelper.activeOutputLabel(appContext),
        ),
    )
    val uiState: StateFlow<PlaybackUiState> = _uiState.asStateFlow()

    private val _playbackProgress = MutableStateFlow(PlaybackProgress())
    val playbackProgress: StateFlow<PlaybackProgress> = _playbackProgress.asStateFlow()

    init {
        PlaybackFocusCoordinator.ensureInitialized(appContext, this)
    }

    private val silentSpotifyRefresher = SilentSpotifyRefresher(
        appContext = appContext,
        onRefreshed = { token, cookie ->
            completeSpotifyLogin(accessToken = token, cookie = cookie, triggerSync = true)
        },
        onFailed = {
            _uiState.update {
                it.copy(statusMessage = "Spotify session expired. Open You > Accounts to sign in again.")
            }
        },
    )

    private val nsdHelper = ConnectNsdHelper(appContext) { devId, devName, kind, ip, port, tags ->
        scope.launch {
            core?.notifyMdnsPeer(
                deviceId = devId,
                deviceName = devName,
                deviceKind = kind,
                ip = ip,
                tcpPort = port.toUShort(),
                roomTagsHex = tags,
            )
        }
    }

    private fun stableDeviceId(): String {
        val prefs = appContext.getSharedPreferences("ferric_device", Context.MODE_PRIVATE)
        val existing = prefs.getString("device_id", null)
        if (!existing.isNullOrBlank()) return existing
        val generated = "android_${UUID.randomUUID().toString().replace("-", "").take(12)}"
        prefs.edit().putString("device_id", generated).apply()
        return generated
    }

    private fun friendlyDeviceName(): String {
        val model = Build.MODEL?.trim().orEmpty()
        val brand = Build.MANUFACTURER?.trim().orEmpty()
        return when {
            model.isBlank() -> "Ferric Android"
            brand.isNotBlank() && !model.startsWith(brand, ignoreCase = true) -> "$brand $model"
            else -> model
        }.take(48)
    }

    private val eventSink = object : FfiEventSink {
        override fun onPlaybackTelemetry(telemetry: FfiTelemetry) {
            val prevPlaying = _uiState.value.isPlaying
            // High-frequency progress goes exclusively to playbackProgress (zero root recomposition).
            _playbackProgress.update { PlaybackStateReducer.applyProgress(it, telemetry) }
            // Low-frequency state only triggers _uiState emission when discrete values actually change.
            _uiState.update { PlaybackStateReducer.applyLowFreqTelemetry(it, telemetry) }
            if (prevPlaying != telemetry.isPlaying) {
                FerricWidgetUpdater.updateAll(appContext, _uiState.value)
            }
        }

        override fun onNowPlayingChanged(track: FfiTrack?) {
            val prevId = _uiState.value.currentTrack?.id
            _uiState.update {
                val next = PlaybackStateReducer.applyNowPlaying(it, track)
                if (track != null) next.copy(miniPlayerDismissed = false) else next
            }
            if (track == null) {
                _playbackProgress.update { PlaybackProgress() }
            } else if (track.id != prevId) {
                _playbackProgress.update {
                    it.copy(
                        positionMs = 0L,
                        durationMs = track.durationMs.toLong().coerceAtLeast(0L),
                    )
                }
            }
            FerricWidgetUpdater.updateAll(appContext, _uiState.value)
            if (track != null && track.id != prevId) {
                extractCoverShadeAsync(track.thumbnailUrl)
                _uiState.update {
                    it.copy(
                        videoStreams = FfiVideoStreamsState(
                            trackId = track.id,
                            videoId = track.youtubeVideoId.orEmpty(),
                            isOfficialMv = false,
                            options = emptyList(),
                            isLoading = false,
                        ),
                    )
                }
            }
        }

        override fun onQueueChanged(upNext: List<FfiTrack>) {
            _uiState.update { it.copy(upNext = upNext, isExtendingQueue = false) }
        }

        override fun onLibraryChanged() {
            scope.launch { refreshLibraryInternal() }
        }

        override fun onHomeUpdated(home: FfiHomeState) {
            _uiState.update { it.copy(homeState = home) }
        }

        override fun onSearchResults(results: FfiSearchResults) {
            val recent = runCatching { core?.recentSearches().orEmpty() }.getOrDefault(emptyList())
            _uiState.update { it.copy(searchResults = results, recentSearches = recent) }
        }

        override fun onDetailLoaded(detail: FfiCollectionDetail) {
            _uiState.update {
                it.copy(
                    activeDetail = detail,
                    originalDetailTracks = detail.tracks,
                    detailSort = FfiDetailSort.ORIGINAL,
                )
            }
        }

        override fun onAccountsChanged(status: FfiAccountStatus) {
            _uiState.update { it.copy(accountStatus = status) }
        }

        override fun onLyricsLoaded(lyrics: FfiLyricsState) {
            _uiState.update { it.copy(lyricsState = lyrics) }
        }

        override fun onVideoStreamsLoaded(video: FfiVideoStreamsState) {
            _uiState.update { it.copy(videoStreams = video) }
        }

        override fun onConnectStateChanged(state: FfiConnectState) {
            _uiState.update { prev ->
                val activeRemote = if (!state.isLocalActiveSpeaker) {
                    state.peers.firstOrNull { it.connected && it.deviceId == state.activeSpeakerId }
                } else {
                    null
                }
                if (activeRemote != null && activeRemote.nowPlayingTitle.isNotBlank()) {
                    extractCoverShadeAsync(activeRemote.nowPlayingThumbnailUrl)
                    prev.copy(
                        connectState = state,
                        miniPlayerDismissed = false,
                    )
                } else {
                    prev.copy(connectState = state)
                }
            }
            updateRemoteProgressTicker(state)
            if (state.localTcpPort.toInt() >= 1024) {
                nsdHelper.syncRegistration(
                    deviceId = state.localDeviceId,
                    deviceName = state.localDeviceName,
                    tcpPort = state.localTcpPort.toInt(),
                    roomTagsHex = state.roomTagsHex,
                )
            }
        }

        override fun onDownloadProgress(trackId: String, percent: UByte) {
            val pct = percent.toInt().coerceIn(0, 100)
            _uiState.update { prev ->
                val nextMap = prev.downloadProgress.toMutableMap()
                if (pct >= 100) nextMap.remove(trackId) else nextMap[trackId] = pct
                prev.copy(downloadProgress = nextMap)
            }
        }

        override fun onSpotifyAuthExpired() {
            silentSpotifyRefresher.triggerRefresh()
        }

        override fun onToast(message: String) {
            _uiState.update { it.copy(statusMessage = message) }
        }
    }

    private fun extractCoverShadeAsync(artUrl: String?) {
        if (artUrl == lastExtractedArtUrl) return
        lastExtractedArtUrl = artUrl
        scope.launch {
            val colors = CoverShadeExtractor.extract(appContext, artUrl)
            _uiState.update { it.copy(coverShade = colors) }
        }
    }

    fun bootstrapLibrary() {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            try {
                handle.seedDemoIfEmpty()
                refreshLibraryInternal()
                val accounts = handle.getAccountStatus()
                val recentSearches = handle.recentSearches()
                var theme = handle.getTheme()
                // On fresh Android install (or first launch of the Liquid Glass overhaul), default
                // to Liquid Glass once while preserving any explicit user preset choice thereafter.
                val prefs = appContext.getSharedPreferences("ferric_device", Context.MODE_PRIVATE)
                if (!prefs.getBoolean("liquid_glass_v2_default_applied", false)) {
                    prefs.edit().putBoolean("liquid_glass_v2_default_applied", true).apply()
                    if (theme.name.equals("AMOLED Black", ignoreCase = true) ||
                        theme.name.equals("Liquid Glass", ignoreCase = true)
                    ) {
                        theme = runCatching { handle.applyThemePreset("Liquid Glass") }.getOrDefault(theme)
                    }
                }
                val userVol = if (prefs.contains("user_local_volume")) {
                    prefs.getFloat("user_local_volume", 1.0f).coerceIn(0f, 1.5f)
                } else {
                    prefs.edit().putFloat("user_local_volume", 1.0f).apply()
                    1.0f
                }
                runCatching { handle.setVolume(userVol) }
                val eq = handle.getEq()
                val eqPresets = handle.listEqPresets()
                val autoEq = handle.searchAutoeq("")
                val dsp = handle.getDsp()
                val connect = handle.getConnectState()
                val tzOffsetSecs = (TimeZone.getDefault().getOffset(System.currentTimeMillis()) / 1000L)
                val stats = runCatching { handle.getListeningStats(tzOffsetSecs) }.getOrNull()
                _uiState.update {
                    it.copy(
                        accountStatus = accounts,
                        recentSearches = recentSearches,
                        themeConfig = theme,
                        eqConfig = eq,
                        eqPresets = eqPresets,
                        autoEqProfiles = autoEq,
                        dspSettings = dsp,
                        localVolume = userVol,
                        connectState = connect,
                        listeningStats = stats ?: it.listeningStats,
                        isReady = true,
                    )
                }
                if (connect.localTcpPort.toInt() >= 1024) {
                    nsdHelper.syncRegistration(
                        deviceId = connect.localDeviceId,
                        deviceName = connect.localDeviceName,
                        tcpPort = connect.localTcpPort.toInt(),
                        roomTagsHex = connect.roomTagsHex,
                    )
                }
                handle.loadHome(forceRefresh = false, moodIdx = null)
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
                    deviceId = stableDeviceId(),
                    deviceName = friendlyDeviceName(),
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

    fun setAppForeground(foreground: Boolean) {
        nsdHelper.setAppForeground(foreground)
    }

    // --- Playback & Queue ---

    fun playTrack(trackId: String) {
        if (!PlaybackFocusCoordinator.requestFocusForPlayback()) return
        _uiState.update { it.copy(miniPlayerDismissed = false) }
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.playTrack(trackId) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun playQueue(tracks: List<FfiTrack>, startIndex: Int, context: String) {
        if (tracks.isEmpty()) return
        if (!PlaybackFocusCoordinator.requestFocusForPlayback()) return
        _uiState.update { it.copy(miniPlayerDismissed = false) }
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            val idx = startIndex.coerceIn(0, tracks.lastIndex).toUInt()
            runCatching { handle.playQueue(tracks, idx, context) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun shuffleQueue(tracks: List<FfiTrack>, context: String) {
        if (tracks.isEmpty()) return
        if (!PlaybackFocusCoordinator.requestFocusForPlayback()) return
        _uiState.update { it.copy(miniPlayerDismissed = false) }
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            val shuffled = tracks.shuffled()
            runCatching { handle.playQueue(shuffled, 0u, context) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun stopAndDismissMiniPlayer() {
        PlaybackFocusCoordinator.onExplicitPause()
        _uiState.update {
            it.copy(
                miniPlayerDismissed = true,
                isPlaying = false,
                currentTrack = null,
                upNext = emptyList(),
                isExtendingQueue = false,
            )
        }
        _playbackProgress.update { PlaybackProgress() }
        FerricWidgetUpdater.updateAll(appContext, _uiState.value)
        scope.launch {
            runCatching { getOrCreateCore()?.stopPlayback() }
        }
    }

    fun extendQueueUpNext() {
        val snap = _uiState.value
        if (snap.isExtendingQueue || snap.currentTrack == null) return
        _uiState.update { it.copy(isExtendingQueue = true) }
        scope.launch {
            val handle = getOrCreateCore()
            val started = handle?.let {
                runCatching { it.extendQueueUpNext() }.getOrDefault(false)
            } ?: false
            if (!started) {
                _uiState.update { it.copy(isExtendingQueue = false) }
            }
        }
    }

    fun togglePlayPause() {
        val state = _uiState.value
        val remotePlaying = if (!state.connectState.isLocalActiveSpeaker) {
            state.connectState.peers.firstOrNull {
                it.connected && it.deviceId == state.connectState.activeSpeakerId
            }?.isPlaying ?: state.isPlaying
        } else {
            state.isPlaying
        }
        if (remotePlaying) {
            PlaybackFocusCoordinator.onExplicitPause()
        } else if (state.connectState.isLocalActiveSpeaker) {
            if (!PlaybackFocusCoordinator.requestFocusForPlayback()) return
        }
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            if (state.currentTrack == null && state.tracks.isNotEmpty() && state.connectState.isLocalActiveSpeaker) {
                runCatching { handle.playTrack(state.tracks.first().id) }
            } else {
                runCatching { handle.togglePlayPause() }
            }
        }
    }

    fun play() {
        val isLocal = _uiState.value.connectState.isLocalActiveSpeaker
        if (isLocal && !PlaybackFocusCoordinator.requestFocusForPlayback()) return
        playInternal()
    }

    fun pause() {
        PlaybackFocusCoordinator.onExplicitPause()
        pauseInternal()
    }

    internal fun playInternal() {
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

    internal fun pauseInternal() {
        scope.launch { core?.pause() }
    }

    fun next() {
        if (_uiState.value.connectState.isLocalActiveSpeaker) {
            PlaybackFocusCoordinator.requestFocusForPlayback()
        }
        scope.launch { runCatching { getOrCreateCore()?.next() } }
    }

    fun prev() {
        if (_uiState.value.connectState.isLocalActiveSpeaker) {
            PlaybackFocusCoordinator.requestFocusForPlayback()
        }
        scope.launch { runCatching { getOrCreateCore()?.prev() } }
    }

    fun seekTo(positionMs: Long) {
        val clamped = positionMs.coerceAtLeast(0L)
        _playbackProgress.update { it.copy(positionMs = clamped) }
        _uiState.update { it.copy(seekSyncNonce = it.seekSyncNonce + 1L) }
        scope.launch { core?.seekMs(clamped.toULong()) }
    }

    fun setDucking(duck: Boolean) {
        scope.launch { core?.setDucking(duck) }
    }

    fun setSleepTimerMinutes(minutes: Int) {
        val clamped = minutes.coerceIn(0, 720)
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            handle.setSleepTimerMinutes(clamped.toUInt())
            val rem = handle.getSleepTimerRemainingSecs().toLong().coerceAtLeast(0L)
            _uiState.update { it.copy(sleepTimerRemainingSecs = rem) }
            restartSleepTimerPoller(rem > 0L)
        }
    }

    private fun restartSleepTimerPoller(active: Boolean) {
        sleepTimerPollJob?.cancel()
        sleepTimerPollJob = null
        if (!active) return
        sleepTimerPollJob = scope.launch {
            while (true) {
                delay(1_000L)
                val rem = runCatching { core?.getSleepTimerRemainingSecs()?.toLong() ?: 0L }.getOrDefault(0L)
                _uiState.update { it.copy(sleepTimerRemainingSecs = rem) }
                if (rem <= 0L) break
            }
        }
    }

    private fun updateRemoteProgressTicker(connectState: FfiConnectState) {
        remoteProgressJob?.cancel()
        remoteProgressJob = null
        if (connectState.isLocalActiveSpeaker) return
        val peer = connectState.peers.firstOrNull {
            it.connected && it.deviceId == connectState.activeSpeakerId
        } ?: return

        val durMs = peer.durationMs.toLong().coerceAtLeast(0L)
        val anchorWall = peer.anchorUtcMs
        val driftAtReceive = if (peer.isPlaying && anchorWall > 0L) {
            (System.currentTimeMillis() - anchorWall).coerceIn(0L, 120_000L)
        } else {
            0L
        }
        val basePos = (peer.positionMs.toLong() + driftAtReceive).let { raw ->
            if (durMs > 0L) raw.coerceIn(0L, durMs) else raw.coerceAtLeast(0L)
        }
        _playbackProgress.update { prev ->
            prev.copy(positionMs = basePos, durationMs = durMs)
        }
        if (!peer.isPlaying) return

        val startMonoMs = android.os.SystemClock.elapsedRealtime()
        remoteProgressJob = scope.launch {
            while (true) {
                delay(500L)
                val latest = _uiState.value.connectState
                if (latest.isLocalActiveSpeaker) break
                val livePeer = latest.peers.firstOrNull {
                    it.connected && it.deviceId == latest.activeSpeakerId
                } ?: break
                if (!livePeer.isPlaying) break
                val elapsed = (android.os.SystemClock.elapsedRealtime() - startMonoMs).coerceAtLeast(0L)
                val nextPos = (basePos + elapsed).let { p ->
                    if (durMs > 0L) p.coerceIn(0L, durMs) else p
                }
                _playbackProgress.update { prev ->
                    prev.copy(positionMs = nextPos, durationMs = durMs)
                }
            }
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

    fun playNext(track: FfiTrack) {
        scope.launch { getOrCreateCore()?.playNext(track) }
    }

    fun appendToQueue(track: FfiTrack) {
        scope.launch { getOrCreateCore()?.appendToQueue(track) }
    }

    fun jumpToUpcoming(offset: Int) {
        if (offset < 0) return
        if (_uiState.value.connectState.isLocalActiveSpeaker) {
            PlaybackFocusCoordinator.requestFocusForPlayback()
        }
        scope.launch { runCatching { getOrCreateCore()?.jumpToUpcoming(offset.toUInt()) } }
    }

    fun moveQueueItemUp(offset: Int) {
        if (offset <= 0) return
        scope.launch { getOrCreateCore()?.moveQueueItemUp(offset.toUInt()) }
    }

    fun moveQueueItemDown(offset: Int) {
        if (offset < 0) return
        scope.launch { getOrCreateCore()?.moveQueueItemDown(offset.toUInt()) }
    }

    fun removeQueueItem(offset: Int) {
        if (offset < 0) return
        scope.launch { getOrCreateCore()?.removeQueueItem(offset.toUInt()) }
    }

    fun clearUpNext() {
        scope.launch { getOrCreateCore()?.clearUpNext() }
    }

    fun saveQueueAsPlaylist(name: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.saveQueueAsPlaylist(name) }
                .onSuccess { pl -> _uiState.update { it.copy(statusMessage = "Saved queue as \"${pl.name}\"") } }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun buildQueueShareText(onReady: (String) -> Unit) {
        scope.launch {
            val text = getOrCreateCore()?.copyQueueTracklist().orEmpty()
            if (text.isNotEmpty()) {
                withContext(Dispatchers.Main) { onReady(text) }
            }
        }
    }

    // --- M3 Synced Lyrics, Video Clip, EQ / AutoEq / APO & Theme Studio ---

    fun loadLyricsForCurrent() {
        val track = _uiState.value.currentTrack ?: return
        scope.launch { getOrCreateCore()?.loadLyrics(track) }
    }

    fun nudgeLyricsOffsetMs(deltaMs: Long) {
        scope.launch { getOrCreateCore()?.nudgeLyricsOffsetMs(deltaMs) }
    }

    fun resolveVideoForCurrent() {
        val track = _uiState.value.currentTrack ?: return
        scope.launch { getOrCreateCore()?.resolveVideoStreams(track) }
    }

    fun updateEq(eq: FfiEqConfig) {
        _uiState.update { it.copy(eqConfig = eq) }
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.setEq(eq) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun applyEqPreset(name: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.applyEqPreset(name) }
                .onSuccess { cfg -> _uiState.update { it.copy(eqConfig = cfg) } }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun searchAutoEq(query: String) {
        autoEqSearchJob?.cancel()
        autoEqSearchJob = scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            val matches = handle.searchAutoeq(query)
            _uiState.update { it.copy(autoEqProfiles = matches) }
        }
    }

    fun applyAutoEqProfile(profileName: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.applyAutoeqProfile(profileName) }
                .onSuccess { cfg -> _uiState.update { it.copy(eqConfig = cfg) } }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun importEqApo(apoText: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.importEqApo(apoText) }
                .onSuccess { cfg -> _uiState.update { it.copy(eqConfig = cfg) } }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun exportEqApo(onReady: (String) -> Unit) {
        scope.launch {
            val text = getOrCreateCore()?.exportEqApo().orEmpty()
            if (text.isNotEmpty()) {
                withContext(Dispatchers.Main) { onReady(text) }
            }
        }
    }

    fun updateDsp(dsp: FfiDspSettings) {
        _uiState.update { it.copy(dspSettings = dsp) }
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.setDsp(dsp) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun previewTheme(theme: FfiThemeConfig) {
        _uiState.update { it.copy(themeConfig = theme) }
    }

    fun updateTheme(theme: FfiThemeConfig) {
        _uiState.update { it.copy(themeConfig = theme) }
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.setTheme(theme) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun applyThemePreset(name: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.applyThemePreset(name) }
                .onSuccess { cfg -> _uiState.update { it.copy(themeConfig = cfg) } }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun exportThemeJson(onReady: (String) -> Unit) {
        scope.launch {
            val json = runCatching { getOrCreateCore()?.exportThemeJson() }.getOrNull().orEmpty()
            if (json.isNotEmpty()) {
                withContext(Dispatchers.Main) { onReady(json) }
            }
        }
    }

    fun importThemeJson(rawJson: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.importThemeJson(rawJson) }
                .onSuccess { cfg -> _uiState.update { it.copy(themeConfig = cfg) } }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    // --- Home & Search ---

    fun loadHome(forceRefresh: Boolean, moodIdx: Int? = null) {
        scope.launch { getOrCreateCore()?.loadHome(forceRefresh, moodIdx?.toUInt()) }
    }

    fun loadHomeMore() {
        val snap = _uiState.value.homeState
        if (snap.isLoading || snap.isLoadingMore) return
        scope.launch { getOrCreateCore()?.loadHomeMore() }
    }

    fun openStation(stationId: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            _uiState.update { it.copy(statusMessage = "Loading station...") }
            runCatching { handle.openStation(stationId) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun openBrowseDetail(browseId: String, kind: FfiCardKind) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            _uiState.update { it.copy(statusMessage = "Loading collection...") }
            runCatching { handle.openBrowseDetail(browseId, kind) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun instantSearchLocal(query: String) {
        localSearchJob?.cancel()
        localSearchJob = scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            val hits = runCatching { handle.instantSearchLocal(query) }.getOrDefault(emptyList())
            _uiState.update { st ->
                st.copy(
                    searchResults = st.searchResults.copy(
                        query = query,
                        localTracks = hits,
                    ),
                )
            }
        }
    }

    fun searchOnline(query: String) {
        onlineSearchJob?.cancel()
        onlineSearchJob = scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.searchOnline(query) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun browseGenre(genre: String) {
        onlineSearchJob?.cancel()
        onlineSearchJob = scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.browseGenre(genre) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun clearRecentSearches() {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.clearRecentSearches() }
            _uiState.update { it.copy(recentSearches = emptyList()) }
        }
    }

    fun importPublicUrl(url: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            _uiState.update { it.copy(statusMessage = "Importing URL...") }
            runCatching { handle.importPublicUrl(url) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    // --- Library, Local Lossless & Collection Detail ---

    fun setPlaylistFilter(source: FfiPlaylistSourceFilter) {
        _uiState.update { it.copy(playlistSourceFilter = source) }
        scope.launch { refreshLibraryInternal() }
    }

    fun cyclePlaylistSort() {
        val next = when (_uiState.value.playlistSort) {
            FfiPlaylistSort.NEWEST -> FfiPlaylistSort.OLDEST
            FfiPlaylistSort.OLDEST -> FfiPlaylistSort.NAME_AZ
            FfiPlaylistSort.NAME_AZ -> FfiPlaylistSort.NEWEST
        }
        _uiState.update { it.copy(playlistSort = next) }
        scope.launch { refreshLibraryInternal() }
    }

    fun openPlaylistDetail(playlistId: String, sort: FfiDetailSort = FfiDetailSort.ORIGINAL) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.getPlaylistDetail(playlistId, sort) }
                .onSuccess { detail ->
                    _uiState.update {
                        it.copy(
                            activeDetail = detail,
                            originalDetailTracks = detail.tracks,
                            detailSort = sort,
                        )
                    }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun openAlbumDetail(albumName: String, artistName: String, sort: FfiDetailSort = FfiDetailSort.ORIGINAL) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.getAlbumDetail(albumName, artistName, sort) }
                .onSuccess { detail ->
                    _uiState.update {
                        it.copy(
                            activeDetail = detail,
                            originalDetailTracks = detail.tracks,
                            detailSort = sort,
                        )
                    }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun openArtistDetail(artistName: String, sort: FfiDetailSort = FfiDetailSort.ORIGINAL) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.getArtistDetail(artistName, sort) }
                .onSuccess { detail ->
                    _uiState.update {
                        it.copy(
                            activeDetail = detail,
                            originalDetailTracks = detail.tracks,
                            detailSort = sort,
                        )
                    }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun openLikedDetail(sort: FfiDetailSort = FfiDetailSort.ORIGINAL) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.getLikedDetail(sort) }
                .onSuccess { detail ->
                    _uiState.update {
                        it.copy(
                            activeDetail = detail,
                            originalDetailTracks = detail.tracks,
                            detailSort = sort,
                        )
                    }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun openLocalCollectionDetail(title: String, subtitle: String, tracks: List<FfiTrack>) {
        val detail = FfiCollectionDetail(
            id = "collection:${title.lowercase()}",
            title = title,
            subtitle = subtitle,
            description = "",
            kind = FfiCardKind.PLAYLIST,
            source = FfiTrackSource.LOCAL,
            thumbnailUrl = tracks.firstOrNull()?.thumbnailUrl,
            shareUrl = null,
            canSyncToAccount = false,
            isEditablePlaylist = false,
            tracks = tracks,
        )
        _uiState.update {
            it.copy(
                activeDetail = detail,
                originalDetailTracks = tracks,
                detailSort = FfiDetailSort.ORIGINAL,
            )
        }
    }

    fun closeDetail() {
        _uiState.update { it.copy(activeDetail = null, originalDetailTracks = emptyList()) }
    }

    fun cycleDetailSort() {
        val st = _uiState.value
        val cur = st.activeDetail ?: return
        val base = st.originalDetailTracks.ifEmpty { cur.tracks }
        val next = when (st.detailSort) {
            FfiDetailSort.ORIGINAL -> FfiDetailSort.TITLE
            FfiDetailSort.TITLE -> FfiDetailSort.ARTIST
            FfiDetailSort.ARTIST -> FfiDetailSort.DURATION
            FfiDetailSort.DURATION -> FfiDetailSort.ORIGINAL
        }
        val sorted = when (next) {
            FfiDetailSort.ORIGINAL -> base
            FfiDetailSort.TITLE -> base.sortedBy { it.title.lowercase() }
            FfiDetailSort.ARTIST -> base.sortedBy { it.artist.lowercase() }
            FfiDetailSort.DURATION -> base.sortedBy { it.durationMs }
        }
        _uiState.update {
            it.copy(
                detailSort = next,
                activeDetail = cur.copy(tracks = sorted),
            )
        }
    }

    fun createPlaylist(name: String, description: String = "", trackIds: List<String> = emptyList()) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.createPlaylist(name, description, trackIds) }
                .onSuccess { pl -> _uiState.update { it.copy(statusMessage = "Created \"${pl.name}\"") } }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun renamePlaylist(playlistId: String, newName: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.renamePlaylist(playlistId, newName) }
                .onSuccess {
                    if (_uiState.value.activeDetail?.id == playlistId) {
                        openPlaylistDetail(playlistId, _uiState.value.detailSort)
                    }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun deletePlaylist(playlistId: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.deletePlaylist(playlistId) }
                .onSuccess {
                    if (_uiState.value.activeDetail?.id == playlistId) {
                        closeDetail()
                    }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun addTrackToPlaylist(playlistId: String, track: FfiTrack) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.addTrackToPlaylist(playlistId, track) }
                .onSuccess { _uiState.update { it.copy(statusMessage = "Added \"${track.title}\" to playlist") } }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun removeTrackFromPlaylist(playlistId: String, trackId: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.removeTrackFromPlaylist(playlistId, trackId) }
                .onSuccess {
                    if (_uiState.value.activeDetail?.id == playlistId) {
                        openPlaylistDetail(playlistId, _uiState.value.detailSort)
                    }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun deduplicatePlaylist(playlistId: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.deduplicatePlaylist(playlistId) }
                .onSuccess {
                    if (_uiState.value.activeDetail?.id == playlistId) {
                        openPlaylistDetail(playlistId, _uiState.value.detailSort)
                    }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun pushLocalPlaylistToCloud(playlistId: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            _uiState.update { it.copy(statusMessage = "Syncing playlist to account...") }
            runCatching { handle.pushLocalPlaylistToCloud(playlistId) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun toggleLike(trackId: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.toggleLike(trackId) }
                .onSuccess { liked ->
                    _uiState.update { st ->
                        val detail = st.activeDetail?.let { d ->
                            d.copy(
                                tracks = d.tracks.map { t ->
                                    if (t.id == trackId) t.copy(liked = liked) else t
                                },
                            )
                        }
                        val orig = st.originalDetailTracks.map { t ->
                            if (t.id == trackId) t.copy(liked = liked) else t
                        }
                        val curTrack = st.currentTrack?.let { c ->
                            if (c.id == trackId) c.copy(liked = liked) else c
                        }
                        st.copy(
                            activeDetail = detail,
                            originalDetailTracks = orig,
                            currentTrack = curTrack,
                        )
                    }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun scanDeviceMediaStore() {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            _uiState.update { it.copy(statusMessage = "Scanning local audio...") }
            val entries = runCatching { LocalMediaScanner.scanMediaStore(appContext) }
                .getOrDefault(emptyList())
            if (entries.isEmpty()) {
                _uiState.update { it.copy(statusMessage = "No local audio files found in MediaStore") }
                return@launch
            }
            runCatching { handle.scanLocalEntries(entries) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun scanSafFolder(treeUri: Uri) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            _uiState.update { it.copy(statusMessage = "Scanning selected music folder...") }
            val entries = runCatching { LocalMediaScanner.scanSafTree(appContext, treeUri) }
                .getOrDefault(emptyList())
            if (entries.isEmpty()) {
                _uiState.update { it.copy(statusMessage = "No supported audio files found in folder") }
                return@launch
            }
            runCatching { handle.scanLocalEntries(entries) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun importPlaylistText(name: String, rawText: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.importPlaylistText(name, rawText) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun buildTrackShareLink(track: FfiTrack, onReady: (String?) -> Unit) {
        scope.launch {
            val link = getOrCreateCore()?.trackShareLink(track)
            withContext(Dispatchers.Main) { onReady(link) }
        }
    }

    fun buildDetailShareTracklist(detail: FfiCollectionDetail, onReady: (String) -> Unit) {
        scope.launch {
            val txt = getOrCreateCore()
                ?.formatShareTracklist(detail.title, detail.shareUrl, detail.tracks)
                .orEmpty()
            if (txt.isNotEmpty()) {
                withContext(Dispatchers.Main) { onReady(txt) }
            }
        }
    }

    fun buildDetailCsv(detail: FfiCollectionDetail, onReady: (String) -> Unit) {
        scope.launch {
            val csv = getOrCreateCore()?.formatShareCsv(detail.tracks).orEmpty()
            if (csv.isNotEmpty()) {
                withContext(Dispatchers.Main) { onReady(csv) }
            }
        }
    }

    // --- Accounts ---

    fun completeSpotifyLogin(accessToken: String, cookie: String, triggerSync: Boolean) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching {
                handle.setSpotifySession(
                    accessToken = accessToken,
                    cookie = cookie,
                    userName = "",
                    userId = "",
                    triggerSync = triggerSync,
                )
            }.onFailure { err ->
                _uiState.update { it.copy(statusMessage = err.message) }
            }
        }
    }

    fun syncSpotify() {
        scope.launch { getOrCreateCore()?.syncSpotifyAccount() }
    }

    fun logoutSpotify() {
        scope.launch {
            WebCookieCleaner.clearSpotifyCookies()
            runCatching { getOrCreateCore()?.logoutSpotify() }
        }
    }

    fun completeYtmLogin(cookie: String, sapisid: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching {
                handle.setYtmSession(
                    cookie = cookie,
                    sapisid = sapisid,
                    visitorData = "",
                    channelName = "",
                    accountId = "",
                )
            }.onFailure { err ->
                _uiState.update { it.copy(statusMessage = err.message) }
            }
        }
    }

    fun syncYtm() {
        scope.launch { getOrCreateCore()?.syncYtmAccount() }
    }

    fun logoutYtm() {
        scope.launch {
            WebCookieCleaner.clearYtmCookies()
            runCatching { getOrCreateCore()?.logoutYtm() }
        }
    }

    fun syncSubsonic(url: String, username: String, password: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            _uiState.update { it.copy(statusMessage = "Connecting to Subsonic...") }
            runCatching { handle.syncSubsonic(url, username, password) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun logoutSubsonic() {
        scope.launch { runCatching { getOrCreateCore()?.logoutSubsonic() } }
    }

    // --- M4 Zero-Pairing Ferric Connect ---

    fun setConnectSheetOpen(open: Boolean) {
        nsdHelper.setSheetDiscoveryBoost(open)
    }

    fun switchConnectSpeaker(targetDeviceId: String) {
        scope.launch {
            getOrCreateCore()?.connectSwitchSpeaker(targetDeviceId)
        }
    }

    fun setConnectVolume(targetDeviceId: String, volumeNorm: Float) {
        val clamped = volumeNorm.coerceIn(0f, 1.5f)
        if (targetDeviceId == _uiState.value.connectState.localDeviceId || targetDeviceId.isEmpty()) {
            _uiState.update { it.copy(localVolume = clamped) }
            appContext.getSharedPreferences("ferric_device", Context.MODE_PRIVATE)
                .edit()
                .putFloat("user_local_volume", clamped)
                .apply()
        }
        scope.launch {
            getOrCreateCore()?.connectSetRemoteVolume(targetDeviceId, clamped)
        }
    }

    fun loadConnectQrCode(regenerate: Boolean) {
        scope.launch {
            val qr = getOrCreateCore()?.connectGetQrCode(regenerate) ?: return@launch
            _uiState.update { it.copy(connectQrCode = qr) }
        }
    }

    fun pairConnectQrUri(uri: String) {
        scope.launch {
            getOrCreateCore()?.connectPairQr(uri)
        }
    }

    // --- M5 Offline Downloads, Storage & Listening Stats ---

    fun downloadTrackOffline(track: FfiTrack) {
        scope.launch {
            _uiState.update { it.copy(statusMessage = "Downloading '${track.title}'...") }
            getOrCreateCore()?.downloadTrack(track)
        }
    }

    fun downloadTracksOffline(tracks: List<FfiTrack>) {
        if (tracks.isEmpty()) return
        scope.launch {
            _uiState.update { it.copy(statusMessage = "Downloading ${tracks.size} track(s) for offline playback...") }
            runCatching { getOrCreateCore()?.downloadTracks(tracks) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun downloadPlaylistOffline(playlistId: String) {
        scope.launch {
            _uiState.update { it.copy(statusMessage = "Downloading playlist for offline playback...") }
            runCatching { getOrCreateCore()?.downloadPlaylist(playlistId) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun deleteOfflineTrack(trackId: String) {
        scope.launch {
            runCatching { getOrCreateCore()?.deleteOfflineTrack(trackId) }
            refreshStorageAndStats()
        }
    }

    fun deleteAllOfflineDownloads() {
        scope.launch {
            runCatching { getOrCreateCore()?.deleteAllOfflineDownloads() }
            refreshStorageAndStats()
        }
    }

    fun clearAudioCache() {
        scope.launch {
            getOrCreateCore()?.clearAudioCache()
            refreshStorageAndStats()
        }
    }

    fun clearArtworkCache() {
        scope.launch {
            getOrCreateCore()?.clearArtworkCache()
            runCatching {
                val loader = SingletonImageLoader.get(appContext)
                loader.memoryCache?.clear()
                loader.diskCache?.clear()
            }
            refreshStorageAndStats()
        }
    }

    fun refreshStorageAndStats() {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            val storage = runCatching { handle.getStorageStats() }.getOrNull()
            val tzOffsetSecs = (TimeZone.getDefault().getOffset(System.currentTimeMillis()) / 1000L)
            val stats = runCatching { handle.getListeningStats(tzOffsetSecs) }.getOrNull()
            _uiState.update {
                it.copy(
                    storageStats = storage ?: it.storageStats,
                    listeningStats = stats ?: it.listeningStats,
                )
            }
        }
    }

    fun postToast(message: String) {
        _uiState.update { it.copy(statusMessage = message) }
    }

    fun refreshOutputRoute() {
        val label = OutputRouteHelper.activeOutputLabel(appContext)
        _uiState.update { it.copy(activeOutputRoute = label) }
    }

    fun clearStatusMessage() {
        _uiState.update { it.copy(statusMessage = null) }
    }

    fun onTrimMemory() {
        scope.launch { core?.trimMemory() }
    }

    fun shutdown() {
        sleepTimerPollJob?.cancel()
        nsdHelper.shutdown()
        synchronized(coreLock) {
            core?.shutdown()
            core = null
        }
    }

    private fun refreshLibraryInternal() {
        val handle = core ?: return
        val filter = _uiState.value.playlistSourceFilter
        val sort = _uiState.value.playlistSort
        val songs = runCatching { handle.listSongs("") }.getOrDefault(emptyList())
        val liked = runCatching { handle.listLikedSongs() }.getOrDefault(emptyList())
        val recent = runCatching { handle.listRecentSongs() }.getOrDefault(emptyList())
        val local = runCatching { handle.listLocalSongs() }.getOrDefault(emptyList())
        val offline = runCatching { handle.listOfflineTracks() }.getOrDefault(emptyList())
        val offlineIds = runCatching { handle.listOfflineTrackIds().toSet() }.getOrDefault(emptySet())
        val playlists = runCatching { handle.listPlaylistsFiltered(filter, sort) }.getOrDefault(emptyList())
        val albums = runCatching { handle.listAlbums() }.getOrDefault(emptyList())
        val artists = runCatching { handle.listArtists() }.getOrDefault(emptyList())
        val current = handle.getCurrentTrack()
        val upNext = handle.getUpNext(250u)
        _uiState.update {
            it.copy(
                tracks = songs,
                likedTracks = liked,
                recentTracks = recent,
                localTracks = local,
                offlineTracks = offline,
                offlineTrackIds = offlineIds,
                playlists = playlists,
                albums = albums,
                artists = artists,
                currentTrack = current ?: it.currentTrack,
                upNext = upNext,
            )
        }
        FerricWidgetUpdater.updateAll(appContext, _uiState.value)
    }
}
