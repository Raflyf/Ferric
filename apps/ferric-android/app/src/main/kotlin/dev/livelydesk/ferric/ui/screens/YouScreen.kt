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
import androidx.compose.material.icons.rounded.LibraryMusic
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
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.displayUiFontName

/**
 * Dedicated "You" tab inspired by Auralys's serene grouped hub:
 * cleanly houses History, Listening Stats & Streak, Ferric Connect, Theme Studio, Equalizer & DSP,
 * side-by-side branded YouTube Music & Spotify account cards, OpenSubsonic, and Settings so Home
 * stays spacious and uncluttered.
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
) {
    val palette = LocalFerricPalette.current
    val accounts = state.accountStatus
    val connectedPeers = state.connectState.peers.filter { it.connected }
    val cardBg = if (palette.isLight) {
        palette.surface.copy(alpha = 0.88f)
    } else {
        palette.elevated.copy(alpha = if (palette.isLiquidGlass) 0.56f else 0.88f)
    }
    val dividerColor = palette.border.copy(alpha = if (palette.isLiquidGlass) 0.40f else 0.70f)

    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 18.dp, end = 18.dp, top = 16.dp, bottom = 180.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        item(key = "you_header") {
            Text(
                text = "You",
                style = MaterialTheme.typography.displaySmall,
                fontWeight = FontWeight.Light,
                color = palette.textPrimary,
            )
        }

        // Group 1: Activity & Insights
        item(key = "you_activity_group") {
            GroupedHubCard(background = cardBg, border = dividerColor) {
                HubActionRow(
                    icon = Icons.Rounded.History,
                    title = "History",
                    subtitle = if (state.recentTracks.isNotEmpty()) {
                        "${state.recentTracks.size} recently played"
                    } else {
                        "Your listening history"
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
                            "Local play insights & daily streak"
                        }
                    } ?: "Local play insights & daily streak",
                    onClick = onOpenStats,
                )
            }
        }

        // Group 2: Connect, Appearance & Audio Engine
        item(key = "you_studio_group") {
            GroupedHubCard(background = cardBg, border = dividerColor) {
                HubActionRow(
                    icon = Icons.Rounded.Devices,
                    title = "Ferric Connect",
                    subtitle = if (connectedPeers.isNotEmpty()) {
                        "Connected to ${connectedPeers.joinToString { it.deviceName }}"
                    } else if (state.connectState.isAccountReady) {
                        "Auto-Connect ready on Wi-Fi"
                    } else {
                        "Link Desktop & phone over Wi-Fi"
                    },
                    badgeText = if (connectedPeers.isNotEmpty()) "${connectedPeers.size} LIVE" else null,
                    badgeColor = Color(0xFF22C55E),
                    onClick = onOpenConnect,
                )
                HorizontalDivider(color = dividerColor, modifier = Modifier.padding(horizontal = 16.dp))
                HubActionRow(
                    icon = Icons.Rounded.Palette,
                    title = "Theme Studio",
                    subtitle = "${state.themeConfig?.name ?: "Liquid Glass"} · ${displayUiFontName(state.themeConfig?.fontFamily)}",
                    onClick = onOpenThemeStudio,
                )
                HorizontalDivider(color = dividerColor, modifier = Modifier.padding(horizontal = 16.dp))
                HubActionRow(
                    icon = Icons.Rounded.Equalizer,
                    title = "Equalizer & Audio DSP",
                    subtitle = "${if (state.eqConfig?.enabled == true) state.eqConfig.presetName else "Flat"} · ${state.activeOutputRoute}",
                    onClick = onOpenEqualizer,
                )
            }
        }

        // Group 3: Side-by-side Branded Account Cards (Auralys signature!)
        item(key = "you_accounts_cards") {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    BrandedAccountTile(
                        title = "YouTube Music",
                        statusLine = if (accounts.youtubeConnected) {
                            accounts.youtubeChannelName.ifBlank { "${accounts.youtubePlaylistCount} playlists" }
                        } else {
                            "Sign in to sync library & personalized Home"
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
                            "Connect for 2-way playlists & Auto-Connect"
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

                // Compact OpenSubsonic / Navidrome card
                Surface(
                    shape = RoundedCornerShape(20.dp),
                    color = cardBg,
                    border = BorderStroke(1.dp, dividerColor),
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable(onClick = onOpenAccounts),
                ) {
                    Row(
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 14.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(
                            modifier = Modifier
                                .size(38.dp)
                                .clip(CircleShape)
                                .background(palette.accent.copy(alpha = 0.16f)),
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                imageVector = Icons.Rounded.CloudQueue,
                                contentDescription = null,
                                tint = palette.accent,
                                modifier = Modifier.size(20.dp),
                            )
                        }
                        Spacer(Modifier.width(12.dp))
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                text = "OpenSubsonic / Navidrome",
                                style = MaterialTheme.typography.titleMedium,
                                color = palette.textPrimary,
                            )
                            Text(
                                text = if (accounts.subsonicConnected) {
                                    "${accounts.subsonicUrl} (${accounts.subsonicUsername})"
                                } else {
                                    "Stream lossless FLAC from your personal server"
                                },
                                style = MaterialTheme.typography.bodyMedium,
                                color = palette.textSecondary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                        Icon(
                            imageVector = Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                            contentDescription = null,
                            tint = palette.textSecondary,
                        )
                    }
                }
            }
        }

        // Group 4: Settings & App Info
        item(key = "you_settings_group") {
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
                    title = "Ferric Android v${BuildConfig.VERSION_NAME}",
                    subtitle = "Native Rust Oboe Dual-Deck · SQLite WAL · Zero Telemetry",
                    onClick = onOpenSettings,
                    showChevron = false,
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
    Surface(
        shape = RoundedCornerShape(22.dp),
        color = background,
        border = BorderStroke(1.dp, border),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(modifier = Modifier.fillMaxWidth()) {
            content()
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
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = icon,
            contentDescription = null,
            tint = palette.textPrimary.copy(alpha = 0.88f),
            modifier = Modifier.size(22.dp),
        )
        Spacer(Modifier.width(14.dp))
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
            Spacer(Modifier.height(1.dp))
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
    val gradient = Brush.verticalGradient(
        colors = if (palette.isLight) {
            listOf(
                tintColor.copy(alpha = 0.15f),
                palette.surface,
            )
        } else {
            listOf(
                tintColor.copy(alpha = 0.24f),
                tintColor.copy(alpha = 0.08f),
            )
        },
    )
    Surface(
        shape = RoundedCornerShape(22.dp),
        color = Color.Transparent,
        border = BorderStroke(1.dp, tintColor.copy(alpha = if (connected) 0.55f else 0.28f)),
        modifier = modifier
            .clip(RoundedCornerShape(22.dp))
            .background(gradient)
            .clickable(onClick = onCardClick),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Box(
                    modifier = Modifier
                        .size(36.dp)
                        .clip(CircleShape)
                        .background(tintColor.copy(alpha = 0.22f)),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = icon,
                        contentDescription = null,
                        tint = tintColor,
                        modifier = Modifier.size(20.dp),
                    )
                }
                if (connected) {
                    Box(
                        modifier = Modifier
                            .size(9.dp)
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
                style = MaterialTheme.typography.bodyMedium,
                color = palette.textSecondary,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.height(36.dp),
            )
            Button(
                onClick = onAction,
                shape = RoundedCornerShape(999.dp),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 6.dp),
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
