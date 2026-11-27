package dev.livelydesk.ferric

import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Home
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import dev.chrisbanes.haze.HazeState
import dev.chrisbanes.haze.hazeSource
import dev.livelydesk.ferric.auth.WebLoginActivity
import dev.livelydesk.ferric.auth.WebLoginTarget
import dev.livelydesk.ferric.core.CoreStateStore
import dev.livelydesk.ferric.playback.FerricPlaybackService
import dev.livelydesk.ferric.playback.OutputRouteHelper
import dev.livelydesk.ferric.ui.components.AddToPlaylistDialog
import dev.livelydesk.ferric.ui.components.AuroraBackdrop
import dev.livelydesk.ferric.ui.components.MiniPlayerBar
import dev.livelydesk.ferric.ui.screens.AccountsSheet
import dev.livelydesk.ferric.ui.screens.ConnectSheet
import dev.livelydesk.ferric.ui.screens.DetailScreen
import dev.livelydesk.ferric.ui.screens.EqualizerSheet
import dev.livelydesk.ferric.ui.screens.FullPlayerScreen
import dev.livelydesk.ferric.ui.screens.HomeScreen
import dev.livelydesk.ferric.ui.screens.LibraryScreen
import dev.livelydesk.ferric.ui.screens.QueueSheet
import dev.livelydesk.ferric.ui.screens.SearchScreen
import dev.livelydesk.ferric.ui.screens.SettingsSheet
import dev.livelydesk.ferric.ui.screens.ShareCardSheet
import dev.livelydesk.ferric.ui.screens.StatsSheet
import dev.livelydesk.ferric.ui.screens.ThemeStudioSheet
import dev.livelydesk.ferric.ui.theme.FerricTheme
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import java.util.Locale
import uniffi.ferric_mobile_ffi.FfiTrack

private enum class BottomTab(val label: String) {
    HOME("Home"),
    SEARCH("Search"),
    LIBRARY("Library"),
}

class MainActivity : ComponentActivity() {
    private var controllerFuture: ListenableFuture<MediaController>? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.BLACK),
            navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.BLACK),
        )
        val store = (application as FerricApp).store
        store.bootstrapLibrary()

        setContent {
            val state by store.uiState.collectAsStateWithLifecycle()
            val appHaze = remember { HazeState() }
            FerricTheme(
                themeConfig = state.themeConfig,
                coverShade = state.coverShade,
                hazeState = appHaze,
            ) {
                FerricMainShell(
                    store = store,
                    appHaze = appHaze,
                    onLaunchWebLogin = { target ->
                        startActivity(WebLoginActivity.createIntent(this, target))
                    },
                    onOpenOutputSwitcher = {
                        OutputRouteHelper.openSystemOutputSwitcher(this)
                        store.refreshOutputRoute()
                    },
                )
            }
        }
    }

    override fun onStart() {
        super.onStart()
        val token = SessionToken(this, ComponentName(this, FerricPlaybackService::class.java))
        controllerFuture = MediaController.Builder(this, token).buildAsync()
        (application as FerricApp).store.refreshOutputRoute()
    }

    override fun onStop() {
        controllerFuture?.let { MediaController.releaseFuture(it) }
        controllerFuture = null
        super.onStop()
    }
}

@Composable
private fun FerricMainShell(
    store: CoreStateStore,
    appHaze: HazeState,
    onLaunchWebLogin: (WebLoginTarget) -> Unit,
    onOpenOutputSwitcher: () -> Unit,
) {
    val context = LocalContext.current
    val palette = LocalFerricPalette.current
    val state by store.uiState.collectAsStateWithLifecycle()
    val snackbarHostState = remember { SnackbarHostState() }

    var activeTab by rememberSaveable { mutableStateOf(BottomTab.HOME) }
    var showFullPlayer by rememberSaveable { mutableStateOf(false) }
    var showQueueSheet by rememberSaveable { mutableStateOf(false) }
    var showAccountsSheet by rememberSaveable { mutableStateOf(false) }
    var showConnectSheet by rememberSaveable { mutableStateOf(false) }
    var showEqSheet by rememberSaveable { mutableStateOf(false) }
    var showThemeSheet by rememberSaveable { mutableStateOf(false) }
    var showSettingsSheet by rememberSaveable { mutableStateOf(false) }
    var showStatsSheet by rememberSaveable { mutableStateOf(false) }
    var showShareCardSheet by rememberSaveable { mutableStateOf(false) }
    var showCreatePlaylistDialog by rememberSaveable { mutableStateOf(false) }
    var newPlaylistName by rememberSaveable { mutableStateOf("") }
    var pendingTrackForPlaylist by remember { mutableStateOf<FfiTrack?>(null) }
    var pendingExportText by remember { mutableStateOf<String?>(null) }

    val audioPermission = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        Manifest.permission.READ_MEDIA_AUDIO
    } else {
        Manifest.permission.READ_EXTERNAL_STORAGE
    }

    val audioPermissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) {
            store.scanDeviceMediaStore()
        }
    }

    val safFolderLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocumentTree(),
    ) { uri ->
        if (uri != null) {
            runCatching {
                context.contentResolver.takePersistableUriPermission(
                    uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION,
                )
            }
            store.scanSafFolder(uri)
        }
    }

    val importPlaylistLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            val text = runCatching {
                context.contentResolver.openInputStream(uri)?.bufferedReader()?.use { it.readText() }
            }.getOrNull()
            if (!text.isNullOrBlank()) {
                val baseName = uri.lastPathSegment?.substringAfterLast('/')?.substringBeforeLast('.')
                    ?.ifBlank { "Imported Playlist" } ?: "Imported Playlist"
                store.importPlaylistText(baseName, text)
            }
        }
    }

    val importApoLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            val text = runCatching {
                context.contentResolver.openInputStream(uri)?.bufferedReader()?.use { it.readText() }
            }.getOrNull()
            if (!text.isNullOrBlank()) {
                store.importEqApo(text)
            }
        }
    }

    val importThemeLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            val text = runCatching {
                context.contentResolver.openInputStream(uri)?.bufferedReader()?.use { it.readText() }
            }.getOrNull()
            if (!text.isNullOrBlank()) {
                store.importThemeJson(text)
            }
        }
    }

    val exportDocLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.CreateDocument("*/*"),
    ) { uri ->
        val content = pendingExportText
        pendingExportText = null
        if (uri != null && !content.isNullOrEmpty()) {
            runCatching {
                context.contentResolver.openOutputStream(uri)?.bufferedWriter()?.use {
                    it.write(content)
                }
            }
        }
    }

    fun copyToClipboard(label: String, text: String) {
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return
        cm.setPrimaryClip(ClipData.newPlainText(label, text))
    }

    LaunchedEffect(state.statusMessage) {
        val msg = state.statusMessage
        if (!msg.isNullOrBlank()) {
            snackbarHostState.showSnackbar(msg)
            store.clearStatusMessage()
        }
    }

    BackHandler(enabled = showFullPlayer || state.activeDetail != null) {
        if (showFullPlayer) {
            showFullPlayer = false
        } else {
            store.closeDetail()
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(palette.canvas),
    ) {
        if (palette.isLiquidGlass) {
            AuroraBackdrop(
                colors = state.coverShade,
                baseCanvas = palette.canvas,
                isPlaying = state.isPlaying,
                intensity = 0.62f,
                modifier = Modifier.hazeSource(appHaze),
            )
        }

        if (showFullPlayer) {
            FullPlayerScreen(
                state = state,
                onCollapse = { showFullPlayer = false },
                onTogglePlayPause = { store.togglePlayPause() },
                onNext = { store.next() },
                onPrev = { store.prev() },
                onSeek = { pos -> store.seekTo(pos) },
                onToggleShuffle = { store.toggleShuffle() },
                onCycleRepeat = { store.cycleRepeat() },
                onToggleLike = { id -> store.toggleLike(id) },
                onOpenArtist = { artist -> store.openArtistDetail(artist) },
                onOpenQueue = { showQueueSheet = true },
                onOpenEqualizer = { showEqSheet = true },
                onOpenThemeStudio = { showThemeSheet = true },
                onOpenConnect = { showConnectSheet = true },
                onOpenShareCard = { showShareCardSheet = true },
                onOpenOutputSwitcher = onOpenOutputSwitcher,
                onLoadLyrics = { store.loadLyricsForCurrent() },
                onNudgeLyricsOffset = { delta -> store.nudgeLyricsOffsetMs(delta) },
                onResolveVideo = { store.resolveVideoForCurrent() },
            )
        } else {
            Scaffold(
                containerColor = if (palette.isLiquidGlass) Color.Transparent else palette.canvas,
                snackbarHost = { SnackbarHost(hostState = snackbarHostState) },
                bottomBar = {
                    Column(
                        modifier = Modifier
                            .background(if (palette.isLiquidGlass) Color.Transparent else palette.canvas)
                            .navigationBarsPadding(),
                    ) {
                        MiniPlayerBar(
                            state = state,
                            onTogglePlayPause = { store.togglePlayPause() },
                            onNext = { store.next() },
                            onPrev = { store.prev() },
                            onOpenFullPlayer = { showFullPlayer = true },
                            onOpenQueue = { showQueueSheet = true },
                            onOpenConnect = { showConnectSheet = true },
                        )
                        NavigationBar(
                            containerColor = if (palette.isLiquidGlass) {
                                palette.glassTint.copy(alpha = 0.78f)
                            } else {
                                palette.surface
                            },
                        ) {
                            val items = listOf(
                                Triple(BottomTab.HOME, "Home", Icons.Rounded.Home),
                                Triple(BottomTab.SEARCH, "Search", Icons.Rounded.Search),
                                Triple(BottomTab.LIBRARY, "Library", Icons.Rounded.LibraryMusic),
                            )
                            items.forEach { (tab, label, icon) ->
                                NavigationBarItem(
                                    selected = activeTab == tab && state.activeDetail == null,
                                    onClick = {
                                        store.closeDetail()
                                        activeTab = tab
                                    },
                                    icon = { Icon(icon, contentDescription = label) },
                                    label = { Text(label) },
                                    colors = NavigationBarItemDefaults.colors(
                                        selectedIconColor = palette.accent,
                                        selectedTextColor = palette.accent,
                                        unselectedIconColor = palette.textSecondary,
                                        unselectedTextColor = palette.textSecondary,
                                        indicatorColor = palette.elevated,
                                    ),
                                )
                            }
                        }
                    }
                },
            ) { innerPadding ->
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .statusBarsPadding()
                        .padding(innerPadding)
                        .then(if (palette.isLiquidGlass) Modifier.hazeSource(appHaze) else Modifier),
                ) {
                    val detail = state.activeDetail
                    if (detail != null) {
                        DetailScreen(
                            detail = detail,
                            detailSort = state.detailSort,
                            currentTrackId = state.currentTrack?.id,
                            offlineTrackIds = state.offlineTrackIds,
                            onBack = { store.closeDetail() },
                            onPlayQueue = { list, idx, ctx -> store.playQueue(list, idx, ctx) },
                            onShuffleQueue = { list, ctx -> store.shuffleQueue(list, ctx) },
                            onCycleSort = { store.cycleDetailSort() },
                            onDeduplicate = { store.deduplicatePlaylist(detail.id) },
                            onSyncToAccount = { store.pushLocalPlaylistToCloud(detail.id) },
                            onDownloadPlaylist = { store.downloadPlaylistOffline(detail.id) },
                            onDownloadTrack = { t -> store.downloadTrackOffline(t) },
                            onDeleteOfflineTrack = { id -> store.deleteOfflineTrack(id) },
                            onCopyShareLink = {
                                detail.shareUrl?.let { copyToClipboard("Ferric Link", it) }
                            },
                            onCopyTracklist = {
                                store.buildDetailShareTracklist(detail) { txt ->
                                    copyToClipboard("Ferric Tracklist", txt)
                                }
                            },
                            onExportCsv = {
                                store.buildDetailCsv(detail) { csv ->
                                    pendingExportText = csv
                                    exportDocLauncher.launch("${detail.title.replace(' ', '_')}.csv")
                                }
                            },
                            onRenamePlaylist = { name -> store.renamePlaylist(detail.id, name) },
                            onDeletePlaylist = { store.deletePlaylist(detail.id) },
                            onToggleLike = { id -> store.toggleLike(id) },
                            onPlayNext = { t -> store.playNext(t) },
                            onAddToQueue = { t -> store.appendToQueue(t) },
                            onAddToPlaylist = { t -> pendingTrackForPlaylist = t },
                            onCopyTrackLink = { t ->
                                store.buildTrackShareLink(t) { link ->
                                    if (link != null) copyToClipboard("Track Link", link)
                                }
                            },
                            onOpenArtist = { artist -> store.openArtistDetail(artist) },
                            onRemoveFromPlaylist = { trackId -> store.removeTrackFromPlaylist(detail.id, trackId) },
                        )
                    } else {
                        when (activeTab) {
                            BottomTab.HOME -> HomeScreen(
                                state = state,
                                onRefresh = { store.loadHome(forceRefresh = true, moodIdx = null) },
                                onSelectMood = { idx -> store.loadHome(forceRefresh = false, moodIdx = idx) },
                                onOpenAccounts = { showAccountsSheet = true },
                                onOpenThemeStudio = { showThemeSheet = true },
                                onOpenConnect = { showConnectSheet = true },
                                onOpenSettings = { showSettingsSheet = true },
                                onPlayQueue = { list, idx, ctx -> store.playQueue(list, idx, ctx) },
                                onShuffleQueue = { list, ctx -> store.shuffleQueue(list, ctx) },
                                onOpenStation = { id -> store.openStation(id) },
                                onOpenShelfCard = { card ->
                                    val bid = card.browseId
                                    if (!bid.isNullOrBlank()) {
                                        store.openBrowseDetail(bid, card.kind)
                                    }
                                },
                                onToggleLike = { id -> store.toggleLike(id) },
                                onPlayNext = { t -> store.playNext(t) },
                                onAddToQueue = { t -> store.appendToQueue(t) },
                                onAddToPlaylist = { t -> pendingTrackForPlaylist = t },
                                onCopyTrackLink = { t ->
                                    store.buildTrackShareLink(t) { link ->
                                        if (link != null) copyToClipboard("Track Link", link)
                                    }
                                },
                                onOpenArtist = { artist -> store.openArtistDetail(artist) },
                            )

                            BottomTab.SEARCH -> SearchScreen(
                                state = state,
                                onInstantLocalSearch = { q -> store.instantSearchLocal(q) },
                                onOnlineSearch = { q -> store.searchOnline(q) },
                                onClearRecentSearches = { store.clearRecentSearches() },
                                onImportUrl = { url -> store.importPublicUrl(url) },
                                onPlayQueue = { list, idx, ctx -> store.playQueue(list, idx, ctx) },
                                onOpenCard = { card ->
                                    val bid = card.browseId
                                    if (!bid.isNullOrBlank()) {
                                        store.openBrowseDetail(bid, card.kind)
                                    }
                                },
                                onToggleLike = { id -> store.toggleLike(id) },
                                onPlayNext = { t -> store.playNext(t) },
                                onAddToQueue = { t -> store.appendToQueue(t) },
                                onAddToPlaylist = { t -> pendingTrackForPlaylist = t },
                                onCopyTrackLink = { t ->
                                    store.buildTrackShareLink(t) { link ->
                                        if (link != null) copyToClipboard("Track Link", link)
                                    }
                                },
                                onOpenArtist = { artist -> store.openArtistDetail(artist) },
                            )

                            BottomTab.LIBRARY -> LibraryScreen(
                                state = state,
                                onPlayQueue = { list, idx, ctx -> store.playQueue(list, idx, ctx) },
                                onOpenPlaylist = { id -> store.openPlaylistDetail(id) },
                                onOpenAlbum = { alb, art -> store.openAlbumDetail(alb, art) },
                                onOpenArtist = { art -> store.openArtistDetail(art) },
                                onOpenLikedDetail = { store.openLikedDetail() },
                                onSetPlaylistFilter = { f -> store.setPlaylistFilter(f) },
                                onCyclePlaylistSort = { store.cyclePlaylistSort() },
                                onCreatePlaylistRequest = {
                                    newPlaylistName = ""
                                    showCreatePlaylistDialog = true
                                },
                                onImportPlaylistFile = {
                                    importPlaylistLauncher.launch(arrayOf("*/*"))
                                },
                                onScanPhoneMusic = {
                                    val hasPerm = ContextCompat.checkSelfPermission(
                                        context,
                                        audioPermission,
                                    ) == PackageManager.PERMISSION_GRANTED
                                    if (hasPerm) {
                                        store.scanDeviceMediaStore()
                                    } else {
                                        audioPermissionLauncher.launch(audioPermission)
                                    }
                                },
                                onPickSafFolder = { safFolderLauncher.launch(null) },
                                onToggleLike = { id -> store.toggleLike(id) },
                                onPlayNext = { t -> store.playNext(t) },
                                onAddToQueue = { t -> store.appendToQueue(t) },
                                onAddToPlaylist = { t -> pendingTrackForPlaylist = t },
                                onCopyTrackLink = { t ->
                                    store.buildTrackShareLink(t) { link ->
                                        if (link != null) copyToClipboard("Track Link", link)
                                    }
                                },
                                onDownloadOffline = { t -> store.downloadTrackOffline(t) },
                                onDeleteOffline = { id -> store.deleteOfflineTrack(id) },
                            )
                        }
                    }
                }
            }
        }
    }

    if (showQueueSheet) {
        QueueSheet(
            state = state,
            onDismiss = { showQueueSheet = false },
            onSeek = { pos -> store.seekTo(pos) },
            onToggleShuffle = { store.toggleShuffle() },
            onCycleRepeat = { store.cycleRepeat() },
            onJumpUpcoming = { idx -> store.jumpToUpcoming(idx) },
            onMoveUp = { idx -> store.moveQueueItemUp(idx) },
            onMoveDown = { idx -> store.moveQueueItemDown(idx) },
            onRemove = { idx -> store.removeQueueItem(idx) },
            onClearUpNext = { store.clearUpNext() },
            onSaveQueue = { name -> store.saveQueueAsPlaylist(name) },
            onCopyQueue = {
                store.buildQueueShareText { txt -> copyToClipboard("Ferric Queue", txt) }
            },
        )
    }

    if (showAccountsSheet) {
        AccountsSheet(
            status = state.accountStatus,
            onDismiss = { showAccountsSheet = false },
            onSignInSpotify = {
                showAccountsSheet = false
                onLaunchWebLogin(WebLoginTarget.SPOTIFY)
            },
            onSyncSpotify = { store.syncSpotify() },
            onLogoutSpotify = { store.logoutSpotify() },
            onSignInYtm = {
                showAccountsSheet = false
                onLaunchWebLogin(WebLoginTarget.YOUTUBE_MUSIC)
            },
            onSyncYtm = { store.syncYtm() },
            onLogoutYtm = { store.logoutYtm() },
            onSyncSubsonic = { url, user, pass -> store.syncSubsonic(url, user, pass) },
            onLogoutSubsonic = { store.logoutSubsonic() },
        )
    }

    if (showConnectSheet) {
        ConnectSheet(
            state = state,
            onDismiss = { showConnectSheet = false },
            onSheetVisibilityChanged = { open -> store.setConnectSheetOpen(open) },
            onSwitchSpeaker = { id -> store.switchConnectSpeaker(id) },
            onSetVolume = { id, vol -> store.setConnectVolume(id, vol) },
            onOpenSystemOutputSwitcher = onOpenOutputSwitcher,
            onLoadQrCode = { regen -> store.loadConnectQrCode(regen) },
            onCopyPairingUri = { uri ->
                copyToClipboard("Ferric Connect URI", uri)
                store.clearStatusMessage()
            },
            onPairWithUri = { uri -> store.pairConnectQrUri(uri) },
        )
    }

    if (showEqSheet) {
        EqualizerSheet(
            eqConfig = state.eqConfig,
            eqPresets = state.eqPresets,
            autoEqProfiles = state.autoEqProfiles,
            dspSettings = state.dspSettings,
            activeOutputRoute = state.activeOutputRoute,
            onDismiss = { showEqSheet = false },
            onUpdateEq = { eq -> store.updateEq(eq) },
            onApplyPreset = { name -> store.applyEqPreset(name) },
            onSearchAutoEq = { q -> store.searchAutoEq(q) },
            onApplyAutoEq = { name -> store.applyAutoEqProfile(name) },
            onImportApo = { importApoLauncher.launch(arrayOf("text/plain", "*/*")) },
            onExportApo = {
                store.exportEqApo { apo ->
                    pendingExportText = apo
                    exportDocLauncher.launch("ferric-eq.txt")
                }
            },
            onUpdateDsp = { dsp -> store.updateDsp(dsp) },
            onOpenOutputSwitcher = onOpenOutputSwitcher,
        )
    }

    if (showThemeSheet) {
        ThemeStudioSheet(
            themeConfig = state.themeConfig,
            onDismiss = { showThemeSheet = false },
            onApplyPreset = { name -> store.applyThemePreset(name) },
            onUpdateTheme = { theme -> store.updateTheme(theme) },
            onImportThemeJson = { importThemeLauncher.launch(arrayOf("application/json", "*/*")) },
            onExportThemeJson = {
                store.exportThemeJson { json ->
                    pendingExportText = json
                    exportDocLauncher.launch("ferric-android.ferric-theme.json")
                }
            },
        )
    }

    if (showSettingsSheet) {
        SettingsSheet(
            state = state,
            onRefreshStorageAndStats = { store.refreshStorageAndStats() },
            onDismiss = { showSettingsSheet = false },
            onOpenAccounts = { showAccountsSheet = true },
            onOpenConnect = { showConnectSheet = true },
            onOpenThemeStudio = { showThemeSheet = true },
            onOpenEqualizer = { showEqSheet = true },
            onOpenOutputSwitcher = onOpenOutputSwitcher,
            onOpenStats = { showStatsSheet = true },
            onScanLocalMusic = {
                val hasPerm = ContextCompat.checkSelfPermission(
                    context,
                    audioPermission,
                ) == PackageManager.PERMISSION_GRANTED
                if (hasPerm) store.scanDeviceMediaStore() else audioPermissionLauncher.launch(audioPermission)
            },
            onPickSafFolder = { safFolderLauncher.launch(null) },
            onClearAudioCache = { store.clearAudioCache() },
            onClearArtworkCache = { store.clearArtworkCache() },
            onDeleteAllOffline = { store.deleteAllOfflineDownloads() },
        )
    }

    if (showStatsSheet) {
        StatsSheet(
            stats = state.listeningStats,
            onRefresh = { store.refreshStorageAndStats() },
            onDismiss = { showStatsSheet = false },
        )
    }

    if (showShareCardSheet && state.currentTrack != null) {
        ShareCardSheet(
            state = state,
            onDismiss = { showShareCardSheet = false },
            onMessage = { msg -> store.postToast(msg) },
        )
    }

    pendingTrackForPlaylist?.let { track ->
        AddToPlaylistDialog(
            track = track,
            playlists = state.playlists,
            onDismiss = { pendingTrackForPlaylist = null },
            onSelectPlaylist = { plId ->
                store.addTrackToPlaylist(plId, track)
                pendingTrackForPlaylist = null
            },
            onCreateNewPlaylist = {
                pendingTrackForPlaylist = null
                newPlaylistName = ""
                showCreatePlaylistDialog = true
            },
        )
    }

    if (showCreatePlaylistDialog) {
        AlertDialog(
            onDismissRequest = { showCreatePlaylistDialog = false },
            containerColor = palette.elevated,
            title = { Text("New Playlist", color = palette.textPrimary) },
            text = {
                OutlinedTextField(
                    value = newPlaylistName,
                    onValueChange = { newPlaylistName = it },
                    singleLine = true,
                    label = { Text("Playlist name") },
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        if (newPlaylistName.isNotBlank()) {
                            store.createPlaylist(newPlaylistName.trim())
                            showCreatePlaylistDialog = false
                        }
                    },
                ) { Text("Create", color = palette.accent) }
            },
            dismissButton = {
                TextButton(onClick = { showCreatePlaylistDialog = false }) {
                    Text("Cancel", color = palette.textSecondary)
                }
            },
        )
    }
}

fun formatDurationMs(ms: Long): String {
    val totalSec = (ms / 1_000L).coerceAtLeast(0L)
    val mins = totalSec / 60L
    val secs = totalSec % 60L
    return String.format(Locale.US, "%d:%02d", mins, secs)
}
