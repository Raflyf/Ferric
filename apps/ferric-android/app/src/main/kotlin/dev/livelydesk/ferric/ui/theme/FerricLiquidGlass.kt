package dev.livelydesk.ferric.ui.theme

import android.graphics.ColorMatrix
import android.graphics.ColorMatrixColorFilter
import android.graphics.RenderEffect
import android.graphics.RuntimeShader
import android.graphics.Shader
import android.os.Build
import android.os.SystemClock
import android.view.WindowManager
import androidx.annotation.RequiresApi
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SheetState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
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
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.asComposeRenderEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogWindowProvider
import kotlin.math.abs
import kotlin.math.min
import kotlin.math.roundToInt

/**
 * Rendering fidelity tier for optical Liquid Glass:
 * - [HERO_LENS]: full AGSL convex-lens bevel refraction + RGB chromatic dispersion + angular chrome
 *   specular rim, sampling the shared 0.28x screen blur layer (ideal for dock, mini-player, and
 *   player chrome).
 * - [FROSTED_PANEL]: zero-FBO shared-blur panel sampling the same pre-blurred backdrop directly with
 *   luminous frosted white wash + directional chrome border (0 extra GPU layers per card/button).
 */
enum class GlassTier {
    HERO_LENS,
    FROSTED_PANEL,
}

/**
 * Shared single-pass hardware backdrop capture state for real-time optical Liquid Glass.
 * Records the parent screen once per frame into [graphicsLayer] and executes ONE shared downscaled
 * Gaussian blur + bright vibrancy pass into [blurredLayer] (~1 MB VRAM at 0.28x resolution), so
 * every floating capsule, card, chip, and button samples blurred content at near-zero marginal GPU cost.
 */
@Stable
class FerricBackdropState(
    val graphicsLayer: GraphicsLayer,
    val blurredLayer: GraphicsLayer,
) {
    val blurScale: Float = 0.20f
    var rootOffset: Offset = Offset.Zero
    var hasRecorded: Boolean = false
    var isRecordingSource: Boolean = false
    var contentSize: IntSize = IntSize.Zero
    var blurDirty: Boolean = true
    var hasBlurred: Boolean = false
    var blurredSize: IntSize = IntSize.Zero
    private var lastBlurPx: Float = -1f
    private var lastIsLight: Boolean = false
    private var lastBlurMs: Long = 0L
    private var sharedBlurEffect: androidx.compose.ui.graphics.RenderEffect? = null

    fun ensureBlurred(
        blurPx: Float,
        isLight: Boolean,
        density: Density,
        layoutDirection: LayoutDirection,
    ): Boolean {
        if (isRecordingSource ||
            Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
            !hasRecorded ||
            contentSize.width < 4 ||
            contentSize.height < 4
        ) {
            return false
        }
        val scaledBlur = (blurPx * blurScale).coerceIn(2f, 20f)
        val bw = kotlin.math.ceil(contentSize.width * blurScale.toDouble()).toInt().coerceAtLeast(1)
        val bh = kotlin.math.ceil(contentSize.height * blurScale.toDouble()).toInt().coerceAtLeast(1)
        val targetSize = IntSize(bw, bh)
        val configChanged = !hasBlurred ||
            blurredSize != targetSize ||
            sharedBlurEffect == null ||
            abs(lastBlurPx - scaledBlur) > 0.5f ||
            lastIsLight != isLight
        val nowMs = SystemClock.uptimeMillis()
        if (configChanged || (blurDirty && nowMs - lastBlurMs >= 24L)) {
            lastBlurPx = scaledBlur
            lastIsLight = isLight
            lastBlurMs = nowMs
            blurredSize = targetSize
            val fx = buildDecoupledBlurEffect(scaledBlur, isLight)
            sharedBlurEffect = fx
            blurredLayer.clip = false
            blurredLayer.renderEffect = fx
            blurredLayer.record(density = density, layoutDirection = layoutDirection, size = targetSize) {
                scale(scaleX = blurScale, scaleY = blurScale, pivot = Offset.Zero) {
                    drawLayer(graphicsLayer)
                }
            }
            blurDirty = false
            hasBlurred = true
        }
        return hasBlurred
    }
}

val LocalBackdropState = compositionLocalOf<FerricBackdropState?> { null }

@Composable
fun rememberFerricBackdropState(): FerricBackdropState {
    val rawLayer = rememberGraphicsLayer()
    val blurLayer = rememberGraphicsLayer()
    return remember(rawLayer, blurLayer) { FerricBackdropState(rawLayer, blurLayer) }
}

/**
 * Captures the underlying scrollable surface into [backdrop] so sibling overlays above it can
 * render genuine optical Liquid Glass refraction and frosted blur with a single shared blur pass.
 */
fun Modifier.ferricBackdropSource(
    backdrop: FerricBackdropState,
    enabled: Boolean = true,
): Modifier =
    this
        .onGloballyPositioned { coords ->
            if (enabled) backdrop.rootOffset = coords.positionInRoot()
        }
        .drawWithContent {
            if (enabled && size.width >= 1f && size.height >= 1f) {
                val exactSize = IntSize(
                    size.width.roundToInt().coerceAtLeast(1),
                    size.height.roundToInt().coerceAtLeast(1),
                )
                backdrop.isRecordingSource = true
                try {
                    backdrop.graphicsLayer.record(
                        density = this@drawWithContent,
                        layoutDirection = layoutDirection,
                        size = exactSize,
                    ) {
                        this@drawWithContent.drawContent()
                    }
                    backdrop.contentSize = exactSize
                    backdrop.hasRecorded = true
                    backdrop.blurDirty = true
                } finally {
                    backdrop.isRecordingSource = false
                }
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

// Directional optical glass specular rim lighting AGSL shader (bright white catchlight + silver bounce).
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
    float baseRim = 0.18;
    float a = clamp(rim.a * (baseRim + intensity * 0.82), 0.0, 1.0);
    return half4(rim.rgb * a, a);
}
"""

private class GlassCache {
    var elementOffset: Offset = Offset.Zero
    var lastSize: IntSize = IntSize.Zero
    var lastRadiusPx: Float = -1f
    var lastRefractionHeightPx: Float = -1f
    var lastRefractionAmountPx: Float = -1f
    var lastLightMode: Boolean = false
    var lastDispersion: Float = -1f
    var cachedRefractEffect: androidx.compose.ui.graphics.RenderEffect? = null
    var refractionShader: Any? = null
    var rimShader: Any? = null
    var cachedRimBrush: ShaderBrush? = null
    var cachedPath: Path = Path()
    var cachedPathSize: Size = Size.Zero
    var cachedPathRadius: Float = -1f
}

@RequiresApi(Build.VERSION_CODES.S)
private fun buildVibrancyEffect(isLight: Boolean): RenderEffect {
    val sat = if (isLight) 1.22f else 1.28f
    val matrix = ColorMatrix().apply {
        setSaturation(sat)
    }
    // Keep blacks deep and colors crystal-clear with zero milky white haze.
    val gain = if (isLight) 1.02f else 1.04f
    val bias = if (isLight) 2f else 0f
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

@RequiresApi(Build.VERSION_CODES.S)
private fun buildDecoupledBlurEffect(blurPx: Float, isLight: Boolean): androidx.compose.ui.graphics.RenderEffect {
    val clampedBlur = blurPx.coerceIn(2f, 28f)
    val blur = RenderEffect.createBlurEffect(clampedBlur, clampedBlur, Shader.TileMode.CLAMP)
    val vibrancy = buildVibrancyEffect(isLight)
    return RenderEffect.createChainEffect(blur, vibrancy).asComposeRenderEffect()
}

@RequiresApi(Build.VERSION_CODES.TIRAMISU)
private fun buildDecoupledRefractionEffect(
    cache: GlassCache,
    widthPx: Float,
    heightPx: Float,
    radiusPx: Float,
    refractionHeightPx: Float,
    refractionAmountPx: Float,
    dispersion: Float,
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
    return RenderEffect.createRuntimeShaderEffect(shader, "content").asComposeRenderEffect()
}

@RequiresApi(Build.VERSION_CODES.TIRAMISU)
private fun buildApi33RimBrush(
    cache: GlassCache,
    widthPx: Float,
    heightPx: Float,
    radiusPx: Float,
    isLight: Boolean,
): ShaderBrush {
    val shader = (cache.rimShader as? RuntimeShader)
        ?: RuntimeShader(SPECULAR_RIM_AGSL).also { cache.rimShader = it }
    val halfW = (widthPx * 0.5f).coerceAtLeast(1f)
    val halfH = (heightPx * 0.5f).coerceAtLeast(1f)
    val minHalf = min(halfW, halfH)
    shader.setFloatUniform("halfSize", halfW, halfH)
    shader.setFloatUniform("minHalf", minHalf)
    shader.setFloatUniform("cornerRadius", radiusPx.coerceAtMost(minHalf))
    shader.setFloatUniform("lightDir", -0.56f, -0.83f)
    shader.setFloatUniform("falloff", 1.45f)
    if (isLight) {
        shader.setFloatUniform("topColor", 1.0f, 1.0f, 1.0f, 0.90f)
        shader.setFloatUniform("bottomColor", 0.32f, 0.34f, 0.42f, 0.28f)
    } else {
        shader.setFloatUniform("topColor", 1.0f, 1.0f, 1.0f, 0.78f)
        shader.setFloatUniform("bottomColor", 0.92f, 0.95f, 1.0f, 0.38f)
    }
    return ShaderBrush(shader).also { cache.cachedRimBrush = it }
}

/**
 * Luminous frosted optical Liquid Glass modifier backed by shared screen-space Gaussian blur.
 *
 * When [tier] is [GlassTier.HERO_LENS], adds a single-layer AGSL convex-lens bevel + chromatic
 * dispersion pass over the shared pre-blurred backdrop. When [tier] is [GlassTier.FROSTED_PANEL],
 * samples the shared blurred backdrop directly with zero extra offscreen GPU FBO allocations.
 */
fun Modifier.ferricLiquidGlass(
    cornerRadius: Dp = 28.dp,
    blurRadius: Dp = 15.dp,
    refractionHeight: Dp = 16.dp,
    refractionAmount: Dp = 13.dp,
    chromaticDispersion: Float = 0.55f,
    tintAlpha: Float? = null,
    alwaysGlass: Boolean = false,
    tier: GlassTier = GlassTier.HERO_LENS,
    accentGlow: Color = Color.Unspecified,
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

    val isHero = tier == GlassTier.HERO_LENS
    val glassLayer = if (isHero) rememberGraphicsLayer() else null
    val cache = remember { GlassCache() }
    val shape = RoundedCornerShape(cornerRadius)
    val opacityNorm = palette.glassOpacity.coerceIn(0.15f, 0.95f)

    // Clear smoked crystal optics matching Auralys: underlying colors shine cleanly through with
    // zero chalky white milk wash while white typography & icons stay razor-sharp.
    val anchorAlpha = if (palette.isLight) {
        0f
    } else if (isHero) {
        (0.10f + opacityNorm * 0.16f).coerceIn(0.12f, 0.28f)
    } else {
        (0.18f + opacityNorm * 0.22f).coerceIn(0.20f, 0.42f)
    }
    val anchorColor = Color(
        red = 0.04f + palette.glassTint.red * 0.10f,
        green = 0.06f + palette.glassTint.green * 0.10f,
        blue = 0.10f + palette.glassTint.blue * 0.10f,
        alpha = anchorAlpha,
    )

    val whiteFrostAlpha = if (palette.isLight) {
        (tintAlpha ?: (0.58f + opacityNorm * 0.28f)).coerceIn(0.32f, 0.86f)
    } else {
        val base = if (isHero) 0.015f + opacityNorm * 0.030f else 0.025f + opacityNorm * 0.040f
        (tintAlpha?.let { (it * 0.18f).coerceIn(0.01f, 0.10f) } ?: base).coerceIn(0.01f, 0.09f)
    }
    val whiteFrostColor = Color(0xFFF6F9FF).copy(alpha = whiteFrostAlpha)

    val sheenBrush = remember(palette.isLight, isHero, accentGlow) {
        if (accentGlow != Color.Unspecified) {
            Brush.verticalGradient(
                0.0f to Color.White.copy(alpha = if (isHero) 0.22f else 0.16f),
                0.35f to accentGlow.copy(alpha = 0.08f),
                0.70f to Color.Transparent,
                1.0f to Color.White.copy(alpha = 0.06f),
            )
        } else {
            Brush.verticalGradient(
                0.0f to Color.White.copy(alpha = if (palette.isLight) 0.40f else if (isHero) 0.20f else 0.15f),
                0.38f to Color.Transparent,
                0.75f to Color.Transparent,
                1.0f to Color.White.copy(alpha = if (palette.isLight) 0.18f else if (isHero) 0.08f else 0.05f),
            )
        }
    }
    val rimStrokeBrush = remember(palette.isLight, accentGlow) {
        Brush.linearGradient(
            colors = if (palette.isLight) {
                listOf(
                    Color.White.copy(alpha = 0.88f),
                    Color.White.copy(alpha = 0.35f),
                    Color(0xFF1F222A).copy(alpha = 0.16f),
                )
            } else {
                listOf(
                    Color.White.copy(alpha = 0.68f),
                    Color.White.copy(alpha = 0.16f),
                    (if (accentGlow != Color.Unspecified) accentGlow else Color.White).copy(alpha = 0.36f),
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
            val opacityBlurScale = (0.45f + opacityNorm * 0.85f)
            val blurPx = (blurRadius.toPx() * opacityBlurScale).coerceIn(4f, 40f)
            val refHeightPx = refractionHeight.toPx()
            val refAmountPx = refractionAmount.toPx()

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

            val activeBackdrop = backdrop
            val hasSharedBlur = activeBackdrop?.ensureBlurred(
                blurPx = blurPx,
                isLight = palette.isLight,
                density = this@drawWithContent,
                layoutDirection = layoutDirection,
            ) == true

            if (hasSharedBlur) {
                val dx = cache.elementOffset.x - activeBackdrop!!.rootOffset.x
                val dy = cache.elementOffset.y - activeBackdrop.rootOffset.y
                val invScale = 1f / activeBackdrop.blurScale

                if (isHero && glassLayer != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    val needsRebuild = cache.cachedRefractEffect == null ||
                        cache.lastSize != intSize ||
                        cache.lastRadiusPx != radiusPx ||
                        cache.lastRefractionHeightPx != refHeightPx ||
                        cache.lastRefractionAmountPx != refAmountPx ||
                        cache.lastLightMode != palette.isLight ||
                        cache.lastDispersion != chromaticDispersion
                    if (needsRebuild) {
                        cache.lastSize = intSize
                        cache.lastRadiusPx = radiusPx
                        cache.lastRefractionHeightPx = refHeightPx
                        cache.lastRefractionAmountPx = refAmountPx
                        cache.lastLightMode = palette.isLight
                        cache.lastDispersion = chromaticDispersion
                        cache.cachedRefractEffect = if (refAmountPx > 0.5f) {
                            buildDecoupledRefractionEffect(
                                cache = cache,
                                widthPx = w,
                                heightPx = h,
                                radiusPx = radiusPx,
                                refractionHeightPx = refHeightPx,
                                refractionAmountPx = refAmountPx,
                                dispersion = chromaticDispersion,
                            )
                        } else {
                            null
                        }
                        buildApi33RimBrush(
                            cache = cache,
                            widthPx = w,
                            heightPx = h,
                            radiusPx = radiusPx,
                            isLight = palette.isLight,
                        )
                    }

                    val refractFx = cache.cachedRefractEffect
                    clipPath(cache.cachedPath) {
                        if (refractFx != null) {
                            glassLayer.clip = true
                            glassLayer.renderEffect = refractFx
                            glassLayer.record(size = intSize) {
                                translate(left = -dx, top = -dy) {
                                    scale(scaleX = invScale, scaleY = invScale, pivot = Offset.Zero) {
                                        drawLayer(activeBackdrop.blurredLayer)
                                    }
                                }
                            }
                            drawLayer(glassLayer)
                        } else {
                            translate(left = -dx, top = -dy) {
                                scale(scaleX = invScale, scaleY = invScale, pivot = Offset.Zero) {
                                    drawLayer(activeBackdrop.blurredLayer)
                                }
                            }
                        }
                        if (anchorAlpha > 0.005f) drawRect(color = anchorColor)
                        drawRect(color = whiteFrostColor)
                        drawRect(brush = sheenBrush)
                    }
                } else {
                    // FROSTED_PANEL tier: zero offscreen FBO allocations per widget!
                    clipPath(cache.cachedPath) {
                        translate(left = -dx, top = -dy) {
                            scale(scaleX = invScale, scaleY = invScale, pivot = Offset.Zero) {
                                drawLayer(activeBackdrop.blurredLayer)
                            }
                        }
                        if (anchorAlpha > 0.005f) drawRect(color = anchorColor)
                        drawRect(color = whiteFrostColor)
                        drawRect(brush = sheenBrush)
                    }
                }
            } else {
                // Zero-FBO translucent smoked crystal directly over live Aurora or sheet glow.
                clipPath(cache.cachedPath) {
                    if (palette.isLight) {
                        drawRect(color = Color.White.copy(alpha = 0.84f))
                    } else {
                        val crystalAlpha = (0.28f + opacityNorm * 0.28f).coerceIn(0.24f, 0.58f)
                        drawRect(color = Color(0xFF0B0F19).copy(alpha = crystalAlpha))
                        drawRect(color = palette.glassTint.copy(alpha = 0.14f))
                        if (accentGlow != Color.Unspecified) {
                            drawRect(color = accentGlow.copy(alpha = 0.10f))
                        }
                        drawRect(brush = sheenBrush)
                    }
                }
            }

            // Crisp luminous directional chrome glass rim border.
            val rimStrokePx = (if (isHero) 1.25.dp else 1.0.dp).toPx()
            val inset = rimStrokePx * 0.5f
            val rimBrush = if (isHero && hasSharedBlur && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                cache.cachedRimBrush ?: rimStrokeBrush
            } else {
                rimStrokeBrush
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

            clipPath(cache.cachedPath) {
                this@drawWithContent.drawContent()
            }
        }
        .clip(shape)
}

/**
 * Translucent Liquid Glass modal bottom sheet container with hardware compositor blur behind the
 * dialog window (API 31+) and an internal [FerricBackdropState] so every card, button, and chip
 * inside the sheet renders as luminous Liquid Glass instead of an opaque dark block.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FerricModalBottomSheet(
    onDismissRequest: () -> Unit,
    modifier: Modifier = Modifier,
    sheetState: SheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    coverShade: CoverShadeColors = CoverShadeColors(),
    content: @Composable ColumnScope.() -> Unit,
) {
    val palette = LocalFerricPalette.current
    if (!palette.isLiquidGlass) {
        ModalBottomSheet(
            onDismissRequest = onDismissRequest,
            modifier = modifier,
            sheetState = sheetState,
            containerColor = palette.surface,
            content = content,
        )
        return
    }

    val opacityNorm = palette.glassOpacity.coerceIn(0.15f, 0.95f)
    val sheetBgAlpha = (0.56f + opacityNorm * 0.22f).coerceIn(0.50f, 0.78f)
    val sheetShape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)

    ModalBottomSheet(
        onDismissRequest = onDismissRequest,
        modifier = modifier,
        sheetState = sheetState,
        containerColor = Color.Transparent,
        scrimColor = Color.Black.copy(alpha = 0.28f),
        dragHandle = null,
    ) {
        val view = LocalView.current
        SideEffect {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                val window = (view.parent as? DialogWindowProvider)?.window
                if (window != null) {
                    runCatching {
                        window.addFlags(WindowManager.LayoutParams.FLAG_BLUR_BEHIND)
                        window.attributes = window.attributes.apply {
                            blurBehindRadius = 42
                        }
                        window.setBackgroundBlurRadius(84)
                    }
                }
            }
        }

        Surface(
            shape = sheetShape,
            color = Color.Transparent,
            border = BorderStroke(
                width = 1.dp,
                brush = Brush.verticalGradient(
                    listOf(
                        Color.White.copy(alpha = 0.50f),
                        Color.White.copy(alpha = 0.14f),
                        Color.Transparent,
                    ),
                ),
            ),
            modifier = Modifier
                .fillMaxWidth()
                .clip(sheetShape),
        ) {
            Box(modifier = Modifier.fillMaxWidth()) {
                Box(
                    modifier = Modifier
                        .matchParentSize()
                        .background(Color(0xFF0C101B).copy(alpha = sheetBgAlpha)),
                )
                Box(
                    modifier = Modifier
                        .matchParentSize()
                        .background(
                            Brush.radialGradient(
                                colors = listOf(
                                    coverShade.accent.copy(alpha = 0.16f),
                                    coverShade.orb1.copy(alpha = 0.16f),
                                    Color.Transparent,
                                ),
                                center = Offset(220f, 140f),
                                radius = 900f,
                            ),
                        ),
                )
                Box(
                    modifier = Modifier
                        .matchParentSize()
                        .background(
                            Brush.verticalGradient(
                                listOf(
                                    Color.White.copy(alpha = 0.12f),
                                    Color.Transparent,
                                    Color.White.copy(alpha = 0.04f),
                                ),
                            ),
                        ),
                )

                CompositionLocalProvider(LocalBackdropState provides null) {
                    Column(modifier = Modifier.fillMaxWidth()) {
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(top = 12.dp, bottom = 6.dp),
                            contentAlignment = Alignment.Center,
                        ) {
                            Box(
                                modifier = Modifier
                                    .width(40.dp)
                                    .height(4.dp)
                                    .clip(RoundedCornerShape(999.dp))
                                    .background(Color.White.copy(alpha = 0.56f)),
                            )
                        }
                        content()
                    }
                }
            }
        }
    }
}

/**
 * Translucent crystal Liquid Glass selection pill replacing solid opaque Material FilterChips
 * across Equalizer, DSP, and Theme Studio controls.
 */
@Composable
fun FerricChoicePill(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    if (selected) {
        Surface(
            shape = RoundedCornerShape(999.dp),
            color = palette.accent,
            border = BorderStroke(1.dp, palette.accent),
            modifier = modifier
                .height(34.dp)
                .clip(RoundedCornerShape(999.dp))
                .clickable(onClick = onClick),
        ) {
            Box(
                modifier = Modifier.padding(horizontal = 14.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = label,
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.Bold,
                    color = if (palette.isLight) Color.White else Color.Black,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    } else {
        Box(
            modifier = modifier
                .height(34.dp)
                .ferricGlassSurface(
                    palette = palette,
                    cornerRadius = 999.dp,
                    tier = GlassTier.FROSTED_PANEL,
                    fallbackColor = palette.elevated,
                )
                .clickable(onClick = onClick)
                .padding(horizontal = 14.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.Medium,
                color = palette.textPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/**
 * Slim precision slider (4 dp capsule track + 14 dp thumb) avoiding bulky Material3 Expressive bars.
 */
@Composable
fun FerricSlimSlider(
    value: Float,
    onValueChange: (Float) -> Unit,
    valueRange: ClosedFloatingPointRange<Float> = 0f..1f,
    steps: Int = 0,
    onValueChangeFinished: (() -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val currentOnChange by rememberUpdatedState(onValueChange)
    val currentOnFinished by rememberUpdatedState(onValueChangeFinished)
    val span = (valueRange.endInclusive - valueRange.start).coerceAtLeast(1e-5f)
    val frac = ((value - valueRange.start) / span).coerceIn(0f, 1f)
    var trackWidthPx by remember { mutableIntStateOf(1) }

    fun emitForX(x: Float) {
        var f = (x / trackWidthPx.toFloat().coerceAtLeast(1f)).coerceIn(0f, 1f)
        if (steps > 0) {
            val segments = steps + 1
            f = kotlin.math.round(f * segments) / segments
        }
        currentOnChange(valueRange.start + f * span)
    }

    val inactiveTrack = if (palette.isLiquidGlass && !palette.isLight) {
        Color.White.copy(alpha = 0.22f)
    } else {
        palette.border
    }

    Canvas(
        modifier = modifier
            .fillMaxWidth()
            .height(28.dp)
            .onSizeChanged { trackWidthPx = it.width.coerceAtLeast(1) }
            .pointerInput(valueRange, steps) {
                detectTapGestures { offset ->
                    emitForX(offset.x)
                    currentOnFinished?.invoke()
                }
            }
            .pointerInput(valueRange, steps) {
                detectHorizontalDragGestures(
                    onDragStart = { offset -> emitForX(offset.x) },
                    onHorizontalDrag = { change, _ ->
                        change.consume()
                        emitForX(change.position.x)
                    },
                    onDragEnd = { currentOnFinished?.invoke() },
                    onDragCancel = { currentOnFinished?.invoke() },
                )
            },
    ) {
        val midY = size.height * 0.5f
        val thumbR = 7.dp.toPx()
        val usableW = (size.width - thumbR * 2f).coerceAtLeast(1f)
        val thumbX = thumbR + frac * usableW
        val strokePx = 4.dp.toPx()

        drawLine(
            color = inactiveTrack,
            start = Offset(thumbR, midY),
            end = Offset(size.width - thumbR, midY),
            strokeWidth = strokePx,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = palette.accent,
            start = Offset(thumbR, midY),
            end = Offset(thumbX, midY),
            strokeWidth = strokePx,
            cap = StrokeCap.Round,
        )
        drawCircle(
            color = Color.Black.copy(alpha = 0.28f),
            radius = thumbR + 1.5f,
            center = Offset(thumbX, midY + 1f),
        )
        drawCircle(
            color = palette.accent,
            radius = thumbR,
            center = Offset(thumbX, midY),
        )
    }
}
