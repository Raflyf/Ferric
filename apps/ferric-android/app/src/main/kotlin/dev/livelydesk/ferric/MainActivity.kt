package dev.livelydesk.ferric

import android.content.ComponentName
import android.os.Bundle
import java.util.Locale
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
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
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Repeat
import androidx.compose.material.icons.rounded.RepeatOne
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import dev.livelydesk.ferric.core.CoreStateStore
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.playback.FerricPlaybackService
import dev.livelydesk.ferric.playback.OutputRouteHelper
import dev.livelydesk.ferric.ui.theme.AmoledBlack
import dev.livelydesk.ferric.ui.theme.FerricOrange
import dev.livelydesk.ferric.ui.theme.FerricTheme
import dev.livelydesk.ferric.ui.theme.HairlineBorder
import dev.livelydesk.ferric.ui.theme.ObsidianElevated
import dev.livelydesk.ferric.ui.theme.ObsidianSurface
import dev.livelydesk.ferric.ui.theme.TextPrimary
import dev.livelydesk.ferric.ui.theme.TextSecondary
import uniffi.ferric_mobile_ffi.FfiRepeatMode
import uniffi.ferric_mobile_ffi.FfiTrack

class MainActivity : ComponentActivity() {
    private var controllerFuture: ListenableFuture<MediaController>? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.BLACK),
            navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.BLACK),
        )
        val store = (application as FerricApp).store
        store.bootstrapLibrary()

        setContent {
            FerricTheme {
                val state by store.uiState.collectAsStateWithLifecycle()
                FerricM1HomeScreen(state = state, store = store)
            }
        }
    }

    override fun onStart() {
        super.onStart()
        val store = (application as FerricApp).store
        store.refreshOutputRoute()
        val token = SessionToken(this, ComponentName(this, FerricPlaybackService::class.java))
        controllerFuture = MediaController.Builder(this, token).buildAsync().also { future ->
            future.addListener({ runCatching { future.get() } }, MoreExecutors.directExecutor())
        }
    }

    override fun onStop() {
        controllerFuture?.let { MediaController.releaseFuture(it) }
        controllerFuture = null
        super.onStop()
    }
}

@Composable
private fun FerricM1HomeScreen(
    state: PlaybackUiState,
    store: CoreStateStore,
) {
    val context = LocalContext.current
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(AmoledBlack)
            .statusBarsPadding()
            .navigationBarsPadding(),
    ) {
        // Top Header + System Output Switcher pill
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Column {
                Text(
                    text = "Ferric",
                    style = MaterialTheme.typography.headlineSmall,
                    color = TextPrimary,
                )
                Text(
                    text = "Rust Oboe Engine · M1 Preview",
                    style = MaterialTheme.typography.labelSmall,
                    color = FerricOrange,
                )
            }
            Surface(
                onClick = {
                    OutputRouteHelper.openSystemOutputSwitcher(context)
                    store.refreshOutputRoute()
                },
                shape = RoundedCornerShape(20.dp),
                color = ObsidianElevated,
                border = BorderStroke(1.dp, HairlineBorder),
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        imageVector = Icons.Rounded.Speaker,
                        contentDescription = "Audio output device",
                        tint = FerricOrange,
                        modifier = Modifier.size(16.dp),
                    )
                    Spacer(Modifier.width(6.dp))
                    Text(
                        text = state.activeOutputRoute,
                        style = MaterialTheme.typography.labelSmall,
                        color = TextPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }

        state.statusMessage?.let { msg ->
            Surface(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 4.dp)
                    .clickable { store.clearStatusMessage() },
                shape = RoundedCornerShape(10.dp),
                color = ObsidianElevated,
                border = BorderStroke(1.dp, FerricOrange.copy(alpha = 0.5f)),
            ) {
                Text(
                    text = msg,
                    style = MaterialTheme.typography.bodyMedium,
                    color = TextPrimary,
                    modifier = Modifier.padding(12.dp),
                )
            }
        }

        // Quick Picks Track List
        LazyColumn(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth(),
            contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item {
                Text(
                    text = "Quick picks",
                    style = MaterialTheme.typography.titleMedium,
                    color = TextPrimary,
                    modifier = Modifier.padding(vertical = 4.dp),
                )
            }
            items(state.tracks, key = { it.id }) { track ->
                val isCurrent = state.currentTrack?.id == track.id
                TrackRowCard(
                    track = track,
                    isActive = isCurrent,
                    isPlaying = isCurrent && state.isPlaying,
                    onClick = { store.playTrack(track.id) },
                )
            }
        }

        // Bottom Player Control Deck
        PlayerDock(state = state, store = store)
    }
}

@Composable
private fun TrackRowCard(
    track: FfiTrack,
    isActive: Boolean,
    isPlaying: Boolean,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        shape = RoundedCornerShape(12.dp),
        color = if (isActive) ObsidianElevated else ObsidianSurface,
        border = BorderStroke(1.dp, if (isActive) FerricOrange else HairlineBorder),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(44.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .background(if (isActive) FerricOrange.copy(alpha = 0.2f) else AmoledBlack),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = if (isPlaying) Icons.Rounded.GraphicEq else Icons.Rounded.MusicNote,
                    contentDescription = null,
                    tint = if (isActive) FerricOrange else TextSecondary,
                )
            }
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = track.title,
                    style = MaterialTheme.typography.titleMedium,
                    color = if (isActive) FerricOrange else TextPrimary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(Modifier.height(2.dp))
                Text(
                    text = "${track.artist} · ${track.album}",
                    style = MaterialTheme.typography.bodyMedium,
                    color = TextSecondary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Spacer(Modifier.width(8.dp))
            Text(
                text = "${track.codec} ${(track.sampleRate.toInt() / 1000.0).let { if (it > 0) String.format(Locale.US, "%.1fk", it) else "" }}".trim(),
                style = MaterialTheme.typography.labelSmall,
                color = TextSecondary,
            )
        }
    }
}

@Composable
private fun PlayerDock(
    state: PlaybackUiState,
    store: CoreStateStore,
) {
    Surface(
        shape = RoundedCornerShape(topStart = 18.dp, topEnd = 18.dp),
        color = ObsidianSurface,
        border = BorderStroke(1.dp, HairlineBorder),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 12.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = state.currentTrack?.title ?: "Tap a track to play",
                        style = MaterialTheme.typography.titleMedium,
                        color = TextPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = state.currentTrack?.artist ?: "Native Rust dual-deck engine ready",
                        style = MaterialTheme.typography.bodyMedium,
                        color = TextSecondary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                if (state.outputRate > 0) {
                    Text(
                        text = "${state.outputRate / 1000} kHz",
                        style = MaterialTheme.typography.labelSmall,
                        color = FerricOrange,
                    )
                }
            }

            val maxDuration = state.durationMs.coerceAtLeast(1L)
            val progress = (state.positionMs.toFloat() / maxDuration.toFloat()).coerceIn(0f, 1f)
            Slider(
                value = progress,
                onValueChange = { frac -> store.seekTo((frac * maxDuration).toLong()) },
                colors = SliderDefaults.colors(
                    thumbColor = FerricOrange,
                    activeTrackColor = FerricOrange,
                    inactiveTrackColor = HairlineBorder,
                ),
                modifier = Modifier.fillMaxWidth(),
            )

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(formatDurationMs(state.positionMs), style = MaterialTheme.typography.labelSmall, color = TextSecondary)
                Text(formatDurationMs(state.durationMs), style = MaterialTheme.typography.labelSmall, color = TextSecondary)
            }

            Spacer(Modifier.height(6.dp))

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceEvenly,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconButton(onClick = { store.toggleShuffle() }, modifier = Modifier.size(48.dp)) {
                    Icon(
                        imageVector = Icons.Rounded.Shuffle,
                        contentDescription = "Shuffle",
                        tint = if (state.shuffle) FerricOrange else TextSecondary,
                    )
                }
                IconButton(onClick = { store.prev() }, modifier = Modifier.size(48.dp)) {
                    Icon(
                        imageVector = Icons.Rounded.SkipPrevious,
                        contentDescription = "Previous",
                        tint = TextPrimary,
                    )
                }
                Surface(
                    onClick = { store.togglePlayPause() },
                    shape = CircleShape,
                    color = FerricOrange,
                    modifier = Modifier.size(56.dp),
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Icon(
                            imageVector = if (state.isPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                            contentDescription = if (state.isPlaying) "Pause" else "Play",
                            tint = AmoledBlack,
                            modifier = Modifier.size(30.dp),
                        )
                    }
                }
                IconButton(onClick = { store.next() }, modifier = Modifier.size(48.dp)) {
                    Icon(
                        imageVector = Icons.Rounded.SkipNext,
                        contentDescription = "Next",
                        tint = TextPrimary,
                    )
                }
                IconButton(onClick = { store.cycleRepeat() }, modifier = Modifier.size(48.dp)) {
                    Icon(
                        imageVector = if (state.repeatMode == FfiRepeatMode.ONE) {
                            Icons.Rounded.RepeatOne
                        } else {
                            Icons.Rounded.Repeat
                        },
                        contentDescription = "Repeat",
                        tint = if (state.repeatMode != FfiRepeatMode.OFF) FerricOrange else TextSecondary,
                    )
                }
            }
        }
    }
}

internal fun formatDurationMs(ms: Long): String {
    val totalSecs = (ms / 1_000L).coerceAtLeast(0L)
    val mins = totalSecs / 60L
    val secs = totalSecs % 60L
    return "%d:%02d".format(mins, secs)
}
