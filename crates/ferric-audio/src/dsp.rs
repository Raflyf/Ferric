use biquad::{Biquad, Coefficients, DirectForm1, ToHertz, Type};
use ferric_core::eq::{EqConfig, EqFilterKind};
use std::f32::consts::FRAC_PI_2;

/// Compute equal-power crossfade gains `(g_a, g_b)` for `t in [0.0, 1.0]`.
/// Satisfies `g_a^2 + g_b^2 == 1.0` to maintain constant acoustic loudness across transitions.
pub fn equal_power_gains(progress: f32) -> (f32, f32) {
    let t = progress.clamp(0.0, 1.0);
    let theta = FRAC_PI_2 * t;
    (theta.cos(), theta.sin())
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
    fn equal_power_crossfade_preserves_constant_power() {
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            let (ga, gb) = equal_power_gains(t);
            let power = ga * ga + gb * gb;
            assert!((power - 1.0).abs() < 1e-5, "power={power} at t={t}");
        }
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
