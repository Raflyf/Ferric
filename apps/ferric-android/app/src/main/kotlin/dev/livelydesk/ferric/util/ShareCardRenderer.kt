package dev.livelydesk.ferric.util

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Shader
import android.graphics.Typeface
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import androidx.compose.ui.graphics.toArgb
import androidx.core.content.FileProvider
import coil3.ImageLoader
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import coil3.request.allowHardware
import coil3.toBitmap
import dev.livelydesk.ferric.ui.theme.CoverShadeColors
import java.io.File
import java.io.FileOutputStream
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.ferric_mobile_ffi.FfiTrack

enum class ShareCardAspect(val label: String, val widthPx: Int, val heightPx: Int) {
    STORY_9_16("9:16 Story", 1080, 1920),
    SQUARE_1_1("1:1 Square", 1080, 1080),
}

enum class ShareCardStyle(val label: String) {
    LIQUID_GLASS("Liquid Glass"),
    AMOLED("AMOLED"),
    ADAPTIVE_GRADIENT("Adaptive"),
}

object ShareCardRenderer {
    suspend fun renderAndSharePng(
        context: Context,
        track: FfiTrack,
        lyricLines: List<String>,
        aspect: ShareCardAspect,
        style: ShareCardStyle,
        coverShade: CoverShadeColors,
    ): Result<File> = withContext(Dispatchers.IO) {
        runCatching {
            val artBitmap = fetchCoverBitmap(context, track.thumbnailUrl)
            val bitmap = Bitmap.createBitmap(aspect.widthPx, aspect.heightPx, Bitmap.Config.ARGB_8888)
            val canvas = Canvas(bitmap)
            drawCard(canvas, aspect.widthPx, aspect.heightPx, track, lyricLines, style, coverShade, artBitmap)

            val outDir = File(context.cacheDir, "shared_cards").apply { mkdirs() }
            val outFile = File(outDir, "ferric_lyrics_card.png")
            FileOutputStream(outFile).use { fos ->
                bitmap.compress(Bitmap.CompressFormat.PNG, 96, fos)
            }
            bitmap.recycle()

            val uri = FileProvider.getUriForFile(
                context,
                "${context.packageName}.fileprovider",
                outFile,
            )
            val shareIntent = Intent(Intent.ACTION_SEND).apply {
                type = "image/png"
                putExtra(Intent.EXTRA_STREAM, uri)
                putExtra(Intent.EXTRA_TEXT, "${track.title} - ${track.artist}")
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            val chooser = Intent.createChooser(shareIntent, "Share Lyrics Card").apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(chooser)
            outFile
        }
    }

    private suspend fun fetchCoverBitmap(context: Context, url: String?): Bitmap? {
        if (url.isNullOrBlank()) return null
        return runCatching {
            val loader = ImageLoader(context.applicationContext)
            val req = ImageRequest.Builder(context.applicationContext)
                .data(url)
                .size(512, 512)
                .allowHardware(false)
                .build()
            (loader.execute(req) as? SuccessResult)?.image?.toBitmap()
        }.getOrNull()
    }

    private fun drawCard(
        canvas: Canvas,
        w: Int,
        h: Int,
        track: FfiTrack,
        lines: List<String>,
        style: ShareCardStyle,
        shade: CoverShadeColors,
        artBitmap: Bitmap?,
    ) {
        val bgPaint = Paint(Paint.ANTI_ALIAS_FLAG)
        val orb1 = shade.orb1.toArgb()
        val orb2 = shade.orb2.toArgb()
        val accent = shade.accent.toArgb()

        when (style) {
            ShareCardStyle.AMOLED -> {
                canvas.drawColor(Color.BLACK)
            }
            ShareCardStyle.LIQUID_GLASS -> {
                bgPaint.shader = LinearGradient(
                    0f,
                    0f,
                    w.toFloat(),
                    h.toFloat(),
                    intArrayOf(orb1, Color.rgb(12, 14, 22), orb2),
                    floatArrayOf(0f, 0.52f, 1f),
                    Shader.TileMode.CLAMP,
                )
                canvas.drawRect(0f, 0f, w.toFloat(), h.toFloat(), bgPaint)
            }
            ShareCardStyle.ADAPTIVE_GRADIENT -> {
                bgPaint.shader = LinearGradient(
                    0f,
                    0f,
                    w.toFloat(),
                    h.toFloat(),
                    intArrayOf(accent, orb1, Color.rgb(10, 10, 14)),
                    floatArrayOf(0f, 0.45f, 1f),
                    Shader.TileMode.CLAMP,
                )
                canvas.drawRect(0f, 0f, w.toFloat(), h.toFloat(), bgPaint)
            }
        }

        val pad = 76f
        val isStory = h > w
        val cardTop = if (isStory) 300f else 84f
        val cardBottom = h - (if (isStory) 300f else 84f)
        val cardRect = RectF(pad, cardTop, w - pad, cardBottom)

        val cardFill = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = when (style) {
                ShareCardStyle.AMOLED -> Color.argb(255, 14, 14, 18)
                ShareCardStyle.LIQUID_GLASS -> Color.argb(175, 18, 22, 34)
                ShareCardStyle.ADAPTIVE_GRADIENT -> Color.argb(165, 12, 12, 16)
            }
        }
        canvas.drawRoundRect(cardRect, 48f, 48f, cardFill)

        val cardBorder = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            this.style = Paint.Style.STROKE
            strokeWidth = 3f
            color = Color.argb(85, 255, 255, 255)
        }
        canvas.drawRoundRect(cardRect, 48f, 48f, cardBorder)

        val innerX = cardRect.left + 58f
        val innerW = (cardRect.width() - 116f).toInt().coerceAtLeast(200)
        var cursorY = cardRect.top + 64f

        // Header: Cover Art + Title/Artist
        val artSize = 136f
        val artRect = RectF(innerX, cursorY, innerX + artSize, cursorY + artSize)
        if (artBitmap != null) {
            val saved = canvas.save()
            val clipPath = android.graphics.Path().apply {
                addRoundRect(artRect, 24f, 24f, android.graphics.Path.Direction.CW)
            }
            canvas.clipPath(clipPath)
            canvas.drawBitmap(artBitmap, null, artRect, Paint(Paint.FILTER_BITMAP_FLAG))
            canvas.restoreToCount(saved)
        } else {
            val fallbackArt = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = accent }
            canvas.drawRoundRect(artRect, 24f, 24f, fallbackArt)
        }

        val titlePaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.WHITE
            textSize = 44f
            typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD)
        }
        val artistPaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.argb(200, 220, 224, 234)
            textSize = 32f
            typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.NORMAL)
        }
        val textStartX = innerX + artSize + 34f
        val headerW = (cardRect.right - 58f - textStartX).toInt().coerceAtLeast(160)
        val titleLayout = StaticLayout.Builder.obtain(track.title, 0, track.title.length, titlePaint, headerW)
            .setMaxLines(2)
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .build()
        canvas.save()
        canvas.translate(textStartX, cursorY + 12f)
        titleLayout.draw(canvas)
        canvas.restore()

        val artistLayout = StaticLayout.Builder.obtain(track.artist, 0, track.artist.length, artistPaint, headerW)
            .setMaxLines(1)
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .build()
        canvas.save()
        canvas.translate(textStartX, cursorY + 18f + titleLayout.height)
        artistLayout.draw(canvas)
        canvas.restore()

        cursorY += artSize + 64f

        // Lyrics body (1-5 lines)
        val bodyText = if (lines.isEmpty()) {
            "\"${track.title}\"\n${track.artist}"
        } else {
            lines.take(5).joinToString("\n\n")
        }
        val lyricsPaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.WHITE
            textSize = if (isStory) 54f else 44f
            typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD)
        }
        val lyricsLayout = StaticLayout.Builder.obtain(bodyText, 0, bodyText.length, lyricsPaint, innerW)
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .setLineSpacing(10f, 1.08f)
            .setMaxLines(10)
            .build()
        canvas.save()
        canvas.translate(innerX, cursorY)
        lyricsLayout.draw(canvas)
        canvas.restore()

        // Footer brand stamp
        val brandPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = accent
            textSize = 28f
            letterSpacing = 0.18f
            typeface = Typeface.create(Typeface.MONOSPACE, Typeface.BOLD)
        }
        canvas.drawText("FERRIC", innerX, cardRect.bottom - 52f, brandPaint)
    }
}
