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
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.layout.onSizeChanged
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import dev.livelydesk.ferric.ui.theme.CoverShadeColors
import kotlin.math.PI
import kotlin.math.cos
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
            kotlinx.coroutines.delay(64L)
            phaseState.floatValue = ((phaseState.floatValue + 0.022f) % (2.0 * PI)).toFloat()
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
        drawRect(baseCanvas)
        val w = size.width
        val h = size.height
        if (w <= 0f || h <= 0f) return@Canvas

        // Read animated value inside draw phase only and reuse cached brushes (0 per-frame allocations).
        val phase = phaseState.value
        val c1x = w * (0.28f + 0.14f * cos(phase))
        val c1y = h * (0.18f + 0.10f * sin(phase))
        val c2x = w * (0.78f + 0.12f * sin(phase * 0.8f))
        val c2y = h * (0.56f + 0.12f * cos(phase * 0.9f))
        val c3x = w * (0.44f + 0.15f * cos(phase * 1.1f + 1.4f))
        val c3y = h * (0.84f + 0.08f * sin(phase * 0.7f))

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
}
