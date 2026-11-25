use crate::{CoreError, CoreResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundMode {
    AmoledSolid,
    CustomWallpaper,
    Gradient,
    MicaBlur,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiquidGlassPreset {
    CrystalPure,
    PrismaticLiquid,
    SmokedCopper,
    UltraFrosted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiquidGlassConfig {
    /// Default OFF to preserve 0% GPU Pure AMOLED identity.
    pub liquid_glass: bool,
    /// Snell optical refraction strength (1.00 ..= 2.00).
    pub ior: f32,
    /// Chromatic RGB edge dispersion intensity (0.0 ..= 1.0).
    pub dispersion: f32,
    /// Interior frost blur radius in px (0.0 ..= 32.0).
    pub frost_blur_px: f32,
    /// Curved glass bevel width in px (8.0 ..= 36.0).
    pub bevel_px: f32,
    /// Specular rim catchlight intensity (0.0 ..= 1.0).
    pub specular_rim: f32,
    /// Dark readability scrim inside glass panels (0.0 ..= 0.65).
    pub scrim_alpha: f32,
    /// Automatically suspend optical shader on minimize / lock / low battery.
    pub auto_fallback_battery: bool,
}

impl Default for LiquidGlassConfig {
    fn default() -> Self {
        Self {
            liquid_glass: false,
            ior: 1.48,
            dispersion: 0.42,
            frost_blur_px: 10.0,
            bevel_px: 24.0,
            specular_rim: 0.78,
            scrim_alpha: 0.28,
            auto_fallback_battery: true,
        }
    }
}

impl LiquidGlassConfig {
    pub fn apply_preset(&mut self, preset: LiquidGlassPreset) {
        self.liquid_glass = true;
        match preset {
            LiquidGlassPreset::CrystalPure => {
                self.ior = 1.42;
                self.dispersion = 0.0;
                self.frost_blur_px = 4.0;
                self.bevel_px = 20.0;
                self.specular_rim = 0.85;
                self.scrim_alpha = 0.24;
            }
            LiquidGlassPreset::PrismaticLiquid => {
                self.ior = 1.48;
                self.dispersion = 0.42;
                self.frost_blur_px = 10.0;
                self.bevel_px = 24.0;
                self.specular_rim = 0.78;
                self.scrim_alpha = 0.28;
            }
            LiquidGlassPreset::SmokedCopper => {
                self.ior = 1.52;
                self.dispersion = 0.22;
                self.frost_blur_px = 14.0;
                self.bevel_px = 22.0;
                self.specular_rim = 0.72;
                self.scrim_alpha = 0.36;
            }
            LiquidGlassPreset::UltraFrosted => {
                self.ior = 1.32;
                self.dispersion = 0.14;
                self.frost_blur_px = 24.0;
                self.bevel_px = 18.0;
                self.specular_rim = 0.60;
                self.scrim_alpha = 0.42;
            }
        }
    }
}

fn default_ui_zoom() -> f32 {
    1.0
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
    pub material: LiquidGlassConfig,
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
            corner_radius_px: 10.0,
            track_row_height_px: 52.0,
            panel_opacity: 1.0,
            adaptive_album_art: true,
            zero_gpu_minimize_saver: true,
            sync_theme_to_android: true,
            ui_zoom: 1.0,
            material: LiquidGlassConfig::default(),
        }
    }
}

impl ThemeConfig {
    pub fn pure_amoled() -> Self {
        Self::default()
    }

    pub fn obsidian_slate() -> Self {
        Self {
            name: "Obsidian Studio".to_string(),
            canvas_hex: "#0D0E12".to_string(),
            surface_hex: "#14151C".to_string(),
            border_hex: "#232530".to_string(),
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
            adaptive_album_art: false,
            ..Self::default()
        }
    }

    pub fn smoked_copper_glass() -> Self {
        let mut material = LiquidGlassConfig::default();
        material.apply_preset(LiquidGlassPreset::PrismaticLiquid);
        Self {
            name: "Ferric Liquid Glass".to_string(),
            canvas_hex: "#040508".to_string(),
            surface_hex: "#0E1018".to_string(),
            border_hex: "#2B2F42".to_string(),
            corner_radius_px: 16.0,
            material,
            ..Self::default()
        }
    }

    pub fn text_contrast_ratio(&self) -> f64 {
        let fg = parse_hex_rgb(&self.text_primary_hex).unwrap_or((243, 238, 227));
        let bg = parse_hex_rgb(&self.canvas_hex).unwrap_or((0, 0, 0));
        wcag_contrast_ratio(fg, bg)
    }

    pub fn accent_contrast_ratio(&self) -> f64 {
        let fg = parse_hex_rgb(&self.accent_hex).unwrap_or((217, 84, 30));
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
        assert!(!theme.material.liquid_glass);
        assert!(theme.text_contrast_ratio() > 17.0);
        assert!(theme.accent_contrast_ratio() > 5.0);
    }

    #[test]
    fn liquid_glass_theme_roundtrips_json() {
        let mut theme = ThemeConfig::default();
        theme
            .material
            .apply_preset(LiquidGlassPreset::PrismaticLiquid);
        let json = serde_json::to_string(&theme).expect("serialize theme");
        let parsed: ThemeConfig = serde_json::from_str(&json).expect("deserialize theme");
        assert!(parsed.material.liquid_glass);
        assert!((parsed.material.ior - 1.48).abs() < 1e-4);
        assert!((parsed.material.dispersion - 0.42).abs() < 1e-4);
    }
}
