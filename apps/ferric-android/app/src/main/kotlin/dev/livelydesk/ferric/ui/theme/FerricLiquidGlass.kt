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
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
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
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.asComposeRenderEffect
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.node.ModifierNodeElement
import androidx.compose.ui.node.invalidateDraw
import androidx.compose.ui.platform.InspectorInfo
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.Velocity
import kotlinx.coroutines.CoroutineStart
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
import kotlin.math.ceil
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/**
 * Rendering fidelity tier for optical Liquid Glass:
 * - [HERO_LENS]: Single-pass 1:1 padded RenderEffect chain (`ColorFilter -> Blur -> AGSL Lens`)
 *   for iOS 26 optical refraction, chromatic dispersion, luminous frosted crystal veil, and
 *   45-degree additive specular rim highlights without intermediate blur buffers or tile flicker.
 * - [FROSTED_PANEL]: Zero-FBO translucent frosted crystal panel (crystal veil + diagonal
 *   sheen + grounded floor + specular rim) for cards, buttons, and chips inside scrollable feeds.
 */
enum class GlassTier {
    HERO_LENS,
    FROSTED_PANEL,
}

internal interface FerricBackdropObserver {
    fun onBackdropUpdated()
}

/**
 * Shared 1:1 hardware backdrop capture state for optical glass.
 * Records the underlying screen once into [graphicsLayer] and invalidates subscribed sibling
 * [HERO_LENS] nodes strictly when the source content actually redraws (zero overhead when idle).
 */
@Stable
class FerricBackdropState(
    val graphicsLayer: GraphicsLayer,
) {
    var rootOffset: Offset = Offset.Zero
    var hasRecorded by mutableStateOf(false)
    internal var recordedFlag: Boolean = false
    var isRecordingSource: Boolean = false
    var contentSize: IntSize = IntSize.Zero
    var version: Long = 0L

    private val observers = LinkedHashSet<FerricBackdropObserver>()
    private var notifyingObservers: Boolean = false

    internal fun attachObserver(observer: FerricBackdropObserver) {
        observers.add(observer)
    }

    internal fun detachObserver(observer: FerricBackdropObserver) {
        observers.remove(observer)
    }

    internal fun notifyObservers() {
        if (notifyingObservers || observers.isEmpty()) return
        notifyingObservers = true
        try {
            // Snapshot count-safe iteration without allocating a copy on hot scroll frames.
            val iterator = observers.iterator()
            while (iterator.hasNext()) {
                iterator.next().onBackdropUpdated()
            }
        } finally {
            notifyingObservers = false
        }
    }
}

val LocalBackdropState = compositionLocalOf<FerricBackdropState?> { null }

@Composable
fun rememberFerricBackdropState(): FerricBackdropState {
    val rawLayer = rememberGraphicsLayer()
    return remember(rawLayer) { FerricBackdropState(rawLayer) }
}

private class FerricBackdropTargetNode(
    var backdrop: FerricBackdropState?,
) : Modifier.Node(), DrawModifierNode, FerricBackdropObserver {
    private var subscribedTo: FerricBackdropState? = null

    override fun onAttach() {
        syncSubscription()
    }

    override fun onDetach() {
        subscribedTo?.detachObserver(this)
        subscribedTo = null
    }

    fun updateBackdrop(next: FerricBackdropState?) {
        if (backdrop === next) return
        backdrop = next
        if (isAttached) {
            syncSubscription()
        }
    }

    private fun syncSubscription() {
        val prev = subscribedTo
        val next = backdrop
        if (prev === next) return
        prev?.detachObserver(this)
        next?.attachObserver(this)
        subscribedTo = next
    }

    override fun onBackdropUpdated() {
        if (isAttached) {
            invalidateDraw()
        }
    }

    override fun ContentDrawScope.draw() {
        drawContent()
    }
}

private class FerricBackdropTargetElement(
    private val backdrop: FerricBackdropState?,
) : ModifierNodeElement<FerricBackdropTargetNode>() {
    override fun create(): FerricBackdropTargetNode = FerricBackdropTargetNode(backdrop)

    override fun update(node: FerricBackdropTargetNode) {
        node.updateBackdrop(backdrop)
    }

    override fun InspectorInfo.inspectableProperties() {
        name = "ferricBackdropTarget"
    }

    override fun equals(other: Any?): Boolean =
        other is FerricBackdropTargetElement && backdrop === other.backdrop

    override fun hashCode(): Int = backdrop?.hashCode() ?: 0
}

/**
 * Captures the underlying scrollable surface at 1:1 screen resolution into [backdrop] so sibling
 * [HERO_LENS] overlays above it can render crisp single-pass optical Liquid Glass refraction.
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
            if (enabled && !backdrop.isRecordingSource && size.width >= 1f && size.height >= 1f) {
                backdrop.isRecordingSource = true
                try {
                    backdrop.graphicsLayer.record {
                        this@drawWithContent.drawContent()
                    }
                    backdrop.contentSize = IntSize(
                        size.width.roundToInt().coerceAtLeast(1),
                        size.height.roundToInt().coerceAtLeast(1),
                    )
                    backdrop.version++
                    if (!backdrop.recordedFlag) {
                        backdrop.recordedFlag = true
                        backdrop.hasRecorded = true
                    }
                } finally {
                    backdrop.isRecordingSource = false
                }
                drawLayer(backdrop.graphicsLayer)
                backdrop.notifyObservers()
            } else {
                drawContent()
            }
        }

// Continuous full-surface iOS 26 convex optical Liquid Glass lens + smooth rim dispersion AGSL shader (API 33+).
private const val REFRACTION_DISPERSION_AGSL = """
uniform shader content;
uniform float2 bufferSize;
uniform float2 halfSize;
uniform float minHalf;
uniform float2 offset;
uniform float4 cornerRadii;
uniform float refractionHeight;
uniform float refractionAmount;
uniform float depthEffect;
uniform float chromaticAberration;

float radiusAt(float2 coord, float4 radii) {
    if (coord.x >= 0.0) {
        if (coord.y <= 0.0) return radii.y;
        else return radii.z;
    } else {
        if (coord.y <= 0.0) return radii.x;
        else return radii.w;
    }
}

float sdRoundedRect(float2 coord, float2 halfSize, float radius) {
    float2 cornerCoord = abs(coord) - (halfSize - float2(radius));
    float outside = length(max(cornerCoord, 0.0)) - radius;
    float inside = min(max(cornerCoord.x, cornerCoord.y), 0.0);
    return outside + inside;
}

float2 gradSdRoundedRect(float2 coord, float2 halfSize, float radius) {
    float2 cornerCoord = abs(coord) - (halfSize - float2(radius));
    if (cornerCoord.x >= 0.0 || cornerCoord.y >= 0.0) {
        return sign(coord) * normalize(max(cornerCoord, 0.0) + float2(1e-4));
    } else {
        float gradX = step(cornerCoord.y, cornerCoord.x);
        return sign(coord) * float2(gradX, 1.0 - gradX);
    }
}

half4 main(float2 coord) {
    float2 pillCoord = coord + offset;
    float2 centeredCoord = pillCoord - halfSize;
    float radius = radiusAt(centeredCoord, cornerRadii);

    float sd = min(sdRoundedRect(centeredCoord, halfSize, radius), 0.0);
    float distIn = -sd;

    // 1. Symmetrical inward meniscus profile along all sides & rounded corners (finite slope -> clean rims)
    float edgeT = clamp(1.0 - (distIn / max(refractionHeight, 1.0)), 0.0, 1.0);
    float rimSmooth = edgeT * edgeT * (3.0 - 2.0 * edgeT);
    float meniscus = rimSmooth * (0.42 + 0.58 * edgeT);

    // 2. Continuous full-surface convex lens dome extending from rim through the entire center
    float depthFrac = clamp(distIn / max(minHalf, 1.0), 0.0, 1.0);
    float2 aspectCoord = centeredCoord / max(halfSize, float2(1.0));
    float radialSq = clamp(dot(aspectCoord, aspectCoord), 0.0, 1.0);
    float centerDome = (1.0 - radialSq * 0.55) * depthFrac;

    // Strictly symmetrical boundary normal + subtle aspect-normalized radial dome blend (zero L/R skew)
    float gradRadius = min(radius, minHalf);
    float2 gradBase = gradSdRoundedRect(centeredCoord, halfSize, gradRadius);
    float2 domeDir = dot(aspectCoord, aspectCoord) > 1e-5 ? normalize(aspectCoord) : float2(0.0);
    float2 grad = depthEffect > 0.5
        ? normalize(gradBase + domeDir * (0.20 * (1.0 - depthFrac)))
        : gradBase;

    // Inward optical refraction (meniscus bevel) + smooth center-to-edge magnification convergence
    float2 rimBend = -grad * (meniscus * refractionAmount);
    float2 interiorMag = -centeredCoord * (0.048 * centerDome);
    float2 safeMax = max(bufferSize - float2(0.5), float2(1.0));
    float2 refractedCoord = clamp(coord + rimBend + interiorMag, float2(0.5), safeMax);

    if (chromaticAberration <= 0.001) {
        return content.eval(refractedCoord);
    }

    // Refined crystal prism dispersion peaking cleanly at rounded corners & bevels with subtle center continuity
    float cornerFactor = clamp(abs(aspectCoord.x) * abs(aspectCoord.y) * 1.45, 0.0, 1.0);
    float dispStrength = chromaticAberration * (0.22 * (1.0 - depthFrac * 0.72) + 0.78 * meniscus * (0.35 + 0.65 * cornerFactor));
    float2 chromaDir = dot(grad, grad) > 1e-4 ? grad : domeDir;
    float2 dispersedCoord = chromaDir * (dispStrength * 3.4);

    half4 refr = content.eval(refractedCoord);
    half4 refrRed = content.eval(clamp(refractedCoord + dispersedCoord, float2(0.5), safeMax));
    half4 refrBlue = content.eval(clamp(refractedCoord - dispersedCoord, float2(0.5), safeMax));
    return half4(refrRed.r, refr.g, refrBlue.b, refr.a);
}
"""

// Directional 45-degree specular rim catchlight + ambient bounce AGSL shader.
private const val SPECULAR_RIM_AGSL = """
uniform float2 halfSize;
uniform float minHalf;
uniform float cornerRadius;
uniform float2 lightDir;
uniform float falloff;
uniform float4 topColor;
uniform float4 bottomColor;

float2 gradSdRoundedRect(float2 coord, float2 halfSize, float radius) {
    float2 cornerCoord = abs(coord) - (halfSize - float2(radius));
    if (cornerCoord.x >= 0.0 || cornerCoord.y >= 0.0) {
        return sign(coord) * normalize(max(cornerCoord, 0.0) + float2(1e-5));
    } else {
        float gradX = step(cornerCoord.y, cornerCoord.x);
        return sign(coord) * float2(gradX, 1.0 - gradX);
    }
}

half4 main(float2 coord) {
    float2 centered = coord - halfSize;
    float r = min(cornerRadius * 1.5, minHalf);
    float2 g = gradSdRoundedRect(centered, halfSize, r);
    float d = dot(g, lightDir);
    float intensity = pow(abs(d), falloff);
    float4 rim = d >= 0.0 ? topColor : bottomColor;
    float a = clamp(rim.a * (0.14 + intensity * 0.86), 0.0, 1.0);
    return half4(rim.rgb * a, a);
}
"""

private class GlassCache {
    var elementOffset by mutableStateOf(Offset.Zero)
    var lastSize: IntSize = IntSize.Zero
    var lastPadPx: Int = -1
    var lastRadiusPx: Float = -1f
    var lastBlurPx: Float = -1f
    var lastSaturation: Float = -1f
    var lastRefractionHeightPx: Float = -1f
    var lastRefractionAmountPx: Float = -1f
    var lastLightMode: Boolean = false
    var lastDispersion: Float = -1f
    var lastRecordedVersion: Long = -1L
    var lastDx: Float = Float.NaN
    var lastDy: Float = Float.NaN
    var cachedEffect: androidx.compose.ui.graphics.RenderEffect? = null
    var refractionShader: Any? = null
    var rimShader: Any? = null
    var cachedRimBrush: ShaderBrush? = null
    var cachedPath: Path = Path()
    var cachedPathSize: Size = Size.Zero
    var cachedPathRadius: Float = -1f
}

@RequiresApi(Build.VERSION_CODES.S)
private fun buildChainedGlassRenderEffect(
    cache: GlassCache,
    paddedWidthPx: Float,
    paddedHeightPx: Float,
    widthPx: Float,
    heightPx: Float,
    padPx: Float,
    radiusPx: Float,
    blurPx: Float,
    saturation: Float,
    refractionHeightPx: Float,
    refractionAmountPx: Float,
    dispersion: Float,
): androidx.compose.ui.graphics.RenderEffect {
    val satMatrix = ColorMatrix().apply {
        setSaturation(saturation.coerceIn(1.0f, 2.2f))
    }
    val satEffect = RenderEffect.createColorFilterEffect(ColorMatrixColorFilter(satMatrix))
    val clampedBlur = blurPx.coerceIn(1f, 64f)
    val blurEffect = RenderEffect.createBlurEffect(
        clampedBlur,
        clampedBlur,
        satEffect,
        Shader.TileMode.CLAMP,
    )
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
        refractionAmountPx <= 0.5f ||
        refractionHeightPx <= 0.5f
    ) {
        return blurEffect.asComposeRenderEffect()
    }
    val shader = (cache.refractionShader as? RuntimeShader)
        ?: RuntimeShader(REFRACTION_DISPERSION_AGSL).also { cache.refractionShader = it }
    val halfW = (widthPx * 0.5f).coerceAtLeast(1f)
    val halfH = (heightPx * 0.5f).coerceAtLeast(1f)
    val minHalf = min(halfW, halfH)
    val clampedRadius = radiusPx.coerceAtMost(minHalf)
    shader.setFloatUniform("bufferSize", paddedWidthPx, paddedHeightPx)
    shader.setFloatUniform("halfSize", halfW, halfH)
    shader.setFloatUniform("minHalf", minHalf)
    shader.setFloatUniform("offset", -padPx, -padPx)
    shader.setFloatUniform("cornerRadii", clampedRadius, clampedRadius, clampedRadius, clampedRadius)
    shader.setFloatUniform("refractionHeight", refractionHeightPx.coerceIn(4f, minHalf))
    shader.setFloatUniform("refractionAmount", refractionAmountPx.coerceAtLeast(0f))
    shader.setFloatUniform("depthEffect", 1.0f)
    shader.setFloatUniform("chromaticAberration", dispersion.coerceIn(0f, 1.25f))
    val lensEffect = RenderEffect.createRuntimeShaderEffect(shader, "content")
    return RenderEffect.createChainEffect(lensEffect, blurEffect).asComposeRenderEffect()
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
    shader.setFloatUniform("lightDir", -0.70710677f, -0.70710677f)
    shader.setFloatUniform("falloff", 1.0f)
    if (isLight) {
        shader.setFloatUniform("topColor", 1.0f, 1.0f, 1.0f, 0.90f)
        shader.setFloatUniform("bottomColor", 0.06f, 0.07f, 0.09f, 0.16f)
    } else {
        shader.setFloatUniform("topColor", 1.0f, 1.0f, 1.0f, 0.62f)
        shader.setFloatUniform("bottomColor", 0.90f, 0.94f, 1.0f, 0.34f)
    }
    return ShaderBrush(shader).also { cache.cachedRimBrush = it }
}

/**
 * Optical Liquid Glass modifier for iOS 26-style lens refraction.
 *
 * When [tier] is [GlassTier.HERO_LENS] with a live [LocalBackdropState], executes ONE padded,
 * full-resolution chained `RenderEffect` (`ColorFilter(saturation) -> Blur -> AGSL Lens`) directly
 * from the parent 1:1 backdrop capture, eliminating multi-layer FBO flicker and edge bleed.
 * When [tier] is [GlassTier.FROSTED_PANEL] (or no live backdrop exists), renders translucent
 * frosted crystal glass with directional sheen and 45-degree specular rim.
 */
fun Modifier.ferricLiquidGlass(
    cornerRadius: Dp = 28.dp,
    blurRadius: Dp = 12.dp,
    refractionHeight: Dp = 16.dp,
    refractionAmount: Dp = 10.dp,
    chromaticDispersion: Float = 0.60f,
    tintAlpha: Float? = null,
    alwaysGlass: Boolean = false,
    tier: GlassTier = GlassTier.HERO_LENS,
    accentGlow: Color = Color.Unspecified,
    anchorAlphaOverride: Float? = null,
    screenOffsetProvider: (() -> Offset)? = null,
    saturation: Float = 1.55f,
    veilScale: Float = 0.76f,
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
    val glassLayer = if (isHero && backdrop != null) rememberGraphicsLayer() else null
    val cache = remember { GlassCache() }
    val shape = RoundedCornerShape(cornerRadius)
    val opacityNorm = palette.glassOpacity.coerceIn(0.20f, 0.95f)
    // Gentle linear range across 20%..95% frost slider so turning opacity up stays airy crystal
    // glass instead of turning into heavy near-opaque black.
    val opacityUserScale = (0.72f + (opacityNorm - 0.50f) * 0.45f).coerceIn(0.58f, 1.05f)

    // Luminous iOS 26 crystal veil: keep underlying vivid colors bright and legible
    // without drowning the glass in black soot.
    val baseVeilAlpha = if (palette.isLight) 0.54f else 0.56f
    val heroVeilAlpha = when {
        anchorAlphaOverride != null -> anchorAlphaOverride.coerceIn(0f, 0.85f)
        tintAlpha != null -> tintAlpha.coerceIn(0.05f, 0.85f)
        else -> (baseVeilAlpha * veilScale.coerceIn(0.25f, 1.15f) * opacityUserScale)
            .coerceIn(0.14f, if (palette.isLight) 0.66f else 0.48f)
    }
    val horizonDarkBase = lerp(Color(0xFF101216), palette.glassTint, 0.22f)
    val heroVeilColor = if (palette.isLight) {
        Color.White.copy(alpha = heroVeilAlpha)
    } else {
        horizonDarkBase.copy(alpha = heroVeilAlpha)
    }

    val frostedVeilAlpha = if (palette.isLight) {
        (0.70f * opacityUserScale).coerceIn(0.52f, 0.88f)
    } else {
        (0.22f * opacityUserScale).coerceIn(0.14f, 0.32f)
    }
    val frostedBaseColor = if (palette.isLight) {
        Color.White.copy(alpha = frostedVeilAlpha)
    } else {
        lerp(Color(0xFF15171C), palette.glassTint, 0.22f).copy(alpha = frostedVeilAlpha)
    }

    val sheenBrush = remember(palette.isLight, isHero, accentGlow) {
        if (accentGlow != Color.Unspecified) {
            Brush.verticalGradient(
                0.0f to Color.White.copy(alpha = if (isHero) 0.16f else 0.12f),
                0.35f to accentGlow.copy(alpha = 0.08f),
                0.72f to Color.Transparent,
                1.0f to Color.White.copy(alpha = if (palette.isLight) 0.10f else 0.05f),
            )
        } else {
            Brush.verticalGradient(
                0.0f to Color.White.copy(alpha = if (palette.isLight) 0.34f else if (isHero) 0.15f else 0.10f),
                0.38f to Color.Transparent,
                0.75f to Color.Transparent,
                1.0f to Color.White.copy(alpha = if (palette.isLight) 0.14f else if (isHero) 0.06f else 0.04f),
            )
        }
    }
    val rimStrokeBrush = remember(palette.isLight, accentGlow) {
        Brush.linearGradient(
            colors = if (palette.isLight) {
                listOf(
                    Color.White.copy(alpha = 0.90f),
                    Color.White.copy(alpha = 0.32f),
                    Color(0xFF1F222A).copy(alpha = 0.16f),
                )
            } else {
                listOf(
                    Color.White.copy(alpha = 0.56f),
                    Color.White.copy(alpha = 0.12f),
                    (if (accentGlow != Color.Unspecified) accentGlow else Color.White).copy(alpha = 0.34f),
                )
            },
        )
    }

    this
        .then(
            if (isHero && backdrop != null) {
                FerricBackdropTargetElement(backdrop)
            } else {
                Modifier
            },
        )
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
            val blurScale = (0.76f + opacityNorm * 0.42f).coerceIn(0.65f, 1.30f)
            val blurPx = (blurRadius.toPx() * blurScale).coerceIn(5f, 64f)
            val halfMinPx = min(w, h) * 0.5f
            val refHeightPx = refractionHeight.toPx().coerceAtMost(halfMinPx * 0.68f)
            val refAmountPx = refractionAmount.toPx().coerceAtMost(halfMinPx * 0.44f)

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

            val rimGeometryDirty = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && (
                cache.cachedRimBrush == null ||
                    cache.lastSize != intSize ||
                    cache.lastRadiusPx != radiusPx ||
                    cache.lastLightMode != palette.isLight
                )
            if (rimGeometryDirty) {
                buildApi33RimBrush(
                    cache = cache,
                    widthPx = w,
                    heightPx = h,
                    radiusPx = radiusPx,
                    isLight = palette.isLight,
                )
            }

            val activeBackdrop = backdrop
            val canLiveRefract = isHero &&
                glassLayer != null &&
                activeBackdrop != null &&
                !activeBackdrop.isRecordingSource &&
                activeBackdrop.hasRecorded &&
                activeBackdrop.contentSize.width >= 4 &&
                activeBackdrop.contentSize.height >= 4 &&
                Build.VERSION.SDK_INT >= Build.VERSION_CODES.S

            if (canLiveRefract) {
                val elementPos = screenOffsetProvider?.invoke() ?: cache.elementOffset
                val dx = elementPos.x - activeBackdrop.rootOffset.x
                val dy = elementPos.y - activeBackdrop.rootOffset.y
                val padPx = ceil(max(blurPx, refAmountPx * 1.45f).toDouble()).toInt().coerceIn(10, 72)
                val padF = padPx.toFloat()
                val paddedW = (intSize.width + 2 * padPx).coerceAtLeast(1)
                val paddedH = (intSize.height + 2 * padPx).coerceAtLeast(1)

                val effectDirty = cache.cachedEffect == null ||
                    cache.lastSize != intSize ||
                    cache.lastPadPx != padPx ||
                    abs(cache.lastRadiusPx - radiusPx) > 0.25f ||
                    abs(cache.lastBlurPx - blurPx) > 0.35f ||
                    abs(cache.lastSaturation - saturation) > 0.02f ||
                    abs(cache.lastRefractionHeightPx - refHeightPx) > 0.25f ||
                    abs(cache.lastRefractionAmountPx - refAmountPx) > 0.25f ||
                    abs(cache.lastDispersion - chromaticDispersion) > 0.02f ||
                    cache.lastLightMode != palette.isLight

                if (effectDirty) {
                    cache.lastSize = intSize
                    cache.lastPadPx = padPx
                    cache.lastRadiusPx = radiusPx
                    cache.lastBlurPx = blurPx
                    cache.lastSaturation = saturation
                    cache.lastRefractionHeightPx = refHeightPx
                    cache.lastRefractionAmountPx = refAmountPx
                    cache.lastDispersion = chromaticDispersion
                    cache.lastLightMode = palette.isLight
                    cache.cachedEffect = buildChainedGlassRenderEffect(
                        cache = cache,
                        paddedWidthPx = paddedW.toFloat(),
                        paddedHeightPx = paddedH.toFloat(),
                        widthPx = w,
                        heightPx = h,
                        padPx = padF,
                        radiusPx = radiusPx,
                        blurPx = blurPx,
                        saturation = saturation,
                        refractionHeightPx = refHeightPx,
                        refractionAmountPx = refAmountPx,
                        dispersion = chromaticDispersion,
                    )
                }

                val recordNeeded = effectDirty ||
                    cache.lastRecordedVersion != activeBackdrop.version ||
                    abs(cache.lastDx - dx) > 0.5f ||
                    abs(cache.lastDy - dy) > 0.5f
                if (recordNeeded) {
                    cache.lastRecordedVersion = activeBackdrop.version
                    cache.lastDx = dx
                    cache.lastDy = dy
                    glassLayer.clip = true
                    glassLayer.renderEffect = cache.cachedEffect
                    glassLayer.record(size = IntSize(paddedW, paddedH)) {
                        // Solid canvas anchor ensures blur & lens bezel sampling NEVER touch alpha=0 pixels.
                        drawRect(color = palette.canvas)
                        translate(left = padF - dx, top = padF - dy) {
                            drawLayer(activeBackdrop.graphicsLayer)
                        }
                    }
                }

                clipPath(cache.cachedPath) {
                    translate(left = -padF, top = -padF) {
                        drawLayer(glassLayer)
                    }
                    if (heroVeilColor.alpha > 0.005f) {
                        drawRect(color = heroVeilColor)
                    }
                    if (accentGlow != Color.Unspecified) {
                        drawRect(color = accentGlow.copy(alpha = 0.08f))
                    }
                    drawRect(brush = sheenBrush)
                }
            } else {
                // FrostColors crystal panel fallback when no live backdrop or below API 31.
                clipPath(cache.cachedPath) {
                    drawRect(color = frostedBaseColor)
                    if (accentGlow != Color.Unspecified) {
                        drawRect(color = accentGlow.copy(alpha = 0.12f))
                    }
                    drawRect(brush = sheenBrush)
                }
            }

            clipPath(cache.cachedPath) {
                this@drawWithContent.drawContent()
            }

            // Crisp 45-degree specular rim highlight drawn AFTER content so child backgrounds never hide it.
            val rimStrokePx = (if (isHero) 1.05.dp else 0.85.dp).toPx()
            val inset = rimStrokePx * 0.5f
            val rimBrush = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                cache.cachedRimBrush ?: rimStrokeBrush
            } else {
                rimStrokeBrush
            }
            val rimBlend = if (!palette.isLight && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                BlendMode.Plus
            } else {
                BlendMode.SrcOver
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
                blendMode = rimBlend,
            )
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
    val dismissThresholdPx = with(density) { 88.dp.toPx() }

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

    val applyDragDelta: (Float) -> Float = remember(entry) {
        { deltaY ->
            if (isClosing) {
                0f
            } else {
                val next = (offsetY.value + deltaY).coerceAtLeast(0f)
                val consumed = next - offsetY.value
                if (consumed != 0f) {
                    scope.launch(start = CoroutineStart.UNDISPATCHED) {
                        offsetY.snapTo(next)
                    }
                }
                consumed
            }
        }
    }

    val releaseDrag: (Float) -> Unit = remember(entry) {
        { velocityY ->
            if (!isClosing && offsetY.value > 0f) {
                if (offsetY.value > dismissThresholdPx || velocityY > 650f) {
                    dismissAnimated()
                } else {
                    scope.launch {
                        offsetY.animateTo(0f, tween(durationMillis = FERRIC_MOTION_MS, easing = FerricEaseOut))
                    }
                }
            }
        }
    }

    val nestedScrollConnection = remember(entry) {
        object : NestedScrollConnection {
            override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                if (isClosing || available.y >= 0f || offsetY.value <= 0f) return Offset.Zero
                return Offset(0f, applyDragDelta(available.y))
            }

            override fun onPostScroll(
                consumed: Offset,
                available: Offset,
                source: NestedScrollSource,
            ): Offset {
                if (isClosing || source != NestedScrollSource.UserInput || available.y <= 0f) {
                    return Offset.Zero
                }
                return Offset(0f, applyDragDelta(available.y))
            }

            override suspend fun onPreFling(available: Velocity): Velocity {
                if (offsetY.value <= 0f) return Velocity.Zero
                releaseDrag(available.y)
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
                    .draggable(
                        orientation = Orientation.Vertical,
                        state = rememberDraggableState { dy -> applyDragDelta(dy) },
                        onDragStopped = { velocityY -> releaseDrag(velocityY) },
                    )
                    .blockUnderlyingTouches(),
            ) {
                // Layer A: Single-pass Optical Liquid Glass shell (0 redraws when scrolling sheet lists)
                if (palette.isLiquidGlass) {
                    CompositionLocalProvider(LocalBackdropState provides sheetBackdrop) {
                        Box(
                            modifier = Modifier
                                .matchParentSize()
                                .ferricLiquidGlass(
                                    cornerRadius = sheetCorner,
                                    blurRadius = 15.dp,
                                    refractionHeight = 18.dp,
                                    refractionAmount = 11.dp,
                                    chromaticDispersion = 0.55f,
                                    saturation = 1.52f,
                                    veilScale = 0.56f,
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
                        Column(
                            modifier = Modifier
                                .weight(1f, fill = false)
                                .fillMaxWidth(),
                        ) {
                            entry.content(this)
                        }
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
 * Seals overlays against pointer passthrough onto underlying siblings without consuming DOWN or
 * pre-slop MOVE events, preserving nested scroll containers, drag handles, and sliders intact.
 */
fun Modifier.blockUnderlyingTouches(): Modifier = pointerInput(Unit) {
    detectTapGestures(onTap = {})
}
