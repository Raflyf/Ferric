use crate::shell::{
    copy_text_to_clipboard, launch_verified_installer, open_allowed_url, open_github_url,
    open_track_video_clip, win32_is_active_window_maximized, win32_minimize_active_window,
    win32_toggle_maximize_active_window,
};
use crate::smtc::{SmtcBridge, SmtcCommand};
use crate::startup_page_override;
use crate::track_list::{TrackListAction, TrackListScope};
use crate::widgets::*;
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

    // Clean AMOLED / Optical Liquid Glass Surface
    FerricSurface = <View> {
        show_bg: true,
        draw_bg: {
            uniform base_col: #0B0B0B
            uniform border_col: #1E1E1E
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

                let center = self.rect_size * 0.5;
                let rel = p - center;
                let half_b = max(center - vec2(r + 1.0, r + 1.0), vec2(1.0, 1.0));
                let q = abs(rel) - half_b;
                let sd = length(max(q, vec2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
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

    FerricCard = <RoundedView> {
        width: Fill,
        height: Fit,
        padding: 20,
        draw_bg: {
            color: #101010,
            border_radius: 12.0,
            border_size: 1.0,
            border_color: #202020
        }
    }

    FerricNavItem = <Button> {
        width: Fill,
        height: 40,
        padding: { left: 14, right: 14 }
        align: { x: 0.0, y: 0.5 }
        draw_text: {
            color: #9E988E,
            color_hover: #F3EEE3,
            color_down: #F3EEE3,
            text_style: { font_size: 13.5 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 9.0);
                let col = mix(mix(vec4(0.0,0.0,0.0,0.0), #151515, self.hover), #1C100A, self.active);
                sdf.fill_keep(col);
                sdf.stroke(mix(vec4(0.0,0.0,0.0,0.0), #D9541E, self.active * 0.65), 1.0);
                return sdf.result;
            }
        }
    }

    FerricPlaylistBtn = <Button> {
        width: Fill,
        height: 34,
        padding: { left: 12, right: 10 }
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
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 7.0);
                let col = mix(mix(vec4(0.0,0.0,0.0,0.0), #141414, self.hover), #181818, self.active);
                sdf.fill(col);
                return sdf.result;
            }
        }
    }

    FerricAccentPill = <Button> {
        width: Fit,
        height: 38,
        padding: { left: 20, right: 20 }
        align: { x: 0.5, y: 0.5 }
        draw_text: {
            color: #000000,
            color_hover: #000000,
            color_down: #000000,
            text_style: { font_size: 12.5 }
        }
        draw_bg: {
            instance hover: 0.0
            instance down: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let r = (self.rect_size.y - 1.0) * 0.5;
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);
                let c = mix(mix(#D9541E, #EB652E, self.hover), #B84214, self.down);
                sdf.fill(c);
                return sdf.result;
            }
        }
    }

    FerricSoftPill = <Button> {
        width: Fit,
        height: 34,
        padding: { left: 15, right: 15 }
        align: { x: 0.5, y: 0.5 }
        draw_text: {
            color: #F3EEE3,
            color_hover: #FFFFFF,
            color_down: #D9541E,
            text_style: { font_size: 12.0 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let r = (self.rect_size.y - 1.0) * 0.5;
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);
                let bg = mix(mix(#141414, #1E1E1E, self.hover), #221109, self.active);
                sdf.fill_keep(bg);
                let border = mix(#282828, #D9541E, self.active);
                sdf.stroke(border, 1.0);
                return sdf.result;
            }
        }
    }

    FerricPlayCircleBtn = <Button> {
        width: 42,
        height: 42,
        padding: 0,
        align: { x: 0.5, y: 0.5 }
        text: "Play"
        draw_text: {
            color: #000000,
            color_hover: #000000,
            text_style: { font_size: 11.0 }
        }
        draw_bg: {
            instance hover: 0.0
            instance down: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let c = self.rect_size * 0.5;
                let r = min(c.x, c.y) - 1.0;
                sdf.circle(c.x, c.y, r);
                let fill = mix(mix(#D9541E, #ED6832, self.hover), #BA4314, self.down);
                sdf.fill(fill);
                return sdf.result;
            }
        }
    }

    FerricInput = <TextInput> {
        width: Fill,
        height: 36,
        draw_bg: {
            color: #090909,
            color_focus: #0F0F0F,
            border_radius: 9.0,
            border_size: 1.0,
            border_color_1: #262626,
            border_color_1_focus: #D9541E
        }
        draw_text: {
            color: #F3EEE3,
            color_empty: #77726A,
            text_style: { font_size: 12.5 }
        }
        draw_cursor: { color: #D9541E }
    }

    App = {{App}} {
        ui: <Root> {
            main_window = <Window> {
                window: { title: "Ferric", inner_size: vec2(1240, 760) }
                pass: { clear_color: #000000 }
                caption_bar = { visible: false }

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
                            if self.glass_bg < 0.5 {
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

                        // 1. UNIFIED FRAMELESS TOP BAR (Spotify / Sonora Clean Header)
                        top_bar = <FerricSurface> {
                            width: Fill,
                            height: 52,
                            margin: { left: 8, right: 8, top: 8, bottom: 4 }
                            padding: { left: 16, right: 10 }
                            flow: Right,
                            align: { y: 0.5 }
                            spacing: 12,
                            draw_bg: { base_col: #0A0A0A, radius: 12.0 }

                            brand_icon = <Image> {
                                width: 26,
                                height: 26,
                                fit: Smallest,
                            }
                            <Label> {
                                draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }
                                text: "Ferric"
                            }

                            <View> { width: Fill, height: 1 }

                            search_bar_wrap = <View> {
                                width: 390,
                                height: 36,
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 6,
                                search_input = <FerricInput> {
                                    width: Fill,
                                    height: 36,
                                    empty_text: "What do you want to play? (Ctrl+K)"
                                    draw_bg: { border_radius: 18.0 }
                                }
                                cmdk_btn = <FerricSoftPill> {
                                    height: 32,
                                    padding: { left: 11, right: 11 }
                                    text: "Ctrl+K"
                                }
                            }

                            <View> { width: Fill, height: 1 }

                            accounts_top_btn = <FerricSoftPill> {
                                height: 32,
                                text: "Sign In"
                            }
                            settings_top_btn = <FerricSoftPill> {
                                height: 32,
                                text: "Settings"
                            }
                            update_badge_btn = <FerricAccentPill> {
                                visible: false,
                                height: 30,
                                padding: { left: 12, right: 12 }
                                text: "Update Ready"
                            }

                            // Crisp Window Controls: Mini-Player, Minimize, Maximize/Restore, Exit
                            win_mini_btn = <FerricWinBtn> { draw_bg: { kind: 3.0 } }
                            win_min_btn = <FerricWinBtn> { draw_bg: { kind: 0.0 } }
                            win_max_btn = <FerricWinBtn> { draw_bg: { kind: 1.0 } }
                            win_close_btn = <FerricWinBtn> { draw_bg: { kind: 2.0 } }
                        }

                        // 2. WORKSPACE MIDDLE (Sidebar + Main Stage + Collapsible Right Drawer)
                        workspace_row = <View> {
                            width: Fill,
                            height: Fill,
                            margin: { left: 8, right: 8, top: 4, bottom: 4 }
                            flow: Right,
                            spacing: 8,

                            // LEFT SIDEBAR (Clean & Calm like Spotify / Sonora)
                            left_sidebar = <FerricSurface> {
                                width: 224,
                                height: Fill,
                                padding: 14,
                                flow: Down,
                                spacing: 6,
                                draw_bg: { base_col: #0A0A0A, radius: 14.0 }

                                nav_home = <FerricNavItem> { text: "Home" }
                                nav_search = <FerricNavItem> { text: "Search" }
                                nav_library = <FerricNavItem> { text: "Your Library", draw_bg: { active: 1.0 } }
                                nav_now_playing = <FerricNavItem> { text: "Now Playing" }

                                <RoundedView> {
                                    width: Fill, height: 1,
                                    margin: { top: 8, bottom: 6 }
                                    draw_bg: { color: #1D1D1D }
                                }

                                <View> {
                                    width: Fill, height: Fit,
                                    flow: Right, align: { y: 0.5 }
                                    <Label> {
                                        width: Fill,
                                        draw_text: { color: #888279, text_style: { font_size: 11.0 } }
                                        text: "Playlists"
                                    }
                                    sidebar_import_btn = <FerricSoftPill> {
                                        height: 26,
                                        padding: { left: 10, right: 10 }
                                        text: "+ Import"
                                    }
                                }

                                pl_all_btn = <FerricPlaylistBtn> { text: "All Songs", draw_bg: { active: 1.0 } }
                                pl_liked_btn = <FerricPlaylistBtn> { text: "Saved Songs" }
                                pl_local_btn = <FerricPlaylistBtn> { text: "Local Lossless" }
                                pl_slot_0 = <FerricPlaylistBtn> { visible: false, text: "Playlist 1" }
                                pl_slot_1 = <FerricPlaylistBtn> { visible: false, text: "Playlist 2" }
                                pl_slot_2 = <FerricPlaylistBtn> { visible: false, text: "Playlist 3" }
                                pl_slot_3 = <FerricPlaylistBtn> { visible: false, text: "Playlist 4" }
                                pl_slot_4 = <FerricPlaylistBtn> { visible: false, text: "Playlist 5" }

                                <View> { width: Fill, height: Fill }

                                sidebar_add_folder_btn = <FerricSoftPill> {
                                    width: Fill,
                                    height: 36,
                                    text: "+ Add Music Folder"
                                }
                            }

                            // CENTER MAIN STAGE (Wide & Breathable)
                            center_stage = <FerricSurface> {
                                width: Fill,
                                height: Fill,
                                flow: Overlay,
                                draw_bg: { base_col: #0B0B0B, radius: 14.0 }

                                // PAGE A: HOME (Quick Picks, Seamless Mix, Listening Capsule)
                                page_home = <ScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: 28,
                                    flow: Down,
                                    spacing: 22,

                                    <View> {
                                        width: Fill, height: Fit, flow: Down, spacing: 6,
                                        <Label> {
                                            draw_text: { color: #F3EEE3, text_style: { font_size: 26.0 } }
                                            text: "Welcome back"
                                        }
                                        home_sub_lbl = <Label> {
                                            draw_text: { color: #9E988E, text_style: { font_size: 13.0 } }
                                            text: "Seamless gapless playback and 6.0s Bass-Swap Automix ready."
                                        }
                                    }

                                    // 3 Clean Action Cards
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 14,

                                        <FerricCard> {
                                            width: Fill, flow: Down, spacing: 10,
                                            <Label> {
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }
                                                text: "Local Music"
                                            }
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }
                                                text: "Play FLAC, Opus, MP3, AAC, ALAC, and WAV directly from your folders."
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8, margin: { top: 4 }
                                                home_add_folder_btn = <FerricAccentPill> { height: 34, text: "Add Folder" }
                                                home_demo_btn = <FerricSoftPill> { height: 34, text: "Play Demo" }
                                            }
                                        }

                                        <FerricCard> {
                                            width: Fill, flow: Down, spacing: 10,
                                            <Label> {
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }
                                                text: "Spotify & YouTube"
                                            }
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }
                                                text: "Sync your playlists and liked songs via official OAuth 2.0 PKCE."
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8, margin: { top: 4 }
                                                home_spotify_btn = <FerricSoftPill> { height: 34, text: "Spotify" }
                                                home_youtube_btn = <FerricSoftPill> { height: 34, text: "YouTube" }
                                            }
                                        }

                                        <FerricCard> {
                                            width: Fill, flow: Down, spacing: 10,
                                            <Label> {
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }
                                                text: "Seamless Fade-In & EQ"
                                            }
                                            home_dsp_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }
                                                text: "6.0s Bass-Swap Crossfade · 240ms Play Fade-In · Gapless On"
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8, margin: { top: 4 }
                                                home_fade_btn = <FerricSoftPill> { height: 34, text: "Fade-In Settings" }
                                                home_eq_btn = <FerricSoftPill> { height: 34, text: "Equalizer" }
                                            }
                                        }
                                    }

                                    // Recently Played / Quick Picks
                                    <FerricCard> {
                                        width: Fill, flow: Down, spacing: 10,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 16.0 } }
                                                text: "Quick Picks"
                                            }
                                            stats_capsule_lbl = <Label> {
                                                draw_text: { color: #888279, text_style: { font_size: 12.0 } }
                                                text: "0 songs played · 0 min listened"
                                            }
                                        }
                                        recent_btn_0 = <FerricPlaylistBtn> { height: 38, text: "1. Click 'Play Demo' or 'Add Folder' to populate Quick Picks" }
                                        recent_btn_1 = <FerricPlaylistBtn> { visible: false, height: 38, text: "" }
                                        recent_btn_2 = <FerricPlaylistBtn> { visible: false, height: 38, text: "" }
                                        recent_btn_3 = <FerricPlaylistBtn> { visible: false, height: 38, text: "" }
                                    }
                                }

                                // PAGE B: YOUR LIBRARY & SEARCH (Clean Spotify / Sonora Playlist View)
                                page_library = <View> {
                                    visible: true,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 26, right: 26, top: 22, bottom: 14 }
                                    flow: Down,
                                    spacing: 16,

                                    // Hero Header
                                    <View> {
                                        width: Fill,
                                        height: Fit,
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 20,

                                        hero_art = <FerricArtTile> {
                                            width: 104,
                                            height: 104,
                                            draw_bg: { variant: 0.0, radius: 12.0 }
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
                                                text: "0 songs · Seamless Fade-In & Gapless active"
                                            }
                                            <View> {
                                                width: Fill,
                                                height: Fit,
                                                margin: { top: 6 }
                                                flow: Right,
                                                align: { y: 0.5 }
                                                spacing: 10,
                                                play_all_btn = <FerricAccentPill> { text: "Play" }
                                                hero_shuffle_btn = <FerricSoftPill> { height: 38, text: "Shuffle" }
                                                add_folder_btn = <FerricSoftPill> { height: 38, text: "+ Add Folder" }
                                                demo_suite_btn = <FerricSoftPill> { height: 38, text: "Load Demo Songs" }
                                            }
                                        }
                                    }

                                    // Filter Chips + Sort Row
                                    <View> {
                                        width: Fill,
                                        height: Fit,
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 8,
                                        flt_all_btn = <FerricSoftPill> { height: 30, text: "All", draw_bg: { active: 1.0 } }
                                        flt_liked_btn = <FerricSoftPill> { height: 30, text: "Saved" }
                                        flt_local_btn = <FerricSoftPill> { height: 30, text: "Local" }
                                        flt_spotify_btn = <FerricSoftPill> { height: 30, text: "Spotify" }
                                        flt_youtube_btn = <FerricSoftPill> { height: 30, text: "YouTube" }
                                        <View> { width: Fill, height: 1 }
                                        sort_cycle_btn = <FerricSoftPill> { height: 30, text: "Sort: Artist" }
                                    }

                                    // Clean Table Column Headers
                                    <View> {
                                        width: Fill,
                                        height: 24,
                                        padding: { left: 16, right: 18 }
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 14,
                                        <Label> { width: 26, draw_text: { color: #78736B, text_style: { font_size: 11.0 } }, text: "#" }
                                        <View> { width: 40, height: 1 }
                                        <Label> { width: Fill, draw_text: { color: #78736B, text_style: { font_size: 11.0 } }, text: "Title" }
                                        <Label> { width: 210, draw_text: { color: #78736B, text_style: { font_size: 11.0 } }, text: "Album" }
                                        <View> { width: 54, height: 1 }
                                        <Label> { width: 52, align: { x: 1.0 }, draw_text: { color: #78736B, text_style: { font_size: 11.0 } }, text: "Time" }
                                    }

                                    track_list = <TrackList> {}
                                }

                                // PAGE C: NOW PLAYING & SYNCED LYRICS (Card / Motion Canvas / Lyrics / Video Clip)
                                page_now_playing = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: 26,
                                    flow: Down,
                                    spacing: 18,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8,
                                        <Label> {
                                            width: Fill,
                                            draw_text: { color: #F3EEE3, text_style: { font_size: 20.0 } }
                                            text: "Now Playing"
                                        }
                                        np_mode_canvas_btn = <FerricSoftPill> { height: 32, text: "Motion Canvas", draw_bg: { active: 1.0 } }
                                        np_mode_card_btn = <FerricSoftPill> { height: 32, text: "Cover Card" }
                                        np_watch_video_btn = <FerricSoftPill> { height: 32, text: "Watch Video Clip" }
                                    }

                                    <View> {
                                        width: Fill, height: Fill, flow: Right, spacing: 22,

                                        // Left Stage: Motion Canvas / Cover Card + Signal Path
                                        <View> {
                                            width: 340, height: Fill, flow: Down, spacing: 14,
                                            np_canvas = <FerricNowPlayingCanvas> {
                                                width: Fill,
                                                height: 270,
                                            }
                                            np_stage_title_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 19.0 } }
                                                text: "No track playing"
                                            }
                                            np_stage_artist_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #9E988E, text_style: { font_size: 13.5 } }
                                                text: "Select any song or play demo tracks"
                                            }
                                            signal_path_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #888279, text_style: { font_size: 11.5 } }
                                                text: "Signal Path: 6.0s Bass-Swap Automix · DirectForm1 EQ · WASAPI"
                                            }
                                        }

                                        // Right Stage: Synced Karaoke Lyrics (Free Multi-Provider)
                                        <FerricCard> {
                                            width: Fill, height: Fill, padding: 24, flow: Down, spacing: 14,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8,
                                                lyrics_source_lbl = <Label> {
                                                    width: Fill,
                                                    draw_text: { color: #888279, text_style: { font_size: 11.5 } }
                                                    text: "Lyrics · Automatic Free API (LRCLIB + Lyrics.ovh)"
                                                }
                                                lyr_minus_btn = <FerricSoftPill> { height: 28, padding: { left: 10, right: 10 }, text: "-0.5s" }
                                                lyr_plus_btn = <FerricSoftPill> { height: 28, padding: { left: 10, right: 10 }, text: "+0.5s" }
                                                lyr_copy_btn = <FerricSoftPill> { height: 28, padding: { left: 10, right: 10 }, text: "Copy" }
                                            }

                                            <View> { width: Fill, height: Fill }
                                            lyric_prev2_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #59554F, text_style: { font_size: 14.0 } }
                                                text: ""
                                            }
                                            lyric_prev_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #888279, text_style: { font_size: 15.5 } }
                                                text: ""
                                            }
                                            lyric_active_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #D9541E, text_style: { font_size: 21.0 } }
                                                text: "Lyrics appear automatically when a song plays."
                                            }
                                            lyric_next_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #9E988E, text_style: { font_size: 15.5 } }
                                                text: ""
                                            }
                                            lyric_next2_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #59554F, text_style: { font_size: 14.0 } }
                                                text: ""
                                            }
                                            <View> { width: Fill, height: Fill }
                                        }
                                    }
                                }

                                // PAGE D: SETTINGS (4 Clean, Uncluttered Tabs)
                                page_settings = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 26, right: 26, top: 20, bottom: 16 }
                                    flow: Down,
                                    spacing: 16,

                                    // Settings Header + 4 Tab Pills
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 10,
                                        <Label> {
                                            draw_text: { color: #F3EEE3, text_style: { font_size: 22.0 } }
                                            text: "Settings"
                                        }
                                        <View> { width: 14, height: 1 }
                                        tab_accounts_btn = <FerricSoftPill> { height: 34, text: "Accounts & Sign In", draw_bg: { active: 1.0 } }
                                        tab_playback_btn = <FerricSoftPill> { height: 34, text: "Playback & Fade-In" }
                                        tab_eq_btn = <FerricSoftPill> { height: 34, text: "Equalizer & AutoEq" }
                                        tab_appearance_btn = <FerricSoftPill> { height: 34, text: "Appearance & Updates" }
                                    }

                                    // TAB 1: ACCOUNTS & SIGN IN (Spotify, YouTube Music, Subsonic, Folders)
                                    subpage_accounts = <ScrollYView> {
                                        visible: true,
                                        width: Fill,
                                        height: Fill,
                                        flow: Down,
                                        spacing: 14,

                                        oauth_status_lbl = <Label> {
                                            width: Fill,
                                            draw_text: { color: #D9541E, text_style: { font_size: 12.5 } }
                                            text: "Redirect URI for Spotify & Google OAuth: http://127.0.0.1:43824/callback"
                                        }

                                        // Spotify Official Login Card
                                        <FerricCard> {
                                            flow: Down, spacing: 10,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                <Label> {
                                                    width: Fill,
                                                    draw_text: { color: #F3EEE3, text_style: { font_size: 16.0 } }
                                                    text: "Spotify Account (Official OAuth 2.0 PKCE)"
                                                }
                                                spotify_state_lbl = <Label> {
                                                    draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }
                                                    text: "Not signed in"
                                                }
                                            }
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }
                                                text: "1. Create a free app at developer.spotify.com/dashboard and add Redirect URI http://127.0.0.1:43824/callback. 2. Paste your Client ID below and click Sign in with Spotify."
                                            }
                                            spotify_id_input = <FerricInput> {
                                                empty_text: "Paste Spotify Client ID..."
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8,
                                                spotify_login_btn = <FerricAccentPill> { height: 34, text: "Sign in with Spotify" }
                                                spotify_sync_btn = <FerricSoftPill> { height: 34, text: "Sync Playlists" }
                                                copy_redirect_btn = <FerricSoftPill> { height: 34, text: "Copy Redirect URI" }
                                                open_spotify_dev_btn = <FerricSoftPill> { height: 34, text: "Spotify Portal" }
                                                spotify_logout_btn = <FerricSoftPill> { height: 34, text: "Disconnect" }
                                            }
                                        }

                                        // YouTube & YouTube Music Official Login Card
                                        <FerricCard> {
                                            flow: Down, spacing: 10,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                <Label> {
                                                    width: Fill,
                                                    draw_text: { color: #F3EEE3, text_style: { font_size: 16.0 } }
                                                    text: "YouTube & YouTube Music (Google OAuth 2.0 PKCE)"
                                                }
                                                yt_state_lbl = <Label> {
                                                    draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }
                                                    text: "Not signed in"
                                                }
                                            }
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }
                                                text: "Create a Desktop OAuth Client in Google Cloud Console (YouTube Data API v3 enabled), paste Client ID (and optional Client Secret), then sign in via browser."
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 10,
                                                yt_id_input = <FerricInput> { empty_text: "Google OAuth Client ID (.apps.googleusercontent.com)" }
                                                yt_secret_input = <FerricInput> { empty_text: "Client Secret (optional for PKCE)" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 10,
                                                yt_login_btn = <FerricAccentPill> { height: 34, text: "Sign in with Google" }
                                                yt_sync_btn = <FerricSoftPill> { height: 34, text: "Sync YouTube" }
                                                open_google_dev_btn = <FerricSoftPill> { height: 34, text: "Google Console" }
                                                yt_logout_btn = <FerricSoftPill> { height: 34, text: "Disconnect" }
                                                oauth_cancel_btn = <FerricSoftPill> { visible: false, height: 34, text: "Cancel Login" }
                                                <View> { width: Fill, height: 1 }
                                                <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Music Only" }
                                                yt_cat_toggle = <FerricToggle> { is_on: true }
                                            }
                                        }

                                        // OpenSubsonic / Navidrome + Local Matcher Card
                                        <FerricCard> {
                                            flow: Down, spacing: 10,
                                            <Label> {
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 16.0 } }
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
                                                sub_sync_btn = <FerricSoftPill> { height: 34, text: "Connect Subsonic" }
                                                match_local_btn = <FerricSoftPill> { height: 34, text: "Match Cloud Songs to Local Files" }
                                                rescan_btn = <FerricSoftPill> { height: 34, text: "Rescan Local Folders" }
                                            }
                                        }
                                    }

                                    // TAB 2: PLAYBACK & SEAMLESS FADE-IN (Spotify Playback Style)
                                    subpage_playback = <ScrollYView> {
                                        visible: false,
                                        width: Fill,
                                        height: Fill,
                                        flow: Down,
                                        spacing: 14,

                                        <FerricCard> {
                                            flow: Down, spacing: 14,
                                            <Label> {
                                                draw_text: { color: #F3EEE3, text_style: { font_size: 16.5 } }
                                                text: "Seamless Fade-In & Automix (Tanpa Jeda)"
                                            }
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #9E988E, text_style: { font_size: 12.5 } }
                                                text: "Dual-deck engine pre-buffers the next song ahead of time and blends songs smoothly with zero silence."
                                            }

                                            // Crossfade Duration Slider
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Crossfade Overlap Between Songs" }
                                                xfade_val_lbl = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 13.0 } }, text: "6.0 s" }
                                            }
                                            xfade_slider = <FerricBarSlider> { min: 0.0, max: 12.0, value: 6.0 }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8,
                                                xf_0_btn = <FerricSoftPill> { height: 30, text: "0s (Pure Gapless)" }
                                                xf_3_btn = <FerricSoftPill> { height: 30, text: "3s Subtle" }
                                                xf_6_btn = <FerricSoftPill> { height: 30, text: "6s Automix" }
                                                xf_10_btn = <FerricSoftPill> { height: 30, text: "10s Long Mix" }
                                            }

                                            // Play / Resume / Seek Velvet Fade-In Slider
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 } margin: { top: 4 }
                                                <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Smooth Fade-In on Play / Resume / Seek" }
                                                fadein_val_lbl = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 13.0 } }, text: "240 ms" }
                                            }
                                            fadein_slider = <FerricBarSlider> { min: 0.0, max: 1500.0, value: 240.0 }

                                            // Fade Curve Selector
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8, margin: { top: 4 }
                                                <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Mix Curve Style" }
                                                curve_bass_btn = <FerricSoftPill> { height: 30, text: "Bass-Swap Automix", draw_bg: { active: 1.0 } }
                                                curve_eqp_btn = <FerricSoftPill> { height: 30, text: "Equal-Power" }
                                                curve_velvet_btn = <FerricSoftPill> { height: 30, text: "S-Curve Velvet" }
                                            }
                                        }

                                        // Clean Spotify-style Toggle Rows
                                        <FerricCard> {
                                            flow: Down, spacing: 14,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Down, spacing: 2,
                                                    <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Gapless Album Lock" }
                                                    <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Keep 0ms sample-exact transitions when playing consecutive songs from the same album." }
                                                }
                                                gapless_lock_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Down, spacing: 2,
                                                    <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Skip Intro & Outro Silence" }
                                                    <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Automatically skip dead silence at song edges so the next song fades in immediately." }
                                                }
                                                trim_silence_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Down, spacing: 2,
                                                    <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Normalize Volume (ReplayGain)" }
                                                    <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Set the same comfortable loudness level across all tracks." }
                                                }
                                                replay_gain_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Down, spacing: 2,
                                                    <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Mono Audio" }
                                                    <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Makes the left and right speakers play the exact same audio." }
                                                }
                                                mono_toggle = <FerricToggle> { is_on: false }
                                            }
                                        }

                                        // Playback Speed & Sleep Timer Card
                                        <FerricCard> {
                                            flow: Down, spacing: 12,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8,
                                                <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Playback Speed" }
                                                spd_075_btn = <FerricSoftPill> { height: 30, text: "0.75x" }
                                                spd_100_btn = <FerricSoftPill> { height: 30, text: "1.0x Normal", draw_bg: { active: 1.0 } }
                                                spd_125_btn = <FerricSoftPill> { height: 30, text: "1.25x" }
                                                spd_150_btn = <FerricSoftPill> { height: 30, text: "1.5x" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8,
                                                sleep_status_lbl = <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }, text: "Sleep Timer: Off" }
                                                slp_off_btn = <FerricSoftPill> { height: 30, text: "Off", draw_bg: { active: 1.0 } }
                                                slp_15_btn = <FerricSoftPill> { height: 30, text: "15m" }
                                                slp_30_btn = <FerricSoftPill> { height: 30, text: "30m" }
                                                slp_60_btn = <FerricSoftPill> { height: 30, text: "60m" }
                                                slp_eot_btn = <FerricSoftPill> { height: 30, text: "End of Song" }
                                            }
                                        }
                                    }

                                    // TAB 3: EQUALIZER & AUTOEQ (FigCraft D2 Studio)
                                    subpage_eq = <ScrollYView> {
                                        visible: false,
                                        width: Fill,
                                        height: Fill,
                                        flow: Down,
                                        spacing: 14,

                                        <FerricCard> {
                                            flow: Down, spacing: 12,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 10,
                                                <View> {
                                                    width: Fill, height: Fit, flow: Down, spacing: 2,
                                                    <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 16.5 } }, text: "10-Band Parametric Equalizer & AutoEq" }
                                                    eq_status_lbl = <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Preset: Flat · Double-click any fader to reset" }
                                                }
                                                eq_import_btn = <FerricSoftPill> { height: 32, text: "Import APO" }
                                                eq_export_btn = <FerricSoftPill> { height: 32, text: "Export APO" }
                                                eq_enable_toggle = <FerricToggle> { is_on: true }
                                            }

                                            // AutoEq Headphone Search + Quick Presets
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 8,
                                                autoeq_input = <FerricInput> {
                                                    width: 240, height: 32,
                                                    empty_text: "Search 24 AutoEq headphones..."
                                                }
                                                autoeq_apply_btn = <FerricAccentPill> { height: 32, padding: { left: 14, right: 14 }, text: "Apply Best Match" }
                                                eq_pre_flat = <FerricSoftPill> { height: 32, text: "Flat" }
                                                eq_pre_copper = <FerricSoftPill> { height: 32, text: "Warm Copper" }
                                                eq_pre_bass = <FerricSoftPill> { height: 32, text: "Deep Bass" }
                                                eq_pre_vocal = <FerricSoftPill> { height: 32, text: "Vocal" }
                                            }

                                            eq_graph = <FerricEqGraph> {}

                                            // Preamp Row
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 12,
                                                <Label> { width: 90, draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Preamp" }
                                                eq_preamp_slider = <FerricBarSlider> { min: -12.0, max: 12.0, value: 0.0 }
                                                eq_preamp_lbl = <Label> { width: 64, align: { x: 1.0 }, draw_text: { color: #D9541E, text_style: { font_size: 12.0 } }, text: "0.0 dB" }
                                            }

                                            // 10 Vertical Octave Faders
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6, margin: { top: 4 }
                                                <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v0 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 10.5 } }, text: "0.0" } eq_b0 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 10.5 } }, text: "32" } }
                                                <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v1 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 10.5 } }, text: "0.0" } eq_b1 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 10.5 } }, text: "64" } }
                                                <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v2 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 10.5 } }, text: "0.0" } eq_b2 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 10.5 } }, text: "125" } }
                                                <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v3 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 10.5 } }, text: "0.0" } eq_b3 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 10.5 } }, text: "250" } }
                                                <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v4 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 10.5 } }, text: "0.0" } eq_b4 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 10.5 } }, text: "500" } }
                                                <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v5 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 10.5 } }, text: "0.0" } eq_b5 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 10.5 } }, text: "1k" } }
                                                <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v6 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 10.5 } }, text: "0.0" } eq_b6 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 10.5 } }, text: "2k" } }
                                                <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v7 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 10.5 } }, text: "0.0" } eq_b7 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 10.5 } }, text: "4k" } }
                                                <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v8 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 10.5 } }, text: "0.0" } eq_b8 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 10.5 } }, text: "8k" } }
                                                <View> { width: Fill, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v9 = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 10.5 } }, text: "0.0" } eq_b9 = <FerricVertSlider> {} <Label> { draw_text: { color: #888279, text_style: { font_size: 10.5 } }, text: "16k" } }
                                            }
                                        }
                                    }

                                    // TAB 4: APPEARANCE (LIQUID GLASS) & UPDATES
                                    subpage_appearance = <ScrollYView> {
                                        visible: false,
                                        width: Fill,
                                        height: Fill,
                                        flow: Right,
                                        spacing: 16,

                                        // Left Column: Theme, Liquid Glass Controls & Software Updates
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 14,

                                            <FerricCard> {
                                                flow: Down, spacing: 12,
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                    <View> {
                                                        width: Fill, height: Fit, flow: Down, spacing: 2,
                                                        <Label> { draw_text: { color: #F3EEE3, text_style: { font_size: 16.0 } }, text: "Liquid Glass Theme (All UI Surfaces)" }
                                                        <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Curved Snell bevel refraction, chromatic edge dispersion & specular rim." }
                                                    }
                                                    liquid_glass_toggle = <FerricToggle> { is_on: false }
                                                }

                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, spacing: 8,
                                                    lg_pre_crystal = <FerricSoftPill> { height: 30, text: "Crystal Pure" }
                                                    lg_pre_prism = <FerricSoftPill> { height: 30, text: "Prismatic Liquid" }
                                                    lg_pre_copper = <FerricSoftPill> { height: 30, text: "Smoked Copper" }
                                                    lg_pre_frost = <FerricSoftPill> { height: 30, text: "Ultra Frosted" }
                                                }

                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                    <Label> { width: Fill, draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Refraction Index (IOR)" }
                                                    lg_ior_lbl = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 12.0 } }, text: "1.48" }
                                                }
                                                lg_ior_slider = <FerricBarSlider> { min: 1.05, max: 1.85, value: 1.48 }

                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                    <Label> { width: Fill, draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Chromatic Edge Dispersion" }
                                                    lg_disp_lbl = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 12.0 } }, text: "42%" }
                                                }
                                                lg_disp_slider = <FerricBarSlider> { min: 0.0, max: 1.0, value: 0.42 }

                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                                    <Label> { width: Fill, draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Specular Rim & Corner Radius" }
                                                    lg_rim_lbl = <Label> { draw_text: { color: #D9541E, text_style: { font_size: 12.0 } }, text: "78% · 14px" }
                                                }
                                                lg_rim_slider = <FerricBarSlider> { min: 0.0, max: 1.0, value: 0.78 }
                                                lg_rad_slider = <FerricBarSlider> { min: 4.0, max: 28.0, value: 14.0 }

                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, spacing: 8, margin: { top: 4 }
                                                    pal_amoled_btn = <FerricSoftPill> { height: 32, text: "AMOLED Dark" }
                                                    pal_cream_btn = <FerricSoftPill> { height: 32, text: "Cream Light" }
                                                    theme_import_btn = <FerricSoftPill> { height: 32, text: "Import JSON" }
                                                    theme_export_btn = <FerricSoftPill> { height: 32, text: "Export JSON" }
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
                                                        update_status_lbl = <Label> { draw_text: { color: #9E988E, text_style: { font_size: 12.0 } }, text: "Current version: v0.1.0" }
                                                    }
                                                    auto_update_toggle = <FerricToggle> { is_on: true }
                                                }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, spacing: 8,
                                                    check_updates_btn = <FerricAccentPill> { height: 34, text: "Check for Updates" }
                                                    open_releases_btn = <FerricSoftPill> { height: 34, text: "GitHub Releases" }
                                                }
                                            }
                                        }

                                        // Right Column: Live Optical Refractive Mini-Player Preview (LG2)
                                        <View> {
                                            width: 340, height: Fit, flow: Down, spacing: 8,
                                            <Label> {
                                                draw_text: { color: #888279, text_style: { font_size: 11.0 } }
                                                text: "LIVE OPTICAL PREVIEW · FLOATING MINI-PLAYER"
                                            }
                                            lg_preview = <LiquidGlassPreview> {}
                                        }
                                    }
                                }
                            }

                            // COLLAPSIBLE RIGHT DRAWER (Hidden by default so main stage stays spacious!)
                            right_drawer = <FerricSurface> {
                                visible: false,
                                width: 288,
                                height: Fill,
                                padding: 16,
                                flow: Down,
                                spacing: 12,
                                draw_bg: { base_col: #0A0A0A, radius: 14.0 }

                                <View> {
                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 } spacing: 6,
                                    drawer_queue_tab = <FerricSoftPill> { height: 30, text: "Queue", draw_bg: { active: 1.0 } }
                                    drawer_connect_tab = <FerricSoftPill> { height: 30, text: "Connect" }
                                    <View> { width: Fill, height: 1 }
                                    drawer_close_btn = <FerricSoftPill> { height: 28, padding: { left: 10, right: 10 }, text: "Close" }
                                }

                                // Drawer View 1: Up Next Queue
                                drawer_queue_view = <View> {
                                    visible: true,
                                    width: Fill, height: Fill, flow: Down, spacing: 8,
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                        <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 14.5 } }, text: "Up Next" }
                                        clear_queue_btn = <FerricSoftPill> { height: 26, padding: { left: 10, right: 10 }, text: "Clear" }
                                    }
                                    queue_sub_lbl = <Label> {
                                        draw_text: { color: #888279, text_style: { font_size: 11.5 } }
                                        text: "Gapless Deck B pre-buffered"
                                    }
                                    q_slot_0 = <FerricPlaylistBtn> { visible: false, height: 38, text: "" }
                                    q_slot_1 = <FerricPlaylistBtn> { visible: false, height: 38, text: "" }
                                    q_slot_2 = <FerricPlaylistBtn> { visible: false, height: 38, text: "" }
                                    q_slot_3 = <FerricPlaylistBtn> { visible: false, height: 38, text: "" }
                                    q_slot_4 = <FerricPlaylistBtn> { visible: false, height: 38, text: "" }
                                }

                                // Drawer View 2: Ferric Connect QR & LAN Peers
                                drawer_connect_view = <View> {
                                    visible: false,
                                    width: Fill, height: Fill, flow: Down, spacing: 10,
                                    <Label> {
                                        draw_text: { color: #F3EEE3, text_style: { font_size: 14.5 } }
                                        text: "Ferric Connect (LAN & P2P)"
                                    }
                                    <Label> {
                                        width: Fill,
                                        draw_text: { color: #9E988E, text_style: { font_size: 11.5 } }
                                        text: "Scan QR from Ferric Android for remote speaker handoff."
                                    }
                                    <View> {
                                        width: Fill, height: Fit, align: { x: 0.5 }, margin: { top: 4, bottom: 4 }
                                        qr_view = <FerricQrView> { width: 148, height: 148 }
                                    }
                                    lan_ip_lbl = <Label> {
                                        width: Fill,
                                        draw_text: { color: #D9541E, text_style: { font_size: 11.5 } }
                                        text: "LAN: 127.0.0.1:43821 · Desktop-PC Active"
                                    }
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 6,
                                        gen_pair_btn = <FerricSoftPill> { height: 30, text: "New QR" }
                                        copy_pair_btn = <FerricSoftPill> { height: 30, text: "Copy Link" }
                                    }
                                    peers_lbl = <Label> {
                                        width: Fill,
                                        draw_text: { color: #9E988E, text_style: { font_size: 11.5 } }
                                        text: "Listening for Ferric devices on local Wi-Fi..."
                                    }
                                }
                            }
                        }

                        // 3. BOTTOM PLAYER BAR (Spacious & Familiar like Spotify / Sonora)
                        bottom_bar = <FerricSurface> {
                            width: Fill,
                            height: 82,
                            margin: { left: 8, right: 8, top: 4, bottom: 8 }
                            padding: { left: 16, right: 16 }
                            flow: Right,
                            align: { y: 0.5 }
                            spacing: 18,
                            draw_bg: { base_col: #0A0A0A, radius: 14.0 }

                            // Left: Cover Art + Song Title / Artist + Save
                            <View> {
                                width: 268,
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
                                        draw_text: { color: #F3EEE3, text_style: { font_size: 13.5 } }
                                        text: "Ready to Play"
                                    }
                                    np_artist_lbl = <Label> {
                                        width: Fill,
                                        draw_text: { color: #9E988E, text_style: { font_size: 11.5 } }
                                        text: "Ferric Hi-Fi Player"
                                    }
                                }
                                np_like_btn = <FerricSoftPill> {
                                    height: 28,
                                    padding: { left: 10, right: 10 }
                                    text: "Save"
                                }
                            }

                            // Center: Transport Controls + Clean Seekbar
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
                                    shuffle_btn = <FerricSoftPill> { height: 30, padding: { left: 12, right: 12 }, text: "Shuffle" }
                                    prev_btn = <FerricSoftPill> { height: 32, padding: { left: 13, right: 13 }, text: "Prev" }
                                    play_pause_btn = <FerricPlayCircleBtn> {}
                                    next_btn = <FerricSoftPill> { height: 32, padding: { left: 13, right: 13 }, text: "Next" }
                                    repeat_btn = <FerricSoftPill> { height: 30, padding: { left: 12, right: 12 }, text: "Repeat" }
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
                                        width: 50,
                                        height: 20,
                                        padding: 0,
                                        align: { x: 1.0, y: 0.5 }
                                        text: "00:00"
                                        draw_text: { color: #888279, color_hover: #F3EEE3, text_style: { font_size: 11.0 } }
                                        draw_bg: { fn pixel(self) -> vec4 { return vec4(0.0, 0.0, 0.0, 0.0); } }
                                    }
                                }
                            }

                            // Right: Lyrics, Queue, Connect & Clean Volume Bar
                            <View> {
                                width: 280,
                                height: Fill,
                                flow: Right,
                                align: { x: 1.0, y: 0.5 }
                                spacing: 8,
                                codec_pill_btn = <FerricSoftPill> {
                                    height: 28,
                                    padding: { left: 10, right: 10 }
                                    text: "Lossless"
                                }
                                bar_lyrics_btn = <FerricSoftPill> {
                                    height: 30,
                                    padding: { left: 11, right: 11 }
                                    text: "Lyrics"
                                }
                                bar_queue_btn = <FerricSoftPill> {
                                    height: 30,
                                    padding: { left: 11, right: 11 }
                                    text: "Queue"
                                }
                                bar_connect_btn = <FerricSoftPill> {
                                    height: 30,
                                    padding: { left: 11, right: 11 }
                                    text: "Devices"
                                }
                                vol_slider = <FerricBarSlider> {
                                    width: 78,
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
                        align: { x: 0.5, y: 0.18 }
                        show_bg: true,
                        draw_bg: { fn pixel(self) -> vec4 { return vec4(0.0, 0.0, 0.0, 0.72); } }

                        <FerricSurface> {
                            width: 520,
                            height: Fit,
                            padding: 20,
                            flow: Down,
                            spacing: 10,
                            draw_bg: { base_col: #0E0E0E, border_col: #D9541E, radius: 14.0 }

                            <View> {
                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }
                                <Label> { width: Fill, draw_text: { color: #F3EEE3, text_style: { font_size: 15.5 } }, text: "Command Palette" }
                                cmdk_close_btn = <FerricSoftPill> { height: 28, text: "Esc" }
                            }
                            cmd_demo_btn = <FerricNavItem> { text: "Play Seamless Demo Suite (Test 6s Fade-In & Gapless)" }
                            cmd_glass_btn = <FerricNavItem> { text: "Toggle Liquid Glass Theme On / Off" }
                            cmd_fade_btn = <FerricNavItem> { text: "Open Playback & Seamless Fade-In Settings" }
                            cmd_eq_btn = <FerricNavItem> { text: "Open 10-Band Parametric Equalizer & AutoEq" }
                            cmd_accounts_btn = <FerricNavItem> { text: "Sign in to Spotify or YouTube Music (OAuth PKCE)" }
                            cmd_sleep30_btn = <FerricNavItem> { text: "Start 30-Minute Sleep Timer (Smooth Fade-Out)" }
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

                        <FerricSurface> {
                            width: 460,
                            height: Fit,
                            padding: 24,
                            flow: Down,
                            spacing: 12,
                            draw_bg: { base_col: #0E0E0E, border_col: #D9541E, radius: 16.0 }

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
                                update_whats_new_btn = <FerricSoftPill> { height: 36, text: "Release Notes" }
                                update_popup_close_btn = <FerricSoftPill> { height: 36, text: "Later" }
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
    show_remaining_time: bool,
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
    fn ensure_initialized(&mut self, cx: &mut Cx) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        self.np_canvas_animated = true;

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
            "accounts" => {
                self.switch_main_page(cx, live_id!(page_settings));
                self.switch_settings_tab(cx, live_id!(subpage_accounts));
            }
            "fade" | "playback" => {
                self.switch_main_page(cx, live_id!(page_settings));
                self.switch_settings_tab(cx, live_id!(subpage_playback));
            }
            "eq" => {
                self.switch_main_page(cx, live_id!(page_settings));
                self.switch_settings_tab(cx, live_id!(subpage_eq));
            }
            "appearance" => {
                self.switch_main_page(cx, live_id!(page_settings));
                self.switch_settings_tab(cx, live_id!(subpage_appearance));
            }
            "glass" => {
                self.theme.material.liquid_glass = true;
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
                self.switch_main_page(cx, live_id!(page_settings));
                self.switch_settings_tab(cx, live_id!(subpage_appearance));
            }
            "connect" => {
                self.open_right_drawer(cx, true);
            }
            "cmdk" => {
                self.cmdk_open = true;
                self.ui.view(id!(cmdk_modal)).set_visible(cx, true);
            }
            _ => {}
        }
    }

    fn switch_main_page(&mut self, cx: &mut Cx, target: LiveId) {
        for (pid, nav_id) in [
            (live_id!(page_home), id!(nav_home)),
            (live_id!(page_library), id!(nav_library)),
            (live_id!(page_now_playing), id!(nav_now_playing)),
        ] {
            let active = pid == target;
            self.ui.view(&[pid]).set_visible(cx, active);
            self.ui.button(nav_id).apply_over(
                cx,
                live! { draw_bg: { active: (if active { 1.0 } else { 0.0 }) } },
            );
        }
        let is_settings = target == live_id!(page_settings);
        self.ui
            .view(id!(page_settings))
            .set_visible(cx, is_settings);
        self.ui.button(id!(settings_top_btn)).apply_over(
            cx,
            live! { draw_bg: { active: (if is_settings { 1.0 } else { 0.0 }) } },
        );
        self.ui.redraw(cx);
    }

    fn switch_settings_tab(&mut self, cx: &mut Cx, sub_target: LiveId) {
        for (sid, btn_id) in [
            (live_id!(subpage_accounts), id!(tab_accounts_btn)),
            (live_id!(subpage_playback), id!(tab_playback_btn)),
            (live_id!(subpage_eq), id!(tab_eq_btn)),
            (live_id!(subpage_appearance), id!(tab_appearance_btn)),
        ] {
            let active = sid == sub_target;
            self.ui.view(&[sid]).set_visible(cx, active);
            self.ui.button(btn_id).apply_over(
                cx,
                live! { draw_bg: { active: (if active { 1.0 } else { 0.0 }) } },
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
            dvec2(460.0, 152.0)
        } else {
            dvec2(1240.0, 760.0)
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
                .set_matrix(cx, qr.size, qr.modules);
        }
        self.ui.label(id!(lan_ip_lbl)).set_text(
            cx,
            &format!("LAN: {}:43821 · Desktop-PC Active", self.lan_ip),
        );
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

        // Update filter chip highlights
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
            LibraryFilter::Liked => "Saved Songs".to_string(),
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
        // Playback & Seamless Fade-In
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

        // Equalizer
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

        // Volume & Transport
        self.ui
            .ferric_bar_slider(id!(vol_slider))
            .set_value(cx, f64::from(self.session_state.volume));
        self.ui.button(id!(shuffle_btn)).apply_over(
            cx,
            live! { draw_bg: { active: (if self.queue.shuffle { 1.0 } else { 0.0 }) } },
        );
        self.ui.button(id!(repeat_btn)).set_text(
            cx,
            match self.queue.repeat {
                RepeatMode::Off => "Repeat",
                RepeatMode::All => "Repeat All",
                RepeatMode::One => "Repeat 1",
            },
        );
        self.ui.button(id!(repeat_btn)).apply_over(
            cx,
            live! { draw_bg: { active: (if self.queue.repeat != RepeatMode::Off { 1.0 } else { 0.0 }) } },
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
        let rad = self.theme.corner_radius_px;

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

        for panel_id in [
            id!(top_bar),
            id!(left_sidebar),
            id!(center_stage),
            id!(right_drawer),
            id!(bottom_bar),
        ] {
            self.ui.view(panel_id).apply_over(
                cx,
                live! {
                    draw_bg: {
                        base_col: (surface),
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
                    radius_px: (rad.max(16.0)),
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

        // Top-bar account pill & Settings account badges
        let sp_ok = self.accounts.spotify_connected;
        let yt_ok = self.accounts.youtube_connected;
        let top_acc_txt = match (sp_ok, yt_ok) {
            (true, true) => "Spotify · YouTube",
            (true, false) => "Spotify Connected",
            (false, true) => "YouTube Connected",
            (false, false) => "Sign In",
        };
        self.ui
            .button(id!(accounts_top_btn))
            .set_text(cx, top_acc_txt);
        self.ui.button(id!(accounts_top_btn)).apply_over(
            cx,
            live! { draw_bg: { active: (if sp_ok || yt_ok { 1.0 } else { 0.0 }) } },
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

        // Sidebar playlists (up to 5 dynamic slots)
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
                    "Click 'Play Demo' above to try Seamless Fade-In right now",
                );
            } else {
                self.ui.button(btn_id).set_visible(cx, false);
            }
        }

        // Queue Drawer slots
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

    /// Pre-queue Deck B in `ferric-audio` so gapless / crossfade overlap triggers with 0 ms delay.
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
            // Cloud metadata track without a matched local file yet: guide user cleanly
            self.switch_main_page(cx, live_id!(page_settings));
            self.switch_settings_tab(cx, live_id!(subpage_accounts));
            self.ui.label(id!(oauth_status_lbl)).set_text(
                cx,
                &format!(
                    "'{}' is a cloud metadata entry. Add your local music folder and click 'Match Cloud Songs to Local Files' to link audio.",
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
        self.ui
            .button(id!(np_like_btn))
            .set_text(cx, if track.liked { "Saved" } else { "Save" });
        self.ui.button(id!(play_pause_btn)).set_text(cx, "Pause");
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

    fn start_oauth_login(&mut self, cx: &mut Cx, kind: AccountKind) {
        let spotify_id = self
            .ui
            .text_input(id!(spotify_id_input))
            .text()
            .trim()
            .to_string();
        let yt_id = self
            .ui
            .text_input(id!(yt_id_input))
            .text()
            .trim()
            .to_string();
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
                "Please paste your OAuth Client ID first, then click Sign In.",
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

        // 3. Sleep Timer Countdown Check
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

        // 4. Periodic LAN UDP Discovery Beacon (every 3 seconds when Connect drawer is open)
        if self.right_drawer_open {
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
                    if peers.is_empty() {
                        self.ui
                            .label(id!(peers_lbl))
                            .set_text(cx, "Listening for Ferric devices on local Wi-Fi...");
                    } else {
                        let txt = peers
                            .iter()
                            .map(|p| format!("- {} ({})", p.device_name, p.addr))
                            .collect::<Vec<_>>()
                            .join("\n");
                        self.ui.label(id!(peers_lbl)).set_text(cx, &txt);
                    }
                }
            }
        }

        // 5. Sync Window Maximize Glyph
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
                        // Deck B was ALREADY promoted gaplessly/crossfaded to Deck A!
                        // Update UI & pre-queue the next Deck B WITHOUT interrupting audio!
                        self.sync_next_deck_to_engine();
                        self.update_now_playing_metadata(cx, &next);
                    } else {
                        self.play_track(cx, next);
                    }
                } else {
                    self.ui.button(id!(play_pause_btn)).set_text(cx, "Play");
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
                .button(id!(play_pause_btn))
                .set_text(cx, if snap.is_playing { "Pause" } else { "Play" });

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
}

impl AppMain for App {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        self.ensure_initialized(cx);

        // Native Windows 11 Titlebar Drag & Aero Snap on empty regions of our unified `top_bar`
        if let Event::WindowDragQuery(dq) = event
            && dq.abs.y >= 8.0
            && dq.abs.y <= 56.0
        {
            let search_rect = self.ui.view(id!(search_bar_wrap)).area().rect(cx);
            let acc_rect = self.ui.button(id!(accounts_top_btn)).area().rect(cx);
            let in_search = dq.abs.x >= search_rect.pos.x - 6.0
                && dq.abs.x <= search_rect.pos.x + search_rect.size.x + 6.0;
            let in_right_btns = acc_rect.size.x > 0.0 && dq.abs.x >= acc_rect.pos.x - 8.0;
            if !in_search && !in_right_btns {
                dq.response.set(WindowDragQueryResponse::Caption);
                return;
            }
        }

        if self.tick_timer.is_event(event).is_some() {
            self.handle_tick(cx);
        }

        // Global Desktop Keyboard Shortcuts (Sonora / Spotify parity)
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
                self.switch_main_page(cx, live_id!(page_settings));
                self.switch_settings_tab(cx, live_id!(subpage_eq));
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
        let actions = {
            let mut scope = Scope::with_props(&props);
            cx.capture_actions(|cx| {
                self.ui.handle_event(cx, event, &mut scope);
            })
        };

        // 1. WINDOW CONTROLS (Minimize, Maximize/Restore, Exit, Mini-Player)
        if self.ui.ferric_win_btn(id!(win_min_btn)).clicked(&actions) {
            win32_minimize_active_window();
        }
        if self.ui.ferric_win_btn(id!(win_max_btn)).clicked(&actions) {
            let is_max = win32_toggle_maximize_active_window();
            self.ui
                .ferric_win_btn(id!(win_max_btn))
                .set_maximized(cx, is_max);
        }
        if self.ui.ferric_win_btn(id!(win_close_btn)).clicked(&actions) {
            cx.quit();
            return;
        }
        if self.ui.ferric_win_btn(id!(win_mini_btn)).clicked(&actions) {
            self.toggle_compact_mini_mode(cx);
        }

        // 2. PRIMARY NAVIGATION & TOP BAR
        if self.ui.button(id!(nav_home)).clicked(&actions) {
            self.switch_main_page(cx, live_id!(page_home));
        }
        if self.ui.button(id!(nav_search)).clicked(&actions) {
            self.switch_main_page(cx, live_id!(page_library));
            self.ui.text_input(id!(search_input)).set_key_focus(cx);
        }
        if self.ui.button(id!(nav_library)).clicked(&actions) {
            self.switch_main_page(cx, live_id!(page_library));
        }
        if self.ui.button(id!(nav_now_playing)).clicked(&actions)
            || self.ui.button(id!(bar_lyrics_btn)).clicked(&actions)
            || self.ui.button(id!(codec_pill_btn)).clicked(&actions)
            || self.ui.view(id!(np_art_tile)).finger_up(&actions).is_some()
        {
            self.switch_main_page(cx, live_id!(page_now_playing));
        }
        if self.ui.button(id!(accounts_top_btn)).clicked(&actions) {
            self.switch_main_page(cx, live_id!(page_settings));
            self.switch_settings_tab(cx, live_id!(subpage_accounts));
        }
        if self.ui.button(id!(settings_top_btn)).clicked(&actions) {
            self.switch_main_page(cx, live_id!(page_settings));
        }
        if self.ui.button(id!(cmdk_btn)).clicked(&actions) {
            self.cmdk_open = true;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, true);
            self.ui.redraw(cx);
        }
        if self.ui.button(id!(cmdk_close_btn)).clicked(&actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.ui.redraw(cx);
        }

        // Settings Sub-Tabs
        if self.ui.button(id!(tab_accounts_btn)).clicked(&actions) {
            self.switch_settings_tab(cx, live_id!(subpage_accounts));
        }
        if self.ui.button(id!(tab_playback_btn)).clicked(&actions)
            || self.ui.button(id!(home_fade_btn)).clicked(&actions)
        {
            self.switch_main_page(cx, live_id!(page_settings));
            self.switch_settings_tab(cx, live_id!(subpage_playback));
        }
        if self.ui.button(id!(tab_eq_btn)).clicked(&actions)
            || self.ui.button(id!(home_eq_btn)).clicked(&actions)
        {
            self.switch_main_page(cx, live_id!(page_settings));
            self.switch_settings_tab(cx, live_id!(subpage_eq));
        }
        if self.ui.button(id!(tab_appearance_btn)).clicked(&actions) {
            self.switch_settings_tab(cx, live_id!(subpage_appearance));
        }

        // Command Palette Quick Actions
        if self.ui.button(id!(cmd_demo_btn)).clicked(&actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.load_and_play_demo_suite(cx);
        }
        if self.ui.button(id!(cmd_glass_btn)).clicked(&actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.theme.material.liquid_glass = !self.theme.material.liquid_glass;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(cmd_fade_btn)).clicked(&actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.switch_main_page(cx, live_id!(page_settings));
            self.switch_settings_tab(cx, live_id!(subpage_playback));
        }
        if self.ui.button(id!(cmd_eq_btn)).clicked(&actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.switch_main_page(cx, live_id!(page_settings));
            self.switch_settings_tab(cx, live_id!(subpage_eq));
        }
        if self.ui.button(id!(cmd_accounts_btn)).clicked(&actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.switch_main_page(cx, live_id!(page_settings));
            self.switch_settings_tab(cx, live_id!(subpage_accounts));
        }
        if self.ui.button(id!(cmd_sleep30_btn)).clicked(&actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.sleep_timer =
                SleepTimerMode::Deadline(Instant::now() + Duration::from_secs(30 * 60));
        }

        // Right Collapsible Drawer (Queue & Connect)
        if self.ui.button(id!(bar_queue_btn)).clicked(&actions) {
            if self.right_drawer_open {
                self.right_drawer_open = false;
                self.ui.view(id!(right_drawer)).set_visible(cx, false);
                self.ui.redraw(cx);
            } else {
                self.open_right_drawer(cx, false);
            }
        }
        if self.ui.button(id!(bar_connect_btn)).clicked(&actions) {
            self.open_right_drawer(cx, true);
        }
        if self.ui.button(id!(drawer_queue_tab)).clicked(&actions) {
            self.open_right_drawer(cx, false);
        }
        if self.ui.button(id!(drawer_connect_tab)).clicked(&actions) {
            self.open_right_drawer(cx, true);
        }
        if self.ui.button(id!(drawer_close_btn)).clicked(&actions) {
            self.right_drawer_open = false;
            self.ui.view(id!(right_drawer)).set_visible(cx, false);
            self.ui.redraw(cx);
        }
        if self.ui.button(id!(clear_queue_btn)).clicked(&actions) {
            self.queue.set_tracks(Vec::new(), None);
            self.sync_next_deck_to_engine();
            self.refresh_ui_labels(cx);
        }

        // 3. LIBRARY, FILTERS, SORT, PLAYLISTS & TRACK ACTIONS
        if let Some(query) = self.ui.text_input(id!(search_input)).changed(&actions) {
            self.search_query = query;
            self.switch_main_page(cx, live_id!(page_library));
            self.rebuild_filtered_tracks(cx);
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
            if self.ui.button(btn).clicked(&actions) {
                self.active_filter = flt;
                self.switch_main_page(cx, live_id!(page_library));
                self.rebuild_filtered_tracks(cx);
            }
        }
        if self.ui.button(id!(sort_cycle_btn)).clicked(&actions) {
            self.sort_field = self.sort_field.next();
            self.rebuild_filtered_tracks(cx);
        }

        // Add Folder / Import M3U-CSV / Demo Suite
        if (self.ui.button(id!(add_folder_btn)).clicked(&actions)
            || self
                .ui
                .button(id!(sidebar_add_folder_btn))
                .clicked(&actions)
            || self.ui.button(id!(home_add_folder_btn)).clicked(&actions))
            && let Some(folder) = rfd::FileDialog::new()
                .set_title("Select Local Music Folder")
                .pick_folder()
        {
            if let Some(db) = &self.db {
                let _ = db.add_music_folder(&folder);
            }
            self.spawn_folder_scan(vec![folder]);
        }
        if self.ui.button(id!(sidebar_import_btn)).clicked(&actions)
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
        if self.ui.button(id!(demo_suite_btn)).clicked(&actions)
            || self.ui.button(id!(home_demo_btn)).clicked(&actions)
        {
            self.load_and_play_demo_suite(cx);
        }
        if self.ui.button(id!(home_spotify_btn)).clicked(&actions) {
            self.switch_main_page(cx, live_id!(page_settings));
            self.switch_settings_tab(cx, live_id!(subpage_accounts));
            self.ui.text_input(id!(spotify_id_input)).set_key_focus(cx);
        }
        if self.ui.button(id!(home_youtube_btn)).clicked(&actions) {
            self.switch_main_page(cx, live_id!(page_settings));
            self.switch_settings_tab(cx, live_id!(subpage_accounts));
            self.ui.text_input(id!(yt_id_input)).set_key_focus(cx);
        }

        // Home Quick Pick clicks
        for (i, btn_id) in [
            id!(recent_btn_0),
            id!(recent_btn_1),
            id!(recent_btn_2),
            id!(recent_btn_3),
        ]
        .into_iter()
        .enumerate()
        {
            if self.ui.button(btn_id).clicked(&actions) {
                if self.all_tracks.is_empty() {
                    self.load_and_play_demo_suite(cx);
                } else if let Some(track) = self.all_tracks.get(i).cloned() {
                    self.queue.set_tracks(self.all_tracks.clone(), Some(i));
                    self.play_track(cx, track);
                }
            }
        }

        // TrackList Actions
        for action in &actions {
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
                TrackListAction::OpenSpotifyLogin => {
                    self.switch_main_page(cx, live_id!(page_settings));
                    self.switch_settings_tab(cx, live_id!(subpage_accounts));
                }
                TrackListAction::OpenYouTubeLogin => {
                    self.switch_main_page(cx, live_id!(page_settings));
                    self.switch_settings_tab(cx, live_id!(subpage_accounts));
                }
                TrackListAction::LoadDemoSuite => {
                    self.load_and_play_demo_suite(cx);
                }
                TrackListAction::None => {}
            }
        }

        if self.ui.button(id!(play_all_btn)).clicked(&actions) {
            if self.filtered_tracks.is_empty() {
                self.load_and_play_demo_suite(cx);
            } else if let Some(first) = self.filtered_tracks.first().cloned() {
                self.queue
                    .set_tracks((*self.filtered_tracks).clone(), Some(0));
                self.play_track(cx, first);
            }
        }
        if self.ui.button(id!(hero_shuffle_btn)).clicked(&actions)
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

        // 4. BOTTOM TRANSPORT, SEEK, VOLUME & NOW PLAYING CONTROLS
        if self.ui.button(id!(play_pause_btn)).clicked(&actions)
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
        if self.ui.button(id!(repeat_btn)).clicked(&actions) {
            self.queue.repeat = self.queue.repeat.next();
            self.session_state.repeat = self.queue.repeat;
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.sync_next_deck_to_engine();
            self.sync_controls_from_state(cx);
        }
        if self.ui.button(id!(dur_btn)).clicked(&actions) {
            self.show_remaining_time = !self.show_remaining_time;
        }
        if self.ui.button(id!(np_like_btn)).clicked(&actions)
            && let Some(cur) = self.queue.current().cloned()
        {
            let next_liked = !cur.liked;
            if let Some(db) = &self.db {
                let _ = db.set_track_liked(&cur.id, next_liked);
            }
            if let Some(t) = self.all_tracks.iter_mut().find(|x| x.id == cur.id) {
                t.liked = next_liked;
            }
            self.ui
                .button(id!(np_like_btn))
                .set_text(cx, if next_liked { "Saved" } else { "Save" });
            self.rebuild_filtered_tracks(cx);
        }
        if let Some(pos_ratio) = self
            .ui
            .ferric_bar_slider(id!(seek_slider))
            .released(&actions)
            && let Some(audio) = &self.audio
        {
            let dur = audio.telemetry().duration_ms;
            if dur > 0 {
                audio.send(EngineCommand::SeekMs((pos_ratio * dur as f64) as u64));
            }
        }
        if let Some(vol) = self.ui.ferric_bar_slider(id!(vol_slider)).changed(&actions) {
            self.session_state.volume = vol as f32;
            if let Some(audio) = &self.audio {
                audio.send(EngineCommand::SetVolume(vol as f32));
            }
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
        }

        // Now Playing Modes & Lyrics Sync Offset
        if self.ui.button(id!(np_mode_canvas_btn)).clicked(&actions) {
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
        if self.ui.button(id!(np_mode_card_btn)).clicked(&actions) {
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
        if self.ui.button(id!(np_watch_video_btn)).clicked(&actions)
            && let Some(cur) = self.queue.current()
        {
            let _ = open_track_video_clip(
                cur.path.as_deref(),
                cur.youtube_video_id.as_deref(),
                &cur.artist,
                &cur.title,
            );
        }
        if self.ui.button(id!(lyr_minus_btn)).clicked(&actions) {
            self.lyrics_offset_ms -= 500;
        }
        if self.ui.button(id!(lyr_plus_btn)).clicked(&actions) {
            self.lyrics_offset_ms += 500;
        }
        if self.ui.button(id!(lyr_copy_btn)).clicked(&actions)
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

        // 5. ACCOUNTS & OAUTH PKCE LOGIN ACTIONS
        if self.ui.button(id!(spotify_login_btn)).clicked(&actions) {
            self.start_oauth_login(cx, AccountKind::Spotify);
        }
        if self.ui.button(id!(spotify_sync_btn)).clicked(&actions) {
            self.sync_existing_cloud_account(cx, AccountKind::Spotify);
        }
        if self.ui.button(id!(spotify_logout_btn)).clicked(&actions) {
            self.accounts.spotify_connected = false;
            self.accounts.spotify_access_token.clear();
            self.accounts.spotify_refresh_token.clear();
            if let Some(db) = &self.db {
                let _ = db.save_accounts(&self.accounts);
            }
            self.refresh_ui_labels(cx);
        }
        if self.ui.button(id!(yt_login_btn)).clicked(&actions) {
            self.start_oauth_login(cx, AccountKind::YouTubeMusic);
        }
        if self.ui.button(id!(yt_sync_btn)).clicked(&actions) {
            self.sync_existing_cloud_account(cx, AccountKind::YouTubeMusic);
        }
        if self.ui.button(id!(yt_logout_btn)).clicked(&actions) {
            self.accounts.youtube_connected = false;
            self.accounts.youtube_access_token.clear();
            self.accounts.youtube_refresh_token.clear();
            if let Some(db) = &self.db {
                let _ = db.save_accounts(&self.accounts);
            }
            self.refresh_ui_labels(cx);
        }
        if self.ui.button(id!(oauth_cancel_btn)).clicked(&actions) {
            if let Some(c) = self.oauth_cancel.take() {
                c.store(true, Ordering::Relaxed);
            }
            self.ui.button(id!(oauth_cancel_btn)).set_visible(cx, false);
            self.ui
                .label(id!(oauth_status_lbl))
                .set_text(cx, "Browser login cancelled.");
        }
        if self.ui.button(id!(copy_redirect_btn)).clicked(&actions) {
            let _ = copy_text_to_clipboard("http://127.0.0.1:43824/callback");
            self.ui
                .label(id!(oauth_status_lbl))
                .set_text(cx, "Copied http://127.0.0.1:43824/callback to clipboard.");
        }
        if self.ui.button(id!(open_spotify_dev_btn)).clicked(&actions) {
            let _ = open_allowed_url("https://developer.spotify.com/dashboard");
        }
        if self.ui.button(id!(open_google_dev_btn)).clicked(&actions) {
            let _ = open_allowed_url("https://console.cloud.google.com/apis/credentials");
        }
        if let Some(cat_only) = self.ui.ferric_toggle(id!(yt_cat_toggle)).toggled(&actions) {
            self.accounts.youtube_music_category_only = cat_only;
            if let Some(db) = &self.db {
                let _ = db.save_accounts(&self.accounts);
            }
        }
        if self.ui.button(id!(sub_sync_btn)).clicked(&actions) {
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
        if self.ui.button(id!(match_local_btn)).clicked(&actions)
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
        if self.ui.button(id!(rescan_btn)).clicked(&actions)
            && let Some(db) = &self.db
            && let Ok(folders) = db.list_music_folders()
            && !folders.is_empty()
        {
            self.spawn_folder_scan(folders);
        }

        // 6. PLAYBACK & SEAMLESS FADE-IN SETTINGS
        if let Some(sec) = self
            .ui
            .ferric_bar_slider(id!(xfade_slider))
            .changed(&actions)
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
            if self.ui.button(btn).clicked(&actions) {
                self.dsp.crossfade_ms = ms;
                self.sync_controls_from_state(cx);
                self.apply_dsp_live(cx);
            }
        }
        if let Some(ms) = self
            .ui
            .ferric_bar_slider(id!(fadein_slider))
            .changed(&actions)
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
            if self.ui.button(btn).clicked(&actions) {
                self.dsp.fade_curve = curve;
                self.sync_controls_from_state(cx);
                self.apply_dsp_live(cx);
            }
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(gapless_lock_toggle))
            .toggled(&actions)
        {
            self.dsp.gapless_album_lock = on;
            self.apply_dsp_live(cx);
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(trim_silence_toggle))
            .toggled(&actions)
        {
            self.dsp.trim_silence = on;
            self.apply_dsp_live(cx);
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(replay_gain_toggle))
            .toggled(&actions)
        {
            self.dsp.replay_gain = on;
            self.apply_dsp_live(cx);
        }
        if let Some(on) = self.ui.ferric_toggle(id!(mono_toggle)).toggled(&actions) {
            self.dsp.mono_audio = on;
            self.apply_dsp_live(cx);
        }
        for (btn, spd) in [
            (id!(spd_075_btn), 0.75_f32),
            (id!(spd_100_btn), 1.0_f32),
            (id!(spd_125_btn), 1.25_f32),
            (id!(spd_150_btn), 1.5_f32),
        ] {
            if self.ui.button(btn).clicked(&actions) {
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
            if self.ui.button(btn).clicked(&actions) {
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
        if self.ui.button(id!(slp_eot_btn)).clicked(&actions) {
            self.sleep_timer = SleepTimerMode::EndOfTrack;
            self.ui
                .label(id!(sleep_status_lbl))
                .set_text(cx, "Sleep Timer: Will stop after current song");
        }

        // 7. EQUALIZER & AUTOEQ CONTROLS
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(eq_enable_toggle))
            .toggled(&actions)
        {
            self.eq.enabled = on;
            self.apply_eq_live(cx);
        }
        if let Some(pre) = self
            .ui
            .ferric_bar_slider(id!(eq_preamp_slider))
            .changed(&actions)
        {
            self.eq.preamp_db = pre as f32;
            self.ui
                .label(id!(eq_preamp_lbl))
                .set_text(cx, &format!("{pre:+.1} dB"));
            self.apply_eq_live(cx);
        }
        for (idx, (slider_id, val_id)) in eq_band_ids().into_iter().enumerate() {
            if let Some(val) = self.ui.ferric_vert_slider(slider_id).changed(&actions)
                && let Some(band) = self.eq.bands.get_mut(idx)
            {
                band.gain_db = val as f32;
                self.eq.preset_name = "Custom".to_string();
                self.ui.label(val_id).set_text(cx, &format!("{val:+.1}"));
                self.apply_eq_live(cx);
            }
        }
        if self.ui.button(id!(autoeq_apply_btn)).clicked(&actions) {
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
            if self.ui.button(btn).clicked(&actions) {
                self.eq = EqConfig::preset(preset);
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

        // 8. CONNECT QR & APPEARANCE / LIQUID GLASS / UPDATES
        if self.ui.button(id!(gen_pair_btn)).clicked(&actions) {
            self.regenerate_pairing_qr(cx);
        }
        if self.ui.button(id!(copy_pair_btn)).clicked(&actions) {
            let _ = copy_text_to_clipboard(&self.pairing_uri);
        }
        if let Some(enabled) = self
            .ui
            .ferric_toggle(id!(liquid_glass_toggle))
            .toggled(&actions)
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
            .changed(&actions)
        {
            self.theme.material.ior = v as f32;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if let Some(v) = self
            .ui
            .ferric_bar_slider(id!(lg_disp_slider))
            .changed(&actions)
        {
            self.theme.material.dispersion = v as f32;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if let Some(v) = self
            .ui
            .ferric_bar_slider(id!(lg_rim_slider))
            .changed(&actions)
        {
            self.theme.material.specular_rim = v as f32;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if let Some(v) = self
            .ui
            .ferric_bar_slider(id!(lg_rad_slider))
            .changed(&actions)
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
            if self.ui.button(btn).clicked(&actions) {
                self.theme.material.apply_preset(preset);
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
            }
        }
        if self.ui.button(id!(pal_amoled_btn)).clicked(&actions) {
            self.theme = ThemeConfig::default();
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

        // Software Updates
        if self.ui.button(id!(update_badge_btn)).clicked(&actions)
            && let Some(info) = self.available_update.clone()
        {
            self.show_update_popup(cx, &info);
        }
        if self.ui.button(id!(check_updates_btn)).clicked(&actions) {
            self.spawn_update_check(cx, true);
        }
        if self.ui.button(id!(open_releases_btn)).clicked(&actions)
            || self.ui.button(id!(update_whats_new_btn)).clicked(&actions)
        {
            let tag = self.available_update.as_ref().map(|u| u.tag.as_str());
            let _ = open_github_url(&release_page_url(tag));
        }
        if let Some(auto_chk) = self
            .ui
            .ferric_toggle(id!(auto_update_toggle))
            .toggled(&actions)
        {
            self.update_settings.auto_check_on_startup = auto_chk;
            if let Some(db) = &self.db {
                let _ = db.save_update_settings(&self.update_settings);
            }
        }
        if self.ui.button(id!(update_now_btn)).clicked(&actions) {
            self.start_update_download(cx);
        }
        if self
            .ui
            .button(id!(update_popup_close_btn))
            .clicked(&actions)
        {
            self.dismiss_update_popup(cx);
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
