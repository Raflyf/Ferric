use crate::{CoreError, CoreResult};
use serde::{Deserialize, Serialize};

pub const STANDARD_10_BAND_FREQS: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EqFilterKind {
    #[default]
    Peaking,
    LowShelf,
    HighShelf,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqBand {
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
    pub kind: EqFilterKind,
    pub enabled: bool,
}

impl Default for EqBand {
    fn default() -> Self {
        Self {
            freq_hz: 1_000.0,
            gain_db: 0.0,
            q: 1.41,
            kind: EqFilterKind::Peaking,
            enabled: true,
        }
    }
}

impl EqBand {
    pub fn sanitized(&self) -> Self {
        Self {
            freq_hz: if self.freq_hz.is_finite() {
                self.freq_hz.clamp(20.0, 20_000.0)
            } else {
                1_000.0
            },
            gain_db: if self.gain_db.is_finite() {
                self.gain_db.clamp(-18.0, 18.0)
            } else {
                0.0
            },
            q: if self.q.is_finite() {
                self.q.clamp(0.1, 18.0)
            } else {
                1.41
            },
            kind: self.kind,
            enabled: self.enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqConfig {
    pub enabled: bool,
    pub preamp_db: f32,
    pub bands: Vec<EqBand>,
    pub preset_name: String,
}

impl Default for EqConfig {
    fn default() -> Self {
        Self::preset("Flat")
    }
}

impl EqConfig {
    pub fn preset(name: &str) -> Self {
        let (preamp, gains): (f32, [f32; 10]) = match name {
            "Warm Copper" => (-1.5, [2.5, 2.0, 1.2, 0.4, 0.0, -0.3, 0.5, 1.2, 1.6, 1.0]),
            "Deep Bass" | "Bass Boost" => {
                (-3.5, [5.5, 4.5, 2.8, 0.8, 0.0, 0.0, 0.4, 1.0, 1.2, 0.8])
            }
            "Bass Reducer" => (0.0, [-5.0, -4.0, -2.5, -1.0, 0.0, 0.5, 0.8, 1.0, 1.0, 0.5]),
            "Treble Boost" => (-2.0, [0.0, 0.0, 0.0, 0.0, 0.5, 1.2, 2.5, 3.8, 4.5, 4.0]),
            "Studio Ref" => (-0.5, [0.5, 0.2, 0.0, -0.3, 0.0, 0.2, 0.4, 0.6, 0.5, 0.3]),
            "Vocal Clarity" | "Vocal" => {
                (-1.5, [-1.5, -0.8, 0.0, 0.8, 1.8, 2.6, 2.2, 1.0, 0.5, 0.0])
            }
            "Rock" => (-2.5, [4.0, 3.0, 1.5, 0.0, -1.0, -0.5, 1.5, 3.0, 3.5, 2.5]),
            "Pop" => (-1.5, [-0.5, 1.5, 2.5, 1.8, 0.5, -0.5, 0.8, 1.8, 2.2, 1.5]),
            "Jazz" => (-1.5, [2.2, 1.6, 0.8, 1.4, -1.0, -1.0, 0.5, 1.5, 2.4, 2.0]),
            "Classical" => (-1.5, [3.0, 2.2, 1.2, 0.5, -0.5, -0.5, 0.0, 1.5, 2.5, 3.0]),
            "Electronic" => (-3.0, [4.5, 3.8, 1.5, -0.5, -1.5, 0.5, 1.2, 2.5, 3.8, 4.2]),
            "Acoustic" => (-1.5, [2.5, 1.8, 0.8, 0.5, 1.2, 1.5, 2.0, 2.2, 1.8, 1.0]),
            "Loudness" => (-3.5, [5.0, 3.8, 1.5, 0.0, -1.2, -0.8, 0.5, 2.5, 4.0, 3.5]),
            _ => (0.0, [0.0; 10]),
        };
        let bands = STANDARD_10_BAND_FREQS
            .iter()
            .enumerate()
            .map(|(idx, &freq)| EqBand {
                freq_hz: freq,
                gain_db: gains[idx],
                q: 1.41,
                kind: match idx {
                    0 => EqFilterKind::LowShelf,
                    9 => EqFilterKind::HighShelf,
                    _ => EqFilterKind::Peaking,
                },
                enabled: true,
            })
            .collect();
        Self {
            enabled: true,
            preamp_db: preamp,
            bands,
            preset_name: name.to_string(),
        }
    }

    /// Sanitize all numeric fields (`NaN`/`Inf` firewall) and cap band count to prevent DSP blowups.
    pub fn sanitized(&self) -> Self {
        const MAX_EQ_BANDS: usize = 32;
        Self {
            enabled: self.enabled,
            preamp_db: if self.preamp_db.is_finite() {
                self.preamp_db.clamp(-18.0, 18.0)
            } else {
                0.0
            },
            bands: self
                .bands
                .iter()
                .take(MAX_EQ_BANDS)
                .map(EqBand::sanitized)
                .collect(),
            preset_name: self.preset_name.clone(),
        }
    }

    /// Parse standard Equalizer APO `.txt` configuration (AutoEq / Peace compatible).
    pub fn from_equalizer_apo(text: &str) -> CoreResult<Self> {
        const MAX_EQ_BANDS: usize = 32;
        let mut preamp_db = 0.0_f32;
        let mut bands = Vec::new();

        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("Preamp:") {
                let num = rest
                    .split_whitespace()
                    .next()
                    .ok_or_else(|| CoreError::InvalidEqApo(line.to_string()))?;
                let parsed_preamp = num
                    .parse::<f32>()
                    .map_err(|_| CoreError::InvalidEqApo(line.to_string()))?;
                if !parsed_preamp.is_finite() {
                    return Err(CoreError::InvalidEqApo(line.to_string()));
                }
                preamp_db = parsed_preamp.clamp(-18.0, 18.0);
                continue;
            }
            if let Some(rest) = line.strip_prefix("Filter") {
                if bands.len() >= MAX_EQ_BANDS {
                    break;
                }
                let Some((_, body)) = rest.split_once(':') else {
                    continue;
                };
                let tokens: Vec<&str> = body.split_whitespace().collect();
                if tokens.len() < 8 {
                    continue;
                }
                let enabled = tokens[0].eq_ignore_ascii_case("ON");
                let kind = match tokens[1].to_ascii_uppercase().as_str() {
                    "LSC" | "LS" => EqFilterKind::LowShelf,
                    "HSC" | "HS" => EqFilterKind::HighShelf,
                    _ => EqFilterKind::Peaking,
                };
                let mut freq_hz = 1000.0_f32;
                let mut gain_db = 0.0_f32;
                let mut q = 1.41_f32;
                let mut i = 2;
                while i + 1 < tokens.len() {
                    match tokens[i] {
                        "Fc" => {
                            if let Ok(v) = tokens[i + 1].parse::<f32>()
                                && v.is_finite()
                            {
                                freq_hz = v.clamp(20.0, 20_000.0);
                            }
                        }
                        "Gain" => {
                            if let Ok(v) = tokens[i + 1].parse::<f32>()
                                && v.is_finite()
                            {
                                gain_db = v.clamp(-18.0, 18.0);
                            }
                        }
                        "Q" => {
                            if let Ok(v) = tokens[i + 1].parse::<f32>()
                                && v.is_finite()
                            {
                                q = v.clamp(0.1, 18.0);
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                bands.push(EqBand {
                    freq_hz,
                    gain_db,
                    q,
                    kind,
                    enabled,
                });
            }
        }

        if bands.is_empty() {
            return Err(CoreError::InvalidEqApo(
                "no Filter entries found in APO text".to_string(),
            ));
        }

        Ok(Self {
            enabled: true,
            preamp_db,
            bands,
            preset_name: "Imported APO".to_string(),
        })
    }

    /// Export as standard Equalizer APO `.txt` format.
    pub fn to_equalizer_apo(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(64 + self.bands.len() * 52);
        let _ = writeln!(
            out,
            "# Ferric Parametric EQ ({})\nPreamp: {:.2} dB",
            self.preset_name, self.preamp_db
        );
        for (idx, b) in self.bands.iter().enumerate() {
            let state = if b.enabled { "ON" } else { "OFF" };
            let kind = match b.kind {
                EqFilterKind::Peaking => "PK",
                EqFilterKind::LowShelf => "LSC",
                EqFilterKind::HighShelf => "HSC",
            };
            let _ = writeln!(
                out,
                "Filter {}: {} {} Fc {:.0} Hz Gain {:.2} dB Q {:.2}",
                idx + 1,
                state,
                kind,
                b.freq_hz,
                b.gain_db,
                b.q
            );
        }
        out
    }

    /// Compute the cascaded RBJ biquad magnitude response in dB at `freq_hz`,
    /// including `preamp_db`, clamped to `-18.0..=18.0`.
    pub fn response_db_at(&self, freq_hz: f32, sample_rate: u32) -> f32 {
        if !self.enabled || !freq_hz.is_finite() {
            return 0.0;
        }
        let fs = f64::from(sample_rate.max(8_000));
        let nyquist = fs * 0.48;
        let f_eval = f64::from(freq_hz).clamp(10.0, fs * 0.499);
        let w = 2.0 * std::f64::consts::PI * f_eval / fs;
        let cos_w = w.cos();
        let sin_w = w.sin();
        let cos_2w = (2.0 * w).cos();
        let sin_2w = (2.0 * w).sin();

        let mut total_db = if self.preamp_db.is_finite() {
            f64::from(self.preamp_db.clamp(-18.0, 18.0))
        } else {
            0.0
        };

        for band in self.bands.iter().take(32) {
            if !band.enabled
                || !band.gain_db.is_finite()
                || !band.freq_hz.is_finite()
                || !band.q.is_finite()
                || band.gain_db.abs() < 1e-4
            {
                continue;
            }
            let fc = f64::from(band.freq_hz).clamp(20.0, nyquist);
            let q = f64::from(band.q).clamp(0.1, 18.0);
            let w0 = 2.0 * std::f64::consts::PI * fc / fs;
            let omega_s = w0.sin();
            let omega_c = w0.cos();
            let alpha = omega_s / (2.0 * q);
            let a = 10.0_f64.powf(f64::from(band.gain_db.clamp(-18.0, 18.0)) / 40.0);
            let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

            let (b0, b1, b2, a1, a2) = match band.kind {
                EqFilterKind::Peaking => {
                    let a0 = 1.0 + alpha / a;
                    (
                        (1.0 + alpha * a) / a0,
                        (-2.0 * omega_c) / a0,
                        (1.0 - alpha * a) / a0,
                        (-2.0 * omega_c) / a0,
                        (1.0 - alpha / a) / a0,
                    )
                }
                EqFilterKind::LowShelf => {
                    let a0 = (a + 1.0) + (a - 1.0) * omega_c + two_sqrt_a_alpha;
                    (
                        (a * ((a + 1.0) - (a - 1.0) * omega_c + two_sqrt_a_alpha)) / a0,
                        (2.0 * a * ((a - 1.0) - (a + 1.0) * omega_c)) / a0,
                        (a * ((a + 1.0) - (a - 1.0) * omega_c - two_sqrt_a_alpha)) / a0,
                        (-2.0 * ((a - 1.0) + (a + 1.0) * omega_c)) / a0,
                        ((a + 1.0) + (a - 1.0) * omega_c - two_sqrt_a_alpha) / a0,
                    )
                }
                EqFilterKind::HighShelf => {
                    let a0 = (a + 1.0) - (a - 1.0) * omega_c + two_sqrt_a_alpha;
                    (
                        (a * ((a + 1.0) + (a - 1.0) * omega_c + two_sqrt_a_alpha)) / a0,
                        (-2.0 * a * ((a - 1.0) + (a + 1.0) * omega_c)) / a0,
                        (a * ((a + 1.0) + (a - 1.0) * omega_c - two_sqrt_a_alpha)) / a0,
                        (2.0 * ((a - 1.0) - (a + 1.0) * omega_c)) / a0,
                        ((a + 1.0) - (a - 1.0) * omega_c - two_sqrt_a_alpha) / a0,
                    )
                }
            };

            let num_re = b0 + b1 * cos_w + b2 * cos_2w;
            let num_im = -(b1 * sin_w + b2 * sin_2w);
            let den_re = 1.0 + a1 * cos_w + a2 * cos_2w;
            let den_im = -(a1 * sin_w + a2 * sin_2w);
            let mag_sq = (num_re * num_re + num_im * num_im)
                / (den_re * den_re + den_im * den_im).max(1e-18);
            total_db += 10.0 * mag_sq.max(1e-18).log10();
        }

        (total_db as f32).clamp(-18.0, 18.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equalizer_apo_roundtrips_cleanly() {
        let orig = EqConfig::preset("Warm Copper");
        let txt = orig.to_equalizer_apo();
        let parsed = EqConfig::from_equalizer_apo(&txt).expect("parse APO");
        assert_eq!(parsed.bands.len(), 10);
        assert!((parsed.preamp_db - orig.preamp_db).abs() < 0.01);
        assert!((parsed.bands[0].gain_db - 2.5).abs() < 0.01);
    }

    #[test]
    fn response_db_at_flat_is_zero_and_boosted_bands_are_positive() {
        let flat = EqConfig::preset("Flat");
        for &freq in &STANDARD_10_BAND_FREQS {
            assert!(
                flat.response_db_at(freq, 48_000).abs() < 1e-4,
                "expected 0 dB at {freq} Hz"
            );
        }

        let mut boosted = EqConfig::preset("Flat");
        boosted.bands[5].gain_db = 6.0; // 1000 Hz peaking band
        let center_db = boosted.response_db_at(1_000.0, 48_000);
        assert!(
            center_db > 5.9 && center_db <= 6.1,
            "expected ~6 dB at 1 kHz, got {center_db}"
        );
    }
}
