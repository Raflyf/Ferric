package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
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
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
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
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
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
import dev.livelydesk.ferric.ui.theme.FerricChoicePill
import dev.livelydesk.ferric.ui.theme.FerricModalBottomSheet
import dev.livelydesk.ferric.ui.theme.FerricSlimSlider
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
    val bandGains = remember { mutableStateListOf<Float>() }
    var draftEnabled by remember { mutableStateOf(eqConfig?.enabled ?: false) }
    var draftPresetName by remember { mutableStateOf(eqConfig?.presetName ?: "Flat") }
    var draftPreampDb by remember { mutableFloatStateOf(eqConfig?.preampDb ?: 0f) }
    var isAdjustingSlider by remember { mutableStateOf(false) }
    var lastLiveDispatchMs by remember { mutableLongStateOf(0L) }
    var draftCrossfadeSec by remember(dspSettings?.crossfadeMs) {
        mutableFloatStateOf(((dspSettings?.crossfadeMs?.toFloat() ?: 0f) / 1000f).coerceIn(0f, 12f))
    }

    LaunchedEffect(eqConfig) {
        if (!isAdjustingSlider && eqConfig != null) {
            draftEnabled = eqConfig.enabled
            draftPresetName = eqConfig.presetName
            draftPreampDb = eqConfig.preampDb
            if (bandGains.size != eqConfig.bands.size) {
                bandGains.clear()
                eqConfig.bands.forEach { bandGains.add(it.gainDb) }
            } else {
                eqConfig.bands.forEachIndexed { idx, band ->
                    if (bandGains[idx] != band.gainDb) bandGains[idx] = band.gainDb
                }
            }
        }
    }

    fun snapshotEqConfig(
        enabledOverride: Boolean = draftEnabled,
        presetOverride: String = draftPresetName,
        preampOverride: Float = draftPreampDb,
    ): FfiEqConfig? {
        val base = eqConfig ?: return null
        val updatedBands = base.bands.mapIndexed { idx, band ->
            val g = bandGains.getOrElse(idx) { band.gainDb }
            if (g == band.gainDb) band else band.copy(gainDb = g)
        }
        return base.copy(
            enabled = enabledOverride,
            presetName = presetOverride,
            preampDb = preampOverride,
            bands = updatedBands,
        )
    }

    fun dispatchThrottledLiveEq() {
        val now = android.os.SystemClock.uptimeMillis()
        if (now - lastLiveDispatchMs >= 75L) {
            lastLiveDispatchMs = now
            snapshotEqConfig()?.let(onUpdateEq)
        }
    }

    val cardRadius = palette.cornerRadiusDp
    val curveDbBuffer = remember { FloatArray(EqCurveMath.DEFAULT_POINTS) }
    val curveLinePath = remember { Path() }
    val curveFillPath = remember { Path() }

    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    FerricModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState(), enabled = !isAdjustingSlider)
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
                    if (eqConfig != null) {
                        Text(
                            text = "$draftPresetName · ${if (draftEnabled) "Active" else "Bypassed"}",
                            style = MaterialTheme.typography.bodyMedium,
                            color = if (draftEnabled) palette.accent else palette.textSecondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                if (eqConfig != null) {
                    Switch(
                        checked = draftEnabled,
                        onCheckedChange = { next ->
                            draftEnabled = next
                            snapshotEqConfig(enabledOverride = next)?.let(onUpdateEq)
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
                    val activeEq = eqConfig
                    if (activeEq != null) {
                        // Frequency Response Plot with zero-allocation real-time draw evaluation
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
                                    val gains = if (bandGains.size == activeEq.bands.size) bandGains else null
                                    EqCurveMath.evaluateCurveDbInto(
                                        outDb = curveDbBuffer,
                                        bands = activeEq.bands,
                                        bandGainsOverride = gains,
                                        preampDb = draftPreampDb,
                                        enabled = draftEnabled,
                                    )
                                    val pts = EqCurveMath.DEFAULT_POINTS
                                    curveLinePath.reset()
                                    curveFillPath.reset()
                                    for (idx in 0 until pts) {
                                        val x = w * idx.toFloat() / (pts - 1)
                                        val y = midY - (curveDbBuffer[idx] / 16f) * (h * 0.46f)
                                        if (idx == 0) {
                                            curveLinePath.moveTo(x, y)
                                            curveFillPath.moveTo(x, midY)
                                            curveFillPath.lineTo(x, y)
                                        } else {
                                            curveLinePath.lineTo(x, y)
                                            curveFillPath.lineTo(x, y)
                                        }
                                    }
                                    curveFillPath.lineTo(w, midY)
                                    curveFillPath.close()

                                    drawPath(
                                        path = curveFillPath,
                                        brush = Brush.verticalGradient(
                                            listOf(
                                                palette.accent.copy(alpha = if (draftEnabled) 0.30f else 0.12f),
                                                palette.accent.copy(alpha = 0.02f),
                                            ),
                                        ),
                                    )
                                    drawPath(
                                        path = curveLinePath,
                                        color = if (draftEnabled) palette.accent else palette.textSecondary,
                                        style = Stroke(width = 4.5f, cap = StrokeCap.Round),
                                    )
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
                                    val selected = draftPresetName.equals(preset, ignoreCase = true)
                                    FerricChoicePill(
                                        label = preset,
                                        selected = selected,
                                        onClick = { onApplyPreset(preset) },
                                    )
                                }
                            }
                            TextButton(
                                onClick = {
                                    isAdjustingSlider = false
                                    for (i in bandGains.indices) bandGains[i] = 0f
                                    draftPreampDb = 0f
                                    draftPresetName = "Flat"
                                    val flatBands = activeEq.bands.map { it.copy(gainDb = 0f) }
                                    onUpdateEq(activeEq.copy(bands = flatBands, preampDb = 0f, presetName = "Flat"))
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
                                    .padding(horizontal = 6.dp, vertical = 12.dp),
                                horizontalArrangement = Arrangement.SpaceEvenly,
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                activeEq.bands.forEachIndexed { idx, band ->
                                    val gainDb = bandGains.getOrElse(idx) { band.gainDb }
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
                                            text = if (kotlin.math.abs(gainDb) < 0.05f) {
                                                "0"
                                            } else {
                                                String.format(Locale.US, "%+.1f", gainDb).removeSuffix(".0")
                                            },
                                            style = MaterialTheme.typography.labelSmall.copy(fontSize = 10.sp),
                                            color = if (kotlin.math.abs(gainDb) >= 0.3f) palette.accent else palette.textSecondary,
                                            textAlign = TextAlign.Center,
                                            maxLines = 1,
                                        )
                                        VerticalEqFader(
                                            gainDb = gainDb,
                                            activeColor = if (draftEnabled) palette.accent else palette.textSecondary,
                                            trackColor = palette.border,
                                            onStart = {
                                                isAdjustingSlider = true
                                                if (bandGains.size != activeEq.bands.size) {
                                                    bandGains.clear()
                                                    activeEq.bands.forEach { bandGains.add(it.gainDb) }
                                                }
                                            },
                                            onChange = { nextDb ->
                                                if (idx in bandGains.indices) {
                                                    bandGains[idx] = nextDb
                                                }
                                                draftPresetName = "Custom"
                                                dispatchThrottledLiveEq()
                                            },
                                            onCommit = {
                                                isAdjustingSlider = false
                                                snapshotEqConfig()?.let(onUpdateEq)
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
                                        text = String.format(Locale.US, "%+.1f dB", draftPreampDb),
                                        style = MaterialTheme.typography.labelSmall,
                                        color = palette.accent,
                                    )
                                }
                                FerricSlimSlider(
                                    value = draftPreampDb.coerceIn(-12f, 12f),
                                    onValueChange = { v ->
                                        isAdjustingSlider = true
                                        draftPreampDb = (v * 10f).roundToInt() / 10f
                                        draftPresetName = "Custom"
                                        dispatchThrottledLiveEq()
                                    },
                                    onValueChangeFinished = {
                                        isAdjustingSlider = false
                                        snapshotEqConfig()?.let(onUpdateEq)
                                    },
                                    valueRange = -12f..12f,
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
                            val selected = draftPresetName.equals(prof.name, ignoreCase = true)
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
                                FerricSlimSlider(
                                    value = draftCrossfadeSec,
                                    onValueChange = { sec -> draftCrossfadeSec = sec },
                                    onValueChangeFinished = {
                                        onUpdateDsp(dspSettings.copy(crossfadeMs = (draftCrossfadeSec.roundToInt() * 1000).toUInt()))
                                    },
                                    valueRange = 0f..12f,
                                    steps = 11,
                                )
                                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    val curves = listOf(
                                        FfiFadeCurve.BASS_SWAP_AUTOMIX to "Bass-Swap 140Hz",
                                        FfiFadeCurve.S_CURVE_VELVET to "S-Curve Velvet",
                                        FfiFadeCurve.EQUAL_POWER to "Equal-Power",
                                    )
                                    items(curves, key = { it.first.name }) { (curve, label) ->
                                        FerricChoicePill(
                                            label = label,
                                            selected = dspSettings.fadeCurve == curve,
                                            onClick = { onUpdateDsp(dspSettings.copy(fadeCurve = curve)) },
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
                                        FerricChoicePill(
                                            label = "${spd}x",
                                            selected = kotlin.math.abs(dspSettings.playbackSpeed - spd) < 0.03f,
                                            onClick = { onUpdateDsp(dspSettings.copy(playbackSpeed = spd)) },
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
                                        FerricChoicePill(
                                            label = lbl,
                                            selected = dspSettings.audioQuality == q,
                                            onClick = { onUpdateDsp(dspSettings.copy(audioQuality = q)) },
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
    onStart: () -> Unit,
    onChange: (Float) -> Unit,
    onCommit: () -> Unit,
) {
    val currentOnStart by rememberUpdatedState(onStart)
    val currentOnChange by rememberUpdatedState(onChange)
    val currentOnCommit by rememberUpdatedState(onCommit)
    var heightPx by remember { mutableIntStateOf(1) }
    val clamped = gainDb.coerceIn(-12f, 12f)
    // +12 dB at top (frac = 0), -12 dB at bottom (frac = 1)
    val thumbFrac = ((12f - clamped) / 24f).coerceIn(0f, 1f)

    fun yToGain(y: Float): Float {
        val frac = (y / heightPx.toFloat().coerceAtLeast(1f)).coerceIn(0f, 1f)
        val raw = 12f - frac * 24f
        return (raw * 10f).roundToInt() / 10f
    }

    Box(
        modifier = Modifier
            .width(34.dp)
            .height(152.dp)
            .onSizeChanged { heightPx = it.height.coerceAtLeast(1) }
            .pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    down.consume()
                    currentOnStart()
                    currentOnChange(yToGain(down.position.y))
                    try {
                        while (true) {
                            val event = awaitPointerEvent()
                            val change = event.changes.firstOrNull { it.id == down.id } ?: event.changes.firstOrNull()
                            if (change == null || !change.pressed || event.changes.none { it.pressed }) {
                                currentOnCommit()
                                break
                            }
                            change.consume()
                            currentOnChange(yToGain(change.position.y))
                        }
                    } catch (e: Throwable) {
                        currentOnCommit()
                        if (e is kotlinx.coroutines.CancellationException) throw e
                    }
                }
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
