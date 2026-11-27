package dev.livelydesk.ferric.ui.theme

import android.graphics.ColorMatrix
import android.graphics.ColorMatrixColorFilter
import android.graphics.RenderEffect
import android.graphics.RuntimeShader
import android.graphics.Shader
import android.os.Build
import androidx.annotation.RequiresApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.graphics.asComposeRenderEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import kotlin.math.min
import kotlin.math.roundToInt

/**
 * Shared hardware backdrop capture state for real-time optical Liquid Glass.
 * Records the live scrolling screen layer once per frame into a GPU [GraphicsLayer] so floating
 * glass capsules (dock, mini-player, pills, player controls) optically blur, vibrate, refract,
 * and chromatically disperse whatever artwork or text glides directly behind them.
 */
@Stable
class FerricBackdropState(
    val graphicsLayer: GraphicsLayer,
) {
    var rootOffset: Offset = Offset.Zero
    var hasRecorded: Boolean = false
}

val LocalBackdropState = compositionLocalOf<FerricBackdropState?> { null }

@Composable
fun rememberFerricBackdropState(): FerricBackdropState {
    val layer = rememberGraphicsLayer()
    return remember(layer) { FerricBackdropState(layer) }
}

/**
 * Captures the underlying scrollable surface into [backdrop] so sibling overlays above it can
 * render genuine optical Liquid Glass refraction over live scrolling content.
 */
fun Modifier.ferricBackdropSource(backdrop: FerricBackdropState): Modifier =
    this
        .onGloballyPositioned { coords ->
            backdrop.rootOffset = coords.positionInRoot()
        }
        .drawWithContent {
            if (size.width >= 1f && size.height >= 1f) {
                backdrop.graphicsLayer.record {
                    this@drawWithContent.drawContent()
                }
                backdrop.hasRecorded = true
                drawLayer(backdrop.graphicsLayer)
            } else {
                drawContent()
            }
        }

// Optical convex-lens bevel refraction + subtle RGB chromatic dispersion AGSL shader (Android 13+).
private const val REFRACTION_DISPERSION_AGSL = """
uniform shader content;
uniform float2 fullSize;
uniform float2 halfSize;
uniform float minHalf;
uniform float cornerRadius;
uniform float refractionHeight;
uniform float refractionAmount;
uniform float depthEffect;
uniform float chromaticAberration;

float sdRoundedRect(float2 p, float2 b, float r) {
    float2 q = abs(p) - (b - float2(r));
    return length(max(q, 0.0)) - r + min(max(q.x, q.y), 0.0);
}

float2 gradSdRoundedRect(float2 p, float2 b, float r) {
    float2 q = abs(p) - (b - float2(r));
    if (q.x >= 0.0 || q.y >= 0.0) {
        return sign(p) * normalize(max(q, 0.0) + float2(1e-5));
    } else {
        float gx = step(q.y, q.x);
        return sign(p) * float2(gx, 1.0 - gx);
    }
}

float lensCurve(float x) {
    float clamped = clamp(x, 0.0, 0.9999);
    return 1.0 - sqrt(1.0 - clamped * clamped);
}

half4 main(float2 coord) {
    float2 centered = coord - halfSize;
    float r = min(cornerRadius, minHalf);
    float sd = sdRoundedRect(centered, halfSize, r);
    if (-sd >= refractionHeight) {
        return content.eval(coord);
    }
    float depth = clamp(1.0 - (-min(sd, 0.0) / max(refractionHeight, 1.0)), 0.0, 1.0);
    float disp = lensCurve(depth) * refractionAmount;
    float gradR = min(r * 1.45, minHalf);
    float2 gBase = gradSdRoundedRect(centered, halfSize, gradR);
    float2 gDir = depthEffect > 0.5
        ? normalize(gBase + normalize(centered + float2(1e-4)) * 0.55)
        : gBase;

    float2 safeMax = max(fullSize - float2(1.0), float2(1.0));
    float2 refrCoord = clamp(coord - disp * gDir, float2(0.5), safeMax);

    if (chromaticAberration <= 0.001) {
        return content.eval(refrCoord);
    }
    float edgeFactor = depth * chromaticAberration;
    float2 chromaDelta = gDir * (disp * edgeFactor * 0.55);
    float2 redCoord = clamp(refrCoord + chromaDelta, float2(0.5), safeMax);
    float2 blueCoord = clamp(refrCoord - chromaDelta, float2(0.5), safeMax);

    half4 baseSample = content.eval(refrCoord);
    half4 redSample = content.eval(redCoord);
    half4 blueSample = content.eval(blueCoord);
    return half4(redSample.r, baseSample.g, blueSample.b, baseSample.a);
}
"""

// Directional optical glass specular rim lighting AGSL shader (top-left bright rim + bottom-right counter bounce).
private const val SPECULAR_RIM_AGSL = """
uniform float2 halfSize;
uniform float minHalf;
uniform float cornerRadius;
uniform float2 lightDir;
uniform float falloff;
uniform float4 topColor;
uniform float4 bottomColor;

float2 gradSdRoundedRect(float2 p, float2 b, float r) {
    float2 q = abs(p) - (b - float2(r));
    if (q.x >= 0.0 || q.y >= 0.0) {
        return sign(p) * normalize(max(q, 0.0) + float2(1e-5));
    } else {
        float gx = step(q.y, q.x);
        return sign(p) * float2(gx, 1.0 - gx);
    }
}

half4 main(float2 coord) {
    float2 centered = coord - halfSize;
    float r = min(cornerRadius * 1.35, minHalf);
    float2 g = gradSdRoundedRect(centered, halfSize, r);
    float d = dot(g, lightDir);
    float intensity = pow(abs(d), falloff);
    float4 rim = d >= 0.0 ? topColor : bottomColor;
    float baseRim = 0.14;
    float a = clamp(rim.a * (baseRim + intensity * 0.86), 0.0, 1.0);
    return half4(rim.rgb * a, a);
}
"""

private class GlassCache {
    var elementOffset: Offset = Offset.Zero
    var lastSize: IntSize = IntSize.Zero
    var lastBlurPx: Float = -1f
    var lastRadiusPx: Float = -1f
    var lastLightMode: Boolean = false
    var lastDispersion: Float = -1f
    var lastAccent: Color = Color.Unspecified
    var cachedEffect: androidx.compose.ui.graphics.RenderEffect? = null
    var refractionShader: Any? = null
    var rimShader: Any? = null
    var cachedRimBrush: ShaderBrush? = null
    var cachedPath: Path = Path()
    var cachedPathSize: Size = Size.Zero
    var cachedPathRadius: Float = -1f
}

@RequiresApi(Build.VERSION_CODES.S)
private fun buildVibrancyEffect(isLight: Boolean): RenderEffect {
    val sat = if (isLight) 1.24f else 1.42f
    val matrix = ColorMatrix().apply {
        setSaturation(sat)
    }
    // Subtle contrast/luminance balance so refracted covers look luminous rather than grey.
    val gain = if (isLight) 1.04f else 1.08f
    val bias = if (isLight) 6f else 4f
    val lift = ColorMatrix(
        floatArrayOf(
            gain, 0f, 0f, 0f, bias,
            0f, gain, 0f, 0f, bias,
            0f, 0f, gain, 0f, bias,
            0f, 0f, 0f, 1f, 0f,
        ),
    )
    matrix.postConcat(lift)
    return RenderEffect.createColorFilterEffect(ColorMatrixColorFilter(matrix))
}

@RequiresApi(Build.VERSION_CODES.TIRAMISU)
private fun buildApi33LiquidGlassEffect(
    cache: GlassCache,
    widthPx: Float,
    heightPx: Float,
    radiusPx: Float,
    blurPx: Float,
    refractionHeightPx: Float,
    refractionAmountPx: Float,
    dispersion: Float,
    isLight: Boolean,
): androidx.compose.ui.graphics.RenderEffect {
    val shader = (cache.refractionShader as? RuntimeShader)
        ?: RuntimeShader(REFRACTION_DISPERSION_AGSL).also { cache.refractionShader = it }
    val halfW = (widthPx * 0.5f).coerceAtLeast(1f)
    val halfH = (heightPx * 0.5f).coerceAtLeast(1f)
    val minHalf = min(halfW, halfH)
    shader.setFloatUniform("fullSize", widthPx, heightPx)
    shader.setFloatUniform("halfSize", halfW, halfH)
    shader.setFloatUniform("minHalf", minHalf)
    shader.setFloatUniform("cornerRadius", radiusPx.coerceAtMost(minHalf))
    shader.setFloatUniform("refractionHeight", refractionHeightPx.coerceIn(4f, minHalf))
    shader.setFloatUniform("refractionAmount", refractionAmountPx.coerceAtLeast(0f))
    shader.setFloatUniform("depthEffect", 1.0f)
    shader.setFloatUniform("chromaticAberration", dispersion.coerceIn(0f, 1.0f))

    val vibrancy = buildVibrancyEffect(isLight)
    val blur = RenderEffect.createBlurEffect(
        blurPx.coerceAtLeast(1f),
        blurPx.coerceAtLeast(1f),
        Shader.TileMode.CLAMP,
    )
    val frosted = RenderEffect.createChainEffect(blur, vibrancy)
    val refracted = RenderEffect.createRuntimeShaderEffect(shader, "content")
    return RenderEffect.createChainEffect(refracted, frosted).asComposeRenderEffect()
}

@RequiresApi(Build.VERSION_CODES.TIRAMISU)
private fun buildApi33RimBrush(
    cache: GlassCache,
    widthPx: Float,
    heightPx: Float,
    radiusPx: Float,
    isLight: Boolean,
    accentHint: Color,
): ShaderBrush {
    val shader = (cache.rimShader as? RuntimeShader)
        ?: RuntimeShader(SPECULAR_RIM_AGSL).also { cache.rimShader = it }
    val halfW = (widthPx * 0.5f).coerceAtLeast(1f)
    val halfH = (heightPx * 0.5f).coerceAtLeast(1f)
    val minHalf = min(halfW, halfH)
    shader.setFloatUniform("halfSize", halfW, halfH)
    shader.setFloatUniform("minHalf", minHalf)
    shader.setFloatUniform("cornerRadius", radiusPx.coerceAtMost(minHalf))
    // Top-left directional specular light (classic iOS 26 / Auralys glass bevel angle).
    shader.setFloatUniform("lightDir", -0.56f, -0.83f)
    shader.setFloatUniform("falloff", 1.55f)
    if (isLight) {
        shader.setFloatUniform("topColor", 1.0f, 1.0f, 1.0f, 0.88f)
        shader.setFloatUniform("bottomColor", 0.30f, 0.32f, 0.38f, 0.24f)
    } else {
        // Luminous white-chrome upper rim + subtly warm lower bounce rim.
        val br = (0.78f + accentHint.red * 0.22f).coerceIn(0f, 1f)
        val bg = (0.80f + accentHint.green * 0.20f).coerceIn(0f, 1f)
        val bb = (0.85f + accentHint.blue * 0.15f).coerceIn(0f, 1f)
        shader.setFloatUniform("topColor", 1.0f, 1.0f, 1.0f, 0.54f)
        shader.setFloatUniform("bottomColor", br, bg, bb, 0.24f)
    }
    return ShaderBrush(shader).also { cache.cachedRimBrush = it }
}

/**
 * Genuine GPU-accelerated Liquid Glass capsule modifier.
 *
 * - On Android 13+ (API 33+) with an active [LocalBackdropState]: renders live vibrancy boost +
 *   optical Gaussian blur + convex-lens AGSL bevel refraction + subtle RGB chromatic dispersion +
 *   directional specular glass rim over the scrolling backdrop underneath.
 * - On Android 12 (API 31-32): hardware `RenderEffect` blur + vibrancy + directional specular rim.
 * - On Android 9-11 (API 28-30) or when `alwaysGlass == false` outside Liquid Glass theme:
 *   clean frosted surface with specular rim.
 */
fun Modifier.ferricLiquidGlass(
    cornerRadius: Dp = 28.dp,
    blurRadius: Dp = 14.dp,
    refractionHeight: Dp = 18.dp,
    refractionAmount: Dp = 14.dp,
    chromaticDispersion: Float = 0.55f,
    tintAlpha: Float? = null,
    alwaysGlass: Boolean = false,
): Modifier = composed {
    val palette = LocalFerricPalette.current
    val backdrop = LocalBackdropState.current
    val useGlass = alwaysGlass || palette.isLiquidGlass

    if (!useGlass) {
        val shape = RoundedCornerShape(cornerRadius)
        return@composed this
            .clip(shape)
            .background(palette.surface)
            .border(width = 1.dp, color = palette.border, shape = shape)
    }

    val glassLayer = rememberGraphicsLayer()
    val cache = remember { GlassCache() }
    val shape = RoundedCornerShape(cornerRadius)
    val effectiveTintAlpha = tintAlpha ?: if (palette.isLight) {
        (palette.glassOpacity * 0.68f).coerceIn(0.46f, 0.72f)
    } else {
        (palette.glassOpacity * 0.56f).coerceIn(0.30f, 0.56f)
    }
    val tintColor = if (palette.isLight) {
        Color.White.copy(alpha = effectiveTintAlpha)
    } else {
        palette.glassTint.copy(alpha = effectiveTintAlpha)
    }
    val fallbackRimBrush = remember(palette.isLight, palette.border) {
        Brush.linearGradient(
            colors = if (palette.isLight) {
                listOf(
                    Color.White.copy(alpha = 0.85f),
                    Color.White.copy(alpha = 0.28f),
                    Color(0xFF1F222A).copy(alpha = 0.14f),
                )
            } else {
                listOf(
                    Color.White.copy(alpha = 0.44f),
                    Color.White.copy(alpha = 0.10f),
                    Color.White.copy(alpha = 0.22f),
                )
            },
        )
    }

    this
        .onGloballyPositioned { coords ->
            cache.elementOffset = coords.positionInRoot()
        }
        .drawWithContent {
            val w = size.width
            val h = size.height
            if (w < 1f || h < 1f) {
                drawContent()
                return@drawWithContent
            }
            val intSize = IntSize(w.roundToInt().coerceAtLeast(1), h.roundToInt().coerceAtLeast(1))
            val radiusPx = min(cornerRadius.toPx(), min(w, h) * 0.5f)
            val blurPx = blurRadius.toPx()

            if (cache.cachedPathSize != size || cache.cachedPathRadius != radiusPx) {
                cache.cachedPath.reset()
                cache.cachedPath.addRoundRect(
                    RoundRect(
                        rect = Rect(Offset.Zero, size),
                        cornerRadius = CornerRadius(radiusPx, radiusPx),
                    ),
                )
                cache.cachedPathSize = size
                cache.cachedPathRadius = radiusPx
            }

            val canSampleBackdrop = backdrop != null &&
                backdrop.hasRecorded &&
                Build.VERSION.SDK_INT >= Build.VERSION_CODES.S

            if (canSampleBackdrop) {
                val dx = cache.elementOffset.x - backdrop.rootOffset.x
                val dy = cache.elementOffset.y - backdrop.rootOffset.y

                val needsRebuild = cache.cachedEffect == null ||
                    cache.lastSize != intSize ||
                    cache.lastBlurPx != blurPx ||
                    cache.lastRadiusPx != radiusPx ||
                    cache.lastLightMode != palette.isLight ||
                    cache.lastDispersion != chromaticDispersion ||
                    cache.lastAccent != palette.accent

                if (needsRebuild) {
                    cache.lastSize = intSize
                    cache.lastBlurPx = blurPx
                    cache.lastRadiusPx = radiusPx
                    cache.lastLightMode = palette.isLight
                    cache.lastDispersion = chromaticDispersion
                    cache.lastAccent = palette.accent
                    cache.cachedEffect = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        buildApi33LiquidGlassEffect(
                            cache = cache,
                            widthPx = w,
                            heightPx = h,
                            radiusPx = radiusPx,
                            blurPx = blurPx,
                            refractionHeightPx = refractionHeight.toPx(),
                            refractionAmountPx = refractionAmount.toPx(),
                            dispersion = chromaticDispersion,
                            isLight = palette.isLight,
                        )
                    } else {
                        val blur = RenderEffect.createBlurEffect(
                            blurPx.coerceAtLeast(1f),
                            blurPx.coerceAtLeast(1f),
                            Shader.TileMode.CLAMP,
                        )
                        val vibrancy = buildVibrancyEffect(palette.isLight)
                        RenderEffect.createChainEffect(blur, vibrancy).asComposeRenderEffect()
                    }
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        buildApi33RimBrush(
                            cache = cache,
                            widthPx = w,
                            heightPx = h,
                            radiusPx = radiusPx,
                            isLight = palette.isLight,
                            accentHint = palette.accent,
                        )
                    }
                }

                glassLayer.clip = true
                glassLayer.renderEffect = cache.cachedEffect
                glassLayer.record(size = intSize) {
                    translate(left = -dx, top = -dy) {
                        drawLayer(backdrop.graphicsLayer)
                    }
                }

                clipPath(cache.cachedPath) {
                    drawLayer(glassLayer)
                    // Subtle optical wash for WCAG >= 4.5:1 legibility over vivid album covers.
                    drawRect(color = tintColor)
                    // Delicate top inner sheen (iOS 26 liquid meniscus highlight).
                    drawRect(
                        brush = Brush.verticalGradient(
                            colors = listOf(
                                Color.White.copy(alpha = if (palette.isLight) 0.34f else 0.11f),
                                Color.Transparent,
                                Color.Black.copy(alpha = if (palette.isLight) 0.04f else 0.12f),
                            ),
                        ),
                    )
                }
            } else {
                // API 28-30 or unrecorded fallback: clean frosted translucent surface.
                clipPath(cache.cachedPath) {
                    drawRect(
                        color = if (palette.isLight) {
                            Color.White.copy(alpha = 0.84f)
                        } else {
                            palette.glassTint.copy(alpha = 0.84f)
                        },
                    )
                }
            }

            // Crisp directional optical specular rim border.
            val rimStrokePx = 1.25.dp.toPx()
            val inset = rimStrokePx * 0.5f
            val rimBrush = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                cache.cachedRimBrush ?: fallbackRimBrush
            } else {
                fallbackRimBrush
            }
            drawRoundRect(
                brush = rimBrush,
                topLeft = Offset(inset, inset),
                size = Size((w - rimStrokePx).coerceAtLeast(0f), (h - rimStrokePx).coerceAtLeast(0f)),
                cornerRadius = CornerRadius(
                    (radiusPx - inset).coerceAtLeast(0f),
                    (radiusPx - inset).coerceAtLeast(0f),
                ),
                style = Stroke(width = rimStrokePx),
            )

            // Draw foreground icons/text razor-sharp on top of the optical glass.
            clipPath(cache.cachedPath) {
                this@drawWithContent.drawContent()
            }
        }
        .clip(shape)
}
