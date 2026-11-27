package dev.livelydesk.ferric.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable

private val FerricAmoledColorScheme = darkColorScheme(
    primary = FerricOrange,
    onPrimary = AmoledBlack,
    secondary = FerricOrangeDark,
    onSecondary = TextPrimary,
    background = AmoledBlack,
    onBackground = TextPrimary,
    surface = ObsidianSurface,
    onSurface = TextPrimary,
    surfaceVariant = ObsidianElevated,
    onSurfaceVariant = TextSecondary,
    outline = HairlineBorder,
)

@Composable
fun FerricTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = FerricAmoledColorScheme,
        typography = FerricTypography,
        content = content,
    )
}
