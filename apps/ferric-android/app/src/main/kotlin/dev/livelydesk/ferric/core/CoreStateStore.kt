package dev.livelydesk.ferric.core

import android.content.Context
import android.net.Uri
import android.os.Build
import dev.livelydesk.ferric.auth.SilentSpotifyRefresher
import dev.livelydesk.ferric.auth.WebCookieCleaner
import dev.livelydesk.ferric.connect.ConnectNsdHelper
import dev.livelydesk.ferric.local.LocalMediaScanner
import dev.livelydesk.ferric.playback.OutputRouteHelper
import dev.livelydesk.ferric.ui.theme.CoverShadeColors
import dev.livelydesk.ferric.ui.theme.CoverShadeExtractor
import java.io.File
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
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
import uniffi.ferric_mobile_ffi.FfiLyricsState
import uniffi.ferric_mobile_ffi.FfiPlaylist
import uniffi.ferric_mobile_ffi.FfiPlaylistSort
import uniffi.ferric_mobile_ffi.FfiPlaylistSourceFilter
import uniffi.ferric_mobile_ffi.FfiQrCode
import uniffi.ferric_mobile_ffi.FfiRepeatMode
import uniffi.ferric_mobile_ffi.FfiSearchResults
import uniffi.ferric_mobile_ffi.FfiTelemetry
import uniffi.ferric_mobile_ffi.FfiThemeConfig
import uniffi.ferric_mobile_ffi.FfiTrack
import uniffi.ferric_mobile_ffi.FfiVideoStreamsState

data class PlaybackUiState(
    val tracks: List<FfiTrack> = emptyList(),
    val likedTracks: List<FfiTrack> = emptyList(),
    val recentTracks: List<FfiTrack> = emptyList(),
    val localTracks: List<FfiTrack> = emptyList(),
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
    val isPlaying: Boolean = false,
    val positionMs: Long = 0L,
    val durationMs: Long = 0L,
    val sampleRate: Int = 0,
    val outputRate: Int = 0,
    val isCrossfading: Boolean = false,
    val spectrumBins: List<Int> = List(8) { 0 },
    val shuffle: Boolean = false,
    val repeatMode: FfiRepeatMode = FfiRepeatMode.OFF,
    val localVolume: Float = 0.85f,
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
        peers = emptyList(),
    ),
    val connectQrCode: FfiQrCode? = null,
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

    @Volatile
    private var lastExtractedArtUrl: String? = null

    private val _uiState = MutableStateFlow(
        PlaybackUiState(
            activeOutputRoute = OutputRouteHelper.activeOutputLabel(appContext),
        ),
    )
    val uiState: StateFlow<PlaybackUiState> = _uiState.asStateFlow()

    private val silentSpotifyRefresher = SilentSpotifyRefresher(
        appContext = appContext,
        onRefreshed = { token, cookie ->
            completeSpotifyLogin(accessToken = token, cookie = cookie, triggerSync = true)
        },
        onFailed = {
            _uiState.update {
                it.copy(statusMessage = "Spotify session expired. Tap Accounts to sign in again.")
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
            _uiState.update { PlaybackStateReducer.applyTelemetry(it, telemetry) }
        }

        override fun onNowPlayingChanged(track: FfiTrack?) {
            val prevId = _uiState.value.currentTrack?.id
            _uiState.update { PlaybackStateReducer.applyNowPlaying(it, track) }
            if (track != null && track.id != prevId) {
                extractCoverShadeAsync(track.thumbnailUrl)
                // Reset stale video streams when track changes; lyrics load on demand or if open.
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
            _uiState.update { it.copy(upNext = upNext) }
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
            _uiState.update { it.copy(activeDetail = detail, detailSort = FfiDetailSort.ORIGINAL) }
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
            _uiState.update { it.copy(connectState = state) }
            if (state.localTcpPort.toInt() >= 1024) {
                nsdHelper.syncRegistration(
                    deviceId = state.localDeviceId,
                    deviceName = state.localDeviceName,
                    tcpPort = state.localTcpPort.toInt(),
                    roomTagsHex = state.roomTagsHex,
                )
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
                val theme = handle.getTheme()
                val eq = handle.getEq()
                val eqPresets = handle.listEqPresets()
                val autoEq = handle.searchAutoeq("")
                val dsp = handle.getDsp()
                val connect = handle.getConnectState()
                _uiState.update {
                    it.copy(
                        accountStatus = accounts,
                        recentSearches = recentSearches,
                        themeConfig = theme,
                        eqConfig = eq,
                        eqPresets = eqPresets,
                        autoEqProfiles = autoEq,
                        dspSettings = dsp,
                        connectState = connect,
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

    // --- Playback & Queue ---

    fun playTrack(trackId: String) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.playTrack(trackId) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun playQueue(tracks: List<FfiTrack>, startIndex: Int, context: String) {
        if (tracks.isEmpty()) return
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            val idx = startIndex.coerceIn(0, tracks.lastIndex).toUInt()
            runCatching { handle.playQueue(tracks, idx, context) }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun shuffleQueue(tracks: List<FfiTrack>, context: String) {
        if (tracks.isEmpty()) return
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            val shuffled = tracks.shuffled()
            runCatching { handle.playQueue(shuffled, 0u, context) }
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
        scope.launch { core?.pause() }
    }

    fun next() {
        scope.launch { runCatching { getOrCreateCore()?.next() } }
    }

    fun prev() {
        scope.launch { runCatching { getOrCreateCore()?.prev() } }
    }

    fun seekTo(positionMs: Long) {
        val clamped = positionMs.coerceAtLeast(0L)
        _uiState.update { it.copy(positionMs = clamped) }
        scope.launch { core?.seekMs(clamped.toULong()) }
    }

    fun setDucking(duck: Boolean) {
        scope.launch { core?.setDucking(duck) }
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
        scope.launch {
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
        scope.launch {
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
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.searchOnline(query) }
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
                    _uiState.update { it.copy(activeDetail = detail, detailSort = sort) }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun openAlbumDetail(albumName: String, artistName: String, sort: FfiDetailSort = FfiDetailSort.ORIGINAL) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.getAlbumDetail(albumName, artistName, sort) }
                .onSuccess { detail ->
                    _uiState.update { it.copy(activeDetail = detail, detailSort = sort) }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun openArtistDetail(artistName: String, sort: FfiDetailSort = FfiDetailSort.ORIGINAL) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.getArtistDetail(artistName, sort) }
                .onSuccess { detail ->
                    _uiState.update { it.copy(activeDetail = detail, detailSort = sort) }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun openLikedDetail(sort: FfiDetailSort = FfiDetailSort.ORIGINAL) {
        scope.launch {
            val handle = getOrCreateCore() ?: return@launch
            runCatching { handle.getLikedDetail(sort) }
                .onSuccess { detail ->
                    _uiState.update { it.copy(activeDetail = detail, detailSort = sort) }
                }
                .onFailure { err -> _uiState.update { it.copy(statusMessage = err.message) } }
        }
    }

    fun closeDetail() {
        _uiState.update { it.copy(activeDetail = null) }
    }

    fun cycleDetailSort() {
        val cur = _uiState.value.activeDetail ?: return
        val next = when (_uiState.value.detailSort) {
            FfiDetailSort.ORIGINAL -> FfiDetailSort.TITLE
            FfiDetailSort.TITLE -> FfiDetailSort.ARTIST
            FfiDetailSort.ARTIST -> FfiDetailSort.DURATION
            FfiDetailSort.DURATION -> FfiDetailSort.ORIGINAL
        }
        val sorted = when (next) {
            FfiDetailSort.ORIGINAL -> cur.tracks
            FfiDetailSort.TITLE -> cur.tracks.sortedBy { it.title.lowercase() }
            FfiDetailSort.ARTIST -> cur.tracks.sortedBy { it.artist.lowercase() }
            FfiDetailSort.DURATION -> cur.tracks.sortedBy { it.durationMs }
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
                        st.copy(activeDetail = detail)
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
            WebCookieCleaner.clearAllWebAuthData()
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
            WebCookieCleaner.clearAllWebAuthData()
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
        if (targetDeviceId == _uiState.value.connectState.localDeviceId) {
            _uiState.update { it.copy(localVolume = clamped) }
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
        val playlists = runCatching { handle.listPlaylistsFiltered(filter, sort) }.getOrDefault(emptyList())
        val albums = runCatching { handle.listAlbums() }.getOrDefault(emptyList())
        val artists = runCatching { handle.listArtists() }.getOrDefault(emptyList())
        val current = handle.getCurrentTrack()
        val upNext = handle.getUpNext(50u)
        _uiState.update {
            it.copy(
                tracks = songs,
                likedTracks = liked,
                recentTracks = recent,
                localTracks = local,
                playlists = playlists,
                albums = albums,
                artists = artists,
                currentTrack = current ?: it.currentTrack,
                upNext = upNext,
            )
        }
    }
}
