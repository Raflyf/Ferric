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
    0.84
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
    /// Native Acrylic window opacity (`0.65 ..= 0.96`).
    #[serde(default = "default_acrylic_opacity")]
    pub acrylic_opacity: f32,
}

impl Default for ThemeConfig {
    /// Pure AMOLED Black canvas (#000000) with crisp obsidian surfaces and adaptive accent (WCAG AAA > 19:1).
    fn default() -> Self {
        Self {
            name: "AMOLED Black".to_string(),
            canvas_hex: "#000000".to_string(),
            surface_hex: "#09090D".to_string(),
            border_hex: "#1C1D25".to_string(),
            text_primary_hex: "#F4F4F7".to_string(),
            text_secondary_hex: "#9A9CA8".to_string(),
            accent_hex: "#A882FF".to_string(),
            background_mode: BackgroundMode::AmoledSolid,
            corner_radius_px: 8.0,
            track_row_height_px: 46.0,
            panel_opacity: 1.0,
            adaptive_album_art: true,
            zero_gpu_minimize_saver: true,
            sync_theme_to_android: true,
            ui_zoom: 1.0,
            acrylic_enabled: false,
            acrylic_opacity: 0.84,
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
            panel_opacity: 0.84,
            acrylic_enabled: true,
            acrylic_opacity: 0.84,
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
            background_mode: BackgroundMode::WarmCream,
            adaptive_album_art: false,
            ..Self::default()
        }
    }

    pub fn text_contrast_ratio(&self) -> f64 {
        let fg = parse_hex_rgb(&self.text_primary_hex).unwrap_or((243, 238, 227));
        let bg = parse_hex_rgb(&self.canvas_hex).unwrap_or((0, 0, 0));
        wcag_contrast_ratio(fg, bg)
    }

    pub fn accent_contrast_ratio(&self) -> f64 {
        let fg = parse_hex_rgb(&self.accent_hex).unwrap_or((168, 130, 255));
        let bg = parse_hex_rgb(&self.canvas_hex).unwrap_or((0, 0, 0));
        wcag_contrast_ratio(fg, bg)
    }

    pub fn export_to_file(&self, path: &Path) -> CoreResult<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn import_from_file(path: &Path) -> CoreResult<Self> {
        let raw = std::fs::read_to_string(path)?;
        let parsed: Self = serde_json::from_str(&raw)?;
        Ok(parsed)
    }
}

pub fn parse_hex_rgb(hex: &str) -> CoreResult<(u8, u8, u8)> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() != 6 {
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
        let theme = ThemeConfig::native_acrylic();
        let json = serde_json::to_string(&theme).expect("serialize theme");
        let parsed: ThemeConfig = serde_json::from_str(&json).expect("deserialize theme");
        assert!(parsed.acrylic_enabled);
        assert!((parsed.acrylic_opacity - 0.84).abs() < 1e-4);
    }
}
