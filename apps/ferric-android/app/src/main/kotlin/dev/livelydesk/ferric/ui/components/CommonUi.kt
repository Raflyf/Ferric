package dev.livelydesk.ferric.ui.components

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
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
import androidx.compose.material3.SnackbarData
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.lerp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import coil3.compose.AsyncImage
import coil3.request.ImageRequest
import coil3.size.Precision
import dev.livelydesk.ferric.core.PlaybackProgress
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.theme.FERRIC_MOTION_MS
import dev.livelydesk.ferric.ui.theme.FerricEaseOut
import dev.livelydesk.ferric.ui.theme.GlassTier
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricDialogContainerColor
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import dev.livelydesk.ferric.ui.theme.ferricLiquidGlass
import kotlin.math.abs
import kotlin.math.roundToInt
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
    val context = LocalContext.current
    val density = LocalDensity.current
    val targetPx = remember(size, density.density) {
        with(density) { size.roundToPx().coerceIn(48, 860) }
    }
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
            val req = remember(url, targetPx) {
                ImageRequest.Builder(context)
                    .data(url)
                    .size(targetPx, targetPx)
                    .precision(Precision.INEXACT)
                    .build()
            }
            AsyncImage(
                model = req,
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
    val activeModifier = if (isActive && palette.isLiquidGlass) {
        modifier
            .fillMaxWidth()
            .ferricGlassSurface(
                palette = palette,
                cornerRadius = 14.dp,
                tier = GlassTier.FROSTED_PANEL,
                accentGlow = palette.accent,
            )
    } else {
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(rowBg)
    }

    Row(
        modifier = activeModifier
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
    onDismissStop: () -> Unit,
    onToggleLike: (String) -> Unit,
    onOpenFullPlayer: () -> Unit,
    onOpenConnect: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val hasActiveTrack = !state.miniPlayerDismissed && (
        state.currentTrack != null ||
            (!state.connectState.isLocalActiveSpeaker && state.connectState.peers.any { it.connected })
        )
    // Collapse works uniformly across all tabs AND Detail / Radio Station screens with one unified curve.
    val compactMode = isCollapsed
    val morphFraction by animateFloatAsState(
        targetValue = if (compactMode) 1f else 0f,
        animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut),
        label = "dock_morph_fraction",
    )
    val miniVisible by animateFloatAsState(
        targetValue = if (hasActiveTrack) 1f else 0f,
        animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut),
        label = "mini_player_visible",
    )

    BoxWithConstraints(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 10.dp),
    ) {
        val fullW = maxWidth
        val shrinkT = (morphFraction * 1.25f).coerceIn(0f, 1f)
        val slideT = ((morphFraction - 0.12f) / 0.88f).coerceIn(0f, 1f)
        val navW = lerp(fullW, 58.dp, shrinkT)
        val navH = lerp(64.dp, 58.dp, morphFraction)
        val extraMiniH = lerp(70.dp, 0.dp, slideT) * miniVisible
        val dockHeight = navH + extraMiniH
        val miniX = lerp(0.dp, 68.dp, slideT)
        val miniBottom = lerp(74.dp, 0.dp, slideT)
        val hideOffsetY = 86.dp * (1f - miniVisible)
        val miniW = (fullW - miniX).coerceAtLeast(120.dp)

        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(dockHeight),
        ) {
            if (miniVisible > 0.01f) {
                FloatingMiniPlayerPill(
                    state = state,
                    progressFlow = progressFlow,
                    compact = morphFraction > 0.45f,
                    onTogglePlayPause = onTogglePlayPause,
                    onNext = onNext,
                    onPrev = onPrev,
                    onDismissStop = onDismissStop,
                    onToggleLike = onToggleLike,
                    onOpenFullPlayer = onOpenFullPlayer,
                    onOpenConnect = onOpenConnect,
                    modifier = Modifier
                        .align(Alignment.BottomStart)
                        .offset(x = miniX, y = -miniBottom + hideOffsetY)
                        .width(miniW),
                )
            }

            FloatingNavPill(
                activeTab = activeTab,
                isDetailOpen = isDetailOpen,
                compactMode = compactMode,
                morphFraction = morphFraction,
                onSelectTab = onSelectTab,
                onExpandDock = onExpandDock,
                modifier = Modifier
                    .align(Alignment.BottomStart)
                    .width(navW)
                    .height(navH),
            )
        }
    }
}

@Composable
private fun FloatingNavPill(
    activeTab: FerricTab,
    isDetailOpen: Boolean,
    compactMode: Boolean,
    morphFraction: Float,
    onSelectTab: (FerricTab) -> Unit,
    onExpandDock: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val density = LocalDensity.current
    val haptic = LocalHapticFeedback.current
    val currentOnSelectTab by rememberUpdatedState(onSelectTab)
    val currentOnExpandDock by rememberUpdatedState(onExpandDock)
    val isCompactNow by rememberUpdatedState(compactMode)
    val currentMorph by rememberUpdatedState(morphFraction)
    val tabs = FerricTab.entries

    var expandedWidthPx by remember { mutableIntStateOf(0) }
    var isDragging by remember { mutableStateOf(false) }
    var rawDragX by remember { mutableFloatStateOf(0f) }

    LaunchedEffect(compactMode) {
        if (compactMode) {
            isDragging = false
        }
    }

    val selectedIndex = tabs.indexOf(activeTab).coerceIn(0, tabs.lastIndex)
    val slotWidthPx = if (expandedWidthPx > 0) expandedWidthPx / tabs.size.toFloat() else 0f
    val targetCenterPx = if (isDragging && slotWidthPx > 0f) {
        rawDragX.coerceIn(slotWidthPx * 0.42f, expandedWidthPx - slotWidthPx * 0.42f)
    } else {
        (selectedIndex + 0.5f) * slotWidthPx
    }

    val animatedCenterPx by animateFloatAsState(
        targetValue = targetCenterPx,
        animationSpec = if (isDragging) {
            tween(durationMillis = 36)
        } else {
            tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)
        },
        label = "nav_lens_center",
    )
    val lensScale by animateFloatAsState(
        targetValue = if (isDragging) 1.15f else 1.0f,
        animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut),
        label = "nav_lens_scale",
    )
    val dragFocus by animateFloatAsState(
        targetValue = if (isDragging) 1f else 0f,
        animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut),
        label = "nav_drag_focus",
    )

    val hoveredIndex = if (isDragging && slotWidthPx > 0f) {
        (rawDragX / slotWidthPx).toInt().coerceIn(0, tabs.lastIndex)
    } else {
        selectedIndex
    }

    val restPillBg = if (palette.isLight) {
        Color.Black.copy(alpha = 0.08f)
    } else if (palette.isLiquidGlass) {
        Color.White.copy(alpha = 0.22f)
    } else {
        Color.White.copy(alpha = 0.14f)
    }
    val glassRadius = lerp(32.dp, 29.dp, morphFraction)
    val expandedAlpha = (1f - morphFraction * 2.15f).coerceIn(0f, 1f)
    val compactAlpha = ((morphFraction - 0.35f) / 0.50f).coerceIn(0f, 1f)

    Box(
        modifier = modifier
            .ferricLiquidGlass(
                cornerRadius = glassRadius,
                blurRadius = 10.dp,
                refractionHeight = 22.dp,
                refractionAmount = 22.dp,
                chromaticDispersion = 0.92f,
                alwaysGlass = palette.isLiquidGlass,
            )
            .onSizeChanged {
                if (!isCompactNow && currentMorph < 0.05f && it.width > 0) {
                    expandedWidthPx = it.width
                } else if (expandedWidthPx == 0 && it.width > 0) {
                    expandedWidthPx = it.width
                }
            }
            .pointerInput(tabs.size) {
                awaitEachGesture {
                    try {
                        val down = awaitFirstDown(requireUnconsumed = false)
                        if (isCompactNow || currentMorph > 0.55f) {
                            while (true) {
                                val event = awaitPointerEvent()
                                val change = event.changes.firstOrNull { it.id == down.id }
                                if (change == null || !change.pressed || event.changes.none { it.pressed }) {
                                    if (change != null &&
                                        change.position.x in 0f..size.width.toFloat() &&
                                        change.position.y in 0f..size.height.toFloat()
                                    ) {
                                        change.consume()
                                        currentOnExpandDock()
                                    }
                                    break
                                }
                            }
                            return@awaitEachGesture
                        }

                        val touchSlop = viewConfiguration.touchSlop
                        val startX = down.position.x
                        val downTime = down.uptimeMillis
                        var dragged = false
                        var lastEmittedIdx = -1
                        rawDragX = startX

                        while (true) {
                            val event = awaitPointerEvent()
                            val change = event.changes.firstOrNull { it.id == down.id }
                            if (change == null || !change.pressed || event.changes.none { it.pressed }) {
                                isDragging = false
                                if (change != null && !change.pressed && event.changes.size <= 1) {
                                    val w = expandedWidthPx.toFloat().coerceAtLeast(size.width.toFloat()).coerceAtLeast(1f)
                                    val idx = ((change.position.x / w) * tabs.size)
                                        .toInt()
                                        .coerceIn(0, tabs.lastIndex)
                                    change.consume()
                                    currentOnSelectTab(tabs[idx])
                                }
                                break
                            }

                            if (event.changes.count { it.pressed } > 1) {
                                isDragging = false
                                break
                            }

                            val movedDist = abs(change.position.x - startX)
                            val heldLong = (change.uptimeMillis - downTime) > 110L
                            val w = expandedWidthPx.toFloat().coerceAtLeast(size.width.toFloat()).coerceAtLeast(1f)
                            if (!dragged && (movedDist > touchSlop * 0.6f || heldLong)) {
                                dragged = true
                                isDragging = true
                                val idx = ((change.position.x / w) * tabs.size)
                                    .toInt()
                                    .coerceIn(0, tabs.lastIndex)
                                lastEmittedIdx = idx
                                haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                currentOnSelectTab(tabs[idx])
                            }
                            if (dragged) {
                                change.consume()
                                rawDragX = change.position.x
                                val idx = ((change.position.x / w) * tabs.size)
                                    .toInt()
                                    .coerceIn(0, tabs.lastIndex)
                                if (idx != lastEmittedIdx) {
                                    lastEmittedIdx = idx
                                    haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    currentOnSelectTab(tabs[idx])
                                }
                            }
                        }
                    } finally {
                        isDragging = false
                    }
                }
            },
    ) {
        if (expandedAlpha > 0.01f) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .graphicsLayer { alpha = expandedAlpha },
            ) {
                // Clear crystal specular lens bubble behind active tab, matching Auralys optics.
                if (slotWidthPx > 0f && (!isDetailOpen || isDragging)) {
                    val bubbleW = with(density) { (slotWidthPx * 0.92f).toDp() }
                    val bubbleOffsetX = (animatedCenterPx - (slotWidthPx * 0.46f)).roundToInt()
                    Box(
                        modifier = Modifier
                            .align(Alignment.CenterStart)
                            .offset { IntOffset(x = bubbleOffsetX, y = 0) }
                            .width(bubbleW)
                            .height(54.dp)
                            .graphicsLayer {
                                scaleX = lensScale
                                scaleY = lensScale
                            }
                            .clip(RoundedCornerShape(27.dp))
                            .background(
                                Brush.verticalGradient(
                                    0.0f to Color.White.copy(alpha = if (palette.isLight) 0.24f else 0.20f + 0.14f * dragFocus),
                                    0.45f to restPillBg.copy(alpha = (0.06f + 0.12f * dragFocus).coerceAtMost(0.24f)),
                                    1.0f to palette.accent.copy(alpha = 0.10f + 0.16f * dragFocus),
                                ),
                            )
                            .border(
                                width = 1.dp,
                                color = Color.White.copy(alpha = (0.42f + 0.36f * dragFocus).coerceAtMost(0.80f)),
                                shape = RoundedCornerShape(27.dp),
                            ),
                    )
                }

                // Tab labels & icons drawn razor-sharp above the crystal lens bubble.
                val glassTextShadow = if (palette.isLiquidGlass && !palette.isLight) {
                    Shadow(color = Color.Black.copy(alpha = 0.55f), offset = Offset(0f, 1f), blurRadius = 4f)
                } else {
                    null
                }
                Row(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(horizontal = 6.dp, vertical = 5.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    tabs.forEachIndexed { idx, tab ->
                        val activeSlot = (if (isDragging) hoveredIndex else selectedIndex) == idx
                        val selected = activeSlot && (!isDetailOpen || isDragging)
                        val iconScale by animateFloatAsState(
                            targetValue = if (isDragging && activeSlot) 1.14f else 1.0f,
                            animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut),
                            label = "nav_tab_scale",
                        )
                        val contentColor by animateColorAsState(
                            targetValue = when {
                                selected && palette.isLiquidGlass -> Color.White
                                selected -> palette.accent
                                palette.isLiquidGlass && !palette.isLight -> Color(0xFFD6DFEE)
                                else -> palette.textSecondary
                            },
                            animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut),
                            label = "nav_tab_color",
                        )
                        Column(
                            modifier = Modifier
                                .weight(1f)
                                .fillMaxHeight()
                                .graphicsLayer {
                                    scaleX = iconScale
                                    scaleY = iconScale
                                },
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
                                style = MaterialTheme.typography.labelSmall.copy(shadow = glassTextShadow),
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

        if (compactAlpha > 0.01f) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        alpha = compactAlpha
                        val s = 0.72f + 0.28f * compactAlpha
                        scaleX = s
                        scaleY = s
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = activeTab.icon,
                    contentDescription = "Expand navigation",
                    tint = palette.textPrimary,
                    modifier = Modifier.size(24.dp),
                )
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
    onDismissStop: () -> Unit,
    onToggleLike: (String) -> Unit,
    onOpenFullPlayer: () -> Unit,
    onOpenConnect: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val haptic = LocalHapticFeedback.current
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

    var visualDragX by remember { mutableFloatStateOf(0f) }
    var visualDragY by remember { mutableFloatStateOf(0f) }
    val animDragX by animateFloatAsState(
        targetValue = visualDragX,
        animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut),
        label = "mini_drag_x",
    )
    val animDragY by animateFloatAsState(
        targetValue = visualDragY,
        animationSpec = tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut),
        label = "mini_drag_y",
    )

    Box(
        modifier = modifier
            .height(60.dp)
            .graphicsLayer {
                translationX = animDragX * 0.42f
                translationY = if (animDragY > 0f) animDragY * 0.65f else animDragY * 0.28f
                alpha = if (animDragY > 0f) (1f - (animDragY / 210f)).coerceIn(0.45f, 1f) else 1f
            }
            .ferricLiquidGlass(
                cornerRadius = 30.dp,
                blurRadius = 8.5.dp,
                refractionHeight = 18.dp,
                refractionAmount = 22.dp,
                chromaticDispersion = 0.90f,
                alwaysGlass = palette.isLiquidGlass,
            )
            .pointerInput(cur?.id, activeRemotePeer?.deviceId) {
                var dragX = 0f
                var dragY = 0f
                detectDragGestures(
                    onDragStart = {
                        dragX = 0f
                        dragY = 0f
                        visualDragX = 0f
                        visualDragY = 0f
                    },
                    onDragEnd = {
                        val absX = abs(dragX)
                        val absY = abs(dragY)
                        when {
                            dragY > 62f && absY > absX -> {
                                haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                                onDismissStop()
                            }
                            dragY < -50f && absY > absX -> onOpenFullPlayer()
                            dragX < -64f -> onNext()
                            dragX > 64f -> onPrev()
                        }
                        dragX = 0f
                        dragY = 0f
                        visualDragX = 0f
                        visualDragY = 0f
                    },
                    onDragCancel = {
                        dragX = 0f
                        dragY = 0f
                        visualDragX = 0f
                        visualDragY = 0f
                    },
                    onDrag = { _, dragAmount ->
                        dragX += dragAmount.x
                        dragY += dragAmount.y
                        visualDragX = dragX.coerceIn(-120f, 120f)
                        visualDragY = dragY.coerceIn(-90f, 130f)
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
                val miniShadow = if (palette.isLiquidGlass && !palette.isLight) {
                    Shadow(color = Color.Black.copy(alpha = 0.55f), offset = Offset(0f, 1f), blurRadius = 4f)
                } else {
                    null
                }
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = displayTitle,
                        style = MaterialTheme.typography.titleMedium.copy(shadow = miniShadow),
                        fontWeight = FontWeight.SemiBold,
                        color = if (palette.isLiquidGlass && !palette.isLight) Color.White else palette.textPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = displaySubtitle,
                        style = MaterialTheme.typography.bodyMedium.copy(shadow = miniShadow),
                        color = when {
                            activeRemotePeer != null -> palette.accent
                            palette.isLiquidGlass && !palette.isLight -> Color(0xFFDCE4F2)
                            else -> palette.textSecondary
                        },
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
                remotePositionMs = null,
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
        containerColor = ferricDialogContainerColor(palette),
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
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .ferricGlassSurface(palette, cornerRadius = 12.dp, fallbackColor = palette.surface)
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

/**
 * Theme-synced floating notification pill replacing default opaque Material3 Snackbar cards.
 * Uses optical AGSL Liquid Glass when Liquid Glass mode is active, and matches solid/AMOLED/Warm
 * Cream surfaces, corner radius, font family, and dynamic accent across every theme.
 */
@Composable
fun FerricSnackbar(
    snackbarData: SnackbarData,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val snackCorner = if (palette.isLiquidGlass) {
        24.dp
    } else {
        (palette.cornerRadiusDp.value * 1.15f).coerceIn(12f, 24f).dp
    }
    val shape = RoundedCornerShape(snackCorner)
    val glassTextShadow = if (palette.isLiquidGlass && !palette.isLight) {
        Shadow(color = Color.Black.copy(alpha = 0.55f), offset = Offset(0f, 1f), blurRadius = 4f)
    } else {
        null
    }
    val containerMod = modifier
        .widthIn(min = 160.dp, max = 420.dp)
        .shadow(
            elevation = if (palette.isLiquidGlass) 12.dp else 8.dp,
            shape = shape,
            ambientColor = Color.Black.copy(alpha = 0.36f),
            spotColor = Color.Black.copy(alpha = 0.44f),
        )
        .let { base ->
            if (palette.isLiquidGlass) {
                base.ferricLiquidGlass(
                    cornerRadius = snackCorner,
                    blurRadius = 9.dp,
                    refractionHeight = 16.dp,
                    refractionAmount = 18.dp,
                    chromaticDispersion = 0.88f,
                    alwaysGlass = true,
                    tier = GlassTier.HERO_LENS,
                    anchorAlphaOverride = if (palette.isLight) 0f else 0.24f,
                )
            } else {
                base
                    .clip(shape)
                    .background(palette.elevated)
                    .border(
                        width = 1.dp,
                        brush = Brush.horizontalGradient(
                            colors = listOf(
                                palette.accent.copy(alpha = if (palette.isLight) 0.42f else 0.50f),
                                palette.border,
                                palette.border,
                            ),
                        ),
                        shape = shape,
                    )
            }
        }
        .clickable { snackbarData.dismiss() }

    Box(modifier = containerMod) {
        Row(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 11.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(7.dp)
                    .clip(CircleShape)
                    .background(palette.accent),
            )
            Spacer(Modifier.width(10.dp))
            Text(
                text = snackbarData.visuals.message,
                style = MaterialTheme.typography.bodyMedium.copy(shadow = glassTextShadow),
                fontWeight = FontWeight.Medium,
                color = if (palette.isLiquidGlass && !palette.isLight) Color.White else palette.textPrimary,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false),
            )
            val actionLabel = snackbarData.visuals.actionLabel
            if (!actionLabel.isNullOrBlank()) {
                Spacer(Modifier.width(12.dp))
                Text(
                    text = actionLabel,
                    style = MaterialTheme.typography.labelLarge.copy(shadow = glassTextShadow),
                    fontWeight = FontWeight.Bold,
                    color = palette.accent,
                    modifier = Modifier.clickable { snackbarData.performAction() },
                )
            }
        }
    }
}
