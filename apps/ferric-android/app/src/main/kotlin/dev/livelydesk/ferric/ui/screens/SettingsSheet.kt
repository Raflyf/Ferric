package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
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
import androidx.compose.material.icons.rounded.AccountCircle
import androidx.compose.material.icons.rounded.BarChart
import androidx.compose.material.icons.rounded.DeleteSweep
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Palette
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.Storage
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.BuildConfig
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.theme.FerricModalBottomSheet
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.displayUiFontName
import dev.livelydesk.ferric.ui.theme.ferricDialogContainerColor
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import java.util.Locale

private fun fmtMb(bytes: ULong): String =
    String.format(Locale.US, "%.1f MB", bytes.toDouble() / (1024.0 * 1024.0))

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsSheet(
    state: PlaybackUiState,
    onRefreshStorageAndStats: () -> Unit,
    onDismiss: () -> Unit,
    onOpenAccounts: () -> Unit,
    onOpenConnect: () -> Unit,
    onOpenThemeStudio: () -> Unit,
    onOpenEqualizer: () -> Unit,
    onOpenOutputSwitcher: () -> Unit,
    onOpenStats: () -> Unit,
    onScanLocalMusic: () -> Unit,
    onPickSafFolder: () -> Unit,
    onClearAudioCache: () -> Unit,
    onClearArtworkCache: () -> Unit,
    onDeleteAllOffline: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    var query by remember { mutableStateOf("") }
    var showDeleteAllConfirm by remember { mutableStateOf(false) }
    val needle = query.trim().lowercase(Locale.ROOT)
    fun matches(vararg terms: String): Boolean =
        needle.isEmpty() || terms.any { it.lowercase(Locale.ROOT).contains(needle) }

    LaunchedEffect(Unit) { onRefreshStorageAndStats() }

    val storage = state.storageStats

    FerricModalBottomSheet(
        onDismissRequest = onDismiss,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text("Settings", style = MaterialTheme.typography.headlineSmall, color = palette.textPrimary)

            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                singleLine = true,
                leadingIcon = { Icon(Icons.Rounded.Search, contentDescription = null, tint = palette.textSecondary) },
                placeholder = { Text("Search settings (theme, eq, cache, connect, offline...)") },
                modifier = Modifier.fillMaxWidth(),
            )

            if (matches("accounts", "spotify", "youtube", "subsonic", "login", "sync")) {
                SettingsRowCard(
                    icon = Icons.Rounded.AccountCircle,
                    title = "Accounts & Two-Way Sync",
                    subtitle = buildString {
                        val parts = mutableListOf<String>()
                        if (state.accountStatus.spotifyConnected) parts += "Spotify"
                        if (state.accountStatus.youtubeConnected) parts += "YouTube Music"
                        if (state.accountStatus.subsonicConnected) parts += "Subsonic"
                        append(if (parts.isEmpty()) "Guest Mode · Tap to sign in" else "Connected: ${parts.joinToString(", ")}")
                    },
                    onClick = {
                        onDismiss()
                        onOpenAccounts()
                    },
                )
            }

            if (matches("connect", "desktop", "handoff", "lan", "wifi", "qr", "speaker", "remote")) {
                val peerCount = state.connectState.peers.count { it.connected }
                SettingsRowCard(
                    icon = Icons.Rounded.Devices,
                    title = "Ferric Connect (Zero-Pairing LAN)",
                    subtitle = if (peerCount > 0) {
                        "$peerCount device(s) connected over encrypted ChaCha20 LAN"
                    } else {
                        "Auto-Connect via Spotify/YTM account · ${state.connectState.localIp}"
                    },
                    onClick = {
                        onDismiss()
                        onOpenConnect()
                    },
                )
            }

            if (matches("theme", "liquid glass", "amoled", "font", "editorial", "covershade", "visual", "appearance")) {
                SettingsRowCard(
                    icon = Icons.Rounded.Palette,
                    title = "Visual & Theme Studio",
                    subtitle = "${state.themeConfig?.name ?: "Liquid Glass"} · Font: ${displayUiFontName(state.themeConfig?.fontFamily)}",
                    onClick = {
                        onDismiss()
                        onOpenThemeStudio()
                    },
                )
            }

            if (matches("equalizer", "eq", "autoeq", "apo", "dsp", "crossfade", "bass", "speed", "quality", "replaygain", "mono")) {
                SettingsRowCard(
                    icon = Icons.Rounded.Equalizer,
                    title = "Equalizer, AutoEq & Audio DSP",
                    subtitle = "${if (state.eqConfig?.enabled == true) "EQ Active (${state.eqConfig.presetName})" else "EQ Off"} · ${state.dspSettings?.audioQuality?.name ?: "HIGH"}",
                    onClick = {
                        onDismiss()
                        onOpenEqualizer()
                    },
                )
            }

            if (matches("audio", "output", "bluetooth", "dac", "usb", "speaker", "headphone")) {
                SettingsRowCard(
                    icon = Icons.Rounded.Speaker,
                    title = "System Audio Output",
                    subtitle = "Active route: ${state.activeOutputRoute}",
                    onClick = onOpenOutputSwitcher,
                )
            }

            if (matches("stats", "streak", "history", "top artists", "listening", "plays")) {
                val st = state.listeningStats
                SettingsRowCard(
                    icon = Icons.Rounded.BarChart,
                    title = "Listening Stats & Daily Streak",
                    subtitle = if (st != null) {
                        "${st.totalPlays} plays · ${st.currentStreakDays}d streak"
                    } else {
                        "View local play history & top artists"
                    },
                    onClick = {
                        onDismiss()
                        onOpenStats()
                    },
                )
            }

            if (matches("local", "flac", "folder", "scan", "storage", "mediastore", "saf")) {
                SettingsRowCard(
                    icon = Icons.Rounded.FolderOpen,
                    title = "Local Lossless Library",
                    subtitle = "${state.localTracks.size} local track(s) indexed via MediaStore / SAF",
                    onClick = onScanLocalMusic,
                )
                OutlinedButton(
                    onClick = onPickSafFolder,
                    border = BorderStroke(1.dp, palette.border),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Pick Custom SAF Folder", color = palette.textPrimary)
                }
            }

            if (matches("storage", "offline", "downloads", "cache", "artwork", "clear", "delete", "database")) {
                Surface(
                    shape = RoundedCornerShape(14.dp),
                    color = palette.elevated,
                    border = BorderStroke(1.dp, palette.border),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(
                        modifier = Modifier.padding(14.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Icon(Icons.Rounded.Storage, contentDescription = null, tint = palette.accent, modifier = Modifier.size(20.dp))
                            Spacer(Modifier.width(8.dp))
                            Text("Offline Downloads & Cache Storage", style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)
                        }
                        if (storage != null) {
                            Text(
                                text = "Offline: ${storage.offlineTrackCount} tracks (${fmtMb(storage.offlineBytes)}) · Stream Cache: ${fmtMb(storage.streamCacheBytes)} · Artwork: ${fmtMb(storage.artworkCacheBytes)} · DB: ${fmtMb(storage.databaseBytes)}",
                                style = MaterialTheme.typography.bodyMedium,
                                color = palette.textSecondary,
                            )
                        }
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            OutlinedButton(
                                onClick = onClearAudioCache,
                                border = BorderStroke(1.dp, palette.border),
                                modifier = Modifier.weight(1f),
                            ) {
                                Text("Clear Stream", color = palette.textPrimary)
                            }
                            OutlinedButton(
                                onClick = onClearArtworkCache,
                                border = BorderStroke(1.dp, palette.border),
                                modifier = Modifier.weight(1f),
                            ) {
                                Text("Clear Art", color = palette.textPrimary)
                            }
                        }
                        OutlinedButton(
                            onClick = { showDeleteAllConfirm = true },
                            border = BorderStroke(1.dp, palette.accent.copy(alpha = 0.6f)),
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Icon(Icons.Rounded.DeleteSweep, contentDescription = null, tint = palette.accent, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text("Delete All Offline Downloads", color = palette.accent)
                        }
                    }
                }
            }

            if (matches("about", "version", "rust", "oboe", "uniffi", "ferric")) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .ferricGlassSurface(
                            palette = palette,
                            cornerRadius = palette.cornerRadiusDp,
                            fallbackColor = palette.elevated,
                        ),
                ) {
                    Row(
                        modifier = Modifier.padding(14.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(Icons.Rounded.Info, contentDescription = null, tint = palette.accent, modifier = Modifier.size(22.dp))
                        Spacer(Modifier.width(12.dp))
                        Column {
                            Text("Ferric Android v${BuildConfig.VERSION_NAME}", style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)
                            Text(
                                "Native Rust Oboe Dual-Deck Audio · UniFFI · SQLite WAL · Zero Telemetry",
                                style = MaterialTheme.typography.bodyMedium,
                                color = palette.textSecondary,
                            )
                        }
                    }
                }
            }

            Spacer(Modifier.height(20.dp))
        }
    }

    if (showDeleteAllConfirm) {
        AlertDialog(
            onDismissRequest = { showDeleteAllConfirm = false },
            containerColor = ferricDialogContainerColor(palette),
            title = { Text("Delete All Offline Downloads?", color = palette.textPrimary) },
            text = {
                Text(
                    "Remove all ${storage?.offlineTrackCount ?: state.offlineTracks.size} downloaded offline tracks from device storage? Local phone music files are untouched.",
                    color = palette.textSecondary,
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteAllConfirm = false
                        onDeleteAllOffline()
                    },
                ) { Text("Delete All", color = palette.accent) }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteAllConfirm = false }) {
                    Text("Cancel", color = palette.textSecondary)
                }
            },
        )
    }
}

@Composable
private fun SettingsRowCard(
    icon: ImageVector,
    title: String,
    subtitle: String,
    onClick: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .ferricGlassSurface(
                palette = palette,
                cornerRadius = palette.cornerRadiusDp,
                fallbackColor = palette.elevated,
            )
            .clickable(onClick = onClick),
    ) {
        Row(
            modifier = Modifier.padding(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(icon, contentDescription = null, tint = palette.accent, modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.titleMedium, color = palette.textPrimary)
                Text(subtitle, style = MaterialTheme.typography.bodyMedium, color = palette.textSecondary)
            }
        }
    }
}
