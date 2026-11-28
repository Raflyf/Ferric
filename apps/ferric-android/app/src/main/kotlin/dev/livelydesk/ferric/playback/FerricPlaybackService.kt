package dev.livelydesk.ferric.playback

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.annotation.OptIn
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.DefaultMediaNotificationProvider
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import androidx.media3.session.MediaStyleNotificationHelper
import coil3.SingletonImageLoader
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import coil3.request.allowHardware
import coil3.toBitmap
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import dev.livelydesk.ferric.FerricApp
import dev.livelydesk.ferric.MainActivity
import dev.livelydesk.ferric.R
import dev.livelydesk.ferric.core.CoreStateStore
import dev.livelydesk.ferric.core.activeDisplayIsPlaying
import dev.livelydesk.ferric.core.activeDisplayTrack
import java.io.ByteArrayOutputStream
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

/**
 * Process-wide AudioFocus and foreground-service coordinator shared by UI playback buttons,
 * Home Screen widgets, and [FerricPlaybackService] MediaSession3 callbacks so every playback
 * entry point properly owns Android AudioFocus, handles phone-call delayed gain, and keeps
 * the foreground media notification alive.
 */
object PlaybackFocusCoordinator {
    @Volatile
    private var initialized = false

    @Volatile
    private var resumeOnFocusGain = false

    @Volatile
    private var hasFocus = false

    private lateinit var appContext: Context
    private lateinit var audioManager: AudioManager
    private lateinit var focusRequest: AudioFocusRequest
    private lateinit var storeRef: CoreStateStore

    private val focusChangeListener = AudioManager.OnAudioFocusChangeListener { focusChange ->
        when (focusChange) {
            AudioManager.AUDIOFOCUS_LOSS -> {
                hasFocus = false
                resumeOnFocusGain = false
                storeRef.setDucking(false)
                storeRef.pauseInternal()
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                hasFocus = false
                resumeOnFocusGain = storeRef.uiState.value.isPlaying
                storeRef.pauseInternal()
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                storeRef.setDucking(true)
            }
            AudioManager.AUDIOFOCUS_GAIN -> {
                hasFocus = true
                storeRef.setDucking(false)
                if (resumeOnFocusGain) {
                    resumeOnFocusGain = false
                    storeRef.playInternal()
                }
            }
        }
    }

    @Synchronized
    fun ensureInitialized(context: Context, store: CoreStateStore) {
        if (initialized) return
        appContext = context.applicationContext
        storeRef = store
        audioManager = appContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        val attrs = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_MEDIA)
            .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
            .build()
        focusRequest = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(attrs)
            .setAcceptsDelayedFocusGain(true)
            .setOnAudioFocusChangeListener(focusChangeListener)
            .build()
        initialized = true
    }

    /**
     * Request Android media AudioFocus and ensure [FerricPlaybackService] is running.
     * Returns `true` if focus is granted immediately; returns `false` if delayed (sets
     * [resumeOnFocusGain] = true so playback starts automatically once the call/alarm finishes)
     * or failed.
     */
    fun requestFocusForPlayback(startService: Boolean = true): Boolean {
        if (!initialized) return true
        if (startService && !FerricPlaybackService.isForegroundActive) {
            runCatching {
                ContextCompat.startForegroundService(
                    appContext,
                    Intent(appContext, FerricPlaybackService::class.java),
                )
            }
        }
        if (hasFocus) return true
        return when (audioManager.requestAudioFocus(focusRequest)) {
            AudioManager.AUDIOFOCUS_REQUEST_GRANTED -> {
                hasFocus = true
                resumeOnFocusGain = false
                true
            }
            AudioManager.AUDIOFOCUS_REQUEST_DELAYED -> {
                hasFocus = false
                resumeOnFocusGain = true
                false
            }
            else -> {
                hasFocus = false
                resumeOnFocusGain = false
                false
            }
        }
    }

    fun onExplicitPause() {
        resumeOnFocusGain = false
    }

    fun abandonFocus() {
        if (!initialized) return
        hasFocus = false
        resumeOnFocusGain = false
        runCatching { audioManager.abandonAudioFocusRequest(focusRequest) }
    }
}

private data class SessionStateKey(
    val trackId: String?,
    val title: String?,
    val artist: String?,
    val album: String?,
    val thumbnailUrl: String?,
    val isPlaying: Boolean,
    val durationMs: Long,
    val playbackSpeed: Float,
    val seekSyncNonce: Long,
)

/**
 * Foreground Media3 [MediaSessionService] bridging Android media notifications (with cover artwork
 * and click-to-open activity intent), lockscreen transport controls, Bluetooth AVRCP keys, live
 * AudioDevice route hot-plug updates, AudioFocus, and `ACTION_AUDIO_BECOMING_NOISY` directly to
 * Ferric's Rust `oboe-rs` audio engine.
 */
@OptIn(UnstableApi::class)
class FerricPlaybackService : MediaSessionService() {
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private lateinit var store: CoreStateStore
    private lateinit var audioManager: AudioManager
    private var mediaSession: MediaSession? = null
    private var bridgePlayer: RustBridgePlayer? = null
    private var noisyRegistered = false
    private var audioDeviceCallback: AudioDeviceCallback? = null
    private var lastArtUrl: String? = null
    private var cachedArtBytes: ByteArray? = null
    private var foregroundPromoted = false

    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) {
                PlaybackFocusCoordinator.onExplicitPause()
                store.pauseInternal()
                store.refreshOutputRoute()
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        store = (application as FerricApp).store
        PlaybackFocusCoordinator.ensureInitialized(this, store)
        audioManager = getSystemService(AUDIO_SERVICE) as AudioManager
        setMediaNotificationProvider(
            DefaultMediaNotificationProvider.Builder(this)
                .setNotificationId(PLAYBACK_NOTIFICATION_ID)
                .setChannelId(PLAYBACK_CHANNEL_ID)
                .setChannelName(R.string.playback_notification_channel_name)
                .build()
                .apply {
                    setSmallIcon(R.drawable.ic_launcher_monochrome)
                },
        )

        // Lazily initialize Rust Oboe audio inside the foreground MediaSessionService.
        serviceScope.launch(Dispatchers.IO) {
            store.ensureAudioStartedSync()
        }

        val player = RustBridgePlayer(
            looper = Looper.getMainLooper(),
            store = store,
        )
        bridgePlayer = player

        val openAppPi = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        val sessionCallback = object : MediaSession.Callback {
            override fun onConnect(
                session: MediaSession,
                controller: MediaSession.ControllerInfo,
            ): MediaSession.ConnectionResult {
                val playerCommands = MediaSession.ConnectionResult.DEFAULT_PLAYER_COMMANDS
                    .buildUpon()
                    .build()
                return MediaSession.ConnectionResult.AcceptedResultBuilder(session)
                    .setAvailablePlayerCommands(playerCommands)
                    .build()
            }
        }

        mediaSession = MediaSession.Builder(this, player)
            .setSessionActivity(openAppPi)
            .setCallback(sessionCallback)
            .build()
            .also { addSession(it) }
        val initUi = store.uiState.value
        if (initUi.activeDisplayIsPlaying() && !hasActiveForegroundMediaNotification()) {
            promoteToForegroundImmediately()
        }

        ContextCompat.registerReceiver(
            this,
            noisyReceiver,
            IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        noisyRegistered = true

        val devCb = object : AudioDeviceCallback() {
            override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>?) {
                store.refreshOutputRoute()
            }

            override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>?) {
                store.refreshOutputRoute()
            }
        }
        audioDeviceCallback = devCb
        audioManager.registerAudioDeviceCallback(devCb, Handler(Looper.getMainLooper()))

        // Deduplicate MediaSession state updates: only call invalidateState() when track, play
        // state, duration, speed, or explicit seek changes, NOT on every 200ms position tick.
        serviceScope.launch {
            store.uiState
                .map { ui ->
                    val disp = ui.activeDisplayTrack()
                    SessionStateKey(
                        trackId = disp?.id,
                        title = disp?.title,
                        artist = disp?.artist,
                        album = disp?.album,
                        thumbnailUrl = disp?.thumbnailUrl,
                        isPlaying = ui.activeDisplayIsPlaying(),
                        durationMs = if (ui.connectState.isLocalActiveSpeaker) {
                            ui.durationMs
                        } else {
                            disp?.durationMs?.toLong()?.coerceAtLeast(0L) ?: 0L
                        },
                        playbackSpeed = ui.dspSettings?.playbackSpeed ?: 1f,
                        seekSyncNonce = ui.seekSyncNonce,
                    )
                }
                .distinctUntilChanged()
                .collect { key ->
                    val posMs = store.playbackProgress.value.positionMs
                    val artBytes = if (!key.thumbnailUrl.isNullOrBlank() && key.thumbnailUrl == lastArtUrl) {
                        cachedArtBytes
                    } else {
                        null
                    }
                    player.pushSnapshot(
                        key = key,
                        positionMs = posMs,
                        artworkBytes = artBytes,
                    )
                    if (key.trackId != null) {
                        triggerNotificationUpdate()
                    }
                    val artUrl = key.thumbnailUrl
                    if (!artUrl.isNullOrBlank() && artUrl != lastArtUrl) {
                        fetchArtworkForNotification(artUrl, player)
                    } else if (artUrl.isNullOrBlank()) {
                        lastArtUrl = null
                        cachedArtBytes = null
                    }
                }
        }
    }

    private fun fetchArtworkForNotification(url: String, player: RustBridgePlayer) {
        serviceScope.launch(Dispatchers.IO) {
            val bytes = runCatching {
                val loader = SingletonImageLoader.get(applicationContext)
                val req = ImageRequest.Builder(applicationContext)
                    .data(url)
                    .size(512, 512)
                    .allowHardware(false)
                    .build()
                val bmp = (loader.execute(req) as? SuccessResult)?.image?.toBitmap() ?: return@runCatching null
                ByteArrayOutputStream().use { out ->
                    bmp.compress(Bitmap.CompressFormat.JPEG, 88, out)
                    out.toByteArray()
                }
            }.getOrNull()
            if (bytes != null) {
                serviceScope.launch(Dispatchers.Main.immediate) {
                    val current = store.uiState.value
                    val disp = current.activeDisplayTrack()
                    if (disp?.thumbnailUrl == url) {
                        lastArtUrl = url
                        cachedArtBytes = bytes
                        val key = SessionStateKey(
                            trackId = disp.id,
                            title = disp.title,
                            artist = disp.artist,
                            album = disp.album,
                            thumbnailUrl = disp.thumbnailUrl,
                            isPlaying = current.activeDisplayIsPlaying(),
                            durationMs = if (current.connectState.isLocalActiveSpeaker) {
                                current.durationMs
                            } else {
                                disp.durationMs.toLong().coerceAtLeast(0L)
                            },
                            playbackSpeed = current.dspSettings?.playbackSpeed ?: 1f,
                            seekSyncNonce = current.seekSyncNonce,
                        )
                        player.pushSnapshot(
                            key = key,
                            positionMs = store.playbackProgress.value.positionMs,
                            artworkBytes = bytes,
                        )
                        triggerNotificationUpdate()
                    }
                }
            }
        }
    }

    private fun hasActiveForegroundMediaNotification(): Boolean = runCatching {
        val nm = getSystemService(NotificationManager::class.java) ?: return false
        nm.activeNotifications.any {
            it.id == PLAYBACK_NOTIFICATION_ID &&
                (it.notification.flags and Notification.FLAG_FOREGROUND_SERVICE) != 0
        }
    }.getOrDefault(false)

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!hasActiveForegroundMediaNotification()) {
            promoteToForegroundImmediately()
        }
        // Do not call triggerNotificationUpdate() here: Media3's MediaNotificationManager invokes
        // startForegroundService(this) inside onUpdateNotification() while playing, so calling
        // triggerNotificationUpdate() in onStartCommand() creates a 250Hz self-retrigger loop.
        return super.onStartCommand(intent, flags, startId)
    }

    override fun onUpdateNotification(session: MediaSession, startInForegroundRequired: Boolean) {
        foregroundPromoted = true
        isForegroundActive = true
        super.onUpdateNotification(session, startInForegroundRequired)
    }

    private fun ensurePlaybackChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = getSystemService(NotificationManager::class.java) ?: return
        if (nm.getNotificationChannel(PLAYBACK_CHANNEL_ID) != null) return
        val channel = NotificationChannel(
            PLAYBACK_CHANNEL_ID,
            getString(R.string.playback_notification_channel_name),
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            setShowBadge(false)
            description = "Ferric media playback controls"
        }
        nm.createNotificationChannel(channel)
    }

    private fun promoteToForegroundImmediately() {
        ensurePlaybackChannel()
        val openAppPi = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val snap = if (::store.isInitialized) store.uiState.value else null
        val disp = snap?.activeDisplayTrack()
        val title = disp?.title?.ifBlank { null } ?: "Ferric"
        val artist = disp?.artist?.ifBlank { null } ?: "Preparing playback"
        val builder = NotificationCompat.Builder(this, PLAYBACK_CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setContentTitle(title)
            .setContentText(artist)
            .setContentIntent(openAppPi)
            .setOngoing(true)
            .setSilent(true)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_TRANSPORT)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
        mediaSession?.let { session ->
            builder.setStyle(MediaStyleNotificationHelper.MediaStyle(session))
        }
        val notification = builder.build()
        val fgType = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK
        } else {
            0
        }
        runCatching {
            ServiceCompat.startForeground(this, PLAYBACK_NOTIFICATION_ID, notification, fgType)
            foregroundPromoted = true
            isForegroundActive = true
        }
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? {
        return mediaSession
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        val st = store.uiState.value
        val activeLocalPlayback = st.isPlaying && st.connectState.isLocalActiveSpeaker
        if (activeLocalPlayback) {
            return
        }
        dev.livelydesk.ferric.widget.FerricWidgetUpdater.updateAll(
            applicationContext,
            st.copy(isPlaying = false),
        )
        foregroundPromoted = false
        isForegroundActive = false
        mediaSession?.let { session ->
            removeSession(session)
            session.release()
        }
        mediaSession = null
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        stopSelf()
        store.shutdown()
    }

    companion object {
        private const val PLAYBACK_CHANNEL_ID = "ferric_playback"
        private const val PLAYBACK_NOTIFICATION_ID = 1001

        @Volatile
        internal var isForegroundActive: Boolean = false
    }

    override fun onDestroy() {
        foregroundPromoted = false
        isForegroundActive = false
        audioDeviceCallback?.let { cb ->
            runCatching { audioManager.unregisterAudioDeviceCallback(cb) }
        }
        audioDeviceCallback = null
        if (noisyRegistered) {
            unregisterReceiver(noisyReceiver)
            noisyRegistered = false
        }
        PlaybackFocusCoordinator.abandonFocus()
        mediaSession?.release()
        mediaSession = null
        bridgePlayer = null
        serviceScope.cancel()
        super.onDestroy()
    }
}

@OptIn(UnstableApi::class)
private class RustBridgePlayer(
    looper: Looper,
    private val store: CoreStateStore,
) : SimpleBasePlayer(looper) {
    private var key: SessionStateKey = SessionStateKey(
        trackId = null,
        title = null,
        artist = null,
        album = null,
        thumbnailUrl = null,
        isPlaying = false,
        durationMs = 0L,
        playbackSpeed = 1f,
        seekSyncNonce = 0L,
    )
    private var anchorPositionMs: Long = 0L
    private var anchorElapsedRealtimeMs: Long = SystemClock.elapsedRealtime()
    private var artworkBytes: ByteArray? = null

    fun pushSnapshot(key: SessionStateKey, positionMs: Long, artworkBytes: ByteArray?) {
        this.key = key
        this.anchorPositionMs = positionMs.coerceAtLeast(0L)
        this.anchorElapsedRealtimeMs = SystemClock.elapsedRealtime()
        this.artworkBytes = artworkBytes
        invalidateState()
    }

    override fun getState(): State {
        val id = key.trackId
        val items = if (id != null) {
            val metaBuilder = MediaMetadata.Builder()
                .setTitle(key.title.orEmpty())
                .setArtist(key.artist.orEmpty())
                .setAlbumTitle(key.album.orEmpty())
            val artUri = key.thumbnailUrl?.takeIf { it.isNotBlank() }?.let { runCatching { Uri.parse(it) }.getOrNull() }
            if (artUri != null) {
                metaBuilder.setArtworkUri(artUri)
            }
            artworkBytes?.let { bytes ->
                metaBuilder.setArtworkData(bytes, MediaMetadata.PICTURE_TYPE_FRONT_COVER)
            }
            val mediaItem = MediaItem.Builder()
                .setMediaId(id)
                .setMediaMetadata(metaBuilder.build())
                .build()
            listOf(
                MediaItemData.Builder(id)
                    .setMediaItem(mediaItem)
                    .setIsSeekable(key.durationMs > 0L)
                    .setDurationUs(
                        if (key.durationMs > 0L) key.durationMs * 1_000L else C.TIME_UNSET,
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

        val playbackState = if (items.isEmpty()) Player.STATE_IDLE else Player.STATE_READY
        val speed = if (key.isPlaying) key.playbackSpeed.coerceIn(0.5f, 2.0f) else 0f
        val posSupplier = PositionSupplier {
            if (speed > 0f) {
                val delta = ((SystemClock.elapsedRealtime() - anchorElapsedRealtimeMs) * speed).toLong()
                val raw = anchorPositionMs + delta
                if (key.durationMs > 0L) raw.coerceIn(0L, key.durationMs) else raw.coerceAtLeast(0L)
            } else {
                anchorPositionMs
            }
        }

        return State.Builder()
            .setAvailableCommands(commands)
            .setPlayWhenReady(key.isPlaying, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
            .setPlaybackState(playbackState)
            .setPlaylist(items)
            .setCurrentMediaItemIndex(if (items.isEmpty()) C.INDEX_UNSET else 0)
            .setContentPositionMs(posSupplier)
            .build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        if (playWhenReady) {
            store.play()
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
            Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> store.next()
            Player.COMMAND_SEEK_TO_PREVIOUS,
            Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> store.prev()
            else -> {
                if (positionMs != C.TIME_UNSET) {
                    store.seekTo(positionMs)
                }
            }
        }
        return Futures.immediateVoidFuture()
    }
}
