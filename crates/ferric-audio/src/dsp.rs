use biquad::{Biquad, Coefficients, DirectForm1, ToHertz, Type};
use ferric_core::eq::{EqConfig, EqFilterKind};
use ferric_core::models::FadeCurve;
use std::f32::consts::{FRAC_PI_2, TAU};

/// Compute equal-power crossfade gains `(g_a, g_b)` for `t in [0.0, 1.0]`.
/// Satisfies `g_a^2 + g_b^2 == 1.0` to maintain constant acoustic loudness across transitions.
#[inline]
pub fn equal_power_gains(progress: f32) -> (f32, f32) {
    let t = progress.clamp(0.0, 1.0);
    let theta = FRAC_PI_2 * t;
    (theta.cos(), theta.sin())
}

/// Hermite cubic S-curve envelope (`3t^2 - 2t^3`) for click-free micro-fades on Play, Pause, and Seek.
#[inline]
pub fn smooth_fade_envelope(progress: f32) -> f32 {
    let t = progress.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Compute deck crossfade gains `(g_a, g_b)` for `progress in [0.0, 1.0]` using the selected `FadeCurve`.
#[inline]
pub fn transition_gains(progress: f32, curve: FadeCurve) -> (f32, f32) {
    let t = progress.clamp(0.0, 1.0);
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
        let t = progress.clamp(0.0, 1.0);
        let (ga, gb) = transition_gains(t, curve);

        if curve != FadeCurve::BassSwapAutomix {
            return (deck_a.0 * ga + deck_b.0 * gb, deck_a.1 * ga + deck_b.1 * gb);
        }

        let a = self.alpha;
        self.lp_a_l += a * (deck_a.0 - self.lp_a_l);
        self.lp_a_r += a * (deck_a.1 - self.lp_a_r);
        self.lp_b_l += a * (deck_b.0 - self.lp_b_l);
        self.lp_b_r += a * (deck_b.1 - self.lp_b_r);

        let hp_a_l = deck_a.0 - self.lp_a_l;
        let hp_a_r = deck_a.1 - self.lp_a_r;
        let hp_b_l = deck_b.0 - self.lp_b_l;
        let hp_b_r = deck_b.1 - self.lp_b_r;

        // Sub-bass (<140 Hz) uses constant-amplitude S-curve handoff to avoid low-end boom,
        // while mid/highs (>=140 Hz) float across with constant-power gains.
        let s = smooth_fade_envelope(t);
        let low_ga = 1.0 - s;
        let low_gb = s;

        let out_l = (self.lp_a_l * low_ga + self.lp_b_l * low_gb) + (hp_a_l * ga + hp_b_l * gb);
        let out_r = (self.lp_a_r * low_ga + self.lp_b_r * low_gb) + (hp_a_r * ga + hp_b_r * gb);
        (out_l, out_r)
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
        for &[l, r] in frames {
            let mono = 0.5 * (l + r);
            for (lp, &alpha) in self.lp_state.iter_mut().zip(&self.alphas) {
                *lp += alpha * (mono - *lp);
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

/// Smooth soft-clip limiter preventing harsh digital clipping when EQ bands are boosted.
#[inline]
pub fn soft_limit(sample: f32) -> f32 {
    if sample.abs() <= 0.85 {
        sample
    } else {
        sample.tanh()
    }
}

/// Stereo 10-band parametric EQ processor built on click-minimized `DirectForm1` biquads.
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
        self.enabled = config.enabled;
        self.preamp_linear = db_to_linear(config.preamp_db);
        let nyquist = self.sample_rate * 0.48;
        let fs = self.sample_rate.hz();

        let mut coeffs_list = Vec::with_capacity(config.bands.len());
        for band in &config.bands {
            if !band.enabled || band.gain_db.abs() < 0.05 {
                continue;
            }
            let fc = band.freq_hz.clamp(20.0, nyquist).hz();
            let q = band.q.clamp(0.1, 16.0);
            let ftype = match band.kind {
                EqFilterKind::Peaking => Type::PeakingEQ(band.gain_db),
                EqFilterKind::LowShelf => Type::LowShelf(band.gain_db),
                EqFilterKind::HighShelf => Type::HighShelf(band.gain_db),
            };
            if let Ok(c) = Coefficients::<f32>::from_params(ftype, fs, fc, q) {
                coeffs_list.push(c);
            }
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

    /// Process interleaved stereo `[L, R, L, R, ...]` samples in place.
    pub fn process_interleaved_stereo(&mut self, samples: &mut [f32], volume: f32) {
        let vol = volume.clamp(0.0, 1.5);
        for frame in samples.as_chunks_mut::<2>().0 {
            let mut l = frame[0];
            let mut r = frame[1];
            if self.enabled {
                l *= self.preamp_linear;
                r *= self.preamp_linear;
                for flt in &mut self.left_filters {
                    l = flt.run(l);
                }
                for flt in &mut self.right_filters {
                    r = flt.run(r);
                }
            }
            frame[0] = soft_limit(l * vol);
            frame[1] = soft_limit(r * vol);
        }
    }
}

#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
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
}
