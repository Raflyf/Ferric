package dev.livelydesk.ferric.ui.components

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.livelydesk.ferric.ui.theme.FerricEaseOut
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.cos
import kotlin.math.sin

/**
 * Sleek Auralys-inspired cold launch reveal animation built with authentic Ferric Type-I Cassette
 * copper geometry. Combines staggered icon motion + fade-in with sequential character reveal
 * (~1.4s hold + smooth 380ms dissolve) before detaching completely from Composition.
 */
@Composable
fun FerricLaunchSplash(
    onFinished: () -> Unit,
) {
    val enterScale = remember { Animatable(0.72f) }
    val enterAlpha = remember { Animatable(0f) }
    val iconRotation = remember { Animatable(-20f) }
    val iconOffsetY = remember { Animatable(32f) }
    val reelSpin = remember { Animatable(0f) }
    val wordReveal = remember { Animatable(0f) }
    val taglineAlpha = remember { Animatable(0f) }
    val exitAlpha = remember { Animatable(1f) }
    val exitScale = remember { Animatable(1f) }

    val brandLetters = remember { listOf("F", "e", "r", "r", "i", "c") }

    val ambientBrush = remember {
        Brush.radialGradient(
            colors = listOf(
                Color(0x55D9541E),
                Color(0x18AE7D57),
                Color.Transparent,
            ),
        )
    }

    LaunchedEffect(Unit) {
        launch {
            enterScale.animateTo(1f, tween(durationMillis = 780, easing = FerricEaseOut))
        }
        launch {
            enterAlpha.animateTo(1f, tween(durationMillis = 560, easing = FerricEaseOut))
        }
        launch {
            iconRotation.animateTo(-7f, tween(durationMillis = 920, easing = FerricEaseOut))
        }
        launch {
            iconOffsetY.animateTo(0f, tween(durationMillis = 700, easing = FerricEaseOut))
        }
        launch {
            reelSpin.animateTo(300f, tween(durationMillis = 1520, easing = FerricEaseOut))
        }
        launch {
            delay(280L)
            wordReveal.animateTo(1f, tween(durationMillis = 640, easing = LinearEasing))
        }
        launch {
            delay(760L)
            taglineAlpha.animateTo(1f, tween(durationMillis = 420, easing = FerricEaseOut))
        }
        delay(1420L)
        launch {
            exitScale.animateTo(1.06f, tween(durationMillis = 360, easing = FerricEaseOut))
        }
        exitAlpha.animateTo(0f, tween(durationMillis = 360, easing = FerricEaseOut))
        onFinished()
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .graphicsLayer {
                alpha = exitAlpha.value
                scaleX = exitScale.value
                scaleY = exitScale.value
            }
            .background(Color(0xFF08090D)),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            modifier = Modifier
                .size(240.dp)
                .graphicsLayer { alpha = enterAlpha.value * 0.90f }
                .background(ambientBrush),
        )

        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Canvas(
                modifier = Modifier
                    .size(116.dp)
                    .graphicsLayer {
                        alpha = enterAlpha.value
                        scaleX = enterScale.value
                        scaleY = enterScale.value
                        rotationZ = iconRotation.value
                        translationY = iconOffsetY.value
                    },
            ) {
                val u = size.minDimension / 100f
                val bodyW = 90f * u
                val bodyH = 60f * u
                val bodyLeft = (size.width - bodyW) * 0.5f
                val bodyTop = (size.height - bodyH) * 0.5f

                // Outer cassette shell + copper bevel rim
                drawRoundRect(
                    brush = Brush.linearGradient(
                        listOf(Color(0xFF443B35), Color(0xFF1A1614), Color(0xFF0D0C0C)),
                        start = Offset(bodyLeft, bodyTop),
                        end = Offset(bodyLeft + bodyW, bodyTop + bodyH),
                    ),
                    topLeft = Offset(bodyLeft, bodyTop),
                    size = Size(bodyW, bodyH),
                    cornerRadius = CornerRadius(9f * u, 9f * u),
                )
                drawRoundRect(
                    brush = Brush.linearGradient(
                        listOf(
                            Color(0xFFFFF1D1).copy(alpha = 0.90f),
                            Color(0xFFAE7D57).copy(alpha = 0.50f),
                            Color(0xFFD9541E).copy(alpha = 0.92f),
                        ),
                        start = Offset(bodyLeft, bodyTop),
                        end = Offset(bodyLeft + bodyW, bodyTop + bodyH),
                    ),
                    topLeft = Offset(bodyLeft, bodyTop),
                    size = Size(bodyW, bodyH),
                    cornerRadius = CornerRadius(9f * u, 9f * u),
                    style = Stroke(width = 1.6f * u),
                )

                // Sunset orange label band
                val lblLeft = bodyLeft + 6.5f * u
                val lblTop = bodyTop + 6f * u
                val lblW = bodyW - 13f * u
                drawRoundRect(
                    brush = Brush.horizontalGradient(
                        listOf(
                            Color(0xFF3A2016),
                            Color(0xFFBA441B),
                            Color(0xFFF6B570),
                        ),
                    ),
                    topLeft = Offset(lblLeft, lblTop),
                    size = Size(lblW, 13.5f * u),
                    cornerRadius = CornerRadius(4.5f * u, 4.5f * u),
                )

                // Cream silk wave on label
                val wavePath = Path().apply {
                    moveTo(lblLeft + 1f * u, lblTop + 9f * u)
                    cubicTo(
                        lblLeft + lblW * 0.30f,
                        lblTop + 2f * u,
                        lblLeft + lblW * 0.65f,
                        lblTop + 12f * u,
                        lblLeft + lblW - 1f * u,
                        lblTop + 5.5f * u,
                    )
                }
                drawPath(
                    path = wavePath,
                    color = Color(0xFFFFF1D1),
                    style = Stroke(width = 2.4f * u, cap = StrokeCap.Round),
                )

                // Frosted window pill
                val winLeft = bodyLeft + 11f * u
                val winTop = bodyTop + 22f * u
                val winW = bodyW - 22f * u
                val winH = 20f * u
                drawRoundRect(
                    color = Color(0xFF0A0908),
                    topLeft = Offset(winLeft, winTop),
                    size = Size(winW, winH),
                    cornerRadius = CornerRadius(winH * 0.5f, winH * 0.5f),
                )
                drawRoundRect(
                    color = Color(0xFFDCC0A1).copy(alpha = 0.55f),
                    topLeft = Offset(winLeft, winTop),
                    size = Size(winW, winH),
                    cornerRadius = CornerRadius(winH * 0.5f, winH * 0.5f),
                    style = Stroke(width = 1.0f * u),
                )

                // Animated spinning tape reels
                val cy = winTop + winH * 0.5f
                val leftCx = winLeft + 14.5f * u
                val rightCx = winLeft + winW - 14.5f * u
                val reelR = 6.2f * u

                for (cx in floatArrayOf(leftCx, rightCx)) {
                    drawCircle(
                        brush = Brush.radialGradient(
                            listOf(Color(0xFFF6B570), Color(0xFFD9541E), Color(0x00D9541E)),
                            center = Offset(cx, cy),
                            radius = reelR * 1.9f,
                        ),
                        center = Offset(cx, cy),
                        radius = reelR * 1.9f,
                    )
                    drawCircle(
                        color = Color(0xFF171412),
                        center = Offset(cx, cy),
                        radius = reelR,
                    )
                    drawCircle(
                        color = Color(0xFFF3EEE3),
                        center = Offset(cx, cy),
                        radius = reelR,
                        style = Stroke(width = 1.3f * u),
                    )
                    rotate(degrees = reelSpin.value, pivot = Offset(cx, cy)) {
                        for (k in 0 until 6) {
                            val angle = Math.toRadians(k * 60.0)
                            val dx1 = (cos(angle) * reelR * 0.55f).toFloat()
                            val dy1 = (sin(angle) * reelR * 0.55f).toFloat()
                            val dx2 = (cos(angle) * reelR * 0.96f).toFloat()
                            val dy2 = (sin(angle) * reelR * 0.96f).toFloat()
                            drawLine(
                                color = Color(0xFFF3EEE3),
                                start = Offset(cx + dx1, cy + dy1),
                                end = Offset(cx + dx2, cy + dy2),
                                strokeWidth = 1.2f * u,
                                cap = StrokeCap.Round,
                            )
                        }
                    }
                }

                // Bottom cassette bridge cutout + orange accent indicator
                val brW = 46f * u
                val brLeft = (size.width - brW) * 0.5f
                val brTop = bodyTop + bodyH - 13f * u
                drawRoundRect(
                    color = Color(0xFFD9541E),
                    topLeft = Offset(size.width * 0.5f - 8.5f * u, brTop + 4.5f * u),
                    size = Size(17f * u, 3.6f * u),
                    cornerRadius = CornerRadius(1.8f * u, 1.8f * u),
                )
                drawCircle(
                    color = Color(0xFFF3EEE3),
                    center = Offset(brLeft + 10f * u, brTop + 6.3f * u),
                    radius = 2.2f * u,
                    style = Stroke(width = 1.0f * u),
                )
                drawCircle(
                    color = Color(0xFFF3EEE3),
                    center = Offset(brLeft + brW - 10f * u, brTop + 6.3f * u),
                    radius = 2.2f * u,
                    style = Stroke(width = 1.0f * u),
                )
            }

            Spacer(Modifier.height(14.dp))

            Row(
                horizontalArrangement = Arrangement.spacedBy(2.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                brandLetters.forEachIndexed { idx, ch ->
                    val charProgress = ((wordReveal.value * 7.2f) - idx).coerceIn(0f, 1f)
                    Text(
                        text = ch,
                        style = MaterialTheme.typography.headlineSmall.copy(
                            fontWeight = FontWeight.SemiBold,
                            letterSpacing = 1.6.sp,
                        ),
                        color = Color(0xFFF3EEE3).copy(alpha = 0.92f),
                        modifier = Modifier.graphicsLayer {
                            alpha = charProgress
                            translationY = (1f - charProgress) * 16f
                            val s = 0.88f + 0.12f * charProgress
                            scaleX = s
                            scaleY = s
                        },
                    )
                }
            }

            Spacer(Modifier.height(4.dp))

            Text(
                text = "PURE AUDIO",
                style = MaterialTheme.typography.labelSmall.copy(
                    letterSpacing = 2.8.sp,
                    fontWeight = FontWeight.Medium,
                ),
                color = Color(0xFFDCC0A1).copy(alpha = 0.56f),
                modifier = Modifier.graphicsLayer {
                    alpha = taglineAlpha.value
                    translationY = (1f - taglineAlpha.value) * 8f
                },
            )
        }
    }
}
