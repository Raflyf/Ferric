package dev.livelydesk.ferric.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.ui.theme.FerricOrange
import dev.livelydesk.ferric.ui.theme.HairlineBorder
import dev.livelydesk.ferric.ui.theme.ObsidianElevated
import dev.livelydesk.ferric.ui.theme.ObsidianSurface
import dev.livelydesk.ferric.ui.theme.TextPrimary
import dev.livelydesk.ferric.ui.theme.TextSecondary
import uniffi.ferric_mobile_ffi.FfiAccountStatus

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AccountsSheet(
    status: FfiAccountStatus,
    onDismiss: () -> Unit,
    onSignInSpotify: () -> Unit,
    onSyncSpotify: () -> Unit,
    onLogoutSpotify: () -> Unit,
    onSignInYtm: () -> Unit,
    onSyncYtm: () -> Unit,
    onLogoutYtm: () -> Unit,
    onSyncSubsonic: (url: String, user: String, pass: String) -> Unit,
    onLogoutSubsonic: () -> Unit,
) {
    var subUrl by remember(status.subsonicUrl) { mutableStateOf(status.subsonicUrl) }
    var subUser by remember(status.subsonicUsername) { mutableStateOf(status.subsonicUsername) }
    var subPass by remember { mutableStateOf("") }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        containerColor = ObsidianSurface,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Connected Accounts", style = MaterialTheme.typography.titleLarge, color = TextPrimary)
            Text(
                "Credentials stay hardware-encrypted in Android Keystore. Sign in with the same Spotify or YouTube Music account as Ferric Desktop for two-way sync and Connect.",
                style = MaterialTheme.typography.bodyMedium,
                color = TextSecondary,
            )

            // Spotify Card
            AccountCard(
                title = "Spotify (Free & Premium)",
                subtitle = if (status.spotifyConnected) {
                    "Signed in as ${status.spotifyUserName.ifBlank { status.spotifyUserId.ifBlank { "Spotify User" } }} · ${status.spotifyPlaylistCount} playlists, ${status.spotifyLikedCount} liked"
                } else {
                    "Two-way Liked Songs, playlists, and Spotify Recommended Stations"
                },
                connected = status.spotifyConnected,
                onSignIn = onSignInSpotify,
                onSync = onSyncSpotify,
                onLogout = onLogoutSpotify,
            )

            // YouTube Music Card
            AccountCard(
                title = "YouTube Music",
                subtitle = if (status.youtubeConnected) {
                    "Signed in as ${status.youtubeChannelName.ifBlank { status.youtubeAccountId.ifBlank { "Google Account" } }} · ${status.youtubePlaylistCount} playlists"
                } else {
                    "Two-way YouTube Music library, Likes, and personalized Home"
                },
                connected = status.youtubeConnected,
                onSignIn = onSignInYtm,
                onSync = onSyncYtm,
                onLogout = onLogoutYtm,
            )

            // OpenSubsonic / Navidrome Card
            Surface(
                shape = RoundedCornerShape(14.dp),
                color = ObsidianElevated,
                border = BorderStroke(1.dp, if (status.subsonicConnected) FerricOrange else HairlineBorder),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(
                    modifier = Modifier.padding(14.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text("OpenSubsonic / Navidrome", style = MaterialTheme.typography.titleMedium, color = TextPrimary)
                    Text(
                        if (status.subsonicConnected) {
                            "Connected to ${status.subsonicUrl} (${status.subsonicUsername})"
                        } else {
                            "Stream lossless FLAC from your personal Navidrome / Subsonic server"
                        },
                        style = MaterialTheme.typography.bodyMedium,
                        color = TextSecondary,
                    )
                    OutlinedTextField(
                        value = subUrl,
                        onValueChange = { subUrl = it },
                        label = { Text("Server URL (https://... or http://192.168.x.x:4533)") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    OutlinedTextField(
                        value = subUser,
                        onValueChange = { subUser = it },
                        label = { Text("Username") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    OutlinedTextField(
                        value = subPass,
                        onValueChange = { subPass = it },
                        label = { Text("Password") },
                        visualTransformation = PasswordVisualTransformation(),
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Button(
                            onClick = {
                                if (subUrl.isNotBlank() && subUser.isNotBlank() && subPass.isNotEmpty()) {
                                    onSyncSubsonic(subUrl.trim(), subUser.trim(), subPass)
                                }
                            },
                            colors = ButtonDefaults.buttonColors(
                                containerColor = FerricOrange,
                                contentColor = ObsidianSurface,
                            ),
                            modifier = Modifier.weight(1f),
                        ) {
                            Text(if (status.subsonicConnected) "Re-Sync" else "Connect & Sync")
                        }
                        if (status.subsonicConnected) {
                            OutlinedButton(
                                onClick = onLogoutSubsonic,
                                border = BorderStroke(1.dp, HairlineBorder),
                            ) {
                                Text("Disconnect", color = TextPrimary)
                            }
                        }
                    }
                }
            }

            Spacer(Modifier.height(18.dp))
        }
    }
}

@Composable
private fun AccountCard(
    title: String,
    subtitle: String,
    connected: Boolean,
    onSignIn: () -> Unit,
    onSync: () -> Unit,
    onLogout: () -> Unit,
) {
    Surface(
        shape = RoundedCornerShape(14.dp),
        color = ObsidianElevated,
        border = BorderStroke(1.dp, if (connected) FerricOrange else HairlineBorder),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier.padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(title, style = MaterialTheme.typography.titleMedium, color = TextPrimary)
            Text(subtitle, style = MaterialTheme.typography.bodyMedium, color = TextSecondary)
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (connected) {
                    Button(
                        onClick = onSync,
                        colors = ButtonDefaults.buttonColors(
                            containerColor = FerricOrange,
                            contentColor = ObsidianSurface,
                        ),
                        modifier = Modifier.weight(1f),
                    ) { Text("Sync Now") }
                    OutlinedButton(
                        onClick = onLogout,
                        border = BorderStroke(1.dp, HairlineBorder),
                    ) { Text("Sign Out", color = TextPrimary) }
                } else {
                    Button(
                        onClick = onSignIn,
                        colors = ButtonDefaults.buttonColors(
                            containerColor = FerricOrange,
                            contentColor = ObsidianSurface,
                        ),
                        modifier = Modifier.weight(1f),
                    ) { Text("Sign In") }
                }
            }
        }
    }
}
