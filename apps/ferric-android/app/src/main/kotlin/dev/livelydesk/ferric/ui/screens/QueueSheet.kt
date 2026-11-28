package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ArrowDownward
import androidx.compose.material.icons.rounded.ArrowUpward
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.DeleteSweep
import androidx.compose.material.icons.rounded.Repeat
import androidx.compose.material.icons.rounded.RepeatOne
import androidx.compose.material.icons.rounded.Save
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.livelydesk.ferric.core.PlaybackProgress
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.formatDurationMs
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.theme.FerricModalBottomSheet
import dev.livelydesk.ferric.ui.theme.FerricSlimSlider
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricDialogContainerColor
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import java.util.Locale
import kotlinx.coroutines.flow.StateFlow
import uniffi.ferric_mobile_ffi.FfiConnectPeer
import uniffi.ferric_mobile_ffi.FfiRepeatMode
import uniffi.ferric_mobile_ffi.FfiTrack
import uniffi.ferric_mobile_ffi.FfiTrackSource

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun QueueSheet(
    state: PlaybackUiState,
    progressFlow: StateFlow<PlaybackProgress>,
    onDismiss: () -> Unit,
    onSeek: (Long) -> Unit,
    onToggleShuffle: () -> Unit,
    onCycleRepeat: () -> Unit,
    onJumpUpcoming: (Int) -> Unit,
    onMoveUp: (Int) -> Unit,
    onMoveDown: (Int) -> Unit,
    onRemove: (Int) -> Unit,
    onClearUpNext: () -> Unit,
    onSaveQueue: (String) -> Unit,
    onCopyQueue: () -> Unit,
    onLoadMoreQueue: () -> Unit = {},
) {
    val palette = LocalFerricPalette.current
    var showSaveDialog by remember { mutableStateOf(false) }
    var saveName by remember { mutableStateOf("Queue Mix") }
    val listState = rememberLazyListState()

    val activeRemote: FfiConnectPeer? = if (!state.connectState.isLocalActiveSpeaker) {
        state.connectState.peers.firstOrNull {
            it.connected && it.deviceId == state.connectState.activeSpeakerId
        }
    } else {
        null
    }
    val isRemote = activeRemote != null
    val remoteQueue = state.connectState.activeRemoteQueue
    val remoteIdx = state.connectState.activeRemoteQueueIndex.toInt()
        .coerceIn(0, (remoteQueue.size - 1).coerceAtLeast(0))

    val displayCurrent: FfiTrack? = if (isRemote) {
        remoteQueue.getOrNull(remoteIdx) ?: activeRemote.nowPlayingTitle.takeIf { it.isNotBlank() }?.let { t ->
            FfiTrack(
                id = activeRemote.activeTrackId ?: "remote:${activeRemote.deviceId}",
                title = t,
                artist = activeRemote.nowPlayingArtist,
                album = "",
                durationMs = activeRemote.durationMs,
                path = null,
                source = FfiTrackSource.YOU_TUBE_MUSIC,
                codec = "",
                sampleRate = 48000u,
                bitDepth = 16u,
                bitrateKbps = 128u,
                spotifyId = null,
                youtubeVideoId = null,
                thumbnailUrl = activeRemote.nowPlayingThumbnailUrl,
                replayGainDb = 0,
                liked = false,
            )
        }
    } else {
        state.currentTrack
    }
    val displayUpNext: List<FfiTrack> = if (isRemote) {
        if (remoteIdx + 1 < remoteQueue.size) remoteQueue.drop(remoteIdx + 1) else emptyList()
    } else {
        state.upNext
    }
    val displayPlaying = activeRemote?.isPlaying ?: state.isPlaying
    val displayShuffle = activeRemote?.shuffleEnabled ?: state.shuffle
    val displayRepeat = activeRemote?.repeatMode ?: state.repeatMode

    val nearEnd by remember {
        derivedStateOf {
            val layout = listState.layoutInfo
            val lastVisible = layout.visibleItemsInfo.lastOrNull()?.index ?: 0
            layout.totalItemsCount > 0 && lastVisible >= (layout.totalItemsCount - 6)
        }
    }
    LaunchedEffect(nearEnd, displayUpNext.size, state.isExtendingQueue, isRemote) {
        if (!isRemote && nearEnd && !state.isExtendingQueue && displayCurrent != null) {
            onLoadMoreQueue()
        }
    }

    val totalSongs = (if (displayCurrent != null) 1 else 0) + displayUpNext.size
    val totalMs = (displayCurrent?.durationMs?.toLong() ?: 0L) +
        displayUpNext.sumOf { it.durationMs.toLong() }

    // Stable occurrence-counted keys so moving rows does not break item identity.
    val queueKeys = remember(displayUpNext) {
        val counts = HashMap<String, Int>(displayUpNext.size)
        displayUpNext.map { t ->
            val occ = (counts[t.id] ?: 0) + 1
            counts[t.id] = occ
            "${t.id}#$occ"
        }
    }

    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    FerricModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        coverShade = state.coverShade,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 18.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column {
                    Text(
                        text = if (isRemote) "Queue · ${activeRemote.deviceName}" else "Queue",
                        style = MaterialTheme.typography.headlineSmall,
                        color = palette.textPrimary,
                    )
                    Text(
                        text = "$totalSongs song(s) · ${formatDurationMs(totalMs)}",
                        style = MaterialTheme.typography.bodyMedium,
                        color = palette.textSecondary,
                    )
                }
                Row {
                    IconButton(onClick = onToggleShuffle) {
                        Icon(
                            imageVector = Icons.Rounded.Shuffle,
                            contentDescription = "Shuffle",
                            tint = if (displayShuffle) palette.accent else palette.textSecondary,
                        )
                    }
                    IconButton(onClick = onCycleRepeat) {
                        Icon(
                            imageVector = if (displayRepeat == FfiRepeatMode.ONE) {
                                Icons.Rounded.RepeatOne
                            } else {
                                Icons.Rounded.Repeat
                            },
                            contentDescription = "Repeat",
                            tint = if (displayRepeat != FfiRepeatMode.OFF) palette.accent else palette.textSecondary,
                        )
                    }
                }
            }

            displayCurrent?.let { cur ->
                androidx.compose.foundation.layout.Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .ferricGlassSurface(
                            palette = palette,
                            cornerRadius = 16.dp,
                            accentGlow = palette.accent,
                            fallbackColor = palette.elevated,
                        ),
                ) {
                    Column(modifier = Modifier.padding(12.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            ArtworkThumb(
                                url = cur.thumbnailUrl,
                                size = 48.dp,
                                cornerRadius = 10.dp,
                                isActive = displayPlaying,
                            )
                            Spacer(Modifier.width(12.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    text = cur.title,
                                    style = MaterialTheme.typography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    color = palette.textPrimary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                Text(
                                    text = cur.artist,
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = palette.textSecondary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                        QueueNowPlayingSlider(
                            progressFlow = progressFlow,
                            onSeek = onSeek,
                        )
                    }
                }
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "Up Next (${displayUpNext.size})",
                    style = MaterialTheme.typography.titleMedium,
                    color = palette.textPrimary,
                )
                if (!isRemote) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        TextButton(onClick = { showSaveDialog = true }) {
                            Icon(Icons.Rounded.Save, contentDescription = null, tint = palette.accent, modifier = Modifier.size(17.dp))
                            Spacer(Modifier.width(4.dp))
                            Text("Save", color = palette.accent)
                        }
                        TextButton(onClick = onCopyQueue) {
                            Icon(Icons.Rounded.ContentCopy, contentDescription = null, tint = palette.textSecondary, modifier = Modifier.size(16.dp))
                            Spacer(Modifier.width(4.dp))
                            Text("Copy", color = palette.textSecondary)
                        }
                        if (displayUpNext.isNotEmpty()) {
                            IconButton(onClick = onClearUpNext) {
                                Icon(Icons.Rounded.DeleteSweep, contentDescription = "Clear Up Next", tint = palette.textSecondary)
                            }
                        }
                    }
                }
            }

            if (displayUpNext.isEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(
                        text = if (!isRemote && state.isExtendingQueue) {
                            "Loading personalized Up Next tracks..."
                        } else if (isRemote) {
                            "Up Next queue from ${activeRemote.deviceName} is empty."
                        } else {
                            "Queue is empty. Add tracks or load more recommendations."
                        },
                        color = palette.textSecondary,
                    )
                    if (!isRemote && displayCurrent != null && !state.isExtendingQueue) {
                        TextButton(onClick = onLoadMoreQueue) {
                            Text("Load Up Next Radio", color = palette.accent)
                        }
                    }
                }
                Spacer(Modifier.height(16.dp))
            } else {
                LazyColumn(
                    state = listState,
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                    modifier = Modifier.heightIn(max = 460.dp),
                ) {
                    itemsIndexed(displayUpNext, key = { idx, _ -> queueKeys[idx] }) { idx, track ->
                        val rowRadius = (palette.cornerRadiusDp.value * 0.6f).coerceIn(4f, 14f).dp
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(rowRadius))
                                .clickable { onJumpUpcoming(idx) }
                                .padding(start = 6.dp, end = 2.dp, top = 6.dp, bottom = 6.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                text = String.format(Locale.US, "%02d", idx + 2),
                                style = MaterialTheme.typography.labelSmall,
                                color = palette.textSecondary,
                                modifier = Modifier.width(28.dp),
                            )
                            ArtworkThumb(
                                url = track.thumbnailUrl,
                                size = 42.dp,
                                cornerRadius = (rowRadius.value * 0.7f).coerceIn(4f, 10f).dp,
                            )
                            Spacer(Modifier.width(10.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    text = track.title,
                                    style = MaterialTheme.typography.titleMedium,
                                    color = palette.textPrimary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                Text(
                                    text = track.artist,
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = palette.textSecondary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                            if (track.durationMs > 0uL) {
                                Text(
                                    text = formatDurationMs(track.durationMs.toLong()),
                                    style = MaterialTheme.typography.labelSmall,
                                    color = palette.textSecondary,
                                    modifier = Modifier.padding(horizontal = 4.dp),
                                )
                            }
                            if (!isRemote) {
                                IconButton(
                                    onClick = { onMoveUp(idx) },
                                    enabled = idx > 0,
                                    modifier = Modifier.size(30.dp),
                                ) {
                                    Icon(
                                        Icons.Rounded.ArrowUpward,
                                        contentDescription = "Move up",
                                        tint = if (idx > 0) palette.textPrimary else palette.border,
                                        modifier = Modifier.size(16.dp),
                                    )
                                }
                                IconButton(
                                    onClick = { onMoveDown(idx) },
                                    enabled = idx < displayUpNext.lastIndex,
                                    modifier = Modifier.size(30.dp),
                                ) {
                                    Icon(
                                        Icons.Rounded.ArrowDownward,
                                        contentDescription = "Move down",
                                        tint = if (idx < displayUpNext.lastIndex) palette.textPrimary else palette.border,
                                        modifier = Modifier.size(16.dp),
                                    )
                                }
                                IconButton(
                                    onClick = { onRemove(idx) },
                                    modifier = Modifier.size(30.dp),
                                ) {
                                    Icon(
                                        Icons.Rounded.Close,
                                        contentDescription = "Remove from queue",
                                        tint = palette.textSecondary,
                                        modifier = Modifier.size(16.dp),
                                    )
                                }
                            }
                        }
                    }

                    if (!isRemote) {
                        item(key = "queue_infinite_footer") {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = 12.dp),
                            horizontalArrangement = Arrangement.Center,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            if (state.isExtendingQueue) {
                                CircularProgressIndicator(
                                    modifier = Modifier.size(18.dp),
                                    strokeWidth = 2.dp,
                                    color = palette.accent,
                                )
                                Spacer(Modifier.width(8.dp))
                                Text(
                                    text = "Loading more Up Next...",
                                    style = MaterialTheme.typography.labelMedium,
                                    color = palette.textSecondary,
                                )
                            } else {
                                TextButton(onClick = onLoadMoreQueue) {
                                    Text(
                                        text = "Load more Up Next",
                                        style = MaterialTheme.typography.labelLarge,
                                        color = palette.accent,
                                    )
                                }
                            }
                        }
                    }
                    }
                }
            }
            Spacer(Modifier.height(14.dp))
        }
    }

    if (showSaveDialog) {
        AlertDialog(
            onDismissRequest = { showSaveDialog = false },
            containerColor = ferricDialogContainerColor(palette),
            title = { Text("Save Queue as Playlist", color = palette.textPrimary) },
            text = {
                OutlinedTextField(
                    value = saveName,
                    onValueChange = { saveName = it },
                    singleLine = true,
                    label = { Text("Playlist name") },
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        if (saveName.isNotBlank()) {
                            onSaveQueue(saveName.trim())
                            showSaveDialog = false
                        }
                    },
                ) { Text("Save", color = palette.accent) }
            },
            dismissButton = {
                TextButton(onClick = { showSaveDialog = false }) {
                    Text("Cancel", color = palette.textSecondary)
                }
            },
        )
    }
}

@Composable
private fun QueueNowPlayingSlider(
    progressFlow: StateFlow<PlaybackProgress>,
    onSeek: (Long) -> Unit,
) {
    val palette = LocalFerricPalette.current
    val progress by progressFlow.collectAsStateWithLifecycle()
    val maxDur = progress.durationMs.coerceAtLeast(1L).toFloat()
    var isDragging by remember { mutableStateOf(false) }
    var draftPos by remember { mutableFloatStateOf(progress.positionMs.toFloat()) }
    val shownPos = if (isDragging) draftPos else progress.positionMs.toFloat()

    FerricSlimSlider(
        value = shownPos.coerceIn(0f, maxDur),
        onValueChange = {
            isDragging = true
            draftPos = it
        },
        onValueChangeFinished = {
            val target = draftPos.toLong().coerceAtLeast(0L)
            isDragging = false
            onSeek(target)
        },
        valueRange = 0f..maxDur,
    )
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            text = formatDurationMs(shownPos.toLong()),
            style = MaterialTheme.typography.labelSmall,
            color = palette.textSecondary,
        )
        Text(
            text = formatDurationMs(progress.durationMs),
            style = MaterialTheme.typography.labelSmall,
            color = palette.textSecondary,
        )
    }
}
