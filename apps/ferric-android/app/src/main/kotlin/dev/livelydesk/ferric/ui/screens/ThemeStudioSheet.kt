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
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Upload
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import uniffi.ferric_mobile_ffi.FfiThemeConfig

private data class ThemePresetCardSpec(
    val name: String,
    val tagline: String,
    val swatchBg: Color,
    val swatchAccent: Color,
)

private val THEME_PRESETS = listOf(
    ThemePresetCardSpec("Liquid Glass", "AGSL Lens + Aurora", Color(0xFF141826), Color(0xFF8AB4F8)),
    ThemePresetCardSpec("AMOLED Black", "Pure #000 OLED", Color(0xFF000000), Color(0xFFE86A2C)),
    ThemePresetCardSpec("Obsidian Studio", "Studio dark slate", Color(0xFF0D0E12), Color(0xFFE86A2C)),
    ThemePresetCardSpec("Warm Cream Light", "High-contrast paper", Color(0xFFF3EEE3), Color(0xFFD9541E)),
)

private val UI_FONTS = listOf(
    "DM Sans",
    "Figtree",
    "Space Grotesk",
    "JetBrains Mono",
)

private val PLAYER_LAYOUTS = listOf(
    "full_bleed" to "Full-Bleed",
    "liquid_glass" to "Liquid Glass",
    "standard" to "Standard",
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ThemeStudioSheet(
    themeConfig: FfiThemeConfig?,
    onDismiss: () -> Unit,
    onApplyPreset: (String) -> Unit,
    onUpdateTheme: (FfiThemeConfig) -> Unit,
    onPreviewTheme: (FfiThemeConfig) -> Unit = onUpdateTheme,
    onImportThemeJson: () -> Unit,
    onExportThemeJson: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var draftOpacity by remember(themeConfig?.acrylicOpacity) {
        mutableFloatStateOf((themeConfig?.acrylicOpacity ?: 0.62f).coerceIn(0.15f, 0.95f))
    }
    var draftRadius by remember(themeConfig?.cornerRadiusPx) {
        mutableFloatStateOf((themeConfig?.cornerRadiusPx ?: 20f).coerceIn(0f, 28f))
    }
    LaunchedEffect(themeConfig?.acrylicOpacity) {
        if (themeConfig != null) draftOpacity = themeConfig.acrylicOpacity.coerceIn(0.15f, 0.95f)
    }
    LaunchedEffect(themeConfig?.cornerRadiusPx) {
        if (themeConfig != null) draftRadius = themeConfig.cornerRadiusPx.coerceIn(0f, 28f)
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = palette.surface.copy(alpha = if (palette.isLiquidGlass) (palette.glassOpacity + 0.14f).coerceIn(0.45f, 0.96f) else 1f),
        scrimColor = Color.Black.copy(alpha = 0.24f),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 18.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text("Theme & Visual Studio", style = MaterialTheme.typography.titleLarge, color = palette.textPrimary)
            Text(
                "100% compatible with Ferric Desktop v0.1.2 .ferric-theme.json files.",
                style = MaterialTheme.typography.bodyMedium,
                color = palette.textSecondary,
            )

            LazyRow(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                items(THEME_PRESETS, key = { it.name }) { preset ->
                    val selected = themeConfig?.name.equals(preset.name, ignoreCase = true)
                    Surface(
                        shape = RoundedCornerShape(palette.cornerRadiusDp),
                        color = palette.elevated,
                        border = BorderStroke(
                            width = if (selected) 2.dp else 1.dp,
                            color = if (selected) palette.accent else palette.border,
                        ),
                        modifier = Modifier
                            .width(146.dp)
                            .clickable { onApplyPreset(preset.name) },
                    ) {
                        Column(
                            modifier = Modifier.padding(12.dp),
                            verticalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                Box(
                                    modifier = Modifier
                                        .size(22.dp)
                                        .clip(CircleShape)
                                        .background(preset.swatchBg),
                                )
                                Box(
                                    modifier = Modifier
                                        .size(22.dp)
                                        .clip(CircleShape)
                                        .background(preset.swatchAccent),
                                )
                            }
                            Text(preset.name, style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)
                            Text(preset.tagline, style = MaterialTheme.typography.labelSmall, color = palette.textSecondary)
                        }
                    }
                }
            }

            if (themeConfig != null) {
                Text("Full Player Layout", style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)
                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    items(PLAYER_LAYOUTS, key = { it.first }) { (key, label) ->
                        FilterChip(
                            selected = themeConfig.playerLayout == key,
                            onClick = { onUpdateTheme(themeConfig.copy(playerLayout = key)) },
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

                Text("UI Font Family", style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)
                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    items(UI_FONTS, key = { it }) { font ->
                        val isSelected = themeConfig.fontFamily.equals(font, ignoreCase = true) ||
                            (font == "DM Sans" && themeConfig.fontFamily.equals("Inter", ignoreCase = true)) ||
                            (font == "Figtree" && themeConfig.fontFamily.equals("Plus Jakarta Sans", ignoreCase = true))
                        FilterChip(
                            selected = isSelected,
                            onClick = { onUpdateTheme(themeConfig.copy(fontFamily = font)) },
                            label = { Text(font) },
                            colors = FilterChipDefaults.filterChipColors(
                                selectedContainerColor = palette.accent,
                                selectedLabelColor = if (palette.isLight) Color.White else Color.Black,
                                containerColor = palette.elevated,
                                labelColor = palette.textPrimary,
                            ),
                        )
                    }
                }

                ThemeToggleRow("Editorial Typography Mode", themeConfig.editorialMode) {
                    onUpdateTheme(themeConfig.copy(editorialMode = it))
                }
                ThemeToggleRow("Adaptive Artwork Accent (WCAG AA)", themeConfig.adaptiveAlbumArt) {
                    onUpdateTheme(themeConfig.copy(adaptiveAlbumArt = it))
                }
                ThemeToggleRow("CoverShade Dynamic Glass Tint", themeConfig.adaptiveAcrylicTint) {
                    onUpdateTheme(themeConfig.copy(adaptiveAcrylicTint = it))
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("Glass Frost Opacity", style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)
                    Text("${(draftOpacity * 100).toInt()}%", color = palette.accent, style = MaterialTheme.typography.labelSmall)
                }
                Slider(
                    value = draftOpacity,
                    onValueChange = { next ->
                        draftOpacity = next
                        onPreviewTheme(themeConfig.copy(acrylicOpacity = next))
                    },
                    onValueChangeFinished = {
                        onUpdateTheme(themeConfig.copy(acrylicOpacity = draftOpacity))
                    },
                    valueRange = 0.15f..0.95f,
                    colors = SliderDefaults.colors(
                        thumbColor = palette.accent,
                        activeTrackColor = palette.accent,
                        inactiveTrackColor = palette.border,
                    ),
                )

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("Card Corner Radius", style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)
                    Text("${draftRadius.toInt()} dp", color = palette.accent, style = MaterialTheme.typography.labelSmall)
                }
                Slider(
                    value = draftRadius,
                    onValueChange = { next ->
                        draftRadius = next
                        onPreviewTheme(themeConfig.copy(cornerRadiusPx = next))
                    },
                    onValueChangeFinished = {
                        onUpdateTheme(themeConfig.copy(cornerRadiusPx = draftRadius))
                    },
                    valueRange = 0f..28f,
                    colors = SliderDefaults.colors(
                        thumbColor = palette.accent,
                        activeTrackColor = palette.accent,
                        inactiveTrackColor = palette.border,
                    ),
                )

                // Live tactile preview reflecting active Font, Corner Radius, Accent, and Frost Opacity
                Surface(
                    shape = RoundedCornerShape(draftRadius.dp),
                    color = palette.glassTint.copy(alpha = (draftOpacity * 0.90f).coerceIn(0.14f, 0.92f)),
                    border = BorderStroke(1.5.dp, palette.accent.copy(alpha = 0.75f)),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Row(
                        modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                text = "Live Surface Preview",
                                style = MaterialTheme.typography.titleMedium,
                                color = palette.textPrimary,
                            )
                            Text(
                                text = "${themeConfig.fontFamily} · ${draftRadius.toInt()}dp radius · ${(draftOpacity * 100).toInt()}% frost",
                                style = MaterialTheme.typography.bodyMedium,
                                color = palette.textSecondary,
                            )
                        }
                        Surface(
                            shape = RoundedCornerShape((draftRadius * 0.7f).coerceIn(0f, 18f).dp),
                            color = palette.accent,
                        ) {
                            Text(
                                text = if (themeConfig.editorialMode) "Serif" else "Sans",
                                style = MaterialTheme.typography.labelLarge,
                                color = Color.Black,
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                            )
                        }
                    }
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    OutlinedButton(
                        onClick = onImportThemeJson,
                        border = BorderStroke(1.dp, palette.border),
                        modifier = Modifier.weight(1f),
                    ) {
                        Icon(Icons.Rounded.Upload, contentDescription = null, tint = palette.textPrimary)
                        Spacer(Modifier.width(6.dp))
                        Text("Import JSON", color = palette.textPrimary)
                    }
                    OutlinedButton(
                        onClick = onExportThemeJson,
                        border = BorderStroke(1.dp, palette.border),
                        modifier = Modifier.weight(1f),
                    ) {
                        Icon(Icons.Rounded.Download, contentDescription = null, tint = palette.textPrimary)
                        Spacer(Modifier.width(6.dp))
                        Text("Export JSON", color = palette.textPrimary)
                    }
                }
            }

            Spacer(Modifier.height(18.dp))
        }
    }
}

@Composable
private fun ThemeToggleRow(
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
