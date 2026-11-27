package dev.livelydesk.ferric.ui.theme

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.chrisbanes.haze.HazeState
import dev.chrisbanes.haze.HazeStyle
import dev.chrisbanes.haze.HazeTint
import dev.chrisbanes.haze.hazeEffect
import uniffi.ferric_mobile_ffi.FfiBackgroundMode
import uniffi.ferric_mobile_ffi.FfiThemeConfig

@Immutable
data class CoverShadeColors(
    val accent: Color = FerricOrange,
    val orb1: Color = Color(0xFF4A2012),
    val orb2: Color = Color(0xFF1A1F38),
    val orb3: Color = Color(0xFF28172E),
    val glassTint: Color = Color(0xFF10131D),
)

@Immutable
data class FerricVisualPalette(
    val canvas: Color = AmoledBlack,
    val surface: Color = ObsidianSurface,
    val elevated: Color = ObsidianElevated,
    val border: Color = HairlineBorder,
    val textPrimary: Color = TextPrimary,
    val textSecondary: Color = TextSecondary,
    val accent: Color = FerricOrange,
    val isLight: Boolean = false,
    val isLiquidGlass: Boolean = false,
    val glassOpacity: Float = 0.72f,
    val glassTint: Color = Color(0xFF10131D),
    val cornerRadiusDp: Dp = 12.dp,
    val editorialMode: Boolean = false,
    val fontFamilyName: String = "Inter",
    val playerLayout: String = "standard",
)

val LocalFerricPalette = staticCompositionLocalOf { FerricVisualPalette() }
val LocalHazeState = compositionLocalOf<HazeState?> { null }

fun parseHexColor(hex: String, fallback: Color): Color {
    val clean = hex.trim().removePrefix("#")
    val parsed = clean.toLongOrNull(16) ?: return fallback
    return when (clean.length) {
        6 -> Color((0xFF000000L or parsed).toInt())
        8 -> Color(parsed.toInt())
        else -> fallback
    }
}

private fun contrastRatio(fg: Color, bg: Color): Double {
    val l1 = fg.luminance().toDouble()
    val l2 = bg.luminance().toDouble()
    val hi = maxOf(l1, l2)
    val lo = minOf(l1, l2)
    return (hi + 0.05) / (lo + 0.05)
}

/**
 * Adjust accent luminance until WCAG AA (>= 4.5:1) holds against `background`.
 */
fun ensureWcagAccent(accent: Color, background: Color, isLightBg: Boolean): Color {
    if (contrastRatio(accent, background) >= 4.5) return accent
    val target = if (isLightBg) Color(0xFF1A120E) else Color(0xFFFFF5EE)
    var candidate = accent
    for (step in 1..10) {
        val t = step * 0.09f
        candidate = Color(
            red = candidate.red + (target.red - candidate.red) * t,
            green = candidate.green + (target.green - candidate.green) * t,
            blue = candidate.blue + (target.blue - candidate.blue) * t,
            alpha = 1f,
        )
        if (contrastRatio(candidate, background) >= 4.5) return candidate
    }
    return if (isLightBg) Color(0xFFB34014) else Color(0xFFFF884D)
}

fun buildVisualPalette(
    theme: FfiThemeConfig?,
    coverShade: CoverShadeColors,
): FerricVisualPalette {
    if (theme == null) return FerricVisualPalette()
    val isLight = theme.backgroundMode == FfiBackgroundMode.WARM_CREAM
    val isLiquidGlass = theme.backgroundMode == FfiBackgroundMode.NATIVE_ACRYLIC || theme.acrylicEnabled
    val canvas = parseHexColor(theme.canvasHex, if (isLight) Color(0xFFF3EEE3) else AmoledBlack)
    val surface = parseHexColor(theme.surfaceHex, if (isLight) Color(0xFFEAE3D4) else ObsidianSurface)
    val elevated = if (isLight) Color(0xFFE0D7C5) else ObsidianElevated
    val border = parseHexColor(theme.borderHex, if (isLight) Color(0xFFD5CCB9) else HairlineBorder)
    val textPrimary = parseHexColor(theme.textPrimaryHex, if (isLight) Color(0xFF111111) else TextPrimary)
    val textSecondary = parseHexColor(theme.textSecondaryHex, if (isLight) Color(0xFF524B42) else TextSecondary)
    val baseAccent = parseHexColor(theme.accentHex, FerricOrange)
    val chosenAccent = if (theme.adaptiveAlbumArt && !isLight) coverShade.accent else baseAccent
    val safeAccent = ensureWcagAccent(chosenAccent, canvas, isLight)
    val glassTint = if (theme.adaptiveAcrylicTint && !isLight) {
        coverShade.glassTint
    } else {
        parseHexColor(theme.acrylicTintHex, Color(0xFF10131D))
    }

    return FerricVisualPalette(
        canvas = canvas,
        surface = surface,
        elevated = elevated,
        border = border,
        textPrimary = textPrimary,
        textSecondary = textSecondary,
        accent = safeAccent,
        isLight = isLight,
        isLiquidGlass = isLiquidGlass,
        glassOpacity = theme.acrylicOpacity.coerceIn(0.25f, 0.92f),
        glassTint = glassTint,
        cornerRadiusDp = theme.cornerRadiusPx.coerceIn(8f, 26f).dp,
        editorialMode = theme.editorialMode,
        fontFamilyName = theme.fontFamily,
        playerLayout = theme.playerLayout,
    )
}

fun resolveUiFontFamily(name: String): FontFamily = when (name.trim().lowercase()) {
    "jetbrains mono", "mono" -> FontFamily.Monospace
    "editorial", "serif" -> FontFamily.Serif
    else -> FontFamily.SansSerif
}

fun buildFerricTypography(fontName: String, editorialMode: Boolean): Typography {
    val bodyFamily = resolveUiFontFamily(fontName)
    val headlineFamily = if (editorialMode) FontFamily.Serif else bodyFamily
    val headlineSize = if (editorialMode) 28.sp else 24.sp
    return Typography(
        headlineMedium = TextStyle(
            fontFamily = headlineFamily,
            fontWeight = if (editorialMode) FontWeight.SemiBold else FontWeight.Bold,
            fontSize = headlineSize,
            lineHeight = (headlineSize.value + 6f).sp,
            letterSpacing = if (editorialMode) (-0.5).sp else (-0.2).sp,
        ),
        headlineSmall = TextStyle(
            fontFamily = headlineFamily,
            fontWeight = FontWeight.Bold,
            fontSize = 21.sp,
            lineHeight = 27.sp,
            letterSpacing = (-0.2).sp,
        ),
        titleLarge = TextStyle(
            fontFamily = headlineFamily,
            fontWeight = FontWeight.SemiBold,
            fontSize = if (editorialMode) 20.sp else 18.sp,
            lineHeight = 24.sp,
        ),
        titleMedium = TextStyle(
            fontFamily = bodyFamily,
            fontWeight = FontWeight.SemiBold,
            fontSize = 15.sp,
            lineHeight = 20.sp,
        ),
        bodyMedium = TextStyle(
            fontFamily = bodyFamily,
            fontWeight = FontWeight.Normal,
            fontSize = 13.sp,
            lineHeight = 18.sp,
        ),
        labelMedium = TextStyle(
            fontFamily = bodyFamily,
            fontWeight = FontWeight.Medium,
            fontSize = 12.sp,
            lineHeight = 16.sp,
        ),
        labelSmall = TextStyle(
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Medium,
            fontSize = 11.sp,
            lineHeight = 14.sp,
        ),
    )
}

/**
 * Applies Liquid Glass real-time frosted backdrop (via Haze 1.7.2 with automatic RenderEffect on
 * Android 12+ and graceful frosted fallback on Android 9-11) plus a 1dp specular top-edge sheen.
 */
fun Modifier.ferricGlassSurface(
    palette: FerricVisualPalette,
    hazeState: HazeState?,
    cornerRadius: Dp = palette.cornerRadiusDp,
): Modifier = composed {
    val shape = RoundedCornerShape(cornerRadius)
    if (palette.isLiquidGlass) {
        val specularBorder = Brush.verticalGradient(
            colors = listOf(
                Color.White.copy(alpha = if (palette.isLight) 0.55f else 0.24f),
                palette.border.copy(alpha = 0.25f),
            ),
        )
        val base = this.clip(shape)
        val blurred = if (hazeState != null) {
            base.hazeEffect(
                state = hazeState,
                style = HazeStyle(
                    backgroundColor = palette.glassTint,
                    tint = HazeTint(palette.glassTint.copy(alpha = (palette.glassOpacity * 0.65f).coerceIn(0.22f, 0.72f))),
                    blurRadius = 24.dp,
                    noiseFactor = 0.06f,
                ),
            )
        } else {
            base.background(palette.glassTint.copy(alpha = palette.glassOpacity.coerceIn(0.45f, 0.88f)))
        }
        blurred.border(width = 1.dp, brush = specularBorder, shape = shape)
    } else {
        this
            .clip(shape)
            .background(palette.surface)
            .border(width = 1.dp, color = palette.border, shape = shape)
    }
}

@Composable
fun FerricTheme(
    themeConfig: FfiThemeConfig? = null,
    coverShade: CoverShadeColors = CoverShadeColors(),
    hazeState: HazeState? = null,
    content: @Composable () -> Unit,
) {
    val palette = buildVisualPalette(themeConfig, coverShade)
    val scheme = if (palette.isLight) {
        lightColorScheme(
            primary = palette.accent,
            onPrimary = Color.White,
            secondary = palette.accent,
            onSecondary = Color.White,
            background = palette.canvas,
            onBackground = palette.textPrimary,
            surface = palette.surface,
            onSurface = palette.textPrimary,
            surfaceVariant = palette.elevated,
            onSurfaceVariant = palette.textSecondary,
            outline = palette.border,
        )
    } else {
        darkColorScheme(
            primary = palette.accent,
            onPrimary = AmoledBlack,
            secondary = FerricOrangeDark,
            onSecondary = palette.textPrimary,
            background = palette.canvas,
            onBackground = palette.textPrimary,
            surface = palette.surface,
            onSurface = palette.textPrimary,
            surfaceVariant = palette.elevated,
            onSurfaceVariant = palette.textSecondary,
            outline = palette.border,
        )
    }

    CompositionLocalProvider(
        LocalFerricPalette provides palette,
        LocalHazeState provides hazeState,
    ) {
        MaterialTheme(
            colorScheme = scheme,
            typography = buildFerricTypography(palette.fontFamilyName, palette.editorialMode),
            content = content,
        )
    }
}
