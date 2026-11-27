package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.ui.theme.FerricModalBottomSheet
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import uniffi.ferric_mobile_ffi.FfiListeningStats

private fun formatListeningTime(ms: Long): String {
    val totalMin = (ms / 60_000L).coerceAtLeast(0L)
    val hrs = totalMin / 60L
    val mins = totalMin % 60L
    return if (hrs > 0L) "${hrs}h ${mins}m" else "${mins}m"
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StatsSheet(
    stats: FfiListeningStats?,
    onRefresh: () -> Unit,
    onDismiss: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    LaunchedEffect(Unit) { onRefresh() }

    FerricModalBottomSheet(
        onDismissRequest = onDismiss,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Listening Stats & Streak", style = MaterialTheme.typography.headlineSmall, color = palette.textPrimary)
            Text(
                text = "Stored 100% locally in SQLite play_history with zero cloud telemetry.",
                style = MaterialTheme.typography.bodyMedium,
                color = palette.textSecondary,
            )

            val totalPlays = stats?.totalPlays?.toLong() ?: 0L
            val listenedMs = stats?.totalListenedMs?.toLong() ?: 0L
            val curStreak = stats?.currentStreakDays?.toInt() ?: 0
            val bestStreak = stats?.longestStreakDays?.toInt() ?: 0

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                StatMetricCard("Qualified Plays", "$totalPlays", Modifier.weight(1f))
                StatMetricCard("Time Listened", formatListeningTime(listenedMs), Modifier.weight(1f))
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                StatMetricCard("Current Streak", "$curStreak day(s)", Modifier.weight(1f))
                StatMetricCard("Longest Streak", "$bestStreak day(s)", Modifier.weight(1f))
            }

            Text("Top Artists", style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)
            val artists = stats?.topArtists.orEmpty()
            if (artists.isEmpty()) {
                Text(
                    "Listen to tracks for 30s+ to build your personal Top Artists and daily streak.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = palette.textSecondary,
                )
            } else {
                val maxPlays = artists.maxOf { it.plays.toLong() }.coerceAtLeast(1L)
                artists.forEachIndexed { idx, item ->
                    androidx.compose.foundation.layout.Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .ferricGlassSurface(
                                palette = palette,
                                cornerRadius = palette.cornerRadiusDp,
                                fallbackColor = palette.elevated,
                            ),
                    ) {
                        Column(
                            modifier = Modifier.padding(12.dp),
                            verticalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween,
                            ) {
                                Text(
                                    "#${idx + 1} ${item.artist}",
                                    style = MaterialTheme.typography.titleMedium,
                                    color = palette.textPrimary,
                                )
                                Text(
                                    "${item.plays} plays",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = palette.accent,
                                )
                            }
                            LinearProgressIndicator(
                                progress = { (item.plays.toFloat() / maxPlays.toFloat()).coerceIn(0.05f, 1f) },
                                modifier = Modifier.fillMaxWidth().height(4.dp),
                                color = palette.accent,
                                trackColor = palette.border,
                            )
                        }
                    }
                }
            }

            Spacer(Modifier.height(20.dp))
        }
    }
}

@Composable
private fun StatMetricCard(
    label: String,
    value: String,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    androidx.compose.foundation.layout.Box(
        modifier = modifier.ferricGlassSurface(
            palette = palette,
            cornerRadius = palette.cornerRadiusDp,
            fallbackColor = palette.elevated,
        ),
    ) {
        Column(modifier = Modifier.padding(12.dp)) {
            Text(label, style = MaterialTheme.typography.labelSmall, color = palette.textSecondary)
            Spacer(Modifier.height(4.dp))
            Text(value, style = MaterialTheme.typography.headlineSmall, color = palette.accent)
        }
    }
}
