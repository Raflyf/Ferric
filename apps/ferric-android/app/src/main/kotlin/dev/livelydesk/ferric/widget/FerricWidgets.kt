package dev.livelydesk.ferric.widget

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.widget.RemoteViews
import coil3.ImageLoader
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import coil3.request.allowHardware
import coil3.toBitmap
import dev.livelydesk.ferric.FerricApp
import dev.livelydesk.ferric.MainActivity
import dev.livelydesk.ferric.R
import dev.livelydesk.ferric.core.PlaybackUiState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

private const val ACTION_WIDGET_PREV = "dev.livelydesk.ferric.widget.ACTION_PREV"
private const val ACTION_WIDGET_PLAY_PAUSE = "dev.livelydesk.ferric.widget.ACTION_PLAY_PAUSE"
private const val ACTION_WIDGET_NEXT = "dev.livelydesk.ferric.widget.ACTION_NEXT"
private const val ACTION_WIDGET_LIKE = "dev.livelydesk.ferric.widget.ACTION_LIKE"

class CompactStripWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        FerricWidgetUpdater.updateAll(context)
    }
}

class SquareCoverWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        FerricWidgetUpdater.updateAll(context)
    }
}

class LiquidGlassWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        FerricWidgetUpdater.updateAll(context)
    }
}

class WidgetActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val app = context.applicationContext as? FerricApp ?: return
        val store = app.store
        when (intent.action) {
            ACTION_WIDGET_PREV -> store.prev()
            ACTION_WIDGET_PLAY_PAUSE -> store.togglePlayPause()
            ACTION_WIDGET_NEXT -> store.next()
            ACTION_WIDGET_LIKE -> {
                val id = store.uiState.value.currentTrack?.id
                if (!id.isNullOrBlank()) {
                    store.toggleLike(id)
                }
            }
        }
    }
}

object FerricWidgetUpdater {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    @Volatile
    private var cachedArtUrl: String? = null

    @Volatile
    private var cachedArtBitmap: Bitmap? = null

    fun updateAll(context: Context, overrideState: PlaybackUiState? = null) {
        val app = context.applicationContext as? FerricApp
        val state = overrideState ?: app?.store?.uiState?.value ?: return
        val manager = AppWidgetManager.getInstance(context)

        val stripIds = manager.getAppWidgetIds(ComponentName(context, CompactStripWidgetProvider::class.java))
        val squareIds = manager.getAppWidgetIds(ComponentName(context, SquareCoverWidgetProvider::class.java))
        val glassIds = manager.getAppWidgetIds(ComponentName(context, LiquidGlassWidgetProvider::class.java))
        if (stripIds.isEmpty() && squareIds.isEmpty() && glassIds.isEmpty()) return

        val artUrl = state.currentTrack?.thumbnailUrl
        val currentBitmap = if (artUrl != null && artUrl == cachedArtUrl) cachedArtBitmap else null
        applyWidgets(context, manager, state, currentBitmap, stripIds, squareIds, glassIds)

        if (!artUrl.isNullOrBlank() && artUrl != cachedArtUrl) {
            scope.launch {
                val bmp = runCatching {
                    val loader = ImageLoader(context.applicationContext)
                    val req = ImageRequest.Builder(context.applicationContext)
                        .data(artUrl)
                        .size(256, 256)
                        .allowHardware(false)
                        .build()
                    val result = loader.execute(req)
                    (result as? SuccessResult)?.image?.toBitmap()
                }.getOrNull()
                if (bmp != null) {
                    cachedArtUrl = artUrl
                    cachedArtBitmap = bmp
                    applyWidgets(context, manager, state, bmp, stripIds, squareIds, glassIds)
                }
            }
        }
    }

    private fun applyWidgets(
        context: Context,
        manager: AppWidgetManager,
        state: PlaybackUiState,
        artBitmap: Bitmap?,
        stripIds: IntArray,
        squareIds: IntArray,
        glassIds: IntArray,
    ) {
        val title = state.currentTrack?.title?.ifBlank { null } ?: "Ready to play"
        val artist = state.currentTrack?.artist?.ifBlank { null } ?: "Ferric"
        val playIcon = if (state.isPlaying) {
            android.R.drawable.ic_media_pause
        } else {
            android.R.drawable.ic_media_play
        }
        val likeIcon = if (state.currentTrack?.liked == true) {
            android.R.drawable.btn_star_big_on
        } else {
            android.R.drawable.btn_star_big_off
        }

        val openAppIntent = PendingIntent.getActivity(
            context,
            100,
            Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        fun actionPi(reqCode: Int, action: String): PendingIntent =
            PendingIntent.getBroadcast(
                context,
                reqCode,
                Intent(context, WidgetActionReceiver::class.java).apply { this.action = action },
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )

        if (stripIds.isNotEmpty()) {
            val views = RemoteViews(context.packageName, R.layout.widget_compact_strip).apply {
                setTextViewText(R.id.widget_title, title)
                setTextViewText(R.id.widget_artist, artist)
                setImageViewResource(R.id.widget_btn_play, playIcon)
                if (artBitmap != null) setImageViewBitmap(R.id.widget_art, artBitmap)
                setOnClickPendingIntent(R.id.widget_root, openAppIntent)
                setOnClickPendingIntent(R.id.widget_btn_prev, actionPi(101, ACTION_WIDGET_PREV))
                setOnClickPendingIntent(R.id.widget_btn_play, actionPi(102, ACTION_WIDGET_PLAY_PAUSE))
                setOnClickPendingIntent(R.id.widget_btn_next, actionPi(103, ACTION_WIDGET_NEXT))
            }
            stripIds.forEach { manager.updateAppWidget(it, views) }
        }

        if (squareIds.isNotEmpty()) {
            val views = RemoteViews(context.packageName, R.layout.widget_square_cover).apply {
                setTextViewText(R.id.widget_title, title)
                setTextViewText(R.id.widget_artist, artist)
                setImageViewResource(R.id.widget_btn_play, playIcon)
                setImageViewResource(R.id.widget_btn_like, likeIcon)
                if (artBitmap != null) setImageViewBitmap(R.id.widget_art, artBitmap)
                setOnClickPendingIntent(R.id.widget_root, openAppIntent)
                setOnClickPendingIntent(R.id.widget_btn_play, actionPi(202, ACTION_WIDGET_PLAY_PAUSE))
                setOnClickPendingIntent(R.id.widget_btn_next, actionPi(203, ACTION_WIDGET_NEXT))
                setOnClickPendingIntent(R.id.widget_btn_like, actionPi(204, ACTION_WIDGET_LIKE))
            }
            squareIds.forEach { manager.updateAppWidget(it, views) }
        }

        if (glassIds.isNotEmpty()) {
            val views = RemoteViews(context.packageName, R.layout.widget_liquid_glass).apply {
                setTextViewText(R.id.widget_title, title)
                setTextViewText(R.id.widget_artist, artist)
                setImageViewResource(R.id.widget_btn_play, playIcon)
                setImageViewResource(R.id.widget_btn_like, likeIcon)
                if (artBitmap != null) setImageViewBitmap(R.id.widget_art, artBitmap)
                setOnClickPendingIntent(R.id.widget_root, openAppIntent)
                setOnClickPendingIntent(R.id.widget_btn_prev, actionPi(301, ACTION_WIDGET_PREV))
                setOnClickPendingIntent(R.id.widget_btn_play, actionPi(302, ACTION_WIDGET_PLAY_PAUSE))
                setOnClickPendingIntent(R.id.widget_btn_next, actionPi(303, ACTION_WIDGET_NEXT))
                setOnClickPendingIntent(R.id.widget_btn_like, actionPi(304, ACTION_WIDGET_LIKE))
            }
            glassIds.forEach { manager.updateAppWidget(it, views) }
        }
    }
}
