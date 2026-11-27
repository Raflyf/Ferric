package dev.livelydesk.ferric.ui.screens

import android.view.ViewGroup
import androidx.annotation.OptIn
import androidx.compose.foundation.BorderStroke
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
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FavoriteBorder
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.Palette
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Repeat
import androidx.compose.material.icons.rounded.RepeatOne
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.AspectRatioFrameLayout
import androidx.media3.ui.PlayerView
import coil3.compose.AsyncImage
import dev.chrisbanes.haze.HazeState
import dev.chrisbanes.haze.hazeSource
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.formatDurationMs
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.components.AuroraBackdrop
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import java.util.Locale
import kotlin.math.abs
import uniffi.ferric_mobile_ffi.FfiRepeatMode

private enum class FullPlayerTab(val label: String) {
    COVER("Cover"),
    VIDEO("Video"),
    LYRICS("Lyrics"),
}

@Composable
fun FullPlayerScreen(
    state: PlaybackUiState,
    onCollapse: () -> Unit,
    onTogglePlayPause: () -> Unit,
    onNext: () -> Unit,
    onPrev: () -> Unit,
    onSeek: (Long) -> Unit,
    onToggleShuffle: () -> Unit,
    onCycleRepeat: () -> Unit,
    onToggleLike: (String) -> Unit,
    onOpenArtist: (String) -> Unit,
    onOpenQueue: () -> Unit,
    onOpenEqualizer: () -> Unit,
    onOpenThemeStudio: () -> Unit,
    onOpenOutputSwitcher: () -> Unit,
    onLoadLyrics: () -> Unit,
    onNudgeLyricsOffset: (Long) -> Unit,
    onResolveVideo: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val playerHaze = remember { HazeState() }
    var activeTab by rememberSaveable { mutableStateOf(FullPlayerTab.COVER) }
    val track = state.currentTrack
    val isFullBleed = palette.playerLayout == "full_bleed"

    LaunchedEffect(activeTab, track?.id) {
        if (track == null) return@LaunchedEffect
        when (activeTab) {
            FullPlayerTab.LYRICS -> {
                if (state.lyricsState.trackId != track.id && !state.lyricsState.isLoading) {
                    onLoadLyrics()
                }
            }
            FullPlayerTab.VIDEO -> {
                if (state.videoStreams.trackId != track.id || state.videoStreams.options.isEmpty()) {
                    onResolveVideo()
                }
            }
            FullPlayerTab.COVER -> {}
        }
    }

    Box(modifier = Modifier.fillMaxSize()) {
        // Aurora backdrop (+ full-bleed cover underlayer when selected) as Haze source
        Box(
            modifier = Modifier
                .fillMaxSize()
                .hazeSource(playerHaze),
        ) {
            if (isFullBleed && !track?.thumbnailUrl.isNullOrBlank()) {
                AsyncImage(
                    model = track.thumbnailUrl,
                    contentDescription = null,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxSize(),
                )
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(
                            Brush.verticalGradient(
                                listOf(
                                    palette.canvas.copy(alpha = 0.55f),
                                    palette.canvas.copy(alpha = 0.72f),
                                    palette.canvas.copy(alpha = 0.96f),
                                ),
                            ),
                        ),
                )
            } else {
                AuroraBackdrop(
                    colors = state.coverShade,
                    baseCanvas = palette.canvas,
                    isPlaying = state.isPlaying,
                    intensity = if (palette.isLight) 0.25f else 0.78f,
                )
            }
        }

        Column(
            modifier = Modifier
                .fillMaxSize()
                .statusBarsPadding()
                .navigationBarsPadding()
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            // Top bar
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                IconButton(onClick = onCollapse) {
                    Icon(
                        imageVector = Icons.Rounded.KeyboardArrowDown,
                        contentDescription = "Minimize player",
                        tint = palette.textPrimary,
                        modifier = Modifier.size(28.dp),
                    )
                }
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    FullPlayerTab.entries.forEach { tab ->
                        val selected = activeTab == tab
                        FilterChip(
                            selected = selected,
                            onClick = { activeTab = tab },
                            label = { Text(tab.label) },
                            colors = FilterChipDefaults.filterChipColors(
                                selectedContainerColor = palette.accent,
                                selectedLabelColor = if (palette.isLight) Color.White else Color.Black,
                                containerColor = palette.surface.copy(alpha = 0.65f),
                                labelColor = palette.textPrimary,
                            ),
                        )
                    }
                }
                IconButton(onClick = onOpenThemeStudio) {
                    Icon(
                        imageVector = Icons.Rounded.Palette,
                        contentDescription = "Theme Studio",
                        tint = palette.textPrimary,
                    )
                }
            }

            // Center Stage: Cover, Docked Video Clip, or Synced Lyrics
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f)
                    .padding(vertical = 8.dp),
                contentAlignment = Alignment.Center,
            ) {
                when (activeTab) {
                    FullPlayerTab.COVER -> {
                        Column(
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.Center,
                            modifier = Modifier.fillMaxSize(),
                        ) {
                            ArtworkThumb(
                                url = track?.thumbnailUrl,
                                size = if (isFullBleed) 220.dp else 280.dp,
                                cornerRadius = if (palette.isLiquidGlass) 24.dp else 18.dp,
                                isActive = state.isPlaying,
                            )
                            Spacer(Modifier.height(14.dp))
                            SpectrumBarRow(
                                bins = state.spectrumBins,
                                accent = palette.accent,
                                trackColor = palette.border,
                            )
                        }
                    }

                    FullPlayerTab.VIDEO -> {
                        DockedVideoStage(
                            state = state,
                            onRetry = onResolveVideo,
                        )
                    }

                    FullPlayerTab.LYRICS -> {
                        SyncedLyricsStage(
                            state = state,
                            onSeek = onSeek,
                            onNudgeOffset = onNudgeLyricsOffset,
                            onRetry = onLoadLyrics,
                        )
                    }
                }
            }

            // Bottom Transport Deck (Glass panel in Liquid Glass mode!)
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .ferricGlassSurface(
                        palette = palette,
                        hazeState = if (palette.isLiquidGlass) playerHaze else null,
                        cornerRadius = if (palette.isLiquidGlass) 24.dp else 16.dp,
                    )
                    .padding(horizontal = 16.dp, vertical = 14.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            text = track?.title ?: "Nothing playing",
                            style = MaterialTheme.typography.titleLarge,
                            color = palette.textPrimary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Text(
                            text = track?.artist ?: "Pick a track to start",
                            style = MaterialTheme.typography.bodyMedium,
                            color = palette.accent,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.clickable(enabled = track != null) {
                                track?.artist?.let {
                                    onCollapse()
                                    onOpenArtist(it)
                                }
                            },
                        )
                    }
                    if (track != null) {
                        IconButton(onClick = { onToggleLike(track.id) }) {
                            Icon(
                                imageVector = if (track.liked) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder,
                                contentDescription = "Toggle like",
                                tint = if (track.liked) palette.accent else palette.textSecondary,
                            )
                        }
                    }
                }

                val maxDur = state.durationMs.coerceAtLeast(1L).toFloat()
                Slider(
                    value = state.positionMs.toFloat().coerceIn(0f, maxDur),
                    onValueChange = { onSeek(it.toLong()) },
                    valueRange = 0f..maxDur,
                    colors = SliderDefaults.colors(
                        thumbColor = palette.accent,
                        activeTrackColor = palette.accent,
                        inactiveTrackColor = palette.border,
                    ),
                )
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text(formatDurationMs(state.positionMs), style = MaterialTheme.typography.labelSmall, color = palette.textSecondary)
                    Text(
                        text = if (state.outputRate > 0) {
                            "${track?.codec.orEmpty()} · ${String.format(Locale.US, "%.1f kHz", state.outputRate / 1000.0)}"
                        } else {
                            track?.codec.orEmpty()
                        },
                        style = MaterialTheme.typography.labelSmall,
                        color = palette.textSecondary,
                    )
                    Text(formatDurationMs(state.durationMs), style = MaterialTheme.typography.labelSmall, color = palette.textSecondary)
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceEvenly,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    IconButton(onClick = onToggleShuffle) {
                        Icon(
                            Icons.Rounded.Shuffle,
                            contentDescription = "Shuffle",
                            tint = if (state.shuffle) palette.accent else palette.textSecondary,
                        )
                    }
                    IconButton(onClick = onPrev) {
                        Icon(
                            Icons.Rounded.SkipPrevious,
                            contentDescription = "Previous",
                            tint = palette.textPrimary,
                            modifier = Modifier.size(32.dp),
                        )
                    }
                    Surface(
                        shape = CircleShape,
                        color = palette.accent,
                        modifier = Modifier.size(60.dp),
                    ) {
                        IconButton(onClick = onTogglePlayPause) {
                            Icon(
                                imageVector = if (state.isPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                                contentDescription = if (state.isPlaying) "Pause" else "Play",
                                tint = if (palette.isLight) Color.White else Color.Black,
                                modifier = Modifier.size(34.dp),
                            )
                        }
                    }
                    IconButton(onClick = onNext) {
                        Icon(
                            Icons.Rounded.SkipNext,
                            contentDescription = "Next",
                            tint = palette.textPrimary,
                            modifier = Modifier.size(32.dp),
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
                            tint = if (state.repeatMode != FfiRepeatMode.OFF) palette.accent else palette.textSecondary,
                        )
                    }
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    TextButton(onClick = onOpenOutputSwitcher) {
                        Icon(Icons.Rounded.Speaker, contentDescription = null, tint = palette.accent, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(4.dp))
                        Text(state.activeOutputRoute, color = palette.textPrimary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    Row {
                        IconButton(onClick = onOpenEqualizer) {
                            Icon(Icons.Rounded.Equalizer, contentDescription = "Equalizer & DSP", tint = palette.textPrimary)
                        }
                        IconButton(onClick = onOpenQueue) {
                            Icon(Icons.AutoMirrored.Rounded.QueueMusic, contentDescription = "Queue", tint = palette.textPrimary)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SpectrumBarRow(
    bins: List<Int>,
    accent: Color,
    trackColor: Color,
) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.Bottom,
        modifier = Modifier.height(28.dp),
    ) {
        bins.forEach { raw ->
            val frac = (raw / 1000f).coerceIn(0.08f, 1f)
            Box(
                modifier = Modifier
                    .width(8.dp)
                    .height((28f * frac).dp)
                    .clip(RoundedCornerShape(4.dp))
                    .background(if (raw > 30) accent else trackColor),
            )
        }
    }
}

@Composable
private fun SyncedLyricsStage(
    state: PlaybackUiState,
    onSeek: (Long) -> Unit,
    onNudgeOffset: (Long) -> Unit,
    onRetry: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val lyrics = state.lyricsState
    val listState = rememberLazyListState()

    val effectivePosMs = (state.positionMs + lyrics.offsetMs).coerceAtLeast(0L)
    val activeIdx = remember(lyrics.lines, effectivePosMs) {
        var found = -1
        for (i in lyrics.lines.indices) {
            if (lyrics.lines[i].timestampMs.toLong() <= effectivePosMs) {
                found = i
            } else {
                break
            }
        }
        found
    }

    LaunchedEffect(activeIdx) {
        if (activeIdx >= 0) {
            listState.animateScrollToItem((activeIdx - 2).coerceAtLeast(0))
        }
    }

    Column(
        modifier = Modifier.fillMaxSize(),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = if (lyrics.sourceName.isNotBlank()) {
                    "${lyrics.sourceName} · Offset ${String.format(Locale.US, "%+.1fs", lyrics.offsetMs / 1000.0)}"
                } else {
                    "Synced Lyrics"
                },
                style = MaterialTheme.typography.labelSmall,
                color = palette.textSecondary,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                OutlinedButton(
                    onClick = { onNudgeOffset(-500L) },
                    border = BorderStroke(1.dp, palette.border),
                    contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                ) { Text("-0.5s", color = palette.textPrimary) }
                OutlinedButton(
                    onClick = { onNudgeOffset(500L) },
                    border = BorderStroke(1.dp, palette.border),
                    contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                ) { Text("+0.5s", color = palette.textPrimary) }
            }
        }

        when {
            lyrics.isLoading -> {
                Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(color = palette.accent)
                }
            }
            lyrics.lines.isEmpty() -> {
                Column(
                    modifier = Modifier.fillMaxSize(),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Text("No synced lyrics found for this track.", color = palette.textSecondary)
                    Spacer(Modifier.height(8.dp))
                    TextButton(onClick = onRetry) { Text("Retry Lyrics", color = palette.accent) }
                }
            }
            else -> {
                LazyColumn(
                    state = listState,
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(vertical = 24.dp),
                    verticalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    itemsIndexed(lyrics.lines, key = { idx, line -> "${idx}_${line.timestampMs}" }) { idx, line ->
                        val isActive = idx == activeIdx
                        Text(
                            text = line.text,
                            fontSize = if (isActive) 22.sp else 17.sp,
                            lineHeight = if (isActive) 28.sp else 23.sp,
                            fontWeight = if (isActive) FontWeight.Bold else FontWeight.Medium,
                            color = if (isActive) palette.textPrimary else palette.textSecondary.copy(alpha = 0.65f),
                            textAlign = TextAlign.Start,
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(8.dp))
                                .clickable {
                                    val targetMs = (line.timestampMs.toLong() - lyrics.offsetMs).coerceAtLeast(0L)
                                    onSeek(targetMs)
                                }
                                .padding(horizontal = 8.dp, vertical = 4.dp),
                        )
                    }
                }
            }
        }
    }
}

@OptIn(UnstableApi::class)
@Composable
private fun DockedVideoStage(
    state: PlaybackUiState,
    onRetry: () -> Unit,
) {
    val context = LocalContext.current
    val palette = LocalFerricPalette.current
    val video = state.videoStreams
    var selectedOptIdx by remember(video.trackId, video.options.size) { mutableIntStateOf(0) }
    val selectedOption = video.options.getOrNull(selectedOptIdx)

    val exoPlayer = remember(context) {
        ExoPlayer.Builder(context).build().apply {
            volume = 0f // Audio is rendered exclusively by Ferric's Rust DSP/EQ engine!
            repeatMode = Player.REPEAT_MODE_OFF
        }
    }

    DisposableEffect(exoPlayer) {
        onDispose { exoPlayer.release() }
    }

    LaunchedEffect(selectedOption?.url) {
        val url = selectedOption?.url
        if (!url.isNullOrBlank()) {
            exoPlayer.setMediaItem(MediaItem.fromUri(url))
            exoPlayer.prepare()
            exoPlayer.seekTo(state.positionMs.coerceAtLeast(0L))
            exoPlayer.playWhenReady = state.isPlaying
        } else {
            exoPlayer.stop()
        }
    }

    // Lock ExoPlayer video surface to Rust audio clock (drift tolerance 350 ms).
    LaunchedEffect(state.positionMs, state.isPlaying, selectedOption?.url) {
        if (selectedOption == null) return@LaunchedEffect
        exoPlayer.playWhenReady = state.isPlaying
        val drift = abs(exoPlayer.currentPosition - state.positionMs)
        if (drift > 350L) {
            exoPlayer.seekTo(state.positionMs.coerceAtLeast(0L))
        }
    }

    Column(
        modifier = Modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        when {
            video.isLoading -> {
                CircularProgressIndicator(color = palette.accent)
                Spacer(Modifier.height(10.dp))
                Text("Resolving official music video...", color = palette.textSecondary)
            }
            selectedOption == null -> {
                Text("No music video stream available.", color = palette.textSecondary)
                Spacer(Modifier.height(8.dp))
                TextButton(onClick = onRetry) { Text("Retry Video", color = palette.accent) }
            }
            else -> {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .aspectRatio(16f / 9f)
                        .clip(RoundedCornerShape(16.dp))
                        .background(Color.Black),
                ) {
                    AndroidView(
                        modifier = Modifier.fillMaxSize(),
                        factory = { ctx ->
                            PlayerView(ctx).apply {
                                layoutParams = ViewGroup.LayoutParams(
                                    ViewGroup.LayoutParams.MATCH_PARENT,
                                    ViewGroup.LayoutParams.MATCH_PARENT,
                                )
                                useController = false
                                resizeMode = AspectRatioFrameLayout.RESIZE_MODE_ZOOM
                                player = exoPlayer
                            }
                        },
                        update = { it.player = exoPlayer },
                    )
                }
                Spacer(Modifier.height(10.dp))
                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    items(video.options.indices.toList()) { idx ->
                        val opt = video.options[idx]
                        FilterChip(
                            selected = idx == selectedOptIdx,
                            onClick = { selectedOptIdx = idx },
                            label = { Text(opt.label) },
                            colors = FilterChipDefaults.filterChipColors(
                                selectedContainerColor = palette.accent,
                                selectedLabelColor = Color.Black,
                                containerColor = palette.surface,
                                labelColor = palette.textPrimary,
                            ),
                        )
                    }
                }
                Text(
                    text = if (video.isOfficialMv) "Official MV · Synced to Rust DSP Audio" else "Video · Synced to Rust Audio",
                    style = MaterialTheme.typography.labelSmall,
                    color = palette.textSecondary,
                )
            }
        }
    }
}
