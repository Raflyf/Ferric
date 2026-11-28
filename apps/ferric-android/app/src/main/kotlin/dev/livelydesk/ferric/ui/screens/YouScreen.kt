package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.BarChart
import androidx.compose.material.icons.rounded.CloudQueue
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.Palette
import androidx.compose.material.icons.rounded.PlayCircle
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.BuildConfig
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.ui.theme.GlassTier
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.displayUiFontName
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface
import dev.livelydesk.ferric.ui.theme.ferricLiquidGlass

/**
 * Cleanly structured "You" hub ordered for rapid scanning:
 * Header + live streak summary -> Accounts -> Playback & Studio -> Activity -> Settings & About.
 */
@Composable
fun YouScreen(
    state: PlaybackUiState,
    listState: LazyListState = rememberLazyListState(),
    onOpenHistory: () -> Unit,
    onOpenStats: () -> Unit,
    onOpenConnect: () -> Unit,
    onOpenThemeStudio: () -> Unit,
    onOpenEqualizer: () -> Unit,
    onSignInYtm: () -> Unit,
    onSyncYtm: () -> Unit,
    onSignInSpotify: () -> Unit,
    onSyncSpotify: () -> Unit,
    onOpenAccounts: () -> Unit,
    onOpenSettings: () -> Unit,
    onOpenAbout: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    val accounts = state.accountStatus
    val connectedPeers = state.connectState.peers.filter { it.connected }
    val cardBg = if (palette.isLight) {
        palette.surface.copy(alpha = if (palette.isLiquidGlass) (palette.glassOpacity + 0.16f).coerceIn(0.35f, 0.96f) else 0.90f)
    } else if (palette.isLiquidGlass) {
        palette.glassTint.copy(alpha = (palette.glassOpacity * 0.88f).coerceIn(0.10f, 0.88f))
    } else {
        palette.elevated.copy(alpha = (palette.glassOpacity * 0.92f).coerceIn(0.45f, 0.96f))
    }
    val dividerColor = palette.border.copy(alpha = if (palette.isLiquidGlass) 0.40f else 0.70f)

    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 18.dp, end = 18.dp, top = 14.dp, bottom = 176.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        // Header + compact quick stats chip
        item(key = "you_header") {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(bottom = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    text = "You",
                    style = MaterialTheme.typography.displaySmall,
                    fontWeight = FontWeight.Light,
                    color = palette.textPrimary,
                )
                val st = state.listeningStats
                if (st != null && st.totalPlays > 0uL) {
                    Box(
                        modifier = Modifier
                            .ferricGlassSurface(
                                palette = palette,
                                cornerRadius = 999.dp,
                                fallbackColor = cardBg,
                            )
                            .clickable(onClick = onOpenStats)
                            .padding(horizontal = 12.dp, vertical = 6.dp),
                    ) {
                        Text(
                            text = "${st.currentStreakDays}d streak · ${st.totalPlays} plays",
                            style = MaterialTheme.typography.labelMedium,
                            color = palette.textPrimary,
                        )
                    }
                }
            }
        }

        // Section 1: Streaming Accounts & Personal Server
        item(key = "you_accounts_section") {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    BrandedAccountTile(
                        title = "YouTube Music",
                        statusLine = if (accounts.youtubeConnected) {
                            accounts.youtubeChannelName.ifBlank { "${accounts.youtubePlaylistCount} playlists" }
                        } else {
                            "Sync library & Home"
                        },
                        connected = accounts.youtubeConnected,
                        tintColor = Color(0xFFFF2B4A),
                        icon = Icons.Rounded.PlayCircle,
                        actionLabel = if (accounts.youtubeConnected) "Sync" else "Sign in",
                        onAction = if (accounts.youtubeConnected) onSyncYtm else onSignInYtm,
                        onCardClick = onOpenAccounts,
                        modifier = Modifier.weight(1f),
                    )
                    BrandedAccountTile(
                        title = "Spotify",
                        statusLine = if (accounts.spotifyConnected) {
                            accounts.spotifyUserName.ifBlank { "${accounts.spotifyPlaylistCount} playlists" }
                        } else {
                            "Playlists & Auto-Connect"
                        },
                        connected = accounts.spotifyConnected,
                        tintColor = Color(0xFF1ED760),
                        icon = Icons.Rounded.MusicNote,
                        actionLabel = if (accounts.spotifyConnected) "Sync" else "Connect",
                        onAction = if (accounts.spotifyConnected) onSyncSpotify else onSignInSpotify,
                        onCardClick = onOpenAccounts,
                        modifier = Modifier.weight(1f),
                    )
                }

                GroupedHubCard(background = cardBg, border = dividerColor) {
                    HubActionRow(
                        icon = Icons.Rounded.CloudQueue,
                        title = "OpenSubsonic / Navidrome",
                        subtitle = if (accounts.subsonicConnected) {
                            "${accounts.subsonicUrl} (${accounts.subsonicUsername})"
                        } else {
                            "Personal lossless server"
                        },
                        onClick = onOpenAccounts,
                    )
                }
            }
        }

        // Section 2: Connect, Audio Engine & Visual Appearance
        item(key = "you_playback_studio_group") {
            GroupedHubCard(background = cardBg, border = dividerColor) {
                HubActionRow(
                    icon = Icons.Rounded.Devices,
                    title = "Ferric Connect",
                    subtitle = if (connectedPeers.isNotEmpty()) {
                        "Connected to ${connectedPeers.joinToString { it.deviceName }}"
                    } else if (state.connectState.isAccountReady) {
                        if (state.connectState.relayConnected) {
                            "Cloud E2EE & Wi-Fi ready"
                        } else {
                            "Auto-Connect ready"
                        }
                    } else {
                        "Link Desktop & phone"
                    },
                    badgeText = if (connectedPeers.isNotEmpty()) "${connectedPeers.size} LIVE" else null,
                    badgeColor = Color(0xFF22C55E),
                    onClick = onOpenConnect,
                )
                HorizontalDivider(color = dividerColor, modifier = Modifier.padding(horizontal = 16.dp))
                HubActionRow(
                    icon = Icons.Rounded.Equalizer,
                    title = "Equalizer & Audio DSP",
                    subtitle = "${if (state.eqConfig?.enabled == true) state.eqConfig.presetName else "Flat"} · ${state.activeOutputRoute}",
                    onClick = onOpenEqualizer,
                )
                HorizontalDivider(color = dividerColor, modifier = Modifier.padding(horizontal = 16.dp))
                HubActionRow(
                    icon = Icons.Rounded.Palette,
                    title = "Theme Studio",
                    subtitle = "${state.themeConfig?.name ?: "Liquid Glass"} · ${displayUiFontName(state.themeConfig?.fontFamily)}",
                    onClick = onOpenThemeStudio,
                )
            }
        }

        // Section 3: Listening Activity
        item(key = "you_activity_group") {
            GroupedHubCard(background = cardBg, border = dividerColor) {
                HubActionRow(
                    icon = Icons.Rounded.History,
                    title = "History",
                    subtitle = if (state.recentTracks.isNotEmpty()) {
                        "${state.recentTracks.size} recently played"
                    } else {
                        "Recently played tracks"
                    },
                    onClick = onOpenHistory,
                )
                HorizontalDivider(color = dividerColor, modifier = Modifier.padding(horizontal = 16.dp))
                HubActionRow(
                    icon = Icons.Rounded.BarChart,
                    title = "Stats & Recap",
                    subtitle = state.listeningStats?.let { st ->
                        if (st.totalPlays > 0uL) {
                            "${st.totalPlays} plays · ${st.currentStreakDays}d streak"
                        } else {
                            "Local play insights & streak"
                        }
                    } ?: "Local play insights & streak",
                    onClick = onOpenStats,
                )
            }
        }

        // Section 4: Separated Settings & About
        item(key = "you_app_group") {
            GroupedHubCard(background = cardBg, border = dividerColor) {
                HubActionRow(
                    icon = Icons.Rounded.Settings,
                    title = "Settings & Storage",
                    subtitle = "Offline downloads, cache, local lossless folders",
                    onClick = onOpenSettings,
                )
                HorizontalDivider(color = dividerColor, modifier = Modifier.padding(horizontal = 16.dp))
                HubActionRow(
                    icon = Icons.Rounded.Info,
                    title = "About Ferric",
                    subtitle = "v${BuildConfig.VERSION_NAME} · Updates, links & support",
                    onClick = onOpenAbout,
                )
            }
        }
    }
}

@Composable
private fun GroupedHubCard(
    background: Color,
    border: Color,
    content: @Composable () -> Unit,
) {
    val palette = LocalFerricPalette.current
    if (palette.isLiquidGlass) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .ferricGlassSurface(
                    palette = palette,
                    cornerRadius = palette.cornerRadiusDp,
                    tier = GlassTier.FROSTED_PANEL,
                ),
        ) {
            Column(modifier = Modifier.fillMaxWidth()) {
                content()
            }
        }
    } else {
        Surface(
            shape = RoundedCornerShape(palette.cornerRadiusDp),
            color = background,
            border = BorderStroke(1.dp, border),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(modifier = Modifier.fillMaxWidth()) {
                content()
            }
        }
    }
}

@Composable
private fun HubActionRow(
    icon: ImageVector,
    title: String,
    subtitle: String,
    badgeText: String? = null,
    badgeColor: Color = Color(0xFF22C55E),
    showChevron: Boolean = true,
    onClick: () -> Unit,
) {
    val palette = LocalFerricPalette.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = icon,
            contentDescription = null,
            tint = palette.textPrimary.copy(alpha = 0.88f),
            modifier = Modifier.size(21.dp),
        )
        Spacer(Modifier.width(13.dp))
        Column(modifier = Modifier.weight(1f)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleMedium,
                    color = palette.textPrimary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (!badgeText.isNullOrBlank()) {
                    Surface(
                        shape = RoundedCornerShape(999.dp),
                        color = badgeColor.copy(alpha = 0.18f),
                    ) {
                        Text(
                            text = badgeText,
                            style = MaterialTheme.typography.labelSmall,
                            color = badgeColor,
                            modifier = Modifier.padding(horizontal = 7.dp, vertical = 2.dp),
                        )
                    }
                }
            }
            Text(
                text = subtitle,
                style = MaterialTheme.typography.bodyMedium,
                color = palette.textSecondary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (showChevron) {
            Icon(
                imageVector = Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                contentDescription = null,
                tint = palette.textSecondary.copy(alpha = 0.7f),
                modifier = Modifier.size(20.dp),
            )
        }
    }
}

@Composable
private fun BrandedAccountTile(
    title: String,
    statusLine: String,
    connected: Boolean,
    tintColor: Color,
    icon: ImageVector,
    actionLabel: String,
    onAction: () -> Unit,
    onCardClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val palette = LocalFerricPalette.current
    val tileMod = if (palette.isLiquidGlass) {
        modifier
            .ferricLiquidGlass(
                cornerRadius = palette.cornerRadiusDp,
                tier = GlassTier.FROSTED_PANEL,
                accentGlow = tintColor,
            )
            .background(
                Brush.verticalGradient(
                    listOf(
                        tintColor.copy(alpha = 0.22f),
                        tintColor.copy(alpha = 0.08f),
                    ),
                ),
            )
    } else {
        val gradient = Brush.verticalGradient(
            colors = if (palette.isLight) {
                listOf(tintColor.copy(alpha = 0.15f), palette.surface)
            } else {
                listOf(tintColor.copy(alpha = 0.24f), tintColor.copy(alpha = 0.08f))
            },
        )
        modifier
            .clip(RoundedCornerShape(palette.cornerRadiusDp))
            .background(gradient)
    }
    Box(
        modifier = tileMod
            .clickable(onClick = onCardClick),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Box(
                    modifier = Modifier
                        .size(34.dp)
                        .clip(CircleShape)
                        .background(tintColor.copy(alpha = 0.22f)),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = icon,
                        contentDescription = null,
                        tint = tintColor,
                        modifier = Modifier.size(19.dp),
                    )
                }
                if (connected) {
                    Box(
                        modifier = Modifier
                            .size(8.dp)
                            .clip(CircleShape)
                            .background(tintColor),
                    )
                }
            }
            Text(
                text = title,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                color = palette.textPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = statusLine,
                style = MaterialTheme.typography.bodySmall,
                color = palette.textSecondary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (palette.isLiquidGlass) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .ferricGlassSurface(
                            palette = palette,
                            cornerRadius = 999.dp,
                            accentGlow = tintColor,
                        )
                        .clickable(onClick = onAction)
                        .padding(horizontal = 14.dp, vertical = 7.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = actionLabel,
                        style = MaterialTheme.typography.labelLarge,
                        color = palette.textPrimary,
                    )
                }
            } else {
                Button(
                    onClick = onAction,
                    shape = RoundedCornerShape(999.dp),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 5.dp),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = if (connected) {
                            palette.textPrimary.copy(alpha = 0.14f)
                        } else if (palette.isLight) {
                            palette.textPrimary
                        } else {
                            Color.White
                        },
                        contentColor = if (connected) {
                            palette.textPrimary
                        } else if (palette.isLight) {
                            Color.White
                        } else {
                            Color.Black
                        },
                    ),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(
                        text = actionLabel,
                        style = MaterialTheme.typography.labelLarge,
                    )
                }
            }
        }
    }
}
