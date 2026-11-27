package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
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
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.formatDurationMs
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.theme.FerricOrange
import dev.livelydesk.ferric.ui.theme.HairlineBorder
import dev.livelydesk.ferric.ui.theme.ObsidianElevated
import dev.livelydesk.ferric.ui.theme.ObsidianSurface
import dev.livelydesk.ferric.ui.theme.TextPrimary
import dev.livelydesk.ferric.ui.theme.TextSecondary
import uniffi.ferric_mobile_ffi.FfiRepeatMode

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun QueueSheet(
    state: PlaybackUiState,
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
) {
    var showSaveDialog by remember { mutableStateOf(false) }
    var saveName by remember { mutableStateOf("Queue Mix") }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        containerColor = ObsidianSurface,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Now Playing & Queue", style = MaterialTheme.typography.titleLarge, color = TextPrimary)
                Row {
                    IconButton(onClick = onToggleShuffle) {
                        Icon(
                            Icons.Rounded.Shuffle,
                            contentDescription = "Shuffle",
                            tint = if (state.shuffle) FerricOrange else TextSecondary,
                        )
                    }
                    IconButton(onClick = onCycleRepeat) {
                        Icon(
                            imageVector = if (state.repeatMode == FfiRepeatMode.ONE) {
                                Icons.Rounded.RepeatOne
                            } else {
                                Icons.Rounded.Repeat
                            },
                            contentDescription = "Repeat",
                            tint = if (state.repeatMode != FfiRepeatMode.OFF) FerricOrange else TextSecondary,
                        )
                    }
                }
            }

            state.currentTrack?.let { cur ->
                val maxDur = state.durationMs.coerceAtLeast(1L).toFloat()
                Surface(
                    shape = RoundedCornerShape(12.dp),
                    color = ObsidianElevated,
                    border = BorderStroke(1.dp, FerricOrange.copy(alpha = 0.5f)),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(modifier = Modifier.padding(12.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            ArtworkThumb(url = cur.thumbnailUrl, size = 50.dp, cornerRadius = 10.dp, isActive = state.isPlaying)
                            Spacer(Modifier.width(12.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(cur.title, color = TextPrimary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(cur.artist, style = MaterialTheme.typography.bodyMedium, color = TextSecondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                        }
                        Slider(
                            value = state.positionMs.toFloat().coerceIn(0f, maxDur),
                            onValueChange = { onSeek(it.toLong()) },
                            valueRange = 0f..maxDur,
                            colors = SliderDefaults.colors(
                                thumbColor = FerricOrange,
                                activeTrackColor = FerricOrange,
                                inactiveTrackColor = HairlineBorder,
                            ),
                        )
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            Text(formatDurationMs(state.positionMs), style = MaterialTheme.typography.labelSmall, color = TextSecondary)
                            Text(formatDurationMs(state.durationMs), style = MaterialTheme.typography.labelSmall, color = TextSecondary)
                        }
                    }
                }
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Up Next (${state.upNext.size})", style = MaterialTheme.typography.titleMedium, color = TextPrimary)
                Row {
                    TextButton(onClick = { showSaveDialog = true }) {
                        Icon(Icons.Rounded.Save, contentDescription = null, tint = FerricOrange)
                        Spacer(Modifier.width(4.dp))
                        Text("Save", color = FerricOrange)
                    }
                    TextButton(onClick = onCopyQueue) {
                        Icon(Icons.Rounded.ContentCopy, contentDescription = null, tint = TextSecondary)
                        Spacer(Modifier.width(4.dp))
                        Text("Copy", color = TextSecondary)
                    }
                    if (state.upNext.isNotEmpty()) {
                        IconButton(onClick = onClearUpNext) {
                            Icon(Icons.Rounded.DeleteSweep, contentDescription = "Clear Up Next", tint = TextSecondary)
                        }
                    }
                }
            }

            if (state.upNext.isEmpty()) {
                Text("Queue is clear. Add tracks with Play Next or Add to Queue.", color = TextSecondary)
                Spacer(Modifier.height(16.dp))
            } else {
                LazyColumn(
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.height(300.dp),
                ) {
                    itemsIndexed(state.upNext, key = { idx, t -> "q_${idx}_${t.id}" }) { idx, track ->
                        Surface(
                            shape = RoundedCornerShape(10.dp),
                            color = ObsidianElevated,
                            border = BorderStroke(1.dp, HairlineBorder),
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable { onJumpUpcoming(idx) },
                        ) {
                            Row(
                                modifier = Modifier.padding(start = 10.dp, end = 4.dp, top = 6.dp, bottom = 6.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                ArtworkThumb(url = track.thumbnailUrl, size = 40.dp, cornerRadius = 8.dp)
                                Spacer(Modifier.width(10.dp))
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(track.title, color = TextPrimary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    Text(track.artist, style = MaterialTheme.typography.labelSmall, color = TextSecondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                }
                                IconButton(onClick = { onMoveUp(idx) }, enabled = idx > 0) {
                                    Icon(Icons.Rounded.ArrowUpward, contentDescription = "Move up", tint = if (idx > 0) TextPrimary else HairlineBorder)
                                }
                                IconButton(onClick = { onMoveDown(idx) }, enabled = idx < state.upNext.lastIndex) {
                                    Icon(Icons.Rounded.ArrowDownward, contentDescription = "Move down", tint = if (idx < state.upNext.lastIndex) TextPrimary else HairlineBorder)
                                }
                                IconButton(onClick = { onRemove(idx) }) {
                                    Icon(Icons.Rounded.Close, contentDescription = "Remove from queue", tint = TextSecondary)
                                }
                            }
                        }
                    }
                }
            }
            Spacer(Modifier.height(12.dp))
        }
    }

    if (showSaveDialog) {
        AlertDialog(
            onDismissRequest = { showSaveDialog = false },
            containerColor = ObsidianElevated,
            title = { Text("Save Queue as Playlist", color = TextPrimary) },
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
                ) { Text("Save", color = FerricOrange) }
            },
            dismissButton = {
                TextButton(onClick = { showSaveDialog = false }) {
                    Text("Cancel", color = TextSecondary)
                }
            },
        )
    }
}
