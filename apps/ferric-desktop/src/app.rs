use crate::artwork::{ArtworkCache, CachedArtwork};
use crate::google_login::{GoogleLoginOutcome, launch_google_ytm_login_window};
use crate::shell::{
    copy_text_to_clipboard, launch_verified_installer, open_github_url, open_track_video_clip,
    win32_close_active_window, win32_is_active_window_maximized, win32_minimize_active_window,
    win32_toggle_maximize_active_window,
};
use crate::smtc::{SmtcBridge, SmtcCommand};
use crate::state::{
    BgMessage, CollectionGroup, DetailViewState, LibraryFilter, MainPage, NavHistory, SearchFilter,
    SettingsTab, SleepTimerMode, SortField, build_album_groups, build_artist_groups, clip_text,
    dir_size_bytes, format_bytes_mb,
};
use crate::track_list::{TrackListAction, TrackListScope};
use crate::widgets::*;
use crate::{startup_load_demo, startup_page_override, startup_play_first};
use ferric_audio::demo_synth::ensure_demo_tracks;
use ferric_audio::engine::{AudioEngine, EngineCommand};
use ferric_core::app_data_dir;
use ferric_core::db::LibraryDb;
use ferric_core::eq::EqConfig;
use ferric_core::models::{
    AccountSettings, FadeCurve, PlaybackDspSettings, Playlist, RepeatMode, SessionResumeState,
    Track, TrackSource, UpdateSettings, format_duration_ms, parse_playlist_import_text,
};
use ferric_core::queue::PlayQueue;
use ferric_core::theme::{LiquidGlassPreset, ThemeConfig, hex_to_rgba_f32};
use ferric_providers::autoeq::search_autoeq_profiles;
use ferric_providers::local_scanner::scan_local_folder;
use ferric_providers::lyrics::{SyncedLyrics, fetch_best_free_lyrics};
use ferric_providers::public_sync::{
    import_spotify_public_url, import_youtube_public_url, parse_spotify_target,
    parse_youtube_target,
};
use ferric_providers::subsonic::{SubsonicCredentials, sync_subsonic_catalog};
use ferric_providers::updater::{
    DownloadEvent, ReleaseInfo, check_latest_release, download_verified_installer,
    format_check_status, release_page_url,
};
use ferric_providers::ytmusic::{
    YtmCardKind, YtmRichHome, YtmRichSearch, YtmSession, YtmShelfCard, fetch_account_library,
    fetch_browse_detail, fetch_radio_up_next, fetch_rich_home, fetch_rich_search,
    normalize_ytm_thumbnail_url, resolve_and_cache_audio_stream, resolve_best_video_id_for_track,
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

    // Sonora Obsidian surface that seamlessly upgrades into Snell Refractive Liquid Glass when toggled ON
    FerricSurface = <View> {
        show_bg: true,
        draw_bg: {
            uniform base_col: #0E0F13
            uniform border_col: #232530
            uniform accent_col: #A882FF
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
                let warm_glow = self.accent_col.xyz * (0.14 * diag + 0.09 * slope);
                let cool_glow = vec3(0.14, 0.26, 0.58) * (0.09 * (1.0 - diag) + 0.07 * slope);
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

                let base_rgb = mix(self.base_col.xyz * 0.72 + vec3(0.03, 0.03, 0.05), vec3(0.02, 0.02, 0.03), self.glass_scrim);
                let rgb = base_rgb + warm_glow + cool_glow + prism + vec3(0.96, 0.96, 0.98) * (fresnel + spec * 0.62);
                sdf.fill(vec4(clamp(rgb, 0.0, 1.0), 1.0));
                return sdf.result;
            }
        }
    }

    // Sonora-style ambient player bar radiating a soft adaptive color wash from active album artwork
    AmbientBottomBar = <View> {
        width: Fill,
        height: 68,
        padding: { left: 14, right: 14 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 14,
        show_bg: true,
        draw_bg: {
            uniform base_col: #13141B
            uniform accent_col: #A882FF
            uniform glow_amt: 0.24
            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                if p.y < 1.0 {
                    return vec4(0.15, 0.16, 0.21, 1.0);
                }
                let d_left = length((self.pos - vec2(0.08, 0.55)) * vec2(1.4, 2.2));
                let d_mid = length((self.pos - vec2(0.46, 0.85)) * vec2(0.9, 2.4));
                let wash = (exp(-d_left * d_left * 2.4) * 0.65 + exp(-d_mid * d_mid * 1.8) * 0.45) * self.glow_amt;
                let rgb = self.base_col.xyz + self.accent_col.xyz * wash;
                return vec4(clamp(rgb, 0.0, 1.0), 1.0);
            }
        }
    }

    FerricDividerH = <SolidView> {
        width: Fill,
        height: 1,
        draw_bg: { color: #222430 }
    }

    FerricDividerV = <SolidView> {
        width: 1,
        height: Fill,
        draw_bg: { color: #222430 }
    }

    SonoraCard = <View> {
        width: Fill,
        height: Fit,
        padding: 18,
        show_bg: true,
        draw_bg: {
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 6.0);
                sdf.fill_keep(#15161C);
                sdf.stroke(#232530, 1.0);
                return sdf.result;
            }
        }
    }

    SonoraNavItem = <Button> {
        width: Fill,
        height: 34,
        padding: { left: 11, right: 10 }
        spacing: 11,
        align: { x: 0.0, y: 0.5 }
        icon_walk: { width: 15, height: Fit }
        draw_icon: {
            color: #888B99,
            color_hover: #F4F4F7,
            color_down: #F4F4F7
        }
        draw_text: {
            color: #A2A5B4,
            color_hover: #F4F4F7,
            color_down: #F4F4F7,
            text_style: { font_size: 13.0 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 4.5);
                let col = mix(mix(vec4(0.0, 0.0, 0.0, 0.0), #1E202B, self.hover), #242636, self.active);
                sdf.fill(col);
                return sdf.result;
            }
        }
    }

    SonoraPinBtn = <Button> {
        width: Fill,
        height: 30,
        padding: { left: 11, right: 8 }
        spacing: 8,
        align: { x: 0.0, y: 0.5 }
        draw_text: {
            color: #9A9CA8,
            color_hover: #F4F4F7,
            text_style: { font_size: 12.0 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 4.0);
                let col = mix(mix(vec4(0.0, 0.0, 0.0, 0.0), #1E202B, self.hover), #242636, self.active);
                sdf.fill(col);
                return sdf.result;
            }
        }
    }

    SonoraPrimaryBtn = <Button> {
        width: Fit,
        height: 34,
        padding: { left: 15, right: 17 }
        spacing: 7,
        align: { x: 0.5, y: 0.5 }
        icon_walk: { width: 13, height: Fit }
        draw_icon: {
            color: #0E0F13,
            color_hover: #0E0F13,
            color_down: #0E0F13
        }
        draw_text: {
            color: #0E0F13,
            color_hover: #0E0F13,
            color_down: #0E0F13,
            text_style: { font_size: 12.5 }
        }
        draw_bg: {
            instance hover: 0.0
            instance down: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let r = (self.rect_size.y - 1.0) * 0.25;
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);
                let c = mix(mix(#F4F4F7, #FFFFFF, self.hover), #DCDCE6, self.down);
                sdf.fill(c);
                return sdf.result;
            }
        }
    }

    SonoraBtn = <Button> {
        width: Fit,
        height: 32,
        padding: { left: 12, right: 13 }
        spacing: 7,
        align: { x: 0.5, y: 0.5 }
        icon_walk: { width: 13, height: Fit }
        draw_icon: {
            color: #9A9CA8,
            color_hover: #F4F4F7,
            color_down: #A882FF
        }
        draw_text: {
            color: #DCDCE6,
            color_hover: #FFFFFF,
            color_down: #FFFFFF,
            text_style: { font_size: 12.0 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 4.5);
                let bg = mix(mix(#1B1C24, #252734, self.hover), #292540, self.active);
                sdf.fill_keep(bg);
                let border = mix(#2B2D3A, #A882FF, self.active * 0.85);
                sdf.stroke(border, 1.0);
                return sdf.result;
            }
        }
    }

    SonoraChipBtn = <Button> {
        width: Fit,
        height: 30,
        padding: { left: 12, right: 12 }
        spacing: 0,
        align: { x: 0.5, y: 0.5 }
        draw_text: {
            color: #C8CAD6,
            color_hover: #FFFFFF,
            color_down: #0E0F13,
            text_style: { font_size: 12.0 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 5.0);
                let bg = mix(mix(#1B1C24, #262834, self.hover), #F4F4F7, self.active);
                sdf.fill_keep(bg);
                sdf.stroke(mix(#292B38, #F4F4F7, self.active), 1.0);
                return sdf.result;
            }
        }
    }

    SonoraIconBtn = <Button> {
        width: 30,
        height: 30,
        padding: 0,
        spacing: 0,
        align: { x: 0.5, y: 0.5 }
        text: ""
        label_walk: { width: 0, height: 0 }
        icon_walk: { width: 15, height: Fit }
        draw_icon: {
            color: #9A9CA8,
            color_hover: #F4F4F7,
            color_down: #A882FF
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 4.5);
                let bg = mix(mix(vec4(0.0, 0.0, 0.0, 0.0), #222430, self.hover), #292540, self.active);
                sdf.fill(bg);
                return sdf.result;
            }
        }
    }

    SonoraInput = <TextInput> {
        width: Fill,
        height: 36,
        draw_bg: {
            color: #111218,
            color_empty: #111218,
            color_hover: #151720,
            color_focus: #161822,
            border_radius: 5.0,
            border_size: 1.0,
            border_color_1: #282A38,
            border_color_2: #282A38,
            border_color_1_empty: #282A38,
            border_color_2_empty: #282A38,
            border_color_1_hover: #3B3E52,
            border_color_2_hover: #3B3E52,
            border_color_1_focus: #A882FF,
            border_color_2_focus: #A882FF
        }
        draw_text: {
            color: #F4F4F7,
            color_empty: #6D7080,
            text_style: { font_size: 12.5 }
        }
        draw_cursor: { color: #A882FF }
    }

    // Sonora 2x4 Home Quick Picks Song Tile (Real 46x46 Cover Art + Title + Artist)
    QuickPickTile = <View> {
        width: Fill,
        height: 58,
        padding: { left: 6, right: 10, top: 5, bottom: 5 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 11,
        show_bg: true,
        cursor: Hand,
        draw_bg: {
            instance active: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 5.0);
                let col = mix(#13141B, #221E36, self.active);
                sdf.fill_keep(col);
                sdf.stroke(mix(#1E202A, #5B4B8A, self.active), 1.0);
                return sdf.result;
            }
        }
        art = <FerricCoverArt> {
            width: 46,
            height: 46,
            draw_bg: { radius: 8.0 }
        }
        <View> {
            width: Fill,
            height: Fit,
            flow: Down,
            spacing: 2,
            title = <Label> {
                width: Fill,
                draw_text: { color: #F4F4F7, text_style: { font_size: 12.5 } }
                text: ""
            }
            sub = <Label> {
                width: Fill,
                draw_text: { color: #8E909E, text_style: { font_size: 11.0 } }
                text: ""
            }
        }
    }

    // Sonora Horizontal Album / Playlist Shelf Card (Real Square Cover Art!)
    ShelfCard = <View> {
        width: Fill,
        height: 184,
        padding: 8,
        flow: Down,
        spacing: 7,
        show_bg: true,
        cursor: Hand,
        draw_bg: {
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 6.0);
                sdf.fill_keep(#14151C);
                sdf.stroke(#1F212D, 1.0);
                return sdf.result;
            }
        }
        art = <FerricCoverArt> {
            width: Fill,
            height: 122,
            draw_bg: { radius: 8.0 }
        }
        title = <Label> {
            width: Fill,
            draw_text: { color: #F4F4F7, text_style: { font_size: 12.0 } }
            text: ""
        }
        sub = <Label> {
            width: Fill,
            draw_text: { color: #8A8D9B, text_style: { font_size: 10.5 } }
            text: ""
        }
    }

    // Visual Grid Card for Albums / Artists / Playlists pages
    GridCard = <View> {
        width: Fill,
        height: 214,
        padding: 10,
        flow: Down,
        spacing: 8,
        show_bg: true,
        cursor: Hand,
        draw_bg: {
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 6.0);
                sdf.fill_keep(#14151C);
                sdf.stroke(#212330, 1.0);
                return sdf.result;
            }
        }
        art = <FerricCoverArt> {
            width: Fill,
            height: 146,
            draw_bg: { radius: 10.0 }
        }
        title = <Label> {
            width: Fill,
            draw_text: { color: #F4F4F7, text_style: { font_size: 12.5 } }
            text: ""
        }
        sub = <Label> {
            width: Fill,
            draw_text: { color: #8A8D9B, text_style: { font_size: 11.0 } }
            text: ""
        }
    }

    // Compact Right Sidebar Queue / History Row with 34x34 real cover art
    QueueMiniRow = <View> {
        width: Fill,
        height: 52,
        padding: { left: 7, right: 8, top: 6, bottom: 6 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 9,
        show_bg: true,
        cursor: Hand,
        draw_bg: {
            instance active: 0.0
            uniform accent_col: #A882FF
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 5.0);
                let act = mix(#1F1E2E, vec4(self.accent_col.xyz, 0.22), 0.32);
                sdf.fill_keep(mix(vec4(0.0, 0.0, 0.0, 0.0), act, self.active));
                sdf.stroke(mix(vec4(0.0, 0.0, 0.0, 0.0), vec4(self.accent_col.xyz, 0.45), self.active), 1.0);
                return sdf.result;
            }
        }
        art = <FerricCoverArt> {
            width: 36,
            height: 36,
            draw_bg: { radius: 6.0 }
        }
        <View> {
            width: Fill,
            height: Fit,
            flow: Down,
            spacing: 2,
            title = <Label> {
                width: Fill,
                draw_text: { color: #ECEEF5, text_style: { font_size: 11.5 } }
                text: ""
            }
            sub = <Label> {
                width: Fill,
                draw_text: { color: #848796, text_style: { font_size: 10.5 } }
                text: ""
            }
        }
        dur = <Label> {
            width: Fit,
            draw_text: { color: #6E7180, text_style: { font_size: 10.0 } }
            text: ""
        }
    }

    App = {{App}} {
        ui: <Root> {
            main_window = <Window> {
                window: { title: "Ferric", inner_size: vec2(1280, 780) }
                pass: { clear_color: #0E0F13 }
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
                        uniform canvas_col: #0E0F13
                        uniform glass_bg: 0.0
                        uniform accent_col: #A882FF
                        fn pixel(self) -> vec4 {
                            if (self.glass_bg < 0.5) {
                                return self.canvas_col;
                            }
                            let d1 = length(self.pos - vec2(0.16, 0.18));
                            let d2 = length(self.pos - vec2(0.84, 0.24));
                            let d3 = length(self.pos - vec2(0.55, 0.92));
                            let g1 = exp(-d1 * d1 * 6.0) * self.accent_col.xyz * 0.28;
                            let g2 = exp(-d2 * d2 * 6.5) * vec3(0.16, 0.32, 0.72) * 0.20;
                            let g3 = exp(-d3 * d3 * 7.0) * vec3(0.62, 0.18, 0.44) * 0.18;
                            return vec4(self.canvas_col.xyz + g1 + g2 + g3, 1.0);
                        }
                    }

                    // ================================================================
                    // MAIN SONORA 3-PANE DESKTOP SHELL
                    // ================================================================
                    shell_col = <View> {
                        width: Fill,
                        height: Fill,
                        flow: Down,

                        // 1. MINIMAL SONORA TOP CHROME (44px)
                        top_bar = <FerricSurface> {
                            width: Fill,
                            height: 44,
                            padding: { left: 10, right: 8 }
                            flow: Right,
                            align: { y: 0.5 }
                            spacing: 6,
                            draw_bg: { base_col: #14151C, radius: 0.0 }

                            top_left_grp = <View> {
                                width: Fit,
                                height: Fill,
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 4,

                                toggle_left_btn = <SonoraIconBtn> {
                                    width: 30, height: 28,
                                    draw_icon: { svg_path: "M3 5H21V19H3V5ZM5 7V17H9V7H5ZM11 7V17H19V7H11Z" }
                                }
                                nav_back_btn = <SonoraIconBtn> {
                                    width: 28, height: 28,
                                    draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" }
                                }
                                nav_fwd_btn = <SonoraIconBtn> {
                                    width: 28, height: 28,
                                    draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" }
                                }
                                refresh_btn = <SonoraIconBtn> {
                                    width: 28, height: 28,
                                    draw_icon: { svg_path: "M17.65 6.35C16.2 4.9 14.21 4 12 4C7.58 4 4.01 7.58 4.01 12C4.01 16.42 7.58 20 12 20C15.73 20 18.84 17.45 19.73 14H17.65C16.83 16.33 14.61 18 12 18C8.69 18 6 15.31 6 12C6 8.69 8.69 6 12 6C13.66 6 15.14 6.69 16.22 7.78L13 11H20V4L17.65 6.35Z" }
                                }
                                <View> { width: 6, height: 1 }
                                brand_icon = <Image> {
                                    width: 18,
                                    height: 18,
                                    fit: Smallest
                                }
                                <Label> {
                                    margin: { left: 4 }
                                    draw_text: { color: #E4E6F0, text_style: { font_size: 13.0 } }
                                    text: "Ferric"
                                }
                            }

                            // Center drag area + subtle quick command pill
                            <View> {
                                width: Fill,
                                height: Fill,
                                align: { x: 0.5, y: 0.5 }
                                top_search_pill = <Button> {
                                    width: 260,
                                    height: 28,
                                    padding: { left: 12, right: 12 }
                                    spacing: 8,
                                    align: { x: 0.0, y: 0.5 }
                                    text: "Search music... (Ctrl+K)"
                                    icon_walk: { width: 13, height: Fit }
                                    draw_icon: {
                                        svg_path: "M15.5 14H14.71L14.43 13.73C15.41 12.59 16 11.11 16 9.5C16 5.91 13.09 3 9.5 3C5.91 3 3 5.91 3 9.5C3 13.09 5.91 16 9.5 16C11.11 16 12.59 15.41 13.73 14.43L14 14.71V15.5L19 20.49L20.49 19L15.5 14ZM9.5 14C7.01 14 5 11.99 5 9.5C5 7.01 7.01 5 9.5 5C11.99 5 14 7.01 14 9.5C14 11.99 11.99 14 9.5 14Z"
                                        color: #7D8090
                                        color_hover: #F4F4F7
                                    }
                                    draw_text: {
                                        color: #7D8090,
                                        color_hover: #C8CAD6,
                                        text_style: { font_size: 11.5 }
                                    }
                                    draw_bg: {
                                        instance hover: 0.0
                                        fn pixel(self) -> vec4 {
                                            let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                            sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 5.0);
                                            sdf.fill_keep(mix(#0E0F14, #181A24, self.hover));
                                            sdf.stroke(#252734, 1.0);
                                            return sdf.result;
                                        }
                                    }
                                }
                            }

                            top_right_grp = <View> {
                                width: Fit,
                                height: Fill,
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 4,

                                update_badge_btn = <SonoraPrimaryBtn> {
                                    visible: false,
                                    height: 26,
                                    padding: { left: 10, right: 10 }
                                    text: "Update"
                                    draw_text: { text_style: { font_size: 11.0 } }
                                }
                                top_account_btn = <SonoraBtn> {
                                    height: 28,
                                    padding: { left: 9, right: 11 }
                                    text: "Sign in"
                                    draw_icon: {
                                        svg_path: "M12 12C14.21 12 16 10.21 16 8C16 5.79 14.21 4 12 4C9.79 4 8 5.79 8 8C8 10.21 9.79 12 12 12ZM12 14C9.33 14 4 15.34 4 18V20H20V18C20 15.34 14.67 14 12 14Z"
                                        color: #A882FF
                                    }
                                    draw_text: { text_style: { font_size: 11.5 } }
                                }
                                top_settings_btn = <SonoraIconBtn> {
                                    width: 28, height: 28,
                                    draw_icon: { svg_path: "M19.14 12.94C19.18 12.64 19.2 12.33 19.2 12C19.2 11.68 19.18 11.36 19.13 11.06L21.16 9.48C21.34 9.34 21.39 9.07 21.28 8.87L19.36 5.55C19.24 5.33 18.99 5.26 18.77 5.33L16.38 6.29C15.88 5.91 15.35 5.59 14.76 5.35L14.4 2.81C14.36 2.57 14.16 2.4 13.92 2.4H10.08C9.84 2.4 9.65 2.57 9.61 2.81L9.25 5.35C8.66 5.59 8.12 5.92 7.63 6.29L5.24 5.33C5.02 5.25 4.77 5.33 4.65 5.55L2.74 8.87C2.62 9.08 2.66 9.34 2.86 9.48L4.89 11.06C4.84 11.36 4.8 11.69 4.8 12C4.8 12.31 4.82 12.64 4.87 12.94L2.84 14.52C2.66 14.66 2.61 14.93 2.72 15.13L4.64 18.45C4.76 18.67 5.01 18.74 5.23 18.67L7.62 17.71C8.12 18.09 8.65 18.41 9.24 18.65L9.6 21.19C9.65 21.43 9.84 21.6 10.08 21.6H13.92C14.16 21.6 14.36 21.43 14.39 21.19L14.75 18.65C15.34 18.41 15.88 18.09 16.37 17.71L18.76 18.67C18.98 18.75 19.23 18.67 19.35 18.45L21.27 15.13C21.39 14.91 21.34 14.66 21.15 14.52L19.14 12.94ZM12 15.6C10.02 15.6 8.4 13.98 8.4 12C8.4 10.02 10.02 8.4 12 8.4C13.98 8.4 15.6 10.02 15.6 12C15.6 13.98 13.98 15.6 12 15.6Z" }
                                }
                                toggle_right_btn = <SonoraIconBtn> {
                                    width: 28, height: 28,
                                    draw_bg: { active: 1.0 }
                                    draw_icon: {
                                        svg_path: "M3 5H21V19H3V5ZM5 7V17H13V7H5ZM15 7V17H19V7H15Z"
                                        color: #F4F4F7
                                    }
                                }
                                <SolidView> { width: 1, height: 16, margin: { left: 4, right: 4 }, draw_bg: { color: #292B38 } }
                                win_mini_btn = <FerricWinBtn> { width: 30, height: 28, draw_bg: { kind: 3.0 } }
                                win_min_btn = <FerricWinBtn> { width: 34, height: 28, draw_bg: { kind: 0.0 } }
                                win_max_btn = <FerricWinBtn> { width: 34, height: 28, draw_bg: { kind: 1.0 } }
                                win_close_btn = <FerricWinBtn> { width: 36, height: 28, draw_bg: { kind: 2.0 } }
                            }
                        }

                        <FerricDividerH> {}

                        // 2. THREE-PANE WORKSPACE (Collapsible Left Nav + Center Stage + Docked Right Queue/Lyrics)
                        workspace_row = <View> {
                            width: Fill,
                            height: Fill,
                            flow: Right,

                            // LEFT SIDEBAR (196px, Pure Music Nav — Exact Sonora Structure)
                            left_sidebar = <FerricSurface> {
                                width: 196,
                                height: Fill,
                                padding: { left: 10, right: 10, top: 12, bottom: 10 }
                                flow: Down,
                                spacing: 2,
                                draw_bg: { base_col: #14151C, radius: 0.0 }

                                nav_home = <SonoraNavItem> {
                                    text: "Home"
                                    draw_bg: { active: 1.0 }
                                    draw_icon: {
                                        svg_path: "M10 20V14H14V20H19V12H22L12 3L2 12H5V20H10Z"
                                        color: #F4F4F7
                                    }
                                    draw_text: { color: #F4F4F7 }
                                }
                                nav_search = <SonoraNavItem> {
                                    text: "Search"
                                    draw_icon: { svg_path: "M15.5 14H14.71L14.43 13.73C15.41 12.59 16 11.11 16 9.5C16 5.91 13.09 3 9.5 3C5.91 3 3 5.91 3 9.5C3 13.09 5.91 16 9.5 16C11.11 16 12.59 15.41 13.73 14.43L14 14.71V15.5L19 20.49L20.49 19L15.5 14ZM9.5 14C7.01 14 5 11.99 5 9.5C5 7.01 7.01 5 9.5 5C11.99 5 14 7.01 14 9.5C14 11.99 11.99 14 9.5 14Z" }
                                }
                                nav_recents = <SonoraNavItem> {
                                    text: "Recents"
                                    draw_icon: { svg_path: "M13 3C8.03 3 4 7.03 4 12H1L4.89 15.89L4.96 16.03L9 12H6C6 8.13 9.13 5 13 5C16.87 5 20 8.13 20 12C20 15.87 16.87 19 13 19C11.07 19 9.32 18.21 8.06 16.94L6.64 18.36C8.27 19.99 10.51 21 13 21C17.97 21 22 16.97 22 12C22 7.03 17.97 3 13 3ZM12 8V13L16.28 15.54L17 14.33L13.5 12.25V8H12Z" }
                                }

                                <Label> {
                                    margin: { left: 11, top: 14, bottom: 4 }
                                    draw_text: { color: #6E7180, text_style: { font_size: 10.5 } }
                                    text: "Library"
                                }
                                nav_songs = <SonoraNavItem> {
                                    text: "Songs"
                                    draw_icon: { svg_path: "M12 3V13.55C11.41 13.21 10.73 13 10 13C7.79 13 6 14.79 6 17C6 19.21 7.79 21 10 21C12.21 21 14 19.21 14 17V7H18V3H12Z" }
                                }
                                nav_albums = <SonoraNavItem> {
                                    text: "Albums"
                                    draw_icon: { svg_path: "M12 2C6.48 2 2 6.48 2 12C2 17.52 6.48 22 12 22C17.52 22 22 17.52 22 12C22 6.48 17.52 2 12 2ZM12 14.5C10.62 14.5 9.5 13.38 9.5 12C9.5 10.62 10.62 9.5 12 9.5C13.38 9.5 14.5 10.62 14.5 12C14.5 13.38 13.38 14.5 12 14.5Z" }
                                }
                                nav_artists = <SonoraNavItem> {
                                    text: "Artists"
                                    draw_icon: { svg_path: "M12 12C14.21 12 16 10.21 16 8C16 5.79 14.21 4 12 4C9.79 4 8 5.79 8 8C8 10.21 9.79 12 12 12ZM12 14C9.33 14 4 15.34 4 18V20H20V18C20 15.34 14.67 14 12 14Z" }
                                }
                                nav_playlists = <SonoraNavItem> {
                                    text: "Playlists"
                                    draw_icon: { svg_path: "M4 6H18V8H4V6ZM4 11H18V13H4V11ZM4 16H14V18H4V16ZM16 16V22L21 19L16 16Z" }
                                }

                                <Label> {
                                    margin: { left: 11, top: 14, bottom: 4 }
                                    draw_text: { color: #6E7180, text_style: { font_size: 10.5 } }
                                    text: "Pinned"
                                }
                                <ScrollYView> {
                                    width: Fill,
                                    height: Fill,
                                    flow: Down,
                                    spacing: 2,
                                    pin_liked_btn = <SonoraPinBtn> { text: "Liked Songs" }
                                    pin_local_btn = <SonoraPinBtn> { text: "Local Lossless" }
                                    pin_slot_0 = <SonoraPinBtn> { visible: false, text: "" }
                                    pin_slot_1 = <SonoraPinBtn> { visible: false, text: "" }
                                    pin_slot_2 = <SonoraPinBtn> { visible: false, text: "" }
                                    pin_slot_3 = <SonoraPinBtn> { visible: false, text: "" }
                                    pin_slot_4 = <SonoraPinBtn> { visible: false, text: "" }
                                }

                                <FerricDividerH> { margin: { top: 6, bottom: 6 } }
                                nav_settings = <SonoraNavItem> {
                                    text: "Settings"
                                    draw_icon: { svg_path: "M19.14 12.94C19.18 12.64 19.2 12.33 19.2 12C19.2 11.68 19.18 11.36 19.13 11.06L21.16 9.48C21.34 9.34 21.39 9.07 21.28 8.87L19.36 5.55C19.24 5.33 18.99 5.26 18.77 5.33L16.38 6.29C15.88 5.91 15.35 5.59 14.76 5.35L14.4 2.81C14.36 2.57 14.16 2.4 13.92 2.4H10.08C9.84 2.4 9.65 2.57 9.61 2.81L9.25 5.35C8.66 5.59 8.12 5.92 7.63 6.29L5.24 5.33C5.02 5.25 4.77 5.33 4.65 5.55L2.74 8.87C2.62 9.08 2.66 9.34 2.86 9.48L4.89 11.06C4.84 11.36 4.8 11.69 4.8 12C4.8 12.31 4.82 12.64 4.87 12.94L2.84 14.52C2.66 14.66 2.61 14.93 2.72 15.13L4.64 18.45C4.76 18.67 5.01 18.74 5.23 18.67L7.62 17.71C8.12 18.09 8.65 18.41 9.24 18.65L9.6 21.19C9.65 21.43 9.84 21.6 10.08 21.6H13.92C14.16 21.6 14.36 21.43 14.39 21.19L14.75 18.65C15.34 18.41 15.88 18.09 16.37 17.71L18.76 18.67C18.98 18.75 19.23 18.67 19.35 18.45L21.27 15.13C21.39 14.91 21.34 14.66 21.15 14.52L19.14 12.94ZM12 15.6C10.02 15.6 8.4 13.98 8.4 12C8.4 10.02 10.02 8.4 12 8.4C13.98 8.4 15.6 10.02 15.6 12C15.6 13.98 13.98 15.6 12 15.6Z" }
                                }
                                sidebar_add_folder_btn = <SonoraNavItem> {
                                    text: "Add Folder"
                                    draw_icon: { svg_path: "M20 6H12L10 4H4C2.89 4 2 4.89 2 6V18C2 19.11 2.89 20 4 20H20C21.11 20 22 19.11 22 18V8C22 6.89 21.11 6 20 6ZM19 14H16V17H14V14H11V12H14V9H16V12H19V14Z" }
                                }
                            }

                            left_divider = <FerricDividerV> {}

                            // ============================================================
                            // CENTER STAGE (All Sonora Views)
                            // ============================================================
                            center_stage = <FerricSurface> {
                                width: Fill,
                                height: Fill,
                                flow: Overlay,
                                draw_bg: { base_col: #0E0F13, radius: 0.0 }

                                // --------------------------------------------------------
                                // PAGE 1: HOME (Default View — Mood Chips + Quick Picks + Cover Shelves!)
                                // --------------------------------------------------------
                                page_home = <ScrollYView> {
                                    visible: true,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 24, right: 24, top: 18, bottom: 26 }
                                    flow: Down,
                                    spacing: 22,

                                    // Mood & Genre Filter Chips (Live YouTube Music FEmusic_home!)
                                    mood_chips_row = <View> {
                                        width: Fill,
                                        height: Fit,
                                        flow: Right,
                                        spacing: 7,
                                        chip_0 = <SonoraChipBtn> { text: "All", draw_bg: { active: 1.0 }, draw_text: { color: #0E0F13 } }
                                        chip_1 = <SonoraChipBtn> { text: "Feel good" }
                                        chip_2 = <SonoraChipBtn> { text: "Relax" }
                                        chip_3 = <SonoraChipBtn> { text: "Energize" }
                                        chip_4 = <SonoraChipBtn> { text: "Workout" }
                                        chip_5 = <SonoraChipBtn> { text: "Focus" }
                                        chip_6 = <SonoraChipBtn> { text: "Commute" }
                                        chip_7 = <SonoraChipBtn> { text: "Sad" }
                                        chip_8 = <SonoraChipBtn> { text: "Party" }
                                    }

                                    // SECTION 1: QUICK PICKS (2x4 Grid w/ Real 46x46 Album Covers)
                                    <View> {
                                        width: Fill,
                                        height: Fit,
                                        flow: Down,
                                        spacing: 10,

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #F4F4F7, text_style: { font_size: 18.0 } }
                                                text: "Quick picks"
                                            }
                                            home_status_lbl = <Label> {
                                                draw_text: { color: #7C8090, text_style: { font_size: 11.5 } }
                                                text: "Loading YouTube Music..."
                                            }
                                            home_play_all_btn = <SonoraBtn> { height: 28, padding: { left: 10, right: 11 }, text: "Play all" }
                                            qp_prev_btn = <SonoraIconBtn> {
                                                width: 28, height: 28,
                                                draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" }
                                            }
                                            qp_next_btn = <SonoraIconBtn> {
                                                width: 28, height: 28,
                                                draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" }
                                            }
                                        }

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 10,
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 6,
                                                qp_0 = <QuickPickTile> {}
                                                qp_1 = <QuickPickTile> {}
                                                qp_2 = <QuickPickTile> {}
                                                qp_3 = <QuickPickTile> {}
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 6,
                                                qp_4 = <QuickPickTile> {}
                                                qp_5 = <QuickPickTile> {}
                                                qp_6 = <QuickPickTile> {}
                                                qp_7 = <QuickPickTile> {}
                                            }
                                        }
                                    }

                                    // SECTION 2: HORIZONTAL SHELF 0 (Trending / Community Playlists)
                                    shelf0_wrap = <View> {
                                        width: Fill, height: Fit, flow: Down, spacing: 10,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                            sh0_title_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #F4F4F7, text_style: { font_size: 17.5 } }
                                                text: "Trending playlists & mixes"
                                            }
                                            sh0_prev_btn = <SonoraIconBtn> {
                                                width: 28, height: 28,
                                                draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" }
                                            }
                                            sh0_next_btn = <SonoraIconBtn> {
                                                width: 28, height: 28,
                                                draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" }
                                            }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 10,
                                            sh0_c0 = <ShelfCard> {}
                                            sh0_c1 = <ShelfCard> {}
                                            sh0_c2 = <ShelfCard> {}
                                            sh0_c3 = <ShelfCard> {}
                                            sh0_c4 = <ShelfCard> {}
                                        }
                                    }

                                    // SECTION 3: HORIZONTAL SHELF 1 (New Albums & Singles)
                                    shelf1_wrap = <View> {
                                        width: Fill, height: Fit, flow: Down, spacing: 10,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                            sh1_title_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #F4F4F7, text_style: { font_size: 17.5 } }
                                                text: "New albums & singles"
                                            }
                                            sh1_prev_btn = <SonoraIconBtn> {
                                                width: 28, height: 28,
                                                draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" }
                                            }
                                            sh1_next_btn = <SonoraIconBtn> {
                                                width: 28, height: 28,
                                                draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" }
                                            }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 10,
                                            sh1_c0 = <ShelfCard> {}
                                            sh1_c1 = <ShelfCard> {}
                                            sh1_c2 = <ShelfCard> {}
                                            sh1_c3 = <ShelfCard> {}
                                            sh1_c4 = <ShelfCard> {}
                                        }
                                    }

                                    // SECTION 4: HORIZONTAL SHELF 2 (Song Mixes / Discover More)
                                    shelf2_wrap = <View> {
                                        width: Fill, height: Fit, flow: Down, spacing: 10,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                            sh2_title_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #F4F4F7, text_style: { font_size: 17.5 } }
                                                text: "Discover more"
                                            }
                                            sh2_prev_btn = <SonoraIconBtn> {
                                                width: 28, height: 28,
                                                draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" }
                                            }
                                            sh2_next_btn = <SonoraIconBtn> {
                                                width: 28, height: 28,
                                                draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" }
                                            }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 10,
                                            sh2_c0 = <ShelfCard> {}
                                            sh2_c1 = <ShelfCard> {}
                                            sh2_c2 = <ShelfCard> {}
                                            sh2_c3 = <ShelfCard> {}
                                            sh2_c4 = <ShelfCard> {}
                                        }
                                    }
                                }

                                // --------------------------------------------------------
                                // PAGE 2: SEARCH (Dedicated Sonora Instant Search + Category Tabs + Cards)
                                // --------------------------------------------------------
                                page_search = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 24, right: 24, top: 18, bottom: 14 }
                                    flow: Down,
                                    spacing: 12,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                        search_page_input = <SonoraInput> {
                                            width: Fill,
                                            height: 40,
                                            empty_text: "Search YouTube Music, local library, or paste Spotify / YouTube link..."
                                        }
                                        search_go_btn = <SonoraPrimaryBtn> { height: 40, text: "Search" }
                                        search_clear_btn = <SonoraBtn> { height: 40, text: "Clear" }
                                    }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 7,
                                        sf_all_btn = <SonoraChipBtn> { text: "All", draw_bg: { active: 1.0 }, draw_text: { color: #0E0F13 } }
                                        sf_songs_btn = <SonoraChipBtn> { text: "Songs" }
                                        sf_albums_btn = <SonoraChipBtn> { text: "Albums" }
                                        sf_artists_btn = <SonoraChipBtn> { text: "Artists" }
                                        sf_playlists_btn = <SonoraChipBtn> { text: "Playlists" }
                                        sf_local_btn = <SonoraChipBtn> { text: "Local" }
                                        <View> { width: Fill, height: 1 }
                                        search_status_lbl = <Label> {
                                            draw_text: { color: #848796, text_style: { font_size: 11.5 } }
                                            text: "Instant YouTube Music + Local Search"
                                        }
                                    }

                                    // Recent searches row
                                    recent_searches_row = <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                        <Label> { draw_text: { color: #6E7180, text_style: { font_size: 11.5 } }, text: "Recent:" }
                                        rsrch_0 = <SonoraBtn> { visible: false, height: 26, text: "" }
                                        rsrch_1 = <SonoraBtn> { visible: false, height: 26, text: "" }
                                        rsrch_2 = <SonoraBtn> { visible: false, height: 26, text: "" }
                                        rsrch_3 = <SonoraBtn> { visible: false, height: 26, text: "" }
                                        rsrch_4 = <SonoraBtn> { visible: false, height: 26, text: "" }
                                    }

                                    // Search Album / Artist / Playlist cards strip
                                    search_cards_wrap = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 8,
                                        <Label> { draw_text: { color: #C5C7D4, text_style: { font_size: 13.5 } }, text: "Albums, Artists & Playlists" }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 10,
                                            sc_0 = <ShelfCard> {}
                                            sc_1 = <ShelfCard> {}
                                            sc_2 = <ShelfCard> {}
                                            sc_3 = <ShelfCard> {}
                                            sc_4 = <ShelfCard> {}
                                        }
                                    }

                                    search_track_list = <TrackList> { width: Fill, height: Fill }
                                }

                                // --------------------------------------------------------
                                // PAGE 3: RECENTS (Chronological Listening History)
                                // --------------------------------------------------------
                                page_recents = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 24, right: 24, top: 20, bottom: 14 }
                                    flow: Down,
                                    spacing: 14,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 3,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 22.0 } }, text: "Recents" }
                                            recents_sub_lbl = <Label> { draw_text: { color: #8A8D9B, text_style: { font_size: 12.0 } }, text: "Your recently played songs" }
                                        }
                                        recents_play_btn = <SonoraPrimaryBtn> { text: "Play All" }
                                        recents_clear_btn = <SonoraBtn> { text: "Clear History" }
                                    }
                                    <FerricDividerH> {}
                                    recents_track_list = <TrackList> { width: Fill, height: Fill }
                                }

                                // --------------------------------------------------------
                                // PAGE 4: LIBRARY -> SONGS
                                // --------------------------------------------------------
                                page_songs = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 24, right: 24, top: 20, bottom: 14 }
                                    flow: Down,
                                    spacing: 12,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 3,
                                            songs_title_lbl = <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 22.0 } }, text: "Songs" }
                                            songs_sub_lbl = <Label> { draw_text: { color: #8A8D9B, text_style: { font_size: 12.0 } }, text: "0 songs · 6.0s Bass-Swap Automix" }
                                        }
                                        play_all_btn = <SonoraPrimaryBtn> {
                                            text: "Play All"
                                            draw_icon: { svg_path: "M8 5V19L19 12L8 5Z" }
                                        }
                                        hero_shuffle_btn = <SonoraBtn> { text: "Shuffle" }
                                        add_folder_btn = <SonoraBtn> { text: "Add Folder" }
                                    }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 7,
                                        flt_all_btn = <SonoraChipBtn> { text: "All", draw_bg: { active: 1.0 }, draw_text: { color: #0E0F13 } }
                                        flt_liked_btn = <SonoraChipBtn> { text: "Liked" }
                                        flt_local_btn = <SonoraChipBtn> { text: "Local" }
                                        flt_spotify_btn = <SonoraChipBtn> { text: "Spotify" }
                                        flt_youtube_btn = <SonoraChipBtn> { text: "YouTube" }
                                        <View> { width: Fill, height: 1 }
                                        sort_cycle_btn = <SonoraBtn> { height: 30, text: "Sort: Artist" }
                                    }

                                    <FerricDividerH> {}
                                    songs_track_list = <TrackList> { width: Fill, height: Fill }
                                }

                                // --------------------------------------------------------
                                // PAGE 5: LIBRARY -> ALBUMS GRID
                                // --------------------------------------------------------
                                page_albums = <ScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 24, right: 24, top: 20, bottom: 24 }
                                    flow: Down,
                                    spacing: 14,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 2,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 22.0 } }, text: "Albums" }
                                            albums_sub_lbl = <Label> { draw_text: { color: #8A8D9B, text_style: { font_size: 12.0 } }, text: "Grouped from your catalog" }
                                        }
                                        alb_prev_btn = <SonoraIconBtn> { width: 30, height: 30, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                        alb_next_btn = <SonoraIconBtn> { width: 30, height: 30, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                    }
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 12,
                                        alb_0 = <GridCard> {}
                                        alb_1 = <GridCard> {}
                                        alb_2 = <GridCard> {}
                                        alb_3 = <GridCard> {}
                                    }
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 12,
                                        alb_4 = <GridCard> {}
                                        alb_5 = <GridCard> {}
                                        alb_6 = <GridCard> {}
                                        alb_7 = <GridCard> {}
                                    }
                                }

                                // --------------------------------------------------------
                                // PAGE 6: LIBRARY -> ARTISTS CIRCULAR GRID
                                // --------------------------------------------------------
                                page_artists = <ScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 24, right: 24, top: 20, bottom: 24 }
                                    flow: Down,
                                    spacing: 14,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 2,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 22.0 } }, text: "Artists" }
                                            artists_sub_lbl = <Label> { draw_text: { color: #8A8D9B, text_style: { font_size: 12.0 } }, text: "Artists in your library" }
                                        }
                                        art_prev_btn = <SonoraIconBtn> { width: 30, height: 30, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                        art_next_btn = <SonoraIconBtn> { width: 30, height: 30, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                    }
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 12,
                                        art_0 = <GridCard> {}
                                        art_1 = <GridCard> {}
                                        art_2 = <GridCard> {}
                                        art_3 = <GridCard> {}
                                    }
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 12,
                                        art_4 = <GridCard> {}
                                        art_5 = <GridCard> {}
                                        art_6 = <GridCard> {}
                                        art_7 = <GridCard> {}
                                    }
                                }

                                // --------------------------------------------------------
                                // PAGE 7: LIBRARY -> PLAYLISTS GRID + CREATOR
                                // --------------------------------------------------------
                                page_playlists = <ScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 24, right: 24, top: 20, bottom: 24 }
                                    flow: Down,
                                    spacing: 14,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                        <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: { font_size: 22.0 } }, text: "Playlists" }
                                        pl_new_input = <SonoraInput> { width: 210, height: 34, empty_text: "New playlist name..." }
                                        pl_create_btn = <SonoraPrimaryBtn> { height: 34, text: "+ Create" }
                                        pl_import_btn = <SonoraBtn> { height: 34, text: "Import M3U/CSV" }
                                        pl_prev_btn = <SonoraIconBtn> { width: 30, height: 30, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                        pl_next_btn = <SonoraIconBtn> { width: 30, height: 30, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                    }
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 12,
                                        plc_0 = <GridCard> {}
                                        plc_1 = <GridCard> {}
                                        plc_2 = <GridCard> {}
                                        plc_3 = <GridCard> {}
                                    }
                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 12,
                                        plc_4 = <GridCard> {}
                                        plc_5 = <GridCard> {}
                                        plc_6 = <GridCard> {}
                                        plc_7 = <GridCard> {}
                                    }
                                }

                                // --------------------------------------------------------
                                // PAGE 8: DETAIL VIEW (Album / Playlist / Artist — Local & Remote YTM)
                                // --------------------------------------------------------
                                page_detail = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 24, right: 24, top: 20, bottom: 14 }
                                    flow: Down,
                                    spacing: 14,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 18,
                                        detail_art = <FerricCoverArt> {
                                            width: 140,
                                            height: 140,
                                            draw_bg: { radius: 12.0 }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 6,
                                            detail_kind_lbl = <Label> { draw_text: { color: #A882FF, text_style: { font_size: 11.0 } }, text: "PLAYLIST" }
                                            detail_title_lbl = <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 24.0 } }, text: "Collection" }
                                            detail_sub_lbl = <Label> { draw_text: { color: #9A9CA8, text_style: { font_size: 12.5 } }, text: "" }
                                            <View> {
                                                width: Fill, height: Fit, margin: { top: 6 }, flow: Right, spacing: 8,
                                                detail_play_btn = <SonoraPrimaryBtn> { text: "Play", draw_icon: { svg_path: "M8 5V19L19 12L8 5Z" } }
                                                detail_shuffle_btn = <SonoraBtn> { height: 34, text: "Shuffle" }
                                                detail_radio_btn = <SonoraBtn> { height: 34, text: "Start Radio" }
                                                detail_pin_btn = <SonoraBtn> { height: 34, text: "Pin" }
                                                detail_queue_btn = <SonoraBtn> { height: 34, text: "+ Queue" }
                                            }
                                        }
                                    }
                                    <FerricDividerH> {}
                                    detail_track_list = <TrackList> { width: Fill, height: Fill }
                                }

                                // --------------------------------------------------------
                                // PAGE 9: TABBED SETTINGS HUB (Keeps Left Sidebar Clean!)
                                // --------------------------------------------------------
                                page_settings = <ScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 24, right: 24, top: 20, bottom: 26 }
                                    flow: Down,
                                    spacing: 16,

                                    <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 22.0 } }, text: "Settings" }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 7,
                                        st_accounts_btn = <SonoraChipBtn> { text: "Accounts", draw_bg: { active: 1.0 }, draw_text: { color: #0E0F13 } }
                                        st_playback_btn = <SonoraChipBtn> { text: "Playback & Fade-In" }
                                        st_eq_btn = <SonoraChipBtn> { text: "Equalizer" }
                                        st_appearance_btn = <SonoraChipBtn> { text: "Appearance" }
                                        st_storage_btn = <SonoraChipBtn> { text: "Storage & Updates" }
                                        st_connect_btn = <SonoraChipBtn> { text: "Connect" }
                                    }

                                    // TAB A: ACCOUNTS & SOURCES
                                    tab_accounts = <View> {
                                        visible: true,
                                        width: Fill, height: Fit, flow: Down, spacing: 14,

                                        <SonoraCard> {
                                            flow: Down, spacing: 10,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: { font_size: 15.5 } }, text: "YouTube Music & Google Account" }
                                                yt_state_lbl = <Label> { draw_text: { color: #A882FF, text_style: { font_size: 12.0 } }, text: "Guest Mode (Optional)" }
                                            }
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #9A9CA8, text_style: { font_size: 12.0 } }
                                                text: "Music works out of the box without any account. Sign in with your regular Google account in one click to sync personal Liked Songs and Playlists."
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8,
                                                yt_login_btn = <SonoraPrimaryBtn> { text: "Sign in with Google" }
                                                yt_sync_btn = <SonoraBtn> { height: 34, text: "Sync Library" }
                                                yt_logout_btn = <SonoraBtn> { height: 34, text: "Sign Out" }
                                                guest_refresh_btn = <SonoraBtn> { height: 34, text: "Refresh Home Feed" }
                                            }
                                            oauth_status_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #C5B2FF, text_style: { font_size: 12.0 } }
                                                text: "Guest Mode active — YouTube Music streaming ready."
                                            }
                                        }

                                        <SonoraCard> {
                                            flow: Down, spacing: 10,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 15.0 } }, text: "Instant Spotify & YouTube Playlist Import" }
                                            quick_sync_input = <SonoraInput> {
                                                empty_text: "Paste public Spotify or YouTube playlist / album / track URL..."
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8,
                                                quick_spotify_btn = <SonoraBtn> { text: "Import Spotify Link" }
                                                quick_youtube_btn = <SonoraBtn> { text: "Import YouTube Link" }
                                                match_local_btn = <SonoraBtn> { text: "Match Cloud to Local FLAC" }
                                                rescan_btn = <SonoraBtn> { text: "Rescan Local Folders" }
                                            }
                                        }

                                        <SonoraCard> {
                                            flow: Down, spacing: 10,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 15.0 } }, text: "OpenSubsonic / Navidrome Self-Hosted Server" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8,
                                                sub_url_input = <SonoraInput> { width: Fill, empty_text: "https://music.example.lan" }
                                                sub_user_input = <SonoraInput> { width: 150, empty_text: "Username" }
                                                sub_pass_input = <SonoraInput> { width: 150, is_password: true, empty_text: "Password" }
                                                sub_sync_btn = <SonoraBtn> { height: 36, text: "Connect" }
                                            }
                                        }
                                    }

                                    // TAB B: PLAYBACK & SEAMLESS FADE-IN DSP
                                    tab_playback = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 14,

                                        <SonoraCard> {
                                            flow: Down, spacing: 12,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: { font_size: 15.5 } }, text: "Seamless Dual-Deck Fade-In & Crossfade" }
                                                xfade_val_lbl = <Label> { draw_text: { color: #A882FF, text_style: { font_size: 13.5 } }, text: "6.0 s" }
                                            }
                                            xfade_slider = <FerricBarSlider> { min: 0.0, max: 12.0, value: 6.0 }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8,
                                                xf_0_btn = <SonoraBtn> { text: "0s Gapless" }
                                                xf_3_btn = <SonoraBtn> { text: "3s Crisp" }
                                                xf_6_btn = <SonoraBtn> { text: "6s Seamless" }
                                                xf_10_btn = <SonoraBtn> { text: "10s Club Mix" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                <Label> { draw_text: { color: #9A9CA8, text_style: { font_size: 12.0 } }, text: "Transition Curve:" }
                                                curve_bass_btn = <SonoraBtn> { text: "Bass-Swap Automix" }
                                                curve_eqp_btn = <SonoraBtn> { text: "Equal-Power" }
                                                curve_velvet_btn = <SonoraBtn> { text: "S-Curve Velvet" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #C8CAD6, text_style: { font_size: 12.5 } }, text: "Soft Play / Resume Fade-In (ms)" }
                                                fadein_val_lbl = <Label> { draw_text: { color: #A882FF, text_style: { font_size: 12.5 } }, text: "240 ms" }
                                            }
                                            fadein_slider = <FerricBarSlider> { min: 0.0, max: 1000.0, value: 240.0 }
                                        }

                                        <SonoraCard> {
                                            flow: Down, spacing: 10,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 15.0 } }, text: "Playback Behavior & Radio" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: { font_size: 12.5 } }, text: "Autoplay Infinite Radio (YouTube Music Next)" }
                                                radio_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: { font_size: 12.5 } }, text: "0ms Gapless Album Lock (Keep consecutive album transitions exact)" }
                                                gapless_lock_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: { font_size: 12.5 } }, text: "Skip Leading & Trailing Silence" }
                                                trim_silence_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: { font_size: 12.5 } }, text: "ReplayGain Loudness Normalization" }
                                                replay_gain_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: { font_size: 12.5 } }, text: "Mono Downmix" }
                                                mono_toggle = <FerricToggle> { is_on: false }
                                            }
                                        }

                                        <SonoraCard> {
                                            flow: Down, spacing: 10,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: { font_size: 14.0 } }, text: "Playback Speed" }
                                                spd_075_btn = <SonoraBtn> { text: "0.75x" }
                                                spd_100_btn = <SonoraBtn> { text: "1.0x" }
                                                spd_125_btn = <SonoraBtn> { text: "1.25x" }
                                                spd_150_btn = <SonoraBtn> { text: "1.5x" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                sleep_status_lbl = <Label> { width: Fill, draw_text: { color: #C8CAD6, text_style: { font_size: 13.0 } }, text: "Sleep Timer: Off" }
                                                slp_off_btn = <SonoraBtn> { text: "Off" }
                                                slp_15_btn = <SonoraBtn> { text: "15m" }
                                                slp_30_btn = <SonoraBtn> { text: "30m" }
                                                slp_60_btn = <SonoraBtn> { text: "60m" }
                                                slp_eot_btn = <SonoraBtn> { text: "End of Track" }
                                            }
                                        }
                                    }

                                    // TAB C: 10-BAND PARAMETRIC EQUALIZER + ALL 12 SONORA PRESETS + AUTOEQ
                                    tab_eq = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 14,

                                        <SonoraCard> {
                                            flow: Down, spacing: 12,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: { font_size: 15.5 } }, text: "10-Band Parametric Biquad Equalizer" }
                                                eq_status_lbl = <Label> { draw_text: { color: #9A9CA8, text_style: { font_size: 11.5 } }, text: "Preset: Flat" }
                                                eq_enable_toggle = <FerricToggle> { is_on: true }
                                            }
                                            eq_graph = <FerricEqGraph> { height: 154 }

                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                                <Label> { width: 84, draw_text: { color: #C8CAD6, text_style: { font_size: 12.0 } }, text: "Preamp" }
                                                eq_preamp_slider = <FerricBarSlider> { min: -12.0, max: 12.0, value: 0.0 }
                                                eq_preamp_lbl = <Label> { width: 58, draw_text: { color: #A882FF, text_style: { font_size: 12.0 } }, text: "+0.0 dB" }
                                            }

                                            // 12 Sonora EQ Presets (2 clean rows of 6)
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                eq_pre_flat = <SonoraBtn> { height: 28, text: "Flat" }
                                                eq_pre_bass = <SonoraBtn> { height: 28, text: "Bass Boost" }
                                                eq_pre_bassred = <SonoraBtn> { height: 28, text: "Bass Reducer" }
                                                eq_pre_treble = <SonoraBtn> { height: 28, text: "Treble Boost" }
                                                eq_pre_vocal = <SonoraBtn> { height: 28, text: "Vocal Clarity" }
                                                eq_pre_rock = <SonoraBtn> { height: 28, text: "Rock" }
                                                eq_pre_pop = <SonoraBtn> { height: 28, text: "Pop" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                eq_pre_jazz = <SonoraBtn> { height: 28, text: "Jazz" }
                                                eq_pre_classical = <SonoraBtn> { height: 28, text: "Classical" }
                                                eq_pre_elec = <SonoraBtn> { height: 28, text: "Electronic" }
                                                eq_pre_acoustic = <SonoraBtn> { height: 28, text: "Acoustic" }
                                                eq_pre_loudness = <SonoraBtn> { height: 28, text: "Loudness" }
                                                eq_pre_copper = <SonoraBtn> { height: 28, text: "Warm Copper" }
                                            }

                                            // 10 Vertical Faders
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { x: 0.5 }, spacing: 14,
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v0 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 10.5 } }, text: "+0.0" }, eq_b0 = <FerricVertSlider> { height: 124 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 10.5 } }, text: "31" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v1 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 10.5 } }, text: "+0.0" }, eq_b1 = <FerricVertSlider> { height: 124 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 10.5 } }, text: "62" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v2 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 10.5 } }, text: "+0.0" }, eq_b2 = <FerricVertSlider> { height: 124 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 10.5 } }, text: "125" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v3 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 10.5 } }, text: "+0.0" }, eq_b3 = <FerricVertSlider> { height: 124 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 10.5 } }, text: "250" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v4 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 10.5 } }, text: "+0.0" }, eq_b4 = <FerricVertSlider> { height: 124 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 10.5 } }, text: "500" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v5 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 10.5 } }, text: "+0.0" }, eq_b5 = <FerricVertSlider> { height: 124 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 10.5 } }, text: "1k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v6 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 10.5 } }, text: "+0.0" }, eq_b6 = <FerricVertSlider> { height: 124 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 10.5 } }, text: "2k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v7 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 10.5 } }, text: "+0.0" }, eq_b7 = <FerricVertSlider> { height: 124 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 10.5 } }, text: "4k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v8 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 10.5 } }, text: "+0.0" }, eq_b8 = <FerricVertSlider> { height: 124 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 10.5 } }, text: "8k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4, eq_v9 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 10.5 } }, text: "+0.0" }, eq_b9 = <FerricVertSlider> { height: 124 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 10.5 } }, text: "16k" } }
                                            }

                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                autoeq_input = <SonoraInput> { width: Fill, empty_text: "AutoEq Headphone Lookup (e.g. Sony WH-1000XM5, HD 650, AirPods Pro)..." }
                                                autoeq_apply_btn = <SonoraBtn> { height: 36, text: "Apply AutoEq" }
                                                eq_import_btn = <SonoraBtn> { height: 36, text: "Import APO" }
                                                eq_export_btn = <SonoraBtn> { height: 36, text: "Export APO" }
                                            }
                                        }
                                    }

                                    // TAB D: APPEARANCE, ADAPTIVE ARTWORK & LIQUID GLASS
                                    tab_appearance = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 14,

                                        <SonoraCard> {
                                            flow: Down, spacing: 12,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 15.5 } }, text: "Theme & Adaptive Artwork Accent" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8,
                                                pal_sonora_btn = <SonoraBtn> { text: "Sonora Midnight" }
                                                pal_amoled_btn = <SonoraBtn> { text: "Pure AMOLED" }
                                                pal_cream_btn = <SonoraBtn> { text: "Warm Cream" }
                                                theme_import_btn = <SonoraBtn> { text: "Import JSON" }
                                                theme_export_btn = <SonoraBtn> { text: "Export JSON" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: { font_size: 12.5 } }, text: "Adaptive Artwork Accent (Tint player glow, seekbar & lyrics from active cover art)" }
                                                adaptive_art_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: { font_size: 12.5 } }, text: "Liquid Glass Optical Refraction (Snell IOR + Prism Dispersion)" }
                                                liquid_glass_toggle = <FerricToggle> { is_on: false }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8,
                                                lg_pre_crystal = <SonoraBtn> { height: 28, text: "Crystal Pure" }
                                                lg_pre_prism = <SonoraBtn> { height: 28, text: "Prismatic" }
                                                lg_pre_copper = <SonoraBtn> { height: 28, text: "Smoked Copper" }
                                                lg_pre_frost = <SonoraBtn> { height: 28, text: "Ultra Frosted" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                                <Label> { width: 88, draw_text: { color: #9A9CA8, text_style: { font_size: 11.5 } }, text: "IOR / Disp" }
                                                lg_ior_slider = <FerricBarSlider> { min: 1.05, max: 1.95, value: 1.48 }
                                                lg_ior_lbl = <Label> { width: 42, draw_text: { color: #C8CAD6, text_style: { font_size: 11.5 } }, text: "1.48" }
                                                lg_disp_slider = <FerricBarSlider> { min: 0.0, max: 1.0, value: 0.42 }
                                                lg_disp_lbl = <Label> { width: 42, draw_text: { color: #C8CAD6, text_style: { font_size: 11.5 } }, text: "42%" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                                <Label> { width: 88, draw_text: { color: #9A9CA8, text_style: { font_size: 11.5 } }, text: "Rim / Radius" }
                                                lg_rim_slider = <FerricBarSlider> { min: 0.0, max: 1.0, value: 0.78 }
                                                lg_rad_slider = <FerricBarSlider> { min: 0.0, max: 32.0, value: 12.0 }
                                                lg_rim_lbl = <Label> { width: 86, draw_text: { color: #C8CAD6, text_style: { font_size: 11.5 } }, text: "78% · 12px" }
                                            }
                                            lg_preview = <LiquidGlassPreview> { height: 260 }
                                        }
                                    }

                                    // TAB E: STORAGE, UPDATES & KEYBOARD SHORTCUTS
                                    tab_storage = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 14,

                                        <SonoraCard> {
                                            flow: Down, spacing: 10,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 15.0 } }, text: "Local Disk Cache" }
                                            cache_stats_lbl = <Label> { draw_text: { color: #9A9CA8, text_style: { font_size: 12.5 } }, text: "Audio Stream Cache: 0 KB · Artwork Cache: 0 KB" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8,
                                                clear_audio_cache_btn = <SonoraBtn> { text: "Clear Audio Cache" }
                                                clear_art_cache_btn = <SonoraBtn> { text: "Clear Artwork Cache" }
                                                load_demo_btn = <SonoraBtn> { text: "Load Offline Demo Tracks" }
                                            }
                                        }

                                        <SonoraCard> {
                                            flow: Down, spacing: 10,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: { font_size: 15.0 } }, text: "Software Updates (Ferric v0.1.0)" }
                                                auto_update_toggle = <FerricToggle> { is_on: true }
                                            }
                                            update_status_lbl = <Label> { draw_text: { color: #9A9CA8, text_style: { font_size: 12.0 } }, text: "SHA-256 verified GitHub Releases MSI updater" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8,
                                                check_updates_btn = <SonoraBtn> { text: "Check for Updates" }
                                                open_releases_btn = <SonoraBtn> { text: "Open GitHub Releases" }
                                            }
                                        }

                                        <SonoraCard> {
                                            flow: Down, spacing: 6,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 14.5 } }, text: "Keyboard Shortcuts" }
                                            <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: { font_size: 12.0 } }, text: "Space: Play / Pause  ·  Ctrl+K: Powerbar  ·  Ctrl+F: Search  ·  Ctrl+Q: Toggle Queue Sidebar  ·  Ctrl+L: Toggle Lyrics  ·  Ctrl+,: Settings  ·  F11: Fullscreen Now Playing  ·  Alt+Left / Alt+Right: Back / Forward  ·  Shift+Left / Shift+Right: Prev / Next Track  ·  Ctrl+Up / Ctrl+Down: Volume" }
                                        }
                                    }

                                    // TAB F: FERRIC CONNECT (LAN QR)
                                    tab_connect = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 14,
                                        <SonoraCard> {
                                            flow: Right, align: { y: 0.5 }, spacing: 20,
                                            connect_qr_view = <FerricQrView> { width: 156, height: 156 }
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 8,
                                                <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 16.0 } }, text: "Ferric Connect (Local LAN Pairing)" }
                                                connect_lan_ip_lbl = <Label> { draw_text: { color: #A882FF, text_style: { font_size: 12.5 } }, text: "LAN: Ready" }
                                                connect_peers_lbl = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: { font_size: 12.0 } }, text: "Listening for Ferric devices on local Wi-Fi..." }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, spacing: 8,
                                                    connect_gen_pair_btn = <SonoraBtn> { text: "Regenerate QR" }
                                                    connect_copy_pair_btn = <SonoraBtn> { text: "Copy Pairing URI" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            right_divider = <FerricDividerV> {}

                            // ============================================================
                            // DOCKED RIGHT SIDEBAR (292px, OPEN BY DEFAULT — Exact Sonora Queue & Lyrics!)
                            // ============================================================
                            right_sidebar = <FerricSurface> {
                                visible: true,
                                width: 292,
                                height: Fill,
                                padding: { left: 10, right: 10, top: 10, bottom: 10 }
                                flow: Down,
                                spacing: 8,
                                draw_bg: { base_col: #14151C, radius: 0.0 }

                                // Right Sidebar Header Tabs: [Queue] [Lyrics] + [Radio] [Clear]
                                <View> {
                                    width: Fill, height: 32, flow: Right, align: { y: 0.5 }, spacing: 6,
                                    rs_tab_queue = <SonoraChipBtn> { height: 28, text: "Queue", draw_bg: { active: 1.0 }, draw_text: { color: #0E0F13 } }
                                    rs_tab_lyrics = <SonoraChipBtn> { height: 28, text: "Lyrics" }
                                    <View> { width: Fill, height: 1 }
                                    rs_radio_btn = <SonoraIconBtn> {
                                        width: 28, height: 28,
                                        draw_bg: { active: 1.0 }
                                        draw_icon: {
                                            svg_path: "M3.24 6.15C2.51 6.43 2 7.17 2 8V20C2 21.1 2.89 22 4 22H20C21.11 22 22 21.1 22 20V8C22 6.89 21.11 6 20 6H8.3L16.56 2.66L15.88 1L3.24 6.15ZM7 20C5.34 20 4 18.66 4 17C4 15.34 5.34 14 7 14C8.66 14 10 15.34 10 17C10 18.66 8.66 20 7 20ZM20 12H18V10H16V12H4V8H20V12Z"
                                            color: #A882FF
                                        }
                                    }
                                    rs_clear_btn = <SonoraIconBtn> {
                                        width: 28, height: 28,
                                        draw_icon: { svg_path: "M6 19C6 20.1 6.9 21 8 21H16C17.1 21 18 20.1 18 19V7H6V19ZM19 4H15.5L14.5 3H9.5L8.5 4H5V6H19V4Z" }
                                    }
                                }

                                <FerricDividerH> {}

                                // TAB 1: SONORA QUEUE (History -> Now Playing -> Up Next)
                                rs_queue_view = <ScrollYView> {
                                    visible: true,
                                    width: Fill,
                                    height: Fill,
                                    padding: { top: 4, bottom: 8 }
                                    flow: Down,
                                    spacing: 5,

                                    qh_header = <View> {
                                        visible: false,
                                        width: Fill,
                                        height: Fit,
                                        margin: { left: 6, top: 2, bottom: 2 }
                                        <Label> {
                                            draw_text: { color: #757888, text_style: { font_size: 10.5 } }
                                            text: "History"
                                        }
                                    }
                                    qh_0 = <QueueMiniRow> { visible: false }
                                    qh_1 = <QueueMiniRow> { visible: false }
                                    qh_2 = <QueueMiniRow> { visible: false }

                                    q_now_header = <Label> {
                                        margin: { left: 6, top: 6, bottom: 2 }
                                        draw_text: { color: #A882FF, text_style: { font_size: 10.5 } }
                                        text: "Now playing"
                                    }
                                    q_now_row = <QueueMiniRow> {
                                        draw_bg: { active: 1.0 }
                                    }

                                    <View> {
                                        width: Fill, height: Fit, margin: { left: 6, right: 4, top: 8, bottom: 2 },
                                        flow: Right, align: { y: 0.5 },
                                        q_upnext_header = <Label> {
                                            width: Fill,
                                            draw_text: { color: #757888, text_style: { font_size: 10.5 } }
                                            text: "Up next"
                                        }
                                        q_radio_status = <Label> {
                                            draw_text: { color: #6E7180, text_style: { font_size: 10.0 } }
                                            text: "Radio On"
                                        }
                                    }
                                    qu_0 = <QueueMiniRow> { visible: false }
                                    qu_1 = <QueueMiniRow> { visible: false }
                                    qu_2 = <QueueMiniRow> { visible: false }
                                    qu_3 = <QueueMiniRow> { visible: false }
                                    qu_4 = <QueueMiniRow> { visible: false }
                                    qu_5 = <QueueMiniRow> { visible: false }
                                    qu_6 = <QueueMiniRow> { visible: false }
                                    qu_7 = <QueueMiniRow> { visible: false }
                                    qu_8 = <QueueMiniRow> { visible: false }
                                    qu_9 = <QueueMiniRow> { visible: false }
                                }

                                // TAB 2: DOCKED LIVE SYNCED LYRICS
                                rs_lyrics_view = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    flow: Down,
                                    spacing: 10,
                                    padding: { left: 6, right: 6, top: 6, bottom: 6 }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 4,
                                        rs_lyr_source = <Label> { width: Fill, draw_text: { color: #8A8D9B, text_style: { font_size: 10.5 } }, text: "Synced Lyrics" }
                                        rs_lyr_minus = <SonoraBtn> { height: 24, padding: { left: 6, right: 6 }, text: "-0.5s", draw_text: { text_style: { font_size: 10.5 } } }
                                        rs_lyr_plus = <SonoraBtn> { height: 24, padding: { left: 6, right: 6 }, text: "+0.5s", draw_text: { text_style: { font_size: 10.5 } } }
                                        rs_lyr_expand = <SonoraBtn> { height: 24, padding: { left: 7, right: 7 }, text: "Full", draw_text: { text_style: { font_size: 10.5 } } }
                                    }

                                    <View> {
                                        width: Fill, height: Fill, flow: Down, align: { y: 0.5 }, spacing: 11,
                                        rs_lyr_0 = <Label> { width: Fill, draw_text: { color: #4A4D5C, text_style: { font_size: 12.5 } }, text: "" }
                                        rs_lyr_1 = <Label> { width: Fill, draw_text: { color: #6E7182, text_style: { font_size: 13.5 } }, text: "" }
                                        rs_lyr_2 = <Label> { width: Fill, draw_text: { color: #A6A9B8, text_style: { font_size: 14.5 } }, text: "" }
                                        rs_lyr_3 = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: { font_size: 17.0 } }, text: "Play a song to view synced lyrics" }
                                        rs_lyr_4 = <Label> { width: Fill, draw_text: { color: #A6A9B8, text_style: { font_size: 14.5 } }, text: "" }
                                        rs_lyr_5 = <Label> { width: Fill, draw_text: { color: #6E7182, text_style: { font_size: 13.5 } }, text: "" }
                                        rs_lyr_6 = <Label> { width: Fill, draw_text: { color: #4A4D5C, text_style: { font_size: 12.5 } }, text: "" }
                                    }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 6,
                                        rs_lyr_copy = <SonoraBtn> { height: 28, text: "Copy Lyrics" }
                                        rs_watch_video = <SonoraBtn> { height: 28, text: "Video Clip" }
                                    }
                                }
                            }
                        }

                        // ============================================================
                        // 3. SLEEK SINGLE-ROW AMBIENT BOTTOM PLAYER BAR (68px — Sonora Signature!)
                        // ============================================================
                        bottom_bar = <AmbientBottomBar> {
                            // Left: 44x44 Real Cover Art + Title + Heart + Artist
                            <View> {
                                width: 256,
                                height: Fill,
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 11,

                                np_art = <FerricCoverArt> {
                                    width: 44,
                                    height: 44,
                                    draw_bg: { radius: 8.0 }
                                }
                                <View> {
                                    width: Fill,
                                    height: Fit,
                                    flow: Down,
                                    spacing: 2,
                                    np_title_lbl = <Label> {
                                        width: Fill,
                                        draw_text: { color: #F4F4F7, text_style: { font_size: 13.0 } }
                                        text: "Ready to play"
                                    }
                                    np_artist_lbl = <Label> {
                                        width: Fill,
                                        draw_text: { color: #9A9CA8, text_style: { font_size: 11.5 } }
                                        text: "Pick a song from Quick picks"
                                    }
                                }
                                np_like_btn = <SonoraIconBtn> {
                                    width: 30, height: 30,
                                    draw_icon: {
                                        svg_path: "M12 21.35L10.55 20.03C5.4 15.36 2 12.28 2 8.5C2 5.42 4.42 3 7.5 3C9.24 3 10.91 3.81 12 5.09C13.09 3.81 14.76 3 16.5 3C19.58 3 22 5.42 22 8.5C22 12.28 18.6 15.36 13.45 20.04L12 21.35Z"
                                        color: #707382
                                    }
                                }
                            }

                            // Center: Single-Row Transport + Inline Seekbar (Exact Sonora Ergonomics!)
                            <View> {
                                width: Fill,
                                height: Fill,
                                flow: Right,
                                align: { x: 0.5, y: 0.5 }
                                spacing: 8,

                                shuffle_btn = <SonoraIconBtn> {
                                    width: 30, height: 30,
                                    draw_icon: { svg_path: "M10.59 9.17L5.41 4L4 5.41L9.17 10.58L10.59 9.17ZM14.5 4L16.54 6.04L4 18.59L5.41 20L17.96 7.46L20 9.5V4H14.5ZM14.83 13.41L13.42 14.82L16.55 17.95L14.5 20H20V14.5L17.96 16.54L14.83 13.41Z" }
                                }
                                prev_btn = <SonoraIconBtn> {
                                    width: 30, height: 30,
                                    draw_icon: { svg_path: "M6 6H8V18H6V6ZM9.5 12L18 18V6L9.5 12Z" }
                                }
                                play_pause_btn = <FerricPlayBtn> {
                                    width: 36, height: 36,
                                }
                                next_btn = <SonoraIconBtn> {
                                    width: 30, height: 30,
                                    draw_icon: { svg_path: "M6 18L14.5 12L6 6V18ZM16 6V18H18V6H16Z" }
                                }
                                repeat_btn = <SonoraIconBtn> {
                                    width: 30, height: 30,
                                    draw_icon: { svg_path: "M7 7H17V10L21 6L17 2V5H5V11H7V7ZM17 17H7V14L3 18L7 22V19H19V13H17V17Z" }
                                }

                                <View> { width: 6, height: 1 }

                                pos_lbl = <Label> {
                                    width: 48,
                                    align: { x: 1.0, y: 0.5 }
                                    draw_text: { color: #9A9CA8, text_style: { font_size: 11.0 } }
                                    text: "00:00"
                                }
                                seek_slider = <FerricBarSlider> {
                                    width: Fill,
                                    height: 20,
                                    min: 0.0, max: 1.0, value: 0.0
                                }
                                dur_btn = <Button> {
                                    width: 48, height: 24, padding: 0,
                                    align: { x: 0.0, y: 0.5 }
                                    text: "00:00"
                                    draw_bg: { fn pixel(self) -> vec4 { return vec4(0.0, 0.0, 0.0, 0.0); } }
                                    draw_text: { color: #9A9CA8, color_hover: #F4F4F7, text_style: { font_size: 11.0 } }
                                }
                            }

                            // Right: Codec Pill + Lyrics + Queue + Volume + Fullscreen
                            <View> {
                                width: 266,
                                height: Fill,
                                flow: Right,
                                align: { x: 1.0, y: 0.5 }
                                spacing: 4,

                                codec_pill_btn = <SonoraBtn> {
                                    height: 24,
                                    padding: { left: 8, right: 8 }
                                    text: "READY"
                                    draw_text: { color: #B4B6C4, color_hover: #FFFFFF, text_style: { font_size: 10.0 } }
                                }
                                bar_lyrics_btn = <SonoraIconBtn> {
                                    width: 28, height: 28,
                                    draw_icon: { svg_path: "M12 14C13.66 14 15 12.66 15 11V5C15 3.34 13.66 2 12 2C10.34 2 9 3.34 9 5V11C9 12.66 10.34 14 12 14ZM17 11C17 13.76 14.76 16 12 16C9.24 16 7 13.76 7 11H5C5 14.53 7.61 17.43 11 17.92V21H13V17.92C16.39 17.43 19 14.53 19 11H17Z" }
                                }
                                bar_queue_btn = <SonoraIconBtn> {
                                    width: 28, height: 28,
                                    draw_icon: { svg_path: "M3 10H14V12H3V10ZM3 6H14V8H3V6ZM3 14H10V16H3V14ZM16 13V21L22 17L16 13Z" }
                                }
                                vol_mute_btn = <SonoraIconBtn> {
                                    width: 28, height: 28,
                                    draw_icon: { svg_path: "M3 9V15H7L12 20V4L7 9H3ZM16.5 12C16.5 10.23 15.48 8.71 14 7.97V16.02C15.48 15.29 16.5 13.77 16.5 12ZM14 3.23V5.29C16.89 6.15 19 8.83 19 12C19 15.17 16.89 17.85 14 18.71V20.77C18.01 19.86 21 16.28 21 12C21 7.72 18.01 4.14 14 3.23Z" }
                                }
                                vol_slider = <FerricBarSlider> {
                                    width: 72,
                                    height: 20,
                                    min: 0.0, max: 1.0, value: 0.85
                                }
                                bar_fullscreen_btn = <SonoraIconBtn> {
                                    width: 28, height: 28,
                                    draw_icon: { svg_path: "M7 14H5V19H10V17H7V14ZM5 10H7V7H10V5H5V10ZM17 17H14V19H19V14H17V17ZM14 5V7H17V10H19V5H14Z" }
                                }
                            }
                        }
                    }

                    // ================================================================
                    // OVERLAY 1: IMMERSIVE FULLSCREEN NOW PLAYING (F11 / Expand)
                    // ================================================================
                    fullscreen_overlay = <View> {
                        visible: false,
                        width: Fill,
                        height: Fill,
                        padding: { left: 36, right: 36, top: 24, bottom: 28 }
                        flow: Down,
                        spacing: 20,
                        show_bg: true,
                        draw_bg: {
                            uniform accent_col: #A882FF
                            fn pixel(self) -> vec4 {
                                let d1 = length((self.pos - vec2(0.24, 0.42)) * vec2(1.1, 1.3));
                                let d2 = length((self.pos - vec2(0.78, 0.65)) * vec2(1.2, 1.4));
                                let glow1 = exp(-d1 * d1 * 2.8) * 0.28;
                                let glow2 = exp(-d2 * d2 * 3.2) * 0.14;
                                let rgb = vec3(0.055, 0.058, 0.075) + self.accent_col.xyz * (glow1 + glow2);
                                return vec4(clamp(rgb, 0.0, 1.0), 0.99);
                            }
                        }

                        // Top Fullscreen Toolbar
                        <View> {
                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                            <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 16.0 } }, text: "Now Playing" }
                            signal_path_lbl = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: { font_size: 11.5 } }, text: "Signal Path: WASAPI 48.0 kHz · 6.0s Bass-Swap" }
                            fs_radio_btn = <SonoraBtn> { height: 32, text: "Start Radio" }
                            fs_video_btn = <SonoraBtn> { height: 32, text: "Watch Video Clip" }
                            fs_close_btn = <SonoraBtn> { height: 32, text: "Close (Esc)" }
                        }

                        // Split Stage: Large Real Cover + Spectrum | Live Synced Karaoke Lyrics
                        <View> {
                            width: Fill, height: Fill, flow: Right, align: { y: 0.5 }, spacing: 36,

                            <View> {
                                width: 380, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 14,
                                fs_art = <FerricCoverArt> {
                                    width: 286,
                                    height: 286,
                                    draw_bg: { radius: 16.0 }
                                }
                                fs_title_lbl = <Label> {
                                    width: Fill,
                                    align: { x: 0.5, y: 0.5 }
                                    draw_text: { color: #F4F4F7, text_style: { font_size: 22.0 } }
                                    text: "No track playing"
                                }
                                fs_artist_lbl = <Label> {
                                    width: Fill,
                                    align: { x: 0.5, y: 0.5 }
                                    draw_text: { color: #A6A9B8, text_style: { font_size: 14.0 } }
                                    text: ""
                                }
                                np_canvas = <FerricNowPlayingCanvas> {
                                    width: Fill,
                                    height: 92,
                                }
                            }

                            <View> {
                                width: Fill, height: Fill, flow: Down, align: { y: 0.5 }, spacing: 14,
                                <View> {
                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                    fs_lyr_source = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: { font_size: 12.0 } }, text: "Synced Lyrics" }
                                    fs_lyr_minus = <SonoraBtn> { height: 28, text: "-0.5s" }
                                    fs_lyr_plus = <SonoraBtn> { height: 28, text: "+0.5s" }
                                    fs_lyr_copy = <SonoraBtn> { height: 28, text: "Copy" }
                                }
                                fs_lyr_0 = <Label> { width: Fill, draw_text: { color: #545768, text_style: { font_size: 15.0 } }, text: "" }
                                fs_lyr_1 = <Label> { width: Fill, draw_text: { color: #7C8094, text_style: { font_size: 17.0 } }, text: "" }
                                fs_lyr_2 = <Label> { width: Fill, draw_text: { color: #A8ACB8, text_style: { font_size: 19.0 } }, text: "" }
                                fs_lyr_3 = <Label> { width: Fill, draw_text: { color: #FFFFFF, text_style: { font_size: 24.0 } }, text: "Play any song to show live synced lyrics" }
                                fs_lyr_4 = <Label> { width: Fill, draw_text: { color: #A8ACB8, text_style: { font_size: 19.0 } }, text: "" }
                                fs_lyr_5 = <Label> { width: Fill, draw_text: { color: #7C8094, text_style: { font_size: 17.0 } }, text: "" }
                                fs_lyr_6 = <Label> { width: Fill, draw_text: { color: #545768, text_style: { font_size: 15.0 } }, text: "" }
                            }
                        }
                    }

                    // ================================================================
                    // OVERLAY 2: POWERBAR COMMAND PALETTE (Ctrl+K)
                    // ================================================================
                    cmdk_modal = <View> {
                        visible: false,
                        width: Fill,
                        height: Fill,
                        align: { x: 0.5, y: 0.20 }
                        show_bg: true,
                        draw_bg: { color: #000000A8 }

                        <SonoraCard> {
                            width: 540,
                            padding: 18,
                            flow: Down,
                            spacing: 12,
                            <View> {
                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: { font_size: 15.0 } }, text: "Ferric Powerbar" }
                                cmdk_close_btn = <SonoraBtn> { height: 26, text: "Esc" }
                            }
                            cmdk_input = <SonoraInput> {
                                height: 38,
                                empty_text: "Type a song/artist and press Enter to search YouTube Music..."
                            }
                            <View> {
                                width: Fill, height: Fit, flow: Right, spacing: 7,
                                cmdk_home_btn = <SonoraBtn> { height: 30, text: "Home" }
                                cmdk_search_btn = <SonoraBtn> { height: 30, text: "Search" }
                                cmdk_songs_btn = <SonoraBtn> { height: 30, text: "Songs" }
                                cmdk_albums_btn = <SonoraBtn> { height: 30, text: "Albums" }
                                cmdk_artists_btn = <SonoraBtn> { height: 30, text: "Artists" }
                                cmdk_playlists_btn = <SonoraBtn> { height: 30, text: "Playlists" }
                            }
                            <View> {
                                width: Fill, height: Fit, flow: Right, spacing: 7,
                                cmdk_eq_btn = <SonoraBtn> { height: 30, text: "Equalizer" }
                                cmdk_fade_btn = <SonoraBtn> { height: 30, text: "Fade-In DSP" }
                                cmdk_google_btn = <SonoraBtn> { height: 30, text: "Google Login" }
                                cmdk_fs_btn = <SonoraBtn> { height: 30, text: "Fullscreen" }
                                cmdk_demo_btn = <SonoraBtn> { height: 30, text: "Offline Demo" }
                            }
                        }
                    }

                    // ================================================================
                    // OVERLAY 3: FLOATING TOAST NOTIFICATION PILL
                    // ================================================================
                    toast_wrap = <View> {
                        visible: false,
                        width: Fill,
                        height: Fill,
                        padding: { right: 20, bottom: 82 }
                        align: { x: 1.0, y: 1.0 }
                        <View> {
                            width: Fit,
                            height: Fit,
                            padding: { left: 14, right: 16, top: 9, bottom: 9 }
                            show_bg: true,
                            draw_bg: {
                                fn pixel(self) -> vec4 {
                                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                    sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 6.0);
                                    sdf.fill_keep(#1D1F2B);
                                    sdf.stroke(#424560, 1.0);
                                    return sdf.result;
                                }
                            }
                            toast_lbl = <Label> {
                                draw_text: { color: #F4F4F7, text_style: { font_size: 12.0 } }
                                text: ""
                            }
                        }
                    }

                    // ================================================================
                    // OVERLAY 4: SHA-256 VERIFIED MSI AUTO-UPDATER POPUP
                    // ================================================================
                    update_popup = <View> {
                        visible: false,
                        width: Fill,
                        height: Fill,
                        align: { x: 0.5, y: 0.30 }
                        show_bg: true,
                        draw_bg: { color: #000000B8 }

                        <SonoraCard> {
                            width: 480,
                            padding: 22,
                            flow: Down,
                            spacing: 12,
                            update_popup_title = <Label> { draw_text: { color: #F4F4F7, text_style: { font_size: 18.0 } }, text: "Ferric Update Available" }
                            update_popup_sub = <Label> { draw_text: { color: #A882FF, text_style: { font_size: 12.0 } }, text: "" }
                            update_popup_notes = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: { font_size: 12.0 } }, text: "" }
                            <View> {
                                width: Fill, height: Fit, margin: { top: 8 }, flow: Right, spacing: 8,
                                update_now_btn = <SonoraPrimaryBtn> { text: "Download & Install" }
                                update_whats_new_btn = <SonoraBtn> { height: 34, text: "Release Notes" }
                                update_popup_close_btn = <SonoraBtn> { height: 34, text: "Later" }
                            }
                        }
                    }
                }
            }
        }
    }
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
    artwork: Option<ArtworkCache>,
    #[rust]
    artwork_snapshot: Rc<HashMap<String, CachedArtwork>>,
    #[rust]
    all_tracks: Vec<Track>,
    #[rust]
    filtered_tracks: Rc<Vec<Track>>,
    #[rust]
    recents_tracks: Rc<Vec<Track>>,
    #[rust]
    playlists: Vec<Playlist>,
    #[rust]
    album_groups: Vec<CollectionGroup>,
    #[rust]
    artist_groups: Vec<CollectionGroup>,
    #[rust]
    rich_home: YtmRichHome,
    #[rust]
    active_mood_idx: usize,
    #[rust]
    qp_page: usize,
    #[rust]
    shelf_pages: [usize; 3],
    #[rust]
    albums_page: usize,
    #[rust]
    artists_page: usize,
    #[rust]
    playlists_page: usize,
    #[rust]
    search_result: YtmRichSearch,
    #[rust]
    search_tracks: Rc<Vec<Track>>,
    #[rust]
    search_filter: SearchFilter,
    #[rust]
    detail_state: DetailViewState,
    #[rust]
    detail_tracks: Rc<Vec<Track>>,
    #[rust]
    queue: PlayQueue,
    #[rust]
    queue_context: String,
    #[rust]
    last_radio_seed_id: Option<String>,
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
    pending_stream_track_id: Option<String>,
    #[rust]
    prefetch_stream_track_id: Option<String>,
    #[rust]
    ytm_signed_out_recently: bool,
    #[rust]
    startup_auto_played: bool,
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
    active_filter: LibraryFilter,
    #[rust]
    sort_field: SortField,
    #[rust]
    active_page: MainPage,
    #[rust]
    active_settings_tab: SettingsTab,
    #[rust]
    nav_history: Option<NavHistory>,
    #[rust]
    left_sidebar_open: bool,
    #[rust]
    right_sidebar_open: bool,
    #[rust]
    right_lyrics_tab: bool,
    #[rust]
    fullscreen_open: bool,
    #[rust]
    show_remaining_time: bool,
    #[rust]
    pre_mute_volume: f32,
    #[rust]
    compact_mini_mode: bool,
    #[rust]
    cmdk_open: bool,
    #[rust]
    toast_until: Option<Instant>,
    #[rust]
    canvas_phase: f32,
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

    fn sync_maximized_chrome_padding(&self, cx: &mut Cx, is_max: bool) {
        let right_pad = if is_max { 20.0 } else { 8.0 };
        self.ui
            .view(id!(top_bar))
            .apply_over(cx, live! { padding: { left: 10.0, right: (right_pad) } });
    }

    fn current_accent(&self) -> Vec4 {
        if self.theme.adaptive_album_art
            && let Some(cur) = self.queue.current()
            && let Some(art_cache) = &self.artwork
            && let Some(cached) = art_cache.get_for_track(cur)
        {
            return cached.accent;
        }
        rgba_vec4(&self.theme.accent_hex)
    }

    fn show_toast(&mut self, cx: &mut Cx, message: &str) {
        self.toast_until = Some(Instant::now() + Duration::from_secs(3));
        self.ui.label(id!(toast_lbl)).set_text(cx, message);
        self.ui.view(id!(toast_wrap)).set_visible(cx, true);
        self.ui.redraw(cx);
    }

    fn ensure_initialized(&mut self, cx: &mut Cx) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        self.active_page = MainPage::Home;
        self.active_settings_tab = SettingsTab::Accounts;
        self.nav_history = Some(NavHistory::new(MainPage::Home));
        self.left_sidebar_open = true;
        self.right_sidebar_open = true;
        self.right_lyrics_tab = false;
        self.pre_mute_volume = 0.85;
        self.queue_context = "Quick picks".to_string();
        self.artwork = Some(ArtworkCache::new());
        self.hide_native_makepad_caption(cx);

        let db_path = app_data_dir().join("ferric.db");
        let db = LibraryDb::open(&db_path)
            .or_else(|_| LibraryDb::open_in_memory())
            .ok();

        if let Some(store) = &db {
            self.theme = store.load_theme().unwrap_or_default();
            // Upgrade pre-Sonora legacy orange/glass configs cleanly to Sonora Midnight
            if self.theme.name != "Sonora Midnight"
                && self.theme.name != "Pure AMOLED"
                && self.theme.name != "Warm Cream Light"
            {
                self.theme = ThemeConfig::default();
            }
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
        self.rebuild_library_views(cx);
        if !self.all_tracks.is_empty() {
            self.queue.set_tracks(self.all_tracks.clone(), None);
        }
        self.sync_controls_from_state(cx);
        self.apply_theme_live(cx);
        self.refresh_home_ui(cx);
        self.refresh_right_sidebar(cx);
        self.refresh_storage_stats(cx);
        self.tick_timer = cx.start_interval(0.10);

        if startup_load_demo() {
            self.load_and_play_demo_suite(cx);
        } else {
            self.spawn_ytm_rich_home(cx, 0, None);
            if startup_play_first()
                && !self.all_tracks.is_empty()
                && let Some((idx, first)) = self
                    .all_tracks
                    .iter()
                    .enumerate()
                    .find(|(_, t)| {
                        t.path.as_ref().is_some_and(|p| p.is_file())
                            || (t.duration_ms > 30_000 && t.youtube_video_id.is_some())
                    })
                    .map(|(i, t)| (i, t.clone()))
            {
                self.startup_auto_played = true;
                self.queue.set_tracks(self.all_tracks.clone(), Some(idx));
                self.play_track(cx, first);
            }
        }

        if let Some(page_arg) = startup_page_override() {
            self.apply_startup_page_override(cx, page_arg);
        } else if self.update_settings.auto_check_on_startup {
            self.spawn_update_check(cx, false);
        }
    }

    fn apply_startup_page_override(&mut self, cx: &mut Cx, arg: &str) {
        match arg {
            "home" => self.navigate_to(cx, MainPage::Home, false),
            "search" => self.navigate_to(cx, MainPage::Search, false),
            "recents" => self.navigate_to(cx, MainPage::Recents, false),
            "songs" | "library" => self.navigate_to(cx, MainPage::Songs, false),
            "albums" => self.navigate_to(cx, MainPage::Albums, false),
            "artists" => self.navigate_to(cx, MainPage::Artists, false),
            "playlists" => self.navigate_to(cx, MainPage::Playlists, false),
            "settings" | "accounts" => {
                self.navigate_to(cx, MainPage::Settings, false);
                self.switch_settings_tab(cx, SettingsTab::Accounts);
            }
            "fade" | "playback" => {
                self.navigate_to(cx, MainPage::Settings, false);
                self.switch_settings_tab(cx, SettingsTab::Playback);
            }
            "eq" => {
                self.navigate_to(cx, MainPage::Settings, false);
                self.switch_settings_tab(cx, SettingsTab::Equalizer);
            }
            "appearance" => {
                self.navigate_to(cx, MainPage::Settings, false);
                self.switch_settings_tab(cx, SettingsTab::Appearance);
            }
            "storage" => {
                self.navigate_to(cx, MainPage::Settings, false);
                self.switch_settings_tab(cx, SettingsTab::Storage);
            }
            "connect" => {
                self.navigate_to(cx, MainPage::Settings, false);
                self.switch_settings_tab(cx, SettingsTab::Connect);
            }
            "now" | "lyrics" | "fullscreen" => {
                self.set_fullscreen(cx, true);
            }
            "glass" => {
                self.theme.material.liquid_glass = true;
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
                self.navigate_to(cx, MainPage::Settings, false);
                self.switch_settings_tab(cx, SettingsTab::Appearance);
            }
            "amoled" | "oled" => {
                self.theme = ThemeConfig::pure_amoled();
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
            }
            "cmdk" => {
                self.cmdk_open = true;
                self.ui.view(id!(cmdk_modal)).set_visible(cx, true);
                self.ui.redraw(cx);
            }
            _ => {}
        }
    }

    fn navigate_to(&mut self, cx: &mut Cx, target: MainPage, push_history: bool) {
        self.active_page = target;
        if push_history && let Some(hist) = &mut self.nav_history {
            hist.push(target);
        }

        let pages = [
            (MainPage::Home, id!(page_home)),
            (MainPage::Search, id!(page_search)),
            (MainPage::Recents, id!(page_recents)),
            (MainPage::Songs, id!(page_songs)),
            (MainPage::Albums, id!(page_albums)),
            (MainPage::Artists, id!(page_artists)),
            (MainPage::Playlists, id!(page_playlists)),
            (MainPage::Detail, id!(page_detail)),
            (MainPage::Settings, id!(page_settings)),
        ];
        for (p, vid) in pages {
            self.ui.view(vid).set_visible(cx, p == target);
        }

        let bright = vec4(0.957, 0.957, 0.969, 1.0);
        let muted = vec4(0.635, 0.647, 0.706, 1.0);
        let icon_muted = vec4(0.533, 0.545, 0.600, 1.0);
        let nav_map = [
            (MainPage::Home, id!(nav_home)),
            (MainPage::Search, id!(nav_search)),
            (MainPage::Recents, id!(nav_recents)),
            (MainPage::Songs, id!(nav_songs)),
            (MainPage::Albums, id!(nav_albums)),
            (MainPage::Artists, id!(nav_artists)),
            (MainPage::Playlists, id!(nav_playlists)),
            (MainPage::Settings, id!(nav_settings)),
        ];
        for (p, btn_id) in nav_map {
            let active = p == target;
            self.ui.button(btn_id).apply_over(
                cx,
                live! {
                    draw_bg: { active: (if active { 1.0 } else { 0.0 }) }
                    draw_icon: { color: (if active { bright } else { icon_muted }) }
                    draw_text: { color: (if active { bright } else { muted }) }
                },
            );
        }

        let can_b = self.nav_history.as_ref().is_some_and(NavHistory::can_back);
        let can_f = self
            .nav_history
            .as_ref()
            .is_some_and(NavHistory::can_forward);
        self.ui.button(id!(nav_back_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if can_b { bright } else { vec4(0.32, 0.34, 0.40, 1.0) }) } },
        );
        self.ui.button(id!(nav_fwd_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if can_f { bright } else { vec4(0.32, 0.34, 0.40, 1.0) }) } },
        );

        match target {
            MainPage::Albums => self.refresh_albums_grid(cx),
            MainPage::Artists => self.refresh_artists_grid(cx),
            MainPage::Playlists => self.refresh_playlists_grid(cx),
            MainPage::Search => {
                if self.search_tracks.is_empty() {
                    self.search_tracks = Rc::new(self.home_quick_picks_pool());
                }
                self.refresh_search_ui(cx);
            }
            MainPage::Settings => self.refresh_storage_stats(cx),
            _ => {}
        }
        self.ui.redraw(cx);
    }

    fn switch_settings_tab(&mut self, cx: &mut Cx, tab: SettingsTab) {
        self.active_settings_tab = tab;
        let dark = vec4(0.055, 0.059, 0.075, 1.0);
        let light = vec4(0.784, 0.792, 0.839, 1.0);
        let defs = [
            (
                SettingsTab::Accounts,
                id!(tab_accounts),
                id!(st_accounts_btn),
            ),
            (
                SettingsTab::Playback,
                id!(tab_playback),
                id!(st_playback_btn),
            ),
            (SettingsTab::Equalizer, id!(tab_eq), id!(st_eq_btn)),
            (
                SettingsTab::Appearance,
                id!(tab_appearance),
                id!(st_appearance_btn),
            ),
            (SettingsTab::Storage, id!(tab_storage), id!(st_storage_btn)),
            (SettingsTab::Connect, id!(tab_connect), id!(st_connect_btn)),
        ];
        for (t, vid, bid) in defs {
            let active = t == tab;
            self.ui.view(vid).set_visible(cx, active);
            self.ui.button(bid).apply_over(
                cx,
                live! {
                    draw_bg: { active: (if active { 1.0 } else { 0.0 }) }
                    draw_text: { color: (if active { dark } else { light }) }
                },
            );
        }
        if tab == SettingsTab::Storage {
            self.refresh_storage_stats(cx);
        }
        self.ui.redraw(cx);
    }

    fn set_right_sidebar(&mut self, cx: &mut Cx, open: bool, lyrics_tab: bool) {
        self.right_sidebar_open = open;
        self.right_lyrics_tab = lyrics_tab;
        self.ui.view(id!(right_sidebar)).set_visible(cx, open);
        self.ui.view(id!(right_divider)).set_visible(cx, open);
        self.ui
            .view(id!(rs_queue_view))
            .set_visible(cx, !lyrics_tab);
        self.ui
            .view(id!(rs_lyrics_view))
            .set_visible(cx, lyrics_tab);

        let dark = vec4(0.055, 0.059, 0.075, 1.0);
        let light = vec4(0.784, 0.792, 0.839, 1.0);
        self.ui.button(id!(rs_tab_queue)).apply_over(
            cx,
            live! {
                draw_bg: { active: (if !lyrics_tab { 1.0 } else { 0.0 }) }
                draw_text: { color: (if !lyrics_tab { dark } else { light }) }
            },
        );
        self.ui.button(id!(rs_tab_lyrics)).apply_over(
            cx,
            live! {
                draw_bg: { active: (if lyrics_tab { 1.0 } else { 0.0 }) }
                draw_text: { color: (if lyrics_tab { dark } else { light }) }
            },
        );
        let accent = self.current_accent();
        let muted = vec4(0.604, 0.612, 0.659, 1.0);
        self.ui.button(id!(toggle_right_btn)).apply_over(
            cx,
            live! {
                draw_bg: { active: (if open { 1.0 } else { 0.0 }) }
                draw_icon: { color: (if open { vec4(0.96, 0.96, 0.97, 1.0) } else { muted }) }
            },
        );
        self.ui.button(id!(bar_lyrics_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if open && lyrics_tab { accent } else { muted }) } },
        );
        self.ui.button(id!(bar_queue_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if open && !lyrics_tab { accent } else { muted }) } },
        );
        self.ui.redraw(cx);
    }

    fn set_fullscreen(&mut self, cx: &mut Cx, open: bool) {
        self.fullscreen_open = open;
        self.ui.view(id!(fullscreen_overlay)).set_visible(cx, open);
        self.ui.redraw(cx);
    }

    fn toggle_compact_mini_mode(&mut self, cx: &mut Cx) {
        self.compact_mini_mode = !self.compact_mini_mode;
        let show_full = !self.compact_mini_mode;
        self.ui.view(id!(workspace_row)).set_visible(cx, show_full);
        let new_size = if self.compact_mini_mode {
            dvec2(520.0, 128.0)
        } else {
            dvec2(1280.0, 780.0)
        };
        self.ui.window(id!(main_window)).resize(cx, new_size);
        self.ui.redraw(cx);
    }

    fn active_track_list(&self) -> Rc<Vec<Track>> {
        match self.active_page {
            MainPage::Search => Rc::clone(&self.search_tracks),
            MainPage::Recents => Rc::clone(&self.recents_tracks),
            MainPage::Detail => Rc::clone(&self.detail_tracks),
            _ => Rc::clone(&self.filtered_tracks),
        }
    }

    fn request_artwork_for_tracks(&mut self, tracks: &[Track]) {
        if let Some(cache) = &mut self.artwork {
            for t in tracks.iter().take(48) {
                let _ = cache.request_for_track(t);
            }
        }
    }

    fn request_artwork_for_cards(&mut self, cards: &[YtmShelfCard]) {
        if let Some(cache) = &mut self.artwork {
            for c in cards.iter().take(30) {
                if let Some(url) = c.thumbnail_url.as_deref() {
                    let _ = cache.request_url(url);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_cover_art_url(
        &self,
        cx: &mut Cx,
        cover_ref: FerricCoverArtRef,
        url_opt: Option<&str>,
        track_opt: Option<&Track>,
        playing: bool,
        level: f32,
        circle: bool,
    ) {
        let accent = self.current_accent();
        let key_opt = track_opt.and_then(ArtworkCache::key_for_track).or_else(|| {
            url_opt
                .map(normalize_ytm_thumbnail_url)
                .filter(|s| !s.is_empty())
        });
        if let Some(key) = key_opt
            && let Some(cache) = &self.artwork
            && let Some(cached) = cache.get(&key)
        {
            cover_ref.set_texture(cx, Some(cached.texture.clone()), cached.aspect);
        } else {
            cover_ref.set_texture(cx, None, 1.0);
        }
        cover_ref.set_state(cx, playing, level, accent, circle);
    }

    fn refresh_all_artwork_slots(&mut self, cx: &mut Cx) {
        if let Some(cache) = &self.artwork {
            self.artwork_snapshot = Rc::new(cache.snapshot());
        }
        self.apply_adaptive_accent_live(cx);
        self.refresh_home_ui(cx);
        self.refresh_right_sidebar(cx);
        self.refresh_detail_hero(cx);
        if self.active_page == MainPage::Albums {
            self.refresh_albums_grid(cx);
        } else if self.active_page == MainPage::Artists {
            self.refresh_artists_grid(cx);
        } else if self.active_page == MainPage::Playlists {
            self.refresh_playlists_grid(cx);
        } else if self.active_page == MainPage::Search {
            self.refresh_search_ui(cx);
        }
        let is_playing = self
            .audio
            .as_ref()
            .is_some_and(|a| a.telemetry().is_playing);
        if let Some(cur) = self.queue.current().cloned() {
            self.bind_cover_art_url(
                cx,
                self.ui.ferric_cover_art(id!(np_art)),
                cur.thumbnail_url.as_deref(),
                Some(&cur),
                is_playing,
                0.55,
                false,
            );
            self.bind_cover_art_url(
                cx,
                self.ui.ferric_cover_art(id!(fs_art)),
                cur.thumbnail_url.as_deref(),
                Some(&cur),
                false,
                0.55,
                false,
            );
        }
        self.ui.redraw(cx);
    }

    fn apply_adaptive_accent_live(&self, cx: &mut Cx) {
        let accent = self.current_accent();
        self.ui.view(id!(bottom_bar)).apply_over(
            cx,
            live! {
                draw_bg: {
                    accent_col: (accent),
                    glow_amt: (if self.queue.current().is_some() { 0.28 } else { 0.14 })
                }
            },
        );
        self.ui
            .view(id!(fullscreen_overlay))
            .apply_over(cx, live! { draw_bg: { accent_col: (accent) } });
        self.ui
            .ferric_bar_slider(id!(seek_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        self.ui
            .ferric_bar_slider(id!(vol_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        self.ui
            .view(id!(q_now_row))
            .apply_over(cx, live! { draw_bg: { accent_col: (accent) } });
        self.ui
            .label(id!(q_now_header))
            .apply_over(cx, live! { draw_text: { color: (accent) } });
        self.ui
            .label(id!(rs_lyr_3))
            .apply_over(cx, live! { draw_text: { color: (accent) } });
        self.ui
            .view(id!(np_canvas))
            .apply_over(cx, live! { draw_bg: { accent: (accent) } });
        self.ui
            .ferric_eq_graph(id!(eq_graph))
            .apply_over(cx, live! { draw_bg: { accent: (accent) } });
    }

    fn rebuild_library_views(&mut self, cx: &mut Cx) {
        let mut list: Vec<Track> = match self.active_filter {
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
        };

        match self.sort_field {
            SortField::Artist => list.sort_by(|a, b| {
                a.artist
                    .to_ascii_lowercase()
                    .cmp(&b.artist.to_ascii_lowercase())
                    .then_with(|| a.title.cmp(&b.title))
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

        let total_ms: u64 = list.iter().map(|t| t.duration_ms).sum();
        let mins = total_ms / 60_000;
        let xfade_txt = if self.dsp.crossfade_ms == 0 {
            "0ms Gapless".to_string()
        } else {
            format!(
                "{:.1}s {}",
                self.dsp.crossfade_ms as f32 / 1_000.0,
                self.dsp.fade_curve.label()
            )
        };
        self.ui.label(id!(songs_sub_lbl)).set_text(
            cx,
            &format!("{} songs · {} min · {}", list.len(), mins, xfade_txt),
        );

        let dark = vec4(0.055, 0.059, 0.075, 1.0);
        let light = vec4(0.784, 0.792, 0.839, 1.0);
        for (btn, active) in [
            (id!(flt_all_btn), self.active_filter == LibraryFilter::All),
            (
                id!(flt_liked_btn),
                self.active_filter == LibraryFilter::Liked,
            ),
            (
                id!(flt_local_btn),
                self.active_filter == LibraryFilter::Local,
            ),
            (
                id!(flt_spotify_btn),
                self.active_filter == LibraryFilter::Spotify,
            ),
            (
                id!(flt_youtube_btn),
                self.active_filter == LibraryFilter::YouTube,
            ),
        ] {
            self.ui.button(btn).apply_over(
                cx,
                live! {
                    draw_bg: { active: (if active { 1.0 } else { 0.0 }) }
                    draw_text: { color: (if active { dark } else { light }) }
                },
            );
        }
        self.ui
            .button(id!(sort_cycle_btn))
            .set_text(cx, self.sort_field.label());

        self.filtered_tracks = Rc::new(list);

        // Recents chronological list
        let recents: Vec<Track> = self
            .session_state
            .recent_track_ids
            .iter()
            .filter_map(|id| self.all_tracks.iter().find(|t| &t.id == id).cloned())
            .collect();
        self.ui.label(id!(recents_sub_lbl)).set_text(
            cx,
            &format!(
                "{} recently played · {} total plays · {} min listened",
                recents.len(),
                self.session_state.total_tracks_played,
                self.session_state.total_listened_secs / 60
            ),
        );
        self.recents_tracks = Rc::new(recents);

        // Album & Artist groups
        self.album_groups = build_album_groups(&self.all_tracks);
        self.artist_groups = build_artist_groups(&self.all_tracks);
        self.request_artwork_for_tracks(&self.all_tracks.clone());

        // Sidebar Pinned slots
        let pin_slots = [
            id!(pin_slot_0),
            id!(pin_slot_1),
            id!(pin_slot_2),
            id!(pin_slot_3),
            id!(pin_slot_4),
        ];
        for (i, slot) in pin_slots.into_iter().enumerate() {
            if let Some(pl) = self.playlists.get(i) {
                self.ui.button(slot).set_visible(cx, true);
                self.ui.button(slot).set_text(cx, &clip_text(&pl.name, 22));
            } else {
                self.ui.button(slot).set_visible(cx, false);
            }
        }

        // Top account status
        let yt_ok = self.accounts.youtube_connected && !self.accounts.youtube_sapisid.is_empty();
        let top_acct = if yt_ok {
            let n = self.accounts.youtube_channel_name.trim();
            if n.is_empty() {
                "Google".to_string()
            } else {
                clip_text(n, 16)
            }
        } else {
            "Sign in".to_string()
        };
        self.ui.button(id!(top_account_btn)).set_text(cx, &top_acct);
        self.ui.label(id!(yt_state_lbl)).set_text(
            cx,
            &if yt_ok {
                format!(
                    "Signed in as {} ({} playlists)",
                    self.accounts.youtube_channel_name, self.accounts.youtube_playlist_count
                )
            } else {
                "Guest Mode (Optional)".to_string()
            },
        );
    }

    fn home_quick_picks_pool(&self) -> Vec<Track> {
        if !self.rich_home.quick_picks.is_empty() {
            return self.rich_home.quick_picks.clone();
        }
        self.all_tracks.clone()
    }

    fn refresh_home_ui(&self, cx: &mut Cx) {
        let dark = vec4(0.055, 0.059, 0.075, 1.0);
        let light = vec4(0.784, 0.792, 0.839, 1.0);
        let chip_ids = [
            id!(chip_0),
            id!(chip_1),
            id!(chip_2),
            id!(chip_3),
            id!(chip_4),
            id!(chip_5),
            id!(chip_6),
            id!(chip_7),
            id!(chip_8),
        ];
        for (i, cid) in chip_ids.into_iter().enumerate() {
            let label = if i == 0 {
                Some("All".to_string())
            } else {
                self.rich_home.chips.get(i - 1).map(|c| c.title.clone())
            };
            if let Some(txt) = label {
                self.ui.button(cid).set_visible(cx, true);
                self.ui.button(cid).set_text(cx, &txt);
                let active = self.active_mood_idx == i;
                self.ui.button(cid).apply_over(
                    cx,
                    live! {
                        draw_bg: { active: (if active { 1.0 } else { 0.0 }) }
                        draw_text: { color: (if active { dark } else { light }) }
                    },
                );
            } else if i > 0 && !self.rich_home.chips.is_empty() {
                self.ui.button(cid).set_visible(cx, false);
            }
        }

        // Quick Picks 2x4 grid
        let pool = self.home_quick_picks_pool();
        let active_id = self.queue.current().map(|t| t.id.as_str());
        let is_playing = self
            .audio
            .as_ref()
            .is_some_and(|a| a.telemetry().is_playing);
        let start = self.qp_page * 8;
        let qp_ids = [
            id!(qp_0),
            id!(qp_1),
            id!(qp_2),
            id!(qp_3),
            id!(qp_4),
            id!(qp_5),
            id!(qp_6),
            id!(qp_7),
        ];
        for (slot, vid) in qp_ids.into_iter().enumerate() {
            let v = self.ui.view(vid);
            if let Some(track) = pool.get(start + slot) {
                v.set_visible(cx, true);
                let is_act = active_id == Some(track.id.as_str());
                v.apply_over(
                    cx,
                    live! { draw_bg: { active: (if is_act { 1.0 } else { 0.0 }) } },
                );
                v.label(id!(title))
                    .set_text(cx, &clip_text(&track.title, 28));
                v.label(id!(sub)).set_text(
                    cx,
                    &clip_text(&format!("{} · {}", track.artist, track.album), 34),
                );
                self.bind_cover_art_url(
                    cx,
                    v.ferric_cover_art(id!(art)),
                    track.thumbnail_url.as_deref(),
                    Some(track),
                    is_act && is_playing,
                    0.55,
                    false,
                );
            } else {
                v.set_visible(cx, false);
            }
        }

        // 3 Horizontal Real-Cover Shelves
        let shelf_defs = [
            (
                0usize,
                id!(shelf0_wrap),
                id!(sh0_title_lbl),
                [
                    id!(sh0_c0),
                    id!(sh0_c1),
                    id!(sh0_c2),
                    id!(sh0_c3),
                    id!(sh0_c4),
                ],
            ),
            (
                1usize,
                id!(shelf1_wrap),
                id!(sh1_title_lbl),
                [
                    id!(sh1_c0),
                    id!(sh1_c1),
                    id!(sh1_c2),
                    id!(sh1_c3),
                    id!(sh1_c4),
                ],
            ),
            (
                2usize,
                id!(shelf2_wrap),
                id!(sh2_title_lbl),
                [
                    id!(sh2_c0),
                    id!(sh2_c1),
                    id!(sh2_c2),
                    id!(sh2_c3),
                    id!(sh2_c4),
                ],
            ),
        ];

        for (si, wrap_id, title_id, card_ids) in shelf_defs {
            if let Some(shelf) = self.rich_home.shelves.get(si)
                && !shelf.cards.is_empty()
            {
                self.ui.view(wrap_id).set_visible(cx, true);
                self.ui.label(title_id).set_text(cx, &shelf.title);
                let offset = self.shelf_pages[si] * 5;
                for (ci, cid) in card_ids.into_iter().enumerate() {
                    let card_v = self.ui.view(cid);
                    if let Some(card) = shelf.cards.get(offset + ci) {
                        card_v.set_visible(cx, true);
                        card_v
                            .label(id!(title))
                            .set_text(cx, &clip_text(&card.title, 16));
                        card_v
                            .label(id!(sub))
                            .set_text(cx, &clip_text(&card.subtitle, 19));
                        self.bind_cover_art_url(
                            cx,
                            card_v.ferric_cover_art(id!(art)),
                            card.thumbnail_url.as_deref(),
                            None,
                            false,
                            0.5,
                            card.kind == YtmCardKind::Artist,
                        );
                    } else {
                        card_v.set_visible(cx, false);
                    }
                }
            } else {
                self.ui.view(wrap_id).set_visible(cx, false);
            }
        }
    }

    fn refresh_right_sidebar(&self, cx: &mut Cx) {
        let is_playing = self
            .audio
            .as_ref()
            .is_some_and(|a| a.telemetry().is_playing);
        let accent = self.current_accent();

        // History (last 3 played tracks excluding current)
        let cur_id = self.queue.current().map(|t| t.id.as_str());
        let hist: Vec<Track> = self
            .session_state
            .recent_track_ids
            .iter()
            .filter(|id| Some(id.as_str()) != cur_id)
            .filter_map(|id| self.all_tracks.iter().find(|t| &t.id == id).cloned())
            .take(3)
            .collect();
        self.ui
            .view(id!(qh_header))
            .set_visible(cx, !hist.is_empty());
        for (i, rid) in [id!(qh_0), id!(qh_1), id!(qh_2)].into_iter().enumerate() {
            let rv = self.ui.view(rid);
            if let Some(t) = hist.get(i) {
                rv.set_visible(cx, true);
                rv.label(id!(title)).set_text(cx, &clip_text(&t.title, 17));
                rv.label(id!(sub)).set_text(cx, &clip_text(&t.artist, 19));
                rv.label(id!(dur)).set_text(
                    cx,
                    &if t.duration_ms > 0 {
                        format_duration_ms(t.duration_ms)
                    } else {
                        String::new()
                    },
                );
                self.bind_cover_art_url(
                    cx,
                    rv.ferric_cover_art(id!(art)),
                    t.thumbnail_url.as_deref(),
                    Some(t),
                    false,
                    0.4,
                    false,
                );
            } else {
                rv.set_visible(cx, false);
            }
        }

        // Now playing
        self.ui.label(id!(q_now_header)).set_text(
            cx,
            &format!("Now playing · {}", clip_text(&self.queue_context, 20)),
        );
        let now_v = self.ui.view(id!(q_now_row));
        if let Some(cur) = self.queue.current() {
            now_v.set_visible(cx, true);
            now_v
                .label(id!(title))
                .set_text(cx, &clip_text(&cur.title, 17));
            now_v
                .label(id!(title))
                .apply_over(cx, live! { draw_text: { color: (accent) } });
            now_v
                .label(id!(sub))
                .set_text(cx, &clip_text(&cur.artist, 19));
            now_v.label(id!(dur)).set_text(
                cx,
                &if cur.duration_ms > 0 {
                    format_duration_ms(cur.duration_ms)
                } else {
                    String::new()
                },
            );
            self.bind_cover_art_url(
                cx,
                now_v.ferric_cover_art(id!(art)),
                cur.thumbnail_url.as_deref(),
                Some(cur),
                is_playing,
                0.6,
                false,
            );
        } else {
            now_v.set_visible(cx, true);
            now_v.label(id!(title)).set_text(cx, "Nothing playing");
            now_v
                .label(id!(sub))
                .set_text(cx, "Click any song to listen");
            now_v.label(id!(dur)).set_text(cx, "");
            now_v.ferric_cover_art(id!(art)).set_texture(cx, None, 1.0);
        }

        // Up next (10 rows)
        let upcoming = self.queue.peek_upcoming(10);
        self.ui
            .label(id!(q_upnext_header))
            .set_text(cx, &format!("Up next ({})", upcoming.len()));
        self.ui.label(id!(q_radio_status)).set_text(
            cx,
            if self.session_state.autoplay_radio {
                "Radio On"
            } else {
                "Radio Off"
            },
        );
        self.ui.button(id!(rs_radio_btn)).apply_over(
            cx,
            live! {
                draw_bg: { active: (if self.session_state.autoplay_radio { 1.0 } else { 0.0 }) }
                draw_icon: { color: (if self.session_state.autoplay_radio { accent } else { vec4(0.55, 0.56, 0.62, 1.0) }) }
            },
        );

        let qu_ids = [
            id!(qu_0),
            id!(qu_1),
            id!(qu_2),
            id!(qu_3),
            id!(qu_4),
            id!(qu_5),
            id!(qu_6),
            id!(qu_7),
            id!(qu_8),
            id!(qu_9),
        ];
        for (i, rid) in qu_ids.into_iter().enumerate() {
            let rv = self.ui.view(rid);
            if let Some(t) = upcoming.get(i) {
                rv.set_visible(cx, true);
                rv.label(id!(title)).set_text(cx, &clip_text(&t.title, 17));
                rv.label(id!(sub)).set_text(cx, &clip_text(&t.artist, 19));
                rv.label(id!(dur)).set_text(
                    cx,
                    &if t.duration_ms > 0 {
                        format_duration_ms(t.duration_ms)
                    } else {
                        String::new()
                    },
                );
                self.bind_cover_art_url(
                    cx,
                    rv.ferric_cover_art(id!(art)),
                    t.thumbnail_url.as_deref(),
                    Some(t),
                    false,
                    0.4,
                    false,
                );
            } else {
                rv.set_visible(cx, false);
            }
        }
    }

    fn refresh_albums_grid(&self, cx: &mut Cx) {
        self.ui.label(id!(albums_sub_lbl)).set_text(
            cx,
            &format!("{} albums in your library", self.album_groups.len()),
        );
        let start = self.albums_page * 8;
        let ids = [
            id!(alb_0),
            id!(alb_1),
            id!(alb_2),
            id!(alb_3),
            id!(alb_4),
            id!(alb_5),
            id!(alb_6),
            id!(alb_7),
        ];
        for (i, vid) in ids.into_iter().enumerate() {
            let v = self.ui.view(vid);
            if let Some(g) = self.album_groups.get(start + i) {
                v.set_visible(cx, true);
                v.label(id!(title)).set_text(cx, &clip_text(&g.title, 22));
                v.label(id!(sub)).set_text(cx, &clip_text(&g.subtitle, 24));
                self.bind_cover_art_url(
                    cx,
                    v.ferric_cover_art(id!(art)),
                    g.thumbnail_url.as_deref(),
                    g.tracks.first(),
                    false,
                    0.4,
                    false,
                );
            } else {
                v.set_visible(cx, false);
            }
        }
    }

    fn refresh_artists_grid(&self, cx: &mut Cx) {
        self.ui.label(id!(artists_sub_lbl)).set_text(
            cx,
            &format!("{} artists in your library", self.artist_groups.len()),
        );
        let start = self.artists_page * 8;
        let ids = [
            id!(art_0),
            id!(art_1),
            id!(art_2),
            id!(art_3),
            id!(art_4),
            id!(art_5),
            id!(art_6),
            id!(art_7),
        ];
        for (i, vid) in ids.into_iter().enumerate() {
            let v = self.ui.view(vid);
            if let Some(g) = self.artist_groups.get(start + i) {
                v.set_visible(cx, true);
                v.label(id!(title)).set_text(cx, &clip_text(&g.title, 22));
                v.label(id!(sub)).set_text(cx, &clip_text(&g.subtitle, 24));
                self.bind_cover_art_url(
                    cx,
                    v.ferric_cover_art(id!(art)),
                    g.thumbnail_url.as_deref(),
                    g.tracks.first(),
                    false,
                    0.4,
                    true,
                );
            } else {
                v.set_visible(cx, false);
            }
        }
    }

    fn refresh_playlists_grid(&self, cx: &mut Cx) {
        let start = self.playlists_page * 8;
        let ids = [
            id!(plc_0),
            id!(plc_1),
            id!(plc_2),
            id!(plc_3),
            id!(plc_4),
            id!(plc_5),
            id!(plc_6),
            id!(plc_7),
        ];
        for (i, vid) in ids.into_iter().enumerate() {
            let v = self.ui.view(vid);
            if let Some(pl) = self.playlists.get(start + i) {
                v.set_visible(cx, true);
                v.label(id!(title)).set_text(cx, &clip_text(&pl.name, 22));
                v.label(id!(sub)).set_text(
                    cx,
                    &format!("{} · {} songs", pl.source.as_badge(), pl.track_count),
                );
                self.bind_cover_art_url(
                    cx,
                    v.ferric_cover_art(id!(art)),
                    pl.thumbnail_url.as_deref(),
                    None,
                    false,
                    0.4,
                    false,
                );
            } else {
                v.set_visible(cx, false);
            }
        }
    }

    fn refresh_search_ui(&self, cx: &mut Cx) {
        let dark = vec4(0.055, 0.059, 0.075, 1.0);
        let light = vec4(0.784, 0.792, 0.839, 1.0);
        for (btn, f) in [
            (id!(sf_all_btn), SearchFilter::All),
            (id!(sf_songs_btn), SearchFilter::Songs),
            (id!(sf_albums_btn), SearchFilter::Albums),
            (id!(sf_artists_btn), SearchFilter::Artists),
            (id!(sf_playlists_btn), SearchFilter::Playlists),
            (id!(sf_local_btn), SearchFilter::Local),
        ] {
            let active = self.search_filter == f;
            self.ui.button(btn).apply_over(
                cx,
                live! {
                    draw_bg: { active: (if active { 1.0 } else { 0.0 }) }
                    draw_text: { color: (if active { dark } else { light }) }
                },
            );
        }

        // Recent searches chips
        let has_recent = !self.session_state.recent_searches.is_empty();
        self.ui
            .view(id!(recent_searches_row))
            .set_visible(cx, has_recent);
        for (i, bid) in [
            id!(rsrch_0),
            id!(rsrch_1),
            id!(rsrch_2),
            id!(rsrch_3),
            id!(rsrch_4),
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(q) = self.session_state.recent_searches.get(i) {
                self.ui.button(bid).set_visible(cx, true);
                self.ui.button(bid).set_text(cx, &clip_text(q, 20));
            } else {
                self.ui.button(bid).set_visible(cx, false);
            }
        }

        // Filtered search cards (Albums / Artists / Playlists)
        let filtered_cards: Vec<&YtmShelfCard> = self
            .search_result
            .cards
            .iter()
            .filter(|c| match self.search_filter {
                SearchFilter::All => true,
                SearchFilter::Albums => c.kind == YtmCardKind::Album,
                SearchFilter::Artists => c.kind == YtmCardKind::Artist,
                SearchFilter::Playlists => c.kind == YtmCardKind::Playlist,
                SearchFilter::Songs | SearchFilter::Local => false,
            })
            .take(5)
            .collect();
        self.ui
            .view(id!(search_cards_wrap))
            .set_visible(cx, !filtered_cards.is_empty());
        for (i, cid) in [id!(sc_0), id!(sc_1), id!(sc_2), id!(sc_3), id!(sc_4)]
            .into_iter()
            .enumerate()
        {
            let cv = self.ui.view(cid);
            if let Some(card) = filtered_cards.get(i) {
                cv.set_visible(cx, true);
                cv.label(id!(title))
                    .set_text(cx, &clip_text(&card.title, 20));
                cv.label(id!(sub))
                    .set_text(cx, &clip_text(&card.subtitle, 22));
                self.bind_cover_art_url(
                    cx,
                    cv.ferric_cover_art(id!(art)),
                    card.thumbnail_url.as_deref(),
                    None,
                    false,
                    0.4,
                    card.kind == YtmCardKind::Artist,
                );
            } else {
                cv.set_visible(cx, false);
            }
        }
    }

    fn refresh_detail_hero(&self, cx: &mut Cx) {
        let kind_str = match self.detail_state.kind {
            YtmCardKind::Album => "ALBUM",
            YtmCardKind::Artist => "ARTIST",
            YtmCardKind::Playlist => "PLAYLIST",
            YtmCardKind::Song => "TRACK",
        };
        self.ui.label(id!(detail_kind_lbl)).set_text(cx, kind_str);
        self.ui
            .label(id!(detail_title_lbl))
            .set_text(cx, &self.detail_state.title);
        let sub = if self.detail_state.loading {
            "Loading tracks from YouTube Music...".to_string()
        } else if !self.detail_state.subtitle.is_empty() {
            format!(
                "{} · {} songs",
                self.detail_state.subtitle,
                self.detail_state.tracks.len()
            )
        } else if !self.detail_state.description.is_empty() {
            format!(
                "{} · {} songs",
                clip_text(&self.detail_state.description, 56),
                self.detail_state.tracks.len()
            )
        } else {
            format!("{} songs", self.detail_state.tracks.len())
        };
        self.ui.label(id!(detail_sub_lbl)).set_text(cx, &sub);
        let is_pinned = self
            .session_state
            .pinned_ids
            .iter()
            .any(|p| p == &self.detail_state.id);
        self.ui
            .button(id!(detail_pin_btn))
            .set_text(cx, if is_pinned { "Pinned" } else { "Pin" });
        self.bind_cover_art_url(
            cx,
            self.ui.ferric_cover_art(id!(detail_art)),
            self.detail_state.thumbnail_url.as_deref(),
            self.detail_state.tracks.first(),
            false,
            0.5,
            self.detail_state.kind == YtmCardKind::Artist,
        );
    }

    fn open_local_detail(&mut self, cx: &mut Cx, group: CollectionGroup) {
        self.request_artwork_for_tracks(&group.tracks);
        self.detail_tracks = Rc::new(group.tracks.clone());
        self.detail_state = DetailViewState {
            id: group.id,
            title: group.title,
            subtitle: group.subtitle,
            description: String::new(),
            kind: group.kind,
            thumbnail_url: group.thumbnail_url,
            loading: false,
            tracks: group.tracks,
        };
        self.refresh_detail_hero(cx);
        self.navigate_to(cx, MainPage::Detail, true);
    }

    fn open_remote_card(&mut self, cx: &mut Cx, card: YtmShelfCard) {
        if let Some(vid) = card.video_id.clone()
            && card.kind == YtmCardKind::Song
        {
            let track = Track {
                id: format!("yt:video:{vid}"),
                title: card.title.clone(),
                artist: if card.subtitle.is_empty() {
                    "YouTube Music".to_string()
                } else {
                    card.subtitle.clone()
                },
                album: "YouTube Music".to_string(),
                duration_ms: 0,
                path: None,
                source: TrackSource::YouTubeMusic,
                codec: "YT AAC".to_string(),
                sample_rate: 44_100,
                bit_depth: 16,
                bitrate_kbps: 128,
                isrc: None,
                spotify_id: None,
                youtube_video_id: Some(vid),
                thumbnail_url: card.thumbnail_url.clone(),
                replay_gain_db: 0,
                liked: false,
            };
            self.queue_context = card.title;
            self.queue.set_tracks(vec![track.clone()], Some(0));
            self.play_track(cx, track);
            return;
        }

        let Some(bid) = card.browse_id.clone() else {
            return;
        };
        if let Some(url) = card.thumbnail_url.as_deref()
            && let Some(cache) = &mut self.artwork
        {
            let _ = cache.request_url(url);
        }
        self.detail_tracks = Rc::new(Vec::new());
        self.detail_state = DetailViewState {
            id: bid.clone(),
            title: card.title,
            subtitle: card.subtitle,
            description: String::new(),
            kind: card.kind,
            thumbnail_url: card.thumbnail_url,
            loading: true,
            tracks: Vec::new(),
        };
        self.refresh_detail_hero(cx);
        self.navigate_to(cx, MainPage::Detail, true);

        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let session = self.current_ytm_session();
        thread::spawn(move || match fetch_browse_detail(&session, &bid) {
            Ok(detail) => {
                let _ = tx.send(BgMessage::YtmBrowseDetailLoaded(Box::new(detail)));
            }
            Err(err) => {
                let _ = tx.send(BgMessage::YtmBrowseDetailFailed {
                    browse_id: bid,
                    error: err.to_string(),
                });
            }
        });
    }

    fn refresh_storage_stats(&self, cx: &mut Cx) {
        let audio_sz = dir_size_bytes(&app_data_dir().join("stream_cache"));
        let art_sz = dir_size_bytes(&app_data_dir().join("artwork_cache"));
        self.ui.label(id!(cache_stats_lbl)).set_text(
            cx,
            &format!(
                "Audio Stream Cache: {} · Artwork Cache: {}",
                format_bytes_mb(audio_sz),
                format_bytes_mb(art_sz)
            ),
        );
    }

    fn sync_controls_from_state(&self, cx: &mut Cx) {
        let accent = self.current_accent();
        let muted = vec4(0.604, 0.612, 0.659, 1.0);

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
            .ferric_toggle(id!(radio_toggle))
            .set_active(cx, self.session_state.autoplay_radio);
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
        self.ui
            .label(id!(eq_status_lbl))
            .set_text(cx, &format!("Preset: {}", self.eq.preset_name));

        self.ui
            .ferric_toggle(id!(adaptive_art_toggle))
            .set_active(cx, self.theme.adaptive_album_art);
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

        self.ui
            .ferric_bar_slider(id!(vol_slider))
            .set_value(cx, f64::from(self.session_state.volume));
        self.ui.button(id!(vol_mute_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if self.session_state.volume < 0.01 { accent } else { muted }) } },
        );
        self.ui.button(id!(shuffle_btn)).apply_over(
            cx,
            live! {
                draw_bg: { active: (if self.queue.shuffle { 1.0 } else { 0.0 }) }
                draw_icon: { color: (if self.queue.shuffle { accent } else { muted }) }
            },
        );
        let rep_active = self.queue.repeat != RepeatMode::Off;
        let rep_col = match self.queue.repeat {
            RepeatMode::Off => muted,
            RepeatMode::All => accent,
            RepeatMode::One => vec4(0.95, 0.78, 0.36, 1.0),
        };
        self.ui.button(id!(repeat_btn)).apply_over(
            cx,
            live! {
                draw_bg: { active: (if rep_active { 1.0 } else { 0.0 }) }
                draw_icon: { color: (rep_col) }
            },
        );

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
        let accent = self.current_accent();
        let rad = if g.liquid_glass {
            self.theme.corner_radius_px.max(12.0)
        } else {
            0.0
        };

        self.ui.view(id!(body)).apply_over(
            cx,
            live! {
                draw_bg: { canvas_col: (canvas), glass_bg: (glass_f), accent_col: (accent) }
            },
        );

        for (pid, flat_col) in [
            (id!(top_bar), surface),
            (id!(left_sidebar), surface),
            (id!(center_stage), canvas),
            (id!(right_sidebar), surface),
        ] {
            self.ui.view(pid).apply_over(
                cx,
                live! {
                    draw_bg: {
                        base_col: (flat_col),
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

        self.apply_adaptive_accent_live(cx);
        if let Some(db) = &self.db {
            let _ = db.save_theme(&self.theme);
        }
        self.ui.redraw(cx);
    }

    fn apply_dsp_live(&mut self, cx: &mut Cx) {
        if let Some(audio) = &self.audio {
            audio.send(EngineCommand::SetDspSettings(self.dsp.clone()));
        }
        if let Some(db) = &self.db {
            let _ = db.save_playback_dsp(&self.dsp);
        }
        self.sync_next_deck_to_engine();
        self.rebuild_library_views(cx);
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

    fn current_ytm_session(&self) -> YtmSession {
        if self.accounts.youtube_connected
            && !self.accounts.youtube_cookie.trim().is_empty()
            && !self.accounts.youtube_sapisid.trim().is_empty()
        {
            YtmSession {
                visitor_data: self.accounts.youtube_visitor_data.clone(),
                cookie: self.accounts.youtube_cookie.clone(),
                sapisid: self.accounts.youtube_sapisid.clone(),
            }
        } else {
            YtmSession::guest(self.accounts.youtube_visitor_data.clone())
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
            let valid_path = next_track.path.as_ref().filter(|p| p.is_file()).cloned();
            let same_album = self.queue.current().is_some_and(|cur| {
                !cur.album.is_empty()
                    && cur.album != "Unknown Album"
                    && cur.album == next_track.album
                    && cur.id != next_track.id
            });
            audio.send(EngineCommand::QueueNextTrack {
                path: valid_path,
                same_album_consecutive: same_album,
            });
        } else {
            audio.send(EngineCommand::QueueNextTrack {
                path: None,
                same_album_consecutive: false,
            });
        }
    }

    fn prefetch_next_online_track(&mut self) {
        let Some(next_track) = self.queue.peek_upcoming(1).into_iter().next().cloned() else {
            return;
        };
        if next_track.path.as_ref().is_some_and(|p| p.is_file()) {
            return;
        }
        if self.prefetch_stream_track_id.as_deref() == Some(next_track.id.as_str())
            || self.pending_stream_track_id.as_deref() == Some(next_track.id.as_str())
        {
            return;
        }
        if next_track.youtube_video_id.is_none()
            && next_track.source != TrackSource::Spotify
            && next_track.source != TrackSource::YouTubeMusic
        {
            return;
        }
        self.prefetch_stream_track_id = Some(next_track.id.clone());
        self.spawn_stream_resolve(next_track, false);
    }

    fn maybe_fetch_radio_for_track(&mut self, track: &Track, force: bool) {
        if !force && !self.session_state.autoplay_radio {
            return;
        }
        if !force && self.queue.peek_upcoming(5).len() >= 4 {
            return;
        }
        if !force && self.last_radio_seed_id.as_deref() == Some(track.id.as_str()) {
            return;
        }
        let Some(vid) = track.youtube_video_id.clone() else {
            return;
        };
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.last_radio_seed_id = Some(track.id.clone());
        let session = self.current_ytm_session();
        let seed_track_id = track.id.clone();
        let seed_title = track.title.clone();
        thread::spawn(move || {
            if let Ok(tracks) = fetch_radio_up_next(&session, &vid, 15)
                && !tracks.is_empty()
            {
                let _ = tx.send(BgMessage::RadioLoaded {
                    seed_track_id,
                    seed_title,
                    tracks,
                });
            }
        });
    }

    fn spawn_stream_resolve(&self, track: Track, auto_play: bool) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let session = self.current_ytm_session();
        let cache_dir = app_data_dir().join("stream_cache");
        let track_id = track.id;
        let artist = track.artist;
        let title = track.title;
        let existing_vid = track.youtube_video_id;

        thread::spawn(move || {
            let outcome = (|| {
                let vid = match existing_vid {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => resolve_best_video_id_for_track(&session, &artist, &title)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| format!("No YouTube Music stream found for '{title}'"))?,
                };
                let stream = resolve_and_cache_audio_stream(&vid, &session, &cache_dir)
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>((vid, stream))
            })();

            match outcome {
                Ok((video_id, stream)) => {
                    let _ = tx.send(BgMessage::StreamReady {
                        track_id,
                        video_id,
                        stream,
                        auto_play,
                    });
                }
                Err(error) => {
                    let _ = tx.send(BgMessage::StreamFailed {
                        track_id,
                        error,
                        auto_play,
                    });
                }
            }
        });
    }

    fn play_track(&mut self, cx: &mut Cx, track: Track) {
        if let Some(cache) = &mut self.artwork {
            let _ = cache.request_for_track(&track);
        }
        if let Some(path) = track.path.as_ref().filter(|p| p.is_file()).cloned() {
            self.pending_stream_track_id = None;
            if let Some(audio) = &self.audio {
                audio.send(EngineCommand::LoadAndPlayTrack {
                    path,
                    replay_gain_db: track.replay_gain_db,
                });
            }
            self.sync_next_deck_to_engine();
            self.prefetch_next_online_track();
            self.maybe_fetch_radio_for_track(&track, false);
            self.update_now_playing_metadata(cx, &track);
            return;
        }

        if track.youtube_video_id.is_some()
            || matches!(
                track.source,
                TrackSource::YouTubeMusic | TrackSource::Spotify
            )
        {
            self.pending_stream_track_id = Some(track.id.clone());
            self.ui
                .label(id!(np_title_lbl))
                .set_text(cx, &clip_text(&track.title, 19));
            self.ui.label(id!(np_artist_lbl)).set_text(
                cx,
                &clip_text(&format!("{} · Buffering...", track.artist), 22),
            );
            self.ui
                .button(id!(codec_pill_btn))
                .set_text(cx, "BUFFERING");
            self.bind_cover_art_url(
                cx,
                self.ui.ferric_cover_art(id!(np_art)),
                track.thumbnail_url.as_deref(),
                Some(&track),
                false,
                0.5,
                false,
            );
            self.refresh_right_sidebar(cx);
            self.ui.redraw(cx);
            self.spawn_stream_resolve(track.clone(), true);
            self.maybe_fetch_radio_for_track(&track, false);
            return;
        }

        self.show_toast(
            cx,
            &format!(
                "'{}' has no local file. Search YouTube Music instead.",
                track.title
            ),
        );
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
        self.ui
            .label(id!(np_title_lbl))
            .set_text(cx, &clip_text(&track.title, 19));
        self.ui
            .label(id!(np_artist_lbl))
            .set_text(cx, &clip_text(&track.artist, 22));
        self.ui.label(id!(fs_title_lbl)).set_text(cx, &track.title);
        self.ui
            .label(id!(fs_artist_lbl))
            .set_text(cx, &format!("{} · {}", track.artist, track.album));
        self.ui
            .button(id!(codec_pill_btn))
            .set_text(cx, &track.quality_badge());

        let accent = self.current_accent();
        let like_col = if track.liked {
            accent
        } else {
            vec4(0.439, 0.451, 0.510, 1.0)
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
                "Signal Path: {} -> {:.1}s {} -> 10-Band EQ -> WASAPI {:.1} kHz",
                track.telemetry_label(),
                self.dsp.crossfade_ms as f32 / 1_000.0,
                self.dsp.fade_curve.label(),
                out_rate as f32 / 1_000.0
            ),
        );

        self.bind_cover_art_url(
            cx,
            self.ui.ferric_cover_art(id!(np_art)),
            track.thumbnail_url.as_deref(),
            Some(track),
            true,
            0.55,
            false,
        );
        self.bind_cover_art_url(
            cx,
            self.ui.ferric_cover_art(id!(fs_art)),
            track.thumbnail_url.as_deref(),
            Some(track),
            false,
            0.55,
            false,
        );
        self.apply_adaptive_accent_live(cx);

        if let Some(smtc) = &mut self.smtc {
            smtc.try_attach_late();
            smtc.set_metadata(&track.title, &track.artist, &track.album, track.duration_ms);
            smtc.set_playing(true);
        }

        self.lyrics_offset_ms = 0;
        self.load_lyrics_for_track(cx, track);
        self.rebuild_library_views(cx);
        self.refresh_home_ui(cx);
        self.refresh_right_sidebar(cx);
        self.ui.redraw(cx);
    }

    fn load_lyrics_for_track(&mut self, cx: &mut Cx, track: &Track) {
        self.synced_lyrics = SyncedLyrics::default();
        self.set_lyrics_lines(
            cx,
            [
                "",
                "",
                "",
                "Searching synced lyrics (LRCLIB)...",
                "",
                "",
                "",
            ],
        );

        if let Some(db) = &self.db
            && let Ok(Some(cached)) = db.get_cached_lyrics(&track.id)
        {
            self.synced_lyrics =
                SyncedLyrics::parse_with_context(&cached, track.duration_ms, "Cached");
            self.ui
                .label(id!(rs_lyr_source))
                .set_text(cx, "Lyrics · Cached");
            self.ui
                .label(id!(fs_lyr_source))
                .set_text(cx, "Lyrics · Local Cache");
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

    fn set_lyrics_lines(&self, cx: &mut Cx, lines: [&str; 7]) {
        let rs_ids = [
            id!(rs_lyr_0),
            id!(rs_lyr_1),
            id!(rs_lyr_2),
            id!(rs_lyr_3),
            id!(rs_lyr_4),
            id!(rs_lyr_5),
            id!(rs_lyr_6),
        ];
        let fs_ids = [
            id!(fs_lyr_0),
            id!(fs_lyr_1),
            id!(fs_lyr_2),
            id!(fs_lyr_3),
            id!(fs_lyr_4),
            id!(fs_lyr_5),
            id!(fs_lyr_6),
        ];
        for i in 0..7 {
            self.ui.label(rs_ids[i]).set_text(cx, lines[i]);
            self.ui.label(fs_ids[i]).set_text(cx, lines[i]);
        }
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
        self.set_lyrics_lines(
            cx,
            [
                get_txt(idx.checked_sub(3)),
                get_txt(idx.checked_sub(2)),
                get_txt(idx.checked_sub(1)),
                get_txt(Some(idx)),
                get_txt(Some(idx + 1)),
                get_txt(Some(idx + 2)),
                get_txt(Some(idx + 3)),
            ],
        );
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
            self.rebuild_library_views(cx);
            if let Some(first) = self.filtered_tracks.first().cloned() {
                self.queue_context = "Offline Demo".to_string();
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

    fn spawn_ytm_rich_home(&mut self, cx: &mut Cx, mood_index: usize, mood_params: Option<String>) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.active_mood_idx = mood_index;
        self.qp_page = 0;
        self.shelf_pages = [0, 0, 0];
        let session = self.current_ytm_session();
        self.ui
            .label(id!(home_status_lbl))
            .set_text(cx, "Refreshing YouTube Music...");
        self.refresh_home_ui(cx);

        thread::spawn(
            move || match fetch_rich_home(&session, mood_params.as_deref()) {
                Ok(home) => {
                    let _ = tx.send(BgMessage::YtmRichHomeLoaded {
                        mood_index,
                        home: Box::new(home),
                    });
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::YtmRichHomeFailed(err.to_string()));
                }
            },
        );
    }

    fn spawn_rich_search(&mut self, cx: &mut Cx, raw_query: String) {
        let query = raw_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        if parse_spotify_target(&query).is_some() {
            self.start_public_sync(cx, "Spotify", query);
            return;
        }
        if parse_youtube_target(&query).is_some() {
            self.start_public_sync(cx, "YouTube", query);
            return;
        }
        self.session_state.record_search(&query);
        if let Some(db) = &self.db {
            let _ = db.save_session_state(&self.session_state);
        }
        self.ui
            .text_input(id!(search_page_input))
            .set_text(cx, &query);
        self.navigate_to(cx, MainPage::Search, true);
        self.ui
            .label(id!(search_status_lbl))
            .set_text(cx, &format!("Searching '{query}'..."));

        // Immediate local matches while YouTube Music responds
        let q_low = query.to_ascii_lowercase();
        let local_hits: Vec<Track> = self
            .all_tracks
            .iter()
            .filter(|t| {
                t.title.to_ascii_lowercase().contains(&q_low)
                    || t.artist.to_ascii_lowercase().contains(&q_low)
                    || t.album.to_ascii_lowercase().contains(&q_low)
            })
            .cloned()
            .collect();
        self.search_tracks = Rc::new(local_hits);
        self.refresh_search_ui(cx);

        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let session = self.current_ytm_session();
        thread::spawn(move || match fetch_rich_search(&session, &query, 30) {
            Ok(result) => {
                let _ = tx.send(BgMessage::YtmRichSearchCompleted {
                    query,
                    result: Box::new(result),
                });
            }
            Err(err) => {
                let _ = tx.send(BgMessage::YtmSearchFailed {
                    query,
                    error: err.to_string(),
                });
            }
        });
    }

    fn apply_search_filter(&mut self, cx: &mut Cx, filter: SearchFilter) {
        self.search_filter = filter;
        let query = self.ui.text_input(id!(search_page_input)).text();
        let q_low = query.trim().to_ascii_lowercase();
        let tracks = match filter {
            SearchFilter::Local => self
                .all_tracks
                .iter()
                .filter(|t| {
                    t.source == TrackSource::Local
                        && (q_low.is_empty()
                            || t.title.to_ascii_lowercase().contains(&q_low)
                            || t.artist.to_ascii_lowercase().contains(&q_low))
                })
                .cloned()
                .collect(),
            _ => {
                if !self.search_result.tracks.is_empty() {
                    self.search_result.tracks.clone()
                } else {
                    self.all_tracks
                        .iter()
                        .filter(|t| {
                            q_low.is_empty()
                                || t.title.to_ascii_lowercase().contains(&q_low)
                                || t.artist.to_ascii_lowercase().contains(&q_low)
                        })
                        .cloned()
                        .collect()
                }
            }
        };
        self.search_tracks = Rc::new(tracks);
        self.refresh_search_ui(cx);
        self.ui.redraw(cx);
    }

    fn start_google_webview_login(&mut self, cx: &mut Cx) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.show_toast(cx, "Opening Google Sign-In window...");
        self.ui.label(id!(oauth_status_lbl)).set_text(
            cx,
            "Opening Google Sign-In... Sign in normally and Ferric connects automatically.",
        );
        let profile_dir = app_data_dir().join("webview2_profile");
        let clear_cookies = self.ytm_signed_out_recently;
        launch_google_ytm_login_window(profile_dir, clear_cookies, move |outcome| {
            let _ = tx.send(BgMessage::GoogleLoginResult(outcome));
        });
    }

    fn sync_ytm_signed_in_library(&self, cx: &mut Cx) {
        let session = self.current_ytm_session();
        if !session.is_signed_in() {
            self.ui.label(id!(oauth_status_lbl)).set_text(
                cx,
                "Sign in with Google first to sync your Liked Songs and Playlists.",
            );
            return;
        }
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.ui
            .label(id!(oauth_status_lbl))
            .set_text(cx, "Syncing your YouTube Music library...");
        thread::spawn(move || match fetch_account_library(&session) {
            Ok(lib) => {
                let _ = tx.send(BgMessage::YtmAccountSynced(Box::new(lib)));
            }
            Err(err) => {
                let _ = tx.send(BgMessage::YtmAccountSyncFailed(err.to_string()));
            }
        });
    }

    fn start_public_sync(&mut self, cx: &mut Cx, mode: &'static str, raw_input: String) {
        let trimmed = raw_input.trim().to_string();
        if trimmed.is_empty() {
            self.show_toast(cx, "Paste a public Spotify or YouTube URL first");
            return;
        }
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.show_toast(cx, &format!("Importing {mode} link..."));
        self.ui
            .label(id!(oauth_status_lbl))
            .set_text(cx, &format!("Importing {mode} link..."));

        thread::spawn(move || {
            let result = match mode {
                "Spotify" => import_spotify_public_url(&trimmed),
                _ => import_youtube_public_url(&trimmed),
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
                .ferric_qr_view(id!(connect_qr_view))
                .set_matrix(cx, qr.size, qr.modules);
        }
        self.ui.label(id!(connect_lan_ip_lbl)).set_text(
            cx,
            &format!("LAN: {}:43821 · Desktop-PC Active", self.lan_ip),
        );
    }

    fn spawn_update_check(&self, cx: &mut Cx, manual: bool) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        if manual {
            self.ui
                .label(id!(update_status_lbl))
                .set_text(cx, "Checking GitHub Releases...");
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
            .set_text(cx, &format!("Ferric {} Available", info.tag));
        self.ui.label(id!(update_popup_sub)).set_text(
            cx,
            &format!(
                "Installed: v{APP_VERSION} -> Latest: {} (SHA-256 MSI)",
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
        // 0. Poll async GPU artwork cache
        if let Some(cache) = &mut self.artwork
            && cache.poll(cx)
        {
            self.refresh_all_artwork_slots(cx);
        }

        // Auto-hide toast after 3s
        if let Some(until) = self.toast_until
            && Instant::now() >= until
        {
            self.toast_until = None;
            self.ui.view(id!(toast_wrap)).set_visible(cx, false);
        }

        // 1. Drain background channel messages
        let mut messages = Vec::new();
        if let Some(rx) = &self.bg_rx {
            while let Ok(msg) = rx.try_recv() {
                messages.push(msg);
            }
        }
        for msg in messages {
            match msg {
                BgMessage::ScanCompleted(tracks) => {
                    let n = tracks.len();
                    if let Some(db) = &mut self.db {
                        let _ = db.upsert_tracks(&tracks);
                        let _ = db.match_cloud_tracks_to_local();
                        self.all_tracks = db.list_tracks("").unwrap_or_default();
                    } else {
                        self.all_tracks.extend(tracks);
                    }
                    self.rebuild_library_views(cx);
                    self.show_toast(cx, &format!("Scanned {n} local audio files"));
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
                            self.set_lyrics_lines(
                                cx,
                                ["", "", "", "No lyrics found for this song.", "", "", ""],
                            );
                        } else {
                            let mode = if parsed.is_synced {
                                "Time-Synced"
                            } else {
                                "Auto-Paced"
                            };
                            let lbl = format!("Lyrics · {} ({})", parsed.source_name, mode);
                            self.ui.label(id!(rs_lyr_source)).set_text(cx, &lbl);
                            self.ui.label(id!(fs_lyr_source)).set_text(cx, &lbl);
                            self.synced_lyrics = parsed;
                            self.update_lyrics_display(cx, 0);
                        }
                    }
                }
                BgMessage::YtmRichHomeLoaded { mood_index, home } => {
                    let home = *home;
                    let pick_n = home.quick_picks.len();
                    let shelf_n = home.shelves.len();
                    if !home.visitor_data.is_empty() {
                        self.accounts.youtube_visitor_data = home.visitor_data.clone();
                    }
                    // Preserve original chips list when switching to a mood filter
                    let saved_chips = if home.chips.is_empty() && !self.rich_home.chips.is_empty() {
                        self.rich_home.chips.clone()
                    } else {
                        home.chips.clone()
                    };
                    let mut merged = home;
                    merged.chips = saved_chips;
                    self.active_mood_idx = mood_index;

                    if let Some(db) = &mut self.db {
                        let _ = db.save_accounts(&self.accounts);
                        let _ = db.upsert_tracks(&merged.all_tracks);
                        if !merged.quick_picks.is_empty() {
                            let pl = Playlist {
                                id: "ytm:home:discover".to_string(),
                                name: "Discover & Quick Picks".to_string(),
                                source: TrackSource::YouTubeMusic,
                                etag: None,
                                track_count: merged.quick_picks.len(),
                                description: "YouTube Music Home Quick Picks".to_string(),
                                thumbnail_url: merged
                                    .quick_picks
                                    .first()
                                    .and_then(|t| t.thumbnail_url.clone()),
                            };
                            let ids: Vec<String> =
                                merged.quick_picks.iter().map(|t| t.id.clone()).collect();
                            let _ = db.upsert_playlist(&pl, &ids);
                        }
                        let _ = db.match_cloud_tracks_to_local();
                        self.all_tracks = db.list_tracks("").unwrap_or_default();
                        self.playlists = db.list_playlists().unwrap_or_default();
                    }

                    // Request real album cover artwork for all visible Quick Picks & Shelf cards!
                    self.request_artwork_for_tracks(&merged.quick_picks);
                    for shelf in &merged.shelves {
                        self.request_artwork_for_cards(&shelf.cards);
                    }
                    self.rich_home = merged;

                    if self.queue.is_empty() && !self.rich_home.quick_picks.is_empty() {
                        self.queue
                            .set_tracks(self.rich_home.quick_picks.clone(), None);
                    }

                    self.ui
                        .label(id!(home_status_lbl))
                        .set_text(cx, &format!("{pick_n} songs · {shelf_n} shelves"));
                    self.rebuild_library_views(cx);
                    self.refresh_home_ui(cx);
                    self.refresh_right_sidebar(cx);

                    if startup_play_first()
                        && !self.startup_auto_played
                        && let Some(first) = self.rich_home.quick_picks.first().cloned()
                    {
                        self.startup_auto_played = true;
                        self.queue_context = "Quick picks".to_string();
                        self.queue
                            .set_tracks(self.rich_home.quick_picks.clone(), Some(0));
                        self.play_track(cx, first);
                    }
                }
                BgMessage::YtmRichHomeFailed(err) => {
                    self.ui
                        .label(id!(home_status_lbl))
                        .set_text(cx, "Offline mode");
                    self.ui
                        .label(id!(oauth_status_lbl))
                        .set_text(cx, &format!("Home refresh failed: {err}"));
                }
                BgMessage::YtmBrowseDetailLoaded(detail) => {
                    let detail = *detail;
                    if self.detail_state.id == detail.browse_id {
                        self.request_artwork_for_tracks(&detail.tracks);
                        if let Some(url) = detail.thumbnail_url.as_deref()
                            && let Some(cache) = &mut self.artwork
                        {
                            let _ = cache.request_url(url);
                        }
                        if let Some(db) = &mut self.db
                            && !detail.tracks.is_empty()
                        {
                            let _ = db.upsert_tracks(&detail.tracks);
                            self.all_tracks = db.list_tracks("").unwrap_or_default();
                        }
                        self.detail_tracks = Rc::new(detail.tracks.clone());
                        self.detail_state = DetailViewState {
                            id: detail.browse_id,
                            title: detail.title,
                            subtitle: detail.subtitle,
                            description: detail.description,
                            kind: detail.kind,
                            thumbnail_url: detail.thumbnail_url,
                            loading: false,
                            tracks: detail.tracks,
                        };
                        self.refresh_detail_hero(cx);
                        self.ui.redraw(cx);
                    }
                }
                BgMessage::YtmBrowseDetailFailed { browse_id, error } => {
                    if self.detail_state.id == browse_id {
                        self.detail_state.loading = false;
                        self.detail_state.subtitle = format!("Failed to load: {error}");
                        self.refresh_detail_hero(cx);
                    }
                }
                BgMessage::YtmRichSearchCompleted { query, result } => {
                    let result = *result;
                    let song_n = result.tracks.len();
                    let card_n = result.cards.len();
                    self.request_artwork_for_tracks(&result.tracks);
                    self.request_artwork_for_cards(&result.cards);
                    if let Some(db) = &mut self.db
                        && !result.tracks.is_empty()
                    {
                        let _ = db.upsert_tracks(&result.tracks);
                        let _ = db.match_cloud_tracks_to_local();
                        self.all_tracks = db.list_tracks("").unwrap_or_default();
                    }
                    self.search_tracks = Rc::new(result.tracks.clone());
                    self.search_result = result;
                    self.ui.label(id!(search_status_lbl)).set_text(
                        cx,
                        &format!("'{query}' · {song_n} songs, {card_n} albums/artists"),
                    );
                    self.refresh_search_ui(cx);
                    self.ui.redraw(cx);
                }
                BgMessage::YtmSearchFailed { query, error } => {
                    self.ui
                        .label(id!(search_status_lbl))
                        .set_text(cx, &format!("Search '{query}' failed: {error}"));
                }
                BgMessage::RadioLoaded {
                    seed_track_id,
                    seed_title,
                    tracks,
                } => {
                    self.request_artwork_for_tracks(&tracks);
                    if let Some(db) = &mut self.db {
                        let _ = db.upsert_tracks(&tracks);
                        self.all_tracks = db.list_tracks("").unwrap_or_default();
                    }
                    let added = self.queue.append_tracks(tracks);
                    if added > 0
                        && self.queue.current().map(|t| t.id.as_str())
                            == Some(seed_track_id.as_str())
                    {
                        self.queue_context = format!("{seed_title} radio");
                        self.sync_next_deck_to_engine();
                        self.prefetch_next_online_track();
                        self.refresh_right_sidebar(cx);
                    }
                }
                BgMessage::GoogleLoginResult(outcome) => match outcome {
                    GoogleLoginOutcome::SignedIn { cookie, sapisid } => {
                        self.ytm_signed_out_recently = false;
                        self.accounts.youtube_connected = true;
                        self.accounts.youtube_cookie = cookie;
                        self.accounts.youtube_sapisid = sapisid;
                        if self.accounts.youtube_channel_name.trim().is_empty() {
                            self.accounts.youtube_channel_name = "Google Account".to_string();
                        }
                        if let Some(db) = &self.db {
                            let _ = db.save_accounts(&self.accounts);
                        }
                        self.rebuild_library_views(cx);
                        self.sync_ytm_signed_in_library(cx);
                        self.show_toast(cx, "Signed in with Google! Syncing library...");
                    }
                    GoogleLoginOutcome::Cancelled => {
                        self.ui
                            .label(id!(oauth_status_lbl))
                            .set_text(cx, "Google sign-in closed. Guest Mode remains active.");
                    }
                    GoogleLoginOutcome::Failed(err) => {
                        self.ui
                            .label(id!(oauth_status_lbl))
                            .set_text(cx, &format!("Google sign-in error: {err}"));
                    }
                },
                BgMessage::YtmAccountSynced(lib) => {
                    let lib = *lib;
                    self.accounts.youtube_connected = true;
                    self.accounts.youtube_channel_name = lib.display_name.clone();
                    if !lib.visitor_data.is_empty() {
                        self.accounts.youtube_visitor_data = lib.visitor_data;
                    }
                    self.accounts.youtube_playlist_count = lib.playlists.len();
                    let liked_n = lib.liked_tracks.len();
                    let pl_n = lib.playlists.len();
                    if let Some(db) = &mut self.db {
                        let _ = db.save_accounts(&self.accounts);
                        let _ = db.upsert_tracks(&lib.liked_tracks);
                        let _ = db.upsert_tracks(&lib.home_tracks);
                        for (pl, pl_tracks) in &lib.playlists {
                            let _ = db.upsert_tracks(pl_tracks);
                            let ids: Vec<String> = pl_tracks.iter().map(|t| t.id.clone()).collect();
                            let _ = db.upsert_playlist(pl, &ids);
                        }
                        let _ = db.match_cloud_tracks_to_local();
                        self.all_tracks = db.list_tracks("").unwrap_or_default();
                        self.playlists = db.list_playlists().unwrap_or_default();
                    }
                    self.ui.label(id!(oauth_status_lbl)).set_text(
                        cx,
                        &format!(
                            "Signed in as {} · Synced {liked_n} liked songs & {pl_n} playlists.",
                            lib.display_name
                        ),
                    );
                    self.rebuild_library_views(cx);
                    self.spawn_ytm_rich_home(cx, 0, None);
                }
                BgMessage::YtmAccountSyncFailed(err) => {
                    self.ui
                        .label(id!(oauth_status_lbl))
                        .set_text(cx, &format!("YouTube Music sync failed: {err}"));
                }
                BgMessage::StreamReady {
                    track_id,
                    video_id,
                    stream,
                    auto_play,
                } => {
                    if self.prefetch_stream_track_id.as_deref() == Some(track_id.as_str()) {
                        self.prefetch_stream_track_id = None;
                    }
                    let mut updated_track: Option<Track> = None;
                    for t in &mut self.all_tracks {
                        if t.id == track_id {
                            t.path = Some(stream.path.clone());
                            t.youtube_video_id = Some(video_id.clone());
                            t.codec = stream.codec.clone();
                            if stream.bitrate_kbps > 0 {
                                t.bitrate_kbps = stream.bitrate_kbps;
                            }
                            if stream.duration_ms > 0 && t.duration_ms == 0 {
                                t.duration_ms = stream.duration_ms;
                            }
                            updated_track = Some(t.clone());
                            break;
                        }
                    }
                    self.queue.update_track_cached_stream(
                        &track_id,
                        stream.path.clone(),
                        Some(video_id),
                        &stream.codec,
                        stream.bitrate_kbps,
                        stream.duration_ms,
                    );
                    if let Some(ref tr) = updated_track
                        && let Some(db) = &mut self.db
                    {
                        let _ = db.upsert_tracks(std::slice::from_ref(tr));
                    }

                    if auto_play
                        && self.pending_stream_track_id.as_deref() == Some(track_id.as_str())
                    {
                        self.pending_stream_track_id = None;
                        let rg = updated_track.as_ref().map_or(0, |t| t.replay_gain_db);
                        if let Some(audio) = &self.audio {
                            audio.send(EngineCommand::LoadAndPlayTrack {
                                path: stream.path,
                                replay_gain_db: rg,
                            });
                        }
                        self.sync_next_deck_to_engine();
                        self.prefetch_next_online_track();
                        if let Some(tr) = updated_track.or_else(|| self.queue.current().cloned()) {
                            self.update_now_playing_metadata(cx, &tr);
                        }
                    } else {
                        self.sync_next_deck_to_engine();
                    }
                }
                BgMessage::StreamFailed {
                    track_id,
                    error,
                    auto_play,
                } => {
                    if self.prefetch_stream_track_id.as_deref() == Some(track_id.as_str()) {
                        self.prefetch_stream_track_id = None;
                    }
                    if auto_play
                        && self.pending_stream_track_id.as_deref() == Some(track_id.as_str())
                    {
                        self.pending_stream_track_id = None;
                        if let Some(next_candidate) = self.queue.next_track().cloned()
                            && next_candidate.id != track_id
                        {
                            self.show_toast(cx, "Skipping unavailable stream...");
                            self.play_track(cx, next_candidate);
                        } else {
                            self.ui
                                .label(id!(np_artist_lbl))
                                .set_text(cx, "Stream unavailable");
                            self.ui.button(id!(codec_pill_btn)).set_text(cx, "OFFLINE");
                            self.show_toast(cx, &format!("Stream failed: {error}"));
                        }
                    }
                }
                BgMessage::PublicSyncCompleted { provider, payload } => {
                    let (playlist, tracks) = *payload;
                    let count = tracks.len();
                    let pl_name = playlist.name.clone();
                    if let Some(db) = &mut self.db {
                        let _ = db.upsert_tracks(&tracks);
                        let ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
                        let _ = db.upsert_playlist(&playlist, &ids);
                        let _ = db.match_cloud_tracks_to_local();
                        self.all_tracks = db.list_tracks("").unwrap_or_default();
                        self.playlists = db.list_playlists().unwrap_or_default();
                    }
                    self.rebuild_library_views(cx);
                    self.show_toast(
                        cx,
                        &format!("Imported {provider}: '{pl_name}' ({count} tracks)"),
                    );
                    self.open_local_detail(
                        cx,
                        CollectionGroup {
                            id: playlist.id,
                            title: playlist.name,
                            subtitle: format!("{provider} Import"),
                            kind: YtmCardKind::Playlist,
                            thumbnail_url: playlist.thumbnail_url,
                            tracks,
                        },
                    );
                }
                BgMessage::PublicSyncFailed { provider, error } => {
                    self.ui
                        .label(id!(oauth_status_lbl))
                        .set_text(cx, &format!("{provider} import failed: {error}"));
                    self.show_toast(cx, &format!("{provider} import failed"));
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
                    self.rebuild_library_views(cx);
                    self.show_toast(cx, &format!("Subsonic synced {} songs", tracks.len()));
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

        // 3. Sleep timer countdown
        if let SleepTimerMode::Deadline(deadline) = self.sleep_timer {
            let now = Instant::now();
            if now >= deadline {
                self.sleep_timer = SleepTimerMode::Off;
                self.ui
                    .label(id!(sleep_status_lbl))
                    .set_text(cx, "Sleep Timer: Paused smoothly");
                if let Some(a) = &self.audio {
                    a.send(EngineCommand::Pause);
                }
            } else {
                let rem_s = deadline.duration_since(now).as_secs();
                self.ui.label(id!(sleep_status_lbl)).set_text(
                    cx,
                    &format!("Sleep Timer: {}:{:02} left", rem_s / 60, rem_s % 60),
                );
            }
        }

        // 4. Periodic LAN beacon when Connect settings tab is visible
        if self.active_page == MainPage::Settings
            && self.active_settings_tab == SettingsTab::Connect
        {
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
                    self.ui.label(id!(connect_peers_lbl)).set_text(cx, &txt);
                }
            }
        }

        // 5. Sync Window Maximize / Restore Glyph
        self.ui
            .ferric_win_btn(id!(win_max_btn))
            .set_maximized(cx, win32_is_active_window_maximized());

        // 6. Audio Telemetry, Auto-Transition, Lyrics & Live Spectrum
        if let Some(audio) = &self.audio {
            let snap = audio.telemetry();
            if snap.track_finished_seq != self.last_finished_seq {
                self.last_finished_seq = snap.track_finished_seq;
                if matches!(self.sleep_timer, SleepTimerMode::EndOfTrack) {
                    self.sleep_timer = SleepTimerMode::Off;
                    audio.send(EngineCommand::Stop);
                    self.ui
                        .label(id!(sleep_status_lbl))
                        .set_text(cx, "Sleep Timer: Stopped after song");
                } else if let Some(next) = self.queue.next_track().cloned() {
                    if snap.auto_transitioned {
                        self.sync_next_deck_to_engine();
                        self.prefetch_next_online_track();
                        self.maybe_fetch_radio_for_track(&next, false);
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

            let accent = self.current_accent();
            if snap.is_playing {
                self.canvas_phase = (self.canvas_phase + 0.09) % 628.0;
                let norm = |i: usize| (snap.spectrum_bins[i] as f32 / 1_000.0).clamp(0.05, 1.0);
                let avg = (norm(0) + norm(1) + norm(2)) / 3.0;
                self.ui
                    .ferric_cover_art(id!(np_art))
                    .set_state(cx, true, avg, accent, false);
                self.ui
                    .view(id!(q_now_row))
                    .ferric_cover_art(id!(art))
                    .set_state(cx, true, avg, accent, false);
                if self.fullscreen_open {
                    self.ui.view(id!(np_canvas)).apply_over(
                        cx,
                        live! {
                            draw_bg: {
                                mode: 1.0,
                                phase: (self.canvas_phase),
                                s0: (norm(0)), s1: (norm(1)), s2: (norm(2)), s3: (norm(3)),
                                s4: (norm(4)), s5: (norm(5)), s6: (norm(6)), s7: (norm(7))
                            }
                        },
                    );
                }
            } else {
                self.ui
                    .ferric_cover_art(id!(np_art))
                    .set_state(cx, false, 0.3, accent, false);
                self.ui
                    .view(id!(q_now_row))
                    .ferric_cover_art(id!(art))
                    .set_state(cx, false, 0.3, accent, false);
            }

            if let Some(smtc) = &mut self.smtc {
                smtc.set_playing(snap.is_playing);
            }
        }
    }

    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        // 1. WINDOW CONTROLS
        if self.ui.ferric_win_btn(id!(win_min_btn)).clicked(actions) {
            win32_minimize_active_window();
            return;
        }
        if self.ui.ferric_win_btn(id!(win_max_btn)).clicked(actions) {
            let is_max = win32_toggle_maximize_active_window();
            self.ui
                .ferric_win_btn(id!(win_max_btn))
                .set_maximized(cx, is_max);
            self.sync_maximized_chrome_padding(cx, is_max);
            cx.redraw_all();
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

        // 2. TOP BAR & SIDEBAR CHROME
        if self.ui.button(id!(toggle_left_btn)).clicked(actions) {
            self.left_sidebar_open = !self.left_sidebar_open;
            self.ui
                .view(id!(left_sidebar))
                .set_visible(cx, self.left_sidebar_open);
            self.ui
                .view(id!(left_divider))
                .set_visible(cx, self.left_sidebar_open);
            self.ui.redraw(cx);
        }
        if self.ui.button(id!(toggle_right_btn)).clicked(actions) {
            self.set_right_sidebar(cx, !self.right_sidebar_open, self.right_lyrics_tab);
        }
        if self.ui.button(id!(nav_back_btn)).clicked(actions)
            && let Some(prev) = self.nav_history.as_mut().and_then(NavHistory::go_back)
        {
            self.navigate_to(cx, prev, false);
        }
        if self.ui.button(id!(nav_fwd_btn)).clicked(actions)
            && let Some(next) = self.nav_history.as_mut().and_then(NavHistory::go_forward)
        {
            self.navigate_to(cx, next, false);
        }
        if self.ui.button(id!(refresh_btn)).clicked(actions) {
            let params = if self.active_mood_idx > 0 {
                self.rich_home
                    .chips
                    .get(self.active_mood_idx - 1)
                    .map(|c| c.params.clone())
            } else {
                None
            };
            self.spawn_ytm_rich_home(cx, self.active_mood_idx, params);
            self.show_toast(cx, "Refreshing YouTube Music feed...");
        }
        if self.ui.button(id!(top_search_pill)).clicked(actions) {
            self.cmdk_open = true;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, true);
            self.ui.redraw(cx);
        }
        if self.ui.button(id!(top_account_btn)).clicked(actions) {
            self.navigate_to(cx, MainPage::Settings, true);
            self.switch_settings_tab(cx, SettingsTab::Accounts);
        }
        if self.ui.button(id!(top_settings_btn)).clicked(actions)
            || self.ui.button(id!(nav_settings)).clicked(actions)
        {
            self.navigate_to(cx, MainPage::Settings, true);
        }

        // Left Sidebar Nav
        for (btn, page) in [
            (id!(nav_home), MainPage::Home),
            (id!(nav_search), MainPage::Search),
            (id!(nav_recents), MainPage::Recents),
            (id!(nav_songs), MainPage::Songs),
            (id!(nav_albums), MainPage::Albums),
            (id!(nav_artists), MainPage::Artists),
            (id!(nav_playlists), MainPage::Playlists),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.navigate_to(cx, page, true);
            }
        }
        if self.ui.button(id!(pin_liked_btn)).clicked(actions) {
            self.active_filter = LibraryFilter::Liked;
            self.rebuild_library_views(cx);
            self.navigate_to(cx, MainPage::Songs, true);
        }
        if self.ui.button(id!(pin_local_btn)).clicked(actions) {
            self.active_filter = LibraryFilter::Local;
            self.rebuild_library_views(cx);
            self.navigate_to(cx, MainPage::Songs, true);
        }
        for (i, slot) in [
            id!(pin_slot_0),
            id!(pin_slot_1),
            id!(pin_slot_2),
            id!(pin_slot_3),
            id!(pin_slot_4),
        ]
        .into_iter()
        .enumerate()
        {
            if self.ui.button(slot).clicked(actions)
                && let Some(pl) = self.playlists.get(i).cloned()
            {
                let tracks = self
                    .db
                    .as_ref()
                    .and_then(|d| d.list_playlist_tracks(&pl.id).ok())
                    .unwrap_or_default();
                self.open_local_detail(
                    cx,
                    CollectionGroup {
                        id: pl.id,
                        title: pl.name,
                        subtitle: pl.description,
                        kind: YtmCardKind::Playlist,
                        thumbnail_url: pl.thumbnail_url,
                        tracks,
                    },
                );
            }
        }

        // 3. HOME INTERACTIONS (Mood Chips, Quick Picks, Horizontal Cover Shelves)
        for (i, cid) in [
            id!(chip_0),
            id!(chip_1),
            id!(chip_2),
            id!(chip_3),
            id!(chip_4),
            id!(chip_5),
            id!(chip_6),
            id!(chip_7),
            id!(chip_8),
        ]
        .into_iter()
        .enumerate()
        {
            if self.ui.button(cid).clicked(actions) {
                let params = if i == 0 {
                    None
                } else {
                    self.rich_home.chips.get(i - 1).map(|c| c.params.clone())
                };
                self.spawn_ytm_rich_home(cx, i, params);
            }
        }
        if self.ui.button(id!(home_play_all_btn)).clicked(actions) {
            let pool = self.home_quick_picks_pool();
            if let Some(first) = pool.first().cloned() {
                self.queue_context = "Quick picks".to_string();
                self.queue.set_tracks(pool, Some(0));
                self.play_track(cx, first);
            }
        }
        if self.ui.button(id!(qp_prev_btn)).clicked(actions) && self.qp_page > 0 {
            self.qp_page -= 1;
            self.refresh_home_ui(cx);
        }
        if self.ui.button(id!(qp_next_btn)).clicked(actions) {
            let total = self.home_quick_picks_pool().len();
            if (self.qp_page + 1) * 8 < total {
                self.qp_page += 1;
                self.refresh_home_ui(cx);
            }
        }
        let qp_ids = [
            id!(qp_0),
            id!(qp_1),
            id!(qp_2),
            id!(qp_3),
            id!(qp_4),
            id!(qp_5),
            id!(qp_6),
            id!(qp_7),
        ];
        for (slot, vid) in qp_ids.into_iter().enumerate() {
            let v = self.ui.view(vid);
            if v.ferric_cover_art(id!(art)).clicked(actions)
                || v.finger_up(actions)
                    .is_some_and(|fe| fe.is_over && fe.was_tap())
            {
                let pool = self.home_quick_picks_pool();
                let idx = self.qp_page * 8 + slot;
                if let Some(track) = pool.get(idx).cloned() {
                    self.queue_context = "Quick picks".to_string();
                    self.queue.set_tracks(pool, Some(idx));
                    self.play_track(cx, track);
                }
            }
        }

        // Shelf paging & clicks
        for (si, prev_b, next_b, cards) in [
            (
                0usize,
                id!(sh0_prev_btn),
                id!(sh0_next_btn),
                [
                    id!(sh0_c0),
                    id!(sh0_c1),
                    id!(sh0_c2),
                    id!(sh0_c3),
                    id!(sh0_c4),
                ],
            ),
            (
                1usize,
                id!(sh1_prev_btn),
                id!(sh1_next_btn),
                [
                    id!(sh1_c0),
                    id!(sh1_c1),
                    id!(sh1_c2),
                    id!(sh1_c3),
                    id!(sh1_c4),
                ],
            ),
            (
                2usize,
                id!(sh2_prev_btn),
                id!(sh2_next_btn),
                [
                    id!(sh2_c0),
                    id!(sh2_c1),
                    id!(sh2_c2),
                    id!(sh2_c3),
                    id!(sh2_c4),
                ],
            ),
        ] {
            if self.ui.button(prev_b).clicked(actions) && self.shelf_pages[si] > 0 {
                self.shelf_pages[si] -= 1;
                self.refresh_home_ui(cx);
            }
            if self.ui.button(next_b).clicked(actions) {
                let len = self.rich_home.shelves.get(si).map_or(0, |s| s.cards.len());
                if (self.shelf_pages[si] + 1) * 5 < len {
                    self.shelf_pages[si] += 1;
                    self.refresh_home_ui(cx);
                }
            }
            for (ci, cid) in cards.into_iter().enumerate() {
                let cv = self.ui.view(cid);
                if cv.ferric_cover_art(id!(art)).clicked(actions)
                    || cv
                        .finger_up(actions)
                        .is_some_and(|fe| fe.is_over && fe.was_tap())
                {
                    let idx = self.shelf_pages[si] * 5 + ci;
                    if let Some(card) = self
                        .rich_home
                        .shelves
                        .get(si)
                        .and_then(|s| s.cards.get(idx))
                        .cloned()
                    {
                        self.open_remote_card(cx, card);
                    }
                }
            }
        }

        // 4. SEARCH PAGE
        if self.ui.button(id!(search_go_btn)).clicked(actions) {
            let q = self.ui.text_input(id!(search_page_input)).text();
            self.spawn_rich_search(cx, q);
        }
        if let Some((submitted, _)) = self.ui.text_input(id!(search_page_input)).returned(actions) {
            self.spawn_rich_search(cx, submitted);
        }
        if self.ui.button(id!(search_clear_btn)).clicked(actions) {
            self.ui.text_input(id!(search_page_input)).set_text(cx, "");
            self.search_result = YtmRichSearch::default();
            self.search_tracks = Rc::new(Vec::new());
            self.refresh_search_ui(cx);
        }
        for (btn, flt) in [
            (id!(sf_all_btn), SearchFilter::All),
            (id!(sf_songs_btn), SearchFilter::Songs),
            (id!(sf_albums_btn), SearchFilter::Albums),
            (id!(sf_artists_btn), SearchFilter::Artists),
            (id!(sf_playlists_btn), SearchFilter::Playlists),
            (id!(sf_local_btn), SearchFilter::Local),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.apply_search_filter(cx, flt);
            }
        }
        for (i, bid) in [
            id!(rsrch_0),
            id!(rsrch_1),
            id!(rsrch_2),
            id!(rsrch_3),
            id!(rsrch_4),
        ]
        .into_iter()
        .enumerate()
        {
            if self.ui.button(bid).clicked(actions)
                && let Some(q) = self.session_state.recent_searches.get(i).cloned()
            {
                self.spawn_rich_search(cx, q);
            }
        }
        for (i, cid) in [id!(sc_0), id!(sc_1), id!(sc_2), id!(sc_3), id!(sc_4)]
            .into_iter()
            .enumerate()
        {
            let cv = self.ui.view(cid);
            if cv.ferric_cover_art(id!(art)).clicked(actions)
                || cv
                    .finger_up(actions)
                    .is_some_and(|fe| fe.is_over && fe.was_tap())
            {
                let card_opt = self
                    .search_result
                    .cards
                    .iter()
                    .filter(|c| match self.search_filter {
                        SearchFilter::All => true,
                        SearchFilter::Albums => c.kind == YtmCardKind::Album,
                        SearchFilter::Artists => c.kind == YtmCardKind::Artist,
                        SearchFilter::Playlists => c.kind == YtmCardKind::Playlist,
                        SearchFilter::Songs | SearchFilter::Local => false,
                    })
                    .nth(i)
                    .cloned();
                if let Some(card) = card_opt {
                    self.open_remote_card(cx, card);
                }
            }
        }

        // 5. RECENTS, SONGS, ALBUMS, ARTISTS, PLAYLISTS & DETAIL ACTIONS
        if self.ui.button(id!(recents_play_btn)).clicked(actions)
            && let Some(first) = self.recents_tracks.first().cloned()
        {
            self.queue_context = "Recents".to_string();
            self.queue
                .set_tracks((*self.recents_tracks).clone(), Some(0));
            self.play_track(cx, first);
        }
        if self.ui.button(id!(recents_clear_btn)).clicked(actions) {
            self.session_state.recent_track_ids.clear();
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.rebuild_library_views(cx);
            self.refresh_right_sidebar(cx);
            self.show_toast(cx, "Listening history cleared");
        }

        for (btn, flt) in [
            (id!(flt_all_btn), LibraryFilter::All),
            (id!(flt_liked_btn), LibraryFilter::Liked),
            (id!(flt_local_btn), LibraryFilter::Local),
            (id!(flt_spotify_btn), LibraryFilter::Spotify),
            (id!(flt_youtube_btn), LibraryFilter::YouTube),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.active_filter = flt;
                self.rebuild_library_views(cx);
            }
        }
        if self.ui.button(id!(sort_cycle_btn)).clicked(actions) {
            self.sort_field = self.sort_field.next();
            self.rebuild_library_views(cx);
        }
        if self.ui.button(id!(play_all_btn)).clicked(actions) {
            if let Some(first) = self.filtered_tracks.first().cloned() {
                self.queue_context = "Library".to_string();
                self.queue
                    .set_tracks((*self.filtered_tracks).clone(), Some(0));
                self.play_track(cx, first);
            } else {
                self.load_and_play_demo_suite(cx);
            }
        }
        if self.ui.button(id!(hero_shuffle_btn)).clicked(actions)
            && !self.filtered_tracks.is_empty()
        {
            self.queue_context = "Library Shuffle".to_string();
            self.queue
                .set_tracks((*self.filtered_tracks).clone(), Some(0));
            self.queue.set_shuffle(true);
            self.session_state.shuffle = true;
            self.sync_controls_from_state(cx);
            if let Some(first) = self.queue.current().cloned() {
                self.play_track(cx, first);
            }
        }

        // Albums grid paging + click
        if self.ui.button(id!(alb_prev_btn)).clicked(actions) && self.albums_page > 0 {
            self.albums_page -= 1;
            self.refresh_albums_grid(cx);
        }
        if self.ui.button(id!(alb_next_btn)).clicked(actions)
            && (self.albums_page + 1) * 8 < self.album_groups.len()
        {
            self.albums_page += 1;
            self.refresh_albums_grid(cx);
        }
        for (i, vid) in [
            id!(alb_0),
            id!(alb_1),
            id!(alb_2),
            id!(alb_3),
            id!(alb_4),
            id!(alb_5),
            id!(alb_6),
            id!(alb_7),
        ]
        .into_iter()
        .enumerate()
        {
            let v = self.ui.view(vid);
            if (v.ferric_cover_art(id!(art)).clicked(actions)
                || v.finger_up(actions)
                    .is_some_and(|fe| fe.is_over && fe.was_tap()))
                && let Some(g) = self.album_groups.get(self.albums_page * 8 + i).cloned()
            {
                self.open_local_detail(cx, g);
            }
        }

        // Artists grid paging + click
        if self.ui.button(id!(art_prev_btn)).clicked(actions) && self.artists_page > 0 {
            self.artists_page -= 1;
            self.refresh_artists_grid(cx);
        }
        if self.ui.button(id!(art_next_btn)).clicked(actions)
            && (self.artists_page + 1) * 8 < self.artist_groups.len()
        {
            self.artists_page += 1;
            self.refresh_artists_grid(cx);
        }
        for (i, vid) in [
            id!(art_0),
            id!(art_1),
            id!(art_2),
            id!(art_3),
            id!(art_4),
            id!(art_5),
            id!(art_6),
            id!(art_7),
        ]
        .into_iter()
        .enumerate()
        {
            let v = self.ui.view(vid);
            if (v.ferric_cover_art(id!(art)).clicked(actions)
                || v.finger_up(actions)
                    .is_some_and(|fe| fe.is_over && fe.was_tap()))
                && let Some(g) = self.artist_groups.get(self.artists_page * 8 + i).cloned()
            {
                self.open_local_detail(cx, g);
            }
        }

        // Playlists grid + Create + Import
        if self.ui.button(id!(pl_create_btn)).clicked(actions) {
            let name = self
                .ui
                .text_input(id!(pl_new_input))
                .text()
                .trim()
                .to_string();
            if !name.is_empty() {
                let pl = Playlist {
                    id: format!("local:pl:{}", fastrand::u32(..)),
                    name: name.clone(),
                    source: TrackSource::Local,
                    etag: None,
                    track_count: 0,
                    description: "Custom playlist".to_string(),
                    thumbnail_url: None,
                };
                if let Some(db) = &mut self.db {
                    let _ = db.upsert_playlist(&pl, &[]);
                    self.playlists = db.list_playlists().unwrap_or_default();
                } else {
                    self.playlists.push(pl);
                }
                self.ui.text_input(id!(pl_new_input)).set_text(cx, "");
                self.rebuild_library_views(cx);
                self.refresh_playlists_grid(cx);
                self.show_toast(cx, &format!("Created playlist '{name}'"));
            }
        }
        if self.ui.button(id!(pl_import_btn)).clicked(actions)
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
            self.rebuild_library_views(cx);
            self.refresh_playlists_grid(cx);
            self.show_toast(cx, &format!("Imported '{}'", pl.name));
        }
        if self.ui.button(id!(pl_prev_btn)).clicked(actions) && self.playlists_page > 0 {
            self.playlists_page -= 1;
            self.refresh_playlists_grid(cx);
        }
        if self.ui.button(id!(pl_next_btn)).clicked(actions)
            && (self.playlists_page + 1) * 8 < self.playlists.len()
        {
            self.playlists_page += 1;
            self.refresh_playlists_grid(cx);
        }
        for (i, vid) in [
            id!(plc_0),
            id!(plc_1),
            id!(plc_2),
            id!(plc_3),
            id!(plc_4),
            id!(plc_5),
            id!(plc_6),
            id!(plc_7),
        ]
        .into_iter()
        .enumerate()
        {
            let v = self.ui.view(vid);
            if (v.ferric_cover_art(id!(art)).clicked(actions)
                || v.finger_up(actions)
                    .is_some_and(|fe| fe.is_over && fe.was_tap()))
                && let Some(pl) = self.playlists.get(self.playlists_page * 8 + i).cloned()
            {
                let tracks = self
                    .db
                    .as_ref()
                    .and_then(|d| d.list_playlist_tracks(&pl.id).ok())
                    .unwrap_or_default();
                self.open_local_detail(
                    cx,
                    CollectionGroup {
                        id: pl.id,
                        title: pl.name,
                        subtitle: pl.description,
                        kind: YtmCardKind::Playlist,
                        thumbnail_url: pl.thumbnail_url,
                        tracks,
                    },
                );
            }
        }

        // Detail Page Buttons
        if self.ui.button(id!(detail_play_btn)).clicked(actions)
            && let Some(first) = self.detail_state.tracks.first().cloned()
        {
            self.queue_context = self.detail_state.title.clone();
            self.queue
                .set_tracks(self.detail_state.tracks.clone(), Some(0));
            self.play_track(cx, first);
        }
        if self.ui.button(id!(detail_shuffle_btn)).clicked(actions)
            && !self.detail_state.tracks.is_empty()
        {
            self.queue_context = self.detail_state.title.clone();
            self.queue
                .set_tracks(self.detail_state.tracks.clone(), Some(0));
            self.queue.set_shuffle(true);
            self.session_state.shuffle = true;
            self.sync_controls_from_state(cx);
            if let Some(first) = self.queue.current().cloned() {
                self.play_track(cx, first);
            }
        }
        if self.ui.button(id!(detail_radio_btn)).clicked(actions)
            && let Some(first) = self.detail_state.tracks.first().cloned()
        {
            self.queue_context = format!("{} radio", first.title);
            self.queue.set_tracks(vec![first.clone()], Some(0));
            self.play_track(cx, first.clone());
            self.maybe_fetch_radio_for_track(&first, true);
            self.show_toast(cx, &format!("Started radio for '{}'", first.title));
        }
        if self.ui.button(id!(detail_pin_btn)).clicked(actions) && !self.detail_state.id.is_empty()
        {
            let pinned = self.session_state.toggle_pin(&self.detail_state.id);
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.refresh_detail_hero(cx);
            self.show_toast(
                cx,
                if pinned {
                    "Pinned to sidebar"
                } else {
                    "Unpinned"
                },
            );
        }
        if self.ui.button(id!(detail_queue_btn)).clicked(actions)
            && !self.detail_state.tracks.is_empty()
        {
            let added = self.queue.append_tracks(self.detail_state.tracks.clone());
            self.sync_next_deck_to_engine();
            self.refresh_right_sidebar(cx);
            self.show_toast(cx, &format!("Added {added} songs to Up next"));
        }

        // Add Local Music Folder
        if (self.ui.button(id!(add_folder_btn)).clicked(actions)
            || self.ui.button(id!(sidebar_add_folder_btn)).clicked(actions))
            && let Some(folder) = rfd::FileDialog::new()
                .set_title("Select Local Music Folder")
                .pick_folder()
        {
            if let Some(db) = &self.db {
                let _ = db.add_music_folder(&folder);
            }
            self.spawn_folder_scan(vec![folder]);
        }

        // TrackList actions (routes automatically to active page's list!)
        let active_list = self.active_track_list();
        for action in actions {
            match action.cast() {
                TrackListAction::PlayIndex(idx) => {
                    if let Some(clicked) = active_list.get(idx).cloned() {
                        self.queue_context = match self.active_page {
                            MainPage::Search => "Search".to_string(),
                            MainPage::Recents => "Recents".to_string(),
                            MainPage::Detail => self.detail_state.title.clone(),
                            _ => "Library".to_string(),
                        };
                        self.queue.set_tracks((*active_list).clone(), Some(idx));
                        self.play_track(cx, clicked);
                    }
                }
                TrackListAction::ToggleLike(idx) => {
                    if let Some(target) = active_list.get(idx) {
                        let next_liked = !target.liked;
                        let tid = target.id.clone();
                        if let Some(db) = &self.db {
                            let _ = db.set_track_liked(&tid, next_liked);
                        }
                        if let Some(t) = self.all_tracks.iter_mut().find(|x| x.id == tid) {
                            t.liked = next_liked;
                        }
                        self.rebuild_library_views(cx);
                        self.show_toast(
                            cx,
                            if next_liked {
                                "Added to Liked Songs"
                            } else {
                                "Removed from Liked"
                            },
                        );
                    }
                }
                TrackListAction::AddToQueue(idx) => {
                    if let Some(target) = active_list.get(idx).cloned() {
                        let title = target.title.clone();
                        self.queue.insert_play_next(target);
                        self.sync_next_deck_to_engine();
                        self.prefetch_next_online_track();
                        self.refresh_right_sidebar(cx);
                        self.show_toast(cx, &format!("Playing next: '{title}'"));
                    }
                }
                TrackListAction::LoadDiscoverFeed => {
                    self.spawn_ytm_rich_home(cx, 0, None);
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
                TrackListAction::OpenYouTubeLogin => {
                    self.start_google_webview_login(cx);
                }
                TrackListAction::LoadDemoSuite => {
                    self.load_and_play_demo_suite(cx);
                }
                TrackListAction::None => {}
            }
        }

        // 6. RIGHT SIDEBAR (Queue / Lyrics Tabs + History + Up Next Jump!)
        if self.ui.button(id!(rs_tab_queue)).clicked(actions) {
            self.set_right_sidebar(cx, true, false);
        }
        if self.ui.button(id!(rs_tab_lyrics)).clicked(actions) {
            self.set_right_sidebar(cx, true, true);
        }
        if self.ui.button(id!(rs_radio_btn)).clicked(actions) {
            self.session_state.autoplay_radio = !self.session_state.autoplay_radio;
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.sync_controls_from_state(cx);
            self.refresh_right_sidebar(cx);
            if self.session_state.autoplay_radio
                && let Some(cur) = self.queue.current().cloned()
            {
                self.maybe_fetch_radio_for_track(&cur, true);
            }
            self.show_toast(
                cx,
                if self.session_state.autoplay_radio {
                    "Autoplay Radio enabled"
                } else {
                    "Autoplay Radio paused"
                },
            );
        }
        if self.ui.button(id!(rs_clear_btn)).clicked(actions) {
            if let Some(cur) = self.queue.current().cloned() {
                self.queue.set_tracks(vec![cur], Some(0));
            } else {
                self.queue.set_tracks(Vec::new(), None);
            }
            self.sync_next_deck_to_engine();
            self.refresh_right_sidebar(cx);
            self.show_toast(cx, "Up next queue cleared");
        }
        // History clicks in Right Sidebar
        let cur_id = self.queue.current().map(|t| t.id.clone());
        let hist_tracks: Vec<Track> = self
            .session_state
            .recent_track_ids
            .iter()
            .filter(|id| cur_id.as_deref() != Some(id.as_str()))
            .filter_map(|id| self.all_tracks.iter().find(|t| &t.id == id).cloned())
            .take(3)
            .collect();
        for (i, rid) in [id!(qh_0), id!(qh_1), id!(qh_2)].into_iter().enumerate() {
            let rv = self.ui.view(rid);
            if (rv.ferric_cover_art(id!(art)).clicked(actions)
                || rv
                    .finger_up(actions)
                    .is_some_and(|fe| fe.is_over && fe.was_tap()))
                && let Some(t) = hist_tracks.get(i).cloned()
            {
                self.queue.insert_play_next(t);
                if let Some(next) = self.queue.next_track().cloned() {
                    self.play_track(cx, next);
                }
            }
        }
        // Up next clicks in Right Sidebar (jump directly!)
        for (i, rid) in [
            id!(qu_0),
            id!(qu_1),
            id!(qu_2),
            id!(qu_3),
            id!(qu_4),
            id!(qu_5),
            id!(qu_6),
            id!(qu_7),
            id!(qu_8),
            id!(qu_9),
        ]
        .into_iter()
        .enumerate()
        {
            let rv = self.ui.view(rid);
            if (rv.ferric_cover_art(id!(art)).clicked(actions)
                || rv
                    .finger_up(actions)
                    .is_some_and(|fe| fe.is_over && fe.was_tap()))
                && let Some(target) = self.queue.jump_to_upcoming(i).cloned()
            {
                self.play_track(cx, target);
            }
        }

        // 7. BOTTOM PLAYER BAR & FULLSCREEN OVERLAY
        if self.ui.ferric_cover_art(id!(np_art)).clicked(actions)
            || self.ui.button(id!(bar_fullscreen_btn)).clicked(actions)
            || self.ui.button(id!(codec_pill_btn)).clicked(actions)
            || self.ui.button(id!(rs_lyr_expand)).clicked(actions)
        {
            self.set_fullscreen(cx, !self.fullscreen_open);
        }
        if self.ui.button(id!(fs_close_btn)).clicked(actions) {
            self.set_fullscreen(cx, false);
        }
        if self.ui.button(id!(bar_lyrics_btn)).clicked(actions) {
            if self.right_sidebar_open && self.right_lyrics_tab {
                self.set_right_sidebar(cx, false, false);
            } else {
                self.set_right_sidebar(cx, true, true);
            }
        }
        if self.ui.button(id!(bar_queue_btn)).clicked(actions) {
            if self.right_sidebar_open && !self.right_lyrics_tab {
                self.set_right_sidebar(cx, false, false);
            } else {
                self.set_right_sidebar(cx, true, false);
            }
        }
        if self
            .ui
            .ferric_play_btn(id!(play_pause_btn))
            .clicked(actions)
            && let Some(audio) = &self.audio
        {
            let snap = audio.telemetry();
            if !snap.is_playing && self.queue.current().is_none() {
                let pool = self.home_quick_picks_pool();
                if let Some(first) = pool.first().cloned() {
                    self.queue.set_tracks(pool, Some(0));
                    self.play_track(cx, first);
                } else {
                    self.load_and_play_demo_suite(cx);
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
            self.refresh_right_sidebar(cx);
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
            let accent = self.current_accent();
            let like_col = if next_liked {
                accent
            } else {
                vec4(0.439, 0.451, 0.510, 1.0)
            };
            self.ui
                .button(id!(np_like_btn))
                .apply_over(cx, live! { draw_icon: { color: (like_col) } });
            self.rebuild_library_views(cx);
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

        // Lyrics & Video Clip actions (Right Sidebar + Fullscreen)
        if self.ui.button(id!(rs_lyr_minus)).clicked(actions)
            || self.ui.button(id!(fs_lyr_minus)).clicked(actions)
        {
            self.lyrics_offset_ms -= 500;
            self.show_toast(cx, &format!("Lyrics offset: {} ms", self.lyrics_offset_ms));
        }
        if self.ui.button(id!(rs_lyr_plus)).clicked(actions)
            || self.ui.button(id!(fs_lyr_plus)).clicked(actions)
        {
            self.lyrics_offset_ms += 500;
            self.show_toast(cx, &format!("Lyrics offset: {} ms", self.lyrics_offset_ms));
        }
        if (self.ui.button(id!(rs_lyr_copy)).clicked(actions)
            || self.ui.button(id!(fs_lyr_copy)).clicked(actions))
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
            self.show_toast(cx, "Lyrics copied to clipboard");
        }
        if (self.ui.button(id!(rs_watch_video)).clicked(actions)
            || self.ui.button(id!(fs_video_btn)).clicked(actions))
            && let Some(cur) = self.queue.current()
        {
            let _ = open_track_video_clip(
                cur.path.as_deref(),
                cur.youtube_video_id.as_deref(),
                &cur.artist,
                &cur.title,
            );
        }
        if self.ui.button(id!(fs_radio_btn)).clicked(actions)
            && let Some(cur) = self.queue.current().cloned()
        {
            self.maybe_fetch_radio_for_track(&cur, true);
            self.show_toast(cx, &format!("Fetching radio for '{}'", cur.title));
        }

        // 8. POWERBAR MODAL (Ctrl+K)
        if self.ui.button(id!(cmdk_close_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.ui.redraw(cx);
        }
        if let Some((submitted, _)) = self.ui.text_input(id!(cmdk_input)).returned(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.ui.text_input(id!(cmdk_input)).set_text(cx, "");
            self.spawn_rich_search(cx, submitted);
        }
        for (btn, page) in [
            (id!(cmdk_home_btn), MainPage::Home),
            (id!(cmdk_search_btn), MainPage::Search),
            (id!(cmdk_songs_btn), MainPage::Songs),
            (id!(cmdk_albums_btn), MainPage::Albums),
            (id!(cmdk_artists_btn), MainPage::Artists),
            (id!(cmdk_playlists_btn), MainPage::Playlists),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.cmdk_open = false;
                self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
                self.navigate_to(cx, page, true);
            }
        }
        if self.ui.button(id!(cmdk_eq_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.navigate_to(cx, MainPage::Settings, true);
            self.switch_settings_tab(cx, SettingsTab::Equalizer);
        }
        if self.ui.button(id!(cmdk_fade_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.navigate_to(cx, MainPage::Settings, true);
            self.switch_settings_tab(cx, SettingsTab::Playback);
        }
        if self.ui.button(id!(cmdk_google_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.start_google_webview_login(cx);
        }
        if self.ui.button(id!(cmdk_fs_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.set_fullscreen(cx, true);
        }
        if self.ui.button(id!(cmdk_demo_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.load_and_play_demo_suite(cx);
        }

        // 9. SETTINGS TABS & ALL SETTINGS CONTROLS
        for (btn, tab) in [
            (id!(st_accounts_btn), SettingsTab::Accounts),
            (id!(st_playback_btn), SettingsTab::Playback),
            (id!(st_eq_btn), SettingsTab::Equalizer),
            (id!(st_appearance_btn), SettingsTab::Appearance),
            (id!(st_storage_btn), SettingsTab::Storage),
            (id!(st_connect_btn), SettingsTab::Connect),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.switch_settings_tab(cx, tab);
            }
        }

        // Accounts Tab
        if self.ui.button(id!(yt_login_btn)).clicked(actions) {
            self.start_google_webview_login(cx);
        }
        if self.ui.button(id!(yt_sync_btn)).clicked(actions) {
            self.sync_ytm_signed_in_library(cx);
        }
        if self.ui.button(id!(guest_refresh_btn)).clicked(actions) {
            self.spawn_ytm_rich_home(cx, 0, None);
        }
        if self.ui.button(id!(yt_logout_btn)).clicked(actions) {
            self.ytm_signed_out_recently = true;
            self.accounts.youtube_connected = false;
            self.accounts.youtube_cookie.clear();
            self.accounts.youtube_sapisid.clear();
            self.accounts.youtube_channel_name.clear();
            self.accounts.youtube_playlist_count = 0;
            if let Some(db) = &self.db {
                let _ = db.save_accounts(&self.accounts);
            }
            self.ui
                .label(id!(oauth_status_lbl))
                .set_text(cx, "Signed out. Guest Mode remains active.");
            self.rebuild_library_views(cx);
        }
        if self.ui.button(id!(quick_spotify_btn)).clicked(actions) {
            let input = self.ui.text_input(id!(quick_sync_input)).text();
            self.start_public_sync(cx, "Spotify", input);
        }
        if self.ui.button(id!(quick_youtube_btn)).clicked(actions) {
            let input = self.ui.text_input(id!(quick_sync_input)).text();
            self.start_public_sync(cx, "YouTube", input);
        }
        if self.ui.button(id!(match_local_btn)).clicked(actions)
            && let Some(db) = &mut self.db
        {
            let matched = db.match_cloud_tracks_to_local().unwrap_or(0);
            self.all_tracks = db.list_tracks("").unwrap_or_default();
            self.rebuild_library_views(cx);
            self.show_toast(
                cx,
                &format!("Matched {matched} cloud tracks to local files"),
            );
        }
        if self.ui.button(id!(rescan_btn)).clicked(actions)
            && let Some(db) = &self.db
            && let Ok(folders) = db.list_music_folders()
            && !folders.is_empty()
        {
            self.spawn_folder_scan(folders);
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

        // Playback & Fade-In Tab
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
        if let Some(on) = self.ui.ferric_toggle(id!(radio_toggle)).toggled(actions) {
            self.session_state.autoplay_radio = on;
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.refresh_right_sidebar(cx);
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
                .set_text(cx, "Sleep Timer: Stop after current song");
        }

        // Equalizer Tab (ALL 12 Sonora Presets + AutoEq + APO!)
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
        for (btn, preset) in [
            (id!(eq_pre_flat), "Flat"),
            (id!(eq_pre_bass), "Bass Boost"),
            (id!(eq_pre_bassred), "Bass Reducer"),
            (id!(eq_pre_treble), "Treble Boost"),
            (id!(eq_pre_vocal), "Vocal Clarity"),
            (id!(eq_pre_rock), "Rock"),
            (id!(eq_pre_pop), "Pop"),
            (id!(eq_pre_jazz), "Jazz"),
            (id!(eq_pre_classical), "Classical"),
            (id!(eq_pre_elec), "Electronic"),
            (id!(eq_pre_acoustic), "Acoustic"),
            (id!(eq_pre_loudness), "Loudness"),
            (id!(eq_pre_copper), "Warm Copper"),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.eq = EqConfig::preset(preset);
                self.sync_controls_from_state(cx);
                self.apply_eq_live(cx);
                self.show_toast(cx, &format!("EQ Preset: {preset}"));
            }
        }
        if self.ui.button(id!(autoeq_apply_btn)).clicked(actions) {
            let q = self.ui.text_input(id!(autoeq_input)).text();
            let hits = search_autoeq_profiles(&q);
            if let Some(best) = hits.first() {
                self.eq = best.to_eq_config();
                self.sync_controls_from_state(cx);
                self.apply_eq_live(cx);
                self.show_toast(cx, &format!("Applied AutoEq: {}", best.name));
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

        // Appearance Tab
        if self.ui.button(id!(pal_sonora_btn)).clicked(actions) {
            self.theme = ThemeConfig::default();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(pal_amoled_btn)).clicked(actions) {
            self.theme = ThemeConfig::pure_amoled();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(pal_cream_btn)).clicked(actions) {
            self.theme = ThemeConfig::cream_light();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(adaptive_art_toggle))
            .toggled(actions)
        {
            self.theme.adaptive_album_art = on;
            self.apply_theme_live(cx);
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

        // Storage, Updates & Connect Tabs
        if self.ui.button(id!(clear_audio_cache_btn)).clicked(actions) {
            clear_flat_cache_files(&app_data_dir().join("stream_cache"));
            self.refresh_storage_stats(cx);
            self.show_toast(cx, "Audio stream cache cleared");
        }
        if self.ui.button(id!(clear_art_cache_btn)).clicked(actions) {
            clear_flat_cache_files(&app_data_dir().join("artwork_cache"));
            if let Some(cache) = &mut self.artwork {
                cache.clear_memory();
            }
            self.refresh_storage_stats(cx);
            self.show_toast(cx, "Artwork cache cleared");
        }
        if self.ui.button(id!(load_demo_btn)).clicked(actions) {
            self.load_and_play_demo_suite(cx);
        }
        if self.ui.button(id!(connect_gen_pair_btn)).clicked(actions) {
            self.regenerate_pairing_qr(cx);
        }
        if self.ui.button(id!(connect_copy_pair_btn)).clicked(actions) {
            let _ = copy_text_to_clipboard(&self.pairing_uri);
            self.show_toast(cx, "Pairing URI copied");
        }
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
            let is_max = win32_is_active_window_maximized();
            self.ui
                .ferric_win_btn(id!(win_max_btn))
                .set_maximized(cx, is_max);
            self.sync_maximized_chrome_padding(cx, is_max);
        }

        // Native Windows 11 Frameless Drag & Aero Snap on empty regions of our 44px top_bar
        if let Event::WindowDragQuery(dq) = event
            && dq.abs.y >= 0.0
            && dq.abs.y <= 44.0
        {
            let left_r = self.ui.view(id!(top_left_grp)).area().rect(cx);
            let pill_r = self.ui.button(id!(top_search_pill)).area().rect(cx);
            let right_r = self.ui.view(id!(top_right_grp)).area().rect(cx);
            let in_left = left_r.size.x > 0.0 && dq.abs.x <= left_r.pos.x + left_r.size.x + 6.0;
            let in_pill = pill_r.size.x > 0.0
                && dq.abs.x >= pill_r.pos.x - 4.0
                && dq.abs.x <= pill_r.pos.x + pill_r.size.x + 4.0;
            let in_right = right_r.size.x > 0.0 && dq.abs.x >= right_r.pos.x - 6.0;
            if !in_left && !in_pill && !in_right {
                dq.response.set(WindowDragQueryResponse::Caption);
                return;
            }
        }

        if self.tick_timer.is_event(event).is_some() {
            self.handle_tick(cx);
        }

        // Full Sonora Keyboard Shortcuts
        if let Event::KeyDown(ke) = event {
            if ke.modifiers.control && ke.key_code == KeyCode::KeyK {
                self.cmdk_open = !self.cmdk_open;
                self.ui
                    .view(id!(cmdk_modal))
                    .set_visible(cx, self.cmdk_open);
                self.ui.redraw(cx);
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::KeyF {
                self.navigate_to(cx, MainPage::Search, true);
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::KeyQ {
                self.set_right_sidebar(cx, !self.right_sidebar_open, false);
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::KeyL {
                self.set_right_sidebar(cx, true, !self.right_lyrics_tab);
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::Comma {
                self.navigate_to(cx, MainPage::Settings, true);
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::KeyE {
                self.navigate_to(cx, MainPage::Settings, true);
                self.switch_settings_tab(cx, SettingsTab::Equalizer);
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::KeyM {
                self.toggle_compact_mini_mode(cx);
                return;
            }
            if ke.key_code == KeyCode::F11 {
                self.set_fullscreen(cx, !self.fullscreen_open);
                return;
            }
            if ke.modifiers.alt
                && ke.key_code == KeyCode::ArrowLeft
                && let Some(prev) = self.nav_history.as_mut().and_then(NavHistory::go_back)
            {
                self.navigate_to(cx, prev, false);
                return;
            }
            if ke.modifiers.alt
                && ke.key_code == KeyCode::ArrowRight
                && let Some(next) = self.nav_history.as_mut().and_then(NavHistory::go_forward)
            {
                self.navigate_to(cx, next, false);
                return;
            }
            if ke.modifiers.shift
                && ke.key_code == KeyCode::ArrowRight
                && let Some(next) = self.queue.next_track().cloned()
            {
                self.play_track(cx, next);
                return;
            }
            if ke.modifiers.shift
                && ke.key_code == KeyCode::ArrowLeft
                && let Some(prev) = self.queue.prev_track().cloned()
            {
                self.play_track(cx, prev);
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::ArrowUp {
                self.session_state.volume = (self.session_state.volume + 0.05).min(1.0);
                if let Some(a) = &self.audio {
                    a.send(EngineCommand::SetVolume(self.session_state.volume));
                }
                self.sync_controls_from_state(cx);
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::ArrowDown {
                self.session_state.volume = (self.session_state.volume - 0.05).max(0.0);
                if let Some(a) = &self.audio {
                    a.send(EngineCommand::SetVolume(self.session_state.volume));
                }
                self.sync_controls_from_state(cx);
                return;
            }
            if ke.key_code == KeyCode::Space
                && !ke.modifiers.control
                && !ke.modifiers.alt
                && !self.cmdk_open
                && (self.fullscreen_open
                    || !matches!(
                        self.active_page,
                        MainPage::Search | MainPage::Playlists | MainPage::Settings
                    ))
                && let Some(audio) = &self.audio
            {
                audio.send(EngineCommand::TogglePlayPause);
                return;
            }
            if ke.key_code == KeyCode::Escape {
                if self.cmdk_open {
                    self.cmdk_open = false;
                    self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
                    self.ui.redraw(cx);
                } else if self.fullscreen_open {
                    self.set_fullscreen(cx, false);
                } else if self.update_popup_visible {
                    self.dismiss_update_popup(cx);
                }
            }
        }

        let snap_opt = self.audio.as_ref().map(|a| a.telemetry());
        let avg_lvl = snap_opt.map_or(0.5_f32, |s| {
            ((s.spectrum_bins[0] + s.spectrum_bins[1]) as f32 / 2_000.0).clamp(0.1, 1.0)
        });
        let props = TrackListScope {
            tracks: self.active_track_list(),
            active_track_id: self.queue.current().map(|t| t.id.clone()),
            is_playing: snap_opt.is_some_and(|s| s.is_playing),
            audio_level: avg_lvl,
            accent: self.current_accent(),
            artworks: Rc::clone(&self.artwork_snapshot),
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

fn clear_flat_cache_files(dir: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                let _ = std::fs::remove_file(p);
            }
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
