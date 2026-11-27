package dev.livelydesk.ferric

import dev.livelydesk.ferric.core.PlaybackStateReducer
import dev.livelydesk.ferric.core.PlaybackUiState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.ferric_mobile_ffi.FfiTelemetry
import uniffi.ferric_mobile_ffi.FfiTrack
import uniffi.ferric_mobile_ffi.FfiTrackSource

class PlaybackStateReducerTest {
    @Test
    fun appliesTelemetryAndClampsSpectrumBins() {
        val initial = PlaybackUiState()
        val telemetry = FfiTelemetry(
            isPlaying = true,
            positionMs = 42_500uL,
            durationMs = 180_000uL,
            sampleRate = 44_100u,
            outputRate = 48_000u,
            isCrossfading = true,
            spectrumBins = listOf(0u, 250u, 500u, 750u, 1000u, 1500u, 10u, 20u),
        )

        val updated = PlaybackStateReducer.applyTelemetry(initial, telemetry)
        assertTrue(updated.isPlaying)
        assertEquals(42_500L, updated.positionMs)
        assertEquals(180_000L, updated.durationMs)
        assertEquals(48_000, updated.outputRate)
        assertTrue(updated.isCrossfading)
        assertEquals(1000, updated.spectrumBins[5])
    }

    @Test
    fun appliesNowPlayingAndFormatsDuration() {
        val track = FfiTrack(
            id = "demo:1",
            title = "Slow Refraction",
            artist = "Mira Vale",
            album = "Liquid Horizon",
            durationMs = 195_000uL,
            path = "/tmp/demo.wav",
            source = FfiTrackSource.LOCAL,
            codec = "WAV",
            sampleRate = 44_100u,
            bitDepth = 16u,
            bitrateKbps = 1411u,
            spotifyId = null,
            youtubeVideoId = null,
            thumbnailUrl = null,
            replayGainDb = 0,
            liked = false,
        )

        val updated = PlaybackStateReducer.applyNowPlaying(PlaybackUiState(), track)
        assertEquals("Slow Refraction", updated.currentTrack?.title)
        assertEquals(195_000L, updated.durationMs)
        assertEquals("3:15", formatDurationMs(195_000L))
    }
}
