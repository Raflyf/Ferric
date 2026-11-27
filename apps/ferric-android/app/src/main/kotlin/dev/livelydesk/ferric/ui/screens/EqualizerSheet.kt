package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.Upload
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.ui.components.EqCurveMath
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import java.util.Locale
import kotlin.math.roundToInt
import uniffi.ferric_mobile_ffi.FfiAudioQuality
import uniffi.ferric_mobile_ffi.FfiAutoEqProfile
import uniffi.ferric_mobile_ffi.FfiDspSettings
import uniffi.ferric_mobile_ffi.FfiEqConfig
import uniffi.ferric_mobile_ffi.FfiFadeCurve

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
    var autoEqQuery by remember { mutableStateOf("") }
    // Keep local draft EQ & crossfade so slider drags animate at 60fps and commit cleanly to SQLite on release.
    var draftEq by remember(eqConfig) { mutableStateOf(eqConfig) }
    var draftCrossfadeSec by remember(dspSettings?.crossfadeMs) {
        mutableFloatStateOf(((dspSettings?.crossfadeMs?.toFloat() ?: 0f) / 1000f).coerceIn(0f, 12f))
    }
    LaunchedEffect(eqConfig) { draftEq = eqConfig }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        containerColor = palette.surface,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 18.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Parametric EQ & Audio DSP", style = MaterialTheme.typography.titleLarge, color = palette.textPrimary)
                val activeEq = draftEq
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

            val activeEq = draftEq
            if (activeEq != null) {
                val curve = remember(activeEq) { EqCurveMath.evaluateCurveDb(activeEq) }
                Surface(
                    shape = RoundedCornerShape(16.dp),
                    color = palette.canvas,
                    border = BorderStroke(1.dp, palette.border),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(modifier = Modifier.padding(10.dp)) {
                        Canvas(
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(130.dp),
                        ) {
                            val w = size.width
                            val h = size.height
                            val midY = h * 0.5f
                            for (frac in listOf(0.25f, 0.5f, 0.75f)) {
                                drawLine(
                                    color = palette.border,
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
                                    val y = midY - (db / 18f) * (h * 0.46f)
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
                                            palette.accent.copy(alpha = 0.28f),
                                            palette.accent.copy(alpha = 0.04f),
                                        ),
                                    ),
                                )
                                drawPath(
                                    path = linePath,
                                    color = if (activeEq.enabled) palette.accent else palette.textSecondary,
                                    style = Stroke(width = 4f, cap = StrokeCap.Round),
                                )
                            }
                        }
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            listOf("31Hz", "125Hz", "500Hz", "2kHz", "8kHz", "16kHz").forEach { lbl ->
                                Text(lbl, style = MaterialTheme.typography.labelSmall, color = palette.textSecondary)
                            }
                        }
                    }
                }

                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    items(eqPresets, key = { it }) { preset ->
                        FilterChip(
                            selected = activeEq.presetName.equals(preset, ignoreCase = true),
                            onClick = { onApplyPreset(preset) },
                            label = { Text(preset) },
                            colors = FilterChipDefaults.filterChipColors(
                                selectedContainerColor = palette.accent,
                                selectedLabelColor = if (palette.isLight) Color.White else Color.Black,
                                containerColor = palette.elevated,
                                labelColor = palette.textPrimary,
                            ),
                        )
                    }
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("Preamp", color = palette.textPrimary, style = MaterialTheme.typography.labelMedium)
                    Text(
                        String.format(Locale.US, "%+.1f dB", activeEq.preampDb),
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

                activeEq.bands.forEachIndexed { idx, band ->
                    val freqLabel = if (band.freqHz >= 1000f) {
                        String.format(Locale.US, "%.0fkHz", band.freqHz / 1000f)
                    } else {
                        String.format(Locale.US, "%.0fHz", band.freqHz)
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = freqLabel,
                            style = MaterialTheme.typography.labelSmall,
                            color = palette.textSecondary,
                            modifier = Modifier.width(48.dp),
                        )
                        Slider(
                            value = band.gainDb.coerceIn(-12f, 12f),
                            onValueChange = { newGain ->
                                val nextBands = activeEq.bands.toMutableList()
                                nextBands[idx] = band.copy(gainDb = newGain)
                                draftEq = activeEq.copy(bands = nextBands, presetName = "Custom")
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
                            modifier = Modifier.weight(1f),
                        )
                        Text(
                            text = String.format(Locale.US, "%+.1f", band.gainDb),
                            style = MaterialTheme.typography.labelSmall,
                            color = palette.textPrimary,
                            modifier = Modifier.width(42.dp),
                        )
                    }
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    OutlinedButton(
                        onClick = onImportApo,
                        border = BorderStroke(1.dp, palette.border),
                        modifier = Modifier.weight(1f),
                    ) {
                        Icon(Icons.Rounded.Upload, contentDescription = null, tint = palette.textPrimary)
                        Spacer(Modifier.width(6.dp))
                        Text("Import APO", color = palette.textPrimary)
                    }
                    OutlinedButton(
                        onClick = onExportApo,
                        border = BorderStroke(1.dp, palette.border),
                        modifier = Modifier.weight(1f),
                    ) {
                        Icon(Icons.Rounded.Download, contentDescription = null, tint = palette.textPrimary)
                        Spacer(Modifier.width(6.dp))
                        Text("Export APO", color = palette.textPrimary)
                    }
                }

                Text("AutoEq Headphone / IEM Profiles (24)", style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)
                OutlinedTextField(
                    value = autoEqQuery,
                    onValueChange = {
                        autoEqQuery = it
                        onSearchAutoEq(it)
                    },
                    placeholder = { Text("Search Sony, Sennheiser, AirPods, Moondrop...") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    items(autoEqProfiles, key = { it.name }) { prof ->
                        Surface(
                            shape = RoundedCornerShape(12.dp),
                            color = palette.elevated,
                            border = BorderStroke(
                                1.dp,
                                if (activeEq.presetName.equals(prof.name, ignoreCase = true)) palette.accent else palette.border,
                            ),
                            modifier = Modifier.clickable { onApplyAutoEq(prof.name) },
                        ) {
                            Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                                Text(prof.name, color = palette.textPrimary, style = MaterialTheme.typography.labelMedium)
                                Text(prof.category, color = palette.textSecondary, style = MaterialTheme.typography.labelSmall)
                            }
                        }
                    }
                }
            }

            if (dspSettings != null) {
                Text("Automix Crossfade & DSP", style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("Auto Crossfade", color = palette.textPrimary)
                    Text("${draftCrossfadeSec.roundToInt()}s", color = palette.accent, style = MaterialTheme.typography.labelSmall)
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
                                selectedLabelColor = if (palette.isLight) Color.White else Color.Black,
                                containerColor = palette.elevated,
                                labelColor = palette.textPrimary,
                            ),
                        )
                    }
                }

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

                Text("Playback Speed", color = palette.textPrimary, style = MaterialTheme.typography.labelMedium)
                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf(0.75f, 1.0f, 1.15f, 1.25f, 1.5f).forEach { spd ->
                        item(key = "spd_$spd") {
                            FilterChip(
                                selected = kotlin.math.abs(dspSettings.playbackSpeed - spd) < 0.03f,
                                onClick = { onUpdateDsp(dspSettings.copy(playbackSpeed = spd)) },
                                label = { Text("${spd}x") },
                                colors = FilterChipDefaults.filterChipColors(
                                    selectedContainerColor = palette.accent,
                                    selectedLabelColor = if (palette.isLight) Color.White else Color.Black,
                                    containerColor = palette.elevated,
                                    labelColor = palette.textPrimary,
                                ),
                            )
                        }
                    }
                }

                Text("Streaming Audio Quality", color = palette.textPrimary, style = MaterialTheme.typography.labelMedium)
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
                                selectedLabelColor = if (palette.isLight) Color.White else Color.Black,
                                containerColor = palette.elevated,
                                labelColor = palette.textPrimary,
                            ),
                        )
                    }
                }

                OutlinedButton(
                    onClick = onOpenOutputSwitcher,
                    border = BorderStroke(1.dp, palette.accent),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Icon(Icons.Rounded.Speaker, contentDescription = null, tint = palette.accent)
                    Spacer(Modifier.width(8.dp))
                    Text("Audio Output: $activeOutputRoute", color = palette.textPrimary)
                }
            }

            Spacer(Modifier.height(18.dp))
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
        Text(label, color = palette.textPrimary)
        Switch(
            checked = checked,
            onCheckedChange = onCheckedChange,
            colors = SwitchDefaults.colors(checkedTrackColor = palette.accent),
        )
    }
}
