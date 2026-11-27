package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.CloudUpload
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.DownloadForOffline
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material.icons.rounded.FilterList
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.components.TrackRow
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import uniffi.ferric_mobile_ffi.FfiCollectionDetail
import uniffi.ferric_mobile_ffi.FfiDetailSort
import uniffi.ferric_mobile_ffi.FfiTrack

@Composable
fun DetailScreen(
    detail: FfiCollectionDetail,
    detailSort: FfiDetailSort,
    currentTrackId: String?,
    offlineTrackIds: Set<String>,
    downloadProgress: Map<String, Int>,
    listState: LazyListState = rememberLazyListState(),
    onBack: () -> Unit,
    onPlayQueue: (List<FfiTrack>, Int, String) -> Unit,
    onShuffleQueue: (List<FfiTrack>, String) -> Unit,
    onCycleSort: () -> Unit,
    onDeduplicate: () -> Unit,
    onSyncToAccount: () -> Unit,
    onDownloadCollection: () -> Unit,
    onDownloadTrack: (FfiTrack) -> Unit,
    onDeleteOfflineTrack: (String) -> Unit,
    onCopyShareLink: () -> Unit,
    onCopyTracklist: () -> Unit,
    onExportCsv: () -> Unit,
    onRenamePlaylist: (String) -> Unit,
    onDeletePlaylist: () -> Unit,
    onToggleLike: (String) -> Unit,
    onPlayNext: (FfiTrack) -> Unit,
    onAddToQueue: (FfiTrack) -> Unit,
    onAddToPlaylist: (FfiTrack) -> Unit,
    onCopyTrackLink: (FfiTrack) -> Unit,
    onOpenArtist: (String) -> Unit,
    onRemoveFromPlaylist: (String) -> Unit,
) {
    val palette = LocalFerricPalette.current
    var showRenameDialog by remember { mutableStateOf(false) }
    var renameText by remember(detail.id) { mutableStateOf(detail.title) }
    var showDeleteConfirm by remember { mutableStateOf(false) }

    val sortLabel = when (detailSort) {
        FfiDetailSort.ORIGINAL -> "Default"
        FfiDetailSort.TITLE -> "Title"
        FfiDetailSort.ARTIST -> "Artist"
        FfiDetailSort.DURATION -> "Duration"
    }
    val heroBg = if (palette.isLight) {
        palette.surface.copy(alpha = 0.92f)
    } else {
        palette.elevated.copy(alpha = if (palette.isLiquidGlass) 0.56f else 0.90f)
    }

    LazyColumn(
        state = listState,
        modifier = Modifier
            .fillMaxSize()
            .background(if (palette.isLiquidGlass) Color.Transparent else palette.canvas),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 180.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        item(key = "detail_top") {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                IconButton(onClick = onBack) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Rounded.ArrowBack,
                        contentDescription = "Back",
                        tint = palette.textPrimary,
                    )
                }
                if (detail.isEditablePlaylist) {
                    Row {
                        IconButton(onClick = { showRenameDialog = true }) {
                            Icon(
                                imageVector = Icons.Rounded.Edit,
                                contentDescription = "Rename playlist",
                                tint = palette.textPrimary,
                            )
                        }
                        IconButton(onClick = { showDeleteConfirm = true }) {
                            Icon(
                                imageVector = Icons.Rounded.Delete,
                                contentDescription = "Delete playlist",
                                tint = palette.accent,
                            )
                        }
                    }
                }
            }
        }

        item(key = "detail_hero") {
            Surface(
                shape = RoundedCornerShape(22.dp),
                color = heroBg,
                border = BorderStroke(1.dp, palette.border.copy(alpha = 0.45f)),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        ArtworkThumb(url = detail.thumbnailUrl, size = 92.dp, cornerRadius = 14.dp)
                        Spacer(Modifier.width(14.dp))
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                text = detail.title,
                                style = MaterialTheme.typography.titleLarge,
                                fontWeight = FontWeight.Bold,
                                color = palette.textPrimary,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Spacer(Modifier.height(4.dp))
                            Text(
                                text = detail.subtitle,
                                style = MaterialTheme.typography.bodyMedium,
                                color = palette.textSecondary,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                            if (detail.description.isNotBlank()) {
                                Spacer(Modifier.height(3.dp))
                                Text(
                                    text = detail.description,
                                    style = MaterialTheme.typography.labelSmall,
                                    color = palette.textSecondary,
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                    }

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Button(
                            onClick = { onPlayQueue(detail.tracks, 0, detail.title) },
                            enabled = detail.tracks.isNotEmpty(),
                            shape = RoundedCornerShape(999.dp),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = if (palette.isLight) palette.textPrimary else Color.White,
                                contentColor = if (palette.isLight) Color.White else Color.Black,
                            ),
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(Icons.Rounded.PlayArrow, contentDescription = null, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text("Play All")
                        }
                        OutlinedButton(
                            onClick = { onShuffleQueue(detail.tracks, detail.title) },
                            enabled = detail.tracks.isNotEmpty(),
                            shape = RoundedCornerShape(999.dp),
                            border = BorderStroke(1.dp, palette.border),
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(Icons.Rounded.Shuffle, contentDescription = null, tint = palette.textPrimary, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text("Shuffle", color = palette.textPrimary)
                        }
                    }

                    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        item(key = "act_sort") {
                            OutlinedButton(
                                onClick = onCycleSort,
                                shape = RoundedCornerShape(999.dp),
                                border = BorderStroke(1.dp, palette.border),
                            ) {
                                Icon(Icons.Rounded.FilterList, contentDescription = null, tint = palette.accent, modifier = Modifier.size(16.dp))
                                Spacer(Modifier.width(4.dp))
                                Text("Sort: $sortLabel", color = palette.textPrimary)
                            }
                        }
                        if (detail.tracks.isNotEmpty()) {
                            item(key = "act_download") {
                                OutlinedButton(
                                    onClick = onDownloadCollection,
                                    shape = RoundedCornerShape(999.dp),
                                    border = BorderStroke(1.dp, palette.accent),
                                ) {
                                    Icon(Icons.Rounded.DownloadForOffline, contentDescription = null, tint = palette.accent, modifier = Modifier.size(16.dp))
                                    Spacer(Modifier.width(4.dp))
                                    Text("Download", color = palette.accent)
                                }
                            }
                        }
                        if (detail.isEditablePlaylist) {
                            item(key = "act_dedup") {
                                OutlinedButton(
                                    onClick = onDeduplicate,
                                    shape = RoundedCornerShape(999.dp),
                                    border = BorderStroke(1.dp, palette.border),
                                ) {
                                    Text("Deduplicate", color = palette.textPrimary)
                                }
                            }
                        }
                        if (detail.canSyncToAccount) {
                            item(key = "act_sync") {
                                OutlinedButton(
                                    onClick = onSyncToAccount,
                                    shape = RoundedCornerShape(999.dp),
                                    border = BorderStroke(1.dp, palette.accent),
                                ) {
                                    Icon(Icons.Rounded.CloudUpload, contentDescription = null, tint = palette.accent, modifier = Modifier.size(16.dp))
                                    Spacer(Modifier.width(4.dp))
                                    Text("Sync to Account", color = palette.accent)
                                }
                            }
                        }
                        if (!detail.shareUrl.isNullOrBlank()) {
                            item(key = "act_link") {
                                OutlinedButton(
                                    onClick = onCopyShareLink,
                                    shape = RoundedCornerShape(999.dp),
                                    border = BorderStroke(1.dp, palette.border),
                                ) {
                                    Icon(Icons.Rounded.ContentCopy, contentDescription = null, tint = palette.textSecondary, modifier = Modifier.size(16.dp))
                                    Spacer(Modifier.width(4.dp))
                                    Text("Copy Link", color = palette.textPrimary)
                                }
                            }
                        }
                        item(key = "act_txt") {
                            OutlinedButton(
                                onClick = onCopyTracklist,
                                shape = RoundedCornerShape(999.dp),
                                border = BorderStroke(1.dp, palette.border),
                            ) {
                                Text("Copy List", color = palette.textPrimary)
                            }
                        }
                        item(key = "act_csv") {
                            OutlinedButton(
                                onClick = onExportCsv,
                                shape = RoundedCornerShape(999.dp),
                                border = BorderStroke(1.dp, palette.border),
                            ) {
                                Text("Export CSV", color = palette.textPrimary)
                            }
                        }
                    }
                }
            }
        }

        if (detail.tracks.isEmpty()) {
            item(key = "detail_empty") {
                Text(
                    text = "No tracks in this collection yet.",
                    color = palette.textSecondary,
                    modifier = Modifier.padding(12.dp),
                )
            }
        } else {
            itemsIndexed(detail.tracks, key = { idx, t -> "dt_${idx}_${t.id}" }) { idx, track ->
                TrackRow(
                    track = track,
                    isActive = currentTrackId == track.id,
                    onPlay = { onPlayQueue(detail.tracks, idx, detail.title) },
                    onToggleLike = { onToggleLike(track.id) },
                    onPlayNext = { onPlayNext(track) },
                    onAddToQueue = { onAddToQueue(track) },
                    onAddToPlaylist = { onAddToPlaylist(track) },
                    onCopyLink = { onCopyTrackLink(track) },
                    onOpenArtist = { onOpenArtist(track.artist) },
                    isOfflineDownloaded = track.id in offlineTrackIds,
                    downloadPercent = downloadProgress[track.id],
                    onDownloadOffline = { onDownloadTrack(track) },
                    onDeleteOffline = { onDeleteOfflineTrack(track.id) },
                    onRemoveFromPlaylist = if (detail.isEditablePlaylist) {
                        { onRemoveFromPlaylist(track.id) }
                    } else {
                        null
                    },
                )
            }
        }
    }

    if (showRenameDialog) {
        AlertDialog(
            onDismissRequest = { showRenameDialog = false },
            containerColor = palette.elevated,
            title = { Text("Rename Playlist", color = palette.textPrimary) },
            text = {
                OutlinedTextField(
                    value = renameText,
                    onValueChange = { renameText = it },
                    singleLine = true,
                    label = { Text("Playlist name") },
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        if (renameText.isNotBlank()) {
                            onRenamePlaylist(renameText.trim())
                            showRenameDialog = false
                        }
                    },
                ) { Text("Save", color = palette.accent) }
            },
            dismissButton = {
                TextButton(onClick = { showRenameDialog = false }) {
                    Text("Cancel", color = palette.textSecondary)
                }
            },
        )
    }

    if (showDeleteConfirm) {
        AlertDialog(
            onDismissRequest = { showDeleteConfirm = false },
            containerColor = palette.elevated,
            title = { Text("Delete Playlist?", color = palette.textPrimary) },
            text = {
                Text(
                    "Delete \"${detail.title}\" from Ferric (and synced cloud if linked)?",
                    color = palette.textSecondary,
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteConfirm = false
                        onDeletePlaylist()
                    },
                ) { Text("Delete", color = palette.accent) }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteConfirm = false }) {
                    Text("Cancel", color = palette.textSecondary)
                }
            },
        )
    }
}
