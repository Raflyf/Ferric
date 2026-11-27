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
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.CloudUpload
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.Delete
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.components.TrackRow
import dev.livelydesk.ferric.ui.theme.AmoledBlack
import dev.livelydesk.ferric.ui.theme.FerricOrange
import dev.livelydesk.ferric.ui.theme.HairlineBorder
import dev.livelydesk.ferric.ui.theme.ObsidianElevated
import dev.livelydesk.ferric.ui.theme.ObsidianSurface
import dev.livelydesk.ferric.ui.theme.TextPrimary
import dev.livelydesk.ferric.ui.theme.TextSecondary
import uniffi.ferric_mobile_ffi.FfiCollectionDetail
import uniffi.ferric_mobile_ffi.FfiDetailSort
import uniffi.ferric_mobile_ffi.FfiTrack

@Composable
fun DetailScreen(
    detail: FfiCollectionDetail,
    detailSort: FfiDetailSort,
    currentTrackId: String?,
    onBack: () -> Unit,
    onPlayQueue: (List<FfiTrack>, Int, String) -> Unit,
    onShuffleQueue: (List<FfiTrack>, String) -> Unit,
    onCycleSort: () -> Unit,
    onDeduplicate: () -> Unit,
    onSyncToAccount: () -> Unit,
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
    var showRenameDialog by remember { mutableStateOf(false) }
    var renameText by remember(detail.id) { mutableStateOf(detail.title) }
    var showDeleteConfirm by remember { mutableStateOf(false) }

    val sortLabel = when (detailSort) {
        FfiDetailSort.ORIGINAL -> "Default"
        FfiDetailSort.TITLE -> "Title"
        FfiDetailSort.ARTIST -> "Artist"
        FfiDetailSort.DURATION -> "Duration"
    }

    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .background(AmoledBlack),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        item(key = "detail_top") {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                IconButton(onClick = onBack) {
                    Icon(Icons.AutoMirrored.Rounded.ArrowBack, contentDescription = "Back", tint = TextPrimary)
                }
                if (detail.isEditablePlaylist) {
                    Row {
                        IconButton(onClick = { showRenameDialog = true }) {
                            Icon(Icons.Rounded.Edit, contentDescription = "Rename playlist", tint = TextPrimary)
                        }
                        IconButton(onClick = { showDeleteConfirm = true }) {
                            Icon(Icons.Rounded.Delete, contentDescription = "Delete playlist", tint = FerricOrange)
                        }
                    }
                }
            }
        }

        item(key = "detail_hero") {
            Surface(
                shape = RoundedCornerShape(16.dp),
                color = ObsidianSurface,
                border = BorderStroke(1.dp, HairlineBorder),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(
                    modifier = Modifier.padding(14.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        ArtworkThumb(url = detail.thumbnailUrl, size = 84.dp, cornerRadius = 12.dp)
                        Spacer(Modifier.width(14.dp))
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                text = detail.title,
                                style = MaterialTheme.typography.titleLarge,
                                color = TextPrimary,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Spacer(Modifier.height(4.dp))
                            Text(
                                text = detail.subtitle,
                                style = MaterialTheme.typography.bodyMedium,
                                color = TextSecondary,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                            if (detail.description.isNotBlank()) {
                                Spacer(Modifier.height(2.dp))
                                Text(
                                    text = detail.description,
                                    style = MaterialTheme.typography.labelSmall,
                                    color = TextSecondary,
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
                            colors = ButtonDefaults.buttonColors(
                                containerColor = FerricOrange,
                                contentColor = ObsidianSurface,
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
                            border = BorderStroke(1.dp, HairlineBorder),
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(Icons.Rounded.Shuffle, contentDescription = null, tint = TextPrimary, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text("Shuffle", color = TextPrimary)
                        }
                    }

                    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        item(key = "act_sort") {
                            OutlinedButton(onClick = onCycleSort, border = BorderStroke(1.dp, HairlineBorder)) {
                                Icon(Icons.Rounded.FilterList, contentDescription = null, tint = FerricOrange, modifier = Modifier.size(16.dp))
                                Spacer(Modifier.width(4.dp))
                                Text("Sort: $sortLabel", color = TextPrimary)
                            }
                        }
                        if (detail.isEditablePlaylist) {
                            item(key = "act_dedup") {
                                OutlinedButton(onClick = onDeduplicate, border = BorderStroke(1.dp, HairlineBorder)) {
                                    Text("Deduplicate", color = TextPrimary)
                                }
                            }
                        }
                        if (detail.canSyncToAccount) {
                            item(key = "act_sync") {
                                OutlinedButton(onClick = onSyncToAccount, border = BorderStroke(1.dp, FerricOrange)) {
                                    Icon(Icons.Rounded.CloudUpload, contentDescription = null, tint = FerricOrange, modifier = Modifier.size(16.dp))
                                    Spacer(Modifier.width(4.dp))
                                    Text("Sync to Account", color = FerricOrange)
                                }
                            }
                        }
                        if (!detail.shareUrl.isNullOrBlank()) {
                            item(key = "act_link") {
                                OutlinedButton(onClick = onCopyShareLink, border = BorderStroke(1.dp, HairlineBorder)) {
                                    Icon(Icons.Rounded.ContentCopy, contentDescription = null, tint = TextSecondary, modifier = Modifier.size(16.dp))
                                    Spacer(Modifier.width(4.dp))
                                    Text("Copy Link", color = TextPrimary)
                                }
                            }
                        }
                        item(key = "act_txt") {
                            OutlinedButton(onClick = onCopyTracklist, border = BorderStroke(1.dp, HairlineBorder)) {
                                Text("Copy List", color = TextPrimary)
                            }
                        }
                        item(key = "act_csv") {
                            OutlinedButton(onClick = onExportCsv, border = BorderStroke(1.dp, HairlineBorder)) {
                                Text("Export CSV", color = TextPrimary)
                            }
                        }
                    }
                }
            }
        }

        if (detail.tracks.isEmpty()) {
            item(key = "detail_empty") {
                Text("No tracks in this collection yet.", color = TextSecondary, modifier = Modifier.padding(12.dp))
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
                    onRemoveFromPlaylist = if (detail.isEditablePlaylist) {
                        { onRemoveFromPlaylist(track.id) }
                    } else {
                        null
                    },
                )
            }
        }

        item(key = "detail_spacer") {
            Spacer(Modifier.height(16.dp))
        }
    }

    if (showRenameDialog) {
        AlertDialog(
            onDismissRequest = { showRenameDialog = false },
            containerColor = ObsidianElevated,
            title = { Text("Rename Playlist", color = TextPrimary) },
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
                ) { Text("Save", color = FerricOrange) }
            },
            dismissButton = {
                TextButton(onClick = { showRenameDialog = false }) {
                    Text("Cancel", color = TextSecondary)
                }
            },
        )
    }

    if (showDeleteConfirm) {
        AlertDialog(
            onDismissRequest = { showDeleteConfirm = false },
            containerColor = ObsidianElevated,
            title = { Text("Delete Playlist?", color = TextPrimary) },
            text = { Text("Delete \"${detail.title}\" from Ferric (and synced cloud if linked)?", color = TextSecondary) },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteConfirm = false
                        onDeletePlaylist()
                    },
                ) { Text("Delete", color = FerricOrange) }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteConfirm = false }) {
                    Text("Cancel", color = TextSecondary)
                }
            },
        )
    }
}
