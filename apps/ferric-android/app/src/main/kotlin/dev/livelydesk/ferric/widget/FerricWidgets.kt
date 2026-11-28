package dev.livelydesk.ferric.widget

import android.app.PendingIntent
import android.app.WallpaperManager
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.Shader
import android.graphics.Typeface
import android.os.Build
import android.os.Bundle
import android.util.TypedValue
import android.view.View
import android.widget.RemoteViews
import androidx.compose.ui.graphics.toArgb
import androidx.core.content.ContextCompat
import androidx.core.graphics.createBitmap
import androidx.core.graphics.withSave
import coil3.SingletonImageLoader
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import coil3.request.allowHardware
import coil3.toBitmap
import dev.livelydesk.ferric.FerricApp
import dev.livelydesk.ferric.MainActivity
import dev.livelydesk.ferric.R
import dev.livelydesk.ferric.core.PlaybackProgress
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.core.activeDisplayIsPlaying
import dev.livelydesk.ferric.core.activeDisplayTrack
import dev.livelydesk.ferric.formatDurationMs
import dev.livelydesk.ferric.playback.FerricPlaybackService
import dev.livelydesk.ferric.ui.theme.CoverShadeColors
import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sqrt
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

private const val ACTION_WIDGET_PREV = "dev.livelydesk.ferric.widget.ACTION_PREV"
private const val ACTION_WIDGET_PLAY_PAUSE = "dev.livelydesk.ferric.widget.ACTION_PLAY_PAUSE"
private const val ACTION_WIDGET_NEXT = "dev.livelydesk.ferric.widget.ACTION_NEXT"
private const val ACTION_WIDGET_LIKE = "dev.livelydesk.ferric.widget.ACTION_LIKE"

enum class FerricWidgetStyle(val id: String, val label: String) {
    GLASS("glass", "Glass"),
    CLEAR_GLASS("clear_glass", "Clear glass"),
    DARK("dark", "Dark"),
    LIGHT("light", "Light"),
    COVER_COLOR("cover_color", "Cover colour"),
    ;

    companion object {
        fun fromId(raw: String?): FerricWidgetStyle =
            entries.firstOrNull { it.id == raw } ?: GLASS
    }
}

enum class WidgetCoverShape(val id: String, val label: String) {
    ROUNDED("rounded", "Rounded"),
    CIRCLE("circle", "Circle"),
    SQUARE("square", "Square"),
    ;

    companion object {
        fun fromId(raw: String?): WidgetCoverShape =
            entries.firstOrNull { it.id == raw } ?: ROUNDED
    }
}

enum class WidgetTextSize(val id: String, val label: String, val titleSp: Float, val artistSp: Float) {
    SMALL("small", "Small", 14.5f, 12f),
    MEDIUM("medium", "Medium", 16.5f, 13f),
    LARGE("large", "Large", 18.5f, 14f),
    ;

    companion object {
        fun fromId(raw: String?): WidgetTextSize =
            entries.firstOrNull { it.id == raw } ?: LARGE
    }
}

enum class WidgetPlayButtonStyle(val id: String, val label: String) {
    ROUND("round", "Round"),
    PLAIN("plain", "Plain"),
    ;

    companion object {
        fun fromId(raw: String?): WidgetPlayButtonStyle =
            entries.firstOrNull { it.id == raw } ?: PLAIN
    }
}

data class FerricWidgetConfig(
    val style: FerricWidgetStyle = FerricWidgetStyle.GLASS,
    val accentSwatchIndex: Int = -1, // -1 = From cover; 0..7 = preset swatches
    val glassShine: Float = 1.0f,
    val glassThickness: Float = 0.15f,
    val cornerRoundness: Float = 0.59f,
    val frost: Float = 0.15f,
    val coverShape: WidgetCoverShape = WidgetCoverShape.ROUNDED,
    val textSize: WidgetTextSize = WidgetTextSize.LARGE,
    val playButtonStyle: WidgetPlayButtonStyle = WidgetPlayButtonStyle.PLAIN,
    val showCover: Boolean = true,
    val showProgressBar: Boolean = true,
    val showTime: Boolean = true,
    val showLikeButton: Boolean = false,
    val showPrevNext: Boolean = true,
    val showStreak: Boolean = false,
) {
    companion object {
        val ACCENT_SWATCHES = intArrayOf(
            0xFF7B61FF.toInt(), // Violet
            0xFF2979FF.toInt(), // Blue
            0xFF00BFA5.toInt(), // Teal
            0xFF22C55E.toInt(), // Green
            0xFFFFB300.toInt(), // Amber
            0xFFE86A2C.toInt(), // Ferric Orange / Coral
            0xFFEC4899.toInt(), // Pink
            0xFFEF4444.toInt(), // Red
        )
    }
}

object FerricWidgetPrefs {
    private const val PREFS_NAME = "ferric_widget_prefs"

    fun load(context: Context, appWidgetId: Int): FerricWidgetConfig {
        val sp = context.applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        val hasWidget = appWidgetId > 0 && sp.contains("w_${appWidgetId}_style")
        val p = if (hasWidget) "w_${appWidgetId}_" else "default_"
        return FerricWidgetConfig(
            style = FerricWidgetStyle.fromId(sp.getString("${p}style", null)),
            accentSwatchIndex = sp.getInt("${p}accent", -1).coerceIn(-1, FerricWidgetConfig.ACCENT_SWATCHES.lastIndex),
            glassShine = sp.getFloat("${p}shine", 1.0f).coerceIn(0f, 1f),
            glassThickness = sp.getFloat("${p}thick", 0.15f).coerceIn(0f, 1f),
            cornerRoundness = sp.getFloat("${p}corner", 0.59f).coerceIn(0f, 1f),
            frost = sp.getFloat("${p}frost", 0.15f).coerceIn(0f, 1f),
            coverShape = WidgetCoverShape.fromId(sp.getString("${p}shape", null)),
            textSize = WidgetTextSize.fromId(sp.getString("${p}text_sz", null)),
            playButtonStyle = WidgetPlayButtonStyle.fromId(sp.getString("${p}play_style", null)),
            showCover = sp.getBoolean("${p}show_cover", true),
            showProgressBar = sp.getBoolean("${p}show_prog", true),
            showTime = sp.getBoolean("${p}show_time", true),
            showLikeButton = sp.getBoolean("${p}show_like", false),
            showPrevNext = sp.getBoolean("${p}show_prev_next", true),
            showStreak = sp.getBoolean("${p}show_streak", false),
        )
    }

    fun save(context: Context, appWidgetId: Int, cfg: FerricWidgetConfig) {
        val sp = context.applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        sp.edit().apply {
            val prefixes = if (appWidgetId > 0) listOf("w_${appWidgetId}_", "default_") else listOf("default_")
            for (p in prefixes) {
                putString("${p}style", cfg.style.id)
                putInt("${p}accent", cfg.accentSwatchIndex)
                putFloat("${p}shine", cfg.glassShine)
                putFloat("${p}thick", cfg.glassThickness)
                putFloat("${p}corner", cfg.cornerRoundness)
                putFloat("${p}frost", cfg.frost)
                putString("${p}shape", cfg.coverShape.id)
                putString("${p}text_sz", cfg.textSize.id)
                putString("${p}play_style", cfg.playButtonStyle.id)
                putBoolean("${p}show_cover", cfg.showCover)
                putBoolean("${p}show_prog", cfg.showProgressBar)
                putBoolean("${p}show_time", cfg.showTime)
                putBoolean("${p}show_like", cfg.showLikeButton)
                putBoolean("${p}show_prev_next", cfg.showPrevNext)
                putBoolean("${p}show_streak", cfg.showStreak)
            }
        }.apply()
    }

    fun deleteForIds(context: Context, appWidgetIds: IntArray) {
        if (appWidgetIds.isEmpty()) return
        val sp = context.applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        val keys = listOf(
            "style", "accent", "shine", "thick", "corner", "frost", "shape",
            "text_sz", "play_style", "show_cover", "show_prog", "show_time",
            "show_like", "show_prev_next", "show_streak",
        )
        sp.edit().apply {
            for (id in appWidgetIds) {
                val p = "w_${id}_"
                keys.forEach { remove("$p$it") }
            }
        }.apply()
    }

    fun saveLastKnownStreak(context: Context, days: Int) {
        if (days <= 0) return
        context.applicationContext
            .getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            .edit()
            .putInt("last_known_streak_days", days)
            .apply()
    }

    fun loadLastKnownStreak(context: Context): Int =
        context.applicationContext
            .getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            .getInt("last_known_streak_days", 0)
}

class CompactStripWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        FerricWidgetUpdater.updateAll(context)
    }

    override fun onAppWidgetOptionsChanged(context: Context, manager: AppWidgetManager, widgetId: Int, opts: Bundle) {
        FerricWidgetUpdater.updateAll(context)
    }

    override fun onDeleted(context: Context, appWidgetIds: IntArray) {
        FerricWidgetPrefs.deleteForIds(context, appWidgetIds)
    }
}

class SquareCoverWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        FerricWidgetUpdater.updateAll(context)
    }

    override fun onAppWidgetOptionsChanged(context: Context, manager: AppWidgetManager, widgetId: Int, opts: Bundle) {
        FerricWidgetUpdater.updateAll(context)
    }

    override fun onDeleted(context: Context, appWidgetIds: IntArray) {
        FerricWidgetPrefs.deleteForIds(context, appWidgetIds)
    }
}

class LiquidGlassWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        FerricWidgetUpdater.updateAll(context)
    }

    override fun onAppWidgetOptionsChanged(context: Context, manager: AppWidgetManager, widgetId: Int, opts: Bundle) {
        FerricWidgetUpdater.updateAll(context)
    }

    override fun onDeleted(context: Context, appWidgetIds: IntArray) {
        FerricWidgetPrefs.deleteForIds(context, appWidgetIds)
    }
}

class WidgetActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val app = context.applicationContext as? FerricApp ?: return
        val store = app.store
        val pendingResult = goAsync()
        FerricWidgetUpdater.runWidgetAction {
            try {
                when (intent.action) {
                    ACTION_WIDGET_PREV -> {
                        startPlaybackService(context)
                        store.prev()
                    }
                    ACTION_WIDGET_PLAY_PAUSE -> {
                        startPlaybackService(context)
                        store.togglePlayPause()
                    }
                    ACTION_WIDGET_NEXT -> {
                        startPlaybackService(context)
                        store.next()
                    }
                    ACTION_WIDGET_LIKE -> {
                        val id = store.uiState.value.activeDisplayTrack()?.id
                        if (!id.isNullOrBlank()) {
                            store.toggleLike(id)
                        }
                    }
                }
            } finally {
                pendingResult.finish()
            }
        }
    }

    private fun startPlaybackService(context: Context) {
        runCatching {
            ContextCompat.startForegroundService(
                context.applicationContext,
                Intent(context.applicationContext, FerricPlaybackService::class.java),
            )
        }
    }
}

object FerricWidgetRenderer {
    fun resolveAccentArgb(config: FerricWidgetConfig, coverShade: CoverShadeColors): Int =
        if (config.accentSwatchIndex in FerricWidgetConfig.ACCENT_SWATCHES.indices) {
            FerricWidgetConfig.ACCENT_SWATCHES[config.accentSwatchIndex]
        } else {
            coverShade.accent.toArgb()
        }

    fun renderGlassBackgroundBitmap(
        widthPx: Int,
        heightPx: Int,
        config: FerricWidgetConfig,
        artBitmap: Bitmap?,
        coverShade: CoverShadeColors,
        pxPerDp: Float = 1.6f,
        wallpaperTintArgb: Int? = null,
    ): Bitmap {
        val rawW = max(24, widthPx)
        val rawH = max(24, heightPx)
        val maxSide = max(rawW, rawH).toFloat()
        val downscale = min(1.0f, 640f / maxSide)
        val w = max(24, (rawW * downscale).roundToInt())
        val h = max(24, (rawH * downscale).roundToInt())
        val scale = (pxPerDp * downscale).coerceAtLeast(0.85f)

        val inset = 1.5f
        val usableMin = (min(w, h) - inset * 2f).coerceAtLeast(8f)
        val cornerPx = ((4f + config.cornerRoundness.coerceIn(0f, 1f) * 36f) * scale)
            .coerceIn(4f * scale, usableMin * 0.5f)
        val shine = config.glassShine.coerceIn(0f, 1f)
        val thickness = config.glassThickness.coerceIn(0f, 1f)
        val frost = config.frost.coerceIn(0f, 1f)

        if (config.style == FerricWidgetStyle.DARK || config.style == FerricWidgetStyle.LIGHT) {
            return renderSolidSurfaceBitmap(w, h, cornerPx, shine, thickness, isLight = config.style == FerricWidgetStyle.LIGHT)
        }

        val isClear = config.style == FerricWidgetStyle.CLEAR_GLASS
        var sharpPixels: IntArray? = null
        var frostedPixels: IntArray? = null

        if (!isClear) {
            val basePixels = when {
                config.style == FerricWidgetStyle.COVER_COLOR ->
                    buildAmbientScenePixels(w, h, coverShade.accent.toArgb())
                artBitmap != null ->
                    cropBitmapToIntArray(artBitmap, w, h)
                else ->
                    buildAmbientScenePixels(w, h, Color.rgb(42, 47, 58))
            }
            sharpPixels = boxBlur3Pass(basePixels, w, h, 1.2f * scale)
            frostedPixels = boxBlur3Pass(basePixels, w, h, (1.5f + frost * 12.5f) * scale)
        }

        val refractionHeight = if (isClear || thickness < 0.02f) {
            0f
        } else {
            ((0.14f * thickness + 0.08f) * usableMin)
                .coerceIn(5f * scale, 30f * scale)
                .coerceAtMost(usableMin * 0.5f)
        }
        val refractionAmount = ((thickness * 2.4f + 0.8f) * refractionHeight)
            .coerceAtMost(usableMin * 0.4f)

        val veils = if (isClear) {
            // Smoked translucent crystal panel tuned like the in-app Home pill:
            // dark crystal anchor + sampled wallpaper tone + cool white frost veil so content pops over any wallpaper.
            val wp = wallpaperTintArgb ?: Color.rgb(22, 30, 44)
            val wpR = (Color.red(wp) * 0.55f + 16f).toInt().coerceIn(12, 130)
            val wpG = (Color.green(wp) * 0.55f + 20f).toInt().coerceIn(14, 135)
            val wpB = (Color.blue(wp) * 0.55f + 30f).toInt().coerceIn(18, 150)
            val smokeAlpha = (0.24f + frost * 0.34f).coerceIn(0.24f, 0.58f)
            val frostMist = (0.06f + frost * 0.16f).coerceIn(0.06f, 0.22f)
            intArrayOf(
                floatArgb(smokeAlpha, wpR, wpG, wpB),
                floatArgb(frostMist, 244, 248, 255),
            )
        } else {
            val avgLuma = averageLuma(frostedPixels)
            intArrayOf(
                floatArgb(0.10f + avgLuma.coerceIn(0f, 1f) * 0.32f, 0, 0, 0),
                floatArgb(0.05f, 255, 255, 255),
            )
        }

        val highlightAlpha = if (isClear) shine * 0.72f + 0.24f else shine * 0.62f + 0.18f
        val highlightWidth = if (isClear) (thickness * 0.9f + 1.25f) * scale else (thickness * 0.7f + 1.1f) * scale
        val glowAlpha = if (isClear) (shine * 0.65f + 0.35f) * (0.14f + 0.18f * thickness) else (shine * 0.7f + 0.3f) * (0.03f + 0.06f * thickness)
        val glowWidth = if (refractionHeight > 0f) refractionHeight else (thickness * 12f + 4f) * scale

        val outPixels = renderOpticalGlassPixels(
            frostedPixels = frostedPixels,
            sharpPixels = sharpPixels,
            w = w,
            h = h,
            inset = inset,
            radius = cornerPx,
            refractionHeight = refractionHeight,
            refractionAmount = refractionAmount,
            saturation = 1.5f,
            veils = veils,
            highlightAlpha = highlightAlpha,
            highlightWidth = highlightWidth,
            glowAlpha = glowAlpha,
            glowWidth = glowWidth,
            homePillCrystal = isClear,
            shine = shine,
        )

        val out = createBitmap(w, h, Bitmap.Config.ARGB_8888)
        out.setPixels(outPixels, 0, w, 0, 0, w, h)
        return out
    }

    private fun renderSolidSurfaceBitmap(
        w: Int,
        h: Int,
        cornerPx: Float,
        shine: Float,
        thickness: Float,
        isLight: Boolean,
    ): Bitmap {
        val out = createBitmap(w, h, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(out)
        val rect = RectF(0f, 0f, w.toFloat(), h.toFloat())
        val clipPath = Path().apply { addRoundRect(rect, cornerPx, cornerPx, Path.Direction.CW) }
        canvas.withSave {
            clipPath(clipPath)
            drawRect(
                rect,
                Paint().apply {
                    color = if (isLight) Color.rgb(244, 241, 248) else Color.rgb(20, 22, 26)
                },
            )
            if (shine > 0.01f) {
                val topAlpha = ((if (isLight) 140 else 44) * shine).roundToInt().coerceIn(0, 180)
                drawRect(
                    RectF(0f, 0f, w.toFloat(), h * 0.5f),
                    Paint().apply {
                        shader = LinearGradient(
                            0f,
                            0f,
                            0f,
                            h * 0.5f,
                            Color.argb(topAlpha, 255, 255, 255),
                            Color.TRANSPARENT,
                            Shader.TileMode.CLAMP,
                        )
                    },
                )
            }
        }
        val strokeW = (1.1f + thickness * 1.4f).coerceAtLeast(1f)
        val half = strokeW * 0.5f
        val hiA = (if (isLight) 96 else 110 + (shine * 70f).roundToInt()).coerceIn(32, 210)
        val loA = (if (isLight) 36 else 30 + (shine * 36f).roundToInt()).coerceIn(16, 100)
        canvas.drawRoundRect(
            RectF(half, half, w - half, h - half),
            (cornerPx - half).coerceAtLeast(1f),
            (cornerPx - half).coerceAtLeast(1f),
            Paint(Paint.ANTI_ALIAS_FLAG).apply {
                style = Paint.Style.STROKE
                strokeWidth = strokeW
                shader = LinearGradient(
                    0f,
                    0f,
                    w.toFloat(),
                    h.toFloat(),
                    intArrayOf(
                        Color.argb(hiA, if (isLight) 0 else 255, if (isLight) 0 else 255, if (isLight) 0 else 255),
                        Color.argb(loA, if (isLight) 0 else 255, if (isLight) 0 else 255, if (isLight) 0 else 255),
                    ),
                    null,
                    Shader.TileMode.CLAMP,
                )
            },
        )
        return out
    }

    private fun floatArgb(alpha: Float, r: Int, g: Int, b: Int): Int {
        val a = (alpha * 255f + 0.5f).toInt().coerceIn(0, 255)
        return (a shl 24) or (r shl 16) or (g shl 8) or b
    }

    private fun cropBitmapToIntArray(src: Bitmap, outW: Int, outH: Int): IntArray {
        val srcAspect = src.width.toFloat() / max(1, src.height).toFloat()
        val dstAspect = outW.toFloat() / max(1, outH).toFloat()
        val srcRect = if (srcAspect > dstAspect) {
            val cropW = (src.height * dstAspect).roundToInt().coerceIn(1, src.width)
            val left = (src.width - cropW) / 2
            Rect(left, 0, left + cropW, src.height)
        } else {
            val cropH = (src.width / dstAspect).roundToInt().coerceIn(1, src.height)
            val top = (src.height - cropH) / 2
            Rect(0, top, src.width, top + cropH)
        }
        val scaled = createBitmap(outW, outH, Bitmap.Config.ARGB_8888)
        Canvas(scaled).drawBitmap(
            src,
            srcRect,
            RectF(0f, 0f, outW.toFloat(), outH.toFloat()),
            Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG),
        )
        val pixels = IntArray(outW * outH)
        scaled.getPixels(pixels, 0, outW, 0, 0, outW, outH)
        scaled.recycle()
        return pixels
    }

    private fun buildAmbientScenePixels(w: Int, h: Int, tintArgb: Int): IntArray {
        val baseR = Color.red(tintArgb).toFloat()
        val baseG = Color.green(tintArgb).toFloat()
        val baseB = Color.blue(tintArgb).toFloat()
        val maxSide = max(w, h).toFloat().coerceAtLeast(1f)
        val wf = w.toFloat()
        val hf = h.toFloat()
        val out = IntArray(w * h)
        var idx = 0
        for (y in 0 until h) {
            val ny = y / hf
            val shade = 1.28f - 0.62f * ny
            val rBase = baseR * shade
            val gBase = baseG * shade
            val bBase = baseB * shade
            for (x in 0 until w) {
                val nx = x / wf
                val d1 = hypot((nx - 0.18f) * wf, (ny - 0.22f) * hf)
                val w1 = (1f - d1 / (maxSide * 0.55f)).coerceIn(0f, 1f)
                val w1Sq = w1 * w1 * 0.45f
                val orbR = rBase + (255f - rBase) * w1Sq
                val orbG = gBase + (255f - gBase) * w1Sq
                val orbB = bBase + (255f - bBase) * w1Sq

                val d2 = hypot((nx - 0.85f) * wf, (ny - 0.85f) * hf)
                val w2 = (1f - d2 / (maxSide * 0.50f)).coerceIn(0f, 1f)
                val w2Sq = w2 * w2 * 0.55f
                val finR = (orbR + (baseR - orbR) * w2Sq).toInt().coerceIn(0, 255)
                val finG = (orbG + (baseG - orbG) * w2Sq).toInt().coerceIn(0, 255)
                val finB = (orbB + (baseB - orbB) * w2Sq).toInt().coerceIn(0, 255)
                out[idx++] = (0xFF shl 24) or (finR shl 16) or (finG shl 8) or finB
            }
        }
        return out
    }

    private fun boxBlur3Pass(src: IntArray, w: Int, h: Int, sigma: Float): IntArray {
        val r = (0.6f * sigma).toInt()
        if (r < 1) return src.copyOf()
        val n = w * h
        val rCh = FloatArray(n)
        val gCh = FloatArray(n)
        val bCh = FloatArray(n)
        for (i in 0 until n) {
            val c = src[i]
            rCh[i] = ((c ushr 16) and 0xFF).toFloat()
            gCh[i] = ((c ushr 8) and 0xFF).toFloat()
            bCh[i] = (c and 0xFF).toFloat()
        }
        val window = (r * 2 + 1).toFloat()
        val lineBuf = FloatArray(max(w, h))
        val channels = arrayOf(rCh, gCh, bCh)
        for (ch in channels) {
            repeat(3) {
                for (y in 0 until h) {
                    val row = y * w
                    var sum = 0f
                    for (k in -r..r) {
                        sum += ch[row + k.coerceIn(0, w - 1)]
                    }
                    for (x in 0 until w) {
                        lineBuf[x] = sum / window
                        val addIdx = (x + r + 1).coerceIn(0, w - 1)
                        val subIdx = (x - r).coerceIn(0, w - 1)
                        sum += ch[row + addIdx] - ch[row + subIdx]
                    }
                    System.arraycopy(lineBuf, 0, ch, row, w)
                }
                for (x in 0 until w) {
                    var sum = 0f
                    for (k in -r..r) {
                        sum += ch[k.coerceIn(0, h - 1) * w + x]
                    }
                    for (y in 0 until h) {
                        lineBuf[y] = sum / window
                        val addY = (y + r + 1).coerceIn(0, h - 1)
                        val subY = (y - r).coerceIn(0, h - 1)
                        sum += ch[addY * w + x] - ch[subY * w + x]
                    }
                    for (y in 0 until h) {
                        ch[y * w + x] = lineBuf[y]
                    }
                }
            }
        }
        val out = IntArray(n)
        for (i in 0 until n) {
            val rr = rCh[i].roundToInt().coerceIn(0, 255)
            val gg = gCh[i].roundToInt().coerceIn(0, 255)
            val bb = bCh[i].roundToInt().coerceIn(0, 255)
            out[i] = (0xFF shl 24) or (rr shl 16) or (gg shl 8) or bb
        }
        return out
    }

    private fun averageLuma(pixels: IntArray?): Float {
        if (pixels == null || pixels.isEmpty()) return 0f
        var sum = 0.0
        val step = max(1, pixels.size / 1024)
        var count = 0
        var i = 0
        while (i < pixels.size) {
            val c = pixels[i]
            val r = ((c ushr 16) and 0xFF) / 255f
            val g = ((c ushr 8) and 0xFF) / 255f
            val b = (c and 0xFF) / 255f
            sum += 0.213f * r + 0.715f * g + 0.072f * b
            count++
            i += step
        }
        return if (count > 0) (sum / count).toFloat() else 0f
    }

    private fun sampleBilinear(pixels: IntArray, w: Int, h: Int, sx: Float, sy: Float): Int {
        val fx = sx - 0.5f
        val fy = sy - 0.5f
        val ix = kotlin.math.floor(fx).toInt()
        val iy = kotlin.math.floor(fy).toInt()
        val tx = fx - ix
        val ty = fy - iy
        val maxX = w - 1
        val maxY = h - 1
        val x0 = ix.coerceIn(0, maxX)
        val x1 = (ix + 1).coerceIn(0, maxX)
        val y0 = iy.coerceIn(0, maxY) * w
        val y1 = (iy + 1).coerceIn(0, maxY) * w

        val c00 = pixels[y0 + x0]
        val c10 = pixels[y0 + x1]
        val c01 = pixels[y1 + x0]
        val c11 = pixels[y1 + x1]

        val itx = 1f - tx
        val ity = 1f - ty
        val r = (itx * ((c00 ushr 16) and 0xFF) + tx * ((c10 ushr 16) and 0xFF)) * ity +
            (itx * ((c01 ushr 16) and 0xFF) + tx * ((c11 ushr 16) and 0xFF)) * ty
        val g = (itx * ((c00 ushr 8) and 0xFF) + tx * ((c10 ushr 8) and 0xFF)) * ity +
            (itx * ((c01 ushr 8) and 0xFF) + tx * ((c11 ushr 8) and 0xFF)) * ty
        val b = (itx * (c00 and 0xFF) + tx * (c10 and 0xFF)) * ity +
            (itx * (c01 and 0xFF) + tx * (c11 and 0xFF)) * ty

        return (0xFF shl 24) or
            ((r + 0.5f).toInt().coerceIn(0, 255) shl 16) or
            ((g + 0.5f).toInt().coerceIn(0, 255) shl 8) or
            (b + 0.5f).toInt().coerceIn(0, 255)
    }

    private fun gradSdRoundedRect(
        px: Float,
        py: Float,
        halfW: Float,
        halfH: Float,
        cornerR: Float,
        outGrad: FloatArray,
    ) {
        val sx = if (px >= 0f) 1f else -1f
        val sy = if (py >= 0f) 1f else -1f
        val qx = abs(px) - (halfW - cornerR)
        val qy = abs(py) - (halfH - cornerR)
        if (qx > 0f && qy > 0f) {
            val len = hypot(qx, qy)
            if (len > 1e-4f) {
                outGrad[0] = (qx / len) * sx
                outGrad[1] = (qy / len) * sy
            } else {
                outGrad[0] = sx * 0.707107f
                outGrad[1] = sy * 0.707107f
            }
        } else if (qx > qy) {
            outGrad[0] = sx
            outGrad[1] = 0f
        } else {
            outGrad[0] = 0f
            outGrad[1] = sy
        }
    }

    private fun renderOpticalGlassPixels(
        frostedPixels: IntArray?,
        sharpPixels: IntArray?,
        w: Int,
        h: Int,
        inset: Float,
        radius: Float,
        refractionHeight: Float,
        refractionAmount: Float,
        saturation: Float,
        veils: IntArray,
        highlightAlpha: Float,
        highlightWidth: Float,
        glowAlpha: Float,
        glowWidth: Float,
        homePillCrystal: Boolean = false,
        shine: Float = 1.0f,
    ): IntArray {
        val out = IntArray(w * h)
        val innerW = w.toFloat() - 2f * inset
        val innerH = h.toFloat() - 2f * inset
        if (innerW < 2f || innerH < 2f) return out

        val halfW = w.toFloat() * 0.5f
        val halfH = h.toFloat() * 0.5f
        val halfInnerW = innerW * 0.5f
        val halfInnerH = innerH * 0.5f
        val r = radius.coerceIn(0f, min(halfInnerW, halfInnerH))
        val lensGradR = min(1.5f * r, min(halfInnerW, halfInnerH))
        val refractionActive = frostedPixels != null && refractionHeight > 0.5f && refractionAmount > 0.5f
        val sharpFadeW = 1.15f * refractionHeight
        val rimRange = max(highlightWidth, glowWidth) + 1.5f
        val grad = FloatArray(2)
        val invArea = 1f / max(halfInnerW * halfInnerH, 1f)

        for (y in 0 until h) {
            val yf = y.toFloat()
            val ny = (yf + 0.5f) / h.toFloat()
            val py = yf + 0.5f - halfH
            val ay = abs(py)
            val qy = ay - (halfInnerH - r)
            val row = y * w

            // Top dome crystal sheen + delicate bottom bounce matching the Home Pill shader
            val sheenAlpha = if (homePillCrystal) {
                val topSheen = if (ny < 0.40f) ((1f - ny / 0.40f) * 0.22f * (0.35f + 0.65f * shine)) else 0f
                val botBounce = if (ny > 0.76f) (((ny - 0.76f) / 0.24f) * 0.08f * (0.35f + 0.65f * shine)) else 0f
                topSheen + botBounce
            } else {
                0f
            }

            for (x in 0 until w) {
                val xf = x.toFloat()
                val px = xf + 0.5f - halfW
                val ax = abs(px)
                val qx = ax - (halfInnerW - r)

                val d = hypot(max(qx, 0f), max(qy, 0f)) - r + min(max(qx, qy), 0f)
                val cov = (0.5f - d).coerceIn(0f, 1f)
                if (cov <= 0f) continue

                var outR = 0f
                var outG = 0f
                var outB = 0f
                var outA = 0f

                if (frostedPixels != null) {
                    var sx = xf + 0.5f
                    var sy = yf + 0.5f
                    val inward = -d
                    if (refractionActive && inward < refractionHeight) {
                        val depthIn = -min(d, 0f)
                        val u = depthIn / refractionHeight
                        val oneMinusU = 1f - u
                        val meniscus = 1f - sqrt(max(0f, 1f - oneMinusU * oneMinusU))
                        val disp = -meniscus * refractionAmount

                        gradSdRoundedRect(px, py, halfInnerW, halfInnerH, lensGradR, grad)
                        var dirX = grad[0]
                        var dirY = grad[1]
                        val pLen = hypot(px, py)
                        if (pLen > 1e-4f) {
                            dirX += px / pLen
                            dirY += py / pLen
                        }
                        val nLen = hypot(dirX, dirY)
                        if (nLen > 1e-4f) {
                            sx = xf + 0.5f + disp * (dirX / nLen)
                            sy = yf + 0.5f + disp * (dirY / nLen)
                        }
                    }

                    var c = sampleBilinear(frostedPixels, w, h, sx, sy)
                    if (sharpPixels != null && refractionActive && inward < sharpFadeW) {
                        val t = (1f - inward / sharpFadeW).coerceIn(0f, 1f)
                        val mix = t * t * (3f - 2f * t)
                        if (mix > 0.001f) {
                            val sharpC = sampleBilinear(sharpPixels, w, h, sx, sy)
                            val invMix = 1f - mix
                            val mr = (((c ushr 16) and 0xFF) * invMix + ((sharpC ushr 16) and 0xFF) * mix + 0.5f).toInt().coerceIn(0, 255)
                            val mg = (((c ushr 8) and 0xFF) * invMix + ((sharpC ushr 8) and 0xFF) * mix + 0.5f).toInt().coerceIn(0, 255)
                            val mb = ((c and 0xFF) * invMix + (sharpC and 0xFF) * mix + 0.5f).toInt().coerceIn(0, 255)
                            c = (0xFF shl 24) or (mr shl 16) or (mg shl 8) or mb
                        }
                    }

                    val sr = ((c ushr 16) and 0xFF).toFloat()
                    val sg = ((c ushr 8) and 0xFF).toFloat()
                    val sb = (c and 0xFF).toFloat()
                    if (saturation != 1f) {
                        val luma = 0.213f * sr + 0.715f * sg + 0.072f * sb
                        outR = (luma + saturation * (sr - luma)).coerceIn(0f, 255f)
                        outG = (luma + saturation * (sg - luma)).coerceIn(0f, 255f)
                        outB = (luma + saturation * (sb - luma)).coerceIn(0f, 255f)
                    } else {
                        outR = sr
                        outG = sg
                        outB = sb
                    }
                    outA = 1f
                }

                for (veil in veils) {
                    val va = (veil ushr 24) / 255f
                    if (va > 0f) {
                        val invVa = 1f - va
                        outR = outR * invVa + ((veil ushr 16) and 0xFF) * va
                        outG = outG * invVa + ((veil ushr 8) and 0xFF) * va
                        outB = outB * invVa + (veil and 0xFF) * va
                        outA = outA * invVa + va
                    }
                }

                if (sheenAlpha > 0.001f) {
                    val invS = 1f - sheenAlpha
                    val sWhite = 255f * sheenAlpha
                    outR = outR * invS + sWhite
                    outG = outG * invS + sWhite
                    outB = outB * invS + sWhite
                    outA = outA * invS + sheenAlpha
                }

                var premulR = outR * cov
                var premulG = outG * cov
                var premulB = outB * cov
                var premulA = outA * cov

                val inwardDist = -d
                if (inwardDist < rimRange) {
                    gradSdRoundedRect(px, py, halfInnerW, halfInnerH, lensGradR, grad)
                    val diag = abs(grad[0] * 0.707107f + grad[1] * 0.707107f)
                    val dirFactor = if (homePillCrystal) 0.18f + 0.82f * diag else diag

                    var rim = 0f
                    if (inwardDist < highlightWidth + 1f) {
                        val hw = ((highlightWidth - inwardDist) / (highlightWidth * 0.5f + 0.5f)).coerceIn(0f, 1f)
                        rim += highlightAlpha * dirFactor * hw
                    }
                    if (glowWidth > 0f && glowAlpha > 0f && inwardDist < glowWidth) {
                        val gw = 1f - inwardDist / glowWidth
                        rim += glowAlpha * dirFactor * gw * gw
                    }
                    rim *= cov
                    if (rim > 0f) {
                        if (homePillCrystal) {
                            // Subtle RGB corner chromatic dispersion along the crystal bevel (sky-cyan <-> rose-violet)
                            val quadChroma = ((px * py) * invArea).coerceIn(-1f, 1f)
                            val rimShift = (quadChroma * 0.36f)
                            val rW = (1f + rimShift).coerceIn(0.62f, 1.38f)
                            val gW = (1f + 0.05f * abs(quadChroma)).coerceIn(0.9f, 1.08f)
                            val bW = (1f - rimShift).coerceIn(0.62f, 1.38f)
                            premulR += rim * rW * 255f
                            premulG += rim * gW * 255f
                            premulB += rim * bW * 255f
                            premulA += rim
                        } else {
                            val add = rim * 255f
                            premulR += add
                            premulG += add
                            premulB += add
                            premulA += rim
                        }
                    }
                }

                val finalA = min(1f, premulA)
                if (finalA > 0f) {
                    val denom = max(finalA, 1e-4f)
                    val rInt = (min(255f, premulR) / denom).toInt().coerceIn(0, 255)
                    val gInt = (min(255f, premulG) / denom).toInt().coerceIn(0, 255)
                    val bInt = (min(255f, premulB) / denom).toInt().coerceIn(0, 255)
                    val aInt = (finalA * 255f + 0.5f).toInt().coerceIn(0, 255)
                    out[row + x] = (aInt shl 24) or (rInt shl 16) or (gInt shl 8) or bInt
                }
            }
        }
        return out
    }

    fun shapeCoverBitmap(src: Bitmap, shape: WidgetCoverShape): Bitmap {
        val side = 144
        val bmp = createBitmap(side, side, Bitmap.Config.ARGB_8888)
        val c = Canvas(bmp)
        val rect = RectF(0f, 0f, side.toFloat(), side.toFloat())
        val corner = when (shape) {
            WidgetCoverShape.ROUNDED -> side * 0.16f
            WidgetCoverShape.CIRCLE -> side * 0.5f
            WidgetCoverShape.SQUARE -> 4f
        }
        val clip = Path().apply { addRoundRect(rect, corner, corner, Path.Direction.CW) }
        c.withSave {
            clipPath(clip)
            drawBitmap(src, null, rect, Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG))
        }
        return bmp
    }

    fun renderProgressBitmap(
        fraction: Float,
        accentArgb: Int,
        isLight: Boolean,
    ): Bitmap {
        val w = 480
        val h = 14
        val bmp = createBitmap(w, h, Bitmap.Config.ARGB_8888)
        val c = Canvas(bmp)
        val r = h * 0.5f
        val trackColor = if (isLight) Color.argb(48, 0, 0, 0) else Color.argb(88, 255, 255, 255)
        c.drawRoundRect(RectF(0f, 0f, w.toFloat(), h.toFloat()), r, r, Paint(Paint.ANTI_ALIAS_FLAG).apply { color = trackColor })
        val fillW = (w * fraction.coerceIn(0f, 1f)).coerceAtLeast(if (fraction > 0.005f) h.toFloat() else 0f)
        if (fillW > 0f) {
            c.drawRoundRect(
                RectF(0f, 0f, fillW, h.toFloat()),
                r,
                r,
                Paint(Paint.ANTI_ALIAS_FLAG).apply { color = accentArgb },
            )
        }
        return bmp
    }

    fun sampleWallpaperTint(context: Context): Int? = runCatching {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            WallpaperManager.getInstance(context.applicationContext)
                .getWallpaperColors(WallpaperManager.FLAG_SYSTEM)
                ?.primaryColor
                ?.toArgb()
        } else {
            null
        }
    }.getOrNull()

    fun renderStreakBadgeBitmap(days: Int, isLight: Boolean): Bitmap {
        val label = "${days.coerceAtLeast(0)}d"
        val h = 48
        val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.WHITE
            textSize = 24f
            typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
        }
        val textW = textPaint.measureText(label)
        val w = (48f + textW + 20f).roundToInt().coerceAtLeast(88)
        val bmp = createBitmap(w, h, Bitmap.Config.ARGB_8888)
        val c = Canvas(bmp)
        val rect = RectF(1.5f, 1.5f, w - 1.5f, h - 1.5f)
        val r = h * 0.5f

        c.drawRoundRect(
            rect,
            r,
            r,
            Paint(Paint.ANTI_ALIAS_FLAG).apply {
                color = if (isLight) Color.argb(215, 24, 28, 38) else Color.argb(190, 14, 18, 28)
            },
        )
        c.drawRoundRect(
            rect,
            r,
            r,
            Paint(Paint.ANTI_ALIAS_FLAG).apply {
                style = Paint.Style.STROKE
                strokeWidth = 2f
                color = Color.argb(110, 255, 255, 255)
            },
        )

        // Crisp vector flame badge on the left
        val fx = 24f
        val flame = Path().apply {
            moveTo(fx, 11f)
            cubicTo(fx + 9f, 18f, fx + 10f, 30f, fx, 36f)
            cubicTo(fx - 10f, 30f, fx - 8f, 19f, fx, 11f)
            close()
        }
        c.drawPath(
            flame,
            Paint(Paint.ANTI_ALIAS_FLAG).apply {
                color = Color.rgb(255, 168, 38)
            },
        )
        val fm = textPaint.fontMetrics
        val textY = h * 0.5f - (fm.ascent + fm.descent) * 0.5f
        c.drawText(label, 42f, textY, textPaint)
        return bmp
    }

    fun renderTransportIcon(
        kind: String,
        fgArgb: Int,
        roundFillArgb: Int? = null,
    ): Bitmap {
        val s = 96
        val bmp = createBitmap(s, s, Bitmap.Config.ARGB_8888)
        val c = Canvas(bmp)
        val cx = s * 0.5f
        val cy = s * 0.5f
        if (roundFillArgb != null) {
            c.drawCircle(cx, cy, s * 0.46f, Paint(Paint.ANTI_ALIAS_FLAG).apply { color = roundFillArgb })
        }
        val p = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = fgArgb }
        when (kind) {
            "play" -> {
                val left = if (roundFillArgb != null) s * 0.37f else s * 0.27f
                val right = if (roundFillArgb != null) s * 0.74f else s * 0.80f
                val path = Path().apply {
                    moveTo(left, s * 0.23f)
                    lineTo(right, cy)
                    lineTo(left, s * 0.77f)
                    close()
                }
                c.drawPath(path, p)
            }
            "pause" -> {
                c.drawRoundRect(RectF(s * 0.28f, s * 0.24f, s * 0.42f, s * 0.76f), 6f, 6f, p)
                c.drawRoundRect(RectF(s * 0.58f, s * 0.24f, s * 0.72f, s * 0.76f), 6f, 6f, p)
            }
            "next" -> {
                val t1 = Path().apply {
                    moveTo(s * 0.16f, s * 0.24f)
                    lineTo(s * 0.50f, cy)
                    lineTo(s * 0.16f, s * 0.76f)
                    close()
                }
                val t2 = Path().apply {
                    moveTo(s * 0.50f, s * 0.24f)
                    lineTo(s * 0.84f, cy)
                    lineTo(s * 0.50f, s * 0.76f)
                    close()
                }
                c.drawPath(t1, p)
                c.drawPath(t2, p)
            }
            "prev" -> {
                val t1 = Path().apply {
                    moveTo(s * 0.84f, s * 0.24f)
                    lineTo(s * 0.50f, cy)
                    lineTo(s * 0.84f, s * 0.76f)
                    close()
                }
                val t2 = Path().apply {
                    moveTo(s * 0.50f, s * 0.24f)
                    lineTo(s * 0.16f, cy)
                    lineTo(s * 0.50f, s * 0.76f)
                    close()
                }
                c.drawPath(t1, p)
                c.drawPath(t2, p)
            }
            "heart_on", "heart_off" -> {
                val hp = Path().apply {
                    moveTo(cx, s * 0.79f)
                    cubicTo(s * 0.10f, s * 0.54f, s * 0.12f, s * 0.22f, s * 0.34f, s * 0.22f)
                    cubicTo(s * 0.44f, s * 0.22f, cx, s * 0.32f, cx, s * 0.38f)
                    cubicTo(cx, s * 0.32f, s * 0.56f, s * 0.22f, s * 0.66f, s * 0.22f)
                    cubicTo(s * 0.88f, s * 0.22f, s * 0.90f, s * 0.54f, cx, s * 0.79f)
                    close()
                }
                if (kind == "heart_off") {
                    p.style = Paint.Style.STROKE
                    p.strokeWidth = 6f
                }
                c.drawPath(hp, p)
            }
        }
        return bmp
    }
}

object FerricWidgetUpdater {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    @Volatile
    private var cachedArtUrl: String? = null

    @Volatile
    private var cachedArtBitmap: Bitmap? = null

    fun runWidgetAction(block: suspend () -> Unit) {
        scope.launch { block() }
    }

    fun clearCachedArt() {
        cachedArtUrl = null
        cachedArtBitmap = null
    }

    fun hasAnyWidgets(context: Context): Boolean {
        val manager = AppWidgetManager.getInstance(context)
        return manager.getAppWidgetIds(ComponentName(context, LiquidGlassWidgetProvider::class.java)).isNotEmpty() ||
            manager.getAppWidgetIds(ComponentName(context, CompactStripWidgetProvider::class.java)).isNotEmpty() ||
            manager.getAppWidgetIds(ComponentName(context, SquareCoverWidgetProvider::class.java)).isNotEmpty()
    }

    fun updateProgressOnly(context: Context, state: PlaybackUiState, progress: PlaybackProgress) {
        val manager = AppWidgetManager.getInstance(context)
        val glassIds = manager.getAppWidgetIds(ComponentName(context, LiquidGlassWidgetProvider::class.java))
        if (glassIds.isEmpty()) return

        val durMs = progress.durationMs.coerceAtLeast(state.activeDisplayTrack()?.durationMs?.toLong() ?: state.durationMs).coerceAtLeast(0L)
        val posMs = progress.positionMs.coerceIn(0L, if (durMs > 0L) durMs else progress.positionMs.coerceAtLeast(0L))
        val frac = if (durMs > 0L) (posMs.toFloat() / durMs.toFloat()).coerceIn(0f, 1f) else 0f
        val elapsedStr = formatDurationMs(posMs)
        val totalStr = formatDurationMs(durMs)

        glassIds.forEach { widgetId ->
            val cfg = FerricWidgetPrefs.load(context, widgetId)
            if (!cfg.showProgressBar && !cfg.showTime) return@forEach
            val accent = FerricWidgetRenderer.resolveAccentArgb(cfg, state.coverShade)
            val isLight = cfg.style == FerricWidgetStyle.LIGHT
            val views = RemoteViews(context.packageName, R.layout.widget_liquid_glass).apply {
                if (cfg.showTime) {
                    setTextViewText(R.id.widget_time_elapsed, elapsedStr)
                    setTextViewText(R.id.widget_time_total, totalStr)
                }
                if (cfg.showProgressBar) {
                    val progBmp = FerricWidgetRenderer.renderProgressBitmap(frac, accent, isLight)
                    setImageViewBitmap(R.id.widget_progress_track, progBmp)
                }
            }
            manager.partiallyUpdateAppWidget(widgetId, views)
        }
    }

    fun updateAll(context: Context, overrideState: PlaybackUiState? = null) {
        val app = context.applicationContext as? FerricApp
        val state = overrideState ?: app?.store?.uiState?.value ?: return
        val progress = app?.store?.playbackProgress?.value ?: PlaybackProgress()
        val manager = AppWidgetManager.getInstance(context)

        val stripIds = manager.getAppWidgetIds(ComponentName(context, CompactStripWidgetProvider::class.java))
        val squareIds = manager.getAppWidgetIds(ComponentName(context, SquareCoverWidgetProvider::class.java))
        val glassIds = manager.getAppWidgetIds(ComponentName(context, LiquidGlassWidgetProvider::class.java))
        if (stripIds.isEmpty() && squareIds.isEmpty() && glassIds.isEmpty()) return

        val displayTrack = state.activeDisplayTrack()
        val artUrl = displayTrack?.thumbnailUrl
        val currentBitmap = if (!artUrl.isNullOrBlank() && artUrl == cachedArtUrl) cachedArtBitmap else null
        applyWidgets(context, manager, state, progress, currentBitmap, stripIds, squareIds, glassIds)

        if (!artUrl.isNullOrBlank() && artUrl != cachedArtUrl) {
            scope.launch {
                val bmp = runCatching {
                    val loader = SingletonImageLoader.get(context.applicationContext)
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
                    val latest = app?.store?.uiState?.value ?: state
                    val latestProg = app?.store?.playbackProgress?.value ?: progress
                    if (latest.activeDisplayTrack()?.thumbnailUrl == artUrl) {
                        applyWidgets(context, manager, latest, latestProg, bmp, stripIds, squareIds, glassIds)
                    }
                }
            }
        }
    }

    private fun applyWidgets(
        context: Context,
        manager: AppWidgetManager,
        state: PlaybackUiState,
        progress: PlaybackProgress,
        artBitmap: Bitmap?,
        stripIds: IntArray,
        squareIds: IntArray,
        glassIds: IntArray,
    ) {
        val activeTrack = state.activeDisplayTrack()
        val activePlaying = state.activeDisplayIsPlaying()
        val title = activeTrack?.title?.ifBlank { null } ?: "Ready to play"
        val artist = activeTrack?.artist?.ifBlank { null } ?: "Ferric"
        val durMs = progress.durationMs.coerceAtLeast(activeTrack?.durationMs?.toLong() ?: state.durationMs).coerceAtLeast(0L)
        val posMs = progress.positionMs.coerceIn(0L, if (durMs > 0L) durMs else progress.positionMs.coerceAtLeast(0L))
        val frac = if (durMs > 0L) (posMs.toFloat() / durMs.toFloat()).coerceIn(0f, 1f) else 0f
        val elapsedStr = formatDurationMs(posMs)
        val totalStr = formatDurationMs(durMs)
        val density = context.resources.displayMetrics.density
        val statsDays = state.listeningStats?.currentStreakDays?.toInt() ?: 0
        if (statsDays > 0) {
            FerricWidgetPrefs.saveLastKnownStreak(context, statsDays)
        }
        val streakDays = if (statsDays > 0) statsDays else FerricWidgetPrefs.loadLastKnownStreak(context)
        val wallpaperTint = FerricWidgetRenderer.sampleWallpaperTint(context)

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

        fun measureTargetPx(widgetId: Int, defaultWdp: Int, defaultHdp: Int): Triple<Int, Int, Float> {
            val opts = manager.getAppWidgetOptions(widgetId)
            val minW = opts.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 0)
            val maxW = opts.getInt(AppWidgetManager.OPTION_APPWIDGET_MAX_WIDTH, 0)
            val minH = opts.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_HEIGHT, 0)
            val wDp = (if (maxW > 0) maxW else if (minW > 0) minW else defaultWdp).coerceAtLeast(48)
            val hDp = (if (minH > 0) minH else defaultHdp).coerceAtLeast(48)
            val scale = min(density, 1.85f).coerceAtLeast(1.35f)
            val wPx = (wDp * scale).roundToInt().coerceIn(220, 620)
            val hPx = (hDp * scale).roundToInt().coerceIn(96, 340)
            return Triple(wPx, hPx, wPx.toFloat() / wDp.toFloat())
        }

        fun bindThemedCommon(
            views: RemoteViews,
            widgetId: Int,
            defWdp: Int,
            defHdp: Int,
            hasProgressViews: Boolean,
        ) {
            val cfg = FerricWidgetPrefs.load(context, widgetId)
            val isLight = cfg.style == FerricWidgetStyle.LIGHT
            val fgPrimary = if (isLight) Color.rgb(20, 22, 28) else Color.WHITE
            val fgSecondary = if (isLight) Color.argb(185, 36, 40, 50) else Color.argb(215, 255, 255, 255)
            val accent = FerricWidgetRenderer.resolveAccentArgb(cfg, state.coverShade)

            val (wPx, hPx, pxPerDp) = measureTargetPx(widgetId, defWdp, defHdp)
            val bgBmp = FerricWidgetRenderer.renderGlassBackgroundBitmap(wPx, hPx, cfg, artBitmap, state.coverShade, pxPerDp, wallpaperTint)
            views.setImageViewBitmap(R.id.widget_glass_bg, bgBmp)

            views.setTextViewText(R.id.widget_title, title)
            views.setTextViewText(R.id.widget_artist, artist)
            views.setTextColor(R.id.widget_title, fgPrimary)
            views.setTextColor(R.id.widget_artist, fgSecondary)
            views.setTextViewTextSize(R.id.widget_title, TypedValue.COMPLEX_UNIT_SP, cfg.textSize.titleSp)
            views.setTextViewTextSize(R.id.widget_artist, TypedValue.COMPLEX_UNIT_SP, cfg.textSize.artistSp)

            if (cfg.showCover && artBitmap != null) {
                views.setViewVisibility(R.id.widget_art, View.VISIBLE)
                views.setImageViewBitmap(R.id.widget_art, FerricWidgetRenderer.shapeCoverBitmap(artBitmap, cfg.coverShape))
            } else if (cfg.showCover) {
                views.setViewVisibility(R.id.widget_art, View.VISIBLE)
                views.setImageViewResource(R.id.widget_art, R.mipmap.ic_launcher)
            } else {
                views.setViewVisibility(R.id.widget_art, View.GONE)
            }

            val playKind = if (activePlaying) "pause" else "play"
            val playBmp = if (cfg.playButtonStyle == WidgetPlayButtonStyle.ROUND) {
                val iconTint = if (isLight) Color.WHITE else Color.rgb(16, 18, 24)
                FerricWidgetRenderer.renderTransportIcon(playKind, iconTint, accent)
            } else {
                FerricWidgetRenderer.renderTransportIcon(playKind, fgPrimary, null)
            }
            views.setImageViewBitmap(R.id.widget_btn_play, playBmp)

            if (hasProgressViews) {
                val showStreakPill = cfg.showStreak && streakDays > 0
                views.setViewVisibility(R.id.widget_streak_badge, if (showStreakPill) View.VISIBLE else View.GONE)
                if (showStreakPill) {
                    views.setImageViewBitmap(
                        R.id.widget_streak_badge,
                        FerricWidgetRenderer.renderStreakBadgeBitmap(streakDays, isLight),
                    )
                }

                views.setViewVisibility(R.id.widget_btn_like, if (cfg.showLikeButton) View.VISIBLE else View.GONE)
                if (cfg.showLikeButton) {
                    val liked = activeTrack?.liked == true
                    val likeColor = if (liked) accent else fgPrimary
                    views.setImageViewBitmap(
                        R.id.widget_btn_like,
                        FerricWidgetRenderer.renderTransportIcon(if (liked) "heart_on" else "heart_off", likeColor),
                    )
                }

                val prevNextVis = if (cfg.showPrevNext) View.VISIBLE else View.GONE
                views.setViewVisibility(R.id.widget_btn_prev, prevNextVis)
                views.setViewVisibility(R.id.widget_btn_next, prevNextVis)
                if (cfg.showPrevNext) {
                    views.setImageViewBitmap(R.id.widget_btn_prev, FerricWidgetRenderer.renderTransportIcon("prev", fgPrimary))
                    views.setImageViewBitmap(R.id.widget_btn_next, FerricWidgetRenderer.renderTransportIcon("next", fgPrimary))
                }

                val showBottomRow = cfg.showProgressBar || cfg.showTime
                views.setViewVisibility(R.id.widget_progress_row, if (showBottomRow) View.VISIBLE else View.GONE)
                views.setViewVisibility(R.id.widget_time_elapsed, if (cfg.showTime) View.VISIBLE else View.GONE)
                views.setViewVisibility(R.id.widget_time_total, if (cfg.showTime) View.VISIBLE else View.GONE)
                views.setViewVisibility(R.id.widget_progress_track, if (cfg.showProgressBar) View.VISIBLE else View.GONE)
                if (cfg.showTime) {
                    views.setTextViewText(R.id.widget_time_elapsed, elapsedStr)
                    views.setTextViewText(R.id.widget_time_total, totalStr)
                    views.setTextColor(R.id.widget_time_elapsed, fgSecondary)
                    views.setTextColor(R.id.widget_time_total, fgSecondary)
                }
                if (cfg.showProgressBar) {
                    views.setImageViewBitmap(
                        R.id.widget_progress_track,
                        FerricWidgetRenderer.renderProgressBitmap(frac, accent, isLight),
                    )
                }
            }
        }

        stripIds.forEach { widgetId ->
            val cfg = FerricWidgetPrefs.load(context, widgetId)
            val isLight = cfg.style == FerricWidgetStyle.LIGHT
            val fgPrimary = if (isLight) Color.rgb(20, 22, 28) else Color.WHITE
            val views = RemoteViews(context.packageName, R.layout.widget_compact_strip).apply {
                bindThemedCommon(this, widgetId, 320, 76, hasProgressViews = false)
                setImageViewBitmap(R.id.widget_btn_prev, FerricWidgetRenderer.renderTransportIcon("prev", fgPrimary))
                setImageViewBitmap(R.id.widget_btn_next, FerricWidgetRenderer.renderTransportIcon("next", fgPrimary))
                setOnClickPendingIntent(R.id.widget_root, openAppIntent)
                setOnClickPendingIntent(R.id.widget_btn_prev, actionPi(101, ACTION_WIDGET_PREV))
                setOnClickPendingIntent(R.id.widget_btn_play, actionPi(102, ACTION_WIDGET_PLAY_PAUSE))
                setOnClickPendingIntent(R.id.widget_btn_next, actionPi(103, ACTION_WIDGET_NEXT))
            }
            manager.updateAppWidget(widgetId, views)
        }

        squareIds.forEach { widgetId ->
            val cfg = FerricWidgetPrefs.load(context, widgetId)
            val isLight = cfg.style == FerricWidgetStyle.LIGHT
            val fgPrimary = if (isLight) Color.rgb(20, 22, 28) else Color.WHITE
            val accent = FerricWidgetRenderer.resolveAccentArgb(cfg, state.coverShade)
            val liked = activeTrack?.liked == true
            val views = RemoteViews(context.packageName, R.layout.widget_square_cover).apply {
                bindThemedCommon(this, widgetId, 180, 180, hasProgressViews = false)
                setImageViewBitmap(R.id.widget_btn_next, FerricWidgetRenderer.renderTransportIcon("next", fgPrimary))
                setImageViewBitmap(
                    R.id.widget_btn_like,
                    FerricWidgetRenderer.renderTransportIcon(if (liked) "heart_on" else "heart_off", if (liked) accent else fgPrimary),
                )
                setOnClickPendingIntent(R.id.widget_root, openAppIntent)
                setOnClickPendingIntent(R.id.widget_btn_play, actionPi(202, ACTION_WIDGET_PLAY_PAUSE))
                setOnClickPendingIntent(R.id.widget_btn_next, actionPi(203, ACTION_WIDGET_NEXT))
                setOnClickPendingIntent(R.id.widget_btn_like, actionPi(204, ACTION_WIDGET_LIKE))
            }
            manager.updateAppWidget(widgetId, views)
        }

        glassIds.forEach { widgetId ->
            val views = RemoteViews(context.packageName, R.layout.widget_liquid_glass).apply {
                bindThemedCommon(this, widgetId, 330, 160, hasProgressViews = true)
                setOnClickPendingIntent(R.id.widget_root, openAppIntent)
                setOnClickPendingIntent(R.id.widget_btn_prev, actionPi(301, ACTION_WIDGET_PREV))
                setOnClickPendingIntent(R.id.widget_btn_play, actionPi(302, ACTION_WIDGET_PLAY_PAUSE))
                setOnClickPendingIntent(R.id.widget_btn_next, actionPi(303, ACTION_WIDGET_NEXT))
                setOnClickPendingIntent(R.id.widget_btn_like, actionPi(304, ACTION_WIDGET_LIKE))
            }
            manager.updateAppWidget(widgetId, views)
        }
    }
}
