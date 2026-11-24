use crate::shell::{launch_verified_installer, open_github_url};
use crate::smtc::{SmtcBridge, SmtcCommand};
use crate::track_list::{TrackListAction, TrackListScope};
use ferric_audio::engine::{AudioEngine, EngineCommand};
use ferric_core::app_data_dir;
use ferric_core::db::LibraryDb;
use ferric_core::eq::EqConfig;
use ferric_core::models::{AccountSettings, Track, UpdateSettings, format_duration_ms};
use ferric_core::queue::PlayQueue;
use ferric_core::theme::{LiquidGlassPreset, ThemeConfig, hex_to_rgba_f32};
use ferric_providers::local_scanner::scan_local_folder;
use ferric_providers::lyrics::{SyncedLyrics, fetch_lrclib_synced, read_local_sidecar_lrc};
use ferric_providers::updater::{
    DownloadEvent, ReleaseInfo, check_latest_release, download_verified_installer,
    format_check_status, release_page_url,
};
use ferric_sync::{ActiveSpeakerLease, PairingPayload};
use makepad_widgets::*;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const BRAND_ICON_PNG: &[u8] = include_bytes!("../resources/ferric.png");

live_design! {
    use link::theme::*;
    use link::shaders::*;
    use link::widgets::*;
    use crate::track_list::TrackList;

    // Reusable GPU SDF Surface:
    // - glass_enabled = 0.0: Strict 3-Color Pure AMOLED (#000000/#0A0A0A + 1px #222222 hairline)
    // - glass_enabled = 1.0: Optical Liquid Glass (convex SDF bevel refraction tint,
    //   RGB chromatic dispersion caustics along rim, Fresnel specular top-left/bottom-right catchlight)
    FerricGlassPanel = <View> {
        show_bg: true,
        draw_bg: {
            uniform base_col: #0A0A0A
            uniform border_col: #222222
            uniform accent_col: #D9541E
            uniform glass_enabled: 0.0
            uniform glass_ior: 1.48
            uniform glass_disp: 0.42
            uniform glass_rim: 0.78
            uniform glass_scrim: 0.28
            uniform radius: 14.0

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                let r = min(self.radius, min(self.rect_size.x, self.rect_size.y) * 0.25);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, r);

                if self.glass_enabled < 0.5 {
                    sdf.fill_keep(self.base_col);
                    sdf.stroke(self.border_col, 1.0);
                    return sdf.result;
                }

                // Analytic convex SDF normal and bevel depth for optical Liquid Glass mode
                let center = self.rect_size * 0.5;
                let rel = p - center;
                let half_b = max(center - vec2(r + 1.0, r + 1.0), vec2(1.0, 1.0));
                let q = abs(rel) - half_b;
                let sd = length(max(q, vec2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
                let depth = max(-sd, 0.0);
                let bevel = 24.0;
                let edge_t = clamp(1.0 - depth / bevel, 0.0, 1.0);
                let slope = edge_t * edge_t * (2.2 - 0.8 * edge_t);

                let n = normalize(rel / max(center, vec2(1.0, 1.0)) + vec2(0.0001, 0.0001));
                let ang = atan(n.y, n.x);

                // Warm copper + ambient refractive interior glow
                let diag = clamp(1.0 - (self.pos.x * 0.65 + self.pos.y * 0.85), 0.0, 1.0);
                let warm_glow = self.accent_col.xyz * (0.14 * diag + 0.10 * slope);
                let cool_glow = vec3(0.16, 0.30, 0.62) * (0.09 * (1.0 - diag) + 0.08 * slope);

                // Prismatic RGB chromatic dispersion along curved glass bevel edge
                let disp_amt = pow(edge_t, 2.2) * self.glass_disp * clamp(self.glass_ior - 1.0, 0.1, 1.0) * 1.15;
                let prism = vec3(
                    0.55 + 0.45 * cos(ang - 0.55),
                    0.45 + 0.55 * cos(ang - 2.55),
                    0.55 + 0.45 * cos(ang + 1.45)
                ) * disp_amt;

                // Fresnel rim sheen & directional specular top-left / bottom-right caustic catchlight
                let tl = clamp(-(n.x * 0.72 + n.y * 0.70), 0.0, 1.0);
                let br = clamp((n.x * 0.70 + n.y * 0.72), 0.0, 1.0);
                let rim_band = clamp(1.0 - depth / 2.4, 0.0, 1.0);
                let spec_tl = rim_band * (0.34 + 0.85 * pow(tl, 1.5)) * self.glass_rim;
                let spec_br = rim_band * (0.18 + 0.58 * pow(br, 1.6)) * self.glass_rim;
                let fresnel = pow(edge_t, 4.0) * 0.28 * self.glass_rim;

                let base_rgb = mix(self.base_col.xyz * 0.65 + vec3(0.03, 0.03, 0.045), vec3(0.02, 0.02, 0.03), self.glass_scrim);
                let rgb = base_rgb
                    + warm_glow
                    + cool_glow
                    + prism
                    + vec3(0.96, 0.94, 0.90) * (fresnel + spec_tl + diag * 0.055)
                    + vec3(0.92, 0.60, 0.42) * spec_br;

                sdf.fill(vec4(clamp(rgb, 0.0, 1.0), 1.0));
                return sdf.result;
            }
        }
    }

    FerricNavBtn = <Button> {
        width: Fill,
        height: 38,
        padding: { left: 14, right: 12 }
        align: { x: 0.0, y: 0.5 }
        draw_text: {
            color: #ABA59B,
            color_hover: #F3EEE3,
            color_down: #F3EEE3,
            text_style: { font_size: 13.0 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            uniform active_bg: #18100C
            uniform active_border: #D9541E
            uniform hover_bg: #141414
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 9.0);
                let col = mix(mix(vec4(0.0,0.0,0.0,0.0), self.hover_bg, self.hover), self.active_bg, self.active);
                sdf.fill_keep(col);
                sdf.stroke(mix(vec4(0.0,0.0,0.0,0.0), self.active_border, self.active * 0.8), 1.0);
                return sdf.result;
            }
        }
    }

    FerricPrimaryBtn = <Button> {
        width: Fit,
        height: 34,
        padding: { left: 14, right: 14 }
        draw_text: {
            color: #000000,
            color_hover: #000000,
            color_down: #000000,
            text_style: { font_size: 12.0 }
        }
        draw_bg: {
            instance hover: 0.0
            instance down: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 8.0);
                let base = mix(#D9541E, #EB6730, self.hover);
                sdf.fill_keep(mix(base, #B94213, self.down));
                sdf.stroke(vec4(1.0, 0.94, 0.82, 0.35), 1.0);
                return sdf.result;
            }
        }
    }

    FerricGhostBtn = <Button> {
        width: Fit,
        height: 34,
        padding: { left: 13, right: 13 }
        draw_text: {
            color: #F3EEE3,
            color_hover: #FFFFFF,
            color_down: #D9541E,
            text_style: { font_size: 12.0 }
        }
        draw_bg: {
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 9.0);
                sdf.fill_keep(mix(#121212, #1D1D1D, self.hover));
                sdf.stroke(#2A2A2A, 1.0);
                return sdf.result;
            }
        }
    }

    FerricSlider = <SliderMinimal> {
        width: Fill,
        height: 28,
        draw_bg: {
            val_color: #D9541E,
            val_color_hover: #EB6730,
            val_color_drag: #EB6730,
            handle_color: #F3EEE3,
            handle_color_hover: #FFFFFF,
            handle_color_drag: #FFFFFF,
            border_color_1: #222222,
            border_color_2: #222222,
        }
    }

    FerricCheckBox = <CheckBox> {
        draw_text: {
            color: #F3EEE3,
            color_hover: #FFFFFF,
            color_active: #F3EEE3,
            text_style: { font_size: 13.0 }
        }
        draw_bg: {
            color: #141414,
            color_active: #D9541E,
            mark_color_active: #000000,
            border_color_1: #333333,
            border_color_2: #333333,
        }
    }

    App = {{App}} {
        ui: <Root> {
            main_window = <Window> {
                window: { title: "Ferric — Native Rust Hi-Fi Music Player (v0.1.0)", inner_size: vec2(1140, 660) }
                pass: { clear_color: #000000 }
                caption_bar = {
                    draw_bg: { color: #000000 }
                    caption_label = {
                        label = {
                            text: "Ferric — Native Rust Hi-Fi Music Player"
                            draw_text: { color: #888279, text_style: { font_size: 11.0 } }
                        }
                    }
                }
                body = <View> {
                    width: Fill,
                    height: Fill,
                    flow: Down,
                    show_bg: true,
                    draw_bg: {
                        uniform canvas_col: #000000
                        uniform glass_bg: 0.0
                        uniform accent_col: #D9541E
                        fn pixel(self) -> vec4 {
                            if self.glass_bg < 0.5 {
                                return self.canvas_col;
                            }
                            // Subtle atmospheric album ambient field when Liquid Glass is active
                            let d1 = length(self.pos - vec2(0.16, 0.18));
                            let d2 = length(self.pos - vec2(0.84, 0.24));
                            let d3 = length(self.pos - vec2(0.55, 0.92));
                            let g1 = exp(-d1 * d1 * 6.0) * self.accent_col.xyz * 0.34;
                            let g2 = exp(-d2 * d2 * 6.5) * vec3(0.18, 0.34, 0.78) * 0.26;
                            let g3 = exp(-d3 * d3 * 7.0) * vec3(0.75, 0.20, 0.44) * 0.24;
                            return vec4(self.canvas_col.xyz + g1 + g2 + g3, 1.0);
                        }
                    }

                    // 1. TOP HEADER BAR
                    top_bar = <FerricGlassPanel> {
                        width: Fill,
                        height: 54,
                        margin: { left: 10, right: 10, top: 8, bottom: 6 }
                        padding: { left: 14, right: 14 }
                        flow: Right,
                        align: { y: 0.5 }
                        spacing: 12,
                        draw_bg: { radius: 14.0 }

                        brand_icon = <Image> {
                            width: 30,
                            height: 30,
                            fit: Smallest,
                        }
                        <Label> {
                            draw_text: { color: #F3EEE3, text_style: { font_size: 16.0 } }
                            text: "Ferric"
                        }
                        glass_mode_pill = <RoundedView> {
                            width: Fit,
                            height: 22,
                            padding: { left: 8, right: 8 }
                            align: { y: 0.5 }
                            draw_bg: { color: #141414, border_radius: 6.0, border_size: 1.0, border_color: #262626 }
                            glass_pill_lbl = <Label> {
                                draw_text: { color: #ABA59B, text_style: { font_size: 9.5 } }
                                text: "AMOLED 3C"
                            }
                        }

                        search_input = <TextInput> {
                            width: Fill,
                            height: 34,
                            empty_text: "Search local library, artists, albums... (Ctrl+K)"
                            draw_bg: {
                                color: #050505,
                                color_focus: #0B0B0B,
                                border_radius: 9.0,
                                border_size: 1.0,
                                border_color_1: #262626,
                                border_color_1_focus: #D9541E
                            }
                            draw_text: {
                                color: #F3EEE3,
                                color_empty: #888279,
                                text_style: { font_size: 12.5 }
                            }
                        }

                        add_folder_btn = <FerricPrimaryBtn> { text: "+ Add Folder" }
                        rescan_btn = <FerricGhostBtn> { text: "Rescan" }
                        check_updates_top_btn = <FerricGhostBtn> { text: "Check Updates" }
                    }

                    // 2. MAIN 3-PANE WORKSPACE + FLOATING STARTUP UPDATE POPUP OVERLAY
                    workspace_stage = <View> {
                        width: Fill,
                        height: Fill,
                        flow: Overlay,

                        workspace_row = <View> {
                        width: Fill,
                        height: Fill,
                        margin: { left: 10, right: 10, bottom: 8 }
                        flow: Right,
                        spacing: 10,

                        // LEFT SIDEBAR NAVIGATION
                        sidebar_panel = <FerricGlassPanel> {
                            width: 196,
                            height: Fill,
                            padding: 10,
                            flow: Down,
                            spacing: 6,

                            nav_library = <FerricNavBtn> { text: "Library", draw_bg: { active: 1.0 } }
                            nav_eq = <FerricNavBtn> { text: "Equalizer APO" }
                            nav_connect = <FerricNavBtn> { text: "Ferric Connect" }
                            nav_appearance = <FerricNavBtn> { text: "Appearance Studio" }

                            <View> { width: Fill, height: 1, margin: { top: 8, bottom: 8 }, show_bg: true, draw_bg: { color: #222222 } }

                            <Label> {
                                margin: { left: 6, bottom: 4 }
                                draw_text: { color: #888279, text_style: { font_size: 10.0 } }
                                text: "SOURCES & BRIDGE"
                            }
                            source_summary_lbl = <Label> {
                                margin: { left: 6 }
                                width: Fill,
                                draw_text: { color: #ABA59B, text_style: { font_size: 11.5 } }
                                text: "Local Files: 0 tracks\nSpotify PKCE: Ready\nYT Music API v3: Ready"
                            }

                            <View> { width: Fill, height: Fill }

                            telemetry_box = <RoundedView> {
                                width: Fill,
                                height: Fit,
                                padding: 10,
                                flow: Down,
                                spacing: 4,
                                draw_bg: { color: #050505, border_radius: 10.0, border_size: 1.0, border_color: #222222 }
                                <Label> {
                                    draw_text: { color: #888279, text_style: { font_size: 9.5 } }
                                    text: "ENGINE TELEMETRY"
                                }
                                telemetry_lbl = <Label> {
                                    draw_text: { color: #F3EEE3, text_style: { font_size: 10.5 } }
                                    text: "Native Rust · DX11 · 48kHz\nGlass Shader: OFF (0% GPU)"
                                }
                            }
                        }

                        // CENTER PAGE FLIP CONTENT
                        center_panel = <FerricGlassPanel> {
                            width: Fill,
                            height: Fill,
                            padding: 16,
                            flow: Down,

                            pages = <PageFlip> {
                                width: Fill,
                                height: Fill,
                                active_page: page_library,

                                // --- VIEW 1: LIBRARY ---
                                page_library = <View> {
                                    width: Fill,
                                    height: Fill,
                                    flow: Down,
                                    spacing: 12,

                                    hero_row = <View> {
                                        width: Fill,
                                        height: Fit,
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 14,

                                        <View> {
                                            width: Fill,
                                            height: Fit,
                                            flow: Down,
                                            spacing: 4,
                                            <Label> {
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 24.0 } }
                                                text: "Local Lossless Library"
                                            }
                                            library_sub_lbl = <Label> {
                                                draw_text: { color: #ABA59B, text_style: { font_size: 12.0 } }
                                                text: "Pure-Rust Symphonia + Opus Decoder · Click any track to play"
                                            }
                                        }
                                        play_all_btn = <FerricPrimaryBtn> { text: "Play All" }
                                        hero_shuffle_btn = <FerricGhostBtn> { text: "Smart Shuffle" }
                                    }

                                    // Column headers
                                    <View> {
                                        width: Fill,
                                        height: 24,
                                        padding: { left: 14, right: 14 }
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 12,
                                        <Label> { width: 30, draw_text: { color: #888279, text_style: { font_size: 10.0 } }, text: "#" }
                                        <Label> { width: Fill, draw_text: { color: #888279, text_style: { font_size: 10.0 } }, text: "TITLE & ARTIST" }
                                        <Label> { width: 78, align: { x: 0.5 }, draw_text: { color: #888279, text_style: { font_size: 10.0 } }, text: "ORIGIN" }
                                        <Label> { width: 82, align: { x: 0.5 }, draw_text: { color: #888279, text_style: { font_size: 10.0 } }, text: "CODEC" }
                                        <Label> { width: 48, align: { x: 1.0 }, draw_text: { color: #888279, text_style: { font_size: 10.0 } }, text: "TIME" }
                                    }
                                    <View> { width: Fill, height: 1, show_bg: true, draw_bg: { color: #222222 } }

                                    track_list = <TrackList> {
                                        width: Fill,
                                        height: Fill,
                                    }
                                }

                                // --- VIEW 2: EQUALIZER APO ---
                                page_eq = <View> {
                                    width: Fill,
                                    height: Fill,
                                    flow: Down,
                                    spacing: 12,

                                    <View> {
                                        width: Fill,
                                        height: Fit,
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 10,
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 3,
                                            <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 22.0 } }, text: "10-Band Parametric Equalizer APO" }
                                            eq_status_lbl = <Label> { draw_text: { color: #ABA59B, text_style: { font_size: 12.0 } }, text: "Preset: Flat · Biquad DirectForm1 + Soft-Clip Limiter" }
                                        }
                                        eq_enable_cb = <FerricCheckBox> { text: "EQ Enabled" }
                                        eq_import_btn = <FerricGhostBtn> { text: "Import APO .txt" }
                                        eq_export_btn = <FerricGhostBtn> { text: "Export APO .txt" }
                                    }

                                    // Presets row
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        eq_pre_flat = <FerricGhostBtn> { text: "Flat" }
                                        eq_pre_copper = <FerricGhostBtn> { text: "Warm Copper" }
                                        eq_pre_bass = <FerricGhostBtn> { text: "Deep Bass" }
                                        eq_pre_studio = <FerricGhostBtn> { text: "Studio Ref" }
                                        eq_pre_vocal = <FerricGhostBtn> { text: "Vocal Clarity" }
                                    }

                                    // Preamp + 10 band sliders in clean two-column grid
                                    eq_preamp_slider = <FerricSlider> { text: "Preamp (dB)", min: -12.0, max: 12.0, default: 0.0, step: 0.5 }

                                    <View> {
                                        width: Fill, height: Fill, flow: Right, spacing: 20,
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 8,
                                            eq_b0 = <FerricSlider> { text: "31 Hz (LowShelf)", min: -12.0, max: 12.0, default: 0.0, step: 0.5 }
                                            eq_b1 = <FerricSlider> { text: "62 Hz (Sub Bass)", min: -12.0, max: 12.0, default: 0.0, step: 0.5 }
                                            eq_b2 = <FerricSlider> { text: "125 Hz (Bass)", min: -12.0, max: 12.0, default: 0.0, step: 0.5 }
                                            eq_b3 = <FerricSlider> { text: "250 Hz (Warmth)", min: -12.0, max: 12.0, default: 0.0, step: 0.5 }
                                            eq_b4 = <FerricSlider> { text: "500 Hz (Body)", min: -12.0, max: 12.0, default: 0.0, step: 0.5 }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 8,
                                            eq_b5 = <FerricSlider> { text: "1 kHz (Mid)", min: -12.0, max: 12.0, default: 0.0, step: 0.5 }
                                            eq_b6 = <FerricSlider> { text: "2 kHz (Vocal)", min: -12.0, max: 12.0, default: 0.0, step: 0.5 }
                                            eq_b7 = <FerricSlider> { text: "4 kHz (Presence)", min: -12.0, max: 12.0, default: 0.0, step: 0.5 }
                                            eq_b8 = <FerricSlider> { text: "8 kHz (Brilliance)", min: -12.0, max: 12.0, default: 0.0, step: 0.5 }
                                            eq_b9 = <FerricSlider> { text: "16 kHz (Air)", min: -12.0, max: 12.0, default: 0.0, step: 0.5 }
                                        }
                                    }
                                }

                                // --- VIEW 3: FERRIC CONNECT ---
                                page_connect = <View> {
                                    width: Fill,
                                    height: Fill,
                                    flow: Down,
                                    spacing: 14,

                                    <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 22.0 } }, text: "Ferric Connect — Zero-Server P2P Sync" }
                                    <Label> { draw_text: { color: #ABA59B, text_style: { font_size: 12.5 } }, text: "Binary postcard framing over ALPN ferric-connect/v1 · Active Speaker Lease prevents double playback." }

                                    <RoundedView> {
                                        width: Fill, height: Fit, padding: 16, flow: Down, spacing: 6,
                                        draw_bg: { color: #160A05, border_radius: 12.0, border_size: 1.0, border_color: #D9541E }
                                        <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 15.0 } }, text: "Desktop-PC (This Device · Active Speaker)" }
                                        connect_lease_lbl = <Label> { draw_text: { color: #ABA59B, text_style: { font_size: 12.0 } }, text: "Lease Revision #1 · WASAPI Output Active · mDNS Ready" }
                                    }

                                    <RoundedView> {
                                        width: Fill, height: Fit, padding: 16, flow: Down, spacing: 8,
                                        draw_bg: { color: #0D0D0D, border_radius: 12.0, border_size: 1.0, border_color: #242424 }
                                        <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 14.0 } }, text: "Android Pairing Token (120s Ephemeral URI)" }
                                        pair_uri_lbl = <Label> { width: Fill, draw_text: { color: #D9541E, text_style: { font_size: 11.5 } }, text: "Click 'Generate Pair URI' to create a one-time pairing token." }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 10,
                                            gen_pair_btn = <FerricPrimaryBtn> { text: "Generate Pair URI" }
                                        }
                                    }
                                }

                                // --- VIEW 4: OPEN THEME STUDIO, ACCOUNTS & UPDATES ---
                                page_appearance = <ScrollYView> {
                                    width: Fill,
                                    height: Fill,
                                    flow: Down,
                                    spacing: 12,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 2,
                                            <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 22.0 } }, text: "Open Theme Studio & Liquid Glass" }
                                            wcag_lbl = <Label> { draw_text: { color: #ABA59B, text_style: { font_size: 11.5 } }, text: "WCAG Contrast Guard: Text 17.9:1 (AAA) · Accent 5.23:1 (AA)" }
                                        }
                                        theme_import_btn = <FerricGhostBtn> { text: "Import JSON" }
                                        theme_export_btn = <FerricGhostBtn> { text: "Export JSON" }
                                        theme_reset_btn = <FerricGhostBtn> { text: "Reset 3C" }
                                    }

                                    // MASTER LIQUID GLASS ON/OFF SECTION
                                    <RoundedView> {
                                        width: Fill, height: Fit, padding: 14, flow: Down, spacing: 10,
                                        draw_bg: { color: #120B08, border_radius: 12.0, border_size: 1.0, border_color: #D9541E }

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 12,
                                            liquid_glass_cb = <FerricCheckBox> { text: "Liquid Glass Theme (Refractive Optical Shader on All UI Surfaces)" }
                                        }
                                        <Label> {
                                            width: Fill,
                                            draw_text: { color: #ABA59B, text_style: { font_size: 11.5 } }
                                            text: "Default OFF (0% GPU Pure AMOLED). Toggle ON to activate curved SDF bevel refraction, prismatic RGB chromatic edge dispersion & specular catchlights live."
                                        }

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 8,
                                            lg_pre_crystal = <FerricGhostBtn> { text: "Crystal Pure" }
                                            lg_pre_prism = <FerricGhostBtn> { text: "Prismatic Liquid" }
                                            lg_pre_copper = <FerricGhostBtn> { text: "Smoked Copper" }
                                            lg_pre_frost = <FerricGhostBtn> { text: "Ultra Frosted" }
                                        }

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 16,
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 6,
                                                lg_ior_slider = <FerricSlider> { text: "Refraction Index (IOR)", min: 1.05, max: 2.0, default: 1.48, step: 0.02 }
                                                lg_rim_slider = <FerricSlider> { text: "Specular Rim Catchlight", min: 0.0, max: 1.0, default: 0.78, step: 0.02 }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 6,
                                                lg_disp_slider = <FerricSlider> { text: "Chromatic Edge Dispersion", min: 0.0, max: 1.0, default: 0.42, step: 0.02 }
                                                lg_rad_slider = <FerricSlider> { text: "Corner Radius (px)", min: 4.0, max: 26.0, default: 14.0, step: 1.0 }
                                            }
                                        }
                                    }

                                    // PALETTE & ACCOUNTS
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8, align: { y: 0.5 },
                                        <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 13.0 } }, text: "Palette Presets:" }
                                        pal_amoled_btn = <FerricGhostBtn> { text: "AMOLED 3-Color" }
                                        pal_glass_btn = <FerricGhostBtn> { text: "Ferric Liquid Glass" }
                                        pal_cream_btn = <FerricGhostBtn> { text: "Cream Light" }
                                    }

                                    <RoundedView> {
                                        width: Fill, height: Fit, padding: 12, flow: Down, spacing: 8,
                                        draw_bg: { color: #0B0B0B, border_radius: 12.0, border_size: 1.0, border_color: #222222 }
                                        <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Multi-Account Catalog Sync (Official OAuth 2.0 PKCE — Metadata Only)" }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 10,
                                            spotify_id_input = <TextInput> {
                                                width: Fill, height: 32,
                                                empty_text: "Spotify Client ID (BYO OAuth 2.0 PKCE)"
                                                draw_bg: { color: #050505, border_radius: 7.0, border_size: 1.0, border_color_1: #262626 }
                                                draw_text: { color: #F3EEE3, color_empty: #888279, text_style: { font_size: 11.5 } }
                                            }
                                            yt_id_input = <TextInput> {
                                                width: Fill, height: 32,
                                                empty_text: "Google / YouTube Data API v3 Client ID"
                                                draw_bg: { color: #050505, border_radius: 7.0, border_size: 1.0, border_color_1: #262626 }
                                                draw_text: { color: #F3EEE3, color_empty: #888279, text_style: { font_size: 11.5 } }
                                            }
                                            save_accounts_btn = <FerricPrimaryBtn> { text: "Save IDs" }
                                        }
                                        yt_cat_cb = <FerricCheckBox> { text: "YouTube Music Category Filter (categoryId = 10 Music Only)" }
                                    }

                                    // SOFTWARE UPDATES (GITHUB RELEASES · SHA-256 VERIFIED MSI)
                                    <RoundedView> {
                                        width: Fill, height: Fit, padding: 14, flow: Down, spacing: 10,
                                        draw_bg: { color: #0B0B0B, border_radius: 12.0, border_size: 1.0, border_color: #222222 }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 2,
                                                <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Software Updates (GitHub Releases · SHA-256 Verified MSI)" }
                                                update_status_lbl = <Label> {
                                                    width: Fill,
                                                    draw_text: { color: #ABA59B, text_style: { font_size: 11.5 } }
                                                    text: "Ferric v0.1.0 · Checks official GitHub releases; never downloads without your click."
                                                }
                                            }
                                            check_updates_btn = <FerricPrimaryBtn> { text: "Check for Updates" }
                                            open_releases_btn = <FerricGhostBtn> { text: "GitHub Releases" }
                                        }
                                        auto_update_cb = <FerricCheckBox> { text: "Check for updates automatically on startup (show update popup when a new version is out)" }
                                    }
                                }
                            }
                        }

                        // RIGHT SIDEBAR: UP NEXT + SYNCED LYRICS
                        right_panel = <FerricGlassPanel> {
                            width: 244,
                            height: Fill,
                            padding: 12,
                            flow: Down,
                            spacing: 10,

                            <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 14.5 } }, text: "Up Next" }
                            up_next_lbl = <Label> {
                                width: Fill,
                                draw_text: { color: #ABA59B, text_style: { font_size: 11.5 } }
                                text: "Queue is empty.\nSelect any track to start listening."
                            }

                            <View> { width: Fill, height: 1, show_bg: true, draw_bg: { color: #222222 } }

                            <Label> { draw_text: { color: #888279, text_style: { font_size: 10.0 } }, text: "SYNCED LYRICS (LOCAL .LRC / LRCLIB)" }
                            lyrics_box = <RoundedView> {
                                width: Fill,
                                height: Fill,
                                padding: 12,
                                flow: Down,
                                spacing: 10,
                                align: { y: 0.5 }
                                draw_bg: { color: #050505, border_radius: 10.0, border_size: 1.0, border_color: #202020 }

                                lyric_prev_lbl = <Label> {
                                    width: Fill,
                                    draw_text: { color: #888279, text_style: { font_size: 12.5 } }
                                    text: ""
                                }
                                lyric_active_lbl = <Label> {
                                    width: Fill,
                                    draw_text: { color: #D9541E, text_style: { font_size: 15.0 } }
                                    text: "No synced lyrics loaded."
                                }
                                lyric_next_lbl = <Label> {
                                    width: Fill,
                                    draw_text: { color: #ABA59B, text_style: { font_size: 12.5 } }
                                    text: ""
                                }
                            }
                        }
                        }

                        // FLOATING UPDATE POPUP CARD (AUTO-SHOWN ON STARTUP / MANUAL CHECK WHEN NEWER VERSION EXISTS)
                        update_popup_anchor = <View> {
                            width: Fill,
                            height: Fill,
                            visible: false,
                            align: { x: 1.0, y: 0.0 }
                            padding: { top: 10, right: 20 }

                            update_popup_card = <FerricGlassPanel> {
                                width: 420,
                                height: Fit,
                                padding: 16,
                                flow: Down,
                                spacing: 10,
                                draw_bg: { radius: 16.0 }

                                <View> {
                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                    <RoundedView> {
                                        width: Fit, height: 22, padding: { left: 8, right: 8 }, align: { y: 0.5 },
                                        draw_bg: { color: #2B1208, border_radius: 6.0, border_size: 1.0, border_color: #D9541E }
                                        <Label> {
                                            draw_text: { color: #D9541E, text_style: { font_size: 9.5 } }
                                            text: "UPDATE AVAILABLE"
                                        }
                                    }
                                    <View> { width: Fill, height: 1 }
                                    update_popup_close_btn = <FerricGhostBtn> { height: 26, text: "Later" }
                                }
                                update_popup_title = <Label> {
                                    width: Fill,
                                    draw_text: { color: #F3EEE3, text_style: { font_size: 16.0 } }
                                    text: "Ferric update is out"
                                }
                                update_popup_sub = <Label> {
                                    width: Fill,
                                    draw_text: { color: #ABA59B, text_style: { font_size: 12.0 } }
                                    text: "Read what changed, or update now."
                                }
                                <View> {
                                    width: Fill, height: Fit, flow: Right, spacing: 8, align: { y: 0.5 },
                                    update_now_btn = <FerricPrimaryBtn> { text: "Update Now" }
                                    update_whats_new_btn = <FerricGhostBtn> { text: "What's New" }
                                }
                                <View> { width: Fill, height: 1, show_bg: true, draw_bg: { color: #222222 } }
                                <View> {
                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                    update_disable_btn = <FerricGhostBtn> { height: 26, text: "Don't check on startup" }
                                    <View> { width: Fill, height: 1 }
                                    <Label> {
                                        draw_text: { color: #888279, text_style: { font_size: 10.0 } }
                                        text: "SHA-256 Verified MSI"
                                    }
                                }
                            }
                        }
                    }

                    // 3. FLOATING BOTTOM PLAYER DOCK
                    player_dock = <FerricGlassPanel> {
                        width: Fill,
                        height: 76,
                        margin: { left: 10, right: 10, bottom: 8 }
                        padding: { left: 14, right: 14 }
                        flow: Right,
                        align: { y: 0.5 }
                        spacing: 12,
                        draw_bg: { radius: 14.0 }

                        // Left: Active Track Metadata
                        <View> {
                            width: 210,
                            height: Fit,
                            flow: Down,
                            spacing: 3,
                            np_title_lbl = <Label> {
                                width: Fill,
                                draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }
                                text: "Not Playing"
                            }
                            np_artist_lbl = <Label> {
                                width: Fill,
                                draw_text: { color: #ABA59B, text_style: { font_size: 11.5 } }
                                text: "Add a music folder to start"
                            }
                        }

                        // Center: Transport Controls + Seekbar
                        <View> {
                            width: Fill,
                            height: Fit,
                            flow: Down,
                            align: { x: 0.5 }
                            spacing: 4,

                            <View> {
                                width: Fit, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                shuffle_btn = <FerricGhostBtn> { text: "Shuffle: Off" }
                                prev_btn = <FerricGhostBtn> { text: "Prev" }
                                play_pause_btn = <FerricPrimaryBtn> { text: "Play" }
                                next_btn = <FerricGhostBtn> { text: "Next" }
                                repeat_btn = <FerricGhostBtn> { text: "Repeat: Off" }
                            }

                            <View> {
                                width: Fill, height: 26, flow: Right, align: { y: 0.5 }, spacing: 10,
                                pos_lbl = <Label> { width: 46, draw_text: { color: #ABA59B, text_style: { font_size: 11.0 } }, text: "00:00" }
                                seek_slider = <FerricSlider> { width: Fill, text: "", min: 0.0, max: 1.0, default: 0.0 }
                                dur_total_lbl = <Label> { width: 46, align: { x: 1.0 }, draw_text: { color: #ABA59B, text_style: { font_size: 11.0 } }, text: "00:00" }
                            }
                        }

                        // Right: Codec Badge + Volume
                        <View> {
                            width: 220,
                            height: Fit,
                            flow: Down,
                            align: { x: 1.0 }
                            spacing: 4,
                            codec_badge_lbl = <Label> {
                                draw_text: { color: #D9541E, text_style: { font_size: 10.5 } }
                                text: "WASAPI · IDLE"
                            }
                            vol_slider = <FerricSlider> { width: 180, text: "Vol", min: 0.0, max: 1.0, default: 0.85 }
                        }
                    }
                }
            }
        }
    }
}

app_main!(App);

enum BgMessage {
    ScannedTracks(Vec<Track>),
    LyricsLoaded {
        track_id: String,
        raw_lrc: String,
    },
    UpdateCheckDone {
        manual: bool,
        result: Result<Option<ReleaseInfo>, String>,
    },
    UpdateDownload(DownloadEvent),
}

#[derive(Live, LiveHook)]
pub struct App {
    #[live]
    ui: WidgetRef,
    #[rust]
    db: Option<LibraryDb>,
    #[rust]
    audio: Option<AudioEngine>,
    #[rust]
    smtc: Option<SmtcBridge>,
    #[rust]
    queue: PlayQueue,
    #[rust]
    all_tracks: Vec<Track>,
    #[rust]
    filtered_tracks: Rc<Vec<Track>>,
    #[rust]
    search_query: String,
    #[rust]
    theme: ThemeConfig,
    #[rust]
    eq: EqConfig,
    #[rust]
    accounts: AccountSettings,
    #[rust]
    update_settings: UpdateSettings,
    #[rust]
    available_update: Option<ReleaseInfo>,
    #[rust]
    update_popup_visible: bool,
    #[rust]
    update_check_in_flight: bool,
    #[rust]
    update_download_in_flight: bool,
    #[rust]
    update_cancel: Option<Arc<AtomicBool>>,
    #[rust]
    lease: Option<ActiveSpeakerLease>,
    #[rust]
    synced_lyrics: SyncedLyrics,
    #[rust]
    last_finished_seq: u64,
    #[rust]
    tick_timer: Timer,
    #[rust]
    bg_rx: Option<Receiver<BgMessage>>,
    #[rust]
    bg_tx: Option<mpsc::Sender<BgMessage>>,
    #[rust]
    initialized: bool,
}

impl LiveRegister for App {
    fn live_register(cx: &mut Cx) {
        makepad_widgets::live_design(cx);
        crate::theme::live_design(cx);
        cx.link(live_id!(theme), live_id!(ferric_theme));
        crate::track_list::live_design(cx);
    }
}

impl App {
    fn ensure_initialized(&mut self, cx: &mut Cx) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        let db_path = app_data_dir().join("ferric.db");
        let db = LibraryDb::open(&db_path)
            .or_else(|_| LibraryDb::open_in_memory())
            .ok();

        if let Some(store) = &db {
            self.theme = store.load_theme().unwrap_or_default();
            self.eq = store.load_eq().unwrap_or_default();
            self.accounts = store.load_accounts().unwrap_or_default();
            self.update_settings = store.load_update_settings().unwrap_or_default();
            self.all_tracks = store.list_tracks("").unwrap_or_default();
        }
        self.filtered_tracks = Rc::new(self.all_tracks.clone());
        self.queue.set_tracks(self.all_tracks.clone(), None);
        self.db = db;

        let audio = AudioEngine::start(self.eq.clone())
            .unwrap_or_else(|_| AudioEngine::start_headless(self.eq.clone()));
        self.audio = Some(audio);
        self.smtc = Some(SmtcBridge::try_init());
        self.lease = Some(ActiveSpeakerLease::new("desktop-pc", "Desktop-PC"));

        let (tx, rx) = mpsc::channel();
        self.bg_tx = Some(tx);
        self.bg_rx = Some(rx);

        // Adaptive 4 Hz progress & background polling timer (PRD: zero 60fps idle drain).
        self.tick_timer = cx.start_interval(0.25);

        let _ = self
            .ui
            .image(id!(brand_icon))
            .load_png_from_data(cx, BRAND_ICON_PNG);

        self.sync_controls_from_state(cx);
        self.apply_theme_live(cx);
        self.refresh_sidebars(cx);

        // Automatic background update check on launch (displays Update Popup if newer release exists).
        if self.update_settings.auto_check_on_startup {
            self.spawn_update_check(cx, false);
        }
    }

    fn sync_controls_from_state(&self, cx: &mut Cx) {
        self.ui
            .check_box(id!(liquid_glass_cb))
            .set_active(cx, self.theme.material.liquid_glass);
        self.ui
            .slider(id!(lg_ior_slider))
            .set_value(cx, f64::from(self.theme.material.ior));
        self.ui
            .slider(id!(lg_disp_slider))
            .set_value(cx, f64::from(self.theme.material.dispersion));
        self.ui
            .slider(id!(lg_rim_slider))
            .set_value(cx, f64::from(self.theme.material.specular_rim));
        self.ui
            .slider(id!(lg_rad_slider))
            .set_value(cx, f64::from(self.theme.corner_radius_px));

        self.ui
            .check_box(id!(eq_enable_cb))
            .set_active(cx, self.eq.enabled);
        self.ui
            .slider(id!(eq_preamp_slider))
            .set_value(cx, f64::from(self.eq.preamp_db));
        let band_ids = eq_band_slider_ids();
        for (idx, slider_id) in band_ids.iter().enumerate() {
            if let Some(band) = self.eq.bands.get(idx) {
                self.ui
                    .slider(slider_id)
                    .set_value(cx, f64::from(band.gain_db));
            }
        }
        self.ui.label(id!(eq_status_lbl)).set_text(
            cx,
            &format!(
                "Preset: {} · Biquad DirectForm1 + Soft-Clip Limiter",
                self.eq.preset_name
            ),
        );

        self.ui
            .text_input(id!(spotify_id_input))
            .set_text(cx, &self.accounts.spotify_client_id);
        self.ui
            .text_input(id!(yt_id_input))
            .set_text(cx, &self.accounts.youtube_client_id);
        self.ui
            .check_box(id!(yt_cat_cb))
            .set_active(cx, self.accounts.youtube_music_category_only);
        self.ui
            .check_box(id!(auto_update_cb))
            .set_active(cx, self.update_settings.auto_check_on_startup);
    }

    fn apply_theme_live(&self, cx: &mut Cx) {
        let g = &self.theme.material;
        let glass_flag = if g.liquid_glass { 1.0_f64 } else { 0.0_f64 };
        let canvas = rgba_vec4(&self.theme.canvas_hex);
        let surface = rgba_vec4(&self.theme.surface_hex);
        let border = rgba_vec4(&self.theme.border_hex);
        let accent = rgba_vec4(&self.theme.accent_hex);

        self.ui.view(id!(body)).apply_over(
            cx,
            live! {
                draw_bg: {
                    canvas_col: (canvas),
                    glass_bg: (glass_flag),
                    accent_col: (accent)
                }
            },
        );

        for panel_id in [
            id!(top_bar),
            id!(sidebar_panel),
            id!(center_panel),
            id!(right_panel),
            id!(player_dock),
            id!(update_popup_card),
        ] {
            self.ui.view(panel_id).apply_over(
                cx,
                live! {
                    draw_bg: {
                        base_col: (surface),
                        border_col: (border),
                        accent_col: (accent),
                        glass_enabled: (glass_flag),
                        glass_ior: (f64::from(g.ior)),
                        glass_disp: (f64::from(g.dispersion)),
                        glass_rim: (f64::from(g.specular_rim)),
                        glass_scrim: (f64::from(g.scrim_alpha)),
                        radius: (f64::from(self.theme.corner_radius_px))
                    }
                },
            );
        }

        let badge_txt = if g.liquid_glass {
            "LIQUID GLASS ON"
        } else {
            "AMOLED 3C"
        };
        self.ui.label(id!(glass_pill_lbl)).set_text(cx, badge_txt);

        let t_ratio = self.theme.text_contrast_ratio();
        let a_ratio = self.theme.accent_contrast_ratio();
        let pass_t = if t_ratio >= 7.0 {
            "AAA"
        } else if t_ratio >= 4.5 {
            "AA"
        } else {
            "LOW"
        };
        let pass_a = if a_ratio >= 4.5 { "AA" } else { "LOW" };
        self.ui.label(id!(wcag_lbl)).set_text(
            cx,
            &format!("WCAG Contrast Guard: Text {t_ratio:.1}:1 ({pass_t}) · Accent {a_ratio:.2}:1 ({pass_a})"),
        );

        let out_rate = self
            .audio
            .as_ref()
            .map_or(48_000, |a| a.telemetry().output_rate);
        let glass_line = if g.liquid_glass {
            format!(
                "Glass ON (IOR {:.2}, Disp {:.0}%)",
                g.ior,
                g.dispersion * 100.0
            )
        } else {
            "Glass OFF (0% GPU AMOLED)".to_string()
        };
        self.ui.label(id!(telemetry_lbl)).set_text(
            cx,
            &format!(
                "Native Rust · DX11 · {:.1}kHz\n{}",
                out_rate as f32 / 1000.0,
                glass_line
            ),
        );

        if let Some(db) = &self.db {
            let _ = db.save_theme(&self.theme);
        }
        self.ui.redraw(cx);
    }

    fn refresh_sidebars(&self, cx: &mut Cx) {
        let total = self.all_tracks.len();
        self.ui.label(id!(library_sub_lbl)).set_text(
            cx,
            &format!(
                "{total} local tracks indexed · Symphonia + Pure-Rust Opus · Click any row to play"
            ),
        );
        self.ui.label(id!(source_summary_lbl)).set_text(
            cx,
            &format!(
                "Local Files: {total} tracks\nSpotify PKCE: {}\nYT Music API v3: {}",
                if self.accounts.spotify_client_id.is_empty() {
                    "Unconfigured"
                } else {
                    "Client ID Saved"
                },
                if self.accounts.youtube_client_id.is_empty() {
                    "Unconfigured"
                } else {
                    "Client ID Saved"
                },
            ),
        );

        let upcoming = self.queue.peek_upcoming(5);
        if upcoming.is_empty() {
            self.ui
                .label(id!(up_next_lbl))
                .set_text(cx, "No upcoming tracks.\nEnable Repeat or add a folder.");
        } else {
            let text = upcoming
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{}. {} - {}", i + 1, t.title, t.artist))
                .collect::<Vec<_>>()
                .join("\n");
            self.ui.label(id!(up_next_lbl)).set_text(cx, &text);
        }
    }

    fn play_track(&mut self, cx: &mut Cx, track: Track) {
        if let Some(path) = &track.path
            && let Some(audio) = &self.audio
        {
            audio.send(EngineCommand::LoadAndPlay(path.clone()));
            let next_path = self
                .queue
                .peek_upcoming(1)
                .first()
                .and_then(|t| t.path.clone());
            audio.send(EngineCommand::QueueNext(next_path));
        }
        self.ui.label(id!(np_title_lbl)).set_text(cx, &track.title);
        self.ui
            .label(id!(np_artist_lbl))
            .set_text(cx, &format!("{} · {}", track.artist, track.album));
        self.ui
            .label(id!(codec_badge_lbl))
            .set_text(cx, &track.telemetry_label());
        self.ui.button(id!(play_pause_btn)).set_text(cx, "Pause");

        if let Some(smtc) = &mut self.smtc {
            smtc.try_attach_late();
            smtc.set_metadata(&track.title, &track.artist, &track.album, track.duration_ms);
            smtc.set_playing(true);
        }

        self.load_lyrics_for_track(cx, &track);
        self.refresh_sidebars(cx);
        self.ui.redraw(cx);
    }

    fn load_lyrics_for_track(&mut self, cx: &mut Cx, track: &Track) {
        self.synced_lyrics = SyncedLyrics::default();
        self.ui.label(id!(lyric_prev_lbl)).set_text(cx, "");
        self.ui
            .label(id!(lyric_active_lbl))
            .set_text(cx, "Loading synced lyrics...");
        self.ui.label(id!(lyric_next_lbl)).set_text(cx, "");

        // 1. Check local sidecar `.lrc` first (0 network)
        if let Some(path) = &track.path
            && let Some(lrc) = read_local_sidecar_lrc(path)
        {
            self.synced_lyrics = SyncedLyrics::parse_lrc(&lrc);
            self.update_lyrics_display(cx, 0);
            return;
        }

        // 2. Check SQLite lyrics cache
        if let Some(db) = &self.db
            && let Ok(Some(cached)) = db.get_cached_lyrics(&track.id)
        {
            self.synced_lyrics = SyncedLyrics::parse_lrc(&cached);
            self.update_lyrics_display(cx, 0);
            return;
        }

        // 3. Fetch asynchronously from open LRCLIB database on a detached worker thread
        if let Some(tx) = &self.bg_tx {
            let tx = tx.clone();
            let track_id = track.id.clone();
            let artist = track.artist.clone();
            let title = track.title.clone();
            let album = track.album.clone();
            let dur_secs = track.duration_ms / 1000;
            thread::spawn(move || {
                if let Ok(Some(raw_lrc)) = fetch_lrclib_synced(&artist, &title, &album, dur_secs) {
                    let _ = tx.send(BgMessage::LyricsLoaded { track_id, raw_lrc });
                }
            });
        }
    }

    fn update_lyrics_display(&self, cx: &mut Cx, pos_ms: u64) {
        if self.synced_lyrics.lines.is_empty() {
            return;
        }
        let (prev, active, next) = self.synced_lyrics.active_window(pos_ms);
        self.ui
            .label(id!(lyric_prev_lbl))
            .set_text(cx, prev.unwrap_or(""));
        self.ui
            .label(id!(lyric_active_lbl))
            .set_text(cx, active.unwrap_or("..."));
        self.ui
            .label(id!(lyric_next_lbl))
            .set_text(cx, next.unwrap_or(""));
    }

    fn spawn_folder_scan(&self, folders: Vec<PathBuf>) {
        if let Some(tx) = &self.bg_tx {
            let tx = tx.clone();
            thread::spawn(move || {
                let mut found = Vec::new();
                for dir in folders {
                    found.extend(scan_local_folder(&dir));
                }
                let _ = tx.send(BgMessage::ScannedTracks(found));
            });
        }
    }

    fn apply_search_filter(&mut self, cx: &mut Cx) {
        if let Some(db) = &self.db
            && let Ok(rows) = db.list_tracks(&self.search_query)
        {
            self.filtered_tracks = Rc::new(rows);
            self.ui.redraw(cx);
            return;
        }
        let q = self.search_query.trim().to_ascii_lowercase();
        if q.is_empty() {
            self.filtered_tracks = Rc::new(self.all_tracks.clone());
        } else {
            self.filtered_tracks = Rc::new(
                self.all_tracks
                    .iter()
                    .filter(|t| {
                        t.title.to_ascii_lowercase().contains(&q)
                            || t.artist.to_ascii_lowercase().contains(&q)
                            || t.album.to_ascii_lowercase().contains(&q)
                    })
                    .cloned()
                    .collect(),
            );
        }
        self.ui.redraw(cx);
    }

    fn switch_page(&self, cx: &mut Cx, page_id: LiveId) {
        self.ui.page_flip(id!(pages)).set_active_page(cx, page_id);
        let navs = [
            (id!(nav_library), live_id!(page_library)),
            (id!(nav_eq), live_id!(page_eq)),
            (id!(nav_connect), live_id!(page_connect)),
            (id!(nav_appearance), live_id!(page_appearance)),
        ];
        for (btn, pid) in navs {
            self.ui.button(btn).apply_over(
                cx,
                live! { draw_bg: { active: (if pid == page_id { 1.0 } else { 0.0 }) } },
            );
        }
    }

    fn apply_eq_live(&mut self, cx: &mut Cx) {
        if let Some(audio) = &self.audio {
            audio.send(EngineCommand::SetEq(self.eq.clone()));
        }
        if let Some(db) = &self.db {
            let _ = db.save_eq(&self.eq);
        }
        self.ui.label(id!(eq_status_lbl)).set_text(
            cx,
            &format!(
                "Preset: {} · {} · Preamp {:+.1} dB",
                self.eq.preset_name,
                if self.eq.enabled {
                    "Active"
                } else {
                    "Bypassed"
                },
                self.eq.preamp_db
            ),
        );
    }

    fn spawn_update_check(&mut self, cx: &mut Cx, manual: bool) {
        if self.update_check_in_flight {
            return;
        }
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.update_check_in_flight = true;
        let cancel = Arc::new(AtomicBool::new(false));
        self.update_cancel = Some(Arc::clone(&cancel));
        if manual {
            self.ui
                .button(id!(check_updates_btn))
                .set_text(cx, "Checking...");
            self.ui
                .button(id!(check_updates_top_btn))
                .set_text(cx, "Checking...");
            self.ui
                .label(id!(update_status_lbl))
                .set_text(cx, "Checking GitHub Releases for updates...");
        }
        thread::spawn(move || {
            let result = check_latest_release(APP_VERSION, &cancel);
            let _ = tx.send(BgMessage::UpdateCheckDone { manual, result });
        });
    }

    fn show_update_popup(&mut self, cx: &mut Cx, info: &ReleaseInfo) {
        self.update_popup_visible = true;
        self.ui
            .label(id!(update_popup_title))
            .set_text(cx, &format!("Ferric {} is out", info.version));
        self.ui
            .label(id!(update_popup_sub))
            .set_text(cx, &info.summary);
        self.ui
            .button(id!(update_now_btn))
            .set_text(cx, "Update Now");
        self.ui.view(id!(update_popup_anchor)).set_visible(cx, true);
        self.ui.redraw(cx);
    }

    fn dismiss_update_popup(&mut self, cx: &mut Cx) {
        if let Some(flag) = &self.update_cancel
            && self.update_download_in_flight
        {
            flag.store(true, Ordering::Relaxed);
            self.update_download_in_flight = false;
        }
        self.update_popup_visible = false;
        self.ui
            .view(id!(update_popup_anchor))
            .set_visible(cx, false);
        self.ui.redraw(cx);
    }

    fn start_update_download(&mut self, cx: &mut Cx) {
        if self.update_download_in_flight {
            return;
        }
        let Some(info) = self.available_update.clone() else {
            return;
        };
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.update_download_in_flight = true;
        let cancel = Arc::new(AtomicBool::new(false));
        self.update_cancel = Some(Arc::clone(&cancel));
        let updates_dir = app_data_dir().join("updates");

        self.ui
            .button(id!(update_now_btn))
            .set_text(cx, "Downloading...");
        self.ui
            .label(id!(update_popup_sub))
            .set_text(cx, "Downloading verified MSI installer (0%)...");

        thread::spawn(move || {
            let progress_tx = tx.clone();
            let res =
                download_verified_installer(&info.tag, APP_VERSION, &updates_dir, &cancel, |pct| {
                    let ev = if pct == Some(100) {
                        DownloadEvent::Verifying
                    } else {
                        DownloadEvent::Progress(pct)
                    };
                    let _ = progress_tx.send(BgMessage::UpdateDownload(ev));
                });
            match res {
                Ok(path) => {
                    let _ = tx.send(BgMessage::UpdateDownload(DownloadEvent::Ready(path)));
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::UpdateDownload(DownloadEvent::Failed(err)));
                }
            }
        });
    }

    fn handle_tick(&mut self, cx: &mut Cx) {
        // 1. Drain background scanner, lyrics, and updater results
        let mut incoming = Vec::new();
        if let Some(rx) = &self.bg_rx {
            while let Ok(msg) = rx.try_recv() {
                incoming.push(msg);
            }
        }
        for msg in incoming {
            match msg {
                BgMessage::ScannedTracks(new_tracks) => {
                    if let Some(db) = &mut self.db {
                        let _ = db.upsert_tracks(&new_tracks);
                        self.all_tracks = db.list_tracks("").unwrap_or(new_tracks);
                    } else {
                        self.all_tracks = new_tracks;
                    }
                    let anchor = self.queue.current_track_index();
                    self.queue.set_tracks(self.all_tracks.clone(), anchor);
                    self.apply_search_filter(cx);
                    self.refresh_sidebars(cx);
                }
                BgMessage::LyricsLoaded { track_id, raw_lrc } => {
                    if let Some(db) = &self.db {
                        let _ = db.save_cached_lyrics(&track_id, &raw_lrc);
                    }
                    if self.queue.current().map(|t| t.id.as_str()) == Some(track_id.as_str()) {
                        self.synced_lyrics = SyncedLyrics::parse_lrc(&raw_lrc);
                        let pos = self.audio.as_ref().map_or(0, |a| a.telemetry().position_ms);
                        self.update_lyrics_display(cx, pos);
                    }
                }
                BgMessage::UpdateCheckDone { manual, result } => {
                    self.update_check_in_flight = false;
                    self.ui
                        .button(id!(check_updates_btn))
                        .set_text(cx, "Check for Updates");
                    let status_text = format_check_status(&result, APP_VERSION);
                    self.ui
                        .label(id!(update_status_lbl))
                        .set_text(cx, &status_text);
                    match result {
                        Ok(Some(info)) => {
                            self.ui
                                .button(id!(check_updates_top_btn))
                                .set_text(cx, &format!("Update {}", info.tag));
                            self.available_update = Some(info.clone());
                            self.show_update_popup(cx, &info);
                        }
                        Ok(None) => {
                            self.available_update = None;
                            let top_label = if manual {
                                format!("v{APP_VERSION} · Up to Date")
                            } else {
                                "Check Updates".to_string()
                            };
                            self.ui
                                .button(id!(check_updates_top_btn))
                                .set_text(cx, &top_label);
                        }
                        Err(_) => {
                            self.ui
                                .button(id!(check_updates_top_btn))
                                .set_text(cx, "Check Updates");
                            if manual {
                                self.switch_page(cx, live_id!(page_appearance));
                            }
                        }
                    }
                }
                BgMessage::UpdateDownload(event) => match event {
                    DownloadEvent::Progress(pct) => {
                        let msg = match pct {
                            Some(p) => format!("Downloading update installer ({p}%)..."),
                            None => "Downloading update installer...".to_string(),
                        };
                        self.ui.label(id!(update_popup_sub)).set_text(cx, &msg);
                        self.ui
                            .button(id!(update_now_btn))
                            .set_text(cx, "Downloading...");
                    }
                    DownloadEvent::Verifying => {
                        self.ui
                            .button(id!(update_now_btn))
                            .set_text(cx, "Verifying...");
                        self.ui
                            .label(id!(update_popup_sub))
                            .set_text(cx, "Verifying installer SHA-256 checksum...");
                    }
                    DownloadEvent::Ready(msi_path) => {
                        self.update_download_in_flight = false;
                        match launch_verified_installer(&msi_path) {
                            Ok(()) => {
                                self.dismiss_update_popup(cx);
                                cx.quit();
                            }
                            Err(err) => {
                                self.ui.button(id!(update_now_btn)).set_text(cx, "Retry");
                                self.ui.label(id!(update_popup_sub)).set_text(cx, &err);
                                self.ui.label(id!(update_status_lbl)).set_text(cx, &err);
                            }
                        }
                    }
                    DownloadEvent::Failed(err) => {
                        self.update_download_in_flight = false;
                        self.ui.button(id!(update_now_btn)).set_text(cx, "Retry");
                        self.ui.label(id!(update_popup_sub)).set_text(cx, &err);
                        self.ui.label(id!(update_status_lbl)).set_text(cx, &err);
                    }
                },
            }
        }

        // 2. Poll Windows SMTC hardware media keys
        let smtc_cmds = self
            .smtc
            .as_ref()
            .map(|s| s.poll_commands())
            .unwrap_or_default();
        for cmd in smtc_cmds {
            match cmd {
                SmtcCommand::Play => {
                    if let Some(a) = &self.audio {
                        a.send(EngineCommand::Play);
                    }
                }
                SmtcCommand::Pause => {
                    if let Some(a) = &self.audio {
                        a.send(EngineCommand::Pause);
                    }
                }
                SmtcCommand::Toggle => {
                    if let Some(a) = &self.audio {
                        a.send(EngineCommand::TogglePlayPause);
                    }
                }
                SmtcCommand::Next => {
                    if let Some(next) = self.queue.next_track().cloned() {
                        self.play_track(cx, next);
                    }
                }
                SmtcCommand::Previous => {
                    if let Some(prev) = self.queue.prev_track().cloned() {
                        self.play_track(cx, prev);
                    }
                }
                SmtcCommand::Stop => {
                    if let Some(a) = &self.audio {
                        a.send(EngineCommand::Stop);
                    }
                }
            }
        }

        // 3. Update playback progress, synced lyrics, and auto-advance queue
        if let Some(audio) = &self.audio {
            let snap = audio.telemetry();
            if snap.track_finished_seq != self.last_finished_seq {
                self.last_finished_seq = snap.track_finished_seq;
                if let Some(next) = self.queue.next_track().cloned() {
                    if !snap.is_playing {
                        self.play_track(cx, next);
                    } else {
                        self.ui.label(id!(np_title_lbl)).set_text(cx, &next.title);
                        self.ui
                            .label(id!(np_artist_lbl))
                            .set_text(cx, &format!("{} · {}", next.artist, next.album));
                        self.load_lyrics_for_track(cx, &next);
                        self.refresh_sidebars(cx);
                    }
                } else {
                    self.ui.button(id!(play_pause_btn)).set_text(cx, "Play");
                }
            }

            if snap.duration_ms > 0 {
                let ratio = (snap.position_ms as f64 / snap.duration_ms as f64).clamp(0.0, 1.0);
                self.ui.slider(id!(seek_slider)).set_value(cx, ratio);
                self.ui
                    .label(id!(pos_lbl))
                    .set_text(cx, &format_duration_ms(snap.position_ms));
                self.ui
                    .label(id!(dur_total_lbl))
                    .set_text(cx, &format_duration_ms(snap.duration_ms));
                self.update_lyrics_display(cx, snap.position_ms);
            }
            self.ui
                .button(id!(play_pause_btn))
                .set_text(cx, if snap.is_playing { "Pause" } else { "Play" });
            if let Some(smtc) = &mut self.smtc {
                smtc.set_playing(snap.is_playing);
            }
        }
    }
}

impl AppMain for App {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        self.ensure_initialized(cx);

        if self.tick_timer.is_event(event).is_some() {
            self.handle_tick(cx);
        }

        if let Event::KeyDown(ke) = event {
            if ke.modifiers.control && ke.key_code == KeyCode::KeyK {
                self.switch_page(cx, live_id!(page_library));
                self.ui.text_input(id!(search_input)).set_key_focus(cx);
            } else if ke.key_code == KeyCode::Escape && self.update_popup_visible {
                self.dismiss_update_popup(cx);
            }
        }

        let props = TrackListScope {
            tracks: Rc::clone(&self.filtered_tracks),
            active_track_id: self.queue.current().map(|t| t.id.clone()),
        };
        let actions = {
            let mut scope = Scope::with_props(&props);
            cx.capture_actions(|cx| {
                self.ui.handle_event(cx, event, &mut scope);
            })
        };

        // Navigation buttons
        if self.ui.button(id!(nav_library)).clicked(&actions) {
            self.switch_page(cx, live_id!(page_library));
        }
        if self.ui.button(id!(nav_eq)).clicked(&actions) {
            self.switch_page(cx, live_id!(page_eq));
        }
        if self.ui.button(id!(nav_connect)).clicked(&actions) {
            self.switch_page(cx, live_id!(page_connect));
        }
        if self.ui.button(id!(nav_appearance)).clicked(&actions) {
            self.switch_page(cx, live_id!(page_appearance));
        }

        // Top Bar: Add Folder & Rescan & Search
        if self.ui.button(id!(add_folder_btn)).clicked(&actions)
            && let Some(folder) = rfd::FileDialog::new()
                .set_title("Select Local Music Folder")
                .pick_folder()
        {
            if let Some(db) = &self.db {
                let _ = db.add_music_folder(&folder);
            }
            self.spawn_folder_scan(vec![folder]);
        }
        if self.ui.button(id!(rescan_btn)).clicked(&actions)
            && let Some(db) = &self.db
            && let Ok(folders) = db.list_music_folders()
            && !folders.is_empty()
        {
            self.spawn_folder_scan(folders);
        }
        if let Some(query) = self.ui.text_input(id!(search_input)).changed(&actions) {
            self.search_query = query;
            self.apply_search_filter(cx);
        }

        // Library track clicks & hero buttons
        for action in &actions {
            if let TrackListAction::PlayIndex(filtered_idx) = action.cast()
                && let Some(clicked) = self.filtered_tracks.get(filtered_idx).cloned()
            {
                self.queue
                    .set_tracks((*self.filtered_tracks).clone(), Some(filtered_idx));
                self.play_track(cx, clicked);
            }
        }
        if self.ui.button(id!(play_all_btn)).clicked(&actions)
            && let Some(first) = self.filtered_tracks.first().cloned()
        {
            self.queue
                .set_tracks((*self.filtered_tracks).clone(), Some(0));
            self.play_track(cx, first);
        }
        if self.ui.button(id!(hero_shuffle_btn)).clicked(&actions)
            && !self.filtered_tracks.is_empty()
        {
            self.queue
                .set_tracks((*self.filtered_tracks).clone(), Some(0));
            self.queue.set_shuffle(true);
            self.ui.button(id!(shuffle_btn)).set_text(cx, "Shuffle: On");
            if let Some(first) = self.queue.current().cloned() {
                self.play_track(cx, first);
            }
        }

        // Bottom Player Transport Controls
        if self.ui.button(id!(play_pause_btn)).clicked(&actions)
            && let Some(audio) = &self.audio
        {
            let snap = audio.telemetry();
            if !snap.is_playing && self.queue.current().is_none() {
                if let Some(first) = self.filtered_tracks.first().cloned() {
                    self.queue
                        .set_tracks((*self.filtered_tracks).clone(), Some(0));
                    self.play_track(cx, first);
                }
            } else {
                audio.send(EngineCommand::TogglePlayPause);
            }
        }
        if self.ui.button(id!(next_btn)).clicked(&actions)
            && let Some(next) = self.queue.next_track().cloned()
        {
            self.play_track(cx, next);
        }
        if self.ui.button(id!(prev_btn)).clicked(&actions)
            && let Some(prev) = self.queue.prev_track().cloned()
        {
            self.play_track(cx, prev);
        }
        if self.ui.button(id!(shuffle_btn)).clicked(&actions) {
            let next_state = !self.queue.shuffle;
            self.queue.set_shuffle(next_state);
            self.ui.button(id!(shuffle_btn)).set_text(
                cx,
                if next_state {
                    "Shuffle: On"
                } else {
                    "Shuffle: Off"
                },
            );
            self.refresh_sidebars(cx);
        }
        if self.ui.button(id!(repeat_btn)).clicked(&actions) {
            self.queue.repeat = self.queue.repeat.next();
            self.ui
                .button(id!(repeat_btn))
                .set_text(cx, self.queue.repeat.label());
        }
        if let Some(pos_ratio) = self.ui.slider(id!(seek_slider)).end_slide(&actions)
            && let Some(audio) = &self.audio
        {
            let dur = audio.telemetry().duration_ms;
            if dur > 0 {
                let target_ms = (pos_ratio.clamp(0.0, 1.0) * dur as f64) as u64;
                audio.send(EngineCommand::SeekMs(target_ms));
            }
        }
        if let Some(vol) = self.ui.slider(id!(vol_slider)).slided(&actions)
            && let Some(audio) = &self.audio
        {
            audio.send(EngineCommand::SetVolume(vol as f32));
        }

        // Equalizer APO Controls
        if let Some(enabled) = self.ui.check_box(id!(eq_enable_cb)).changed(&actions) {
            self.eq.enabled = enabled;
            self.apply_eq_live(cx);
        }
        if let Some(val) = self.ui.slider(id!(eq_preamp_slider)).slided(&actions) {
            self.eq.preamp_db = val as f32;
            self.eq.preset_name = "Custom".to_string();
            self.apply_eq_live(cx);
        }
        for (idx, slider_id) in eq_band_slider_ids().iter().enumerate() {
            if let Some(val) = self.ui.slider(slider_id).slided(&actions)
                && let Some(band) = self.eq.bands.get_mut(idx)
            {
                band.gain_db = val as f32;
                self.eq.preset_name = "Custom".to_string();
                self.apply_eq_live(cx);
            }
        }
        for (btn_id, preset_name) in [
            (id!(eq_pre_flat), "Flat"),
            (id!(eq_pre_copper), "Warm Copper"),
            (id!(eq_pre_bass), "Deep Bass"),
            (id!(eq_pre_studio), "Studio Ref"),
            (id!(eq_pre_vocal), "Vocal Clarity"),
        ] {
            if self.ui.button(btn_id).clicked(&actions) {
                self.eq = EqConfig::preset(preset_name);
                self.sync_controls_from_state(cx);
                self.apply_eq_live(cx);
            }
        }
        if self.ui.button(id!(eq_import_btn)).clicked(&actions)
            && let Some(file) = rfd::FileDialog::new()
                .add_filter("Equalizer APO", &["txt"])
                .pick_file()
            && let Ok(raw) = std::fs::read_to_string(file)
            && let Ok(parsed) = EqConfig::from_equalizer_apo(&raw)
        {
            self.eq = parsed;
            self.sync_controls_from_state(cx);
            self.apply_eq_live(cx);
        }
        if self.ui.button(id!(eq_export_btn)).clicked(&actions)
            && let Some(file) = rfd::FileDialog::new()
                .set_file_name("ferric-eq-apo.txt")
                .add_filter("Equalizer APO", &["txt"])
                .save_file()
        {
            let _ = std::fs::write(file, self.eq.to_equalizer_apo());
        }

        // Ferric Connect Pairing URI Generator
        if self.ui.button(id!(gen_pair_btn)).clicked(&actions) {
            let tok = format!("{:016x}", fastrand::u64(..));
            let ep = format!("ed25519_ferric_{:08x}", fastrand::u32(..));
            let payload = PairingPayload {
                endpoint_id: ep,
                token_hex: tok,
                device_name: "Desktop-PC".to_string(),
            };
            self.ui
                .label(id!(pair_uri_lbl))
                .set_text(cx, &payload.to_uri());
            if let Some(lease) = &self.lease {
                self.ui.label(id!(connect_lease_lbl)).set_text(
                    cx,
                    &format!(
                        "Lease Revision #{} · Local Active: {} · Ready for Android Scan",
                        lease.revision,
                        lease.is_local_active()
                    ),
                );
            }
        }

        // Appearance Studio & Liquid Glass Live Controls
        if let Some(enabled) = self.ui.check_box(id!(liquid_glass_cb)).changed(&actions) {
            self.theme.material.liquid_glass = enabled;
            if enabled && self.theme.corner_radius_px < 14.0 {
                self.theme.corner_radius_px = 16.0;
                self.ui.slider(id!(lg_rad_slider)).set_value(cx, 16.0);
            }
            self.apply_theme_live(cx);
        }
        if let Some(v) = self.ui.slider(id!(lg_ior_slider)).slided(&actions) {
            self.theme.material.ior = v as f32;
            self.apply_theme_live(cx);
        }
        if let Some(v) = self.ui.slider(id!(lg_disp_slider)).slided(&actions) {
            self.theme.material.dispersion = v as f32;
            self.apply_theme_live(cx);
        }
        if let Some(v) = self.ui.slider(id!(lg_rim_slider)).slided(&actions) {
            self.theme.material.specular_rim = v as f32;
            self.apply_theme_live(cx);
        }
        if let Some(v) = self.ui.slider(id!(lg_rad_slider)).slided(&actions) {
            self.theme.corner_radius_px = v as f32;
            self.apply_theme_live(cx);
        }
        for (btn_id, preset) in [
            (id!(lg_pre_crystal), LiquidGlassPreset::CrystalPure),
            (id!(lg_pre_prism), LiquidGlassPreset::PrismaticLiquid),
            (id!(lg_pre_copper), LiquidGlassPreset::SmokedCopper),
            (id!(lg_pre_frost), LiquidGlassPreset::UltraFrosted),
        ] {
            if self.ui.button(btn_id).clicked(&actions) {
                self.theme.material.apply_preset(preset);
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
            }
        }
        if self.ui.button(id!(pal_amoled_btn)).clicked(&actions)
            || self.ui.button(id!(theme_reset_btn)).clicked(&actions)
        {
            self.theme = ThemeConfig::default();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(pal_glass_btn)).clicked(&actions) {
            self.theme = ThemeConfig::smoked_copper_glass();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(pal_cream_btn)).clicked(&actions) {
            self.theme = ThemeConfig::cream_light();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(theme_export_btn)).clicked(&actions)
            && let Some(file) = rfd::FileDialog::new()
                .set_file_name("custom.ferric-theme.json")
                .add_filter("Ferric Theme JSON", &["json"])
                .save_file()
        {
            let _ = self.theme.export_to_file(&file);
        }
        if self.ui.button(id!(theme_import_btn)).clicked(&actions)
            && let Some(file) = rfd::FileDialog::new()
                .add_filter("Ferric Theme JSON", &["json"])
                .pick_file()
            && let Ok(loaded) = ThemeConfig::import_from_file(&file)
        {
            self.theme = loaded;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }

        // Accounts OAuth PKCE Client ID Persistence
        if self.ui.button(id!(save_accounts_btn)).clicked(&actions) {
            self.accounts.spotify_client_id = self
                .ui
                .text_input(id!(spotify_id_input))
                .text()
                .trim()
                .to_string();
            self.accounts.youtube_client_id = self
                .ui
                .text_input(id!(yt_id_input))
                .text()
                .trim()
                .to_string();
            if let Some(db) = &self.db {
                let _ = db.save_accounts(&self.accounts);
            }
            self.refresh_sidebars(cx);
        }
        if let Some(cat_only) = self.ui.check_box(id!(yt_cat_cb)).changed(&actions) {
            self.accounts.youtube_music_category_only = cat_only;
            if let Some(db) = &self.db {
                let _ = db.save_accounts(&self.accounts);
            }
        }

        // Software Updates: Manual Check, Startup Preference & Floating Update Popup Actions
        if self.ui.button(id!(check_updates_top_btn)).clicked(&actions) {
            if let Some(info) = self.available_update.clone() {
                self.show_update_popup(cx, &info);
            } else {
                self.spawn_update_check(cx, true);
            }
        }
        if self.ui.button(id!(check_updates_btn)).clicked(&actions) {
            self.spawn_update_check(cx, true);
        }
        if self.ui.button(id!(open_releases_btn)).clicked(&actions) {
            let tag = self.available_update.as_ref().map(|u| u.tag.as_str());
            let _ = open_github_url(&release_page_url(tag));
        }
        if let Some(auto_chk) = self.ui.check_box(id!(auto_update_cb)).changed(&actions) {
            self.update_settings.auto_check_on_startup = auto_chk;
            if let Some(db) = &self.db {
                let _ = db.save_update_settings(&self.update_settings);
            }
        }
        if self.ui.button(id!(update_now_btn)).clicked(&actions) {
            self.start_update_download(cx);
        }
        if self.ui.button(id!(update_whats_new_btn)).clicked(&actions) {
            let tag = self.available_update.as_ref().map(|u| u.tag.as_str());
            let _ = open_github_url(&release_page_url(tag));
        }
        if self
            .ui
            .button(id!(update_popup_close_btn))
            .clicked(&actions)
        {
            self.dismiss_update_popup(cx);
        }
        if self.ui.button(id!(update_disable_btn)).clicked(&actions) {
            self.update_settings.auto_check_on_startup = false;
            self.ui.check_box(id!(auto_update_cb)).set_active(cx, false);
            if let Some(db) = &self.db {
                let _ = db.save_update_settings(&self.update_settings);
            }
            self.dismiss_update_popup(cx);
        }
    }
}

fn eq_band_slider_ids() -> [&'static [LiveId]; 10] {
    [
        id!(eq_b0),
        id!(eq_b1),
        id!(eq_b2),
        id!(eq_b3),
        id!(eq_b4),
        id!(eq_b5),
        id!(eq_b6),
        id!(eq_b7),
        id!(eq_b8),
        id!(eq_b9),
    ]
}

fn rgba_vec4(hex: &str) -> Vec4 {
    let [r, g, b, a] = hex_to_rgba_f32(hex, 1.0);
    vec4(r, g, b, a)
}
