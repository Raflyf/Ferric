use crate::shell::{
    copy_text_to_clipboard, launch_verified_installer, open_allowed_url, open_github_url,
    open_track_video_clip, win32_close_active_window, win32_is_active_window_maximized,
    win32_minimize_active_window, win32_toggle_maximize_active_window,
};
use crate::smtc::{SmtcBridge, SmtcCommand};
use crate::track_list::{TrackListAction, TrackListScope};
use crate::widgets::*;
use crate::{startup_load_demo, startup_page_override};
use ferric_audio::demo_synth::ensure_demo_tracks;
use ferric_audio::engine::{AudioEngine, EngineCommand};
use ferric_core::app_data_dir;
use ferric_core::db::LibraryDb;
use ferric_core::eq::EqConfig;
use ferric_core::models::{
    AccountKind, AccountSettings, FadeCurve, PlaybackDspSettings, Playlist, RepeatMode,
    SessionResumeState, Track, TrackSource, UpdateSettings, format_duration_ms,
    parse_playlist_import_text,
};
use ferric_core::queue::PlayQueue;
use ferric_core::theme::{LiquidGlassPreset, ThemeConfig, hex_to_rgba_f32};
use ferric_providers::autoeq::search_autoeq_profiles;
use ferric_providers::local_scanner::scan_local_folder;
use ferric_providers::lyrics::{SyncedLyrics, fetch_best_free_lyrics};
use ferric_providers::oauth::{
    CloudSyncReport, DEFAULT_OAUTH_CALLBACK_PATH, DEFAULT_OAUTH_CALLBACK_PORT, OAuthTokens,
    bind_loopback_callback, generate_pkce_pair, google_authorize_url, google_exchange_code,
    spotify_authorize_url, spotify_exchange_code, sync_spotify_catalog, sync_youtube_catalog,
};
use ferric_providers::public_sync::{
    import_spotify_public_url, import_youtube_public_url, parse_spotify_target,
    parse_youtube_target, search_youtube_public,
};
use ferric_providers::subsonic::{SubsonicCredentials, sync_subsonic_catalog};
use ferric_providers::updater::{
    DownloadEvent, ReleaseInfo, check_latest_release, download_verified_installer,
    format_check_status, release_page_url,
};
use ferric_sync::{
    ActiveSpeakerLease, LanDiscoveryBeacon, PairingPayload, QrMatrix, detect_local_lan_ip,
};
use makepad_widgets::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const BRAND_ICON_PNG: &[u8] = include_bytes!("../resources/ferric.png");

live_design! {
    use link::theme::*;
    use link::shaders::*;
    use link::widgets::*;
    use crate::widgets::*;
    use crate::track_list::TrackList;

    // Pure OLED Flat Surface (default) transforming into Snell Refractive Liquid Glass when toggled ON
    FerricSurface = <View> {
        show_bg: true,
        draw_bg: {
            uniform base_col: #060606
            uniform border_col: #1A1A1A
            uniform accent_col: #D9541E
            uniform glass_enabled: 0.0
            uniform glass_ior: 1.48
            uniform glass_disp: 0.42
            uniform glass_rim: 0.78
            uniform glass_scrim: 0.28
            uniform radius: 0.0

            fn pixel(self) -> vec4 {
                if (self.glass_enabled < 0.5) {
                    return self.base_col;
                }
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                let r = min(max(self.radius, 12.0) * 0.5, min(self.rect_size.x, self.rect_size.y) * 0.18);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, r);

                let center = self.rect_size * 0.5;
                let rel = p - center;
                let half_b = max(center - vec2(r * 2.0 + 1.0, r * 2.0 + 1.0), vec2(1.0, 1.0));
                let q = abs(rel) - half_b;
                let sd = length(max(q, vec2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r * 2.0;
                let depth = max(-sd, 0.0);
                let edge_t = clamp(1.0 - depth / 24.0, 0.0, 1.0);
                let slope = edge_t * edge_t * (2.2 - 0.8 * edge_t);
                let n = normalize(rel / max(center, vec2(1.0, 1.0)) + vec2(0.0001, 0.0001));
                let ang = atan(n.y, n.x);

                let diag = clamp(1.0 - (self.pos.x * 0.65 + self.pos.y * 0.85), 0.0, 1.0);
                let warm_glow = self.accent_col.xyz * (0.12 * diag + 0.09 * slope);
                let cool_glow = vec3(0.14, 0.28, 0.58) * (0.08 * (1.0 - diag) + 0.07 * slope);
                let disp_amt = pow(edge_t, 2.2) * self.glass_disp * clamp(self.glass_ior - 1.0, 0.1, 1.0);
                let prism = vec3(
                    0.55 + 0.45 * cos(ang - 0.55),
                    0.45 + 0.55 * cos(ang - 2.55),
                    0.55 + 0.45 * cos(ang + 1.45)
                ) * disp_amt;

                let tl = clamp(-(n.x * 0.72 + n.y * 0.70), 0.0, 1.0);
                let br = clamp((n.x * 0.70 + n.y * 0.72), 0.0, 1.0);
                let rim_band = clamp(1.0 - depth / 2.2, 0.0, 1.0);
                let spec = rim_band * ((0.30 + 0.80 * pow(tl, 1.5)) + (0.16 + 0.52 * pow(br, 1.6))) * self.glass_rim;
                let fresnel = pow(edge_t, 4.0) * 0.24 * self.glass_rim;

                let base_rgb = mix(self.base_col.xyz * 0.65 + vec3(0.03, 0.03, 0.045), vec3(0.02, 0.02, 0.03), self.glass_scrim);
                let rgb = base_rgb + warm_glow + cool_glow + prism + vec3(0.96, 0.94, 0.90) * (fresnel + spec * 0.65);
                sdf.fill(vec4(clamp(rgb, 0.0, 1.0), 1.0));
                return sdf.result;
            }
        }
    }

    FerricDividerH = <SolidView> {
        width: Fill,
        height: 1,
        draw_bg: { color: #1A1A1A }
    }

    FerricDividerV = <SolidView> {
        width: 1,
        height: Fill,
        draw_bg: { color: #1A1A1A }
    }

    FerricCard = <View> {
        width: Fill,
        height: Fit,
        padding: 20,
        show_bg: true,
        draw_bg: {
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 6.0);
                sdf.fill_keep(#0B0B0B);
                sdf.stroke(#1F1F1F, 1.0);
                return sdf.result;
            }
        }
    }

    // Sidebar Navigation Item with Real Crisp GPU SVG Vector Icon
    FerricNavItem = <Button> {
        width: Fill,
        height: 34,
        padding: { left: 12, right: 12 }
        spacing: 11,
        align: { x: 0.0, y: 0.5 }
        icon_walk: { width: 15, height: Fit }
        draw_icon: {
            color: #888279,
            color_hover: #F3EEE3,
            color_down: #D9541E
        }
        draw_text: {
            color: #9E988E,
            color_hover: #F3EEE3,
            color_down: #F3EEE3,
            text_style: { font_size: 13.0 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 4.0);
                let col = mix(mix(vec4(0.0, 0.0, 0.0, 0.0), #141414, self.hover), #1A0E08, self.active);
                sdf.fill_keep(col);
                sdf.stroke(mix(vec4(0.0, 0.0, 0.0, 0.0), #D9541E, self.active * 0.65), 1.0);
                return sdf.result;
            }
        }
    }

    FerricPlaylistBtn = <Button> {
        width: Fill,
        height: 32,
        padding: { left: 12, right: 10 }
        spacing: 8,
        align: { x: 0.0, y: 0.5 }
        draw_text: {
            color: #9E988E,
            color_hover: #F3EEE3,
            text_style: { font_size: 12.5 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 3.5);
                let col = mix(mix(vec4(0.0, 0.0, 0.0, 0.0), #141414, self.hover), #191919, self.active);
                sdf.fill(col);
                return sdf.result;
            }
        }
    }

    // Primary Copper Stadium Pill CTA (true stadium radius = (h - 1) * 0.25 in Makepad SDF)
    FerricAccentPill = <Button> {
        width: Fit,
        height: 36,
        padding: { left: 16, right: 18 }
        spacing: 8,
        align: { x: 0.5, y: 0.5 }
        icon_walk: { width: 14, height: Fit }
        draw_icon: {
            color: #080808,
            color_hover: #080808,
            color_down: #080808
        }
        draw_text: {
            color: #080808,
            color_hover: #080808,
            color_down: #080808,
            text_style: { font_size: 12.5 }
        }
        draw_bg: {
            instance hover: 0.0
            instance down: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let r = (self.rect_size.y - 1.0) * 0.25;
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);
                let c = mix(mix(#D9541E, #EB652E, self.hover), #B84214, self.down);
                sdf.fill(c);
                return sdf.result;
            }
        }
    }

    // Crisp FigCraft 8px Rounded-Rect Button (NO pointy football pills!)
    FerricBtn = <Button> {
        width: Fit,
        height: 34,
        padding: { left: 13, right: 14 }
        spacing: 7,
        align: { x: 0.5, y: 0.5 }
        icon_walk: { width: 14, height: Fit }
        draw_icon: {
            color: #ABA59B,
            color_hover: #F3EEE3,
            color_down: #D9541E
        }
        draw_text: {
            color: #E7E1D6,
            color_hover: #FFFFFF,
            color_down: #D9541E,
            text_style: { font_size: 12.0 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 4.0);
                let bg = mix(mix(#111111, #1B1B1B, self.hover), #1F1009, self.active);
                sdf.fill_keep(bg);
                let border = mix(#262626, #D9541E, self.active);
                sdf.stroke(border, 1.0);
                return sdf.result;
            }
        }
    }

    // Pure Centered Vector SVG Icon Button (for Bottom Player Bar & Transport)
    FerricIconBtn = <Button> {
        width: 34,
        height: 34,
        padding: 0,
        spacing: 0,
        align: { x: 0.5, y: 0.5 }
        text: ""
        label_walk: { width: 0, height: 0 }
        icon_walk: { width: 16, height: Fit }
        draw_icon: {
            color: #9E988E,
            color_hover: #F3EEE3,
            color_down: #D9541E
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 4.0);
                let bg = mix(mix(vec4(0.0, 0.0, 0.0, 0.0), #171717, self.hover), #201008, self.active);
                sdf.fill_keep(bg);
                sdf.stroke(mix(vec4(0.0, 0.0, 0.0, 0.0), #D9541E, self.active * 0.55), 1.0);
                return sdf.result;
            }
        }
    }

    FerricInput = <TextInput> {
        width: Fill,
        height: 36,
        draw_bg: {
            color: #090909,
            color_empty: #090909,
            color_hover: #0D0D0D,
            color_focus: #0F0F0F,
            border_radius: 4.0,
            border_size: 1.0,
            border_color_1: #262626,
            border_color_2: #262626,
            border_color_1_empty: #262626,
            border_color_2_empty: #262626,
            border_color_1_hover: #383838,
            border_color_2_hover: #383838,
            border_color_1_focus: #D9541E,
            border_color_2_focus: #D9541E
        }
        draw_text: {
            color: #F3EEE3,
            color_empty: #6F6A62,
            text_style: { font_size: 12.5 }
        }
        draw_cursor: { color: #D9541E }
    }

    App = {{App}} {
        ui: <Root> {
            main_window = <Window> {
                window: { title: "Ferric", inner_size: vec2(1280, 780) }
                pass: { clear_color: #000000 }
                caption_bar = {
                    visible: false,
                    height: 0,
                    caption_label = { label = { text: "Ferric" } }
                }

                body = <View> {
                    width: Fill,
                    height: Fill,
                    flow: Overlay,
                    show_bg: true,
                    draw_bg: {
                        uniform canvas_col: #000000
                        uniform glass_bg: 0.0
                        uniform accent_col: #D9541E
                        fn pixel(self) -> vec4 {
                            if (self.glass_bg < 0.5) {
                                return self.canvas_col;
                            }
                            let d1 = length(self.pos - vec2(0.16, 0.18));
                            let d2 = length(self.pos - vec2(0.84, 0.24));
                            let d3 = length(self.pos - vec2(0.55, 0.92));
                            let g1 = exp(-d1 * d1 * 6.0) * self.accent_col.xyz * 0.30;
                            let g2 = exp(-d2 * d2 * 6.5) * vec3(0.16, 0.32, 0.72) * 0.22;
                            let g3 = exp(-d3 * d3 * 7.0) * vec3(0.68, 0.18, 0.40) * 0.20;
                            return vec4(self.canvas_col.xyz + g1 + g2 + g3, 1.0);
                        }
                    }

                    // MAIN APPLICATION SHELL
                    shell_col = <View> {
                        width: Fill,
                        height: Fill,
                        flow: Down,

                        // 1. UNIFIED FRAMELESS TOP BAR (Zero duplicate Makepad titlebar!)
                        top_bar = <FerricSurface> {
                            width: Fill,
                            height: 52,
                            padding: { left: 16, right: 8 }
                            flow: Right,
                            align: { y: 0.5 }
                            spacing: 12,
                            draw_bg: { base_col: #050505, radius: 0.0 }

                            // Brand Anchor
                            <View> {
                                width: 212,
                                height: Fill,
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 10,
                                brand_icon = <Image> {
                                    width: 24,
                                    height: 24,
                                    fit: Smallest
                                }
                                <Label> {
                                    draw_text: { color: #F3EEE3, text_style: { font_size: 16.0 } }
                                    text: "Ferric"
                                }
                                <Label> {
                                    draw_text: { color: #66615A, text_style: { font_size: 11.0 } }
                                    text: "v0.1.0"
                                }
                            }

                            // Centered Search Box (with Crisp SVG Search Icon + Ctrl+K Trigger)
                            <View> {
                                width: Fill,
                                height: Fill,
                                align: { x: 0.5, y: 0.5 }
                                search_bar_wrap = <View> {
                                    width: 470,
                                    height: 36,
                                    padding: { left: 12, right: 6 }
                                    flow: Right,
                                    align: { y: 0.5 }
                                    spacing: 8,
                                    show_bg: true,
                                    draw_bg: {
                                        fn pixel(self) -> vec4 {
                                            let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                            sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 4.0);
                                            sdf.fill_keep(#0E0E0E);
                                            sdf.stroke(#242424, 1.0);
                                            return sdf.result;
                                        }
                                    }
                                    <Icon> {
                                        icon_walk: { width: 14, height: Fit }
                                        draw_icon: {
                                            svg_path: "M15.5 14H14.71L14.43 13.73C15.41 12.59 16 11.11 16 9.5C16 5.91 13.09 3 9.5 3C5.91 3 3 5.91 3 9.5C3 13.09 5.91 16 9.5 16C11.11 16 12.59 15.41 13.73 14.43L14 14.71V15.5L19 20.49L20.49 19L15.5 14ZM9.5 14C7.01 14 5 11.99 5 9.5C5 7.01 7.01 5 9.5 5C11.99 5 14 7.01 14 9.5C14 11.99 11.99 14 9.5 14Z"
                                            color: #7A756D
                                        }
                                    }
                                    search_input = <TextInput> {
                                        width: Fill,
                                        height: 30,
                                        empty_text: "Search library or paste Spotify / YouTube link..."
                                        draw_bg: {
                                            fn pixel(self) -> vec4 {
                                                return vec4(0.0, 0.0, 0.0, 0.0);
                                            }
                                        }
                                        draw_text: {
                                            color: #F3EEE3,
                                            color_empty: #6F6A62,
                                            text_style: { font_size: 12.5 }
                                        }
                                        draw_cursor: { color: #D9541E }
                                    }
                                    cmdk_btn = <FerricBtn> {
                                        height: 24,
                                        padding: { left: 8, right: 8 }
                                        text: "Ctrl+K"
                                        draw_text: { color: #888279, color_hover: #F3EEE3, text_style: { font_size: 10.5 } }
                                    }
                                }
                            }

                            // Right Quick Account Buttons & Custom Vector Window Controls
                            <View> {
                                width: Fit,
                                height: Fill,
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 6,

                                update_badge_btn = <FerricAccentPill> {
                                    visible: false,
                                    height: 28,
                                    padding: { left: 10, right: 10 }
                                    text: "Update"
                                    draw_text: { text_style: { font_size: 11.0 } }
                                }
                                top_spotify_btn = <FerricBtn> {
                                    height: 30,
                                    padding: { left: 10, right: 11 }
                                    text: "Spotify"
                                    draw_icon: {
                                        svg_path: "M19.35 10.04C18.67 6.59 15.64 4 12 4C9.11 4 6.6 5.64 5.35 8.04C2.34 8.36 0 10.91 0 14C0 17.31 2.69 20 6 20H19C21.76 20 24 17.76 24 15C24 12.36 21.95 10.22 19.35 10.04Z"
                                        color: #D9541E
                                    }
                                }
                                top_youtube_btn = <FerricBtn> {
                                    height: 30,
                                    padding: { left: 10, right: 11 }
                                    text: "YouTube"
                                    draw_icon: {
                                        svg_path: "M17 10.5V7C17 6.45 16.55 6 16 6H4C3.45 6 3 6.45 3 7V17C3 17.55 3.45 18 4 18H16C16.55 18 17 17.55 17 17V13.5L21 17.5V6.5L17 10.5Z"
                                        color: #D9541E
                                    }
                                }
                                <SolidView> { width: 1, height: 18, margin: { left: 4, right: 4 }, draw_bg: { color: #242424 } }
                                win_mini_btn = <FerricWinBtn> { width: 34, height: 30, draw_bg: { kind: 3.0 } }
                                win_min_btn = <FerricWinBtn> { width: 36, height: 30, draw_bg: { kind: 0.0 } }
                                win_max_btn = <FerricWinBtn> { width: 36, height: 30, draw_bg: { kind: 1.0 } }
                                win_close_btn = <FerricWinBtn> { width: 38, height: 30, draw_bg: { kind: 2.0 } }
                            }
                        }

                        <FerricDividerH> {}

                        // 2. WORKSPACE ROW (Calm Sidebar + Spacious Direct Center Stage + Optional Drawer)
                        workspace_row = <View> {
                            width: Fill,
                            height: Fill,
                            flow: Right,

                            // LEFT SIDEBAR (228px, FigCraft D1 Proportions, Real SVG Icons)
                            left_sidebar = <FerricSurface> {
                                width: 228,
                                height: Fill,
                                padding: { left: 12, right: 12, top: 14, bottom: 12 }
                                flow: Down,
                                spacing: 3,
                                draw_bg: { base_col: #070707, radius: 0.0 }

                                nav_home = <FerricNavItem> {
                                    text: "Home"
                                    draw_icon: { svg_path: "M10 20V14H14V20H19V12H22L12 3L2 12H5V20H10Z" }
                                }
                                nav_library = <FerricNavItem> {
                                    text: "Library"
                                    draw_bg: { active: 1.0 }
                                    draw_icon: {
                                        svg_path: "M4 4H6.5V20H4V4ZM9.5 4H12V20H9.5V4ZM14.5 4.6L16.9 4L20.5 19.4L18.1 20L14.5 4.6Z"
                                        color: #D9541E
                                    }
                                    draw_text: { color: #F3EEE3 }
                                }
                                nav_now_playing = <FerricNavItem> {
                                    text: "Now Playing"
                                    draw_icon: { svg_path: "M12 3V13.55C11.41 13.21 10.73 13 10 13C7.79 13 6 14.79 6 17C6 19.21 7.79 21 10 21C12.21 21 14 19.21 14 17V7H18V3H12Z" }
                                }

                                <Label> {
                                    margin: { left: 10, top: 8, bottom: 2 }
                                    draw_text: { color: #66615A, text_style: { font_size: 10.0 } }
                                    text: "STUDIO & SOUND"
                                }
                                nav_fade = <FerricNavItem> {
                                    text: "Fade-In & Audio"
                                    draw_icon: { svg_path: "M3 10H5V14H3V10ZM7 6H9V18H7V6ZM11 2H13V22H11V2ZM15 6H17V18H15V6ZM19 10H21V14H19V10Z" }
                                }
                                nav_eq = <FerricNavItem> {
                                    text: "Equalizer"
                                    draw_icon: { svg_path: "M5 4H7V11H9V13H3V11H5V4ZM5 15H7V20H5V15ZM11 4H13V7H15V9H9V7H11V4ZM11 11H13V20H11V11ZM17 4H19V14H21V16H15V14H17V4ZM17 18H19V20H17V18Z" }
                                }
                                nav_connect = <FerricNavItem> {
                                    text: "Ferric Connect"
                                    draw_icon: { svg_path: "M4 6H20V4H4C2.9 4 2 4.9 2 6V17H0V20H14V17H4V6ZM23 8H17C16.45 8 16 8.45 16 9V19C16 19.55 16.45 20 17 20H23C23.55 20 24 19.55 24 19V9C24 8.45 23.55 8 23 8ZM22 17H18V10H22V17Z" }
                                }
                                nav_appearance = <FerricNavItem> {
                                    text: "Appearance"
                                    draw_icon: { svg_path: "M12 2L2 9L12 16L22 9L12 2ZM12 18.54L4.62 12.81L3 14.07L12 21.07L21 14.07L19.37 12.8L12 18.54Z" }
                                }
                                nav_accounts = <FerricNavItem> {
                                    text: "Accounts & Login"
                                    draw_icon: { svg_path: "M12 12C14.21 12 16 10.21 16 8C16 5.79 14.21 4 12 4C9.79 4 8 5.79 8 8C8 10.21 9.79 12 12 12ZM12 14C9.33 14 4 15.34 4 18V20H20V18C20 15.34 14.67 14 12 14Z" }
                                }

                                <View> {
                                    width: Fill, height: Fit, margin: { left: 10, right: 2, top: 8, bottom: 2 }
                                    flow: Right, align: { y: 0.5 }
                                    <Label> {
                                        width: Fill,
                                        draw_text: { color: #66615A, text_style: { font_size: 10.0 } }
                                        text: "PLAYLISTS"
                                    }
                                    sidebar_import_btn = <FerricBtn> {
                                        height: 24,
                                        padding: { left: 8, right: 8 }
                                        text: "Import"
                                        icon_walk: { width: 12, height: Fit }
                                        draw_icon: { svg_path: "M19 9H15V3H9V9H5L12 16L19 9ZM5 18V20H19V18H5Z" }
                                        draw_text: { text_style: { font_size: 10.5 } }
                                    }
                                }

                                <ScrollYView> {
                                    width: Fill,
                                    height: Fill,
                                    flow: Down,
                                    spacing: 2,
                                    pl_all_btn = <FerricPlaylistBtn> { text: "All Songs", draw_bg: { active: 1.0 } }
                                    pl_liked_btn = <FerricPlaylistBtn> { text: "Liked Songs" }
                                    pl_local_btn = <FerricPlaylistBtn> { text: "Local Lossless" }
                                    pl_slot_0 = <FerricPlaylistBtn> { visible: false, text: "" }
                                    pl_slot_1 = <FerricPlaylistBtn> { visible: false, text: "" }
                                    pl_slot_2 = <FerricPlaylistBtn> { visible: false, text: "" }
                                    pl_slot_3 = <FerricPlaylistBtn> { visible: false, text: "" }
                                    pl_slot_4 = <FerricPlaylistBtn> { visible: false, text: "" }
                                }

                                sidebar_add_folder_btn = <FerricBtn> {
                                    width: Fill,
                                    height: 34,
                                    margin: { top: 6 }
                                    text: "Add Music Folder"
                                    draw_icon: {
                                        svg_path: "M20 6H12L10 4H4C2.89 4 2 4.89 2 6V18C2 19.11 2.89 20 4 20H20C21.11 20 22 19.11 22 18V8C22 6.89 21.11 6 20 6ZM19 14H16V17H14V14H11V12H14V9H16V12H19V14Z"
                                        color: #D9541E
                                    }
                                }
                            }

                            <FerricDividerV> {}

                            // CENTER STAGE (Clean, Direct-Access Pages)
                            center_stage = <FerricSurface> {
                                width: Fill,
                                height: Fill,
                                flow: Overlay,
                                draw_bg: { base_col: #000000, radius: 0.0 }

                                // PAGE 1: HOME
                                page_home = <ScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 32, right: 32, top: 28, bottom: 24 }
                                    flow: Down,
                                    spacing: 18,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 4,
                                            <Label> {
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 26.0 } }
                                                text: "Welcome to Ferric"
                                            }
                                            home_sub_lbl = <Label> {
                                                draw_text: { color: #9E988E, text_style: { font_size: 13.0 } }
                                                text: "Bit-perfect local audio, seamless dual-deck fade-in, Spotify & YouTube sync."
                                            }
                                        }
                                        stats_capsule_lbl = <Label> {
                                            draw_text: { color: #888279, text_style: { font_size: 12.0 } }
                                            text: "0 songs played · 0 min listened"
                                        }
                                    }

                                    // 3 Clean Action Cards
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 14,

                                        <FerricCard> {
                                            width: Fill, flow: Down, spacing: 10,
                                            <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }, text: "Local Lossless Library" }
                                            <Label> { width: Fill, draw_text: { color: #9E988E, text_style: { font_size: 12.5 } }, text: "Scan FLAC, WAV, MP3, Opus, and OGG folders directly with zero latency." }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8, margin: { top: 4 }
                                                home_add_folder_btn = <FerricAccentPill> {
                                                    height: 34,
                                                    text: "Add Folder"
                                                    draw_icon: { svg_path: "M20 6H12L10 4H4C2.89 4 2 4.89 2 6V18C2 19.11 2.89 20 4 20H20C21.11 20 22 19.11 22 18V8C22 6.89 21.11 6 20 6ZM19 14H16V17H14V14H11V12H14V9H16V12H19V14Z" }
                                                }
                                                home_demo_btn = <FerricBtn> {
                                                    height: 34,
                                                    text: "Play Demo"
                                                    draw_icon: { svg_path: "M8 5V19L19 12L8 5Z", color: #D9541E }
                                                }
                                            }
                                        }

                                        <FerricCard> {
                                            width: Fill, flow: Down, spacing: 10,
                                            <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }, text: "Spotify & YouTube Sync" }
                                            <Label> { width: Fill, draw_text: { color: #9E988E, text_style: { font_size: 12.5 } }, text: "Import any public Spotify or YouTube playlist instantly, or sign in via OAuth PKCE." }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8, margin: { top: 4 }
                                                home_spotify_btn = <FerricBtn> { height: 34, text: "Spotify Sync" }
                                                home_youtube_btn = <FerricBtn> { height: 34, text: "YouTube Sync" }
                                            }
                                        }

                                        <FerricCard> {
                                            width: Fill, flow: Down, spacing: 10,
                                            <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }, text: "Seamless Fade-In Engine" }
                                            home_dsp_lbl = <Label> { width: Fill, draw_text: { color: #D9541E, text_style: { font_size: 12.5 } }, text: "6.0s Bass-Swap Automix · 240ms Play Fade-In" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8, margin: { top: 4 }
                                                home_fade_btn = <FerricBtn> { height: 34, text: "Tune Fade-In" }
                                                home_eq_btn = <FerricBtn> { height: 34, text: "10-Band EQ" }
                                            }
                                        }
                                    }

                                    // Quick Picks
                                    <FerricCard> {
                                        flow: Down, spacing: 8,
                                        <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 16.0 } }, text: "Quick Picks" }
                                        recent_btn_0 = <FerricPlaylistBtn> { height: 38, text: "Click 'Play Demo' above to experience Seamless Fade-In right now" }
                                        recent_btn_1 = <FerricPlaylistBtn> { visible: false, height: 38, text: "" }
                                        recent_btn_2 = <FerricPlaylistBtn> { visible: false, height: 38, text: "" }
                                        recent_btn_3 = <FerricPlaylistBtn> { visible: false, height: 38, text: "" }
                                    }
                                }

                                // PAGE 2: LIBRARY (Default Clean View Matching FigCraft D1)
                                page_library = <View> {
                                    visible: true,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 28, right: 28, top: 22, bottom: 12 }
                                    flow: Down,
                                    spacing: 14,

                                    // Compact Editorial Hero
                                    <View> {
                                        width: Fill,
                                        height: Fit,
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 20,

                                        hero_art = <FerricArtTile> {
                                            width: 102,
                                            height: 102,
                                            draw_bg: { variant: 1.0, radius: 12.0 }
                                        }

                                        <View> {
                                            width: Fill,
                                            height: Fit,
                                            flow: Down,
                                            spacing: 6,

                                            hero_title_lbl = <Label> {
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 26.0 } }
                                                text: "Your Library"
                                            }
                                            library_sub_lbl = <Label> {
                                                draw_text: { color: #9E988E, text_style: { font_size: 12.5 } }
                                                text: "0 songs · 0 min · 6.0s Bass-Swap Automix"
                                            }
                                            <View> {
                                                width: Fill,
                                                height: Fit,
                                                margin: { top: 6 }
                                                flow: Right,
                                                align: { y: 0.5 }
                                                spacing: 8,

                                                play_all_btn = <FerricAccentPill> {
                                                    text: "Play All"
                                                    draw_icon: { svg_path: "M8 5V19L19 12L8 5Z" }
                                                }
                                                hero_shuffle_btn = <FerricBtn> {
                                                    height: 36,
                                                    text: "Shuffle"
                                                    draw_icon: { svg_path: "M10.59 9.17L5.41 4L4 5.41L9.17 10.58L10.59 9.17ZM14.5 4L16.54 6.04L4 18.59L5.41 20L17.96 7.46L20 9.5V4H14.5ZM14.83 13.41L13.42 14.82L16.55 17.95L14.5 20H20V14.5L17.96 16.54L14.83 13.41Z" }
                                                }
                                                add_folder_btn = <FerricBtn> {
                                                    height: 36,
                                                    text: "Add Folder"
                                                    draw_icon: { svg_path: "M20 6H12L10 4H4C2.89 4 2 4.89 2 6V18C2 19.11 2.89 20 4 20H20C21.11 20 22 19.11 22 18V8C22 6.89 21.11 6 20 6ZM19 14H16V17H14V14H11V12H14V9H16V12H19V14Z", color: #D9541E }
                                                }
                                                demo_suite_btn = <FerricBtn> {
                                                    height: 36,
                                                    text: "Play Demo"
                                                    draw_icon: { svg_path: "M8 5V19L19 12L8 5Z", color: #D9541E }
                                                }
                                            }
                                        }
                                    }

                                    // Source Filter Bar + Sort
                                    <View> {
                                        width: Fill,
                                        height: Fit,
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 8,

                                        flt_all_btn = <FerricBtn> { height: 30, text: "All", draw_bg: { active: 1.0 } }
                                        flt_liked_btn = <FerricBtn> { height: 30, text: "Liked" }
                                        flt_local_btn = <FerricBtn> { height: 30, text: "Local" }
                                        flt_spotify_btn = <FerricBtn> { height: 30, text: "Spotify" }
                                        flt_youtube_btn = <FerricBtn> { height: 30, text: "YouTube" }
                                        <View> { width: Fill, height: 1 }
                                        sort_cycle_btn = <FerricBtn> { height: 30, text: "Sort: Artist" }
                                    }

                                    // Crisp Table Column Header
                                    <View> {
                                        width: Fill,
                                        height: 28,
                                        padding: { left: 16, right: 18 }
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 14,
                                        show_bg: true,
                                        draw_bg: {
                                            fn pixel(self) -> vec4 {
                                                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                                sdf.rect(0.0, self.rect_size.y - 1.0, self.rect_size.x, 1.0);
                                                sdf.fill(#1E1E1E);
                                                return sdf.result;
                                            }
                                        }
                                        <Label> { width: 26, draw_text: { color: #757068, text_style: { font_size: 11.0 } }, text: "#" }
                                        <View> { width: 40, height: 1 }
                                        <Label> { width: Fill, draw_text: { color: #757068, text_style: { font_size: 11.0 } }, text: "Title" }
                                        <Label> { width: 220, draw_text: { color: #757068, text_style: { font_size: 11.0 } }, text: "Album" }
                                        <View> { width: 34, height: 1 }
                                        <Label> { width: 60, align: { x: 1.0 }, draw_text: { color: #757068, text_style: { font_size: 11.0 } }, text: "Time" }
                                    }

                                    track_list = <TrackList> {}
                                }

                                // PAGE 3: NOW PLAYING & FREE SYNCED LYRICS
                                page_now_playing = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 28, right: 28, top: 22, bottom: 20 }
                                    flow: Down,
                                    spacing: 16,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8,
                                        <Label> {
                                            width: Fill,
                                            draw_text: { color: #F3EEE3, text_style: { font_size: 22.0 } }
                                            text: "Now Playing & Synced Lyrics"
                                        }
                                        np_mode_canvas_btn = <FerricBtn> { height: 32, text: "Motion Canvas", draw_bg: { active: 1.0 } }
                                        np_mode_card_btn = <FerricBtn> { height: 32, text: "Cover Card" }
                                        np_watch_video_btn = <FerricAccentPill> {
                                            height: 32,
                                            text: "Watch Video Clip"
                                            draw_icon: { svg_path: "M17 10.5V7C17 6.45 16.55 6 16 6H4C3.45 6 3 6.45 3 7V17C3 17.55 3.45 18 4 18H16C16.55 18 17 17.55 17 17V13.5L21 17.5V6.5L17 10.5Z" }
                                        }
                                    }

                                    <View> {
                                        width: Fill, height: Fill, flow: Right, spacing: 20,

                                        // Left: Animated Motion Canvas + Signal Path
                                        <FerricCard> {
                                            width: 350,
                                            height: Fill,
                                            flow: Down,
                                            spacing: 12,

                                            np_canvas = <FerricNowPlayingCanvas> { height: 280 }

                                            np_stage_title_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 19.0 } }
                                                text: "No Track Playing"
                                            }
                                            np_stage_artist_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #9E988E, text_style: { font_size: 13.0 } }
                                                text: "Pick a song from Library or click Play Demo"
                                            }
                                            <View> { width: Fill, height: Fill }
                                            signal_path_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #D9541E, text_style: { font_size: 11.5 } }
                                                text: "Signal Path: Ready · 6.0s Bass-Swap Automix · 10-Band EQ -> WASAPI"
                                            }
                                        }

                                        // Right: Karaoke Synced Lyrics (LRCLIB + Lyrics.ovh)
                                        <FerricCard> {
                                            width: Fill,
                                            height: Fill,
                                            flow: Down,
                                            spacing: 12,

                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8,
                                                lyrics_source_lbl = <Label> {
                                                    width: Fill,
                                                    draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }
                                                    text: "Synced Lyrics · LRCLIB & Lyrics.ovh (Free)"
                                                }
                                                lyr_minus_btn = <FerricBtn> { height: 28, text: "-0.5s" }
                                                lyr_plus_btn = <FerricBtn> { height: 28, text: "+0.5s" }
                                                lyr_copy_btn = <FerricBtn> { height: 28, text: "Copy" }
                                            }

                                            <View> { width: Fill, height: Fill }
                                            lyric_prev2_lbl = <Label> { width: Fill, draw_text: { color: #59554F, text_style: { font_size: 14.0 } }, text: "" }
                                            lyric_prev_lbl = <Label> { width: Fill, draw_text: { color: #888279, text_style: { font_size: 15.5 } }, text: "" }
                                            lyric_active_lbl = <Label> { width: Fill, draw_text: { color: #D9541E, text_style: { font_size: 21.0 } }, text: "Lyrics load automatically when a track plays." }
                                            lyric_next_lbl = <Label> { width: Fill, draw_text: { color: #9E988E, text_style: { font_size: 15.5 } }, text: "" }
                                            lyric_next2_lbl = <Label> { width: Fill, draw_text: { color: #59554F, text_style: { font_size: 14.0 } }, text: "" }
                                            <View> { width: Fill, height: Fill }
                                        }
                                    }
                                }

                                // PAGE 4: SEAMLESS FADE-IN & AUTOMIX STUDIO (Sonora + Spotify Parity)
                                page_fade = <ScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 32, right: 32, top: 24, bottom: 24 }
                                    flow: Down,
                                    spacing: 16,

                                    <View> {
                                        width: Fill, height: Fit, flow: Down, spacing: 4,
                                        <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 22.0 } }, text: "Seamless Fade-In & Automix (Tanpa Jeda)" }
                                        <Label> { width: Fill, draw_text: { color: #9E988E, text_style: { font_size: 12.5 } }, text: "Dual-deck engine pre-buffers Deck B ahead of time and blends songs smoothly with zero dead air." }
                                    }

                                    <FerricCard> {
                                        flow: Down, spacing: 14,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                            <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Crossfade Overlap Between Songs" }
                                            xfade_val_lbl = <Label> { width: 72, align: { x: 1.0 }, draw_text: { color: #D9541E, text_style: { font_size: 13.0 } }, text: "6.0 s" }
                                        }
                                        xfade_slider = <FerricBarSlider> { min: 0.0, max: 12.0, value: 6.0 }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 8,
                                            xf_0_btn = <FerricBtn> { height: 30, text: "0s Pure Gapless" }
                                            xf_3_btn = <FerricBtn> { height: 30, text: "3s Subtle" }
                                            xf_6_btn = <FerricBtn> { height: 30, text: "6s Automix" }
                                            xf_10_btn = <FerricBtn> { height: 30, text: "10s Long Mix" }
                                        }

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 } margin: { top: 4 }
                                            <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Smooth Fade-In on Play / Resume / Seek" }
                                            fadein_val_lbl = <Label> { width: 72, align: { x: 1.0 }, draw_text: { color: #D9541E, text_style: { font_size: 13.0 } }, text: "240 ms" }
                                        }
                                        fadein_slider = <FerricBarSlider> { min: 0.0, max: 1500.0, value: 240.0 }

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8, margin: { top: 4 }
                                            <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Mix Curve Style" }
                                            curve_bass_btn = <FerricBtn> { height: 30, text: "Bass-Swap Automix", draw_bg: { active: 1.0 } }
                                            curve_eqp_btn = <FerricBtn> { height: 30, text: "Equal-Power" }
                                            curve_velvet_btn = <FerricBtn> { height: 30, text: "S-Curve Velvet" }
                                        }
                                    }

                                    <FerricCard> {
                                        flow: Down, spacing: 14,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 2,
                                                <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Gapless Album Lock" }
                                                <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Keep 0ms sample-exact transitions when playing consecutive tracks from the same album." }
                                            }
                                            gapless_lock_toggle = <FerricToggle> { is_on: true }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 2,
                                                <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Skip Intro & Outro Silence" }
                                                <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Trim dead silence at track boundaries so the next song fades in immediately." }
                                            }
                                            trim_silence_toggle = <FerricToggle> { is_on: true }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 2,
                                                <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Normalize Volume (ReplayGain)" }
                                                <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Keep consistent perceived loudness across all tracks." }
                                            }
                                            replay_gain_toggle = <FerricToggle> { is_on: true }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 2,
                                                <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Mono Audio" }
                                                <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Sum left and right channels equally to both speakers." }
                                            }
                                            mono_toggle = <FerricToggle> { is_on: false }
                                        }
                                    }

                                    <FerricCard> {
                                        flow: Down, spacing: 12,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8,
                                            <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Playback Speed" }
                                            spd_075_btn = <FerricBtn> { height: 30, text: "0.75x" }
                                            spd_100_btn = <FerricBtn> { height: 30, text: "1.0x Normal", draw_bg: { active: 1.0 } }
                                            spd_125_btn = <FerricBtn> { height: 30, text: "1.25x" }
                                            spd_150_btn = <FerricBtn> { height: 30, text: "1.5x" }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8,
                                            sleep_status_lbl = <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Sleep Timer: Off" }
                                            slp_off_btn = <FerricBtn> { height: 30, text: "Off", draw_bg: { active: 1.0 } }
                                            slp_15_btn = <FerricBtn> { height: 30, text: "15m" }
                                            slp_30_btn = <FerricBtn> { height: 30, text: "30m" }
                                            slp_60_btn = <FerricBtn> { height: 30, text: "60m" }
                                            slp_eot_btn = <FerricBtn> { height: 30, text: "End of Song" }
                                        }
                                    }
                                }

                                // PAGE 5: 10-BAND PARAMETRIC EQUALIZER & AUTOEQ (FigCraft D2 Studio)
                                page_eq = <ScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 32, right: 32, top: 24, bottom: 24 }
                                    flow: Down,
                                    spacing: 14,

                                    <FerricCard> {
                                        flow: Down, spacing: 14,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 10,
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 2,
                                                <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 20.0 } }, text: "10-Band Parametric Equalizer & AutoEq" }
                                                eq_status_lbl = <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Preset: Flat · Double-click any fader to reset" }
                                            }
                                            eq_import_btn = <FerricBtn> { height: 32, text: "Import APO" }
                                            eq_export_btn = <FerricBtn> { height: 32, text: "Export APO" }
                                            eq_enable_toggle = <FerricToggle> { is_on: true }
                                        }

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8,
                                            autoeq_input = <FerricInput> {
                                                width: 248, height: 36,
                                                empty_text: "Search 24 AutoEq headphones..."
                                            }
                                            autoeq_apply_btn = <FerricAccentPill> { height: 34, padding: { left: 14, right: 14 }, text: "Apply Best Match" }
                                            eq_pre_flat = <FerricBtn> { height: 34, text: "Flat" }
                                            eq_pre_copper = <FerricBtn> { height: 34, text: "Warm Copper" }
                                            eq_pre_bass = <FerricBtn> { height: 34, text: "Deep Bass" }
                                            eq_pre_vocal = <FerricBtn> { height: 34, text: "Vocal" }
                                        }

                                        eq_graph = <FerricEqGraph> {}

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 12,
                                            <Label> { width: 72, draw_text: { color: #9E988E, text_style: { font_size: 12.5 } }, text: "Preamp" }
                                            eq_preamp_slider = <FerricBarSlider> { min: -12.0, max: 12.0, value: 0.0 }
                                            eq_preamp_lbl = <Label> { width: Fit, align: { x: 1.0 }, draw_text: { color: #D9541E, text_style: { font_size: 12.5 } }, text: "+0.0 dB" }
                                        }

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 6, margin: { top: 4 }
                                            <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v0 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 11.0 } }, text: "+0.0" } eq_b0 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 11.0 } }, text: "32" } }
                                            <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v1 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 11.0 } }, text: "+0.0" } eq_b1 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 11.0 } }, text: "64" } }
                                            <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v2 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 11.0 } }, text: "+0.0" } eq_b2 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 11.0 } }, text: "125" } }
                                            <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v3 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 11.0 } }, text: "+0.0" } eq_b3 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 11.0 } }, text: "250" } }
                                            <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v4 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 11.0 } }, text: "+0.0" } eq_b4 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 11.0 } }, text: "500" } }
                                            <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v5 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 11.0 } }, text: "+0.0" } eq_b5 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 11.0 } }, text: "1k" } }
                                            <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v6 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 11.0 } }, text: "+0.0" } eq_b6 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 11.0 } }, text: "2k" } }
                                            <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v7 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 11.0 } }, text: "+0.0" } eq_b7 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 11.0 } }, text: "4k" } }
                                            <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v8 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 11.0 } }, text: "+0.0" } eq_b8 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 11.0 } }, text: "8k" } }
                                            <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v9 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 11.0 } }, text: "+0.0" } eq_b9 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 11.0 } }, text: "16k" } }
                                        }
                                    }
                                }

                                // PAGE 6: FERRIC CONNECT HUB (FigCraft D2 Dedicated QR + LAN P2P)
                                page_connect = <ScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 32, right: 32, top: 24, bottom: 24 }
                                    flow: Down,
                                    spacing: 16,

                                    <View> {
                                        width: Fill, height: Fit, flow: Down, spacing: 4,
                                        <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 22.0 } }, text: "Ferric Connect (LAN & P2P Handoff)" }
                                        <Label> { width: Fill, draw_text: { color: #9E988E, text_style: { font_size: 12.5 } }, text: "Pair Ferric Android over local Wi-Fi with zero cloud servers. Single active speaker lease prevents audio overlap." }
                                    }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 16,

                                        <FerricCard> {
                                            width: 300, flow: Down, align: { x: 0.5 }, spacing: 12,
                                            <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 15.0 } }, text: "Scan Pairing QR Code" }
                                            connect_qr_view = <FerricQrView> { width: 184, height: 184 }
                                            connect_lan_ip_lbl = <Label> {
                                                width: Fill, align: { x: 0.5 },
                                                draw_text: { color: #D9541E, text_style: { font_size: 12.0 } }
                                                text: "LAN: 127.0.0.1:43821"
                                            }
                                            <View> {
                                                width: Fit, height: Fit, flow: Right, spacing: 8,
                                                connect_gen_pair_btn = <FerricAccentPill> { height: 32, text: "New QR" }
                                                connect_copy_pair_btn = <FerricBtn> { height: 32, text: "Copy Link" }
                                            }
                                        }

                                        <FerricCard> {
                                            width: Fill, flow: Down, spacing: 10,
                                            <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }, text: "Active Speaker & Discovered LAN Devices" }
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #D9541E, text_style: { font_size: 13.0 } }
                                                text: "Active Speaker: Desktop-PC (WASAPI Exclusive-Ready · UDP Port 43821)"
                                            }
                                            connect_peers_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #9E988E, text_style: { font_size: 12.5 } }
                                                text: "Broadcasting LAN discovery beacon on local Wi-Fi... Waiting for Ferric mobile peers."
                                            }
                                        }
                                    }
                                }

                                // PAGE 7: APPEARANCE (LIQUID GLASS) & UPDATES (FigCraft D3 / LG2)
                                page_appearance = <ScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 32, right: 32, top: 24, bottom: 24 }
                                    flow: Down,
                                    spacing: 14,

                                    <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 22.0 } }, text: "Open Theme Studio, Liquid Glass & Updates" }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 18,

                                        // Left Column: Theme, Liquid Glass Controls & Software Updates
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 14,

                                            <FerricCard> {
                                                flow: Down, spacing: 12,
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                    <View> {
                                                        width: Fill, height: Fit, flow: Down, spacing: 2,
                                                        <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }, text: "Liquid Glass Theme (All UI Surfaces)" }
                                                        <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Curved Snell bevel refraction, chromatic edge dispersion & specular rim." }
                                                    }
                                                    liquid_glass_toggle = <FerricToggle> { is_on: false }
                                                }

                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, spacing: 8,
                                                    lg_pre_crystal = <FerricBtn> { height: 30, text: "Crystal Pure" }
                                                    lg_pre_prism = <FerricBtn> { height: 30, text: "Prismatic Liquid" }
                                                    lg_pre_copper = <FerricBtn> { height: 30, text: "Smoked Copper" }
                                                    lg_pre_frost = <FerricBtn> { height: 30, text: "Ultra Frosted" }
                                                }

                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                    <Label> { width: Fill, draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Refraction Index (IOR)" }
                                                    lg_ior_lbl = <Label> { width: Fit, align: { x: 1.0 }, draw_text: { color: #D9541E, text_style: { font_size: 12.0 } }, text: "1.48" }
                                                }
                                                lg_ior_slider = <FerricBarSlider> { min: 1.05, max: 1.85, value: 1.48 }

                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                    <Label> { width: Fill, draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Chromatic Edge Dispersion" }
                                                    lg_disp_lbl = <Label> { width: Fit, align: { x: 1.0 }, draw_text: { color: #D9541E, text_style: { font_size: 12.0 } }, text: "42%" }
                                                }
                                                lg_disp_slider = <FerricBarSlider> { min: 0.0, max: 1.0, value: 0.42 }

                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                    <Label> { width: Fill, draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Specular Rim & Corner Radius" }
                                                    lg_rim_lbl = <Label> { width: Fit, align: { x: 1.0 }, draw_text: { color: #D9541E, text_style: { font_size: 12.0 } }, text: "78% · 14px" }
                                                }
                                                lg_rim_slider = <FerricBarSlider> { min: 0.0, max: 1.0, value: 0.78 }
                                                lg_rad_slider = <FerricBarSlider> { min: 4.0, max: 28.0, value: 14.0 }

                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, spacing: 8, margin: { top: 4 }
                                                    pal_amoled_btn = <FerricBtn> { height: 32, text: "AMOLED Dark" }
                                                    pal_cream_btn = <FerricBtn> { height: 32, text: "Cream Light" }
                                                    theme_import_btn = <FerricBtn> { height: 32, text: "Import JSON" }
                                                    theme_export_btn = <FerricBtn> { height: 32, text: "Export JSON" }
                                                }
                                            }

                                            // Software Updates Card (WallpaperLive Parity)
                                            <FerricCard> {
                                                flow: Down, spacing: 10,
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                    <View> {
                                                        width: Fill, height: Fit, flow: Down, spacing: 2,
                                                        <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }, text: "Software Updates" }
                                                        update_status_lbl = <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Current version: v0.1.0 · Auto-check on startup enabled" }
                                                    }
                                                    auto_update_toggle = <FerricToggle> { is_on: true }
                                                }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, spacing: 8,
                                                    check_updates_btn = <FerricAccentPill> { height: 34, text: "Check for Updates" }
                                                    open_releases_btn = <FerricBtn> { height: 34, text: "GitHub Releases" }
                                                }
                                            }
                                        }

                                        // Right Column: Live Optical Refractive Mini-Player Preview (LG2)
                                        <View> {
                                            width: 344, height: Fit, flow: Down, spacing: 8,
                                            <Label> {
                                                draw_text: { color: #888279, text_style: { font_size: 11.0 } }
                                                text: "LIVE OPTICAL PREVIEW · FLOATING MINI-PLAYER"
                                            }
                                            lg_preview = <LiquidGlassPreview> {}
                                        }
                                    }
                                }

                                // PAGE 8: ACCOUNTS, INSTANT ZERO-KEY SYNC & OAUTH 2.0 PKCE LOGIN
                                page_accounts = <ScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 32, right: 32, top: 24, bottom: 24 }
                                    flow: Down,
                                    spacing: 14,

                                    <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 22.0 } }, text: "Spotify, YouTube Music & Cloud Accounts" }

                                    oauth_status_lbl = <Label> {
                                        width: Fill,
                                        draw_text: { color: #D9541E, text_style: { font_size: 12.5 } }
                                        text: "Ready: Paste any public Spotify/YouTube link for instant zero-key sync, or sign in via Official OAuth PKCE below."
                                    }

                                    // TIER 1: INSTANT ZERO-KEY SPOTIFY & YOUTUBE IMPORT / SEARCH (WORKS IMMEDIATELY!)
                                    <FerricCard> {
                                        flow: Down, spacing: 10,
                                        <Label> {
                                            draw_text: { color: #F3EEE3, text_style: { font_size: 16.0 } }
                                            text: "Instant Sync — Spotify & YouTube Music (No API Key Required)"
                                        }
                                        <Label> {
                                            width: Fill,
                                            draw_text: { color: #9E988E, text_style: { font_size: 12.5 } }
                                            text: "Paste any public Spotify playlist/album/track URL, YouTube/YouTube Music playlist URL, or type an artist/song name to search YouTube immediately."
                                        }
                                        quick_sync_input = <FerricInput> {
                                            empty_text: "https://open.spotify.com/playlist/... or https://music.youtube.com/playlist?list=... or song name"
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 8,
                                            quick_spotify_btn = <FerricAccentPill> {
                                                height: 34,
                                                text: "Import Spotify Link"
                                                draw_icon: { svg_path: "M19.35 10.04C18.67 6.59 15.64 4 12 4C9.11 4 6.6 5.64 5.35 8.04C2.34 8.36 0 10.91 0 14C0 17.31 2.69 20 6 20H19C21.76 20 24 17.76 24 15C24 12.36 21.95 10.22 19.35 10.04Z" }
                                            }
                                            quick_youtube_btn = <FerricBtn> {
                                                height: 34,
                                                text: "Import YouTube Link"
                                                draw_icon: { svg_path: "M17 10.5V7C17 6.45 16.55 6 16 6H4C3.45 6 3 6.45 3 7V17C3 17.55 3.45 18 4 18H16C16.55 18 17 17.55 17 17V13.5L21 17.5V6.5L17 10.5Z", color: #D9541E }
                                            }
                                            quick_yt_search_btn = <FerricBtn> {
                                                height: 34,
                                                text: "Search YouTube"
                                                draw_icon: { svg_path: "M15.5 14H14.71L14.43 13.73C15.41 12.59 16 11.11 16 9.5C16 5.91 13.09 3 9.5 3C5.91 3 3 5.91 3 9.5C3 13.09 5.91 16 9.5 16C11.11 16 12.59 15.41 13.73 14.43L14 14.71V15.5L19 20.49L20.49 19L15.5 14ZM9.5 14C7.01 14 5 11.99 5 9.5C5 7.01 7.01 5 9.5 5C11.99 5 14 7.01 14 9.5C14 11.99 11.99 14 9.5 14Z", color: #D9541E }
                                            }
                                        }
                                    }

                                    // TIER 2A: SPOTIFY OFFICIAL OAUTH 2.0 PKCE
                                    <FerricCard> {
                                        flow: Down, spacing: 10,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }
                                                text: "Spotify Account Sign-In (Private Playlists & Liked Songs — OAuth 2.0 PKCE)"
                                            }
                                            spotify_state_lbl = <Label> {
                                                draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }
                                                text: "Not signed in"
                                            }
                                        }
                                        <Label> {
                                            width: Fill,
                                            draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }
                                            text: "Add Redirect URI http://127.0.0.1:43824/callback in Spotify Developer Dashboard, paste Client ID (or set FERRIC_SPOTIFY_CLIENT_ID), then sign in via browser."
                                        }
                                        spotify_id_input = <FerricInput> {
                                            empty_text: "Spotify Developer Client ID (optional if using Instant Link above)"
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 8,
                                            spotify_login_btn = <FerricAccentPill> { height: 34, text: "Sign in with Spotify" }
                                            spotify_sync_btn = <FerricBtn> { height: 34, text: "Sync Playlists" }
                                            copy_redirect_btn = <FerricBtn> { height: 34, text: "Copy Redirect URI" }
                                            open_spotify_dev_btn = <FerricBtn> { height: 34, text: "Spotify Portal" }
                                            spotify_logout_btn = <FerricBtn> { height: 34, text: "Disconnect" }
                                        }
                                    }

                                    // TIER 2B: YOUTUBE & YOUTUBE MUSIC OFFICIAL GOOGLE OAUTH 2.0 PKCE
                                    <FerricCard> {
                                        flow: Down, spacing: 10,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }
                                                text: "YouTube & YouTube Music Sign-In (Private Playlists — Google OAuth PKCE)"
                                            }
                                            yt_state_lbl = <Label> {
                                                draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }
                                                text: "Not signed in"
                                            }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 10,
                                            yt_id_input = <FerricInput> { empty_text: "Google OAuth Client ID (.apps.googleusercontent.com)" }
                                            yt_secret_input = <FerricInput> { empty_text: "Client Secret (optional for PKCE)" }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8,
                                            yt_login_btn = <FerricAccentPill> { height: 34, text: "Sign in with Google" }
                                            yt_sync_btn = <FerricBtn> { height: 34, text: "Sync YouTube" }
                                            open_google_dev_btn = <FerricBtn> { height: 34, text: "Google Console" }
                                            yt_logout_btn = <FerricBtn> { height: 34, text: "Disconnect" }
                                            oauth_cancel_btn = <FerricBtn> { visible: false, height: 34, text: "Cancel Login" }
                                            <View> { width: Fill, height: 1 }
                                            <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Music Only" }
                                            yt_cat_toggle = <FerricToggle> { is_on: true }
                                        }
                                    }

                                    // TIER 3: OPENSUBSONIC / NAVIDROME & LOCAL LOSSLESS MATCHER
                                    <FerricCard> {
                                        flow: Down, spacing: 10,
                                        <Label> {
                                            draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }
                                            text: "Self-Hosted Server (OpenSubsonic / Navidrome) & Local Matcher"
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 10,
                                            sub_url_input = <FerricInput> { empty_text: "https://music.example.com" }
                                            sub_user_input = <FerricInput> { empty_text: "Username" }
                                            sub_pass_input = <FerricInput> { empty_text: "Password" }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 8,
                                            sub_sync_btn = <FerricBtn> { height: 34, text: "Connect Subsonic" }
                                            match_local_btn = <FerricBtn> { height: 34, text: "Match Cloud Songs to Local Files" }
                                            rescan_btn = <FerricBtn> { height: 34, text: "Rescan Local Folders" }
                                        }
                                    }
                                }
                            }

                            // OPTIONAL COLLAPSIBLE RIGHT DRAWER (Up Next Queue + Quick Connect)
                            right_drawer = <FerricSurface> {
                                visible: false,
                                width: 288,
                                height: Fill,
                                padding: 16,
                                flow: Down,
                                spacing: 12,
                                draw_bg: { base_col: #070707, radius: 0.0 }

                                <View> {
                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 6,
                                    drawer_queue_tab = <FerricBtn> { height: 30, text: "Queue", draw_bg: { active: 1.0 } }
                                    drawer_connect_tab = <FerricBtn> { height: 30, text: "Connect" }
                                    <View> { width: Fill, height: 1 }
                                    drawer_close_btn = <FerricBtn> { height: 28, padding: { left: 10, right: 10 }, text: "Close" }
                                }

                                drawer_queue_view = <View> {
                                    visible: true,
                                    width: Fill, height: Fill, flow: Down, spacing: 8,
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                        <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 14.5 } }, text: "Up Next" }
                                        clear_queue_btn = <FerricBtn> { height: 26, padding: { left: 10, right: 10 }, text: "Clear" }
                                    }
                                    queue_sub_lbl = <Label> {
                                        draw_text: { color: #888279, text_style: { font_size: 11.5 } }
                                        text: "Gapless Deck B pre-buffered"
                                    }
                                    q_slot_0 = <FerricPlaylistBtn> { visible: false, height: 36, text: "" }
                                    q_slot_1 = <FerricPlaylistBtn> { visible: false, height: 36, text: "" }
                                    q_slot_2 = <FerricPlaylistBtn> { visible: false, height: 36, text: "" }
                                    q_slot_3 = <FerricPlaylistBtn> { visible: false, height: 36, text: "" }
                                    q_slot_4 = <FerricPlaylistBtn> { visible: false, height: 36, text: "" }
                                }

                                drawer_connect_view = <View> {
                                    visible: false,
                                    width: Fill, height: Fill, flow: Down, spacing: 10,
                                    <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 14.5 } }, text: "Ferric Connect" }
                                    <View> {
                                        width: Fill, height: Fit, align: { x: 0.5 }, margin: { top: 4, bottom: 4 }
                                        qr_view = <FerricQrView> { width: 148, height: 148 }
                                    }
                                    lan_ip_lbl = <Label> {
                                        width: Fill,
                                        draw_text: { color: #D9541E, text_style: { font_size: 11.5 } }
                                        text: "LAN: 127.0.0.1:43821"
                                    }
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 6,
                                        gen_pair_btn = <FerricBtn> { height: 30, text: "New QR" }
                                        copy_pair_btn = <FerricBtn> { height: 30, text: "Copy Link" }
                                    }
                                    peers_lbl = <Label> {
                                        width: Fill,
                                        draw_text: { color: #9E988E, text_style: { font_size: 11.5 } }
                                        text: "Listening for Ferric devices on local Wi-Fi..."
                                    }
                                }
                            }
                        }

                        <FerricDividerH> {}

                        // 3. DOCKED BOTTOM PLAYER BAR (100% Crisp Vector SVG Icons — FigCraft D1 Parity)
                        bottom_bar = <FerricSurface> {
                            width: Fill,
                            height: 82,
                            padding: { left: 18, right: 18 }
                            flow: Right,
                            align: { y: 0.5 }
                            spacing: 18,
                            draw_bg: { base_col: #060606, radius: 0.0 }

                            // Left: Cover Art + Song Title / Artist + Vector Heart Icon
                            <View> {
                                width: 300,
                                height: Fill,
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 12,
                                np_art_tile = <FerricArtTile> {
                                    width: 50,
                                    height: 50,
                                    cursor: Hand,
                                    draw_bg: { variant: 0.0, radius: 8.0 }
                                }
                                <View> {
                                    width: Fill,
                                    height: Fit,
                                    flow: Down,
                                    spacing: 3,
                                    np_title_lbl = <Label> {
                                        width: Fill,
                                        draw_text: { color: #F3EEE3, text_style: { font_size: 12.5 } }
                                        text: "Ready to Play"
                                    }
                                    np_artist_lbl = <Label> {
                                        width: Fill,
                                        draw_text: { color: #9E988E, text_style: { font_size: 11.5 } }
                                        text: "Ferric Hi-Fi Player"
                                    }
                                }
                                np_like_btn = <FerricIconBtn> {
                                    width: 32, height: 32,
                                    icon_walk: { width: 15, height: Fit }
                                    draw_icon: {
                                        svg_path: "M12 21.35L10.55 20.03C5.4 15.36 2 12.28 2 8.5C2 5.42 4.42 3 7.5 3C9.24 3 10.91 3.81 12 5.09C13.09 3.81 14.76 3 16.5 3C19.58 3 22 5.42 22 8.5C22 12.28 18.6 15.36 13.45 20.04L12 21.35Z"
                                        color: #6B665E
                                    }
                                }
                            }

                            // Center: 5 Pure Vector Transport Controls + Working Copper Seekbar
                            <View> {
                                width: Fill,
                                height: Fill,
                                flow: Down,
                                align: { x: 0.5, y: 0.5 }
                                spacing: 4,

                                <View> {
                                    width: Fit,
                                    height: Fit,
                                    flow: Right,
                                    align: { y: 0.5 }
                                    spacing: 10,
                                    shuffle_btn = <FerricIconBtn> {
                                        icon_walk: { width: 15, height: Fit }
                                        draw_icon: { svg_path: "M10.59 9.17L5.41 4L4 5.41L9.17 10.58L10.59 9.17ZM14.5 4L16.54 6.04L4 18.59L5.41 20L17.96 7.46L20 9.5V4H14.5ZM14.83 13.41L13.42 14.82L16.55 17.95L14.5 20H20V14.5L17.96 16.54L14.83 13.41Z" }
                                    }
                                    prev_btn = <FerricIconBtn> {
                                        icon_walk: { width: 17, height: Fit }
                                        draw_icon: { svg_path: "M6 6H8.5V18H6V6ZM10 12L18.5 18V6L10 12Z" }
                                    }
                                    play_pause_btn = <FerricPlayBtn> { width: 40, height: 40 }
                                    next_btn = <FerricIconBtn> {
                                        icon_walk: { width: 17, height: Fit }
                                        draw_icon: { svg_path: "M5.5 18L14 12L5.5 6V18ZM15.5 6V18H18V6H15.5Z" }
                                    }
                                    repeat_btn = <FerricIconBtn> {
                                        icon_walk: { width: 15, height: Fit }
                                        draw_icon: { svg_path: "M7 7H17V10L21 6L17 2V5H5V11H7V7ZM17 17H7V14L3 18L7 22V19H19V13H17V17Z" }
                                    }
                                }

                                <View> {
                                    width: Fill,
                                    height: 22,
                                    flow: Right,
                                    align: { y: 0.5 }
                                    spacing: 10,
                                    pos_lbl = <Label> {
                                        width: 44,
                                        draw_text: { color: #888279, text_style: { font_size: 11.0 } }
                                        text: "00:00"
                                    }
                                    seek_slider = <FerricBarSlider> { min: 0.0, max: 1.0, value: 0.0 }
                                    dur_btn = <Button> {
                                        width: 52,
                                        height: 20,
                                        padding: 0,
                                        align: { x: 1.0, y: 0.5 }
                                        text: "00:00"
                                        draw_text: { color: #888279, color_hover: #F3EEE3, text_style: { font_size: 11.0 } }
                                        draw_bg: { fn pixel(self) -> vec4 { return vec4(0.0, 0.0, 0.0, 0.0); } }
                                    }
                                }
                            }

                            // Right: Codec Tag + Crisp Vector Icon Actions (Lyrics, Queue, EQ, Connect, Volume)
                            <View> {
                                width: 326,
                                height: Fill,
                                flow: Right,
                                align: { x: 1.0, y: 0.5 }
                                spacing: 5,

                                codec_pill_btn = <FerricBtn> {
                                    height: 26,
                                    padding: { left: 9, right: 9 }
                                    text: "Lossless"
                                    draw_text: { color: #D9541E, color_hover: #F3EEE3, text_style: { font_size: 10.5 } }
                                }
                                bar_lyrics_btn = <FerricIconBtn> {
                                    width: 30, height: 30,
                                    icon_walk: { width: 15, height: Fit }
                                    draw_icon: { svg_path: "M12 14C13.66 14 15 12.66 15 11V5C15 3.34 13.66 2 12 2C10.34 2 9 3.34 9 5V11C9 12.66 10.34 14 12 14ZM17 11C17 13.76 14.76 16 12 16C9.24 16 7 13.76 7 11H5C5 14.53 7.61 17.43 11 17.92V21H13V17.92C16.39 17.43 19 14.53 19 11H17Z" }
                                }
                                bar_queue_btn = <FerricIconBtn> {
                                    width: 30, height: 30,
                                    icon_walk: { width: 15, height: Fit }
                                    draw_icon: { svg_path: "M3 6H15V8H3V6ZM3 10H15V12H3V10ZM3 14H11V16H3V14ZM15 13V19L20 16L15 13Z" }
                                }
                                bar_eq_btn = <FerricIconBtn> {
                                    width: 30, height: 30,
                                    icon_walk: { width: 15, height: Fit }
                                    draw_icon: { svg_path: "M5 4H7V11H9V13H3V11H5V4ZM5 15H7V20H5V15ZM11 4H13V7H15V9H9V7H11V4ZM11 11H13V20H11V11ZM17 4H19V14H21V16H15V14H17V4ZM17 18H19V20H17V18Z" }
                                }
                                bar_connect_btn = <FerricIconBtn> {
                                    width: 30, height: 30,
                                    icon_walk: { width: 15, height: Fit }
                                    draw_icon: { svg_path: "M4 6H20V4H4C2.9 4 2 4.9 2 6V17H0V20H14V17H4V6ZM23 8H17C16.45 8 16 8.45 16 9V19C16 19.55 16.45 20 17 20H23C23.55 20 24 19.55 24 19V9C24 8.45 23.55 8 23 8ZM22 17H18V10H22V17Z" }
                                }
                                vol_mute_btn = <FerricIconBtn> {
                                    width: 30, height: 30,
                                    icon_walk: { width: 15, height: Fit }
                                    draw_icon: { svg_path: "M3 9V15H7L12 20V4L7 9H3ZM16.5 12C16.5 10.23 15.48 8.71 14 7.97V16.02C15.48 15.29 16.5 13.77 16.5 12ZM14 3.23V5.29C16.89 6.15 19 8.83 19 12C19 15.17 16.89 17.85 14 18.71V20.77C18.01 19.86 21 16.28 21 12C21 7.72 18.01 4.14 14 3.23Z" }
                                }
                                vol_slider = <FerricBarSlider> {
                                    width: 82,
                                    min: 0.0,
                                    max: 1.0,
                                    value: 0.85
                                }
                            }
                        }
                    }

                    // OVERLAY 1: COMMAND PALETTE (Ctrl+K)
                    cmdk_modal = <View> {
                        visible: false,
                        width: Fill,
                        height: Fill,
                        align: { x: 0.5, y: 0.16 }
                        show_bg: true,
                        draw_bg: { fn pixel(self) -> vec4 { return vec4(0.0, 0.0, 0.0, 0.72); } }

                        <FerricCard> {
                            width: 520,
                            height: Fit,
                            padding: 20,
                            flow: Down,
                            spacing: 8,

                            <View> {
                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 } margin: { bottom: 4 }
                                <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }, text: "Command Palette" }
                                cmdk_close_btn = <FerricBtn> { height: 28, text: "Esc" }
                            }
                            cmd_demo_btn = <FerricNavItem> {
                                text: "Play Seamless Demo Suite (Test 6s Fade-In & Gapless)"
                                draw_icon: { svg_path: "M8 5V19L19 12L8 5Z", color: #D9541E }
                            }
                            cmd_glass_btn = <FerricNavItem> {
                                text: "Toggle Liquid Glass Theme On / Off"
                                draw_icon: { svg_path: "M12 2L2 9L12 16L22 9L12 2ZM12 18.54L4.62 12.81L3 14.07L12 21.07L21 14.07L19.37 12.8L12 18.54Z" }
                            }
                            cmd_fade_btn = <FerricNavItem> {
                                text: "Open Seamless Fade-In & Automix Studio"
                                draw_icon: { svg_path: "M3 10H5V14H3V10ZM7 6H9V18H7V6ZM11 2H13V22H11V2ZM15 6H17V18H15V6ZM19 10H21V14H19V10Z" }
                            }
                            cmd_eq_btn = <FerricNavItem> {
                                text: "Open 10-Band Parametric Equalizer & AutoEq"
                                draw_icon: { svg_path: "M5 4H7V11H9V13H3V11H5V4ZM5 15H7V20H5V15ZM11 4H13V7H15V9H9V7H11V4ZM11 11H13V20H11V11ZM17 4H19V14H21V16H15V14H17V4ZM17 18H19V20H17V18Z" }
                            }
                            cmd_accounts_btn = <FerricNavItem> {
                                text: "Import Spotify / YouTube Music or Sign In"
                                draw_icon: { svg_path: "M12 12C14.21 12 16 10.21 16 8C16 5.79 14.21 4 12 4C9.79 4 8 5.79 8 8C8 10.21 9.79 12 12 12ZM12 14C9.33 14 4 15.34 4 18V20H20V18C20 15.34 14.67 14 12 14Z" }
                            }
                            cmd_sleep30_btn = <FerricNavItem> {
                                text: "Start 30-Minute Sleep Timer (Smooth Fade-Out)"
                                draw_icon: { svg_path: "M12 3V13.55C11.41 13.21 10.73 13 10 13C7.79 13 6 14.79 6 17C6 19.21 7.79 21 10 21C12.21 21 14 19.21 14 17V7H18V3H12Z" }
                            }
                        }
                    }

                    // OVERLAY 2: SOFTWARE UPDATE POPUP (WallpaperLive Parity)
                    update_popup = <View> {
                        visible: false,
                        width: Fill,
                        height: Fill,
                        align: { x: 0.5, y: 0.5 }
                        show_bg: true,
                        draw_bg: { fn pixel(self) -> vec4 { return vec4(0.0, 0.0, 0.0, 0.76); } }

                        <FerricCard> {
                            width: 460,
                            height: Fit,
                            padding: 24,
                            flow: Down,
                            spacing: 12,

                            update_popup_title = <Label> {
                                draw_text: { color: #F3EEE3, text_style: { font_size: 18.0 } }
                                text: "Ferric Update Available"
                            }
                            update_popup_sub = <Label> {
                                width: Fill,
                                draw_text: { color: #9E988E, text_style: { font_size: 12.5 } }
                                text: "A newer signed release is ready to install."
                            }
                            update_popup_notes = <Label> {
                                width: Fill,
                                draw_text: { color: #ABA59B, text_style: { font_size: 12.0 } }
                                text: ""
                            }
                            <View> {
                                width: Fill, height: Fit, flow: Right, spacing: 8, margin: { top: 6 }
                                update_now_btn = <FerricAccentPill> { height: 36, text: "Update Now" }
                                update_whats_new_btn = <FerricBtn> { height: 36, text: "Release Notes" }
                                update_popup_close_btn = <FerricBtn> { height: 36, text: "Later" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum LibraryFilter {
    #[default]
    All,
    Liked,
    Local,
    Spotify,
    YouTube,
    Playlist(usize),
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum SortField {
    #[default]
    Artist,
    Title,
    Album,
    Duration,
}

impl SortField {
    fn next(self) -> Self {
        match self {
            Self::Artist => Self::Title,
            Self::Title => Self::Album,
            Self::Album => Self::Duration,
            Self::Duration => Self::Artist,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Artist => "Sort: Artist",
            Self::Title => "Sort: Title",
            Self::Album => "Sort: Album",
            Self::Duration => "Sort: Time",
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum SleepTimerMode {
    #[default]
    Off,
    Deadline(Instant),
    EndOfTrack,
}

enum BgMessage {
    ScanCompleted(Vec<Track>),
    LyricsLoaded {
        track_id: String,
        lrc_raw: String,
        parsed: SyncedLyrics,
    },
    PublicSyncCompleted {
        provider: &'static str,
        payload: Box<(Playlist, Vec<Track>)>,
    },
    PublicSyncFailed {
        provider: &'static str,
        error: String,
    },
    OAuthCompleted {
        kind: AccountKind,
        tokens: OAuthTokens,
        report: Box<CloudSyncReport>,
    },
    OAuthFailed {
        kind: AccountKind,
        error: String,
    },
    SubsonicSynced {
        tracks: Vec<Track>,
        playlists: Vec<(Playlist, Vec<Track>)>,
    },
    SubsonicFailed(String),
    UpdateChecked {
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
    all_tracks: Vec<Track>,
    #[rust]
    filtered_tracks: Rc<Vec<Track>>,
    #[rust]
    playlists: Vec<Playlist>,
    #[rust]
    queue: PlayQueue,
    #[rust]
    theme: ThemeConfig,
    #[rust]
    eq: EqConfig,
    #[rust]
    dsp: PlaybackDspSettings,
    #[rust]
    accounts: AccountSettings,
    #[rust]
    update_settings: UpdateSettings,
    #[rust]
    session_state: SessionResumeState,
    #[rust]
    available_update: Option<ReleaseInfo>,
    #[rust]
    update_popup_visible: bool,
    #[rust]
    update_download_in_flight: bool,
    #[rust]
    update_cancel: Option<Arc<AtomicBool>>,
    #[rust]
    oauth_cancel: Option<Arc<AtomicBool>>,
    #[rust]
    lease: Option<ActiveSpeakerLease>,
    #[rust]
    lan_beacon: Option<LanDiscoveryBeacon>,
    #[rust]
    pairing_uri: String,
    #[rust]
    lan_ip: String,
    #[rust]
    last_lan_broadcast: Option<Instant>,
    #[rust]
    synced_lyrics: SyncedLyrics,
    #[rust]
    lyrics_offset_ms: i64,
    #[rust]
    last_finished_seq: u64,
    #[rust]
    search_query: String,
    #[rust]
    active_filter: LibraryFilter,
    #[rust]
    sort_field: SortField,
    #[rust]
    active_page: LiveId,
    #[rust]
    show_remaining_time: bool,
    #[rust]
    pre_mute_volume: f32,
    #[rust]
    compact_mini_mode: bool,
    #[rust]
    right_drawer_open: bool,
    #[rust]
    cmdk_open: bool,
    #[rust]
    canvas_phase: f32,
    #[rust]
    np_canvas_animated: bool,
    #[rust]
    sleep_timer: SleepTimerMode,
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
        crate::widgets::live_design(cx);
        crate::track_list::live_design(cx);
    }
}

impl App {
    fn hide_native_makepad_caption(&self, cx: &mut Cx) {
        self.ui.view(id!(caption_bar)).set_visible(cx, false);
        self.ui.view(id!(windows_buttons)).set_visible(cx, false);
    }

    fn ensure_initialized(&mut self, cx: &mut Cx) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        self.np_canvas_animated = true;
        self.active_page = live_id!(page_library);
        self.pre_mute_volume = 0.85;
        self.hide_native_makepad_caption(cx);

        let db_path = app_data_dir().join("ferric.db");
        let db = LibraryDb::open(&db_path)
            .or_else(|_| LibraryDb::open_in_memory())
            .ok();

        if let Some(store) = &db {
            self.theme = store.load_theme().unwrap_or_default();
            self.eq = store.load_eq().unwrap_or_default();
            self.dsp = store.load_playback_dsp().unwrap_or_default();
            self.accounts = store.load_accounts().unwrap_or_default();
            self.update_settings = store.load_update_settings().unwrap_or_default();
            self.session_state = store.load_session_state().unwrap_or_default();
            self.all_tracks = store.list_tracks("").unwrap_or_default();
            self.playlists = store.list_playlists().unwrap_or_default();
        }
        self.queue.shuffle = self.session_state.shuffle;
        self.queue.repeat = self.session_state.repeat;
        self.db = db;

        let audio = AudioEngine::start(self.eq.clone())
            .unwrap_or_else(|_| AudioEngine::start_headless(self.eq.clone()));
        audio.send(EngineCommand::SetDspSettings(self.dsp.clone()));
        audio.send(EngineCommand::SetVolume(self.session_state.volume));
        self.audio = Some(audio);

        self.smtc = Some(SmtcBridge::try_init());
        let dev_id = format!("ferric-pc-{:08x}", fastrand::u32(..));
        self.lease = Some(ActiveSpeakerLease::new(dev_id.clone(), "Desktop-PC"));
        self.lan_ip = detect_local_lan_ip();
        self.lan_beacon = LanDiscoveryBeacon::bind_default(&dev_id).ok();

        let (tx, rx) = mpsc::channel();
        self.bg_tx = Some(tx);
        self.bg_rx = Some(rx);

        let _ = self
            .ui
            .image(id!(brand_icon))
            .load_png_from_data(cx, BRAND_ICON_PNG);

        self.regenerate_pairing_qr(cx);
        self.rebuild_filtered_tracks(cx);
        self.queue.set_tracks((*self.filtered_tracks).clone(), None);
        self.sync_controls_from_state(cx);
        self.apply_theme_live(cx);
        self.refresh_ui_labels(cx);
        self.tick_timer = cx.start_interval(0.10);

        if startup_load_demo() {
            self.load_and_play_demo_suite(cx);
        }

        if let Some(page_arg) = startup_page_override() {
            self.apply_startup_page_override(cx, page_arg);
        } else if self.update_settings.auto_check_on_startup {
            self.spawn_update_check(cx, false);
        }
    }

    fn apply_startup_page_override(&mut self, cx: &mut Cx, arg: &str) {
        match arg {
            "home" => self.switch_main_page(cx, live_id!(page_home)),
            "library" => self.switch_main_page(cx, live_id!(page_library)),
            "now" | "lyrics" => self.switch_main_page(cx, live_id!(page_now_playing)),
            "fade" | "playback" => self.switch_main_page(cx, live_id!(page_fade)),
            "eq" => self.switch_main_page(cx, live_id!(page_eq)),
            "connect" => self.switch_main_page(cx, live_id!(page_connect)),
            "appearance" => self.switch_main_page(cx, live_id!(page_appearance)),
            "accounts" => self.switch_main_page(cx, live_id!(page_accounts)),
            "oled" => {
                self.theme = ThemeConfig::default();
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
                self.switch_main_page(cx, live_id!(page_library));
            }
            "glass" => {
                self.theme.material.liquid_glass = true;
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
                self.switch_main_page(cx, live_id!(page_appearance));
            }
            "cmdk" => {
                self.cmdk_open = true;
                self.ui.view(id!(cmdk_modal)).set_visible(cx, true);
                self.ui.redraw(cx);
            }
            _ => {}
        }
    }

    fn switch_main_page(&mut self, cx: &mut Cx, target: LiveId) {
        self.active_page = target;
        let copper = vec4(0.851, 0.329, 0.118, 1.0);
        let cream = vec4(0.953, 0.933, 0.890, 1.0);
        let muted = vec4(0.620, 0.596, 0.557, 1.0);
        let icon_muted = vec4(0.533, 0.510, 0.475, 1.0);

        for (pid, nav_id) in [
            (live_id!(page_home), id!(nav_home)),
            (live_id!(page_library), id!(nav_library)),
            (live_id!(page_now_playing), id!(nav_now_playing)),
            (live_id!(page_fade), id!(nav_fade)),
            (live_id!(page_eq), id!(nav_eq)),
            (live_id!(page_connect), id!(nav_connect)),
            (live_id!(page_appearance), id!(nav_appearance)),
            (live_id!(page_accounts), id!(nav_accounts)),
        ] {
            let active = pid == target;
            self.ui.view(&[pid]).set_visible(cx, active);
            self.ui.button(nav_id).apply_over(
                cx,
                live! {
                    draw_bg: { active: (if active { 1.0 } else { 0.0 }) }
                    draw_icon: { color: (if active { copper } else { icon_muted }) }
                    draw_text: { color: (if active { cream } else { muted }) }
                },
            );
        }
        self.ui.redraw(cx);
    }

    fn open_right_drawer(&mut self, cx: &mut Cx, connect_tab: bool) {
        self.right_drawer_open = true;
        self.ui.view(id!(right_drawer)).set_visible(cx, true);
        self.ui
            .view(id!(drawer_queue_view))
            .set_visible(cx, !connect_tab);
        self.ui
            .view(id!(drawer_connect_view))
            .set_visible(cx, connect_tab);
        self.ui.button(id!(drawer_queue_tab)).apply_over(
            cx,
            live! { draw_bg: { active: (if !connect_tab { 1.0 } else { 0.0 }) } },
        );
        self.ui.button(id!(drawer_connect_tab)).apply_over(
            cx,
            live! { draw_bg: { active: (if connect_tab { 1.0 } else { 0.0 }) } },
        );
        self.ui.redraw(cx);
    }

    fn toggle_compact_mini_mode(&mut self, cx: &mut Cx) {
        self.compact_mini_mode = !self.compact_mini_mode;
        let show_full = !self.compact_mini_mode;
        self.ui.view(id!(workspace_row)).set_visible(cx, show_full);
        let new_size = if self.compact_mini_mode {
            dvec2(480.0, 148.0)
        } else {
            dvec2(1280.0, 780.0)
        };
        self.ui.window(id!(main_window)).resize(cx, new_size);
        self.ui.redraw(cx);
    }

    fn regenerate_pairing_qr(&mut self, cx: &mut Cx) {
        let tok = format!("{:016x}", fastrand::u64(..));
        let ep = format!("ed25519_{:08x}", fastrand::u32(..));
        let payload = PairingPayload {
            endpoint_id: ep,
            token_hex: tok,
            device_name: "Desktop-PC".to_string(),
        };
        self.pairing_uri = payload.to_uri();
        if let Ok(qr) = QrMatrix::encode(&self.pairing_uri) {
            self.ui
                .ferric_qr_view(id!(qr_view))
                .set_matrix(cx, qr.size, qr.modules.clone());
            self.ui
                .ferric_qr_view(id!(connect_qr_view))
                .set_matrix(cx, qr.size, qr.modules);
        }
        let lan_txt = format!("LAN: {}:43821 · Desktop-PC Active", self.lan_ip);
        self.ui.label(id!(lan_ip_lbl)).set_text(cx, &lan_txt);
        self.ui
            .label(id!(connect_lan_ip_lbl))
            .set_text(cx, &lan_txt);
    }

    fn rebuild_filtered_tracks(&mut self, cx: &mut Cx) {
        let base_pool: Vec<Track> = match self.active_filter {
            LibraryFilter::All => self.all_tracks.clone(),
            LibraryFilter::Liked => self
                .all_tracks
                .iter()
                .filter(|t| t.liked)
                .cloned()
                .collect(),
            LibraryFilter::Local => self
                .all_tracks
                .iter()
                .filter(|t| t.source == TrackSource::Local)
                .cloned()
                .collect(),
            LibraryFilter::Spotify => self
                .all_tracks
                .iter()
                .filter(|t| t.source == TrackSource::Spotify)
                .cloned()
                .collect(),
            LibraryFilter::YouTube => self
                .all_tracks
                .iter()
                .filter(|t| t.source == TrackSource::YouTubeMusic)
                .cloned()
                .collect(),
            LibraryFilter::Playlist(idx) => {
                if let Some(pl) = self.playlists.get(idx)
                    && let Some(db) = &self.db
                {
                    db.list_playlist_tracks(&pl.id).unwrap_or_default()
                } else {
                    Vec::new()
                }
            }
        };

        let q = self.search_query.trim().to_ascii_lowercase();
        let mut list: Vec<Track> = if q.is_empty() {
            base_pool
        } else {
            base_pool
                .into_iter()
                .filter(|t| {
                    t.title.to_ascii_lowercase().contains(&q)
                        || t.artist.to_ascii_lowercase().contains(&q)
                        || t.album.to_ascii_lowercase().contains(&q)
                })
                .collect()
        };

        match self.sort_field {
            SortField::Artist => list.sort_by(|a, b| {
                a.artist
                    .to_ascii_lowercase()
                    .cmp(&b.artist.to_ascii_lowercase())
                    .then_with(|| a.album.cmp(&b.album))
            }),
            SortField::Title => list.sort_by(|a, b| {
                a.title
                    .to_ascii_lowercase()
                    .cmp(&b.title.to_ascii_lowercase())
            }),
            SortField::Album => list.sort_by(|a, b| {
                a.album
                    .to_ascii_lowercase()
                    .cmp(&b.album.to_ascii_lowercase())
            }),
            SortField::Duration => list.sort_by_key(|t| std::cmp::Reverse(t.duration_ms)),
        }

        self.filtered_tracks = Rc::new(list);

        for (btn, active) in [
            (
                id!(flt_all_btn),
                matches!(self.active_filter, LibraryFilter::All),
            ),
            (
                id!(flt_liked_btn),
                matches!(self.active_filter, LibraryFilter::Liked),
            ),
            (
                id!(flt_local_btn),
                matches!(self.active_filter, LibraryFilter::Local),
            ),
            (
                id!(flt_spotify_btn),
                matches!(self.active_filter, LibraryFilter::Spotify),
            ),
            (
                id!(flt_youtube_btn),
                matches!(self.active_filter, LibraryFilter::YouTube),
            ),
            (
                id!(pl_all_btn),
                matches!(self.active_filter, LibraryFilter::All),
            ),
            (
                id!(pl_liked_btn),
                matches!(self.active_filter, LibraryFilter::Liked),
            ),
            (
                id!(pl_local_btn),
                matches!(self.active_filter, LibraryFilter::Local),
            ),
        ] {
            self.ui.button(btn).apply_over(
                cx,
                live! { draw_bg: { active: (if active { 1.0 } else { 0.0 }) } },
            );
        }

        let hero_title = match self.active_filter {
            LibraryFilter::All => "Your Library".to_string(),
            LibraryFilter::Liked => "Liked Songs".to_string(),
            LibraryFilter::Local => "Local Lossless".to_string(),
            LibraryFilter::Spotify => "Spotify Synced".to_string(),
            LibraryFilter::YouTube => "YouTube Music".to_string(),
            LibraryFilter::Playlist(idx) => self
                .playlists
                .get(idx)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "Playlist".to_string()),
        };
        self.ui.label(id!(hero_title_lbl)).set_text(cx, &hero_title);
        self.ui
            .button(id!(sort_cycle_btn))
            .set_text(cx, self.sort_field.label());
        self.refresh_ui_labels(cx);
    }

    fn sync_controls_from_state(&self, cx: &mut Cx) {
        let copper = vec4(0.851, 0.329, 0.118, 1.0);
        let muted = vec4(0.620, 0.596, 0.557, 1.0);

        // Seamless Fade-In & Automix
        self.ui
            .ferric_bar_slider(id!(xfade_slider))
            .set_value(cx, f64::from(self.dsp.crossfade_ms) / 1_000.0);
        self.ui.label(id!(xfade_val_lbl)).set_text(
            cx,
            &format!("{:.1} s", self.dsp.crossfade_ms as f32 / 1_000.0),
        );
        self.ui
            .ferric_bar_slider(id!(fadein_slider))
            .set_value(cx, f64::from(self.dsp.fade_in_play_ms));
        self.ui
            .label(id!(fadein_val_lbl))
            .set_text(cx, &format!("{} ms", self.dsp.fade_in_play_ms));

        for (btn, curve) in [
            (id!(curve_bass_btn), FadeCurve::BassSwapAutomix),
            (id!(curve_eqp_btn), FadeCurve::EqualPower),
            (id!(curve_velvet_btn), FadeCurve::SCurveVelvet),
        ] {
            let active = self.dsp.fade_curve == curve;
            self.ui.button(btn).apply_over(
                cx,
                live! { draw_bg: { active: (if active { 1.0 } else { 0.0 }) } },
            );
        }

        self.ui
            .ferric_toggle(id!(gapless_lock_toggle))
            .set_active(cx, self.dsp.gapless_album_lock);
        self.ui
            .ferric_toggle(id!(trim_silence_toggle))
            .set_active(cx, self.dsp.trim_silence);
        self.ui
            .ferric_toggle(id!(replay_gain_toggle))
            .set_active(cx, self.dsp.replay_gain);
        self.ui
            .ferric_toggle(id!(mono_toggle))
            .set_active(cx, self.dsp.mono_audio);

        for (btn, spd) in [
            (id!(spd_075_btn), 0.75_f32),
            (id!(spd_100_btn), 1.0_f32),
            (id!(spd_125_btn), 1.25_f32),
            (id!(spd_150_btn), 1.5_f32),
        ] {
            let active = (self.dsp.playback_speed - spd).abs() < 0.05;
            self.ui.button(btn).apply_over(
                cx,
                live! { draw_bg: { active: (if active { 1.0 } else { 0.0 }) } },
            );
        }

        // 10-Band Parametric Equalizer
        self.ui
            .ferric_toggle(id!(eq_enable_toggle))
            .set_active(cx, self.eq.enabled);
        self.ui
            .ferric_bar_slider(id!(eq_preamp_slider))
            .set_value(cx, f64::from(self.eq.preamp_db));
        self.ui
            .label(id!(eq_preamp_lbl))
            .set_text(cx, &format!("{:+.1} dB", self.eq.preamp_db));

        let mut gains = [0.0_f32; 10];
        for (idx, (slider_id, val_id)) in eq_band_ids().into_iter().enumerate() {
            if let Some(band) = self.eq.bands.get(idx) {
                gains[idx] = band.gain_db;
                self.ui
                    .ferric_vert_slider(slider_id)
                    .set_value(cx, f64::from(band.gain_db));
                self.ui
                    .label(val_id)
                    .set_text(cx, &format!("{:+.1}", band.gain_db));
            }
        }
        self.ui.ferric_eq_graph(id!(eq_graph)).set_bands(
            cx,
            self.eq.enabled,
            self.eq.preamp_db,
            &gains,
        );
        self.ui.label(id!(eq_status_lbl)).set_text(
            cx,
            &format!(
                "Preset: {} · Double-click any fader to reset",
                self.eq.preset_name
            ),
        );

        // Appearance & Liquid Glass
        self.ui
            .ferric_toggle(id!(liquid_glass_toggle))
            .set_active(cx, self.theme.material.liquid_glass);
        self.ui
            .ferric_bar_slider(id!(lg_ior_slider))
            .set_value(cx, f64::from(self.theme.material.ior));
        self.ui
            .ferric_bar_slider(id!(lg_disp_slider))
            .set_value(cx, f64::from(self.theme.material.dispersion));
        self.ui
            .ferric_bar_slider(id!(lg_rim_slider))
            .set_value(cx, f64::from(self.theme.material.specular_rim));
        self.ui
            .ferric_bar_slider(id!(lg_rad_slider))
            .set_value(cx, f64::from(self.theme.corner_radius_px));
        self.ui
            .label(id!(lg_ior_lbl))
            .set_text(cx, &format!("{:.2}", self.theme.material.ior));
        self.ui.label(id!(lg_disp_lbl)).set_text(
            cx,
            &format!("{:.0}%", self.theme.material.dispersion * 100.0),
        );
        self.ui.label(id!(lg_rim_lbl)).set_text(
            cx,
            &format!(
                "{:.0}% · {:.0}px",
                self.theme.material.specular_rim * 100.0,
                self.theme.corner_radius_px
            ),
        );

        // Volume & Transport Icon States
        self.ui
            .ferric_bar_slider(id!(vol_slider))
            .set_value(cx, f64::from(self.session_state.volume));
        self.ui.button(id!(vol_mute_btn)).apply_over(
            cx,
            live! {
                draw_icon: { color: (if self.session_state.volume < 0.01 { copper } else { muted }) }
            },
        );
        self.ui.button(id!(shuffle_btn)).apply_over(
            cx,
            live! {
                draw_bg: { active: (if self.queue.shuffle { 1.0 } else { 0.0 }) }
                draw_icon: { color: (if self.queue.shuffle { copper } else { muted }) }
            },
        );
        let rep_active = self.queue.repeat != RepeatMode::Off;
        let rep_col = match self.queue.repeat {
            RepeatMode::Off => muted,
            RepeatMode::All => copper,
            RepeatMode::One => vec4(0.953, 0.720, 0.350, 1.0),
        };
        self.ui.button(id!(repeat_btn)).apply_over(
            cx,
            live! {
                draw_bg: { active: (if rep_active { 1.0 } else { 0.0 }) }
                draw_icon: { color: (rep_col) }
            },
        );

        // Accounts & Updates
        self.ui
            .text_input(id!(spotify_id_input))
            .set_text(cx, &self.accounts.spotify_client_id);
        self.ui
            .text_input(id!(yt_id_input))
            .set_text(cx, &self.accounts.youtube_client_id);
        self.ui
            .text_input(id!(yt_secret_input))
            .set_text(cx, &self.accounts.youtube_client_secret);
        self.ui
            .ferric_toggle(id!(yt_cat_toggle))
            .set_active(cx, self.accounts.youtube_music_category_only);
        self.ui
            .text_input(id!(sub_url_input))
            .set_text(cx, &self.accounts.subsonic_url);
        self.ui
            .text_input(id!(sub_user_input))
            .set_text(cx, &self.accounts.subsonic_username);
        self.ui
            .ferric_toggle(id!(auto_update_toggle))
            .set_active(cx, self.update_settings.auto_check_on_startup);
    }

    fn apply_theme_live(&self, cx: &mut Cx) {
        let g = &self.theme.material;
        let glass_f = if g.liquid_glass { 1.0_f32 } else { 0.0_f32 };
        let canvas = rgba_vec4(&self.theme.canvas_hex);
        let surface = rgba_vec4(&self.theme.surface_hex);
        let border = rgba_vec4(&self.theme.border_hex);
        let accent = rgba_vec4(&self.theme.accent_hex);
        let rad = if g.liquid_glass {
            self.theme.corner_radius_px.max(12.0)
        } else {
            0.0
        };

        self.ui.view(id!(body)).apply_over(
            cx,
            live! {
                draw_bg: {
                    canvas_col: (canvas),
                    glass_bg: (glass_f),
                    accent_col: (accent)
                }
            },
        );

        let panel_defs = [
            (id!(top_bar), vec4(0.024, 0.024, 0.024, 1.0)),
            (id!(left_sidebar), vec4(0.031, 0.031, 0.031, 1.0)),
            (id!(center_stage), canvas),
            (id!(right_drawer), vec4(0.031, 0.031, 0.031, 1.0)),
            (id!(bottom_bar), vec4(0.027, 0.027, 0.027, 1.0)),
        ];
        for (panel_id, flat_col) in panel_defs {
            let chosen_base = if g.liquid_glass { surface } else { flat_col };
            self.ui.view(panel_id).apply_over(
                cx,
                live! {
                    draw_bg: {
                        base_col: (chosen_base),
                        border_col: (border),
                        accent_col: (accent),
                        glass_enabled: (glass_f),
                        glass_ior: (g.ior),
                        glass_disp: (g.dispersion),
                        glass_rim: (g.specular_rim),
                        glass_scrim: (g.scrim_alpha),
                        radius: (rad)
                    }
                },
            );
        }

        self.ui.view(id!(lg_preview)).apply_over(
            cx,
            live! {
                draw_bg: {
                    ior: (g.ior),
                    dispersion: (g.dispersion),
                    blur_amt: (g.frost_blur_px / 28.0),
                    rim_amt: (g.specular_rim),
                    scrim_amt: (g.scrim_alpha),
                    radius_px: (self.theme.corner_radius_px.max(16.0)),
                    glass_on: (glass_f)
                }
            },
        );

        if let Some(db) = &self.db {
            let _ = db.save_theme(&self.theme);
        }
        self.ui.redraw(cx);
    }

    fn apply_dsp_live(&self, cx: &mut Cx) {
        if let Some(audio) = &self.audio {
            audio.send(EngineCommand::SetDspSettings(self.dsp.clone()));
        }
        if let Some(db) = &self.db {
            let _ = db.save_playback_dsp(&self.dsp);
        }
        self.sync_next_deck_to_engine();
        self.refresh_ui_labels(cx);
    }

    fn apply_eq_live(&self, cx: &mut Cx) {
        if let Some(audio) = &self.audio {
            audio.send(EngineCommand::SetEq(self.eq.clone()));
        }
        let mut gains = [0.0_f32; 10];
        for (i, b) in self.eq.bands.iter().enumerate().take(10) {
            gains[i] = b.gain_db;
        }
        self.ui.ferric_eq_graph(id!(eq_graph)).set_bands(
            cx,
            self.eq.enabled,
            self.eq.preamp_db,
            &gains,
        );
        if let Some(db) = &self.db {
            let _ = db.save_eq(&self.eq);
        }
        self.ui.redraw(cx);
    }

    fn refresh_ui_labels(&self, cx: &mut Cx) {
        let count = self.filtered_tracks.len();
        let total_ms: u64 = self.filtered_tracks.iter().map(|t| t.duration_ms).sum();
        let mins = total_ms / 60_000;
        let xfade_txt = if self.dsp.crossfade_ms == 0 {
            "Pure Gapless (0s)".to_string()
        } else {
            format!(
                "{:.1}s {} Fade-In",
                self.dsp.crossfade_ms as f32 / 1_000.0,
                self.dsp.fade_curve.label()
            )
        };
        self.ui
            .label(id!(library_sub_lbl))
            .set_text(cx, &format!("{count} songs · {mins} min · {xfade_txt}"));
        self.ui.label(id!(home_dsp_lbl)).set_text(
            cx,
            &format!(
                "{} · {}ms Play Fade-In · {}",
                xfade_txt,
                self.dsp.fade_in_play_ms,
                if self.dsp.trim_silence {
                    "Skip Silence On"
                } else {
                    "Skip Silence Off"
                }
            ),
        );
        self.ui.label(id!(home_sub_lbl)).set_text(
            cx,
            &format!(
                "{} tracks in library · {} active",
                self.all_tracks.len(),
                xfade_txt
            ),
        );
        self.ui.label(id!(stats_capsule_lbl)).set_text(
            cx,
            &format!(
                "{} songs played · {} min listened",
                self.session_state.total_tracks_played,
                self.session_state.total_listened_secs / 60
            ),
        );

        let sp_ok = self.accounts.spotify_connected;
        let yt_ok = self.accounts.youtube_connected;
        self.ui.button(id!(top_spotify_btn)).apply_over(
            cx,
            live! { draw_bg: { active: (if sp_ok { 1.0 } else { 0.0 }) } },
        );
        self.ui.button(id!(top_youtube_btn)).apply_over(
            cx,
            live! { draw_bg: { active: (if yt_ok { 1.0 } else { 0.0 }) } },
        );

        self.ui.label(id!(spotify_state_lbl)).set_text(
            cx,
            &if sp_ok {
                format!(
                    "Connected as {} ({} playlists)",
                    self.accounts.spotify_user_name, self.accounts.spotify_playlist_count
                )
            } else {
                "Not signed in".to_string()
            },
        );
        self.ui.label(id!(yt_state_lbl)).set_text(
            cx,
            &if yt_ok {
                format!(
                    "Connected as {} ({} playlists)",
                    self.accounts.youtube_channel_name, self.accounts.youtube_playlist_count
                )
            } else {
                "Not signed in".to_string()
            },
        );

        // Dynamic Sidebar Playlists
        let pl_slots = [
            id!(pl_slot_0),
            id!(pl_slot_1),
            id!(pl_slot_2),
            id!(pl_slot_3),
            id!(pl_slot_4),
        ];
        for (i, slot) in pl_slots.into_iter().enumerate() {
            if let Some(pl) = self.playlists.get(i) {
                self.ui.button(slot).set_visible(cx, true);
                self.ui
                    .button(slot)
                    .set_text(cx, &format!("{} ({})", pl.name, pl.track_count));
                let active = matches!(self.active_filter, LibraryFilter::Playlist(sel) if sel == i);
                self.ui.button(slot).apply_over(
                    cx,
                    live! { draw_bg: { active: (if active { 1.0 } else { 0.0 }) } },
                );
            } else {
                self.ui.button(slot).set_visible(cx, false);
            }
        }

        // Home Quick Picks
        let rec_slots = [
            id!(recent_btn_0),
            id!(recent_btn_1),
            id!(recent_btn_2),
            id!(recent_btn_3),
        ];
        let mut quick_tracks: Vec<&Track> = self
            .session_state
            .recent_track_ids
            .iter()
            .filter_map(|id| self.all_tracks.iter().find(|t| &t.id == id))
            .take(4)
            .collect();
        if quick_tracks.is_empty() {
            quick_tracks = self.all_tracks.iter().take(4).collect();
        }
        for (i, btn_id) in rec_slots.into_iter().enumerate() {
            if let Some(t) = quick_tracks.get(i) {
                self.ui.button(btn_id).set_visible(cx, true);
                self.ui.button(btn_id).set_text(
                    cx,
                    &format!(
                        "{}. {} — {} ({})",
                        i + 1,
                        t.title,
                        t.artist,
                        format_duration_ms(t.duration_ms)
                    ),
                );
            } else if i == 0 {
                self.ui.button(btn_id).set_visible(cx, true);
                self.ui.button(btn_id).set_text(
                    cx,
                    "Click 'Play Demo' above to experience Seamless Fade-In right now",
                );
            } else {
                self.ui.button(btn_id).set_visible(cx, false);
            }
        }

        // Queue Drawer Slots
        let q_slots = [
            id!(q_slot_0),
            id!(q_slot_1),
            id!(q_slot_2),
            id!(q_slot_3),
            id!(q_slot_4),
        ];
        let upcoming = self.queue.peek_upcoming(5);
        for (i, slot) in q_slots.into_iter().enumerate() {
            if let Some(t) = upcoming.get(i) {
                self.ui.button(slot).set_visible(cx, true);
                self.ui
                    .button(slot)
                    .set_text(cx, &format!("{}. {} · {}", i + 1, t.title, t.artist));
            } else {
                self.ui.button(slot).set_visible(cx, false);
            }
        }
    }

    fn sync_next_deck_to_engine(&self) {
        let Some(audio) = &self.audio else {
            return;
        };
        let next_opt = if self.queue.repeat == RepeatMode::One {
            self.queue.current()
        } else {
            self.queue.peek_upcoming(1).into_iter().next()
        };
        if let Some(next_track) = next_opt {
            let same_album = self.queue.current().is_some_and(|cur| {
                !cur.album.is_empty()
                    && cur.album != "Unknown Album"
                    && cur.album == next_track.album
                    && cur.id != next_track.id
            });
            audio.send(EngineCommand::QueueNextTrack {
                path: next_track.path.clone(),
                same_album_consecutive: same_album,
            });
        } else {
            audio.send(EngineCommand::QueueNextTrack {
                path: None,
                same_album_consecutive: false,
            });
        }
    }

    fn play_track(&mut self, cx: &mut Cx, track: Track) {
        let Some(path) = track.path.clone() else {
            self.switch_main_page(cx, live_id!(page_accounts));
            self.ui.label(id!(oauth_status_lbl)).set_text(
                cx,
                &format!(
                    "'{}' is a cloud catalog entry. Add your local music folder and click 'Match Cloud Songs to Local Files', or open its official video clip in Now Playing.",
                    track.title
                ),
            );
            return;
        };

        if let Some(audio) = &self.audio {
            audio.send(EngineCommand::LoadAndPlayTrack {
                path,
                replay_gain_db: track.replay_gain_db,
            });
        }
        self.sync_next_deck_to_engine();
        self.update_now_playing_metadata(cx, &track);
    }

    fn update_now_playing_metadata(&mut self, cx: &mut Cx, track: &Track) {
        self.session_state.record_play(&track.id, track.duration_ms);
        if let Some(db) = &self.db {
            let _ = db.save_session_state(&self.session_state);
        }

        let out_rate = self
            .audio
            .as_ref()
            .map_or(48_000, |a| a.telemetry().output_rate);
        self.ui.label(id!(np_title_lbl)).set_text(cx, &track.title);
        self.ui
            .label(id!(np_artist_lbl))
            .set_text(cx, &track.artist);
        self.ui
            .label(id!(np_stage_title_lbl))
            .set_text(cx, &track.title);
        self.ui
            .label(id!(np_stage_artist_lbl))
            .set_text(cx, &format!("{} · {}", track.artist, track.album));
        self.ui
            .button(id!(codec_pill_btn))
            .set_text(cx, &track.quality_badge());
        let like_col = if track.liked {
            vec4(0.851, 0.329, 0.118, 1.0)
        } else {
            vec4(0.420, 0.400, 0.369, 1.0)
        };
        self.ui
            .button(id!(np_like_btn))
            .apply_over(cx, live! { draw_icon: { color: (like_col) } });
        self.ui
            .ferric_play_btn(id!(play_pause_btn))
            .set_playing(cx, true);
        self.ui.label(id!(signal_path_lbl)).set_text(
            cx,
            &format!(
                "Signal Path: {} -> {:.1}s {} -> 10-Band Biquad EQ -> WASAPI {:.1} kHz",
                track.telemetry_label(),
                self.dsp.crossfade_ms as f32 / 1_000.0,
                self.dsp.fade_curve.label(),
                out_rate as f32 / 1_000.0
            ),
        );

        if let Some(smtc) = &mut self.smtc {
            smtc.try_attach_late();
            smtc.set_metadata(&track.title, &track.artist, &track.album, track.duration_ms);
            smtc.set_playing(true);
        }

        self.lyrics_offset_ms = 0;
        self.load_lyrics_for_track(cx, track);
        self.refresh_ui_labels(cx);
        self.ui.redraw(cx);
    }

    fn load_lyrics_for_track(&mut self, cx: &mut Cx, track: &Track) {
        self.synced_lyrics = SyncedLyrics::default();
        for lbl in [
            id!(lyric_prev2_lbl),
            id!(lyric_prev_lbl),
            id!(lyric_next_lbl),
            id!(lyric_next2_lbl),
        ] {
            self.ui.label(lbl).set_text(cx, "");
        }
        self.ui
            .label(id!(lyric_active_lbl))
            .set_text(cx, "Searching free lyrics (LRCLIB + Lyrics.ovh)...");

        if let Some(db) = &self.db
            && let Ok(Some(cached)) = db.get_cached_lyrics(&track.id)
        {
            self.synced_lyrics =
                SyncedLyrics::parse_with_context(&cached, track.duration_ms, "Cached");
            self.ui
                .label(id!(lyrics_source_lbl))
                .set_text(cx, "Lyrics · Loaded from Local Cache");
            self.update_lyrics_display(cx, 0);
            return;
        }

        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let track_id = track.id.clone();
        let artist = track.artist.clone();
        let title = track.title.clone();
        let album = track.album.clone();
        let dur_ms = track.duration_ms;
        let local_path = track.path.clone();

        thread::spawn(move || {
            if let Ok(Some((parsed, raw))) =
                fetch_best_free_lyrics(&artist, &title, &album, dur_ms, local_path.as_deref())
            {
                let _ = tx.send(BgMessage::LyricsLoaded {
                    track_id,
                    lrc_raw: raw,
                    parsed,
                });
            } else {
                let _ = tx.send(BgMessage::LyricsLoaded {
                    track_id,
                    lrc_raw: String::new(),
                    parsed: SyncedLyrics::default(),
                });
            }
        });
    }

    fn update_lyrics_display(&self, cx: &mut Cx, raw_pos_ms: u64) {
        if self.synced_lyrics.lines.is_empty() {
            return;
        }
        let shifted = (raw_pos_ms as i64 + self.lyrics_offset_ms).max(0) as u64;
        let lines = &self.synced_lyrics.lines;
        let idx = match lines.binary_search_by_key(&shifted, |l| l.timestamp_ms) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        };
        let get_txt = |opt_i: Option<usize>| -> &str {
            opt_i
                .and_then(|i| lines.get(i))
                .map_or("", |l| l.text.as_str())
        };
        self.ui
            .label(id!(lyric_prev2_lbl))
            .set_text(cx, get_txt(idx.checked_sub(2)));
        self.ui
            .label(id!(lyric_prev_lbl))
            .set_text(cx, get_txt(idx.checked_sub(1)));
        self.ui
            .label(id!(lyric_active_lbl))
            .set_text(cx, get_txt(Some(idx)));
        self.ui
            .label(id!(lyric_next_lbl))
            .set_text(cx, get_txt(Some(idx + 1)));
        self.ui
            .label(id!(lyric_next2_lbl))
            .set_text(cx, get_txt(Some(idx + 2)));
    }

    fn load_and_play_demo_suite(&mut self, cx: &mut Cx) {
        let demo_dir = app_data_dir().join("demo_tracks");
        if let Ok(tracks) = ensure_demo_tracks(&demo_dir) {
            if let Some(db) = &mut self.db {
                let _ = db.upsert_tracks(&tracks);
                self.all_tracks = db.list_tracks("").unwrap_or_default();
            } else {
                self.all_tracks = tracks;
            }
            self.active_filter = LibraryFilter::All;
            self.rebuild_filtered_tracks(cx);
            if let Some(first) = self.filtered_tracks.first().cloned() {
                self.queue
                    .set_tracks((*self.filtered_tracks).clone(), Some(0));
                self.play_track(cx, first);
            }
        }
    }

    fn spawn_folder_scan(&self, folders: Vec<PathBuf>) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        thread::spawn(move || {
            let mut gathered = Vec::new();
            for folder in folders {
                let mut found = scan_local_folder(&folder);
                gathered.append(&mut found);
            }
            let _ = tx.send(BgMessage::ScanCompleted(gathered));
        });
    }

    fn start_public_sync(&self, cx: &mut Cx, mode: &'static str, raw_input: String) {
        let trimmed = raw_input.trim().to_string();
        if trimmed.is_empty() {
            self.ui.label(id!(oauth_status_lbl)).set_text(
                cx,
                "Paste a public Spotify or YouTube URL (or type a song/artist name) first.",
            );
            return;
        }
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.ui
            .label(id!(oauth_status_lbl))
            .set_text(cx, &format!("Fetching {mode} metadata..."));

        thread::spawn(move || {
            let result = match mode {
                "Spotify" => import_spotify_public_url(&trimmed),
                "YouTube" => import_youtube_public_url(&trimmed),
                _ => search_youtube_public(&trimmed, 25).map(|tracks| {
                    let pl = Playlist {
                        id: format!("yt:search:{}", trimmed.to_ascii_lowercase()),
                        name: format!("YouTube: {trimmed}"),
                        source: TrackSource::YouTubeMusic,
                        etag: None,
                        track_count: tracks.len(),
                        description: "Instant YouTube Music Search".to_string(),
                    };
                    (pl, tracks)
                }),
            };
            match result {
                Ok(pair) => {
                    let _ = tx.send(BgMessage::PublicSyncCompleted {
                        provider: mode,
                        payload: Box::new(pair),
                    });
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::PublicSyncFailed {
                        provider: mode,
                        error: err.to_string(),
                    });
                }
            }
        });
    }

    fn start_oauth_login(&mut self, cx: &mut Cx, kind: AccountKind) {
        let mut spotify_id = self
            .ui
            .text_input(id!(spotify_id_input))
            .text()
            .trim()
            .to_string();
        if spotify_id.is_empty()
            && let Ok(env_id) = std::env::var("FERRIC_SPOTIFY_CLIENT_ID")
        {
            spotify_id = env_id.trim().to_string();
        }
        let mut yt_id = self
            .ui
            .text_input(id!(yt_id_input))
            .text()
            .trim()
            .to_string();
        if yt_id.is_empty()
            && let Ok(env_id) = std::env::var("FERRIC_GOOGLE_CLIENT_ID")
        {
            yt_id = env_id.trim().to_string();
        }
        let yt_secret = self
            .ui
            .text_input(id!(yt_secret_input))
            .text()
            .trim()
            .to_string();

        self.accounts.spotify_client_id = spotify_id.clone();
        self.accounts.youtube_client_id = yt_id.clone();
        self.accounts.youtube_client_secret = yt_secret.clone();
        if let Some(db) = &self.db {
            let _ = db.save_accounts(&self.accounts);
        }

        let client_id = match kind {
            AccountKind::Spotify => spotify_id,
            AccountKind::YouTubeMusic => yt_id,
        };
        if client_id.is_empty() {
            self.ui.label(id!(oauth_status_lbl)).set_text(
                cx,
                "For private OAuth login, paste a free Developer Client ID (or use Instant Sync above with zero keys!).",
            );
            return;
        }

        if let Some(prev) = self.oauth_cancel.take() {
            prev.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.oauth_cancel = Some(Arc::clone(&cancel));

        let pkce = generate_pkce_pair();
        let listener = match bind_loopback_callback(
            DEFAULT_OAUTH_CALLBACK_PORT,
            DEFAULT_OAUTH_CALLBACK_PATH,
            &pkce.state,
        ) {
            Ok(l) => l,
            Err(err) => {
                self.ui
                    .label(id!(oauth_status_lbl))
                    .set_text(cx, &format!("Loopback listener error: {err}"));
                return;
            }
        };

        let redirect_uri = listener.redirect_uri.clone();
        let auth_url = match kind {
            AccountKind::Spotify => spotify_authorize_url(&client_id, &redirect_uri, &pkce),
            AccountKind::YouTubeMusic => google_authorize_url(&client_id, &redirect_uri, &pkce),
        };

        if let Err(err) = open_allowed_url(&auth_url) {
            self.ui
                .label(id!(oauth_status_lbl))
                .set_text(cx, &format!("Could not launch browser: {err}"));
            return;
        }

        self.ui.button(id!(oauth_cancel_btn)).set_visible(cx, true);
        self.ui.label(id!(oauth_status_lbl)).set_text(
            cx,
            &format!("Waiting for browser sign-in on {redirect_uri}..."),
        );

        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let music_only = self.accounts.youtube_music_category_only;
        let verifier = pkce.verifier;

        thread::spawn(move || {
            let outcome: Result<(OAuthTokens, CloudSyncReport), String> = (|| {
                let code = listener
                    .wait_for_auth_code(&cancel, Duration::from_secs(180))
                    .map_err(|e| e.to_string())?;
                match kind {
                    AccountKind::Spotify => {
                        let tokens =
                            spotify_exchange_code(&client_id, &code, &redirect_uri, &verifier)
                                .map_err(|e| e.to_string())?;
                        let report = sync_spotify_catalog(&tokens.access_token)
                            .map_err(|e| e.to_string())?;
                        Ok((tokens, report))
                    }
                    AccountKind::YouTubeMusic => {
                        let sec = if yt_secret.is_empty() {
                            None
                        } else {
                            Some(yt_secret.as_str())
                        };
                        let tokens =
                            google_exchange_code(&client_id, sec, &code, &redirect_uri, &verifier)
                                .map_err(|e| e.to_string())?;
                        let report =
                            sync_youtube_catalog(&tokens.access_token, music_only, &HashMap::new())
                                .map_err(|e| e.to_string())?;
                        Ok((tokens, report))
                    }
                }
            })();

            match outcome {
                Ok((tokens, report)) => {
                    let _ = tx.send(BgMessage::OAuthCompleted {
                        kind,
                        tokens,
                        report: Box::new(report),
                    });
                }
                Err(error) => {
                    let _ = tx.send(BgMessage::OAuthFailed { kind, error });
                }
            }
        });
    }

    fn sync_existing_cloud_account(&mut self, cx: &mut Cx, kind: AccountKind) {
        let token = match kind {
            AccountKind::Spotify => self.accounts.spotify_access_token.clone(),
            AccountKind::YouTubeMusic => self.accounts.youtube_access_token.clone(),
        };
        if token.is_empty() {
            self.start_oauth_login(cx, kind);
            return;
        }
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let music_only = self.accounts.youtube_music_category_only;
        self.ui
            .label(id!(oauth_status_lbl))
            .set_text(cx, "Syncing cloud playlists and liked songs...");

        thread::spawn(move || {
            let res = match kind {
                AccountKind::Spotify => sync_spotify_catalog(&token),
                AccountKind::YouTubeMusic => {
                    sync_youtube_catalog(&token, music_only, &HashMap::new())
                }
            };
            match res {
                Ok(report) => {
                    let _ = tx.send(BgMessage::OAuthCompleted {
                        kind,
                        tokens: OAuthTokens {
                            access_token: token,
                            refresh_token: None,
                            expires_in_secs: 3600,
                        },
                        report: Box::new(report),
                    });
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::OAuthFailed {
                        kind,
                        error: err.to_string(),
                    });
                }
            }
        });
    }

    fn spawn_update_check(&self, cx: &mut Cx, manual: bool) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        if manual {
            self.ui
                .label(id!(update_status_lbl))
                .set_text(cx, "Checking GitHub Releases for updates...");
        }
        thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let result = check_latest_release(APP_VERSION, &cancel);
            let _ = tx.send(BgMessage::UpdateChecked { manual, result });
        });
    }

    fn show_update_popup(&mut self, cx: &mut Cx, info: &ReleaseInfo) {
        self.update_popup_visible = true;
        self.ui.view(id!(update_popup)).set_visible(cx, true);
        self.ui
            .label(id!(update_popup_title))
            .set_text(cx, &format!("Ferric {} is Available", info.tag));
        self.ui.label(id!(update_popup_sub)).set_text(
            cx,
            &format!(
                "Installed: v{APP_VERSION} -> Latest: {} (SHA-256 verified MSI)",
                info.tag
            ),
        );
        self.ui
            .label(id!(update_popup_notes))
            .set_text(cx, &info.summary);
        self.ui.redraw(cx);
    }

    fn dismiss_update_popup(&mut self, cx: &mut Cx) {
        if let Some(cancel) = self.update_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.update_download_in_flight = false;
        self.update_popup_visible = false;
        self.ui.view(id!(update_popup)).set_visible(cx, false);
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
        self.ui
            .button(id!(update_now_btn))
            .set_text(cx, "Downloading...");

        let dest_dir = std::env::temp_dir().join("ferric-updates");
        thread::spawn(move || {
            let progress_tx = tx.clone();
            let res = download_verified_installer(
                &info.tag,
                APP_VERSION,
                &dest_dir,
                &cancel,
                move |pct| {
                    let _ =
                        progress_tx.send(BgMessage::UpdateDownload(DownloadEvent::Progress(pct)));
                },
            );
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
        // 1. Drain background messages
        let mut messages = Vec::new();
        if let Some(rx) = &self.bg_rx {
            while let Ok(msg) = rx.try_recv() {
                messages.push(msg);
            }
        }
        for msg in messages {
            match msg {
                BgMessage::ScanCompleted(tracks) => {
                    if let Some(db) = &mut self.db {
                        let _ = db.upsert_tracks(&tracks);
                        let _ = db.match_cloud_tracks_to_local();
                        self.all_tracks = db.list_tracks("").unwrap_or_default();
                    } else {
                        self.all_tracks = tracks;
                    }
                    self.rebuild_filtered_tracks(cx);
                    if self.queue.is_empty() {
                        self.queue.set_tracks((*self.filtered_tracks).clone(), None);
                    }
                }
                BgMessage::LyricsLoaded {
                    track_id,
                    lrc_raw,
                    parsed,
                } => {
                    if !lrc_raw.is_empty()
                        && let Some(db) = &self.db
                    {
                        let _ = db.save_cached_lyrics(&track_id, &lrc_raw);
                    }
                    if self.queue.current().map(|t| t.id.as_str()) == Some(track_id.as_str()) {
                        if parsed.lines.is_empty() {
                            self.ui
                                .label(id!(lyric_active_lbl))
                                .set_text(cx, "No lyrics found for this track.");
                        } else {
                            let mode = if parsed.is_synced {
                                "Time-Synced"
                            } else {
                                "Auto-Paced"
                            };
                            self.ui.label(id!(lyrics_source_lbl)).set_text(
                                cx,
                                &format!("Lyrics · {} ({})", parsed.source_name, mode),
                            );
                            self.synced_lyrics = parsed;
                            self.update_lyrics_display(cx, 0);
                        }
                    }
                }
                BgMessage::PublicSyncCompleted { provider, payload } => {
                    let (playlist, tracks) = *payload;
                    let count = tracks.len();
                    let pl_name = playlist.name.clone();
                    let mut matched = 0usize;
                    if let Some(db) = &mut self.db {
                        let _ = db.upsert_tracks(&tracks);
                        let ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
                        let _ = db.upsert_playlist(&playlist, &ids);
                        matched = db.match_cloud_tracks_to_local().unwrap_or(0);
                        self.all_tracks = db.list_tracks("").unwrap_or_default();
                        self.playlists = db.list_playlists().unwrap_or_default();
                    } else {
                        self.all_tracks.extend(tracks);
                        self.playlists.push(playlist);
                    }
                    self.ui.label(id!(oauth_status_lbl)).set_text(
                        cx,
                        &format!(
                            "{provider} Sync Complete: imported '{pl_name}' ({count} tracks, {matched} matched to local audio). Open Library to browse."
                        ),
                    );
                    self.rebuild_filtered_tracks(cx);
                }
                BgMessage::PublicSyncFailed { provider, error } => {
                    self.ui
                        .label(id!(oauth_status_lbl))
                        .set_text(cx, &format!("{provider} import failed: {error}"));
                }
                BgMessage::OAuthCompleted {
                    kind,
                    tokens,
                    report,
                } => {
                    self.ui.button(id!(oauth_cancel_btn)).set_visible(cx, false);
                    match kind {
                        AccountKind::Spotify => {
                            self.accounts.spotify_connected = true;
                            self.accounts.spotify_user_name = report.display_name.clone();
                            self.accounts.spotify_access_token = tokens.access_token;
                            if let Some(rt) = tokens.refresh_token {
                                self.accounts.spotify_refresh_token = rt;
                            }
                            self.accounts.spotify_playlist_count = report.playlists.len();
                            self.accounts.spotify_liked_count = report.liked_tracks.len();
                        }
                        AccountKind::YouTubeMusic => {
                            self.accounts.youtube_connected = true;
                            self.accounts.youtube_channel_name = report.display_name.clone();
                            self.accounts.youtube_access_token = tokens.access_token;
                            if let Some(rt) = tokens.refresh_token {
                                self.accounts.youtube_refresh_token = rt;
                            }
                            self.accounts.youtube_playlist_count = report.playlists.len();
                        }
                    }
                    let mut matched = 0usize;
                    if let Some(db) = &mut self.db {
                        let _ = db.save_accounts(&self.accounts);
                        let _ = db.upsert_tracks(&report.liked_tracks);
                        for bundle in &report.playlists {
                            let _ = db.upsert_tracks(&bundle.tracks);
                            let ids: Vec<String> =
                                bundle.tracks.iter().map(|t| t.id.clone()).collect();
                            let _ = db.upsert_playlist(&bundle.playlist, &ids);
                        }
                        matched = db.match_cloud_tracks_to_local().unwrap_or(0);
                        self.all_tracks = db.list_tracks("").unwrap_or_default();
                        self.playlists = db.list_playlists().unwrap_or_default();
                    }
                    self.ui.label(id!(oauth_status_lbl)).set_text(
                        cx,
                        &format!(
                            "Signed in as {} · {} playlists & {} liked songs synced ({} matched to local audio)",
                            report.display_name,
                            report.playlists.len(),
                            report.liked_tracks.len(),
                            matched
                        ),
                    );
                    self.rebuild_filtered_tracks(cx);
                }
                BgMessage::OAuthFailed { kind, error } => {
                    self.ui.button(id!(oauth_cancel_btn)).set_visible(cx, false);
                    let provider = match kind {
                        AccountKind::Spotify => "Spotify",
                        AccountKind::YouTubeMusic => "YouTube",
                    };
                    self.ui
                        .label(id!(oauth_status_lbl))
                        .set_text(cx, &format!("{provider} sign-in failed: {error}"));
                }
                BgMessage::SubsonicSynced { tracks, playlists } => {
                    self.accounts.subsonic_connected = true;
                    if let Some(db) = &mut self.db {
                        let _ = db.save_accounts(&self.accounts);
                        let _ = db.upsert_tracks(&tracks);
                        for (pl, pl_tracks) in &playlists {
                            let _ = db.upsert_tracks(pl_tracks);
                            let ids: Vec<String> = pl_tracks.iter().map(|t| t.id.clone()).collect();
                            let _ = db.upsert_playlist(pl, &ids);
                        }
                        self.all_tracks = db.list_tracks("").unwrap_or_default();
                        self.playlists = db.list_playlists().unwrap_or_default();
                    }
                    self.ui.label(id!(oauth_status_lbl)).set_text(
                        cx,
                        &format!(
                            "Subsonic connected: {} songs & {} playlists synced.",
                            tracks.len(),
                            playlists.len()
                        ),
                    );
                    self.rebuild_filtered_tracks(cx);
                }
                BgMessage::SubsonicFailed(err) => {
                    self.ui
                        .label(id!(oauth_status_lbl))
                        .set_text(cx, &format!("Subsonic error: {err}"));
                }
                BgMessage::UpdateChecked { manual, result } => {
                    let status = format_check_status(&result, APP_VERSION);
                    self.ui.label(id!(update_status_lbl)).set_text(cx, &status);
                    if let Ok(Some(info)) = result {
                        self.available_update = Some(info.clone());
                        self.ui.button(id!(update_badge_btn)).set_visible(cx, true);
                        self.ui
                            .button(id!(update_badge_btn))
                            .set_text(cx, &format!("Update {}", info.tag));
                        if manual || self.update_settings.auto_check_on_startup {
                            self.show_update_popup(cx, &info);
                        }
                    }
                }
                BgMessage::UpdateDownload(ev) => match ev {
                    DownloadEvent::Progress(pct_opt) => {
                        let txt = match pct_opt {
                            Some(pct) => format!("Downloading {pct}%"),
                            None => "Downloading...".to_string(),
                        };
                        self.ui.button(id!(update_now_btn)).set_text(cx, &txt);
                    }
                    DownloadEvent::Verifying => {
                        self.ui
                            .button(id!(update_now_btn))
                            .set_text(cx, "Verifying SHA-256...");
                    }
                    DownloadEvent::Ready(msi_path) => {
                        self.update_download_in_flight = false;
                        if launch_verified_installer(&msi_path).is_ok() {
                            win32_close_active_window();
                            cx.quit();
                        }
                    }
                    DownloadEvent::Failed(err) => {
                        self.update_download_in_flight = false;
                        self.ui.button(id!(update_now_btn)).set_text(cx, "Retry");
                        self.ui.label(id!(update_popup_sub)).set_text(cx, &err);
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

        // 3. Sleep Timer Countdown
        if let SleepTimerMode::Deadline(deadline) = self.sleep_timer {
            let now = Instant::now();
            if now >= deadline {
                self.sleep_timer = SleepTimerMode::Off;
                self.ui
                    .label(id!(sleep_status_lbl))
                    .set_text(cx, "Sleep Timer: Ended (Paused smoothly)");
                if let Some(a) = &self.audio {
                    a.send(EngineCommand::Pause);
                }
            } else {
                let rem_s = deadline.duration_since(now).as_secs();
                self.ui.label(id!(sleep_status_lbl)).set_text(
                    cx,
                    &format!("Sleep Timer: {}:{:02} remaining", rem_s / 60, rem_s % 60),
                );
            }
        }

        // 4. Periodic LAN UDP Discovery Beacon (Connect page or drawer open)
        if self.right_drawer_open || self.active_page == live_id!(page_connect) {
            let should_ping = self
                .last_lan_broadcast
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(3));
            if should_ping {
                self.last_lan_broadcast = Some(Instant::now());
                let is_local = self
                    .lease
                    .as_ref()
                    .is_none_or(ActiveSpeakerLease::is_local_active);
                if let Some(beacon) = &mut self.lan_beacon {
                    let _ = beacon.broadcast_announce("Desktop-PC", 43821, is_local);
                    let now_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let peers = beacon.poll_peers(now_ms);
                    let txt = if peers.is_empty() {
                        "Listening for Ferric devices on local Wi-Fi...".to_string()
                    } else {
                        peers
                            .iter()
                            .map(|p| format!("- {} ({})", p.device_name, p.addr))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    self.ui.label(id!(peers_lbl)).set_text(cx, &txt);
                    self.ui.label(id!(connect_peers_lbl)).set_text(cx, &txt);
                }
            }
        }

        // 5. Sync Window Maximize / Restore Glyph
        self.ui
            .ferric_win_btn(id!(win_max_btn))
            .set_maximized(cx, win32_is_active_window_maximized());

        // 6. Audio Telemetry, Seamless Auto-Transition, Lyrics & Spectrum Update
        if let Some(audio) = &self.audio {
            let snap = audio.telemetry();
            if snap.track_finished_seq != self.last_finished_seq {
                self.last_finished_seq = snap.track_finished_seq;
                if matches!(self.sleep_timer, SleepTimerMode::EndOfTrack) {
                    self.sleep_timer = SleepTimerMode::Off;
                    audio.send(EngineCommand::Stop);
                    self.ui
                        .label(id!(sleep_status_lbl))
                        .set_text(cx, "Sleep Timer: Stopped at end of song");
                } else if let Some(next) = self.queue.next_track().cloned() {
                    if snap.auto_transitioned {
                        self.sync_next_deck_to_engine();
                        self.update_now_playing_metadata(cx, &next);
                    } else {
                        self.play_track(cx, next);
                    }
                } else {
                    self.ui
                        .ferric_play_btn(id!(play_pause_btn))
                        .set_playing(cx, false);
                }
            }

            if snap.duration_ms > 0 {
                let ratio = (snap.position_ms as f64 / snap.duration_ms as f64).clamp(0.0, 1.0);
                self.ui
                    .ferric_bar_slider(id!(seek_slider))
                    .set_value(cx, ratio);
                self.ui
                    .label(id!(pos_lbl))
                    .set_text(cx, &format_duration_ms(snap.position_ms));
                let dur_txt = if self.show_remaining_time {
                    format!(
                        "-{}",
                        format_duration_ms(snap.duration_ms.saturating_sub(snap.position_ms))
                    )
                } else {
                    format_duration_ms(snap.duration_ms)
                };
                self.ui.button(id!(dur_btn)).set_text(cx, &dur_txt);
                self.update_lyrics_display(cx, snap.position_ms);
            }

            self.ui
                .ferric_play_btn(id!(play_pause_btn))
                .set_playing(cx, snap.is_playing);

            if snap.is_playing {
                self.canvas_phase = (self.canvas_phase + 0.09) % 628.0;
                let norm = |i: usize| (snap.spectrum_bins[i] as f32 / 1_000.0).clamp(0.04, 1.0);
                let avg = (norm(0) + norm(1) + norm(2)) / 3.0;
                self.ui
                    .view(id!(np_art_tile))
                    .apply_over(cx, live! { draw_bg: { playing: 1.0, level: (avg) } });
                self.ui.view(id!(np_canvas)).apply_over(
                    cx,
                    live! {
                        draw_bg: {
                            mode: (if self.np_canvas_animated { 1.0 } else { 0.0 }),
                            phase: (self.canvas_phase),
                            s0: (norm(0)), s1: (norm(1)), s2: (norm(2)), s3: (norm(3)),
                            s4: (norm(4)), s5: (norm(5)), s6: (norm(6)), s7: (norm(7))
                        }
                    },
                );
            } else {
                self.ui
                    .view(id!(np_art_tile))
                    .apply_over(cx, live! { draw_bg: { playing: 0.0 } });
            }

            if let Some(smtc) = &mut self.smtc {
                smtc.set_playing(snap.is_playing);
            }
        }
    }

    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        // 1. WINDOW CONTROLS (Minimize, Maximize/Restore, Close, Mini-Player)
        if self.ui.ferric_win_btn(id!(win_min_btn)).clicked(actions) {
            win32_minimize_active_window();
            return;
        }
        if self.ui.ferric_win_btn(id!(win_max_btn)).clicked(actions) {
            let is_max = win32_toggle_maximize_active_window();
            self.ui
                .ferric_win_btn(id!(win_max_btn))
                .set_maximized(cx, is_max);
            return;
        }
        if self.ui.ferric_win_btn(id!(win_close_btn)).clicked(actions) {
            win32_close_active_window();
            cx.quit();
            return;
        }
        if self.ui.ferric_win_btn(id!(win_mini_btn)).clicked(actions) {
            self.toggle_compact_mini_mode(cx);
            return;
        }

        // 2. DIRECT-ACCESS NAVIGATION
        if self.ui.button(id!(nav_home)).clicked(actions) {
            self.switch_main_page(cx, live_id!(page_home));
        }
        if self.ui.button(id!(nav_library)).clicked(actions) {
            self.switch_main_page(cx, live_id!(page_library));
        }
        if self.ui.button(id!(nav_now_playing)).clicked(actions)
            || self.ui.button(id!(bar_lyrics_btn)).clicked(actions)
            || self.ui.button(id!(codec_pill_btn)).clicked(actions)
            || self.ui.view(id!(np_art_tile)).finger_up(actions).is_some()
        {
            self.switch_main_page(cx, live_id!(page_now_playing));
        }
        if self.ui.button(id!(nav_fade)).clicked(actions)
            || self.ui.button(id!(home_fade_btn)).clicked(actions)
        {
            self.switch_main_page(cx, live_id!(page_fade));
        }
        if self.ui.button(id!(nav_eq)).clicked(actions)
            || self.ui.button(id!(home_eq_btn)).clicked(actions)
            || self.ui.button(id!(bar_eq_btn)).clicked(actions)
        {
            self.switch_main_page(cx, live_id!(page_eq));
        }
        if self.ui.button(id!(nav_connect)).clicked(actions) {
            self.switch_main_page(cx, live_id!(page_connect));
        }
        if self.ui.button(id!(nav_appearance)).clicked(actions) {
            self.switch_main_page(cx, live_id!(page_appearance));
        }
        if self.ui.button(id!(nav_accounts)).clicked(actions)
            || self.ui.button(id!(top_spotify_btn)).clicked(actions)
            || self.ui.button(id!(top_youtube_btn)).clicked(actions)
            || self.ui.button(id!(home_spotify_btn)).clicked(actions)
            || self.ui.button(id!(home_youtube_btn)).clicked(actions)
        {
            self.switch_main_page(cx, live_id!(page_accounts));
        }
        if self.ui.button(id!(cmdk_btn)).clicked(actions) {
            self.cmdk_open = true;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, true);
            self.ui.redraw(cx);
        }
        if self.ui.button(id!(cmdk_close_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.ui.redraw(cx);
        }

        // Command Palette Quick Actions
        if self.ui.button(id!(cmd_demo_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.load_and_play_demo_suite(cx);
        }
        if self.ui.button(id!(cmd_glass_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.theme.material.liquid_glass = !self.theme.material.liquid_glass;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(cmd_fade_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.switch_main_page(cx, live_id!(page_fade));
        }
        if self.ui.button(id!(cmd_eq_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.switch_main_page(cx, live_id!(page_eq));
        }
        if self.ui.button(id!(cmd_accounts_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.switch_main_page(cx, live_id!(page_accounts));
        }
        if self.ui.button(id!(cmd_sleep30_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.sleep_timer =
                SleepTimerMode::Deadline(Instant::now() + Duration::from_secs(30 * 60));
        }

        // Right Drawer (Queue & Quick Connect)
        if self.ui.button(id!(bar_queue_btn)).clicked(actions) {
            if self.right_drawer_open {
                self.right_drawer_open = false;
                self.ui.view(id!(right_drawer)).set_visible(cx, false);
                self.ui.redraw(cx);
            } else {
                self.open_right_drawer(cx, false);
            }
        }
        if self.ui.button(id!(bar_connect_btn)).clicked(actions) {
            self.open_right_drawer(cx, true);
        }
        if self.ui.button(id!(drawer_queue_tab)).clicked(actions) {
            self.open_right_drawer(cx, false);
        }
        if self.ui.button(id!(drawer_connect_tab)).clicked(actions) {
            self.open_right_drawer(cx, true);
        }
        if self.ui.button(id!(drawer_close_btn)).clicked(actions) {
            self.right_drawer_open = false;
            self.ui.view(id!(right_drawer)).set_visible(cx, false);
            self.ui.redraw(cx);
        }
        if self.ui.button(id!(clear_queue_btn)).clicked(actions) {
            self.queue.set_tracks(Vec::new(), None);
            self.sync_next_deck_to_engine();
            self.refresh_ui_labels(cx);
        }

        // 3. SEARCH, LIBRARY FILTERS, PLAYLISTS & TRACK ACTIONS
        if let Some(query) = self.ui.text_input(id!(search_input)).changed(actions) {
            self.search_query = query;
            self.switch_main_page(cx, live_id!(page_library));
            self.rebuild_filtered_tracks(cx);
        }
        if let Some((submitted, _)) = self.ui.text_input(id!(search_input)).returned(actions) {
            if parse_spotify_target(&submitted).is_some() {
                self.switch_main_page(cx, live_id!(page_accounts));
                self.start_public_sync(cx, "Spotify", submitted);
            } else if parse_youtube_target(&submitted).is_some() {
                self.switch_main_page(cx, live_id!(page_accounts));
                self.start_public_sync(cx, "YouTube", submitted);
            }
        }

        for (btn, flt) in [
            (id!(flt_all_btn), LibraryFilter::All),
            (id!(pl_all_btn), LibraryFilter::All),
            (id!(flt_liked_btn), LibraryFilter::Liked),
            (id!(pl_liked_btn), LibraryFilter::Liked),
            (id!(flt_local_btn), LibraryFilter::Local),
            (id!(pl_local_btn), LibraryFilter::Local),
            (id!(flt_spotify_btn), LibraryFilter::Spotify),
            (id!(flt_youtube_btn), LibraryFilter::YouTube),
            (id!(pl_slot_0), LibraryFilter::Playlist(0)),
            (id!(pl_slot_1), LibraryFilter::Playlist(1)),
            (id!(pl_slot_2), LibraryFilter::Playlist(2)),
            (id!(pl_slot_3), LibraryFilter::Playlist(3)),
            (id!(pl_slot_4), LibraryFilter::Playlist(4)),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.active_filter = flt;
                self.switch_main_page(cx, live_id!(page_library));
                self.rebuild_filtered_tracks(cx);
            }
        }
        if self.ui.button(id!(sort_cycle_btn)).clicked(actions) {
            self.sort_field = self.sort_field.next();
            self.rebuild_filtered_tracks(cx);
        }

        if (self.ui.button(id!(add_folder_btn)).clicked(actions)
            || self.ui.button(id!(sidebar_add_folder_btn)).clicked(actions)
            || self.ui.button(id!(home_add_folder_btn)).clicked(actions))
            && let Some(folder) = rfd::FileDialog::new()
                .set_title("Select Local Music Folder")
                .pick_folder()
        {
            if let Some(db) = &self.db {
                let _ = db.add_music_folder(&folder);
            }
            self.spawn_folder_scan(vec![folder]);
        }
        if self.ui.button(id!(sidebar_import_btn)).clicked(actions)
            && let Some(file) = rfd::FileDialog::new()
                .set_title("Import Playlist (.m3u, .m3u8, .csv)")
                .add_filter("Playlist Files", &["m3u", "m3u8", "csv"])
                .pick_file()
            && let Ok(content) = std::fs::read_to_string(&file)
        {
            let stem = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Imported Playlist");
            let (pl, tracks) = parse_playlist_import_text(stem, &content, file.parent());
            if let Some(db) = &mut self.db {
                let _ = db.upsert_tracks(&tracks);
                let ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
                let _ = db.upsert_playlist(&pl, &ids);
                let _ = db.match_cloud_tracks_to_local();
                self.all_tracks = db.list_tracks("").unwrap_or_default();
                self.playlists = db.list_playlists().unwrap_or_default();
            }
            self.rebuild_filtered_tracks(cx);
        }
        if self.ui.button(id!(demo_suite_btn)).clicked(actions)
            || self.ui.button(id!(home_demo_btn)).clicked(actions)
        {
            self.load_and_play_demo_suite(cx);
        }

        for (i, btn_id) in [
            id!(recent_btn_0),
            id!(recent_btn_1),
            id!(recent_btn_2),
            id!(recent_btn_3),
        ]
        .into_iter()
        .enumerate()
        {
            if self.ui.button(btn_id).clicked(actions) {
                if self.all_tracks.is_empty() {
                    self.load_and_play_demo_suite(cx);
                } else if let Some(track) = self.all_tracks.get(i).cloned() {
                    self.queue.set_tracks(self.all_tracks.clone(), Some(i));
                    self.play_track(cx, track);
                }
            }
        }

        // TrackList Actions
        for action in actions {
            match action.cast() {
                TrackListAction::PlayIndex(filtered_idx) => {
                    if let Some(clicked) = self.filtered_tracks.get(filtered_idx).cloned() {
                        self.queue
                            .set_tracks((*self.filtered_tracks).clone(), Some(filtered_idx));
                        self.play_track(cx, clicked);
                    }
                }
                TrackListAction::ToggleLike(filtered_idx) => {
                    if let Some(target) = self.filtered_tracks.get(filtered_idx) {
                        let next_liked = !target.liked;
                        let tid = target.id.clone();
                        if let Some(db) = &self.db {
                            let _ = db.set_track_liked(&tid, next_liked);
                        }
                        if let Some(t) = self.all_tracks.iter_mut().find(|x| x.id == tid) {
                            t.liked = next_liked;
                        }
                        self.rebuild_filtered_tracks(cx);
                    }
                }
                TrackListAction::AddLocalFolder => {
                    if let Some(folder) = rfd::FileDialog::new()
                        .set_title("Select Local Music Folder")
                        .pick_folder()
                    {
                        if let Some(db) = &self.db {
                            let _ = db.add_music_folder(&folder);
                        }
                        self.spawn_folder_scan(vec![folder]);
                    }
                }
                TrackListAction::OpenSpotifyLogin | TrackListAction::OpenYouTubeLogin => {
                    self.switch_main_page(cx, live_id!(page_accounts));
                }
                TrackListAction::LoadDemoSuite => {
                    self.load_and_play_demo_suite(cx);
                }
                TrackListAction::None => {}
            }
        }

        if self.ui.button(id!(play_all_btn)).clicked(actions) {
            if self.filtered_tracks.is_empty() {
                self.load_and_play_demo_suite(cx);
            } else if let Some(first) = self.filtered_tracks.first().cloned() {
                self.queue
                    .set_tracks((*self.filtered_tracks).clone(), Some(0));
                self.play_track(cx, first);
            }
        }
        if self.ui.button(id!(hero_shuffle_btn)).clicked(actions)
            && !self.filtered_tracks.is_empty()
        {
            self.queue
                .set_tracks((*self.filtered_tracks).clone(), Some(0));
            self.queue.set_shuffle(true);
            self.session_state.shuffle = true;
            self.sync_controls_from_state(cx);
            if let Some(first) = self.queue.current().cloned() {
                self.play_track(cx, first);
            }
        }

        // 4. TRANSPORT, SEEK, VOLUME & NOW PLAYING CONTROLS
        if self
            .ui
            .ferric_play_btn(id!(play_pause_btn))
            .clicked(actions)
            && let Some(audio) = &self.audio
        {
            let snap = audio.telemetry();
            if !snap.is_playing && self.queue.current().is_none() {
                if self.filtered_tracks.is_empty() {
                    self.load_and_play_demo_suite(cx);
                } else if let Some(first) = self.filtered_tracks.first().cloned() {
                    self.queue
                        .set_tracks((*self.filtered_tracks).clone(), Some(0));
                    self.play_track(cx, first);
                }
            } else {
                audio.send(EngineCommand::TogglePlayPause);
            }
        }
        if self.ui.button(id!(next_btn)).clicked(actions)
            && let Some(next) = self.queue.next_track().cloned()
        {
            self.play_track(cx, next);
        }
        if self.ui.button(id!(prev_btn)).clicked(actions)
            && let Some(prev) = self.queue.prev_track().cloned()
        {
            self.play_track(cx, prev);
        }
        if self.ui.button(id!(shuffle_btn)).clicked(actions) {
            let next_shuf = !self.queue.shuffle;
            self.queue.set_shuffle(next_shuf);
            self.session_state.shuffle = next_shuf;
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.sync_next_deck_to_engine();
            self.sync_controls_from_state(cx);
            self.refresh_ui_labels(cx);
        }
        if self.ui.button(id!(repeat_btn)).clicked(actions) {
            self.queue.repeat = self.queue.repeat.next();
            self.session_state.repeat = self.queue.repeat;
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.sync_next_deck_to_engine();
            self.sync_controls_from_state(cx);
        }
        if self.ui.button(id!(dur_btn)).clicked(actions) {
            self.show_remaining_time = !self.show_remaining_time;
        }
        if self.ui.button(id!(np_like_btn)).clicked(actions)
            && let Some(cur) = self.queue.current().cloned()
        {
            let next_liked = !cur.liked;
            if let Some(db) = &self.db {
                let _ = db.set_track_liked(&cur.id, next_liked);
            }
            if let Some(t) = self.all_tracks.iter_mut().find(|x| x.id == cur.id) {
                t.liked = next_liked;
            }
            let like_col = if next_liked {
                vec4(0.851, 0.329, 0.118, 1.0)
            } else {
                vec4(0.420, 0.400, 0.369, 1.0)
            };
            self.ui
                .button(id!(np_like_btn))
                .apply_over(cx, live! { draw_icon: { color: (like_col) } });
            self.rebuild_filtered_tracks(cx);
        }
        if let Some(pos_ratio) = self
            .ui
            .ferric_bar_slider(id!(seek_slider))
            .released(actions)
            && let Some(audio) = &self.audio
        {
            let dur = audio.telemetry().duration_ms;
            if dur > 0 {
                audio.send(EngineCommand::SeekMs((pos_ratio * dur as f64) as u64));
            }
        }
        if let Some(vol) = self.ui.ferric_bar_slider(id!(vol_slider)).changed(actions) {
            self.session_state.volume = vol as f32;
            if self.session_state.volume > 0.02 {
                self.pre_mute_volume = self.session_state.volume;
            }
            if let Some(audio) = &self.audio {
                audio.send(EngineCommand::SetVolume(vol as f32));
            }
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.sync_controls_from_state(cx);
        }
        if self.ui.button(id!(vol_mute_btn)).clicked(actions) {
            if self.session_state.volume > 0.01 {
                self.pre_mute_volume = self.session_state.volume;
                self.session_state.volume = 0.0;
            } else {
                self.session_state.volume = self.pre_mute_volume.max(0.5);
            }
            if let Some(audio) = &self.audio {
                audio.send(EngineCommand::SetVolume(self.session_state.volume));
            }
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.sync_controls_from_state(cx);
        }

        // Now Playing Modes & Lyrics Sync Offset
        if self.ui.button(id!(np_mode_canvas_btn)).clicked(actions) {
            self.np_canvas_animated = true;
            self.ui
                .button(id!(np_mode_canvas_btn))
                .apply_over(cx, live! { draw_bg: { active: 1.0 } });
            self.ui
                .button(id!(np_mode_card_btn))
                .apply_over(cx, live! { draw_bg: { active: 0.0 } });
            self.ui
                .view(id!(np_canvas))
                .apply_over(cx, live! { draw_bg: { mode: 1.0 } });
        }
        if self.ui.button(id!(np_mode_card_btn)).clicked(actions) {
            self.np_canvas_animated = false;
            self.ui
                .button(id!(np_mode_canvas_btn))
                .apply_over(cx, live! { draw_bg: { active: 0.0 } });
            self.ui
                .button(id!(np_mode_card_btn))
                .apply_over(cx, live! { draw_bg: { active: 1.0 } });
            self.ui
                .view(id!(np_canvas))
                .apply_over(cx, live! { draw_bg: { mode: 0.0 } });
        }
        if self.ui.button(id!(np_watch_video_btn)).clicked(actions)
            && let Some(cur) = self.queue.current()
        {
            let _ = open_track_video_clip(
                cur.path.as_deref(),
                cur.youtube_video_id.as_deref(),
                &cur.artist,
                &cur.title,
            );
        }
        if self.ui.button(id!(lyr_minus_btn)).clicked(actions) {
            self.lyrics_offset_ms -= 500;
        }
        if self.ui.button(id!(lyr_plus_btn)).clicked(actions) {
            self.lyrics_offset_ms += 500;
        }
        if self.ui.button(id!(lyr_copy_btn)).clicked(actions)
            && !self.synced_lyrics.lines.is_empty()
        {
            let full = self
                .synced_lyrics
                .lines
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let _ = copy_text_to_clipboard(&full);
        }

        // 5. INSTANT ZERO-KEY SYNC & OAUTH 2.0 PKCE ACCOUNTS
        if self.ui.button(id!(quick_spotify_btn)).clicked(actions) {
            let input = self.ui.text_input(id!(quick_sync_input)).text();
            self.start_public_sync(cx, "Spotify", input);
        }
        if self.ui.button(id!(quick_youtube_btn)).clicked(actions) {
            let input = self.ui.text_input(id!(quick_sync_input)).text();
            self.start_public_sync(cx, "YouTube", input);
        }
        if self.ui.button(id!(quick_yt_search_btn)).clicked(actions) {
            let input = self.ui.text_input(id!(quick_sync_input)).text();
            self.start_public_sync(cx, "YouTube Search", input);
        }
        if let Some((submitted, _)) = self.ui.text_input(id!(quick_sync_input)).returned(actions) {
            let mode = if parse_spotify_target(&submitted).is_some() {
                "Spotify"
            } else if parse_youtube_target(&submitted).is_some() {
                "YouTube"
            } else {
                "YouTube Search"
            };
            self.start_public_sync(cx, mode, submitted);
        }
        if self.ui.button(id!(spotify_login_btn)).clicked(actions) {
            self.start_oauth_login(cx, AccountKind::Spotify);
        }
        if self.ui.button(id!(spotify_sync_btn)).clicked(actions) {
            self.sync_existing_cloud_account(cx, AccountKind::Spotify);
        }
        if self.ui.button(id!(spotify_logout_btn)).clicked(actions) {
            self.accounts.spotify_connected = false;
            self.accounts.spotify_access_token.clear();
            self.accounts.spotify_refresh_token.clear();
            if let Some(db) = &self.db {
                let _ = db.save_accounts(&self.accounts);
            }
            self.refresh_ui_labels(cx);
        }
        if self.ui.button(id!(yt_login_btn)).clicked(actions) {
            self.start_oauth_login(cx, AccountKind::YouTubeMusic);
        }
        if self.ui.button(id!(yt_sync_btn)).clicked(actions) {
            self.sync_existing_cloud_account(cx, AccountKind::YouTubeMusic);
        }
        if self.ui.button(id!(yt_logout_btn)).clicked(actions) {
            self.accounts.youtube_connected = false;
            self.accounts.youtube_access_token.clear();
            self.accounts.youtube_refresh_token.clear();
            if let Some(db) = &self.db {
                let _ = db.save_accounts(&self.accounts);
            }
            self.refresh_ui_labels(cx);
        }
        if self.ui.button(id!(oauth_cancel_btn)).clicked(actions) {
            if let Some(c) = self.oauth_cancel.take() {
                c.store(true, Ordering::Relaxed);
            }
            self.ui.button(id!(oauth_cancel_btn)).set_visible(cx, false);
            self.ui
                .label(id!(oauth_status_lbl))
                .set_text(cx, "Browser login cancelled.");
        }
        if self.ui.button(id!(copy_redirect_btn)).clicked(actions) {
            let _ = copy_text_to_clipboard("http://127.0.0.1:43824/callback");
            self.ui
                .label(id!(oauth_status_lbl))
                .set_text(cx, "Copied http://127.0.0.1:43824/callback to clipboard.");
        }
        if self.ui.button(id!(open_spotify_dev_btn)).clicked(actions) {
            let _ = open_allowed_url("https://developer.spotify.com/dashboard");
        }
        if self.ui.button(id!(open_google_dev_btn)).clicked(actions) {
            let _ = open_allowed_url("https://console.cloud.google.com/apis/credentials");
        }
        if let Some(cat_only) = self.ui.ferric_toggle(id!(yt_cat_toggle)).toggled(actions) {
            self.accounts.youtube_music_category_only = cat_only;
            if let Some(db) = &self.db {
                let _ = db.save_accounts(&self.accounts);
            }
        }
        if self.ui.button(id!(sub_sync_btn)).clicked(actions) {
            self.accounts.subsonic_url = self
                .ui
                .text_input(id!(sub_url_input))
                .text()
                .trim()
                .to_string();
            self.accounts.subsonic_username = self
                .ui
                .text_input(id!(sub_user_input))
                .text()
                .trim()
                .to_string();
            self.accounts.subsonic_password = self
                .ui
                .text_input(id!(sub_pass_input))
                .text()
                .trim()
                .to_string();
            if let Some(db) = &self.db {
                let _ = db.save_accounts(&self.accounts);
            }
            let creds = SubsonicCredentials {
                base_url: self.accounts.subsonic_url.clone(),
                username: self.accounts.subsonic_username.clone(),
                password: self.accounts.subsonic_password.clone(),
            };
            if let Some(tx) = self.bg_tx.clone() {
                thread::spawn(move || match sync_subsonic_catalog(&creds) {
                    Ok((tracks, bundles)) => {
                        let playlists = bundles
                            .into_iter()
                            .map(|b| (b.playlist, b.tracks))
                            .collect();
                        let _ = tx.send(BgMessage::SubsonicSynced { tracks, playlists });
                    }
                    Err(e) => {
                        let _ = tx.send(BgMessage::SubsonicFailed(e.to_string()));
                    }
                });
            }
        }
        if self.ui.button(id!(match_local_btn)).clicked(actions)
            && let Some(db) = &mut self.db
        {
            let matched = db.match_cloud_tracks_to_local().unwrap_or(0);
            self.all_tracks = db.list_tracks("").unwrap_or_default();
            self.rebuild_filtered_tracks(cx);
            self.ui.label(id!(oauth_status_lbl)).set_text(
                cx,
                &format!("Matched {matched} cloud songs to local lossless files."),
            );
        }
        if self.ui.button(id!(rescan_btn)).clicked(actions)
            && let Some(db) = &self.db
            && let Ok(folders) = db.list_music_folders()
            && !folders.is_empty()
        {
            self.spawn_folder_scan(folders);
        }

        // 6. SEAMLESS FADE-IN & AUTOMIX SETTINGS
        if let Some(sec) = self
            .ui
            .ferric_bar_slider(id!(xfade_slider))
            .changed(actions)
        {
            self.dsp.crossfade_ms = (sec * 1_000.0).round() as u32;
            self.ui
                .label(id!(xfade_val_lbl))
                .set_text(cx, &format!("{sec:.1} s"));
            self.apply_dsp_live(cx);
        }
        for (btn, ms) in [
            (id!(xf_0_btn), 0_u32),
            (id!(xf_3_btn), 3_000_u32),
            (id!(xf_6_btn), 6_000_u32),
            (id!(xf_10_btn), 10_000_u32),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.dsp.crossfade_ms = ms;
                self.sync_controls_from_state(cx);
                self.apply_dsp_live(cx);
            }
        }
        if let Some(ms) = self
            .ui
            .ferric_bar_slider(id!(fadein_slider))
            .changed(actions)
        {
            self.dsp.fade_in_play_ms = ms.round() as u32;
            self.ui
                .label(id!(fadein_val_lbl))
                .set_text(cx, &format!("{} ms", self.dsp.fade_in_play_ms));
            self.apply_dsp_live(cx);
        }
        for (btn, curve) in [
            (id!(curve_bass_btn), FadeCurve::BassSwapAutomix),
            (id!(curve_eqp_btn), FadeCurve::EqualPower),
            (id!(curve_velvet_btn), FadeCurve::SCurveVelvet),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.dsp.fade_curve = curve;
                self.sync_controls_from_state(cx);
                self.apply_dsp_live(cx);
            }
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(gapless_lock_toggle))
            .toggled(actions)
        {
            self.dsp.gapless_album_lock = on;
            self.apply_dsp_live(cx);
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(trim_silence_toggle))
            .toggled(actions)
        {
            self.dsp.trim_silence = on;
            self.apply_dsp_live(cx);
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(replay_gain_toggle))
            .toggled(actions)
        {
            self.dsp.replay_gain = on;
            self.apply_dsp_live(cx);
        }
        if let Some(on) = self.ui.ferric_toggle(id!(mono_toggle)).toggled(actions) {
            self.dsp.mono_audio = on;
            self.apply_dsp_live(cx);
        }
        for (btn, spd) in [
            (id!(spd_075_btn), 0.75_f32),
            (id!(spd_100_btn), 1.0_f32),
            (id!(spd_125_btn), 1.25_f32),
            (id!(spd_150_btn), 1.5_f32),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.dsp.playback_speed = spd;
                self.sync_controls_from_state(cx);
                self.apply_dsp_live(cx);
            }
        }
        for (btn, mins_opt) in [
            (id!(slp_off_btn), None),
            (id!(slp_15_btn), Some(15_u64)),
            (id!(slp_30_btn), Some(30_u64)),
            (id!(slp_60_btn), Some(60_u64)),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.sleep_timer = match mins_opt {
                    Some(m) => {
                        SleepTimerMode::Deadline(Instant::now() + Duration::from_secs(m * 60))
                    }
                    None => SleepTimerMode::Off,
                };
                if mins_opt.is_none() {
                    self.ui
                        .label(id!(sleep_status_lbl))
                        .set_text(cx, "Sleep Timer: Off");
                }
            }
        }
        if self.ui.button(id!(slp_eot_btn)).clicked(actions) {
            self.sleep_timer = SleepTimerMode::EndOfTrack;
            self.ui
                .label(id!(sleep_status_lbl))
                .set_text(cx, "Sleep Timer: Will stop after current song");
        }

        // 7. EQUALIZER & AUTOEQ CONTROLS
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(eq_enable_toggle))
            .toggled(actions)
        {
            self.eq.enabled = on;
            self.apply_eq_live(cx);
        }
        if let Some(pre) = self
            .ui
            .ferric_bar_slider(id!(eq_preamp_slider))
            .changed(actions)
        {
            self.eq.preamp_db = pre as f32;
            self.ui
                .label(id!(eq_preamp_lbl))
                .set_text(cx, &format!("{pre:+.1} dB"));
            self.apply_eq_live(cx);
        }
        for (idx, (slider_id, val_id)) in eq_band_ids().into_iter().enumerate() {
            if let Some(val) = self.ui.ferric_vert_slider(slider_id).changed(actions)
                && let Some(band) = self.eq.bands.get_mut(idx)
            {
                band.gain_db = val as f32;
                self.eq.preset_name = "Custom".to_string();
                self.ui.label(val_id).set_text(cx, &format!("{val:+.1}"));
                self.apply_eq_live(cx);
            }
        }
        if self.ui.button(id!(autoeq_apply_btn)).clicked(actions) {
            let q = self.ui.text_input(id!(autoeq_input)).text();
            let hits = search_autoeq_profiles(&q);
            if let Some(best) = hits.first() {
                self.eq = best.to_eq_config();
                self.sync_controls_from_state(cx);
                self.apply_eq_live(cx);
            }
        }
        for (btn, preset) in [
            (id!(eq_pre_flat), "Flat"),
            (id!(eq_pre_copper), "Warm Copper"),
            (id!(eq_pre_bass), "Deep Bass"),
            (id!(eq_pre_vocal), "Vocal Clarity"),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.eq = EqConfig::preset(preset);
                self.sync_controls_from_state(cx);
                self.apply_eq_live(cx);
            }
        }
        if self.ui.button(id!(eq_import_btn)).clicked(actions)
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
        if self.ui.button(id!(eq_export_btn)).clicked(actions)
            && let Some(file) = rfd::FileDialog::new()
                .set_file_name("ferric-eq-apo.txt")
                .add_filter("Equalizer APO", &["txt"])
                .save_file()
        {
            let _ = std::fs::write(file, self.eq.to_equalizer_apo());
        }

        // 8. CONNECT QR, APPEARANCE / LIQUID GLASS & UPDATES
        if self.ui.button(id!(gen_pair_btn)).clicked(actions)
            || self.ui.button(id!(connect_gen_pair_btn)).clicked(actions)
        {
            self.regenerate_pairing_qr(cx);
        }
        if self.ui.button(id!(copy_pair_btn)).clicked(actions)
            || self.ui.button(id!(connect_copy_pair_btn)).clicked(actions)
        {
            let _ = copy_text_to_clipboard(&self.pairing_uri);
        }
        if let Some(enabled) = self
            .ui
            .ferric_toggle(id!(liquid_glass_toggle))
            .toggled(actions)
        {
            self.theme.material.liquid_glass = enabled;
            if enabled && self.theme.corner_radius_px < 14.0 {
                self.theme.corner_radius_px = 16.0;
            }
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if let Some(v) = self
            .ui
            .ferric_bar_slider(id!(lg_ior_slider))
            .changed(actions)
        {
            self.theme.material.ior = v as f32;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if let Some(v) = self
            .ui
            .ferric_bar_slider(id!(lg_disp_slider))
            .changed(actions)
        {
            self.theme.material.dispersion = v as f32;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if let Some(v) = self
            .ui
            .ferric_bar_slider(id!(lg_rim_slider))
            .changed(actions)
        {
            self.theme.material.specular_rim = v as f32;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if let Some(v) = self
            .ui
            .ferric_bar_slider(id!(lg_rad_slider))
            .changed(actions)
        {
            self.theme.corner_radius_px = v as f32;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        for (btn, preset) in [
            (id!(lg_pre_crystal), LiquidGlassPreset::CrystalPure),
            (id!(lg_pre_prism), LiquidGlassPreset::PrismaticLiquid),
            (id!(lg_pre_copper), LiquidGlassPreset::SmokedCopper),
            (id!(lg_pre_frost), LiquidGlassPreset::UltraFrosted),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.theme.material.apply_preset(preset);
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
            }
        }
        if self.ui.button(id!(pal_amoled_btn)).clicked(actions) {
            self.theme = ThemeConfig::default();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(pal_cream_btn)).clicked(actions) {
            self.theme = ThemeConfig::cream_light();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(theme_export_btn)).clicked(actions)
            && let Some(file) = rfd::FileDialog::new()
                .set_file_name("custom.ferric-theme.json")
                .add_filter("Ferric Theme JSON", &["json"])
                .save_file()
        {
            let _ = self.theme.export_to_file(&file);
        }
        if self.ui.button(id!(theme_import_btn)).clicked(actions)
            && let Some(file) = rfd::FileDialog::new()
                .add_filter("Ferric Theme JSON", &["json"])
                .pick_file()
            && let Ok(loaded) = ThemeConfig::import_from_file(&file)
        {
            self.theme = loaded;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }

        // Software Updates
        if self.ui.button(id!(update_badge_btn)).clicked(actions)
            && let Some(info) = self.available_update.clone()
        {
            self.show_update_popup(cx, &info);
        }
        if self.ui.button(id!(check_updates_btn)).clicked(actions) {
            self.spawn_update_check(cx, true);
        }
        if self.ui.button(id!(open_releases_btn)).clicked(actions)
            || self.ui.button(id!(update_whats_new_btn)).clicked(actions)
        {
            let tag = self.available_update.as_ref().map(|u| u.tag.as_str());
            let _ = open_github_url(&release_page_url(tag));
        }
        if let Some(auto_chk) = self
            .ui
            .ferric_toggle(id!(auto_update_toggle))
            .toggled(actions)
        {
            self.update_settings.auto_check_on_startup = auto_chk;
            if let Some(db) = &self.db {
                let _ = db.save_update_settings(&self.update_settings);
            }
        }
        if self.ui.button(id!(update_now_btn)).clicked(actions) {
            self.start_update_download(cx);
        }
        if self.ui.button(id!(update_popup_close_btn)).clicked(actions) {
            self.dismiss_update_popup(cx);
        }
    }
}

impl AppMain for App {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        self.ensure_initialized(cx);

        if matches!(event, Event::WindowGeomChange(_)) {
            self.hide_native_makepad_caption(cx);
        }

        // Native Windows 11 Frameless Drag & Aero Snap on empty regions of our unified `top_bar`
        if let Event::WindowDragQuery(dq) = event
            && dq.abs.y >= 0.0
            && dq.abs.y <= 52.0
        {
            let search_rect = self.ui.view(id!(search_bar_wrap)).area().rect(cx);
            let sp_rect = self.ui.button(id!(top_spotify_btn)).area().rect(cx);
            let in_search = search_rect.size.x > 0.0
                && dq.abs.x >= search_rect.pos.x - 6.0
                && dq.abs.x <= search_rect.pos.x + search_rect.size.x + 6.0;
            let in_right_btns = sp_rect.size.x > 0.0 && dq.abs.x >= sp_rect.pos.x - 8.0;
            if !in_search && !in_right_btns {
                dq.response.set(WindowDragQueryResponse::Caption);
                return;
            }
        }

        if self.tick_timer.is_event(event).is_some() {
            self.handle_tick(cx);
        }

        // Global Keyboard Shortcuts (Sonora / Spotify Parity)
        if let Event::KeyDown(ke) = event {
            if ke.modifiers.control && ke.key_code == KeyCode::KeyK {
                self.cmdk_open = !self.cmdk_open;
                self.ui
                    .view(id!(cmdk_modal))
                    .set_visible(cx, self.cmdk_open);
                self.ui.redraw(cx);
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::KeyE {
                self.switch_main_page(cx, live_id!(page_eq));
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::KeyL {
                self.switch_main_page(cx, live_id!(page_now_playing));
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::KeyM {
                self.toggle_compact_mini_mode(cx);
                return;
            }
            if ke.key_code == KeyCode::Escape {
                if self.cmdk_open {
                    self.cmdk_open = false;
                    self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
                    self.ui.redraw(cx);
                } else if self.update_popup_visible {
                    self.dismiss_update_popup(cx);
                } else if self.right_drawer_open {
                    self.right_drawer_open = false;
                    self.ui.view(id!(right_drawer)).set_visible(cx, false);
                    self.ui.redraw(cx);
                }
            }
        }

        let snap_opt = self.audio.as_ref().map(|a| a.telemetry());
        let avg_lvl = snap_opt.map_or(0.5_f32, |s| {
            ((s.spectrum_bins[0] + s.spectrum_bins[1]) as f32 / 2_000.0).clamp(0.1, 1.0)
        });
        let props = TrackListScope {
            tracks: Rc::clone(&self.filtered_tracks),
            active_track_id: self.queue.current().map(|t| t.id.clone()),
            is_playing: snap_opt.is_some_and(|s| s.is_playing),
            audio_level: avg_lvl,
        };
        {
            let mut scope = Scope::with_props(&props);
            self.ui.handle_event(cx, event, &mut scope);
        }

        if let Event::Actions(actions) = event {
            self.handle_actions(cx, actions);
        }
    }
}

fn eq_band_ids() -> [(&'static [LiveId], &'static [LiveId]); 10] {
    [
        (id!(eq_b0), id!(eq_v0)),
        (id!(eq_b1), id!(eq_v1)),
        (id!(eq_b2), id!(eq_v2)),
        (id!(eq_b3), id!(eq_v3)),
        (id!(eq_b4), id!(eq_v4)),
        (id!(eq_b5), id!(eq_v5)),
        (id!(eq_b6), id!(eq_v6)),
        (id!(eq_b7), id!(eq_v7)),
        (id!(eq_b8), id!(eq_v8)),
        (id!(eq_b9), id!(eq_v9)),
    ]
}

fn rgba_vec4(hex: &str) -> Vec4 {
    let [r, g, b, a] = hex_to_rgba_f32(hex, 1.0);
    vec4(r, g, b, a)
}

app_main!(App);
