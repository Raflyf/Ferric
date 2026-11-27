package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material.icons.rounded.UploadFile
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.components.TrackRow
import dev.livelydesk.ferric.ui.components.sourceBadgeLabel
import dev.livelydesk.ferric.ui.theme.FerricOrange
import dev.livelydesk.ferric.ui.theme.HairlineBorder
import dev.livelydesk.ferric.ui.theme.ObsidianElevated
import dev.livelydesk.ferric.ui.theme.ObsidianSurface
import dev.livelydesk.ferric.ui.theme.TextPrimary
import dev.livelydesk.ferric.ui.theme.TextSecondary
import uniffi.ferric_mobile_ffi.FfiPlaylistSort
import uniffi.ferric_mobile_ffi.FfiPlaylistSourceFilter
import uniffi.ferric_mobile_ffi.FfiTrack

private enum class LibraryTab(val label: String) {
    PLAYLISTS("Playlists"),
    SONGS("Songs"),
    ALBUMS("Albums"),
    ARTISTS("Artists"),
    LIKED("Liked"),
    OFFLINE("Offline"),
    RECENTS("Recents"),
    LOCAL("Local"),
}

@Composable
fun LibraryScreen(
    state: PlaybackUiState,
    onPlayQueue: (List<FfiTrack>, Int, String) -> Unit,
    onOpenPlaylist: (String) -> Unit,
    onOpenAlbum: (String, String) -> Unit,
    onOpenArtist: (String) -> Unit,
    onOpenLikedDetail: () -> Unit,
    onSetPlaylistFilter: (FfiPlaylistSourceFilter) -> Unit,
    onCyclePlaylistSort: () -> Unit,
    onCreatePlaylistRequest: () -> Unit,
    onImportPlaylistFile: () -> Unit,
    onScanPhoneMusic: () -> Unit,
    onPickSafFolder: () -> Unit,
    onToggleLike: (String) -> Unit,
    onPlayNext: (FfiTrack) -> Unit,
    onAddToQueue: (FfiTrack) -> Unit,
    onAddToPlaylist: (FfiTrack) -> Unit,
    onCopyTrackLink: (FfiTrack) -> Unit,
    onDownloadOffline: (FfiTrack) -> Unit,
    onDeleteOffline: (String) -> Unit,
) {
    var activeTab by rememberSaveable { mutableStateOf(LibraryTab.PLAYLISTS) }

    val sortLabel = when (state.playlistSort) {
        FfiPlaylistSort.NEWEST -> "Newest"
        FfiPlaylistSort.OLDEST -> "Oldest"
        FfiPlaylistSort.NAME_AZ -> "A-Z"
    }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        item(key = "lib_header") {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Library", style = MaterialTheme.typography.headlineMedium, color = TextPrimary)
                Row {
                    TextButton(onClick = onImportPlaylistFile) {
                        Icon(Icons.Rounded.UploadFile, contentDescription = null, tint = TextSecondary)
                        Spacer(Modifier.width(4.dp))
                        Text("Import", color = TextSecondary)
                    }
                    TextButton(onClick = onCreatePlaylistRequest) {
                        Icon(Icons.Rounded.Add, contentDescription = null, tint = FerricOrange)
                        Spacer(Modifier.width(4.dp))
                        Text("New", color = FerricOrange)
                    }
                }
            }
        }

        item(key = "lib_tabs") {
            LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                items(LibraryTab.entries, key = { it.name }) { tab ->
                    FilterChip(
                        selected = activeTab == tab,
                        onClick = { activeTab = tab },
                        label = { Text(tab.label) },
                        colors = FilterChipDefaults.filterChipColors(
                            selectedContainerColor = FerricOrange,
                            selectedLabelColor = ObsidianSurface,
                            containerColor = ObsidianElevated,
                            labelColor = TextPrimary,
                        ),
                    )
                }
            }
        }

        when (activeTab) {
            LibraryTab.PLAYLISTS -> {
                item(key = "pl_filters") {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        LazyRow(
                            modifier = Modifier.weight(1f),
                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            val sources = listOf(
                                FfiPlaylistSourceFilter.ALL to "All",
                                FfiPlaylistSourceFilter.SPOTIFY to "Spotify",
                                FfiPlaylistSourceFilter.YOU_TUBE to "YouTube",
                                FfiPlaylistSourceFilter.LOCAL to "Local",
                            )
                            items(sources, key = { it.first.name }) { (src, label) ->
                                FilterChip(
                                    selected = state.playlistSourceFilter == src,
                                    onClick = { onSetPlaylistFilter(src) },
                                    label = { Text(label) },
                                    colors = FilterChipDefaults.filterChipColors(
                                        selectedContainerColor = ObsidianElevated,
                                        selectedLabelColor = FerricOrange,
                                        containerColor = ObsidianSurface,
                                        labelColor = TextSecondary,
                                    ),
                                )
                            }
                        }
                        TextButton(onClick = onCyclePlaylistSort) {
                            Text("Sort: $sortLabel", color = FerricOrange)
                        }
                    }
                }

                item(key = "liked_shortcut") {
                    Surface(
                        shape = RoundedCornerShape(12.dp),
                        color = ObsidianElevated,
                        border = BorderStroke(1.dp, FerricOrange.copy(alpha = 0.45f)),
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable(onClick = onOpenLikedDetail),
                    ) {
                        Row(
                            modifier = Modifier.padding(12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(Icons.Rounded.LibraryMusic, contentDescription = null, tint = FerricOrange)
                            Spacer(Modifier.width(12.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text("Liked Songs", style = MaterialTheme.typography.titleMedium, color = TextPrimary)
                                Text(
                                    "${state.likedTracks.size} songs across all accounts",
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = TextSecondary,
                                )
                            }
                        }
                    }
                }

                if (state.playlists.isEmpty()) {
                    item(key = "pl_empty") {
                        Text(
                            "No playlists yet. Create one, import .m3u/.csv, or sign in to Spotify / YouTube Music.",
                            color = TextSecondary,
                            modifier = Modifier.padding(vertical = 12.dp),
                        )
                    }
                } else {
                    items(state.playlists, key = { it.id }) { pl ->
                        Surface(
                            shape = RoundedCornerShape(12.dp),
                            color = ObsidianSurface,
                            border = BorderStroke(1.dp, HairlineBorder),
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable { onOpenPlaylist(pl.id) },
                        ) {
                            Row(
                                modifier = Modifier.padding(10.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                ArtworkThumb(url = pl.thumbnailUrl, size = 52.dp, cornerRadius = 10.dp)
                                Spacer(Modifier.width(12.dp))
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(
                                        text = pl.name,
                                        style = MaterialTheme.typography.titleMedium,
                                        color = TextPrimary,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                    Text(
                                        text = "${sourceBadgeLabel(pl.source)} · ${pl.trackCount} songs",
                                        style = MaterialTheme.typography.bodyMedium,
                                        color = TextSecondary,
                                    )
                                }
                            }
                        }
                    }
                }
            }

            LibraryTab.SONGS -> {
                itemsIndexed(state.tracks, key = { idx, t -> "song_${idx}_${t.id}" }) { idx, track ->
                    val isOff = track.id in state.offlineTrackIds
                    TrackRow(
                        track = track,
                        isActive = state.currentTrack?.id == track.id,
                        onPlay = { onPlayQueue(state.tracks, idx, "All Songs") },
                        onToggleLike = { onToggleLike(track.id) },
                        onPlayNext = { onPlayNext(track) },
                        onAddToQueue = { onAddToQueue(track) },
                        onAddToPlaylist = { onAddToPlaylist(track) },
                        onCopyLink = { onCopyTrackLink(track) },
                        onOpenArtist = { onOpenArtist(track.artist) },
                        isOfflineDownloaded = isOff,
                        onDownloadOffline = { onDownloadOffline(track) },
                        onDeleteOffline = { onDeleteOffline(track.id) },
                    )
                }
            }

            LibraryTab.ALBUMS -> {
                itemsIndexed(state.albums, key = { idx, a -> "alb_${idx}_${a.name}_${a.artist}" }) { _, alb ->
                    Surface(
                        shape = RoundedCornerShape(12.dp),
                        color = ObsidianSurface,
                        border = BorderStroke(1.dp, HairlineBorder),
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { onOpenAlbum(alb.name, alb.artist) },
                    ) {
                        Row(
                            modifier = Modifier.padding(10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            ArtworkThumb(url = alb.thumbnailUrl, size = 52.dp, cornerRadius = 10.dp)
                            Spacer(Modifier.width(12.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(alb.name, color = TextPrimary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(
                                    "${alb.artist} · ${alb.trackCount} tracks",
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = TextSecondary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                    }
                }
            }

            LibraryTab.ARTISTS -> {
                itemsIndexed(state.artists, key = { idx, a -> "art_${idx}_${a.name}" }) { _, art ->
                    Surface(
                        shape = RoundedCornerShape(12.dp),
                        color = ObsidianSurface,
                        border = BorderStroke(1.dp, HairlineBorder),
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { onOpenArtist(art.name) },
                    ) {
                        Row(
                            modifier = Modifier.padding(10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            ArtworkThumb(url = art.thumbnailUrl, size = 52.dp, cornerRadius = 26.dp)
                            Spacer(Modifier.width(12.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(art.name, color = TextPrimary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(
                                    "${art.trackCount} songs · ${art.albumCount} albums",
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = TextSecondary,
                                )
                            }
                        }
                    }
                }
            }

            LibraryTab.LIKED -> {
                if (state.likedTracks.isEmpty()) {
                    item(key = "liked_empty") {
                        Text("Tap the heart on any song to build your Liked Songs.", color = TextSecondary)
                    }
                } else {
                    itemsIndexed(state.likedTracks, key = { idx, t -> "lik_${idx}_${t.id}" }) { idx, track ->
                        TrackRow(
                            track = track,
                            isActive = state.currentTrack?.id == track.id,
                            onPlay = { onPlayQueue(state.likedTracks, idx, "Liked Songs") },
                            onToggleLike = { onToggleLike(track.id) },
                            onPlayNext = { onPlayNext(track) },
                            onAddToQueue = { onAddToQueue(track) },
                            onAddToPlaylist = { onAddToPlaylist(track) },
                            onCopyLink = { onCopyTrackLink(track) },
                            onOpenArtist = { onOpenArtist(track.artist) },
                        )
                    }
                }
            }

            LibraryTab.OFFLINE -> {
                if (state.offlineTracks.isEmpty()) {
                    item(key = "offline_empty") {
                        Text(
                            "No offline downloads yet. Tap the menu on any song or Download inside a playlist to save music for airplane mode.",
                            color = TextSecondary,
                        )
                    }
                } else {
                    item(key = "offline_play_all") {
                        Button(
                            onClick = { onPlayQueue(state.offlineTracks, 0, "Offline Downloads") },
                            colors = ButtonDefaults.buttonColors(
                                containerColor = FerricOrange,
                                contentColor = ObsidianSurface,
                            ),
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text("Play All Offline (${state.offlineTracks.size})")
                        }
                    }
                    itemsIndexed(state.offlineTracks, key = { idx, t -> "off_${idx}_${t.id}" }) { idx, track ->
                        TrackRow(
                            track = track,
                            isActive = state.currentTrack?.id == track.id,
                            onPlay = { onPlayQueue(state.offlineTracks, idx, "Offline Downloads") },
                            onToggleLike = { onToggleLike(track.id) },
                            onPlayNext = { onPlayNext(track) },
                            onAddToQueue = { onAddToQueue(track) },
                            onAddToPlaylist = { onAddToPlaylist(track) },
                            onCopyLink = { onCopyTrackLink(track) },
                            onOpenArtist = { onOpenArtist(track.artist) },
                            isOfflineDownloaded = true,
                            onDeleteOffline = { onDeleteOffline(track.id) },
                        )
                    }
                }
            }

            LibraryTab.RECENTS -> {
                if (state.recentTracks.isEmpty()) {
                    item(key = "rec_empty") {
                        Text("Tracks you listen to for 30s+ appear here.", color = TextSecondary)
                    }
                } else {
                    itemsIndexed(state.recentTracks, key = { idx, t -> "rec_${idx}_${t.id}" }) { idx, track ->
                        TrackRow(
                            track = track,
                            isActive = state.currentTrack?.id == track.id,
                            onPlay = { onPlayQueue(state.recentTracks, idx, "Recents") },
                            onToggleLike = { onToggleLike(track.id) },
                            onPlayNext = { onPlayNext(track) },
                            onAddToQueue = { onAddToQueue(track) },
                            onAddToPlaylist = { onAddToPlaylist(track) },
                            onCopyLink = { onCopyTrackLink(track) },
                            onOpenArtist = { onOpenArtist(track.artist) },
                        )
                    }
                }
            }

            LibraryTab.LOCAL -> {
                item(key = "local_actions") {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Button(
                            onClick = onScanPhoneMusic,
                            colors = ButtonDefaults.buttonColors(
                                containerColor = FerricOrange,
                                contentColor = ObsidianSurface,
                            ),
                            modifier = Modifier.weight(1f),
                        ) {
                            Text("Scan Phone Music")
                        }
                        OutlinedButton(
                            onClick = onPickSafFolder,
                            border = BorderStroke(1.dp, HairlineBorder),
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(Icons.Rounded.FolderOpen, contentDescription = null, tint = TextPrimary)
                            Spacer(Modifier.width(6.dp))
                            Text("Pick Folder", color = TextPrimary)
                        }
                    }
                }
                if (state.localTracks.isEmpty()) {
                    item(key = "loc_empty") {
                        Text(
                            "Scan MediaStore or pick a custom FLAC/WAV/MP3 folder via Scoped Storage.",
                            color = TextSecondary,
                        )
                    }
                } else {
                    itemsIndexed(state.localTracks, key = { idx, t -> "locl_${idx}_${t.id}" }) { idx, track ->
                        TrackRow(
                            track = track,
                            isActive = state.currentTrack?.id == track.id,
                            onPlay = { onPlayQueue(state.localTracks, idx, "Local Lossless") },
                            onToggleLike = { onToggleLike(track.id) },
                            onPlayNext = { onPlayNext(track) },
                            onAddToQueue = { onAddToQueue(track) },
                            onAddToPlaylist = { onAddToPlaylist(track) },
                            onCopyLink = { onCopyTrackLink(track) },
                            onOpenArtist = { onOpenArtist(track.artist) },
                        )
                    }
                }
            }
        }

        item(key = "lib_bottom_spacer") {
            Spacer(Modifier.height(12.dp))
        }
    }
}
