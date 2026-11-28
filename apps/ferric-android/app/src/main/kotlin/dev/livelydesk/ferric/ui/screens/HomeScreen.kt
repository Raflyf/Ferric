package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import coil3.request.ImageRequest
import coil3.size.Precision
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.components.TrackRow
import dev.livelydesk.ferric.ui.theme.GlassTier
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import java.util.Calendar
import uniffi.ferric_mobile_ffi.FfiCardKind
import uniffi.ferric_mobile_ffi.FfiShelfCard
import uniffi.ferric_mobile_ffi.FfiStationCard
import uniffi.ferric_mobile_ffi.FfiTrack
import uniffi.ferric_mobile_ffi.FfiTrackSource

private fun timeGreeting(): String {
    val hour = Calendar.getInstance().get(Calendar.HOUR_OF_DAY)
    return when {
        hour < 12 -> "Good morning"
        hour < 18 -> "Good afternoon"
        else -> "Good evening"
    }
}

private data class GoToTile(
    val id: String,
    val title: String,
    val thumbnailUrl: String?,
    val isLikedShortcut: Boolean = false,
    val onClick: () -> Unit,
)

/**
 * Editorial high-craft Home screen:
 * - Clean lightweight time-of-day greeting with zero header icon clutter (settings/accounts/themes
 *   live in the dedicated You tab).
 * - Compact recent track + daily streak chips when listening activity exists.
 * - Borderless frosted pill mood selector.
 * - 2-column quick access shortcuts (playlists & Liked Songs) OR 3x3
 *   "Your go-tos" artwork grid.
 * - Horizontal paged 4-row borderless Quick Picks columns with peeking right column.
 * - Rich full-bleed showcase cards for Recommended Stations and personalized shelves.
 * - Generous bottom contentPadding (180dp) so artwork scrolls optically behind the floating
 *   Liquid Glass dock and mini-player.
 */
@Composable
fun HomeScreen(
    state: PlaybackUiState,
    listState: LazyListState = rememberLazyListState(),
    onRefresh: () -> Unit,
    onLoadMoreHome: () -> Unit,
    onSelectMood: (Int?) -> Unit,
    onOpenConnect: () -> Unit,
    onOpenStats: () -> Unit,
    onOpenPlaylist: (String) -> Unit,
    onOpenLikedDetail: () -> Unit,
    onPlayQueue: (List<FfiTrack>, Int, String) -> Unit,
    onShuffleQueue: (List<FfiTrack>, String) -> Unit,
    onOpenStation: (String) -> Unit,
    onOpenShelfCard: (FfiShelfCard) -> Unit,
    onToggleLike: (String) -> Unit,
    onPlayNext: (FfiTrack) -> Unit,
    onAddToQueue: (FfiTrack) -> Unit,
    onAddToPlaylist: (FfiTrack) -> Unit,
    onCopyTrackLink: (FfiTrack) -> Unit,
    onOpenArtist: (String) -> Unit,
) {
    val palette = LocalFerricPalette.current
    val greeting = remember { timeGreeting() }
    val home = state.homeState
    val quickPicks = remember(home.quickPicks, state.tracks) {
        if (home.quickPicks.isNotEmpty()) {
            home.quickPicks
        } else {
            state.tracks
                .filter { t ->
                    !t.title.contains("[Playlist]", ignoreCase = true) &&
                        !t.title.contains("[Full Album]", ignoreCase = true) &&
                        !t.album.contains("watching", ignoreCase = true)
                }
                .take(24)
        }
    }
    val quickPickPages = remember(quickPicks) { quickPicks.chunked(4) }
    val screenWidthDp = LocalConfiguration.current.screenWidthDp.dp
    val quickPickColWidth = (screenWidthDp - 52.dp).coerceIn(272.dp, 360.dp)
    val activePeer = state.connectState.peers.firstOrNull { it.connected }
    val streakDays = state.listeningStats?.currentStreakDays?.toInt() ?: 0
    val latestTrack = state.recentTracks.firstOrNull() ?: state.currentTrack

    // Endless pagination on scroll (mirrors Desktop Load More): triggers automatically near bottom.
    LaunchedEffect(listState, home.shelves.size, home.isLoading, home.isLoadingMore) {
        snapshotFlow {
            val info = listState.layoutInfo
            val lastVisible = info.visibleItemsInfo.lastOrNull()?.index ?: 0
            lastVisible to info.totalItemsCount
        }.collect { (lastVisible, totalCount) ->
            if (!home.isLoading &&
                !home.isLoadingMore &&
                home.shelves.size in 1..47 &&
                totalCount > 2 &&
                lastVisible >= totalCount - 2
            ) {
                onLoadMoreHome()
            }
        }
    }

    // Build 2-column quick shortcuts when user has playlists/liked songs.
    val quickShortcuts = remember(state.playlists, state.likedTracks.size) {
        buildList {
            if (state.likedTracks.isNotEmpty()) {
                add(
                    GoToTile(
                        id = "shortcut_liked",
                        title = "Liked Songs",
                        thumbnailUrl = state.likedTracks.firstOrNull()?.thumbnailUrl,
                        isLikedShortcut = true,
                        onClick = onOpenLikedDetail,
                    ),
                )
            }
            state.playlists.take(5).forEach { pl ->
                add(
                    GoToTile(
                        id = "shortcut_pl_${pl.id}",
                        title = pl.name,
                        thumbnailUrl = pl.thumbnailUrl,
                        onClick = { onOpenPlaylist(pl.id) },
                    ),
                )
            }
        }
    }

    // Build 3x3 "Your go-tos" artwork grid when user has rich visual discovery cards.
    val goToArtworkTiles = remember(home.stations, home.shelves) {
        buildList {
            home.stations.forEach { st ->
                add(
                    GoToTile(
                        id = "goto_st_${st.id}",
                        title = st.title,
                        thumbnailUrl = st.thumbnailUrl,
                        onClick = { onOpenStation(st.id) },
                    ),
                )
            }
            home.shelves.flatMap { it.cards }.forEach { card ->
                if (size < 9 && !card.thumbnailUrl.isNullOrBlank()) {
                    add(
                        GoToTile(
                            id = "goto_card_${card.id}_$size",
                            title = card.title,
                            thumbnailUrl = card.thumbnailUrl,
                            onClick = {
                                if (card.kind == FfiCardKind.SONG && !card.videoId.isNullOrBlank()) {
                                    val vid = card.videoId!!
                                    onPlayQueue(
                                        listOf(cardToPlayableTrack(card, vid, "Go-tos")),
                                        0,
                                        "Your go-tos",
                                    )
                                } else {
                                    onOpenShelfCard(card)
                                }
                            },
                        ),
                    )
                }
            }
        }.take(6)
    }

    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(top = 16.dp, bottom = 180.dp),
        verticalArrangement = Arrangement.spacedBy(22.dp),
    ) {
        // Editorial greeting header + subtle live Connect status pill when a peer is active
        item(key = "home_header") {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 18.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text(
                        text = greeting,
                        style = MaterialTheme.typography.displaySmall,
                        fontWeight = FontWeight.Light,
                        color = palette.textPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    if (activePeer != null) {
                        Box(
                            modifier = Modifier
                                .ferricGlassSurface(
                                    palette = palette,
                                    cornerRadius = 999.dp,
                                    accentGlow = palette.accent,
                                    fallbackColor = palette.accent.copy(alpha = 0.18f),
                                )
                                .clickable(onClick = onOpenConnect),
                        ) {
                            Row(
                                modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(5.dp),
                            ) {
                                Icon(
                                    imageVector = Icons.Rounded.Devices,
                                    contentDescription = "Connected device",
                                    tint = palette.accent,
                                    modifier = Modifier.size(15.dp),
                                )
                                Text(
                                    text = activePeer.deviceName,
                                    style = MaterialTheme.typography.labelMedium,
                                    color = palette.accent,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                    }
                }

                // Compact recent song + daily streak row
                if (latestTrack != null || streakDays > 0) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        if (latestTrack != null) {
                            Box(
                                modifier = Modifier
                                    .weight(1f)
                                    .ferricGlassSurface(
                                        palette = palette,
                                        cornerRadius = 999.dp,
                                        fallbackColor = if (palette.isLight) {
                                            palette.surface.copy(alpha = 0.9f)
                                        } else {
                                            Color.White.copy(alpha = 0.08f)
                                        },
                                    )
                                    .clickable {
                                        onPlayQueue(listOf(latestTrack), 0, "Jump Back In")
                                    },
                            ) {
                                Row(
                                    modifier = Modifier.padding(start = 6.dp, end = 12.dp, top = 5.dp, bottom = 5.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                                ) {
                                    ArtworkThumb(
                                        url = latestTrack.thumbnailUrl,
                                        size = 30.dp,
                                        cornerRadius = 15.dp,
                                    )
                                    Column(modifier = Modifier.weight(1f)) {
                                        Text(
                                            text = latestTrack.title,
                                            style = MaterialTheme.typography.labelLarge,
                                            color = palette.textPrimary,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                        Text(
                                            text = latestTrack.artist,
                                            style = MaterialTheme.typography.labelSmall,
                                            color = palette.textSecondary,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                }
                            }
                        }
                        if (streakDays > 0) {
                            Box(
                                modifier = Modifier
                                    .ferricGlassSurface(
                                        palette = palette,
                                        cornerRadius = 999.dp,
                                        accentGlow = Color(0xFFFF7A26),
                                        fallbackColor = if (palette.isLight) Color(0x20FF7A26) else Color(0x2A1A1412),
                                    )
                                    .border(
                                        width = 1.dp,
                                        brush = Brush.linearGradient(
                                            listOf(
                                                Color(0x99FFC27A),
                                                Color(0x33FF6B22),
                                            ),
                                        ),
                                        shape = CircleShape,
                                    )
                                    .clickable(onClick = onOpenStats),
                            ) {
                                Row(
                                    modifier = Modifier.padding(horizontal = 11.dp, vertical = 7.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                                ) {
                                    Box(
                                        modifier = Modifier
                                            .size(22.dp)
                                            .clip(CircleShape)
                                            .background(
                                                Brush.radialGradient(
                                                    listOf(
                                                        Color(0x55FF7B24),
                                                        Color(0x14FF551A),
                                                    ),
                                                ),
                                            ),
                                        contentAlignment = Alignment.Center,
                                    ) {
                                        Canvas(modifier = Modifier.size(14.dp)) {
                                            val w = size.width
                                            val h = size.height
                                            val outer = Path().apply {
                                                moveTo(w * 0.56f, h * 0.06f)
                                                cubicTo(w * 0.68f, h * 0.28f, w * 0.90f, h * 0.42f, w * 0.88f, h * 0.66f)
                                                cubicTo(w * 0.86f, h * 0.88f, w * 0.68f, h * 0.98f, w * 0.49f, h * 0.98f)
                                                cubicTo(w * 0.26f, h * 0.98f, w * 0.12f, h * 0.82f, w * 0.13f, h * 0.60f)
                                                cubicTo(w * 0.14f, h * 0.44f, w * 0.25f, h * 0.36f, w * 0.32f, h * 0.29f)
                                                cubicTo(w * 0.33f, h * 0.38f, w * 0.39f, h * 0.41f, w * 0.43f, h * 0.34f)
                                                cubicTo(w * 0.48f, h * 0.25f, w * 0.48f, h * 0.14f, w * 0.56f, h * 0.06f)
                                                close()
                                            }
                                            drawPath(
                                                path = outer,
                                                brush = Brush.verticalGradient(
                                                    listOf(
                                                        Color(0xFFFFE05C),
                                                        Color(0xFFFF8324),
                                                        Color(0xFFF23F1A),
                                                    ),
                                                ),
                                            )
                                            val core = Path().apply {
                                                moveTo(w * 0.52f, h * 0.40f)
                                                cubicTo(w * 0.62f, h * 0.50f, w * 0.70f, h * 0.62f, w * 0.67f, h * 0.76f)
                                                cubicTo(w * 0.65f, h * 0.87f, w * 0.57f, h * 0.92f, w * 0.49f, h * 0.92f)
                                                cubicTo(w * 0.39f, h * 0.92f, w * 0.33f, h * 0.84f, w * 0.34f, h * 0.73f)
                                                cubicTo(w * 0.35f, h * 0.61f, w * 0.44f, h * 0.51f, w * 0.52f, h * 0.40f)
                                                close()
                                            }
                                            drawPath(
                                                path = core,
                                                brush = Brush.verticalGradient(
                                                    listOf(Color(0xFFFFF8D6), Color(0xFFFFB83E)),
                                                    startY = h * 0.40f,
                                                    endY = h * 0.92f,
                                                ),
                                            )
                                        }
                                    }
                                    Text(
                                        text = "${streakDays}d",
                                        style = MaterialTheme.typography.labelLarge,
                                        fontWeight = FontWeight.Bold,
                                        color = palette.textPrimary,
                                    )
                                }
                            }
                        }
                    }
                }

                if (home.isLoading) {
                    LinearProgressIndicator(
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(2.dp)
                            .clip(RoundedCornerShape(1.dp)),
                        color = palette.accent,
                        trackColor = palette.border.copy(alpha = 0.4f),
                    )
                }
            }
        }

        // Borderless pill mood chips
        if (home.chips.isNotEmpty()) {
            item(key = "home_mood_chips") {
                LazyRow(
                    contentPadding = PaddingValues(horizontal = 18.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    item(key = "mood_all") {
                        MoodPillChip(
                            label = "All",
                            selected = home.selectedMoodIdx == null,
                            onClick = { onSelectMood(null) },
                        )
                    }
                    itemsIndexed(home.chips, key = { idx, c -> "mood_${idx}_${c.title}" }) { idx, chip ->
                        MoodPillChip(
                            label = chip.title,
                            selected = home.selectedMoodIdx?.toInt() == idx,
                            onClick = { onSelectMood(idx) },
                        )
                    }
                }
            }
        }

        // 2-column quick access grid (when user has synced playlists / Liked Songs)
        if (quickShortcuts.size >= 2) {
            item(key = "home_quick_shortcuts") {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 18.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    quickShortcuts.chunked(2).forEach { pair ->
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            pair.forEach { tile ->
                                QuickShortcutCard(
                                    tile = tile,
                                    modifier = Modifier.weight(1f),
                                )
                            }
                            if (pair.size == 1) {
                                Spacer(Modifier.weight(1f))
                            }
                        }
                    }
                }
            }
        }

        // Quick Picks: horizontal paged 4-row borderless columns
        if (quickPicks.isNotEmpty()) {
            item(key = "home_quick_picks") {
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 18.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                text = "Quick picks",
                                style = MaterialTheme.typography.titleLarge,
                                color = palette.textPrimary,
                            )
                            IconButton(
                                onClick = onRefresh,
                                modifier = Modifier.size(32.dp),
                            ) {
                                Icon(
                                    imageVector = Icons.Rounded.Refresh,
                                    contentDescription = "Refresh Home",
                                    tint = palette.textSecondary,
                                    modifier = Modifier.size(18.dp),
                                )
                            }
                        }
                        Box(
                            modifier = Modifier
                                .ferricGlassSurface(
                                    palette = palette,
                                    cornerRadius = 999.dp,
                                    fallbackColor = if (palette.isLight) palette.surface else Color.White.copy(alpha = 0.09f),
                                )
                                .clickable {
                                    onPlayQueue(quickPicks, 0, "Quick Picks")
                                },
                        ) {
                            Row(
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(4.dp),
                            ) {
                                Icon(
                                    imageVector = Icons.Rounded.PlayArrow,
                                    contentDescription = null,
                                    tint = palette.textPrimary,
                                    modifier = Modifier.size(16.dp),
                                )
                                Text(
                                    text = "Play all",
                                    style = MaterialTheme.typography.labelMedium,
                                    color = palette.textPrimary,
                                )
                            }
                        }
                    }

                    LazyRow(
                        contentPadding = PaddingValues(horizontal = 14.dp),
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        itemsIndexed(quickPickPages, key = { pageIdx, _ -> "qp_page_$pageIdx" }) { pageIdx, page ->
                            Column(
                                modifier = Modifier.width(quickPickColWidth),
                                verticalArrangement = Arrangement.spacedBy(2.dp),
                            ) {
                                page.forEachIndexed { rowIdx, track ->
                                    val globalIdx = pageIdx * 4 + rowIdx
                                    TrackRow(
                                        track = track,
                                        isActive = state.currentTrack?.id == track.id,
                                        isOfflineDownloaded = track.id in state.offlineTrackIds,
                                        downloadPercent = state.downloadProgress[track.id],
                                        compact = true,
                                        showLikeButton = false,
                                        onPlay = { onPlayQueue(quickPicks, globalIdx, "Quick Picks") },
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
                }
            }
        }

        // 3-column visual "Your go-tos" grid (shown when stations/cards exist)
        if (goToArtworkTiles.size >= 3) {
            item(key = "home_your_gotos") {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 18.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Text(
                            text = "Your go-tos",
                            style = MaterialTheme.typography.titleLarge,
                            color = palette.textPrimary,
                        )
                        Icon(
                            imageVector = Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                            contentDescription = null,
                            tint = palette.textSecondary,
                            modifier = Modifier.size(20.dp),
                        )
                    }
                    goToArtworkTiles.chunked(3).forEach { rowTiles ->
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(10.dp),
                        ) {
                            rowTiles.forEach { tile ->
                                VisualGoToCell(
                                    tile = tile,
                                    modifier = Modifier.weight(1f),
                                )
                            }
                            repeat(3 - rowTiles.size) {
                                Spacer(Modifier.weight(1f))
                            }
                        }
                    }
                }
            }
        }

        // Recommended Stations (rich showcase artwork cards)
        if (home.stations.isNotEmpty()) {
            item(key = "home_stations") {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(
                        text = "Recommended stations",
                        style = MaterialTheme.typography.titleLarge,
                        color = palette.textPrimary,
                        modifier = Modifier.padding(horizontal = 18.dp),
                    )
                    LazyRow(
                        contentPadding = PaddingValues(horizontal = 18.dp),
                        horizontalArrangement = Arrangement.spacedBy(14.dp),
                    ) {
                        items(home.stations, key = { it.id }) { station ->
                            StationShowcaseCard(
                                station = station,
                                onClick = { onOpenStation(station.id) },
                            )
                        }
                    }
                }
            }
        }

        // Personalized Shelves (clean borderless artwork + typography cards)
        itemsIndexed(home.shelves, key = { idx, sh -> "shelf_${idx}_${sh.title}" }) { _, shelf ->
            if (shelf.cards.isNotEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(
                        text = shelf.title,
                        style = MaterialTheme.typography.titleLarge,
                        color = palette.textPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(horizontal = 18.dp),
                    )
                    LazyRow(
                        contentPadding = PaddingValues(horizontal = 18.dp),
                        horizontalArrangement = Arrangement.spacedBy(14.dp),
                    ) {
                        itemsIndexed(shelf.cards, key = { cIdx, card -> "${cIdx}_${card.id}" }) { _, card ->
                            ShelfShowcaseCard(
                                card = card,
                                onClick = {
                                    if (card.kind == FfiCardKind.SONG && !card.videoId.isNullOrBlank()) {
                                        val vid = card.videoId!!
                                        onPlayQueue(
                                            listOf(cardToPlayableTrack(card, vid, shelf.title)),
                                            0,
                                            shelf.title,
                                        )
                                    } else {
                                        onOpenShelfCard(card)
                                    }
                                },
                            )
                        }
                    }
                }
            }
        }

        if (home.isLoadingMore) {
            item(key = "home_load_more_spinner") {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(vertical = 12.dp),
                    horizontalArrangement = Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(18.dp),
                        strokeWidth = 2.dp,
                        color = palette.accent,
                    )
                    Spacer(Modifier.width(10.dp))
                    Text(
                        text = "Loading more recommendations...",
                        style = MaterialTheme.typography.labelMedium,
                        color = palette.textSecondary,
                    )
                }
            }
        }
    }
}

private fun cardToPlayableTrack(card: FfiShelfCard, videoId: String, albumTitle: String): FfiTrack =
    FfiTrack(
        id = "yt:video:$videoId",
        title = card.title,
        artist = card.subtitle.ifBlank { "YouTube Music" },
        album = albumTitle,
        durationMs = 0uL,
        path = null,
        source = FfiTrackSource.YOU_TUBE_MUSIC,
        codec = "OPUS",
        sampleRate = 48_000u,
        bitDepth = 16u,
        bitrateKbps = 128u,
        spotifyId = null,
        youtubeVideoId = videoId,
        thumbnailUrl = card.thumbnailUrl,
        replayGainDb = 0,
        liked = false,
    )

@Composable
private fun MoodPillChip(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val fg = when {
        selected -> Color.Black
        else -> palette.textPrimary
    }
    if (selected) {
        Surface(
            shape = RoundedCornerShape(999.dp),
            color = palette.accent,
            modifier = Modifier.clickable(onClick = onClick),
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelLarge,
                color = fg,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 9.dp),
            )
        }
    } else {
        Box(
            modifier = Modifier
                .ferricGlassSurface(
                    palette = palette,
                    cornerRadius = 999.dp,
                    fallbackColor = if (palette.isLight) {
                        palette.surface.copy(alpha = 0.92f)
                    } else {
                        palette.glassTint.copy(alpha = (palette.glassOpacity * 0.80f).coerceIn(0.10f, 0.86f))
                    },
                )
                .clickable(onClick = onClick),
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelLarge,
                color = fg,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 9.dp),
            )
        }
    }
}

@Composable
private fun QuickShortcutCard(
    tile: GoToTile,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val cardRadius = (palette.cornerRadiusDp.value * 0.65f).coerceIn(0f, 18f).dp
    val bg = if (palette.isLight) {
        palette.surface.copy(alpha = (palette.glassOpacity + 0.16f).coerceIn(0.35f, 0.98f))
    } else {
        palette.glassTint.copy(alpha = (palette.glassOpacity * 0.85f).coerceIn(0.10f, 0.88f))
    }
    Row(
        modifier = modifier
            .height(54.dp)
            .ferricGlassSurface(
                palette = palette,
                cornerRadius = cardRadius,
                tier = GlassTier.FROSTED_PANEL,
                fallbackColor = bg,
            )
            .clickable(onClick = tile.onClick),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (tile.isLikedShortcut && tile.thumbnailUrl.isNullOrBlank()) {
            Box(
                modifier = Modifier
                    .size(54.dp)
                    .background(
                        Brush.linearGradient(
                            listOf(palette.accent, Color(0xFF6B3FA0)),
                        ),
                    ),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Rounded.Favorite,
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier.size(22.dp),
                )
            }
        } else {
            ArtworkThumb(
                url = tile.thumbnailUrl,
                size = 54.dp,
                cornerRadius = 0.dp,
            )
        }
        Spacer(Modifier.width(10.dp))
        Text(
            text = tile.title,
            style = MaterialTheme.typography.labelLarge,
            color = palette.textPrimary,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier
                .weight(1f)
                .padding(end = 8.dp),
        )
    }
}

@Composable
private fun VisualGoToCell(
    tile: GoToTile,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val context = LocalContext.current
    val cellRadius = palette.cornerRadiusDp
    Column(
        modifier = modifier
            .clickable(onClick = tile.onClick),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .aspectRatio(1f)
                .clip(RoundedCornerShape(cellRadius))
                .background(palette.elevated),
            contentAlignment = Alignment.Center,
        ) {
            ArtworkThumb(url = null, size = 56.dp, cornerRadius = cellRadius, modifier = Modifier.fillMaxSize())
            if (!tile.thumbnailUrl.isNullOrBlank()) {
                val req = remember(tile.thumbnailUrl) {
                    ImageRequest.Builder(context)
                        .data(tile.thumbnailUrl)
                        .size(320, 320)
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
        Text(
            text = tile.title,
            style = MaterialTheme.typography.labelMedium,
            color = palette.textPrimary,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun StationShowcaseCard(
    station: FfiStationCard,
    onClick: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val cardRadius = palette.cornerRadiusDp
    Column(
        modifier = Modifier
            .width(164.dp)
            .clickable(onClick = onClick),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Box(
            modifier = Modifier
                .size(164.dp)
                .clip(RoundedCornerShape(cardRadius))
                .background(palette.elevated),
        ) {
            ArtworkThumb(
                url = station.thumbnailUrl,
                size = 164.dp,
                cornerRadius = cardRadius,
            )
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(
                        Brush.verticalGradient(
                            0f to Color.Transparent,
                            0.55f to Color.Transparent,
                            1f to Color.Black.copy(alpha = 0.72f),
                        ),
                    ),
            )
            Surface(
                shape = RoundedCornerShape(999.dp),
                color = Color.Black.copy(alpha = 0.52f),
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .padding(8.dp),
            ) {
                Text(
                    text = if (station.isSpotify) "SPOTIFY" else "RADIO",
                    style = MaterialTheme.typography.labelSmall,
                    color = if (station.isSpotify) Color(0xFF1ED760) else Color.White,
                    modifier = Modifier.padding(horizontal = 8.dp, vertical = 3.dp),
                )
            }
            Box(
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(10.dp)
                    .size(34.dp)
                    .clip(CircleShape)
                    .background(Color.White.copy(alpha = 0.92f)),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Rounded.PlayArrow,
                    contentDescription = null,
                    tint = Color.Black,
                    modifier = Modifier.size(20.dp),
                )
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                text = station.title,
                style = MaterialTheme.typography.titleMedium,
                color = palette.textPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = station.subtitle,
                style = MaterialTheme.typography.bodyMedium,
                color = palette.textSecondary,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun ShelfShowcaseCard(
    card: FfiShelfCard,
    onClick: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val isArtist = card.kind == FfiCardKind.ARTIST
    val cardRadius = palette.cornerRadiusDp
    Column(
        modifier = Modifier
            .width(150.dp)
            .clickable(onClick = onClick),
        horizontalAlignment = if (isArtist) Alignment.CenterHorizontally else Alignment.Start,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        ArtworkThumb(
            url = card.thumbnailUrl,
            size = 150.dp,
            cornerRadius = if (isArtist) 75.dp else cardRadius,
        )
        Column(
            horizontalAlignment = if (isArtist) Alignment.CenterHorizontally else Alignment.Start,
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            Text(
                text = card.title,
                style = MaterialTheme.typography.titleMedium,
                color = palette.textPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (card.subtitle.isNotBlank()) {
                Text(
                    text = card.subtitle,
                    style = MaterialTheme.typography.bodyMedium,
                    color = palette.textSecondary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}
