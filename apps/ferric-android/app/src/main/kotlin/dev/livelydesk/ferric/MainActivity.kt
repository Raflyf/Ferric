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
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import dev.chrisbanes.haze.HazeState
import dev.livelydesk.ferric.auth.WebLoginActivity
import dev.livelydesk.ferric.auth.WebLoginTarget
import dev.livelydesk.ferric.core.CoreStateStore
import dev.livelydesk.ferric.core.activeDisplayTrack
import dev.livelydesk.ferric.playback.FerricPlaybackService
import dev.livelydesk.ferric.playback.OutputRouteHelper
import dev.livelydesk.ferric.ui.components.AddToPlaylistDialog
import dev.livelydesk.ferric.ui.components.AuroraBackdrop
import dev.livelydesk.ferric.ui.components.FerricLaunchSplash
import dev.livelydesk.ferric.ui.components.FerricSnackbar
import dev.livelydesk.ferric.ui.components.FerricTab
import dev.livelydesk.ferric.ui.components.FloatingLiquidGlassDock
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
import dev.livelydesk.ferric.ui.screens.YouScreen
import dev.livelydesk.ferric.ui.theme.FERRIC_MOTION_MS
import dev.livelydesk.ferric.ui.theme.FerricEaseOut
import dev.livelydesk.ferric.ui.theme.FerricSheetPortalOverlay
import dev.livelydesk.ferric.ui.theme.FerricTheme
import dev.livelydesk.ferric.ui.theme.LocalBackdropState
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.LocalFerricSheetHost
import dev.livelydesk.ferric.ui.theme.ferricBackdropSource
import dev.livelydesk.ferric.ui.theme.ferricDialogContainerColor
import dev.livelydesk.ferric.ui.theme.rememberFerricBackdropState
import dev.livelydesk.ferric.ui.theme.rememberFerricSheetHostState
import java.util.Locale
import kotlin.math.abs
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch
import uniffi.ferric_mobile_ffi.FfiCollectionDetail
import uniffi.ferric_mobile_ffi.FfiTrack

class MainActivity : ComponentActivity() {
    private var controllerFuture: ListenableFuture<MediaController>? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
        )
        val store = (application as FerricApp).store
        store.bootstrapLibrary()

        setContent {
            val state by store.uiState.collectAsStateWithLifecycle()
            val appHaze = remember { HazeState() }
            val backdropState = rememberFerricBackdropState()

            val isLightTheme = state.themeConfig?.backgroundMode == uniffi.ferric_mobile_ffi.FfiBackgroundMode.WARM_CREAM
            LaunchedEffect(isLightTheme) {
                enableEdgeToEdge(
                    statusBarStyle = if (isLightTheme) {
                        SystemBarStyle.light(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT)
                    } else {
                        SystemBarStyle.dark(android.graphics.Color.TRANSPARENT)
                    },
                    navigationBarStyle = if (isLightTheme) {
                        SystemBarStyle.light(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT)
                    } else {
                        SystemBarStyle.dark(android.graphics.Color.TRANSPARENT)
                    },
                )
            }

            FerricTheme(
                themeConfig = state.themeConfig,
                coverShade = state.coverShade,
                hazeState = appHaze,
                backdropState = backdropState,
            ) {
                FerricMainShell(
                    store = store,
                    backdropState = backdropState,
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

    override fun onDestroy() {
        val store = (application as FerricApp).store
        val st = store.uiState.value
        val activeLocalPlayback = st.isPlaying && st.connectState.isLocalActiveSpeaker
        if (isFinishing && !activeLocalPlayback) {
            store.shutdown()
        }
        super.onDestroy()
    }
}

@Composable
private fun FerricMainShell(
    store: CoreStateStore,
    backdropState: dev.livelydesk.ferric.ui.theme.FerricBackdropState,
    onLaunchWebLogin: (WebLoginTarget) -> Unit,
    onOpenOutputSwitcher: () -> Unit,
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val ioScope = rememberCoroutineScope()
    val palette = LocalFerricPalette.current
    val state by store.uiState.collectAsStateWithLifecycle()
    val snackbarHostState = remember { SnackbarHostState() }
    val sheetHost = rememberFerricSheetHostState()
    val sheetBackdrop = rememberFerricBackdropState()
    val hasActiveSheets = sheetHost.hasActiveSheets

    var activeTab by rememberSaveable { mutableStateOf(FerricTab.HOME) }
    var dockCollapsed by rememberSaveable { mutableStateOf(false) }
    var showFullPlayer by rememberSaveable { mutableStateOf(false) }
    var showQueueSheet by rememberSaveable { mutableStateOf(false) }
    var showAccountsSheet by rememberSaveable { mutableStateOf(false) }
    var showConnectSheet by rememberSaveable { mutableStateOf(false) }
    var showEqSheet by rememberSaveable { mutableStateOf(false) }
    var showThemeSheet by rememberSaveable { mutableStateOf(false) }
    var showSettingsSheet by rememberSaveable { mutableStateOf(false) }
    var showStatsSheet by rememberSaveable { mutableStateOf(false) }
    var showShareCardSheet by rememberSaveable { mutableStateOf(false) }
    var shareCardInitialPosMs by rememberSaveable { mutableLongStateOf(0L) }
    var showCreatePlaylistDialog by rememberSaveable { mutableStateOf(false) }
    var newPlaylistName by rememberSaveable { mutableStateOf("") }
    var pendingTrackForPlaylist by remember { mutableStateOf<FfiTrack?>(null) }
    var pendingExportText by rememberSaveable { mutableStateOf<String?>(null) }
    var askedNotifPerm by rememberSaveable { mutableStateOf(false) }
    var showLaunchSplash by rememberSaveable { mutableStateOf(true) }

    // Persistent per-tab LazyListStates so scroll positions survive tab switches & FullPlayer open.
    val homeListState = rememberLazyListState()
    val searchListState = rememberLazyListState()
    val libraryListState = rememberLazyListState()
    val youListState = rememberLazyListState()
    val detailListState = rememberLazyListState()

    val currentListState: LazyListState = when {
        state.activeDetail != null -> detailListState
        activeTab == FerricTab.HOME -> homeListState
        activeTab == FerricTab.SEARCH -> searchListState
        activeTab == FerricTab.LIBRARY -> libraryListState
        else -> youListState
    }

    // Track scroll direction to morph the 4-tab Liquid Glass dock cleanly into a compact orb on scroll-down.
    LaunchedEffect(state.activeDetail?.id) {
        dockCollapsed = false
        if (state.activeDetail != null) {
            detailListState.scrollToItem(0)
        }
    }

    LaunchedEffect(currentListState, activeTab, state.activeDetail?.id) {
        var prevIndex = currentListState.firstVisibleItemIndex
        var prevOffset = currentListState.firstVisibleItemScrollOffset
        snapshotFlow { currentListState.firstVisibleItemIndex to currentListState.firstVisibleItemScrollOffset }
            .distinctUntilChanged()
            .collect { (index, offset) ->
                if (index == 0 && offset < 48) {
                    dockCollapsed = false
                } else {
                    val scrollingDown = index > prevIndex || (index == prevIndex && offset - prevOffset > 28)
                    val scrollingUp = index < prevIndex || (index == prevIndex && prevOffset - offset > 28)
                    if (scrollingDown) dockCollapsed = true
                    else if (scrollingUp) dockCollapsed = false
                }
                prevIndex = index
                prevOffset = offset
            }
    }

    // Pause background mDNS NSD discovery whenever the app moves to the background.
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_START -> store.setAppForeground(true)
                Lifecycle.Event.ON_STOP -> store.setAppForeground(false)
                else -> {}
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    val notifPermissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestPermission(),
    ) { /* MediaSession notification permission handled gracefully if denied */ }

    // Request POST_NOTIFICATIONS on Android 13+ once when playback starts so media controls appear.
    LaunchedEffect(state.isPlaying) {
        if (state.isPlaying && !askedNotifPerm && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            askedNotifPerm = true
            val granted = ContextCompat.checkSelfPermission(
                context,
                Manifest.permission.POST_NOTIFICATIONS,
            ) == PackageManager.PERMISSION_GRANTED
            if (!granted) {
                notifPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
    }

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
            ioScope.launch(Dispatchers.IO) {
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
    }

    val importApoLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            ioScope.launch(Dispatchers.IO) {
                val text = runCatching {
                    context.contentResolver.openInputStream(uri)?.bufferedReader()?.use { it.readText() }
                }.getOrNull()
                if (!text.isNullOrBlank()) {
                    store.importEqApo(text)
                }
            }
        }
    }

    val importThemeLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            ioScope.launch(Dispatchers.IO) {
                val text = runCatching {
                    context.contentResolver.openInputStream(uri)?.bufferedReader()?.use { it.readText() }
                }.getOrNull()
                if (!text.isNullOrBlank()) {
                    store.importThemeJson(text)
                }
            }
        }
    }

    val exportDocLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.CreateDocument("*/*"),
    ) { uri ->
        val content = pendingExportText
        pendingExportText = null
        if (uri != null && !content.isNullOrEmpty()) {
            ioScope.launch(Dispatchers.IO) {
                runCatching {
                    context.contentResolver.openOutputStream(uri)?.bufferedWriter()?.use {
                        it.write(content)
                    }
                }.onSuccess {
                    store.postToast("Export saved")
                }.onFailure { err ->
                    store.postToast("Export failed: ${err.message}")
                }
            }
        }
    }

    fun copyToClipboard(label: String, text: String) {
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return
        cm.setPrimaryClip(ClipData.newPlainText(label, text))
        store.postToast("Copied $label")
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

    CompositionLocalProvider(LocalFerricSheetHost provides sheetHost) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(palette.canvas),
        ) {
            // Active screen host captured into sheetBackdrop ONLY when a sheet is open
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .ferricBackdropSource(sheetBackdrop, enabled = hasActiveSheets),
            ) {
                // Layer 1: Full-screen scrollable canvas captured into ONE shared backdropState so the
                // floating Liquid Glass dock & mini-player opt-refract scrolling cards. Inner page cards draw
                // directly over live AuroraBackdrop as zero-FBO translucent crystal glass.
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .ferricBackdropSource(backdropState, enabled = !showFullPlayer),
                ) {
                    if (palette.isLiquidGlass) {
                        AuroraBackdrop(
                            colors = state.coverShade,
                            baseCanvas = palette.canvas,
                            isPlaying = state.isPlaying && !showFullPlayer && !hasActiveSheets,
                            intensity = 0.86f,
                        )
                    }

                    CompositionLocalProvider(LocalBackdropState provides null) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .statusBarsPadding(),
                ) {
                var cachedDetail by remember { mutableStateOf<FfiCollectionDetail?>(null) }
                if (state.activeDetail != null) {
                    cachedDetail = state.activeDetail
                }
                val isDetailShowing = state.activeDetail != null
                val tabProgress by animateFloatAsState(
                    targetValue = activeTab.ordinal.toFloat(),
                    animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut),
                    label = "main_tab_slide",
                )
                val tabsLayerAlpha by animateFloatAsState(
                    targetValue = if (isDetailShowing) 0f else 1f,
                    animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut),
                    label = "tabs_under_detail_alpha",
                )

                // Keep all 4 primary tabs permanently mounted so Coil art and scroll state never flash or reload.
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .layout { measurable, constraints ->
                            if (tabsLayerAlpha < 0.01f) {
                                layout(0, 0) {}
                            } else {
                                val placeable = measurable.measure(constraints)
                                layout(placeable.width, placeable.height) {
                                    if (tabsLayerAlpha >= 0.999f) {
                                        placeable.placeRelative(0, 0)
                                    } else {
                                        placeable.placeRelativeWithLayer(0, 0) {
                                            alpha = tabsLayerAlpha
                                            val s = 0.96f + 0.04f * tabsLayerAlpha
                                            scaleX = s
                                            scaleY = s
                                        }
                                    }
                                }
                            }
                        },
                ) {
                    FerricTab.entries.forEach { tab ->
                        val delta = tab.ordinal.toFloat() - tabProgress
                        Box(
                            modifier = Modifier
                                .fillMaxSize()
                                .layout { measurable, constraints ->
                                    // Unplace any neighbor >= 0.995 tabs away so only transitioning
                                    // or active tabs measure, draw, and receive touch events.
                                    if (abs(delta) >= 0.995f) {
                                        layout(0, 0) {}
                                    } else {
                                        val placeable = measurable.measure(constraints)
                                        layout(placeable.width, placeable.height) {
                                            if (abs(delta) < 0.001f) {
                                                placeable.placeRelative(0, 0)
                                            } else {
                                                placeable.placeRelativeWithLayer(
                                                    0,
                                                    0,
                                                    zIndex = 1f - abs(delta),
                                                ) {
                                                    translationX = delta * placeable.width * 0.22f
                                                    alpha = (1f - abs(delta)).coerceIn(0f, 1f)
                                                }
                                            }
                                        }
                                    }
                                },
                        ) {
                            when (tab) {
                                FerricTab.HOME -> HomeScreen(
                                    state = state,
                                    listState = homeListState,
                                    onRefresh = { store.loadHome(forceRefresh = true, moodIdx = null) },
                                    onLoadMoreHome = { store.loadHomeMore() },
                                    onSelectMood = { idx -> store.loadHome(forceRefresh = false, moodIdx = idx) },
                                    onOpenConnect = { showConnectSheet = true },
                                    onOpenStats = { showStatsSheet = true },
                                    onOpenPlaylist = { id -> store.openPlaylistDetail(id) },
                                    onOpenLikedDetail = { store.openLikedDetail() },
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

                                FerricTab.SEARCH -> SearchScreen(
                                    state = state,
                                    listState = searchListState,
                                    onInstantLocalSearch = { q -> store.instantSearchLocal(q) },
                                    onOnlineSearch = { q -> store.searchOnline(q) },
                                    onBrowseGenre = { genre -> store.browseGenre(genre) },
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
                                    onDownloadOffline = { t -> store.downloadTrackOffline(t) },
                                    onDeleteOffline = { id -> store.deleteOfflineTrack(id) },
                                )

                                FerricTab.LIBRARY -> LibraryScreen(
                                    state = state,
                                    listState = libraryListState,
                                    onPlayQueue = { list, idx, ctx -> store.playQueue(list, idx, ctx) },
                                    onOpenPlaylist = { id -> store.openPlaylistDetail(id) },
                                    onOpenAlbum = { alb, art -> store.openAlbumDetail(alb, art) },
                                    onOpenArtist = { art -> store.openArtistDetail(art) },
                                    onOpenLikedDetail = { store.openLikedDetail() },
                                    onOpenLocalCollection = { title, sub, tracks ->
                                        store.openLocalCollectionDetail(title, sub, tracks)
                                    },
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

                                FerricTab.YOU -> YouScreen(
                                    state = state,
                                    listState = youListState,
                                    onOpenHistory = {
                                        store.openLocalCollectionDetail(
                                            "Listening History",
                                            "${state.recentTracks.size} recent tracks",
                                            state.recentTracks,
                                        )
                                    },
                                    onOpenStats = { showStatsSheet = true },
                                    onOpenConnect = { showConnectSheet = true },
                                    onOpenThemeStudio = { showThemeSheet = true },
                                    onOpenEqualizer = { showEqSheet = true },
                                    onSignInYtm = { onLaunchWebLogin(WebLoginTarget.YOUTUBE_MUSIC) },
                                    onSyncYtm = { store.syncYtm() },
                                    onSignInSpotify = { onLaunchWebLogin(WebLoginTarget.SPOTIFY) },
                                    onSyncSpotify = { store.syncSpotify() },
                                    onOpenAccounts = { showAccountsSheet = true },
                                    onOpenSettings = { showSettingsSheet = true },
                                )
                            }
                        }
                    }
                }

                AnimatedVisibility(
                    visible = isDetailShowing,
                    enter = slideInHorizontally(animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)) { it / 4 } +
                        fadeIn(tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)),
                    exit = slideOutHorizontally(animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)) { it / 4 } +
                        fadeOut(tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)),
                ) {
                    val detail = state.activeDetail ?: cachedDetail
                    if (detail != null) {
                        DetailScreen(
                            detail = detail,
                            detailSort = state.detailSort,
                            currentTrackId = state.currentTrack?.id,
                            offlineTrackIds = state.offlineTrackIds,
                            downloadProgress = state.downloadProgress,
                            listState = detailListState,
                            onBack = { store.closeDetail() },
                            onPlayQueue = { list, idx, ctx -> store.playQueue(list, idx, ctx) },
                            onShuffleQueue = { list, ctx -> store.shuffleQueue(list, ctx) },
                            onCycleSort = { store.cycleDetailSort() },
                            onDeduplicate = { store.deduplicatePlaylist(detail.id) },
                            onSyncToAccount = { store.pushLocalPlaylistToCloud(detail.id) },
                            onDownloadCollection = { store.downloadTracksOffline(detail.tracks) },
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
                    }
                }
            }
            }
        }

        // Layer 2: Floating Optical Liquid Glass Dock + Mini-Player Pill (unmounted while FullPlayer covers)
        AnimatedVisibility(
            visible = !showFullPlayer,
            enter = fadeIn(tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)),
            exit = fadeOut(tween(durationMillis = 140)),
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .navigationBarsPadding(),
        ) {
            FloatingLiquidGlassDock(
                state = state,
                progressFlow = store.playbackProgress,
                activeTab = activeTab,
                isDetailOpen = state.activeDetail != null,
                isCollapsed = dockCollapsed,
                onSelectTab = { tab ->
                    store.closeDetail()
                    activeTab = tab
                    dockCollapsed = false
                },
                onExpandDock = { dockCollapsed = false },
                onTogglePlayPause = { store.togglePlayPause() },
                onNext = { store.next() },
                onPrev = { store.prev() },
                onDismissStop = { store.stopAndDismissMiniPlayer() },
                onToggleLike = { id -> store.toggleLike(id) },
                onOpenFullPlayer = { showFullPlayer = true },
                onOpenConnect = { showConnectSheet = true },
            )
        }

        // Layer 3: Full Player rendered as sibling overlay (preserves all tab scroll positions!)
        AnimatedVisibility(
            visible = showFullPlayer,
            enter = slideInVertically(animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)) { it / 2 } +
                fadeIn(tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)),
            exit = slideOutVertically(animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)) { it / 2 } +
                fadeOut(tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)),
        ) {
            FullPlayerScreen(
                state = state,
                progressFlow = store.playbackProgress,
                onCollapse = { showFullPlayer = false },
                onTogglePlayPause = { store.togglePlayPause() },
                onNext = { store.next() },
                onPrev = { store.prev() },
                onSeek = { pos -> store.seekTo(pos) },
                onSetVolume = { id, vol -> store.setConnectVolume(id, vol) },
                onToggleShuffle = { store.toggleShuffle() },
                onCycleRepeat = { store.cycleRepeat() },
                onToggleLike = { id -> store.toggleLike(id) },
                onSetSleepTimerMinutes = { mins -> store.setSleepTimerMinutes(mins) },
                onSetPlaybackSpeed = { spd ->
                    state.dspSettings?.let { dsp -> store.updateDsp(dsp.copy(playbackSpeed = spd)) }
                },
                onOpenArtist = { artist -> store.openArtistDetail(artist) },
                onOpenQueue = { showQueueSheet = true },
                onOpenEqualizer = { showEqSheet = true },
                onOpenThemeStudio = { showThemeSheet = true },
                onOpenConnect = { showConnectSheet = true },
                onOpenShareCard = {
                    shareCardInitialPosMs = store.playbackProgress.value.positionMs
                    showShareCardSheet = true
                },
                onOpenOutputSwitcher = onOpenOutputSwitcher,
                onLoadLyrics = { store.loadLyricsForCurrent() },
                onNudgeLyricsOffset = { delta -> store.nudgeLyricsOffsetMs(delta) },
                    onResolveVideo = { store.resolveVideoForCurrent() },
                )
            }
        }

        // Layer 4: In-Window Optical Liquid Glass Sheet Portal matching Home Pill optics
        FerricSheetPortalOverlay(
            host = sheetHost,
            sheetBackdrop = sheetBackdrop,
        )

        // Layer 5: Top-most theme-synced floating toast pill above dock, sheets & FullPlayer
        val hasActiveTrackForDock = !state.miniPlayerDismissed && (
            state.currentTrack != null ||
                (!state.connectState.isLocalActiveSpeaker && state.connectState.peers.any { it.connected })
            )
        val snackBottomPad = when {
            showFullPlayer || hasActiveSheets -> 24.dp
            dockCollapsed && hasActiveTrackForDock -> 84.dp
            dockCollapsed -> 82.dp
            hasActiveTrackForDock -> 158.dp
            else -> 88.dp
        }
        val snackBackdrop = when {
            hasActiveSheets && sheetBackdrop.hasRecorded -> sheetBackdrop
            !showFullPlayer && backdropState.hasRecorded -> backdropState
            else -> null
        }
        CompositionLocalProvider(LocalBackdropState provides snackBackdrop) {
            SnackbarHost(
                hostState = snackbarHostState,
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .navigationBarsPadding()
                    .padding(bottom = snackBottomPad, start = 16.dp, end = 16.dp),
            ) { data ->
                FerricSnackbar(snackbarData = data)
            }
        }

        // Layer 6: Cold-start Auralys-style animated Ferric Type-I cassette splash intro
        if (showLaunchSplash) {
            FerricLaunchSplash(onFinished = { showLaunchSplash = false })
        }
    }

    if (showQueueSheet) {
        QueueSheet(
            state = state,
            progressFlow = store.playbackProgress,
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
            onLoadMoreQueue = { store.extendQueueUpNext() },
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
            onPreviewTheme = { theme -> store.previewTheme(theme) },
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

    if (showShareCardSheet && state.activeDisplayTrack() != null) {
        ShareCardSheet(
            state = state,
            initialPositionMs = shareCardInitialPosMs,
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
            containerColor = ferricDialogContainerColor(palette),
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
}

fun formatDurationMs(ms: Long): String {
    val totalSec = (ms / 1_000L).coerceAtLeast(0L)
    val mins = totalSec / 60L
    val secs = totalSec % 60L
    return String.format(Locale.US, "%d:%02d", mins, secs)
}
