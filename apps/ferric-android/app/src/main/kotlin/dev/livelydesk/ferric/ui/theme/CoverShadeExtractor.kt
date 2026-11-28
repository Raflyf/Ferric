package dev.livelydesk.ferric.ui.theme

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Color as AndroidColor
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.core.graphics.ColorUtils
import androidx.palette.graphics.Palette
import coil3.SingletonImageLoader
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import coil3.request.allowHardware
import coil3.toBitmap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Extracts `CoverShade` palette accents and Aurora backdrop colors from the playing cover art
 * using a tiny 96x96 software copy so RAM overhead stays negligible (< 40 KB transient).
 */
object CoverShadeExtractor {
    fun extractFromBitmap(bmp: Bitmap): CoverShadeColors {
        val pal = Palette.from(bmp)
            .maximumColorCount(24)
            .clearFilters()
            .generate()

        val bestChromatic = pal.swatches
            .asSequence()
            .filter { sw ->
                val hsl = sw.hsl
                hsl[1] >= 0.10f && hsl[2] in 0.12f..0.88f
            }
            .maxByOrNull { sw ->
                val hsl = sw.hsl
                sw.population.toFloat() * (0.35f + hsl[1] * 2.6f)
            }
            ?.rgb

        val fallbackRgb = bestChromatic
            ?: pal.vibrantSwatch?.rgb
            ?: pal.lightVibrantSwatch?.rgb
            ?: pal.darkVibrantSwatch?.rgb
            ?: pal.mutedSwatch?.rgb
            ?: pal.dominantSwatch?.rgb

        val darkVibrant = pal.darkVibrantSwatch?.rgb
            ?: pal.darkMutedSwatch?.rgb
            ?: pal.dominantSwatch?.rgb
        val muted = pal.mutedSwatch?.rgb
            ?: pal.darkMutedSwatch?.rgb
            ?: darkVibrant

        val accentColor = if (fallbackRgb != null) {
            val hsl = FloatArray(3)
            ColorUtils.colorToHSL(fallbackRgb, hsl)
            if (hsl[1] < 0.08f) {
                // True monochrome / B&W album cover: pick a soft warm/cool neutral silver tone
                // instead of hardcoded orange so "From the cover" visibly reflects neutral covers.
                val r = AndroidColor.red(fallbackRgb)
                val b = AndroidColor.blue(fallbackRgb)
                if (r >= b + 4) Color(0xFFEBE4DA) else Color(0xFFDCE4EE)
            } else {
                hsl[1] = hsl[1].coerceAtLeast(0.42f).coerceAtMost(0.92f)
                hsl[2] = hsl[2].coerceIn(0.48f, 0.72f)
                Color(ColorUtils.HSLToColor(hsl))
            }
        } else {
            FerricOrange
        }

        val orb1 = darkVibrant?.let { Color(it) } ?: Color(0xFF4A2012)
        val orb2 = muted?.let { Color(it) } ?: Color(0xFF1A1F38)
        val orb3 = accentColor.copy(alpha = 0.55f)
        val glassTint = Color(
            red = (0.04f + accentColor.red * 0.16f).coerceIn(0.03f, 0.24f),
            green = (0.04f + accentColor.green * 0.16f).coerceIn(0.03f, 0.24f),
            blue = (0.06f + accentColor.blue * 0.18f).coerceIn(0.04f, 0.28f),
            alpha = 1f,
        )
        return CoverShadeColors(
            accent = accentColor,
            orb1 = orb1,
            orb2 = orb2,
            orb3 = orb3,
            glassTint = glassTint,
        )
    }

    suspend fun extract(context: Context, artworkUrl: String?): CoverShadeColors =
        withContext(Dispatchers.IO) {
            if (artworkUrl.isNullOrBlank()) return@withContext CoverShadeColors()
            runCatching {
                val loader = SingletonImageLoader.get(context)
                val req = ImageRequest.Builder(context)
                    .data(artworkUrl)
                    .size(96)
                    .allowHardware(false)
                    .build()
                val res = loader.execute(req) as? SuccessResult ?: return@withContext CoverShadeColors()
                val bmp: Bitmap = res.image.toBitmap(96, 96)
                extractFromBitmap(bmp)
            }.getOrElse { CoverShadeColors() }
        }
}
