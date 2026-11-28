package dev.livelydesk.ferric.util

import android.content.ClipData
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BlurMaskFilter
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.ColorMatrix
import android.graphics.ColorMatrixColorFilter
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.RadialGradient
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
import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sqrt
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

    private fun buildVibratedGaussianCrop(src: Bitmap, radius: Int = 6): Bitmap {
        val w = src.width.coerceAtLeast(1)
        val h = src.height.coerceAtLeast(1)
        val bw = (w * 0.25f).roundToInt().coerceAtLeast(12)
        val bh = (h * 0.25f).roundToInt().coerceAtLeast(12)

        val small = createBitmap(bw, bh, Bitmap.Config.ARGB_8888)
        val smallCanvas = Canvas(small)
        val satMatrix = ColorMatrix().apply { setSaturation(1.55f) }
        val satPaint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG).apply {
            colorFilter = ColorMatrixColorFilter(satMatrix)
        }
        smallCanvas.drawBitmap(src, null, RectF(0f, 0f, bw.toFloat(), bh.toFloat()), satPaint)

        val count = bw * bh
        val pxA = IntArray(count)
        val pxB = IntArray(count)
        small.getPixels(pxA, 0, bw, 0, 0, bw, bh)

        // Separable 2-pass Gaussian kernel matching Ferric's 0.25x GPU blur pass
        val sigma = max(1.0f, radius * 0.55f)
        val weights = FloatArray(radius * 2 + 1)
        var wSum = 0f
        for (i in -radius..radius) {
            val wgt = kotlin.math.exp(-(i * i) / (2f * sigma * sigma))
            weights[i + radius] = wgt
            wSum += wgt
        }
        for (i in weights.indices) weights[i] /= wSum

        for (y in 0 until bh) {
            val rowOff = y * bw
            for (x in 0 until bw) {
                var r = 0f
                var g = 0f
                var b = 0f
                for (k in -radius..radius) {
                    val sx = (x + k).coerceIn(0, bw - 1)
                    val c = pxA[rowOff + sx]
                    val wt = weights[k + radius]
                    r += ((c ushr 16) and 0xFF) * wt
                    g += ((c ushr 8) and 0xFF) * wt
                    b += (c and 0xFF) * wt
                }
                pxB[rowOff + x] = (0xFF shl 24) or
                    (r.roundToInt().coerceIn(0, 255) shl 16) or
                    (g.roundToInt().coerceIn(0, 255) shl 8) or
                    b.roundToInt().coerceIn(0, 255)
            }
        }
        for (y in 0 until bh) {
            val rowOff = y * bw
            for (x in 0 until bw) {
                var r = 0f
                var g = 0f
                var b = 0f
                for (k in -radius..radius) {
                    val sy = (y + k).coerceIn(0, bh - 1)
                    val c = pxB[sy * bw + x]
                    val wt = weights[k + radius]
                    r += ((c ushr 16) and 0xFF) * wt
                    g += ((c ushr 8) and 0xFF) * wt
                    b += (c and 0xFF) * wt
                }
                pxA[rowOff + x] = (0xFF shl 24) or
                    (r.roundToInt().coerceIn(0, 255) shl 16) or
                    (g.roundToInt().coerceIn(0, 255) shl 8) or
                    b.roundToInt().coerceIn(0, 255)
            }
        }
        small.setPixels(pxA, 0, bw, 0, 0, bw, bh)
        val smooth = Bitmap.createScaledBitmap(small, w, h, true)
        if (smooth !== small) small.recycle()
        return smooth
    }

    private fun buildLiquidGlassCardBitmap(
        backdropBmp: Bitmap,
        cardRect: RectF,
        cornerRadius: Float,
        glassTintArgb: Int,
    ): Bitmap {
        val margin = 72
        val cropL = (cardRect.left - margin).roundToInt().coerceIn(0, backdropBmp.width - 2)
        val cropT = (cardRect.top - margin).roundToInt().coerceIn(0, backdropBmp.height - 2)
        val cropR = (cardRect.right + margin).roundToInt().coerceIn(cropL + 2, backdropBmp.width)
        val cropB = (cardRect.bottom + margin).roundToInt().coerceIn(cropT + 2, backdropBmp.height)
        val srcW = (cropR - cropL).coerceAtLeast(2)
        val srcH = (cropB - cropT).coerceAtLeast(2)

        val rawCrop = Bitmap.createBitmap(backdropBmp, cropL, cropT, srcW, srcH)
        val blurredCrop = buildVibratedGaussianCrop(rawCrop, radius = 6)
        rawCrop.recycle()

        val cardW = cardRect.width().roundToInt().coerceAtLeast(4)
        val cardH = cardRect.height().roundToInt().coerceAtLeast(4)
        val offX = cardRect.left - cropL
        val offY = cardRect.top - cropT

        val srcPixels = IntArray(srcW * srcH)
        blurredCrop.getPixels(srcPixels, 0, srcW, 0, 0, srcW, srcH)
        blurredCrop.recycle()

        val outPixels = IntArray(cardW * cardH)
        val halfW = cardW * 0.5f
        val halfH = cardH * 0.5f
        val minHalf = min(halfW, halfH)
        val r = min(cornerRadius, minHalf)
        val innerBoxW = (halfW - r).coerceAtLeast(0f)
        val innerBoxH = (halfH - r).coerceAtLeast(0f)
        val gradR = min(r * 1.45f, minHalf)
        val gradBoxW = (halfW - gradR).coerceAtLeast(0f)
        val gradBoxH = (halfH - gradR).coerceAtLeast(0f)
        val refractionHeight = 58f.coerceIn(8f, minHalf)
        val refractionAmount = 64f
        val dispersion = 0.92f
        val invArea = 1f / max(halfW * halfH, 1f)
        val maxSrcX = srcW - 1
        val maxSrcY = srcH - 1

        // Smoked crystal tint + meniscus wash matching Home Pill (FerricLiquidGlass.kt)
        val anchorA = 0.24f
        val anchorR = (0.04f + (Color.red(glassTintArgb) / 255f) * 0.10f) * 255f
        val anchorG = (0.06f + (Color.green(glassTintArgb) / 255f) * 0.10f) * 255f
        val anchorB = (0.10f + (Color.blue(glassTintArgb) / 255f) * 0.10f) * 255f
        val frostA = 0.04f

        for (py in 0 until cardH) {
            val cy = py - halfH
            val syDirect = (py + offY).roundToInt().coerceIn(0, maxSrcY)
            val tY = py.toFloat() / cardH.toFloat()
            val sheenA = when {
                tY < 0.38f -> 0.20f * (1f - tY / 0.38f)
                tY > 0.75f -> 0.08f * ((tY - 0.75f) / 0.25f)
                else -> 0f
            }
            val rowOut = py * cardW

            for (px in 0 until cardW) {
                val cx = px - halfW
                val ax = abs(cx)
                val ay = abs(cy)
                val qx = ax - innerBoxW
                val qy = ay - innerBoxH
                val sd = if (qx > 0f || qy > 0f) {
                    hypot(max(qx, 0f), max(qy, 0f)) - r
                } else {
                    max(qx, qy) - r
                }

                var red: Float
                var green: Float
                var blue: Float

                if (-sd >= refractionHeight) {
                    val sx = (px + offX).roundToInt().coerceIn(0, maxSrcX)
                    val c = srcPixels[syDirect * srcW + sx]
                    red = ((c ushr 16) and 0xFF).toFloat()
                    green = ((c ushr 8) and 0xFF).toFloat()
                    blue = (c and 0xFF).toFloat()
                } else {
                    val depth = (1f - (-min(sd, 0f) / refractionHeight)).coerceIn(0f, 1f)
                    val clampedD = min(depth, 0.9999f)
                    val disp = (1f - sqrt(1f - clampedD * clampedD)) * refractionAmount

                    val gqx = ax - gradBoxW
                    val gqy = ay - gradBoxH
                    var gbx: Float
                    var gby: Float
                    if (gqx >= 0f || gqy >= 0f) {
                        val mx = max(gqx, 0f) + 1e-5f
                        val my = max(gqy, 0f) + 1e-5f
                        val gl = hypot(mx, my)
                        gbx = (if (cx >= 0f) 1f else -1f) * (mx / gl)
                        gby = (if (cy >= 0f) 1f else -1f) * (my / gl)
                    } else {
                        val stepG = if (gqy <= gqx) 1f else 0f
                        gbx = (if (cx >= 0f) 1f else -1f) * stepG
                        gby = (if (cy >= 0f) 1f else -1f) * (1f - stepG)
                    }

                    val clen = hypot(cx, cy).coerceAtLeast(1e-4f)
                    val combX = gbx + (cx / clen) * 0.55f
                    val combY = gby + (cy / clen) * 0.55f
                    val normL = hypot(combX, combY).coerceAtLeast(1e-4f)
                    val gx = combX / normL
                    val gy = combY / normL

                    val refX = (px + offX + disp * gx).roundToInt().coerceIn(0, maxSrcX)
                    val refY = (py + offY + disp * gy).roundToInt().coerceIn(0, maxSrcY)
                    val greenSample = srcPixels[refY * srcW + refX]

                    val cornerDispersion = dispersion * ((cx * cy) * invArea)
                    val chX = gx * (disp * cornerDispersion)
                    val chY = gy * (disp * cornerDispersion)
                    val rX = (px + offX + disp * gx + chX).roundToInt().coerceIn(0, maxSrcX)
                    val rY = (py + offY + disp * gy + chY).roundToInt().coerceIn(0, maxSrcY)
                    val bX = (px + offX + disp * gx - chX).roundToInt().coerceIn(0, maxSrcX)
                    val bY = (py + offY + disp * gy - chY).roundToInt().coerceIn(0, maxSrcY)

                    val redSample = srcPixels[rY * srcW + rX]
                    val blueSample = srcPixels[bY * srcW + bX]

                    red = ((redSample ushr 16) and 0xFF).toFloat()
                    green = ((greenSample ushr 8) and 0xFF).toFloat()
                    blue = (blueSample and 0xFF).toFloat()
                }

                // Clear dark crystal smoke
                red = red * (1f - anchorA) + anchorR * anchorA
                green = green * (1f - anchorA) + anchorG * anchorA
                blue = blue * (1f - anchorA) + anchorB * anchorA

                // Subtle white optical frost veil (#F6F9FF)
                red = red * (1f - frostA) + 246f * frostA
                green = green * (1f - frostA) + 249f * frostA
                blue = blue * (1f - frostA) + 255f * frostA

                // Top meniscus highlight sheen
                if (sheenA > 0f) {
                    red += (255f - red) * sheenA
                    green += (255f - green) * sheenA
                    blue += (255f - blue) * sheenA
                }

                val ri = red.roundToInt().coerceIn(0, 255)
                val gi = green.roundToInt().coerceIn(0, 255)
                val bi = blue.roundToInt().coerceIn(0, 255)
                outPixels[rowOut + px] = (0xFF shl 24) or (ri shl 16) or (gi shl 8) or bi
            }
        }

        return Bitmap.createBitmap(outPixels, cardW, cardH, Bitmap.Config.ARGB_8888)
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
                    // Crisp native cover art behind the glass card so convex-lens edge refraction
                    // bends vivid artwork contours around the glass bezel just like the Home Pill.
                    val scaleW = maxOf(
                        w.toFloat() / artBitmap.width.coerceAtLeast(1),
                        h.toFloat() / artBitmap.height.coerceAtLeast(1),
                    )
                    val drawW = artBitmap.width * scaleW
                    val drawH = artBitmap.height * scaleW
                    val left = (w - drawW) * 0.5f
                    val top = (h - drawH) * 0.5f
                    bgCanvas.drawBitmap(
                        artBitmap,
                        null,
                        RectF(left, top, left + drawW, top + drawH),
                        Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG),
                    )
                } else {
                    bgCanvas.drawColor(Color.rgb(10, 13, 22))
                    val radPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                        shader = RadialGradient(
                            w * 0.45f,
                            h * 0.35f,
                            maxOf(w, h) * 0.72f,
                            intArrayOf(
                                Color.argb(185, Color.red(accent), Color.green(accent), Color.blue(accent)),
                                Color.argb(215, Color.red(orb1), Color.green(orb1), Color.blue(orb1)),
                                Color.rgb(10, 13, 22),
                            ),
                            floatArrayOf(0f, 0.52f, 1f),
                            Shader.TileMode.CLAMP,
                        )
                    }
                    bgCanvas.drawRect(0f, 0f, w.toFloat(), h.toFloat(), radPaint)
                }
                bgPaint.shader = LinearGradient(
                    0f,
                    0f,
                    0f,
                    h.toFloat(),
                    intArrayOf(
                        Color.argb(30, 0, 0, 0),
                        Color.argb(10, 0, 0, 0),
                        Color.argb(72, 0, 0, 0),
                    ),
                    floatArrayOf(0f, 0.45f, 1f),
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

        val isLiquid = style == ShareCardStyle.LIQUID_GLASS
        val titlePaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.WHITE
            textSize = 40f
            this.typeface = typeface
            if (isLiquid) setShadowLayer(6f, 0f, 2f, Color.argb(130, 0, 0, 0))
        }
        val artistPaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.argb(220, 224, 229, 240)
            textSize = 30f
            this.typeface = Typeface.create(typeface, Typeface.NORMAL)
            if (isLiquid) setShadowLayer(5f, 0f, 1.5f, Color.argb(120, 0, 0, 0))
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
            if (isLiquid) setShadowLayer(7f, 0f, 2.5f, Color.argb(138, 0, 0, 0))
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
        val corner = if (isLiquid) 72f else 48f

        val cardPath = android.graphics.Path().apply {
            addRoundRect(cardRect, corner, corner, android.graphics.Path.Direction.CW)
        }

        if (isLiquid) {
            val shadowRect = RectF(cardRect).apply { offset(0f, 14f) }
            val dropShadowPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                color = Color.argb(115, 0, 0, 0)
                maskFilter = BlurMaskFilter(36f, BlurMaskFilter.Blur.NORMAL)
            }
            canvas.drawRoundRect(shadowRect, corner, corner, dropShadowPaint)
        }

        if (backdropBmp != null && isLiquid) {
            val glassCardBmp = buildLiquidGlassCardBitmap(
                backdropBmp = backdropBmp,
                cardRect = cardRect,
                cornerRadius = corner,
                glassTintArgb = shade.glassTint.toArgb(),
            )
            backdropBmp.recycle()
            canvas.withSave {
                clipPath(cardPath)
                drawBitmap(glassCardBmp, null, cardRect, Paint(Paint.FILTER_BITMAP_FLAG))
            }
            glassCardBmp.recycle()
        } else {
            backdropBmp?.recycle()
            val cardFill = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                color = when (style) {
                    ShareCardStyle.AMOLED -> Color.argb(255, 13, 13, 17)
                    ShareCardStyle.LIQUID_GLASS -> Color.argb(185, 18, 22, 34)
                    ShareCardStyle.ADAPTIVE_GRADIENT -> Color.argb(122, 8, 9, 14)
                }
            }
            canvas.drawRoundRect(cardRect, corner, corner, cardFill)
        }

        // Home-Pill directional white top-left catchlight + cool silver bounce rim border
        val cardBorder = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            this.style = Paint.Style.STROKE
            strokeWidth = if (isLiquid) 3.6f else 2.5f
            if (isLiquid) {
                shader = LinearGradient(
                    cardRect.left,
                    cardRect.top,
                    cardRect.right,
                    cardRect.bottom,
                    intArrayOf(
                        Color.argb(212, 255, 255, 255),
                        Color.argb(74, 255, 255, 255),
                        Color.argb(38, 255, 255, 255),
                        Color.argb(68, 235, 242, 255),
                        Color.argb(122, 235, 242, 255),
                    ),
                    floatArrayOf(0f, 0.25f, 0.52f, 0.78f, 1f),
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
            color = if (isLiquid) {
                Color.argb(228, 255, 255, 255)
            } else {
                accent
            }
            textSize = 27f
            letterSpacing = 0.18f
            textAlign = Paint.Align.RIGHT
            this.typeface = Typeface.create(Typeface.MONOSPACE, Typeface.BOLD)
            if (isLiquid) setShadowLayer(4f, 0f, 1.5f, Color.argb(120, 0, 0, 0))
        }
        canvas.drawText("FERRIC", cardRight - 56f, cardRect.bottom - 44f, brandPaint)
    }
}
