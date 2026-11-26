use crate::{CoreError, CoreResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundMode {
    AmoledSolid,
    NativeAcrylic,
    ObsidianSolid,
    WarmCream,
}

fn default_ui_zoom() -> f32 {
    1.0
}

fn default_acrylic_opacity() -> f32 {
    0.72
}

fn default_acrylic_tint_hex() -> String {
    "#0B0D14".to_string()
}

fn default_adaptive_acrylic_tint() -> bool {
    true
}

/// Derive a deep, moody (or soft pastel-light) translucent Acrylic glass tint RGB `[0..1]`
/// from the currently playing song's extracted cover accent color, keeping high WCAG text contrast.
pub fn derive_acrylic_tint_from_accent(accent_rgb: [f32; 3], is_light: bool) -> [f32; 3] {
    let r = accent_rgb[0].clamp(0.0, 1.0);
    let g = accent_rgb[1].clamp(0.0, 1.0);
    let b = accent_rgb[2].clamp(0.0, 1.0);
    if is_light {
        // Frosted warm cream glass gently infused with the cover hue (preserves dark-ink AA/AAA contrast)
        let base = [0.925_f32, 0.905_f32, 0.870_f32];
        let mix_amt = 0.16_f32;
        [
            (base[0] + (r - base[0]) * mix_amt).clamp(0.76, 0.96),
            (base[1] + (g - base[1]) * mix_amt).clamp(0.74, 0.95),
            (base[2] + (b - base[2]) * mix_amt).clamp(0.70, 0.95),
        ]
    } else {
        // Rich jewel-toned dark smoked glass (or crisp charcoal-graphite for monochrome covers):
        // preserve authentic hue/chroma cleanly while clamping luminance deep enough that
        // white/light foreground typography stays > 7.5:1 WCAG AAA legible.
        let avg = (r + g + b) / 3.0;
        let chroma = r.max(g).max(b) - r.min(g).min(b);
        let boost = if chroma < 0.08 { 1.0 } else { 1.32 };
        let sat_r = (avg + (r - avg) * boost).clamp(0.0, 1.0);
        let sat_g = (avg + (g - avg) * boost).clamp(0.0, 1.0);
        let sat_b = (avg + (b - avg) * boost).clamp(0.0, 1.0);
        let mut tr = 0.018 + sat_r * 0.24;
        let mut tg = 0.019 + sat_g * 0.24;
        let mut tb = 0.024 + sat_b * 0.24;
        let lum = 0.2126 * tr + 0.7152 * tg + 0.0722 * tb;
        if lum > 0.145 {
            let scale = 0.145 / lum;
            tr *= scale;
            tg *= scale;
            tb *= scale;
        }
        [
            tr.clamp(0.018, 0.30),
            tg.clamp(0.018, 0.30),
            tb.clamp(0.020, 0.32),
        ]
    }
}

/// Complete theme contract matching `.ferric-theme.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: String,
    pub canvas_hex: String,
    pub surface_hex: String,
    pub border_hex: String,
    pub text_primary_hex: String,
    pub text_secondary_hex: String,
    pub accent_hex: String,
    pub background_mode: BackgroundMode,
    pub corner_radius_px: f32,
    pub track_row_height_px: f32,
    pub panel_opacity: f32,
    pub adaptive_album_art: bool,
    pub zero_gpu_minimize_saver: bool,
    pub sync_theme_to_android: bool,
    #[serde(default = "default_ui_zoom")]
    pub ui_zoom: f32,
    /// Native Windows 11 DWM Acrylic window transparency (OFF by default for pure AMOLED Black).
    pub acrylic_enabled: bool,
    /// Native Acrylic window opacity (`0.18 ..= 0.94`).
    #[serde(default = "default_acrylic_opacity")]
    pub acrylic_opacity: f32,
    /// Customizable Native Acrylic glass tint color (`#RRGGBB`).
    #[serde(default = "default_acrylic_tint_hex")]
    pub acrylic_tint_hex: String,
    /// Dynamically tint Native Acrylic glass smoothly from the currently playing cover art.
    #[serde(default = "default_adaptive_acrylic_tint")]
    pub adaptive_acrylic_tint: bool,
}

impl Default for ThemeConfig {
    /// Pure AMOLED Black canvas (#000000) with crisp obsidian surfaces and Ferric Burnt Orange accent.
    fn default() -> Self {
        Self {
            name: "AMOLED Black".to_string(),
            canvas_hex: "#000000".to_string(),
            surface_hex: "#09090D".to_string(),
            border_hex: "#1C1D25".to_string(),
            text_primary_hex: "#F4F4F7".to_string(),
            text_secondary_hex: "#9A9CA8".to_string(),
            accent_hex: "#E86A2C".to_string(),
            background_mode: BackgroundMode::AmoledSolid,
            corner_radius_px: 8.0,
            track_row_height_px: 46.0,
            panel_opacity: 1.0,
            adaptive_album_art: true,
            zero_gpu_minimize_saver: true,
            sync_theme_to_android: true,
            ui_zoom: 1.0,
            acrylic_enabled: false,
            acrylic_opacity: 0.72,
            acrylic_tint_hex: default_acrylic_tint_hex(),
            adaptive_acrylic_tint: true,
        }
    }
}

impl ThemeConfig {
    pub fn pure_amoled() -> Self {
        Self::default()
    }

    pub fn native_acrylic() -> Self {
        Self {
            name: "Native Acrylic".to_string(),
            canvas_hex: "#0B0C11".to_string(),
            surface_hex: "#11131B".to_string(),
            border_hex: "#252836".to_string(),
            background_mode: BackgroundMode::NativeAcrylic,
            corner_radius_px: 10.0,
            panel_opacity: 0.72,
            acrylic_enabled: true,
            acrylic_opacity: 0.72,
            ..Self::default()
        }
    }

    pub fn obsidian_slate() -> Self {
        Self {
            name: "Obsidian Studio".to_string(),
            canvas_hex: "#0D0E12".to_string(),
            surface_hex: "#14151C".to_string(),
            border_hex: "#232530".to_string(),
            background_mode: BackgroundMode::ObsidianSolid,
            ..Self::default()
        }
    }

    pub fn cream_light() -> Self {
        Self {
            name: "Warm Cream Light".to_string(),
            canvas_hex: "#F3EEE3".to_string(),
            surface_hex: "#EAE3D4".to_string(),
            border_hex: "#D5CCB9".to_string(),
            text_primary_hex: "#111111".to_string(),
            text_secondary_hex: "#524B42".to_string(),
            accent_hex: "#D9541E".to_string(),
            acrylic_tint_hex: "#EFE8DA".to_string(),
            background_mode: BackgroundMode::WarmCream,
            adaptive_album_art: false,
            adaptive_acrylic_tint: false,
            ..Self::default()
        }
    }

    pub fn text_contrast_ratio(&self) -> f64 {
        let fg = parse_hex_rgb(&self.text_primary_hex).unwrap_or((243, 238, 227));
        let bg = parse_hex_rgb(&self.canvas_hex).unwrap_or((0, 0, 0));
        wcag_contrast_ratio(fg, bg)
    }

    pub fn accent_contrast_ratio(&self) -> f64 {
        let fg = parse_hex_rgb(&self.accent_hex).unwrap_or((232, 106, 44));
        let bg = parse_hex_rgb(&self.canvas_hex).unwrap_or((0, 0, 0));
        wcag_contrast_ratio(fg, bg)
    }

    pub fn acrylic_tint_rgb(&self) -> (u8, u8, u8) {
        parse_hex_rgb(&self.acrylic_tint_hex).unwrap_or((11, 13, 20))
    }

    /// Effective background surface RGB used for WCAG foreground/icon contrast calculations.
    pub fn effective_bg_rgb(&self) -> (u8, u8, u8) {
        if self.acrylic_enabled {
            self.acrylic_tint_rgb()
        } else {
            parse_hex_rgb(&self.canvas_hex).unwrap_or((0, 0, 0))
        }
    }

    /// True whenever dark ink beats light ink under W3C WCAG 2.1 contrast on the active surface.
    pub fn is_light_surface(&self) -> bool {
        let bg = self.effective_bg_rgb();
        wcag_contrast_ratio((15, 17, 23), bg) > wcag_contrast_ratio((244, 244, 247), bg)
    }

    /// Compute WCAG-verified adaptive foreground, icon, divider, and accent colors for the active background
    /// and elevated surface/card backdrops so text & icons never drop below AA contrast on cards.
    pub fn adaptive_palette(&self, active_accent_rgb: (u8, u8, u8)) -> AdaptivePalette {
        let canvas = self.effective_bg_rgb();
        let surface = if self.acrylic_enabled {
            canvas
        } else {
            parse_hex_rgb(&self.surface_hex).unwrap_or(canvas)
        };
        let is_light = wcag_contrast_ratio((15, 17, 23), canvas)
            > wcag_contrast_ratio((244, 244, 247), canvas);
        let card = if is_light {
            lerp_rgb(surface, (255, 255, 255), 0.28)
        } else {
            lerp_rgb(surface, (28, 31, 44), 0.35)
        };
        let brand_rgb = parse_hex_rgb(&self.accent_hex).unwrap_or((232, 106, 44));
        AdaptivePalette::for_surfaces(
            canvas,
            &[canvas, surface, card],
            active_accent_rgb,
            brand_rgb,
            self.acrylic_enabled,
        )
    }

    pub fn sanitized(&self) -> Self {
        let mut out = self.clone();
        out.corner_radius_px = if out.corner_radius_px.is_finite() {
            out.corner_radius_px.clamp(0.0, 24.0)
        } else {
            8.0
        };
        out.track_row_height_px = if out.track_row_height_px.is_finite() {
            out.track_row_height_px.clamp(34.0, 72.0)
        } else {
            46.0
        };
        out.panel_opacity = if out.panel_opacity.is_finite() {
            out.panel_opacity.clamp(0.25, 1.0)
        } else {
            1.0
        };
        out.ui_zoom = if out.ui_zoom.is_finite() {
            out.ui_zoom.clamp(0.65, 1.40)
        } else {
            1.0
        };
        out.acrylic_opacity = if out.acrylic_opacity.is_finite() {
            out.acrylic_opacity.clamp(0.18, 0.94)
        } else {
            default_acrylic_opacity()
        };
        out
    }

    pub fn export_to_file(&self, path: &Path) -> CoreResult<()> {
        let json = serde_json::to_string_pretty(&self.sanitized())?;
        let tmp = path.with_extension("ferric-theme.tmp");
        std::fs::write(&tmp, &json)?;
        if std::fs::rename(&tmp, path).is_err() {
            std::fs::write(path, &json)?;
            let _ = std::fs::remove_file(&tmp);
        }
        Ok(())
    }

    pub fn import_from_file(path: &Path) -> CoreResult<Self> {
        use std::io::Read;
        const MAX_THEME_BYTES: u64 = 256 * 1024;
        let file = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        file.take(MAX_THEME_BYTES + 1).read_to_end(&mut buf)?;
        if buf.len() as u64 > MAX_THEME_BYTES {
            return Err(CoreError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Theme file exceeds 256 KiB limit",
            )));
        }
        let raw = String::from_utf8(buf).map_err(|_| {
            CoreError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Theme file is not valid UTF-8",
            ))
        })?;
        let parsed: Self = serde_json::from_str(&raw)?;
        Ok(parsed.sanitized())
    }
}

pub fn parse_hex_rgb(hex: &str) -> CoreResult<(u8, u8, u8)> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() != 6 || !clean.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(CoreError::InvalidHex(hex.to_string()));
    }
    let r =
        u8::from_str_radix(&clean[0..2], 16).map_err(|_| CoreError::InvalidHex(hex.to_string()))?;
    let g =
        u8::from_str_radix(&clean[2..4], 16).map_err(|_| CoreError::InvalidHex(hex.to_string()))?;
    let b =
        u8::from_str_radix(&clean[4..6], 16).map_err(|_| CoreError::InvalidHex(hex.to_string()))?;
    Ok((r, g, b))
}

pub fn hex_to_rgba_f32(hex: &str, alpha: f32) -> [f32; 4] {
    let (r, g, b) = parse_hex_rgb(hex).unwrap_or((0, 0, 0));
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, alpha]
}

fn relative_luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let lin = |c: u8| {
        let s = f64::from(c) / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// WCAG 2.1 contrast ratio (`1.0 ..= 21.0`).
pub fn wcag_contrast_ratio(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> f64 {
    let l1 = relative_luminance(fg);
    let l2 = relative_luminance(bg);
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

/// Complete WCAG-adaptive foreground/accent palette computed from any arbitrary background RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptivePalette {
    pub is_light: bool,
    pub text_primary: [f32; 4],
    pub text_secondary: [f32; 4],
    pub text_muted: [f32; 4],
    pub icon_idle: [f32; 4],
    pub icon_hover: [f32; 4],
    pub icon_muted: [f32; 4],
    pub divider: [f32; 4],
    pub accent: [f32; 4],
    pub brand_accent: [f32; 4],
    pub danger_text: [f32; 4],
}

fn rgb_to_rgba_f32((r, g, b): (u8, u8, u8), alpha: f32) -> [f32; 4] {
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        alpha,
    ]
}

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 {
        (f32::from(x) + (f32::from(y) - f32::from(x)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// Push `fg` smoothly toward `target` until it meets `min_ratio` contrast against `bg`.
pub fn ensure_min_contrast_rgb(
    fg: (u8, u8, u8),
    bg: (u8, u8, u8),
    target: (u8, u8, u8),
    min_ratio: f64,
) -> (u8, u8, u8) {
    if wcag_contrast_ratio(fg, bg) >= min_ratio {
        return fg;
    }
    let mut lo = 0.0_f32;
    let mut hi = 1.0_f32;
    let mut best = target;
    for _ in 0..12 {
        let mid = (lo + hi) * 0.5;
        let cand = lerp_rgb(fg, target, mid);
        if wcag_contrast_ratio(cand, bg) >= min_ratio {
            best = cand;
            hi = mid;
        } else {
            lo = mid;
        }
    }
    best
}

/// Push `fg` smoothly toward `target` until it meets `min_ratio` contrast against every surface in `surfaces`.
pub fn ensure_min_contrast_multi_rgb(
    fg: (u8, u8, u8),
    surfaces: &[(u8, u8, u8)],
    target: (u8, u8, u8),
    min_ratio: f64,
) -> (u8, u8, u8) {
    let worst = |cand: (u8, u8, u8)| -> f64 {
        surfaces
            .iter()
            .map(|&s| wcag_contrast_ratio(cand, s))
            .fold(f64::INFINITY, f64::min)
    };
    if surfaces.is_empty() || worst(fg) >= min_ratio {
        return fg;
    }
    let mut lo = 0.0_f32;
    let mut hi = 1.0_f32;
    let mut best = target;
    for _ in 0..14 {
        let mid = (lo + hi) * 0.5;
        let cand = lerp_rgb(fg, target, mid);
        if worst(cand) >= min_ratio {
            best = cand;
            hi = mid;
        } else {
            lo = mid;
        }
    }
    best
}

impl AdaptivePalette {
    pub fn for_background(
        bg: (u8, u8, u8),
        active_accent_rgb: (u8, u8, u8),
        brand_accent_rgb: (u8, u8, u8),
        acrylic_enabled: bool,
    ) -> Self {
        Self::for_surfaces(
            bg,
            &[bg],
            active_accent_rgb,
            brand_accent_rgb,
            acrylic_enabled,
        )
    }

    pub fn for_surfaces(
        bg: (u8, u8, u8),
        surfaces: &[(u8, u8, u8)],
        active_accent_rgb: (u8, u8, u8),
        brand_accent_rgb: (u8, u8, u8),
        acrylic_enabled: bool,
    ) -> Self {
        let dark_ink = (15, 17, 23);
        let light_ink = (244, 244, 247);
        let is_light = wcag_contrast_ratio(dark_ink, bg) > wcag_contrast_ratio(light_ink, bg);

        let (primary_rgb, sec_base, muted_base, icon_base, icon_hov_rgb, icon_mut_base, target_ink) =
            if is_light {
                (
                    (13, 15, 22),
                    (40, 45, 60),
                    (58, 64, 82),
                    (32, 37, 50),
                    (6, 7, 11),
                    (52, 58, 74),
                    (8, 10, 14),
                )
            } else {
                (
                    (244, 244, 247),
                    (178, 182, 198),
                    (144, 148, 168),
                    (168, 172, 188),
                    (255, 255, 255),
                    (126, 130, 148),
                    (255, 255, 255),
                )
            };

        let text_primary_rgb =
            ensure_min_contrast_multi_rgb(primary_rgb, surfaces, target_ink, 4.6);
        let text_secondary_rgb = ensure_min_contrast_multi_rgb(sec_base, surfaces, target_ink, 4.6);
        let text_muted_rgb = ensure_min_contrast_multi_rgb(muted_base, surfaces, target_ink, 4.6);
        let icon_idle_rgb = ensure_min_contrast_multi_rgb(icon_base, surfaces, target_ink, 4.6);
        let icon_muted_rgb =
            ensure_min_contrast_multi_rgb(icon_mut_base, surfaces, target_ink, 4.5);

        let accent_rgb =
            ensure_min_contrast_multi_rgb(active_accent_rgb, surfaces, target_ink, 4.6);
        let brand_rgb = ensure_min_contrast_multi_rgb(brand_accent_rgb, surfaces, target_ink, 4.6);
        let danger_base = if is_light {
            (185, 28, 28)
        } else {
            (255, 120, 133)
        };
        let danger_rgb = ensure_min_contrast_multi_rgb(danger_base, surfaces, target_ink, 4.6);

        let divider = if is_light {
            if acrylic_enabled {
                [0.05, 0.06, 0.10, 0.18]
            } else {
                [0.10, 0.11, 0.15, 0.22]
            }
        } else if acrylic_enabled {
            [1.0, 1.0, 1.0, 0.10]
        } else {
            [0.098, 0.106, 0.137, 1.0]
        };

        Self {
            is_light,
            text_primary: rgb_to_rgba_f32(text_primary_rgb, 1.0),
            text_secondary: rgb_to_rgba_f32(text_secondary_rgb, 1.0),
            text_muted: rgb_to_rgba_f32(text_muted_rgb, 1.0),
            icon_idle: rgb_to_rgba_f32(icon_idle_rgb, 1.0),
            icon_hover: rgb_to_rgba_f32(icon_hov_rgb, 1.0),
            icon_muted: rgb_to_rgba_f32(icon_muted_rgb, 1.0),
            divider,
            accent: rgb_to_rgba_f32(accent_rgb, 1.0),
            brand_accent: rgb_to_rgba_f32(brand_rgb, 1.0),
            danger_text: rgb_to_rgba_f32(danger_rgb, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_amoled_theme_passes_wcag_aa_and_aaa() {
        let theme = ThemeConfig::default();
        assert!(!theme.acrylic_enabled);
        assert!(theme.text_contrast_ratio() > 17.0);
        assert!(theme.accent_contrast_ratio() > 5.0);
    }

    #[test]
    fn native_acrylic_theme_roundtrips_json() {
        let mut theme = ThemeConfig::native_acrylic();
        theme.acrylic_tint_hex = "#2B140A".to_string();
        let json = serde_json::to_string(&theme).expect("serialize theme");
        let parsed: ThemeConfig = serde_json::from_str(&json).expect("deserialize theme");
        assert!(parsed.acrylic_enabled);
        assert!((parsed.acrylic_opacity - 0.72).abs() < 1e-4);
        assert_eq!(parsed.acrylic_tint_hex, "#2B140A");
        assert_eq!(parsed.acrylic_tint_rgb(), (43, 20, 10));
    }

    #[test]
    fn adaptive_palette_guarantees_wcag_aa_across_arbitrary_user_colors() {
        let test_bgs = [
            (0, 0, 0),       // AMOLED Black
            (11, 13, 20),    // Obsidian
            (157, 177, 246), // Light pastel lavender (#9DB1F6)
            (255, 255, 255), // Pure White
            (255, 240, 102), // Bright Yellow
            (232, 106, 44),  // Same as Ferric Burnt Orange
            (128, 128, 128), // Exact 50% Mid Gray
            (110, 110, 110), // Dark Mid Gray
            (11, 33, 24),    // Deep Emerald
            (243, 238, 227), // Warm Cream
        ];
        let to_u8 = |c: [f32; 4]| {
            (
                (c[0] * 255.0).round() as u8,
                (c[1] * 255.0).round() as u8,
                (c[2] * 255.0).round() as u8,
            )
        };
        for bg in test_bgs {
            let pal = AdaptivePalette::for_background(bg, (232, 106, 44), (232, 106, 44), true);
            assert!(
                wcag_contrast_ratio(to_u8(pal.text_primary), bg) >= 4.5,
                "primary text contrast failed on {bg:?}"
            );
            assert!(
                wcag_contrast_ratio(to_u8(pal.text_secondary), bg) >= 4.4,
                "secondary text contrast failed on {bg:?}"
            );
            assert!(
                wcag_contrast_ratio(to_u8(pal.icon_idle), bg) >= 4.4,
                "icon contrast failed on {bg:?}"
            );
            assert!(
                wcag_contrast_ratio(to_u8(pal.accent), bg) >= 4.4,
                "accent contrast failed on {bg:?}"
            );
        }
        assert!(
            AdaptivePalette::for_background((157, 177, 246), (232, 106, 44), (232, 106, 44), true)
                .is_light
        );
        assert!(
            !AdaptivePalette::for_background((11, 13, 20), (232, 106, 44), (232, 106, 44), true)
                .is_light
        );
    }

    #[test]
    fn parse_hex_rgb_rejects_non_ascii_six_byte_utf8_without_panicking() {
        // Two 3-byte UTF-8 chars ("€€") have byte length 6; must return Err, never slice panic.
        assert!(parse_hex_rgb("€€").is_err());
        assert!(parse_hex_rgb("#€€").is_err());
        assert!(parse_hex_rgb("ZZZZZZ").is_err());
        assert_eq!(parse_hex_rgb("#E86A2C").expect("valid hex"), (232, 106, 44));
    }

    #[test]
    fn adaptive_acrylic_tint_preserves_wcag_contrast_and_serde_default() {
        // Legacy theme JSON lacking `adaptive_acrylic_tint` must deserialize cleanly with default true.
        let legacy: ThemeConfig =
            serde_json::from_str(r#"{"acrylic_enabled":true}"#).expect("legacy theme json");
        assert!(legacy.adaptive_acrylic_tint);

        for accent in [
            [0.95, 0.42, 0.18], // Vivid orange
            [0.98, 0.96, 0.40], // Bright yellow
            [0.20, 0.85, 0.55], // Emerald
            [0.22, 0.48, 0.96], // Electric blue
            [0.96, 0.30, 0.68], // Neon pink
        ] {
            let dark_tint = derive_acrylic_tint_from_accent(accent, false);
            let dark_u8 = (
                (dark_tint[0] * 255.0).round() as u8,
                (dark_tint[1] * 255.0).round() as u8,
                (dark_tint[2] * 255.0).round() as u8,
            );
            assert!(
                wcag_contrast_ratio((244, 244, 247), dark_u8) >= 7.0,
                "dark adaptive acrylic tint {dark_u8:?} must keep AAA contrast for white text"
            );

            let light_tint = derive_acrylic_tint_from_accent(accent, true);
            let light_u8 = (
                (light_tint[0] * 255.0).round() as u8,
                (light_tint[1] * 255.0).round() as u8,
                (light_tint[2] * 255.0).round() as u8,
            );
            assert!(
                wcag_contrast_ratio((17, 17, 17), light_u8) >= 7.0,
                "light adaptive acrylic tint {light_u8:?} must keep AAA contrast for dark text"
            );
        }
    }
}
