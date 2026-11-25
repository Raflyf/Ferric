use crate::artwork::{ArtworkCache, CachedArtwork};
use crate::discord_rpc::{DiscordRpcBridge, DiscordTrackSnapshot, is_valid_client_id};
use crate::google_login::{GoogleLoginOutcome, launch_google_ytm_login_window};
use crate::shell::{
    TrayEvent, copy_text_to_clipboard, launch_verified_installer, open_allowed_url,
    open_github_url, win32_client_rect_screen, win32_close_active_window, win32_force_quit,
    win32_install_window_hooks, win32_is_active_window_maximized,
    win32_is_window_hidden_or_minimized, win32_main_hwnd_raw, win32_minimize_active_window,
    win32_poll_tray_events, win32_set_close_to_tray, win32_set_native_acrylic,
    win32_toggle_maximize_active_window, win32_trim_working_set, win32_update_taskbar_playback,
    win32_update_tray_tooltip,
};
use crate::smtc::{SmtcBridge, SmtcCommand};
use crate::state::{
    BgMessage, CollectionGroup, DetailViewState, LibraryFilter, MainPage, NavHistory, SearchFilter,
    SettingsTab, SleepTimerMode, SortField, build_album_groups, build_artist_groups, clip_text,
    dir_size_bytes, format_bytes_mb,
};
use crate::track_list::{TrackListAction, TrackListScope};
use crate::video_player::{NativeVideoPlayer, VideoViewportRect};
use crate::widgets::*;
use crate::{startup_load_demo, startup_page_override, startup_play_first};
use ferric_audio::demo_synth::ensure_demo_tracks;
use ferric_audio::engine::{AudioEngine, EngineCommand};
use ferric_core::app_data_dir;
use ferric_core::db::LibraryDb;
use ferric_core::eq::EqConfig;
use ferric_core::models::{
    AccountSettings, AudioQuality, DiscordRpcSettings, DiscordStatusFormat, FadeCurve,
    PlaybackDspSettings, Playlist, RepeatMode, SessionResumeState, Track, TrackSource,
    UpdateSettings, VideoQuality, format_duration_ms, parse_playlist_import_text,
};
use ferric_core::queue::PlayQueue;
use ferric_core::theme::{ThemeConfig, hex_to_rgba_f32, parse_hex_rgb};
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
    VideoStreamOption, YtmCardKind, YtmRichHome, YtmRichSearch, YtmSession, YtmShelfCard,
    fetch_account_library, fetch_browse_detail, fetch_home_continuation,
    fetch_radio_up_next_for_track, fetch_rich_home, fetch_rich_search, fetch_video_streams,
    normalize_ytm_thumbnail_url, resolve_and_cache_audio_stream_with_quality,
    resolve_best_video_id_for_track,
};
use ferric_sync::{
    ActiveSpeakerLease, LanDiscoveryBeacon, PairingPayload, QrMatrix, detect_local_lan_ip,
};
use makepad_widgets::*;
use std::cell::RefCell;
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

    // Smooth-scrolling vertical container (60 FPS damped mouse-wheel interpolation)
    FerricScrollYView = <ScrollYView> {
        scroll_bars: <ScrollBars> {
            show_scroll_x: false,
            show_scroll_y: true,
            scroll_bar_y: { smoothing: 0.16 }
        }
    }

    // Ferric Pure AMOLED Black surface that transitions cleanly into true per-pixel Windows 11 Acrylic
    FerricSurface = <View> {
        show_bg: true,
        draw_bg: {
            uniform base_col: #09090D
            uniform border_col: #1C1D25
            uniform acrylic_on: 0.0
            uniform acrylic_opacity: 0.72
            uniform acrylic_tint: #0B0D14
            uniform is_canvas: 0.0

            fn pixel(self) -> vec4 {
                if (self.acrylic_on < 0.5) {
                    return self.base_col;
                }
                if (self.is_canvas > 0.5) {
                    return vec4(0.0, 0.0, 0.0, 0.0);
                }
                let a = clamp(self.acrylic_opacity * 0.38, 0.07, 0.48);
                let tint = clamp(self.acrylic_tint.xyz * 1.18 + vec3(0.014, 0.014, 0.018), 0.0, 1.0);
                return vec4(tint * a, a);
            }
        }
    }

    // Centered 2-Row Ambient Bottom Player Bar (74px, crisp symmetry, per-pixel Acrylic ready)
    AmbientBottomBar = <View> {
        width: Fill,
        height: 74,
        padding: { left: 14, right: 14, top: 6, bottom: 6 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 10,
        show_bg: true,
        draw_bg: {
            uniform base_col: #08080C
            uniform accent_col: #E86A2C
            uniform glow_amt: 0.20
            uniform acrylic_on: 0.0
            uniform acrylic_opacity: 0.72
            uniform acrylic_tint: #0B0D14
            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                if (p.y < 1.0) {
                    if (self.acrylic_on > 0.5) {
                        return vec4(0.14, 0.14, 0.16, 0.16);
                    }
                    return #181A22;
                }
                let d_left = length((self.pos - vec2(0.08, 0.55)) * vec2(1.4, 2.2));
                let d_mid = length((self.pos - vec2(0.48, 0.85)) * vec2(0.9, 2.4));
                let wash = (exp(-d_left * d_left * 2.4) * 0.55 + exp(-d_mid * d_mid * 1.8) * 0.35) * self.glow_amt;
                if (self.acrylic_on > 0.5) {
                    let a = clamp(self.acrylic_opacity * 0.62, 0.20, 0.80);
                    let rgb = clamp(self.acrylic_tint.xyz * 1.12 + self.accent_col.xyz * wash, 0.0, 1.0);
                    return vec4(rgb * a, a);
                }
                let rgb = self.base_col.xyz + self.accent_col.xyz * wash;
                return vec4(clamp(rgb, 0.0, 1.0), 1.0);
            }
        }
    }

    FerricDividerH = <SolidView> {
        width: Fill,
        height: 1,
        draw_bg: { color: #191B23 }
    }

    FerricDividerV = <SolidView> {
        width: 1,
        height: Fill,
        draw_bg: { color: #191B23 }
    }

    FerricCard = <View> {
        width: Fill,
        height: Fit,
        padding: 14,
        show_bg: true,
        draw_bg: {
            uniform acrylic_on: 0.0
            uniform accent_col: #E86A2C
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 8.0);
                let solid_fill = vec4(0.052, 0.056, 0.076, 1.0);
                let acrylic_fill = vec4(1.0, 1.0, 1.0, 0.036);
                sdf.fill_keep(mix(solid_fill, acrylic_fill, self.acrylic_on));
                let solid_border = vec4(0.114, 0.122, 0.161, 1.0);
                let acrylic_border = vec4(1.0, 1.0, 1.0, 0.075);
                sdf.stroke(mix(solid_border, acrylic_border, self.acrylic_on), 1.0);
                return sdf.result;
            }
        }
    }

    FerricNavItem = <Button> {
        width: Fill,
        height: 28,
        padding: { left: 9, right: 8 }
        spacing: 9,
        align: { x: 0.0, y: 0.5 }
        icon_walk: { width: Fit, height: 14 }
        draw_icon: {
            curve: 1.15,
            linearize: 0.08,
            brightness: 1.05,
            color: #888B99,
            color_hover: #F4F4F7,
            color_focus: #F4F4F7,
            color_down: #FFFFFF
        }
        draw_text: {
            color: #A2A5B4,
            color_hover: #F4F4F7,
            color_focus: #F4F4F7,
            color_down: #FFFFFF,
            text_style: <THEME_FONT_MEDIUM> { font_size: 9.2 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            uniform acrylic_on: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 6.0);
                let hov = mix(#161822, vec4(1.0, 1.0, 1.0, 0.075), self.acrylic_on);
                let act = mix(#1E202F, vec4(1.0, 1.0, 1.0, 0.135), self.acrylic_on);
                let col = mix(mix(vec4(0.0, 0.0, 0.0, 0.0), hov, self.hover), act, self.active);
                sdf.fill_keep(col);
                let rim = vec4(1.0, 1.0, 1.0, (0.09 * self.active + 0.05 * self.hover) * self.acrylic_on);
                sdf.stroke(rim, 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.15 } }
                    apply: {
                        draw_bg: { down: 0.0, hover: 0.0 }
                        draw_icon: { down: 0.0, hover: 0.0 }
                        draw_text: { down: 0.0, hover: 0.0 }
                    }
                }
                on = {
                    from: { all: Forward { duration: 0.12 } }
                    apply: {
                        draw_bg: { down: 0.0, hover: 1.0 }
                        draw_icon: { down: 0.0, hover: 1.0 }
                        draw_text: { down: 0.0, hover: 1.0 }
                    }
                }
                down = {
                    from: { all: Forward { duration: 0.06 } }
                    apply: {
                        draw_bg: { down: 1.0, hover: 1.0 }
                        draw_icon: { down: 1.0, hover: 1.0 }
                        draw_text: { down: 1.0, hover: 1.0 }
                    }
                }
            }
        }
    }

    FerricPinBtn = <Button> {
        width: Fill,
        height: 25,
        padding: { left: 9, right: 8 }
        spacing: 6,
        align: { x: 0.0, y: 0.5 }
        draw_text: {
            color: #9A9CA8,
            color_hover: #F4F4F7,
            color_focus: #F4F4F7,
            color_down: #FFFFFF,
            text_style: <THEME_FONT_REGULAR> { font_size: 8.6 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            uniform acrylic_on: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 5.0);
                let hov = mix(#161822, vec4(1.0, 1.0, 1.0, 0.075), self.acrylic_on);
                let act = mix(#1E202F, vec4(1.0, 1.0, 1.0, 0.135), self.acrylic_on);
                let col = mix(mix(vec4(0.0, 0.0, 0.0, 0.0), hov, self.hover), act, self.active);
                sdf.fill(col);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.15 } }
                    apply: { draw_bg: { down: 0.0, hover: 0.0 }, draw_text: { down: 0.0, hover: 0.0 } }
                }
                on = {
                    from: { all: Forward { duration: 0.12 } }
                    apply: { draw_bg: { down: 0.0, hover: 1.0 }, draw_text: { down: 0.0, hover: 1.0 } }
                }
                down = {
                    from: { all: Forward { duration: 0.06 } }
                    apply: { draw_bg: { down: 1.0, hover: 1.0 }, draw_text: { down: 1.0, hover: 1.0 } }
                }
            }
        }
    }

    FerricPrimaryBtn = <Button> {
        width: Fit,
        height: 28,
        padding: { left: 12, right: 13 }
        spacing: 6,
        align: { x: 0.5, y: 0.5 }
        icon_walk: { width: Fit, height: 13 }
        draw_icon: {
            curve: 1.15,
            linearize: 0.08,
            brightness: 1.05,
            color: #09090D,
            color_hover: #09090D,
            color_focus: #09090D,
            color_down: #09090D
        }
        draw_text: {
            color: #09090D,
            color_hover: #09090D,
            color_focus: #09090D,
            color_down: #09090D,
            text_style: <THEME_FONT_BOLD> { font_size: 8.8 }
        }
        draw_bg: {
            instance hover: 0.0
            instance down: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5 + self.down * 0.65;
                let r = (self.rect_size.y - inset * 2.0) * 0.28;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, r);
                let c = mix(mix(#F2F3F7, #FFFFFF, self.hover), #D6D8E4, self.down);
                sdf.fill(c);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.14 } }
                    apply: { draw_bg: { down: 0.0, hover: 0.0 }, draw_icon: { down: 0.0, hover: 0.0 }, draw_text: { down: 0.0, hover: 0.0 } }
                }
                on = {
                    from: { all: Forward { duration: 0.11 } }
                    apply: { draw_bg: { down: 0.0, hover: 1.0 }, draw_icon: { down: 0.0, hover: 1.0 }, draw_text: { down: 0.0, hover: 1.0 } }
                }
                down = {
                    from: { all: Forward { duration: 0.05 } }
                    apply: { draw_bg: { down: 1.0, hover: 1.0 }, draw_icon: { down: 1.0, hover: 1.0 }, draw_text: { down: 1.0, hover: 1.0 } }
                }
            }
        }
    }

    FerricBtn = <Button> {
        width: Fit,
        height: 27,
        padding: { left: 10, right: 11 }
        spacing: 6,
        align: { x: 0.5, y: 0.5 }
        icon_walk: { width: Fit, height: 13 }
        draw_icon: {
            curve: 1.15,
            linearize: 0.08,
            brightness: 1.05,
            color: #9A9CA8,
            color_hover: #F4F4F7,
            color_focus: #F4F4F7,
            color_down: #E86A2C
        }
        draw_text: {
            color: #DCDCE6,
            color_hover: #FFFFFF,
            color_focus: #FFFFFF,
            color_down: #FFFFFF,
            text_style: <THEME_FONT_MEDIUM> { font_size: 8.6 }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            uniform acrylic_on: 0.0
            uniform accent_col: #E86A2C
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5 + self.down * 0.65;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 6.0);
                let solid_idle = mix(#14151D, #232737, self.hover);
                let solid_act = mix(#2E1B11, vec4(self.accent_col.xyz, 0.28), 0.36);
                let acrylic_idle = mix(vec4(1.0, 1.0, 1.0, 0.065), vec4(1.0, 1.0, 1.0, 0.16), self.hover);
                let acrylic_act = vec4(self.accent_col.xyz, 0.30);
                let bg = mix(mix(solid_idle, solid_act, self.active), mix(acrylic_idle, acrylic_act, self.active), self.acrylic_on);
                let pressed = mix(bg, vec4(1.0, 1.0, 1.0, 0.18), self.down * 0.24);
                sdf.fill_keep(pressed);
                let solid_b = mix(mix(#252734, #484E68, self.hover), vec4(self.accent_col.xyz, 0.88), self.active);
                let acrylic_b = mix(vec4(1.0, 1.0, 1.0, 0.10 + 0.12 * self.hover), vec4(self.accent_col.xyz, 0.90), self.active);
                sdf.stroke(mix(solid_b, acrylic_b, self.acrylic_on), 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.15 } }
                    apply: { draw_bg: { down: 0.0, hover: 0.0 }, draw_icon: { down: 0.0, hover: 0.0 }, draw_text: { down: 0.0, hover: 0.0 } }
                }
                on = {
                    from: { all: Forward { duration: 0.12 } }
                    apply: { draw_bg: { down: 0.0, hover: 1.0 }, draw_icon: { down: 0.0, hover: 1.0 }, draw_text: { down: 0.0, hover: 1.0 } }
                }
                down = {
                    from: { all: Forward { duration: 0.05 } }
                    apply: { draw_bg: { down: 1.0, hover: 1.0 }, draw_icon: { down: 1.0, hover: 1.0 }, draw_text: { down: 1.0, hover: 1.0 } }
                }
            }
        }
    }

    FerricChipBtn = <Button> {
        width: Fit,
        height: 24,
        padding: { left: 9, right: 9 }
        spacing: 0,
        align: { x: 0.5, y: 0.5 }
        draw_text: {
            instance active: 0.0
            color: #C8CAD6,
            color_hover: #FFFFFF,
            color_focus: #FFFFFF,
            color_down: #FFFFFF,
            text_style: <THEME_FONT_MEDIUM> { font_size: 8.5 }
            fn get_color(self) -> vec4 {
                let idle_c = mix(self.color, #FFFFFF, max(self.hover, self.down));
                return mix(idle_c, #09090D, self.active);
            }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            uniform acrylic_on: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5 + self.down * 0.65;
                let r = (self.rect_size.y - inset * 2.0) * 0.28;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, r);
                let solid_idle = mix(#14151D, #25293A, self.hover);
                let acrylic_idle = mix(vec4(1.0, 1.0, 1.0, 0.065), vec4(1.0, 1.0, 1.0, 0.17), self.hover);
                let idle = mix(solid_idle, acrylic_idle, self.acrylic_on);
                let bg = mix(idle, mix(#F4F5FA, #FFFFFF, self.hover), self.active);
                sdf.fill_keep(bg);
                let idle_b = mix(mix(#242633, #464B66, self.hover), vec4(1.0, 1.0, 1.0, 0.12 + 0.12 * self.hover), self.acrylic_on);
                sdf.stroke(mix(idle_b, #FFFFFF, self.active), 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.15 } }
                    apply: { draw_bg: { down: 0.0, hover: 0.0 }, draw_text: { down: 0.0, hover: 0.0 } }
                }
                on = {
                    from: { all: Forward { duration: 0.12 } }
                    apply: { draw_bg: { down: 0.0, hover: 1.0 }, draw_text: { down: 0.0, hover: 1.0 } }
                }
                down = {
                    from: { all: Forward { duration: 0.05 } }
                    apply: { draw_bg: { down: 1.0, hover: 1.0 }, draw_text: { down: 1.0, hover: 1.0 } }
                }
            }
        }
    }

    FerricIconBtn = <Button> {
        width: 28,
        height: 28,
        padding: 0,
        spacing: 0,
        align: { x: 0.5, y: 0.5 }
        text: ""
        label_walk: { width: 0, height: 0 }
        icon_walk: { width: Fit, height: 14 }
        draw_icon: {
            curve: 1.15,
            linearize: 0.08,
            brightness: 1.05,
            color: #9A9CA8,
            color_hover: #F4F4F7,
            color_focus: #F4F4F7,
            color_down: #E86A2C
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            uniform acrylic_on: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 1.0 + self.down * 0.85;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 6.0);
                let hov = mix(#212432, vec4(1.0, 1.0, 1.0, 0.13), self.acrylic_on);
                let act = mix(#321E14, vec4(1.0, 1.0, 1.0, 0.19), self.acrylic_on);
                let bg = mix(mix(vec4(0.0, 0.0, 0.0, 0.0), hov, self.hover), act, self.active);
                sdf.fill_keep(mix(bg, vec4(1.0, 1.0, 1.0, 0.16), self.down * 0.25));
                let rim = vec4(1.0, 1.0, 1.0, (0.11 * self.hover + 0.14 * self.active));
                sdf.stroke(rim, 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.14 } }
                    apply: { draw_bg: { down: 0.0, hover: 0.0 }, draw_icon: { down: 0.0, hover: 0.0 } }
                }
                on = {
                    from: { all: Forward { duration: 0.11 } }
                    apply: { draw_bg: { down: 0.0, hover: 1.0 }, draw_icon: { down: 0.0, hover: 1.0 } }
                }
                down = {
                    from: { all: Forward { duration: 0.05 } }
                    apply: { draw_bg: { down: 1.0, hover: 1.0 }, draw_icon: { down: 1.0, hover: 1.0 } }
                }
            }
        }
    }

    FerricInput = <TextInput> {
        width: Fill,
        height: 32,
        margin: 0,
        padding: { left: 12, right: 12, top: 0, bottom: 0 }
        flow: Right,
        align: { x: 0.0, y: 0.5 }
        label_align: { x: 0.0, y: 0.5 }
        draw_bg: {
            color: #090A0F,
            color_empty: #090A0F,
            color_hover: #10121A,
            color_focus: #12141F,
            border_radius: 7.0,
            border_size: 1.0,
            border_color_1: #232634,
            border_color_2: #232634,
            border_color_1_empty: #232634,
            border_color_2_empty: #232634,
            border_color_1_hover: #3B3F56,
            border_color_2_hover: #3B3F56,
            border_color_1_focus: #E86A2C,
            border_color_2_focus: #E86A2C
        }
        draw_text: {
            color: #F4F4F7,
            color_empty: #6D7080,
            text_style: <THEME_FONT_REGULAR> { font_size: 9.2, line_spacing: 1.0 }
        }
        draw_cursor: { color: #E86A2C }
    }

    // Compact 2x4 Home Quick Picks Song Tile (48px, crisp Inter scale, tactile hover/press)
    QuickPickTile = <View> {
        width: Fill,
        height: 48,
        padding: { left: 6, right: 10, top: 4, bottom: 4 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 9,
        show_bg: true,
        cursor: Hand,
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            uniform acrylic_on: 0.0
            uniform accent_col: #E86A2C
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5 + self.down * 1.1;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 7.0);
                let solid_idle = mix(vec4(0.050, 0.054, 0.074, 1.0), vec4(0.112, 0.120, 0.168, 1.0), self.hover);
                let solid_act = mix(vec4(0.105, 0.098, 0.175, 1.0), vec4(self.accent_col.xyz, 0.24), 0.36);
                let acr_idle = mix(vec4(1.0, 1.0, 1.0, 0.030), vec4(1.0, 1.0, 1.0, 0.095), self.hover);
                let acr_act = vec4(mix(vec3(0.14, 0.14, 0.22), self.accent_col.xyz, 0.34), 0.36);
                let col = mix(mix(solid_idle, solid_act, self.active), mix(acr_idle, acr_act, self.active), self.acrylic_on);
                let sheen = smoothstep(0.55, 0.0, self.pos.y) * self.hover * 0.065;
                let lifted = vec4(clamp(col.xyz + vec3(sheen), 0.0, 1.0), col.w);
                let pressed = mix(lifted, vec4(1.0, 1.0, 1.0, 0.16), self.down * 0.22);
                sdf.fill_keep(pressed);
                let hov_rim = mix(#3D435E, self.accent_col, 0.24);
                let solid_b = mix(mix(#1A1C26, hov_rim, self.hover), vec4(self.accent_col.xyz, 0.78), self.active);
                let acr_b = mix(vec4(1.0, 1.0, 1.0, 0.055 + 0.14 * self.hover), vec4(self.accent_col.xyz, 0.68), self.active);
                sdf.stroke(mix(solid_b, acr_b, self.acrylic_on), 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = { from: { all: Forward { duration: 0.20 } } apply: { draw_bg: { hover: 0.0 } } }
                on = { from: { all: Forward { duration: 0.11 } } apply: { draw_bg: { hover: 1.0 } } }
            }
            down = {
                default: off,
                off = { from: { all: Forward { duration: 0.12 } } apply: { draw_bg: { down: 0.0 } } }
                on = { from: { all: Forward { duration: 0.05 } } apply: { draw_bg: { down: 1.0 } } }
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
                flow: Right,
                padding: 0,
                draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 9.4, line_spacing: 1.0 } }
                text: ""
            }
            sub = <Label> {
                width: Fill,
                flow: Right,
                padding: 0,
                draw_text: { color: #9A9EAE, text_style: <THEME_FONT_REGULAR> { font_size: 8.2, line_spacing: 1.0 } }
                text: ""
            }
        }
    }

    // Sleek Horizontal Album / Playlist / Chart Shelf Card (174px, tactile hover lift)
    ShelfCard = <View> {
        width: Fill,
        height: 174,
        padding: 8,
        flow: Down,
        spacing: 6,
        show_bg: true,
        cursor: Hand,
        draw_bg: {
            instance hover: 0.0
            instance down: 0.0
            uniform ghost: 0.0
            uniform acrylic_on: 0.0
            uniform accent_col: #E86A2C
            fn pixel(self) -> vec4 {
                if self.ghost > 0.5 {
                    return vec4(0.0, 0.0, 0.0, 0.0);
                }
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5 + self.down * 1.4;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 8.0);
                let solid_fill = mix(vec4(0.052, 0.056, 0.076, 1.0), vec4(0.118, 0.126, 0.174, 1.0), self.hover);
                let acr_fill = mix(vec4(1.0, 1.0, 1.0, 0.032), vec4(1.0, 1.0, 1.0, 0.095), self.hover);
                let fill = mix(solid_fill, acr_fill, self.acrylic_on);
                let sheen = smoothstep(0.45, 0.0, self.pos.y) * self.hover * 0.075;
                let lifted = vec4(clamp(fill.xyz + vec3(sheen), 0.0, 1.0), fill.w);
                let pressed = mix(lifted, vec4(1.0, 1.0, 1.0, 0.14), self.down * 0.22);
                sdf.fill_keep(pressed);
                let hov_rim = mix(#434966, self.accent_col, 0.28);
                let solid_b = mix(#1B1D27, hov_rim, self.hover);
                let acr_b = vec4(1.0, 1.0, 1.0, 0.058 + 0.15 * self.hover);
                sdf.stroke(mix(solid_b, acr_b, self.acrylic_on), 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = { from: { all: Forward { duration: 0.22 } } apply: { draw_bg: { hover: 0.0 } } }
                on = { from: { all: Forward { duration: 0.12 } } apply: { draw_bg: { hover: 1.0 } } }
            }
            down = {
                default: off,
                off = { from: { all: Forward { duration: 0.12 } } apply: { draw_bg: { down: 0.0 } } }
                on = { from: { all: Forward { duration: 0.05 } } apply: { draw_bg: { down: 1.0 } } }
            }
        }
        inner = <View> {
            width: Fill,
            height: Fill,
            flow: Down,
            spacing: 6,
            art = <FerricCoverArt> {
                width: Fill,
                height: 116,
                draw_bg: { radius: 7.0 }
            }
            <View> {
                width: Fill,
                height: Fit,
                flow: Down,
                spacing: 2,
                title = <Label> {
                    width: Fill,
                    flow: Right,
                    padding: 0,
                    draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 9.0, line_spacing: 1.0 } }
                    text: ""
                }
                sub = <Label> {
                    width: Fill,
                    flow: Right,
                    padding: 0,
                    draw_text: { color: #969AAC, text_style: <THEME_FONT_REGULAR> { font_size: 8.0, line_spacing: 1.0 } }
                    text: ""
                }
            }
        }
    }

    // Visual Grid Card for Albums / Artists / Playlists pages (5-col balanced, ghost-spacer safe)
    GridCard = <View> {
        width: Fill,
        height: 194,
        padding: 8,
        flow: Down,
        spacing: 6,
        show_bg: true,
        cursor: Hand,
        draw_bg: {
            instance hover: 0.0
            instance down: 0.0
            uniform ghost: 0.0
            uniform acrylic_on: 0.0
            uniform accent_col: #E86A2C
            fn pixel(self) -> vec4 {
                if self.ghost > 0.5 {
                    return vec4(0.0, 0.0, 0.0, 0.0);
                }
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5 + self.down * 1.4;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 8.0);
                let solid_fill = mix(vec4(0.052, 0.056, 0.076, 1.0), vec4(0.118, 0.126, 0.174, 1.0), self.hover);
                let acr_fill = mix(vec4(1.0, 1.0, 1.0, 0.032), vec4(1.0, 1.0, 1.0, 0.095), self.hover);
                let fill = mix(solid_fill, acr_fill, self.acrylic_on);
                let sheen = smoothstep(0.45, 0.0, self.pos.y) * self.hover * 0.075;
                let lifted = vec4(clamp(fill.xyz + vec3(sheen), 0.0, 1.0), fill.w);
                let pressed = mix(lifted, vec4(1.0, 1.0, 1.0, 0.14), self.down * 0.22);
                sdf.fill_keep(pressed);
                let hov_rim = mix(#434966, self.accent_col, 0.28);
                let solid_b = mix(#1C1E28, hov_rim, self.hover);
                let acr_b = vec4(1.0, 1.0, 1.0, 0.058 + 0.15 * self.hover);
                sdf.stroke(mix(solid_b, acr_b, self.acrylic_on), 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = { from: { all: Forward { duration: 0.22 } } apply: { draw_bg: { hover: 0.0 } } }
                on = { from: { all: Forward { duration: 0.12 } } apply: { draw_bg: { hover: 1.0 } } }
            }
            down = {
                default: off,
                off = { from: { all: Forward { duration: 0.12 } } apply: { draw_bg: { down: 0.0 } } }
                on = { from: { all: Forward { duration: 0.05 } } apply: { draw_bg: { down: 1.0 } } }
            }
        }
        inner = <View> {
            width: Fill,
            height: Fill,
            flow: Down,
            spacing: 6,
            art = <FerricCoverArt> {
                width: Fill,
                height: 136,
                draw_bg: { radius: 8.0 }
            }
            <View> {
                width: Fill,
                height: Fit,
                flow: Down,
                spacing: 2,
                title = <Label> {
                    width: Fill,
                    flow: Right,
                    padding: 0,
                    draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 9.0, line_spacing: 1.0 } }
                    text: ""
                }
                sub = <Label> {
                    width: Fill,
                    flow: Right,
                    padding: 0,
                    draw_text: { color: #969AAC, text_style: <THEME_FONT_REGULAR> { font_size: 8.0, line_spacing: 1.0 } }
                    text: ""
                }
            }
        }
    }

    // Compact Right Sidebar Queue / History Row (44px, Inter scale, crisp hover)
    QueueMiniRow = <View> {
        width: Fill,
        height: 44,
        padding: { left: 7, right: 8, top: 4, bottom: 4 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 6,
        show_bg: true,
        cursor: Hand,
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            uniform accent_col: #E86A2C
            uniform acrylic_on: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 6.0);
                let solid_idle = mix(vec4(0.0, 0.0, 0.0, 0.0), vec4(0.095, 0.102, 0.140, 1.0), self.hover);
                let acr_idle = mix(vec4(1.0, 1.0, 1.0, 0.024), vec4(1.0, 1.0, 1.0, 0.090), self.hover);
                let idle = mix(solid_idle, acr_idle, self.acrylic_on);
                let act_solid = mix(#1A1928, vec4(self.accent_col.xyz, 0.24), 0.34);
                let act_acr = vec4(mix(vec3(0.14, 0.14, 0.22), self.accent_col.xyz, 0.34), 0.36);
                sdf.fill_keep(mix(idle, mix(act_solid, act_acr, self.acrylic_on), self.active));
                let i_rim = vec4(1.0, 1.0, 1.0, (0.045 + 0.08 * self.hover) * self.acrylic_on);
                let a_rim = vec4(self.accent_col.xyz, 0.62);
                sdf.stroke(mix(i_rim, a_rim, self.active), 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = { from: { all: Forward { duration: 0.15 } } apply: { draw_bg: { hover: 0.0 } } }
                on = { from: { all: Forward { duration: 0.11 } } apply: { draw_bg: { hover: 1.0 } } }
            }
            down = {
                default: off,
                off = { from: { all: Forward { duration: 0.10 } } apply: { draw_bg: { down: 0.0 } } }
                on = { from: { all: Forward { duration: 0.05 } } apply: { draw_bg: { down: 1.0 } } }
            }
        }
        art = <FerricCoverArt> {
            width: 32,
            height: 32,
            draw_bg: { radius: 5.0 }
        }
        <View> {
            width: Fill,
            height: Fit,
            flow: Down,
            spacing: 2,
            title = <Label> {
                width: Fill,
                flow: Right,
                padding: 0,
                draw_text: { color: #ECEEF5, text_style: <THEME_FONT_BOLD> { font_size: 9.0, line_spacing: 1.0 } }
                text: ""
            }
            sub = <Label> {
                width: Fill,
                flow: Right,
                padding: 0,
                draw_text: { color: #969AAC, text_style: <THEME_FONT_REGULAR> { font_size: 8.0, line_spacing: 1.0 } }
                text: ""
            }
        }
        dur = <Label> {
            width: Fit,
            flow: Right,
            padding: 0,
            draw_text: { color: #888CA0, text_style: <THEME_FONT_MEDIUM> { font_size: 8.0, line_spacing: 1.0 } }
            text: ""
        }
        up_btn = <Button> {
            visible: false,
            width: 18, height: 18, padding: 0, spacing: 0,
            align: { x: 0.5, y: 0.5 }
            text: ""
            label_walk: { width: 0, height: 0 }
            icon_walk: { width: 11, height: 11 }
            draw_icon: {
                curve: 1.15,
                linearize: 0.08,
                brightness: 1.05,
                svg_path: "M7.41 15.41L12 10.83L16.59 15.41L18 14L12 8L6 14L7.41 15.41Z"
                color: #6B6F82
                color_hover: #F4F4F7
                color_down: #E86A2C
            }
            draw_bg: {
                instance hover: 0.0
                fn pixel(self) -> vec4 {
                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                    sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 4.0);
                    sdf.fill(vec4(1.0, 1.0, 1.0, 0.12 * self.hover));
                    return sdf.result;
                }
            }
            animator: {
                hover = {
                    default: off,
                    off = { from: { all: Forward { duration: 0.14 } } apply: { draw_bg: { hover: 0.0 }, draw_icon: { hover: 0.0 } } }
                    on = { from: { all: Forward { duration: 0.10 } } apply: { draw_bg: { hover: 1.0 }, draw_icon: { hover: 1.0 } } }
                }
            }
        }
        rm_btn = <Button> {
            visible: false,
            width: 18, height: 18, padding: 0, spacing: 0,
            align: { x: 0.5, y: 0.5 }
            text: ""
            label_walk: { width: 0, height: 0 }
            icon_walk: { width: 11, height: 11 }
            draw_icon: {
                curve: 1.15,
                linearize: 0.08,
                brightness: 1.05,
                svg_path: "M19 6.41L17.59 5L12 10.59L6.41 5L5 6.41L10.59 12L5 17.59L6.41 19L12 13.41L17.59 19L19 17.59L13.41 12L19 6.41Z"
                color: #6B6F82
                color_hover: #FF6B7A
                color_down: #FF4D5E
            }
            draw_bg: {
                instance hover: 0.0
                fn pixel(self) -> vec4 {
                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                    sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 4.0);
                    sdf.fill(vec4(0.86, 0.22, 0.26, 0.24 * self.hover));
                    return sdf.result;
                }
            }
            animator: {
                hover = {
                    default: off,
                    off = { from: { all: Forward { duration: 0.14 } } apply: { draw_bg: { hover: 0.0 }, draw_icon: { hover: 0.0 } } }
                    on = { from: { all: Forward { duration: 0.10 } } apply: { draw_bg: { hover: 1.0 }, draw_icon: { hover: 1.0 } } }
                }
            }
        }
    }

    App = {{App}} {
        ui: <Root> {
            main_window = <Window> {
                window: { title: "Ferric", inner_size: vec2(1280, 780) }
                pass: { clear_color: #00000000 }
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
                        uniform acrylic_on: 0.0
                        uniform acrylic_opacity: 0.72
                        uniform acrylic_tint: #0B0D14
                        uniform accent_col: #E86A2C
                        fn pixel(self) -> vec4 {
                            if (self.acrylic_on < 0.5) {
                                return self.canvas_col;
                            }
                            let d1 = length((self.pos - vec2(0.14, 0.14)) * vec2(0.95, 1.15));
                            let subtle = exp(-d1 * d1 * 3.2) * self.accent_col.xyz * 0.065;
                            let a = clamp(self.acrylic_opacity * 0.78, 0.14, 0.90);
                            let rgb = clamp(self.acrylic_tint.xyz + subtle, 0.0, 1.0);
                            return vec4(rgb * a, a);
                        }
                    }

                    // ================================================================
                    // MAIN FERRIC 3-PANE DESKTOP SHELL
                    // ================================================================
                    shell_col = <View> {
                        width: Fill,
                        height: Fill,
                        flow: Down,

                        // 1. MINIMAL TOP CHROME (36px)
                        top_bar = <FerricSurface> {
                            width: Fill,
                            height: 36,
                            padding: { left: 8, right: 6 }
                            flow: Right,
                            align: { y: 0.5 }
                            spacing: 4,
                            draw_bg: { base_col: #09090D }

                            top_left_grp = <View> {
                                width: Fit,
                                height: Fill,
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 3,

                                <View> { width: 2, height: 1 }
                                brand_icon = <Image> {
                                    width: 17,
                                    height: 17,
                                    fit: Smallest
                                }
                                <Label> {
                                    margin: { left: 3, right: 6 }
                                    padding: 0,
                                    draw_text: { color: #EEF0F8, text_style: <THEME_FONT_BOLD> { font_size: 9.8 } }
                                    text: "Ferric"
                                }
                                <SolidView> { width: 1, height: 13, margin: { left: 1, right: 4 }, draw_bg: { color: #242634 } }
                                toggle_left_btn = <FerricIconBtn> {
                                    width: 26, height: 26,
                                    draw_icon: { svg_path: "M3 5H21V19H3V5ZM5 7V17H9V7H5ZM11 7V17H19V7H11Z" }
                                }
                                nav_back_btn = <FerricIconBtn> {
                                    width: 26, height: 26,
                                    draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" }
                                }
                                nav_fwd_btn = <FerricIconBtn> {
                                    width: 26, height: 26,
                                    draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" }
                                }
                                refresh_btn = <FerricIconBtn> {
                                    width: 26, height: 26,
                                    draw_icon: { svg_path: "M17.65 6.35C16.2 4.9 14.21 4 12 4C7.58 4 4.01 7.58 4.01 12C4.01 16.42 7.58 20 12 20C15.73 20 18.84 17.45 19.73 14H17.65C16.83 16.33 14.61 18 12 18C8.69 18 6 15.31 6 12C6 8.69 8.69 6 12 6C13.66 6 15.14 6.69 16.22 7.78L13 11H20V4L17.65 6.35Z" }
                                }
                            }

                            // Center drag area + inline direct search bar (no modal popup)
                            <View> {
                                width: Fill,
                                height: Fill,
                                align: { x: 0.5, y: 0.5 }
                                top_search_box = <View> {
                                    width: 268,
                                    height: 26,
                                    padding: { left: 9, right: 6, top: 0, bottom: 0 }
                                    flow: Right,
                                    align: { y: 0.5 }
                                    spacing: 5,
                                    show_bg: true,
                                    draw_bg: {
                                        instance hover: 0.0
                                        uniform acrylic_on: 0.0
                                        uniform accent_col: #E86A2C
                                        fn pixel(self) -> vec4 {
                                            let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                            let r = (self.rect_size.y - 1.0) * 0.30;
                                            sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);
                                            let solid_c = mix(#07080C, #12141F, self.hover);
                                            let acr_c = mix(vec4(1.0, 1.0, 1.0, 0.065), vec4(1.0, 1.0, 1.0, 0.13), self.hover);
                                            sdf.fill_keep(mix(solid_c, acr_c, self.acrylic_on));
                                            let b_col = mix(mix(#212433, #3A3F57, self.hover), vec4(1.0, 1.0, 1.0, 0.14 + 0.08 * self.hover), self.acrylic_on);
                                            sdf.stroke(b_col, 1.0);
                                            return sdf.result;
                                        }
                                    }
                                    animator: {
                                        hover = {
                                            default: off,
                                            off = { from: { all: Forward { duration: 0.15 } } apply: { draw_bg: { hover: 0.0 } } }
                                            on = { from: { all: Forward { duration: 0.11 } } apply: { draw_bg: { hover: 1.0 } } }
                                        }
                                    }
                                    <Icon> {
                                        icon_walk: { width: 13, height: 13 }
                                        draw_icon: {
                                            curve: 1.15,
                                            linearize: 0.08,
                                            brightness: 1.05,
                                            svg_path: "M15.5 14H14.71L14.43 13.73C15.41 12.59 16 11.11 16 9.5C16 5.91 13.09 3 9.5 3C5.91 3 3 5.91 3 9.5C3 13.09 5.91 16 9.5 16C11.11 16 12.59 15.41 13.73 14.43L14 14.71V15.5L19 20.49L20.49 19L15.5 14ZM9.5 14C7.01 14 5 11.99 5 9.5C5 7.01 7.01 5 9.5 5C11.99 5 14 7.01 14 9.5C14 11.99 11.99 14 9.5 14Z"
                                            color: #7D8090
                                        }
                                    }
                                    top_search_input = <FerricInput> {
                                        width: Fill,
                                        height: Fill,
                                        padding: { left: 2, right: 6, top: 0, bottom: 0 }
                                        empty_text: "Search music... (Ctrl+K)"
                                        draw_bg: {
                                            color: #00000000,
                                            color_empty: #00000000,
                                            color_hover: #00000000,
                                            color_focus: #00000000,
                                            border_size: 0.0,
                                            border_color_1: #00000000,
                                            border_color_2: #00000000,
                                            border_color_1_empty: #00000000,
                                            border_color_2_empty: #00000000,
                                            border_color_1_hover: #00000000,
                                            border_color_2_hover: #00000000,
                                            border_color_1_focus: #00000000,
                                            border_color_2_focus: #00000000
                                        }
                                        draw_text: {
                                            color: #F4F4F7,
                                            color_empty: #7D8090,
                                            text_style: <THEME_FONT_REGULAR> { font_size: 8.6, line_spacing: 1.0 }
                                        }
                                    }
                                }
                            }

                            top_right_grp = <View> {
                                width: Fit,
                                height: Fill,
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 3,

                                update_badge_btn = <FerricPrimaryBtn> {
                                    visible: false,
                                    height: 22,
                                    padding: { left: 8, right: 8 }
                                    text: "Update"
                                    draw_text: { text_style: { font_size: 8.2 } }
                                }
                                top_account_btn = <FerricBtn> {
                                    height: 24,
                                    padding: { left: 8, right: 9 }
                                    text: "Sign in"
                                    draw_icon: {
                                        svg_path: "M12 12C14.21 12 16 10.21 16 8C16 5.79 14.21 4 12 4C9.79 4 8 5.79 8 8C8 10.21 9.79 12 12 12ZM12 14C9.33 14 4 15.34 4 18V20H20V18C20 15.34 14.67 14 12 14Z"
                                        color: #E86A2C
                                    }
                                    draw_text: { text_style: { font_size: 8.4 } }
                                }
                                top_settings_btn = <FerricIconBtn> {
                                    width: 26, height: 26,
                                    draw_icon: { svg_path: "M19.14 12.94C19.18 12.64 19.2 12.33 19.2 12C19.2 11.68 19.18 11.36 19.13 11.06L21.16 9.48C21.34 9.34 21.39 9.07 21.28 8.87L19.36 5.55C19.24 5.33 18.99 5.26 18.77 5.33L16.38 6.29C15.88 5.91 15.35 5.59 14.76 5.35L14.4 2.81C14.36 2.57 14.16 2.4 13.92 2.4H10.08C9.84 2.4 9.65 2.57 9.61 2.81L9.25 5.35C8.66 5.59 8.12 5.92 7.63 6.29L5.24 5.33C5.02 5.25 4.77 5.33 4.65 5.55L2.74 8.87C2.62 9.08 2.66 9.34 2.86 9.48L4.89 11.06C4.84 11.36 4.8 11.69 4.8 12C4.8 12.31 4.82 12.64 4.87 12.94L2.84 14.52C2.66 14.66 2.61 14.93 2.72 15.13L4.64 18.45C4.76 18.67 5.01 18.74 5.23 18.67L7.62 17.71C8.12 18.09 8.65 18.41 9.24 18.65L9.6 21.19C9.65 21.43 9.84 21.6 10.08 21.6H13.92C14.16 21.6 14.36 21.43 14.39 21.19L14.75 18.65C15.34 18.41 15.88 18.09 16.37 17.71L18.76 18.67C18.98 18.75 19.23 18.67 19.35 18.45L21.27 15.13C21.39 14.91 21.34 14.66 21.15 14.52L19.14 12.94ZM12 15.6C10.02 15.6 8.4 13.98 8.4 12C8.4 10.02 10.02 8.4 12 8.4C13.98 8.4 15.6 10.02 15.6 12C15.6 13.98 13.98 15.6 12 15.6Z" }
                                }
                                toggle_right_btn = <FerricIconBtn> {
                                    width: 26, height: 26,
                                    draw_bg: { active: 1.0 }
                                    draw_icon: {
                                        svg_path: "M3 5H21V19H3V5ZM5 7V17H13V7H5ZM15 7V17H19V7H15Z"
                                        color: #F4F4F7
                                    }
                                }
                                <SolidView> { width: 1, height: 14, margin: { left: 3, right: 3 }, draw_bg: { color: #242634 } }
                                win_min_btn = <FerricWinBtn> { width: 30, height: 24, draw_bg: { kind: 0.0 } }
                                win_max_btn = <FerricWinBtn> { width: 30, height: 24, draw_bg: { kind: 1.0 } }
                                win_close_btn = <FerricWinBtn> { width: 32, height: 24, draw_bg: { kind: 2.0 } }
                            }
                        }

                        <FerricDividerH> {}

                        // 2. THREE-PANE WORKSPACE (Collapsible Left Nav + Center Stage + Docked Right Queue/Lyrics)
                        workspace_row = <View> {
                            width: Fill,
                            height: Fill,
                            flow: Right,

                            // LEFT SIDEBAR (168px, Pure Music Nav)
                            left_sidebar = <FerricSurface> {
                                width: 168,
                                height: Fill,
                                padding: { left: 8, right: 8, top: 8, bottom: 8 }
                                flow: Down,
                                spacing: 2,
                                draw_bg: { base_col: #09090D }

                                nav_home = <FerricNavItem> {
                                    text: "Home"
                                    draw_bg: { active: 1.0 }
                                    draw_icon: {
                                        svg_path: "M10 20V14H14V20H19V12H22L12 3L2 12H5V20H10Z"
                                        color: #F4F4F7
                                    }
                                    draw_text: { color: #F4F4F7 }
                                }
                                nav_search = <FerricNavItem> {
                                    text: "Search"
                                    draw_icon: { svg_path: "M15.5 14H14.71L14.43 13.73C15.41 12.59 16 11.11 16 9.5C16 5.91 13.09 3 9.5 3C5.91 3 3 5.91 3 9.5C3 13.09 5.91 16 9.5 16C11.11 16 12.59 15.41 13.73 14.43L14 14.71V15.5L19 20.49L20.49 19L15.5 14ZM9.5 14C7.01 14 5 11.99 5 9.5C5 7.01 7.01 5 9.5 5C11.99 5 14 7.01 14 9.5C14 11.99 11.99 14 9.5 14Z" }
                                }
                                nav_recents = <FerricNavItem> {
                                    text: "Recents"
                                    draw_icon: { svg_path: "M13 3C8.03 3 4 7.03 4 12H1L4.89 15.89L4.96 16.03L9 12H6C6 8.13 9.13 5 13 5C16.87 5 20 8.13 20 12C20 15.87 16.87 19 13 19C11.07 19 9.32 18.21 8.06 16.94L6.64 18.36C8.27 19.99 10.51 21 13 21C17.97 21 22 16.97 22 12C22 7.03 17.97 3 13 3ZM12 8V13L16.28 15.54L17 14.33L13.5 12.25V8H12Z" }
                                }

                                <Label> {
                                    margin: { left: 9, top: 10, bottom: 2 }
                                    draw_text: { color: #8A8EA2, text_style: <THEME_FONT_MEDIUM> { font_size: 7.8 } }
                                    text: "LIBRARY"
                                }
                                nav_songs = <FerricNavItem> {
                                    text: "Songs"
                                    draw_icon: { svg_path: "M12 3V13.55C11.41 13.21 10.73 13 10 13C7.79 13 6 14.79 6 17C6 19.21 7.79 21 10 21C12.21 21 14 19.21 14 17V7H18V3H12Z" }
                                }
                                nav_albums = <FerricNavItem> {
                                    text: "Albums"
                                    draw_icon: { svg_path: "M12 2C6.48 2 2 6.48 2 12C2 17.52 6.48 22 12 22C17.52 22 22 17.52 22 12C22 6.48 17.52 2 12 2ZM12 14.5C10.62 14.5 9.5 13.38 9.5 12C9.5 10.62 10.62 9.5 12 9.5C13.38 9.5 14.5 10.62 14.5 12C14.5 13.38 13.38 14.5 12 14.5Z" }
                                }
                                nav_artists = <FerricNavItem> {
                                    text: "Artists"
                                    draw_icon: { svg_path: "M12 12C14.21 12 16 10.21 16 8C16 5.79 14.21 4 12 4C9.79 4 8 5.79 8 8C8 10.21 9.79 12 12 12ZM12 14C9.33 14 4 15.34 4 18V20H20V18C20 15.34 14.67 14 12 14Z" }
                                }
                                nav_playlists = <FerricNavItem> {
                                    text: "Playlists"
                                    draw_icon: { svg_path: "M4 6H18V8H4V6ZM4 11H18V13H4V11ZM4 16H14V18H4V16ZM16 16V22L21 19L16 16Z" }
                                }

                                <Label> {
                                    margin: { left: 9, top: 10, bottom: 2 }
                                    draw_text: { color: #8A8EA2, text_style: <THEME_FONT_MEDIUM> { font_size: 7.8 } }
                                    text: "PINNED"
                                }
                                <FerricScrollYView> {
                                    width: Fill,
                                    height: Fill,
                                    flow: Down,
                                    spacing: 1,
                                    pin_liked_btn = <FerricPinBtn> { text: "Liked Songs" }
                                    pin_local_btn = <FerricPinBtn> { text: "Local Lossless" }
                                    pin_slot_0 = <FerricPinBtn> { visible: false, text: "" }
                                    pin_slot_1 = <FerricPinBtn> { visible: false, text: "" }
                                    pin_slot_2 = <FerricPinBtn> { visible: false, text: "" }
                                    pin_slot_3 = <FerricPinBtn> { visible: false, text: "" }
                                    pin_slot_4 = <FerricPinBtn> { visible: false, text: "" }
                                }

                                <FerricDividerH> { margin: { top: 4, bottom: 4 } }
                                nav_settings = <FerricNavItem> {
                                    text: "Settings"
                                    draw_icon: { svg_path: "M19.14 12.94C19.18 12.64 19.2 12.33 19.2 12C19.2 11.68 19.18 11.36 19.13 11.06L21.16 9.48C21.34 9.34 21.39 9.07 21.28 8.87L19.36 5.55C19.24 5.33 18.99 5.26 18.77 5.33L16.38 6.29C15.88 5.91 15.35 5.59 14.76 5.35L14.4 2.81C14.36 2.57 14.16 2.4 13.92 2.4H10.08C9.84 2.4 9.65 2.57 9.61 2.81L9.25 5.35C8.66 5.59 8.12 5.92 7.63 6.29L5.24 5.33C5.02 5.25 4.77 5.33 4.65 5.55L2.74 8.87C2.62 9.08 2.66 9.34 2.86 9.48L4.89 11.06C4.84 11.36 4.8 11.69 4.8 12C4.8 12.31 4.82 12.64 4.87 12.94L2.84 14.52C2.66 14.66 2.61 14.93 2.72 15.13L4.64 18.45C4.76 18.67 5.01 18.74 5.23 18.67L7.62 17.71C8.12 18.09 8.65 18.41 9.24 18.65L9.6 21.19C9.65 21.43 9.84 21.6 10.08 21.6H13.92C14.16 21.6 14.36 21.43 14.39 21.19L14.75 18.65C15.34 18.41 15.88 18.09 16.37 17.71L18.76 18.67C18.98 18.75 19.23 18.67 19.35 18.45L21.27 15.13C21.39 14.91 21.34 14.66 21.15 14.52L19.14 12.94ZM12 15.6C10.02 15.6 8.4 13.98 8.4 12C8.4 10.02 10.02 8.4 12 8.4C13.98 8.4 15.6 10.02 15.6 12C15.6 13.98 13.98 15.6 12 15.6Z" }
                                }
                                sidebar_add_folder_btn = <FerricNavItem> {
                                    text: "Add Folder"
                                    draw_icon: { svg_path: "M20 6H12L10 4H4C2.89 4 2 4.89 2 6V18C2 19.11 2.89 20 4 20H20C21.11 20 22 19.11 22 18V8C22 6.89 21.11 6 20 6ZM19 14H16V17H14V14H11V12H14V9H16V12H19V14Z" }
                                }
                            }

                            left_divider = <FerricDividerV> {}

                            // ============================================================
                            // CENTER STAGE (All Ferric Views)
                            // ============================================================
                            center_stage = <FerricSurface> {
                                width: Fill,
                                height: Fill,
                                flow: Overlay,
                                draw_bg: { base_col: #000000, is_canvas: 1.0 }

                                // --------------------------------------------------------
                                // PAGE 1: HOME (Mood Chips + Quick Picks + 8 Dynamic Shelves + Load More)
                                // --------------------------------------------------------
                                page_home = <FerricScrollYView> {
                                    visible: true,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 18, right: 18, top: 12, bottom: 26 }
                                    flow: Down,
                                    spacing: 14,

                                    // Mood & Genre Filter Chips (Live YouTube Music FEmusic_home)
                                    mood_chips_row = <View> {
                                        width: Fill,
                                        height: Fit,
                                        flow: Right,
                                        spacing: 5,
                                        chip_0 = <FerricChipBtn> { text: "All", draw_bg: { active: 1.0 }, draw_text: { active: 1.0 } }
                                        chip_1 = <FerricChipBtn> { text: "Feel good" }
                                        chip_2 = <FerricChipBtn> { text: "Relax" }
                                        chip_3 = <FerricChipBtn> { text: "Energize" }
                                        chip_4 = <FerricChipBtn> { text: "Workout" }
                                        chip_5 = <FerricChipBtn> { text: "Focus" }
                                        chip_6 = <FerricChipBtn> { text: "Commute" }
                                        chip_7 = <FerricChipBtn> { text: "Sad" }
                                        chip_8 = <FerricChipBtn> { text: "Party" }
                                    }

                                    // SECTION 1: QUICK PICKS (2x4 Grid w/ 36x36 Album Covers)
                                    <View> {
                                        width: Fill,
                                        height: Fit,
                                        flow: Down,
                                        spacing: 8,

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }
                                                text: "Quick picks"
                                            }
                                            home_status_lbl = <Label> {
                                                draw_text: { color: #7C8090, text_style: <THEME_FONT_REGULAR> { font_size: 8.2 } }
                                                text: "Loading YouTube Music..."
                                            }
                                            home_retry_btn = <FerricBtn> { visible: false, height: 24, padding: { left: 8, right: 9 }, text: "Retry" }
                                            home_play_all_btn = <FerricBtn> { height: 24, padding: { left: 9, right: 9 }, text: "Play all" }
                                            qp_prev_btn = <FerricIconBtn> {
                                                width: 24, height: 24,
                                                draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" }
                                            }
                                            qp_next_btn = <FerricIconBtn> {
                                                width: 24, height: 24,
                                                draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" }
                                            }
                                        }

                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 8,
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 5,
                                                qp_0 = <QuickPickTile> {}
                                                qp_1 = <QuickPickTile> {}
                                                qp_2 = <QuickPickTile> {}
                                                qp_3 = <QuickPickTile> {}
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 5,
                                                qp_4 = <QuickPickTile> {}
                                                qp_5 = <QuickPickTile> {}
                                                qp_6 = <QuickPickTile> {}
                                                qp_7 = <QuickPickTile> {}
                                            }
                                        }
                                    }

                                    // 8 DYNAMIC HORIZONTAL COVER SHELVES (Trending, New Releases, Charts, Mixes, Artists, Continuations)
                                    shelf0_wrap = <View> {
                                        width: Fill, height: Fit, flow: Down, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 4,
                                            sh0_title_lbl = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }, text: "Trending playlists & mixes" }
                                            sh0_prev_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                            sh0_next_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                        }
                                        <View> { width: Fill, height: Fit, flow: Right, spacing: 8, sh0_c0 = <ShelfCard> {}, sh0_c1 = <ShelfCard> {}, sh0_c2 = <ShelfCard> {}, sh0_c3 = <ShelfCard> {}, sh0_c4 = <ShelfCard> {} }
                                    }
                                    shelf1_wrap = <View> {
                                        width: Fill, height: Fit, flow: Down, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 4,
                                            sh1_title_lbl = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }, text: "New albums & singles" }
                                            sh1_prev_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                            sh1_next_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                        }
                                        <View> { width: Fill, height: Fit, flow: Right, spacing: 8, sh1_c0 = <ShelfCard> {}, sh1_c1 = <ShelfCard> {}, sh1_c2 = <ShelfCard> {}, sh1_c3 = <ShelfCard> {}, sh1_c4 = <ShelfCard> {} }
                                    }
                                    shelf2_wrap = <View> {
                                        width: Fill, height: Fit, flow: Down, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 4,
                                            sh2_title_lbl = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }, text: "Charts & popular" }
                                            sh2_prev_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                            sh2_next_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                        }
                                        <View> { width: Fill, height: Fit, flow: Right, spacing: 8, sh2_c0 = <ShelfCard> {}, sh2_c1 = <ShelfCard> {}, sh2_c2 = <ShelfCard> {}, sh2_c3 = <ShelfCard> {}, sh2_c4 = <ShelfCard> {} }
                                    }
                                    shelf3_wrap = <View> {
                                        visible: false, width: Fill, height: Fit, flow: Down, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 4,
                                            sh3_title_lbl = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }, text: "" }
                                            sh3_prev_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                            sh3_next_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                        }
                                        <View> { width: Fill, height: Fit, flow: Right, spacing: 8, sh3_c0 = <ShelfCard> {}, sh3_c1 = <ShelfCard> {}, sh3_c2 = <ShelfCard> {}, sh3_c3 = <ShelfCard> {}, sh3_c4 = <ShelfCard> {} }
                                    }
                                    shelf4_wrap = <View> {
                                        visible: false, width: Fill, height: Fit, flow: Down, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 4,
                                            sh4_title_lbl = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }, text: "" }
                                            sh4_prev_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                            sh4_next_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                        }
                                        <View> { width: Fill, height: Fit, flow: Right, spacing: 8, sh4_c0 = <ShelfCard> {}, sh4_c1 = <ShelfCard> {}, sh4_c2 = <ShelfCard> {}, sh4_c3 = <ShelfCard> {}, sh4_c4 = <ShelfCard> {} }
                                    }
                                    shelf5_wrap = <View> {
                                        visible: false, width: Fill, height: Fit, flow: Down, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 4,
                                            sh5_title_lbl = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }, text: "" }
                                            sh5_prev_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                            sh5_next_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                        }
                                        <View> { width: Fill, height: Fit, flow: Right, spacing: 8, sh5_c0 = <ShelfCard> {}, sh5_c1 = <ShelfCard> {}, sh5_c2 = <ShelfCard> {}, sh5_c3 = <ShelfCard> {}, sh5_c4 = <ShelfCard> {} }
                                    }
                                    shelf6_wrap = <View> {
                                        visible: false, width: Fill, height: Fit, flow: Down, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 4,
                                            sh6_title_lbl = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }, text: "" }
                                            sh6_prev_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                            sh6_next_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                        }
                                        <View> { width: Fill, height: Fit, flow: Right, spacing: 8, sh6_c0 = <ShelfCard> {}, sh6_c1 = <ShelfCard> {}, sh6_c2 = <ShelfCard> {}, sh6_c3 = <ShelfCard> {}, sh6_c4 = <ShelfCard> {} }
                                    }
                                    shelf7_wrap = <View> {
                                        visible: false, width: Fill, height: Fit, flow: Down, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 4,
                                            sh7_title_lbl = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }, text: "" }
                                            sh7_prev_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                            sh7_next_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                        }
                                        <View> { width: Fill, height: Fit, flow: Right, spacing: 8, sh7_c0 = <ShelfCard> {}, sh7_c1 = <ShelfCard> {}, sh7_c2 = <ShelfCard> {}, sh7_c3 = <ShelfCard> {}, sh7_c4 = <ShelfCard> {} }
                                    }

                                    // Endless Discovery Footer (Loads fresh YouTube Music continuations & genres on demand)
                                    home_more_wrap = <View> {
                                        width: Fill,
                                        height: Fit,
                                        padding: { top: 6, bottom: 8 }
                                        flow: Right,
                                        align: { x: 0.5, y: 0.5 }
                                        spacing: 10,
                                        home_load_more_btn = <FerricBtn> { height: 28, padding: { left: 14, right: 14 }, text: "Load More Recommendations" }
                                        home_shuffle_feed_btn = <FerricBtn> { height: 28, padding: { left: 12, right: 12 }, text: "Discover Fresh Mix" }
                                    }
                                }

                                // --------------------------------------------------------
                                // PAGE 2: SEARCH (Instant Search + Category Tabs + Cards)
                                // --------------------------------------------------------
                                page_search = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 18, right: 18, top: 14, bottom: 12 }
                                    flow: Down,
                                    spacing: 10,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 7,
                                        search_page_input = <FerricInput> {
                                            width: Fill,
                                            height: 32,
                                            empty_text: "Search YouTube Music, local library, or paste Spotify / YouTube link..."
                                        }
                                        search_go_btn = <FerricPrimaryBtn> { height: 32, text: "Search" }
                                        search_clear_btn = <FerricBtn> { height: 32, text: "Clear" }
                                    }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 5,
                                        sf_all_btn = <FerricChipBtn> { text: "All", draw_bg: { active: 1.0 }, draw_text: { active: 1.0 } }
                                        sf_songs_btn = <FerricChipBtn> { text: "Songs" }
                                        sf_albums_btn = <FerricChipBtn> { text: "Albums" }
                                        sf_artists_btn = <FerricChipBtn> { text: "Artists" }
                                        sf_playlists_btn = <FerricChipBtn> { text: "Playlists" }
                                        sf_local_btn = <FerricChipBtn> { text: "Local" }
                                        <View> { width: Fill, height: 1 }
                                        search_status_lbl = <Label> {
                                            draw_text: { color: #848796, text_style: <THEME_FONT_REGULAR> { font_size: 8.2 } }
                                            text: "Instant YouTube Music + Local Search"
                                        }
                                    }

                                    // Recent searches row (unified frosted glass chips)
                                    recent_searches_row = <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 5,
                                        <Label> { draw_text: { color: #848796, text_style: <THEME_FONT_REGULAR> { font_size: 8.2 } }, text: "Recent:" }
                                        rsrch_0 = <FerricChipBtn> { visible: false, height: 22, text: "" }
                                        rsrch_1 = <FerricChipBtn> { visible: false, height: 22, text: "" }
                                        rsrch_2 = <FerricChipBtn> { visible: false, height: 22, text: "" }
                                        rsrch_3 = <FerricChipBtn> { visible: false, height: 22, text: "" }
                                        rsrch_4 = <FerricChipBtn> { visible: false, height: 22, text: "" }
                                    }

                                    // Search Top Result / Album / Artist / Playlist cards strip
                                    search_cards_wrap = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 6,
                                        <Label> { draw_text: { color: #D4D6E2, text_style: <THEME_FONT_BOLD> { font_size: 10.2 } }, text: "Top Results · Artists, Albums & Playlists" }
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, spacing: 8,
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
                                    padding: { left: 18, right: 18, top: 14, bottom: 12 }
                                    flow: Down,
                                    spacing: 10,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 2,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 15.5 } }, text: "Recents" }
                                            recents_sub_lbl = <Label> { draw_text: { color: #8A8D9B, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Your recently played songs" }
                                        }
                                        recents_play_btn = <FerricPrimaryBtn> { text: "Play All" }
                                        recents_clear_btn = <FerricBtn> { text: "Clear History" }
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
                                    padding: { left: 18, right: 18, top: 14, bottom: 12 }
                                    flow: Down,
                                    spacing: 10,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 2,
                                            songs_title_lbl = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 15.5 } }, text: "Songs" }
                                            songs_sub_lbl = <Label> { draw_text: { color: #8A8D9B, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "0 songs · 6.0s Bass-Swap Automix" }
                                        }
                                        play_all_btn = <FerricPrimaryBtn> {
                                            text: "Play All"
                                            draw_icon: { svg_path: "M8 5V19L19 12L8 5Z" }
                                        }
                                        hero_shuffle_btn = <FerricBtn> { text: "Shuffle" }
                                        add_folder_btn = <FerricBtn> { text: "Add Folder" }
                                        songs_clear_demo_btn = <FerricBtn> {
                                            visible: false,
                                            text: "Clear Demo"
                                            draw_text: { color: #FF7885, color_hover: #FFA3AC }
                                        }
                                    }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 5,
                                        flt_all_btn = <FerricChipBtn> { text: "All", draw_bg: { active: 1.0 }, draw_text: { active: 1.0 } }
                                        flt_liked_btn = <FerricChipBtn> { text: "Liked" }
                                        flt_local_btn = <FerricChipBtn> { text: "Local" }
                                        flt_spotify_btn = <FerricChipBtn> { text: "Spotify" }
                                        flt_youtube_btn = <FerricChipBtn> { text: "YouTube" }
                                        <View> { width: Fill, height: 1 }
                                        songs_filter_input = <FerricInput> {
                                            width: 176,
                                            height: 24,
                                            padding: { left: 9, right: 9, top: 0, bottom: 0 }
                                            empty_text: "Filter songs..."
                                            draw_text: { text_style: <THEME_FONT_REGULAR> { font_size: 8.4, line_spacing: 1.0 } }
                                        }
                                        sort_cycle_btn = <FerricBtn> { height: 24, text: "Sort: Recent" }
                                    }

                                    <FerricDividerH> {}
                                    songs_track_list = <TrackList> { width: Fill, height: Fill }
                                }

                                // --------------------------------------------------------
                                // PAGE 5: LIBRARY -> ALBUMS GRID (5 cols x 4 rows = 20/page)
                                // --------------------------------------------------------
                                page_albums = <FerricScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 18, right: 18, top: 14, bottom: 26 }
                                    flow: Down,
                                    spacing: 10,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 2,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 15.5 } }, text: "Albums" }
                                            albums_sub_lbl = <Label> { draw_text: { color: #8A8D9B, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Grouped from your catalog" }
                                        }
                                        alb_prev_btn = <FerricIconBtn> { width: 26, height: 26, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                        alb_next_btn = <FerricIconBtn> { width: 26, height: 26, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                    }
                                    alb_row_0 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        alb_0 = <GridCard> {}
                                        alb_1 = <GridCard> {}
                                        alb_2 = <GridCard> {}
                                        alb_3 = <GridCard> {}
                                        alb_4 = <GridCard> {}
                                    }
                                    alb_row_1 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        alb_5 = <GridCard> {}
                                        alb_6 = <GridCard> {}
                                        alb_7 = <GridCard> {}
                                        alb_8 = <GridCard> {}
                                        alb_9 = <GridCard> {}
                                    }
                                    alb_row_2 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        alb_10 = <GridCard> {}
                                        alb_11 = <GridCard> {}
                                        alb_12 = <GridCard> {}
                                        alb_13 = <GridCard> {}
                                        alb_14 = <GridCard> {}
                                    }
                                    alb_row_3 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        alb_15 = <GridCard> {}
                                        alb_16 = <GridCard> {}
                                        alb_17 = <GridCard> {}
                                        alb_18 = <GridCard> {}
                                        alb_19 = <GridCard> {}
                                    }
                                }

                                // --------------------------------------------------------
                                // PAGE 6: LIBRARY -> ARTISTS CIRCULAR GRID (5 cols x 4 rows = 20/page)
                                // --------------------------------------------------------
                                page_artists = <FerricScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 18, right: 18, top: 14, bottom: 26 }
                                    flow: Down,
                                    spacing: 10,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 2,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 15.5 } }, text: "Artists" }
                                            artists_sub_lbl = <Label> { draw_text: { color: #8A8D9B, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Artists in your library" }
                                        }
                                        art_prev_btn = <FerricIconBtn> { width: 26, height: 26, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                        art_next_btn = <FerricIconBtn> { width: 26, height: 26, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                    }
                                    art_row_0 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        art_0 = <GridCard> {}
                                        art_1 = <GridCard> {}
                                        art_2 = <GridCard> {}
                                        art_3 = <GridCard> {}
                                        art_4 = <GridCard> {}
                                    }
                                    art_row_1 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        art_5 = <GridCard> {}
                                        art_6 = <GridCard> {}
                                        art_7 = <GridCard> {}
                                        art_8 = <GridCard> {}
                                        art_9 = <GridCard> {}
                                    }
                                    art_row_2 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        art_10 = <GridCard> {}
                                        art_11 = <GridCard> {}
                                        art_12 = <GridCard> {}
                                        art_13 = <GridCard> {}
                                        art_14 = <GridCard> {}
                                    }
                                    art_row_3 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        art_15 = <GridCard> {}
                                        art_16 = <GridCard> {}
                                        art_17 = <GridCard> {}
                                        art_18 = <GridCard> {}
                                        art_19 = <GridCard> {}
                                    }
                                }

                                // --------------------------------------------------------
                                // PAGE 7: LIBRARY -> PLAYLISTS GRID + CREATOR (5 cols x 4 rows = 20/page)
                                // --------------------------------------------------------
                                page_playlists = <FerricScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 18, right: 18, top: 14, bottom: 26 }
                                    flow: Down,
                                    spacing: 10,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                        <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 15.5 } }, text: "Playlists" }
                                        pl_new_input = <FerricInput> { width: 180, height: 28, empty_text: "New playlist name..." }
                                        pl_create_btn = <FerricPrimaryBtn> { height: 28, text: "+ Create" }
                                        pl_import_btn = <FerricBtn> { height: 28, text: "Import M3U/CSV" }
                                        pl_prev_btn = <FerricIconBtn> { width: 26, height: 26, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                        pl_next_btn = <FerricIconBtn> { width: 26, height: 26, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                    }
                                    plc_row_0 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        plc_0 = <GridCard> {}
                                        plc_1 = <GridCard> {}
                                        plc_2 = <GridCard> {}
                                        plc_3 = <GridCard> {}
                                        plc_4 = <GridCard> {}
                                    }
                                    plc_row_1 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        plc_5 = <GridCard> {}
                                        plc_6 = <GridCard> {}
                                        plc_7 = <GridCard> {}
                                        plc_8 = <GridCard> {}
                                        plc_9 = <GridCard> {}
                                    }
                                    plc_row_2 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        plc_10 = <GridCard> {}
                                        plc_11 = <GridCard> {}
                                        plc_12 = <GridCard> {}
                                        plc_13 = <GridCard> {}
                                        plc_14 = <GridCard> {}
                                    }
                                    plc_row_3 = <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 8,
                                        plc_15 = <GridCard> {}
                                        plc_16 = <GridCard> {}
                                        plc_17 = <GridCard> {}
                                        plc_18 = <GridCard> {}
                                        plc_19 = <GridCard> {}
                                    }
                                }

                                // --------------------------------------------------------
                                // PAGE 8: DETAIL VIEW (Album / Playlist / Artist)
                                // --------------------------------------------------------
                                page_detail = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 18, right: 18, top: 14, bottom: 12 }
                                    flow: Down,
                                    spacing: 10,

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 14,
                                        detail_art = <FerricCoverArt> {
                                            width: 112,
                                            height: 112,
                                            draw_bg: { radius: 10.0 }
                                        }
                                        <View> {
                                            width: Fill, height: Fit, flow: Down, spacing: 4,
                                            detail_kind_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_BOLD> { font_size: 8.2 } }, text: "PLAYLIST" }
                                            detail_title_lbl = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 16.0 } }, text: "Collection" }
                                            detail_sub_lbl = <Label> { draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "" }
                                            <View> {
                                                width: Fill, height: Fit, margin: { top: 4 }, flow: Right, spacing: 6,
                                                detail_play_btn = <FerricPrimaryBtn> { text: "Play", draw_icon: { svg_path: "M8 5V19L19 12L8 5Z" } }
                                                detail_shuffle_btn = <FerricBtn> { height: 28, text: "Shuffle" }
                                                detail_radio_btn = <FerricBtn> { height: 28, text: "Start Radio" }
                                                detail_pin_btn = <FerricBtn> { height: 28, text: "Pin" }
                                                detail_queue_btn = <FerricBtn> { height: 28, text: "+ Queue" }
                                                detail_export_btn = <FerricBtn> { height: 28, text: "Export M3U" }
                                                detail_delete_btn = <FerricBtn> {
                                                    visible: false,
                                                    height: 28,
                                                    text: "Delete"
                                                    draw_text: { color: #FF7885, color_hover: #FFA3AC }
                                                }
                                            }
                                            detail_rename_row = <View> {
                                                visible: false,
                                                width: Fill, height: Fit, margin: { top: 4 }, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                detail_rename_input = <FerricInput> {
                                                    width: 220,
                                                    height: 26,
                                                    empty_text: "Rename playlist..."
                                                    draw_text: { text_style: <THEME_FONT_REGULAR> { font_size: 8.5, line_spacing: 1.0 } }
                                                }
                                                detail_rename_btn = <FerricBtn> { height: 26, text: "Rename" }
                                            }
                                        }
                                    }
                                    <FerricDividerH> {}
                                    detail_track_list = <TrackList> { width: Fill, height: Fill }
                                }

                                // --------------------------------------------------------
                                // PAGE 9: TABBED SETTINGS HUB
                                // --------------------------------------------------------
                                page_settings = <FerricScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 18, right: 18, top: 14, bottom: 26 }
                                    flow: Down,
                                    spacing: 12,

                                    <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 15.5 } }, text: "Settings" }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 5,
                                        st_accounts_btn = <FerricChipBtn> { text: "Accounts", draw_bg: { active: 1.0 }, draw_text: { active: 1.0 } }
                                        st_playback_btn = <FerricChipBtn> { text: "Playback" }
                                        st_eq_btn = <FerricChipBtn> { text: "Equalizer" }
                                        st_appearance_btn = <FerricChipBtn> { text: "Appearance" }
                                        st_integrations_btn = <FerricChipBtn> { text: "Integrations" }
                                        st_storage_btn = <FerricChipBtn> { text: "Storage" }
                                        st_connect_btn = <FerricChipBtn> { text: "Connect" }
                                    }

                                    // TAB A: ACCOUNTS & SOURCES
                                    tab_accounts = <View> {
                                        visible: true,
                                        width: Fill, height: Fit, flow: Down, spacing: 10,

                                        card_acct_yt = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "YouTube Music & Google Account" }
                                                yt_state_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "Guest Mode (Optional)" }
                                            }
                                            <Label> {
                                                width: Fill,
                                                draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }
                                                text: "Music works out of the box without any account. Sign in with your regular Google account in one click to sync personal Liked Songs and Playlists."
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                yt_login_btn = <FerricPrimaryBtn> { text: "Sign in with Google" }
                                                yt_sync_btn = <FerricBtn> { height: 28, text: "Sync Library" }
                                                yt_logout_btn = <FerricBtn> { height: 28, text: "Sign Out" }
                                                guest_refresh_btn = <FerricBtn> { height: 28, text: "Refresh Home Feed" }
                                            }
                                            oauth_status_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #F5A473, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }
                                                text: "Guest Mode active — YouTube Music streaming ready."
                                            }
                                        }

                                        card_acct_import = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Instant Spotify & YouTube Playlist Import" }
                                            quick_sync_input = <FerricInput> {
                                                empty_text: "Paste public Spotify or YouTube playlist / album / track URL..."
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                quick_spotify_btn = <FerricBtn> { text: "Import Spotify Link" }
                                                quick_youtube_btn = <FerricBtn> { text: "Import YouTube Link" }
                                                match_local_btn = <FerricBtn> { text: "Match Cloud to Local FLAC" }
                                                rescan_btn = <FerricBtn> { text: "Rescan Local Folders" }
                                            }
                                        }

                                        card_acct_sub = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "OpenSubsonic / Navidrome Self-Hosted Server" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                sub_url_input = <FerricInput> { width: Fill, empty_text: "https://music.example.lan" }
                                                sub_user_input = <FerricInput> { width: 130, empty_text: "Username" }
                                                sub_pass_input = <FerricInput> { width: 130, is_password: true, empty_text: "Password" }
                                                sub_sync_btn = <FerricBtn> { height: 30, text: "Connect" }
                                            }
                                        }
                                    }

                                    // TAB B: PLAYBACK, STREAM QUALITY, CROSSFADE & TRAY
                                    tab_playback = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 10,

                                        card_pb_xfade = <FerricCard> {
                                            flow: Down, spacing: 9,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Seamless Dual-Deck Fade-In & Crossfade" }
                                                xfade_val_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_BOLD> { font_size: 9.2 } }, text: "6.0 s" }
                                            }
                                            xfade_slider = <FerricBarSlider> { min: 0.0, max: 12.0, value: 6.0 }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                xf_0_btn = <FerricBtn> { text: "0s Gapless" }
                                                xf_3_btn = <FerricBtn> { text: "3s Crisp" }
                                                xf_6_btn = <FerricBtn> { text: "6s Seamless" }
                                                xf_10_btn = <FerricBtn> { text: "10s Club Mix" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                <Label> { draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Transition Curve:" }
                                                curve_bass_btn = <FerricBtn> { text: "Bass-Swap Automix" }
                                                curve_eqp_btn = <FerricBtn> { text: "Equal-Power" }
                                                curve_velvet_btn = <FerricBtn> { text: "S-Curve Velvet" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #C8CAD6, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Soft Play / Resume Fade-In (ms)" }
                                                fadein_val_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "240 ms" }
                                            }
                                            fadein_slider = <FerricBarSlider> { min: 0.0, max: 1000.0, value: 240.0 }
                                        }

                                        card_pb_quality = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Audio & In-App Video Clip Quality" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                <Label> { width: 140, draw_text: { color: #C8CAD6, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Streaming Audio Quality" }
                                                aq_low_btn = <FerricChipBtn> { height: 24, text: "Low 64k" }
                                                aq_norm_btn = <FerricChipBtn> { height: 24, text: "Normal 128k" }
                                                aq_high_btn = <FerricChipBtn> { height: 24, text: "High 160k" }
                                                aq_best_btn = <FerricChipBtn> { height: 24, text: "Best" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                <Label> { width: 140, draw_text: { color: #C8CAD6, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Default Video Quality" }
                                                svq_auto_btn = <FerricChipBtn> { height: 24, text: "Auto" }
                                                svq_360_btn = <FerricChipBtn> { height: 24, text: "360p" }
                                                svq_480_btn = <FerricChipBtn> { height: 24, text: "480p" }
                                                svq_720_btn = <FerricChipBtn> { height: 24, text: "720p" }
                                                svq_1080_btn = <FerricChipBtn> { height: 24, text: "1080p" }
                                            }
                                        }

                                        card_pb_behavior = <FerricCard> {
                                            flow: Down, spacing: 7,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Playback Behavior, Radio & System Tray" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Keep running in System Tray when clicking Close (X)" }
                                                close_to_tray_toggle = <FerricToggle> { is_on: false }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Autoplay Infinite Radio (YouTube Music Next)" }
                                                radio_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "0ms Gapless Album Lock (Local consecutive albums only)" }
                                                gapless_lock_toggle = <FerricToggle> { is_on: false }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Skip Leading & Trailing Silence" }
                                                trim_silence_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "ReplayGain Loudness Normalization" }
                                                replay_gain_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Mono Downmix" }
                                                mono_toggle = <FerricToggle> { is_on: false }
                                            }
                                        }

                                        card_pb_speed = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.2 } }, text: "Playback Speed" }
                                                spd_075_btn = <FerricBtn> { text: "0.75x" }
                                                spd_100_btn = <FerricBtn> { text: "1.0x" }
                                                spd_125_btn = <FerricBtn> { text: "1.25x" }
                                                spd_150_btn = <FerricBtn> { text: "1.5x" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                sleep_status_lbl = <Label> { width: Fill, draw_text: { color: #C8CAD6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Sleep Timer: Off" }
                                                slp_off_btn = <FerricBtn> { text: "Off" }
                                                slp_15_btn = <FerricBtn> { text: "15m" }
                                                slp_30_btn = <FerricBtn> { text: "30m" }
                                                slp_60_btn = <FerricBtn> { text: "60m" }
                                                slp_eot_btn = <FerricBtn> { text: "End of Track" }
                                            }
                                        }
                                    }

                                    // TAB C: 10-BAND PARAMETRIC EQUALIZER + 13 STUDIO PRESETS + AUTOEQ
                                    tab_eq = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 10,

                                        card_eq_main = <FerricCard> {
                                            flow: Down, spacing: 9,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "10-Band Parametric Biquad Equalizer" }
                                                eq_status_lbl = <Label> { draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Preset: Flat" }
                                                eq_enable_toggle = <FerricToggle> { is_on: true }
                                            }
                                            eq_graph = <FerricEqGraph> { height: 136 }

                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                <Label> { width: 68, draw_text: { color: #C8CAD6, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Preamp" }
                                                eq_preamp_slider = <FerricBarSlider> { min: -12.0, max: 12.0, value: 0.0 }
                                                eq_preamp_lbl = <Label> { width: 48, draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "+0.0 dB" }
                                            }

                                            // 13 EQ Presets
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 5,
                                                eq_pre_flat = <FerricBtn> { height: 24, text: "Flat" }
                                                eq_pre_bass = <FerricBtn> { height: 24, text: "Bass Boost" }
                                                eq_pre_bassred = <FerricBtn> { height: 24, text: "Bass Reducer" }
                                                eq_pre_treble = <FerricBtn> { height: 24, text: "Treble Boost" }
                                                eq_pre_vocal = <FerricBtn> { height: 24, text: "Vocal Clarity" }
                                                eq_pre_rock = <FerricBtn> { height: 24, text: "Rock" }
                                                eq_pre_pop = <FerricBtn> { height: 24, text: "Pop" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 5,
                                                eq_pre_jazz = <FerricBtn> { height: 24, text: "Jazz" }
                                                eq_pre_classical = <FerricBtn> { height: 24, text: "Classical" }
                                                eq_pre_elec = <FerricBtn> { height: 24, text: "Electronic" }
                                                eq_pre_acoustic = <FerricBtn> { height: 24, text: "Acoustic" }
                                                eq_pre_loudness = <FerricBtn> { height: 24, text: "Loudness" }
                                                eq_pre_copper = <FerricBtn> { height: 24, text: "Warm Copper" }
                                            }

                                            // 10 Vertical Faders
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { x: 0.5 }, spacing: 12,
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v0 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b0 = <FerricVertSlider> { height: 112 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "31" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v1 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b1 = <FerricVertSlider> { height: 112 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "62" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v2 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b2 = <FerricVertSlider> { height: 112 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "125" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v3 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b3 = <FerricVertSlider> { height: 112 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "250" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v4 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b4 = <FerricVertSlider> { height: 112 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "500" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v5 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b5 = <FerricVertSlider> { height: 112 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "1k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v6 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b6 = <FerricVertSlider> { height: 112 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "2k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v7 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b7 = <FerricVertSlider> { height: 112 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "4k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v8 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b8 = <FerricVertSlider> { height: 112 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "8k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v9 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b9 = <FerricVertSlider> { height: 112 }, <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "16k" } }
                                            }

                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                autoeq_input = <FerricInput> { width: Fill, empty_text: "AutoEq Headphone Lookup (e.g. Sony WH-1000XM5, HD 650, AirPods Pro)..." }
                                                autoeq_apply_btn = <FerricBtn> { height: 30, text: "Apply AutoEq" }
                                                eq_import_btn = <FerricBtn> { height: 30, text: "Import APO" }
                                                eq_export_btn = <FerricBtn> { height: 30, text: "Export APO" }
                                            }
                                        }
                                    }

                                    // TAB D: APPEARANCE, UI ZOOM, ADAPTIVE ARTWORK & NATIVE WINDOWS ACRYLIC
                                    tab_appearance = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 10,

                                        card_app_theme = <FerricCard> {
                                            flow: Down, spacing: 10,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Theme, Interface Zoom & Native Windows Acrylic" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                pal_amoled_btn = <FerricBtn> { text: "AMOLED Black" }
                                                pal_acrylic_btn = <FerricBtn> { text: "Native Acrylic" }
                                                pal_obsidian_btn = <FerricBtn> { text: "Obsidian Studio" }
                                                pal_cream_btn = <FerricBtn> { text: "Warm Cream" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Interface Zoom (Ctrl - / Ctrl + / Ctrl 0)" }
                                                zoom_out_btn = <FerricBtn> { height: 24, padding: { left: 9, right: 9 }, text: "-" }
                                                zoom_val_lbl = <Label> { width: 42, align: { x: 0.5, y: 0.5 }, draw_text: { color: #E86A2C, text_style: <THEME_FONT_BOLD> { font_size: 9.0 } }, text: "100%" }
                                                zoom_in_btn = <FerricBtn> { height: 24, padding: { left: 9, right: 9 }, text: "+" }
                                                zoom_reset_btn = <FerricBtn> { height: 24, text: "Reset" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Native Windows 11 Acrylic Transparency (Real OS DWM blur behind window)" }
                                                acrylic_toggle = <FerricToggle> { is_on: false }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                <Label> { width: 140, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Acrylic Opacity" }
                                                acrylic_opacity_slider = <FerricBarSlider> { min: 0.18, max: 0.94, value: 0.72, wheel_adjust: true, wheel_step: 0.04 }
                                                acrylic_opacity_lbl = <Label> { width: 42, draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "72%" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 6,
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                    <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Acrylic Glass Tint Color (Custom RGB)" }
                                                    acrylic_tint_preview = <View> {
                                                        width: 18, height: 18,
                                                        show_bg: true,
                                                        draw_bg: {
                                                            uniform tint_col: #0B0D14
                                                            fn pixel(self) -> vec4 {
                                                                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                                                sdf.circle(self.rect_size.x * 0.5, self.rect_size.y * 0.5, self.rect_size.x * 0.5 - 1.0);
                                                                sdf.fill_keep(self.tint_col);
                                                                sdf.stroke(vec4(1.0, 1.0, 1.0, 0.35), 1.0);
                                                                return sdf.result;
                                                            }
                                                        }
                                                    }
                                                    acrylic_tint_lbl = <Label> { width: 64, draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.6 } }, text: "#0B0D14" }
                                                }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, spacing: 5,
                                                    tint_obsidian_btn = <FerricChipBtn> { text: "Obsidian" }
                                                    tint_ember_btn = <FerricChipBtn> { text: "Ember" }
                                                    tint_copper_btn = <FerricChipBtn> { text: "Copper" }
                                                    tint_midnight_btn = <FerricChipBtn> { text: "Midnight" }
                                                    tint_emerald_btn = <FerricChipBtn> { text: "Emerald" }
                                                    tint_crimson_btn = <FerricChipBtn> { text: "Crimson" }
                                                    tint_slate_btn = <FerricChipBtn> { text: "Slate" }
                                                    tint_smoke_btn = <FerricChipBtn> { text: "Smoke" }
                                                }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                    acrylic_tint_input = <FerricInput> { width: 160, height: 28, empty_text: "#RRGGBB (e.g. #2B140A)" }
                                                    acrylic_tint_apply_btn = <FerricBtn> { height: 28, text: "Apply Tint" }
                                                }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Adaptive Artwork Accent (Tint seekbar, lyrics & active indicator from cover art)" }
                                                adaptive_art_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <FerricDividerH> {}
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Custom Theme Profiles" }
                                                theme_import_btn = <FerricBtn> { height: 24, text: "Import Theme JSON" }
                                                theme_export_btn = <FerricBtn> { height: 24, text: "Export Theme JSON" }
                                            }
                                        }
                                    }

                                    // TAB D2: INTEGRATIONS (DISCORD RICH PRESENCE)
                                    tab_integrations = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 10,

                                        card_discord_rpc = <FerricCard> {
                                            flow: Down, spacing: 10,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Discord Rich Presence" }
                                                discord_status_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.6 } }, text: "Disabled" }
                                            }

                                            // Live Discord Listening Activity Preview Card
                                            discord_preview_box = <View> {
                                                width: Fill, height: Fit,
                                                padding: { left: 14, right: 14, top: 12, bottom: 12 }
                                                flow: Down, spacing: 8,
                                                show_bg: true,
                                                draw_bg: {
                                                    uniform acrylic_on: 0.0
                                                    fn pixel(self) -> vec4 {
                                                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 9.0);
                                                        let solid_bg = vec4(0.085, 0.088, 0.112, 1.0);
                                                        let acr_bg = vec4(1.0, 1.0, 1.0, 0.052);
                                                        sdf.fill_keep(mix(solid_bg, acr_bg, self.acrylic_on));
                                                        sdf.stroke(vec4(1.0, 1.0, 1.0, 0.095), 1.0);
                                                        return sdf.result;
                                                    }
                                                }
                                                discord_prev_header = <Label> {
                                                    width: Fill,
                                                    draw_text: { color: #B4B7C6, text_style: <THEME_FONT_MEDIUM> { font_size: 8.4 } }
                                                    text: "Listening to Artist - Title"
                                                }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 12,
                                                    discord_prev_art = <FerricCoverArt> { width: 60, height: 60, draw_bg: { radius: 8.0 } }
                                                    <View> {
                                                        width: Fill, height: Fit, flow: Down, spacing: 3,
                                                        discord_prev_title = <Label> {
                                                            width: Fill,
                                                            draw_text: { color: #FFFFFF, text_style: <THEME_FONT_BOLD> { font_size: 9.8 } }
                                                            text: "Play a song to preview Discord status"
                                                        }
                                                        discord_prev_artist = <Label> {
                                                            width: Fill,
                                                            draw_text: { color: #C8CAD6, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }
                                                            text: "Ferric Hi-Fi Player"
                                                        }
                                                        discord_prev_album = <Label> {
                                                            width: Fill,
                                                            draw_text: { color: #8E92A4, text_style: <THEME_FONT_REGULAR> { font_size: 8.3 } }
                                                            text: "Native Rust IPC"
                                                        }
                                                    }
                                                }
                                            }

                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_MEDIUM> { font_size: 9.2 } }, text: "Show on Discord (Listening activity via local IPC)" }
                                                discord_enable_toggle = <FerricToggle> { is_on: false }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Status name display" }
                                                discord_format_btn = <FerricBtn> { height: 26, text: "Artist - Title" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Show when paused" }
                                                discord_paused_toggle = <FerricToggle> { is_on: false }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Show album cover art" }
                                                discord_cover_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Hide track details (Privacy mode)" }
                                                discord_hide_details_toggle = <FerricToggle> { is_on: false }
                                            }
                                            <FerricDividerH> {}
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 6,
                                                <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "Discord Application ID" }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                    discord_client_id_input = <FerricInput> { width: Fill, height: 28, empty_text: "Paste 18-19 digit Discord Application ID (e.g. from Discord Developer Portal)..." }
                                                    discord_save_id_btn = <FerricPrimaryBtn> { height: 28, text: "Save ID" }
                                                    discord_portal_btn = <FerricBtn> { height: 28, text: "Create App ID" }
                                                }
                                                <Label> {
                                                    width: Fill,
                                                    draw_text: { color: #8A8EA2, text_style: <THEME_FONT_REGULAR> { font_size: 8.2 } }
                                                    text: "Click 'Create App ID' -> New Application (name it 'Ferric') -> copy Application ID -> paste here & click Save ID."
                                                }
                                            }
                                        }
                                    }

                                    // TAB E: STORAGE, UPDATES & KEYBOARD SHORTCUTS
                                    tab_storage = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 10,

                                        card_st_cache = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Local Disk Cache" }
                                            cache_stats_lbl = <Label> { draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Audio Stream Cache: 0 KB · Artwork Cache: 0 KB" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                clear_audio_cache_btn = <FerricBtn> { text: "Clear Audio Cache" }
                                                clear_art_cache_btn = <FerricBtn> { text: "Clear Artwork Cache" }
                                                load_demo_btn = <FerricBtn> { text: "Load Offline Demo Tracks" }
                                                remove_demo_btn = <FerricBtn> {
                                                    text: "Remove Demo Tracks"
                                                    draw_text: { color: #FF7885, color_hover: #FFA3AC }
                                                }
                                                clear_local_btn = <FerricBtn> {
                                                    text: "Clear Local Folders"
                                                    draw_text: { color: #FF7885, color_hover: #FFA3AC }
                                                }
                                            }
                                        }

                                        card_st_update = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Software Updates (Ferric v0.1.0)" }
                                                auto_update_toggle = <FerricToggle> { is_on: true }
                                            }
                                            update_status_lbl = <Label> { draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "SHA-256 verified GitHub Releases MSI updater" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                check_updates_btn = <FerricBtn> { text: "Check for Updates" }
                                                open_releases_btn = <FerricBtn> { text: "Open GitHub Releases" }
                                            }
                                        }

                                        card_st_keys = <FerricCard> {
                                            flow: Down, spacing: 5,
                                            <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Keyboard Shortcuts" }
                                            <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Space: Play / Pause  ·  Ctrl+K: Focus Search  ·  Ctrl+F: Search Page  ·  Ctrl+Q: Queue  ·  Ctrl+L: Lyrics  ·  Ctrl+ / Ctrl- / Ctrl+0: UI Zoom  ·  Ctrl+,: Settings  ·  F11: Fullscreen  ·  Alt+Left/Right: Back/Forward  ·  Shift+Left/Right: Prev/Next  ·  Ctrl+Up/Down / Mouse Wheel: Volume" }
                                        }
                                    }

                                    // TAB F: FERRIC CONNECT (LAN QR)
                                    tab_connect = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 10,
                                        card_conn_main = <FerricCard> {
                                            flow: Right, align: { y: 0.5 }, spacing: 16,
                                            connect_qr_view = <FerricQrView> { width: 136, height: 136 }
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 6,
                                                <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Ferric Connect (Local LAN Pairing)" }
                                                connect_lan_ip_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "LAN: Ready" }
                                                connect_peers_lbl = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Listening for Ferric devices on local Wi-Fi..." }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, spacing: 6,
                                                    connect_gen_pair_btn = <FerricBtn> { text: "Regenerate QR" }
                                                    connect_copy_pair_btn = <FerricBtn> { text: "Copy Pairing URI" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            right_divider = <FerricDividerV> {}

                            // ============================================================
                            // DOCKED RIGHT SIDEBAR (260px — Queue & Spring Synced Lyrics)
                            // ============================================================
                            right_sidebar = <FerricSurface> {
                                visible: true,
                                width: 260,
                                height: Fill,
                                padding: { left: 8, right: 10, top: 8, bottom: 8 }
                                flow: Down,
                                spacing: 6,
                                draw_bg: { base_col: #09090D }

                                // Right Sidebar Header Tabs: [Queue] [Lyrics] + [Radio] [Refresh] [Clear]
                                <View> {
                                    width: Fill, height: 26, flow: Right, align: { y: 0.5 }, spacing: 4,
                                    rs_tab_queue = <FerricChipBtn> { height: 24, text: "Queue", draw_bg: { active: 1.0 }, draw_text: { active: 1.0 } }
                                    rs_tab_lyrics = <FerricChipBtn> { height: 24, text: "Lyrics" }
                                    <View> { width: Fill, height: 1 }
                                    rs_radio_btn = <FerricIconBtn> {
                                        width: 24, height: 24,
                                        draw_bg: { active: 1.0 }
                                        draw_icon: {
                                            svg_path: "M3.24 6.15C2.51 6.43 2 7.17 2 8V20C2 21.1 2.89 22 4 22H20C21.11 22 22 21.1 22 20V8C22 6.89 21.11 6 20 6H8.3L16.56 2.66L15.88 1L3.24 6.15ZM7 20C5.34 20 4 18.66 4 17C4 15.34 5.34 14 7 14C8.66 14 10 15.34 10 17C10 18.66 8.66 20 7 20ZM20 12H18V10H16V12H4V8H20V12Z"
                                            color: #E86A2C
                                        }
                                    }
                                    rs_refresh_queue_btn = <FerricIconBtn> {
                                        width: 24, height: 24,
                                        draw_icon: { svg_path: "M17.65 6.35C16.2 4.9 14.21 4 12 4C7.58 4 4.01 7.58 4.01 12C4.01 16.42 7.58 20 12 20C15.73 20 18.84 17.45 19.73 14H17.65C16.83 16.33 14.61 18 12 18C8.69 18 6 15.31 6 12C6 8.69 8.69 6 12 6C13.66 6 15.14 6.69 16.22 7.78L13 11H20V4L17.65 6.35Z" }
                                    }
                                    rs_clear_btn = <FerricIconBtn> {
                                        width: 24, height: 24,
                                        draw_icon: { svg_path: "M6 19C6 20.1 6.9 21 8 21H16C17.1 21 18 20.1 18 19V7H6V19ZM19 4H15.5L14.5 3H9.5L8.5 4H5V6H19V4Z" }
                                    }
                                }

                                <FerricDividerH> {}

                                // TAB 1: QUEUE (History -> Now Playing -> Up Next)
                                rs_queue_view = <FerricScrollYView> {
                                    visible: true,
                                    width: Fill,
                                    height: Fill,
                                    padding: { top: 2, right: 8, bottom: 10 }
                                    flow: Down,
                                    spacing: 4,

                                    qh_header = <View> {
                                        visible: false,
                                        width: Fill,
                                        height: Fit,
                                        margin: { left: 4, top: 2, bottom: 1 }
                                        <Label> {
                                            padding: 0,
                                            draw_text: { color: #8A8EA2, text_style: <THEME_FONT_MEDIUM> { font_size: 7.8 } }
                                            text: "HISTORY"
                                        }
                                    }
                                    qh_0 = <QueueMiniRow> { visible: false }
                                    qh_1 = <QueueMiniRow> { visible: false }
                                    qh_2 = <QueueMiniRow> { visible: false }

                                    q_now_header = <Label> {
                                        margin: { left: 4, top: 4, bottom: 1 }
                                        padding: 0,
                                        draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 7.8 } }
                                        text: "NOW PLAYING"
                                    }
                                    q_now_row = <QueueMiniRow> {
                                        draw_bg: { active: 1.0 }
                                    }

                                    <View> {
                                        width: Fill, height: Fit, margin: { left: 4, right: 12, top: 6, bottom: 1 },
                                        flow: Right, align: { y: 0.5 },
                                        q_upnext_header = <Label> {
                                            width: Fill,
                                            padding: 0,
                                            draw_text: { color: #8A8EA2, text_style: <THEME_FONT_MEDIUM> { font_size: 7.8 } }
                                            text: "UP NEXT"
                                        }
                                        q_radio_status = <Label> {
                                            padding: 0,
                                            draw_text: { color: #8A8EA2, text_style: <THEME_FONT_MEDIUM> { font_size: 7.8 } }
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

                                // TAB 2: DOCKED LIVE 60 FPS SPRING SYNCED LYRICS (Click any line to seek)
                                rs_lyrics_view = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    flow: Down,
                                    spacing: 6,
                                    padding: { left: 2, right: 2, top: 2, bottom: 2 }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 3,
                                        rs_lyr_source = <Label> { width: Fill, draw_text: { color: #8A8D9B, text_style: <THEME_FONT_REGULAR> { font_size: 8.0 } }, text: "Synced Lyrics" }
                                        rs_lyr_minus = <FerricBtn> { height: 22, padding: { left: 5, right: 5 }, text: "-0.5s", draw_text: { text_style: { font_size: 7.8 } } }
                                        rs_lyr_plus = <FerricBtn> { height: 22, padding: { left: 5, right: 5 }, text: "+0.5s", draw_text: { text_style: { font_size: 7.8 } } }
                                        rs_lyr_expand = <FerricBtn> { height: 22, padding: { left: 6, right: 6 }, text: "Full", draw_text: { text_style: { font_size: 7.8 } } }
                                    }

                                    rs_lyrics_scroll = <FerricLyricsView> {
                                        width: Fill,
                                        height: Fill,
                                        compact: true,
                                    }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 5,
                                        rs_lyr_copy = <FerricBtn> { height: 24, text: "Copy Lyrics" }
                                        rs_watch_video = <FerricBtn> { height: 24, text: "Video Clip" }
                                    }
                                }
                            }
                        }

                        // ============================================================
                        // 3. CENTERED 2-ROW AMBIENT BOTTOM PLAYER BAR (74px, GEOMETRIC SYMMETRY)
                        // ============================================================
                        bottom_bar = <AmbientBottomBar> {
                            // Left (272px): 44x44 Cover Art + Title/Artist + Heart + Add Playlist
                            bottom_left_grp = <View> {
                                width: 272,
                                height: Fill,
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 10,

                                np_art = <FerricCoverArt> {
                                    width: 44,
                                    height: 44,
                                    draw_bg: { radius: 6.0 }
                                }
                                <View> {
                                    width: Fill,
                                    height: Fit,
                                    flow: Down,
                                    spacing: 2,
                                    np_title_lbl = <Label> {
                                        width: Fill,
                                        flow: Right,
                                        padding: 0,
                                        draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 9.6, line_spacing: 1.0 } }
                                        text: "Ready to play"
                                    }
                                    np_artist_lbl = <Label> {
                                        width: Fill,
                                        flow: Right,
                                        padding: 0,
                                        draw_text: { color: #A2A6B8, text_style: <THEME_FONT_REGULAR> { font_size: 8.3, line_spacing: 1.0 } }
                                        text: "Pick a song to play"
                                    }
                                }
                                np_like_btn = <FerricIconBtn> {
                                    width: 26, height: 26,
                                    draw_icon: {
                                        svg_path: "M12 21.35L10.55 20.03C5.4 15.36 2 12.28 2 8.5C2 5.42 4.42 3 7.5 3C9.24 3 10.91 3.81 12 5.09C13.09 3.81 14.76 3 16.5 3C19.58 3 22 5.42 22 8.5C22 12.28 18.6 15.36 13.45 20.04L12 21.35Z"
                                        color: #707382
                                    }
                                }
                                np_add_pl_btn = <FerricIconBtn> {
                                    width: 24, height: 24,
                                    draw_icon: {
                                        svg_path: "M19 13H13V19H11V13H5V11H11V5H13V11H19V13Z"
                                        color: #707382
                                    }
                                }
                            }

                            // Center (Stacked 2-Row Transport + Wide Scrubber — Dead-Center Symmetry)
                            <View> {
                                width: Fill,
                                height: Fill,
                                flow: Down,
                                align: { x: 0.5, y: 0.5 }
                                spacing: 4,
                                padding: { left: 16, right: 16 }

                                // Row 1: Centered Transport Controls
                                <View> {
                                    width: Fit,
                                    height: Fit,
                                    flow: Right,
                                    align: { x: 0.5, y: 0.5 }
                                    spacing: 14,

                                    shuffle_btn = <FerricIconBtn> {
                                        width: 28, height: 28,
                                        icon_walk: { width: Fit, height: 15 }
                                        draw_icon: { svg_path: "M10.59 9.17L5.41 4L4 5.41L9.17 10.58L10.59 9.17ZM14.5 4L16.54 6.04L4 18.59L5.41 20L17.96 7.46L20 9.5V4H14.5ZM14.83 13.41L13.42 14.82L16.55 17.95L14.5 20H20V14.5L17.96 16.54L14.83 13.41Z" }
                                    }
                                    prev_btn = <FerricIconBtn> {
                                        width: 28, height: 28,
                                        icon_walk: { width: Fit, height: 15 }
                                        draw_icon: { svg_path: "M6 6H8V18H6V6ZM9.5 12L18 18V6L9.5 12Z" }
                                    }
                                    play_pause_btn = <FerricPlayBtn> {
                                        width: 34, height: 34,
                                        draw_bg: { accent_col: #FFFFFF, accent_hover: #FFFFFF, glyph_col: #09090D }
                                    }
                                    next_btn = <FerricIconBtn> {
                                        width: 28, height: 28,
                                        icon_walk: { width: Fit, height: 15 }
                                        draw_icon: { svg_path: "M6 18L14.5 12L6 6V18ZM16 6V18H18V6H16Z" }
                                    }
                                    repeat_btn = <FerricIconBtn> {
                                        width: 28, height: 28,
                                        icon_walk: { width: Fit, height: 15 }
                                        draw_icon: { svg_path: "M7 7H17V10L21 6L17 2V5H5V11H7V7ZM17 17H7V14L3 18L7 22V19H19V13H17V17Z" }
                                    }
                                }

                                // Row 2: Wide Scrubber Track with Left Elapsed & Right Total/Remaining
                                <View> {
                                    width: Fill,
                                    height: 18,
                                    flow: Right,
                                    align: { x: 0.5, y: 0.5 }
                                    spacing: 10,

                                    pos_lbl = <Label> {
                                        width: 38,
                                        padding: 0,
                                        align: { x: 1.0, y: 0.5 }
                                        draw_text: { color: #B4B7C5, text_style: <THEME_FONT_MEDIUM> { font_size: 8.2 } }
                                        text: "0:00"
                                    }
                                    seek_slider = <FerricBarSlider> {
                                        width: Fill,
                                        height: 16,
                                        min: 0.0, max: 1.0, value: 0.0,
                                        draw_bg: { track_col: #3E404C, fill_col: #F4F4F7, thumb_col: #FFFFFF }
                                    }
                                    dur_btn = <Button> {
                                        width: 40, height: 18, padding: 0,
                                        align: { x: 0.0, y: 0.5 }
                                        text: "0:00"
                                        draw_bg: { fn pixel(self) -> vec4 { return vec4(0.0, 0.0, 0.0, 0.0); } }
                                        draw_text: { color: #B4B7C5, color_hover: #FFFFFF, color_focus: #FFFFFF, text_style: <THEME_FONT_MEDIUM> { font_size: 8.2 } }
                                    }
                                }
                            }

                            // Right (272px matching Left width for exact geometric centering of Play/Pause!)
                            bottom_right_grp = <View> {
                                width: 272,
                                height: Fill,
                                flow: Right,
                                align: { x: 1.0, y: 0.5 }
                                spacing: 3,

                                codec_pill_btn = <FerricBtn> {
                                    height: 22,
                                    padding: { left: 7, right: 7 }
                                    text: "READY"
                                    draw_text: { color: #B4B6C4, color_hover: #FFFFFF, text_style: <THEME_FONT_MEDIUM> { font_size: 7.8 } }
                                }
                                bar_video_btn = <FerricIconBtn> {
                                    width: 28, height: 28,
                                    icon_walk: { width: Fit, height: 15 }
                                    draw_icon: { svg_path: "M18 4L20 8H17L15 4H13L15 8H12L10 4H8L10 8H7L5 4H4C2.9 4 2.01 4.9 2.01 6L2 18C2 19.1 2.9 20 4 20H20C21.1 20 22 19.1 22 18V4H18Z" }
                                }
                                bar_lyrics_btn = <FerricIconBtn> {
                                    width: 28, height: 28,
                                    icon_walk: { width: Fit, height: 15 }
                                    draw_icon: { svg_path: "M12 14C13.66 14 15 12.66 15 11V5C15 3.34 13.66 2 12 2C10.34 2 9 3.34 9 5V11C9 12.66 10.34 14 12 14ZM17 11C17 13.76 14.76 16 12 16C9.24 16 7 13.76 7 11H5C5 14.53 7.61 17.43 11 17.92V21H13V17.92C16.39 17.43 19 14.53 19 11H17Z" }
                                }
                                bar_queue_btn = <FerricIconBtn> {
                                    width: 28, height: 28,
                                    icon_walk: { width: Fit, height: 15 }
                                    draw_icon: { svg_path: "M3 10H14V12H3V10ZM3 6H14V8H3V6ZM3 14H10V16H3V14ZM16 13V21L22 17L16 13Z" }
                                }
                                vol_mute_btn = <FerricIconBtn> {
                                    width: 28, height: 28,
                                    icon_walk: { width: Fit, height: 15 }
                                    draw_icon: { svg_path: "M3 9V15H7L12 20V4L7 9H3ZM16.5 12C16.5 10.23 15.48 8.71 14 7.97V16.02C15.48 15.29 16.5 13.77 16.5 12ZM14 3.23V5.29C16.89 6.15 19 8.83 19 12C19 15.17 16.89 17.85 14 18.71V20.77C18.01 19.86 21 16.28 21 12C21 7.72 18.01 4.14 14 3.23Z" }
                                }
                                vol_slider = <FerricBarSlider> {
                                    width: 62,
                                    height: 18,
                                    min: 0.0, max: 1.0, value: 0.85,
                                    wheel_adjust: true,
                                    wheel_step: 0.05
                                }
                                bar_fullscreen_btn = <FerricIconBtn> {
                                    width: 28, height: 28,
                                    icon_walk: { width: Fit, height: 15 }
                                    draw_icon: { svg_path: "M7 14H5V19H10V17H7V14ZM5 10H7V7H10V5H5V10ZM17 17H14V19H19V14H17V17ZM14 5V7H17V10H19V5H14Z" }
                                }
                            }
                        }
                    }

                    // ================================================================
                    // OVERLAY 1: IMMERSIVE FULL-BLEED INTERACTIVE LYRICS & VIDEO STAGE
                    // ================================================================
                    fullscreen_overlay = <View> {
                        visible: false,
                        width: Fill,
                        height: Fill,
                        flow: Overlay,

                        // Animated full-bleed blurred album artwork backdrop with cursor spotlight
                        fs_backdrop = <FerricAmbientStage> {
                            width: Fill,
                            height: Fill,
                        }

                        fs_stage_content = <View> {
                            width: Fill,
                            height: Fill,
                            padding: { left: 48, right: 38, top: 14, bottom: 18 }
                            flow: Down,
                            spacing: 4,

                            // TOP ULTRA-MINIMAL FLOATING STRIP (generous hit target for Close (Esc))
                            <View> {
                                width: Fill, height: 30, flow: Right, align: { y: 0.5 }, spacing: 8,
                                signal_path_lbl = <Label> {
                                    width: Fill, padding: 0,
                                    draw_text: { color: #FFFFFF4C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.0 } }
                                    text: "Signal Path: WASAPI 48.0 kHz · 6.0s Bass-Swap"
                                }
                                fs_close_btn = <Button> {
                                    width: Fit, height: 28, padding: { left: 14, right: 14 },
                                    cursor: Hand,
                                    text: "Close (Esc)"
                                    draw_text: { color: #FFFFFFCC, color_hover: #FFFFFF, text_style: <THEME_FONT_MEDIUM> { font_size: 8.6 } }
                                    draw_bg: {
                                        instance hover: 0.0
                                        fn pixel(self) -> vec4 {
                                            let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                            let r = (self.rect_size.y - 1.0) * 0.28;
                                            sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);
                                            sdf.fill_keep(vec4(1.0, 1.0, 1.0, 0.08 + 0.12 * self.hover));
                                            sdf.stroke(vec4(1.0, 1.0, 1.0, 0.15 + 0.18 * self.hover), 1.0);
                                            return sdf.result;
                                        }
                                    }
                                    animator: {
                                        hover = {
                                            default: off,
                                            off = { from: { all: Forward { duration: 0.16 } } apply: { draw_bg: { hover: 0.0 }, draw_text: { hover: 0.0 } } }
                                            on = { from: { all: Forward { duration: 0.10 } } apply: { draw_bg: { hover: 1.0 }, draw_text: { hover: 1.0 } } }
                                        }
                                    }
                                }
                            }

                            // MAIN FULL-BLEED IMMERSIVE SPLIT STAGE (Left Floating Cover+Controls | Right Interactive Lyrics)
                            <View> {
                                width: Fill, height: Fill, flow: Right, spacing: 52,

                                // LEFT FLOATING PLAYER STAGE (transparent, no heavy card outline)
                                fs_left_card = <View> {
                                    width: 410,
                                    height: Fill,
                                    padding: { left: 12, right: 12, top: 8, bottom: 8 }
                                    flow: Down,
                                    align: { x: 0.5, y: 0.5 }
                                    spacing: 12,

                                    // Hero Cover Art (304x304 rounded with cursor-tracked hover glare & zoom) OR Docked 16:9 Native Video
                                    <View> {
                                        width: 316,
                                        height: 316,
                                        flow: Overlay,
                                        align: { x: 0.5, y: 0.5 }

                                        fs_art_wrap = <View> {
                                            visible: true,
                                            width: Fill,
                                            height: Fill,
                                            align: { x: 0.5, y: 0.5 }
                                            fs_art = <FerricCoverArt> {
                                                width: 304,
                                                height: 304,
                                                draw_bg: { radius: 28.0, hero: 1.0 }
                                            }
                                        }

                                        fs_video_wrap = <View> {
                                            visible: false,
                                            width: Fill,
                                            height: Fill,
                                            flow: Down,
                                            align: { x: 0.5, y: 0.5 }
                                            spacing: 8,

                                            video_stage_slot = <View> {
                                                width: Fill,
                                                height: 196,
                                                align: { x: 0.5, y: 0.5 }
                                                show_bg: true,
                                                draw_bg: { color: #050508 }
                                                vid_stage_hint_lbl = <Label> {
                                                    padding: 0,
                                                    draw_text: { color: #8A8E9E, text_style: <THEME_FONT_MEDIUM> { font_size: 9.0 } }
                                                    text: "Loading native in-app video clip..."
                                                }
                                            }

                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { x: 0.5, y: 0.5 }, spacing: 4,
                                                vq_auto_btn = <FerricChipBtn> { height: 22, padding: { left: 7, right: 7 }, text: "Auto", draw_text: { text_style: { font_size: 8.0 } } }
                                                vq_360_btn = <FerricChipBtn> { height: 22, padding: { left: 7, right: 7 }, text: "360p", draw_text: { text_style: { font_size: 8.0 } } }
                                                vq_480_btn = <FerricChipBtn> { height: 22, padding: { left: 7, right: 7 }, text: "480p", draw_text: { text_style: { font_size: 8.0 } } }
                                                vq_720_btn = <FerricChipBtn> { height: 22, padding: { left: 7, right: 7 }, text: "720p", draw_text: { text_style: { font_size: 8.0 } } }
                                                vq_1080_btn = <FerricChipBtn> { height: 22, padding: { left: 7, right: 7 }, text: "1080p", draw_text: { text_style: { font_size: 8.0 } } }
                                            }

                                            vid_status_lbl = <Label> {
                                                width: Fill,
                                                padding: 0,
                                                align: { x: 0.5, y: 0.5 }
                                                draw_text: { color: #FFFFFF7A, text_style: <THEME_FONT_REGULAR> { font_size: 8.2 } }
                                                text: "Synced to WASAPI"
                                            }
                                        }
                                    }

                                    // Clean Centered Title + Like Heart & Artist directly below Cover Art
                                    <View> {
                                        width: 312, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3,
                                        <View> {
                                            width: Fit, height: Fit, flow: Right, align: { x: 0.5, y: 0.5 }, spacing: 6,
                                            fs_title_lbl = <Label> {
                                                padding: 0,
                                                align: { x: 0.5, y: 0.5 }
                                                draw_text: { color: #FFFFFF, text_style: <THEME_FONT_DISPLAY> { font_size: 14.5, line_spacing: 1.05 } }
                                                text: "No track playing"
                                            }
                                            fs_like_btn = <FerricIconBtn> {
                                                width: 24, height: 24,
                                                draw_icon: {
                                                    svg_path: "M12 21.35L10.55 20.03C5.4 15.36 2 12.28 2 8.5C2 5.42 4.42 3 7.5 3C9.24 3 10.91 3.81 12 5.09C13.09 3.81 14.76 3 16.5 3C19.58 3 22 5.42 22 8.5C22 12.28 18.6 15.36 13.45 20.04L12 21.35Z"
                                                    color: #FFFFFF70
                                                }
                                            }
                                        }
                                        fs_artist_lbl = <Label> {
                                            width: Fill,
                                            padding: 0,
                                            align: { x: 0.5, y: 0.5 }
                                            draw_text: { color: #FFFFFF8C, text_style: <THEME_FONT_REGULAR> { font_size: 9.6, line_spacing: 1.05 } }
                                            text: ""
                                        }
                                    }

                                    // Minimalist Crisp White Progress Scrubber directly under Artist
                                    <View> {
                                        width: 312, height: 22, flow: Right, align: { y: 0.5 }, spacing: 8,
                                        fs_pos_lbl = <Label> {
                                            width: 34, padding: 0, align: { x: 0.0, y: 0.5 }
                                            draw_text: { color: #FFFFFF80, text_style: <THEME_FONT_MEDIUM> { font_size: 8.0 } }
                                            text: "00:00"
                                        }
                                        fs_seek_slider = <FerricBarSlider> {
                                            width: Fill, height: 18, min: 0.0, max: 1.0, value: 0.0
                                            draw_bg: {
                                                track_col: #FFFFFF2A
                                                fill_col: #FFFFFF
                                                thumb_col: #FFFFFF
                                            }
                                        }
                                        fs_dur_lbl = <Label> {
                                            width: 34, padding: 0, align: { x: 1.0, y: 0.5 }
                                            draw_text: { color: #FFFFFF80, text_style: <THEME_FONT_MEDIUM> { font_size: 8.0 } }
                                            text: "00:00"
                                        }
                                    }

                                    // Minimalist Centered Transport Controls + Volume right beneath Scrubber
                                    <View> {
                                        width: 312, height: 36, flow: Right, align: { x: 0.5, y: 0.5 }, spacing: 6,
                                        fs_shuffle_btn = <FerricIconBtn> {
                                            width: 28, height: 28,
                                            draw_icon: { svg_path: "M10.59 9.17L5.41 4L4 5.41L9.17 10.58L10.59 9.17ZM14.5 4L16.54 6.04L4 18.59L5.41 20L17.96 7.46L20 9.5V4H14.5ZM14.83 13.41L13.42 14.82L16.55 17.95L14.5 20H20V14.5L17.96 16.54L14.83 13.41Z", color: #FFFFFF90 }
                                        }
                                        fs_prev_btn = <FerricIconBtn> {
                                            width: 30, height: 30,
                                            draw_icon: { svg_path: "M6 6H8V18H6V6ZM9.5 12L18 18V6L9.5 12Z", color: #FFFFFFCC }
                                        }
                                        fs_play_pause_btn = <FerricPlayBtn> {
                                            width: 34, height: 34,
                                        }
                                        fs_next_btn = <FerricIconBtn> {
                                            width: 30, height: 30,
                                            draw_icon: { svg_path: "M6 18L14.5 12L6 6V18ZM16 6V18H18V6H16Z", color: #FFFFFFCC }
                                        }
                                        fs_repeat_btn = <FerricIconBtn> {
                                            width: 28, height: 28,
                                            draw_icon: { svg_path: "M7 7H17V10L21 6L17 2V5H5V11H7V7ZM17 17H7V14L3 18L7 22V19H19V13H17V17Z", color: #FFFFFF90 }
                                        }
                                        <View> { width: 6, height: 1 }
                                        fs_vol_mute_btn = <FerricIconBtn> {
                                            width: 26, height: 26,
                                            draw_icon: { svg_path: "M3 9V15H7L12 20V4L7 9H3ZM16.5 12C16.5 10.23 15.48 8.71 14 7.97V16.02C15.48 15.29 16.5 13.77 16.5 12ZM14 3.23V5.29C16.89 6.15 19 8.83 19 12C19 15.17 16.89 17.85 14 18.71V20.77C18.01 19.86 21 16.28 21 12C21 7.72 18.01 4.14 14 3.23Z", color: #FFFFFF90 }
                                        }
                                        fs_vol_slider = <FerricBarSlider> {
                                            width: 56, height: 18, min: 0.0, max: 1.0, value: 0.85,
                                            wheel_adjust: true,
                                            wheel_step: 0.05
                                            draw_bg: {
                                                track_col: #FFFFFF28
                                                fill_col: #FFFFFF
                                                thumb_col: #FFFFFF
                                            }
                                        }
                                    }

                                    // Compact Ambient Spectrum Accent (subtle audio motion pulse)
                                    np_canvas = <FerricNowPlayingCanvas> {
                                        width: 256,
                                        height: 24,
                                    }
                                }

                                // RIGHT FULL-BLEED INTERACTIVE SYNCED LYRICS STAGE (zero box border, pure typography + SDF blur + karaoke sweep)
                                fs_right_card = <View> {
                                    width: Fill,
                                    height: Fill,
                                    padding: { left: 4, right: 16, top: 4, bottom: 4 }
                                    flow: Down,

                                    fs_lyrics_scroll = <FerricLyricsView> {
                                        width: Fill,
                                        height: Fill,
                                        compact: false,
                                    }
                                }
                            }

                            // BOTTOM FLOATING GLASS PILL DOCK (minimal controls tucked bottom-right)
                            fs_transport_card = <View> {
                                width: Fill,
                                height: 34,
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 8,

                                fs_lyr_source = <Label> {
                                    width: Fill, padding: 0,
                                    draw_text: { color: #FFFFFF45, text_style: <THEME_FONT_MEDIUM> { font_size: 8.0 } }
                                    text: "Synced Lyrics"
                                }

                                <View> {
                                    width: Fit,
                                    height: 30,
                                    padding: { left: 8, right: 8, top: 3, bottom: 3 }
                                    flow: Right,
                                    align: { y: 0.5 }
                                    spacing: 5,
                                    show_bg: true,
                                    draw_bg: {
                                        fn pixel(self) -> vec4 {
                                            let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                            let r = (self.rect_size.y - 1.0) * 0.25;
                                            sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);
                                            sdf.fill_keep(vec4(1.0, 1.0, 1.0, 0.055));
                                            sdf.stroke(vec4(1.0, 1.0, 1.0, 0.11), 1.0);
                                            return sdf.result;
                                        }
                                    }
                                    fs_mode_art_btn = <FerricChipBtn> {
                                        height: 22, padding: { left: 8, right: 8 }, text: "Cover",
                                        draw_bg: { active: 1.0 },
                                        draw_text: { active: 1.0, text_style: { font_size: 8.0 } }
                                    }
                                    fs_mode_video_btn = <FerricChipBtn> { height: 22, padding: { left: 8, right: 8 }, text: "Video", draw_text: { text_style: { font_size: 8.0 } } }
                                    fs_lyr_minus = <FerricBtn> { height: 22, padding: { left: 7, right: 7 }, text: "-0.5s", draw_text: { text_style: { font_size: 8.0 } } }
                                    fs_lyr_plus = <FerricBtn> { height: 22, padding: { left: 7, right: 7 }, text: "+0.5s", draw_text: { text_style: { font_size: 8.0 } } }
                                    fs_lyr_copy = <FerricBtn> { height: 22, padding: { left: 8, right: 8 }, text: "Copy", draw_text: { text_style: { font_size: 8.0 } } }
                                    fs_radio_btn = <FerricBtn> { height: 22, padding: { left: 8, right: 8 }, text: "Radio", draw_text: { text_style: { font_size: 8.0 } } }
                                }
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

                        card_cmdk = <FerricCard> {
                            width: 480,
                            padding: 14,
                            flow: Down,
                            spacing: 10,
                            <View> {
                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Ferric Powerbar" }
                                cmdk_close_btn = <FerricBtn> { height: 22, text: "Esc" }
                            }
                            cmdk_input = <FerricInput> {
                                height: 32,
                                empty_text: "Type a song/artist and press Enter to search YouTube Music..."
                            }
                            <View> {
                                width: Fill, height: Fit, flow: Right, spacing: 5,
                                cmdk_home_btn = <FerricBtn> { height: 26, text: "Home" }
                                cmdk_search_btn = <FerricBtn> { height: 26, text: "Search" }
                                cmdk_songs_btn = <FerricBtn> { height: 26, text: "Songs" }
                                cmdk_albums_btn = <FerricBtn> { height: 26, text: "Albums" }
                                cmdk_artists_btn = <FerricBtn> { height: 26, text: "Artists" }
                                cmdk_playlists_btn = <FerricBtn> { height: 26, text: "Playlists" }
                            }
                            <View> {
                                width: Fill, height: Fit, flow: Right, spacing: 5,
                                cmdk_eq_btn = <FerricBtn> { height: 26, text: "Equalizer" }
                                cmdk_fade_btn = <FerricBtn> { height: 26, text: "Fade-In DSP" }
                                cmdk_google_btn = <FerricBtn> { height: 26, text: "Google Login" }
                                cmdk_fs_btn = <FerricBtn> { height: 26, text: "Fullscreen" }
                                cmdk_demo_btn = <FerricBtn> { height: 26, text: "Offline Demo" }
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
                        padding: { right: 16, bottom: 68 }
                        align: { x: 1.0, y: 1.0 }
                        toast_pill = <View> {
                            width: Fit,
                            height: Fit,
                            padding: { left: 12, right: 14, top: 7, bottom: 7 }
                            show_bg: true,
                            draw_bg: {
                                uniform acrylic_on: 0.0
                                fn pixel(self) -> vec4 {
                                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                    sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 7.0);
                                    let fill_c = mix(vec4(0.08, 0.09, 0.13, 0.96), vec4(0.12, 0.13, 0.18, 0.96), self.acrylic_on);
                                    let stroke_c = mix(#2C2E40, vec4(1.0, 1.0, 1.0, 0.16), self.acrylic_on);
                                    sdf.fill_keep(fill_c);
                                    sdf.stroke(stroke_c, 1.0);
                                    return sdf.result;
                                }
                            }
                            toast_lbl = <Label> {
                                draw_text: { color: #F4F4F7, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }
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

                        card_update_popup = <FerricCard> {
                            width: 440,
                            padding: 18,
                            flow: Down,
                            spacing: 10,
                            update_popup_title = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 13.5 } }, text: "Ferric Update Available" }
                            update_popup_sub = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "" }
                            update_popup_notes = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "" }
                            <View> {
                                width: Fill, height: Fit, margin: { top: 6 }, flow: Right, spacing: 6,
                                update_now_btn = <FerricPrimaryBtn> { text: "Download & Install" }
                                update_whats_new_btn = <FerricBtn> { height: 28, text: "Release Notes" }
                                update_popup_close_btn = <FerricBtn> { height: 28, text: "Later" }
                            }
                        }
                    }

                    // ================================================================
                    // OVERLAY 5: TRACK ACTIONS & ADD-TO-PLAYLIST POPOVER (Right-Click / +)
                    // ================================================================
                    track_menu_modal = <View> {
                        visible: false,
                        width: Fill,
                        height: Fill,
                        align: { x: 0.5, y: 0.45 }
                        show_bg: true,
                        draw_bg: { color: #000000A4 }

                        card_track_menu = <FerricCard> {
                            width: 440,
                            padding: 16,
                            flow: Down,
                            spacing: 10,

                            <View> {
                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                <View> {
                                    width: Fill, height: Fit, flow: Down, spacing: 2,
                                    tm_title_lbl = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 11.2 } }, text: "Track Actions" }
                                    tm_sub_lbl = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "" }
                                }
                                tm_close_btn = <FerricBtn> { height: 24, text: "Esc" }
                            }

                            <View> {
                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                tm_play_btn = <FerricPrimaryBtn> { height: 26, text: "Play Now" }
                                tm_play_next_btn = <FerricBtn> { height: 26, text: "Play Next" }
                                tm_add_queue_btn = <FerricBtn> { height: 26, text: "+ Queue" }
                                tm_radio_btn = <FerricBtn> { height: 26, text: "Start Radio" }
                            }

                            <View> {
                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                tm_like_btn = <FerricBtn> { height: 26, text: "Like" }
                                tm_artist_btn = <FerricBtn> { height: 26, text: "Go to Artist" }
                                tm_album_btn = <FerricBtn> { height: 26, text: "Go to Album" }
                                tm_copy_btn = <FerricBtn> { height: 26, text: "Copy Info" }
                            }

                            <FerricDividerH> {}

                            <Label> { draw_text: { color: #9A9CA8, text_style: <THEME_FONT_BOLD> { font_size: 8.2 } }, text: "ADD TO PLAYLIST" }
                            <View> {
                                width: Fill, height: Fit, flow: Right, spacing: 5,
                                tm_pl_0 = <FerricChipBtn> { visible: false, text: "Playlist 1" }
                                tm_pl_1 = <FerricChipBtn> { visible: false, text: "Playlist 2" }
                                tm_pl_2 = <FerricChipBtn> { visible: false, text: "Playlist 3" }
                            }
                            <View> {
                                width: Fill, height: Fit, flow: Right, spacing: 5,
                                tm_pl_3 = <FerricChipBtn> { visible: false, text: "Playlist 4" }
                                tm_pl_4 = <FerricChipBtn> { visible: false, text: "Playlist 5" }
                                tm_pl_5 = <FerricChipBtn> { visible: false, text: "Playlist 6" }
                            }
                            <View> {
                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                tm_new_pl_input = <FerricInput> { width: Fill, height: 28, empty_text: "Create new playlist & add track..." }
                                tm_new_pl_btn = <FerricPrimaryBtn> { height: 28, text: "+ Create" }
                            }

                            <FerricDividerH> {}

                            <View> {
                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                <View> { width: Fill, height: 1 }
                                tm_remove_btn = <FerricBtn> {
                                    height: 26,
                                    text: "Remove from Library"
                                    draw_text: { color: #FF7885, color_hover: #FFA3AC }
                                }
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
    shelf_pages: [usize; 8],
    #[rust]
    home_loading_more: bool,
    #[rust]
    home_more_cycle: usize,
    #[rust]
    missing_art_queue: Rc<RefCell<Vec<Track>>>,
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
    search_request_seq: u64,
    #[rust]
    pending_live_search: Option<(String, Instant)>,
    #[rust]
    last_dispatched_search: String,
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
    video_player: Option<NativeVideoPlayer>,
    #[rust]
    video_open: bool,
    #[rust]
    video_track_id: Option<String>,
    #[rust]
    video_streams: Vec<VideoStreamOption>,
    #[rust]
    last_video_sync: Option<Instant>,
    #[rust]
    displayed_accent: Vec4,
    #[rust]
    started_at: Option<Instant>,
    #[rust]
    startup_trim_stage: u8,
    #[rust]
    last_trim_at: Option<Instant>,
    #[rust]
    artwork_uploads_since_trim: u32,
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
    cmdk_open: bool,
    #[rust]
    track_menu_open: bool,
    #[rust]
    track_menu_target: Option<Track>,
    #[rust]
    songs_filter_query: String,
    #[rust]
    window_hidden_or_minimized: bool,
    #[rust]
    tray_second_trim_at: Option<Instant>,
    #[rust]
    toast_until: Option<Instant>,
    #[rust]
    canvas_phase: f32,
    #[rust]
    motion_frame: NextFrame,
    #[rust]
    motion_last_t: f64,
    #[rust]
    page_reveal_t: f32,
    #[rust]
    fs_reveal_t: f32,
    #[rust]
    cmdk_reveal_t: f32,
    #[rust]
    toast_reveal_t: f32,
    #[rust]
    left_w_cur: f64,
    #[rust]
    left_w_target: f64,
    #[rust]
    right_w_cur: f64,
    #[rust]
    right_w_target: f64,
    #[rust]
    last_pos_txt: String,
    #[rust]
    last_dur_txt: String,
    #[rust]
    last_codec_txt: String,
    #[rust]
    sleep_timer: SleepTimerMode,
    #[rust]
    discord_settings: DiscordRpcSettings,
    #[rust]
    discord_rpc: Option<DiscordRpcBridge>,
    #[rust]
    last_discord_status: String,
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
        let right_pad = if is_max { 12.0 } else { 8.0 };
        self.ui
            .view(id!(top_bar))
            .apply_over(cx, live! { padding: { left: 10.0, right: (right_pad) } });
    }

    fn start_motion(&mut self, cx: &mut Cx) {
        self.motion_frame = cx.new_next_frame();
    }

    fn active_page_view_id(&self) -> &'static [LiveId] {
        match self.active_page {
            MainPage::Home => id!(page_home),
            MainPage::Search => id!(page_search),
            MainPage::Recents => id!(page_recents),
            MainPage::Songs => id!(page_songs),
            MainPage::Albums => id!(page_albums),
            MainPage::Artists => id!(page_artists),
            MainPage::Playlists => id!(page_playlists),
            MainPage::Detail => id!(page_detail),
            MainPage::Settings => id!(page_settings),
        }
    }

    fn step_motion(&mut self, cx: &mut Cx, time: f64) {
        let dt = if self.motion_last_t > 0.0 {
            (time - self.motion_last_t).clamp(0.004, 0.045)
        } else {
            0.016
        };
        self.motion_last_t = time;
        let k = (1.0 - (-16.0 * dt).exp()) as f32;
        let mut active = false;

        if self.page_reveal_t < 0.995 {
            self.page_reveal_t = (self.page_reveal_t + (1.0 - self.page_reveal_t) * k).min(1.0);
            if 1.0 - self.page_reveal_t < 0.01 {
                self.page_reveal_t = 1.0;
            } else {
                active = true;
            }
            let (base_top, base_bottom) = match self.active_page {
                MainPage::Home => (12.0, 26.0),
                MainPage::Albums | MainPage::Artists | MainPage::Playlists | MainPage::Settings => {
                    (14.0, 26.0)
                }
                _ => (14.0, 12.0),
            };
            let top_pad = base_top + 16.0 * (1.0 - f64::from(self.page_reveal_t));
            self.ui.view(self.active_page_view_id()).apply_over(
                cx,
                live! { padding: { left: 18.0, right: 18.0, top: (top_pad), bottom: (base_bottom) } },
            );
        }

        if self.fullscreen_open && self.fs_reveal_t < 0.995 {
            self.fs_reveal_t = (self.fs_reveal_t + (1.0 - self.fs_reveal_t) * k).min(1.0);
            if 1.0 - self.fs_reveal_t < 0.01 {
                self.fs_reveal_t = 1.0;
            } else {
                active = true;
            }
            let top_pad = 14.0 + 18.0 * (1.0 - f64::from(self.fs_reveal_t));
            self.ui.view(id!(fs_stage_content)).apply_over(
                cx,
                live! { padding: { left: 48.0, right: 38.0, top: (top_pad), bottom: 18.0 } },
            );
        }

        if self.cmdk_open && self.cmdk_reveal_t < 0.995 {
            self.cmdk_reveal_t = (self.cmdk_reveal_t + (1.0 - self.cmdk_reveal_t) * k).min(1.0);
            if 1.0 - self.cmdk_reveal_t < 0.01 {
                self.cmdk_reveal_t = 1.0;
            } else {
                active = true;
            }
            let y_align = 0.20 + 0.03 * (1.0 - f64::from(self.cmdk_reveal_t));
            self.ui
                .view(id!(cmdk_modal))
                .apply_over(cx, live! { align: { x: 0.5, y: (y_align) } });
        }

        if self.toast_until.is_some() && self.toast_reveal_t < 0.995 {
            self.toast_reveal_t = (self.toast_reveal_t + (1.0 - self.toast_reveal_t) * k).min(1.0);
            if 1.0 - self.toast_reveal_t < 0.01 {
                self.toast_reveal_t = 1.0;
            } else {
                active = true;
            }
            let b_pad = 54.0 + 14.0 * f64::from(self.toast_reveal_t);
            self.ui
                .view(id!(toast_wrap))
                .apply_over(cx, live! { padding: { right: 16.0, bottom: (b_pad) } });
        }

        if (self.left_w_target - self.left_w_cur).abs() > 0.5 {
            self.left_w_cur += (self.left_w_target - self.left_w_cur) * f64::from(k);
            if (self.left_w_target - self.left_w_cur).abs() <= 0.8 {
                self.left_w_cur = self.left_w_target;
            } else {
                active = true;
            }
            let vis = self.left_w_cur > 2.0;
            self.ui.view(id!(left_sidebar)).set_visible(cx, vis);
            self.ui.view(id!(left_divider)).set_visible(cx, vis);
            if vis {
                self.ui
                    .view(id!(left_sidebar))
                    .apply_over(cx, live! { width: (self.left_w_cur) });
            }
            self.sync_video_bounds(cx);
        }

        if (self.right_w_target - self.right_w_cur).abs() > 0.5 {
            self.right_w_cur += (self.right_w_target - self.right_w_cur) * f64::from(k);
            if (self.right_w_target - self.right_w_cur).abs() <= 0.8 {
                self.right_w_cur = self.right_w_target;
            } else {
                active = true;
            }
            let vis = self.right_w_cur > 2.0;
            self.ui.view(id!(right_sidebar)).set_visible(cx, vis);
            self.ui.view(id!(right_divider)).set_visible(cx, vis);
            if vis {
                self.ui
                    .view(id!(right_sidebar))
                    .apply_over(cx, live! { width: (self.right_w_cur) });
            }
            self.sync_video_bounds(cx);
        }

        if active {
            self.motion_frame = cx.new_next_frame();
        } else {
            self.motion_last_t = 0.0;
        }
        self.ui.redraw(cx);
    }

    fn style_chip_btn(&self, cx: &mut Cx, btn_id: &[LiveId], active: bool) {
        let a = if active { 1.0_f32 } else { 0.0_f32 };
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        self.ui.button(btn_id).apply_over(
            cx,
            live! {
                draw_bg: { active: (a), acrylic_on: (acr) }
                draw_text: { active: (a) }
            },
        );
    }

    fn target_accent(&self) -> Vec4 {
        if self.theme.adaptive_album_art
            && let Some(cur) = self.queue.current()
            && let Some(art_cache) = &self.artwork
            && let Some(cached) = art_cache.get_for_track(cur)
        {
            return cached.accent;
        }
        rgba_vec4(&self.theme.accent_hex)
    }

    fn current_accent(&self) -> Vec4 {
        if self.displayed_accent.w > 0.5 {
            self.displayed_accent
        } else {
            self.target_accent()
        }
    }

    fn show_toast(&mut self, cx: &mut Cx, message: &str) {
        self.toast_until = Some(Instant::now() + Duration::from_secs(3));
        self.toast_reveal_t = 0.0;
        self.ui.label(id!(toast_lbl)).set_text(cx, message);
        self.ui.view(id!(toast_wrap)).set_visible(cx, true);
        self.start_motion(cx);
        self.ui.redraw(cx);
    }

    fn adjust_ui_zoom(&mut self, cx: &mut Cx, delta: f32) {
        let next = ((self.theme.ui_zoom + delta) * 10.0).round() / 10.0;
        self.set_ui_zoom(cx, next);
    }

    fn set_ui_zoom(&mut self, cx: &mut Cx, zoom: f32) {
        let clamped = zoom.clamp(0.65, 1.40);
        self.theme.ui_zoom = clamped;
        self.apply_ui_zoom(cx);
        if let Some(db) = &self.db {
            let _ = db.save_theme(&self.theme);
        }
        self.show_toast(cx, &format!("UI Zoom: {:.0}%", clamped * 100.0));
    }

    fn apply_ui_zoom(&mut self, cx: &mut Cx) {
        let z = self.theme.ui_zoom.clamp(0.65, 1.40) as f64;
        self.ui
            .label(id!(zoom_val_lbl))
            .set_text(cx, &format!("{:.0}%", z * 100.0));

        self.left_w_target = if self.left_sidebar_open {
            168.0 * z
        } else {
            0.0
        };
        self.left_w_cur = self.left_w_target;
        self.right_w_target = if self.right_sidebar_open {
            260.0 * z
        } else {
            0.0
        };
        self.right_w_cur = self.right_w_target;

        self.ui
            .view(id!(top_bar))
            .apply_over(cx, live! { height: (36.0 * z) });
        if self.left_sidebar_open {
            self.ui
                .view(id!(left_sidebar))
                .apply_over(cx, live! { width: (168.0 * z) });
        }
        if self.right_sidebar_open {
            self.ui
                .view(id!(right_sidebar))
                .apply_over(cx, live! { width: (260.0 * z) });
        }
        self.ui
            .view(id!(bottom_bar))
            .apply_over(cx, live! { height: (74.0 * z) });
        self.ui
            .view(id!(bottom_left_grp))
            .apply_over(cx, live! { width: (272.0 * z) });
        self.ui
            .view(id!(bottom_right_grp))
            .apply_over(cx, live! { width: (272.0 * z) });
        self.ui
            .ferric_cover_art(id!(np_art))
            .apply_over(cx, live! { width: (44.0 * z), height: (44.0 * z) });
        self.ui
            .ferric_play_btn(id!(play_pause_btn))
            .apply_over(cx, live! { width: (34.0 * z), height: (34.0 * z) });
        self.ui.label(id!(np_title_lbl)).apply_over(
            cx,
            live! { draw_text: { text_style: { font_size: (9.4 * z) } } },
        );
        self.ui.label(id!(np_artist_lbl)).apply_over(
            cx,
            live! { draw_text: { text_style: { font_size: (8.2 * z) } } },
        );
        self.ui
            .view(id!(fs_left_card))
            .apply_over(cx, live! { width: (410.0 * z) });
        self.ui
            .ferric_cover_art(id!(fs_art))
            .apply_over(cx, live! { width: (304.0 * z), height: (304.0 * z) });

        for nid in [
            id!(nav_home),
            id!(nav_search),
            id!(nav_recents),
            id!(nav_songs),
            id!(nav_albums),
            id!(nav_artists),
            id!(nav_playlists),
            id!(nav_settings),
            id!(sidebar_add_folder_btn),
        ] {
            self.ui.button(nid).apply_over(
                cx,
                live! {
                    height: (28.0 * z),
                    draw_text: { text_style: { font_size: (9.2 * z) } }
                },
            );
        }

        for vid in [
            id!(qp_0),
            id!(qp_1),
            id!(qp_2),
            id!(qp_3),
            id!(qp_4),
            id!(qp_5),
            id!(qp_6),
            id!(qp_7),
        ] {
            self.ui.view(vid).apply_over(
                cx,
                live! {
                    height: (48.0 * z),
                    art = { width: (36.0 * z), height: (36.0 * z) }
                    title = { draw_text: { text_style: { font_size: (9.4 * z) } } }
                    sub = { draw_text: { text_style: { font_size: (8.2 * z) } } }
                },
            );
        }

        for cid in [
            id!(sh0_c0),
            id!(sh0_c1),
            id!(sh0_c2),
            id!(sh0_c3),
            id!(sh0_c4),
            id!(sh1_c0),
            id!(sh1_c1),
            id!(sh1_c2),
            id!(sh1_c3),
            id!(sh1_c4),
            id!(sh2_c0),
            id!(sh2_c1),
            id!(sh2_c2),
            id!(sh2_c3),
            id!(sh2_c4),
            id!(sh3_c0),
            id!(sh3_c1),
            id!(sh3_c2),
            id!(sh3_c3),
            id!(sh3_c4),
            id!(sh4_c0),
            id!(sh4_c1),
            id!(sh4_c2),
            id!(sh4_c3),
            id!(sh4_c4),
            id!(sh5_c0),
            id!(sh5_c1),
            id!(sh5_c2),
            id!(sh5_c3),
            id!(sh5_c4),
            id!(sh6_c0),
            id!(sh6_c1),
            id!(sh6_c2),
            id!(sh6_c3),
            id!(sh6_c4),
            id!(sh7_c0),
            id!(sh7_c1),
            id!(sh7_c2),
            id!(sh7_c3),
            id!(sh7_c4),
            id!(sc_0),
            id!(sc_1),
            id!(sc_2),
            id!(sc_3),
            id!(sc_4),
        ] {
            let cv = self.ui.view(cid);
            cv.apply_over(
                cx,
                live! {
                    height: (174.0 * z),
                    inner = { art = { height: (116.0 * z) } }
                },
            );
            cv.label(id!(title)).apply_over(
                cx,
                live! { draw_text: { text_style: { font_size: (9.0 * z) } } },
            );
            cv.label(id!(sub)).apply_over(
                cx,
                live! { draw_text: { text_style: { font_size: (8.0 * z) } } },
            );
        }

        for gid in [
            id!(alb_0),
            id!(alb_1),
            id!(alb_2),
            id!(alb_3),
            id!(alb_4),
            id!(alb_5),
            id!(alb_6),
            id!(alb_7),
            id!(alb_8),
            id!(alb_9),
            id!(alb_10),
            id!(alb_11),
            id!(alb_12),
            id!(alb_13),
            id!(alb_14),
            id!(alb_15),
            id!(alb_16),
            id!(alb_17),
            id!(alb_18),
            id!(alb_19),
            id!(art_0),
            id!(art_1),
            id!(art_2),
            id!(art_3),
            id!(art_4),
            id!(art_5),
            id!(art_6),
            id!(art_7),
            id!(art_8),
            id!(art_9),
            id!(art_10),
            id!(art_11),
            id!(art_12),
            id!(art_13),
            id!(art_14),
            id!(art_15),
            id!(art_16),
            id!(art_17),
            id!(art_18),
            id!(art_19),
            id!(plc_0),
            id!(plc_1),
            id!(plc_2),
            id!(plc_3),
            id!(plc_4),
            id!(plc_5),
            id!(plc_6),
            id!(plc_7),
            id!(plc_8),
            id!(plc_9),
            id!(plc_10),
            id!(plc_11),
            id!(plc_12),
            id!(plc_13),
            id!(plc_14),
            id!(plc_15),
            id!(plc_16),
            id!(plc_17),
            id!(plc_18),
            id!(plc_19),
        ] {
            let gv = self.ui.view(gid);
            gv.apply_over(
                cx,
                live! {
                    height: (194.0 * z),
                    inner = { art = { height: (136.0 * z) } }
                },
            );
            gv.label(id!(title)).apply_over(
                cx,
                live! { draw_text: { text_style: { font_size: (9.0 * z) } } },
            );
            gv.label(id!(sub)).apply_over(
                cx,
                live! { draw_text: { text_style: { font_size: (8.0 * z) } } },
            );
        }

        for qid in [
            id!(qh_0),
            id!(qh_1),
            id!(qh_2),
            id!(q_now_row),
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
        ] {
            self.ui.view(qid).apply_over(
                cx,
                live! {
                    height: (44.0 * z),
                    art = { width: (32.0 * z), height: (32.0 * z) }
                    title = { draw_text: { text_style: { font_size: (9.0 * z) } } }
                    sub = { draw_text: { text_style: { font_size: (8.0 * z) } } }
                },
            );
        }

        let accent = self.current_accent();
        self.ui
            .ferric_lyrics_view(id!(rs_lyrics_scroll))
            .set_style(cx, accent, z as f32);
        self.ui
            .ferric_lyrics_view(id!(fs_lyrics_scroll))
            .set_style(cx, accent, z as f32);

        self.ui.redraw(cx);
    }

    fn compute_video_viewport_rect(&self, cx: &Cx) -> Option<VideoViewportRect> {
        let client = win32_client_rect_screen()?;
        let slot = self.ui.view(id!(video_stage_slot)).area().rect(cx);
        let scale = client.dpi_scale.max(0.5);
        if slot.size.x > 40.0 && slot.size.y > 40.0 {
            Some(VideoViewportRect {
                x: client.left + (slot.pos.x as f32 * scale).round() as i32,
                y: client.top + (slot.pos.y as f32 * scale).round() as i32,
                width: (slot.size.x as f32 * scale).round() as i32,
                height: (slot.size.y as f32 * scale).round() as i32,
            })
        } else {
            // Fallback geometry matching fs_left_card's 16:9 video_stage_slot inside Fullscreen Lyrics Mode
            let left_px = (48.0 * scale).round() as i32;
            let top_px = (128.0 * scale).round() as i32;
            let width_px = (412.0 * scale).round() as i32;
            let height_px = (234.0 * scale).round() as i32;
            Some(VideoViewportRect {
                x: client.left + left_px,
                y: client.top + top_px,
                width: width_px.min((client.width - left_px * 2).max(240)),
                height: height_px.min((client.height - top_px - 160).max(160)),
            })
        }
    }

    fn refresh_video_quality_buttons(&self, cx: &mut Cx) {
        let cur_q = self.dsp.video_quality;
        for (btn, q) in [
            (id!(vq_auto_btn), VideoQuality::Auto),
            (id!(vq_360_btn), VideoQuality::P360),
            (id!(vq_480_btn), VideoQuality::P480),
            (id!(vq_720_btn), VideoQuality::P720),
            (id!(vq_1080_btn), VideoQuality::P1080),
            (id!(svq_auto_btn), VideoQuality::Auto),
            (id!(svq_360_btn), VideoQuality::P360),
            (id!(svq_480_btn), VideoQuality::P480),
            (id!(svq_720_btn), VideoQuality::P720),
            (id!(svq_1080_btn), VideoQuality::P1080),
        ] {
            self.style_chip_btn(cx, btn, cur_q == q);
        }
    }

    fn pick_best_video_stream(&self) -> Option<&VideoStreamOption> {
        if self.video_streams.is_empty() {
            return None;
        }
        let target_h = self.dsp.video_quality.target_height();
        if let Some(exact) = self
            .video_streams
            .iter()
            .find(|s| s.quality == self.dsp.video_quality)
        {
            return Some(exact);
        }
        self.video_streams
            .iter()
            .min_by_key(|s| s.height.abs_diff(target_h))
    }

    fn load_selected_video_quality(&mut self, cx: &mut Cx) {
        self.refresh_video_quality_buttons(cx);
        let Some(chosen) = self.pick_best_video_stream().cloned() else {
            return;
        };
        let Some(rect) = self.compute_video_viewport_rect(cx) else {
            return;
        };
        let (pos_sec, playing) = self
            .audio
            .as_ref()
            .map(|a| {
                let s = a.telemetry();
                (s.position_ms as f64 / 1000.0, s.is_playing)
            })
            .unwrap_or((0.0, true));
        self.ui.label(id!(vid_status_lbl)).set_text(
            cx,
            &format!(
                "{} ({}p · {} kbps) · Synced to WASAPI",
                chosen.label, chosen.height, chosen.bitrate_kbps
            ),
        );
        self.ui.label(id!(vid_stage_hint_lbl)).set_text(cx, "");
        let owner = win32_main_hwnd_raw();
        if let Some(vp) = &mut self.video_player {
            vp.open_or_load(owner, rect, chosen.url, pos_sec, playing);
        }
        self.last_video_sync = Some(Instant::now());
    }

    fn sync_video_bounds(&self, cx: &Cx) {
        if !self.video_open {
            return;
        }
        let Some(vp) = &self.video_player else {
            return;
        };
        if !vp.is_open() {
            return;
        }
        let obstructed = !self.fullscreen_open
            || self.cmdk_open
            || self.track_menu_open
            || self.update_popup_visible
            || win32_is_window_hidden_or_minimized();
        if obstructed {
            vp.set_visible(false);
            return;
        }
        if let Some(rect) = self.compute_video_viewport_rect(cx) {
            vp.set_bounds(rect);
            vp.set_visible(true);
        }
    }

    fn open_native_video_stage(&mut self, cx: &mut Cx) {
        let Some(cur) = self.queue.current().cloned() else {
            self.show_toast(cx, "Play a song first to watch its video clip");
            return;
        };
        // Video Clip lives INSIDE Fullscreen Lyrics Mode replacing the Left Cover Art card
        if !self.fullscreen_open {
            self.set_fullscreen(cx, true);
        }
        self.video_open = true;
        self.ui.view(id!(fs_art_wrap)).set_visible(cx, false);
        self.ui.view(id!(fs_video_wrap)).set_visible(cx, true);
        self.style_chip_btn(cx, id!(fs_mode_art_btn), false);
        self.style_chip_btn(cx, id!(fs_mode_video_btn), true);
        self.refresh_video_quality_buttons(cx);
        let accent = self.current_accent();
        self.ui.button(id!(bar_video_btn)).apply_over(
            cx,
            live! { draw_bg: { active: 1.0 }, draw_icon: { color: (accent) } },
        );

        // Reuse already loaded streams for the same track
        if self.video_track_id.as_deref() == Some(cur.id.as_str()) && !self.video_streams.is_empty()
        {
            self.load_selected_video_quality(cx);
            self.ui.redraw(cx);
            return;
        }

        // Check for a REAL local sidecar video file next to a Local audio track only
        // (never match cached YouTube audio-only `.webm` files in stream_cache!).
        if cur.source == TrackSource::Local
            && let Some(local_p) = &cur.path
        {
            for ext in ["mp4", "webm", "mkv"] {
                let cand = local_p.with_extension(ext);
                if cand != *local_p && cand.is_file() {
                    let uri = format!("file:///{}", cand.to_string_lossy().replace('\\', "/"));
                    self.video_track_id = Some(cur.id.clone());
                    self.video_streams = vec![VideoStreamOption {
                        quality: VideoQuality::P1080,
                        label: "Local HD".to_string(),
                        width: 1920,
                        height: 1080,
                        bitrate_kbps: 0,
                        url: uri,
                        mime: "video/mp4".to_string(),
                    }];
                    self.load_selected_video_quality(cx);
                    self.ui.redraw(cx);
                    return;
                }
            }
        }

        self.video_track_id = Some(cur.id.clone());
        self.video_streams.clear();
        self.ui
            .label(id!(vid_status_lbl))
            .set_text(cx, "Resolving native MP4 video streams...");
        self.ui
            .label(id!(vid_stage_hint_lbl))
            .set_text(cx, "Loading in-app video clip...");
        self.ui.redraw(cx);

        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let session = self.current_ytm_session();
        let track_id = cur.id;
        let artist = cur.artist;
        let title = cur.title;
        let existing_vid = cur.youtube_video_id;
        thread::spawn(move || {
            let res = (|| {
                let vid = match existing_vid {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => resolve_best_video_id_for_track(&session, &artist, &title)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| "No official music video found for this song".to_string())?,
                };
                let streams = fetch_video_streams(&vid, &session).map_err(|e| e.to_string())?;
                Ok::<_, String>((vid, streams))
            })();
            match res {
                Ok((video_id, streams)) => {
                    let _ = tx.send(BgMessage::VideoStreamsLoaded {
                        track_id,
                        video_id,
                        streams,
                    });
                }
                Err(error) => {
                    let _ = tx.send(BgMessage::VideoStreamsFailed { track_id, error });
                }
            }
        });
    }

    fn close_native_video_stage(&mut self, cx: &mut Cx) {
        self.video_open = false;
        self.ui.view(id!(fs_video_wrap)).set_visible(cx, false);
        self.ui.view(id!(fs_art_wrap)).set_visible(cx, true);
        self.style_chip_btn(cx, id!(fs_mode_art_btn), true);
        self.style_chip_btn(cx, id!(fs_mode_video_btn), false);
        if let Some(vp) = &mut self.video_player {
            vp.close();
        }
        let muted = vec4(0.604, 0.612, 0.659, 1.0);
        self.ui.button(id!(bar_video_btn)).apply_over(
            cx,
            live! { draw_bg: { active: 0.0 }, draw_icon: { color: (muted) } },
        );
        self.ui.redraw(cx);
    }

    fn ensure_initialized(&mut self, cx: &mut Cx) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        self.started_at = Some(Instant::now());
        self.startup_trim_stage = 0;
        self.last_trim_at = None;
        self.artwork_uploads_since_trim = 0;
        self.active_page = MainPage::Home;
        self.active_settings_tab = SettingsTab::Accounts;
        self.nav_history = Some(NavHistory::new(MainPage::Home));
        self.left_sidebar_open = true;
        self.right_sidebar_open = true;
        self.right_lyrics_tab = false;
        self.page_reveal_t = 1.0;
        self.fs_reveal_t = 1.0;
        self.cmdk_reveal_t = 1.0;
        self.toast_reveal_t = 1.0;
        self.missing_art_queue = Rc::new(RefCell::new(Vec::new()));
        self.pre_mute_volume = 0.85;
        self.queue_context = "Quick picks".to_string();
        self.artwork = Some(ArtworkCache::new());
        self.video_player = Some(NativeVideoPlayer::new(
            app_data_dir().join("webview2_video"),
        ));
        self.hide_native_makepad_caption(cx);
        win32_install_window_hooks();

        let db_path = app_data_dir().join("ferric.db");
        let mut db = LibraryDb::open(&db_path)
            .or_else(|_| LibraryDb::open_in_memory())
            .ok();

        if let Some(store) = &mut db {
            self.theme = store.load_theme().unwrap_or_default();
            // Upgrade legacy Liquid Glass / older theme names cleanly to AMOLED Black default
            if !matches!(
                self.theme.name.as_str(),
                "AMOLED Black" | "Native Acrylic" | "Obsidian Studio" | "Warm Cream Light"
            ) {
                self.theme = ThemeConfig::default();
                let _ = store.save_theme(&self.theme);
            }
            // Migrate legacy violet default accent to Ferric signature Burnt Orange (#E86A2C)
            if self.theme.accent_hex.eq_ignore_ascii_case("#A882FF")
                || parse_hex_rgb(&self.theme.accent_hex).is_err()
            {
                self.theme.accent_hex = "#E86A2C".to_string();
                let _ = store.save_theme(&self.theme);
            }
            if parse_hex_rgb(&self.theme.acrylic_tint_hex).is_err() {
                self.theme.acrylic_tint_hex = "#0B0D14".to_string();
                let _ = store.save_theme(&self.theme);
            }
            if !(0.65..=1.40).contains(&self.theme.ui_zoom) {
                self.theme.ui_zoom = 1.0;
            }
            self.theme.acrylic_opacity = self.theme.acrylic_opacity.clamp(0.18, 0.94);
            self.eq = store.load_eq().unwrap_or_default();
            self.dsp = store.load_playback_dsp().unwrap_or_default();
            self.accounts = store.load_accounts().unwrap_or_default();
            self.discord_settings = store.load_discord_rpc().unwrap_or_default();
            // When signed out (Guest Mode), remove stale synced Google/YouTube account playlists
            // while keeping manually created and manually imported playlists intact.
            let yt_signed_in = self.accounts.youtube_connected
                && !self.accounts.youtube_cookie.trim().is_empty()
                && !self.accounts.youtube_sapisid.trim().is_empty();
            if !yt_signed_in {
                let _ = store.delete_account_synced_playlists();
            }
            self.update_settings = store.load_update_settings().unwrap_or_default();
            self.session_state = store.load_session_state().unwrap_or_default();
            let _ = store.prune_ephemeral_discovery_bloat(&self.session_state.recent_track_ids);
            self.all_tracks = store.list_tracks("").unwrap_or_default();
            self.playlists = store.list_playlists().unwrap_or_default();
        }
        let init_z = self.theme.ui_zoom.clamp(0.65, 1.40) as f64;
        self.left_w_cur = 168.0 * init_z;
        self.left_w_target = 168.0 * init_z;
        self.right_w_cur = 260.0 * init_z;
        self.right_w_target = 260.0 * init_z;
        self.displayed_accent = rgba_vec4(&self.theme.accent_hex);
        win32_set_close_to_tray(self.dsp.close_to_tray);
        self.queue.shuffle = self.session_state.shuffle;
        self.queue.repeat = self.session_state.repeat;
        self.db = db;

        let audio = AudioEngine::start(self.eq.clone())
            .unwrap_or_else(|_| AudioEngine::start_headless(self.eq.clone()));
        audio.send(EngineCommand::SetDspSettings(self.dsp.clone()));
        audio.send(EngineCommand::SetVolume(self.session_state.volume));
        self.audio = Some(audio);

        self.smtc = Some(SmtcBridge::try_init());
        self.discord_rpc = Some(DiscordRpcBridge::start(self.discord_settings.clone()));
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
        self.apply_ui_zoom(cx);
        self.refresh_home_ui(cx);
        self.refresh_right_sidebar(cx);
        self.refresh_storage_stats(cx);
        self.tick_timer = cx.start_interval(0.05);

        if startup_load_demo() {
            self.load_and_play_demo_suite(cx);
        } else {
            self.spawn_ytm_rich_home(cx, 0, None, !self.accounts.youtube_connected);
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
            "integrations" | "discord" => {
                self.navigate_to(cx, MainPage::Settings, false);
                self.switch_settings_tab(cx, SettingsTab::Integrations);
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
            "lyrics-tab" => {
                self.set_right_sidebar(cx, true, true);
            }
            "video" => {
                self.open_native_video_stage(cx);
            }
            "acrylic" => {
                let z = self.theme.ui_zoom;
                self.theme = ThemeConfig::native_acrylic();
                self.theme.ui_zoom = z;
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
                self.navigate_to(cx, MainPage::Home, false);
            }
            "acrylic-settings" => {
                let z = self.theme.ui_zoom;
                self.theme = ThemeConfig::native_acrylic();
                self.theme.ui_zoom = z;
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
                self.navigate_to(cx, MainPage::Settings, false);
                self.switch_settings_tab(cx, SettingsTab::Appearance);
            }
            "amoled" | "oled" => {
                let z = self.theme.ui_zoom;
                self.theme = ThemeConfig::pure_amoled();
                self.theme.ui_zoom = z;
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
            }
            "cmdk" => {
                self.cmdk_open = true;
                self.cmdk_reveal_t = 0.0;
                self.ui.view(id!(cmdk_modal)).set_visible(cx, true);
                self.start_motion(cx);
                self.ui.redraw(cx);
            }
            _ => {}
        }
    }

    fn navigate_to(&mut self, cx: &mut Cx, target: MainPage, push_history: bool) {
        if self.video_open {
            self.close_native_video_stage(cx);
        }
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
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
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
                    draw_bg: { active: (if active { 1.0 } else { 0.0 }), acrylic_on: (acr) }
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
            MainPage::Home => self.refresh_home_ui(cx),
            MainPage::Songs => {
                let visible: Vec<Track> = self.filtered_tracks.iter().take(16).cloned().collect();
                self.request_artwork_for_tracks(&visible);
            }
            MainPage::Recents => {
                let visible: Vec<Track> = self.recents_tracks.iter().take(16).cloned().collect();
                self.request_artwork_for_tracks(&visible);
            }
            MainPage::Albums => self.refresh_albums_grid(cx),
            MainPage::Artists => self.refresh_artists_grid(cx),
            MainPage::Playlists => self.refresh_playlists_grid(cx),
            MainPage::Search => {
                if self.search_tracks.is_empty() {
                    self.search_tracks = Rc::new(self.home_quick_picks_pool());
                }
                let visible: Vec<Track> = self.search_tracks.iter().take(16).cloned().collect();
                self.request_artwork_for_tracks(&visible);
                self.refresh_search_ui(cx);
            }
            MainPage::Settings => self.refresh_storage_stats(cx),
            _ => {}
        }
        self.page_reveal_t = 0.0;
        self.start_motion(cx);
        self.ui.redraw(cx);
    }

    fn switch_settings_tab(&mut self, cx: &mut Cx, tab: SettingsTab) {
        self.active_settings_tab = tab;
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
            (
                SettingsTab::Integrations,
                id!(tab_integrations),
                id!(st_integrations_btn),
            ),
            (SettingsTab::Storage, id!(tab_storage), id!(st_storage_btn)),
            (SettingsTab::Connect, id!(tab_connect), id!(st_connect_btn)),
        ];
        for (t, vid, bid) in defs {
            let active = t == tab;
            self.ui.view(vid).set_visible(cx, active);
            self.style_chip_btn(cx, bid, active);
        }
        if tab == SettingsTab::Storage {
            self.refresh_storage_stats(cx);
        } else if tab == SettingsTab::Integrations {
            self.refresh_discord_preview(cx);
        }
        self.page_reveal_t = 0.0;
        self.start_motion(cx);
        self.ui.redraw(cx);
    }

    fn set_right_sidebar(&mut self, cx: &mut Cx, open: bool, lyrics_tab: bool) {
        let z = self.theme.ui_zoom.clamp(0.65, 1.40) as f64;
        self.right_sidebar_open = open;
        self.right_lyrics_tab = lyrics_tab;
        self.right_w_target = if open { 260.0 * z } else { 0.0 };
        if open {
            self.ui.view(id!(right_sidebar)).set_visible(cx, true);
            self.ui.view(id!(right_divider)).set_visible(cx, true);
        }
        self.ui
            .view(id!(rs_queue_view))
            .set_visible(cx, !lyrics_tab);
        self.ui
            .view(id!(rs_lyrics_view))
            .set_visible(cx, lyrics_tab);

        self.style_chip_btn(cx, id!(rs_tab_queue), !lyrics_tab);
        self.style_chip_btn(cx, id!(rs_tab_lyrics), lyrics_tab);

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
        self.start_motion(cx);
        self.sync_video_bounds(cx);
        let pos_ms = self.audio.as_ref().map_or(0, |a| a.telemetry().position_ms);
        self.update_lyrics_display(cx, pos_ms);
        self.ui.redraw(cx);
    }

    fn set_fullscreen(&mut self, cx: &mut Cx, open: bool) {
        if !open && self.video_open {
            self.close_native_video_stage(cx);
        }
        self.fullscreen_open = open;
        self.last_pos_txt.clear();
        self.last_dur_txt.clear();
        self.ui.view(id!(shell_col)).set_visible(cx, !open);
        self.ui.view(id!(fullscreen_overlay)).set_visible(cx, open);
        let accent = self.current_accent();
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        let (is_playing, pos_ms) = self.audio.as_ref().map_or((false, 0), |a| {
            let t = a.telemetry();
            (t.is_playing, t.position_ms)
        });
        self.ui.ferric_ambient_stage(id!(fs_backdrop)).set_state(
            cx,
            open,
            open && is_playing,
            accent,
            acr,
        );
        if open {
            self.sync_fullscreen_backdrop_artwork(cx);
            self.fs_reveal_t = 0.0;
            self.ui
                .view(id!(fs_art_wrap))
                .set_visible(cx, !self.video_open);
            self.ui
                .view(id!(fs_video_wrap))
                .set_visible(cx, self.video_open);
            self.style_chip_btn(cx, id!(fs_mode_art_btn), !self.video_open);
            self.style_chip_btn(cx, id!(fs_mode_video_btn), self.video_open);
            self.start_motion(cx);
        } else {
            self.ui
                .ferric_lyrics_view(id!(fs_lyrics_scroll))
                .set_playback_state(cx, pos_ms, false);
            if let Some(db) = &self.db {
                db.shrink_memory();
            }
            win32_trim_working_set();
        }
        self.update_lyrics_display(cx, pos_ms);
        self.sync_video_bounds(cx);
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

    /// Request cover art strictly lazily for currently visible items (never bulk-loading hundreds
    /// of covers into RAM) so working set stays strictly under 100 MB.
    fn request_artwork_for_tracks(&mut self, tracks: &[Track]) {
        if let Some(cache) = &mut self.artwork {
            for t in tracks.iter().take(14) {
                let _ = cache.request_for_track(t);
            }
        }
    }

    fn request_artwork_for_cards(&mut self, cards: &[YtmShelfCard]) {
        if let Some(cache) = &mut self.artwork {
            for c in cards.iter().take(6) {
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
        if self.active_page == MainPage::Home {
            self.refresh_home_ui(cx);
        } else if self.active_page == MainPage::Albums {
            self.refresh_albums_grid(cx);
        } else if self.active_page == MainPage::Artists {
            self.refresh_artists_grid(cx);
        } else if self.active_page == MainPage::Playlists {
            self.refresh_playlists_grid(cx);
        } else if self.active_page == MainPage::Search {
            self.refresh_search_ui(cx);
        } else if self.active_page == MainPage::Detail {
            self.refresh_detail_hero(cx);
        }
        self.refresh_right_sidebar(cx);
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
        self.sync_fullscreen_backdrop_artwork(cx);
        self.ui.redraw(cx);
    }

    fn sync_fullscreen_backdrop_artwork(&self, cx: &mut Cx) {
        let tex = self.queue.current().and_then(|cur| {
            let key = ArtworkCache::key_for_track(cur)?;
            self.artwork_snapshot
                .get(&key)
                .map(|cached| cached.texture.clone())
        });
        self.ui
            .ferric_ambient_stage(id!(fs_backdrop))
            .set_texture(cx, tex);
    }

    fn apply_adaptive_accent_live(&self, cx: &mut Cx) {
        let accent = self.current_accent();
        let brand_accent = rgba_vec4(&self.theme.accent_hex);
        let acrylic_tint = rgba_vec4(&self.theme.acrylic_tint_hex);
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        let op = self.theme.acrylic_opacity.clamp(0.18, 0.94);
        let z = self.theme.ui_zoom.clamp(0.65, 1.40);
        let is_playing = self
            .audio
            .as_ref()
            .is_some_and(|a| a.telemetry().is_playing);
        self.ui.view(id!(body)).apply_over(
            cx,
            live! {
                draw_bg: {
                    accent_col: (accent),
                    acrylic_on: (acr),
                    acrylic_opacity: (op),
                    acrylic_tint: (acrylic_tint)
                }
            },
        );
        self.ui.view(id!(bottom_bar)).apply_over(
            cx,
            live! {
                draw_bg: {
                    accent_col: (accent),
                    acrylic_on: (acr),
                    acrylic_opacity: (op),
                    acrylic_tint: (acrylic_tint),
                    glow_amt: (if self.queue.current().is_some() { 0.28 } else { 0.14 })
                }
            },
        );
        self.ui.ferric_ambient_stage(id!(fs_backdrop)).set_state(
            cx,
            self.fullscreen_open,
            is_playing,
            accent,
            acr,
        );
        self.ui
            .ferric_bar_slider(id!(vol_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        self.ui
            .ferric_bar_slider(id!(fs_vol_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        for sid in [
            id!(xfade_slider),
            id!(fadein_slider),
            id!(eq_preamp_slider),
            id!(acrylic_opacity_slider),
        ] {
            self.ui
                .ferric_bar_slider(sid)
                .apply_over(cx, live! { draw_bg: { fill_col: (brand_accent) } });
        }
        for (slider_id, _) in eq_band_ids() {
            self.ui
                .ferric_vert_slider(slider_id)
                .apply_over(cx, live! { draw_bg: { fill_col: (brand_accent) } });
        }
        for tid in [
            id!(close_to_tray_toggle),
            id!(radio_toggle),
            id!(gapless_lock_toggle),
            id!(trim_silence_toggle),
            id!(replay_gain_toggle),
            id!(mono_toggle),
            id!(eq_enable_toggle),
            id!(acrylic_toggle),
            id!(adaptive_art_toggle),
            id!(discord_enable_toggle),
            id!(discord_paused_toggle),
            id!(discord_cover_toggle),
            id!(discord_hide_details_toggle),
            id!(auto_update_toggle),
        ] {
            self.ui
                .ferric_toggle(tid)
                .apply_over(cx, live! { draw_bg: { on_col: (brand_accent) } });
        }
        for lid in [
            id!(zoom_val_lbl),
            id!(acrylic_opacity_lbl),
            id!(acrylic_tint_lbl),
            id!(eq_preamp_lbl),
            id!(connect_lan_ip_lbl),
            id!(xfade_val_lbl),
            id!(fadein_val_lbl),
            id!(yt_state_lbl),
            id!(discord_status_lbl),
            id!(detail_kind_lbl),
            id!(update_popup_sub),
        ] {
            self.ui
                .label(lid)
                .apply_over(cx, live! { draw_text: { color: (brand_accent) } });
        }
        self.ui.view(id!(q_now_row)).apply_over(
            cx,
            live! { draw_bg: { accent_col: (accent), acrylic_on: (acr) } },
        );
        self.ui
            .label(id!(q_now_header))
            .apply_over(cx, live! { draw_text: { color: (accent) } });
        self.ui.view(id!(np_canvas)).apply_over(
            cx,
            live! { draw_bg: { accent: (accent), acrylic_on: (acr) } },
        );
        self.ui
            .ferric_eq_graph(id!(eq_graph))
            .apply_over(cx, live! { draw_bg: { accent: (brand_accent) } });
        self.ui
            .ferric_lyrics_view(id!(rs_lyrics_scroll))
            .set_style(cx, accent, z);
        self.ui
            .ferric_lyrics_view(id!(fs_lyrics_scroll))
            .set_style(cx, accent, z);
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

        let q_filter = self.songs_filter_query.trim().to_ascii_lowercase();
        if !q_filter.is_empty() {
            list.retain(|t| {
                t.title.to_ascii_lowercase().contains(&q_filter)
                    || t.artist.to_ascii_lowercase().contains(&q_filter)
                    || t.album.to_ascii_lowercase().contains(&q_filter)
            });
        }

        let has_demo = self.all_tracks.iter().any(|t| t.id.starts_with("demo:"));
        self.ui
            .button(id!(songs_clear_demo_btn))
            .set_visible(cx, has_demo);

        match self.sort_field {
            SortField::Recent => {
                let recents = &self.session_state.recent_track_ids;
                list.sort_by(|a, b| {
                    let ra = recents
                        .iter()
                        .position(|id| id == &a.id)
                        .unwrap_or(usize::MAX);
                    let rb = recents
                        .iter()
                        .position(|id| id == &b.id)
                        .unwrap_or(usize::MAX);
                    ra.cmp(&rb)
                        .then_with(|| b.liked.cmp(&a.liked))
                        .then_with(|| {
                            (b.source == TrackSource::Local).cmp(&(a.source == TrackSource::Local))
                        })
                        .then_with(|| {
                            a.title
                                .to_ascii_lowercase()
                                .cmp(&b.title.to_ascii_lowercase())
                        })
                });
            }
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
            self.style_chip_btn(cx, btn, active);
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

        // Album & Artist groups: user's saved library first, enriched with clean live Home catalog
        // so Albums & Artists pages always show a lush multi-row visual gallery without bloating Songs.
        let mut gallery_tracks: Vec<Track> = Vec::with_capacity(
            self.all_tracks.len()
                + self.rich_home.quick_picks.len()
                + self.rich_home.all_tracks.len(),
        );
        let mut seen_gallery = std::collections::HashSet::new();
        for t in self
            .all_tracks
            .iter()
            .chain(self.rich_home.quick_picks.iter())
            .chain(self.rich_home.all_tracks.iter())
        {
            let low_t = t.title.to_ascii_lowercase();
            let low_a = t.artist.to_ascii_lowercase();
            if low_t.contains("minecraft")
                || low_a.contains("minecraft")
                || low_a.contains("watching")
            {
                continue;
            }
            if seen_gallery.insert(t.id.clone()) {
                gallery_tracks.push(t.clone());
            }
        }
        self.album_groups = build_album_groups(&gallery_tracks);
        self.artist_groups = build_artist_groups(&gallery_tracks);
        let max_alb_page = self.album_groups.len().saturating_sub(1) / 20;
        if self.albums_page > max_alb_page {
            self.albums_page = max_alb_page;
        }
        let max_art_page = self.artist_groups.len().saturating_sub(1) / 20;
        if self.artists_page > max_art_page {
            self.artists_page = max_art_page;
        }
        let max_pl_page = self.playlists.len().saturating_sub(1) / 20;
        if self.playlists_page > max_pl_page {
            self.playlists_page = max_pl_page;
        }

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
                self.ui
                    .button(slot)
                    .set_text(cx, &clip_text(&clean_display_text(&pl.name), 18));
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
        let raw = if !self.rich_home.quick_picks.is_empty() {
            &self.rich_home.quick_picks
        } else {
            &self.all_tracks
        };
        raw.iter()
            .filter(|t| {
                let alb = t.album.to_ascii_lowercase();
                let art = t.artist.to_ascii_lowercase();
                !alb.contains("watching") && !art.contains("watching")
            })
            .cloned()
            .collect()
    }

    fn refresh_home_ui(&mut self, cx: &mut Cx) {
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
                let clean_lbl = clean_display_text(&txt);
                self.ui.button(cid).set_visible(cx, true);
                self.ui.button(cid).set_text(cx, &clean_lbl);
                self.style_chip_btn(cx, cid, self.active_mood_idx == i);
            } else if i > 0 && !self.rich_home.chips.is_empty() {
                self.ui.button(cid).set_visible(cx, false);
            }
        }

        // Quick Picks 2x4 grid + 8 Horizontal Dynamic Shelves (request artwork lazily for visible slots)
        let pool = self.home_quick_picks_pool();
        let start = self.qp_page * 8;
        if let Some(cache) = &mut self.artwork {
            for t in pool.iter().skip(start).take(8) {
                let _ = cache.request_for_track(t);
            }
            for (si, shelf) in self.rich_home.shelves.iter().take(8).enumerate() {
                let off = self.shelf_pages[si] * 5;
                for card in shelf.cards.iter().skip(off).take(5) {
                    if let Some(url) = card.thumbnail_url.as_deref() {
                        let _ = cache.request_url(url);
                    }
                }
            }
        }

        let active_id = self.queue.current().map(|t| t.id.as_str());
        let is_playing = self
            .audio
            .as_ref()
            .is_some_and(|a| a.telemetry().is_playing);
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        let accent = self.current_accent();
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
                    live! {
                        draw_bg: {
                            active: (if is_act { 1.0 } else { 0.0 }),
                            acrylic_on: (acr),
                            accent_col: (accent)
                        }
                    },
                );
                v.label(id!(title))
                    .set_text(cx, &clip_text(&clean_display_text(&track.title), 24));
                v.label(id!(sub)).set_text(
                    cx,
                    &clip_text(
                        &clean_display_text(&format!("{} · {}", track.artist, track.album)),
                        28,
                    ),
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

        // 8 Dynamic Horizontal Real-Cover Shelves (Trending, New Releases, Charts, Mixes, Radio, Continuations)
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
            (
                3usize,
                id!(shelf3_wrap),
                id!(sh3_title_lbl),
                [
                    id!(sh3_c0),
                    id!(sh3_c1),
                    id!(sh3_c2),
                    id!(sh3_c3),
                    id!(sh3_c4),
                ],
            ),
            (
                4usize,
                id!(shelf4_wrap),
                id!(sh4_title_lbl),
                [
                    id!(sh4_c0),
                    id!(sh4_c1),
                    id!(sh4_c2),
                    id!(sh4_c3),
                    id!(sh4_c4),
                ],
            ),
            (
                5usize,
                id!(shelf5_wrap),
                id!(sh5_title_lbl),
                [
                    id!(sh5_c0),
                    id!(sh5_c1),
                    id!(sh5_c2),
                    id!(sh5_c3),
                    id!(sh5_c4),
                ],
            ),
            (
                6usize,
                id!(shelf6_wrap),
                id!(sh6_title_lbl),
                [
                    id!(sh6_c0),
                    id!(sh6_c1),
                    id!(sh6_c2),
                    id!(sh6_c3),
                    id!(sh6_c4),
                ],
            ),
            (
                7usize,
                id!(shelf7_wrap),
                id!(sh7_title_lbl),
                [
                    id!(sh7_c0),
                    id!(sh7_c1),
                    id!(sh7_c2),
                    id!(sh7_c3),
                    id!(sh7_c4),
                ],
            ),
        ];

        for (si, wrap_id, title_id, card_ids) in shelf_defs {
            if let Some(shelf) = self.rich_home.shelves.get(si)
                && !shelf.cards.is_empty()
            {
                self.ui.view(wrap_id).set_visible(cx, true);
                self.ui
                    .label(title_id)
                    .set_text(cx, &clip_text(&clean_display_text(&shelf.title), 48));
                let offset = self.shelf_pages[si] * 5;
                for (ci, cid) in card_ids.into_iter().enumerate() {
                    let card_v = self.ui.view(cid);
                    card_v.set_visible(cx, true);
                    if let Some(card) = shelf.cards.get(offset + ci) {
                        card_v.view(id!(inner)).set_visible(cx, true);
                        card_v.apply_over(
                            cx,
                            live! {
                                cursor: Hand,
                                draw_bg: {
                                    ghost: 0.0,
                                    acrylic_on: (acr),
                                    accent_col: (accent)
                                }
                            },
                        );
                        card_v
                            .label(id!(title))
                            .set_text(cx, &clip_text(&clean_display_text(&card.title), 17));
                        card_v
                            .label(id!(sub))
                            .set_text(cx, &clip_text(&clean_display_text(&card.subtitle), 20));
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
                        // Keep empty slot width so shelves with < 5 cards never stretch horizontally
                        card_v.view(id!(inner)).set_visible(cx, false);
                        card_v.apply_over(
                            cx,
                            live! {
                                cursor: Default,
                                draw_bg: { ghost: 1.0 }
                            },
                        );
                    }
                }
            } else {
                self.ui.view(wrap_id).set_visible(cx, false);
            }
        }

        self.ui.button(id!(home_load_more_btn)).set_text(
            cx,
            if self.home_loading_more {
                "Loading more..."
            } else {
                "Load More Recommendations"
            },
        );
    }

    fn refresh_right_sidebar(&mut self, cx: &mut Cx) {
        let is_playing = self
            .audio
            .as_ref()
            .is_some_and(|a| a.telemetry().is_playing);
        let accent = self.current_accent();
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };

        // History (last 3 played tracks excluding current)
        let cur_id = self.queue.current().map(|t| t.id.clone());
        let hist: Vec<Track> = self
            .session_state
            .recent_track_ids
            .iter()
            .filter(|id| cur_id.as_deref() != Some(id.as_str()))
            .filter_map(|id| self.all_tracks.iter().find(|t| &t.id == id).cloned())
            .take(3)
            .collect();
        let upcoming: Vec<Track> = self.queue.peek_upcoming(10).into_iter().cloned().collect();
        if let Some(cache) = &mut self.artwork {
            for t in &hist {
                let _ = cache.request_for_track(t);
            }
            for t in upcoming.iter().take(10) {
                let _ = cache.request_for_track(t);
            }
        }

        self.ui
            .view(id!(qh_header))
            .set_visible(cx, !hist.is_empty());
        for (i, rid) in [id!(qh_0), id!(qh_1), id!(qh_2)].into_iter().enumerate() {
            let rv = self.ui.view(rid);
            if let Some(t) = hist.get(i) {
                rv.set_visible(cx, true);
                rv.apply_over(
                    cx,
                    live! {
                        draw_bg: {
                            acrylic_on: (acr),
                            accent_col: (accent)
                        }
                    },
                );
                rv.label(id!(title))
                    .set_text(cx, &clip_text(&clean_display_text(&t.title), 16));
                rv.label(id!(sub))
                    .set_text(cx, &clip_text(&clean_display_text(&t.artist), 18));
                rv.label(id!(dur)).set_text(
                    cx,
                    &if t.duration_ms > 0 {
                        format_duration_ms(t.duration_ms)
                    } else {
                        String::new()
                    },
                );
                rv.button(id!(up_btn)).set_visible(cx, false);
                rv.button(id!(rm_btn)).set_visible(cx, false);
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
            &format!("Now playing · {}", clip_text(&self.queue_context, 18)),
        );
        let now_v = self.ui.view(id!(q_now_row));
        now_v.apply_over(
            cx,
            live! {
                draw_bg: {
                    active: 1.0,
                    acrylic_on: (acr),
                    accent_col: (accent)
                }
            },
        );
        if let Some(cur) = self.queue.current().cloned() {
            now_v.set_visible(cx, true);
            now_v
                .label(id!(title))
                .set_text(cx, &clip_text(&clean_display_text(&cur.title), 16));
            now_v
                .label(id!(title))
                .apply_over(cx, live! { draw_text: { color: (accent) } });
            now_v
                .label(id!(sub))
                .set_text(cx, &clip_text(&clean_display_text(&cur.artist), 18));
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
                Some(&cur),
                is_playing,
                0.6,
                false,
            );
        } else {
            now_v.set_visible(cx, true);
            now_v.label(id!(title)).set_text(cx, "Nothing playing");
            now_v
                .label(id!(title))
                .apply_over(cx, live! { draw_text: { color: #ECEEF5 } });
            now_v.label(id!(sub)).set_text(cx, "Pick any song");
            now_v.label(id!(dur)).set_text(cx, "");
            now_v.ferric_cover_art(id!(art)).set_texture(cx, None, 1.0);
        }

        // Up next (10 rows)
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
                draw_bg: { active: (if self.session_state.autoplay_radio { 1.0 } else { 0.0 }), acrylic_on: (acr) }
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
                rv.apply_over(
                    cx,
                    live! {
                        draw_bg: {
                            acrylic_on: (acr),
                            accent_col: (accent)
                        }
                    },
                );
                rv.label(id!(title))
                    .set_text(cx, &clip_text(&clean_display_text(&t.title), 14));
                rv.label(id!(sub))
                    .set_text(cx, &clip_text(&clean_display_text(&t.artist), 16));
                rv.label(id!(dur)).set_text(
                    cx,
                    &if t.duration_ms > 0 {
                        format_duration_ms(t.duration_ms)
                    } else {
                        String::new()
                    },
                );
                rv.button(id!(up_btn)).set_visible(cx, i > 0);
                rv.button(id!(rm_btn)).set_visible(cx, true);
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

    fn refresh_albums_grid(&mut self, cx: &mut Cx) {
        const PER_PAGE: usize = 20;
        let total = self.album_groups.len();
        let total_pages = total.div_ceil(PER_PAGE).max(1);
        let cur_page = (self.albums_page + 1).min(total_pages);
        let sub_txt = if total_pages > 1 {
            format!("{total} albums · Page {cur_page} / {total_pages}")
        } else {
            format!("{total} albums in your catalog")
        };
        self.ui.label(id!(albums_sub_lbl)).set_text(cx, &sub_txt);
        let start = self.albums_page * PER_PAGE;
        if let Some(cache) = &mut self.artwork {
            for g in self.album_groups.iter().skip(start).take(PER_PAGE) {
                if let Some(url) = g.thumbnail_url.as_deref() {
                    let _ = cache.request_url(url);
                }
            }
        }
        let rows = [
            id!(alb_row_0),
            id!(alb_row_1),
            id!(alb_row_2),
            id!(alb_row_3),
        ];
        let ids = [
            id!(alb_0),
            id!(alb_1),
            id!(alb_2),
            id!(alb_3),
            id!(alb_4),
            id!(alb_5),
            id!(alb_6),
            id!(alb_7),
            id!(alb_8),
            id!(alb_9),
            id!(alb_10),
            id!(alb_11),
            id!(alb_12),
            id!(alb_13),
            id!(alb_14),
            id!(alb_15),
            id!(alb_16),
            id!(alb_17),
            id!(alb_18),
            id!(alb_19),
        ];
        for (r_idx, rid) in rows.into_iter().enumerate() {
            let row_has_any = start + r_idx * 5 < total;
            self.ui.view(rid).set_visible(cx, row_has_any);
        }
        for (i, vid) in ids.into_iter().enumerate() {
            let v = self.ui.view(vid);
            if let Some(g) = self.album_groups.get(start + i) {
                v.set_visible(cx, true);
                v.apply_over(cx, live! { draw_bg: { ghost: 0.0 } });
                v.view(id!(inner)).set_visible(cx, true);
                v.label(id!(title)).set_text(cx, &clip_text(&g.title, 20));
                v.label(id!(sub)).set_text(cx, &clip_text(&g.subtitle, 23));
                let best_track = g
                    .tracks
                    .iter()
                    .find(|t| t.thumbnail_url.is_some() || t.youtube_video_id.is_some())
                    .or_else(|| g.tracks.first());
                self.bind_cover_art_url(
                    cx,
                    v.ferric_cover_art(id!(art)),
                    g.thumbnail_url.as_deref(),
                    best_track,
                    false,
                    0.4,
                    false,
                );
            } else {
                v.set_visible(cx, true);
                v.apply_over(cx, live! { draw_bg: { ghost: 1.0 } });
                v.view(id!(inner)).set_visible(cx, false);
            }
        }
    }

    fn refresh_artists_grid(&mut self, cx: &mut Cx) {
        const PER_PAGE: usize = 20;
        let total = self.artist_groups.len();
        let total_pages = total.div_ceil(PER_PAGE).max(1);
        let cur_page = (self.artists_page + 1).min(total_pages);
        let sub_txt = if total_pages > 1 {
            format!("{total} artists · Page {cur_page} / {total_pages}")
        } else {
            format!("{total} artists in your catalog")
        };
        self.ui.label(id!(artists_sub_lbl)).set_text(cx, &sub_txt);
        let start = self.artists_page * PER_PAGE;
        if let Some(cache) = &mut self.artwork {
            for g in self.artist_groups.iter().skip(start).take(PER_PAGE) {
                if let Some(url) = g.thumbnail_url.as_deref() {
                    let _ = cache.request_url(url);
                }
            }
        }
        let rows = [
            id!(art_row_0),
            id!(art_row_1),
            id!(art_row_2),
            id!(art_row_3),
        ];
        let ids = [
            id!(art_0),
            id!(art_1),
            id!(art_2),
            id!(art_3),
            id!(art_4),
            id!(art_5),
            id!(art_6),
            id!(art_7),
            id!(art_8),
            id!(art_9),
            id!(art_10),
            id!(art_11),
            id!(art_12),
            id!(art_13),
            id!(art_14),
            id!(art_15),
            id!(art_16),
            id!(art_17),
            id!(art_18),
            id!(art_19),
        ];
        for (r_idx, rid) in rows.into_iter().enumerate() {
            let row_has_any = start + r_idx * 5 < total;
            self.ui.view(rid).set_visible(cx, row_has_any);
        }
        for (i, vid) in ids.into_iter().enumerate() {
            let v = self.ui.view(vid);
            if let Some(g) = self.artist_groups.get(start + i) {
                v.set_visible(cx, true);
                v.apply_over(cx, live! { draw_bg: { ghost: 0.0 } });
                v.view(id!(inner)).set_visible(cx, true);
                v.label(id!(title)).set_text(cx, &clip_text(&g.title, 20));
                v.label(id!(sub)).set_text(cx, &clip_text(&g.subtitle, 23));
                let best_track = g
                    .tracks
                    .iter()
                    .find(|t| t.thumbnail_url.is_some() || t.youtube_video_id.is_some())
                    .or_else(|| g.tracks.first());
                self.bind_cover_art_url(
                    cx,
                    v.ferric_cover_art(id!(art)),
                    g.thumbnail_url.as_deref(),
                    best_track,
                    false,
                    0.4,
                    true,
                );
            } else {
                v.set_visible(cx, true);
                v.apply_over(cx, live! { draw_bg: { ghost: 1.0 } });
                v.view(id!(inner)).set_visible(cx, false);
            }
        }
    }

    fn refresh_playlists_grid(&mut self, cx: &mut Cx) {
        const PER_PAGE: usize = 20;
        let total = self.playlists.len();
        let start = self.playlists_page * PER_PAGE;
        if let Some(cache) = &mut self.artwork {
            for pl in self.playlists.iter().skip(start).take(PER_PAGE) {
                if let Some(url) = pl.thumbnail_url.as_deref() {
                    let _ = cache.request_url(url);
                }
            }
        }
        let rows = [
            id!(plc_row_0),
            id!(plc_row_1),
            id!(plc_row_2),
            id!(plc_row_3),
        ];
        let ids = [
            id!(plc_0),
            id!(plc_1),
            id!(plc_2),
            id!(plc_3),
            id!(plc_4),
            id!(plc_5),
            id!(plc_6),
            id!(plc_7),
            id!(plc_8),
            id!(plc_9),
            id!(plc_10),
            id!(plc_11),
            id!(plc_12),
            id!(plc_13),
            id!(plc_14),
            id!(plc_15),
            id!(plc_16),
            id!(plc_17),
            id!(plc_18),
            id!(plc_19),
        ];
        for (r_idx, rid) in rows.into_iter().enumerate() {
            let row_has_any = start + r_idx * 5 < total;
            self.ui.view(rid).set_visible(cx, row_has_any);
        }
        for (i, vid) in ids.into_iter().enumerate() {
            let v = self.ui.view(vid);
            if let Some(pl) = self.playlists.get(start + i) {
                v.set_visible(cx, true);
                v.apply_over(cx, live! { draw_bg: { ghost: 0.0 } });
                v.view(id!(inner)).set_visible(cx, true);
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
                v.set_visible(cx, true);
                v.apply_over(cx, live! { draw_bg: { ghost: 1.0 } });
                v.view(id!(inner)).set_visible(cx, false);
            }
        }
    }

    fn refresh_search_ui(&mut self, cx: &mut Cx) {
        for (btn, f) in [
            (id!(sf_all_btn), SearchFilter::All),
            (id!(sf_songs_btn), SearchFilter::Songs),
            (id!(sf_albums_btn), SearchFilter::Albums),
            (id!(sf_artists_btn), SearchFilter::Artists),
            (id!(sf_playlists_btn), SearchFilter::Playlists),
            (id!(sf_local_btn), SearchFilter::Local),
        ] {
            self.style_chip_btn(cx, btn, self.search_filter == f);
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
                self.style_chip_btn(cx, bid, false);
            } else {
                self.ui.button(bid).set_visible(cx, false);
            }
        }

        // Filtered search cards (Albums / Artists / Playlists) — priority artwork fetch
        let filtered_cards: Vec<YtmShelfCard> = self
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
            .cloned()
            .collect();
        self.request_artwork_for_cards(&filtered_cards);
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        let accent = self.current_accent();
        self.ui
            .view(id!(search_cards_wrap))
            .set_visible(cx, !filtered_cards.is_empty());
        for (i, cid) in [id!(sc_0), id!(sc_1), id!(sc_2), id!(sc_3), id!(sc_4)]
            .into_iter()
            .enumerate()
        {
            let cv = self.ui.view(cid);
            cv.set_visible(cx, true);
            if let Some(card) = filtered_cards.get(i) {
                cv.view(id!(inner)).set_visible(cx, true);
                cv.apply_over(
                    cx,
                    live! { cursor: Hand, draw_bg: { ghost: 0.0, acrylic_on: (acr), accent_col: (accent) } },
                );
                cv.label(id!(title))
                    .set_text(cx, &clip_text(&clean_display_text(&card.title), 18));
                cv.label(id!(sub))
                    .set_text(cx, &clip_text(&clean_display_text(&card.subtitle), 22));
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
                cv.view(id!(inner)).set_visible(cx, false);
                cv.apply_over(cx, live! { cursor: Default, draw_bg: { ghost: 1.0 } });
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
        let is_saved_pl = self.detail_state.kind == YtmCardKind::Playlist
            && self.playlists.iter().any(|p| p.id == self.detail_state.id);
        self.ui
            .button(id!(detail_delete_btn))
            .set_visible(cx, is_saved_pl);
        self.ui
            .view(id!(detail_rename_row))
            .set_visible(cx, is_saved_pl);
        if is_saved_pl {
            self.ui
                .text_input(id!(detail_rename_input))
                .set_text(cx, &self.detail_state.title);
        }
        self.ui
            .button(id!(detail_export_btn))
            .set_visible(cx, !self.detail_state.tracks.is_empty());
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
            && (card.kind == YtmCardKind::Song || card.browse_id.is_none())
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
            if !card.title.trim().is_empty() {
                self.spawn_rich_search(cx, card.title);
            }
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

        // Streaming Audio Quality & Default Video Clip Quality chips
        for (btn, q) in [
            (id!(aq_low_btn), AudioQuality::Low),
            (id!(aq_norm_btn), AudioQuality::Normal),
            (id!(aq_high_btn), AudioQuality::High),
            (id!(aq_best_btn), AudioQuality::Best),
        ] {
            self.style_chip_btn(cx, btn, self.dsp.audio_quality == q);
        }
        self.refresh_video_quality_buttons(cx);

        self.ui
            .ferric_toggle(id!(close_to_tray_toggle))
            .set_active(cx, self.dsp.close_to_tray);
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
            let active = self.eq.preset_name.eq_ignore_ascii_case(preset);
            self.ui.button(btn).apply_over(
                cx,
                live! { draw_bg: { active: (if active { 1.0 } else { 0.0 }) } },
            );
        }

        // Theme buttons active indicator
        for (btn, active) in [
            (
                id!(pal_amoled_btn),
                self.theme.name == "AMOLED Black" && !self.theme.acrylic_enabled,
            ),
            (id!(pal_acrylic_btn), self.theme.acrylic_enabled),
            (
                id!(pal_obsidian_btn),
                self.theme.name == "Obsidian Studio" && !self.theme.acrylic_enabled,
            ),
            (
                id!(pal_cream_btn),
                self.theme.name == "Warm Cream Light" && !self.theme.acrylic_enabled,
            ),
        ] {
            self.ui.button(btn).apply_over(
                cx,
                live! { draw_bg: { active: (if active { 1.0 } else { 0.0 }) } },
            );
        }
        self.ui
            .label(id!(zoom_val_lbl))
            .set_text(cx, &format!("{:.0}%", self.theme.ui_zoom * 100.0));

        self.ui
            .ferric_toggle(id!(adaptive_art_toggle))
            .set_active(cx, self.theme.adaptive_album_art);
        self.ui
            .ferric_toggle(id!(acrylic_toggle))
            .set_active(cx, self.theme.acrylic_enabled);
        self.ui
            .ferric_bar_slider(id!(acrylic_opacity_slider))
            .set_value(cx, f64::from(self.theme.acrylic_opacity));
        self.ui
            .label(id!(acrylic_opacity_lbl))
            .set_text(cx, &format!("{:.0}%", self.theme.acrylic_opacity * 100.0));
        let tint_hex = self.theme.acrylic_tint_hex.to_ascii_uppercase();
        let tint_v4 = rgba_vec4(&tint_hex);
        self.ui.label(id!(acrylic_tint_lbl)).set_text(cx, &tint_hex);
        self.ui
            .view(id!(acrylic_tint_preview))
            .apply_over(cx, live! { draw_bg: { tint_col: (tint_v4) } });
        self.ui
            .text_input(id!(acrylic_tint_input))
            .set_text(cx, &tint_hex);
        for (btn, hex) in [
            (id!(tint_obsidian_btn), "#0B0D14"),
            (id!(tint_ember_btn), "#2B140A"),
            (id!(tint_copper_btn), "#2E1A10"),
            (id!(tint_midnight_btn), "#0C182E"),
            (id!(tint_emerald_btn), "#0B2118"),
            (id!(tint_crimson_btn), "#290E16"),
            (id!(tint_slate_btn), "#1D222E"),
            (id!(tint_smoke_btn), "#181818"),
        ] {
            self.style_chip_btn(cx, btn, tint_hex.eq_ignore_ascii_case(hex));
        }

        // Discord Rich Presence controls
        self.ui
            .ferric_toggle(id!(discord_enable_toggle))
            .set_active(cx, self.discord_settings.enabled);
        self.ui
            .button(id!(discord_format_btn))
            .set_text(cx, self.discord_settings.status_format.label());
        self.ui
            .ferric_toggle(id!(discord_paused_toggle))
            .set_active(cx, self.discord_settings.show_when_paused);
        self.ui
            .ferric_toggle(id!(discord_cover_toggle))
            .set_active(cx, self.discord_settings.show_cover_art);
        self.ui
            .ferric_toggle(id!(discord_hide_details_toggle))
            .set_active(cx, self.discord_settings.hide_details);
        self.ui
            .text_input(id!(discord_client_id_input))
            .set_text(cx, &self.discord_settings.client_id);
        self.refresh_discord_preview(cx);

        self.ui
            .ferric_bar_slider(id!(vol_slider))
            .set_value(cx, f64::from(self.session_state.volume));
        self.ui
            .ferric_bar_slider(id!(fs_vol_slider))
            .set_value(cx, f64::from(self.session_state.volume));
        let mute_col = if self.session_state.volume < 0.01 {
            accent
        } else {
            muted
        };
        for mbtn in [id!(vol_mute_btn), id!(fs_vol_mute_btn)] {
            self.ui
                .button(mbtn)
                .apply_over(cx, live! { draw_icon: { color: (mute_col) } });
        }
        for sbtn in [id!(shuffle_btn), id!(fs_shuffle_btn)] {
            self.ui.button(sbtn).apply_over(
                cx,
                live! {
                    draw_bg: { active: (if self.queue.shuffle { 1.0 } else { 0.0 }) }
                    draw_icon: { color: (if self.queue.shuffle { accent } else { muted }) }
                },
            );
        }
        let rep_active = self.queue.repeat != RepeatMode::Off;
        let rep_col = match self.queue.repeat {
            RepeatMode::Off => muted,
            RepeatMode::All => accent,
            RepeatMode::One => vec4(0.95, 0.78, 0.36, 1.0),
        };
        for rbtn in [id!(repeat_btn), id!(fs_repeat_btn)] {
            self.ui.button(rbtn).apply_over(
                cx,
                live! {
                    draw_bg: { active: (if rep_active { 1.0 } else { 0.0 }) }
                    draw_icon: { color: (rep_col) }
                },
            );
        }

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
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        win32_set_native_acrylic(
            self.theme.acrylic_enabled,
            self.theme.acrylic_opacity,
            self.theme.acrylic_tint_rgb(),
        );

        let canvas = rgba_vec4(&self.theme.canvas_hex);
        let surface = rgba_vec4(&self.theme.surface_hex);
        let border = rgba_vec4(&self.theme.border_hex);
        let accent = self.current_accent();
        let acrylic_tint = rgba_vec4(&self.theme.acrylic_tint_hex);

        let op = self.theme.acrylic_opacity.clamp(0.18, 0.94);
        self.ui.view(id!(body)).apply_over(
            cx,
            live! {
                draw_bg: {
                    canvas_col: (canvas),
                    acrylic_on: (acr),
                    acrylic_opacity: (op),
                    acrylic_tint: (acrylic_tint),
                    accent_col: (accent)
                }
            },
        );

        for (pid, flat_col, is_canv) in [
            (id!(top_bar), surface, 0.0_f32),
            (id!(left_sidebar), surface, 0.0_f32),
            (id!(center_stage), canvas, 1.0_f32),
            (id!(right_sidebar), surface, 0.0_f32),
        ] {
            self.ui.view(pid).apply_over(
                cx,
                live! {
                    draw_bg: {
                        base_col: (flat_col),
                        border_col: (border),
                        acrylic_on: (acr),
                        acrylic_opacity: (op),
                        acrylic_tint: (acrylic_tint),
                        is_canvas: (is_canv)
                    }
                },
            );
        }

        // Propagate clean Native Acrylic / AMOLED surface states to all cards, shelves, grids & queue rows
        for card_id in [
            id!(card_acct_yt),
            id!(card_acct_import),
            id!(card_acct_sub),
            id!(card_pb_xfade),
            id!(card_pb_quality),
            id!(card_pb_behavior),
            id!(card_pb_speed),
            id!(card_eq_main),
            id!(card_app_theme),
            id!(card_discord_rpc),
            id!(card_st_cache),
            id!(card_st_update),
            id!(card_st_keys),
            id!(card_conn_main),
            id!(card_cmdk),
            id!(card_update_popup),
            id!(qp_0),
            id!(qp_1),
            id!(qp_2),
            id!(qp_3),
            id!(qp_4),
            id!(qp_5),
            id!(qp_6),
            id!(qp_7),
            id!(sh0_c0),
            id!(sh0_c1),
            id!(sh0_c2),
            id!(sh0_c3),
            id!(sh0_c4),
            id!(sh1_c0),
            id!(sh1_c1),
            id!(sh1_c2),
            id!(sh1_c3),
            id!(sh1_c4),
            id!(sh2_c0),
            id!(sh2_c1),
            id!(sh2_c2),
            id!(sh2_c3),
            id!(sh2_c4),
            id!(sh3_c0),
            id!(sh3_c1),
            id!(sh3_c2),
            id!(sh3_c3),
            id!(sh3_c4),
            id!(sh4_c0),
            id!(sh4_c1),
            id!(sh4_c2),
            id!(sh4_c3),
            id!(sh4_c4),
            id!(sh5_c0),
            id!(sh5_c1),
            id!(sh5_c2),
            id!(sh5_c3),
            id!(sh5_c4),
            id!(sh6_c0),
            id!(sh6_c1),
            id!(sh6_c2),
            id!(sh6_c3),
            id!(sh6_c4),
            id!(sh7_c0),
            id!(sh7_c1),
            id!(sh7_c2),
            id!(sh7_c3),
            id!(sh7_c4),
            id!(sc_0),
            id!(sc_1),
            id!(sc_2),
            id!(sc_3),
            id!(sc_4),
            id!(alb_0),
            id!(alb_1),
            id!(alb_2),
            id!(alb_3),
            id!(alb_4),
            id!(alb_5),
            id!(alb_6),
            id!(alb_7),
            id!(alb_8),
            id!(alb_9),
            id!(alb_10),
            id!(alb_11),
            id!(alb_12),
            id!(alb_13),
            id!(alb_14),
            id!(alb_15),
            id!(alb_16),
            id!(alb_17),
            id!(alb_18),
            id!(alb_19),
            id!(art_0),
            id!(art_1),
            id!(art_2),
            id!(art_3),
            id!(art_4),
            id!(art_5),
            id!(art_6),
            id!(art_7),
            id!(art_8),
            id!(art_9),
            id!(art_10),
            id!(art_11),
            id!(art_12),
            id!(art_13),
            id!(art_14),
            id!(art_15),
            id!(art_16),
            id!(art_17),
            id!(art_18),
            id!(art_19),
            id!(plc_0),
            id!(plc_1),
            id!(plc_2),
            id!(plc_3),
            id!(plc_4),
            id!(plc_5),
            id!(plc_6),
            id!(plc_7),
            id!(plc_8),
            id!(plc_9),
            id!(plc_10),
            id!(plc_11),
            id!(plc_12),
            id!(plc_13),
            id!(plc_14),
            id!(plc_15),
            id!(plc_16),
            id!(plc_17),
            id!(plc_18),
            id!(plc_19),
            id!(qh_0),
            id!(qh_1),
            id!(qh_2),
            id!(q_now_row),
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
        ] {
            self.ui.view(card_id).apply_over(
                cx,
                live! {
                    draw_bg: {
                        acrylic_on: (acr),
                        accent_col: (accent)
                    }
                },
            );
        }

        self.ui
            .view(id!(toast_pill))
            .apply_over(cx, live! { draw_bg: { acrylic_on: (acr) } });
        self.ui
            .view(id!(discord_preview_box))
            .apply_over(cx, live! { draw_bg: { acrylic_on: (acr) } });
        self.ui.view(id!(top_search_box)).apply_over(
            cx,
            live! { draw_bg: { acrylic_on: (acr), accent_col: (accent) } },
        );

        let (inp_bg, inp_hov, inp_foc, inp_b, inp_bh) = if self.theme.acrylic_enabled {
            (
                vec4(1.0, 1.0, 1.0, 0.055),
                vec4(1.0, 1.0, 1.0, 0.095),
                vec4(1.0, 1.0, 1.0, 0.115),
                vec4(1.0, 1.0, 1.0, 0.095),
                vec4(1.0, 1.0, 1.0, 0.19),
            )
        } else {
            (
                rgba_vec4("#090A0F"),
                rgba_vec4("#10121A"),
                rgba_vec4("#12141F"),
                rgba_vec4("#232634"),
                rgba_vec4("#3B3F56"),
            )
        };
        for inp_id in [
            id!(songs_filter_input),
            id!(search_page_input),
            id!(pl_new_input),
            id!(detail_rename_input),
            id!(quick_sync_input),
            id!(sub_url_input),
            id!(sub_user_input),
            id!(sub_pass_input),
            id!(autoeq_input),
            id!(acrylic_tint_input),
            id!(discord_client_id_input),
            id!(cmdk_input),
            id!(tm_new_pl_input),
        ] {
            self.ui.text_input(inp_id).apply_over(
                cx,
                live! {
                    draw_bg: {
                        color: (inp_bg),
                        color_empty: (inp_bg),
                        color_hover: (inp_hov),
                        color_focus: (inp_foc),
                        border_color_1: (inp_b),
                        border_color_2: (inp_b),
                        border_color_1_empty: (inp_b),
                        border_color_2_empty: (inp_b),
                        border_color_1_hover: (inp_bh),
                        border_color_2_hover: (inp_bh),
                        border_color_1_focus: (accent),
                        border_color_2_focus: (accent)
                    }
                },
            );
        }

        for btn_id in [
            id!(top_account_btn),
            id!(home_retry_btn),
            id!(home_play_all_btn),
            id!(home_load_more_btn),
            id!(home_shuffle_feed_btn),
            id!(search_clear_btn),
            id!(recents_clear_btn),
            id!(hero_shuffle_btn),
            id!(add_folder_btn),
            id!(songs_clear_demo_btn),
            id!(sort_cycle_btn),
            id!(pl_import_btn),
            id!(detail_shuffle_btn),
            id!(detail_radio_btn),
            id!(detail_pin_btn),
            id!(detail_queue_btn),
            id!(detail_export_btn),
            id!(detail_delete_btn),
            id!(detail_rename_btn),
            id!(yt_sync_btn),
            id!(yt_logout_btn),
            id!(guest_refresh_btn),
            id!(quick_spotify_btn),
            id!(quick_youtube_btn),
            id!(match_local_btn),
            id!(rescan_btn),
            id!(sub_sync_btn),
            id!(xf_0_btn),
            id!(xf_3_btn),
            id!(xf_6_btn),
            id!(xf_10_btn),
            id!(curve_bass_btn),
            id!(curve_eqp_btn),
            id!(curve_velvet_btn),
            id!(spd_075_btn),
            id!(spd_100_btn),
            id!(spd_125_btn),
            id!(spd_150_btn),
            id!(slp_off_btn),
            id!(slp_15_btn),
            id!(slp_30_btn),
            id!(slp_60_btn),
            id!(slp_eot_btn),
            id!(eq_pre_flat),
            id!(eq_pre_bass),
            id!(eq_pre_bassred),
            id!(eq_pre_treble),
            id!(eq_pre_vocal),
            id!(eq_pre_rock),
            id!(eq_pre_pop),
            id!(eq_pre_jazz),
            id!(eq_pre_classical),
            id!(eq_pre_elec),
            id!(eq_pre_acoustic),
            id!(eq_pre_loudness),
            id!(eq_pre_copper),
            id!(autoeq_apply_btn),
            id!(eq_import_btn),
            id!(eq_export_btn),
            id!(pal_amoled_btn),
            id!(pal_acrylic_btn),
            id!(pal_obsidian_btn),
            id!(pal_cream_btn),
            id!(acrylic_tint_apply_btn),
            id!(discord_format_btn),
            id!(discord_portal_btn),
            id!(theme_import_btn),
            id!(theme_export_btn),
            id!(zoom_out_btn),
            id!(zoom_in_btn),
            id!(zoom_reset_btn),
            id!(clear_audio_cache_btn),
            id!(clear_art_cache_btn),
            id!(load_demo_btn),
            id!(remove_demo_btn),
            id!(clear_local_btn),
            id!(check_updates_btn),
            id!(open_releases_btn),
            id!(connect_gen_pair_btn),
            id!(connect_copy_pair_btn),
            id!(rs_lyr_minus),
            id!(rs_lyr_plus),
            id!(rs_lyr_expand),
            id!(rs_lyr_copy),
            id!(rs_watch_video),
            id!(codec_pill_btn),
            id!(fs_radio_btn),
            id!(fs_lyr_minus),
            id!(fs_lyr_plus),
            id!(fs_lyr_copy),
            id!(tm_close_btn),
            id!(tm_play_next_btn),
            id!(tm_add_queue_btn),
            id!(tm_radio_btn),
            id!(tm_like_btn),
            id!(tm_artist_btn),
            id!(tm_album_btn),
            id!(tm_copy_btn),
            id!(tm_remove_btn),
        ] {
            self.ui.button(btn_id).apply_over(
                cx,
                live! { draw_bg: { acrylic_on: (acr), accent_col: (accent) } },
            );
        }

        for chip_or_nav_id in [
            id!(nav_home),
            id!(nav_search),
            id!(nav_recents),
            id!(nav_songs),
            id!(nav_albums),
            id!(nav_artists),
            id!(nav_playlists),
            id!(nav_settings),
            id!(sidebar_add_folder_btn),
            id!(pin_liked_btn),
            id!(pin_local_btn),
            id!(pin_slot_0),
            id!(pin_slot_1),
            id!(pin_slot_2),
            id!(pin_slot_3),
            id!(pin_slot_4),
            id!(chip_0),
            id!(chip_1),
            id!(rsrch_0),
            id!(rsrch_1),
            id!(rsrch_2),
            id!(rsrch_3),
            id!(rsrch_4),
            id!(chip_2),
            id!(chip_3),
            id!(chip_4),
            id!(chip_5),
            id!(chip_6),
            id!(chip_7),
            id!(chip_8),
            id!(sf_all_btn),
            id!(sf_songs_btn),
            id!(sf_albums_btn),
            id!(sf_artists_btn),
            id!(sf_playlists_btn),
            id!(sf_local_btn),
            id!(flt_all_btn),
            id!(flt_liked_btn),
            id!(flt_local_btn),
            id!(flt_spotify_btn),
            id!(flt_youtube_btn),
            id!(st_accounts_btn),
            id!(st_playback_btn),
            id!(st_eq_btn),
            id!(st_appearance_btn),
            id!(st_integrations_btn),
            id!(st_storage_btn),
            id!(st_connect_btn),
            id!(tint_obsidian_btn),
            id!(tint_ember_btn),
            id!(tint_copper_btn),
            id!(tint_midnight_btn),
            id!(tint_emerald_btn),
            id!(tint_crimson_btn),
            id!(tint_slate_btn),
            id!(tint_smoke_btn),
            id!(rs_tab_queue),
            id!(rs_tab_lyrics),
            id!(fs_mode_art_btn),
            id!(fs_mode_video_btn),
            id!(aq_low_btn),
            id!(aq_norm_btn),
            id!(aq_high_btn),
            id!(aq_best_btn),
            id!(vq_auto_btn),
            id!(vq_360_btn),
            id!(vq_480_btn),
            id!(vq_720_btn),
            id!(vq_1080_btn),
            id!(svq_auto_btn),
            id!(svq_360_btn),
            id!(svq_480_btn),
            id!(svq_720_btn),
            id!(svq_1080_btn),
            id!(toggle_left_btn),
            id!(nav_back_btn),
            id!(nav_fwd_btn),
            id!(refresh_btn),
            id!(top_settings_btn),
            id!(toggle_right_btn),
            id!(rs_radio_btn),
            id!(rs_refresh_queue_btn),
            id!(rs_clear_btn),
            id!(bar_video_btn),
            id!(bar_lyrics_btn),
            id!(bar_queue_btn),
            id!(np_add_pl_btn),
            id!(vol_mute_btn),
            id!(tm_pl_0),
            id!(tm_pl_1),
            id!(tm_pl_2),
            id!(tm_pl_3),
            id!(tm_pl_4),
            id!(tm_pl_5),
            id!(bar_fullscreen_btn),
            id!(fs_like_btn),
            id!(fs_shuffle_btn),
            id!(fs_prev_btn),
            id!(fs_next_btn),
            id!(fs_repeat_btn),
            id!(fs_vol_mute_btn),
        ] {
            self.ui
                .button(chip_or_nav_id)
                .apply_over(cx, live! { draw_bg: { acrylic_on: (acr) } });
        }

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

    fn apply_discord_settings(&mut self, cx: &mut Cx) {
        if let Some(db) = &self.db {
            let _ = db.save_discord_rpc(&self.discord_settings);
        }
        if let Some(rpc) = &mut self.discord_rpc {
            rpc.configure(self.discord_settings.clone());
            if let Some(cur) = self.queue.current() {
                let snap = self.audio.as_ref().map(|a| a.telemetry());
                rpc.update_playback(Some(DiscordTrackSnapshot {
                    title: cur.title.clone(),
                    artist: cur.artist.clone(),
                    album: cur.album.clone(),
                    cover_url: cur.thumbnail_url.clone(),
                    position_ms: snap.as_ref().map_or(0, |s| s.position_ms),
                    duration_ms: snap
                        .as_ref()
                        .map_or(cur.duration_ms, |s| s.duration_ms.max(cur.duration_ms)),
                    is_playing: snap.as_ref().is_some_and(|s| s.is_playing),
                }));
            }
        }
        self.sync_controls_from_state(cx);
        self.ui.redraw(cx);
    }

    fn refresh_discord_preview(&self, cx: &mut Cx) {
        if let Some(rpc) = &self.discord_rpc {
            self.ui
                .label(id!(discord_status_lbl))
                .set_text(cx, &rpc.status_text());
        }
        let Some(track) = self.queue.current() else {
            self.ui
                .label(id!(discord_prev_header))
                .set_text(cx, "Listening to Ferric");
            self.ui
                .label(id!(discord_prev_title))
                .set_text(cx, "Play a song to preview Discord activity");
            self.ui
                .label(id!(discord_prev_artist))
                .set_text(cx, "Ferric Hi-Fi Player");
            self.ui
                .label(id!(discord_prev_album))
                .set_text(cx, "Native Windows IPC");
            return;
        };

        if self.discord_settings.hide_details {
            self.ui
                .label(id!(discord_prev_header))
                .set_text(cx, "Listening to Ferric");
            self.ui
                .label(id!(discord_prev_title))
                .set_text(cx, "Listening to music");
            self.ui
                .label(id!(discord_prev_artist))
                .set_text(cx, "Ferric Hi-Fi Player");
            self.ui
                .label(id!(discord_prev_album))
                .set_text(cx, "Track details hidden");
        } else {
            let header = match self.discord_settings.status_format {
                DiscordStatusFormat::ArtistTitle => {
                    format!("Listening to {} - {}", track.artist, track.title)
                }
                DiscordStatusFormat::TrackTitle => format!("Listening to {}", track.title),
                DiscordStatusFormat::Artist => format!("Listening to {}", track.artist),
                DiscordStatusFormat::AppName => "Listening to Ferric".to_string(),
            };
            self.ui
                .label(id!(discord_prev_header))
                .set_text(cx, &clip_text(&clean_display_text(&header), 56));
            self.ui
                .label(id!(discord_prev_title))
                .set_text(cx, &clip_text(&clean_display_text(&track.title), 44));
            self.ui
                .label(id!(discord_prev_artist))
                .set_text(cx, &clip_text(&clean_display_text(&track.artist), 44));
            let alb = if track.album.trim().is_empty() {
                "Ferric".to_string()
            } else {
                track.album.clone()
            };
            self.ui
                .label(id!(discord_prev_album))
                .set_text(cx, &clip_text(&clean_display_text(&alb), 44));
        }

        self.bind_cover_art_url(
            cx,
            self.ui.ferric_cover_art(id!(discord_prev_art)),
            if self.discord_settings.show_cover_art && !self.discord_settings.hide_details {
                track.thumbnail_url.as_deref()
            } else {
                None
            },
            Some(track),
            false,
            0.55,
            false,
        );
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
            // Only lock 0ms gapless album transitions for true Local albums (never streaming playlists
            // whose album labels like "Quick picks" or "YouTube Music" would wrongly disable crossfade).
            let same_album = self.queue.current().is_some_and(|cur| {
                cur.source == TrackSource::Local
                    && next_track.source == TrackSource::Local
                    && !cur.album.is_empty()
                    && cur.album != "Unknown Album"
                    && cur.album == next_track.album
                    && cur.id != next_track.id
            });
            audio.send(EngineCommand::QueueNextTrack {
                path: valid_path,
                same_album_consecutive: same_album,
                fallback_duration_ms: next_track.duration_ms,
            });
        } else {
            audio.send(EngineCommand::QueueNextTrack {
                path: None,
                same_album_consecutive: false,
                fallback_duration_ms: 0,
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
        let seed_artist = track.artist.clone();
        let seed_album = track.album.clone();
        thread::spawn(move || {
            if let Ok(tracks) = fetch_radio_up_next_for_track(
                &session,
                &vid,
                &seed_title,
                &seed_artist,
                &seed_album,
                15,
                force,
            ) && !tracks.is_empty()
            {
                let _ = tx.send(BgMessage::RadioLoaded {
                    seed_track_id,
                    seed_title,
                    tracks,
                });
            }
        });
    }

    fn is_text_input_focused(&self, cx: &Cx) -> bool {
        for input_id in [
            id!(top_search_input),
            id!(search_page_input),
            id!(songs_filter_input),
            id!(pl_new_input),
            id!(detail_rename_input),
            id!(quick_sync_input),
            id!(sub_url_input),
            id!(sub_user_input),
            id!(sub_pass_input),
            id!(autoeq_input),
            id!(acrylic_tint_input),
            id!(discord_client_id_input),
            id!(cmdk_input),
            id!(tm_new_pl_input),
        ] {
            if cx.has_key_focus(self.ui.widget(input_id).area()) {
                return true;
            }
        }
        false
    }

    fn open_track_menu(&mut self, cx: &mut Cx, track: Track) {
        self.ui
            .label(id!(tm_title_lbl))
            .set_text(cx, &clip_text(&clean_display_text(&track.title), 44));
        self.ui.label(id!(tm_sub_lbl)).set_text(
            cx,
            &clip_text(
                &clean_display_text(&format!("{} · {}", track.artist, track.album)),
                54,
            ),
        );
        self.ui
            .button(id!(tm_like_btn))
            .set_text(cx, if track.liked { "Unlike" } else { "Like" });
        let in_saved_pl = self.active_page == MainPage::Detail
            && self.detail_state.kind == YtmCardKind::Playlist
            && self.playlists.iter().any(|p| p.id == self.detail_state.id);
        let remove_lbl = if in_saved_pl {
            "Remove from Playlist"
        } else if self.active_page == MainPage::Recents {
            "Remove from Recents"
        } else {
            "Remove from Library"
        };
        self.ui.button(id!(tm_remove_btn)).set_text(cx, remove_lbl);

        for (i, pid) in [
            id!(tm_pl_0),
            id!(tm_pl_1),
            id!(tm_pl_2),
            id!(tm_pl_3),
            id!(tm_pl_4),
            id!(tm_pl_5),
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(pl) = self.playlists.get(i) {
                self.ui.button(pid).set_visible(cx, true);
                self.ui.button(pid).set_text(cx, &clip_text(&pl.name, 16));
            } else {
                self.ui.button(pid).set_visible(cx, false);
            }
        }
        self.ui.text_input(id!(tm_new_pl_input)).set_text(cx, "");
        self.track_menu_target = Some(track);
        self.track_menu_open = true;
        self.ui.view(id!(track_menu_modal)).set_visible(cx, true);
        self.sync_video_bounds(cx);
        self.ui.redraw(cx);
    }

    fn close_track_menu(&mut self, cx: &mut Cx) {
        self.track_menu_open = false;
        self.track_menu_target = None;
        self.ui.view(id!(track_menu_modal)).set_visible(cx, false);
        self.sync_video_bounds(cx);
        self.ui.redraw(cx);
    }

    fn add_track_to_saved_playlist(
        &mut self,
        cx: &mut Cx,
        playlist_id: &str,
        playlist_name: &str,
        track: &Track,
    ) {
        if let Some(db) = &mut self.db {
            let _ = db.upsert_tracks(std::slice::from_ref(track));
            let _ = db.add_track_to_playlist(playlist_id, &track.id);
            self.all_tracks = db.list_tracks("").unwrap_or_default();
            self.playlists = db.list_playlists().unwrap_or_default();
        }
        if self.active_page == MainPage::Detail
            && self.detail_state.id == playlist_id
            && let Some(db) = &self.db
            && let Ok(updated) = db.list_playlist_tracks(playlist_id)
        {
            self.detail_state.tracks = updated.clone();
            self.detail_tracks = Rc::new(updated);
            self.refresh_detail_hero(cx);
        }
        self.refresh_playlists_grid(cx);
        self.rebuild_library_views(cx);
        self.show_toast(
            cx,
            &format!(
                "Added '{}' to '{}'",
                clip_text(&track.title, 22),
                playlist_name
            ),
        );
    }

    fn remove_track_contextually(&mut self, cx: &mut Cx, target: &Track) {
        let title = clip_text(&target.title, 24);
        let in_saved_pl = self.active_page == MainPage::Detail
            && self.detail_state.kind == YtmCardKind::Playlist
            && self.playlists.iter().any(|p| p.id == self.detail_state.id);

        if in_saved_pl {
            let pl_id = self.detail_state.id.clone();
            if let Some(db) = &mut self.db {
                let _ = db.remove_track_from_playlist(&pl_id, &target.id);
                self.playlists = db.list_playlists().unwrap_or_default();
                let updated = db.list_playlist_tracks(&pl_id).unwrap_or_default();
                self.detail_state.tracks = updated.clone();
                self.detail_tracks = Rc::new(updated);
            }
            self.refresh_detail_hero(cx);
            self.refresh_playlists_grid(cx);
            self.show_toast(cx, &format!("Removed '{title}' from playlist"));
            return;
        }

        if self.active_page == MainPage::Recents {
            self.session_state
                .recent_track_ids
                .retain(|id| id != &target.id);
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.rebuild_library_views(cx);
            self.refresh_right_sidebar(cx);
            self.show_toast(cx, &format!("Removed '{title}' from Recents"));
            return;
        }

        // Remove from Ferric SQLite library (never deletes user's own music files on disk;
        // removes generated demo `.wav` only for `demo:` tracks).
        if target.id.starts_with("demo:")
            && let Some(p) = target.path.as_ref()
            && (p.starts_with(app_data_dir().join("demo_tracks"))
                || p.starts_with(app_data_dir().join("demo")))
        {
            let _ = std::fs::remove_file(p);
        }
        if let Some(db) = &mut self.db {
            let _ = db.delete_track(&target.id);
            self.all_tracks = db.list_tracks("").unwrap_or_default();
            self.playlists = db.list_playlists().unwrap_or_default();
        } else {
            self.all_tracks.retain(|t| t.id != target.id);
        }
        self.session_state
            .recent_track_ids
            .retain(|id| id != &target.id);
        if let Some(db) = &self.db {
            let _ = db.save_session_state(&self.session_state);
        }
        let was_playing_target = self.queue.current().is_some_and(|cur| cur.id == target.id);
        let _ = self.queue.remove_track_by_id(&target.id);
        if was_playing_target {
            if let Some(next) = self.queue.current().cloned() {
                self.play_track(cx, next);
            } else if let Some(audio) = &self.audio {
                audio.send(EngineCommand::Stop);
            }
        }
        if self.active_page == MainPage::Search {
            let mut next_search: Vec<Track> = (*self.search_tracks).clone();
            next_search.retain(|t| t.id != target.id);
            self.search_tracks = Rc::new(next_search);
        } else if self.active_page == MainPage::Detail {
            self.detail_state.tracks.retain(|t| t.id != target.id);
            self.detail_tracks = Rc::new(self.detail_state.tracks.clone());
            self.refresh_detail_hero(cx);
        }
        self.sync_next_deck_to_engine();
        self.rebuild_library_views(cx);
        self.refresh_right_sidebar(cx);
        self.show_toast(cx, &format!("Removed '{title}' from library"));
    }

    fn remove_all_demo_tracks(&mut self, cx: &mut Cx) {
        let removed = if let Some(db) = &mut self.db {
            let n = db.delete_demo_tracks().unwrap_or(0);
            self.all_tracks = db.list_tracks("").unwrap_or_default();
            self.playlists = db.list_playlists().unwrap_or_default();
            n
        } else {
            let before = self.all_tracks.len();
            self.all_tracks.retain(|t| !t.id.starts_with("demo:"));
            before.saturating_sub(self.all_tracks.len())
        };
        clear_flat_cache_files(&app_data_dir().join("demo_tracks"));
        clear_flat_cache_files(&app_data_dir().join("demo"));
        self.session_state
            .recent_track_ids
            .retain(|id| !id.starts_with("demo:"));
        if let Some(db) = &self.db {
            let _ = db.save_session_state(&self.session_state);
        }
        let demo_ids: Vec<String> = self
            .queue
            .tracks()
            .iter()
            .filter(|t| t.id.starts_with("demo:"))
            .map(|t| t.id.clone())
            .collect();
        for did in demo_ids {
            let _ = self.queue.remove_track_by_id(&did);
        }
        self.rebuild_library_views(cx);
        self.refresh_right_sidebar(cx);
        self.show_toast(
            cx,
            if removed > 0 {
                "Removed offline demo tracks"
            } else {
                "No demo tracks in library"
            },
        );
    }

    fn export_detail_m3u(&mut self, cx: &mut Cx) {
        if self.detail_state.tracks.is_empty() {
            return;
        }
        let safe_name: String = self
            .detail_state
            .title
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == ' ' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let default_file = format!("{}.m3u8", safe_name.trim());
        if let Some(dest) = rfd::FileDialog::new()
            .set_title("Export Playlist (.m3u8)")
            .set_file_name(&default_file)
            .add_filter("M3U8 Playlist", &["m3u8", "m3u"])
            .save_file()
        {
            let mut out = String::from("#EXTM3U\n");
            for t in &self.detail_state.tracks {
                let secs = (t.duration_ms / 1000) as i64;
                out.push_str(&format!("#EXTINF:{secs},{} - {}\n", t.artist, t.title));
                if let Some(p) = &t.path {
                    out.push_str(&format!("{}\n", p.to_string_lossy()));
                } else if let Some(vid) = &t.youtube_video_id {
                    out.push_str(&format!("https://music.youtube.com/watch?v={vid}\n"));
                }
            }
            if std::fs::write(&dest, out).is_ok() {
                self.show_toast(cx, &format!("Exported '{}'", self.detail_state.title));
            }
        }
    }

    fn spawn_stream_resolve(&self, track: Track, auto_play: bool) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let session = self.current_ytm_session();
        let cache_dir = app_data_dir().join("stream_cache");
        let audio_quality = self.dsp.audio_quality;
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
                let stream = resolve_and_cache_audio_stream_with_quality(
                    &vid,
                    &session,
                    &cache_dir,
                    audio_quality,
                )
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

    fn send_play_or_crossfade(
        &self,
        path: PathBuf,
        replay_gain_db: i32,
        fallback_duration_ms: u64,
    ) {
        let Some(audio) = &self.audio else {
            return;
        };
        let currently_playing = audio.telemetry().is_playing;
        if currently_playing && self.dsp.crossfade_ms >= 500 {
            audio.send(EngineCommand::CrossfadeToTrack {
                path,
                replay_gain_db,
                fallback_duration_ms,
                overlap_ms: self.dsp.crossfade_ms.clamp(1_200, 3_000),
            });
        } else {
            audio.send(EngineCommand::LoadAndPlayTrack {
                path,
                replay_gain_db,
                fallback_duration_ms,
            });
        }
    }

    fn play_track(&mut self, cx: &mut Cx, track: Track) {
        self.last_pos_txt.clear();
        self.last_dur_txt.clear();
        self.last_codec_txt.clear();
        if let Some(cache) = &mut self.artwork {
            cache.pin_active_track(Some(&track));
        }
        if let Some(path) = track.path.as_ref().filter(|p| p.is_file()).cloned() {
            self.pending_stream_track_id = None;
            self.send_play_or_crossfade(path, track.replay_gain_db, track.duration_ms);
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
                .set_text(cx, &clip_text(&track.title, 22));
            self.ui.label(id!(np_artist_lbl)).set_text(
                cx,
                &clip_text(&format!("{} · Buffering...", track.artist), 24),
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

    fn persist_track_to_library(&mut self, track: &Track) {
        let mut to_save = track.clone();
        if let Some(existing) = self.all_tracks.iter().find(|t| t.id == track.id) {
            to_save.liked = to_save.liked || existing.liked;
            if to_save.path.is_none() {
                to_save.path = existing.path.clone();
            }
        }
        if let Some(db) = &mut self.db {
            let _ = db.upsert_tracks(std::slice::from_ref(&to_save));
        }
        if let Some(slot) = self.all_tracks.iter_mut().find(|t| t.id == to_save.id) {
            *slot = to_save;
        } else {
            self.all_tracks.push(to_save);
        }
    }

    fn toggle_track_like(&mut self, cx: &mut Cx, target: &Track) -> bool {
        let cur_liked = self
            .all_tracks
            .iter()
            .find(|t| t.id == target.id)
            .map_or(target.liked, |t| t.liked);
        let next_liked = !cur_liked;
        let mut saved = target.clone();
        saved.liked = next_liked;
        if let Some(db) = &mut self.db {
            let _ = db.upsert_tracks(std::slice::from_ref(&saved));
            let _ = db.set_track_liked(&saved.id, next_liked);
        }
        if let Some(slot) = self.all_tracks.iter_mut().find(|t| t.id == saved.id) {
            slot.liked = next_liked;
        } else {
            self.all_tracks.push(saved.clone());
        }
        self.queue.set_track_liked(&saved.id, next_liked);
        if self.queue.current().map(|t| t.id.as_str()) == Some(saved.id.as_str()) {
            let accent = self.current_accent();
            let like_col = if next_liked {
                accent
            } else {
                vec4(0.439, 0.451, 0.510, 1.0)
            };
            for lbtn in [id!(np_like_btn), id!(fs_like_btn)] {
                self.ui
                    .button(lbtn)
                    .apply_over(cx, live! { draw_icon: { color: (like_col) } });
            }
        }
        self.rebuild_library_views(cx);
        next_liked
    }

    fn update_now_playing_metadata(&mut self, cx: &mut Cx, track: &Track) {
        self.persist_track_to_library(track);
        self.session_state.record_play(&track.id, track.duration_ms);
        if let Some(db) = &self.db {
            let _ = db.save_session_state(&self.session_state);
        }
        if let Some(cache) = &mut self.artwork {
            cache.pin_active_track(Some(track));
        }

        win32_update_tray_tooltip(&format!("Ferric — {} · {}", track.title, track.artist));
        win32_update_taskbar_playback(&track.title, &track.artist, true);

        let out_rate = self
            .audio
            .as_ref()
            .map_or(48_000, |a| a.telemetry().output_rate);
        self.ui
            .label(id!(np_title_lbl))
            .set_text(cx, &clip_text(&clean_display_text(&track.title), 20));
        self.ui
            .label(id!(np_artist_lbl))
            .set_text(cx, &clip_text(&clean_display_text(&track.artist), 22));
        self.ui
            .label(id!(fs_title_lbl))
            .set_text(cx, &clip_text(&clean_display_text(&track.title), 36));
        self.ui.label(id!(fs_artist_lbl)).set_text(
            cx,
            &clip_text(
                &clean_display_text(&format!("{} · {}", track.artist, track.album)),
                44,
            ),
        );
        self.ui
            .button(id!(codec_pill_btn))
            .set_text(cx, &track.quality_badge());

        let accent = self.current_accent();
        let is_liked = self
            .all_tracks
            .iter()
            .find(|t| t.id == track.id)
            .map_or(track.liked, |t| t.liked);
        let like_col = if is_liked {
            accent
        } else {
            vec4(0.439, 0.451, 0.510, 1.0)
        };
        for lbtn in [id!(np_like_btn), id!(fs_like_btn)] {
            self.ui
                .button(lbtn)
                .apply_over(cx, live! { draw_icon: { color: (like_col) } });
        }
        self.ui
            .ferric_play_btn(id!(play_pause_btn))
            .set_playing(cx, true);
        self.ui
            .ferric_play_btn(id!(fs_play_pause_btn))
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
        self.sync_fullscreen_backdrop_artwork(cx);
        self.apply_adaptive_accent_live(cx);

        if let Some(smtc) = &mut self.smtc {
            smtc.try_attach_late();
            smtc.set_metadata(&track.title, &track.artist, &track.album, track.duration_ms);
            smtc.set_playing(true);
        }
        if let Some(rpc) = &mut self.discord_rpc {
            rpc.update_playback(Some(DiscordTrackSnapshot {
                title: track.title.clone(),
                artist: track.artist.clone(),
                album: track.album.clone(),
                cover_url: track.thumbnail_url.clone(),
                position_ms: 0,
                duration_ms: track.duration_ms,
                is_playing: true,
            }));
        }
        self.refresh_discord_preview(cx);

        self.lyrics_offset_ms = 0;
        self.load_lyrics_for_track(cx, track);
        self.rebuild_library_views(cx);
        self.refresh_home_ui(cx);
        self.refresh_right_sidebar(cx);

        // If native in-app video clip stage is open, follow the newly playing track automatically
        if self.video_open && self.video_track_id.as_deref() != Some(track.id.as_str()) {
            self.open_native_video_stage(cx);
        }
        self.ui.redraw(cx);
    }

    fn load_lyrics_for_track(&mut self, cx: &mut Cx, track: &Track) {
        self.synced_lyrics = SyncedLyrics::default();
        self.set_lyrics_center_message(cx, "Searching synced lyrics (LRCLIB)...");

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
            self.push_lyrics_to_views(cx);
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

    fn set_lyrics_center_message(&self, cx: &mut Cx, msg: &str) {
        let accent = self.current_accent();
        let z = self.theme.ui_zoom.clamp(0.65, 1.40);
        for vid in [id!(rs_lyrics_scroll), id!(fs_lyrics_scroll)] {
            let lv = self.ui.ferric_lyrics_view(vid);
            lv.set_style(cx, accent, z);
            lv.set_empty_message(cx, msg);
        }
    }

    fn push_lyrics_to_views(&self, cx: &mut Cx) {
        let accent = self.current_accent();
        let z = self.theme.ui_zoom.clamp(0.65, 1.40);
        let items: Vec<(u64, String)> = self
            .synced_lyrics
            .lines
            .iter()
            .map(|l| (l.timestamp_ms, l.text.clone()))
            .collect();
        for vid in [id!(rs_lyrics_scroll), id!(fs_lyrics_scroll)] {
            let lv = self.ui.ferric_lyrics_view(vid);
            lv.set_style(cx, accent, z);
            lv.set_lyrics(cx, items.clone());
        }
    }

    fn update_lyrics_display(&self, cx: &mut Cx, raw_pos_ms: u64) {
        if self.synced_lyrics.lines.is_empty() {
            return;
        }
        let shifted = (raw_pos_ms as i64 + self.lyrics_offset_ms).max(0) as u64;
        let is_playing = self
            .audio
            .as_ref()
            .is_some_and(|a| a.telemetry().is_playing);
        let lines = &self.synced_lyrics.lines;
        let idx = match lines.binary_search_by_key(&shifted, |l| l.timestamp_ms) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        };
        let rs_active = !self.window_hidden_or_minimized
            && !self.fullscreen_open
            && self.right_sidebar_open
            && self.right_lyrics_tab;
        let fs_active = !self.window_hidden_or_minimized && self.fullscreen_open;

        let rs = self.ui.ferric_lyrics_view(id!(rs_lyrics_scroll));
        rs.set_playback_state(cx, shifted, is_playing && rs_active);
        if rs_active {
            rs.set_active_index(cx, idx);
        }
        let fs = self.ui.ferric_lyrics_view(id!(fs_lyrics_scroll));
        fs.set_playback_state(cx, shifted, is_playing && fs_active);
        if fs_active {
            fs.set_active_index(cx, idx);
        }
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

    fn spawn_ytm_rich_home(
        &mut self,
        cx: &mut Cx,
        mood_index: usize,
        mood_params: Option<String>,
        fresh_seed: bool,
    ) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.active_mood_idx = mood_index;
        self.qp_page = 0;
        self.shelf_pages = [0; 8];
        self.home_loading_more = false;
        let session = self.current_ytm_session();
        let recent_vids: Vec<String> = self
            .session_state
            .recent_track_ids
            .iter()
            .filter_map(|id| {
                self.all_tracks
                    .iter()
                    .find(|t| &t.id == id)
                    .and_then(|t| t.youtube_video_id.clone())
            })
            .take(8)
            .collect();
        self.ui.button(id!(home_retry_btn)).set_visible(cx, false);
        self.ui
            .label(id!(home_status_lbl))
            .set_text(cx, "Refreshing YouTube Music...");
        self.refresh_home_ui(cx);

        thread::spawn(move || {
            match fetch_rich_home(&session, mood_params.as_deref(), fresh_seed, &recent_vids) {
                Ok(home) => {
                    let _ = tx.send(BgMessage::YtmRichHomeLoaded {
                        mood_index,
                        home: Box::new(home),
                    });
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::YtmRichHomeFailed(err.to_string()));
                }
            }
        });
    }

    fn spawn_home_load_more(&mut self, cx: &mut Cx) {
        if self.home_loading_more {
            return;
        }
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.home_loading_more = true;
        self.ui
            .button(id!(home_load_more_btn))
            .set_text(cx, "Loading more...");
        let session = self.current_ytm_session();
        let cont = self.rich_home.continuation_token.clone();
        let fallback_mood = if !self.rich_home.chips.is_empty() {
            let idx = self.home_more_cycle % self.rich_home.chips.len();
            self.home_more_cycle = self.home_more_cycle.wrapping_add(1);
            Some(self.rich_home.chips[idx].params.clone())
        } else {
            None
        };
        thread::spawn(move || {
            match fetch_home_continuation(&session, cont.as_deref(), fallback_mood.as_deref()) {
                Ok(more) => {
                    let _ = tx.send(BgMessage::YtmHomeMoreLoaded(Box::new(more)));
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::YtmHomeMoreFailed(err.to_string()));
                }
            }
        });
    }

    fn spawn_rich_search(&mut self, cx: &mut Cx, raw_query: String) {
        self.dispatch_rich_search(cx, raw_query, true);
    }

    fn schedule_live_search(&mut self, cx: &mut Cx, raw: String, from_top_bar: bool) {
        if from_top_bar {
            if self.ui.text_input(id!(search_page_input)).text() != raw {
                self.ui
                    .text_input(id!(search_page_input))
                    .set_text(cx, &raw);
            }
        } else if self.ui.text_input(id!(top_search_input)).text() != raw {
            self.ui.text_input(id!(top_search_input)).set_text(cx, &raw);
        }

        let q = raw.trim().to_string();
        if q.is_empty() {
            self.pending_live_search = None;
            self.last_dispatched_search.clear();
            self.search_request_seq = self.search_request_seq.wrapping_add(1);
            self.search_result = YtmRichSearch::default();
            self.search_tracks = Rc::new(Vec::new());
            self.ui
                .label(id!(search_status_lbl))
                .set_text(cx, "Instant YouTube Music + Local Search");
            self.refresh_search_ui(cx);
            self.ui.redraw(cx);
            return;
        }
        // Wait for explicit Enter / Search click on pasted playlist URLs
        if parse_spotify_target(&q).is_some() || parse_youtube_target(&q).is_some() {
            self.pending_live_search = None;
            return;
        }
        if q.chars().count() < 2 {
            self.pending_live_search = None;
            return;
        }

        if self.active_page != MainPage::Search {
            self.navigate_to(cx, MainPage::Search, true);
            if from_top_bar {
                self.ui.widget(id!(top_search_input)).set_key_focus(cx);
            }
        }

        // Instant zero-latency local + Home feed filtering while live YouTube Music results load
        let q_low = q.to_ascii_lowercase();
        let mut instant_hits = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for t in self
            .all_tracks
            .iter()
            .chain(self.rich_home.quick_picks.iter())
            .chain(self.rich_home.all_tracks.iter())
        {
            if (t.title.to_ascii_lowercase().contains(&q_low)
                || t.artist.to_ascii_lowercase().contains(&q_low)
                || t.album.to_ascii_lowercase().contains(&q_low))
                && seen.insert(t.id.clone())
            {
                instant_hits.push(t.clone());
            }
        }
        if !instant_hits.is_empty() {
            self.search_tracks = Rc::new(instant_hits);
        }
        self.ui
            .label(id!(search_status_lbl))
            .set_text(cx, &format!("Searching '{q}'..."));
        self.refresh_search_ui(cx);
        self.pending_live_search = Some((q, Instant::now() + Duration::from_millis(260)));
    }

    fn dispatch_rich_search(&mut self, cx: &mut Cx, raw_query: String, record_history: bool) {
        let query = raw_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.pending_live_search = None;
        if parse_spotify_target(&query).is_some() {
            self.start_public_sync(cx, "Spotify", query);
            return;
        }
        if parse_youtube_target(&query).is_some() {
            self.start_public_sync(cx, "YouTube", query);
            return;
        }
        if record_history {
            self.session_state.record_search(&query);
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
        }
        if self.ui.text_input(id!(search_page_input)).text() != query {
            self.ui
                .text_input(id!(search_page_input))
                .set_text(cx, &query);
        }
        if self.ui.text_input(id!(top_search_input)).text() != query {
            self.ui
                .text_input(id!(top_search_input))
                .set_text(cx, &query);
        }
        if self.active_page != MainPage::Search {
            self.navigate_to(cx, MainPage::Search, true);
        }
        self.ui
            .label(id!(search_status_lbl))
            .set_text(cx, &format!("Searching '{query}'..."));

        // Immediate local + cached matches while YouTube Music responds
        let q_low = query.to_ascii_lowercase();
        let mut local_hits = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for t in self
            .all_tracks
            .iter()
            .chain(self.rich_home.quick_picks.iter())
            .chain(self.rich_home.all_tracks.iter())
        {
            if (t.title.to_ascii_lowercase().contains(&q_low)
                || t.artist.to_ascii_lowercase().contains(&q_low)
                || t.album.to_ascii_lowercase().contains(&q_low))
                && seen.insert(t.id.clone())
            {
                local_hits.push(t.clone());
            }
        }
        if !local_hits.is_empty() || record_history {
            self.search_tracks = Rc::new(local_hits);
        }
        self.refresh_search_ui(cx);

        self.search_request_seq = self.search_request_seq.wrapping_add(1);
        let seq = self.search_request_seq;
        self.last_dispatched_search = query.clone();
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let session = self.current_ytm_session();
        thread::spawn(move || match fetch_rich_search(&session, &query, 30) {
            Ok(result) => {
                let _ = tx.send(BgMessage::YtmRichSearchCompleted {
                    seq,
                    query,
                    result: Box::new(result),
                });
            }
            Err(err) => {
                let _ = tx.send(BgMessage::YtmSearchFailed {
                    seq,
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
        // 0a. Guarantee Win32 subclass hook is attached to the real MakepadWindow HWND
        // (no-op single atomic load once installed).
        win32_install_window_hooks();

        // 0b. Multi-stage & idle working-set trim (releases DX11 shader scratch, SQLite cache,
        // and artwork decode staging pages so Ferric stays strictly <100 MB open and even lower in tray).
        if let Some(due) = self.tray_second_trim_at
            && Instant::now() >= due
        {
            self.tray_second_trim_at = None;
            if let Some(db) = &self.db {
                db.shrink_memory();
            }
            win32_trim_working_set();
        }
        if let Some(started) = self.started_at {
            let elapsed = started.elapsed();
            if self.startup_trim_stage == 0 && elapsed >= Duration::from_millis(1_600) {
                self.startup_trim_stage = 1;
                self.last_trim_at = Some(Instant::now());
                self.artwork_uploads_since_trim = 0;
                if let Some(db) = &self.db {
                    db.shrink_memory();
                }
                win32_trim_working_set();
            } else if self.startup_trim_stage == 1 && elapsed >= Duration::from_millis(3_400) {
                self.startup_trim_stage = 2;
                self.last_trim_at = Some(Instant::now());
                self.artwork_uploads_since_trim = 0;
                if let Some(db) = &self.db {
                    db.shrink_memory();
                }
                win32_trim_working_set();
            } else if self.startup_trim_stage >= 2
                && !self.window_hidden_or_minimized
                && self.page_reveal_t >= 0.995
                && ((self.artwork_uploads_since_trim >= 3
                    && self
                        .last_trim_at
                        .is_none_or(|t| t.elapsed() >= Duration::from_secs(3)))
                    || self
                        .last_trim_at
                        .is_none_or(|t| t.elapsed() >= Duration::from_secs(8)))
            {
                self.last_trim_at = Some(Instant::now());
                self.artwork_uploads_since_trim = 0;
                if let Some(db) = &self.db {
                    db.shrink_memory();
                }
                win32_trim_working_set();
            }
        }

        // 0b. Poll System Tray & Win32 subclass events
        for tray_ev in win32_poll_tray_events() {
            match tray_ev {
                TrayEvent::HiddenToTray => {
                    self.window_hidden_or_minimized = true;
                    if self.video_open {
                        self.close_native_video_stage(cx);
                    }
                    // Immediately halt hidden 60 FPS lyrics/ambient GPU frames so DX11 stays completely asleep in tray
                    let accent = self.current_accent();
                    let acr = if self.theme.acrylic_enabled {
                        1.0_f32
                    } else {
                        0.0_f32
                    };
                    self.ui
                        .ferric_ambient_stage(id!(fs_backdrop))
                        .set_state(cx, false, false, accent, acr);
                    self.ui
                        .ferric_lyrics_view(id!(rs_lyrics_scroll))
                        .set_playback_state(cx, 0, false);
                    self.ui
                        .ferric_lyrics_view(id!(fs_lyrics_scroll))
                        .set_playback_state(cx, 0, false);
                    if let Some(cache) = &mut self.artwork {
                        cache.clear_all_for_tray();
                        self.artwork_snapshot = Rc::new(HashMap::new());
                    }
                    self.missing_art_queue.borrow_mut().clear();
                    self.missing_art_queue.borrow_mut().shrink_to_fit();
                    if let Some(db) = &self.db {
                        db.shrink_memory();
                    }
                    win32_trim_working_set();
                    // Schedule a second pass after DX11 finishes retiring frame buffers
                    self.tray_second_trim_at = Some(Instant::now() + Duration::from_millis(900));
                }
                TrayEvent::ShowWindow => {
                    self.window_hidden_or_minimized = false;
                    self.tray_second_trim_at = None;
                    if let Some(cur) = self.queue.current().cloned()
                        && let Some(cache) = &mut self.artwork
                    {
                        cache.pin_active_track(Some(&cur));
                    }
                    let (is_playing, pos_ms) = self.audio.as_ref().map_or((false, 0), |a| {
                        let t = a.telemetry();
                        (t.is_playing, t.position_ms)
                    });
                    if self.fullscreen_open {
                        let accent = self.current_accent();
                        let acr = if self.theme.acrylic_enabled {
                            1.0_f32
                        } else {
                            0.0_f32
                        };
                        self.ui
                            .ferric_ambient_stage(id!(fs_backdrop))
                            .set_state(cx, true, is_playing, accent, acr);
                    }
                    self.update_lyrics_display(cx, pos_ms);
                    self.refresh_all_artwork_slots(cx);
                    self.sync_video_bounds(cx);
                    cx.redraw_all();
                }
                TrayEvent::TogglePlayPause => {
                    if let Some(a) = &self.audio {
                        a.send(EngineCommand::TogglePlayPause);
                    }
                }
                TrayEvent::NextTrack => {
                    if let Some(next) = self.queue.next_track().cloned() {
                        self.play_track(cx, next);
                    }
                }
                TrayEvent::PrevTrack => {
                    if let Some(prev) = self.queue.prev_track().cloned() {
                        self.play_track(cx, prev);
                    }
                }
                TrayEvent::QuitApp => {
                    self.close_native_video_stage(cx);
                    win32_force_quit();
                    cx.quit();
                    return;
                }
                TrayEvent::WindowMovedOrResized => {
                    self.sync_video_bounds(cx);
                }
            }
        }

        let window_hidden = win32_is_window_hidden_or_minimized();

        // 0c. Poll async GPU artwork cache (skip uploads while minimized/hidden in tray)
        if !window_hidden
            && let Some(cache) = &mut self.artwork
            && cache.poll(cx)
        {
            self.artwork_uploads_since_trim = self.artwork_uploads_since_trim.saturating_add(1);
            self.refresh_all_artwork_slots(cx);
        }

        // 0d. Smooth exponential accent color lerp when switching tracks
        if !window_hidden {
            let target = self.target_accent();
            let diff = (target.x - self.displayed_accent.x).abs()
                + (target.y - self.displayed_accent.y).abs()
                + (target.z - self.displayed_accent.z).abs();
            if diff > 0.008 {
                let k = 0.16_f32;
                self.displayed_accent = vec4(
                    self.displayed_accent.x + (target.x - self.displayed_accent.x) * k,
                    self.displayed_accent.y + (target.y - self.displayed_accent.y) * k,
                    self.displayed_accent.z + (target.z - self.displayed_accent.z) * k,
                    1.0,
                );
                self.apply_adaptive_accent_live(cx);
            }
        }

        // Fire debounced live instant search while typing in top bar or Search page
        if let Some((_, due)) = self.pending_live_search
            && Instant::now() >= due
            && let Some((q, _)) = self.pending_live_search.take()
            && q != self.last_dispatched_search
        {
            self.dispatch_rich_search(cx, q, false);
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
                            self.set_lyrics_center_message(cx, "No lyrics found for this song.");
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
                            self.push_lyrics_to_views(cx);
                            let pos_ms =
                                self.audio.as_ref().map_or(0, |a| a.telemetry().position_ms);
                            self.update_lyrics_display(cx, pos_ms);
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
                    self.home_loading_more = false;
                    self.ui.button(id!(home_retry_btn)).set_visible(cx, false);

                    if let Some(db) = &mut self.db {
                        let _ = db.save_accounts(&self.accounts);
                        self.playlists = db.list_playlists().unwrap_or_default();
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
                    self.home_loading_more = false;
                    self.ui.button(id!(home_retry_btn)).set_visible(cx, true);
                    let has_cached = !self.home_quick_picks_pool().is_empty();
                    if has_cached {
                        let pick_n = self.home_quick_picks_pool().len();
                        let shelf_n = self.rich_home.shelves.len();
                        self.ui
                            .label(id!(home_status_lbl))
                            .set_text(cx, &format!("{pick_n} songs · {shelf_n} shelves"));
                        self.show_toast(cx, "YouTube Music timed out — showing cached feed");
                    } else {
                        self.ui
                            .label(id!(home_status_lbl))
                            .set_text(cx, "Connection timed out — click Retry");
                    }
                    self.ui
                        .label(id!(oauth_status_lbl))
                        .set_text(cx, &format!("Home refresh failed: {err}"));
                }
                BgMessage::YtmHomeMoreLoaded(more) => {
                    let more = *more;
                    self.home_loading_more = false;
                    self.rich_home.continuation_token = more.continuation_token;
                    if !more.visitor_data.is_empty() {
                        self.accounts.youtube_visitor_data = more.visitor_data;
                    }
                    let mut new_picks = 0usize;
                    for t in more.quick_picks {
                        if !self.rich_home.quick_picks.iter().any(|q| q.id == t.id) {
                            self.rich_home.quick_picks.push(t);
                            new_picks += 1;
                        }
                    }
                    let mut fresh_shelves = Vec::new();
                    for shelf in more.shelves {
                        if !shelf.cards.is_empty()
                            && !self
                                .rich_home
                                .shelves
                                .iter()
                                .any(|s| s.title.eq_ignore_ascii_case(&shelf.title))
                        {
                            fresh_shelves.push(shelf);
                        }
                    }
                    let added_shelves = fresh_shelves.len();
                    if !fresh_shelves.is_empty() {
                        if self.rich_home.shelves.len() < 8 {
                            self.rich_home.shelves.extend(fresh_shelves);
                        } else {
                            // Surface newly fetched shelves cleanly inside the 8-shelf showcase
                            let keep_head = 2usize.min(self.rich_home.shelves.len());
                            let mut next = self.rich_home.shelves[..keep_head].to_vec();
                            next.extend(fresh_shelves);
                            next.extend_from_slice(&self.rich_home.shelves[keep_head..]);
                            next.truncate(24);
                            self.rich_home.shelves = next;
                            self.shelf_pages = [0; 8];
                        }
                    } else if self.rich_home.shelves.len() > 8 {
                        // Rotate deeper already-fetched shelves into view so Load More always surprises
                        let keep_head = 2usize;
                        self.rich_home.shelves[keep_head..].rotate_left(3);
                        self.shelf_pages = [0; 8];
                    }
                    for t in more.all_tracks {
                        if !self.rich_home.all_tracks.iter().any(|x| x.id == t.id) {
                            self.rich_home.all_tracks.push(t);
                        }
                    }
                    self.ui.label(id!(home_status_lbl)).set_text(
                        cx,
                        &format!(
                            "{} songs · {} shelves",
                            self.rich_home.quick_picks.len(),
                            self.rich_home.shelves.len()
                        ),
                    );
                    self.rebuild_library_views(cx);
                    self.refresh_home_ui(cx);
                    self.show_toast(
                        cx,
                        &format!("Loaded {added_shelves} new shelves & {new_picks} songs"),
                    );
                }
                BgMessage::YtmHomeMoreFailed(err) => {
                    self.home_loading_more = false;
                    self.ui
                        .button(id!(home_load_more_btn))
                        .set_text(cx, "Load More Recommendations");
                    self.show_toast(cx, &format!("Load more failed: {err}"));
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
                BgMessage::YtmRichSearchCompleted { seq, query, result } => {
                    if seq != self.search_request_seq {
                        continue;
                    }
                    let result = *result;
                    let song_n = result.tracks.len();
                    let card_n = result.cards.len();
                    // Priority-load Search album/artist/playlist cards first so top cards never look blank
                    self.request_artwork_for_cards(&result.cards);
                    self.request_artwork_for_tracks(&result.tracks);
                    self.search_result = result;
                    self.apply_search_filter(cx, self.search_filter);
                    self.ui.label(id!(search_status_lbl)).set_text(
                        cx,
                        &format!("'{query}' · {song_n} songs, {card_n} albums/artists"),
                    );
                    self.ui.redraw(cx);
                }
                BgMessage::YtmSearchFailed { seq, query, error } => {
                    if seq != self.search_request_seq {
                        continue;
                    }
                    self.ui
                        .label(id!(search_status_lbl))
                        .set_text(cx, &format!("Search '{query}' failed: {error}"));
                }
                BgMessage::RadioLoaded {
                    seed_track_id,
                    seed_title,
                    tracks,
                } => {
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
                    self.spawn_ytm_rich_home(cx, 0, None, false);
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
                        let dur_ms = updated_track.as_ref().map_or(stream.duration_ms, |t| {
                            t.duration_ms.max(stream.duration_ms)
                        });
                        self.send_play_or_crossfade(stream.path, rg, dur_ms);
                        self.sync_next_deck_to_engine();
                        self.prefetch_next_online_track();
                        if let Some(tr) = updated_track.or_else(|| self.queue.current().cloned()) {
                            self.update_now_playing_metadata(cx, &tr);
                        }
                    } else {
                        self.sync_next_deck_to_engine();
                    }
                    // Release TLS/stream download scratch pages immediately so active playback stays <100 MB
                    if let Some(db) = &self.db {
                        db.shrink_memory();
                    }
                    win32_trim_working_set();
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
                BgMessage::VideoStreamsLoaded {
                    track_id,
                    video_id,
                    streams,
                } => {
                    if let Some(t) = self.all_tracks.iter_mut().find(|x| x.id == track_id)
                        && t.youtube_video_id.is_none()
                    {
                        t.youtube_video_id = Some(video_id);
                    }
                    if self.video_open && self.video_track_id.as_deref() == Some(track_id.as_str())
                    {
                        self.video_streams = streams;
                        self.load_selected_video_quality(cx);
                    }
                }
                BgMessage::VideoStreamsFailed { track_id, error } => {
                    if self.video_open && self.video_track_id.as_deref() == Some(track_id.as_str())
                    {
                        self.ui
                            .label(id!(vid_status_lbl))
                            .set_text(cx, "Video unavailable");
                        self.ui
                            .label(id!(vid_stage_hint_lbl))
                            .set_text(cx, &format!("Could not load video clip: {error}"));
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
                            self.close_native_video_stage(cx);
                            win32_force_quit();
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
        if !window_hidden
            && self.active_page == MainPage::Settings
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
        if !window_hidden {
            self.ui
                .ferric_win_btn(id!(win_max_btn))
                .set_maximized(cx, win32_is_active_window_maximized());
        }

        // 6. Audio Telemetry, Zero-Gap Crossfade Auto-Transition, Prefetch, Video Sync & Spectrum
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

            // Proactively prefetch the next online song well before crossfade begins (<= 45s left)
            if snap.is_playing
                && snap.duration_ms > 0
                && snap.duration_ms.saturating_sub(snap.position_ms) <= 45_000
            {
                self.prefetch_next_online_track();
            }

            // Keep native in-app video clip tightly synced to Ferric's Rust WASAPI audio clock
            if self.video_open
                && self
                    .last_video_sync
                    .is_none_or(|t| t.elapsed() >= Duration::from_millis(400))
            {
                self.last_video_sync = Some(Instant::now());
                if let Some(vp) = &self.video_player
                    && vp.is_open()
                {
                    vp.sync_playback(snap.position_ms as f64 / 1000.0, snap.is_playing);
                }
                self.sync_video_bounds(cx);
            }

            if let Some(smtc) = &mut self.smtc {
                smtc.set_playing(snap.is_playing);
            }
            if let Some(cur) = self.queue.current() {
                win32_update_taskbar_playback(&cur.title, &cur.artist, snap.is_playing);
                if let Some(rpc) = &mut self.discord_rpc {
                    rpc.update_playback(Some(DiscordTrackSnapshot {
                        title: cur.title.clone(),
                        artist: cur.artist.clone(),
                        album: cur.album.clone(),
                        cover_url: cur.thumbnail_url.clone(),
                        position_ms: snap.position_ms,
                        duration_ms: snap.duration_ms.max(cur.duration_ms),
                        is_playing: snap.is_playing,
                    }));
                }
            } else {
                win32_update_taskbar_playback("", "", false);
                if let Some(rpc) = &mut self.discord_rpc {
                    rpc.update_playback(None);
                }
            }
            if let Some(rpc) = &self.discord_rpc {
                let st = rpc.status_text();
                if st != self.last_discord_status {
                    self.last_discord_status = st.clone();
                    self.ui.label(id!(discord_status_lbl)).set_text(cx, &st);
                }
            }

            // Skip GPU UI telemetry redraws when hidden in tray / minimized -> near-0% idle GPU/CPU
            if window_hidden {
                return;
            }

            if snap.duration_ms > 0 {
                let ratio = (snap.position_ms as f64 / snap.duration_ms as f64).clamp(0.0, 1.0);
                let pos_txt = format_compact_time_ms(snap.position_ms);
                self.ui
                    .ferric_bar_slider(id!(seek_slider))
                    .set_value(cx, ratio);
                let dur_txt = if self.show_remaining_time {
                    format!(
                        "-{}",
                        format_compact_time_ms(snap.duration_ms.saturating_sub(snap.position_ms))
                    )
                } else {
                    format_compact_time_ms(snap.duration_ms)
                };
                if pos_txt != self.last_pos_txt {
                    self.last_pos_txt = pos_txt.clone();
                    self.ui.label(id!(pos_lbl)).set_text(cx, &pos_txt);
                    if self.fullscreen_open {
                        self.ui.label(id!(fs_pos_lbl)).set_text(cx, &pos_txt);
                    }
                }
                if dur_txt != self.last_dur_txt {
                    self.last_dur_txt = dur_txt.clone();
                    self.ui.button(id!(dur_btn)).set_text(cx, &dur_txt);
                    if self.fullscreen_open {
                        self.ui.label(id!(fs_dur_lbl)).set_text(cx, &dur_txt);
                    }
                }
                if self.fullscreen_open {
                    self.ui
                        .ferric_bar_slider(id!(fs_seek_slider))
                        .set_value(cx, ratio);
                }
                self.update_lyrics_display(cx, snap.position_ms);
            }

            self.ui
                .ferric_play_btn(id!(play_pause_btn))
                .set_playing(cx, snap.is_playing);
            if self.fullscreen_open {
                self.ui
                    .ferric_play_btn(id!(fs_play_pause_btn))
                    .set_playing(cx, snap.is_playing);
            }

            let next_codec = if snap.is_crossfading {
                Some("XFADE".to_string())
            } else if let Some(cur) = self.queue.current()
                && self.pending_stream_track_id.is_none()
            {
                Some(cur.quality_badge())
            } else {
                None
            };
            if let Some(badge) = next_codec
                && badge != self.last_codec_txt
            {
                self.last_codec_txt = badge.clone();
                self.ui.button(id!(codec_pill_btn)).set_text(cx, &badge);
            }

            let accent = self.current_accent();
            if snap.is_playing {
                self.canvas_phase = (self.canvas_phase + 0.08) % 628.0;
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
            if self.dsp.close_to_tray {
                win32_close_active_window();
            } else {
                self.close_native_video_stage(cx);
                win32_force_quit();
                cx.quit();
            }
            return;
        }

        // 2. TOP BAR & SIDEBAR CHROME
        if self.ui.button(id!(toggle_left_btn)).clicked(actions) {
            let z = self.theme.ui_zoom.clamp(0.65, 1.40) as f64;
            self.left_sidebar_open = !self.left_sidebar_open;
            self.left_w_target = if self.left_sidebar_open {
                168.0 * z
            } else {
                0.0
            };
            if self.left_sidebar_open {
                self.ui.view(id!(left_sidebar)).set_visible(cx, true);
                self.ui.view(id!(left_divider)).set_visible(cx, true);
            }
            self.start_motion(cx);
            self.sync_video_bounds(cx);
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
            self.spawn_ytm_rich_home(cx, self.active_mood_idx, params, true);
            self.show_toast(cx, "Refreshing YouTube Music feed...");
        }
        if let Some(typed) = self.ui.text_input(id!(top_search_input)).changed(actions) {
            self.schedule_live_search(cx, typed, true);
        }
        if let Some((submitted, _)) = self.ui.text_input(id!(top_search_input)).returned(actions) {
            let q = submitted.trim().to_string();
            if !q.is_empty() {
                self.spawn_rich_search(cx, q);
            }
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

        // 3. HOME INTERACTIONS (Mood Chips, Quick Picks, 8 Dynamic Shelves, Endless Load More)
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
                self.spawn_ytm_rich_home(cx, i, params, i == 0);
            }
        }
        if self.ui.button(id!(home_retry_btn)).clicked(actions) {
            let params = if self.active_mood_idx > 0 {
                self.rich_home
                    .chips
                    .get(self.active_mood_idx - 1)
                    .map(|c| c.params.clone())
            } else {
                None
            };
            self.spawn_ytm_rich_home(cx, self.active_mood_idx, params, true);
        }
        if self.ui.button(id!(home_load_more_btn)).clicked(actions) {
            self.spawn_home_load_more(cx);
        }
        if self.ui.button(id!(home_shuffle_feed_btn)).clicked(actions) {
            self.spawn_ytm_rich_home(cx, 0, None, true);
            self.show_toast(cx, "Discovering fresh recommendations...");
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
                || v.finger_up(actions).is_some_and(|fe| is_desktop_click(&fe))
            {
                let pool = self.home_quick_picks_pool();
                let idx = self.qp_page * 8 + slot;
                if let Some(track) = pool.get(idx).cloned() {
                    if track.youtube_video_id.is_some() && self.session_state.autoplay_radio {
                        self.queue_context = format!("{} radio", track.title);
                        self.queue.set_tracks(vec![track.clone()], Some(0));
                        self.last_radio_seed_id = None;
                        self.play_track(cx, track.clone());
                        self.maybe_fetch_radio_for_track(&track, true);
                    } else {
                        self.queue_context = "Quick picks".to_string();
                        self.queue.set_tracks(pool, Some(idx));
                        self.play_track(cx, track);
                    }
                }
            }
        }

        // 8 Horizontal Shelves paging & clicks
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
            (
                3usize,
                id!(sh3_prev_btn),
                id!(sh3_next_btn),
                [
                    id!(sh3_c0),
                    id!(sh3_c1),
                    id!(sh3_c2),
                    id!(sh3_c3),
                    id!(sh3_c4),
                ],
            ),
            (
                4usize,
                id!(sh4_prev_btn),
                id!(sh4_next_btn),
                [
                    id!(sh4_c0),
                    id!(sh4_c1),
                    id!(sh4_c2),
                    id!(sh4_c3),
                    id!(sh4_c4),
                ],
            ),
            (
                5usize,
                id!(sh5_prev_btn),
                id!(sh5_next_btn),
                [
                    id!(sh5_c0),
                    id!(sh5_c1),
                    id!(sh5_c2),
                    id!(sh5_c3),
                    id!(sh5_c4),
                ],
            ),
            (
                6usize,
                id!(sh6_prev_btn),
                id!(sh6_next_btn),
                [
                    id!(sh6_c0),
                    id!(sh6_c1),
                    id!(sh6_c2),
                    id!(sh6_c3),
                    id!(sh6_c4),
                ],
            ),
            (
                7usize,
                id!(sh7_prev_btn),
                id!(sh7_next_btn),
                [
                    id!(sh7_c0),
                    id!(sh7_c1),
                    id!(sh7_c2),
                    id!(sh7_c3),
                    id!(sh7_c4),
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
                        .is_some_and(|fe| is_desktop_click(&fe))
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
        if let Some(typed) = self.ui.text_input(id!(search_page_input)).changed(actions) {
            self.schedule_live_search(cx, typed, false);
        }
        if self.ui.button(id!(search_go_btn)).clicked(actions) {
            let q = self.ui.text_input(id!(search_page_input)).text();
            self.spawn_rich_search(cx, q);
        }
        if let Some((submitted, _)) = self.ui.text_input(id!(search_page_input)).returned(actions) {
            self.spawn_rich_search(cx, submitted);
        }
        if self.ui.button(id!(search_clear_btn)).clicked(actions) {
            self.pending_live_search = None;
            self.last_dispatched_search.clear();
            self.search_request_seq = self.search_request_seq.wrapping_add(1);
            self.ui.text_input(id!(search_page_input)).set_text(cx, "");
            self.ui.text_input(id!(top_search_input)).set_text(cx, "");
            self.search_result = YtmRichSearch::default();
            self.search_tracks = Rc::new(Vec::new());
            self.ui
                .label(id!(search_status_lbl))
                .set_text(cx, "Instant YouTube Music + Local Search");
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
                    .is_some_and(|fe| is_desktop_click(&fe))
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
        if let Some(filter_txt) = self.ui.text_input(id!(songs_filter_input)).changed(actions) {
            self.songs_filter_query = filter_txt;
            self.rebuild_library_views(cx);
        }
        if self.ui.button(id!(songs_clear_demo_btn)).clicked(actions) {
            self.remove_all_demo_tracks(cx);
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

        // Albums grid paging + click (20 cards/page)
        if self.ui.button(id!(alb_prev_btn)).clicked(actions) && self.albums_page > 0 {
            self.albums_page -= 1;
            self.refresh_albums_grid(cx);
        }
        if self.ui.button(id!(alb_next_btn)).clicked(actions)
            && (self.albums_page + 1) * 20 < self.album_groups.len()
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
            id!(alb_8),
            id!(alb_9),
            id!(alb_10),
            id!(alb_11),
            id!(alb_12),
            id!(alb_13),
            id!(alb_14),
            id!(alb_15),
            id!(alb_16),
            id!(alb_17),
            id!(alb_18),
            id!(alb_19),
        ]
        .into_iter()
        .enumerate()
        {
            let v = self.ui.view(vid);
            if (v.ferric_cover_art(id!(art)).clicked(actions)
                || v.finger_up(actions).is_some_and(|fe| is_desktop_click(&fe)))
                && let Some(g) = self.album_groups.get(self.albums_page * 20 + i).cloned()
            {
                self.open_local_detail(cx, g);
            }
        }

        // Artists grid paging + click (20 cards/page)
        if self.ui.button(id!(art_prev_btn)).clicked(actions) && self.artists_page > 0 {
            self.artists_page -= 1;
            self.refresh_artists_grid(cx);
        }
        if self.ui.button(id!(art_next_btn)).clicked(actions)
            && (self.artists_page + 1) * 20 < self.artist_groups.len()
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
            id!(art_8),
            id!(art_9),
            id!(art_10),
            id!(art_11),
            id!(art_12),
            id!(art_13),
            id!(art_14),
            id!(art_15),
            id!(art_16),
            id!(art_17),
            id!(art_18),
            id!(art_19),
        ]
        .into_iter()
        .enumerate()
        {
            let v = self.ui.view(vid);
            if (v.ferric_cover_art(id!(art)).clicked(actions)
                || v.finger_up(actions).is_some_and(|fe| is_desktop_click(&fe)))
                && let Some(g) = self.artist_groups.get(self.artists_page * 20 + i).cloned()
            {
                self.open_local_detail(cx, g);
            }
        }

        // Playlists grid + Create + Import
        let create_pl_submitted = self
            .ui
            .text_input(id!(pl_new_input))
            .returned(actions)
            .is_some();
        if self.ui.button(id!(pl_create_btn)).clicked(actions) || create_pl_submitted {
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
            && (self.playlists_page + 1) * 20 < self.playlists.len()
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
            id!(plc_8),
            id!(plc_9),
            id!(plc_10),
            id!(plc_11),
            id!(plc_12),
            id!(plc_13),
            id!(plc_14),
            id!(plc_15),
            id!(plc_16),
            id!(plc_17),
            id!(plc_18),
            id!(plc_19),
        ]
        .into_iter()
        .enumerate()
        {
            let v = self.ui.view(vid);
            if (v.ferric_cover_art(id!(art)).clicked(actions)
                || v.finger_up(actions).is_some_and(|fe| is_desktop_click(&fe)))
                && let Some(pl) = self.playlists.get(self.playlists_page * 20 + i).cloned()
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
        if self.ui.button(id!(detail_export_btn)).clicked(actions) {
            self.export_detail_m3u(cx);
        }
        let rename_submitted = self
            .ui
            .text_input(id!(detail_rename_input))
            .returned(actions)
            .is_some();
        if (self.ui.button(id!(detail_rename_btn)).clicked(actions) || rename_submitted)
            && self.detail_state.kind == YtmCardKind::Playlist
            && !self.detail_state.id.is_empty()
        {
            let next_name = self
                .ui
                .text_input(id!(detail_rename_input))
                .text()
                .trim()
                .to_string();
            if !next_name.is_empty() && next_name != self.detail_state.title {
                let pl_id = self.detail_state.id.clone();
                if let Some(db) = &mut self.db {
                    let _ = db.rename_playlist(&pl_id, &next_name);
                    self.playlists = db.list_playlists().unwrap_or_default();
                }
                self.detail_state.title = next_name.clone();
                self.refresh_detail_hero(cx);
                self.refresh_playlists_grid(cx);
                self.rebuild_library_views(cx);
                self.show_toast(cx, &format!("Renamed playlist to '{next_name}'"));
            }
        }
        if self.ui.button(id!(detail_delete_btn)).clicked(actions)
            && self.detail_state.kind == YtmCardKind::Playlist
            && !self.detail_state.id.is_empty()
        {
            let pl_id = self.detail_state.id.clone();
            let pl_title = self.detail_state.title.clone();
            if let Some(db) = &mut self.db {
                let _ = db.delete_playlist(&pl_id);
                self.playlists = db.list_playlists().unwrap_or_default();
            }
            self.session_state.pinned_ids.retain(|id| id != &pl_id);
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.refresh_playlists_grid(cx);
            self.rebuild_library_views(cx);
            self.navigate_to(cx, MainPage::Playlists, false);
            self.show_toast(cx, &format!("Deleted playlist '{pl_title}'"));
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
        // TrackList emits WidgetAction via cx.widget_action(...), so unwrap via as_widget_action()!
        let active_list = self.active_track_list();
        for action in actions {
            match action.as_widget_action().cast() {
                TrackListAction::PlayIndex(idx) => {
                    if let Some(clicked) = active_list.get(idx).cloned() {
                        if self.active_page == MainPage::Search
                            && clicked.youtube_video_id.is_some()
                            && self.session_state.autoplay_radio
                        {
                            self.queue_context = format!("{} radio", clicked.title);
                            self.queue.set_tracks(vec![clicked.clone()], Some(0));
                            self.last_radio_seed_id = None;
                            self.play_track(cx, clicked.clone());
                            self.maybe_fetch_radio_for_track(&clicked, true);
                        } else {
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
                }
                TrackListAction::ToggleLike(idx) => {
                    if let Some(target) = active_list.get(idx).cloned() {
                        let next_liked = self.toggle_track_like(cx, &target);
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
                TrackListAction::AddToPlaylist(idx) | TrackListAction::ContextMenu(idx) => {
                    if let Some(target) = active_list.get(idx).cloned() {
                        self.open_track_menu(cx, target);
                    }
                }
                TrackListAction::RemoveTrack(idx) => {
                    if let Some(target) = active_list.get(idx).cloned() {
                        self.remove_track_contextually(cx, &target);
                    }
                }
                TrackListAction::LoadDiscoverFeed => {
                    self.spawn_ytm_rich_home(cx, 0, None, true);
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
        if self.ui.button(id!(rs_refresh_queue_btn)).clicked(actions) {
            let mut seed_state = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0x9E37_79B9_7F4A_7C15_u64, |d| d.as_nanos() as u64)
                | 1;
            let mut next_u64 = || {
                seed_state ^= seed_state << 13;
                seed_state ^= seed_state >> 7;
                seed_state ^= seed_state << 17;
                seed_state
            };

            if let Some(cur) = self.queue.current().cloned() {
                self.queue.set_tracks(vec![cur.clone()], Some(0));
                self.last_radio_seed_id = None;
                if cur.youtube_video_id.is_some() {
                    self.maybe_fetch_radio_for_track(&cur, true);
                    self.sync_next_deck_to_engine();
                    self.refresh_right_sidebar(cx);
                    self.show_toast(cx, "Refreshing Up next queue...");
                } else {
                    let mut pool: Vec<Track> = self
                        .home_quick_picks_pool()
                        .into_iter()
                        .filter(|t| t.id != cur.id)
                        .collect();
                    for i in (1..pool.len()).rev() {
                        let j = (next_u64() as usize) % (i + 1);
                        pool.swap(i, j);
                    }
                    let mut rebuilt = vec![cur];
                    rebuilt.extend(pool.into_iter().take(12));
                    self.queue.set_tracks(rebuilt, Some(0));
                    self.sync_next_deck_to_engine();
                    self.refresh_right_sidebar(cx);
                    self.show_toast(cx, "Up next queue refreshed");
                }
            } else {
                let mut pool = self.home_quick_picks_pool();
                for i in (1..pool.len()).rev() {
                    let j = (next_u64() as usize) % (i + 1);
                    pool.swap(i, j);
                }
                if !pool.is_empty() {
                    self.queue
                        .set_tracks(pool.into_iter().take(12).collect(), None);
                    self.sync_next_deck_to_engine();
                    self.refresh_right_sidebar(cx);
                    self.show_toast(cx, "Up next queue refreshed");
                } else {
                    self.show_toast(cx, "No tracks available to refresh queue");
                }
            }
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
                    .is_some_and(|fe| is_desktop_click(&fe)))
                && let Some(t) = hist_tracks.get(i).cloned()
            {
                self.queue.insert_play_next(t);
                if let Some(next) = self.queue.next_track().cloned() {
                    self.play_track(cx, next);
                }
            }
        }
        // Up next clicks in Right Sidebar (reorder up, remove, or jump directly!)
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
            if rv.button(id!(up_btn)).clicked(actions) {
                if self.queue.move_upcoming_up(i) {
                    self.sync_next_deck_to_engine();
                    self.refresh_right_sidebar(cx);
                }
                continue;
            }
            if rv.button(id!(rm_btn)).clicked(actions) {
                if let Some(removed) = self.queue.remove_upcoming(i) {
                    self.sync_next_deck_to_engine();
                    self.refresh_right_sidebar(cx);
                    self.show_toast(
                        cx,
                        &format!("Removed '{}' from queue", clip_text(&removed.title, 22)),
                    );
                }
                continue;
            }
            if (rv.ferric_cover_art(id!(art)).clicked(actions)
                || rv
                    .finger_up(actions)
                    .is_some_and(|fe| is_desktop_click(&fe)))
                && let Some(target) = self.queue.jump_to_upcoming(i).cloned()
            {
                self.play_track(cx, target);
            }
        }

        if self.ui.button(id!(np_add_pl_btn)).clicked(actions)
            && let Some(cur) = self.queue.current().cloned()
        {
            self.open_track_menu(cx, cur);
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
        if (self
            .ui
            .ferric_play_btn(id!(play_pause_btn))
            .clicked(actions)
            || self
                .ui
                .ferric_play_btn(id!(fs_play_pause_btn))
                .clicked(actions))
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
        if (self.ui.button(id!(next_btn)).clicked(actions)
            || self.ui.button(id!(fs_next_btn)).clicked(actions))
            && let Some(next) = self.queue.next_track().cloned()
        {
            self.play_track(cx, next);
        }
        if (self.ui.button(id!(prev_btn)).clicked(actions)
            || self.ui.button(id!(fs_prev_btn)).clicked(actions))
            && let Some(prev) = self.queue.prev_track().cloned()
        {
            self.play_track(cx, prev);
        }
        if self.ui.button(id!(shuffle_btn)).clicked(actions)
            || self.ui.button(id!(fs_shuffle_btn)).clicked(actions)
        {
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
        if self.ui.button(id!(repeat_btn)).clicked(actions)
            || self.ui.button(id!(fs_repeat_btn)).clicked(actions)
        {
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
            self.last_dur_txt.clear();
        }
        if (self.ui.button(id!(np_like_btn)).clicked(actions)
            || self.ui.button(id!(fs_like_btn)).clicked(actions))
            && let Some(cur) = self.queue.current().cloned()
        {
            let next_liked = self.toggle_track_like(cx, &cur);
            self.show_toast(
                cx,
                if next_liked {
                    "Added to Liked Songs"
                } else {
                    "Removed from Liked"
                },
            );
        }
        if let Some(pos_ratio) = self
            .ui
            .ferric_bar_slider(id!(seek_slider))
            .released(actions)
            .or_else(|| {
                self.ui
                    .ferric_bar_slider(id!(fs_seek_slider))
                    .released(actions)
            })
            && let Some(audio) = &self.audio
        {
            let snap = audio.telemetry();
            let dur = snap.duration_ms;
            if dur > 0 {
                let target_ms = (pos_ratio * dur as f64) as u64;
                audio.send(EngineCommand::SeekMs(target_ms));
                if self.video_open
                    && let Some(vp) = &self.video_player
                    && vp.is_open()
                {
                    vp.sync_playback(target_ms as f64 / 1000.0, snap.is_playing);
                }
            }
        }
        if let Some(vol) = self
            .ui
            .ferric_bar_slider(id!(vol_slider))
            .changed(actions)
            .or_else(|| {
                self.ui
                    .ferric_bar_slider(id!(fs_vol_slider))
                    .changed(actions)
            })
        {
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
        if self.ui.button(id!(vol_mute_btn)).clicked(actions)
            || self.ui.button(id!(fs_vol_mute_btn)).clicked(actions)
        {
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

        // Lyrics & Video Clip actions (Right Sidebar + Integrated Fullscreen Lyrics/Video Stage)
        if let Some(seek_ms) = self
            .ui
            .ferric_lyrics_view(id!(rs_lyrics_scroll))
            .seek_ms(actions)
            .or_else(|| {
                self.ui
                    .ferric_lyrics_view(id!(fs_lyrics_scroll))
                    .seek_ms(actions)
            })
            && let Some(audio) = &self.audio
        {
            let playing = audio.telemetry().is_playing;
            audio.send(EngineCommand::SeekMs(seek_ms));
            self.update_lyrics_display(cx, seek_ms);
            if self.video_open
                && let Some(vp) = &self.video_player
                && vp.is_open()
            {
                vp.sync_playback(seek_ms as f64 / 1000.0, playing);
            }
        }
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
        if self.ui.button(id!(bar_video_btn)).clicked(actions) {
            if self.video_open && self.fullscreen_open {
                self.close_native_video_stage(cx);
            } else {
                self.open_native_video_stage(cx);
            }
        }
        if self.ui.button(id!(rs_watch_video)).clicked(actions)
            || self.ui.button(id!(fs_mode_video_btn)).clicked(actions)
        {
            self.open_native_video_stage(cx);
        }
        if self.ui.button(id!(fs_mode_art_btn)).clicked(actions) {
            self.close_native_video_stage(cx);
        }
        for (btn, vq) in [
            (id!(vq_auto_btn), VideoQuality::Auto),
            (id!(vq_360_btn), VideoQuality::P360),
            (id!(vq_480_btn), VideoQuality::P480),
            (id!(vq_720_btn), VideoQuality::P720),
            (id!(vq_1080_btn), VideoQuality::P1080),
            (id!(svq_auto_btn), VideoQuality::Auto),
            (id!(svq_360_btn), VideoQuality::P360),
            (id!(svq_480_btn), VideoQuality::P480),
            (id!(svq_720_btn), VideoQuality::P720),
            (id!(svq_1080_btn), VideoQuality::P1080),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.dsp.video_quality = vq;
                self.apply_dsp_live(cx);
                self.refresh_video_quality_buttons(cx);
                if self.video_open {
                    self.load_selected_video_quality(cx);
                }
                self.show_toast(cx, &format!("Video quality: {}", vq.label()));
            }
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
            self.sync_video_bounds(cx);
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
            (id!(st_integrations_btn), SettingsTab::Integrations),
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
            self.spawn_ytm_rich_home(cx, 0, None, true);
        }
        if self.ui.button(id!(yt_logout_btn)).clicked(actions) {
            self.ytm_signed_out_recently = true;
            self.accounts.youtube_connected = false;
            self.accounts.youtube_cookie.clear();
            self.accounts.youtube_sapisid.clear();
            self.accounts.youtube_channel_name.clear();
            self.accounts.youtube_visitor_data.clear();
            self.accounts.youtube_playlist_count = 0;
            if let Some(db) = &mut self.db {
                let _ = db.save_accounts(&self.accounts);
                let _ = db.delete_account_synced_playlists();
                self.playlists = db.list_playlists().unwrap_or_default();
            } else {
                self.playlists
                    .retain(|pl| !pl.id.starts_with("ytm:pl:") && pl.id != "ytm:home:discover");
            }
            self.ui.label(id!(oauth_status_lbl)).set_text(
                cx,
                "Signed out. Synced account playlists removed; manual playlists kept.",
            );
            self.rebuild_library_views(cx);
            self.refresh_playlists_grid(cx);
            self.spawn_ytm_rich_home(cx, 0, None, true);
            self.show_toast(cx, "Signed out — Guest Mode active");
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
        for (btn, aq) in [
            (id!(aq_low_btn), AudioQuality::Low),
            (id!(aq_norm_btn), AudioQuality::Normal),
            (id!(aq_high_btn), AudioQuality::High),
            (id!(aq_best_btn), AudioQuality::Best),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.dsp.audio_quality = aq;
                self.sync_controls_from_state(cx);
                self.apply_dsp_live(cx);
                self.show_toast(cx, &format!("Audio quality: {}", aq.label()));
            }
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(close_to_tray_toggle))
            .toggled(actions)
        {
            self.dsp.close_to_tray = on;
            win32_set_close_to_tray(on);
            self.apply_dsp_live(cx);
            self.show_toast(
                cx,
                if on {
                    "Close (X) will keep Ferric running in System Tray"
                } else {
                    "Close (X) will exit Ferric"
                },
            );
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

        // Equalizer Tab (13 Studio Presets + AutoEq + APO)
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

        // Appearance Tab (AMOLED Black, Native Windows 11 Acrylic, Obsidian Studio, Warm Cream)
        if self.ui.button(id!(pal_amoled_btn)).clicked(actions) {
            let z = self.theme.ui_zoom;
            self.theme = ThemeConfig::pure_amoled();
            self.theme.ui_zoom = z;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(pal_acrylic_btn)).clicked(actions) {
            let z = self.theme.ui_zoom;
            self.theme = ThemeConfig::native_acrylic();
            self.theme.ui_zoom = z;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
            self.show_toast(cx, "Native Windows 11 Acrylic enabled");
        }
        if self.ui.button(id!(pal_obsidian_btn)).clicked(actions) {
            let z = self.theme.ui_zoom;
            self.theme = ThemeConfig::obsidian_slate();
            self.theme.ui_zoom = z;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(pal_cream_btn)).clicked(actions) {
            let z = self.theme.ui_zoom;
            self.theme = ThemeConfig::cream_light();
            self.theme.ui_zoom = z;
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(zoom_out_btn)).clicked(actions) {
            self.adjust_ui_zoom(cx, -0.10);
        }
        if self.ui.button(id!(zoom_in_btn)).clicked(actions) {
            self.adjust_ui_zoom(cx, 0.10);
        }
        if self.ui.button(id!(zoom_reset_btn)).clicked(actions) {
            self.set_ui_zoom(cx, 1.0);
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(adaptive_art_toggle))
            .toggled(actions)
        {
            self.theme.adaptive_album_art = on;
            self.apply_theme_live(cx);
        }
        if let Some(enabled) = self.ui.ferric_toggle(id!(acrylic_toggle)).toggled(actions) {
            self.theme.acrylic_enabled = enabled;
            if enabled {
                self.theme.name = "Native Acrylic".to_string();
            } else if self.theme.name == "Native Acrylic" {
                self.theme.name = "AMOLED Black".to_string();
            }
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if let Some(v) = self
            .ui
            .ferric_bar_slider(id!(acrylic_opacity_slider))
            .changed(actions)
        {
            self.theme.acrylic_opacity = (v as f32).clamp(0.18, 0.94);
            self.ui
                .label(id!(acrylic_opacity_lbl))
                .set_text(cx, &format!("{:.0}%", self.theme.acrylic_opacity * 100.0));
            self.apply_theme_live(cx);
        }
        for (btn, name, hex) in [
            (id!(tint_obsidian_btn), "Obsidian", "#0B0D14"),
            (id!(tint_ember_btn), "Ember", "#2B140A"),
            (id!(tint_copper_btn), "Copper", "#2E1A10"),
            (id!(tint_midnight_btn), "Midnight", "#0C182E"),
            (id!(tint_emerald_btn), "Emerald", "#0B2118"),
            (id!(tint_crimson_btn), "Crimson", "#290E16"),
            (id!(tint_slate_btn), "Slate", "#1D222E"),
            (id!(tint_smoke_btn), "Smoke", "#181818"),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.theme.acrylic_tint_hex = hex.to_string();
                if !self.theme.acrylic_enabled {
                    self.theme.acrylic_enabled = true;
                    self.theme.name = "Native Acrylic".to_string();
                }
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
                self.show_toast(cx, &format!("Acrylic tint: {name} ({hex})"));
            }
        }
        let apply_custom_tint = self.ui.button(id!(acrylic_tint_apply_btn)).clicked(actions)
            || self
                .ui
                .text_input(id!(acrylic_tint_input))
                .returned(actions)
                .is_some();
        if apply_custom_tint {
            let raw = self.ui.text_input(id!(acrylic_tint_input)).text();
            let clean = raw.trim().trim_start_matches('#');
            let formatted = format!("#{}", clean.to_ascii_uppercase());
            if parse_hex_rgb(&formatted).is_ok() {
                self.theme.acrylic_tint_hex = formatted.clone();
                if !self.theme.acrylic_enabled {
                    self.theme.acrylic_enabled = true;
                    self.theme.name = "Native Acrylic".to_string();
                }
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
                self.show_toast(cx, &format!("Custom Acrylic tint applied: {formatted}"));
            } else {
                self.show_toast(cx, "Invalid hex color — use #RRGGBB (e.g. #2B140A)");
            }
        }

        // Integrations Tab (Discord Rich Presence)
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(discord_enable_toggle))
            .toggled(actions)
        {
            self.discord_settings.enabled = on;
            self.apply_discord_settings(cx);
            self.show_toast(
                cx,
                if on {
                    if is_valid_client_id(&self.discord_settings.client_id) {
                        "Discord Rich Presence enabled"
                    } else {
                        "Discord RPC enabled — paste your Discord Application ID"
                    }
                } else {
                    "Discord Rich Presence disabled"
                },
            );
        }
        if self.ui.button(id!(discord_format_btn)).clicked(actions) {
            self.discord_settings.status_format = self.discord_settings.status_format.next();
            self.apply_discord_settings(cx);
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(discord_paused_toggle))
            .toggled(actions)
        {
            self.discord_settings.show_when_paused = on;
            self.apply_discord_settings(cx);
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(discord_cover_toggle))
            .toggled(actions)
        {
            self.discord_settings.show_cover_art = on;
            self.apply_discord_settings(cx);
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(discord_hide_details_toggle))
            .toggled(actions)
        {
            self.discord_settings.hide_details = on;
            self.apply_discord_settings(cx);
        }
        let save_discord_id = self.ui.button(id!(discord_save_id_btn)).clicked(actions)
            || self
                .ui
                .text_input(id!(discord_client_id_input))
                .returned(actions)
                .is_some();
        if save_discord_id {
            let raw = self.ui.text_input(id!(discord_client_id_input)).text();
            let trimmed = raw.trim().to_string();
            if trimmed.is_empty() || is_valid_client_id(&trimmed) {
                self.discord_settings.client_id = trimmed.clone();
                if !trimmed.is_empty() {
                    self.discord_settings.enabled = true;
                }
                self.apply_discord_settings(cx);
                self.show_toast(
                    cx,
                    if trimmed.is_empty() {
                        "Discord Application ID cleared"
                    } else {
                        "Discord Application ID saved — connecting..."
                    },
                );
            } else {
                self.show_toast(cx, "Invalid Discord Application ID (must be 17-22 digits)");
            }
        }
        if self.ui.button(id!(discord_portal_btn)).clicked(actions) {
            let _ = open_allowed_url("https://discord.com/developers/applications");
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
        if self.ui.button(id!(remove_demo_btn)).clicked(actions) {
            self.remove_all_demo_tracks(cx);
        }
        if self.ui.button(id!(clear_local_btn)).clicked(actions) {
            let removed = if let Some(db) = &mut self.db {
                let n = db.clear_music_folders_and_local_tracks().unwrap_or(0);
                self.all_tracks = db.list_tracks("").unwrap_or_default();
                self.playlists = db.list_playlists().unwrap_or_default();
                n
            } else {
                0
            };
            self.rebuild_library_views(cx);
            self.refresh_right_sidebar(cx);
            self.show_toast(
                cx,
                &format!("Cleared local folders ({removed} tracks removed)"),
            );
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

        // Track Actions & Add-to-Playlist Popover Modal (Right-Click / +)
        if self.ui.button(id!(tm_close_btn)).clicked(actions) {
            self.close_track_menu(cx);
        }
        if let Some(target) = self.track_menu_target.clone() {
            if self.ui.button(id!(tm_play_btn)).clicked(actions) {
                self.close_track_menu(cx);
                self.queue_context = format!("{} radio", target.title);
                self.queue.set_tracks(vec![target.clone()], Some(0));
                self.last_radio_seed_id = None;
                self.play_track(cx, target.clone());
                if self.session_state.autoplay_radio {
                    self.maybe_fetch_radio_for_track(&target, true);
                }
            } else if self.ui.button(id!(tm_play_next_btn)).clicked(actions) {
                self.close_track_menu(cx);
                let title = clip_text(&target.title, 22);
                self.queue.insert_play_next(target);
                self.sync_next_deck_to_engine();
                self.prefetch_next_online_track();
                self.refresh_right_sidebar(cx);
                self.show_toast(cx, &format!("Playing next: '{title}'"));
            } else if self.ui.button(id!(tm_add_queue_btn)).clicked(actions) {
                self.close_track_menu(cx);
                let title = clip_text(&target.title, 22);
                self.queue.append_single_to_end(target);
                self.sync_next_deck_to_engine();
                self.refresh_right_sidebar(cx);
                self.show_toast(cx, &format!("Added '{title}' to end of queue"));
            } else if self.ui.button(id!(tm_radio_btn)).clicked(actions) {
                self.close_track_menu(cx);
                self.queue_context = format!("{} radio", target.title);
                self.queue.set_tracks(vec![target.clone()], Some(0));
                self.last_radio_seed_id = None;
                self.play_track(cx, target.clone());
                self.maybe_fetch_radio_for_track(&target, true);
                self.show_toast(
                    cx,
                    &format!("Started radio for '{}'", clip_text(&target.title, 22)),
                );
            } else if self.ui.button(id!(tm_like_btn)).clicked(actions) {
                let next_liked = self.toggle_track_like(cx, &target);
                self.close_track_menu(cx);
                self.show_toast(
                    cx,
                    if next_liked {
                        "Added to Liked Songs"
                    } else {
                        "Removed from Liked"
                    },
                );
            } else if self.ui.button(id!(tm_artist_btn)).clicked(actions) {
                let artist = target.artist.clone();
                self.close_track_menu(cx);
                if let Some(g) = self
                    .artist_groups
                    .iter()
                    .find(|g| g.title.eq_ignore_ascii_case(&artist))
                    .cloned()
                {
                    self.open_local_detail(cx, g);
                } else if !artist.trim().is_empty() {
                    self.spawn_rich_search(cx, artist);
                }
            } else if self.ui.button(id!(tm_album_btn)).clicked(actions) {
                let album = target.album.clone();
                let query = if album.is_empty()
                    || album.eq_ignore_ascii_case("single")
                    || album.eq_ignore_ascii_case("youtube music")
                {
                    format!("{} {}", target.artist, target.title)
                } else {
                    format!("{} {}", target.artist, album)
                };
                self.close_track_menu(cx);
                if let Some(g) = self
                    .album_groups
                    .iter()
                    .find(|g| g.title.eq_ignore_ascii_case(&album))
                    .cloned()
                {
                    self.open_local_detail(cx, g);
                } else {
                    self.spawn_rich_search(cx, query);
                }
            } else if self.ui.button(id!(tm_copy_btn)).clicked(actions) {
                let info = if let Some(vid) = &target.youtube_video_id {
                    format!(
                        "{} - {} (https://music.youtube.com/watch?v={vid})",
                        target.artist, target.title
                    )
                } else {
                    format!("{} - {}", target.artist, target.title)
                };
                let _ = copy_text_to_clipboard(&info);
                self.close_track_menu(cx);
                self.show_toast(cx, "Copied track info to clipboard");
            } else if self.ui.button(id!(tm_remove_btn)).clicked(actions) {
                self.close_track_menu(cx);
                self.remove_track_contextually(cx, &target);
            } else {
                let mut handled_pl = false;
                for (i, pid) in [
                    id!(tm_pl_0),
                    id!(tm_pl_1),
                    id!(tm_pl_2),
                    id!(tm_pl_3),
                    id!(tm_pl_4),
                    id!(tm_pl_5),
                ]
                .into_iter()
                .enumerate()
                {
                    if self.ui.button(pid).clicked(actions)
                        && let Some(pl) = self.playlists.get(i).cloned()
                    {
                        self.add_track_to_saved_playlist(cx, &pl.id, &pl.name, &target);
                        self.close_track_menu(cx);
                        handled_pl = true;
                        break;
                    }
                }
                let new_pl_enter = self
                    .ui
                    .text_input(id!(tm_new_pl_input))
                    .returned(actions)
                    .is_some();
                if !handled_pl
                    && (self.ui.button(id!(tm_new_pl_btn)).clicked(actions) || new_pl_enter)
                {
                    let name = self
                        .ui
                        .text_input(id!(tm_new_pl_input))
                        .text()
                        .trim()
                        .to_string();
                    if !name.is_empty() {
                        let pl = Playlist {
                            id: format!("local:pl:{}", fastrand::u32(..)),
                            name: name.clone(),
                            source: TrackSource::Local,
                            etag: None,
                            track_count: 1,
                            description: "Custom playlist".to_string(),
                            thumbnail_url: target.thumbnail_url.clone(),
                        };
                        if let Some(db) = &mut self.db {
                            let _ = db.upsert_tracks(std::slice::from_ref(&target));
                            let _ = db.upsert_playlist(&pl, std::slice::from_ref(&target.id));
                            self.all_tracks = db.list_tracks("").unwrap_or_default();
                            self.playlists = db.list_playlists().unwrap_or_default();
                        }
                        self.refresh_playlists_grid(cx);
                        self.rebuild_library_views(cx);
                        self.close_track_menu(cx);
                        self.show_toast(
                            cx,
                            &format!("Created '{name}' with '{}'", clip_text(&target.title, 20)),
                        );
                    }
                }
            }
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
            self.sync_video_bounds(cx);
        }

        // Native Windows 11 Frameless Drag & Aero Snap on empty regions of top_bar
        // (never hijack clicks over Fullscreen Lyrics' Close (Esc) button!)
        let top_bar_h = 36.0 * self.theme.ui_zoom.clamp(0.65, 1.40) as f64;
        if let Event::WindowDragQuery(dq) = event
            && dq.abs.y >= 0.0
            && dq.abs.y <= top_bar_h
        {
            if self.fullscreen_open {
                let close_r = self.ui.button(id!(fs_close_btn)).area().rect(cx);
                let near_close = close_r.size.x > 0.0
                    && dq.abs.x >= close_r.pos.x - 20.0
                    && dq.abs.x <= close_r.pos.x + close_r.size.x + 20.0;
                if !near_close {
                    dq.response.set(WindowDragQueryResponse::Caption);
                    return;
                }
            } else {
                let left_r = self.ui.view(id!(top_left_grp)).area().rect(cx);
                let pill_r = self.ui.view(id!(top_search_box)).area().rect(cx);
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
        }

        // Scroll wheel over speaker mute icon also adjusts volume smoothly (+/- 5%)
        if let Event::Scroll(se) = event
            && !se.handled_y.get()
            && se.scroll.y.abs() > 0.01
        {
            let on_bottom_vol = self
                .ui
                .button(id!(vol_mute_btn))
                .area()
                .rect(cx)
                .contains(se.abs);
            let on_fs_vol = self.fullscreen_open
                && self
                    .ui
                    .button(id!(fs_vol_mute_btn))
                    .area()
                    .rect(cx)
                    .contains(se.abs);
            if on_bottom_vol || on_fs_vol {
                se.handled_y.set(true);
                let delta = if se.scroll.y < 0.0 {
                    0.05_f32
                } else {
                    -0.05_f32
                };
                self.session_state.volume = (self.session_state.volume + delta).clamp(0.0, 1.0);
                if self.session_state.volume > 0.02 {
                    self.pre_mute_volume = self.session_state.volume;
                }
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::SetVolume(self.session_state.volume));
                }
                if let Some(db) = &self.db {
                    let _ = db.save_session_state(&self.session_state);
                }
                self.sync_controls_from_state(cx);
                return;
            }
        }

        if let Some(nf) = self.motion_frame.is_event(event) {
            self.step_motion(cx, nf.time);
        }

        if self.tick_timer.is_event(event).is_some() {
            self.handle_tick(cx);
        }

        // Keyboard Shortcuts (including Ctrl+ / Ctrl- / Ctrl+0 UI Zoom)
        if let Event::KeyDown(ke) = event {
            if ke.modifiers.control && matches!(ke.key_code, KeyCode::Equals | KeyCode::NumpadAdd) {
                self.adjust_ui_zoom(cx, 0.10);
                return;
            }
            if ke.modifiers.control
                && matches!(ke.key_code, KeyCode::Minus | KeyCode::NumpadSubtract)
            {
                self.adjust_ui_zoom(cx, -0.10);
                return;
            }
            if ke.modifiers.control && matches!(ke.key_code, KeyCode::Key0 | KeyCode::Numpad0) {
                self.set_ui_zoom(cx, 1.0);
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::KeyK {
                if self.active_page == MainPage::Search {
                    self.ui.widget(id!(search_page_input)).set_key_focus(cx);
                } else {
                    self.ui.widget(id!(top_search_input)).set_key_focus(cx);
                }
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
            let typing = self.is_text_input_focused(cx);
            if !typing && !ke.modifiers.control && !ke.modifiers.alt && !ke.modifiers.shift {
                if ke.key_code == KeyCode::Space
                    && !self.cmdk_open
                    && !self.track_menu_open
                    && let Some(audio) = &self.audio
                {
                    audio.send(EngineCommand::TogglePlayPause);
                    return;
                }
                if ke.key_code == KeyCode::ArrowRight
                    && let Some(audio) = &self.audio
                {
                    let snap = audio.telemetry();
                    let next_ms = snap.position_ms.saturating_add(5_000).min(snap.duration_ms);
                    audio.send(EngineCommand::SeekMs(next_ms));
                    self.update_lyrics_display(cx, next_ms);
                    return;
                }
                if ke.key_code == KeyCode::ArrowLeft
                    && let Some(audio) = &self.audio
                {
                    let snap = audio.telemetry();
                    let next_ms = snap.position_ms.saturating_sub(5_000);
                    audio.send(EngineCommand::SeekMs(next_ms));
                    self.update_lyrics_display(cx, next_ms);
                    return;
                }
                if ke.key_code == KeyCode::KeyM {
                    if self.session_state.volume > 0.01 {
                        self.pre_mute_volume = self.session_state.volume;
                        self.session_state.volume = 0.0;
                    } else {
                        self.session_state.volume = self.pre_mute_volume.max(0.5);
                    }
                    if let Some(audio) = &self.audio {
                        audio.send(EngineCommand::SetVolume(self.session_state.volume));
                    }
                    self.sync_controls_from_state(cx);
                    return;
                }
                if ke.key_code == KeyCode::KeyS {
                    let next_s = !self.session_state.shuffle;
                    self.session_state.shuffle = next_s;
                    self.queue.set_shuffle(next_s);
                    self.sync_controls_from_state(cx);
                    self.refresh_right_sidebar(cx);
                    self.show_toast(cx, if next_s { "Shuffle On" } else { "Shuffle Off" });
                    return;
                }
                if ke.key_code == KeyCode::KeyR {
                    let next_r = match self.session_state.repeat {
                        RepeatMode::Off => RepeatMode::All,
                        RepeatMode::All => RepeatMode::One,
                        RepeatMode::One => RepeatMode::Off,
                    };
                    self.session_state.repeat = next_r;
                    self.queue.repeat = next_r;
                    self.sync_controls_from_state(cx);
                    self.show_toast(
                        cx,
                        match next_r {
                            RepeatMode::Off => "Repeat Off",
                            RepeatMode::All => "Repeat All",
                            RepeatMode::One => "Repeat One",
                        },
                    );
                    return;
                }
                if ke.key_code == KeyCode::KeyL
                    && let Some(cur) = self.queue.current().cloned()
                {
                    let next_liked = self.toggle_track_like(cx, &cur);
                    self.show_toast(
                        cx,
                        if next_liked {
                            "Added to Liked Songs"
                        } else {
                            "Removed from Liked"
                        },
                    );
                    return;
                }
                if ke.key_code == KeyCode::KeyF && !self.track_menu_open {
                    self.set_fullscreen(cx, !self.fullscreen_open);
                    return;
                }
                if ke.key_code == KeyCode::KeyQ && !self.fullscreen_open {
                    self.set_right_sidebar(cx, !self.right_sidebar_open, false);
                    return;
                }
            }
            if ke.key_code == KeyCode::Escape {
                if self.track_menu_open {
                    self.close_track_menu(cx);
                } else if self.cmdk_open {
                    self.cmdk_open = false;
                    self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
                    self.sync_video_bounds(cx);
                    self.ui.redraw(cx);
                } else if self.video_open {
                    self.close_native_video_stage(cx);
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
        self.missing_art_queue.borrow_mut().clear();
        let props = TrackListScope {
            tracks: self.active_track_list(),
            active_track_id: self.queue.current().map(|t| t.id.clone()),
            is_playing: snap_opt.is_some_and(|s| s.is_playing),
            audio_level: avg_lvl,
            accent: self.current_accent(),
            acrylic_on: if self.theme.acrylic_enabled { 1.0 } else { 0.0 },
            ui_zoom: self.theme.ui_zoom.clamp(0.65, 1.40),
            artworks: Rc::clone(&self.artwork_snapshot),
            missing_art_tracks: Rc::clone(&self.missing_art_queue),
        };
        {
            let mut scope = Scope::with_props(&props);
            self.ui.handle_event(cx, event, &mut scope);
        }

        // Drain lazily requested artwork for scrolled TrackList rows beyond initial batch
        let lazy_tracks: Vec<Track> = self.missing_art_queue.borrow_mut().drain(..).collect();
        if !lazy_tracks.is_empty()
            && let Some(cache) = &mut self.artwork
        {
            for t in &lazy_tracks {
                let _ = cache.request_for_track(t);
            }
        }

        if let Event::Actions(actions) = event {
            self.handle_actions(cx, actions);
        }
    }
}

fn format_compact_time_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins}:{secs:02}")
}

fn is_desktop_click(fe: &FingerUpEvent) -> bool {
    fe.is_over && (fe.abs - fe.abs_start).length() < 16.0
}

fn clean_display_text(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|&c| {
            let cp = c as u32;
            !matches!(
                cp,
                0x200D | 0xFE00..=0xFE0F | 0x2600..=0x27BF | 0x1F000..=0x1FFFF
            )
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
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
