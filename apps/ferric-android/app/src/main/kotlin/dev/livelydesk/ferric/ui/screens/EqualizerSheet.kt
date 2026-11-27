package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.Upload
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.livelydesk.ferric.ui.components.EqCurveMath
import dev.livelydesk.ferric.ui.theme.FerricModalBottomSheet
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import java.util.Locale
import kotlin.math.roundToInt
import uniffi.ferric_mobile_ffi.FfiAudioQuality
import uniffi.ferric_mobile_ffi.FfiAutoEqProfile
import uniffi.ferric_mobile_ffi.FfiDspSettings
import uniffi.ferric_mobile_ffi.FfiEqConfig
import uniffi.ferric_mobile_ffi.FfiFadeCurve

private enum class EqStudioTab(val label: String) {
    GRAPHIC("Equalizer"),
    AUTOEQ("AutoEq"),
    DSP("DSP & Mix"),
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EqualizerSheet(
    eqConfig: FfiEqConfig?,
    eqPresets: List<String>,
    autoEqProfiles: List<FfiAutoEqProfile>,
    dspSettings: FfiDspSettings?,
    activeOutputRoute: String,
    onDismiss: () -> Unit,
    onUpdateEq: (FfiEqConfig) -> Unit,
    onApplyPreset: (String) -> Unit,
    onSearchAutoEq: (String) -> Unit,
    onApplyAutoEq: (String) -> Unit,
    onImportApo: () -> Unit,
    onExportApo: () -> Unit,
    onUpdateDsp: (FfiDspSettings) -> Unit,
    onOpenOutputSwitcher: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    var selectedTab by rememberSaveable { mutableStateOf(EqStudioTab.GRAPHIC) }
    var autoEqQuery by remember { mutableStateOf("") }
    var draftEq by remember(eqConfig) { mutableStateOf(eqConfig) }
    var draftCrossfadeSec by remember(dspSettings?.crossfadeMs) {
        mutableFloatStateOf(((dspSettings?.crossfadeMs?.toFloat() ?: 0f) / 1000f).coerceIn(0f, 12f))
    }
    LaunchedEffect(eqConfig) { draftEq = eqConfig }

    val cardRadius = palette.cornerRadiusDp
    val activeEq = draftEq

    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    FerricModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 18.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            // Precision Header Row
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = "Studio Equalizer",
                        style = MaterialTheme.typography.titleLarge,
                        color = palette.textPrimary,
                    )
                    if (activeEq != null) {
                        Text(
                            text = "${activeEq.presetName} · ${if (activeEq.enabled) "Active" else "Bypassed"}",
                            style = MaterialTheme.typography.bodyMedium,
                            color = if (activeEq.enabled) palette.accent else palette.textSecondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                if (activeEq != null) {
                    Switch(
                        checked = activeEq.enabled,
                        onCheckedChange = {
                            val updated = activeEq.copy(enabled = it)
                            draftEq = updated
                            onUpdateEq(updated)
                        },
                        colors = SwitchDefaults.colors(checkedTrackColor = palette.accent),
                    )
                }
            }

            // Segmented 3-Tab Switcher
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(42.dp)
                    .ferricGlassSurface(palette = palette, cornerRadius = 999.dp, fallbackColor = palette.elevated)
                    .padding(4.dp),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                EqStudioTab.entries.forEach { tab ->
                    val selected = selectedTab == tab
                    Box(
                        modifier = Modifier
                            .weight(1f)
                            .fillMaxHeight()
                            .clip(RoundedCornerShape(999.dp))
                            .background(if (selected) palette.accent else Color.Transparent)
                            .clickable { selectedTab = tab },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = tab.label,
                            style = MaterialTheme.typography.labelLarge,
                            color = if (selected) Color.Black else palette.textSecondary,
                        )
                    }
                }
            }

            when (selectedTab) {
                EqStudioTab.GRAPHIC -> {
                    if (activeEq != null) {
                        val curve = remember(activeEq) { EqCurveMath.evaluateCurveDb(activeEq) }

                        // Frequency Response Plot with symmetrical dB guides
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .ferricGlassSurface(
                                    palette = palette,
                                    cornerRadius = cardRadius,
                                    fallbackColor = palette.canvas,
                                ),
                        ) {
                            Column(
                                modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
                                verticalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                ) {
                                    Text("+12 dB", style = MaterialTheme.typography.labelSmall, color = palette.textSecondary)
                                    Text("0 dB", style = MaterialTheme.typography.labelSmall, color = palette.textSecondary)
                                    Text("-12 dB", style = MaterialTheme.typography.labelSmall, color = palette.textSecondary)
                                }
                                Canvas(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .height(112.dp),
                                ) {
                                    val w = size.width
                                    val h = size.height
                                    val midY = h * 0.5f
                                    for (frac in listOf(0.25f, 0.5f, 0.75f)) {
                                        drawLine(
                                            color = palette.border.copy(alpha = if (frac == 0.5f) 0.9f else 0.45f),
                                            start = Offset(0f, h * frac),
                                            end = Offset(w, h * frac),
                                            strokeWidth = if (frac == 0.5f) 2f else 1f,
                                        )
                                    }
                                    if (curve.size >= 2) {
                                        val linePath = Path()
                                        val fillPath = Path()
                                        curve.forEachIndexed { idx, db ->
                                            val x = w * idx.toFloat() / (curve.size - 1)
                                            val y = midY - (db / 16f) * (h * 0.46f)
                                            if (idx == 0) {
                                                linePath.moveTo(x, y)
                                                fillPath.moveTo(x, midY)
                                                fillPath.lineTo(x, y)
                                            } else {
                                                linePath.lineTo(x, y)
                                                fillPath.lineTo(x, y)
                                            }
                                        }
                                        fillPath.lineTo(w, midY)
                                        fillPath.close()

                                        drawPath(
                                            path = fillPath,
                                            brush = Brush.verticalGradient(
                                                listOf(
                                                    palette.accent.copy(alpha = if (activeEq.enabled) 0.30f else 0.12f),
                                                    palette.accent.copy(alpha = 0.02f),
                                                ),
                                            ),
                                        )
                                        drawPath(
                                            path = linePath,
                                            color = if (activeEq.enabled) palette.accent else palette.textSecondary,
                                            style = Stroke(width = 4.5f, cap = StrokeCap.Round),
                                        )
                                    }
                                }
                            }
                        }

                        // Symmetrical Preset Selector + Reset Flat action
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            LazyRow(
                                modifier = Modifier.weight(1f),
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                items(eqPresets, key = { it }) { preset ->
                                    val selected = activeEq.presetName.equals(preset, ignoreCase = true)
                                    FilterChip(
                                        selected = selected,
                                        onClick = { onApplyPreset(preset) },
                                        label = { Text(preset) },
                                        colors = FilterChipDefaults.filterChipColors(
                                            selectedContainerColor = palette.accent,
                                            selectedLabelColor = Color.Black,
                                            containerColor = palette.elevated,
                                            labelColor = palette.textPrimary,
                                        ),
                                    )
                                }
                            }
                            TextButton(
                                onClick = {
                                    val flatBands = activeEq.bands.map { it.copy(gainDb = 0f) }
                                    val reset = activeEq.copy(bands = flatBands, preampDb = 0f, presetName = "Flat")
                                    draftEq = reset
                                    onUpdateEq(reset)
                                },
                            ) {
                                Icon(Icons.Rounded.Refresh, contentDescription = "Reset Flat", tint = palette.textSecondary, modifier = Modifier.size(16.dp))
                                Spacer(Modifier.width(4.dp))
                                Text("Flat", color = palette.textSecondary)
                            }
                        }

                        // Balanced Symmetrical 10-Band Vertical Fader Console
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .ferricGlassSurface(
                                    palette = palette,
                                    cornerRadius = cardRadius,
                                    fallbackColor = palette.elevated,
                                ),
                        ) {
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(horizontal = 8.dp, vertical = 12.dp),
                                horizontalArrangement = Arrangement.SpaceEvenly,
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                activeEq.bands.forEachIndexed { idx, band ->
                                    val freqShort = if (band.freqHz >= 1000f) {
                                        "${(band.freqHz / 1000f).roundToInt()}k"
                                    } else {
                                        "${band.freqHz.roundToInt()}"
                                    }
                                    Column(
                                        modifier = Modifier.weight(1f),
                                        horizontalAlignment = Alignment.CenterHorizontally,
                                        verticalArrangement = Arrangement.spacedBy(6.dp),
                                    ) {
                                        Text(
                                            text = if (kotlin.math.abs(band.gainDb) < 0.05f) {
                                                "0"
                                            } else {
                                                String.format(Locale.US, "%+.0f", band.gainDb)
                                            },
                                            style = MaterialTheme.typography.labelSmall.copy(fontSize = 10.sp),
                                            color = if (kotlin.math.abs(band.gainDb) >= 0.3f) palette.accent else palette.textSecondary,
                                            textAlign = TextAlign.Center,
                                            maxLines = 1,
                                        )
                                        VerticalEqFader(
                                            gainDb = band.gainDb,
                                            activeColor = if (activeEq.enabled) palette.accent else palette.textSecondary,
                                            trackColor = palette.border,
                                            onChange = { nextDb ->
                                                val nextBands = activeEq.bands.toMutableList()
                                                nextBands[idx] = band.copy(gainDb = nextDb)
                                                draftEq = activeEq.copy(bands = nextBands, presetName = "Custom")
                                            },
                                            onCommit = {
                                                draftEq?.let(onUpdateEq)
                                            },
                                        )
                                        Text(
                                            text = freqShort,
                                            style = MaterialTheme.typography.labelSmall.copy(fontSize = 10.sp),
                                            color = palette.textSecondary,
                                            textAlign = TextAlign.Center,
                                            maxLines = 1,
                                        )
                                    }
                                }
                            }
                        }

                        // Preamp Gain Card
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .ferricGlassSurface(
                                    palette = palette,
                                    cornerRadius = cardRadius,
                                    fallbackColor = palette.elevated,
                                ),
                        ) {
                            Column(
                                modifier = Modifier.padding(horizontal = 14.dp, vertical = 10.dp),
                            ) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                ) {
                                    Text("Preamp Gain", color = palette.textPrimary, style = MaterialTheme.typography.titleSmall)
                                    Text(
                                        text = String.format(Locale.US, "%+.1f dB", activeEq.preampDb),
                                        style = MaterialTheme.typography.labelSmall,
                                        color = palette.accent,
                                    )
                                }
                                Slider(
                                    value = activeEq.preampDb.coerceIn(-12f, 12f),
                                    onValueChange = { v ->
                                        draftEq = activeEq.copy(preampDb = v, presetName = "Custom")
                                    },
                                    onValueChangeFinished = {
                                        draftEq?.let(onUpdateEq)
                                    },
                                    valueRange = -12f..12f,
                                    colors = SliderDefaults.colors(
                                        thumbColor = palette.accent,
                                        activeTrackColor = palette.accent,
                                        inactiveTrackColor = palette.border,
                                    ),
                                )
                            }
                        }

                        // Equal-width Import / Export APO Buttons
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(10.dp),
                        ) {
                            OutlinedButton(
                                onClick = onImportApo,
                                border = BorderStroke(1.dp, palette.border),
                                shape = RoundedCornerShape(999.dp),
                                modifier = Modifier.weight(1f),
                            ) {
                                Icon(Icons.Rounded.Upload, contentDescription = null, tint = palette.textPrimary, modifier = Modifier.size(18.dp))
                                Spacer(Modifier.width(6.dp))
                                Text("Import APO", color = palette.textPrimary)
                            }
                            OutlinedButton(
                                onClick = onExportApo,
                                border = BorderStroke(1.dp, palette.border),
                                shape = RoundedCornerShape(999.dp),
                                modifier = Modifier.weight(1f),
                            ) {
                                Icon(Icons.Rounded.Download, contentDescription = null, tint = palette.textPrimary, modifier = Modifier.size(18.dp))
                                Spacer(Modifier.width(6.dp))
                                Text("Export APO", color = palette.textPrimary)
                            }
                        }
                    }
                }

                EqStudioTab.AUTOEQ -> {
                    OutlinedTextField(
                        value = autoEqQuery,
                        onValueChange = {
                            autoEqQuery = it
                            onSearchAutoEq(it)
                        },
                        placeholder = { Text("Search Sony, Sennheiser, AirPods, Moondrop...") },
                        singleLine = true,
                        shape = RoundedCornerShape(cardRadius),
                        colors = OutlinedTextFieldDefaults.colors(
                            focusedBorderColor = palette.accent,
                            unfocusedBorderColor = palette.border,
                            focusedTextColor = palette.textPrimary,
                            unfocusedTextColor = palette.textPrimary,
                        ),
                        modifier = Modifier.fillMaxWidth(),
                    )
                    LazyColumn(
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(max = 360.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        items(autoEqProfiles, key = { it.name }) { prof ->
                            val selected = activeEq?.presetName.equals(prof.name, ignoreCase = true)
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .ferricGlassSurface(
                                        palette = palette,
                                        cornerRadius = cardRadius,
                                        accentGlow = if (selected) palette.accent else Color.Unspecified,
                                        fallbackColor = if (selected) palette.accent.copy(alpha = 0.14f) else palette.elevated,
                                    )
                                    .clickable { onApplyAutoEq(prof.name) },
                            ) {
                                Row(
                                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Column(modifier = Modifier.weight(1f)) {
                                        Text(prof.name, color = palette.textPrimary, style = MaterialTheme.typography.titleSmall)
                                        Text(prof.category, color = palette.textSecondary, style = MaterialTheme.typography.bodySmall)
                                    }
                                    if (selected) {
                                        Icon(Icons.Rounded.Check, contentDescription = "Selected", tint = palette.accent, modifier = Modifier.size(19.dp))
                                    }
                                }
                            }
                        }
                    }
                }

                EqStudioTab.DSP -> {
                    if (dspSettings != null) {
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .ferricGlassSurface(
                                    palette = palette,
                                    cornerRadius = cardRadius,
                                    fallbackColor = palette.elevated,
                                ),
                        ) {
                            Column(
                                modifier = Modifier.padding(14.dp),
                                verticalArrangement = Arrangement.spacedBy(10.dp),
                            ) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                ) {
                                    Text("Velvet Auto Crossfade", style = MaterialTheme.typography.titleSmall, color = palette.textPrimary)
                                    Text("${draftCrossfadeSec.roundToInt()} s", color = palette.accent, style = MaterialTheme.typography.labelSmall)
                                }
                                Slider(
                                    value = draftCrossfadeSec,
                                    onValueChange = { sec -> draftCrossfadeSec = sec },
                                    onValueChangeFinished = {
                                        onUpdateDsp(dspSettings.copy(crossfadeMs = (draftCrossfadeSec.roundToInt() * 1000).toUInt()))
                                    },
                                    valueRange = 0f..12f,
                                    steps = 11,
                                    colors = SliderDefaults.colors(
                                        thumbColor = palette.accent,
                                        activeTrackColor = palette.accent,
                                        inactiveTrackColor = palette.border,
                                    ),
                                )
                                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    val curves = listOf(
                                        FfiFadeCurve.BASS_SWAP_AUTOMIX to "Bass-Swap 140Hz",
                                        FfiFadeCurve.S_CURVE_VELVET to "S-Curve Velvet",
                                        FfiFadeCurve.EQUAL_POWER to "Equal-Power",
                                    )
                                    items(curves, key = { it.first.name }) { (curve, label) ->
                                        FilterChip(
                                            selected = dspSettings.fadeCurve == curve,
                                            onClick = { onUpdateDsp(dspSettings.copy(fadeCurve = curve)) },
                                            label = { Text(label) },
                                            colors = FilterChipDefaults.filterChipColors(
                                                selectedContainerColor = palette.accent,
                                                selectedLabelColor = Color.Black,
                                                containerColor = palette.surface,
                                                labelColor = palette.textPrimary,
                                            ),
                                        )
                                    }
                                }
                                HorizontalDivider(color = palette.border.copy(alpha = 0.5f))
                                DspToggleRow("Gapless Album Lock", dspSettings.gaplessAlbumLock) {
                                    onUpdateDsp(dspSettings.copy(gaplessAlbumLock = it))
                                }
                                DspToggleRow("Trim Track Silence", dspSettings.trimSilence) {
                                    onUpdateDsp(dspSettings.copy(trimSilence = it))
                                }
                                DspToggleRow("ReplayGain Normalization", dspSettings.replayGain) {
                                    onUpdateDsp(dspSettings.copy(replayGain = it))
                                }
                                DspToggleRow("Mono Downmix", dspSettings.monoAudio) {
                                    onUpdateDsp(dspSettings.copy(monoAudio = it))
                                }
                            }
                        }

                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .ferricGlassSurface(
                                    palette = palette,
                                    cornerRadius = cardRadius,
                                    fallbackColor = palette.elevated,
                                ),
                        ) {
                            Column(
                                modifier = Modifier.padding(14.dp),
                                verticalArrangement = Arrangement.spacedBy(10.dp),
                            ) {
                                Text("Playback Speed", color = palette.textPrimary, style = MaterialTheme.typography.titleSmall)
                                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    items(listOf(0.75f, 1.0f, 1.15f, 1.25f, 1.5f), key = { it }) { spd ->
                                        FilterChip(
                                            selected = kotlin.math.abs(dspSettings.playbackSpeed - spd) < 0.03f,
                                            onClick = { onUpdateDsp(dspSettings.copy(playbackSpeed = spd)) },
                                            label = { Text("${spd}x") },
                                            colors = FilterChipDefaults.filterChipColors(
                                                selectedContainerColor = palette.accent,
                                                selectedLabelColor = Color.Black,
                                                containerColor = palette.surface,
                                                labelColor = palette.textPrimary,
                                            ),
                                        )
                                    }
                                }
                                Text("Streaming Audio Quality", color = palette.textPrimary, style = MaterialTheme.typography.titleSmall)
                                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    val qualities = listOf(
                                        FfiAudioQuality.LOW to "Low (64k)",
                                        FfiAudioQuality.NORMAL to "Normal (128k)",
                                        FfiAudioQuality.HIGH to "High (160k)",
                                        FfiAudioQuality.BEST to "Best",
                                    )
                                    items(qualities, key = { it.first.name }) { (q, lbl) ->
                                        FilterChip(
                                            selected = dspSettings.audioQuality == q,
                                            onClick = { onUpdateDsp(dspSettings.copy(audioQuality = q)) },
                                            label = { Text(lbl) },
                                            colors = FilterChipDefaults.filterChipColors(
                                                selectedContainerColor = palette.accent,
                                                selectedLabelColor = Color.Black,
                                                containerColor = palette.surface,
                                                labelColor = palette.textPrimary,
                                            ),
                                        )
                                    }
                                }
                            }
                        }

                        OutlinedButton(
                            onClick = onOpenOutputSwitcher,
                            border = BorderStroke(1.dp, palette.accent),
                            shape = RoundedCornerShape(999.dp),
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Icon(Icons.Rounded.Speaker, contentDescription = null, tint = palette.accent)
                            Spacer(Modifier.width(8.dp))
                            Text("Audio Output: $activeOutputRoute", color = palette.textPrimary)
                        }
                    }
                }
            }

            Spacer(Modifier.height(18.dp))
        }
    }
}

@Composable
private fun VerticalEqFader(
    gainDb: Float,
    activeColor: Color,
    trackColor: Color,
    onChange: (Float) -> Unit,
    onCommit: () -> Unit,
) {
    var heightPx by remember { mutableIntStateOf(1) }
    val clamped = gainDb.coerceIn(-12f, 12f)
    // +12 dB at top (frac = 0), -12 dB at bottom (frac = 1)
    val thumbFrac = ((12f - clamped) / 24f).coerceIn(0f, 1f)

    fun yToGain(y: Float): Float {
        val frac = (y / heightPx.toFloat().coerceAtLeast(1f)).coerceIn(0f, 1f)
        val raw = 12f - frac * 24f
        return (raw * 2f).roundToInt() / 2f
    }

    Box(
        modifier = Modifier
            .width(30.dp)
            .height(148.dp)
            .onSizeChanged { heightPx = it.height }
            .pointerInput(Unit) {
                detectTapGestures { offset ->
                    onChange(yToGain(offset.y))
                    onCommit()
                }
            }
            .pointerInput(Unit) {
                detectVerticalDragGestures(
                    onDragEnd = { onCommit() },
                    onDragCancel = { onCommit() },
                    onVerticalDrag = { change, _ ->
                        change.consume()
                        onChange(yToGain(change.position.y))
                    },
                )
            },
        contentAlignment = Alignment.Center,
    ) {
        Canvas(modifier = Modifier.fillMaxSize()) {
            val w = size.width
            val h = size.height
            val cx = w * 0.5f
            val padY = 9.dp.toPx()
            val usableH = (h - padY * 2f).coerceAtLeast(10f)
            val topY = padY
            val midY = padY + usableH * 0.5f
            val thumbY = padY + usableH * thumbFrac
            val trackW = 3.5.dp.toPx()

            // Full track rail
            drawRoundRect(
                color = trackColor.copy(alpha = 0.65f),
                topLeft = Offset(cx - trackW * 0.5f, topY),
                size = Size(trackW, usableH),
                cornerRadius = CornerRadius(trackW, trackW),
            )
            // 0 dB center notch
            drawLine(
                color = trackColor,
                start = Offset(cx - 5.dp.toPx(), midY),
                end = Offset(cx + 5.dp.toPx(), midY),
                strokeWidth = 1.5f,
            )
            // Active fill from 0 dB to current thumb
            val fillTop = minOf(midY, thumbY)
            val fillBot = maxOf(midY, thumbY)
            if (fillBot - fillTop > 1f) {
                drawRoundRect(
                    color = activeColor,
                    topLeft = Offset(cx - trackW * 0.5f, fillTop),
                    size = Size(trackW, fillBot - fillTop),
                    cornerRadius = CornerRadius(trackW, trackW),
                )
            }
            // Precision circular thumb knob
            drawCircle(
                color = activeColor,
                radius = 6.5.dp.toPx(),
                center = Offset(cx, thumbY),
            )
        }
    }
}

@Composable
private fun DspToggleRow(
    label: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
) {
    val palette = LocalFerricPalette.current
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge, color = palette.textPrimary)
        Switch(
            checked = checked,
            onCheckedChange = onCheckedChange,
            colors = SwitchDefaults.colors(checkedTrackColor = palette.accent),
        )
    }
}
