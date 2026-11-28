package dev.livelydesk.ferric.ui.theme

import androidx.compose.animation.core.CubicBezierEasing
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
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.chrisbanes.haze.HazeState
import dev.livelydesk.ferric.R
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
    val glassOpacity: Float = 0.62f,
    val glassTint: Color = Color(0xFF10131D),
    val cornerRadiusDp: Dp = 18.dp,
    val editorialMode: Boolean = false,
    val fontFamilyName: String = "DM Sans",
    val playerLayout: String = "liquid_glass",
)

val FerricEaseOut = CubicBezierEasing(0.25f, 0.08f, 0.18f, 1.0f)
const val FERRIC_MOTION_MS = 300

val LocalFerricPalette = staticCompositionLocalOf { FerricVisualPalette() }
val LocalHazeState = compositionLocalOf<HazeState?> { null }

@OptIn(ExperimentalTextApi::class)
private fun variableFontFamily(resId: Int): FontFamily = FontFamily(
    Font(
        resId = resId,
        weight = FontWeight.Light,
        variationSettings = FontVariation.Settings(FontVariation.weight(300)),
    ),
    Font(
        resId = resId,
        weight = FontWeight.Normal,
        variationSettings = FontVariation.Settings(FontVariation.weight(400)),
    ),
    Font(
        resId = resId,
        weight = FontWeight.Medium,
        variationSettings = FontVariation.Settings(FontVariation.weight(500)),
    ),
    Font(
        resId = resId,
        weight = FontWeight.SemiBold,
        variationSettings = FontVariation.Settings(FontVariation.weight(600)),
    ),
    Font(
        resId = resId,
        weight = FontWeight.Bold,
        variationSettings = FontVariation.Settings(FontVariation.weight(700)),
    ),
)

private val DmSansFamily: FontFamily by lazy { variableFontFamily(R.font.dm_sans) }
private val FigtreeFamily: FontFamily by lazy { variableFontFamily(R.font.figtree) }
private val SpaceGroteskFamily: FontFamily by lazy { variableFontFamily(R.font.space_grotesk) }
private val JetBrainsMonoFamily: FontFamily by lazy { variableFontFamily(R.font.jetbrains_mono) }

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
 * Adjust accent luminance until WCAG AA+ (>= 5.2:1) holds against [background].
 */
fun ensureWcagAccent(accent: Color, background: Color, isLightBg: Boolean): Color {
    if (contrastRatio(accent, background) >= 5.2) return accent
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
        if (contrastRatio(candidate, background) >= 5.2) return candidate
    }
    return if (isLightBg) Color(0xFFB34014) else Color(0xFFFF884D)
}

fun displayUiFontName(raw: String?): String = when (raw?.trim()?.lowercase()) {
    "figtree", "plus jakarta sans" -> "Figtree"
    "space grotesk" -> "Space Grotesk"
    "jetbrains mono", "mono" -> "JetBrains Mono"
    else -> "DM Sans"
}

fun buildVisualPalette(
    theme: FfiThemeConfig?,
    coverShade: CoverShadeColors,
): FerricVisualPalette {
    if (theme == null) return FerricVisualPalette()
    val isLight = theme.backgroundMode == FfiBackgroundMode.WARM_CREAM
    val isLiquidGlass = theme.backgroundMode == FfiBackgroundMode.NATIVE_ACRYLIC || theme.acrylicEnabled
    val canvas = parseHexColor(theme.canvasHex, if (isLight) Color(0xFFF3EEE3) else AmoledBlack)
    val baseSurface = parseHexColor(theme.surfaceHex, if (isLight) Color(0xFFEAE3D4) else ObsidianSurface)
    val surface = if (isLiquidGlass && !isLight) Color(0x52121622) else baseSurface
    val elevated = when {
        isLight -> Color(0xFFE0D7C5)
        isLiquidGlass -> Color(0x70181E2C)
        else -> ObsidianElevated
    }
    val border = if (isLiquidGlass && !isLight) {
        Color.White.copy(alpha = 0.22f)
    } else {
        parseHexColor(theme.borderHex, if (isLight) Color(0xFFD5CCB9) else HairlineBorder)
    }
    val textPrimary = if (isLiquidGlass && !isLight) {
        Color(0xFFFFFFFF)
    } else {
        parseHexColor(theme.textPrimaryHex, if (isLight) Color(0xFF111111) else TextPrimary)
    }
    val rawSecondary = parseHexColor(theme.textSecondaryHex, if (isLight) Color(0xFF524B42) else TextSecondary)
    val textSecondary = if (isLiquidGlass && !isLight && rawSecondary.luminance() < 0.50f) {
        Color(0xFFC8D2E3)
    } else {
        rawSecondary
    }
    val baseAccent = parseHexColor(theme.accentHex, FerricOrange)
    val chosenAccent = if (theme.adaptiveAlbumArt && !isLight) coverShade.accent else baseAccent
    val safeAccent = ensureWcagAccent(chosenAccent, elevated, isLight)
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
        glassOpacity = theme.acrylicOpacity.coerceIn(0.15f, 0.95f),
        glassTint = glassTint,
        cornerRadiusDp = theme.cornerRadiusPx.coerceIn(0f, 28f).dp,
        editorialMode = theme.editorialMode,
        fontFamilyName = theme.fontFamily,
        playerLayout = theme.playerLayout,
    )
}

fun resolveUiFontFamily(name: String): FontFamily = when (name.trim().lowercase()) {
    "figtree", "plus jakarta sans" -> FigtreeFamily
    "space grotesk" -> SpaceGroteskFamily
    "jetbrains mono", "mono" -> JetBrainsMonoFamily
    "editorial", "serif" -> FontFamily.Serif
    else -> DmSansFamily
}

fun buildFerricTypography(fontName: String, editorialMode: Boolean): Typography {
    val bodyFamily = resolveUiFontFamily(fontName)
    val headlineFamily = if (editorialMode) FontFamily.Serif else bodyFamily
    return Typography(
        displayLarge = TextStyle(
            fontFamily = headlineFamily,
            fontWeight = if (editorialMode) FontWeight.Normal else FontWeight.Light,
            fontSize = 40.sp,
            lineHeight = 46.sp,
            letterSpacing = if (editorialMode) (-0.8).sp else (-0.5).sp,
        ),
        displayMedium = TextStyle(
            fontFamily = headlineFamily,
            fontWeight = if (editorialMode) FontWeight.Normal else FontWeight.Light,
            fontSize = 34.sp,
            lineHeight = 40.sp,
            letterSpacing = if (editorialMode) (-0.7).sp else (-0.4).sp,
        ),
        displaySmall = TextStyle(
            fontFamily = headlineFamily,
            fontWeight = if (editorialMode) FontWeight.Normal else FontWeight.Light,
            fontSize = if (editorialMode) 32.sp else 30.sp,
            lineHeight = 38.sp,
            letterSpacing = if (editorialMode) (-0.7).sp else (-0.4).sp,
        ),
        headlineLarge = TextStyle(
            fontFamily = headlineFamily,
            fontWeight = if (editorialMode) FontWeight.Medium else FontWeight.SemiBold,
            fontSize = 30.sp,
            lineHeight = 36.sp,
            letterSpacing = (-0.4).sp,
        ),
        headlineMedium = TextStyle(
            fontFamily = headlineFamily,
            fontWeight = if (editorialMode) FontWeight.Medium else FontWeight.SemiBold,
            fontSize = if (editorialMode) 28.sp else 26.sp,
            lineHeight = 32.sp,
            letterSpacing = (-0.35).sp,
        ),
        headlineSmall = TextStyle(
            fontFamily = headlineFamily,
            fontWeight = FontWeight.SemiBold,
            fontSize = 21.sp,
            lineHeight = 27.sp,
            letterSpacing = (-0.25).sp,
        ),
        titleLarge = TextStyle(
            fontFamily = headlineFamily,
            fontWeight = if (editorialMode) FontWeight.Medium else FontWeight.SemiBold,
            fontSize = if (editorialMode) 21.sp else 19.sp,
            lineHeight = 25.sp,
            letterSpacing = if (editorialMode) (-0.3).sp else (-0.2).sp,
        ),
        titleMedium = TextStyle(
            fontFamily = if (editorialMode) headlineFamily else bodyFamily,
            fontWeight = FontWeight.Medium,
            fontSize = 15.sp,
            lineHeight = 20.sp,
            letterSpacing = (-0.1).sp,
        ),
        titleSmall = TextStyle(
            fontFamily = bodyFamily,
            fontWeight = FontWeight.Medium,
            fontSize = 14.sp,
            lineHeight = 19.sp,
        ),
        bodyLarge = TextStyle(
            fontFamily = bodyFamily,
            fontWeight = FontWeight.Normal,
            fontSize = 15.sp,
            lineHeight = 21.sp,
        ),
        bodyMedium = TextStyle(
            fontFamily = bodyFamily,
            fontWeight = FontWeight.Normal,
            fontSize = 13.sp,
            lineHeight = 18.sp,
        ),
        bodySmall = TextStyle(
            fontFamily = bodyFamily,
            fontWeight = FontWeight.Normal,
            fontSize = 12.sp,
            lineHeight = 16.sp,
        ),
        labelLarge = TextStyle(
            fontFamily = bodyFamily,
            fontWeight = FontWeight.SemiBold,
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
            fontFamily = bodyFamily,
            fontWeight = FontWeight.Medium,
            fontSize = 11.sp,
            lineHeight = 14.sp,
        ),
    )
}

fun ferricDialogContainerColor(palette: FerricVisualPalette): Color =
    if (palette.isLiquidGlass && !palette.isLight) Color(0xE0131824) else palette.elevated

/**
 * Applies Liquid Glass optical blur + luminous frosted finish when Liquid Glass mode is active,
 * or clean solid surface styling otherwise.
 */
fun Modifier.ferricGlassSurface(
    palette: FerricVisualPalette,
    @Suppress("UNUSED_PARAMETER") hazeState: HazeState? = null,
    cornerRadius: Dp = palette.cornerRadiusDp,
    tier: GlassTier = GlassTier.FROSTED_PANEL,
    accentGlow: Color = Color.Unspecified,
    fallbackColor: Color = palette.surface,
): Modifier {
    return if (palette.isLiquidGlass) {
        this.ferricLiquidGlass(
            cornerRadius = cornerRadius,
            blurRadius = 14.dp,
            refractionHeight = 14.dp,
            refractionAmount = if (tier == GlassTier.HERO_LENS) 12.dp else 0.dp,
            chromaticDispersion = if (tier == GlassTier.HERO_LENS) 0.45f else 0f,
            tier = tier,
            accentGlow = accentGlow,
        )
    } else {
        val shape = RoundedCornerShape(cornerRadius)
        this
            .clip(shape)
            .background(fallbackColor)
            .border(width = 1.dp, color = palette.border, shape = shape)
    }
}

@Composable
fun FerricTheme(
    themeConfig: FfiThemeConfig? = null,
    coverShade: CoverShadeColors = CoverShadeColors(),
    hazeState: HazeState? = null,
    backdropState: FerricBackdropState? = null,
    content: @Composable () -> Unit,
) {
    val effectiveCoverShade = if (
        themeConfig != null &&
        themeConfig.backgroundMode != FfiBackgroundMode.WARM_CREAM &&
        (themeConfig.adaptiveAlbumArt || themeConfig.adaptiveAcrylicTint)
    ) {
        coverShade
    } else {
        null
    }
    val palette = remember(themeConfig, effectiveCoverShade) {
        buildVisualPalette(themeConfig, effectiveCoverShade ?: CoverShadeColors())
    }
    val scheme = remember(palette) {
        if (palette.isLight) {
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
    }
    val typography = remember(palette.fontFamilyName, palette.editorialMode) {
        buildFerricTypography(palette.fontFamilyName, palette.editorialMode)
    }

    CompositionLocalProvider(
        LocalFerricPalette provides palette,
        LocalHazeState provides hazeState,
        LocalBackdropState provides backdropState,
    ) {
        MaterialTheme(
            colorScheme = scheme,
            typography = typography,
            content = content,
        )
    }
}
