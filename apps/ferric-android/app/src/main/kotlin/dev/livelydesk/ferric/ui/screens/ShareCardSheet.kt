package dev.livelydesk.ferric.ui.screens

import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil3.compose.AsyncImage
import coil3.request.ImageRequest
import coil3.size.Precision
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.components.ArtworkThumb
import dev.livelydesk.ferric.ui.theme.FERRIC_MOTION_MS
import dev.livelydesk.ferric.ui.theme.FerricEaseOut
import dev.livelydesk.ferric.ui.theme.FerricModalBottomSheet
import dev.livelydesk.ferric.ui.theme.GlassTier
import dev.livelydesk.ferric.ui.theme.LocalBackdropState
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricBackdropSource
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import dev.livelydesk.ferric.ui.theme.ferricLiquidGlass
import dev.livelydesk.ferric.ui.theme.rememberFerricBackdropState
import dev.livelydesk.ferric.util.ShareCardAspect
import dev.livelydesk.ferric.util.ShareCardRenderer
import dev.livelydesk.ferric.util.ShareCardStyle
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShareCardSheet(
    state: PlaybackUiState,
    initialPositionMs: Long = 0L,
    onDismiss: () -> Unit,
    onMessage: (String) -> Unit,
) {
    val context = LocalContext.current
    val palette = LocalFerricPalette.current
    val scope = rememberCoroutineScope()
    val track = state.currentTrack ?: return
    val rawLines = state.lyricsState.lines
    val indexedNonBlank = remember(track.id, rawLines.size) {
        rawLines.mapIndexedNotNull { idx, line ->
            val trimmed = line.text.trim()
            if (trimmed.isNotEmpty()) line.timestampMs.toLong() to trimmed else null
        }
    }
    val allLines = remember(indexedNonBlank) { indexedNonBlank.map { it.second } }

    // Anchor default selection around the active line currently being sung.
    val initialActiveIdx = remember(track.id, indexedNonBlank.size, state.lyricsState.isSynced) {
        if (!state.lyricsState.isSynced || indexedNonBlank.isEmpty()) {
            0
        } else {
            val eff = (initialPositionMs + state.lyricsState.offsetMs).coerceAtLeast(0L)
            var found = 0
            for (i in indexedNonBlank.indices) {
                if (indexedNonBlank[i].first <= eff) found = i else break
            }
            found
        }
    }

    val selectedIndices = remember(track.id, allLines.size) {
        mutableStateListOf<Int>().apply {
            val start = (initialActiveIdx - 1).coerceAtLeast(0)
            val end = minOf(allLines.lastIndex, start + 2)
            for (i in start..end) add(i)
        }
    }
    var aspect by remember { mutableStateOf(ShareCardAspect.STORY_9_16) }
    var style by remember { mutableStateOf(ShareCardStyle.LIQUID_GLASS) }
    var exporting by remember { mutableStateOf(false) }

    val pickerListState = rememberLazyListState(
        initialFirstVisibleItemIndex = (initialActiveIdx - 1).coerceAtLeast(0),
    )

    val selectedLines = selectedIndices.sorted().mapNotNull { allLines.getOrNull(it) }
    val cardBackdrop = rememberFerricBackdropState()
    val outerCorner = (palette.cornerRadiusDp.value * 0.9f).coerceIn(10f, 24f).dp
    val cardCorner = (palette.cornerRadiusDp.value * 0.85f).coerceIn(10f, 22f).dp

    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    FerricModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        coverShade = state.coverShade,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 18.dp, vertical = 6.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "Lyrics Share Card",
                    style = MaterialTheme.typography.headlineSmall,
                    color = palette.textPrimary,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    if (allLines.isNotEmpty()) {
                        TextButton(
                            onClick = {
                                selectedIndices.clear()
                                val start = (initialActiveIdx - 1).coerceAtLeast(0)
                                val end = minOf(allLines.lastIndex, start + 2)
                                for (i in start..end) selectedIndices.add(i)
                            },
                        ) {
                            Text("Now", color = palette.accent)
                        }
                    }
                    if (selectedIndices.isNotEmpty()) {
                        TextButton(onClick = { selectedIndices.clear() }) {
                            Text("Clear (${selectedIndices.size})", color = palette.textSecondary)
                        }
                    }
                }
            }

            // Live proportional card preview: Liquid Glass uses real-time AGSL optical refraction
            // & Gaussian blur over vibrant blurred cover art, matching the Home dock capsule 1:1.
            Box(
                modifier = Modifier.fillMaxWidth(),
                contentAlignment = Alignment.Center,
            ) {
                val previewWidth = if (aspect == ShareCardAspect.STORY_9_16) 178.dp else 216.dp
                Box(
                    modifier = Modifier
                        .width(previewWidth)
                        .aspectRatio(if (aspect == ShareCardAspect.STORY_9_16) 9f / 16f else 1f)
                        .animateContentSize(animationSpec = tween(FERRIC_MOTION_MS, easing = FerricEaseOut))
                        .clip(RoundedCornerShape(outerCorner)),
                    contentAlignment = Alignment.Center,
                ) {
                    // Preview Layer 1: Optical backdrop source captured into cardBackdrop
                    Box(
                        modifier = Modifier
                            .fillMaxSize()
                            .ferricBackdropSource(cardBackdrop),
                    ) {
                        when (style) {
                            ShareCardStyle.AMOLED -> {
                                Box(
                                    modifier = Modifier
                                        .fillMaxSize()
                                        .background(Color.Black),
                                )
                            }
                            ShareCardStyle.ADAPTIVE_GRADIENT -> {
                                Box(
                                    modifier = Modifier
                                        .fillMaxSize()
                                        .background(
                                            Brush.linearGradient(
                                                listOf(
                                                    state.coverShade.accent,
                                                    state.coverShade.orb1,
                                                    Color(0xFF0A0B10),
                                                ),
                                            ),
                                        ),
                                )
                            }
                            ShareCardStyle.LIQUID_GLASS -> {
                                if (!track.thumbnailUrl.isNullOrBlank()) {
                                    val bgReq = remember(track.thumbnailUrl) {
                                        ImageRequest.Builder(context)
                                            .data(track.thumbnailUrl)
                                            .size(360, 360)
                                            .precision(Precision.INEXACT)
                                            .build()
                                    }
                                    AsyncImage(
                                        model = bgReq,
                                        contentDescription = null,
                                        contentScale = ContentScale.Crop,
                                        modifier = Modifier
                                            .fillMaxSize()
                                            .blur(26.dp),
                                    )
                                }
                                Box(
                                    modifier = Modifier
                                        .fillMaxSize()
                                        .background(
                                            Brush.radialGradient(
                                                colors = listOf(
                                                    state.coverShade.accent.copy(alpha = 0.55f),
                                                    state.coverShade.orb1.copy(alpha = 0.72f),
                                                    state.coverShade.orb2.copy(alpha = 0.85f),
                                                ),
                                            ),
                                        ),
                                )
                            }
                        }
                    }

                    // Preview Layer 2: Floating Lyric Card (true AGSL Liquid Glass when selected)
                    CompositionLocalProvider(LocalBackdropState provides cardBackdrop) {
                        val cardModifier = when (style) {
                            ShareCardStyle.LIQUID_GLASS -> Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 12.dp, vertical = 14.dp)
                                .ferricLiquidGlass(
                                    cornerRadius = cardCorner,
                                    blurRadius = 14.dp,
                                    refractionHeight = 16.dp,
                                    refractionAmount = 14.dp,
                                    chromaticDispersion = 0.65f,
                                    tintAlpha = 0.06f,
                                    alwaysGlass = true,
                                )
                            ShareCardStyle.AMOLED -> Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 12.dp, vertical = 14.dp)
                                .clip(RoundedCornerShape(cardCorner))
                                .background(Color(0xFF0C0C10))
                                .border(1.dp, Color.White.copy(alpha = 0.16f), RoundedCornerShape(cardCorner))
                            ShareCardStyle.ADAPTIVE_GRADIENT -> Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 12.dp, vertical = 14.dp)
                                .clip(RoundedCornerShape(cardCorner))
                                .background(Color.Black.copy(alpha = 0.44f))
                                .border(1.dp, Color.White.copy(alpha = 0.26f), RoundedCornerShape(cardCorner))
                        }

                        Box(modifier = cardModifier) {
                            Column(
                                modifier = Modifier.padding(13.dp),
                                verticalArrangement = Arrangement.spacedBy(9.dp),
                            ) {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    ArtworkThumb(url = track.thumbnailUrl, size = 34.dp, cornerRadius = 8.dp)
                                    Spacer(Modifier.width(8.dp))
                                    Column(modifier = Modifier.weight(1f)) {
                                        Text(
                                            text = track.title,
                                            style = MaterialTheme.typography.titleSmall,
                                            fontWeight = FontWeight.Bold,
                                            color = Color.White,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                        Text(
                                            text = track.artist,
                                            style = MaterialTheme.typography.labelSmall,
                                            color = Color(0xFFDCE1EC),
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                }
                                val previewLines = if (selectedLines.isEmpty()) listOf("\"${track.title}\"") else selectedLines
                                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                    previewLines.forEach { line ->
                                        Text(
                                            text = line,
                                            style = MaterialTheme.typography.titleMedium,
                                            fontSize = if (aspect == ShareCardAspect.STORY_9_16) 11.5.sp else 11.sp,
                                            lineHeight = 14.5.sp,
                                            fontWeight = FontWeight.Bold,
                                            color = Color.White,
                                            maxLines = 3,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                }
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Text(
                                        text = "FERRIC",
                                        style = MaterialTheme.typography.labelSmall,
                                        letterSpacing = 2.sp,
                                        color = if (style == ShareCardStyle.LIQUID_GLASS) {
                                            Color.White.copy(alpha = 0.90f)
                                        } else {
                                            state.coverShade.accent
                                        },
                                    )
                                    if (style == ShareCardStyle.LIQUID_GLASS) {
                                        Text(
                                            text = "LIQUID GLASS",
                                            style = MaterialTheme.typography.labelSmall,
                                            fontSize = 9.sp,
                                            color = Color.White.copy(alpha = 0.65f),
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Sleek Segmented Controls for Aspect & Card Material Style
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                ShareCardAspect.entries.forEach { opt ->
                    SharePillOption(
                        label = opt.label,
                        selected = aspect == opt,
                        onClick = { aspect = opt },
                        modifier = Modifier.weight(1f),
                    )
                }
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                ShareCardStyle.entries.forEach { opt ->
                    SharePillOption(
                        label = opt.label,
                        selected = style == opt,
                        onClick = { style = opt },
                        modifier = Modifier.weight(1f),
                    )
                }
            }

            // Dedicated non-conflicting LazyColumn lyrics selector
            if (allLines.isEmpty()) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .ferricGlassSurface(palette = palette, cornerRadius = 12.dp, fallbackColor = palette.elevated),
                ) {
                    Text(
                        text = "No lyrics loaded yet. Card will showcase cover art and title.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = palette.textSecondary,
                        modifier = Modifier.padding(12.dp),
                    )
                }
            } else {
                Text(
                    text = "Select 1 to 5 lines (${selectedIndices.size}/5 selected · ${allLines.size} total)",
                    style = MaterialTheme.typography.labelMedium,
                    color = palette.textSecondary,
                )
                LazyColumn(
                    state = pickerListState,
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(176.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    itemsIndexed(allLines, key = { idx, _ -> "sc_line_$idx" }) { idx, line ->
                        val picked = idx in selectedIndices
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .ferricGlassSurface(
                                    palette = palette,
                                    cornerRadius = 10.dp,
                                    accentGlow = if (picked) palette.accent else Color.Unspecified,
                                    fallbackColor = if (picked) palette.accent.copy(alpha = 0.20f) else palette.elevated,
                                )
                                .clickable {
                                    if (picked) {
                                        selectedIndices.remove(idx)
                                    } else if (selectedIndices.size < 5) {
                                        selectedIndices.add(idx)
                                    } else {
                                        onMessage("Maximum 5 lyric lines per card")
                                    }
                                },
                        ) {
                            Row(
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 9.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Text(
                                    text = line,
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = palette.textPrimary,
                                    fontWeight = if (picked) FontWeight.SemiBold else FontWeight.Normal,
                                    modifier = Modifier.weight(1f),
                                )
                                if (picked) {
                                    Icon(
                                        imageVector = Icons.Rounded.Check,
                                        contentDescription = "Selected",
                                        tint = palette.accent,
                                        modifier = Modifier.size(17.dp),
                                    )
                                }
                            }
                        }
                    }
                }
            }

            // Dual actions: Save PNG directly to Pictures/Ferric + Share Sheet
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                OutlinedButton(
                    onClick = {
                        if (exporting) return@OutlinedButton
                        exporting = true
                        scope.launch {
                            val res = ShareCardRenderer.saveToGalleryPng(
                                context = context,
                                track = track,
                                lyricLines = selectedLines,
                                aspect = aspect,
                                style = style,
                                coverShade = state.coverShade,
                                fontFamilyName = palette.fontFamilyName,
                                editorialMode = palette.editorialMode,
                            )
                            exporting = false
                            res.onSuccess { path -> onMessage("Saved card to $path") }
                                .onFailure { err -> onMessage("Save failed: ${err.message}") }
                        }
                    },
                    enabled = !exporting,
                    border = BorderStroke(1.dp, palette.accent),
                    shape = RoundedCornerShape(999.dp),
                    modifier = Modifier.weight(1f),
                ) {
                    Icon(Icons.Rounded.Download, contentDescription = null, tint = palette.accent, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Save PNG", color = palette.accent)
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
                                fontFamilyName = palette.fontFamilyName,
                                editorialMode = palette.editorialMode,
                            )
                            exporting = false
                            res.onFailure { err -> onMessage("Share card failed: ${err.message}") }
                        }
                    },
                    enabled = !exporting,
                    shape = RoundedCornerShape(999.dp),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = palette.accent,
                        contentColor = Color.Black,
                    ),
                    modifier = Modifier.weight(1.3f),
                ) {
                    Icon(Icons.Rounded.Share, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text(if (exporting) "Rendering..." else "Share ${aspect.label}")
                }
            }

            Spacer(Modifier.height(10.dp))
        }
    }
}

@Composable
private fun SharePillOption(
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
                .height(36.dp)
                .clip(RoundedCornerShape(999.dp))
                .clickable(onClick = onClick),
        ) {
            Box(contentAlignment = Alignment.Center) {
                Text(
                    text = label,
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.Bold,
                    color = Color.Black,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    } else {
        Box(
            modifier = modifier
                .height(36.dp)
                .ferricGlassSurface(palette = palette, cornerRadius = 999.dp, fallbackColor = palette.elevated)
                .clickable(onClick = onClick),
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
