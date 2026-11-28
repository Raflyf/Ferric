package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Clear
import androidx.compose.material.icons.rounded.Link
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
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
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.components.TrackRow
import dev.livelydesk.ferric.ui.theme.GlassTier
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import dev.livelydesk.ferric.ui.theme.ferricLiquidGlass
import kotlinx.coroutines.delay
import uniffi.ferric_mobile_ffi.FfiCardKind
import uniffi.ferric_mobile_ffi.FfiShelfCard
import uniffi.ferric_mobile_ffi.FfiTrack

private enum class SearchScope(val label: String) {
    ONLINE("Online"),
    DEVICE("Device"),
}

private enum class SearchCategory(val label: String) {
    ALL("All"),
    SONGS("Songs"),
    ALBUMS("Albums"),
    ARTISTS("Artists"),
    PLAYLISTS("Playlists"),
}

private data class BrowseGenreTile(
    val title: String,
    val query: String,
    val startColor: Color,
    val endColor: Color,
)

private val BROWSE_GENRES = listOf(
    BrowseGenreTile("Pop", "Pop hits", Color(0xFF8B2F52), Color(0xFF3E1528)),
    BrowseGenreTile("Indie", "Indie chill", Color(0xFF2E5C4E), Color(0xFF142B24)),
    BrowseGenreTile("Rock", "Alternative rock", Color(0xFF7A3828), Color(0xFF361811)),
    BrowseGenreTile("Hip Hop", "Hip Hop beats", Color(0xFF6B4E1E), Color(0xFF2D200C)),
    BrowseGenreTile("Acoustic", "Acoustic coffeehouse", Color(0xFF475776), Color(0xFF1C2333)),
    BrowseGenreTile("Jazz", "Late night jazz", Color(0xFF53396F), Color(0xFF21162E)),
    BrowseGenreTile("K-Pop", "K-Pop hits", Color(0xFF8A3569), Color(0xFF3A152C)),
    BrowseGenreTile("Chill", "Chill lofi", Color(0xFF2B5B6E), Color(0xFF112630)),
    BrowseGenreTile("Focus", "Deep focus instrumental", Color(0xFF3B4D70), Color(0xFF171F2E)),
    BrowseGenreTile("Workout", "Workout energy", Color(0xFF8C4120), Color(0xFF3B1A0D)),
    BrowseGenreTile("R&B", "R&B soul", Color(0xFF69335E), Color(0xFF2B1427)),
    BrowseGenreTile("Sleep", "Ambient sleep", Color(0xFF263B5C), Color(0xFF0F1827)),
)

@Composable
fun SearchScreen(
    state: PlaybackUiState,
    listState: LazyListState = rememberLazyListState(),
    onInstantLocalSearch: (String) -> Unit,
    onOnlineSearch: (String) -> Unit,
    onBrowseGenre: (String) -> Unit,
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
    onDownloadOffline: (FfiTrack) -> Unit,
    onDeleteOffline: (String) -> Unit,
) {
    val palette = LocalFerricPalette.current
    var query by rememberSaveable { mutableStateOf(state.searchResults.query) }
    var searchScope by rememberSaveable { mutableStateOf(SearchScope.ONLINE) }
    var selectedCategory by rememberSaveable { mutableStateOf(SearchCategory.ALL) }
    var showUrlImport by rememberSaveable { mutableStateOf(false) }
    var urlInput by rememberSaveable { mutableStateOf("") }
    var activeGenre by rememberSaveable { mutableStateOf<String?>(null) }
    var suppressNextOnlineSearch by remember { mutableStateOf(false) }

    // Debounce live YouTube Music search while keeping local search instantaneous.
    LaunchedEffect(query, searchScope) {
        onInstantLocalSearch(query)
        val trimmed = query.trim()
        if (suppressNextOnlineSearch) {
            suppressNextOnlineSearch = false
            return@LaunchedEffect
        }
        val alreadyShowingSameQuery = trimmed.equals(state.searchResults.query, ignoreCase = true) &&
            (state.searchResults.onlineTracks.isNotEmpty() || state.searchResults.onlineCards.isNotEmpty())
        if (searchScope == SearchScope.ONLINE) {
            if (trimmed.length >= 2 && !(activeGenre == trimmed && alreadyShowingSameQuery)) {
                delay(380L)
                onOnlineSearch(trimmed)
            } else if (trimmed.isEmpty()) {
                activeGenre = null
                onOnlineSearch("")
            }
        }
    }

    val results = state.searchResults
    val distinctRecent = remember(state.recentSearches) {
        state.recentSearches.map { it.trim() }.filter { it.isNotEmpty() }.distinct()
    }
    val filteredCards = results.onlineCards.filter { card ->
        when (selectedCategory) {
            SearchCategory.ALL -> true
            SearchCategory.ALBUMS -> card.kind == FfiCardKind.ALBUM
            SearchCategory.ARTISTS -> card.kind == FfiCardKind.ARTIST
            SearchCategory.PLAYLISTS -> card.kind == FfiCardKind.PLAYLIST
            SearchCategory.SONGS -> false
        }
    }
    val searchPillBg = if (palette.isLight) {
        palette.surface.copy(alpha = (palette.glassOpacity + 0.16f).coerceIn(0.40f, 0.96f))
    } else if (palette.isLiquidGlass) {
        palette.glassTint.copy(alpha = (palette.glassOpacity * 0.82f).coerceIn(0.10f, 0.85f))
    } else {
        Color.White.copy(alpha = (palette.glassOpacity * 0.22f).coerceIn(0.06f, 0.24f))
    }

    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 18.dp, end = 18.dp, top = 16.dp, bottom = 180.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        item(key = "search_header") {
            Text(
                text = "Search",
                style = MaterialTheme.typography.displaySmall,
                fontWeight = FontWeight.Light,
                color = palette.textPrimary,
            )
        }

        // Sleek floating pill search bar with inline URL import toggle
        item(key = "search_input") {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(52.dp)
                        .ferricGlassSurface(
                            palette = palette,
                            cornerRadius = 999.dp,
                            tier = GlassTier.FROSTED_PANEL,
                            fallbackColor = searchPillBg,
                        ),
                ) {
                    Row(
                        modifier = Modifier
                            .fillMaxSize()
                            .padding(start = 16.dp, end = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(
                            imageVector = Icons.Rounded.Search,
                            contentDescription = null,
                            tint = palette.textSecondary,
                            modifier = Modifier.size(21.dp),
                        )
                        Spacer(Modifier.width(10.dp))
                        Box(
                            modifier = Modifier.weight(1f),
                            contentAlignment = Alignment.CenterStart,
                        ) {
                            if (query.isEmpty()) {
                                Text(
                                    text = "Songs, albums, artists, or local FLAC...",
                                    style = MaterialTheme.typography.bodyLarge,
                                    color = palette.textSecondary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                            BasicTextField(
                                value = query,
                                onValueChange = {
                                    activeGenre = null
                                    suppressNextOnlineSearch = false
                                    query = it
                                },
                                singleLine = true,
                                textStyle = MaterialTheme.typography.bodyLarge.copy(color = palette.textPrimary),
                                cursorBrush = SolidColor(palette.accent),
                                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                                keyboardActions = KeyboardActions(
                                    onSearch = {
                                        val trimmed = query.trim()
                                        if (trimmed.isNotEmpty() && searchScope == SearchScope.ONLINE) {
                                            activeGenre = null
                                            onOnlineSearch(trimmed)
                                        }
                                    },
                                ),
                                modifier = Modifier.fillMaxWidth(),
                            )
                        }
                        if (query.isNotEmpty()) {
                            IconButton(
                                onClick = {
                                    activeGenre = null
                                    suppressNextOnlineSearch = false
                                    query = ""
                                },
                                modifier = Modifier.size(38.dp),
                            ) {
                                Icon(
                                    imageVector = Icons.Rounded.Clear,
                                    contentDescription = "Clear search",
                                    tint = palette.textSecondary,
                                    modifier = Modifier.size(19.dp),
                                )
                            }
                        }
                        IconButton(
                            onClick = { showUrlImport = !showUrlImport },
                            modifier = Modifier.size(38.dp),
                        ) {
                            Icon(
                                imageVector = Icons.Rounded.Link,
                                contentDescription = "Import public Spotify or YTM URL",
                                tint = if (showUrlImport) palette.accent else palette.textSecondary,
                                modifier = Modifier.size(20.dp),
                            )
                        }
                    }
                }

                // Segmented Online | Device scope pill
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(42.dp)
                        .ferricGlassSurface(
                            palette = palette,
                            cornerRadius = 999.dp,
                            fallbackColor = searchPillBg,
                        )
                        .padding(4.dp),
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    SearchScope.entries.forEach { scopeOpt ->
                        val selected = searchScope == scopeOpt
                        Box(
                            modifier = Modifier
                                .weight(1f)
                                .fillMaxSize()
                                .clip(RoundedCornerShape(999.dp))
                                .background(
                                    if (selected) {
                                        palette.accent
                                    } else {
                                        Color.Transparent
                                    },
                                )
                                .clickable { searchScope = scopeOpt },
                            contentAlignment = Alignment.Center,
                        ) {
                            Text(
                                text = scopeOpt.label,
                                style = MaterialTheme.typography.labelLarge,
                                color = if (selected) {
                                    Color.Black
                                } else {
                                    palette.textSecondary
                                },
                            )
                        }
                    }
                }

                if (results.isLoading && searchScope == SearchScope.ONLINE) {
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

        if (showUrlImport) {
            item(key = "url_import_box") {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .ferricGlassSurface(
                            palette = palette,
                            cornerRadius = palette.cornerRadiusDp,
                            fallbackColor = palette.elevated,
                        ),
                ) {
                    Column(
                        modifier = Modifier.padding(14.dp),
                        verticalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Text(
                            text = "Paste public Spotify or YouTube Music URL",
                            style = MaterialTheme.typography.labelLarge,
                            color = palette.textPrimary,
                        )
                        OutlinedTextField(
                            value = urlInput,
                            onValueChange = { urlInput = it },
                            placeholder = {
                                Text(
                                    "https://open.spotify.com/playlist/...",
                                    color = palette.textSecondary,
                                )
                            },
                            singleLine = true,
                            colors = OutlinedTextFieldDefaults.colors(
                                focusedBorderColor = palette.accent,
                                unfocusedBorderColor = palette.border,
                                focusedTextColor = palette.textPrimary,
                                unfocusedTextColor = palette.textPrimary,
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
                            shape = RoundedCornerShape(999.dp),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = palette.accent,
                                contentColor = if (palette.isLight) Color.White else Color.Black,
                            ),
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text("Import Playlist / Track")
                        }
                    }
                }
            }
        }

        // Category pills when searching Online (+ genre Mix Refresh chip when browsing a genre)
        if (query.isNotBlank() && searchScope == SearchScope.ONLINE) {
            item(key = "category_chips") {
                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (activeGenre != null) {
                        item(key = "genre_refresh_chip") {
                            val gName = activeGenre.orEmpty()
                            Box(
                                modifier = Modifier
                                    .ferricGlassSurface(palette, cornerRadius = 999.dp, fallbackColor = searchPillBg)
                                    .clickable {
                                        selectedCategory = SearchCategory.ALL
                                        onBrowseGenre(gName)
                                    },
                            ) {
                                Text(
                                    text = "Refresh mix",
                                    style = MaterialTheme.typography.labelLarge,
                                    color = palette.accent,
                                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 8.dp),
                                )
                            }
                        }
                    }
                    items(SearchCategory.entries, key = { it.name }) { cat ->
                        val selected = selectedCategory == cat
                        val chipModifier = if (selected) {
                            Modifier
                                .clip(RoundedCornerShape(999.dp))
                                .background(palette.accent)
                        } else {
                            Modifier.ferricGlassSurface(palette, cornerRadius = 999.dp, fallbackColor = searchPillBg)
                        }
                        Box(
                            modifier = chipModifier.clickable { selectedCategory = cat },
                        ) {
                            Text(
                                text = cat.label,
                                style = MaterialTheme.typography.labelLarge,
                                color = if (selected) {
                                    Color.Black
                                } else {
                                    palette.textPrimary
                                },
                                modifier = Modifier.padding(horizontal = 14.dp, vertical = 8.dp),
                            )
                        }
                    }
                }
            }
        }

        // Idle state: Recent Searches + 2-Column Browse All genre cards
        if (query.isBlank()) {
            if (distinctRecent.isNotEmpty()) {
                item(key = "recent_searches") {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                text = "Recent searches",
                                style = MaterialTheme.typography.titleMedium,
                                color = palette.textPrimary,
                            )
                            TextButton(onClick = onClearRecentSearches) {
                                Text("Clear", color = palette.textSecondary)
                            }
                        }
                        LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            items(distinctRecent, key = { "rec_$it" }) { term ->
                                Box(
                                    modifier = Modifier
                                        .ferricGlassSurface(palette, cornerRadius = 999.dp, fallbackColor = searchPillBg)
                                        .clickable {
                                            activeGenre = null
                                            suppressNextOnlineSearch = false
                                            searchScope = SearchScope.ONLINE
                                            query = term
                                        },
                                ) {
                                    Text(
                                        text = term,
                                        style = MaterialTheme.typography.labelLarge,
                                        color = palette.textPrimary,
                                        modifier = Modifier.padding(horizontal = 14.dp, vertical = 8.dp),
                                    )
                                }
                            }
                        }
                    }
                }
            }

            if (searchScope == SearchScope.ONLINE) {
                item(key = "browse_all_title") {
                    Text(
                        text = "Browse all",
                        style = MaterialTheme.typography.titleLarge,
                        color = palette.textPrimary,
                    )
                }
                items(BROWSE_GENRES.chunked(2), key = { pair -> "genre_${pair.first().title}" }) { pair ->
                    val genreRadius = palette.cornerRadiusDp
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        pair.forEach { genre ->
                            val tileMod = if (palette.isLiquidGlass) {
                                Modifier
                                    .weight(1f)
                                    .height(78.dp)
                                    .ferricLiquidGlass(
                                        cornerRadius = genreRadius,
                                        tier = GlassTier.FROSTED_PANEL,
                                        accentGlow = genre.startColor,
                                    )
                                    .background(
                                        Brush.linearGradient(
                                            listOf(
                                                genre.startColor.copy(alpha = 0.38f),
                                                genre.endColor.copy(alpha = 0.24f),
                                            ),
                                        ),
                                    )
                            } else {
                                Modifier
                                    .weight(1f)
                                    .height(78.dp)
                                    .clip(RoundedCornerShape(genreRadius))
                                    .background(
                                        Brush.linearGradient(listOf(genre.startColor, genre.endColor)),
                                    )
                            }
                            Box(
                                modifier = tileMod
                                    .clickable {
                                        searchScope = SearchScope.ONLINE
                                        selectedCategory = SearchCategory.ALL
                                        activeGenre = genre.title
                                        suppressNextOnlineSearch = true
                                        query = genre.title
                                        onBrowseGenre(genre.title)
                                    }
                                    .padding(14.dp),
                                contentAlignment = Alignment.BottomStart,
                            ) {
                                Text(
                                    text = genre.title,
                                    style = MaterialTheme.typography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    color = Color.White,
                                )
                            }
                        }
                    }
                }
            } else if (results.localTracks.isNotEmpty()) {
                item(key = "device_idle_header") {
                    Text(
                        text = "In Your Library (${results.localTracks.size})",
                        style = MaterialTheme.typography.titleMedium,
                        color = palette.textPrimary,
                    )
                }
                itemsIndexed(results.localTracks, key = { idx, t -> "dev_idle_${idx}_${t.id}" }) { idx, track ->
                    TrackRow(
                        track = track,
                        isActive = state.currentTrack?.id == track.id,
                        isOfflineDownloaded = track.id in state.offlineTrackIds,
                        downloadPercent = state.downloadProgress[track.id],
                        onPlay = { onPlayQueue(results.localTracks, idx, "Library Search") },
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
        } else {
            // Active Search Results
            if (searchScope == SearchScope.ONLINE && filteredCards.isNotEmpty()) {
                item(key = "online_cards_section") {
                    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        Text(
                            text = "Albums, Artists & Playlists",
                            style = MaterialTheme.typography.titleMedium,
                            color = palette.textPrimary,
                        )
                        LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            itemsIndexed(filteredCards, key = { idx, c -> "sc_${idx}_${c.id}" }) { _, card ->
                                Column(
                                    modifier = Modifier
                                        .width(136.dp)
                                        .clip(RoundedCornerShape(14.dp))
                                        .clickable { onOpenCard(card) },
                                    verticalArrangement = Arrangement.spacedBy(6.dp),
                                ) {
                                    ArtworkThumb(
                                        url = card.thumbnailUrl,
                                        size = 136.dp,
                                        cornerRadius = if (card.kind == FfiCardKind.ARTIST) 68.dp else 14.dp,
                                    )
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
                    }
                }
            }

            if (searchScope == SearchScope.DEVICE || selectedCategory == SearchCategory.ALL) {
                if (results.localTracks.isNotEmpty()) {
                    item(key = "local_hits_header") {
                        Text(
                            text = "Library Matches (${results.localTracks.size})",
                            style = MaterialTheme.typography.titleMedium,
                            color = palette.textPrimary,
                        )
                    }
                    itemsIndexed(results.localTracks, key = { idx, t -> "loc_${idx}_${t.id}" }) { idx, track ->
                        TrackRow(
                            track = track,
                            isActive = state.currentTrack?.id == track.id,
                            isOfflineDownloaded = track.id in state.offlineTrackIds,
                            downloadPercent = state.downloadProgress[track.id],
                            onPlay = { onPlayQueue(results.localTracks, idx, "Search Library") },
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

            val showOnlineSongs = searchScope == SearchScope.ONLINE &&
                (selectedCategory == SearchCategory.ALL || selectedCategory == SearchCategory.SONGS)
            if (showOnlineSongs && results.onlineTracks.isNotEmpty()) {
                item(key = "online_songs_header") {
                    Text(
                        text = "Songs (${results.onlineTracks.size})",
                        style = MaterialTheme.typography.titleMedium,
                        color = palette.textPrimary,
                    )
                }
                itemsIndexed(results.onlineTracks, key = { idx, t -> "onl_${idx}_${t.id}" }) { idx, track ->
                    TrackRow(
                        track = track,
                        isActive = state.currentTrack?.id == track.id,
                        isOfflineDownloaded = track.id in state.offlineTrackIds,
                        downloadPercent = state.downloadProgress[track.id],
                        onPlay = { onPlayQueue(results.onlineTracks, idx, "Search: $query") },
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
    }
}
