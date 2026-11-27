package dev.livelydesk.ferric.ui.screens

import android.os.SystemClock
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
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.Bedtime
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FavoriteBorder
import androidx.compose.material.icons.rounded.FormatQuote
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.Palette
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Repeat
import androidx.compose.material.icons.rounded.RepeatOne
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.AspectRatioFrameLayout
import androidx.media3.ui.PlayerView
import coil3.compose.AsyncImage
import dev.livelydesk.ferric.core.PlaybackProgress
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.formatDurationMs
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.components.AuroraBackdrop
import dev.livelydesk.ferric.ui.theme.LocalBackdropState
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricBackdropSource
import dev.livelydesk.ferric.ui.theme.ferricLiquidGlass
import dev.livelydesk.ferric.ui.theme.rememberFerricBackdropState
import java.util.Locale
import kotlin.math.abs
import kotlinx.coroutines.flow.StateFlow
import uniffi.ferric_mobile_ffi.FfiConnectPeer
import uniffi.ferric_mobile_ffi.FfiRepeatMode

private enum class MediaMode(val label: String) {
    AUDIO("Audio"),
    VIDEO("Video"),
}

private val SLEEP_PRESET_MINUTES = listOf(0, 15, 30, 45, 60, 90)
private val SPEED_CYCLE = listOf(0.75f, 1.0f, 1.15f, 1.25f, 1.5f)

@kotlin.OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FullPlayerScreen(
    state: PlaybackUiState,
    progressFlow: StateFlow<PlaybackProgress>,
    onCollapse: () -> Unit,
    onTogglePlayPause: () -> Unit,
    onNext: () -> Unit,
    onPrev: () -> Unit,
    onSeek: (Long) -> Unit,
    onSetVolume: (String, Float) -> Unit,
    onToggleShuffle: () -> Unit,
    onCycleRepeat: () -> Unit,
    onToggleLike: (String) -> Unit,
    onSetSleepTimerMinutes: (Int) -> Unit,
    onSetPlaybackSpeed: (Float) -> Unit,
    onOpenArtist: (String) -> Unit,
    onOpenQueue: () -> Unit,
    onOpenEqualizer: () -> Unit,
    onOpenThemeStudio: () -> Unit,
    onOpenConnect: () -> Unit,
    onOpenShareCard: () -> Unit,
    onOpenOutputSwitcher: () -> Unit,
    onLoadLyrics: () -> Unit,
    onNudgeLyricsOffset: (Long) -> Unit,
    onResolveVideo: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val playerBackdrop = rememberFerricBackdropState()
    var mediaMode by rememberSaveable { mutableStateOf(MediaMode.AUDIO) }
    var showLyrics by rememberSaveable { mutableStateOf(false) }
    var showSleepSheet by rememberSaveable { mutableStateOf(false) }

    val activeRemote: FfiConnectPeer? = if (!state.connectState.isLocalActiveSpeaker) {
        state.connectState.peers.firstOrNull {
            it.connected && it.deviceId == state.connectState.activeSpeakerId
        }
    } else {
        null
    }
    val track = state.currentTrack
    val displayTitle = activeRemote?.nowPlayingTitle?.ifBlank { null } ?: track?.title ?: "Nothing playing"
    val displayArtist = activeRemote?.nowPlayingArtist?.ifBlank { null } ?: track?.artist ?: "Pick a track to start"
    val displayThumb = activeRemote?.nowPlayingThumbnailUrl ?: track?.thumbnailUrl
    val displayPlaying = activeRemote?.isPlaying ?: state.isPlaying
    val isLiquidCardLayout = palette.playerLayout == "liquid_glass" && !showLyrics && mediaMode == MediaMode.AUDIO
    val speed = state.dspSettings?.playbackSpeed ?: 1.0f

    val activeTargetId = activeRemote?.deviceId ?: state.connectState.localDeviceId
    val remoteVol = activeRemote?.volumeNorm ?: state.localVolume
    var draggingVol by remember { mutableStateOf(false) }
    var draftVol by remember(activeTargetId) { mutableFloatStateOf(remoteVol.coerceIn(0f, 1f)) }
    LaunchedEffect(remoteVol, draggingVol) {
        if (!draggingVol) draftVol = remoteVol.coerceIn(0f, 1f)
    }

    LaunchedEffect(showLyrics, mediaMode, track?.id) {
        if (track == null) return@LaunchedEffect
        if (showLyrics && state.lyricsState.trackId != track.id && !state.lyricsState.isLoading) {
            onLoadLyrics()
        }
        if (mediaMode == MediaMode.VIDEO && (state.videoStreams.trackId != track.id || state.videoStreams.options.isEmpty())) {
            onResolveVideo()
        }
    }

    // Deep cover-derived canvas (Auralys rich atmospheric tone)
    val deepTone = remember(state.coverShade, palette.isLight) {
        if (palette.isLight) {
            palette.canvas
        } else {
            Color(
                red = (state.coverShade.orb1.red * 0.42f + 0.04f).coerceIn(0f, 0.28f),
                green = (state.coverShade.orb1.green * 0.42f + 0.04f).coerceIn(0f, 0.28f),
                blue = (state.coverShade.orb1.blue * 0.42f + 0.05f).coerceIn(0f, 0.30f),
                alpha = 1f,
            )
        }
    }
    val onCanvasPrimary = if (palette.isLight && !isLiquidCardLayout && mediaMode == MediaMode.AUDIO && showLyrics) {
        palette.textPrimary
    } else {
        Color.White
    }
    val onCanvasSecondary = onCanvasPrimary.copy(alpha = 0.72f)

    CompositionLocalProvider(LocalBackdropState provides playerBackdrop) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(deepTone),
        ) {
            // Layer 1: Optical backdrop captured into playerBackdrop for true AGSL Liquid Glass pills
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .ferricBackdropSource(playerBackdrop),
            ) {
                if (!showLyrics && mediaMode == MediaMode.AUDIO && !displayThumb.isNullOrBlank()) {
                    // Full-bleed edge-to-edge artwork (atmospherically blurred under Liquid Glass card
                    // so faces never double-render; razor-sharp in Full-Bleed layout).
                    Box(modifier = Modifier.fillMaxSize()) {
                        AsyncImage(
                            model = displayThumb,
                            contentDescription = null,
                            contentScale = ContentScale.Crop,
                            modifier = Modifier
                                .then(
                                    if (isLiquidCardLayout) {
                                        Modifier
                                            .fillMaxSize()
                                            .blur(44.dp)
                                    } else {
                                        Modifier
                                            .fillMaxWidth()
                                            .aspectRatio(0.92f)
                                            .align(Alignment.TopCenter)
                                    },
                                ),
                        )
                        Box(
                            modifier = Modifier
                                .fillMaxSize()
                                .background(
                                    Brush.verticalGradient(
                                        0.0f to Color.Black.copy(alpha = if (isLiquidCardLayout) 0.42f else 0.30f),
                                        0.20f to deepTone.copy(alpha = if (isLiquidCardLayout) 0.32f else 0.0f),
                                        0.48f to deepTone.copy(alpha = if (isLiquidCardLayout) 0.56f else 0.28f),
                                        0.64f to deepTone.copy(alpha = 0.88f),
                                        0.78f to deepTone,
                                        1.0f to Color.Black.copy(alpha = 0.92f),
                                    ),
                                ),
                        )
                    }
                } else {
                    AuroraBackdrop(
                        colors = state.coverShade,
                        baseCanvas = deepTone,
                        isPlaying = displayPlaying,
                        intensity = 0.82f,
                    )
                }
            }

            // Layer 2: Foreground controls + glass overlays
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .statusBarsPadding()
                    .navigationBarsPadding()
                    .padding(horizontal = 20.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.SpaceBetween,
            ) {
                // Top Bar: Collapse + Centered Floating Glass "Audio | Video" Pill + Theme Studio
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Box(
                        modifier = Modifier
                            .size(40.dp)
                            .ferricLiquidGlass(
                                cornerRadius = 20.dp,
                                blurRadius = 14.dp,
                                refractionHeight = 12.dp,
                                refractionAmount = 10.dp,
                                chromaticDispersion = 0.45f,
                                alwaysGlass = true,
                            )
                            .clickable(onClick = onCollapse),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Rounded.KeyboardArrowDown,
                            contentDescription = "Minimize player",
                            tint = Color.White,
                            modifier = Modifier.size(26.dp),
                        )
                    }

                    // Auralys-style Floating Liquid Glass Audio | Video Segmented Pill
                    Row(
                        modifier = Modifier
                            .height(38.dp)
                            .ferricLiquidGlass(
                                cornerRadius = 19.dp,
                                blurRadius = 16.dp,
                                refractionHeight = 14.dp,
                                refractionAmount = 12.dp,
                                chromaticDispersion = 0.55f,
                                alwaysGlass = true,
                            )
                            .padding(3.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        MediaMode.entries.forEach { mode ->
                            val selected = mediaMode == mode && !showLyrics
                            Box(
                                modifier = Modifier
                                    .clip(RoundedCornerShape(16.dp))
                                    .background(if (selected) Color.White.copy(alpha = 0.22f) else Color.Transparent)
                                    .clickable {
                                        showLyrics = false
                                        mediaMode = mode
                                    }
                                    .padding(horizontal = 16.dp, vertical = 6.dp),
                                contentAlignment = Alignment.Center,
                            ) {
                                Text(
                                    text = mode.label,
                                    style = MaterialTheme.typography.labelLarge,
                                    color = if (selected) Color.White else Color.White.copy(alpha = 0.72f),
                                )
                            }
                        }
                    }

                    Box(
                        modifier = Modifier
                            .size(40.dp)
                            .ferricLiquidGlass(
                                cornerRadius = 20.dp,
                                blurRadius = 14.dp,
                                refractionHeight = 12.dp,
                                refractionAmount = 10.dp,
                                chromaticDispersion = 0.45f,
                                alwaysGlass = true,
                            )
                            .clickable(onClick = onOpenThemeStudio),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Rounded.Palette,
                            contentDescription = "Theme Studio",
                            tint = Color.White,
                            modifier = Modifier.size(20.dp),
                        )
                    }
                }

                // Middle Stage: Full-Bleed breathing space (or Liquid Glass card), Video, or Synced Lyrics
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f)
                        .padding(vertical = 8.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    when {
                        showLyrics -> {
                            SyncedLyricsStage(
                                state = state,
                                progressFlow = progressFlow,
                                onSeek = onSeek,
                                onNudgeOffset = onNudgeLyricsOffset,
                                onRetry = onLoadLyrics,
                                onOpenShareCard = onOpenShareCard,
                            )
                        }
                        mediaMode == MediaMode.VIDEO -> {
                            DockedVideoStage(
                                state = state,
                                progressFlow = progressFlow,
                                playbackSpeed = speed,
                                onRetry = onResolveVideo,
                            )
                        }
                        isLiquidCardLayout -> {
                            // LG3 glowing frosted glass artwork card + live spectrum
                            Column(
                                horizontalAlignment = Alignment.CenterHorizontally,
                                verticalArrangement = Arrangement.Center,
                                modifier = Modifier.fillMaxSize(),
                            ) {
                                Box(
                                    modifier = Modifier
                                        .size(276.dp)
                                        .ferricLiquidGlass(
                                            cornerRadius = 28.dp,
                                            blurRadius = 18.dp,
                                            refractionHeight = 20.dp,
                                            refractionAmount = 14.dp,
                                            chromaticDispersion = 0.55f,
                                            alwaysGlass = true,
                                        )
                                        .padding(10.dp),
                                ) {
                                    ArtworkThumb(
                                        url = displayThumb,
                                        size = 256.dp,
                                        cornerRadius = 20.dp,
                                        isActive = displayPlaying,
                                        modifier = Modifier.fillMaxSize(),
                                    )
                                }
                                Spacer(Modifier.height(14.dp))
                                LiveSpectrumRow(
                                    progressFlow = progressFlow,
                                    accent = palette.accent,
                                    trackColor = Color.White.copy(alpha = 0.22f),
                                )
                            }
                        }
                        else -> {
                            // Full-bleed layout leaves upper cover unobstructed; show artwork placeholder if no cover
                            if (displayThumb.isNullOrBlank()) {
                                ArtworkThumb(
                                    url = null,
                                    size = 240.dp,
                                    cornerRadius = 24.dp,
                                    isActive = displayPlaying,
                                )
                            }
                        }
                    }
                }

                // Bottom Transport Area
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .then(
                            if (isLiquidCardLayout) {
                                Modifier
                                    .ferricLiquidGlass(
                                        cornerRadius = 28.dp,
                                        blurRadius = 20.dp,
                                        refractionHeight = 20.dp,
                                        refractionAmount = 16.dp,
                                        chromaticDispersion = 0.58f,
                                        alwaysGlass = true,
                                    )
                                    .padding(horizontal = 18.dp, vertical = 16.dp)
                            } else {
                                Modifier.padding(horizontal = 2.dp, vertical = 4.dp)
                            },
                        ),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    // Track Title + Artist + Frosted Like & Speed chips
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                text = displayTitle,
                                style = MaterialTheme.typography.headlineSmall,
                                fontWeight = FontWeight.Bold,
                                color = onCanvasPrimary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Spacer(Modifier.height(2.dp))
                            Text(
                                text = displayArtist,
                                style = MaterialTheme.typography.bodyLarge,
                                color = onCanvasSecondary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.clickable(enabled = track != null && activeRemote == null) {
                                    track?.artist?.let {
                                        onCollapse()
                                        onOpenArtist(it)
                                    }
                                },
                            )
                        }
                        Spacer(Modifier.width(8.dp))
                        // Playback speed pill (cycles 0.75x .. 1.5x)
                        Surface(
                            shape = RoundedCornerShape(999.dp),
                            color = Color.White.copy(alpha = if (abs(speed - 1.0f) > 0.02f) 0.24f else 0.12f),
                            modifier = Modifier.clickable {
                                val curIdx = SPEED_CYCLE.indexOfFirst { abs(it - speed) < 0.03f }.coerceAtLeast(0)
                                val nextSpeed = SPEED_CYCLE[(curIdx + 1) % SPEED_CYCLE.size]
                                onSetPlaybackSpeed(nextSpeed)
                            },
                        ) {
                            Text(
                                text = if (abs(speed - 1.0f) < 0.02f) "1x" else String.format(Locale.US, "%.2gx", speed),
                                style = MaterialTheme.typography.labelSmall,
                                color = if (abs(speed - 1.0f) > 0.02f) palette.accent else onCanvasPrimary,
                                modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
                            )
                        }
                        if (track != null) {
                            Spacer(Modifier.width(6.dp))
                            Box(
                                modifier = Modifier
                                    .size(40.dp)
                                    .clip(CircleShape)
                                    .background(Color.White.copy(alpha = 0.12f))
                                    .clickable { onToggleLike(track.id) },
                                contentAlignment = Alignment.Center,
                            ) {
                                Icon(
                                    imageVector = if (track.liked) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder,
                                    contentDescription = if (track.liked) "Unlike" else "Like",
                                    tint = if (track.liked) palette.accent else onCanvasPrimary,
                                    modifier = Modifier.size(21.dp),
                                )
                            }
                        }
                    }

                    // Jitter-free isolated Seekbar + Elapsed / Codec / Remaining row
                    PlayerSeekSection(
                        progressFlow = progressFlow,
                        remotePositionMs = activeRemote?.positionMs?.toLong(),
                        remoteDurationMs = activeRemote?.durationMs?.toLong(),
                        codecLabel = buildString {
                            val c = track?.codec.orEmpty()
                            if (c.isNotBlank()) append(c)
                            if (state.outputRate > 0) {
                                if (isNotEmpty()) append(" · ")
                                append(String.format(Locale.US, "%.1f kHz", state.outputRate / 1000.0))
                            }
                        },
                        accent = palette.accent,
                        onCanvasPrimary = onCanvasPrimary,
                        onCanvasSecondary = onCanvasSecondary,
                        onSeek = onSeek,
                    )

                    // Transport Controls
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        IconButton(onClick = onToggleShuffle) {
                            Icon(
                                imageVector = Icons.Rounded.Shuffle,
                                contentDescription = "Shuffle",
                                tint = if (state.shuffle) palette.accent else onCanvasSecondary,
                                modifier = Modifier.size(23.dp),
                            )
                        }
                        IconButton(onClick = onPrev, modifier = Modifier.size(48.dp)) {
                            Icon(
                                imageVector = Icons.Rounded.SkipPrevious,
                                contentDescription = "Previous",
                                tint = onCanvasPrimary,
                                modifier = Modifier.size(36.dp),
                            )
                        }
                        Surface(
                            shape = CircleShape,
                            color = Color.White,
                            modifier = Modifier.size(66.dp),
                        ) {
                            IconButton(onClick = onTogglePlayPause) {
                                Icon(
                                    imageVector = if (displayPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                                    contentDescription = if (displayPlaying) "Pause" else "Play",
                                    tint = Color.Black,
                                    modifier = Modifier.size(36.dp),
                                )
                            }
                        }
                        IconButton(onClick = onNext, modifier = Modifier.size(48.dp)) {
                            Icon(
                                imageVector = Icons.Rounded.SkipNext,
                                contentDescription = "Next",
                                tint = onCanvasPrimary,
                                modifier = Modifier.size(36.dp),
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
                                tint = if (state.repeatMode != FfiRepeatMode.OFF) palette.accent else onCanvasSecondary,
                                modifier = Modifier.size(23.dp),
                            )
                        }
                    }

                    // Inline Volume Slider + Output / Connect Pill (mirrors remote Desktop volume!)
                    if (!showLyrics) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Icon(
                                imageVector = Icons.AutoMirrored.Rounded.VolumeUp,
                                contentDescription = "Volume",
                                tint = onCanvasSecondary,
                                modifier = Modifier.size(18.dp),
                            )
                            SleekScrubSlider(
                                value = draftVol,
                                valueRange = 0f..1f,
                                activeColor = Color.White.copy(alpha = 0.90f),
                                inactiveColor = Color.White.copy(alpha = 0.22f),
                                thumbSize = 11.dp,
                                trackHeight = 3.5.dp,
                                onValueChange = { next ->
                                    draggingVol = true
                                    draftVol = next
                                    onSetVolume(activeTargetId, next)
                                },
                                onValueChangeFinished = {
                                    draggingVol = false
                                    onSetVolume(activeTargetId, draftVol)
                                },
                                modifier = Modifier.weight(1f),
                            )
                            Surface(
                                shape = RoundedCornerShape(999.dp),
                                color = if (activeRemote != null) {
                                    palette.accent.copy(alpha = 0.24f)
                                } else {
                                    Color.White.copy(alpha = 0.12f)
                                },
                                modifier = Modifier.clickable(onClick = onOpenConnect),
                            ) {
                                Row(
                                    modifier = Modifier.padding(horizontal = 10.dp, vertical = 5.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                                ) {
                                    Icon(
                                        imageVector = if (activeRemote != null) Icons.Rounded.Devices else Icons.Rounded.Speaker,
                                        contentDescription = "Output device",
                                        tint = if (activeRemote != null) palette.accent else onCanvasPrimary,
                                        modifier = Modifier.size(15.dp),
                                    )
                                    Text(
                                        text = activeRemote?.deviceName ?: state.activeOutputRoute,
                                        style = MaterialTheme.typography.labelSmall,
                                        color = onCanvasPrimary,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                            }
                        }
                    }

                    // Bottom Action Icon Dock: Lyrics | Queue | Connect | EQ | Share | Sleep Timer
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        IconButton(onClick = { showLyrics = !showLyrics }) {
                            Icon(
                                imageVector = Icons.Rounded.FormatQuote,
                                contentDescription = "Synced Lyrics",
                                tint = if (showLyrics) palette.accent else onCanvasSecondary,
                                modifier = Modifier.size(23.dp),
                            )
                        }
                        IconButton(onClick = onOpenQueue) {
                            Icon(
                                imageVector = Icons.AutoMirrored.Rounded.QueueMusic,
                                contentDescription = "Queue",
                                tint = onCanvasSecondary,
                                modifier = Modifier.size(23.dp),
                            )
                        }
                        IconButton(onClick = onOpenConnect) {
                            val hasPeer = state.connectState.peers.any { it.connected }
                            Icon(
                                imageVector = Icons.Rounded.Devices,
                                contentDescription = "Ferric Connect",
                                tint = if (hasPeer) palette.accent else onCanvasSecondary,
                                modifier = Modifier.size(22.dp),
                            )
                        }
                        IconButton(onClick = onOpenEqualizer) {
                            Icon(
                                imageVector = Icons.Rounded.Equalizer,
                                contentDescription = "Equalizer & DSP",
                                tint = if (state.eqConfig?.enabled == true) palette.accent else onCanvasSecondary,
                                modifier = Modifier.size(22.dp),
                            )
                        }
                        IconButton(onClick = onOpenShareCard, enabled = track != null) {
                            Icon(
                                imageVector = Icons.Rounded.Share,
                                contentDescription = "Share Lyrics Card",
                                tint = onCanvasSecondary,
                                modifier = Modifier.size(21.dp),
                            )
                        }
                        Row(
                            modifier = Modifier
                                .clip(RoundedCornerShape(999.dp))
                                .clickable { showSleepSheet = true }
                                .padding(horizontal = 8.dp, vertical = 6.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(4.dp),
                        ) {
                            Icon(
                                imageVector = Icons.Rounded.Bedtime,
                                contentDescription = "Sleep Timer",
                                tint = if (state.sleepTimerRemainingSecs > 0L) palette.accent else onCanvasSecondary,
                                modifier = Modifier.size(21.dp),
                            )
                            if (state.sleepTimerRemainingSecs > 0L) {
                                Text(
                                    text = formatDurationMs(state.sleepTimerRemainingSecs * 1_000L),
                                    style = MaterialTheme.typography.labelSmall,
                                    color = palette.accent,
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    if (showSleepSheet) {
        ModalBottomSheet(
            onDismissRequest = { showSleepSheet = false },
            containerColor = palette.surface,
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 20.dp, vertical = 10.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Text(
                    text = "Sleep Timer",
                    style = MaterialTheme.typography.titleLarge,
                    color = palette.textPrimary,
                )
                Text(
                    text = if (state.sleepTimerRemainingSecs > 0L) {
                        "Playback will fade out and pause in ${formatDurationMs(state.sleepTimerRemainingSecs * 1_000L)}."
                    } else {
                        "Automatically pause playback after the selected duration."
                    },
                    style = MaterialTheme.typography.bodyMedium,
                    color = palette.textSecondary,
                )
                SLEEP_PRESET_MINUTES.forEach { mins ->
                    Surface(
                        shape = RoundedCornerShape(14.dp),
                        color = palette.elevated,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable {
                                onSetSleepTimerMinutes(mins)
                                showSleepSheet = false
                            },
                    ) {
                        Row(
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 14.dp),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                text = if (mins == 0) "Turn Off Timer" else "$mins minutes",
                                style = MaterialTheme.typography.titleMedium,
                                color = if (mins == 0) palette.accent else palette.textPrimary,
                            )
                            if (mins == 0 && state.sleepTimerRemainingSecs > 0L) {
                                Text("Active", style = MaterialTheme.typography.labelSmall, color = palette.accent)
                            }
                        }
                    }
                }
                TextButton(
                    onClick = {
                        showSleepSheet = false
                        onOpenOutputSwitcher()
                    },
                    modifier = Modifier.align(Alignment.End),
                ) {
                    Text("System Audio Output", color = palette.textSecondary)
                }
                Spacer(Modifier.height(16.dp))
            }
        }
    }
}

@Composable
private fun PlayerSeekSection(
    progressFlow: StateFlow<PlaybackProgress>,
    remotePositionMs: Long?,
    remoteDurationMs: Long?,
    codecLabel: String,
    accent: Color,
    onCanvasPrimary: Color,
    onCanvasSecondary: Color,
    onSeek: (Long) -> Unit,
) {
    val progress by progressFlow.collectAsStateWithLifecycle()
    val effectiveDur = (remoteDurationMs ?: progress.durationMs).coerceAtLeast(1L)
    val effectivePos = (remotePositionMs ?: progress.positionMs).coerceIn(0L, effectiveDur)

    var isDragging by remember { mutableStateOf(false) }
    var dragPosMs by remember { mutableFloatStateOf(effectivePos.toFloat()) }

    val sliderValue = if (isDragging) dragPosMs else effectivePos.toFloat()
    val shownPosMs = sliderValue.toLong().coerceIn(0L, effectiveDur)
    val remainingMs = (effectiveDur - shownPosMs).coerceAtLeast(0L)

    Column(modifier = Modifier.fillMaxWidth()) {
        SleekScrubSlider(
            value = sliderValue.coerceIn(0f, effectiveDur.toFloat()),
            valueRange = 0f..effectiveDur.toFloat(),
            activeColor = onCanvasPrimary,
            inactiveColor = onCanvasPrimary.copy(alpha = 0.22f),
            thumbSize = 13.dp,
            trackHeight = 4.dp,
            onValueChange = { v ->
                isDragging = true
                dragPosMs = v
            },
            onValueChangeFinished = {
                val target = dragPosMs.toLong().coerceAtLeast(0L)
                isDragging = false
                onSeek(target)
            },
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(2.dp))
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = formatDurationMs(shownPosMs),
                style = MaterialTheme.typography.labelSmall,
                color = onCanvasSecondary,
            )
            if (codecLabel.isNotBlank()) {
                Surface(
                    shape = RoundedCornerShape(999.dp),
                    color = accent.copy(alpha = 0.16f),
                ) {
                    Text(
                        text = codecLabel,
                        style = MaterialTheme.typography.labelSmall,
                        color = accent,
                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 2.dp),
                    )
                }
            }
            Text(
                text = "-${formatDurationMs(remainingMs)}",
                style = MaterialTheme.typography.labelSmall,
                color = onCanvasSecondary,
            )
        }
    }
}

@kotlin.OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SleekScrubSlider(
    value: Float,
    valueRange: ClosedFloatingPointRange<Float>,
    activeColor: Color,
    inactiveColor: Color,
    thumbSize: Dp,
    trackHeight: Dp,
    onValueChange: (Float) -> Unit,
    onValueChangeFinished: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Slider(
        value = value,
        onValueChange = onValueChange,
        onValueChangeFinished = onValueChangeFinished,
        valueRange = valueRange,
        modifier = modifier.height(28.dp),
        thumb = {
            Box(
                modifier = Modifier
                    .size(thumbSize)
                    .clip(CircleShape)
                    .background(activeColor),
            )
        },
        track = { sliderState ->
            val span = (sliderState.valueRange.endInclusive - sliderState.valueRange.start)
                .coerceAtLeast(1e-5f)
            val fraction = ((sliderState.value - sliderState.valueRange.start) / span)
                .coerceIn(0f, 1f)
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(trackHeight)
                    .clip(RoundedCornerShape(999.dp))
                    .background(inactiveColor),
            ) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth(fraction)
                        .height(trackHeight)
                        .clip(RoundedCornerShape(999.dp))
                        .background(activeColor),
                )
            }
        },
    )
}

@Composable
private fun LiveSpectrumRow(
    progressFlow: StateFlow<PlaybackProgress>,
    accent: Color,
    trackColor: Color,
) {
    val progress by progressFlow.collectAsStateWithLifecycle()
    Row(
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.Bottom,
        modifier = Modifier.height(26.dp),
    ) {
        progress.spectrumBins.forEach { raw ->
            val frac = (raw / 1000f).coerceIn(0.10f, 1f)
            Box(
                modifier = Modifier
                    .width(7.dp)
                    .height((26f * frac).dp)
                    .clip(RoundedCornerShape(4.dp))
                    .background(if (raw > 30) accent else trackColor),
            )
        }
    }
}

@Composable
private fun SyncedLyricsStage(
    state: PlaybackUiState,
    progressFlow: StateFlow<PlaybackProgress>,
    onSeek: (Long) -> Unit,
    onNudgeOffset: (Long) -> Unit,
    onRetry: () -> Unit,
    onOpenShareCard: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val progress by progressFlow.collectAsStateWithLifecycle()
    val lyrics = state.lyricsState
    val listState = rememberLazyListState()

    val effectivePosMs = (progress.positionMs + lyrics.offsetMs).coerceAtLeast(0L)
    val activeIdx = remember(lyrics.lines, lyrics.isSynced, effectivePosMs) {
        if (!lyrics.isSynced) return@remember -1
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

    LaunchedEffect(activeIdx, lyrics.isSynced) {
        if (lyrics.isSynced && activeIdx >= 0 && !listState.isScrollInProgress) {
            listState.animateScrollToItem((activeIdx - 2).coerceAtLeast(0))
        }
    }

    Column(
        modifier = Modifier.fillMaxSize(),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = if (lyrics.sourceName.isNotBlank()) {
                    if (lyrics.isSynced) {
                        "${lyrics.sourceName} · ${String.format(Locale.US, "%+.1fs", lyrics.offsetMs / 1000.0)}"
                    } else {
                        "${lyrics.sourceName} · Unsynced"
                    }
                } else {
                    "Lyrics"
                },
                style = MaterialTheme.typography.labelSmall,
                color = Color.White.copy(alpha = 0.72f),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                if (lyrics.isSynced) {
                    OutlinedButton(
                        onClick = { onNudgeOffset(-500L) },
                        border = BorderStroke(1.dp, Color.White.copy(alpha = 0.24f)),
                        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp),
                    ) { Text("-0.5s", color = Color.White, style = MaterialTheme.typography.labelSmall) }
                    OutlinedButton(
                        onClick = { onNudgeOffset(500L) },
                        border = BorderStroke(1.dp, Color.White.copy(alpha = 0.24f)),
                        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp),
                    ) { Text("+0.5s", color = Color.White, style = MaterialTheme.typography.labelSmall) }
                }
                OutlinedButton(
                    onClick = onOpenShareCard,
                    border = BorderStroke(1.dp, palette.accent),
                    contentPadding = PaddingValues(horizontal = 10.dp, vertical = 2.dp),
                ) { Text("Card", color = palette.accent, style = MaterialTheme.typography.labelSmall) }
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
                    Text("No lyrics found for this track.", color = Color.White.copy(alpha = 0.72f))
                    Spacer(Modifier.height(8.dp))
                    TextButton(onClick = onRetry) { Text("Retry Lyrics", color = palette.accent) }
                }
            }
            else -> {
                LazyColumn(
                    state = listState,
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(vertical = 28.dp),
                    verticalArrangement = Arrangement.spacedBy(18.dp),
                ) {
                    itemsIndexed(lyrics.lines, key = { idx, line -> "${idx}_${line.timestampMs}" }) { idx, line ->
                        val isActive = lyrics.isSynced && idx == activeIdx
                        val lineAlpha = when {
                            !lyrics.isSynced -> 0.88f
                            isActive -> 1.0f
                            idx < activeIdx -> 0.42f
                            else -> 0.56f
                        }
                        Text(
                            text = line.text,
                            fontSize = if (isActive) 26.sp else 21.sp,
                            lineHeight = if (isActive) 34.sp else 28.sp,
                            fontWeight = if (isActive) FontWeight.Bold else FontWeight.SemiBold,
                            color = Color.White.copy(alpha = lineAlpha),
                            textAlign = TextAlign.Start,
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(10.dp))
                                .clickable(enabled = lyrics.isSynced) {
                                    val targetMs = (line.timestampMs.toLong() - lyrics.offsetMs).coerceAtLeast(0L)
                                    onSeek(targetMs)
                                }
                                .padding(horizontal = 6.dp, vertical = 4.dp),
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
    progressFlow: StateFlow<PlaybackProgress>,
    playbackSpeed: Float,
    onRetry: () -> Unit,
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val palette = LocalFerricPalette.current
    val progress by progressFlow.collectAsStateWithLifecycle()
    val video = state.videoStreams
    var selectedOptIdx by remember(video.trackId, video.options.size) { mutableIntStateOf(0) }
    val selectedOption = video.options.getOrNull(selectedOptIdx)
    var lastSyncSeekUptimeMs by remember { mutableLongStateOf(0L) }

    val exoPlayer = remember(context) {
        ExoPlayer.Builder(context).build().apply {
            // Disable ExoPlayer audio decoder completely; Rust Oboe owns 100% of DSP/EQ audio.
            trackSelectionParameters = TrackSelectionParameters.Builder()
                .setTrackTypeDisabled(C.TRACK_TYPE_AUDIO, true)
                .build()
            volume = 0f
            repeatMode = Player.REPEAT_MODE_OFF
        }
    }

    DisposableEffect(exoPlayer, lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_STOP -> exoPlayer.playWhenReady = false
                Lifecycle.Event.ON_START -> {
                    if (selectedOption != null) {
                        exoPlayer.playWhenReady = state.isPlaying
                    }
                }
                else -> {}
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            exoPlayer.release()
        }
    }

    LaunchedEffect(playbackSpeed) {
        exoPlayer.playbackParameters = PlaybackParameters(playbackSpeed.coerceIn(0.5f, 2.0f))
    }

    LaunchedEffect(selectedOption?.url) {
        val url = selectedOption?.url
        if (!url.isNullOrBlank()) {
            exoPlayer.setMediaItem(MediaItem.fromUri(url))
            exoPlayer.prepare()
            exoPlayer.seekTo(progress.positionMs.coerceAtLeast(0L))
            exoPlayer.playWhenReady = state.isPlaying
            lastSyncSeekUptimeMs = SystemClock.uptimeMillis()
        } else {
            exoPlayer.stop()
        }
    }

    // Lock ExoPlayer video surface to Rust audio clock only when STATE_READY with cooldown
    // (prevents buffering seek loops on slow connections).
    LaunchedEffect(progress.positionMs, state.isPlaying, state.seekSyncNonce, selectedOption?.url) {
        if (selectedOption == null) return@LaunchedEffect
        exoPlayer.playWhenReady = state.isPlaying
        if (exoPlayer.playbackState == Player.STATE_READY) {
            val now = SystemClock.uptimeMillis()
            val drift = abs(exoPlayer.currentPosition - progress.positionMs)
            if (drift > 450L && (now - lastSyncSeekUptimeMs) > 1_500L) {
                lastSyncSeekUptimeMs = now
                exoPlayer.seekTo(progress.positionMs.coerceAtLeast(0L))
            }
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
                Text("Resolving official music video...", color = Color.White.copy(alpha = 0.72f))
            }
            selectedOption == null -> {
                Text("No music video stream available.", color = Color.White.copy(alpha = 0.72f))
                Spacer(Modifier.height(8.dp))
                TextButton(onClick = onRetry) { Text("Retry Video", color = palette.accent) }
            }
            else -> {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .aspectRatio(16f / 9f)
                        .clip(RoundedCornerShape(20.dp))
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
                                containerColor = Color.White.copy(alpha = 0.14f),
                                labelColor = Color.White,
                            ),
                        )
                    }
                }
                Text(
                    text = if (video.isOfficialMv) {
                        "Official MV · Synced to Rust DSP Audio"
                    } else {
                        "Video · Synced to Rust Audio"
                    },
                    style = MaterialTheme.typography.labelSmall,
                    color = Color.White.copy(alpha = 0.68f),
                )
            }
        }
    }
}
