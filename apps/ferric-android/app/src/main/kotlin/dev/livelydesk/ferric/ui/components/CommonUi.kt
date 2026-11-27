package dev.livelydesk.ferric.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
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
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FavoriteBorder
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material3.AlertDialog
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
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.theme.FerricOrange
import dev.livelydesk.ferric.ui.theme.HairlineBorder
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.LocalHazeState
import dev.livelydesk.ferric.ui.theme.ObsidianElevated
import dev.livelydesk.ferric.ui.theme.ObsidianSurface
import dev.livelydesk.ferric.ui.theme.TextPrimary
import dev.livelydesk.ferric.ui.theme.TextSecondary
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import kotlin.math.abs
import uniffi.ferric_mobile_ffi.FfiPlaylist
import uniffi.ferric_mobile_ffi.FfiTrack
import uniffi.ferric_mobile_ffi.FfiTrackSource

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
    Box(
        modifier = modifier
            .size(size)
            .clip(RoundedCornerShape(cornerRadius))
            .background(if (isActive) FerricOrange.copy(alpha = 0.2f) else ObsidianElevated),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            imageVector = if (isActive) Icons.Rounded.GraphicEq else Icons.Rounded.MusicNote,
            contentDescription = null,
            tint = if (isActive) FerricOrange else TextSecondary,
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
    onDownloadOffline: (() -> Unit)? = null,
    onDeleteOffline: (() -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    var menuExpanded by remember { mutableStateOf(false) }

    Surface(
        shape = RoundedCornerShape(12.dp),
        color = if (isActive) ObsidianElevated else ObsidianSurface,
        border = BorderStroke(1.dp, if (isActive) FerricOrange else HairlineBorder),
        modifier = modifier
            .fillMaxWidth()
            .clickable(onClick = onPlay),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 10.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ArtworkThumb(
                url = track.thumbnailUrl,
                size = 48.dp,
                cornerRadius = 10.dp,
                isActive = isActive,
            )
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = track.title,
                    style = MaterialTheme.typography.titleMedium,
                    color = if (isActive) FerricOrange else TextPrimary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(Modifier.height(2.dp))
                Text(
                    text = buildString {
                        append(track.artist)
                        append(" · ")
                        append(sourceBadgeLabel(track.source))
                        if (isOfflineDownloaded) append(" · Offline")
                    },
                    style = MaterialTheme.typography.bodyMedium,
                    color = TextSecondary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            IconButton(onClick = onToggleLike) {
                Icon(
                    imageVector = if (track.liked) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder,
                    contentDescription = if (track.liked) "Unlike" else "Like",
                    tint = if (track.liked) FerricOrange else TextSecondary,
                    modifier = Modifier.size(20.dp),
                )
            }
            Box {
                IconButton(onClick = { menuExpanded = true }) {
                    Icon(
                        imageVector = Icons.Rounded.MoreVert,
                        contentDescription = "Track options",
                        tint = TextSecondary,
                        modifier = Modifier.size(20.dp),
                    )
                }
                DropdownMenu(
                    expanded = menuExpanded,
                    onDismissRequest = { menuExpanded = false },
                    containerColor = ObsidianElevated,
                ) {
                    DropdownMenuItem(
                        text = { Text("Play Next", color = TextPrimary) },
                        onClick = {
                            menuExpanded = false
                            onPlayNext()
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Add to Queue", color = TextPrimary) },
                        onClick = {
                            menuExpanded = false
                            onAddToQueue()
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Add to Playlist", color = TextPrimary) },
                        onClick = {
                            menuExpanded = false
                            onAddToPlaylist()
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("View Artist", color = TextPrimary) },
                        onClick = {
                            menuExpanded = false
                            onOpenArtist()
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Copy Share Link", color = TextPrimary) },
                        onClick = {
                            menuExpanded = false
                            onCopyLink()
                        },
                    )
                    if (!isOfflineDownloaded && onDownloadOffline != null) {
                        DropdownMenuItem(
                            text = { Text("Download Offline", color = TextPrimary) },
                            onClick = {
                                menuExpanded = false
                                onDownloadOffline()
                            },
                        )
                    }
                    if (isOfflineDownloaded && onDeleteOffline != null) {
                        DropdownMenuItem(
                            text = { Text("Remove Offline Download", color = FerricOrange) },
                            onClick = {
                                menuExpanded = false
                                onDeleteOffline()
                            },
                        )
                    }
                    if (onRemoveFromPlaylist != null) {
                        DropdownMenuItem(
                            text = { Text("Remove from Playlist", color = FerricOrange) },
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
}

@Composable
fun MiniPlayerBar(
    state: PlaybackUiState,
    onTogglePlayPause: () -> Unit,
    onNext: () -> Unit,
    onPrev: () -> Unit,
    onOpenFullPlayer: () -> Unit,
    onOpenQueue: () -> Unit,
    onOpenConnect: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val hazeState = LocalHazeState.current
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
        activeRemotePeer != null -> "Playing on ${activeRemotePeer.deviceName} · ${activeRemotePeer.nowPlayingArtist}"
        cur != null -> "${cur.artist} · ${state.activeOutputRoute}"
        else -> "Swipe up for Player · tap a track"
    }
    val displayThumb = activeRemotePeer?.nowPlayingThumbnailUrl ?: cur?.thumbnailUrl
    val displayPlaying = activeRemotePeer?.isPlaying ?: state.isPlaying
    var dragX by remember { mutableFloatStateOf(0f) }
    var dragY by remember { mutableFloatStateOf(0f) }
    val progress = if (activeRemotePeer != null && activeRemotePeer.durationMs > 0uL) {
        (activeRemotePeer.positionMs.toFloat() / activeRemotePeer.durationMs.toFloat()).coerceIn(0f, 1f)
    } else if (state.durationMs > 0L) {
        (state.positionMs.toFloat() / state.durationMs.toFloat()).coerceIn(0f, 1f)
    } else {
        0f
    }

    Box(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 10.dp, vertical = 6.dp)
            .ferricGlassSurface(
                palette = palette,
                hazeState = hazeState,
                cornerRadius = if (palette.isLiquidGlass) 18.dp else 14.dp,
            )
            .pointerInput(cur?.id) {
                detectDragGestures(
                    onDragEnd = {
                        if (dragY < -56f && abs(dragY) > abs(dragX)) {
                            onOpenFullPlayer()
                        } else if (dragX < -72f) {
                            onNext()
                        } else if (dragX > 72f) {
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
        Column(modifier = Modifier.fillMaxWidth()) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 10.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ArtworkThumb(
                    url = displayThumb,
                    size = 44.dp,
                    cornerRadius = 9.dp,
                    isActive = displayPlaying,
                )
                Spacer(Modifier.width(10.dp))
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = displayTitle,
                        style = MaterialTheme.typography.titleMedium,
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
                IconButton(onClick = onOpenConnect) {
                    Icon(
                        imageVector = Icons.Rounded.Devices,
                        contentDescription = "Ferric Connect",
                        tint = if (hasConnectedPeer) palette.accent else palette.textSecondary,
                    )
                }
                Surface(
                    shape = CircleShape,
                    color = palette.accent,
                    modifier = Modifier.size(40.dp),
                ) {
                    IconButton(onClick = onTogglePlayPause) {
                        Icon(
                            imageVector = if (displayPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                            contentDescription = if (displayPlaying) "Pause" else "Play",
                            tint = if (palette.isLight) androidx.compose.ui.graphics.Color.White else ObsidianSurface,
                        )
                    }
                }
                IconButton(onClick = onNext) {
                    Icon(
                        imageVector = Icons.Rounded.SkipNext,
                        contentDescription = "Next",
                        tint = palette.textPrimary,
                    )
                }
                IconButton(onClick = onOpenQueue) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Rounded.QueueMusic,
                        contentDescription = "Queue",
                        tint = palette.textSecondary,
                    )
                }
            }
            LinearProgressIndicator(
                progress = { progress },
                modifier = Modifier
                    .fillMaxWidth()
                    .height(2.5.dp),
                color = palette.accent,
                trackColor = palette.border,
            )
        }
    }
}

@Composable
fun AddToPlaylistDialog(
    track: FfiTrack,
    playlists: List<FfiPlaylist>,
    onDismiss: () -> Unit,
    onSelectPlaylist: (String) -> Unit,
    onCreateNewPlaylist: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = ObsidianElevated,
        title = {
            Text(
                text = "Add \"${track.title}\"",
                color = TextPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        },
        text = {
            if (playlists.isEmpty()) {
                Text(
                    text = "No playlists yet. Create one first.",
                    color = TextSecondary,
                )
            } else {
                LazyColumn(
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.height(240.dp),
                ) {
                    items(playlists, key = { it.id }) { pl ->
                        Surface(
                            shape = RoundedCornerShape(10.dp),
                            color = ObsidianSurface,
                            border = BorderStroke(1.dp, HairlineBorder),
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
                                    tint = FerricOrange,
                                )
                                Spacer(Modifier.width(10.dp))
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(pl.name, color = TextPrimary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    Text(
                                        "${sourceBadgeLabel(pl.source)} · ${pl.trackCount} songs",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = TextSecondary,
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
                Text("New Playlist", color = FerricOrange)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel", color = TextSecondary)
            }
        },
    )
}
