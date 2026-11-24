use crate::{CoreError, CoreResult};
use serde::{Deserialize, Serialize};

pub const STANDARD_10_BAND_FREQS: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EqFilterKind {
    Peaking,
    LowShelf,
    HighShelf,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
    pub kind: EqFilterKind,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
            "Deep Bass" => (-3.5, [5.5, 4.5, 2.8, 0.8, 0.0, 0.0, 0.4, 1.0, 1.2, 0.8]),
            "Studio Ref" => (-0.5, [0.5, 0.2, 0.0, -0.3, 0.0, 0.2, 0.4, 0.6, 0.5, 0.3]),
            "Vocal Clarity" => (-1.5, [-1.5, -0.8, 0.0, 0.8, 1.8, 2.6, 2.2, 1.0, 0.5, 0.0]),
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

    /// Parse standard Equalizer APO `.txt` configuration (AutoEq / Peace / Sonora compatible).
    pub fn from_equalizer_apo(text: &str) -> CoreResult<Self> {
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
                preamp_db = num
                    .parse::<f32>()
                    .map_err(|_| CoreError::InvalidEqApo(line.to_string()))?
                    .clamp(-18.0, 18.0);
                continue;
            }
            if let Some(rest) = line.strip_prefix("Filter") {
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
                            if let Ok(v) = tokens[i + 1].parse::<f32>() {
                                freq_hz = v.clamp(20.0, 20_000.0);
                            }
                        }
                        "Gain" => {
                            if let Ok(v) = tokens[i + 1].parse::<f32>() {
                                gain_db = v.clamp(-18.0, 18.0);
                            }
                        }
                        "Q" => {
                            if let Ok(v) = tokens[i + 1].parse::<f32>() {
                                q = v.clamp(0.1, 16.0);
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
        let mut out = format!(
            "# Ferric Parametric EQ ({})\nPreamp: {:.2} dB\n",
            self.preset_name, self.preamp_db
        );
        for (idx, b) in self.bands.iter().enumerate() {
            let state = if b.enabled { "ON" } else { "OFF" };
            let kind = match b.kind {
                EqFilterKind::Peaking => "PK",
                EqFilterKind::LowShelf => "LSC",
                EqFilterKind::HighShelf => "HSC",
            };
            out.push_str(&format!(
                "Filter {}: {} {} Fc {:.0} Hz Gain {:.2} dB Q {:.2}\n",
                idx + 1,
                state,
                kind,
                b.freq_hz,
                b.gain_db,
                b.q
            ));
        }
        out
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
}
