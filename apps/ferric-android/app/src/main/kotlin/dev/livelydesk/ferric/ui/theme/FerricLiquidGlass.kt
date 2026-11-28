package dev.livelydesk.ferric.ui.theme

import android.graphics.ColorMatrix
import android.graphics.ColorMatrixColorFilter
import android.graphics.RenderEffect
import android.graphics.RuntimeShader
import android.graphics.Shader
import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.annotation.RequiresApi
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
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
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
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
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.Velocity
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
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
    val blurScale: Float = 0.25f
    var rootOffset: Offset = Offset.Zero
    var hasRecorded: Boolean = false
    var isRecordingSource: Boolean = false
    var contentSize: IntSize = IntSize.Zero
    var blurDirty: Boolean = true
    var hasBlurred: Boolean = false
    var blurredSize: IntSize = IntSize.Zero
    var blurVersion: Long = 0L
    private var lastBlurPx: Float = -1f
    private var lastIsLight: Boolean = false
    private var sharedBlurEffect: androidx.compose.ui.graphics.RenderEffect? = null

    fun ensureBlurred(
        blurPx: Float,
        isLight: Boolean,
        drawScope: androidx.compose.ui.graphics.drawscope.DrawScope,
    ): Boolean {
        if (isRecordingSource ||
            Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
            !hasRecorded ||
            contentSize.width < 4 ||
            contentSize.height < 4
        ) {
            return false
        }
        val scaledBlur = (blurPx * blurScale).coerceIn(1.8f, 22f)
        val bw = kotlin.math.ceil(contentSize.width * blurScale.toDouble()).toInt().coerceAtLeast(1)
        val bh = kotlin.math.ceil(contentSize.height * blurScale.toDouble()).toInt().coerceAtLeast(1)
        val targetSize = IntSize(bw, bh)
        if (!hasBlurred ||
            blurDirty ||
            blurredSize != targetSize ||
            sharedBlurEffect == null ||
            abs(lastBlurPx - scaledBlur) > 0.4f ||
            lastIsLight != isLight
        ) {
            lastBlurPx = scaledBlur
            lastIsLight = isLight
            blurredSize = targetSize
            val fx = buildDecoupledBlurEffect(scaledBlur, isLight)
            sharedBlurEffect = fx
            blurredLayer.clip = false
            blurredLayer.renderEffect = fx
            with(drawScope) {
                blurredLayer.record(size = targetSize) {
                    scale(scaleX = blurScale, scaleY = blurScale, pivot = Offset.Zero) {
                        drawLayer(graphicsLayer)
                    }
                }
            }
            blurDirty = false
            hasBlurred = true
            blurVersion++
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
                backdrop.isRecordingSource = true
                try {
                    // Use ContentDrawScope's native record{} extension so drawContent() captures into
                    // backdrop.graphicsLayer rather than drawing directly to the root window canvas.
                    backdrop.graphicsLayer.record {
                        this@drawWithContent.drawContent()
                    }
                    backdrop.contentSize = IntSize(
                        size.width.roundToInt().coerceAtLeast(1),
                        size.height.roundToInt().coerceAtLeast(1),
                    )
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

    float2 safeMax = max(fullSize - float2(0.5), float2(1.0));
    float2 refrCoord = clamp(coord + disp * gDir, float2(0.5), safeMax);

    if (chromaticAberration <= 0.001) {
        return content.eval(refrCoord);
    }
    float cornerDispersion = chromaticAberration * ((centered.x * centered.y) / max(halfSize.x * halfSize.y, 1.0));
    float2 chromaDelta = gDir * (disp * cornerDispersion);
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
    var elementOffset by mutableStateOf(Offset.Zero)
    var lastSize: IntSize = IntSize.Zero
    var lastRadiusPx: Float = -1f
    var lastRefractionHeightPx: Float = -1f
    var lastRefractionAmountPx: Float = -1f
    var lastLightMode: Boolean = false
    var lastDispersion: Float = -1f
    var lastRecordedVersion: Long = -1L
    var lastDx: Float = Float.NaN
    var lastDy: Float = Float.NaN
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
    val sat = if (isLight) 1.32f else 1.55f
    val matrix = ColorMatrix().apply {
        setSaturation(sat)
    }
    return RenderEffect.createColorFilterEffect(ColorMatrixColorFilter(matrix))
}

@RequiresApi(Build.VERSION_CODES.S)
private fun buildDecoupledBlurEffect(blurPx: Float, isLight: Boolean): androidx.compose.ui.graphics.RenderEffect {
    val clampedBlur = blurPx.coerceIn(1.8f, 24f)
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
    anchorAlphaOverride: Float? = null,
    screenOffsetProvider: (() -> Offset)? = null,
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
    val anchorAlpha = when {
        anchorAlphaOverride != null -> anchorAlphaOverride.coerceIn(0f, 0.85f)
        palette.isLight -> 0f
        isHero -> (0.10f + opacityNorm * 0.16f).coerceIn(0.12f, 0.28f)
        else -> (0.18f + opacityNorm * 0.22f).coerceIn(0.20f, 0.42f)
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
        .then(
            if (screenOffsetProvider == null) {
                Modifier.onGloballyPositioned { coords ->
                    cache.elementOffset = coords.positionInRoot()
                }
            } else {
                Modifier
            },
        )
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
            val hasSharedBlur = activeBackdrop != null &&
                activeBackdrop.ensureBlurred(
                    blurPx = blurPx,
                    isLight = palette.isLight,
                    drawScope = this@drawWithContent,
                )
            if (activeBackdrop != null && hasSharedBlur) {
                val elementPos = screenOffsetProvider?.invoke() ?: cache.elementOffset
                val dx = elementPos.x - activeBackdrop.rootOffset.x
                val dy = elementPos.y - activeBackdrop.rootOffset.y
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
                            val recordNeeded = needsRebuild ||
                                cache.lastRecordedVersion != activeBackdrop.blurVersion ||
                                abs(cache.lastDx - dx) > 0.5f ||
                                abs(cache.lastDy - dy) > 0.5f
                            if (recordNeeded) {
                                cache.lastRecordedVersion = activeBackdrop.blurVersion
                                cache.lastDx = dx
                                cache.lastDy = dy
                                glassLayer.clip = true
                                glassLayer.renderEffect = refractFx
                                glassLayer.record(size = intSize) {
                                    translate(left = -dx, top = -dy) {
                                        scale(scaleX = invScale, scaleY = invScale, pivot = Offset.Zero) {
                                            drawLayer(activeBackdrop.blurredLayer)
                                        }
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
                // Zero-FBO translucent smoked crystal directly over live Aurora or HWUI blurred sheet.
                clipPath(cache.cachedPath) {
                    if (palette.isLight) {
                        drawRect(color = Color.White.copy(alpha = 0.84f))
                    } else {
                        val crystalAlpha = (0.14f + opacityNorm * 0.16f).coerceIn(0.14f, 0.34f)
                        drawRect(color = Color(0xFF0A0D16).copy(alpha = crystalAlpha))
                        drawRect(color = palette.glassTint.copy(alpha = 0.12f))
                        if (accentGlow != Color.Unspecified) {
                            drawRect(color = accentGlow.copy(alpha = 0.12f))
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

@Stable
internal class FerricSheetEntry(
    onDismiss: () -> Unit,
    coverShade: CoverShadeColors,
    content: @Composable ColumnScope.() -> Unit,
) {
    var onDismiss by mutableStateOf(onDismiss)
    var coverShade by mutableStateOf(coverShade)
    var content by mutableStateOf(content)
    var requestClose: (() -> Unit)? = null
}

/**
 * In-window bottom sheet stack manager enabling every sheet across the app to render as a genuine
 * AGSL optical Liquid Glass lens over the active window backdrop without Dialog window boundaries.
 */
@Stable
class FerricSheetHostState internal constructor() {
    internal val entries = mutableStateListOf<FerricSheetEntry>()
    val hasActiveSheets: Boolean get() = entries.isNotEmpty()

    internal fun add(entry: FerricSheetEntry) {
        if (!entries.contains(entry)) {
            entries.add(entry)
        }
    }

    internal fun remove(entry: FerricSheetEntry) {
        entries.remove(entry)
    }
}

val LocalFerricSheetHost = compositionLocalOf<FerricSheetHostState?> { null }

@Composable
fun rememberFerricSheetHostState(): FerricSheetHostState = remember { FerricSheetHostState() }

/**
 * Renders active sheets in the same Activity window above [sheetBackdrop], giving every bottom
 * sheet genuine Home-Pill AGSL blur, convex-lens bevel refraction, and diagonal chromatic dispersion.
 */
@Composable
fun FerricSheetPortalOverlay(
    host: FerricSheetHostState,
    sheetBackdrop: FerricBackdropState,
) {
    if (!host.hasActiveSheets) return

    val palette = LocalFerricPalette.current
    BackHandler(enabled = host.hasActiveSheets) {
        host.entries.lastOrNull()?.let { top ->
            top.requestClose?.invoke() ?: top.onDismiss()
        }
    }

    Box(modifier = Modifier.fillMaxSize()) {
        host.entries.forEach { entry ->
            key(entry) {
                FerricSheetWindowItem(
                    entry = entry,
                    sheetBackdrop = sheetBackdrop,
                    palette = palette,
                )
            }
        }
    }
}

@Composable
private fun FerricSheetWindowItem(
    entry: FerricSheetEntry,
    sheetBackdrop: FerricBackdropState,
    palette: FerricVisualPalette,
) {
    val density = LocalDensity.current
    val scope = rememberCoroutineScope()
    val config = LocalConfiguration.current
    val screenHeightPx = with(density) { config.screenHeightDp.dp.toPx() }.coerceAtLeast(1200f)
    val maxSheetHeight = (config.screenHeightDp * 0.88f).dp
    val dismissThresholdPx = with(density) { 108.dp.toPx() }

    val offsetY = remember(entry) { Animatable(screenHeightPx * 0.55f) }
    val scrimAlpha = remember(entry) { Animatable(0f) }
    var sheetHeightPx by remember(entry) { mutableFloatStateOf(screenHeightPx * 0.60f) }
    var isClosing by remember(entry) { mutableStateOf(false) }

    val dismissAnimated: () -> Unit = remember(entry) {
        {
            if (!isClosing) {
                isClosing = true
                scope.launch {
                    coroutineScope {
                        launch { offsetY.animateTo(sheetHeightPx + 120f, tween(180, easing = FerricEaseOut)) }
                        launch { scrimAlpha.animateTo(0f, tween(160, easing = FerricEaseOut)) }
                    }
                    entry.onDismiss()
                }
            }
        }
    }
    SideEffect { entry.requestClose = dismissAnimated }

    LaunchedEffect(entry) {
        coroutineScope {
            launch { offsetY.animateTo(0f, tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)) }
            launch { scrimAlpha.animateTo(1f, tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut)) }
        }
    }

    val nestedScrollConnection = remember(entry) {
        object : NestedScrollConnection {
            override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                if (isClosing || available.y >= 0f || offsetY.value <= 0f) return Offset.Zero
                val next = (offsetY.value + available.y).coerceAtLeast(0f)
                val consumedY = next - offsetY.value
                scope.launch { offsetY.snapTo(next) }
                return Offset(0f, consumedY)
            }

            override fun onPostScroll(
                consumed: Offset,
                available: Offset,
                source: NestedScrollSource,
            ): Offset {
                if (isClosing || source != NestedScrollSource.UserInput || available.y <= 0f) {
                    return Offset.Zero
                }
                val next = (offsetY.value + available.y).coerceAtLeast(0f)
                scope.launch { offsetY.snapTo(next) }
                return Offset(0f, available.y)
            }

            override suspend fun onPreFling(available: Velocity): Velocity {
                if (offsetY.value <= 0f) return Velocity.Zero
                if (offsetY.value > dismissThresholdPx || available.y > 950f) {
                    dismissAnimated()
                } else {
                    offsetY.animateTo(0f, tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut))
                }
                return available
            }
        }
    }

    val sheetCorner = 28.dp
    val solidShape = RoundedCornerShape(topStart = sheetCorner, topEnd = sheetCorner)
    val liquidShape = RoundedCornerShape(sheetCorner)
    val opacityNorm = palette.glassOpacity.coerceIn(0.15f, 0.95f)
    val sheetAnchorAlpha = (0.12f + opacityNorm * 0.14f).coerceIn(0.14f, 0.28f)
    val dismissInteraction = remember { MutableInteractionSource() }
    var sheetRestOffset by remember(entry) { mutableStateOf(Offset.Zero) }

    Box(modifier = Modifier.fillMaxSize()) {
        // 1. Sharp unblurred underlying screen softly dimmed behind the optical glass lens
        Box(
            modifier = Modifier
                .fillMaxSize()
                .blockUnderlyingTouches()
                .background(Color.Black.copy(alpha = (if (palette.isLiquidGlass) 0.26f else 0.48f) * scrimAlpha.value))
                .clickable(
                    interactionSource = dismissInteraction,
                    indication = null,
                    onClick = dismissAnimated,
                ),
        )

        // 2. Floating Bottom Sheet Portal Container (outer box measures true rest coords without transform)
        Box(
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .padding(horizontal = if (palette.isLiquidGlass) 8.dp else 0.dp)
                .navigationBarsPadding()
                .imePadding()
                .padding(bottom = if (palette.isLiquidGlass) 8.dp else 0.dp)
                .heightIn(max = maxSheetHeight)
                .onGloballyPositioned { coords ->
                    sheetRestOffset = coords.positionInRoot()
                }
                .onSizeChanged { if (it.height > 0) sheetHeightPx = it.height.toFloat() },
        ) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .graphicsLayer {
                        translationY = offsetY.value.coerceAtLeast(0f)
                    }
                    .nestedScroll(nestedScrollConnection)
                    .blockUnderlyingTouches(),
            ) {
                // Layer A: Decoupled Liquid Glass backdrop shell (never redraws when scrolling sheet lists)
                if (palette.isLiquidGlass) {
                    CompositionLocalProvider(LocalBackdropState provides sheetBackdrop) {
                        Box(
                            modifier = Modifier
                                .matchParentSize()
                                .ferricLiquidGlass(
                                    cornerRadius = sheetCorner,
                                    blurRadius = 10.dp,
                                    refractionHeight = 24.dp,
                                    refractionAmount = 24.dp,
                                    chromaticDispersion = 0.96f,
                                    anchorAlphaOverride = sheetAnchorAlpha,
                                    alwaysGlass = true,
                                    tier = GlassTier.HERO_LENS,
                                    screenOffsetProvider = {
                                        Offset(
                                            x = sheetRestOffset.x,
                                            y = sheetRestOffset.y + offsetY.value.coerceAtLeast(0f),
                                        )
                                    },
                                ),
                        )
                    }
                } else {
                    Box(
                        modifier = Modifier
                            .matchParentSize()
                            .clip(solidShape)
                            .background(palette.surface)
                            .border(1.dp, palette.border, solidShape),
                    )
                }

                // Layer B: Sheet Handle + Body Content isolated cleanly over the Liquid Glass lens
                CompositionLocalProvider(LocalBackdropState provides null) {
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(if (palette.isLiquidGlass) liquidShape else solidShape),
                    ) {
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .pointerInput(entry) {
                                    detectVerticalDragGestures(
                                        onDragEnd = {
                                            if (offsetY.value > dismissThresholdPx) {
                                                dismissAnimated()
                                            } else if (!isClosing) {
                                                scope.launch {
                                                    offsetY.animateTo(0f, tween(FERRIC_MOTION_MS, easing = FerricEaseOut))
                                                }
                                            }
                                        },
                                        onDragCancel = {
                                            if (!isClosing) {
                                                scope.launch {
                                                    offsetY.animateTo(0f, tween(FERRIC_MOTION_MS, easing = FerricEaseOut))
                                                }
                                            }
                                        },
                                        onVerticalDrag = { _, dy ->
                                            if (!isClosing) {
                                                val next = (offsetY.value + dy).coerceAtLeast(0f)
                                                scope.launch { offsetY.snapTo(next) }
                                            }
                                        },
                                    )
                                }
                                .padding(top = 12.dp, bottom = 6.dp),
                            contentAlignment = Alignment.Center,
                        ) {
                            Box(
                                modifier = Modifier
                                    .width(42.dp)
                                    .height(4.dp)
                                    .clip(RoundedCornerShape(999.dp))
                                    .background(
                                        if (palette.isLiquidGlass) {
                                            Color.White.copy(alpha = 0.62f)
                                        } else {
                                            palette.textSecondary.copy(alpha = 0.45f)
                                        },
                                    ),
                            )
                        }
                        entry.content(this)
                        if (palette.isLiquidGlass) {
                            Spacer(Modifier.height(10.dp))
                        }
                    }
                }
            }
        }
    }
}

/**
 * Optical Liquid Glass bottom sheet container matching the Home pill's AGSL refraction, blur, and
 * chromatic dispersion when hosted inside [LocalFerricSheetHost]. Falls back cleanly to Material
 * [ModalBottomSheet] outside the main Activity tree.
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
    val host = LocalFerricSheetHost.current
    if (host != null) {
        val entry = remember {
            FerricSheetEntry(
                onDismiss = onDismissRequest,
                coverShade = coverShade,
                content = content,
            )
        }
        SideEffect {
            entry.onDismiss = onDismissRequest
            entry.coverShade = coverShade
            entry.content = content
        }
        DisposableEffect(host, entry) {
            host.add(entry)
            onDispose { host.remove(entry) }
        }
        return
    }

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
    val sheetBgAlpha = (0.26f + opacityNorm * 0.22f).coerceIn(0.24f, 0.52f)
    val sheetShape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)

    ModalBottomSheet(
        onDismissRequest = onDismissRequest,
        modifier = modifier,
        sheetState = sheetState,
        containerColor = Color.Transparent,
        scrimColor = Color.Black.copy(alpha = 0.14f),
        dragHandle = null,
    ) {
        val view = LocalView.current
        SideEffect {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                val window = (view.parent as? DialogWindowProvider)?.window
                if (window != null) {
                    runCatching {
                        window.setBackgroundBlurRadius(76)
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
                                    coverShade.accent.copy(alpha = 0.14f),
                                    coverShade.orb1.copy(alpha = 0.14f),
                                    Color.Transparent,
                                ),
                                center = Offset(220f, 140f),
                                radius = 900f,
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

/**
 * Seals full-screen overlays against pointer passthrough onto underlying tabs or mini-player bars.
 * Consumes unhandled changes in [PointerEventPass.Main] (bottom-up) after interactive children
 * and scroll containers run, avoiding interference with child [PointerEventPass.Final] checks.
 */
fun Modifier.blockUnderlyingTouches(): Modifier = pointerInput(Unit) {
    awaitPointerEventScope {
        while (true) {
            val event = awaitPointerEvent(PointerEventPass.Main)
            event.changes.forEach { change ->
                if (!change.isConsumed) {
                    change.consume()
                }
            }
        }
    }
}
