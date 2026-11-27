use ferric_core::eq::{EqBand, EqConfig, EqFilterKind};

/// Ferric 10-band octave center frequencies (Hz) aligned with ISO / AutoEq graphic profiles.
pub const AUTOEQ_BAND_FREQS_HZ: [f32; 10] = [
    31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoEqProfile {
    pub name: &'static str,
    pub category: &'static str,
    pub preamp_db: f32,
    pub gains_10band: [f32; 10],
}

impl AutoEqProfile {
    /// Convert this 10-band AutoEq profile into an active `ferric_core::eq::EqConfig`.
    pub fn to_eq_config(&self) -> EqConfig {
        let bands = AUTOEQ_BAND_FREQS_HZ
            .iter()
            .enumerate()
            .map(|(idx, &freq_hz)| EqBand {
                freq_hz,
                gain_db: self.gains_10band[idx],
                q: 1.41,
                kind: match idx {
                    0 => EqFilterKind::LowShelf,
                    9 => EqFilterKind::HighShelf,
                    _ => EqFilterKind::Peaking,
                },
                enabled: true,
            })
            .collect();

        EqConfig {
            enabled: true,
            preamp_db: self.preamp_db,
            bands,
            preset_name: self.name.to_string(),
        }
    }
}

/// Curated database of 24 Harman / AutoEq 10-band compensation profiles.
pub const AUTOEQ_PROFILES: [AutoEqProfile; 24] = [
    AutoEqProfile {
        name: "Sony WH-1000XM5",
        category: "Over-Ear ANC",
        preamp_db: -4.4,
        gains_10band: [-1.8, -3.2, -4.5, -3.6, -1.0, 0.8, 3.2, 4.2, 1.5, 0.6],
    },
    AutoEqProfile {
        name: "Sony WF-1000XM5",
        category: "TWS IEM",
        preamp_db: -3.6,
        gains_10band: [-0.8, -1.6, -2.2, -1.0, 0.2, 1.0, 2.8, 3.4, 0.8, -0.5],
    },
    AutoEqProfile {
        name: "Sennheiser HD 600",
        category: "Open-Back",
        preamp_db: -5.6,
        gains_10band: [5.4, 3.2, 0.4, -0.8, -0.4, -0.2, 0.3, -1.8, -0.6, 1.2],
    },
    AutoEqProfile {
        name: "Sennheiser HD 650",
        category: "Open-Back",
        preamp_db: -5.4,
        gains_10band: [5.2, 2.8, -0.6, -1.6, -0.5, 0.0, 0.8, -0.6, 1.4, 2.2],
    },
    AutoEqProfile {
        name: "Sennheiser HD 800 S",
        category: "Open-Back",
        preamp_db: -5.8,
        gains_10band: [5.6, 3.5, 1.2, 0.2, 0.4, 0.8, 1.9, -0.8, -4.2, -1.5],
    },
    AutoEqProfile {
        name: "Apple AirPods Pro 2",
        category: "TWS IEM",
        preamp_db: -2.0,
        gains_10band: [0.6, -0.8, -1.2, -0.4, 0.2, 0.4, 0.8, 1.2, 1.8, 1.0],
    },
    AutoEqProfile {
        name: "Apple AirPods Max",
        category: "Over-Ear ANC",
        preamp_db: -4.0,
        gains_10band: [-1.4, -0.8, 0.4, 0.8, 0.2, 0.6, 3.4, 3.8, -1.2, 0.5],
    },
    AutoEqProfile {
        name: "Moondrop Blessing 3",
        category: "Hybrid IEM",
        preamp_db: -3.2,
        gains_10band: [3.0, 2.1, 0.6, -0.2, 0.0, -0.4, -1.2, -1.8, -0.9, 0.8],
    },
    AutoEqProfile {
        name: "Moondrop Chu II",
        category: "Dynamic IEM",
        preamp_db: -2.2,
        gains_10band: [-1.6, -2.0, -1.1, 0.3, 0.8, 0.4, -0.6, -1.4, -2.2, 1.8],
    },
    AutoEqProfile {
        name: "Truthear Hexa",
        category: "Hybrid IEM",
        preamp_db: -3.4,
        gains_10band: [3.2, 2.4, 0.9, 0.0, -0.3, -0.2, 0.5, 0.8, -1.6, 1.4],
    },
    AutoEqProfile {
        name: "Truthear Nova",
        category: "Harman IEM",
        preamp_db: -2.0,
        gains_10band: [-0.8, -0.4, 0.5, 1.1, 0.6, -0.4, -1.5, -1.8, 0.4, 1.8],
    },
    AutoEqProfile {
        name: "7Hz Timeless",
        category: "Planar IEM",
        preamp_db: -2.4,
        gains_10band: [-1.2, -1.8, -1.4, 0.2, 1.0, 0.6, -2.1, -1.2, -3.4, 2.2],
    },
    AutoEqProfile {
        name: "Beyerdynamic DT 770 Pro (80 Ohm)",
        category: "Closed-Back",
        preamp_db: -4.2,
        gains_10band: [-1.2, -2.8, 1.4, 4.0, 0.8, -0.6, 1.2, -1.5, -5.2, -2.0],
    },
    AutoEqProfile {
        name: "Beyerdynamic DT 990 Pro",
        category: "Open-Back",
        preamp_db: -5.0,
        gains_10band: [4.8, 1.2, -2.4, -3.0, -0.6, 0.5, 1.4, -1.2, -6.2, -3.0],
    },
    AutoEqProfile {
        name: "Audio-Technica ATH-M50x",
        category: "Closed-Back",
        preamp_db: -3.6,
        gains_10band: [0.8, -1.4, -3.2, 1.6, 3.4, 0.4, 0.9, -2.4, -1.8, -0.6],
    },
    AutoEqProfile {
        name: "Hifiman Sundara",
        category: "Planar Open",
        preamp_db: -5.4,
        gains_10band: [5.2, 3.4, 1.1, 0.2, 0.0, -0.4, 2.6, 0.4, -2.2, -0.8],
    },
    AutoEqProfile {
        name: "Hifiman Edition XS",
        category: "Planar Open",
        preamp_db: -4.8,
        gains_10band: [4.6, 2.8, 0.8, 0.0, 0.2, -0.6, 3.2, 0.8, -2.8, -1.4],
    },
    AutoEqProfile {
        name: "Focal Clear",
        category: "Open-Back",
        preamp_db: -4.8,
        gains_10band: [4.6, 2.5, 0.3, -0.5, -0.4, -1.2, 0.8, 1.2, -1.0, 0.6],
    },
    AutoEqProfile {
        name: "AKG K371",
        category: "Closed-Back",
        preamp_db: -2.0,
        gains_10band: [-1.2, -0.6, 0.4, 0.8, 0.2, -0.3, 0.6, -1.4, 1.6, 0.8],
    },
    AutoEqProfile {
        name: "Shure SE215",
        category: "Dynamic IEM",
        preamp_db: -5.2,
        gains_10band: [-0.8, -2.2, -3.4, -2.6, -0.8, 0.6, 2.4, 4.2, 5.0, 3.8],
    },
    AutoEqProfile {
        name: "Samsung Galaxy Buds2 Pro",
        category: "TWS IEM",
        preamp_db: -1.8,
        gains_10band: [-0.6, -1.2, -0.4, 0.5, 0.4, 0.0, -0.8, -1.0, -2.4, 1.6],
    },
    AutoEqProfile {
        name: "Bose QuietComfort Ultra",
        category: "Over-Ear ANC",
        preamp_db: -3.2,
        gains_10band: [-3.4, -4.2, -3.0, -1.2, 0.4, 1.0, 2.6, 3.0, -0.8, 0.6],
    },
    AutoEqProfile {
        name: "Koss Porta Pro",
        category: "On-Ear Open",
        preamp_db: -5.0,
        gains_10band: [4.8, 0.6, -3.4, -2.8, -0.8, 0.4, 2.2, 3.6, 2.8, 3.2],
    },
    AutoEqProfile {
        name: "Harman Flat Reference",
        category: "Reference",
        preamp_db: 0.0,
        gains_10band: [0.0; 10],
    },
];

/// Case-insensitive multi-token search across AutoEq headphone/IEM names and categories.
pub fn search_autoeq_profiles(query: &str) -> Vec<&'static AutoEqProfile> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return AUTOEQ_PROFILES.iter().collect();
    }

    let mut scored: Vec<(u8, usize, &'static AutoEqProfile)> = Vec::new();
    for (idx, profile) in AUTOEQ_PROFILES.iter().enumerate() {
        let all_match = trimmed.split_whitespace().all(|tok| {
            ferric_core::text::contains_ascii_icase(profile.name, tok)
                || ferric_core::text::contains_ascii_icase(profile.category, tok)
        });
        if !all_match {
            continue;
        }

        let rank = if profile.name.eq_ignore_ascii_case(trimmed) {
            0
        } else if ferric_core::text::starts_with_ascii_icase(profile.name, trimmed) {
            1
        } else if ferric_core::text::contains_ascii_icase(profile.name, trimmed) {
            2
        } else {
            3
        };
        scored.push((rank, idx, profile));
    }

    scored.sort_by_key(|&(rank, idx, _)| (rank, idx));
    scored.into_iter().map(|(_, _, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_contains_all_24_profiles_and_searches_cleanly() {
        let all = search_autoeq_profiles("");
        assert_eq!(all.len(), 24);

        let hd = search_autoeq_profiles("sennheiser hd 600");
        assert_eq!(hd.len(), 1);
        assert_eq!(hd[0].name, "Sennheiser HD 600");

        let eq = hd[0].to_eq_config();
        assert!(eq.enabled);
        assert_eq!(eq.bands.len(), 10);
        assert_eq!(eq.bands[0].kind, EqFilterKind::LowShelf);
        assert_eq!(eq.bands[9].kind, EqFilterKind::HighShelf);
        assert!((eq.bands[0].freq_hz - 31.5).abs() < 0.01);
        assert!((eq.bands[9].freq_hz - 16000.0).abs() < 0.01);
    }
}
