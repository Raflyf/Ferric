package dev.livelydesk.ferric.ui.components

import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.log10
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt
import uniffi.ferric_mobile_ffi.FfiEqBand
import uniffi.ferric_mobile_ffi.FfiEqConfig
import uniffi.ferric_mobile_ffi.FfiEqFilterKind

/**
 * Pure JVM-testable Audio EQ Cookbook biquad transfer function magnitude evaluator (20 Hz..20 kHz)
 * plotting the true cumulative frequency response of Ferric's 10-band Parametric EQ + preamp.
 */
object EqCurveMath {
    private const val SAMPLE_RATE = 48_000.0
    const val DEFAULT_POINTS = 64

    private val defaultFreqs: List<Double> by lazy { computeLogFrequencies(DEFAULT_POINTS) }
    private val defaultCosW: DoubleArray by lazy { DoubleArray(DEFAULT_POINTS) { cos(2.0 * PI * defaultFreqs[it] / SAMPLE_RATE) } }
    private val defaultSinW: DoubleArray by lazy { DoubleArray(DEFAULT_POINTS) { sin(2.0 * PI * defaultFreqs[it] / SAMPLE_RATE) } }
    private val defaultCos2W: DoubleArray by lazy { DoubleArray(DEFAULT_POINTS) { cos(4.0 * PI * defaultFreqs[it] / SAMPLE_RATE) } }
    private val defaultSin2W: DoubleArray by lazy { DoubleArray(DEFAULT_POINTS) { sin(4.0 * PI * defaultFreqs[it] / SAMPLE_RATE) } }

    fun logFrequencies(points: Int = DEFAULT_POINTS): List<Double> {
        if (points == DEFAULT_POINTS) return defaultFreqs
        return computeLogFrequencies(points)
    }

    private fun computeLogFrequencies(points: Int): List<Double> {
        val minLog = log10(20.0)
        val maxLog = log10(20_000.0)
        return List(points) { idx ->
            val frac = idx.toDouble() / (points - 1).coerceAtLeast(1)
            10.0.pow(minLog + frac * (maxLog - minLog))
        }
    }

    fun evaluateCurveDb(eq: FfiEqConfig, freqsHz: List<Double> = logFrequencies()): List<Float> {
        if (!eq.enabled) return List(freqsHz.size) { 0f }
        val activeBands = eq.bands.filter { it.enabled && kotlin.math.abs(it.gainDb) > 0.01f }
        return freqsHz.map { f ->
            var db = eq.preampDb.toDouble()
            for (band in activeBands) {
                db += bandMagnitudeDb(band, f)
            }
            db.coerceIn(-18.0, 18.0).toFloat()
        }
    }

    /** Zero-allocation fast curve evaluator for 120 FPS interactive EQ fader drags. */
    fun evaluateCurveDbInto(
        outDb: FloatArray,
        bands: List<FfiEqBand>,
        bandGainsOverride: List<Float>?,
        preampDb: Float,
        enabled: Boolean,
    ) {
        val n = minOf(outDb.size, DEFAULT_POINTS)
        if (!enabled) {
            for (i in 0 until n) outDb[i] = 0f
            return
        }
        val baseDb = preampDb.toDouble()
        for (i in 0 until n) outDb[i] = baseDb.toFloat()

        for (bIdx in bands.indices) {
            val band = bands[bIdx]
            if (!band.enabled) continue
            val g = bandGainsOverride?.getOrNull(bIdx) ?: band.gainDb
            if (kotlin.math.abs(g) <= 0.01f) continue

            val f0 = band.freqHz.toDouble().coerceIn(20.0, 20_000.0)
            val q = band.q.toDouble().coerceIn(0.1, 18.0)
            val gainDb = g.toDouble().coerceIn(-18.0, 18.0)
            val a = 10.0.pow(gainDb / 40.0)
            val w0 = 2.0 * PI * f0 / SAMPLE_RATE
            val cosW0 = cos(w0)
            val sinW0 = sin(w0)
            val alpha = sinW0 / (2.0 * q)

            val b0: Double
            val b1: Double
            val b2: Double
            val a0: Double
            val a1: Double
            val a2: Double
            when (band.kind) {
                FfiEqFilterKind.PEAKING -> {
                    b0 = 1.0 + alpha * a
                    b1 = -2.0 * cosW0
                    b2 = 1.0 - alpha * a
                    a0 = 1.0 + alpha / a
                    a1 = -2.0 * cosW0
                    a2 = 1.0 - alpha / a
                }
                FfiEqFilterKind.LOW_SHELF -> {
                    val twoSqrtAAlpha = 2.0 * sqrt(a) * alpha
                    b0 = a * ((a + 1.0) - (a - 1.0) * cosW0 + twoSqrtAAlpha)
                    b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cosW0)
                    b2 = a * ((a + 1.0) - (a - 1.0) * cosW0 - twoSqrtAAlpha)
                    a0 = (a + 1.0) + (a - 1.0) * cosW0 + twoSqrtAAlpha
                    a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cosW0)
                    a2 = (a + 1.0) + (a - 1.0) * cosW0 - twoSqrtAAlpha
                }
                FfiEqFilterKind.HIGH_SHELF -> {
                    val twoSqrtAAlpha = 2.0 * sqrt(a) * alpha
                    b0 = a * ((a + 1.0) + (a - 1.0) * cosW0 + twoSqrtAAlpha)
                    b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cosW0)
                    b2 = a * ((a + 1.0) + (a - 1.0) * cosW0 - twoSqrtAAlpha)
                    a0 = (a + 1.0) - (a - 1.0) * cosW0 + twoSqrtAAlpha
                    a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cosW0)
                    a2 = (a + 1.0) - (a - 1.0) * cosW0 - twoSqrtAAlpha
                }
            }

            for (i in 0 until n) {
                val cw = defaultCosW[i]
                val sw = defaultSinW[i]
                val c2w = defaultCos2W[i]
                val s2w = defaultSin2W[i]
                val numRe = b0 + b1 * cw + b2 * c2w
                val numIm = -(b1 * sw + b2 * s2w)
                val denRe = a0 + a1 * cw + a2 * c2w
                val denIm = -(a1 * sw + a2 * s2w)
                val magSq = (numRe * numRe + numIm * numIm) / (denRe * denRe + denIm * denIm).coerceAtLeast(1e-12)
                outDb[i] += (10.0 * log10(magSq.coerceAtLeast(1e-12))).toFloat()
            }
        }
        for (i in 0 until n) {
            outDb[i] = outDb[i].coerceIn(-18f, 18f)
        }
    }

    private fun bandMagnitudeDb(band: FfiEqBand, freqHz: Double): Double {
        val f0 = band.freqHz.toDouble().coerceIn(20.0, 20_000.0)
        val q = band.q.toDouble().coerceIn(0.1, 18.0)
        val gainDb = band.gainDb.toDouble().coerceIn(-18.0, 18.0)
        val a = 10.0.pow(gainDb / 40.0)
        val w0 = 2.0 * PI * f0 / SAMPLE_RATE
        val cosW0 = cos(w0)
        val sinW0 = sin(w0)
        val alpha = sinW0 / (2.0 * q)

        val b0: Double
        val b1: Double
        val b2: Double
        val a0: Double
        val a1: Double
        val a2: Double
        when (band.kind) {
            FfiEqFilterKind.PEAKING -> {
                b0 = 1.0 + alpha * a
                b1 = -2.0 * cosW0
                b2 = 1.0 - alpha * a
                a0 = 1.0 + alpha / a
                a1 = -2.0 * cosW0
                a2 = 1.0 - alpha / a
            }
            FfiEqFilterKind.LOW_SHELF -> {
                val twoSqrtAAlpha = 2.0 * sqrt(a) * alpha
                b0 = a * ((a + 1.0) - (a - 1.0) * cosW0 + twoSqrtAAlpha)
                b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cosW0)
                b2 = a * ((a + 1.0) - (a - 1.0) * cosW0 - twoSqrtAAlpha)
                a0 = (a + 1.0) + (a - 1.0) * cosW0 + twoSqrtAAlpha
                a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cosW0)
                a2 = (a + 1.0) + (a - 1.0) * cosW0 - twoSqrtAAlpha
            }
            FfiEqFilterKind.HIGH_SHELF -> {
                val twoSqrtAAlpha = 2.0 * sqrt(a) * alpha
                b0 = a * ((a + 1.0) + (a - 1.0) * cosW0 + twoSqrtAAlpha)
                b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cosW0)
                b2 = a * ((a + 1.0) + (a - 1.0) * cosW0 - twoSqrtAAlpha)
                a0 = (a + 1.0) - (a - 1.0) * cosW0 + twoSqrtAAlpha
                a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cosW0)
                a2 = (a + 1.0) - (a - 1.0) * cosW0 - twoSqrtAAlpha
            }
        }

        val w = 2.0 * PI * freqHz / SAMPLE_RATE
        val cosW = cos(w)
        val sinW = sin(w)
        val cos2W = cos(2.0 * w)
        val sin2W = sin(2.0 * w)

        val numRe = b0 + b1 * cosW + b2 * cos2W
        val numIm = -(b1 * sinW + b2 * sin2W)
        val denRe = a0 + a1 * cosW + a2 * cos2W
        val denIm = -(a1 * sinW + a2 * sin2W)

        val magSq = (numRe * numRe + numIm * numIm) / (denRe * denRe + denIm * denIm).coerceAtLeast(1e-12)
        return 10.0 * log10(magSq.coerceAtLeast(1e-12))
    }
}
