package dev.livelydesk.ferric.ui.screens

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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.AccountCircle
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.Palette
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.components.TrackRow
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.LocalHazeState
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

@Composable
fun HomeScreen(
    state: PlaybackUiState,
    onRefresh: () -> Unit,
    onSelectMood: (Int?) -> Unit,
    onOpenAccounts: () -> Unit,
    onOpenEqualizer: () -> Unit,
    onOpenThemeStudio: () -> Unit,
    onOpenConnect: () -> Unit,
    onOpenSettings: () -> Unit,
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
    val quickPicks = if (home.quickPicks.isNotEmpty()) home.quickPicks else state.tracks.take(10)
    val anyConnected = state.accountStatus.spotifyConnected ||
        state.accountStatus.youtubeConnected ||
        state.accountStatus.subsonicConnected

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        item(key = "header") {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = "FERRIC",
                        style = MaterialTheme.typography.labelSmall,
                        color = palette.accent,
                    )
                    Text(
                        text = greeting,
                        style = MaterialTheme.typography.headlineMedium,
                        color = palette.textPrimary,
                    )
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = onRefresh) {
                        Icon(Icons.Rounded.Refresh, contentDescription = "Refresh Home", tint = palette.textPrimary)
                    }
                    IconButton(onClick = onOpenEqualizer) {
                        Icon(
                            Icons.Rounded.Equalizer,
                            contentDescription = "Equalizer and DSP",
                            tint = if (state.eqConfig?.enabled == true) palette.accent else palette.textPrimary,
                        )
                    }
                    IconButton(onClick = onOpenThemeStudio) {
                        Icon(
                            Icons.Rounded.Palette,
                            contentDescription = "Theme Studio",
                            tint = if (palette.isLiquidGlass) palette.accent else palette.textPrimary,
                        )
                    }
                    IconButton(onClick = onOpenConnect) {
                        val hasPeer = state.connectState.peers.any { it.connected }
                        Icon(
                            Icons.Rounded.Devices,
                            contentDescription = "Ferric Connect",
                            tint = if (hasPeer) palette.accent else palette.textPrimary,
                        )
                    }
                    IconButton(onClick = onOpenAccounts) {
                        Icon(
                            imageVector = Icons.Rounded.AccountCircle,
                            contentDescription = "Accounts",
                            tint = if (anyConnected) palette.accent else palette.textPrimary,
                        )
                    }
                    IconButton(onClick = onOpenSettings) {
                        Icon(
                            imageVector = Icons.Rounded.Settings,
                            contentDescription = "Settings",
                            tint = palette.textPrimary,
                        )
                    }
                }
            }
            if (home.isLoading) {
                Spacer(Modifier.height(6.dp))
                LinearProgressIndicator(
                    modifier = Modifier.fillMaxWidth().height(2.dp),
                    color = palette.accent,
                    trackColor = palette.elevated,
                )
            }
        }

        if (home.chips.isNotEmpty()) {
            item(key = "mood_chips") {
                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    item(key = "mood_all") {
                        val selected = home.selectedMoodIdx == null
                        FilterChip(
                            selected = selected,
                            onClick = { onSelectMood(null) },
                            label = { Text("All") },
                            colors = FilterChipDefaults.filterChipColors(
                                selectedContainerColor = palette.accent,
                                selectedLabelColor = palette.canvas,
                                containerColor = palette.elevated,
                                labelColor = palette.textPrimary,
                            ),
                        )
                    }
                    itemsIndexed(home.chips, key = { idx, c -> "${idx}_${c.title}" }) { idx, chip ->
                        val selected = home.selectedMoodIdx?.toInt() == idx
                        FilterChip(
                            selected = selected,
                            onClick = { onSelectMood(idx) },
                            label = { Text(chip.title) },
                            colors = FilterChipDefaults.filterChipColors(
                                selectedContainerColor = palette.accent,
                                selectedLabelColor = palette.canvas,
                                containerColor = palette.elevated,
                                labelColor = palette.textPrimary,
                            ),
                        )
                    }
                }
            }
        }

        item(key = "quick_picks_header") {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    text = "Quick picks",
                    style = MaterialTheme.typography.titleLarge,
                    color = palette.textPrimary,
                )
                if (quickPicks.isNotEmpty()) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        TextButton(onClick = { onPlayQueue(quickPicks, 0, "Quick Picks") }) {
                            Icon(Icons.Rounded.PlayArrow, contentDescription = null, tint = palette.accent, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(4.dp))
                            Text("Play", color = palette.accent)
                        }
                        TextButton(onClick = { onShuffleQueue(quickPicks, "Quick Picks") }) {
                            Icon(Icons.Rounded.Shuffle, contentDescription = null, tint = palette.textSecondary, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(4.dp))
                            Text("Shuffle", color = palette.textSecondary)
                        }
                    }
                }
            }
        }

        itemsIndexed(quickPicks.take(8), key = { idx, t -> "qp_${idx}_${t.id}" }) { idx, track ->
            TrackRow(
                track = track,
                isActive = state.currentTrack?.id == track.id,
                onPlay = { onPlayQueue(quickPicks, idx, "Quick Picks") },
                onToggleLike = { onToggleLike(track.id) },
                onPlayNext = { onPlayNext(track) },
                onAddToQueue = { onAddToQueue(track) },
                onAddToPlaylist = { onAddToPlaylist(track) },
                onCopyLink = { onCopyTrackLink(track) },
                onOpenArtist = { onOpenArtist(track.artist) },
            )
        }

        if (home.stations.isNotEmpty()) {
            item(key = "stations_section") {
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Text(
                        text = "Recommended Stations",
                        style = MaterialTheme.typography.titleLarge,
                        color = palette.textPrimary,
                    )
                    LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        items(home.stations, key = { it.id }) { station ->
                            StationCardItem(
                                station = station,
                                onClick = { onOpenStation(station.id) },
                            )
                        }
                    }
                }
            }
        }

        itemsIndexed(home.shelves, key = { idx, sh -> "shelf_${idx}_${sh.title}" }) { _, shelf ->
            if (shelf.cards.isNotEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Text(
                        text = shelf.title,
                        style = MaterialTheme.typography.titleLarge,
                        color = palette.textPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        itemsIndexed(shelf.cards, key = { cIdx, card -> "${cIdx}_${card.id}" }) { _, card ->
                            ShelfCardItem(
                                card = card,
                                onClick = {
                                    if (card.kind == FfiCardKind.SONG && !card.videoId.isNullOrBlank()) {
                                        val vid = card.videoId!!
                                        val single = FfiTrack(
                                            id = "yt:video:$vid",
                                            title = card.title,
                                            artist = card.subtitle.ifBlank { "YouTube Music" },
                                            album = shelf.title,
                                            durationMs = 0uL,
                                            path = null,
                                            source = FfiTrackSource.YOU_TUBE_MUSIC,
                                            codec = "OPUS",
                                            sampleRate = 48_000u,
                                            bitDepth = 16u,
                                            bitrateKbps = 128u,
                                            spotifyId = null,
                                            youtubeVideoId = vid,
                                            thumbnailUrl = card.thumbnailUrl,
                                            replayGainDb = 0,
                                            liked = false,
                                        )
                                        onPlayQueue(listOf(single), 0, shelf.title)
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

        item(key = "home_bottom_spacer") {
            Spacer(Modifier.height(12.dp))
        }
    }
}

@Composable
private fun StationCardItem(
    station: FfiStationCard,
    onClick: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val haze = LocalHazeState.current
    Column(
        modifier = Modifier
            .width(156.dp)
            .ferricGlassSurface(palette, haze)
            .clickable(onClick = onClick)
            .padding(10.dp),
    ) {
        ArtworkThumb(
            url = station.thumbnailUrl,
            size = 136.dp,
            cornerRadius = 10.dp,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            text = if (station.isSpotify) "SPOTIFY RADIO" else "FERRIC RADIO",
            style = MaterialTheme.typography.labelSmall,
            color = palette.accent,
            maxLines = 1,
        )
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

@Composable
private fun ShelfCardItem(
    card: FfiShelfCard,
    onClick: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val haze = LocalHazeState.current
    Column(
        modifier = Modifier
            .width(148.dp)
            .ferricGlassSurface(palette, haze)
            .clickable(onClick = onClick)
            .padding(10.dp),
    ) {
        ArtworkThumb(
            url = card.thumbnailUrl,
            size = 128.dp,
            cornerRadius = if (card.kind == FfiCardKind.ARTIST) 64.dp else 10.dp,
        )
        Spacer(Modifier.height(8.dp))
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
