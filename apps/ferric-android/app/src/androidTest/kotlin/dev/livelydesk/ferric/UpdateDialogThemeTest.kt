package dev.livelydesk.ferric

import android.graphics.Bitmap
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.test.platform.app.InstrumentationRegistry
import dev.livelydesk.ferric.core.UpdateCheckOutcome
import dev.livelydesk.ferric.ui.components.AuroraBackdrop
import dev.livelydesk.ferric.ui.components.FerricUpdateDialogOverlay
import dev.livelydesk.ferric.ui.theme.CoverShadeColors
import dev.livelydesk.ferric.ui.theme.FerricTheme
import dev.livelydesk.ferric.ui.theme.LocalBackdropState
import dev.livelydesk.ferric.ui.theme.LocalFerricPalette
import dev.livelydesk.ferric.ui.theme.ferricBackdropSource
import dev.livelydesk.ferric.ui.theme.rememberFerricBackdropState
import java.io.File
import java.io.FileOutputStream
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import uniffi.ferric_mobile_ffi.FfiBackgroundMode
import uniffi.ferric_mobile_ffi.FfiThemeConfig

class UpdateDialogThemeTest {
    @get:Rule
    val composeRule = createComposeRule()

    private val sampleUpdate = UpdateCheckOutcome(
        hasUpdate = true,
        tag = "android-v0.1.1",
        releaseUrl = "https://github.com/Raflyf/Ferric/releases/tag/android-v0.1.1",
        summary = "Improved widget cover color extraction, smoother queue handoff, and faster startup sync.",
    )

    private fun saveTestShot(filename: String) {
        val ctx = InstrumentationRegistry.getInstrumentation().targetContext
        val dir = ctx.getExternalFilesDir(null) ?: return
        dir.mkdirs()
        val out = File(dir, filename)
        val bmp = composeRule.onRoot().captureToImage().asAndroidBitmap()
        FileOutputStream(out).use { fos ->
            bmp.compress(Bitmap.CompressFormat.PNG, 100, fos)
        }
    }

    @Test
    fun updatePopupAdaptsAcrossLiquidGlassWarmCreamAndAmoledAndHandlesActions() {
        val liquidGlass = FfiThemeConfig(
            name = "Liquid Glass",
            canvasHex = "#080A10",
            surfaceHex = "#121622",
            borderHex = "#2A3244",
            textPrimaryHex = "#F4F6FA",
            textSecondaryHex = "#B0BCD0",
            accentHex = "#FF783D",
            backgroundMode = FfiBackgroundMode.NATIVE_ACRYLIC,
            cornerRadiusPx = 22f,
            panelOpacity = 0.72f,
            adaptiveAlbumArt = false,
            acrylicEnabled = true,
            acrylicOpacity = 0.65f,
            acrylicTintHex = "#10131D",
            adaptiveAcrylicTint = false,
            editorialMode = false,
            fontFamily = "DM Sans",
            playerLayout = "liquid_glass",
        )
        val warmCream = liquidGlass.copy(
            name = "Warm Cream",
            canvasHex = "#F3EEE3",
            surfaceHex = "#EAE3D4",
            borderHex = "#D5CCB9",
            textPrimaryHex = "#111111",
            textSecondaryHex = "#524B42",
            accentHex = "#C44E1B",
            backgroundMode = FfiBackgroundMode.WARM_CREAM,
            cornerRadiusPx = 16f,
            acrylicEnabled = false,
            editorialMode = true,
            fontFamily = "Editorial",
        )
        val amoled = liquidGlass.copy(
            name = "AMOLED Black",
            canvasHex = "#000000",
            surfaceHex = "#0E0E13",
            borderHex = "#24242F",
            textPrimaryHex = "#F4F4F7",
            textSecondaryHex = "#9A9CA8",
            accentHex = "#4EA8FF",
            backgroundMode = FfiBackgroundMode.AMOLED_SOLID,
            cornerRadiusPx = 14f,
            acrylicEnabled = false,
            fontFamily = "Space Grotesk",
        )

        var activeTheme by mutableStateOf(liquidGlass)
        var actionTaken by mutableStateOf("")

        composeRule.setContent {
            FerricTheme(
                themeConfig = activeTheme,
                coverShade = CoverShadeColors(),
            ) {
                val palette = LocalFerricPalette.current
                val backdrop = rememberFerricBackdropState()
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(palette.canvas),
                ) {
                    Box(
                        modifier = Modifier
                            .fillMaxSize()
                            .ferricBackdropSource(backdrop, enabled = true),
                    ) {
                        if (palette.isLiquidGlass) {
                            AuroraBackdrop(
                                colors = CoverShadeColors(),
                                baseCanvas = palette.canvas,
                                isPlaying = false,
                                intensity = 0.9f,
                            )
                        }
                    }
                    CompositionLocalProvider(LocalBackdropState provides backdrop) {
                        FerricUpdateDialogOverlay(
                            outcome = sampleUpdate,
                            onUpdateNow = { url -> actionTaken = "update:$url" },
                            onLater = { actionTaken = "later" },
                            onSkipVersion = { tag -> actionTaken = "skip:$tag" },
                        )
                    }
                }
            }
        }

        composeRule.waitForIdle()
        composeRule.onNodeWithText("Update Available").assertIsDisplayed()
        composeRule.onNodeWithText("Update").assertIsDisplayed()
        composeRule.onNodeWithText("Later").assertIsDisplayed()
        composeRule.onNodeWithText("Skip android-v0.1.1").assertIsDisplayed()
        saveTestShot("update_popup_liquid_glass.png")

        composeRule.runOnUiThread { activeTheme = warmCream }
        composeRule.waitForIdle()
        composeRule.onNodeWithText("Update Available").assertIsDisplayed()
        saveTestShot("update_popup_warm_cream.png")

        composeRule.runOnUiThread { activeTheme = amoled }
        composeRule.waitForIdle()
        composeRule.onNodeWithText("Update Available").assertIsDisplayed()
        saveTestShot("update_popup_amoled.png")

        composeRule.onNodeWithText("Later").performClick()
        assertEquals("later", actionTaken)

        composeRule.onNodeWithText("Skip android-v0.1.1").performClick()
        assertEquals("skip:android-v0.1.1", actionTaken)

        composeRule.onNodeWithText("Update").performClick()
        assertEquals("update:https://github.com/Raflyf/Ferric/releases/tag/android-v0.1.1", actionTaken)
    }
}
