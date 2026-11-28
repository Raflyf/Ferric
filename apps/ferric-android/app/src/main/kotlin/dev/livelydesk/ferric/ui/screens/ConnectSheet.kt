package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
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
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.QrCode2
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Smartphone
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.theme.FerricModalBottomSheet
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import kotlin.math.roundToInt
import uniffi.ferric_mobile_ffi.FfiConnectPeer
import uniffi.ferric_mobile_ffi.FfiDeviceKind
import uniffi.ferric_mobile_ffi.FfiQrCode

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConnectSheet(
    state: PlaybackUiState,
    onDismiss: () -> Unit,
    onSheetVisibilityChanged: (Boolean) -> Unit,
    onSwitchSpeaker: (String) -> Unit,
    onSetVolume: (String, Float) -> Unit,
    onOpenSystemOutputSwitcher: () -> Unit,
    onLoadQrCode: (Boolean) -> Unit,
    onCopyPairingUri: (String) -> Unit,
    onPairWithUri: (String) -> Unit,
) {
    val palette = LocalFerricPalette.current
    val connect = state.connectState
    val activePeer: FfiConnectPeer? = connect.peers.firstOrNull {
        it.connected && it.deviceId == connect.activeSpeakerId
    }
    val activeName = if (connect.isLocalActiveSpeaker || activePeer == null) {
        "This phone"
    } else {
        activePeer.deviceName
    }
    val activeSubtitle = if (connect.isLocalActiveSpeaker || activePeer == null) {
        state.currentTrack?.let { "${it.title} - ${it.artist}" } ?: state.activeOutputRoute
    } else {
        if (activePeer.nowPlayingTitle.isNotBlank()) {
            "${activePeer.nowPlayingTitle} - ${activePeer.nowPlayingArtist}"
        } else if (activePeer.viaRelay) {
            "Linked over Cloud E2EE · Anywhere"
        } else {
            "Connected over encrypted Wi-Fi LAN"
        }
    }
    val activeTargetId = if (connect.isLocalActiveSpeaker || activePeer == null) {
        connect.localDeviceId
    } else {
        activePeer.deviceId
    }
    val remoteVol = if (connect.isLocalActiveSpeaker || activePeer == null) {
        state.localVolume
    } else {
        activePeer.volumeNorm
    }

    var isDraggingVol by remember { mutableStateOf(false) }
    var sliderVol by remember(activeTargetId) { mutableFloatStateOf(remoteVol.coerceIn(0f, 1f)) }
    var showGuestQr by remember { mutableStateOf(false) }
    var manualUri by remember { mutableStateOf("") }

    LaunchedEffect(remoteVol, isDraggingVol) {
        if (!isDraggingVol) {
            sliderVol = remoteVol.coerceIn(0f, 1f)
        }
    }

    DisposableEffect(Unit) {
        onSheetVisibilityChanged(true)
        onDispose { onSheetVisibilityChanged(false) }
    }

    FerricModalBottomSheet(
        onDismissRequest = onDismiss,
        coverShade = state.coverShade,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column {
                    Text(
                        text = "Connect",
                        style = MaterialTheme.typography.headlineSmall,
                        color = palette.textPrimary,
                    )
                    Text(
                        text = if (connect.isAccountReady) {
                            if (connect.relayConnected) {
                                "Cloud E2EE + Wi-Fi Connect active (works across any network)"
                            } else {
                                "Auto-Connect active (same Spotify / YouTube Music account)"
                            }
                        } else {
                            "Sign in to Spotify or YouTube Music on PC & phone to link automatically anywhere"
                        },
                        style = MaterialTheme.typography.bodyMedium,
                        color = palette.textSecondary,
                    )
                }
            }

            // Active Speaker Highlight Card (Spotify Connect style)
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .ferricGlassSurface(
                        palette = palette,
                        cornerRadius = palette.cornerRadiusDp,
                        accentGlow = palette.accent,
                        fallbackColor = palette.elevated,
                    ),
            ) {
                Column(
                    modifier = Modifier.padding(14.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
                                Icon(
                                    imageVector = Icons.Rounded.Equalizer,
                                    contentDescription = null,
                                    tint = palette.accent,
                                    modifier = Modifier.size(18.dp),
                                )
                                Text(
                                    text = activeName,
                                    style = MaterialTheme.typography.titleLarge,
                                    color = palette.textPrimary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                            Spacer(Modifier.height(2.dp))
                            Text(
                                text = activeSubtitle,
                                style = MaterialTheme.typography.bodyMedium,
                                color = palette.accent,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                        Icon(
                            imageVector = if (activePeer != null && !connect.isLocalActiveSpeaker && activePeer.deviceKind == FfiDeviceKind.DESKTOP) {
                                Icons.Rounded.Computer
                            } else {
                                Icons.Rounded.Smartphone
                            },
                            contentDescription = null,
                            tint = palette.accent,
                            modifier = Modifier.size(30.dp),
                        )
                    }

                    // Live Volume Slider (controls active Desktop PC or Phone directly)
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Rounded.VolumeUp,
                            contentDescription = "Volume",
                            tint = palette.textSecondary,
                            modifier = Modifier.size(20.dp),
                        )
                        Slider(
                            value = sliderVol,
                            onValueChange = { nextVol ->
                                isDraggingVol = true
                                sliderVol = nextVol
                            },
                            onValueChangeFinished = {
                                isDraggingVol = false
                                onSetVolume(activeTargetId, sliderVol)
                            },
                            valueRange = 0f..1f,
                            modifier = Modifier.weight(1f),
                            colors = SliderDefaults.colors(
                                thumbColor = palette.accent,
                                activeTrackColor = palette.accent,
                                inactiveTrackColor = palette.border,
                            ),
                        )
                        Text(
                            text = "${(sliderVol * 100f).roundToInt()}%",
                            style = MaterialTheme.typography.labelSmall,
                            color = palette.textSecondary,
                            modifier = Modifier.width(36.dp),
                        )
                    }
                }
            }

            // Switch Output To: Device List
            Text(
                text = "Switch output to",
                style = MaterialTheme.typography.titleMedium,
                color = palette.textSecondary,
            )

            DeviceTargetRow(
                title = "This phone",
                subtitle = state.activeOutputRoute,
                isDesktop = false,
                isActive = connect.isLocalActiveSpeaker,
                isConnected = true,
                onClick = { onSwitchSpeaker(connect.localDeviceId) },
            )

            if (connect.peers.isEmpty()) {
                Surface(
                    shape = RoundedCornerShape(12.dp),
                    color = palette.elevated,
                    border = BorderStroke(1.dp, palette.border),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(
                        text = if (connect.relayConnected) {
                            "Cloud E2EE online · Open Ferric Desktop signed into the same Spotify or YouTube Music account on any Wi-Fi or cellular network."
                        } else {
                            "Searching Wi-Fi (${connect.localIp}:${connect.localTcpPort}) and Cloud Relay for Ferric Desktop or other devices on the same account..."
                        },
                        style = MaterialTheme.typography.bodyMedium,
                        color = palette.textSecondary,
                        modifier = Modifier.padding(12.dp),
                    )
                }
            } else {
                connect.peers.forEach { peer ->
                    val modeTag = if (peer.viaRelay) "Cloud E2EE" else "Wi-Fi Direct"
                    DeviceTargetRow(
                        title = peer.deviceName,
                        subtitle = if (peer.connected) {
                            if (peer.nowPlayingTitle.isNotBlank()) {
                                "$modeTag · ${peer.nowPlayingTitle} - ${peer.nowPlayingArtist}"
                            } else {
                                "Connected · $modeTag"
                            }
                        } else {
                            "Connecting · ${peer.addr}"
                        },
                        isDesktop = peer.deviceKind == FfiDeviceKind.DESKTOP,
                        isActive = peer.deviceId == connect.activeSpeakerId,
                        isConnected = peer.connected,
                        onClick = { onSwitchSpeaker(peer.deviceId) },
                    )
                }
            }

            // System Audio Output Switcher (Bluetooth / Wired / Phone Speaker)
            OutlinedButton(
                onClick = onOpenSystemOutputSwitcher,
                modifier = Modifier.fillMaxWidth(),
                border = BorderStroke(1.dp, palette.border),
            ) {
                Icon(Icons.Rounded.Speaker, contentDescription = null, tint = palette.textPrimary)
                Spacer(Modifier.width(8.dp))
                Text("System Audio Output (${state.activeOutputRoute})", color = palette.textPrimary)
            }

            // Guest Mode QR / Pairing URI fallback
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                TextButton(
                    onClick = {
                        showGuestQr = !showGuestQr
                        if (showGuestQr) onLoadQrCode(false)
                    },
                ) {
                    Icon(Icons.Rounded.QrCode2, contentDescription = null, tint = palette.accent)
                    Spacer(Modifier.width(6.dp))
                    Text(
                        text = if (showGuestQr) "Hide Guest QR Pairing" else "Pair another device (Guest Mode)",
                        color = palette.accent,
                    )
                }
            }

            if (showGuestQr) {
                GuestQrSection(
                    qrCode = state.connectQrCode,
                    manualUri = manualUri,
                    onManualUriChange = { manualUri = it },
                    onRegenerate = { onLoadQrCode(true) },
                    onCopyUri = onCopyPairingUri,
                    onPair = {
                        if (manualUri.isNotBlank()) {
                            onPairWithUri(manualUri.trim())
                            manualUri = ""
                        }
                    },
                )
            }

            Spacer(Modifier.height(20.dp))
        }
    }
}

@Composable
private fun DeviceTargetRow(
    title: String,
    subtitle: String,
    isDesktop: Boolean,
    isActive: Boolean,
    isConnected: Boolean,
    onClick: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .ferricGlassSurface(
                palette = palette,
                cornerRadius = 14.dp,
                accentGlow = if (isActive) palette.accent else Color.Unspecified,
                fallbackColor = if (isActive) palette.elevated else palette.surface,
            )
            .clickable(enabled = isConnected, onClick = onClick),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(
                imageVector = if (isDesktop) Icons.Rounded.Computer else Icons.Rounded.Smartphone,
                contentDescription = null,
                tint = if (isActive) palette.accent else palette.textPrimary,
                modifier = Modifier.size(24.dp),
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleMedium,
                    color = if (isActive) palette.accent else palette.textPrimary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = subtitle,
                    style = MaterialTheme.typography.bodyMedium,
                    color = palette.textSecondary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (isActive) {
                Icon(
                    imageVector = Icons.Rounded.CheckCircle,
                    contentDescription = "Active output",
                    tint = palette.accent,
                    modifier = Modifier.size(20.dp),
                )
            }
        }
    }
}

@Composable
private fun GuestQrSection(
    qrCode: FfiQrCode?,
    manualUri: String,
    onManualUriChange: (String) -> Unit,
    onRegenerate: () -> Unit,
    onCopyUri: (String) -> Unit,
    onPair: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .ferricGlassSurface(
                palette = palette,
                cornerRadius = palette.cornerRadiusDp,
                fallbackColor = palette.elevated,
            ),
    ) {
        Column(
            modifier = Modifier.padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            if (qrCode != null && qrCode.size > 0u) {
                val qrDim = qrCode.size.toInt()
                Box(
                    modifier = Modifier
                        .size(164.dp)
                        .clip(RoundedCornerShape(12.dp))
                        .background(Color.White)
                        .padding(10.dp),
                ) {
                    Canvas(modifier = Modifier.size(144.dp)) {
                        val cell = size.width / qrDim.toFloat()
                        for (y in 0 until qrDim) {
                            for (x in 0 until qrDim) {
                                val idx = y * qrDim + x
                                if (idx < qrCode.modules.size && qrCode.modules[idx]) {
                                    drawRect(
                                        color = Color.Black,
                                        topLeft = Offset(x * cell, y * cell),
                                        size = Size(cell + 0.5f, cell + 0.5f),
                                    )
                                }
                            }
                        }
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = onRegenerate) {
                        Icon(Icons.Rounded.Refresh, contentDescription = null, modifier = Modifier.size(16.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("Regenerate")
                    }
                    OutlinedButton(onClick = { onCopyUri(qrCode.uri) }) {
                        Icon(Icons.Rounded.ContentCopy, contentDescription = null, modifier = Modifier.size(16.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("Copy URI")
                    }
                }
            }

            OutlinedTextField(
                value = manualUri,
                onValueChange = onManualUriChange,
                singleLine = true,
                label = { Text("Paste Desktop ferric://pair?... URI") },
                modifier = Modifier.fillMaxWidth(),
            )
            Button(
                onClick = onPair,
                enabled = manualUri.trim().startsWith("ferric://pair?"),
                colors = ButtonDefaults.buttonColors(containerColor = palette.accent),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Pair Guest Device", color = if (palette.isLight) Color.White else Color.Black)
            }
        }
    }
}
