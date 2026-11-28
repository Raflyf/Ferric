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
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
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
import dev.livelydesk.ferric.core.PlaybackProgress
import dev.livelydesk.ferric.formatDurationMs
import dev.livelydesk.ferric.ui.theme.FerricOrange
import dev.livelydesk.ferric.ui.theme.FerricSlimSlider
import dev.livelydesk.ferric.ui.theme.FerricTheme
import dev.livelydesk.ferric.ui.theme.FerricVisualPalette
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import kotlin.math.roundToInt

private enum class WidgetVariant { LIQUID_GLASS, COMPACT_STRIP, SQUARE_COVER }

private val FerricWidgetConfigSaver = listSaver<FerricWidgetConfig, Any>(
    save = { cfg ->
        listOf(
            cfg.style.id,
            cfg.accentSwatchIndex,
            cfg.glassShine,
            cfg.glassThickness,
            cfg.cornerRoundness,
            cfg.frost,
            cfg.coverShape.id,
            cfg.textSize.id,
            cfg.playButtonStyle.id,
            cfg.showCover,
            cfg.showProgressBar,
            cfg.showTime,
            cfg.showLikeButton,
            cfg.showPrevNext,
            cfg.showStreak,
        )
    },
    restore = { list ->
        FerricWidgetConfig(
            style = FerricWidgetStyle.fromId(list[0] as String),
            accentSwatchIndex = (list[1] as Int).coerceIn(-1, FerricWidgetConfig.ACCENT_SWATCHES.lastIndex),
            glassShine = (list[2] as Float).coerceIn(0f, 1f),
            glassThickness = (list[3] as Float).coerceIn(0f, 1f),
            cornerRoundness = (list[4] as Float).coerceIn(0f, 1f),
            frost = (list[5] as Float).coerceIn(0f, 1f),
            coverShape = WidgetCoverShape.fromId(list[6] as String),
            textSize = WidgetTextSize.fromId(list[7] as String),
            playButtonStyle = WidgetPlayButtonStyle.fromId(list[8] as String),
            showCover = list[9] as Boolean,
            showProgressBar = list[10] as Boolean,
            showTime = list[11] as Boolean,
            showLikeButton = list[12] as Boolean,
            showPrevNext = list[13] as Boolean,
            showStreak = list[14] as Boolean,
        )
    },
)

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

        val providerClassName = if (appWidgetId != AppWidgetManager.INVALID_APPWIDGET_ID) {
            AppWidgetManager.getInstance(this).getAppWidgetInfo(appWidgetId)?.provider?.className.orEmpty()
        } else {
            ""
        }
        val widgetVariant = when {
            providerClassName.endsWith("CompactStripWidgetProvider") -> WidgetVariant.COMPACT_STRIP
            providerClassName.endsWith("SquareCoverWidgetProvider") -> WidgetVariant.SQUARE_COVER
            else -> WidgetVariant.LIQUID_GLASS
        }

        val app = application as FerricApp
        val store = app.store

        setContent {
            val state by store.uiState.collectAsStateWithLifecycle()
            FerricTheme(
                themeConfig = state.themeConfig,
                coverShade = state.coverShade,
            ) {
                var config by rememberSaveable(appWidgetId, stateSaver = FerricWidgetConfigSaver) {
                    mutableStateOf(FerricWidgetPrefs.load(this, appWidgetId))
                }
                val darkConfigPalette = remember {
                    FerricVisualPalette(
                        canvas = Color(0xFF0B0C10),
                        surface = Color(0xFF14161D),
                        elevated = Color(0xFF1E212B),
                        border = Color.White.copy(alpha = 0.24f),
                        textPrimary = Color.White,
                        textSecondary = Color(0xFF9EA2AF),
                        accent = FerricOrange,
                        isLight = false,
                        isLiquidGlass = true,
                    )
                }
                CompositionLocalProvider(LocalFerricPalette provides darkConfigPalette) {
                    WidgetSettingsScreen(
                        config = config,
                        variant = widgetVariant,
                        state = state,
                        onConfigChange = { config = it },
                        onReset = { config = FerricWidgetConfig() },
                        onCancel = { finish() },
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
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun WidgetSettingsScreen(
    config: FerricWidgetConfig,
    variant: WidgetVariant,
    state: dev.livelydesk.ferric.core.PlaybackUiState,
    onConfigChange: (FerricWidgetConfig) -> Unit,
    onReset: () -> Unit,
    onCancel: () -> Unit,
    onDone: () -> Unit,
) {
    val context = LocalContext.current
    var coverBmp by remember { mutableStateOf<Bitmap?>(null) }
    val snap = remember(state) {
        FerricWidgetUpdater.resolveDisplaySnapshot(context, state, PlaybackProgress())
    }
    val artUrl = snap.thumbnailUrl

    LaunchedEffect(artUrl) {
        val diskBmp = FerricWidgetPrefs.loadLastCoverBitmap(context, artUrl)
        if (diskBmp != null) {
            coverBmp = diskBmp
        }
        if (artUrl.isNullOrBlank()) {
            if (diskBmp == null) coverBmp = null
            return@LaunchedEffect
        }
        val fetched = runCatching {
            val loader = SingletonImageLoader.get(context.applicationContext)
            val req = ImageRequest.Builder(context.applicationContext)
                .data(artUrl)
                .size(256, 256)
                .allowHardware(false)
                .build()
            (loader.execute(req) as? SuccessResult)?.image?.toBitmap()
        }.getOrNull()
        if (fetched != null) {
            coverBmp = fetched
            FerricWidgetPrefs.saveLastCoverBitmap(context, artUrl, fetched)
        } else if (diskBmp == null) {
            coverBmp = null
        }
    }

    Scaffold(
        containerColor = Color(0xFF0B0C10),
        bottomBar = {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(Color(0xF00B0C10))
                    .navigationBarsPadding()
                    .padding(horizontal = 18.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OutlinedButton(
                    onClick = onCancel,
                    shape = RoundedCornerShape(999.dp),
                    border = BorderStroke(1.dp, Color(0xFF323642)),
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = Color(0xFFD0D4DE)),
                    modifier = Modifier
                        .weight(1f)
                        .height(50.dp),
                ) {
                    Text(
                        text = "Cancel",
                        style = MaterialTheme.typography.titleMedium,
                    )
                }
                Button(
                    onClick = onDone,
                    shape = RoundedCornerShape(999.dp),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = Color.White,
                        contentColor = Color(0xFF101116),
                    ),
                    modifier = Modifier
                        .weight(1.4f)
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
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "Widget settings",
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.Medium,
                    color = Color.White,
                )
                TextButton(onClick = onReset) {
                    Text(
                        text = "Reset",
                        style = MaterialTheme.typography.labelLarge,
                        color = Color(0xFF9EA2AF),
                    )
                }
            }

            // Live preview frame simulating home wallpaper behind Clear glass / Glass
            val cachedStreak = remember { FerricWidgetPrefs.loadLastKnownStreak(context) }
            val resolvedStreak = (state.listeningStats?.currentStreakDays?.toInt() ?: 0).let {
                if (it > 0) it else if (cachedStreak > 0) cachedStreak else 1
            }
            LiveWidgetPreviewBox(
                config = config,
                title = snap.title,
                artist = snap.artist,
                durationMs = snap.durationMs.takeIf { it > 0L } ?: 238_000L,
                isPlaying = snap.isPlaying,
                liked = snap.liked,
                streakDays = resolvedStreak,
                artBitmap = coverBmp,
                cachedAccentArgb = snap.cachedAccentArgb,
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

            Text("Size (text & controls)", style = MaterialTheme.typography.labelLarge, color = Color.White)
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
            if (variant == WidgetVariant.LIQUID_GLASS) {
                ToggleRow("Show progress bar", config.showProgressBar) {
                    onConfigChange(config.copy(showProgressBar = it))
                }
                ToggleRow("Show time", config.showTime) {
                    onConfigChange(config.copy(showTime = it))
                }
            }
            if (variant != WidgetVariant.COMPACT_STRIP) {
                ToggleRow("Show like button", config.showLikeButton) {
                    onConfigChange(config.copy(showLikeButton = it))
                }
            }
            ToggleRow(
                label = if (variant == WidgetVariant.SQUARE_COVER) "Show next button" else "Show previous and next",
                checked = config.showPrevNext,
            ) {
                onConfigChange(config.copy(showPrevNext = it))
            }
            if (variant == WidgetVariant.LIQUID_GLASS) {
                ToggleRow("Show streak", config.showStreak) {
                    onConfigChange(config.copy(showStreak = it))
                }
            }
        }
    }
}

@Composable
private fun LiveWidgetPreviewBox(
    config: FerricWidgetConfig,
    title: String,
    artist: String,
    durationMs: Long,
    isPlaying: Boolean,
    liked: Boolean,
    streakDays: Int,
    artBitmap: Bitmap?,
    cachedAccentArgb: Int?,
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
    val isLight = config.style == FerricWidgetStyle.LIGHT
    val accentColor = remember(config.accentSwatchIndex, coverShade, artBitmap, cachedAccentArgb, isLight) {
        Color(
            FerricWidgetRenderer.resolveAccentArgb(
                config,
                coverShade,
                artBitmap = artBitmap,
                cachedAccentArgb = cachedAccentArgb,
                isLight = isLight,
            ),
        )
    }
    val shapedCover = remember(artBitmap, config.coverShape, config.showCover, accentColor, isLight) {
        if (config.showCover && artBitmap != null) {
            FerricWidgetRenderer.shapeCoverBitmap(artBitmap, config.coverShape)
        } else if (config.showCover) {
            FerricWidgetRenderer.renderFerricFallbackCover(context, config.coverShape, accentColor.toArgb(), isLight)
        } else {
            null
        }
    }
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
                .height(198.dp),
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
                    .padding(horizontal = 18.dp, vertical = 14.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (config.showCover && shapedCover != null) {
                        Image(
                            bitmap = shapedCover.asImageBitmap(),
                            contentDescription = null,
                            modifier = Modifier.size(config.textSize.coverDp.dp),
                        )
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
                                .height(26.dp)
                                .align(Alignment.Top),
                        )
                    }
                }

                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    val useSideSpacers = !(config.showLikeButton && config.showPrevNext)
                    if (useSideSpacers) {
                        Spacer(Modifier.weight(0.4f))
                    }
                    if (config.showPrevNext) {
                        val prevBmp = remember(isLight) {
                            FerricWidgetRenderer.renderTransportIcon(
                                "prev",
                                if (isLight) AndroidColor.rgb(20, 22, 28) else AndroidColor.WHITE,
                            )
                        }
                        Image(
                            bitmap = prevBmp.asImageBitmap(),
                            contentDescription = null,
                            modifier = Modifier
                                .size(config.textSize.prevNextDp.dp)
                                .padding(4.dp),
                        )
                        Spacer(Modifier.weight(1f))
                    }
                    Image(
                        bitmap = playIconBmp.asImageBitmap(),
                        contentDescription = null,
                        modifier = Modifier
                            .size(config.textSize.playDp.dp)
                            .padding(2.dp),
                    )
                    if (config.showPrevNext) {
                        Spacer(Modifier.weight(1f))
                        val nextBmp = remember(isLight) {
                            FerricWidgetRenderer.renderTransportIcon(
                                "next",
                                if (isLight) AndroidColor.rgb(20, 22, 28) else AndroidColor.WHITE,
                            )
                        }
                        Image(
                            bitmap = nextBmp.asImageBitmap(),
                            contentDescription = null,
                            modifier = Modifier
                                .size(config.textSize.prevNextDp.dp)
                                .padding(4.dp),
                        )
                    }
                    if (config.showLikeButton) {
                        Spacer(Modifier.weight(1f))
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
                                .size(config.textSize.likeDp.dp)
                                .padding(4.dp),
                        )
                    }
                    if (useSideSpacers) {
                        Spacer(Modifier.weight(0.4f))
                    }
                }

                if (config.showProgressBar || config.showTime) {
                    val elapsedPreviewMs = (durationMs * 0.65).roundToInt().toLong().coerceAtLeast(1_000L)
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        if (config.showTime) {
                            Text(
                                text = formatDurationMs(elapsedPreviewMs),
                                fontSize = config.textSize.timeSp.sp,
                                color = fgSecondary,
                            )
                            Spacer(Modifier.width(10.dp))
                        }
                        if (config.showProgressBar) {
                            Box(
                                modifier = Modifier
                                    .weight(1f)
                                    .height(7.dp)
                                    .clip(RoundedCornerShape(999.dp))
                                    .background(if (isLight) Color.Black.copy(alpha = 0.16f) else Color.White.copy(alpha = 0.30f)),
                            ) {
                                Box(
                                    modifier = Modifier
                                        .fillMaxWidth(0.65f)
                                        .height(7.dp)
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
                                text = formatDurationMs(durationMs),
                                fontSize = config.textSize.timeSp.sp,
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
