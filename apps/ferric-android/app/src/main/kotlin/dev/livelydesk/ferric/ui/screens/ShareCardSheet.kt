package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.util.ShareCardAspect
import dev.livelydesk.ferric.util.ShareCardRenderer
import dev.livelydesk.ferric.util.ShareCardStyle
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShareCardSheet(
    state: PlaybackUiState,
    onDismiss: () -> Unit,
    onMessage: (String) -> Unit,
) {
    val context = LocalContext.current
    val palette = LocalFerricPalette.current
    val scope = rememberCoroutineScope()
    val track = state.currentTrack ?: return
    val allLines = state.lyricsState.lines.map { it.text }.filter { it.isNotBlank() }

    val selectedIndices = remember(track.id, allLines.size) {
        mutableStateListOf<Int>().apply {
            for (i in 0 until minOf(3, allLines.size)) add(i)
        }
    }
    var aspect by remember { mutableStateOf(ShareCardAspect.STORY_9_16) }
    var style by remember { mutableStateOf(ShareCardStyle.LIQUID_GLASS) }
    var exporting by remember { mutableStateOf(false) }

    val selectedLines = selectedIndices.sorted().mapNotNull { allLines.getOrNull(it) }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        containerColor = palette.surface,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Lyrics Share Card", style = MaterialTheme.typography.headlineSmall, color = palette.textPrimary)

            // Live Mini Card Preview
            val bgBrush = when (style) {
                ShareCardStyle.AMOLED -> Brush.linearGradient(listOf(Color.Black, Color(0xFF0E0E12)))
                ShareCardStyle.LIQUID_GLASS -> Brush.linearGradient(listOf(state.coverShade.orb1, Color(0xFF10131C), state.coverShade.orb2))
                ShareCardStyle.ADAPTIVE_GRADIENT -> Brush.linearGradient(listOf(state.coverShade.accent, state.coverShade.orb1, Color(0xFF0A0A0E)))
            }
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(18.dp))
                    .background(bgBrush)
                    .padding(16.dp),
            ) {
                Surface(
                    shape = RoundedCornerShape(14.dp),
                    color = Color(0xAA121520),
                    border = BorderStroke(1.dp, Color.White.copy(alpha = 0.24f)),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(
                        modifier = Modifier.padding(14.dp),
                        verticalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            ArtworkThumb(url = track.thumbnailUrl, size = 44.dp, cornerRadius = 8.dp)
                            Spacer(Modifier.width(10.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(track.title, style = MaterialTheme.typography.titleMedium, color = Color.White, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(track.artist, style = MaterialTheme.typography.bodyMedium, color = Color(0xFFCFD3DE), maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                        }
                        val previewLines = if (selectedLines.isEmpty()) listOf(track.title) else selectedLines
                        previewLines.forEach { line ->
                            Text(
                                text = line,
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.Bold,
                                color = Color.White,
                            )
                        }
                        Text("FERRIC", style = MaterialTheme.typography.labelSmall, color = state.coverShade.accent)
                    }
                }
            }

            // Aspect & Style Chips
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ShareCardAspect.entries.forEach { opt ->
                    FilterChip(
                        selected = aspect == opt,
                        onClick = { aspect = opt },
                        label = { Text(opt.label) },
                        colors = FilterChipDefaults.filterChipColors(
                            selectedContainerColor = palette.accent,
                            selectedLabelColor = palette.canvas,
                            containerColor = palette.elevated,
                            labelColor = palette.textPrimary,
                        ),
                    )
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ShareCardStyle.entries.forEach { opt ->
                    FilterChip(
                        selected = style == opt,
                        onClick = { style = opt },
                        label = { Text(opt.label) },
                        colors = FilterChipDefaults.filterChipColors(
                            selectedContainerColor = palette.accent,
                            selectedLabelColor = palette.canvas,
                            containerColor = palette.elevated,
                            labelColor = palette.textPrimary,
                        ),
                    )
                }
            }

            // Select up to 5 lyric lines
            if (allLines.isNotEmpty()) {
                Text(
                    text = "Select 1 to 5 lyric lines (${selectedIndices.size}/5)",
                    style = MaterialTheme.typography.titleMedium,
                    color = palette.textSecondary,
                )
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    allLines.take(24).forEachIndexed { idx, line ->
                        val picked = idx in selectedIndices
                        Surface(
                            shape = RoundedCornerShape(10.dp),
                            color = if (picked) palette.accent.copy(alpha = 0.22f) else palette.elevated,
                            border = BorderStroke(1.dp, if (picked) palette.accent else palette.border),
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable {
                                    if (picked) {
                                        selectedIndices.remove(idx)
                                    } else if (selectedIndices.size < 5) {
                                        selectedIndices.add(idx)
                                    }
                                },
                        ) {
                            Text(
                                text = line,
                                style = MaterialTheme.typography.bodyMedium,
                                color = palette.textPrimary,
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                            )
                        }
                    }
                }
            }

            Button(
                onClick = {
                    if (exporting) return@Button
                    exporting = true
                    scope.launch {
                        val res = ShareCardRenderer.renderAndSharePng(
                            context = context,
                            track = track,
                            lyricLines = selectedLines,
                            aspect = aspect,
                            style = style,
                            coverShade = state.coverShade,
                        )
                        exporting = false
                        res.onFailure { err -> onMessage("Share card failed: ${err.message}") }
                    }
                },
                enabled = !exporting,
                colors = ButtonDefaults.buttonColors(containerColor = palette.accent),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Icon(Icons.Rounded.Share, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text(if (exporting) "Rendering PNG..." else "Share ${aspect.label} Card")
            }

            Spacer(Modifier.height(20.dp))
        }
    }
}
