use biquad::{Biquad, Coefficients, DirectForm1, ToHertz, Type};
use ferric_core::eq::{EqConfig, EqFilterKind};
use ferric_core::models::FadeCurve;
use std::f32::consts::{FRAC_PI_2, TAU};

#[inline]
fn sanitize_sample(s: f32) -> f32 {
    if s.is_finite() { s } else { 0.0 }
}

/// Compute equal-power crossfade gains `(g_a, g_b)` for `t in [0.0, 1.0]`.
/// Satisfies `g_a^2 + g_b^2 == 1.0` to maintain constant acoustic loudness across transitions.
#[inline]
pub fn equal_power_gains(progress: f32) -> (f32, f32) {
    let t = if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let theta = FRAC_PI_2 * t;
    (theta.cos(), theta.sin())
}

/// Hermite cubic S-curve envelope (`3t^2 - 2t^3`) for click-free micro-fades on Play, Pause, and Seek.
#[inline]
pub fn smooth_fade_envelope(progress: f32) -> f32 {
    let t = if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    t * t * (3.0 - 2.0 * t)
}

/// Compute deck crossfade gains `(g_a, g_b)` for `progress in [0.0, 1.0]` using the selected `FadeCurve`.
#[inline]
pub fn transition_gains(progress: f32, curve: FadeCurve) -> (f32, f32) {
    let t = if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    match curve {
        FadeCurve::EqualPower | FadeCurve::BassSwapAutomix => equal_power_gains(t),
        FadeCurve::SCurveVelvet => {
            let s = smooth_fade_envelope(t);
            ((1.0 - s).max(0.0).sqrt(), s.max(0.0).sqrt())
        }
    }
}

/// Complementary 1-pole 140 Hz crossover mixer for `FadeCurve::BassSwapAutomix`.
/// Splits each stereo deck into `<140 Hz` sub/bass and `>=140 Hz` mid/high bands so two
/// basslines hand off cleanly without +3..+6 dB low-frequency muddiness, while remaining
/// algebraically bit-transparent (`lp + hp == input`) at `progress == 0.0` and `progress == 1.0`.
#[derive(Debug, Clone)]
pub struct BassSwapMixer {
    alpha: f32,
    lp_a_l: f32,
    lp_a_r: f32,
    lp_b_l: f32,
    lp_b_r: f32,
}

impl BassSwapMixer {
    pub fn new(sample_rate: u32) -> Self {
        let mut mixer = Self {
            alpha: 0.018,
            lp_a_l: 0.0,
            lp_a_r: 0.0,
            lp_b_l: 0.0,
            lp_b_r: 0.0,
        };
        mixer.set_sample_rate(sample_rate);
        mixer
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        let fs = sample_rate.max(8_000) as f32;
        self.alpha = (1.0 - (-TAU * 140.0 / fs).exp()).clamp(0.001, 0.5);
    }

    pub fn reset(&mut self) {
        self.lp_a_l = 0.0;
        self.lp_a_r = 0.0;
        self.lp_b_l = 0.0;
        self.lp_b_r = 0.0;
    }

    #[inline]
    pub fn mix_frame(
        &mut self,
        deck_a: (f32, f32),
        deck_b: (f32, f32),
        progress: f32,
        curve: FadeCurve,
    ) -> (f32, f32) {
        let a_l = sanitize_sample(deck_a.0);
        let a_r = sanitize_sample(deck_a.1);
        let b_l = sanitize_sample(deck_b.0);
        let b_r = sanitize_sample(deck_b.1);
        let t = if progress.is_finite() {
            progress.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (ga, gb) = transition_gains(t, curve);

        if curve != FadeCurve::BassSwapAutomix {
            return (a_l * ga + b_l * gb, a_r * ga + b_r * gb);
        }

        let a = self.alpha;
        self.lp_a_l = sanitize_sample(self.lp_a_l + a * (a_l - self.lp_a_l));
        self.lp_a_r = sanitize_sample(self.lp_a_r + a * (a_r - self.lp_a_r));
        self.lp_b_l = sanitize_sample(self.lp_b_l + a * (b_l - self.lp_b_l));
        self.lp_b_r = sanitize_sample(self.lp_b_r + a * (b_r - self.lp_b_r));

        let hp_a_l = a_l - self.lp_a_l;
        let hp_a_r = a_r - self.lp_a_r;
        let hp_b_l = b_l - self.lp_b_l;
        let hp_b_r = b_r - self.lp_b_r;

        // Sub-bass (<140 Hz) uses constant-amplitude S-curve handoff to avoid low-end boom,
        // while mid/highs (>=140 Hz) float across with constant-power gains.
        let s = smooth_fade_envelope(t);
        let low_ga = 1.0 - s;
        let low_gb = s;

        let out_l = (self.lp_a_l * low_ga + self.lp_b_l * low_gb) + (hp_a_l * ga + hp_b_l * gb);
        let out_r = (self.lp_a_r * low_ga + self.lp_b_r * low_gb) + (hp_a_r * ga + hp_b_r * gb);
        (sanitize_sample(out_l), sanitize_sample(out_r))
    }
}

/// Lightweight 8-band dyadic filterbank envelope follower for zero-lock UI visualizer telemetry (`0..=1000`).
#[derive(Debug, Clone)]
pub struct SpectrumAnalyzer8 {
    alphas: [f32; 7],
    lp_state: [f32; 7],
    smoothed: [f32; 8],
}

impl SpectrumAnalyzer8 {
    const SPLIT_FREQS: [f32; 7] = [90.0, 180.0, 360.0, 720.0, 1_440.0, 2_880.0, 6_000.0];

    pub fn new(sample_rate: u32) -> Self {
        let fs = sample_rate.max(8_000) as f32;
        let mut alphas = [0.0_f32; 7];
        for (i, &fc) in Self::SPLIT_FREQS.iter().enumerate() {
            let clamped = fc.min(fs * 0.45);
            alphas[i] = (1.0 - (-TAU * clamped / fs).exp()).clamp(0.001, 0.95);
        }
        Self {
            alphas,
            lp_state: [0.0; 7],
            smoothed: [0.0; 8],
        }
    }

    pub fn decay_idle(&mut self) -> [u32; 8] {
        let mut out = [0_u32; 8];
        for (i, s) in self.smoothed.iter_mut().enumerate() {
            *s *= 0.78;
            if *s < 0.002 {
                *s = 0.0;
            }
            out[i] = (*s * 1_000.0).round().clamp(0.0, 1_000.0) as u32;
        }
        out
    }

    pub fn analyze_interleaved(&mut self, samples: &[f32]) -> [u32; 8] {
        let frames = samples.as_chunks::<2>().0;
        if frames.is_empty() {
            return self.decay_idle();
        }

        let mut energy_sum = [0.0_f32; 8];
        for &[raw_l, raw_r] in frames {
            let mono = 0.5 * (sanitize_sample(raw_l) + sanitize_sample(raw_r));
            for (lp, &alpha) in self.lp_state.iter_mut().zip(&self.alphas) {
                *lp = sanitize_sample(*lp + alpha * (mono - *lp));
            }
            let bands = [
                self.lp_state[0],
                self.lp_state[1] - self.lp_state[0],
                self.lp_state[2] - self.lp_state[1],
                self.lp_state[3] - self.lp_state[2],
                self.lp_state[4] - self.lp_state[3],
                self.lp_state[5] - self.lp_state[4],
                self.lp_state[6] - self.lp_state[5],
                mono - self.lp_state[6],
            ];
            for (acc, b) in energy_sum.iter_mut().zip(bands) {
                *acc += b * b;
            }
        }

        let inv_n = 1.0 / (frames.len() as f32);
        let mut out = [0_u32; 8];
        for (i, ((&e, smoothed), out_slot)) in energy_sum
            .iter()
            .zip(&mut self.smoothed)
            .zip(&mut out)
            .enumerate()
        {
            let rms = (e * inv_n).sqrt();
            // Apply mild +3 dB/octave visual pink-noise tilt so upper bands read clearly.
            let tilt = 1.8 + (i as f32) * 0.55;
            let target = (rms * tilt).sqrt().clamp(0.0, 1.0);
            let prev = *smoothed;
            *smoothed = if target >= prev {
                prev + 0.52 * (target - prev)
            } else {
                prev * 0.84 + target * 0.16
            };
            *out_slot = (*smoothed * 1_000.0).round().clamp(0.0, 1_000.0) as u32;
        }
        out
    }
}

/// C1-continuous transparent soft-knee limiter (exact 1:1 linear below 0.92, smooth asymptotic
/// tanh knee from 0.92..1.0) so mastered loud tracks stay 100% distortion-free with zero step cliff.
#[inline]
pub fn soft_limit(sample: f32) -> f32 {
    const THRESHOLD: f32 = 0.92;
    const KNEE: f32 = 1.0 - THRESHOLD;
    if !sample.is_finite() {
        return 0.0;
    }
    let abs = sample.abs();
    if abs <= THRESHOLD {
        sample
    } else {
        let excess = (abs - THRESHOLD) / KNEE;
        sample.signum() * (THRESHOLD + KNEE * excess.tanh())
    }
}

const UNITY_BIQUAD: Coefficients<f32> = Coefficients {
    a1: 0.0,
    a2: 0.0,
    b0: 1.0,
    b1: 0.0,
    b2: 0.0,
};

#[inline]
fn coeffs_are_finite(c: &Coefficients<f32>) -> bool {
    c.a1.is_finite() && c.a2.is_finite() && c.b0.is_finite() && c.b1.is_finite() && c.b2.is_finite()
}

/// Stereo parametric EQ processor built on click-minimized `DirectForm1` biquads with persistent
/// per-band slots (unity bypass for 0 dB / disabled bands) and full `NaN`/`Inf` firewalling.
pub struct StereoEqProcessor {
    sample_rate: f32,
    enabled: bool,
    preamp_linear: f32,
    left_filters: Vec<DirectForm1<f32>>,
    right_filters: Vec<DirectForm1<f32>>,
}

impl StereoEqProcessor {
    pub fn new(sample_rate: u32, config: &EqConfig) -> Self {
        let mut proc = Self {
            sample_rate: sample_rate.max(8_000) as f32,
            enabled: config.enabled,
            preamp_linear: db_to_linear(config.preamp_db),
            left_filters: Vec::new(),
            right_filters: Vec::new(),
        };
        proc.apply_config(config);
        proc
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32, config: &EqConfig) {
        let sr = sample_rate.max(8_000) as f32;
        if (sr - self.sample_rate).abs() > 1.0 {
            self.sample_rate = sr;
            self.apply_config(config);
        }
    }

    pub fn apply_config(&mut self, config: &EqConfig) {
        const MAX_EQ_BANDS: usize = 32;
        self.enabled = config.enabled;
        let preamp_db = if config.preamp_db.is_finite() {
            config.preamp_db.clamp(-18.0, 18.0)
        } else {
            0.0
        };
        self.preamp_linear = db_to_linear(preamp_db);
        let nyquist = self.sample_rate * 0.48;
        let fs = self.sample_rate.hz();

        let band_slice = &config.bands[..config.bands.len().min(MAX_EQ_BANDS)];
        let mut coeffs_list = Vec::with_capacity(band_slice.len());
        for band in band_slice {
            if !band.enabled
                || !band.gain_db.is_finite()
                || !band.freq_hz.is_finite()
                || !band.q.is_finite()
                || band.gain_db.abs() < 0.05
            {
                coeffs_list.push(UNITY_BIQUAD);
                continue;
            }
            let gain_db = band.gain_db.clamp(-18.0, 18.0);
            let fc = band.freq_hz.clamp(20.0, nyquist).hz();
            let q = band.q.clamp(0.1, 18.0);
            let ftype = match band.kind {
                EqFilterKind::Peaking => Type::PeakingEQ(gain_db),
                EqFilterKind::LowShelf => Type::LowShelf(gain_db),
                EqFilterKind::HighShelf => Type::HighShelf(gain_db),
            };
            let coeffs = Coefficients::<f32>::from_params(ftype, fs, fc, q)
                .ok()
                .filter(coeffs_are_finite)
                .unwrap_or(UNITY_BIQUAD);
            coeffs_list.push(coeffs);
        }

        if self.left_filters.len() == coeffs_list.len() {
            for (idx, c) in coeffs_list.into_iter().enumerate() {
                self.left_filters[idx].update_coefficients(c);
                self.right_filters[idx].update_coefficients(c);
            }
        } else {
            self.left_filters = coeffs_list
                .iter()
                .map(|&c| DirectForm1::<f32>::new(c))
                .collect();
            self.right_filters = coeffs_list
                .iter()
                .map(|&c| DirectForm1::<f32>::new(c))
                .collect();
        }
    }

    pub fn reset(&mut self) {
        for f in &mut self.left_filters {
            f.reset_state();
        }
        for f in &mut self.right_filters {
            f.reset_state();
        }
    }

    /// Process interleaved stereo `[L, R, L, R, ...]` samples in place with click-free volume smoothing.
    pub fn process_interleaved_stereo_ramped(
        &mut self,
        samples: &mut [f32],
        current_vol: &mut f32,
        target_vol: f32,
    ) {
        let target = if target_vol.is_finite() {
            target_vol.clamp(0.0, 1.5)
        } else {
            0.85
        };
        if !current_vol.is_finite() {
            *current_vol = target;
        }
        for frame in samples.as_chunks_mut::<2>().0 {
            *current_vol += (target - *current_vol) * 0.004;
            if (target - *current_vol).abs() < 1e-4 {
                *current_vol = target;
            }
            let vol = *current_vol;
            let mut l = sanitize_sample(frame[0]);
            let mut r = sanitize_sample(frame[1]);
            if self.enabled {
                l *= self.preamp_linear;
                r *= self.preamp_linear;
                for flt in &mut self.left_filters {
                    l = flt.run(l);
                    if !l.is_finite() {
                        flt.reset_state();
                        l = 0.0;
                    }
                }
                for flt in &mut self.right_filters {
                    r = flt.run(r);
                    if !r.is_finite() {
                        flt.reset_state();
                        r = 0.0;
                    }
                }
            }
            frame[0] = soft_limit(l * vol);
            frame[1] = soft_limit(r * vol);
        }
    }

    /// Process interleaved stereo `[L, R, L, R, ...]` samples in place.
    pub fn process_interleaved_stereo(&mut self, samples: &mut [f32], volume: f32) {
        let target = if volume.is_finite() {
            volume.clamp(0.0, 1.5)
        } else {
            0.85
        };
        let mut cur = target;
        self.process_interleaved_stereo_ramped(samples, &mut cur, target);
    }
}

#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    if !db.is_finite() {
        return 1.0;
    }
    10.0_f32.powf(db.clamp(-48.0, 24.0) / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_power_and_scurve_preserve_constant_power() {
        for curve in FadeCurve::all() {
            for i in 0..=10 {
                let t = i as f32 / 10.0;
                let (ga, gb) = transition_gains(t, curve);
                let power = ga * ga + gb * gb;
                assert!((power - 1.0).abs() < 1e-5, "power={power} at t={t}");
            }
        }
    }

    #[test]
    fn bass_swap_mixer_is_exact_at_boundaries_and_attenuates_dual_sub_boom() {
        let mut mixer = BassSwapMixer::new(48_000);
        let start = mixer.mix_frame((0.8, -0.6), (0.5, 0.5), 0.0, FadeCurve::BassSwapAutomix);
        assert!((start.0 - 0.8).abs() < 1e-6);
        assert!((start.1 + 0.6).abs() < 1e-6);

        let end = mixer.mix_frame((0.8, -0.6), (0.35, -0.25), 1.0, FadeCurve::BassSwapAutomix);
        assert!((end.0 - 0.35).abs() < 1e-6);
        assert!((end.1 + 0.25).abs() < 1e-6);

        // Feed DC / sub-bass into both decks and verify midpoint sum stays at 0.8 instead of 1.13.
        mixer.reset();
        let mut mid = (0.0, 0.0);
        for _ in 0..2_000 {
            mid = mixer.mix_frame((0.8, 0.8), (0.8, 0.8), 0.5, FadeCurve::BassSwapAutomix);
        }
        assert!(
            mid.0 < 0.85,
            "expected sub-bass swap to avoid +3 dB buildup, got {}",
            mid.0
        );
    }

    #[test]
    fn stereo_eq_processes_without_nan_or_clipping() {
        let eq = EqConfig::preset("Deep Bass");
        let mut proc = StereoEqProcessor::new(48_000, &eq);
        let mut buf = vec![0.5_f32; 256];
        proc.process_interleaved_stereo(&mut buf, 1.0);
        assert!(buf.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }

    #[test]
    fn soft_limit_is_c1_continuous_and_monotonic() {
        let mut prev = soft_limit(-4.0);
        for i in -3999..=4000 {
            let x = i as f32 / 1000.0;
            let y = soft_limit(x);
            assert!(y.is_finite() && y.abs() <= 1.0);
            assert!(
                y >= prev - 1e-6,
                "non-monotonic step at x={x}: prev={prev} y={y}"
            );
            assert!(
                (y - prev).abs() <= 0.002,
                "discontinuity cliff at x={x}: prev={prev} y={y}"
            );
            prev = y;
        }
        // Strictly transparent inside linear headroom (-0.92..0.92)
        assert!((soft_limit(0.86) - 0.86).abs() < 1e-6);
        assert!((soft_limit(-0.91) + 0.91).abs() < 1e-6);
    }
}
