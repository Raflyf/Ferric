package dev.livelydesk.ferric.playback

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Looper
import androidx.annotation.OptIn
import androidx.core.content.ContextCompat
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import dev.livelydesk.ferric.FerricApp
import dev.livelydesk.ferric.core.CoreStateStore
import dev.livelydesk.ferric.core.PlaybackUiState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/**
 * Foreground Media3 [MediaSessionService] bridging Android media notifications, lockscreen
 * transport controls, Bluetooth AVRCP keys, AudioFocus ducking/pausing, and `ACTION_AUDIO_BECOMING_NOISY`
 * directly to Ferric's Rust `oboe-rs` audio engine.
 */
@OptIn(UnstableApi::class)
class FerricPlaybackService : MediaSessionService() {
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private lateinit var store: CoreStateStore
    private lateinit var audioManager: AudioManager
    private lateinit var focusRequest: AudioFocusRequest
    private var mediaSession: MediaSession? = null
    private var bridgePlayer: RustBridgePlayer? = null
    private var noisyRegistered = false
    private var resumeOnFocusGain = false

    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) {
                resumeOnFocusGain = false
                store.pause()
                store.refreshOutputRoute()
            }
        }
    }

    private val focusChangeListener = AudioManager.OnAudioFocusChangeListener { focusChange ->
        when (focusChange) {
            AudioManager.AUDIOFOCUS_LOSS -> {
                resumeOnFocusGain = false
                store.setDucking(false)
                store.pause()
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                resumeOnFocusGain = store.uiState.value.isPlaying
                store.pause()
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                store.setDucking(true)
            }
            AudioManager.AUDIOFOCUS_GAIN -> {
                store.setDucking(false)
                if (resumeOnFocusGain) {
                    resumeOnFocusGain = false
                    store.play()
                }
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        store = (application as FerricApp).store
        audioManager = getSystemService(AUDIO_SERVICE) as AudioManager

        val attrs = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_MEDIA)
            .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
            .build()
        focusRequest = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(attrs)
            .setAcceptsDelayedFocusGain(true)
            .setOnAudioFocusChangeListener(focusChangeListener)
            .build()

        // Lazily initialize Rust Oboe audio inside the foreground MediaSessionService (not Application.onCreate).
        serviceScope.launch(Dispatchers.IO) {
            store.ensureAudioStartedSync()
        }

        val player = RustBridgePlayer(
            looper = Looper.getMainLooper(),
            store = store,
            requestAudioFocus = { requestMediaFocus() },
        )
        bridgePlayer = player
        mediaSession = MediaSession.Builder(this, player).build()

        ContextCompat.registerReceiver(
            this,
            noisyReceiver,
            IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        noisyRegistered = true

        serviceScope.launch {
            store.uiState.collect { ui ->
                player.pushState(ui)
            }
        }
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? {
        return mediaSession
    }

    override fun onDestroy() {
        if (noisyRegistered) {
            unregisterReceiver(noisyReceiver)
            noisyRegistered = false
        }
        audioManager.abandonAudioFocusRequest(focusRequest)
        mediaSession?.release()
        mediaSession = null
        bridgePlayer = null
        serviceScope.cancel()
        super.onDestroy()
    }

    private fun requestMediaFocus(): Boolean {
        val granted = audioManager.requestAudioFocus(focusRequest)
        return granted == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
    }
}

@OptIn(UnstableApi::class)
private class RustBridgePlayer(
    looper: Looper,
    private val store: CoreStateStore,
    private val requestAudioFocus: () -> Boolean,
) : SimpleBasePlayer(looper) {
    private var snapshot: PlaybackUiState = store.uiState.value

    fun pushState(newState: PlaybackUiState) {
        snapshot = newState
        invalidateState()
    }

    override fun getState(): State {
        val current = snapshot.currentTrack
        val items = if (current != null) {
            val metadata = MediaMetadata.Builder()
                .setTitle(current.title)
                .setArtist(current.artist)
                .setAlbumTitle(current.album)
                .build()
            val mediaItem = MediaItem.Builder()
                .setMediaId(current.id)
                .setMediaMetadata(metadata)
                .build()
            listOf(
                MediaItemData.Builder(current.id)
                    .setMediaItem(mediaItem)
                    .setDurationUs(
                        if (snapshot.durationMs > 0L) snapshot.durationMs * 1_000L else C.TIME_UNSET,
                    )
                    .build(),
            )
        } else {
            emptyList()
        }

        val commands = Player.Commands.Builder()
            .addAll(
                Player.COMMAND_PLAY_PAUSE,
                Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
                Player.COMMAND_SEEK_TO_NEXT,
                Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
                Player.COMMAND_SEEK_TO_PREVIOUS,
                Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
                Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
                Player.COMMAND_GET_METADATA,
                Player.COMMAND_GET_TIMELINE,
            )
            .build()

        val playbackState = when {
            items.isEmpty() -> Player.STATE_IDLE
            else -> Player.STATE_READY
        }

        return State.Builder()
            .setAvailableCommands(commands)
            .setPlayWhenReady(snapshot.isPlaying, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
            .setPlaybackState(playbackState)
            .setPlaylist(items)
            .setCurrentMediaItemIndex(if (items.isEmpty()) C.INDEX_UNSET else 0)
            .setContentPositionMs { snapshot.positionMs }
            .build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        if (playWhenReady) {
            if (requestAudioFocus()) {
                store.play()
            }
        } else {
            store.pause()
        }
        return Futures.immediateVoidFuture()
    }

    override fun handleSeek(
        mediaItemIndex: Int,
        positionMs: Long,
        seekCommand: Int,
    ): ListenableFuture<*> {
        when (seekCommand) {
            Player.COMMAND_SEEK_TO_NEXT,
            Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> {
                requestAudioFocus()
                store.next()
            }
            Player.COMMAND_SEEK_TO_PREVIOUS,
            Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> {
                requestAudioFocus()
                store.prev()
            }
            else -> {
                if (positionMs != C.TIME_UNSET) {
                    store.seekTo(positionMs)
                }
            }
        }
        return Futures.immediateVoidFuture()
    }
}
