package dev.livelydesk.ferric.ui.theme

import android.content.Context
import android.graphics.Bitmap
import androidx.compose.ui.graphics.Color
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
                val pal = Palette.from(bmp).maximumColorCount(12).generate()

                val vibrant = pal.vibrantSwatch?.rgb
                    ?: pal.lightVibrantSwatch?.rgb
                    ?: pal.dominantSwatch?.rgb
                val darkVibrant = pal.darkVibrantSwatch?.rgb
                    ?: pal.darkMutedSwatch?.rgb
                    ?: pal.dominantSwatch?.rgb
                val muted = pal.mutedSwatch?.rgb
                    ?: pal.darkMutedSwatch?.rgb
                    ?: darkVibrant

                val accentColor = vibrant?.let { Color(it) } ?: FerricOrange
                val orb1 = darkVibrant?.let { Color(it) } ?: Color(0xFF4A2012)
                val orb2 = muted?.let { Color(it) } ?: Color(0xFF1A1F38)
                val orb3 = vibrant?.let { Color(it).copy(alpha = 0.55f) } ?: Color(0xFF28172E)
                val glassTint = Color(
                    red = (0.04f + accentColor.red * 0.16f).coerceIn(0.03f, 0.24f),
                    green = (0.04f + accentColor.green * 0.16f).coerceIn(0.03f, 0.24f),
                    blue = (0.06f + accentColor.blue * 0.18f).coerceIn(0.04f, 0.28f),
                    alpha = 1f,
                )
                CoverShadeColors(
                    accent = accentColor,
                    orb1 = orb1,
                    orb2 = orb2,
                    orb3 = orb3,
                    glassTint = glassTint,
                )
            }.getOrElse { CoverShadeColors() }
        }
}
