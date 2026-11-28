package dev.livelydesk.ferric.ui.screens

import android.content.Intent
import android.net.Uri
import android.os.Build
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.BugReport
import androidx.compose.material.icons.rounded.CloudDownload
import androidx.compose.material.icons.rounded.Code
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.Language
import androidx.compose.material.icons.rounded.NewReleases
import androidx.compose.material.icons.rounded.VolunteerActivism
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.livelydesk.ferric.BuildConfig
import dev.livelydesk.ferric.R
import dev.livelydesk.ferric.core.PlaybackUiState
import dev.livelydesk.ferric.core.UpdateCheckOutcome
import dev.livelydesk.ferric.ui.theme.FerricModalBottomSheet
import dev.livelydesk.ferric.ui.theme.GlassTier
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricGlassSurface

private const val FERRIC_REPO_URL = "https://github.com/Raflyf/Ferric"
private const val FERRIC_RELEASES_URL = "https://github.com/Raflyf/Ferric/releases"
private const val FERRIC_ISSUES_URL = "https://github.com/Raflyf/Ferric/issues"
private const val FERRIC_WEB_URL = "https://livelydesk.vercel.app/ferric"
private const val SUPPORT_COFFEE_URL = "https://yapp.ink/raflyf"
private const val SUPPORT_SAWERIA_URL = "https://saweria.co/BuyMatcha"

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AboutSheet(
    state: PlaybackUiState,
    onCheckForUpdates: ((UpdateCheckOutcome) -> Unit) -> Unit,
    onMessage: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    val palette = LocalFerricPalette.current
    val dividerColor = palette.border.copy(alpha = if (palette.isLiquidGlass) 0.40f else 0.70f)
    var updateOutcome by remember { mutableStateOf<UpdateCheckOutcome?>(null) }
    var checkingUpdate by remember { mutableStateOf(false) }

    fun openUrl(url: String) {
        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        runCatching { context.startActivity(intent) }
            .onFailure { onMessage("Could not open browser link") }
    }

    val primaryAbi = remember { Build.SUPPORTED_ABIS.firstOrNull() ?: "universal" }

    FerricModalBottomSheet(
        onDismissRequest = onDismiss,
        coverShade = state.coverShade,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 18.dp, vertical = 6.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            // Header navigation bar
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth(),
            ) {
                IconButton(onClick = onDismiss) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Rounded.ArrowBack,
                        contentDescription = "Back",
                        tint = palette.textPrimary,
                    )
                }
                Spacer(Modifier.width(4.dp))
                Text(
                    text = "About",
                    style = MaterialTheme.typography.titleLarge,
                    color = palette.textPrimary,
                )
            }

            // App Identity Card
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .ferricGlassSurface(
                        palette = palette,
                        cornerRadius = palette.cornerRadiusDp,
                        tier = GlassTier.FROSTED_PANEL,
                        fallbackColor = palette.elevated,
                    ),
            ) {
                Row(
                    modifier = Modifier.padding(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Image(
                        painter = painterResource(id = R.drawable.ic_launcher_foreground),
                        contentDescription = "Ferric",
                        contentScale = ContentScale.Fit,
                        modifier = Modifier
                            .size(56.dp)
                            .clip(RoundedCornerShape(16.dp))
                            .background(Color(0xFF0E0E13))
                            .border(1.dp, palette.border.copy(alpha = 0.7f), RoundedCornerShape(16.dp)),
                    )
                    Spacer(Modifier.width(14.dp))
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            text = "Ferric",
                            style = MaterialTheme.typography.headlineSmall,
                            fontWeight = FontWeight.SemiBold,
                            color = palette.textPrimary,
                        )
                        Spacer(Modifier.height(2.dp))
                        Text(
                            text = "v${BuildConfig.VERSION_NAME} · $primaryAbi · Native Rust Oboe",
                            style = MaterialTheme.typography.bodyMedium,
                            color = palette.textSecondary,
                        )
                    }
                }
            }

            // Check for Updates Card
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .ferricGlassSurface(
                        palette = palette,
                        cornerRadius = palette.cornerRadiusDp,
                        tier = GlassTier.FROSTED_PANEL,
                        fallbackColor = palette.elevated,
                    )
                    .clickable(enabled = !checkingUpdate) {
                        val availableUrl = updateOutcome?.releaseUrl
                        if (availableUrl != null && updateOutcome?.hasUpdate == true) {
                            openUrl(availableUrl)
                        } else {
                            checkingUpdate = true
                            onCheckForUpdates { res ->
                                checkingUpdate = false
                                updateOutcome = res
                            }
                        }
                    },
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp, vertical = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        imageVector = Icons.Rounded.CloudDownload,
                        contentDescription = null,
                        tint = palette.accent,
                        modifier = Modifier.size(22.dp),
                    )
                    Spacer(Modifier.width(12.dp))
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            text = when {
                                checkingUpdate -> "Checking for updates..."
                                updateOutcome?.hasUpdate == true -> "Update available: ${updateOutcome?.tag}"
                                updateOutcome?.upToDate == true -> "Up to date (v${BuildConfig.VERSION_NAME})"
                                updateOutcome?.error != null -> "Update check failed · Tap to retry"
                                else -> "Check for updates"
                            },
                            style = MaterialTheme.typography.titleMedium,
                            color = palette.textPrimary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        val detail = updateOutcome?.error ?: updateOutcome?.summary
                        if (!detail.isNullOrBlank()) {
                            Spacer(Modifier.height(2.dp))
                            Text(
                                text = detail,
                                style = MaterialTheme.typography.bodySmall,
                                color = palette.textSecondary,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                    if (checkingUpdate) {
                        CircularProgressIndicator(
                            color = palette.accent,
                            strokeWidth = 2.dp,
                            modifier = Modifier.size(18.dp),
                        )
                    } else {
                        Icon(
                            imageVector = Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                            contentDescription = null,
                            tint = palette.textSecondary,
                        )
                    }
                }
            }

            // Creator Hero Banner Card
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(210.dp)
                    .ferricGlassSurface(
                        palette = palette,
                        cornerRadius = 22.dp,
                        tier = GlassTier.FROSTED_PANEL,
                        fallbackColor = palette.elevated,
                    )
                    .clickable { openUrl(FERRIC_WEB_URL) },
            ) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(
                            Brush.radialGradient(
                                colors = listOf(
                                    palette.accent.copy(alpha = 0.28f),
                                    state.coverShade.orb1.copy(alpha = 0.36f),
                                    Color.Black.copy(alpha = if (palette.isLight) 0.14f else 0.72f),
                                ),
                            ),
                        ),
                )
                Image(
                    painter = painterResource(id = R.drawable.ic_launcher_foreground),
                    contentDescription = null,
                    contentScale = ContentScale.Fit,
                    modifier = Modifier
                        .align(Alignment.Center)
                        .size(172.dp)
                        .alpha(0.32f),
                )
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(
                            Brush.verticalGradient(
                                0.0f to Color.Transparent,
                                0.52f to Color.Transparent,
                                1.0f to Color.Black.copy(alpha = 0.72f),
                            ),
                        ),
                )
                Column(
                    modifier = Modifier
                        .align(Alignment.BottomStart)
                        .padding(18.dp),
                ) {
                    Text(
                        text = "RAFLYF",
                        style = MaterialTheme.typography.headlineSmall,
                        fontWeight = FontWeight.Medium,
                        color = Color.White,
                    )
                    Text(
                        text = "Developer · LivelyDesk",
                        style = MaterialTheme.typography.bodyMedium,
                        color = Color.White.copy(alpha = 0.76f),
                    )
                }
                Box(
                    modifier = Modifier
                        .align(Alignment.BottomEnd)
                        .padding(16.dp)
                        .size(58.dp)
                        .clip(CircleShape)
                        .background(Color(0xFF121318))
                        .border(1.5.dp, Color.White.copy(alpha = 0.65f), CircleShape),
                    contentAlignment = Alignment.Center,
                ) {
                    Image(
                        painter = painterResource(id = R.drawable.ic_launcher_foreground),
                        contentDescription = "Raflyf badge",
                        modifier = Modifier.size(50.dp),
                    )
                }
            }

            // Links & Project Group
            Text(
                text = "Project & Links",
                style = MaterialTheme.typography.labelLarge,
                color = palette.textSecondary,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .ferricGlassSurface(
                        palette = palette,
                        cornerRadius = palette.cornerRadiusDp,
                        tier = GlassTier.FROSTED_PANEL,
                        fallbackColor = palette.elevated,
                    ),
            ) {
                Column(modifier = Modifier.fillMaxWidth()) {
                    AboutLinkRow(
                        icon = Icons.Rounded.Code,
                        title = "GitHub Repository",
                        trailing = "Raflyf/Ferric",
                        onClick = { openUrl(FERRIC_REPO_URL) },
                    )
                    HorizontalDivider(color = dividerColor, modifier = Modifier.padding(horizontal = 16.dp))
                    AboutLinkRow(
                        icon = Icons.Rounded.Language,
                        title = "LivelyDesk Web",
                        trailing = "livelydesk.vercel.app/ferric",
                        onClick = { openUrl(FERRIC_WEB_URL) },
                    )
                    HorizontalDivider(color = dividerColor, modifier = Modifier.padding(horizontal = 16.dp))
                    AboutLinkRow(
                        icon = Icons.Rounded.NewReleases,
                        title = "Releases & Changelog",
                        trailing = "v${BuildConfig.VERSION_NAME}",
                        onClick = { openUrl(FERRIC_RELEASES_URL) },
                    )
                    HorizontalDivider(color = dividerColor, modifier = Modifier.padding(horizontal = 16.dp))
                    AboutLinkRow(
                        icon = Icons.Rounded.BugReport,
                        title = "Report an Issue",
                        trailing = "Issues",
                        onClick = { openUrl(FERRIC_ISSUES_URL) },
                    )
                }
            }

            // Support CTAs (Primary rounded pill CTA + Saweria)
            Button(
                onClick = { openUrl(SUPPORT_COFFEE_URL) },
                shape = RoundedCornerShape(999.dp),
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 14.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = if (palette.isLight) palette.textPrimary else Color.White,
                    contentColor = if (palette.isLight) Color.White else Color(0xFF101116),
                ),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Icon(
                    imageVector = Icons.Rounded.Favorite,
                    contentDescription = null,
                    modifier = Modifier.size(20.dp),
                )
                Spacer(Modifier.width(12.dp))
                Column(horizontalAlignment = Alignment.Start) {
                    Text(
                        text = "Buy me a coffee",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        text = "Support development via yapp.ink/raflyf",
                        style = MaterialTheme.typography.labelMedium,
                        modifier = Modifier.alpha(0.75f),
                    )
                }
            }

            Surface(
                shape = RoundedCornerShape(999.dp),
                color = palette.elevated.copy(alpha = if (palette.isLiquidGlass) 0.56f else 1f),
                border = BorderStroke(1.dp, palette.border),
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(999.dp))
                    .clickable { openUrl(SUPPORT_SAWERIA_URL) },
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = 20.dp, vertical = 13.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.Center,
                ) {
                    Icon(
                        imageVector = Icons.Rounded.VolunteerActivism,
                        contentDescription = null,
                        tint = palette.accent,
                        modifier = Modifier.size(19.dp),
                    )
                    Spacer(Modifier.width(10.dp))
                    Text(
                        text = "Saweria · saweria.co/BuyMatcha",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.SemiBold,
                        color = palette.textPrimary,
                    )
                }
            }

            Text(
                text = "Zero Telemetry · SQLite WAL · MIT License",
                style = MaterialTheme.typography.labelSmall,
                color = palette.textSecondary.copy(alpha = 0.72f),
                modifier = Modifier
                    .align(Alignment.CenterHorizontally)
                    .padding(top = 2.dp, bottom = 18.dp),
            )
        }
    }
}

@Composable
private fun AboutLinkRow(
    icon: ImageVector,
    title: String,
    trailing: String,
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
            modifier = Modifier.size(20.dp),
        )
        Spacer(Modifier.width(14.dp))
        Text(
            text = title,
            style = MaterialTheme.typography.titleMedium,
            color = palette.textPrimary,
            modifier = Modifier.weight(1f),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Spacer(Modifier.width(8.dp))
        Text(
            text = trailing,
            style = MaterialTheme.typography.bodyMedium,
            color = palette.textSecondary,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Spacer(Modifier.width(4.dp))
        Icon(
            imageVector = Icons.AutoMirrored.Rounded.KeyboardArrowRight,
            contentDescription = null,
            tint = palette.textSecondary.copy(alpha = 0.7f),
            modifier = Modifier.size(18.dp),
        )
    }
}
