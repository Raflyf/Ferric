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
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Clear
import androidx.compose.material.icons.rounded.Link
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.components.TrackRow
import dev.livelydesk.ferric.ui.theme.FerricOrange
import dev.livelydesk.ferric.ui.theme.HairlineBorder
import dev.livelydesk.ferric.ui.theme.ObsidianElevated
import dev.livelydesk.ferric.ui.theme.ObsidianSurface
import dev.livelydesk.ferric.ui.theme.TextPrimary
import dev.livelydesk.ferric.ui.theme.TextSecondary
import kotlinx.coroutines.delay
import uniffi.ferric_mobile_ffi.FfiCardKind
import uniffi.ferric_mobile_ffi.FfiShelfCard
import uniffi.ferric_mobile_ffi.FfiTrack

private enum class SearchCategory(val label: String) {
    ALL("All"),
    SONGS("Songs"),
    ALBUMS("Albums"),
    ARTISTS("Artists"),
    PLAYLISTS("Playlists"),
    LOCAL("Local"),
}

@Composable
fun SearchScreen(
    state: PlaybackUiState,
    onInstantLocalSearch: (String) -> Unit,
    onOnlineSearch: (String) -> Unit,
    onClearRecentSearches: () -> Unit,
    onImportUrl: (String) -> Unit,
    onPlayQueue: (List<FfiTrack>, Int, String) -> Unit,
    onOpenCard: (FfiShelfCard) -> Unit,
    onToggleLike: (String) -> Unit,
    onPlayNext: (FfiTrack) -> Unit,
    onAddToQueue: (FfiTrack) -> Unit,
    onAddToPlaylist: (FfiTrack) -> Unit,
    onCopyTrackLink: (FfiTrack) -> Unit,
    onOpenArtist: (String) -> Unit,
) {
    var query by rememberSaveable { mutableStateOf(state.searchResults.query) }
    var selectedCategory by rememberSaveable { mutableStateOf(SearchCategory.ALL) }
    var showUrlImport by rememberSaveable { mutableStateOf(false) }
    var urlInput by rememberSaveable { mutableStateOf("") }

    // Debounce live YouTube Music search while keeping local search instantaneous.
    LaunchedEffect(query) {
        onInstantLocalSearch(query)
        val trimmed = query.trim()
        if (trimmed.length >= 2) {
            delay(400L)
            onOnlineSearch(trimmed)
        } else if (trimmed.isEmpty()) {
            onOnlineSearch("")
        }
    }

    val results = state.searchResults
    val filteredCards = results.onlineCards.filter { card ->
        when (selectedCategory) {
            SearchCategory.ALL -> true
            SearchCategory.ALBUMS -> card.kind == FfiCardKind.ALBUM
            SearchCategory.ARTISTS -> card.kind == FfiCardKind.ARTIST
            SearchCategory.PLAYLISTS -> card.kind == FfiCardKind.PLAYLIST
            SearchCategory.SONGS, SearchCategory.LOCAL -> false
        }
    }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(key = "search_header") {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Search", style = MaterialTheme.typography.headlineMedium, color = TextPrimary)
                TextButton(onClick = { showUrlImport = !showUrlImport }) {
                    Icon(Icons.Rounded.Link, contentDescription = null, tint = FerricOrange)
                    Spacer(Modifier.width(4.dp))
                    Text("Import URL", color = FerricOrange)
                }
            }
        }

        if (showUrlImport) {
            item(key = "url_import_box") {
                Surface(
                    shape = RoundedCornerShape(12.dp),
                    color = ObsidianSurface,
                    border = BorderStroke(1.dp, HairlineBorder),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(
                        modifier = Modifier.padding(12.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(
                            "Paste public Spotify or YouTube Music URL",
                            style = MaterialTheme.typography.labelMedium,
                            color = TextSecondary,
                        )
                        OutlinedTextField(
                            value = urlInput,
                            onValueChange = { urlInput = it },
                            placeholder = { Text("https://open.spotify.com/playlist/...", color = TextSecondary) },
                            singleLine = true,
                            colors = OutlinedTextFieldDefaults.colors(
                                focusedBorderColor = FerricOrange,
                                unfocusedBorderColor = HairlineBorder,
                                focusedTextColor = TextPrimary,
                                unfocusedTextColor = TextPrimary,
                            ),
                            modifier = Modifier.fillMaxWidth(),
                        )
                        Button(
                            onClick = {
                                if (urlInput.isNotBlank()) {
                                    onImportUrl(urlInput.trim())
                                    urlInput = ""
                                    showUrlImport = false
                                }
                            },
                            colors = ButtonDefaults.buttonColors(
                                containerColor = FerricOrange,
                                contentColor = ObsidianSurface,
                            ),
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text("Import Playlist / Track")
                        }
                    }
                }
            }
        }

        item(key = "search_input") {
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                placeholder = { Text("Songs, artists, albums, or local FLAC...", color = TextSecondary) },
                leadingIcon = { Icon(Icons.Rounded.Search, contentDescription = null, tint = TextSecondary) },
                trailingIcon = {
                    if (query.isNotEmpty()) {
                        IconButton(onClick = { query = "" }) {
                            Icon(Icons.Rounded.Clear, contentDescription = "Clear search", tint = TextSecondary)
                        }
                    }
                },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { onOnlineSearch(query.trim()) }),
                shape = RoundedCornerShape(14.dp),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedBorderColor = FerricOrange,
                    unfocusedBorderColor = HairlineBorder,
                    focusedContainerColor = ObsidianSurface,
                    unfocusedContainerColor = ObsidianSurface,
                    focusedTextColor = TextPrimary,
                    unfocusedTextColor = TextPrimary,
                ),
                modifier = Modifier.fillMaxWidth(),
            )
            if (results.isLoading) {
                Spacer(Modifier.height(6.dp))
                LinearProgressIndicator(
                    modifier = Modifier.fillMaxWidth().height(2.dp),
                    color = FerricOrange,
                    trackColor = ObsidianElevated,
                )
            }
        }

        item(key = "category_chips") {
            LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                items(SearchCategory.entries, key = { it.name }) { cat ->
                    FilterChip(
                        selected = selectedCategory == cat,
                        onClick = { selectedCategory = cat },
                        label = { Text(cat.label) },
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

        if (query.isBlank() && state.recentSearches.isNotEmpty()) {
            item(key = "recent_searches") {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("Recent searches", style = MaterialTheme.typography.titleMedium, color = TextPrimary)
                        TextButton(onClick = onClearRecentSearches) {
                            Text("Clear", color = TextSecondary)
                        }
                    }
                    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        items(state.recentSearches, key = { it }) { term ->
                            FilterChip(
                                selected = false,
                                onClick = {
                                    query = term
                                    onOnlineSearch(term)
                                },
                                label = { Text(term) },
                                colors = FilterChipDefaults.filterChipColors(
                                    containerColor = ObsidianSurface,
                                    labelColor = TextPrimary,
                                ),
                            )
                        }
                    }
                }
            }
        }

        if (filteredCards.isNotEmpty()) {
            item(key = "online_cards_section") {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("Albums, Artists & Playlists", style = MaterialTheme.typography.titleMedium, color = TextPrimary)
                    LazyRow(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        itemsIndexed(filteredCards, key = { idx, c -> "sc_${idx}_${c.id}" }) { _, card ->
                            Surface(
                                shape = RoundedCornerShape(12.dp),
                                color = ObsidianSurface,
                                border = BorderStroke(1.dp, HairlineBorder),
                                modifier = Modifier
                                    .width(140.dp)
                                    .clickable { onOpenCard(card) },
                            ) {
                                Column(modifier = Modifier.padding(10.dp)) {
                                    ArtworkThumb(
                                        url = card.thumbnailUrl,
                                        size = 120.dp,
                                        cornerRadius = if (card.kind == FfiCardKind.ARTIST) 60.dp else 10.dp,
                                    )
                                    Spacer(Modifier.height(6.dp))
                                    Text(card.title, color = TextPrimary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    Text(
                                        card.subtitle,
                                        style = MaterialTheme.typography.labelSmall,
                                        color = TextSecondary,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }

        val showLocal = selectedCategory == SearchCategory.ALL || selectedCategory == SearchCategory.LOCAL
        if (showLocal && results.localTracks.isNotEmpty()) {
            item(key = "local_hits_header") {
                Text(
                    text = if (query.isBlank()) "In Your Library" else "Library Matches (${results.localTracks.size})",
                    style = MaterialTheme.typography.titleMedium,
                    color = TextPrimary,
                )
            }
            itemsIndexed(results.localTracks.take(20), key = { idx, t -> "loc_${idx}_${t.id}" }) { idx, track ->
                TrackRow(
                    track = track,
                    isActive = state.currentTrack?.id == track.id,
                    onPlay = { onPlayQueue(results.localTracks, idx, "Search Local") },
                    onToggleLike = { onToggleLike(track.id) },
                    onPlayNext = { onPlayNext(track) },
                    onAddToQueue = { onAddToQueue(track) },
                    onAddToPlaylist = { onAddToPlaylist(track) },
                    onCopyLink = { onCopyTrackLink(track) },
                    onOpenArtist = { onOpenArtist(track.artist) },
                )
            }
        }

        val showOnlineSongs = selectedCategory == SearchCategory.ALL || selectedCategory == SearchCategory.SONGS
        if (showOnlineSongs && results.onlineTracks.isNotEmpty()) {
            item(key = "online_songs_header") {
                Text(
                    text = "YouTube Music Songs (${results.onlineTracks.size})",
                    style = MaterialTheme.typography.titleMedium,
                    color = TextPrimary,
                )
            }
            itemsIndexed(results.onlineTracks, key = { idx, t -> "onl_${idx}_${t.id}" }) { idx, track ->
                TrackRow(
                    track = track,
                    isActive = state.currentTrack?.id == track.id,
                    onPlay = { onPlayQueue(results.onlineTracks, idx, "Search: $query") },
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
