package dev.livelydesk.ferric.util

import android.content.ClipData
import android.content.ContentValues
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
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import androidx.compose.ui.graphics.toArgb
import androidx.core.content.FileProvider
import androidx.core.content.res.ResourcesCompat
import androidx.core.graphics.createBitmap
import androidx.core.graphics.withSave
import androidx.core.graphics.withTranslation
import coil3.SingletonImageLoader
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import coil3.request.allowHardware
import coil3.toBitmap
import dev.livelydesk.ferric.R
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
        fontFamilyName: String = "DM Sans",
        editorialMode: Boolean = false,
    ): Result<File> = withContext(Dispatchers.IO) {
        runCatching {
            val outFile = renderToCachePng(
                context = context,
                track = track,
                lyricLines = lyricLines,
                aspect = aspect,
                style = style,
                coverShade = coverShade,
                fontFamilyName = fontFamilyName,
                editorialMode = editorialMode,
            )

            val uri = FileProvider.getUriForFile(
                context,
                "${context.packageName}.fileprovider",
                outFile,
            )
            val shareIntent = Intent(Intent.ACTION_SEND).apply {
                type = "image/png"
                putExtra(Intent.EXTRA_STREAM, uri)
                putExtra(Intent.EXTRA_TEXT, "${track.title} - ${track.artist}")
                clipData = ClipData.newRawUri("Ferric Lyrics Card", uri)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            val chooser = Intent.createChooser(shareIntent, "Share Lyrics Card").apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            withContext(Dispatchers.Main) {
                context.startActivity(chooser)
            }
            outFile
        }
    }

    suspend fun saveToGalleryPng(
        context: Context,
        track: FfiTrack,
        lyricLines: List<String>,
        aspect: ShareCardAspect,
        style: ShareCardStyle,
        coverShade: CoverShadeColors,
        fontFamilyName: String = "DM Sans",
        editorialMode: Boolean = false,
    ): Result<String> = withContext(Dispatchers.IO) {
        runCatching {
            val cacheFile = renderToCachePng(
                context = context,
                track = track,
                lyricLines = lyricLines,
                aspect = aspect,
                style = style,
                coverShade = coverShade,
                fontFamilyName = fontFamilyName,
                editorialMode = editorialMode,
            )
            val cleanTitle = track.title
                .replace(Regex("[^A-Za-z0-9_\\-]+"), "_")
                .trim('_')
                .ifBlank { "ferric" }
                .take(36)
            val fileName = "${cleanTitle}_${System.currentTimeMillis() / 1000L}.png"

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val values = ContentValues().apply {
                    put(MediaStore.Images.Media.DISPLAY_NAME, fileName)
                    put(MediaStore.Images.Media.MIME_TYPE, "image/png")
                    put(MediaStore.Images.Media.RELATIVE_PATH, "${Environment.DIRECTORY_PICTURES}/Ferric")
                    put(MediaStore.Images.Media.IS_PENDING, 1)
                }
                val resolver = context.contentResolver
                val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
                    ?: error("Failed to create gallery entry")
                resolver.openOutputStream(uri)?.use { out ->
                    cacheFile.inputStream().use { input -> input.copyTo(out) }
                } ?: error("Failed to write gallery PNG")
                values.clear()
                values.put(MediaStore.Images.Media.IS_PENDING, 0)
                resolver.update(uri, values, null, null)
                "Pictures/Ferric/$fileName"
            } else {
                val picsDir = File(
                    Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_PICTURES),
                    "Ferric",
                ).apply { mkdirs() }
                val dest = File(picsDir, fileName)
                cacheFile.copyTo(dest, overwrite = true)
                dest.absolutePath
            }
        }
    }

    private suspend fun renderToCachePng(
        context: Context,
        track: FfiTrack,
        lyricLines: List<String>,
        aspect: ShareCardAspect,
        style: ShareCardStyle,
        coverShade: CoverShadeColors,
        fontFamilyName: String,
        editorialMode: Boolean,
    ): File {
        val artBitmap = fetchCoverBitmap(context, track.thumbnailUrl)
        val bitmap = createBitmap(aspect.widthPx, aspect.heightPx, Bitmap.Config.ARGB_8888)
        val outDir = File(context.cacheDir, "shared_cards").apply { mkdirs() }
        val outFile = File(outDir, "ferric_lyrics_card.png")
        val typeface = resolveCardTypeface(context, fontFamilyName, editorialMode)
        try {
            val canvas = Canvas(bitmap)
            drawCard(canvas, aspect.widthPx, aspect.heightPx, track, lyricLines, style, coverShade, artBitmap, typeface)
            FileOutputStream(outFile).use { fos ->
                bitmap.compress(Bitmap.CompressFormat.PNG, 96, fos)
            }
        } finally {
            bitmap.recycle()
        }
        return outFile
    }

    private fun resolveCardTypeface(context: Context, fontName: String, editorial: Boolean): Typeface {
        if (editorial) return Typeface.create(Typeface.SERIF, Typeface.BOLD)
        val resId = when (fontName.trim().lowercase()) {
            "figtree", "plus jakarta sans" -> R.font.figtree
            "space grotesk" -> R.font.space_grotesk
            "jetbrains mono", "mono" -> R.font.jetbrains_mono
            else -> R.font.dm_sans
        }
        val base = runCatching { ResourcesCompat.getFont(context, resId) }.getOrNull()
        return if (base != null) Typeface.create(base, Typeface.BOLD) else Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD)
    }

    private suspend fun fetchCoverBitmap(context: Context, url: String?): Bitmap? {
        if (url.isNullOrBlank()) return null
        return runCatching {
            val loader = SingletonImageLoader.get(context.applicationContext)
            val req = ImageRequest.Builder(context.applicationContext)
                .data(url)
                .size(512, 512)
                .allowHardware(false)
                .build()
            (loader.execute(req) as? SuccessResult)?.image?.toBitmap()
        }.getOrNull()
    }

    private fun softBlurBitmap(src: Bitmap, downscaleFactor: Int = 10): Bitmap {
        val w = src.width.coerceAtLeast(1)
        val h = src.height.coerceAtLeast(1)
        val smallW = (w / downscaleFactor).coerceAtLeast(8)
        val smallH = (h / downscaleFactor).coerceAtLeast(8)
        val tiny = Bitmap.createScaledBitmap(src, smallW, smallH, true)
        val midW = (w / 3).coerceAtLeast(smallW)
        val midH = (h / 3).coerceAtLeast(smallH)
        val mid = Bitmap.createScaledBitmap(tiny, midW, midH, true)
        if (tiny !== src) tiny.recycle()
        val smooth = Bitmap.createScaledBitmap(mid, w, h, true)
        if (mid !== src) mid.recycle()
        return smooth
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
        typeface: Typeface,
    ) {
        val bgPaint = Paint(Paint.ANTI_ALIAS_FLAG)
        val orb1 = shade.orb1.toArgb()
        val orb2 = shade.orb2.toArgb()
        val accent = shade.accent.toArgb()

        // Render full canvas backdrop onto an offscreen bitmap when using LIQUID_GLASS so we can
        // refract & frost the true blurred pixels directly inside the glass card.
        val backdropBmp = if (style == ShareCardStyle.LIQUID_GLASS) {
            createBitmap(w, h, Bitmap.Config.ARGB_8888)
        } else {
            null
        }
        val bgCanvas = backdropBmp?.let { Canvas(it) } ?: canvas

        when (style) {
            ShareCardStyle.AMOLED -> {
                bgCanvas.drawColor(Color.BLACK)
            }
            ShareCardStyle.LIQUID_GLASS -> {
                if (artBitmap != null) {
                    val blurredBg = softBlurBitmap(
                        Bitmap.createScaledBitmap(artBitmap, 160, 160, true),
                        downscaleFactor = 6,
                    )
                    val side = maxOf(w, h).toFloat()
                    val left = (w - side) * 0.5f
                    val top = (h - side) * 0.5f
                    bgCanvas.drawBitmap(blurredBg, null, RectF(left, top, left + side, top + side), Paint(Paint.FILTER_BITMAP_FLAG))
                    blurredBg.recycle()
                } else {
                    bgCanvas.drawColor(Color.rgb(12, 14, 22))
                }
                bgPaint.shader = android.graphics.RadialGradient(
                    w * 0.25f,
                    h * 0.22f,
                    w * 0.95f,
                    intArrayOf(
                        Color.argb(145, Color.red(accent), Color.green(accent), Color.blue(accent)),
                        Color.argb(165, Color.red(orb1), Color.green(orb1), Color.blue(orb1)),
                        Color.argb(210, Color.red(orb2), Color.green(orb2), Color.blue(orb2)),
                    ),
                    floatArrayOf(0f, 0.48f, 1f),
                    Shader.TileMode.CLAMP,
                )
                bgCanvas.drawRect(0f, 0f, w.toFloat(), h.toFloat(), bgPaint)
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
                bgCanvas.drawRect(0f, 0f, w.toFloat(), h.toFloat(), bgPaint)
            }
        }

        if (backdropBmp != null) {
            canvas.drawBitmap(backdropBmp, 0f, 0f, null)
        }

        val pad = 76f
        val isStory = h > w
        val cardLeft = pad
        val cardRight = w - pad
        val innerX = cardLeft + 56f
        val innerW = (cardRight - cardLeft - 112f).toInt().coerceAtLeast(220)
        val artSize = 132f
        val textStartX = innerX + artSize + 30f
        val headerW = (cardRight - 56f - textStartX).toInt().coerceAtLeast(160)

        val titlePaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.WHITE
            textSize = 40f
            this.typeface = typeface
        }
        val artistPaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.argb(215, 224, 229, 240)
            textSize = 30f
            this.typeface = Typeface.create(typeface, Typeface.NORMAL)
        }
        val titleLayout = StaticLayout.Builder.obtain(track.title, 0, track.title.length, titlePaint, headerW)
            .setMaxLines(2)
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .build()
        val artistLayout = StaticLayout.Builder.obtain(track.artist, 0, track.artist.length, artistPaint, headerW)
            .setMaxLines(1)
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .build()

        val bodyText = if (lines.isEmpty()) {
            "\"${track.title}\"\n${track.artist}"
        } else {
            lines.take(5).joinToString("\n\n")
        }
        val lyricsPaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.WHITE
            textSize = if (isStory) 48f else 40f
            this.typeface = typeface
        }
        val lyricsLayout = StaticLayout.Builder.obtain(bodyText, 0, bodyText.length, lyricsPaint, innerW)
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .setLineSpacing(10f, 1.08f)
            .setMaxLines(12)
            .build()

        val headerHeight = maxOf(artSize, (titleLayout.height + artistLayout.height + 16f))
        val contentHeight = 56f + headerHeight + 52f + lyricsLayout.height + 56f + 44f
        val minH = if (isStory) 620f else 540f
        val cardH = contentHeight.coerceIn(minH, h - (if (isStory) 300f else 130f))
        val cardTop = (h - cardH) * 0.5f
        val cardBottom = cardTop + cardH
        val cardRect = RectF(cardLeft, cardTop, cardRight, cardBottom)
        val corner = 48f

        val cardPath = android.graphics.Path().apply {
            addRoundRect(cardRect, corner, corner, android.graphics.Path.Direction.CW)
        }

        if (backdropBmp != null && style == ShareCardStyle.LIQUID_GLASS) {
            // Crop & heavily frost the exact area beneath the card for true optical Liquid Glass
            val cropL = cardRect.left.toInt().coerceIn(0, w - 2)
            val cropT = cardRect.top.toInt().coerceIn(0, h - 2)
            val cropW = (cardRect.width().toInt()).coerceIn(2, w - cropL)
            val cropH = (cardRect.height().toInt()).coerceIn(2, h - cropT)
            val slice = Bitmap.createBitmap(backdropBmp, cropL, cropT, cropW, cropH)
            val frosted = softBlurBitmap(slice, downscaleFactor = 12)
            slice.recycle()

            canvas.withSave {
                clipPath(cardPath)
                drawBitmap(frosted, null, cardRect, Paint(Paint.FILTER_BITMAP_FLAG))
                // Translucent optical tint + top inner meniscus highlight sheen
                drawRect(cardRect, Paint().apply { color = Color.argb(66, 20, 26, 44) })
                val sheenPaint = Paint().apply {
                    shader = LinearGradient(
                        cardRect.left,
                        cardRect.top,
                        cardRect.left,
                        cardRect.bottom,
                        intArrayOf(
                            Color.argb(72, 255, 255, 255),
                            Color.argb(0, 255, 255, 255),
                            Color.argb(46, 0, 0, 0),
                        ),
                        floatArrayOf(0f, 0.35f, 1f),
                        Shader.TileMode.CLAMP,
                    )
                }
                drawRect(cardRect, sheenPaint)
            }
            frosted.recycle()
            backdropBmp.recycle()
        } else {
            backdropBmp?.recycle()
            val cardFill = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                color = when (style) {
                    ShareCardStyle.AMOLED -> Color.argb(255, 13, 13, 17)
                    ShareCardStyle.LIQUID_GLASS -> Color.argb(170, 20, 24, 38)
                    ShareCardStyle.ADAPTIVE_GRADIENT -> Color.argb(122, 8, 9, 14)
                }
            }
            canvas.drawRoundRect(cardRect, corner, corner, cardFill)
        }

        // Directional top-left chrome specular rim border (matches Live Preview AGSL rim)
        val cardBorder = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            this.style = Paint.Style.STROKE
            strokeWidth = if (style == ShareCardStyle.LIQUID_GLASS) 3.5f else 2.5f
            if (style == ShareCardStyle.LIQUID_GLASS) {
                shader = LinearGradient(
                    cardRect.left,
                    cardRect.top,
                    cardRect.right,
                    cardRect.bottom,
                    intArrayOf(
                        Color.argb(200, 255, 255, 255),
                        Color.argb(56, 255, 255, 255),
                        Color.argb(125, Color.red(accent), Color.green(accent), Color.blue(accent)),
                    ),
                    floatArrayOf(0f, 0.50f, 1f),
                    Shader.TileMode.CLAMP,
                )
            } else {
                color = Color.argb(if (style == ShareCardStyle.AMOLED) 45 else 92, 255, 255, 255)
            }
        }
        canvas.drawRoundRect(cardRect, corner, corner, cardBorder)

        var cursorY = cardRect.top + 56f
        val artRect = RectF(innerX, cursorY, innerX + artSize, cursorY + artSize)
        if (artBitmap != null) {
            canvas.withSave {
                val clipPath = android.graphics.Path().apply {
                    addRoundRect(artRect, 24f, 24f, android.graphics.Path.Direction.CW)
                }
                clipPath(clipPath)
                drawBitmap(artBitmap, null, artRect, Paint(Paint.FILTER_BITMAP_FLAG))
            }
        } else {
            val fallbackArt = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = accent }
            canvas.drawRoundRect(artRect, 24f, 24f, fallbackArt)
        }

        canvas.withTranslation(textStartX, cursorY + 8f) {
            titleLayout.draw(this)
        }
        canvas.withTranslation(textStartX, cursorY + 14f + titleLayout.height) {
            artistLayout.draw(this)
        }

        cursorY += headerHeight + 50f
        canvas.withTranslation(innerX, cursorY) {
            lyricsLayout.draw(this)
        }

        val brandPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = if (style == ShareCardStyle.LIQUID_GLASS) {
                Color.argb(232, 255, 255, 255)
            } else {
                accent
            }
            textSize = 27f
            letterSpacing = 0.18f
            this.typeface = Typeface.create(Typeface.MONOSPACE, Typeface.BOLD)
        }
        canvas.drawText("FERRIC", innerX, cardRect.bottom - 44f, brandPaint)
        if (style == ShareCardStyle.LIQUID_GLASS) {
            val tagPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                color = Color.argb(175, 255, 255, 255)
                textSize = 21f
                letterSpacing = 0.08f
                textAlign = Paint.Align.RIGHT
                this.typeface = Typeface.create(typeface, Typeface.NORMAL)
            }
            canvas.drawText("LIQUID GLASS", cardRight - 56f, cardRect.bottom - 44f, tagPaint)
        }
    }
}
