package dev.livelydesk.ferric.ui.components

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FavoriteBorder
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.Home
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import coil3.compose.AsyncImage
import dev.livelydesk.ferric.core.PlaybackProgress
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricLiquidGlass
import kotlin.math.abs
import kotlinx.coroutines.flow.StateFlow
import uniffi.ferric_mobile_ffi.FfiPlaylist
import uniffi.ferric_mobile_ffi.FfiTrack
import uniffi.ferric_mobile_ffi.FfiTrackSource

enum class FerricTab(val label: String, val icon: ImageVector) {
    HOME("Home", Icons.Rounded.Home),
    SEARCH("Search", Icons.Rounded.Search),
    LIBRARY("Library", Icons.Rounded.LibraryMusic),
    YOU("You", Icons.Rounded.Person),
}

fun sourceBadgeLabel(source: FfiTrackSource): String = when (source) {
    FfiTrackSource.LOCAL -> "Local"
    FfiTrackSource.SPOTIFY -> "Spotify"
    FfiTrackSource.YOU_TUBE_MUSIC -> "YTM"
    FfiTrackSource.SUBSONIC -> "Subsonic"
    FfiTrackSource.P2P_HOME -> "Connect"
}

@Composable
fun ArtworkThumb(
    url: String?,
    size: Dp,
    cornerRadius: Dp = 10.dp,
    isActive: Boolean = false,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    Box(
        modifier = modifier
            .size(size)
            .clip(RoundedCornerShape(cornerRadius))
            .background(
                if (isActive) {
                    palette.accent.copy(alpha = 0.20f)
                } else {
                    palette.elevated
                },
            ),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            imageVector = if (isActive) Icons.Rounded.GraphicEq else Icons.Rounded.MusicNote,
            contentDescription = null,
            tint = if (isActive) palette.accent else palette.textSecondary,
            modifier = Modifier.size((size.value * 0.42f).dp),
        )
        if (!url.isNullOrBlank()) {
            AsyncImage(
                model = url,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
            )
        }
    }
}

/**
 * Clean borderless Auralys/Spotify-grade track row with subtle active tint, inline download
 * progress indicator, and full palette theming across Light, AMOLED, Obsidian, and Liquid Glass.
 */
@Composable
fun TrackRow(
    track: FfiTrack,
    isActive: Boolean,
    onPlay: () -> Unit,
    onToggleLike: () -> Unit,
    onPlayNext: () -> Unit,
    onAddToQueue: () -> Unit,
    onAddToPlaylist: () -> Unit,
    onCopyLink: () -> Unit,
    onOpenArtist: () -> Unit,
    onRemoveFromPlaylist: (() -> Unit)? = null,
    isOfflineDownloaded: Boolean = false,
    downloadPercent: Int? = null,
    onDownloadOffline: (() -> Unit)? = null,
    onDeleteOffline: (() -> Unit)? = null,
    compact: Boolean = false,
    showLikeButton: Boolean = true,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    var menuExpanded by remember { mutableStateOf(false) }

    val rowBg = if (isActive) {
        palette.accent.copy(alpha = if (palette.isLight) 0.12f else 0.16f)
    } else {
        Color.Transparent
    }

    Row(
        modifier = modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(rowBg)
            .clickable(onClick = onPlay)
            .padding(
                start = if (compact) 4.dp else 6.dp,
                end = 2.dp,
                top = if (compact) 5.dp else 7.dp,
                bottom = if (compact) 5.dp else 7.dp,
            ),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ArtworkThumb(
            url = track.thumbnailUrl,
            size = if (compact) 46.dp else 50.dp,
            cornerRadius = if (compact) 8.dp else 10.dp,
            isActive = isActive,
        )
        Spacer(Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = track.title,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = if (isActive) FontWeight.SemiBold else FontWeight.Medium,
                color = if (isActive) palette.accent else palette.textPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Spacer(Modifier.height(2.dp))
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                if (downloadPercent != null && downloadPercent in 0..99) {
                    CircularProgressIndicator(
                        progress = { (downloadPercent / 100f).coerceIn(0.05f, 1f) },
                        modifier = Modifier.size(12.dp),
                        strokeWidth = 1.8.dp,
                        color = palette.accent,
                        trackColor = palette.border,
                    )
                } else if (isOfflineDownloaded) {
                    Icon(
                        imageVector = Icons.Rounded.CheckCircle,
                        contentDescription = "Downloaded offline",
                        tint = palette.accent,
                        modifier = Modifier.size(13.dp),
                    )
                }
                Text(
                    text = buildString {
                        append(track.artist)
                        if (!compact) {
                            append(" · ")
                            append(sourceBadgeLabel(track.source))
                        }
                    },
                    style = MaterialTheme.typography.bodyMedium,
                    color = palette.textSecondary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        if (showLikeButton) {
            IconButton(
                onClick = onToggleLike,
                modifier = Modifier.size(38.dp),
            ) {
                Icon(
                    imageVector = if (track.liked) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder,
                    contentDescription = if (track.liked) "Unlike" else "Like",
                    tint = if (track.liked) palette.accent else palette.textSecondary,
                    modifier = Modifier.size(19.dp),
                )
            }
        }
        Box {
            IconButton(
                onClick = { menuExpanded = true },
                modifier = Modifier.size(38.dp),
            ) {
                Icon(
                    imageVector = Icons.Rounded.MoreVert,
                    contentDescription = "Track options",
                    tint = palette.textSecondary,
                    modifier = Modifier.size(19.dp),
                )
            }
            DropdownMenu(
                expanded = menuExpanded,
                onDismissRequest = { menuExpanded = false },
                containerColor = palette.elevated,
            ) {
                DropdownMenuItem(
                    text = { Text("Play Next", color = palette.textPrimary) },
                    onClick = {
                        menuExpanded = false
                        onPlayNext()
                    },
                )
                DropdownMenuItem(
                    text = { Text("Add to Queue", color = palette.textPrimary) },
                    onClick = {
                        menuExpanded = false
                        onAddToQueue()
                    },
                )
                DropdownMenuItem(
                    text = { Text("Add to Playlist", color = palette.textPrimary) },
                    onClick = {
                        menuExpanded = false
                        onAddToPlaylist()
                    },
                )
                DropdownMenuItem(
                    text = { Text("View Artist", color = palette.textPrimary) },
                    onClick = {
                        menuExpanded = false
                        onOpenArtist()
                    },
                )
                DropdownMenuItem(
                    text = { Text("Copy Share Link", color = palette.textPrimary) },
                    onClick = {
                        menuExpanded = false
                        onCopyLink()
                    },
                )
                if (!isOfflineDownloaded && onDownloadOffline != null) {
                    DropdownMenuItem(
                        text = { Text("Download Offline", color = palette.textPrimary) },
                        onClick = {
                            menuExpanded = false
                            onDownloadOffline()
                        },
                    )
                }
                if (isOfflineDownloaded && onDeleteOffline != null) {
                    DropdownMenuItem(
                        text = { Text("Remove Offline Download", color = palette.accent) },
                        onClick = {
                            menuExpanded = false
                            onDeleteOffline()
                        },
                    )
                }
                if (onRemoveFromPlaylist != null) {
                    DropdownMenuItem(
                        text = { Text("Remove from Playlist", color = palette.accent) },
                        onClick = {
                            menuExpanded = false
                            onRemoveFromPlaylist()
                        },
                    )
                }
            }
        }
    }
}

/**
 * Floating optical Liquid Glass bottom dock combining Auralys's scroll-to-collapse morphing pill
 * bar with Ferric's gesture-enabled Mini-Player and Connect indicator.
 *
 * - Expanded state (top of page / scrolling up): Floating Glass Mini-Player capsule stacked cleanly
 *   above the 4-tab (`Home`, `Search`, `Library`, `You`) Liquid Glass Navigation Pill.
 * - Compact scrolled state (scrolling down): The 4-tab pill morphs smoothly into a single circular
 *   glass active-tab orb on the left beside the floating glass Mini-Player pill on the right,
 *   freeing maximum canvas space while artwork glides optically behind both capsules.
 */
@Composable
fun FloatingLiquidGlassDock(
    state: PlaybackUiState,
    progressFlow: StateFlow<PlaybackProgress>,
    activeTab: FerricTab,
    isDetailOpen: Boolean,
    isCollapsed: Boolean,
    onSelectTab: (FerricTab) -> Unit,
    onExpandDock: () -> Unit,
    onTogglePlayPause: () -> Unit,
    onNext: () -> Unit,
    onPrev: () -> Unit,
    onToggleLike: (String) -> Unit,
    onOpenFullPlayer: () -> Unit,
    onOpenConnect: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val hasActiveTrack = state.currentTrack != null ||
        (!state.connectState.isLocalActiveSpeaker && state.connectState.peers.any { it.connected })
    val compactMode = isCollapsed && hasActiveTrack && !isDetailOpen

    AnimatedContent(
        targetState = compactMode,
        transitionSpec = { fadeIn() togetherWith fadeOut() },
        label = "floating_glass_dock",
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 10.dp),
    ) { compact ->
        if (compact) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                // Compact active-tab circular Liquid Glass orb (tap expands full 4-tab pill).
                Box(
                    modifier = Modifier
                        .size(58.dp)
                        .ferricLiquidGlass(
                            cornerRadius = 29.dp,
                            blurRadius = 16.dp,
                            refractionHeight = 16.dp,
                            refractionAmount = 14.dp,
                            chromaticDispersion = 0.62f,
                            alwaysGlass = palette.isLiquidGlass,
                        )
                        .clickable(onClick = onExpandDock),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = activeTab.icon,
                        contentDescription = "Expand navigation",
                        tint = palette.textPrimary,
                        modifier = Modifier.size(24.dp),
                    )
                }

                FloatingMiniPlayerPill(
                    state = state,
                    progressFlow = progressFlow,
                    compact = true,
                    onTogglePlayPause = onTogglePlayPause,
                    onNext = onNext,
                    onPrev = onPrev,
                    onToggleLike = onToggleLike,
                    onOpenFullPlayer = onOpenFullPlayer,
                    onOpenConnect = onOpenConnect,
                    modifier = Modifier.weight(1f),
                )
            }
        } else {
            Column(
                modifier = Modifier.fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                if (hasActiveTrack) {
                    FloatingMiniPlayerPill(
                        state = state,
                        progressFlow = progressFlow,
                        compact = false,
                        onTogglePlayPause = onTogglePlayPause,
                        onNext = onNext,
                        onPrev = onPrev,
                        onToggleLike = onToggleLike,
                        onOpenFullPlayer = onOpenFullPlayer,
                        onOpenConnect = onOpenConnect,
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
                FloatingNavPill(
                    activeTab = activeTab,
                    isDetailOpen = isDetailOpen,
                    onSelectTab = onSelectTab,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}

@Composable
private fun FloatingNavPill(
    activeTab: FerricTab,
    isDetailOpen: Boolean,
    onSelectTab: (FerricTab) -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val pillHighlight = if (palette.isLight) {
        Color.Black.copy(alpha = 0.09f)
    } else {
        Color.White.copy(alpha = 0.14f)
    }

    Row(
        modifier = modifier
            .height(64.dp)
            .ferricLiquidGlass(
                cornerRadius = 32.dp,
                blurRadius = 18.dp,
                refractionHeight = 20.dp,
                refractionAmount = 16.dp,
                chromaticDispersion = 0.65f,
                alwaysGlass = palette.isLiquidGlass,
            )
            .padding(horizontal = 6.dp, vertical = 5.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceEvenly,
    ) {
        FerricTab.entries.forEach { tab ->
            val selected = activeTab == tab && !isDetailOpen
            val contentColor by animateColorAsState(
                targetValue = if (selected) {
                    if (palette.isLiquidGlass) palette.textPrimary else palette.accent
                } else {
                    palette.textSecondary
                },
                label = "nav_tab_color",
            )
            val indicatorPad by animateDpAsState(
                targetValue = if (selected) 0.dp else 4.dp,
                animationSpec = spring(stiffness = Spring.StiffnessMediumLow),
                label = "nav_tab_pad",
            )
            val interactionSource = remember { MutableInteractionSource() }

            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxSize()
                    .padding(horizontal = indicatorPad)
                    .clip(RoundedCornerShape(26.dp))
                    .background(if (selected) pillHighlight else Color.Transparent)
                    .clickable(
                        interactionSource = interactionSource,
                        indication = null,
                        onClick = { onSelectTab(tab) },
                    ),
                contentAlignment = Alignment.Center,
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Icon(
                        imageVector = tab.icon,
                        contentDescription = tab.label,
                        tint = contentColor,
                        modifier = Modifier.size(22.dp),
                    )
                    Spacer(Modifier.height(2.dp))
                    Text(
                        text = tab.label,
                        fontSize = 11.sp,
                        fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Medium,
                        color = contentColor,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
            }
        }
    }
}

@Composable
private fun FloatingMiniPlayerPill(
    state: PlaybackUiState,
    progressFlow: StateFlow<PlaybackProgress>,
    compact: Boolean,
    onTogglePlayPause: () -> Unit,
    onNext: () -> Unit,
    onPrev: () -> Unit,
    onToggleLike: (String) -> Unit,
    onOpenFullPlayer: () -> Unit,
    onOpenConnect: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val cur = state.currentTrack
    val activeRemotePeer = if (!state.connectState.isLocalActiveSpeaker) {
        state.connectState.peers.firstOrNull {
            it.connected && it.deviceId == state.connectState.activeSpeakerId
        }
    } else {
        null
    }
    val hasConnectedPeer = state.connectState.peers.any { it.connected }
    val displayTitle = activeRemotePeer?.nowPlayingTitle?.ifBlank { null } ?: cur?.title ?: "Ready to play"
    val displaySubtitle = when {
        activeRemotePeer != null -> "On ${activeRemotePeer.deviceName} · ${activeRemotePeer.nowPlayingArtist}"
        cur != null -> cur.artist
        else -> "Tap a track to start"
    }
    val displayThumb = activeRemotePeer?.nowPlayingThumbnailUrl ?: cur?.thumbnailUrl
    val displayPlaying = activeRemotePeer?.isPlaying ?: state.isPlaying

    Box(
        modifier = modifier
            .height(60.dp)
            .ferricLiquidGlass(
                cornerRadius = 30.dp,
                blurRadius = 18.dp,
                refractionHeight = 18.dp,
                refractionAmount = 15.dp,
                chromaticDispersion = 0.60f,
                alwaysGlass = palette.isLiquidGlass,
            )
            .pointerInput(cur?.id, activeRemotePeer?.deviceId) {
                var dragX = 0f
                var dragY = 0f
                detectDragGestures(
                    onDragStart = {
                        dragX = 0f
                        dragY = 0f
                    },
                    onDragEnd = {
                        if (dragY < -52f && abs(dragY) > abs(dragX)) {
                            onOpenFullPlayer()
                        } else if (dragX < -68f) {
                            onNext()
                        } else if (dragX > 68f) {
                            onPrev()
                        }
                        dragX = 0f
                        dragY = 0f
                    },
                    onDragCancel = {
                        dragX = 0f
                        dragY = 0f
                    },
                    onDrag = { _, dragAmount ->
                        dragX += dragAmount.x
                        dragY += dragAmount.y
                    },
                )
            }
            .clickable(onClick = onOpenFullPlayer),
    ) {
        Column(modifier = Modifier.fillMaxSize()) {
            Row(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .padding(start = 8.dp, end = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ArtworkThumb(
                    url = displayThumb,
                    size = 44.dp,
                    cornerRadius = 22.dp,
                    isActive = displayPlaying,
                )
                Spacer(Modifier.width(10.dp))
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = displayTitle,
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                        color = palette.textPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = displaySubtitle,
                        style = MaterialTheme.typography.bodyMedium,
                        color = if (activeRemotePeer != null) palette.accent else palette.textSecondary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                if (!compact && (hasConnectedPeer || activeRemotePeer != null)) {
                    IconButton(
                        onClick = onOpenConnect,
                        modifier = Modifier.size(36.dp),
                    ) {
                        Icon(
                            imageVector = Icons.Rounded.Devices,
                            contentDescription = "Ferric Connect",
                            tint = palette.accent,
                            modifier = Modifier.size(19.dp),
                        )
                    }
                } else if (!compact && cur != null) {
                    IconButton(
                        onClick = { onToggleLike(cur.id) },
                        modifier = Modifier.size(36.dp),
                    ) {
                        Icon(
                            imageVector = if (cur.liked) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder,
                            contentDescription = if (cur.liked) "Unlike" else "Like",
                            tint = if (cur.liked) palette.accent else palette.textSecondary,
                            modifier = Modifier.size(19.dp),
                        )
                    }
                }
                Surface(
                    shape = CircleShape,
                    color = if (palette.isLight) palette.textPrimary else Color.White,
                    modifier = Modifier.size(38.dp),
                ) {
                    IconButton(onClick = onTogglePlayPause) {
                        Icon(
                            imageVector = if (displayPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                            contentDescription = if (displayPlaying) "Pause" else "Play",
                            tint = if (palette.isLight) Color.White else Color.Black,
                            modifier = Modifier.size(21.dp),
                        )
                    }
                }
                IconButton(
                    onClick = onNext,
                    modifier = Modifier.size(36.dp),
                ) {
                    Icon(
                        imageVector = Icons.Rounded.SkipNext,
                        contentDescription = "Next",
                        tint = palette.textPrimary,
                        modifier = Modifier.size(22.dp),
                    )
                }
            }
            MiniProgressBar(
                progressFlow = progressFlow,
                remotePositionMs = activeRemotePeer?.positionMs?.toLong(),
                remoteDurationMs = activeRemotePeer?.durationMs?.toLong(),
                accent = palette.accent,
                trackColor = palette.textSecondary.copy(alpha = 0.16f),
            )
        }
    }
}

@Composable
private fun MiniProgressBar(
    progressFlow: StateFlow<PlaybackProgress>,
    remotePositionMs: Long?,
    remoteDurationMs: Long?,
    accent: Color,
    trackColor: Color,
) {
    val progress by progressFlow.collectAsStateWithLifecycle()
    val frac = if (remoteDurationMs != null && remoteDurationMs > 0L && remotePositionMs != null) {
        (remotePositionMs.toFloat() / remoteDurationMs.toFloat()).coerceIn(0f, 1f)
    } else if (progress.durationMs > 0L) {
        (progress.positionMs.toFloat() / progress.durationMs.toFloat()).coerceIn(0f, 1f)
    } else {
        0f
    }
    LinearProgressIndicator(
        progress = { frac },
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 24.dp)
            .height(2.dp)
            .clip(RoundedCornerShape(1.dp)),
        color = accent,
        trackColor = trackColor,
    )
}

@Composable
fun AddToPlaylistDialog(
    track: FfiTrack,
    playlists: List<FfiPlaylist>,
    onDismiss: () -> Unit,
    onSelectPlaylist: (String) -> Unit,
    onCreateNewPlaylist: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = palette.elevated,
        title = {
            Text(
                text = "Add \"${track.title}\"",
                color = palette.textPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        },
        text = {
            if (playlists.isEmpty()) {
                Text(
                    text = "No playlists yet. Create one first.",
                    color = palette.textSecondary,
                )
            } else {
                LazyColumn(
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.height(240.dp),
                ) {
                    items(playlists, key = { it.id }) { pl ->
                        Surface(
                            shape = RoundedCornerShape(12.dp),
                            color = palette.surface,
                            border = BorderStroke(1.dp, palette.border),
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable { onSelectPlaylist(pl.id) },
                        ) {
                            Row(
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Icon(
                                    imageVector = Icons.AutoMirrored.Rounded.PlaylistAdd,
                                    contentDescription = null,
                                    tint = palette.accent,
                                )
                                Spacer(Modifier.width(10.dp))
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(
                                        pl.name,
                                        color = palette.textPrimary,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                    Text(
                                        "${sourceBadgeLabel(pl.source)} · ${pl.trackCount} songs",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = palette.textSecondary,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onCreateNewPlaylist) {
                Text("New Playlist", color = palette.accent)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel", color = palette.textSecondary)
            }
        },
    )
}
