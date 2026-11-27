package dev.livelydesk.ferric.ui.components

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import dev.livelydesk.ferric.ui.theme.CoverShadeColors
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.sin

/**
 * Battery-safe Aurora ambient backdrop driven by `CoverShadeColors`.
 * Automatically freezes its infinite animation when paused OR when the Activity leaves `RESUMED`
 * (`ON_STOP` / screen off) so GPU utilization drops to 0% in standby.
 */
@Composable
fun AuroraBackdrop(
    colors: CoverShadeColors,
    baseCanvas: Color,
    isPlaying: Boolean,
    intensity: Float = 0.65f,
    modifier: Modifier = Modifier,
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
    val phase = if (animate) {
        val transition = rememberInfiniteTransition(label = "aurora")
        val p by transition.animateFloat(
            initialValue = 0f,
            targetValue = (2.0 * PI).toFloat(),
            animationSpec = infiniteRepeatable(
                animation = tween(durationMillis = 16_000, easing = LinearEasing),
                repeatMode = RepeatMode.Restart,
            ),
            label = "aurora_phase",
        )
        p
    } else {
        0.6f
    }

    Canvas(modifier = modifier.fillMaxSize()) {
        drawRect(baseCanvas)
        val w = size.width
        val h = size.height
        if (w <= 0f || h <= 0f) return@Canvas

        val c1 = Offset(
            x = w * (0.28f + 0.16f * cos(phase)),
            y = h * (0.22f + 0.12f * sin(phase)),
        )
        val c2 = Offset(
            x = w * (0.76f + 0.14f * sin(phase * 0.8f)),
            y = h * (0.58f + 0.15f * cos(phase * 0.9f)),
        )
        val c3 = Offset(
            x = w * (0.45f + 0.18f * cos(phase * 1.1f + 1.4f)),
            y = h * (0.82f + 0.10f * sin(phase * 0.7f)),
        )
        val radius = maxOf(w, h) * 0.72f

        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(colors.orb1.copy(alpha = 0.52f * intensity), Color.Transparent),
                center = c1,
                radius = radius,
            ),
            center = c1,
            radius = radius,
        )
        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(colors.orb2.copy(alpha = 0.45f * intensity), Color.Transparent),
                center = c2,
                radius = radius,
            ),
            center = c2,
            radius = radius,
        )
        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(colors.orb3.copy(alpha = 0.34f * intensity), Color.Transparent),
                center = c3,
                radius = radius * 0.85f,
            ),
            center = c3,
            radius = radius * 0.85f,
        )
    }
}
