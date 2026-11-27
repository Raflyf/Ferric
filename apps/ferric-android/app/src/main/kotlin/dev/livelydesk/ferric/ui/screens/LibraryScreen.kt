package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.Sort
import androidx.compose.material.icons.automirrored.rounded.ViewList
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.DownloadDone
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.GridView
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.PhoneAndroid
import androidx.compose.material.icons.rounded.UploadFile
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.components.TrackRow
import dev.livelydesk.ferric.ui.components.sourceBadgeLabel
import dev.livelydesk.ferric.ui.theme.FerricModalBottomSheet
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import uniffi.ferric_mobile_ffi.FfiPlaylistSort
import uniffi.ferric_mobile_ffi.FfiPlaylistSourceFilter
import uniffi.ferric_mobile_ffi.FfiTrack

private enum class LibraryTab(val label: String) {
    ALL("All"),
    PLAYLISTS("Playlists"),
    SONGS("Songs"),
    ALBUMS("Albums"),
    ARTISTS("Artists"),
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(
    state: PlaybackUiState,
    listState: LazyListState = rememberLazyListState(),
    onPlayQueue: (List<FfiTrack>, Int, String) -> Unit,
    onOpenPlaylist: (String) -> Unit,
    onOpenAlbum: (String, String) -> Unit,
    onOpenArtist: (String) -> Unit,
    onOpenLikedDetail: () -> Unit,
    onOpenLocalCollection: (title: String, subtitle: String, tracks: List<FfiTrack>) -> Unit,
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
    val palette = LocalFerricPalette.current
    var activeTab by rememberSaveable { mutableStateOf(LibraryTab.ALL) }
    var gridView by rememberSaveable { mutableStateOf(false) }
    var showCreateMenu by rememberSaveable { mutableStateOf(false) }

    val sortLabel = when (state.playlistSort) {
        FfiPlaylistSort.NEWEST -> "Newest"
        FfiPlaylistSort.OLDEST -> "Oldest"
        FfiPlaylistSort.NAME_AZ -> "A-Z"
    }
    val pillBg = if (palette.isLight) {
        palette.surface.copy(alpha = 0.92f)
    } else {
        Color.White.copy(alpha = 0.08f)
    }

    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 18.dp, end = 18.dp, top = 16.dp, bottom = 180.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        // Header: Clean light-weight "Library" + floating glass "+" action button
        item(key = "lib_header") {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "Library",
                    style = MaterialTheme.typography.displaySmall,
                    fontWeight = FontWeight.Light,
                    color = palette.textPrimary,
                )
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    if (activeTab == LibraryTab.ALL || activeTab == LibraryTab.PLAYLISTS || activeTab == LibraryTab.ALBUMS) {
                        IconButton(
                            onClick = { gridView = !gridView },
                            modifier = Modifier.size(40.dp),
                        ) {
                            Icon(
                                imageVector = if (gridView) Icons.AutoMirrored.Rounded.ViewList else Icons.Rounded.GridView,
                                contentDescription = if (gridView) "List view" else "Grid view",
                                tint = palette.textSecondary,
                            )
                        }
                    }
                    Surface(
                        shape = CircleShape,
                        color = palette.accent,
                        modifier = Modifier
                            .size(40.dp)
                            .clickable { showCreateMenu = true },
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            Icon(
                                imageVector = Icons.Rounded.Add,
                                contentDescription = "Add or import music",
                                tint = Color.Black,
                                modifier = Modifier.size(22.dp),
                            )
                        }
                    }
                }
            }
        }

        // Auralys-style crisp underline text tabs (All | Playlists | Songs | Albums | Artists)
        item(key = "lib_underline_tabs") {
            LazyRow(horizontalArrangement = Arrangement.spacedBy(20.dp)) {
                items(LibraryTab.entries, key = { it.name }) { tab ->
                    val selected = activeTab == tab
                    Column(
                        modifier = Modifier
                            .clickable { activeTab = tab }
                            .padding(vertical = 4.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        Text(
                            text = tab.label,
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
                            color = if (selected) palette.textPrimary else palette.textSecondary,
                        )
                        Spacer(Modifier.height(6.dp))
                        Box(
                            modifier = Modifier
                                .width(if (selected) 26.dp else 0.dp)
                                .height(2.5.dp)
                                .clip(RoundedCornerShape(2.dp))
                                .background(palette.accent),
                        )
                    }
                }
            }
        }

        when (activeTab) {
            LibraryTab.ALL -> {
                // Clean borderless Collections (Auralys signature!)
                item(key = "collections_section_label") {
                    Text(
                        text = "Collections",
                        style = MaterialTheme.typography.labelLarge,
                        color = palette.textSecondary,
                    )
                }
                item(key = "col_liked") {
                    CollectionRowItem(
                        title = "Liked Songs",
                        subtitle = "${state.likedTracks.size} songs across all accounts",
                        icon = Icons.Rounded.Favorite,
                        gradient = listOf(Color(0xFF8C3B5C), Color(0xFF42224A)),
                        onClick = onOpenLikedDetail,
                    )
                }
                item(key = "col_offline") {
                    CollectionRowItem(
                        title = "Downloaded",
                        subtitle = "${state.offlineTracks.size} tracks ready offline",
                        icon = Icons.Rounded.DownloadDone,
                        gradient = listOf(Color(0xFF1E6A52), Color(0xFF10332A)),
                        onClick = {
                            onOpenLocalCollection(
                                "Offline Downloads",
                                "${state.offlineTracks.size} offline tracks",
                                state.offlineTracks,
                            )
                        },
                    )
                }
                item(key = "col_device") {
                    CollectionRowItem(
                        title = "On This Device",
                        subtitle = if (state.localTracks.isNotEmpty()) {
                            "${state.localTracks.size} local FLAC / WAV / MP3 tracks"
                        } else {
                            "Tap + to scan phone music or pick folder"
                        },
                        icon = Icons.Rounded.PhoneAndroid,
                        gradient = listOf(Color(0xFF3A4F7A), Color(0xFF1A243B)),
                        onClick = {
                            if (state.localTracks.isEmpty()) {
                                onScanPhoneMusic()
                            } else {
                                onOpenLocalCollection(
                                    "On This Device",
                                    "${state.localTracks.size} local lossless tracks",
                                    state.localTracks,
                                )
                            }
                        },
                    )
                }
                item(key = "col_recents") {
                    CollectionRowItem(
                        title = "Recently Played",
                        subtitle = "${state.recentTracks.size} songs in your recent history",
                        icon = Icons.Rounded.History,
                        gradient = listOf(Color(0xFF7A4B28), Color(0xFF382012)),
                        onClick = {
                            onOpenLocalCollection(
                                "Recently Played",
                                "${state.recentTracks.size} recent tracks",
                                state.recentTracks,
                            )
                        },
                    )
                }

                // Playlists section right below Collections
                item(key = "all_playlists_header") {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(top = 6.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = "Playlists (${state.playlists.size})",
                            style = MaterialTheme.typography.titleMedium,
                            color = palette.textPrimary,
                        )
                        Box(
                            modifier = Modifier
                                .ferricGlassSurface(palette = palette, cornerRadius = 999.dp, fallbackColor = pillBg)
                                .clickable(onClick = onCyclePlaylistSort),
                        ) {
                            Row(
                                modifier = Modifier.padding(horizontal = 10.dp, vertical = 5.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(4.dp),
                            ) {
                                Icon(
                                    imageVector = Icons.AutoMirrored.Rounded.Sort,
                                    contentDescription = null,
                                    tint = palette.textSecondary,
                                    modifier = Modifier.size(15.dp),
                                )
                                Text(
                                    text = sortLabel,
                                    style = MaterialTheme.typography.labelMedium,
                                    color = palette.textPrimary,
                                )
                            }
                        }
                    }
                }

                if (state.playlists.isEmpty()) {
                    item(key = "all_pl_empty") {
                        Text(
                            text = "No playlists yet. Tap + to create or import one, or connect Spotify / YouTube Music in You.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = palette.textSecondary,
                            modifier = Modifier.padding(vertical = 8.dp),
                        )
                    }
                } else if (gridView) {
                    items(state.playlists.chunked(2), key = { pair -> "all_plg_${pair.first().id}" }) { pair ->
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            pair.forEach { pl ->
                                PlaylistGridCard(
                                    name = pl.name,
                                    subtitle = "${sourceBadgeLabel(pl.source)} · ${pl.trackCount} songs",
                                    thumbnailUrl = pl.thumbnailUrl,
                                    onClick = { onOpenPlaylist(pl.id) },
                                    modifier = Modifier.weight(1f),
                                )
                            }
                            if (pair.size == 1) Spacer(Modifier.weight(1f))
                        }
                    }
                } else {
                    items(state.playlists, key = { "all_pl_${it.id}" }) { pl ->
                        BorderlessPlaylistRow(
                            name = pl.name,
                            subtitle = "${sourceBadgeLabel(pl.source)} · ${pl.trackCount} songs",
                            thumbnailUrl = pl.thumbnailUrl,
                            onClick = { onOpenPlaylist(pl.id) },
                        )
                    }
                }
            }

            LibraryTab.PLAYLISTS -> {
                item(key = "pl_filters") {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        LazyRow(
                            modifier = Modifier.weight(1f),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            val sources = listOf(
                                FfiPlaylistSourceFilter.ALL to "All",
                                FfiPlaylistSourceFilter.SPOTIFY to "Spotify",
                                FfiPlaylistSourceFilter.YOU_TUBE to "YouTube",
                                FfiPlaylistSourceFilter.LOCAL to "Local",
                            )
                            items(sources, key = { it.first.name }) { (src, label) ->
                                val selected = state.playlistSourceFilter == src
                                if (selected) {
                                    Surface(
                                        shape = RoundedCornerShape(999.dp),
                                        color = if (palette.isLight) palette.textPrimary else Color.White,
                                        modifier = Modifier.clickable { onSetPlaylistFilter(src) },
                                    ) {
                                        Text(
                                            text = label,
                                            style = MaterialTheme.typography.labelLarge,
                                            color = if (palette.isLight) Color.White else Color.Black,
                                            modifier = Modifier.padding(horizontal = 14.dp, vertical = 7.dp),
                                        )
                                    }
                                } else {
                                    Box(
                                        modifier = Modifier
                                            .ferricGlassSurface(palette = palette, cornerRadius = 999.dp, fallbackColor = pillBg)
                                            .clickable { onSetPlaylistFilter(src) },
                                    ) {
                                        Text(
                                            text = label,
                                            style = MaterialTheme.typography.labelLarge,
                                            color = palette.textPrimary,
                                            modifier = Modifier.padding(horizontal = 14.dp, vertical = 7.dp),
                                        )
                                    }
                                }
                            }
                        }
                        Spacer(Modifier.width(8.dp))
                        Box(
                            modifier = Modifier
                                .ferricGlassSurface(palette = palette, cornerRadius = 999.dp, fallbackColor = pillBg)
                                .clickable(onClick = onCyclePlaylistSort),
                        ) {
                            Text(
                                text = sortLabel,
                                style = MaterialTheme.typography.labelMedium,
                                color = palette.accent,
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 7.dp),
                            )
                        }
                    }
                }

                if (state.playlists.isEmpty()) {
                    item(key = "pl_empty") {
                        Text(
                            text = "No playlists match this filter.",
                            color = palette.textSecondary,
                            modifier = Modifier.padding(vertical = 12.dp),
                        )
                    }
                } else if (gridView) {
                    items(state.playlists.chunked(2), key = { pair -> "plg_${pair.first().id}" }) { pair ->
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            pair.forEach { pl ->
                                PlaylistGridCard(
                                    name = pl.name,
                                    subtitle = "${sourceBadgeLabel(pl.source)} · ${pl.trackCount} songs",
                                    thumbnailUrl = pl.thumbnailUrl,
                                    onClick = { onOpenPlaylist(pl.id) },
                                    modifier = Modifier.weight(1f),
                                )
                            }
                            if (pair.size == 1) Spacer(Modifier.weight(1f))
                        }
                    }
                } else {
                    items(state.playlists, key = { it.id }) { pl ->
                        BorderlessPlaylistRow(
                            name = pl.name,
                            subtitle = "${sourceBadgeLabel(pl.source)} · ${pl.trackCount} songs",
                            thumbnailUrl = pl.thumbnailUrl,
                            onClick = { onOpenPlaylist(pl.id) },
                        )
                    }
                }
            }

            LibraryTab.SONGS -> {
                if (state.tracks.isEmpty()) {
                    item(key = "songs_empty") {
                        Text("No songs in your library yet.", color = palette.textSecondary)
                    }
                } else {
                    itemsIndexed(state.tracks, key = { idx, t -> "song_${idx}_${t.id}" }) { idx, track ->
                        TrackRow(
                            track = track,
                            isActive = state.currentTrack?.id == track.id,
                            isOfflineDownloaded = track.id in state.offlineTrackIds,
                            downloadPercent = state.downloadProgress[track.id],
                            onPlay = { onPlayQueue(state.tracks, idx, "All Songs") },
                            onToggleLike = { onToggleLike(track.id) },
                            onPlayNext = { onPlayNext(track) },
                            onAddToQueue = { onAddToQueue(track) },
                            onAddToPlaylist = { onAddToPlaylist(track) },
                            onCopyLink = { onCopyTrackLink(track) },
                            onOpenArtist = { onOpenArtist(track.artist) },
                            onDownloadOffline = { onDownloadOffline(track) },
                            onDeleteOffline = { onDeleteOffline(track.id) },
                        )
                    }
                }
            }

            LibraryTab.ALBUMS -> {
                if (state.albums.isEmpty()) {
                    item(key = "albums_empty") {
                        Text("No albums indexed yet.", color = palette.textSecondary)
                    }
                } else if (gridView) {
                    items(state.albums.chunked(2), key = { pair -> "albg_${pair.first().name}_${pair.first().artist}" }) { pair ->
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            pair.forEach { alb ->
                                PlaylistGridCard(
                                    name = alb.name,
                                    subtitle = "${alb.artist} · ${alb.trackCount} tracks",
                                    thumbnailUrl = alb.thumbnailUrl,
                                    onClick = { onOpenAlbum(alb.name, alb.artist) },
                                    modifier = Modifier.weight(1f),
                                )
                            }
                            if (pair.size == 1) Spacer(Modifier.weight(1f))
                        }
                    }
                } else {
                    itemsIndexed(state.albums, key = { idx, a -> "alb_${idx}_${a.name}_${a.artist}" }) { _, alb ->
                        BorderlessPlaylistRow(
                            name = alb.name,
                            subtitle = "${alb.artist} · ${alb.trackCount} tracks",
                            thumbnailUrl = alb.thumbnailUrl,
                            onClick = { onOpenAlbum(alb.name, alb.artist) },
                        )
                    }
                }
            }

            LibraryTab.ARTISTS -> {
                if (state.artists.isEmpty()) {
                    item(key = "artists_empty") {
                        Text("No artists indexed yet.", color = palette.textSecondary)
                    }
                } else {
                    itemsIndexed(state.artists, key = { idx, a -> "art_${idx}_${a.name}" }) { _, art ->
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(12.dp))
                                .clickable { onOpenArtist(art.name) }
                                .padding(horizontal = 4.dp, vertical = 6.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            ArtworkThumb(url = art.thumbnailUrl, size = 54.dp, cornerRadius = 27.dp)
                            Spacer(Modifier.width(12.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    text = art.name,
                                    style = MaterialTheme.typography.titleMedium,
                                    color = palette.textPrimary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                Text(
                                    text = "${art.trackCount} songs · ${art.albumCount} albums",
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = palette.textSecondary,
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    if (showCreateMenu) {
        FerricModalBottomSheet(
            onDismissRequest = { showCreateMenu = false },
            coverShade = state.coverShade,
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 18.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Text(
                    text = "Add to Library",
                    style = MaterialTheme.typography.titleLarge,
                    color = palette.textPrimary,
                    modifier = Modifier.padding(bottom = 6.dp),
                )
                CreateMenuOption(
                    icon = Icons.AutoMirrored.Rounded.PlaylistAdd,
                    title = "Create Playlist",
                    subtitle = "Start a new playlist synced locally or to your account",
                    onClick = {
                        showCreateMenu = false
                        onCreatePlaylistRequest()
                    },
                )
                CreateMenuOption(
                    icon = Icons.Rounded.UploadFile,
                    title = "Import Playlist File (.m3u / .csv)",
                    subtitle = "Bring playlists from any player or spreadsheet",
                    onClick = {
                        showCreateMenu = false
                        onImportPlaylistFile()
                    },
                )
                CreateMenuOption(
                    icon = Icons.Rounded.PhoneAndroid,
                    title = "Scan Phone Music (MediaStore)",
                    subtitle = "Index FLAC, WAV, Opus, AAC, and MP3 files on device",
                    onClick = {
                        showCreateMenu = false
                        onScanPhoneMusic()
                    },
                )
                CreateMenuOption(
                    icon = Icons.Rounded.FolderOpen,
                    title = "Pick Custom Music Folder (SAF)",
                    subtitle = "Grant persistent read access to a custom lossless folder",
                    onClick = {
                        showCreateMenu = false
                        onPickSafFolder()
                    },
                )
                Spacer(Modifier.height(20.dp))
            }
        }
    }
}

@Composable
private fun CollectionRowItem(
    title: String,
    subtitle: String,
    icon: ImageVector,
    gradient: List<Color>,
    onClick: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val itemRadius = (palette.cornerRadiusDp.value * 0.85f).coerceIn(0f, 24f).dp
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(itemRadius))
            .clickable(onClick = onClick)
            .padding(horizontal = 4.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(54.dp)
                .clip(RoundedCornerShape(itemRadius))
                .background(Brush.linearGradient(gradient)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = Color.White,
                modifier = Modifier.size(24.dp),
            )
        }
        Spacer(Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleMedium,
                color = palette.textPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = subtitle,
                style = MaterialTheme.typography.bodyMedium,
                color = palette.textSecondary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun BorderlessPlaylistRow(
    name: String,
    subtitle: String,
    thumbnailUrl: String?,
    onClick: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 4.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ArtworkThumb(url = thumbnailUrl, size = 54.dp, cornerRadius = 10.dp)
        Spacer(Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = name,
                style = MaterialTheme.typography.titleMedium,
                color = palette.textPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = subtitle,
                style = MaterialTheme.typography.bodyMedium,
                color = palette.textSecondary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun PlaylistGridCard(
    name: String,
    subtitle: String,
    thumbnailUrl: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val cardRadius = palette.cornerRadiusDp
    Column(
        modifier = modifier
            .clip(RoundedCornerShape(cardRadius))
            .clickable(onClick = onClick),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .aspectRatio(1f)
                .clip(RoundedCornerShape(cardRadius)),
        ) {
            ArtworkThumb(
                url = thumbnailUrl,
                size = 180.dp,
                cornerRadius = cardRadius,
                modifier = Modifier.fillMaxSize(),
            )
        }
        Text(
            text = name,
            style = MaterialTheme.typography.titleMedium,
            color = palette.textPrimary,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Text(
            text = subtitle,
            style = MaterialTheme.typography.bodyMedium,
            color = palette.textSecondary,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun CreateMenuOption(
    icon: ImageVector,
    title: String,
    subtitle: String,
    onClick: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(40.dp)
                .clip(CircleShape)
                .background(palette.accent.copy(alpha = 0.16f)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = palette.accent,
                modifier = Modifier.size(21.dp),
            )
        }
        Spacer(Modifier.width(14.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)
            Text(subtitle, style = MaterialTheme.typography.bodyMedium, color = palette.textSecondary)
        }
    }
}
