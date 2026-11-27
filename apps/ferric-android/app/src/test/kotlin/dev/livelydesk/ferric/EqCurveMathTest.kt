package dev.livelydesk.ferric

import androidx.compose.ui.graphics.Color
import dev.livelydesk.ferric.ui.components.EqCurveMath
import dev.livelydesk.ferric.ui.theme.ensureWcagAccent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.ferric_mobile_ffi.FfiEqBand
import uniffi.ferric_mobile_ffi.FfiEqConfig
import uniffi.ferric_mobile_ffi.FfiEqFilterKind

class EqCurveMathTest {
    @Test
    fun flatEqEvaluatesToZeroDbAcrossSpectrum() {
        val flat = FfiEqConfig(
            enabled = true,
            preampDb = 0f,
            bands = listOf(
                FfiEqBand(31f, 0f, 1.41f, FfiEqFilterKind.LOW_SHELF, true),
                FfiEqBand(1000f, 0f, 1.41f, FfiEqFilterKind.PEAKING, true),
                FfiEqBand(16000f, 0f, 1.41f, FfiEqFilterKind.HIGH_SHELF, true),
            ),
            presetName = "Flat",
        )
        val curve = EqCurveMath.evaluateCurveDb(flat)
        assertEquals(64, curve.size)
        assertTrue(curve.all { kotlin.math.abs(it) < 0.05f })
    }

    @Test
    fun bassBoostRaisesLowFrequenciesAndPreampShiftsUniformly() {
        val boosted = FfiEqConfig(
            enabled = true,
            preampDb = -2f,
            bands = listOf(
                FfiEqBand(62f, 6f, 1.41f, FfiEqFilterKind.LOW_SHELF, true),
            ),
            presetName = "Deep Bass",
        )
        val freqs = listOf(40.0, 1000.0, 12000.0)
        val curve = EqCurveMath.evaluateCurveDb(boosted, freqs)
        // Low shelf +6 dB minus 2 dB preamp should exceed +2 dB at 40 Hz and approach -2 dB at 12 kHz.
        assertTrue("Expected low freq boost > 2 dB, got ${curve[0]}", curve[0] > 2.0f)
        assertTrue("Expected high freq near preamp (-2 dB), got ${curve[2]}", curve[2] < -1.5f)
    }

    @Test
    fun ensureWcagAccentProtectsReadabilityOnBlackAndCream() {
        val darkBlueOnBlack = ensureWcagAccent(Color(0xFF0A1030), Color.Black, isLightBg = false)
        assertTrue(darkBlueOnBlack.blue > 0.35f || darkBlueOnBlack.red > 0.35f)

        val paleYellowOnCream = ensureWcagAccent(Color(0xFFFFF8B0), Color(0xFFF3EEE3), isLightBg = true)
        assertTrue(paleYellowOnCream.red < 0.85f || paleYellowOnCream.green < 0.55f)
    }
}
