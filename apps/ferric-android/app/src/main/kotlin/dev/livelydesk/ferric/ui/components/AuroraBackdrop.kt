package dev.livelydesk.ferric.ui.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlinx.coroutines.delay
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.layer.CompositingStrategy as LayerCompositingStrategy
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntSize
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import dev.livelydesk.ferric.ui.theme.CoverShadeColors
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.roundToInt
import kotlin.math.sin

/**
 * Battery-safe Aurora ambient backdrop driven by [CoverShadeColors].
 * Reads animation phase strictly inside Canvas draw scope (zero per-frame recomposition) and
 * freezes cleanly when paused or when the Activity leaves `RESUMED`.
 */
@Composable
fun AuroraBackdrop(
    colors: CoverShadeColors,
    baseCanvas: Color,
    isPlaying: Boolean,
    modifier: Modifier = Modifier,
    intensity: Float = 0.65f,
) {
    val lifecycleOwner = LocalLifecycleOwner.current
    var isResumed by remember {
        mutableStateOf(lifecycleOwner.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED))
    }
    DisposableEffect(lifecycleOwner) {
        val obs = LifecycleEventObserver { _, _ ->
            isResumed = lifecycleOwner.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)
        }
        lifecycleOwner.lifecycle.addObserver(obs)
        onDispose { lifecycleOwner.lifecycle.removeObserver(obs) }
    }

    val animate = isPlaying && isResumed
    val phaseState = remember { mutableFloatStateOf(0.6f) }
    androidx.compose.runtime.LaunchedEffect(animate) {
        if (!animate) return@LaunchedEffect
        while (true) {
            kotlinx.coroutines.delay(80L)
            phaseState.floatValue = ((phaseState.floatValue + 0.025f) % (2.0 * PI)).toFloat()
        }
    }

    var canvasRadius by remember { mutableFloatStateOf(720f) }
    val orb1Brush = remember(colors.orb1, intensity, canvasRadius) {
        Brush.radialGradient(
            colors = listOf(colors.orb1.copy(alpha = 0.50f * intensity), Color.Transparent),
            center = Offset.Zero,
            radius = canvasRadius,
        )
    }
    val orb2Brush = remember(colors.orb2, intensity, canvasRadius) {
        Brush.radialGradient(
            colors = listOf(colors.orb2.copy(alpha = 0.42f * intensity), Color.Transparent),
            center = Offset.Zero,
            radius = canvasRadius,
        )
    }
    val orb3Brush = remember(colors.orb3, intensity, canvasRadius) {
        val r3 = (canvasRadius * 0.85f).coerceAtLeast(1f)
        Brush.radialGradient(
            colors = listOf(colors.orb3.copy(alpha = 0.30f * intensity), Color.Transparent),
            center = Offset.Zero,
            radius = r3,
        )
    }

    val cachedLayer = rememberGraphicsLayer()
    val cacheState = remember { AuroraStaticCache() }

    Canvas(
        modifier = modifier
            .fillMaxSize()
            .onSizeChanged {
                val r = maxOf(it.width, it.height) * 0.72f
                if (r > 16f && kotlin.math.abs(r - canvasRadius) > 24f) {
                    canvasRadius = r
                }
            },
    ) {
        val w = size.width
        val h = size.height
        if (w <= 0f || h <= 0f) return@Canvas

        val phase = phaseState.floatValue
        if (!animate && w >= 16f && h >= 16f) {
            val cacheScale = 0.25f
            val cw = (w * cacheScale).roundToInt().coerceAtLeast(16)
            val ch = (h * cacheScale).roundToInt().coerceAtLeast(16)
            val targetSize = IntSize(cw, ch)
            if (!cacheState.valid ||
                cacheState.size != targetSize ||
                cacheState.colors != colors ||
                cacheState.baseCanvas != baseCanvas ||
                cacheState.intensity != intensity ||
                cacheState.canvasRadius != canvasRadius
            ) {
                cachedLayer.compositingStrategy = LayerCompositingStrategy.Offscreen
                cachedLayer.record(size = targetSize) {
                    scale(scaleX = cacheScale, scaleY = cacheScale, pivot = Offset.Zero) {
                        drawAuroraGradients(
                            width = w,
                            height = h,
                            baseCanvas = baseCanvas,
                            canvasRadius = canvasRadius,
                            phase = phase,
                            orb1Brush = orb1Brush,
                            orb2Brush = orb2Brush,
                            orb3Brush = orb3Brush,
                        )
                    }
                }
                cacheState.valid = true
                cacheState.size = targetSize
                cacheState.colors = colors
                cacheState.baseCanvas = baseCanvas
                cacheState.intensity = intensity
                cacheState.canvasRadius = canvasRadius
            }
            scale(scaleX = w / cw.toFloat(), scaleY = h / ch.toFloat(), pivot = Offset.Zero) {
                drawLayer(cachedLayer)
            }
            return@Canvas
        }

        drawAuroraGradients(
            width = w,
            height = h,
            baseCanvas = baseCanvas,
            canvasRadius = canvasRadius,
            phase = phase,
            orb1Brush = orb1Brush,
            orb2Brush = orb2Brush,
            orb3Brush = orb3Brush,
        )
    }
}

private class AuroraStaticCache {
    var valid: Boolean = false
    var size: IntSize = IntSize.Zero
    var colors: CoverShadeColors? = null
    var baseCanvas: Color = Color.Unspecified
    var intensity: Float = -1f
    var canvasRadius: Float = -1f
}

private fun DrawScope.drawAuroraGradients(
    width: Float,
    height: Float,
    baseCanvas: Color,
    canvasRadius: Float,
    phase: Float,
    orb1Brush: Brush,
    orb2Brush: Brush,
    orb3Brush: Brush,
) {
    drawRect(baseCanvas)
    val c1x = width * (0.28f + 0.14f * cos(phase))
    val c1y = height * (0.18f + 0.10f * sin(phase))
    val c2x = width * (0.78f + 0.12f * sin(phase * 0.8f))
    val c2y = height * (0.56f + 0.12f * cos(phase * 0.9f))
    val c3x = width * (0.44f + 0.15f * cos(phase * 1.1f + 1.4f))
    val c3y = height * (0.84f + 0.08f * sin(phase * 0.7f))

    translate(left = c1x, top = c1y) {
        drawCircle(brush = orb1Brush, radius = canvasRadius, center = Offset.Zero)
    }
    translate(left = c2x, top = c2y) {
        drawCircle(brush = orb2Brush, radius = canvasRadius, center = Offset.Zero)
    }
    translate(left = c3x, top = c3y) {
        drawCircle(brush = orb3Brush, radius = canvasRadius * 0.85f, center = Offset.Zero)
    }
}
