package dev.livelydesk.ferric.widget

import android.appwidget.AppWidgetManager
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Color as AndroidColor
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import coil3.SingletonImageLoader
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import coil3.request.allowHardware
import coil3.toBitmap
import dev.livelydesk.ferric.FerricApp
import dev.livelydesk.ferric.R
import dev.livelydesk.ferric.core.activeDisplayIsPlaying
import dev.livelydesk.ferric.core.activeDisplayTrack
import dev.livelydesk.ferric.formatDurationMs
import dev.livelydesk.ferric.ui.theme.FerricSlimSlider
import dev.livelydesk.ferric.ui.theme.FerricTheme
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import kotlin.math.roundToInt

/**
 * Reconfigurable Home-Screen Widget settings for optical Liquid Glass customization:
 * supports Glass, Clear glass (pure wallpaper-through glass without cover background), Dark, Light,
 * Cover colour, accent swatches, shine/thickness/roundness/frost sliders, and visibility toggles.
 */
class FerricWidgetConfigActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(AndroidColor.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(AndroidColor.TRANSPARENT),
        )
        setResult(RESULT_CANCELED)

        val appWidgetId = intent?.extras?.getInt(
            AppWidgetManager.EXTRA_APPWIDGET_ID,
            AppWidgetManager.INVALID_APPWIDGET_ID,
        ) ?: AppWidgetManager.INVALID_APPWIDGET_ID

        val app = application as FerricApp
        val store = app.store

        setContent {
            val state by store.uiState.collectAsStateWithLifecycle()
            FerricTheme(
                themeConfig = state.themeConfig,
                coverShade = state.coverShade,
            ) {
                var config by remember(appWidgetId) {
                    mutableStateOf(FerricWidgetPrefs.load(this, appWidgetId))
                }
                WidgetSettingsScreen(
                    config = config,
                    state = state,
                    onConfigChange = { config = it },
                    onDone = {
                        FerricWidgetPrefs.save(this, appWidgetId, config)
                        FerricWidgetUpdater.updateAll(applicationContext, state)
                        if (appWidgetId != AppWidgetManager.INVALID_APPWIDGET_ID) {
                            setResult(
                                RESULT_OK,
                                Intent().putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, appWidgetId),
                            )
                        } else {
                            setResult(RESULT_OK)
                        }
                        finish()
                    },
                )
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun WidgetSettingsScreen(
    config: FerricWidgetConfig,
    state: dev.livelydesk.ferric.core.PlaybackUiState,
    onConfigChange: (FerricWidgetConfig) -> Unit,
    onDone: () -> Unit,
) {
    val context = LocalContext.current
    val palette = LocalFerricPalette.current
    var coverBmp by remember { mutableStateOf<Bitmap?>(null) }
    val activeTrack = state.activeDisplayTrack()
    val activePlaying = state.activeDisplayIsPlaying()
    val artUrl = activeTrack?.thumbnailUrl

    LaunchedEffect(artUrl) {
        if (artUrl.isNullOrBlank()) {
            coverBmp = null
            return@LaunchedEffect
        }
        coverBmp = runCatching {
            val loader = SingletonImageLoader.get(context.applicationContext)
            val req = ImageRequest.Builder(context.applicationContext)
                .data(artUrl)
                .size(256, 256)
                .allowHardware(false)
                .build()
            (loader.execute(req) as? SuccessResult)?.image?.toBitmap()
        }.getOrNull()
    }

    Scaffold(
        containerColor = Color(0xFF0B0C10),
        bottomBar = {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(Color(0xF00B0C10))
                    .navigationBarsPadding()
                    .padding(horizontal = 18.dp, vertical = 12.dp),
            ) {
                Button(
                    onClick = onDone,
                    shape = RoundedCornerShape(999.dp),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = Color.White,
                        contentColor = Color(0xFF101116),
                    ),
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(50.dp),
                ) {
                    Text(
                        text = "Done",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                }
            }
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .statusBarsPadding()
                .padding(innerPadding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 18.dp, vertical = 10.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text(
                text = "Widget settings",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.Medium,
                color = Color.White,
            )

            // Live preview frame simulating home wallpaper behind Clear glass / Glass
            val cachedStreak = remember { FerricWidgetPrefs.loadLastKnownStreak(context) }
            val resolvedStreak = (state.listeningStats?.currentStreakDays?.toInt() ?: 0).let {
                if (it > 0) it else if (cachedStreak > 0) cachedStreak else 1
            }
            LiveWidgetPreviewBox(
                config = config,
                title = activeTrack?.title?.ifBlank { null } ?: "Ready to play",
                artist = activeTrack?.artist?.ifBlank { null } ?: "Ferric",
                isPlaying = activePlaying,
                liked = activeTrack?.liked == true,
                streakDays = resolvedStreak,
                artBitmap = coverBmp,
                coverShade = state.coverShade,
            )

            Text(
                text = "The size comes from the widget you picked in the widget list (4x2, 4x1, or 2x2). Long-press the widget on your home screen anytime to resize or reopen Widget settings.",
                style = MaterialTheme.typography.bodySmall,
                color = Color(0xFF9EA2AF),
            )

            Text("Style", style = MaterialTheme.typography.labelLarge, color = Color.White)
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                FerricWidgetStyle.entries.forEach { st ->
                    ConfigPill(
                        label = st.label,
                        selected = config.style == st,
                        onClick = { onConfigChange(config.copy(style = st)) },
                    )
                }
            }

            Text("Accent colour", style = MaterialTheme.typography.labelLarge, color = Color.White)
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
                itemVerticalAlignment = Alignment.CenterVertically,
            ) {
                ConfigPill(
                    label = "From the cover",
                    selected = config.accentSwatchIndex == -1,
                    onClick = { onConfigChange(config.copy(accentSwatchIndex = -1)) },
                )
                FerricWidgetConfig.ACCENT_SWATCHES.forEachIndexed { idx, argb ->
                    val selected = config.accentSwatchIndex == idx
                    Box(
                        modifier = Modifier
                            .size(32.dp)
                            .clip(CircleShape)
                            .background(Color(argb))
                            .then(
                                if (selected) {
                                    Modifier.border(2.5.dp, Color.White, CircleShape)
                                } else {
                                    Modifier
                                },
                            )
                            .clickable { onConfigChange(config.copy(accentSwatchIndex = idx)) },
                    )
                }
            }

            SliderSettingRow(
                label = "Glass shine: ${(config.glassShine * 100).roundToInt()}%",
                value = config.glassShine,
                onValueChange = { onConfigChange(config.copy(glassShine = it)) },
            )
            SliderSettingRow(
                label = "Glass thickness (lens + rim): ${(config.glassThickness * 100).roundToInt()}%",
                value = config.glassThickness,
                onValueChange = { onConfigChange(config.copy(glassThickness = it)) },
            )
            SliderSettingRow(
                label = "Corner roundness: ${(config.cornerRoundness * 100).roundToInt()}%",
                value = config.cornerRoundness,
                onValueChange = { onConfigChange(config.copy(cornerRoundness = it)) },
            )
            SliderSettingRow(
                label = "Frost (cover blur): ${(config.frost * 100).roundToInt()}%",
                value = config.frost,
                onValueChange = { onConfigChange(config.copy(frost = it)) },
            )

            Text("Cover shape", style = MaterialTheme.typography.labelLarge, color = Color.White)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                WidgetCoverShape.entries.forEach { shape ->
                    ConfigPill(
                        label = shape.label,
                        selected = config.coverShape == shape,
                        onClick = { onConfigChange(config.copy(coverShape = shape)) },
                    )
                }
            }

            Text("Text size", style = MaterialTheme.typography.labelLarge, color = Color.White)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                WidgetTextSize.entries.forEach { sz ->
                    ConfigPill(
                        label = sz.label,
                        selected = config.textSize == sz,
                        onClick = { onConfigChange(config.copy(textSize = sz)) },
                    )
                }
            }

            Text("Play button", style = MaterialTheme.typography.labelLarge, color = Color.White)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                WidgetPlayButtonStyle.entries.forEach { pb ->
                    ConfigPill(
                        label = pb.label,
                        selected = config.playButtonStyle == pb,
                        onClick = { onConfigChange(config.copy(playButtonStyle = pb)) },
                    )
                }
            }

            ToggleRow("Show cover", config.showCover) {
                onConfigChange(config.copy(showCover = it))
            }
            ToggleRow("Show progress bar", config.showProgressBar) {
                onConfigChange(config.copy(showProgressBar = it))
            }
            ToggleRow("Show time", config.showTime) {
                onConfigChange(config.copy(showTime = it))
            }
            ToggleRow("Show like button", config.showLikeButton) {
                onConfigChange(config.copy(showLikeButton = it))
            }
            ToggleRow("Show previous and next", config.showPrevNext) {
                onConfigChange(config.copy(showPrevNext = it))
            }
            ToggleRow("Show streak", config.showStreak) {
                onConfigChange(config.copy(showStreak = it))
            }
        }
    }
}

@Composable
private fun LiveWidgetPreviewBox(
    config: FerricWidgetConfig,
    title: String,
    artist: String,
    isPlaying: Boolean,
    liked: Boolean,
    streakDays: Int,
    artBitmap: Bitmap?,
    coverShade: dev.livelydesk.ferric.ui.theme.CoverShadeColors,
) {
    val context = LocalContext.current
    val wpTint = remember { FerricWidgetRenderer.sampleWallpaperTint(context) }
    val bgBitmap = remember(
        config.style,
        config.accentSwatchIndex,
        config.glassShine,
        config.glassThickness,
        config.cornerRoundness,
        config.frost,
        artBitmap,
        coverShade,
        wpTint,
    ) {
        FerricWidgetRenderer.renderGlassBackgroundBitmap(528, 256, config, artBitmap, coverShade, pxPerDp = 1.6f, wallpaperTintArgb = wpTint)
    }
    val shapedCover = remember(artBitmap, config.coverShape, config.showCover) {
        if (config.showCover && artBitmap != null) {
            FerricWidgetRenderer.shapeCoverBitmap(artBitmap, config.coverShape)
        } else {
            null
        }
    }
    val accentColor = remember(config.accentSwatchIndex, coverShade) {
        Color(FerricWidgetRenderer.resolveAccentArgb(config, coverShade))
    }
    val isLight = config.style == FerricWidgetStyle.LIGHT
    val fgPrimary = if (isLight) Color(0xFF14161C) else Color.White
    val fgSecondary = if (isLight) Color(0xCC242832) else Color(0xD8FFFFFF)
    val playKind = if (isPlaying) "pause" else "play"
    val playIconBmp = remember(playKind, config.playButtonStyle, accentColor, isLight) {
        val fill = if (config.playButtonStyle == WidgetPlayButtonStyle.ROUND) {
            AndroidColor.rgb(
                (accentColor.red * 255).roundToInt(),
                (accentColor.green * 255).roundToInt(),
                (accentColor.blue * 255).roundToInt(),
            )
        } else {
            null
        }
        val fg = if (fill != null) {
            if (isLight) AndroidColor.WHITE else AndroidColor.rgb(16, 18, 24)
        } else {
            if (isLight) AndroidColor.rgb(20, 22, 28) else AndroidColor.WHITE
        }
        FerricWidgetRenderer.renderTransportIcon(playKind, fg, fill)
    }

    // Simulated rich wallpaper behind the preview card so Clear glass transparency is immediately visible
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(22.dp))
            .background(
                Brush.linearGradient(
                    listOf(
                        Color(0xFFB39B62),
                        Color(0xFF8C734B),
                        Color(0xFF3E5A68),
                    ),
                ),
            )
            .padding(14.dp),
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(178.dp),
        ) {
            Image(
                bitmap = bgBitmap.asImageBitmap(),
                contentDescription = null,
                contentScale = ContentScale.FillBounds,
                modifier = Modifier.fillMaxSize(),
            )

            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(horizontal = 16.dp, vertical = 12.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (config.showCover) {
                        if (shapedCover != null) {
                            Image(
                                bitmap = shapedCover.asImageBitmap(),
                                contentDescription = null,
                                modifier = Modifier.size(64.dp),
                            )
                        } else {
                            Image(
                                painter = painterResource(id = R.drawable.ic_launcher_foreground),
                                contentDescription = null,
                                modifier = Modifier
                                    .size(64.dp)
                                    .clip(RoundedCornerShape(14.dp))
                                    .background(Color(0xFF13151B)),
                            )
                        }
                        Spacer(Modifier.width(14.dp))
                    }
                    Column(
                        modifier = Modifier.weight(1f),
                        verticalArrangement = Arrangement.Center,
                    ) {
                        Text(
                            text = title,
                            fontSize = config.textSize.titleSp.sp,
                            fontWeight = FontWeight.Bold,
                            color = fgPrimary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Text(
                            text = artist,
                            fontSize = config.textSize.artistSp.sp,
                            color = fgSecondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    if (config.showStreak && streakDays > 0) {
                        val streakBmp = remember(streakDays, isLight) {
                            FerricWidgetRenderer.renderStreakBadgeBitmap(streakDays, isLight)
                        }
                        Spacer(Modifier.width(8.dp))
                        Image(
                            bitmap = streakBmp.asImageBitmap(),
                            contentDescription = null,
                            modifier = Modifier
                                .height(24.dp)
                                .align(Alignment.Top),
                        )
                    }
                }

                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f),
                ) {
                    Row(
                        modifier = Modifier.align(Alignment.Center),
                        horizontalArrangement = Arrangement.spacedBy(22.dp, Alignment.CenterHorizontally),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        if (config.showPrevNext) {
                            val prevBmp = remember(isLight) {
                                FerricWidgetRenderer.renderTransportIcon(
                                    "prev",
                                    if (isLight) AndroidColor.rgb(20, 22, 28) else AndroidColor.WHITE,
                                )
                            }
                            Image(bitmap = prevBmp.asImageBitmap(), contentDescription = null, modifier = Modifier.size(38.dp))
                        }
                        Image(
                            bitmap = playIconBmp.asImageBitmap(),
                            contentDescription = null,
                            modifier = Modifier.size(46.dp),
                        )
                        if (config.showPrevNext) {
                            val nextBmp = remember(isLight) {
                                FerricWidgetRenderer.renderTransportIcon(
                                    "next",
                                    if (isLight) AndroidColor.rgb(20, 22, 28) else AndroidColor.WHITE,
                                )
                            }
                            Image(bitmap = nextBmp.asImageBitmap(), contentDescription = null, modifier = Modifier.size(38.dp))
                        }
                    }
                    if (config.showLikeButton) {
                        val likeIcon = remember(liked, accentColor, isLight) {
                            val col = if (liked) {
                                AndroidColor.rgb(
                                    (accentColor.red * 255).roundToInt(),
                                    (accentColor.green * 255).roundToInt(),
                                    (accentColor.blue * 255).roundToInt(),
                                )
                            } else if (isLight) {
                                AndroidColor.rgb(20, 22, 28)
                            } else {
                                AndroidColor.WHITE
                            }
                            FerricWidgetRenderer.renderTransportIcon(if (liked) "heart_on" else "heart_off", col)
                        }
                        Image(
                            bitmap = likeIcon.asImageBitmap(),
                            contentDescription = null,
                            modifier = Modifier
                                .align(Alignment.CenterEnd)
                                .size(34.dp),
                        )
                    }
                }

                if (config.showProgressBar || config.showTime) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        if (config.showTime) {
                            Text(
                                text = formatDurationMs(158_000L),
                                fontSize = 12.sp,
                                color = fgSecondary,
                            )
                            Spacer(Modifier.width(10.dp))
                        }
                        if (config.showProgressBar) {
                            Box(
                                modifier = Modifier
                                    .weight(1f)
                                    .height(5.5.dp)
                                    .clip(RoundedCornerShape(999.dp))
                                    .background(if (isLight) Color.Black.copy(alpha = 0.16f) else Color.White.copy(alpha = 0.30f)),
                            ) {
                                Box(
                                    modifier = Modifier
                                        .fillMaxWidth(0.65f)
                                        .height(5.5.dp)
                                        .clip(RoundedCornerShape(999.dp))
                                        .background(accentColor),
                                )
                            }
                        } else {
                            Spacer(Modifier.weight(1f))
                        }
                        if (config.showTime) {
                            Spacer(Modifier.width(10.dp))
                            Text(
                                text = formatDurationMs(238_000L),
                                fontSize = 12.sp,
                                color = fgSecondary,
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ConfigPill(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
) {
    Surface(
        shape = RoundedCornerShape(10.dp),
        color = if (selected) Color(0xFF2B2E36) else Color.Transparent,
        border = BorderStroke(
            width = 1.dp,
            color = if (selected) Color.White.copy(alpha = 0.24f) else Color(0xFF2B2E36),
        ),
        modifier = Modifier
            .clip(RoundedCornerShape(10.dp))
            .clickable(onClick = onClick),
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelLarge,
            color = if (selected) Color.White else Color(0xFFD0D4DE),
            modifier = Modifier.padding(horizontal = 14.dp, vertical = 8.dp),
        )
    }
}

@Composable
private fun SliderSettingRow(
    label: String,
    value: Float,
    onValueChange: (Float) -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth()) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodyMedium,
            color = Color.White,
        )
        Spacer(Modifier.height(4.dp))
        FerricSlimSlider(
            value = value,
            onValueChange = onValueChange,
            valueRange = 0f..1f,
        )
    }
}

@Composable
private fun ToggleRow(
    label: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable { onCheckedChange(!checked) }
            .padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodyLarge,
            color = Color.White,
        )
        Switch(
            checked = checked,
            onCheckedChange = onCheckedChange,
            colors = SwitchDefaults.colors(
                checkedThumbColor = Color(0xFF101116),
                checkedTrackColor = Color.White,
                uncheckedThumbColor = Color(0xFF7B808C),
                uncheckedTrackColor = Color(0xFF20222A),
                uncheckedBorderColor = Color(0xFF424552),
            ),
        )
    }
}
