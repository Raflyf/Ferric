use crate::artwork::{ArtworkCache, CachedArtwork, decode_and_downscale_image};
use crate::discord_rpc::{
    DiscordRpcBridge, DiscordTrackSnapshot, effective_client_id, is_valid_client_id,
};
use crate::google_login::{
    GoogleLoginOutcome, SpotifyLoginOutcome, launch_google_ytm_login_window,
    launch_spotify_login_window,
};
use crate::shell::{
    TrayEvent, copy_text_to_clipboard, launch_verified_installer, open_allowed_url,
    open_github_url, reveal_file_in_explorer, win32_client_rect_screen, win32_close_active_window,
    win32_force_quit, win32_install_window_hooks, win32_is_active_window_maximized,
    win32_is_window_hidden_or_minimized, win32_main_hwnd_raw, win32_minimize_active_window,
    win32_poll_tray_events, win32_set_close_to_tray, win32_set_native_acrylic,
    win32_toggle_maximize_active_window, win32_trim_working_set, win32_update_taskbar_playback,
    win32_update_tray_tooltip,
};
use crate::smtc::{SmtcBridge, SmtcCommand};
use crate::state::{
    BgMessage, CollectionGroup, DetailSortOrder, DetailViewState, LibraryFilter, MainPage,
    NavEntry, NavHistory, PlaylistSortOrder, PlaylistSourceFilter, SearchFilter, SettingsTab,
    SleepTimerMode, SortField, StationTracksPayload, StreamReadyPayload, build_album_groups,
    build_artist_groups, clip_text, cmp_ascii_icase, collection_share_url, contains_ascii_icase,
    deduplicate_tracks_in_order, dir_size_bytes, format_bytes_mb, format_collection_csv,
    format_collection_tracklist, track_primary_link, track_youtube_link,
};
use crate::track_list::{TrackListAction, TrackListScope, TrackListWidgetRefExt};
use crate::video_player::{NativeVideoPlayer, VideoViewportRect};
use crate::widgets::*;
use crate::{startup_load_demo, startup_page_override, startup_play_first};
use ferric_audio::demo_synth::ensure_demo_tracks;
use ferric_audio::engine::{AudioEngine, EngineCommand, list_output_devices};
use ferric_core::app_data_dir;
use ferric_core::db::LibraryDb;
use ferric_core::eq::EqConfig;
use ferric_core::models::{
    AccountSettings, AudioQuality, DiscordRpcSettings, DiscordStatusFormat, FadeCurve,
    PlaybackDspSettings, Playlist, RepeatMode, SessionResumeState, Track, TrackSource,
    UpdateSettings, VideoQuality, format_duration_ms, parse_playlist_import_text,
    primary_artist_key,
};
use ferric_core::queue::PlayQueue;
use ferric_core::secrets::csprng_hex;
use ferric_core::theme::{
    AdaptivePalette, ThemeConfig, derive_acrylic_tint_from_accent, hex_to_rgba_f32, parse_hex_rgb,
};
use ferric_providers::autoeq::search_autoeq_profiles;
use ferric_providers::local_scanner::scan_local_folder;
use ferric_providers::lyrics::{
    SyncedLyrics, fetch_best_free_lyrics, is_polluted_multilingual_lrc,
};
use ferric_providers::oauth::sync_spotify_catalog_with_snapshots;
use ferric_providers::public_sync::{
    import_spotify_public_url, import_youtube_public_url, parse_spotify_target,
    parse_youtube_target,
};
use ferric_providers::spotify_web::{
    add_tracks_to_spotify_playlist, create_spotify_playlist, fetch_spotify_playlist_by_id,
    fetch_spotify_recommended_stations, remove_tracks_from_spotify_playlist,
    rename_spotify_playlist, replace_spotify_playlist_tracks, search_spotify_track_uri,
    set_spotify_track_liked, unfollow_spotify_playlist,
};
use ferric_providers::subsonic::{SubsonicCredentials, sync_subsonic_catalog};
use ferric_providers::updater::{
    DownloadEvent, ReleaseInfo, check_latest_release, download_verified_installer,
    format_check_status, release_page_url,
};
use ferric_providers::ytmusic::{
    RecommendedStation, VideoStreamOption, YtmCardKind, YtmHomeTasteProfile, YtmRichHome,
    YtmRichSearch, YtmSession, YtmShelfCard, add_videos_to_ytm_playlist,
    build_algorithmic_recommended_stations_rotated, create_ytm_playlist, delete_ytm_playlist,
    expand_recommended_station_tracks, fetch_account_library, fetch_browse_detail,
    fetch_home_continuation, fetch_radio_up_next_for_track, fetch_rich_home, fetch_rich_search,
    normalize_ytm_thumbnail_url, remove_video_from_ytm_playlist, rename_ytm_playlist,
    resolve_and_cache_audio_stream_with_quality, resolve_best_video_id_for_track,
    resolve_music_video_streams, set_ytm_track_liked,
};
use ferric_sync::{
    ConnectConfig, ConnectEngine, ConnectEvent, ConnectSnapshot, DeviceKind, LocalPlaybackSnapshot,
    QrMatrix, QueueEntry, RemoteAction, detect_local_lan_ip,
};
use makepad_widgets::*;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                if (p.y < 1.0) {
                    if (self.acrylic_on > 0.5) {
                        let line_dark = vec4(1.0, 1.0, 1.0, 0.14);
                        let line_light = vec4(0.05, 0.06, 0.10, 0.18);
                        return mix(line_dark, line_light, self.light_mode);
                    }
                    return mix(#181A22, #D5CCB9, self.light_mode);
                }
                let d_left = length((self.pos - vec2(0.08, 0.55)) * vec2(1.4, 2.2));
                let d_mid = length((self.pos - vec2(0.48, 0.85)) * vec2(0.9, 2.4));
                let wash = (exp(-d_left * d_left * 2.4) * 0.55 + exp(-d_mid * d_mid * 1.8) * 0.35) * self.glow_amt * mix(1.0, 0.28, self.light_mode);
                if (self.acrylic_on > 0.5) {
                    let a = clamp(self.acrylic_opacity * 0.62, 0.20, 0.80);
                    let base_t = mix(self.acrylic_tint.xyz * 1.12, self.acrylic_tint.xyz * 0.94, self.light_mode);
                    let rgb = clamp(base_t + self.accent_col.xyz * wash, 0.0, 1.0);
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
            uniform card_col: #0D0E13
            uniform border_col: #1D1F29
            uniform tint_col: #0B0D14
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 8.0);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let solid_fill = mix(self.card_col, vec4(0.918, 0.890, 0.835, 1.0), self.light_mode);
                let acr_tint_rgb = clamp(self.tint_col.xyz * 1.40 + vec3(0.08, 0.08, 0.08), 0.0, 1.0);
                let acr_rgb = mix(ink, acr_tint_rgb, 0.42 * (1.0 - self.light_mode));
                let acrylic_fill = vec4(acr_rgb, 0.075 + 0.035 * self.light_mode);
                sdf.fill_keep(mix(solid_fill, acrylic_fill, self.acrylic_on));
                let solid_border = mix(self.border_col, vec4(0.80, 0.76, 0.69, 1.0), self.light_mode);
                let acrylic_border = vec4(ink, 0.085 + 0.085 * self.light_mode);
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
            uniform accent_col: #E86A2C
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 6.0);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let hov = mix(mix(#161822, vec4(0.05, 0.06, 0.10, 0.07), self.light_mode), vec4(ink, 0.085), self.acrylic_on);
                let act_solid = mix(mix(#1A1C28, vec4(0.05, 0.06, 0.10, 0.08), self.light_mode), vec4(self.accent_col.xyz, 0.24), 0.28);
                let act_acr = vec4(mix(ink, self.accent_col.xyz, 0.34), 0.16);
                let act = mix(act_solid, act_acr, self.acrylic_on);
                let col = mix(mix(vec4(0.0, 0.0, 0.0, 0.0), hov, self.hover), act, self.active);
                sdf.fill_keep(col);
                let idle_rim = vec4(ink, 0.06 * self.hover * self.acrylic_on);
                let act_rim = vec4(self.accent_col.xyz, 0.42);
                sdf.stroke(mix(idle_rim, act_rim, self.active), 1.0);
                if (self.active > 0.5) {
                    sdf.box(2.5, 6.0, 2.5, max(self.rect_size.y - 12.0, 6.0), 0.60);
                    sdf.fill(vec4(self.accent_col.xyz, self.active));
                }
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
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5;
                let r = (self.rect_size.y - inset * 2.0) * 0.25;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, r);
                let c_dark = mix(mix(#F2F3F7, #FFFFFF, self.hover), #D6D8E4, self.down);
                let c_light = mix(mix(#12151F, #232838, self.hover), #090B10, self.down);
                sdf.fill(mix(c_dark, c_light, self.light_mode));
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.18 } }
                    apply: { draw_bg: { down: 0.0, hover: 0.0 }, draw_icon: { down: 0.0, hover: 0.0 }, draw_text: { down: 0.0, hover: 0.0 } }
                }
                on = {
                    from: { all: Forward { duration: 0.14 } }
                    apply: { draw_bg: { down: 0.0, hover: 1.0 }, draw_icon: { down: 0.0, hover: 1.0 }, draw_text: { down: 0.0, hover: 1.0 } }
                }
                down = {
                    from: { all: Forward { duration: 0.08 } }
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
            uniform surface_col: #14151D
            uniform border_col: #252734
            uniform tint_col: #0B0D14
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5;
                let bw = self.rect_size.x - inset * 2.0;
                let bh = self.rect_size.y - inset * 2.0;
                let br = min(6.0, min(bw, bh) * 0.24);
                sdf.box(inset, inset, bw, bh, br);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let hov_dark = vec4(clamp(self.surface_col.xyz + vec3(0.065, 0.068, 0.085), 0.0, 1.0), 1.0);
                let solid_idle = mix(mix(self.surface_col, hov_dark, self.hover), mix(#E4DDD0, #D9D0C0, self.hover), self.light_mode);
                let solid_act = mix(mix(vec4(clamp(self.surface_col.xyz * 0.8 + self.accent_col.xyz * 0.18, 0.0, 1.0), 1.0), vec4(self.accent_col.xyz, 0.16), self.light_mode), vec4(self.accent_col.xyz, 0.28), 0.36);
                let acr_tint_rgb = clamp(self.tint_col.xyz * 1.35 + vec3(0.10, 0.10, 0.10), 0.0, 1.0);
                let acr_rgb = mix(ink, acr_tint_rgb, 0.36 * (1.0 - self.light_mode));
                let acrylic_idle = mix(vec4(acr_rgb, 0.095 + 0.025 * self.light_mode), vec4(acr_rgb, 0.19), self.hover);
                let acrylic_act = vec4(self.accent_col.xyz, 0.26);
                let bg = mix(mix(solid_idle, solid_act, self.active), mix(acrylic_idle, acrylic_act, self.active), self.acrylic_on);
                let pressed = mix(bg, vec4(ink, 0.20), self.down * 0.20);
                sdf.fill_keep(pressed);
                let b_dark_hov = vec4(clamp(self.border_col.xyz + vec3(0.13, 0.13, 0.16), 0.0, 1.0), 1.0);
                let solid_b = mix(mix(mix(self.border_col, b_dark_hov, self.hover), mix(#C8BFA8, #A89E88, self.hover), self.light_mode), vec4(self.accent_col.xyz, 0.88), self.active);
                let acrylic_b = mix(vec4(ink, 0.12 + 0.07 * self.light_mode + 0.12 * self.hover), vec4(self.accent_col.xyz, 0.90), self.active);
                sdf.stroke(mix(solid_b, acrylic_b, self.acrylic_on), 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.18 } }
                    apply: { draw_bg: { down: 0.0, hover: 0.0 }, draw_icon: { down: 0.0, hover: 0.0 }, draw_text: { down: 0.0, hover: 0.0 } }
                }
                on = {
                    from: { all: Forward { duration: 0.14 } }
                    apply: { draw_bg: { down: 0.0, hover: 1.0 }, draw_icon: { down: 0.0, hover: 1.0 }, draw_text: { down: 0.0, hover: 1.0 } }
                }
                down = {
                    from: { all: Forward { duration: 0.08 } }
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
            uniform light_mode: 0.0
            color: #C8CAD6,
            color_hover: #FFFFFF,
            color_focus: #FFFFFF,
            color_down: #FFFFFF,
            text_style: <THEME_FONT_MEDIUM> { font_size: 8.5 }
            fn get_color(self) -> vec4 {
                let hov_c = mix(#FFFFFF, #06070B, self.light_mode);
                let act_c = mix(#09090D, #FFFFFF, self.light_mode);
                let idle_c = mix(self.color, hov_c, max(self.hover, self.down));
                return mix(idle_c, act_c, self.active);
            }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            uniform acrylic_on: 0.0
            uniform surface_col: #14151D
            uniform border_col: #242633
            uniform tint_col: #0B0D14
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5;
                let r = (self.rect_size.y - inset * 2.0) * 0.25;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, r);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let hov_dark = vec4(clamp(self.surface_col.xyz + vec3(0.068, 0.072, 0.090), 0.0, 1.0), 1.0);
                let solid_idle = mix(mix(self.surface_col, hov_dark, self.hover), mix(#E4DDD0, #D8CFC0, self.hover), self.light_mode);
                let acr_tint_rgb = clamp(self.tint_col.xyz * 1.35 + vec3(0.10, 0.10, 0.10), 0.0, 1.0);
                let acr_rgb = mix(ink, acr_tint_rgb, 0.36 * (1.0 - self.light_mode));
                let acrylic_idle = mix(vec4(acr_rgb, 0.085 + 0.025 * self.light_mode), vec4(acr_rgb, 0.19), self.hover);
                let idle = mix(solid_idle, acrylic_idle, self.acrylic_on);
                let act_dark = mix(#F4F5FA, #FFFFFF, self.hover);
                let act_light = mix(#11131A, #222634, self.hover);
                let bg = mix(idle, mix(act_dark, act_light, self.light_mode), self.active);
                sdf.fill_keep(bg);
                let b_dark_hov = vec4(clamp(self.border_col.xyz + vec3(0.13, 0.13, 0.16), 0.0, 1.0), 1.0);
                let idle_b = mix(mix(mix(self.border_col, b_dark_hov, self.hover), mix(#C8BFA8, #A89E88, self.hover), self.light_mode), vec4(ink, 0.12 + 0.08 * self.light_mode + 0.12 * self.hover), self.acrylic_on);
                let act_b = mix(#FFFFFF, #11131A, self.light_mode);
                sdf.stroke(mix(idle_b, act_b, self.active), 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.18 } }
                    apply: { draw_bg: { down: 0.0, hover: 0.0 }, draw_text: { down: 0.0, hover: 0.0 } }
                }
                on = {
                    from: { all: Forward { duration: 0.14 } }
                    apply: { draw_bg: { down: 0.0, hover: 1.0 }, draw_text: { down: 0.0, hover: 1.0 } }
                }
                down = {
                    from: { all: Forward { duration: 0.08 } }
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
            instance pulse: 0.0
            instance pulse_kind: 0.0
            curve: 1.15,
            linearize: 0.08,
            brightness: 1.05,
            color: #9A9CA8,
            color_hover: #F4F4F7,
            color_focus: #F4F4F7,
            color_down: #E86A2C
            fn pixel(self) -> vec4 {
                let t = clamp(self.pulse, 0.0, 1.0);
                let scale = 1.0 - 0.03 * self.down;
                let mut ang = 0.0;
                if (t > 0.001 && t < 0.999 && self.pulse_kind > 0.5 && self.pulse_kind < 1.5) {
                    // Refresh: silky monotonic 180-deg cubic ease-out spin (constant scale, zero bounce)
                    let ease = 1.0 - pow(1.0 - t, 3.0);
                    ang = (1.0 - ease) * 3.14159265;
                }
                let uv_c = (self.pos - vec2(0.5, 0.5)) / max(scale, 0.45);
                let ca = cos(ang);
                let sa = sin(ang);
                let rot = vec2(uv_c.x * ca - uv_c.y * sa, uv_c.x * sa + uv_c.y * ca) + vec2(0.5, 0.5);
                if (rot.x < 0.0 || rot.x > 1.0 || rot.y < 0.0 || rot.y > 1.0) {
                    return vec4(0.0, 0.0, 0.0, 0.0);
                }
                let tc = mix(self.icon_t1.xy, self.icon_t2.xy, rot);
                let s = pow(sample2d_rt(self.tex, tc).x, self.u_curve);
                let col = self.get_color();
                return vec4(s * col.rgb * self.u_brightness * col.a, s * col.a);
            }
        }
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            instance pulse: 0.0
            uniform acrylic_on: 0.0
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                let inset = 1.0 + self.down * 0.25;
                let bw = self.rect_size.x - inset * 2.0;
                let bh = self.rect_size.y - inset * 2.0;
                let br = min(6.0, min(bw, bh) * 0.24);
                sdf.box(inset, inset, bw, bh, br);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let hov = mix(mix(#212432, vec4(0.05, 0.06, 0.10, 0.09), self.light_mode), vec4(ink, 0.13), self.acrylic_on);
                let act = mix(mix(#321E14, vec4(0.05, 0.06, 0.10, 0.14), self.light_mode), vec4(ink, 0.18), self.acrylic_on);
                let bg = mix(mix(vec4(0.0, 0.0, 0.0, 0.0), hov, self.hover), act, self.active);
                let pt = clamp(self.pulse, 0.0, 1.0);
                let mut soft_glow = 0.0;
                if (pt > 0.001 && pt < 0.999) {
                    soft_glow = (1.0 - pt) * (1.0 - pt) * 0.10;
                }
                sdf.fill_keep(mix(bg, vec4(ink, 0.18), self.down * 0.20 + soft_glow));
                let rim = vec4(ink, (0.12 * self.hover + 0.16 * self.active));
                sdf.stroke(rim, 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.18 } }
                    apply: { draw_bg: { down: 0.0, hover: 0.0 }, draw_icon: { down: 0.0, hover: 0.0 } }
                }
                on = {
                    from: { all: Forward { duration: 0.14 } }
                    apply: { draw_bg: { down: 0.0, hover: 1.0 }, draw_icon: { down: 0.0, hover: 1.0 } }
                }
                down = {
                    from: { all: Forward { duration: 0.08 } }
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
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 7.0);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let solid_idle = mix(mix(vec4(0.050, 0.054, 0.074, 1.0), vec4(0.112, 0.120, 0.168, 1.0), self.hover), mix(vec4(0.90, 0.87, 0.81, 1.0), vec4(0.84, 0.80, 0.74, 1.0), self.hover), self.light_mode);
                let solid_act = mix(mix(vec4(0.105, 0.098, 0.175, 1.0), vec4(self.accent_col.xyz, 0.16), self.light_mode), vec4(self.accent_col.xyz, 0.24), 0.36);
                let acr_idle = mix(vec4(ink, 0.032 + 0.035 * self.light_mode), vec4(ink, 0.10 + 0.03 * self.light_mode), self.hover);
                let acr_act = vec4(mix(mix(vec3(0.14, 0.14, 0.22), vec3(0.05, 0.06, 0.10), self.light_mode), self.accent_col.xyz, 0.36), 0.28);
                let col = mix(mix(solid_idle, solid_act, self.active), mix(acr_idle, acr_act, self.active), self.acrylic_on);
                let sheen = smoothstep(0.55, 0.0, self.pos.y) * self.hover * 0.055 * (1.0 - self.light_mode);
                let lifted = vec4(clamp(col.xyz + vec3(sheen), 0.0, 1.0), col.w);
                let pressed = mix(lifted, vec4(ink, 0.16), self.down * 0.22);
                sdf.fill_keep(pressed);
                let hov_rim = mix(mix(#3D435E, #A89E88, self.light_mode), self.accent_col, 0.24);
                let solid_b = mix(mix(mix(#1A1C26, #D0C6B2, self.light_mode), hov_rim, self.hover), vec4(self.accent_col.xyz, 0.78), self.active);
                let acr_b = mix(vec4(ink, 0.06 + 0.07 * self.light_mode + 0.12 * self.hover), vec4(self.accent_col.xyz, 0.72), self.active);
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
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                if (self.ghost > 0.5) {
                    return vec4(0.0, 0.0, 0.0, 0.0);
                }
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 8.0);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let solid_fill = mix(mix(vec4(0.052, 0.056, 0.076, 1.0), vec4(0.118, 0.126, 0.174, 1.0), self.hover), mix(vec4(0.90, 0.87, 0.81, 1.0), vec4(0.84, 0.80, 0.74, 1.0), self.hover), self.light_mode);
                let acr_fill = mix(vec4(ink, 0.032 + 0.035 * self.light_mode), vec4(ink, 0.10 + 0.03 * self.light_mode), self.hover);
                let fill = mix(solid_fill, acr_fill, self.acrylic_on);
                let sheen = smoothstep(0.45, 0.0, self.pos.y) * self.hover * 0.065 * (1.0 - self.light_mode);
                let lifted = vec4(clamp(fill.xyz + vec3(sheen), 0.0, 1.0), fill.w);
                let pressed = mix(lifted, vec4(ink, 0.15), self.down * 0.22);
                sdf.fill_keep(pressed);
                let hov_rim = mix(mix(#434966, #A89E88, self.light_mode), self.accent_col, 0.28);
                let solid_b = mix(mix(#1B1D27, #D0C6B2, self.light_mode), hov_rim, self.hover);
                let acr_b = vec4(ink, 0.06 + 0.07 * self.light_mode + 0.14 * self.hover);
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

    // Spotify-Style Pastel Radio Station Card (supports both prebuilt Spotify 3-circle CDN artwork
    // and Ferric's GPU-rendered pastel 3-circle triptych for YouTube/Local/Guest users)
    StationCard = <View> {
        width: Fill,
        height: 188,
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
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                if (self.ghost > 0.5) {
                    return vec4(0.0, 0.0, 0.0, 0.0);
                }
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 8.0);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let solid_fill = mix(mix(vec4(0.052, 0.056, 0.076, 1.0), vec4(0.118, 0.126, 0.174, 1.0), self.hover), mix(vec4(0.90, 0.87, 0.81, 1.0), vec4(0.84, 0.80, 0.74, 1.0), self.hover), self.light_mode);
                let acr_fill = mix(vec4(ink, 0.032 + 0.035 * self.light_mode), vec4(ink, 0.10 + 0.03 * self.light_mode), self.hover);
                let fill = mix(solid_fill, acr_fill, self.acrylic_on);
                let sheen = smoothstep(0.45, 0.0, self.pos.y) * self.hover * 0.065 * (1.0 - self.light_mode);
                let lifted = vec4(clamp(fill.xyz + vec3(sheen), 0.0, 1.0), fill.w);
                let pressed = mix(lifted, vec4(ink, 0.15), self.down * 0.22);
                sdf.fill_keep(pressed);
                let hov_rim = mix(mix(#434966, #A89E88, self.light_mode), self.accent_col, 0.28);
                let solid_b = mix(mix(#1B1D27, #D0C6B2, self.light_mode), hov_rim, self.hover);
                let acr_b = vec4(ink, 0.06 + 0.07 * self.light_mode + 0.14 * self.hover);
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
            art_stage = <View> {
                width: Fill,
                height: 124,
                flow: Overlay,
                full_art_wrap = <View> {
                    visible: false,
                    width: Fill,
                    height: Fill,
                    full_art = <FerricCoverArt> {
                        width: Fill,
                        height: Fill,
                        draw_bg: { radius: 7.0 }
                    }
                }
                triptych_box = <View> {
                    visible: true,
                    width: Fill,
                    height: Fill,
                    flow: Down,
                    padding: { left: 7, right: 7, top: 6, bottom: 6 }
                    show_bg: true,
                    cursor: Hand,
                    draw_bg: {
                        uniform card_col: #7CECD4
                        fn pixel(self) -> vec4 {
                            let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                            sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 7.0);
                            let top_lift = mix(self.card_col.xyz, vec3(1.0, 1.0, 1.0), 0.14 * (1.0 - self.pos.y));
                            let bot_shade = mix(top_lift, self.card_col.xyz * 0.90, smoothstep(0.55, 1.0, self.pos.y));
                            sdf.fill(vec4(bot_shade, 1.0));
                            return sdf.result;
                        }
                    }
                    <View> {
                        width: Fill,
                        height: 13,
                        flow: Right,
                        align: { y: 0.5 },
                        <View> {
                            width: 11,
                            height: 11,
                            show_bg: true,
                            draw_bg: {
                                fn pixel(self) -> vec4 {
                                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                    sdf.circle(5.5, 5.5, 4.6);
                                    sdf.fill(#0B0D14);
                                    sdf.circle(5.5, 5.5, 2.0);
                                    sdf.fill(vec4(1.0, 1.0, 1.0, 0.90));
                                    return sdf.result;
                                }
                            }
                        }
                        <View> { width: Fill, height: 1 }
                        radio_tag = <Label> {
                            width: Fit,
                            padding: 0,
                            draw_text: {
                                color: #0B0D14,
                                text_style: <THEME_FONT_BOLD> { font_size: 7.2, line_spacing: 1.0 }
                            }
                            text: "RADIO"
                        }
                    }
                    <View> {
                        width: Fill,
                        height: Fill,
                        flow: Overlay,
                        align: { x: 0.5, y: 0.5 },
                        left_wrap = <View> {
                            width: Fill,
                            height: Fill,
                            align: { x: 0.08, y: 0.5 },
                            art_left = <FerricCoverArt> {
                                width: 44,
                                height: 44,
                                draw_bg: { circle: 1.0 }
                            }
                        }
                        right_wrap = <View> {
                            width: Fill,
                            height: Fill,
                            align: { x: 0.92, y: 0.5 },
                            art_right = <FerricCoverArt> {
                                width: 44,
                                height: 44,
                                draw_bg: { circle: 1.0 }
                            }
                        }
                        center_wrap = <View> {
                            width: Fill,
                            height: Fill,
                            align: { x: 0.5, y: 0.5 },
                            art_center = <FerricCoverArt> {
                                width: 64,
                                height: 64,
                                draw_bg: { circle: 1.0 }
                            }
                        }
                    }
                    card_title = <Label> {
                        width: Fill,
                        flow: Right,
                        padding: 0,
                        draw_text: {
                            color: #0B0D14,
                            text_style: <THEME_FONT_DISPLAY> { font_size: 9.8, line_spacing: 1.0 }
                        }
                        text: ""
                    }
                }
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
                    draw_text: { color: #969AAC, text_style: <THEME_FONT_REGULAR> { font_size: 7.8, line_spacing: 1.0 } }
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
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                if (self.ghost > 0.5) {
                    return vec4(0.0, 0.0, 0.0, 0.0);
                }
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let inset = 0.5;
                sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 8.0);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let solid_fill = mix(mix(vec4(0.052, 0.056, 0.076, 1.0), vec4(0.118, 0.126, 0.174, 1.0), self.hover), mix(vec4(0.90, 0.87, 0.81, 1.0), vec4(0.84, 0.80, 0.74, 1.0), self.hover), self.light_mode);
                let acr_fill = mix(vec4(ink, 0.032 + 0.035 * self.light_mode), vec4(ink, 0.10 + 0.03 * self.light_mode), self.hover);
                let fill = mix(solid_fill, acr_fill, self.acrylic_on);
                let sheen = smoothstep(0.45, 0.0, self.pos.y) * self.hover * 0.065 * (1.0 - self.light_mode);
                let lifted = vec4(clamp(fill.xyz + vec3(sheen), 0.0, 1.0), fill.w);
                let pressed = mix(lifted, vec4(ink, 0.15), self.down * 0.22);
                sdf.fill_keep(pressed);
                let hov_rim = mix(mix(#434966, #A89E88, self.light_mode), self.accent_col, 0.28);
                let solid_b = mix(mix(#1C1E28, #D0C6B2, self.light_mode), hov_rim, self.hover);
                let acr_b = vec4(ink, 0.06 + 0.07 * self.light_mode + 0.14 * self.hover);
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
        grab_key_focus: false,
        cursor: Hand,
        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            uniform accent_col: #E86A2C
            uniform acrylic_on: 0.0
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 6.0);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let solid_idle = mix(vec4(0.0, 0.0, 0.0, 0.0), mix(vec4(0.095, 0.102, 0.140, 1.0), vec4(0.05, 0.06, 0.10, 0.09), self.light_mode), self.hover);
                let acr_idle = mix(vec4(ink, 0.028 + 0.032 * self.light_mode), vec4(ink, 0.095 + 0.03 * self.light_mode), self.hover);
                let idle = mix(solid_idle, acr_idle, self.acrylic_on);
                let act_solid = mix(mix(#1A1928, vec4(self.accent_col.xyz, 0.16), self.light_mode), vec4(self.accent_col.xyz, 0.24), 0.34);
                let act_acr = vec4(mix(mix(vec3(0.14, 0.14, 0.22), vec3(0.05, 0.06, 0.10), self.light_mode), self.accent_col.xyz, 0.36), 0.26);
                sdf.fill_keep(mix(idle, mix(act_solid, act_acr, self.acrylic_on), self.active));
                let i_rim = vec4(ink, (0.05 + 0.06 * self.light_mode + 0.08 * self.hover) * self.acrylic_on);
                let a_rim = vec4(self.accent_col.xyz, 0.68);
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

    SidebarLibraryRow = <View> {
        width: Fill,
        height: 50,
        padding: { left: 6, right: 6, top: 5, bottom: 5 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 9,
        cursor: Hand,
        grab_key_focus: false,
        show_bg: true,
        draw_bg: {
            instance hover: 0.0
            instance active: 0.0
            uniform acrylic_on: 0.0
            uniform accent_col: #E86A2C
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 6.0);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let solid_idle = mix(vec4(0.0, 0.0, 0.0, 0.0), mix(vec4(0.105, 0.112, 0.155, 1.0), vec4(0.05, 0.06, 0.10, 0.08), self.light_mode), self.hover);
                let acr_idle = mix(vec4(0.0, 0.0, 0.0, 0.0), vec4(ink, 0.095 + 0.025 * self.light_mode), self.hover);
                let idle = mix(solid_idle, acr_idle, self.acrylic_on);
                let act_solid = mix(mix(#191C26, vec4(0.05, 0.06, 0.10, 0.10), self.light_mode), vec4(self.accent_col.xyz, 0.18), 0.30);
                let act_acr = vec4(mix(ink, self.accent_col.xyz, 0.28), 0.16);
                sdf.fill_keep(mix(idle, mix(act_solid, act_acr, self.acrylic_on), self.active));
                let rim = mix(vec4(ink, 0.05 * self.hover * self.acrylic_on), vec4(self.accent_col.xyz, 0.42), self.active);
                sdf.stroke(rim, 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = { from: { all: Forward { duration: 0.14 } } apply: { draw_bg: { hover: 0.0 } } }
                on = { from: { all: Forward { duration: 0.10 } } apply: { draw_bg: { hover: 1.0 } } }
            }
        }
        art = <FerricCoverArt> {
            width: 40,
            height: 40,
            draw_bg: { radius: 5.0 }
        }
        meta = <View> {
            width: Fill,
            height: Fit,
            flow: Down,
            spacing: 2,
            title = <Label> {
                width: Fill,
                flow: Right,
                padding: 0,
                draw_text: { color: #ECEEF5, text_style: <THEME_FONT_BOLD> { font_size: 8.9, line_spacing: 1.0 } }
                text: ""
            }
            sub = <Label> {
                width: Fill,
                flow: Right,
                padding: 0,
                draw_text: { color: #969AAC, text_style: <THEME_FONT_REGULAR> { font_size: 7.7, line_spacing: 1.0 } }
                text: ""
            }
        }
    }

    SidebarSmartRow = <View> {
        width: Fill,
        height: 50,
        padding: { left: 6, right: 6, top: 5, bottom: 5 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 9,
        cursor: Hand,
        grab_key_focus: false,
        show_bg: true,
        draw_bg: {
            instance hover: 0.0
            instance active: 0.0
            uniform acrylic_on: 0.0
            uniform accent_col: #E86A2C
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 6.0);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let solid_idle = mix(vec4(0.0, 0.0, 0.0, 0.0), mix(vec4(0.105, 0.112, 0.155, 1.0), vec4(0.05, 0.06, 0.10, 0.08), self.light_mode), self.hover);
                let acr_idle = mix(vec4(0.0, 0.0, 0.0, 0.0), vec4(ink, 0.095 + 0.025 * self.light_mode), self.hover);
                let idle = mix(solid_idle, acr_idle, self.acrylic_on);
                let act_solid = mix(mix(#191C26, vec4(0.05, 0.06, 0.10, 0.10), self.light_mode), vec4(self.accent_col.xyz, 0.18), 0.30);
                let act_acr = vec4(mix(ink, self.accent_col.xyz, 0.28), 0.16);
                sdf.fill_keep(mix(idle, mix(act_solid, act_acr, self.acrylic_on), self.active));
                let rim = mix(vec4(ink, 0.05 * self.hover * self.acrylic_on), vec4(self.accent_col.xyz, 0.42), self.active);
                sdf.stroke(rim, 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = { from: { all: Forward { duration: 0.14 } } apply: { draw_bg: { hover: 0.0 } } }
                on = { from: { all: Forward { duration: 0.10 } } apply: { draw_bg: { hover: 1.0 } } }
            }
        }
        badge = <View> {
            width: 40,
            height: 40,
            align: { x: 0.5, y: 0.5 }
            show_bg: true,
            draw_bg: {
                uniform bg_a: #4A35D6
                uniform bg_b: #8A74F9
                fn pixel(self) -> vec4 {
                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                    sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 5.0);
                    let t = clamp((self.pos.x + self.pos.y) * 0.5, 0.0, 1.0);
                    sdf.fill(mix(self.bg_a, self.bg_b, t));
                    return sdf.result;
                }
            }
            icon = <Icon> {
                icon_walk: { width: 17, height: 17 }
                draw_icon: {
                    curve: 1.15,
                    linearize: 0.08,
                    brightness: 1.08,
                    svg_path: "M12 21.35L10.55 20.03C5.4 15.36 2 12.28 2 8.5C2 5.42 4.42 3 7.5 3C9.24 3 10.91 3.81 12 5.09C13.09 3.81 14.76 3 16.5 3C19.58 3 22 5.42 22 8.5C22 12.28 18.6 15.36 13.45 20.04L12 21.35Z"
                    color: #FFFFFF
                }
            }
        }
        meta = <View> {
            width: Fill,
            height: Fit,
            flow: Down,
            spacing: 2,
            title = <Label> {
                width: Fill,
                flow: Right,
                padding: 0,
                draw_text: { color: #ECEEF5, text_style: <THEME_FONT_BOLD> { font_size: 8.9, line_spacing: 1.0 } }
                text: ""
            }
            sub = <Label> {
                width: Fill,
                flow: Right,
                padding: 0,
                draw_text: { color: #969AAC, text_style: <THEME_FONT_REGULAR> { font_size: 7.7, line_spacing: 1.0 } }
                text: ""
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
                                brand_title_lbl = <Label> {
                                    margin: { left: 3, right: 6 }
                                    padding: 0,
                                    draw_text: { color: #EEF0F8, text_style: <THEME_FONT_BOLD> { font_size: 9.8 } }
                                    text: "Ferric"
                                }
                                div_top_l = <SolidView> { width: 1, height: 13, margin: { left: 1, right: 4 }, draw_bg: { color: #242634 } }
                                toggle_left_btn = <FerricIconBtn> {
                                    width: 26, height: 26,
                                    draw_bg: { active: 1.0 }
                                    draw_icon: {
                                        svg_path: "M3 5H21V19H3V5ZM5 7V17H9V7H5ZM11 7V17H19V7H11Z"
                                        color: #F4F4F7
                                    }
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

                            // Center drag area + Spotify-style Home pill + Search bar + Recents pill
                            <View> {
                                width: Fill,
                                height: Fill,
                                align: { x: 0.5, y: 0.5 }
                                top_center_grp = <View> {
                                    width: Fit,
                                    height: Fit,
                                    flow: Right,
                                    align: { y: 0.5 }
                                    spacing: 8,

                                    nav_home = <FerricIconBtn> {
                                        width: 28,
                                        height: 28,
                                        icon_walk: { width: Fit, height: 15 }
                                        draw_bg: {
                                            active: 1.0
                                            fn pixel(self) -> vec4 {
                                                let p = self.pos * self.rect_size;
                                                let sdf = Sdf2d::viewport(p);
                                                let inset = 0.75 + self.down * 0.55;
                                                let rad = min(self.rect_size.x, self.rect_size.y) * 0.5 - inset;
                                                sdf.circle(self.rect_size.x * 0.5, self.rect_size.y * 0.5, rad);
                                                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                                                let solid_idle = mix(mix(#14151D, #222533, self.hover), mix(#E4DDD0, #D8CFC0, self.hover), self.light_mode);
                                                let acr_idle = mix(vec4(ink, 0.08 + 0.025 * self.light_mode), vec4(ink, 0.16), self.hover);
                                                let idle = mix(solid_idle, acr_idle, self.acrylic_on);
                                                let solid_act = mix(#252838, #DDD5C5, self.light_mode);
                                                let acr_act = vec4(ink, 0.21);
                                                sdf.fill_keep(mix(idle, mix(solid_act, acr_act, self.acrylic_on), self.active));
                                                let rim = mix(vec4(ink, 0.10 + 0.09 * self.hover), vec4(ink, 0.24), self.active);
                                                sdf.stroke(rim, 1.0);
                                                return sdf.result;
                                            }
                                        }
                                        draw_icon: {
                                            svg_path: "M10 20V14H14V20H19V12H22L12 3L2 12H5V20H10Z"
                                            color: #F4F4F7
                                        }
                                    }

                                    top_search_box = <View> {
                                        width: 318,
                                        height: 28,
                                        padding: { left: 11, right: 8, top: 0, bottom: 0 }
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 6,
                                        show_bg: true,
                                        draw_bg: {
                                            instance hover: 0.0
                                            uniform acrylic_on: 0.0
                                            uniform accent_col: #E86A2C
                                            uniform light_mode: 0.0
                                            fn pixel(self) -> vec4 {
                                                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                                let r = (self.rect_size.y - 1.0) * 0.25;
                                                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);
                                                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                                                let solid_c = mix(mix(#11131B, #1A1D2B, self.hover), mix(#E4DDD0, #DAD1C1, self.hover), self.light_mode);
                                                let acr_c = mix(vec4(ink, 0.075 + 0.03 * self.light_mode), vec4(ink, 0.145), self.hover);
                                                sdf.fill_keep(mix(solid_c, acr_c, self.acrylic_on));
                                                let b_col = mix(mix(mix(#242736, #3E445E, self.hover), mix(#C8BFA8, #A89E88, self.hover), self.light_mode), vec4(ink, 0.15 + 0.06 * self.light_mode + 0.08 * self.hover), self.acrylic_on);
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
                                        top_search_icon = <Icon> {
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
                                            padding: { left: 2, right: 2, top: 0, bottom: 0 }
                                            empty_text: "What do you want to play? (Ctrl+K)"
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
                                        top_search_clear_btn = <FerricIconBtn> {
                                            visible: false,
                                            width: 20,
                                            height: 20,
                                            icon_walk: { width: Fit, height: 11 }
                                            draw_icon: {
                                                svg_path: "M19 6.41L17.59 5L12 10.59L6.41 5L5 6.41L10.59 12L5 17.59L6.41 19L12 13.41L17.59 19L19 17.59L13.41 12L19 6.41Z"
                                                color: #8A8E9E,
                                                color_hover: #FFFFFF
                                            }
                                        }
                                        div_top_search = <SolidView> { width: 1, height: 14, draw_bg: { color: #2A2D3D } }
                                        nav_search = <FerricIconBtn> {
                                            width: 22,
                                            height: 22,
                                            icon_walk: { width: Fit, height: 13 }
                                            draw_icon: {
                                                svg_path: "M12 2C6.48 2 2 6.48 2 12C2 17.52 6.48 22 12 22C17.52 22 22 17.52 22 12C22 6.48 17.52 2 12 2ZM14.19 14.19L6 18L9.81 9.81L18 6L14.19 14.19ZM12 10.9C11.39 10.9 10.9 11.39 10.9 12C10.9 12.61 11.39 13.1 12 13.1C12.61 13.1 13.1 12.61 13.1 12C13.1 11.39 12.61 10.9 12 10.9Z"
                                            }
                                        }
                                    }

                                    nav_recents = <FerricIconBtn> {
                                        width: 28,
                                        height: 28,
                                        icon_walk: { width: Fit, height: 14 }
                                        draw_bg: {
                                            fn pixel(self) -> vec4 {
                                                let p = self.pos * self.rect_size;
                                                let sdf = Sdf2d::viewport(p);
                                                let inset = 0.75 + self.down * 0.55;
                                                let rad = min(self.rect_size.x, self.rect_size.y) * 0.5 - inset;
                                                sdf.circle(self.rect_size.x * 0.5, self.rect_size.y * 0.5, rad);
                                                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                                                let solid_idle = mix(mix(#14151D, #222533, self.hover), mix(#E4DDD0, #D8CFC0, self.hover), self.light_mode);
                                                let acr_idle = mix(vec4(ink, 0.08 + 0.025 * self.light_mode), vec4(ink, 0.16), self.hover);
                                                let idle = mix(solid_idle, acr_idle, self.acrylic_on);
                                                let solid_act = mix(#252838, #DDD5C5, self.light_mode);
                                                let acr_act = vec4(ink, 0.21);
                                                sdf.fill_keep(mix(idle, mix(solid_act, acr_act, self.acrylic_on), self.active));
                                                let rim = mix(vec4(ink, 0.10 + 0.09 * self.hover), vec4(ink, 0.24), self.active);
                                                sdf.stroke(rim, 1.0);
                                                return sdf.result;
                                            }
                                        }
                                        draw_icon: {
                                            svg_path: "M13 3C8.03 3 4 7.03 4 12H1L4.89 15.89L4.96 16.03L9 12H6C6 8.13 9.13 5 13 5C16.87 5 20 8.13 20 12C20 15.87 16.87 19 13 19C11.07 19 9.32 18.21 8.06 16.94L6.64 18.36C8.27 19.99 10.51 21 13 21C17.97 21 22 16.97 22 12C22 7.03 17.97 3 13 3ZM12 8V13L16.28 15.54L17 14.33L13.5 12.25V8H12Z"
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
                                div_top_r = <SolidView> { width: 1, height: 14, margin: { left: 3, right: 3 }, draw_bg: { color: #242634 } }
                                win_min_btn = <FerricWinBtn> { width: 30, height: 24, draw_bg: { kind: 0.0 } }
                                win_max_btn = <FerricWinBtn> { width: 30, height: 24, draw_bg: { kind: 1.0 } }
                                win_close_btn = <FerricWinBtn> { width: 32, height: 24, draw_bg: { kind: 2.0 } }
                            }
                        }

                        div_h_top = <FerricDividerH> {}

                        // 2. THREE-PANE WORKSPACE (Collapsible Left Nav + Center Stage + Docked Right Queue/Lyrics)
                        workspace_row = <View> {
                            width: Fill,
                            height: Fill,
                            flow: Right,

                            // LEFT SIDEBAR (236px, Spotify-style "Your Library")
                            left_sidebar = <FerricSurface> {
                                width: 236,
                                height: Fill,
                                padding: { left: 8, right: 8, top: 8, bottom: 8 }
                                flow: Down,
                                spacing: 5,
                                draw_bg: { base_col: #09090D }

                                // Header: Your Library + "+ Create"
                                <View> {
                                    width: Fill,
                                    height: Fit,
                                    padding: { left: 4, right: 2, top: 1, bottom: 1 }
                                    flow: Right,
                                    align: { y: 0.5 }
                                    spacing: 6,

                                    lib_hdr_icon = <Icon> {
                                        icon_walk: { width: 15, height: 15 }
                                        draw_icon: {
                                            curve: 1.15,
                                            linearize: 0.08,
                                            brightness: 1.05,
                                            svg_path: "M4 6H2V20C2 21.1 2.9 22 4 22H18V20H4V6ZM20 2H8C6.9 2 6 2.9 6 4V16C6 17.1 6.9 18 8 18H20C21.1 18 22 17.1 22 16V4C22 2.9 21.1 2 20 2ZM19 11H15V15H13V11H9V9H13V5H15V9H19V11Z"
                                            color: #9A9CA8
                                        }
                                    }
                                    lbl_m_library = <Label> {
                                        width: Fill,
                                        draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 10.0 } }
                                        text: "Your Library"
                                    }
                                    sidebar_create_pl_btn = <FerricBtn> {
                                        height: 23,
                                        padding: { left: 8, right: 9 }
                                        draw_text: { text_style: { font_size: 8.1 } }
                                        text: "+ Create"
                                    }
                                }

                                // Library category chips (Spotify-style filter/navigation row)
                                <View> {
                                    width: Fill,
                                    height: Fit,
                                    padding: { left: 2, right: 2 }
                                    flow: Right,
                                    spacing: 4,
                                    nav_playlists = <FerricChipBtn> { height: 22, padding: { left: 7, right: 7 }, draw_text: { text_style: { font_size: 7.9 } }, text: "Playlists" }
                                    nav_songs = <FerricChipBtn> { height: 22, padding: { left: 7, right: 7 }, draw_text: { text_style: { font_size: 7.9 } }, text: "Songs" }
                                    nav_albums = <FerricChipBtn> { height: 22, padding: { left: 7, right: 7 }, draw_text: { text_style: { font_size: 7.9 } }, text: "Albums" }
                                    nav_artists = <FerricChipBtn> { height: 22, padding: { left: 7, right: 7 }, draw_text: { text_style: { font_size: 7.9 } }, text: "Artists" }
                                }

                                // Dynamic account/source playlist filter row (auto-visible when 2+ sources exist)
                                sidebar_pl_source_row = <View> {
                                    visible: false,
                                    width: Fill,
                                    height: Fit,
                                    padding: { left: 2, right: 2 }
                                    flow: Right,
                                    spacing: 3,
                                    sb_src_all_btn = <FerricChipBtn> { height: 20, padding: { left: 5, right: 5 }, draw_bg: { active: 1.0 }, draw_text: { active: 1.0, text_style: { font_size: 7.3 } }, text: "All" }
                                    sb_src_yt_btn = <FerricChipBtn> { visible: false, height: 20, padding: { left: 5, right: 5 }, draw_text: { text_style: { font_size: 7.3 } }, text: "YouTube" }
                                    sb_src_sp_btn = <FerricChipBtn> { visible: false, height: 20, padding: { left: 5, right: 5 }, draw_text: { text_style: { font_size: 7.3 } }, text: "Spotify" }
                                    sb_src_local_btn = <FerricChipBtn> { visible: false, height: 20, padding: { left: 5, right: 5 }, draw_text: { text_style: { font_size: 7.3 } }, text: "Local" }
                                }

                                // Inline filter search + count + Newest/Oldest/A-Z sort cycle row
                                <View> {
                                    width: Fill,
                                    height: 25,
                                    flow: Right,
                                    align: { y: 0.5 }
                                    spacing: 4,
                                    sidebar_filter_box = <View> {
                                        width: Fill,
                                        height: 25,
                                        padding: { left: 7, right: 7, top: 0, bottom: 0 }
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 5,
                                        show_bg: true,
                                        draw_bg: {
                                            uniform acrylic_on: 0.0
                                            uniform light_mode: 0.0
                                            fn pixel(self) -> vec4 {
                                                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 5.0);
                                                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                                                let solid_c = mix(#11131B, #E6DFD2, self.light_mode);
                                                let acr_c = vec4(ink, 0.055 + 0.025 * self.light_mode);
                                                sdf.fill_keep(mix(solid_c, acr_c, self.acrylic_on));
                                                let b_col = mix(mix(#1E2130, #CBC2AC, self.light_mode), vec4(ink, 0.09 + 0.04 * self.light_mode), self.acrylic_on);
                                                sdf.stroke(b_col, 1.0);
                                                return sdf.result;
                                            }
                                        }
                                        sidebar_filter_icon = <Icon> {
                                            icon_walk: { width: 11, height: 11 }
                                            draw_icon: {
                                                curve: 1.15,
                                                linearize: 0.08,
                                                brightness: 1.05,
                                                svg_path: "M15.5 14H14.71L14.43 13.73C15.41 12.59 16 11.11 16 9.5C16 5.91 13.09 3 9.5 3C5.91 3 3 5.91 3 9.5C3 13.09 5.91 16 9.5 16C11.11 16 12.59 15.41 13.73 14.43L14 14.71V15.5L19 20.49L20.49 19L15.5 14ZM9.5 14C7.01 14 5 11.99 5 9.5C5 7.01 7.01 5 9.5 5C11.99 5 14 7.01 14 9.5C14 11.99 11.99 14 9.5 14Z"
                                                color: #7D8090
                                            }
                                        }
                                        sidebar_lib_filter_input = <FerricInput> {
                                            width: Fill,
                                            height: Fill,
                                            padding: { left: 1, right: 1, top: 0, bottom: 0 }
                                            empty_text: "Filter..."
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
                                                text_style: <THEME_FONT_REGULAR> { font_size: 8.0, line_spacing: 1.0 }
                                            }
                                        }
                                        lbl_m_pinned = <Label> {
                                            width: Fit,
                                            draw_text: { color: #8A8EA2, text_style: <THEME_FONT_REGULAR> { font_size: 7.5 } }
                                            text: "0"
                                        }
                                    }
                                    sb_pl_sort_btn = <FerricChipBtn> {
                                        height: 24,
                                        padding: { left: 6, right: 6 }
                                        draw_text: { text_style: { font_size: 7.3 } }
                                        text: "Newest"
                                    }
                                }

                                // Scrollable rich playlist list (40x40 artwork + title + subtitle)
                                <FerricScrollYView> {
                                    width: Fill,
                                    height: Fill,
                                    flow: Down,
                                    spacing: 2,
                                    pin_liked_row = <SidebarSmartRow> {
                                        badge = {
                                            draw_bg: { bg_a: #4A35D6, bg_b: #8F7BF9 }
                                        }
                                        meta = {
                                            title = { text: "Liked Songs" }
                                            sub = { text: "Playlist · 0 songs" }
                                        }
                                    }
                                    pin_local_row = <SidebarSmartRow> {
                                        badge = {
                                            draw_bg: { bg_a: #2E3446, bg_b: #5A627A }
                                            icon = {
                                                draw_icon: {
                                                    svg_path: "M12 2C6.48 2 2 6.48 2 12C2 17.52 6.48 22 12 22C17.52 22 22 17.52 22 12C22 6.48 17.52 2 12 2ZM12 14.5C10.62 14.5 9.5 13.38 9.5 12C9.5 10.62 10.62 9.5 12 9.5C13.38 9.5 14.5 10.62 14.5 12C14.5 13.38 13.38 14.5 12 14.5Z"
                                                }
                                            }
                                        }
                                        meta = {
                                            title = { text: "Local Lossless" }
                                            sub = { text: "Local · 0 songs" }
                                        }
                                    }
                                    sidebar_empty_wrap = <View> {
                                        visible: false,
                                        width: Fill,
                                        height: Fit,
                                        padding: { top: 8, bottom: 4, left: 6, right: 6 }
                                        sidebar_empty_lbl = <Label> {
                                            width: Fill,
                                            height: Fit,
                                            draw_text: {
                                                color: #8A8EA2,
                                                text_style: <THEME_FONT_REGULAR> { font_size: 8.1, line_spacing: 1.35 }
                                            }
                                            text: "No playlists yet. Sign in via Settings or click + Create."
                                        }
                                    }
                                    pin_slot_0 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_1 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_2 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_3 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_4 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_5 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_6 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_7 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_8 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_9 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_10 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_11 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_12 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_13 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_14 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_15 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_16 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_17 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_18 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_19 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_20 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_21 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_22 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_23 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_24 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_25 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_26 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_27 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_28 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_29 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_30 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_31 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_32 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_33 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_34 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_35 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_36 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_37 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_38 = <SidebarLibraryRow> { visible: false }
                                    pin_slot_39 = <SidebarLibraryRow> { visible: false }
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
                                            lbl_t_quick_picks = <Label> {
                                                width: Fill,
                                                draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }
                                                text: "Quick picks"
                                            }
                                            home_status_lbl = <Label> {
                                                draw_text: { color: #7C8090, text_style: <THEME_FONT_REGULAR> { font_size: 8.2 } }
                                                text: "Loading YouTube Music..."
                                            }
                                            home_retry_btn = <FerricBtn> { visible: false, height: 24, padding: { left: 8, right: 9 }, text: "Retry" }
                                            qp_refresh_btn = <FerricBtn> { height: 24, padding: { left: 8, right: 8 }, text: "Refresh" }
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

                                    // SECTION 2: RECOMMENDED STATIONS (Spotify-Style Pastel Radio Cards)
                                    stations_wrap = <View> {
                                        visible: false,
                                        width: Fill,
                                        height: Fit,
                                        flow: Down,
                                        spacing: 8,

                                        <View> {
                                            width: Fill,
                                            height: Fit,
                                            flow: Right,
                                            align: { y: 0.85 },
                                            spacing: 4,
                                            <View> {
                                                width: Fill,
                                                height: Fit,
                                                flow: Down,
                                                spacing: 1,
                                                lbl_m_stations = <Label> {
                                                    width: Fill,
                                                    draw_text: { color: #8B8E9E, text_style: <THEME_FONT_REGULAR> { font_size: 7.9 } }
                                                    text: "Non-stop music based on your favorite songs and artists."
                                                }
                                                lbl_t_stations = <Label> {
                                                    width: Fill,
                                                    draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }
                                                    text: "Recommended Stations"
                                                }
                                            }
                                            stations_refresh_btn = <FerricBtn> { height: 24, padding: { left: 8, right: 8 }, text: "Shuffle" }
                                            stations_prev_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                            stations_next_btn = <FerricIconBtn> { width: 24, height: 24, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                        }

                                        <View> {
                                            width: Fill,
                                            height: Fit,
                                            flow: Right,
                                            spacing: 8,
                                            st_c0 = <StationCard> {}
                                            st_c1 = <StationCard> {}
                                            st_c2 = <StationCard> {}
                                            st_c3 = <StationCard> {}
                                            st_c4 = <StationCard> {}
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
                                        lbl_m_recent = <Label> { draw_text: { color: #848796, text_style: <THEME_FONT_REGULAR> { font_size: 8.2 } }, text: "Recent:" }
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
                                        lbl_t_top_results = <Label> { draw_text: { color: #D4D6E2, text_style: <THEME_FONT_BOLD> { font_size: 10.2 } }, text: "Top Results · Artists, Albums & Playlists" }
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
                                            lbl_t_recents = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 15.5 } }, text: "Recents" }
                                            recents_sub_lbl = <Label> { draw_text: { color: #8A8D9B, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Your recently played songs" }
                                        }
                                        recents_play_btn = <FerricPrimaryBtn> { text: "Play All" }
                                        recents_clear_btn = <FerricBtn> { text: "Clear History" }
                                    }
                                    div_h_recents = <FerricDividerH> {}
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

                                    div_h_songs = <FerricDividerH> {}
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
                                            lbl_t_albums = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 15.5 } }, text: "Albums" }
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
                                            lbl_t_artists = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 15.5 } }, text: "Artists" }
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
                                        lbl_t_playlists = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 15.5 } }, text: "Playlists" }
                                        pl_new_input = <FerricInput> { width: 180, height: 28, empty_text: "New playlist name..." }
                                        pl_create_btn = <FerricPrimaryBtn> { height: 28, text: "+ Create" }
                                        pl_import_btn = <FerricBtn> { height: 28, text: "Import M3U/CSV" }
                                        pl_prev_btn = <FerricIconBtn> { width: 26, height: 26, draw_icon: { svg_path: "M15.41 7.41L14 6L8 12L14 18L15.41 16.59L10.83 12L15.41 7.41Z" } }
                                        pl_next_btn = <FerricIconBtn> { width: 26, height: 26, draw_icon: { svg_path: "M8.59 16.59L10 18L16 12L10 6L8.59 7.41L13.17 12L8.59 16.59Z" } }
                                    }
                                    <View> {
                                        width: Fill,
                                        height: Fit,
                                        flow: Right,
                                        align: { y: 0.5 }
                                        spacing: 8,
                                        pl_source_filter_row = <View> {
                                            visible: false,
                                            width: Fit,
                                            height: Fit,
                                            flow: Right,
                                            spacing: 5,
                                            pl_src_all_btn = <FerricChipBtn> { text: "All", draw_bg: { active: 1.0 }, draw_text: { active: 1.0 } }
                                            pl_src_yt_btn = <FerricChipBtn> { visible: false, text: "YouTube" }
                                            pl_src_sp_btn = <FerricChipBtn> { visible: false, text: "Spotify" }
                                            pl_src_local_btn = <FerricChipBtn> { visible: false, text: "Local" }
                                        }
                                        <View> { width: Fill, height: 1 }
                                        <View> {
                                            width: Fit,
                                            height: Fit,
                                            flow: Right,
                                            spacing: 5,
                                            pl_sort_newest_btn = <FerricChipBtn> { text: "Newest", draw_bg: { active: 1.0 }, draw_text: { active: 1.0 } }
                                            pl_sort_oldest_btn = <FerricChipBtn> { text: "Oldest" }
                                            pl_sort_az_btn = <FerricChipBtn> { text: "A-Z" }
                                        }
                                    }
                                    playlists_empty_card = <FerricCard> {
                                        visible: false,
                                        width: Fill,
                                        height: Fit,
                                        padding: { left: 22, right: 22, top: 24, bottom: 24 }
                                        margin: { top: 8 }
                                        flow: Down,
                                        align: { x: 0.0, y: 0.5 }
                                        spacing: 10,
                                        lbl_t_pl_empty = <Label> {
                                            width: Fill,
                                            padding: 0,
                                            draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 13.0 } }
                                            text: "No playlists yet"
                                        }
                                        lbl_m_pl_empty = <Label> {
                                            width: Fill,
                                            padding: 0,
                                            draw_text: { wrap: Word, color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 9.0, line_spacing: 1.35 } }
                                            text: "Enter a playlist name in the bar above and click + Create to start building your own collection, import an existing M3U/CSV playlist, or sign in with Google in Settings to sync your YouTube Music playlists."
                                        }
                                        <View> {
                                            width: Fill,
                                            height: Fit,
                                            margin: { top: 4 }
                                            flow: Right,
                                            spacing: 8,
                                            pl_empty_focus_btn = <FerricPrimaryBtn> { height: 28, text: "+ Create Playlist" }
                                            pl_empty_import_btn = <FerricBtn> { height: 28, text: "Import M3U/CSV" }
                                            pl_empty_google_btn = <FerricBtn> { height: 28, text: "Sync from Google" }
                                        }
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
                                                detail_save_pl_btn = <FerricBtn> { visible: false, height: 28, text: "+ Save Playlist" }
                                                detail_radio_btn = <FerricBtn> { height: 28, text: "Start Radio" }
                                                detail_copy_link_btn = <FerricBtn> { visible: false, height: 28, text: "Copy Link" }
                                                detail_pin_btn = <FerricBtn> { height: 28, text: "Pin" }
                                                detail_queue_btn = <FerricBtn> { height: 28, text: "+ Queue" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, margin: { top: 2 }, flow: Right, spacing: 6,
                                                detail_copy_list_btn = <FerricBtn> { height: 26, text: "Copy Tracklist" }
                                                detail_open_source_btn = <FerricBtn> { visible: false, height: 26, text: "Open Source" }
                                                detail_cloud_push_btn = <FerricBtn> { visible: false, height: 26, text: "Sync to Account" }
                                                detail_dedup_btn = <FerricBtn> { visible: false, height: 26, text: "Deduplicate" }
                                                detail_export_btn = <FerricBtn> { height: 26, text: "Export M3U" }
                                                detail_export_csv_btn = <FerricBtn> { height: 26, text: "Export CSV" }
                                                detail_delete_btn = <FerricBtn> {
                                                    visible: false,
                                                    height: 26,
                                                    text: "Delete"
                                                    draw_text: { color: #FF7885, color_hover: #FFA3AC }
                                                }
                                            }
                                            detail_tools_row = <View> {
                                                width: Fill, height: Fit, margin: { top: 2 }, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                detail_filter_input = <FerricInput> {
                                                    width: 195,
                                                    height: 26,
                                                    empty_text: "Filter songs in list..."
                                                    draw_text: { text_style: <THEME_FONT_REGULAR> { font_size: 8.4, line_spacing: 1.0 } }
                                                }
                                                detail_sort_btn = <FerricBtn> { height: 26, text: "Sort: Default" }
                                                detail_rename_row = <View> {
                                                    visible: false,
                                                    width: Fit, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                    detail_rename_input = <FerricInput> {
                                                        width: 155,
                                                        height: 26,
                                                        empty_text: "Rename playlist..."
                                                        draw_text: { text_style: <THEME_FONT_REGULAR> { font_size: 8.4, line_spacing: 1.0 } }
                                                    }
                                                    detail_rename_btn = <FerricBtn> { height: 26, text: "Rename" }
                                                }
                                            }
                                        }
                                    }
                                    div_h_detail = <FerricDividerH> {}
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

                                    lbl_t_settings = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 15.5 } }, text: "Settings" }

                                    <View> {
                                        width: Fill, height: Fit, flow: Right, spacing: 5,
                                        st_accounts_btn = <FerricChipBtn> { text: "Accounts", draw_bg: { active: 1.0 }, draw_text: { active: 1.0 } }
                                        st_playback_btn = <FerricChipBtn> { text: "Playback" }
                                        st_eq_btn = <FerricChipBtn> { text: "Equalizer" }
                                        st_appearance_btn = <FerricChipBtn> { text: "Appearance" }
                                        st_integrations_btn = <FerricChipBtn> { text: "Integrations" }
                                        st_storage_btn = <FerricChipBtn> { text: "Storage" }
                                        st_connect_btn = <FerricChipBtn> { text: "Connect" }
                                        st_about_btn = <FerricChipBtn> { text: "About" }
                                    }

                                    // TAB A: ACCOUNTS & SOURCES
                                    tab_accounts = <View> {
                                        visible: true,
                                        width: Fill, height: Fit, flow: Down, spacing: 10,

                                        card_acct_yt = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_acct_yt = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "YouTube Music & Google Account" }
                                                yt_state_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "Guest Mode (Optional)" }
                                            }
                                            lbl_m_acct_desc = <Label> {
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
                                                text: "Guest Mode active. YouTube Music streaming ready."
                                            }
                                        }

                                        card_acct_spotify_web = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_acct_spotify_web = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Spotify Account (Free & Premium)" }
                                                spotify_web_state_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "Not Connected" }
                                            }
                                            lbl_m_spotify_web = <Label> {
                                                width: Fill,
                                                draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }
                                                text: "Works for both Spotify Free and Premium accounts: signs in via Spotify and syncs your Liked Songs, playlists, and Recommended Stations directly (cookies & tokens encrypted with Windows DPAPI)."
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                spotify_web_login_btn = <FerricPrimaryBtn> { text: "Sign in with Spotify" }
                                                spotify_web_sync_btn = <FerricBtn> { height: 28, text: "Sync Library" }
                                                spotify_web_logout_btn = <FerricBtn> { height: 28, text: "Sign Out" }
                                            }
                                            spotify_web_status_lbl = <Label> {
                                                width: Fill,
                                                draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.5 } }
                                                text: "Not signed in. Connect any Spotify Free or Premium account, or import public Spotify links below."
                                            }
                                        }

                                        card_acct_import = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            lbl_t_acct_import = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Instant Spotify & YouTube Playlist Import" }
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
                                            lbl_t_acct_sub = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "OpenSubsonic / Navidrome Self-Hosted Server" }
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

                                        card_pb_device = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                lbl_t_pb_dev = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Audio Output Device (WASAPI)" }
                                                pb_device_active_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.6 } }, text: "Active: System Default" }
                                            }
                                            lbl_m_pb_dev = <Label> {
                                                width: Fill,
                                                draw_text: { wrap: Word, color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.4 } }
                                                text: "Switch between headphones, speakers, USB DAC, or Bluetooth output live without restarting playback. Automatically falls back to Windows default if unplugged."
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                pb_device_cycle_btn = <FerricBtn> { height: 26, text: "Output: System Default" }
                                                pb_device_refresh_btn = <FerricBtn> { height: 26, text: "Refresh Devices" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 5,
                                                pb_dev_default = <FerricChipBtn> { height: 24, text: "System Default", draw_bg: { active: 1.0 }, draw_text: { active: 1.0 } }
                                                pb_dev_0 = <FerricChipBtn> { visible: false, height: 24, text: "" }
                                                pb_dev_1 = <FerricChipBtn> { visible: false, height: 24, text: "" }
                                                pb_dev_2 = <FerricChipBtn> { visible: false, height: 24, text: "" }
                                                pb_dev_3 = <FerricChipBtn> { visible: false, height: 24, text: "" }
                                            }
                                        }

                                        card_pb_xfade = <FerricCard> {
                                            flow: Down, spacing: 9,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_pb_xfade = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Seamless Dual-Deck Fade-In & Crossfade" }
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
                                                lbl_m_pb_curve = <Label> { draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Transition Curve:" }
                                                curve_bass_btn = <FerricBtn> { text: "Bass-Swap Automix" }
                                                curve_eqp_btn = <FerricBtn> { text: "Equal-Power" }
                                                curve_velvet_btn = <FerricBtn> { text: "S-Curve Velvet" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_pb_fadein = <Label> { width: Fill, draw_text: { color: #C8CAD6, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Soft Play / Resume Fade-In (ms)" }
                                                fadein_val_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "240 ms" }
                                            }
                                            fadein_slider = <FerricBarSlider> { min: 0.0, max: 1000.0, value: 240.0 }
                                        }

                                        card_pb_quality = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            lbl_t_pb_qual = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Audio & In-App Video Clip Quality" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                lbl_t_pb_aq = <Label> { width: 140, draw_text: { color: #C8CAD6, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Streaming Audio Quality" }
                                                aq_low_btn = <FerricChipBtn> { height: 24, text: "Low 64k" }
                                                aq_norm_btn = <FerricChipBtn> { height: 24, text: "Normal 128k" }
                                                aq_high_btn = <FerricChipBtn> { height: 24, text: "High 160k" }
                                                aq_best_btn = <FerricChipBtn> { height: 24, text: "Best" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                lbl_t_pb_vq = <Label> { width: 140, draw_text: { color: #C8CAD6, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Default Video Quality" }
                                                svq_auto_btn = <FerricChipBtn> { height: 24, text: "Auto" }
                                                svq_360_btn = <FerricChipBtn> { height: 24, text: "360p" }
                                                svq_480_btn = <FerricChipBtn> { height: 24, text: "480p" }
                                                svq_720_btn = <FerricChipBtn> { height: 24, text: "720p" }
                                                svq_1080_btn = <FerricChipBtn> { height: 24, text: "1080p" }
                                            }
                                        }

                                        card_pb_behavior = <FerricCard> {
                                            flow: Down, spacing: 7,
                                            lbl_t_pb_beh = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Playback Behavior, Radio & System Tray" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_pb_tray = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Keep running in System Tray when clicking Close (X)" }
                                                close_to_tray_toggle = <FerricToggle> { is_on: false }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_pb_radio = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Autoplay Infinite Radio (YouTube Music Next)" }
                                                radio_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_pb_gapless = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "0ms Gapless Album Lock (Local consecutive albums only)" }
                                                gapless_lock_toggle = <FerricToggle> { is_on: false }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_pb_trim = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Skip Leading & Trailing Silence" }
                                                trim_silence_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_pb_rg = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "ReplayGain Loudness Normalization" }
                                                replay_gain_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_pb_mono = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Mono Downmix" }
                                                mono_toggle = <FerricToggle> { is_on: false }
                                            }
                                        }

                                        card_pb_speed = <FerricCard> {
                                            flow: Down, spacing: 8,
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
                                                lbl_t_eq_title = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "10-Band Parametric Biquad Equalizer" }
                                                eq_status_lbl = <Label> { draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Preset: Flat" }
                                                eq_enable_toggle = <FerricToggle> { is_on: true }
                                            }
                                            eq_graph = <FerricEqGraph> { height: 136 }

                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                lbl_t_eq_preamp = <Label> { width: 68, draw_text: { color: #C8CAD6, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Preamp" }
                                                eq_preamp_slider = <FerricBarSlider> { min: -12.0, max: 12.0, value: 0.0 }
                                                eq_preamp_lbl = <Label> { width: 64, padding: 0, align: { x: 1.0, y: 0.5 }, draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "+0.0 dB" }
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
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v0 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b0 = <FerricVertSlider> { height: 112 }, eq_f0 = <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "31" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v1 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b1 = <FerricVertSlider> { height: 112 }, eq_f1 = <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "62" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v2 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b2 = <FerricVertSlider> { height: 112 }, eq_f2 = <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "125" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v3 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b3 = <FerricVertSlider> { height: 112 }, eq_f3 = <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "250" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v4 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b4 = <FerricVertSlider> { height: 112 }, eq_f4 = <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "500" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v5 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b5 = <FerricVertSlider> { height: 112 }, eq_f5 = <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "1k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v6 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b6 = <FerricVertSlider> { height: 112 }, eq_f6 = <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "2k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v7 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b7 = <FerricVertSlider> { height: 112 }, eq_f7 = <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "4k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v8 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b8 = <FerricVertSlider> { height: 112 }, eq_f8 = <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "8k" } }
                                                <View> { width: Fit, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 3, eq_v9 = <Label> { draw_text: { color: #C8CAD6, text_style: { font_size: 8.0 } }, text: "+0.0" }, eq_b9 = <FerricVertSlider> { height: 112 }, eq_f9 = <Label> { draw_text: { color: #7C8090, text_style: { font_size: 8.0 } }, text: "16k" } }
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
                                        width: Fill, height: Fit, flow: Down, spacing: 12,

                                        // CARD 1: THEME & INTERFACE SCALE
                                        card_app_theme = <FerricCard> {
                                            flow: Down, spacing: 12,
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 2,
                                                lbl_t_app_title = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.6 } }, text: "Theme & Interface" }
                                                lbl_m_app_theme_desc = <Label> { draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.4 } }, text: "Choose your surface palette, interface zoom scale, and cover-driven accent tint." }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 8,
                                                pal_amoled_btn = <FerricBtn> { height: 30, padding: { left: 13, right: 13 }, text: "AMOLED Black" }
                                                pal_acrylic_btn = <FerricBtn> { height: 30, padding: { left: 13, right: 13 }, text: "Native Acrylic" }
                                                pal_obsidian_btn = <FerricBtn> { height: 30, padding: { left: 13, right: 13 }, text: "Obsidian Studio" }
                                                pal_cream_btn = <FerricBtn> { height: 30, padding: { left: 13, right: 13 }, text: "Warm Cream" }
                                            }
                                            div_h_app = <FerricDividerH> {}
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                <View> {
                                                    width: Fill, height: Fit, flow: Down, spacing: 2,
                                                    lbl_t_app_zoom = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_MEDIUM> { font_size: 9.0 } }, text: "Interface Zoom" }
                                                    lbl_m_app_zoom_hint = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.2 } }, text: "Scale typography, spacing, and sidebars (Ctrl - / Ctrl + / Ctrl 0)" }
                                                }
                                                zoom_out_btn = <FerricBtn> { height: 26, padding: { left: 10, right: 10 }, text: "-" }
                                                zoom_val_lbl = <Label> { width: 50, padding: 0, align: { x: 0.5, y: 0.5 }, draw_text: { color: #E86A2C, text_style: <THEME_FONT_BOLD> { font_size: 9.0 } }, text: "100%" }
                                                zoom_in_btn = <FerricBtn> { height: 26, padding: { left: 10, right: 10 }, text: "+" }
                                                zoom_reset_btn = <FerricBtn> { height: 26, padding: { left: 10, right: 10 }, text: "Reset" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                                <View> {
                                                    width: Fill, height: Fit, flow: Down, spacing: 2,
                                                    lbl_t_app_adaptive = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_MEDIUM> { font_size: 9.0 } }, text: "Adaptive Artwork Accent" }
                                                    lbl_m_app_adaptive_hint = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.2 } }, text: "Tint seekbar, active indicators, and lyrics highlight from the playing cover art." }
                                                }
                                                adaptive_art_toggle = <FerricToggle> { is_on: true }
                                            }
                                        }

                                        // CARD 2: NATIVE WINDOWS 11 ACRYLIC & GLASS TINT
                                        card_app_acrylic = <FerricCard> {
                                            flow: Down, spacing: 12,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                                <View> {
                                                    width: Fill, height: Fit, flow: Down, spacing: 2,
                                                    lbl_t_app_acr = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.4 } }, text: "Native Windows 11 Acrylic" }
                                                    lbl_m_app_acr_desc = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.3 } }, text: "Real OS DWM blur behind the window using hardware DirectComposition." }
                                                }
                                                acrylic_toggle = <FerricToggle> { is_on: false }
                                            }
                                            div_h_app_acr = <FerricDividerH> {}
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                                lbl_m_app_opacity = <Label> { width: 118, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "Acrylic Opacity" }
                                                acrylic_opacity_slider = <FerricBarSlider> { min: 0.18, max: 0.94, value: 0.72, wheel_adjust: true, wheel_step: 0.04 }
                                                <View> {
                                                    width: 52, height: Fit, align: { x: 1.0, y: 0.5 },
                                                    acrylic_opacity_lbl = <Label> { width: Fit, draw_text: { color: #E86A2C, text_style: <THEME_FONT_BOLD> { font_size: 8.8 } }, text: "72%" }
                                                }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                                <View> {
                                                    width: Fill, height: Fit, flow: Down, spacing: 2,
                                                    lbl_t_app_acr_adaptive = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_MEDIUM> { font_size: 9.0 } }, text: "Adaptive Acrylic Tint" }
                                                    lbl_m_app_acr_adaptive_hint = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.2 } }, text: "Smoothly shift the glass tint to harmonize with the current song cover." }
                                                }
                                                adaptive_acrylic_tint_toggle = <FerricToggle> { is_on: true }
                                            }
                                            div_h_app_tint = <FerricDividerH> {}
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 9,
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                    lbl_t_app_tint = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_MEDIUM> { font_size: 9.0 } }, text: "Manual Glass Tint" }
                                                    acrylic_tint_preview = <View> {
                                                        width: 16, height: 16,
                                                        show_bg: true,
                                                        draw_bg: {
                                                            uniform tint_col: #0B0D14
                                                            uniform light_mode: 0.0
                                                            fn pixel(self) -> vec4 {
                                                                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                                                sdf.circle(self.rect_size.x * 0.5, self.rect_size.y * 0.5, self.rect_size.x * 0.5 - 1.0);
                                                                sdf.fill_keep(self.tint_col);
                                                                let ring = mix(vec4(1.0, 1.0, 1.0, 0.35), vec4(0.05, 0.06, 0.10, 0.45), self.light_mode);
                                                                sdf.stroke(ring, 1.0);
                                                                return sdf.result;
                                                            }
                                                        }
                                                    }
                                                    acrylic_tint_lbl = <Label> { width: Fit, draw_text: { color: #E86A2C, text_style: <THEME_FONT_BOLD> { font_size: 8.6 } }, text: "#0B0D14" }
                                                }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.0 }, spacing: 14,
                                                    acrylic_color_picker = <FerricColorPicker> {
                                                        width: Fill,
                                                        height: 110,
                                                    }
                                                    <View> {
                                                        width: 316, height: Fit, flow: Down, spacing: 8,
                                                        lbl_m_app_presets = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_MEDIUM> { font_size: 8.2 } }, text: "Quick Presets" }
                                                        <View> {
                                                            width: Fill, height: Fit, flow: Right, spacing: 5,
                                                            tint_obsidian_btn = <FerricChipBtn> { width: Fill, text: "Obsidian" }
                                                            tint_ember_btn = <FerricChipBtn> { width: Fill, text: "Ember" }
                                                            tint_copper_btn = <FerricChipBtn> { width: Fill, text: "Copper" }
                                                            tint_midnight_btn = <FerricChipBtn> { width: Fill, text: "Midnight" }
                                                        }
                                                        <View> {
                                                            width: Fill, height: Fit, flow: Right, spacing: 5,
                                                            tint_emerald_btn = <FerricChipBtn> { width: Fill, text: "Emerald" }
                                                            tint_crimson_btn = <FerricChipBtn> { width: Fill, text: "Crimson" }
                                                            tint_slate_btn = <FerricChipBtn> { width: Fill, text: "Slate" }
                                                            tint_smoke_btn = <FerricChipBtn> { width: Fill, text: "Smoke" }
                                                        }
                                                        <View> {
                                                            width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                            acrylic_tint_input = <FerricInput> { width: Fill, height: 28, empty_text: "#RRGGBB" }
                                                            acrylic_tint_apply_btn = <FerricBtn> { height: 28, padding: { left: 12, right: 12 }, text: "Apply" }
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        // CARD 3: THEME PROFILES
                                        card_app_profiles = <FerricCard> {
                                            flow: Right, align: { y: 0.5 }, spacing: 10,
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 2,
                                                lbl_t_app_profiles = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.0 } }, text: "Theme Profiles" }
                                                lbl_m_app_profiles = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.3 } }, text: "Import or export your custom Ferric color palette as JSON." }
                                            }
                                            theme_import_btn = <FerricBtn> { height: 28, padding: { left: 12, right: 12 }, text: "Import JSON" }
                                            theme_export_btn = <FerricBtn> { height: 28, padding: { left: 12, right: 12 }, text: "Export JSON" }
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
                                                lbl_t_disc_title = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Discord Rich Presence" }
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
                                                    uniform light_mode: 0.0
                                                    fn pixel(self) -> vec4 {
                                                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 9.0);
                                                        let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                                                        let solid_bg = mix(vec4(0.085, 0.088, 0.112, 1.0), vec4(0.87, 0.84, 0.78, 1.0), self.light_mode);
                                                        let acr_bg = vec4(ink, 0.052 + 0.04 * self.light_mode);
                                                        sdf.fill_keep(mix(solid_bg, acr_bg, self.acrylic_on));
                                                        sdf.stroke(vec4(ink, 0.10 + 0.08 * self.light_mode), 1.0);
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
                                                lbl_t_disc_en = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_MEDIUM> { font_size: 9.2 } }, text: "Show on Discord (Listening activity via local IPC)" }
                                                discord_enable_toggle = <FerricToggle> { is_on: false }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 8,
                                                lbl_t_disc_fmt = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Status name display" }
                                                discord_format_btn = <FerricBtn> { height: 26, text: "Artist - Title" }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_disc_paused = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Show when paused" }
                                                discord_paused_toggle = <FerricToggle> { is_on: false }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_disc_cover = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Show album cover art" }
                                                discord_cover_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_disc_btn = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Show 'Listen on Spotify / YouTube' button" }
                                                discord_button_toggle = <FerricToggle> { is_on: true }
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                                lbl_t_disc_hide = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }, text: "Hide track details (Privacy mode)" }
                                                discord_hide_details_toggle = <FerricToggle> { is_on: false }
                                            }
                                            div_h_disc = <FerricDividerH> {}
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 6,
                                                lbl_t_disc_id = <Label> { width: Fill, draw_text: { color: #DCDCE6, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "Discord Application ID" }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 6,
                                                    discord_client_id_input = <FerricInput> { width: Fill, height: 28, empty_text: "Optional custom Discord App ID (built-in Ferric ID active by default)..." }
                                                    discord_save_id_btn = <FerricPrimaryBtn> { height: 28, text: "Save ID" }
                                                    discord_portal_btn = <FerricBtn> { height: 28, text: "Create App ID" }
                                                }
                                                lbl_m_disc_hint = <Label> {
                                                    width: Fill,
                                                    draw_text: { wrap: Word, color: #8A8EA2, text_style: <THEME_FONT_REGULAR> { font_size: 8.2 } }
                                                    text: "Ferric's official Discord Application ID is built in: simply enable 'Show on Discord' above. Leave blank for automatic connection, or paste a custom ID to override."
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
                                            lbl_t_st_cache = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Local Disk Cache" }
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

                                        card_st_keys = <FerricCard> {
                                            flow: Down, spacing: 5,
                                            lbl_t_st_keys = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Keyboard Shortcuts" }
                                            lbl_m_st_keys = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Space: Play / Pause  ·  Ctrl+K: Focus Search  ·  Ctrl+P: Powerbar  ·  Ctrl+Shift+C: Copy Song Link  ·  Ctrl+F: Search Page  ·  Ctrl+Q: Queue  ·  Ctrl+L: Lyrics  ·  Ctrl+ / Ctrl- / Ctrl+0: UI Zoom  ·  Ctrl+,: Settings  ·  F11: Fullscreen  ·  Alt+Left/Right: Back/Forward  ·  Shift+Left/Right: Prev/Next  ·  Ctrl+Up/Down / Mouse Wheel: Volume" }
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
                                                lbl_t_conn_title = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Ferric Connect (Cloud E2EE & Wi-Fi)" }
                                                connect_lan_ip_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 } }, text: "Connect: Ready" }
                                                connect_peers_lbl = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Searching Cloud E2EE Relay and local Wi-Fi for your Ferric devices..." }
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, spacing: 6,
                                                    connect_gen_pair_btn = <FerricBtn> { text: "Regenerate QR" }
                                                    connect_copy_pair_btn = <FerricBtn> { text: "Copy Pairing URI" }
                                                }
                                            }
                                        }
                                    }

                                    // TAB G: ABOUT, UPDATES & LINKS
                                    tab_about = <View> {
                                        visible: false,
                                        width: Fill, height: Fit, flow: Down, spacing: 10,

                                        card_ab_hero = <FerricCard> {
                                            flow: Right, align: { y: 0.5 }, spacing: 16,
                                            about_brand_icon = <Image> {
                                                width: 56,
                                                height: 56,
                                                fit: Smallest
                                            }
                                            <View> {
                                                width: Fill, height: Fit, flow: Down, spacing: 4,
                                                <View> {
                                                    width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                                    lbl_t_ab_name = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 14.5 } }, text: "Ferric" }
                                                    about_version_lbl = <Label> { draw_text: { color: #E86A2C, text_style: <THEME_FONT_BOLD> { font_size: 9.2 } }, text: "v0.1.3 · Windows x64" }
                                                }
                                                lbl_m_ab_tagline = <Label> {
                                                    width: Fill,
                                                    draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }
                                                    text: "Native Rust desktop music player for local lossless libraries, YouTube Music, and Spotify, with synced lyrics, 10-band EQ, and Windows 11 Acrylic."
                                                }
                                                lbl_m_ab_meta = <Label> {
                                                    width: Fill,
                                                    draw_text: { color: #9A9CA8, text_style: <THEME_FONT_MEDIUM> { font_size: 8.3 } }
                                                    text: "Part of the LivelyDesk suite  ·  MIT License  ·  © Rafly Firmansyah"
                                                }
                                            }
                                        }

                                        card_st_update = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 }, spacing: 10,
                                                lbl_t_st_update = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Software Updates (Ferric)" }
                                                lbl_m_st_auto_upd = <Label> { draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.4 } }, text: "Auto-check on startup" }
                                                auto_update_toggle = <FerricToggle> { is_on: true }
                                            }
                                            update_status_lbl = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "SHA-256 verified GitHub Releases MSI updater" }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                check_updates_btn = <FerricBtn> { height: 28, text: "Check for Updates" }
                                                open_releases_btn = <FerricBtn> { height: 28, text: "Open GitHub Releases" }
                                            }
                                        }

                                        card_ab_links = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            lbl_t_ab_links = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Links & Resources" }
                                            lbl_m_ab_links = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Official product page, source repository, changelog, license, and issue tracker." }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                about_web_btn = <FerricBtn> { height: 28, text: "Website (LivelyDesk)" }
                                                about_github_btn = <FerricBtn> { height: 28, text: "GitHub" }
                                                about_changelog_btn = <FerricBtn> { height: 28, text: "Changelog" }
                                                about_issues_btn = <FerricBtn> { height: 28, text: "Report Issue" }
                                                about_license_btn = <FerricBtn> { height: 28, text: "MIT License" }
                                            }
                                        }

                                        card_ab_support = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            lbl_t_ab_support = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Support Development" }
                                            lbl_m_ab_support = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.8 } }, text: "Ferric is free and open source. Optional support helps fund development and never gates or unlocks features." }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                about_coffee_btn = <FerricBtn> { height: 28, text: "Buy Me a Coffee" }
                                                about_saweria_btn = <FerricBtn> { height: 28, text: "Saweria" }
                                            }
                                        }

                                        card_ab_system = <FerricCard> {
                                            flow: Down, spacing: 8,
                                            lbl_t_ab_sys = <Label> { draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Engine & Privacy" }
                                            lbl_m_ab_sys = <Label> { width: Fill, draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 8.6 } }, text: "Built with Rust, Makepad (DirectX 11 + DirectComposition), Symphonia, pure-Rust Opus, Rubato, and SQLite. Saved credentials are encrypted locally with Windows DPAPI; zero analytics or telemetry." }
                                            <View> {
                                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                                about_copy_diag_btn = <FerricBtn> { height: 28, text: "Copy System Info" }
                                            }
                                        }
                                    }
                                }
                            }

                            right_divider = <FerricDividerV> {}

                            // ============================================================
                            // DOCKED RIGHT SIDEBAR (260px: Queue & Spring Synced Lyrics)
                            // ============================================================
                            right_sidebar = <FerricSurface> {
                                visible: true,
                                width: 260,
                                height: Fill,
                                padding: { left: 8, right: 10, top: 8, bottom: 8 }
                                flow: Down,
                                spacing: 6,
                                draw_bg: { base_col: #09090D }

                                // Right Sidebar Header Tabs: [Queue] [Lyrics] [Connect] + [Radio] [Save] [Copy] [Refresh] [Clear]
                                <View> {
                                    width: Fill, height: 26, flow: Right, align: { y: 0.5 }, spacing: 3,
                                    rs_tab_queue = <FerricChipBtn> { height: 24, padding: { left: 7, right: 7 }, text: "Queue", draw_bg: { active: 1.0 }, draw_text: { active: 1.0 } }
                                    rs_tab_lyrics = <FerricChipBtn> { height: 24, padding: { left: 7, right: 7 }, text: "Lyrics" }
                                    rs_tab_connect = <FerricChipBtn> { height: 24, padding: { left: 7, right: 7 }, text: "Connect" }
                                    <View> { width: Fill, height: 1 }
                                    rs_radio_btn = <FerricIconBtn> {
                                        width: 24, height: 24,
                                        draw_bg: { active: 1.0 }
                                        draw_icon: {
                                            svg_path: "M3.24 6.15C2.51 6.43 2 7.17 2 8V20C2 21.1 2.89 22 4 22H20C21.11 22 22 21.1 22 20V8C22 6.89 21.11 6 20 6H8.3L16.56 2.66L15.88 1L3.24 6.15ZM7 20C5.34 20 4 18.66 4 17C4 15.34 5.34 14 7 14C8.66 14 10 15.34 10 17C10 18.66 8.66 20 7 20ZM20 12H18V10H16V12H4V8H20V12Z"
                                            color: #E86A2C
                                        }
                                    }
                                    rs_save_queue_btn = <FerricIconBtn> {
                                        width: 24, height: 24,
                                        draw_icon: { svg_path: "M14 10H3V12H14V10ZM14 6H3V8H14V6ZM18 14V10H16V14H12V16H16V20H18V16H22V14H18ZM3 16H10V14H3V16Z" }
                                    }
                                    rs_copy_queue_btn = <FerricIconBtn> {
                                        width: 24, height: 24,
                                        draw_icon: { svg_path: "M16 1H4C2.9 1 2 1.9 2 3V17H4V3H16V1ZM19 5H8C6.9 5 6 5.9 6 7V21C6 22.1 6.9 23 8 23H19C20.1 23 21 22.1 21 21V7C21 5.9 20.1 5 19 5ZM19 21H8V7H19V21Z" }
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

                                div_h_rs = <FerricDividerH> {}

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
                                        lbl_m_history = <Label> {
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

                                // TAB 3: ZERO-PAIRING FERRIC CONNECT (Spotify-Connect-Style Device Switcher)
                                rs_connect_view = <FerricScrollYView> {
                                    visible: false,
                                    width: Fill,
                                    height: Fill,
                                    padding: { top: 4, right: 6, bottom: 10 }
                                    flow: Down,
                                    spacing: 8,

                                    card_rs_conn_active = <FerricCard> {
                                        flow: Down,
                                        spacing: 6,
                                        rs_conn_active_lbl = <Label> {
                                            width: Fill,
                                            draw_text: { color: #E86A2C, text_style: <THEME_FONT_BOLD> { font_size: 9.8 } }
                                            text: "This PC (Active)"
                                        }
                                        rs_conn_now_lbl = <Label> {
                                            width: Fill,
                                            draw_text: { color: #C2C5D2, text_style: <THEME_FONT_REGULAR> { font_size: 8.4 } }
                                            text: "Ready to connect across Cloud or Wi-Fi"
                                        }
                                        <View> {
                                            width: Fill, height: 20, flow: Right, align: { y: 0.5 }, spacing: 6,
                                            lbl_m_rs_vol = <Label> {
                                                draw_text: { color: #8A8EA2, text_style: <THEME_FONT_MEDIUM> { font_size: 7.8 } }
                                                text: "VOL"
                                            }
                                            rs_conn_vol_slider = <FerricBarSlider> {
                                                width: Fill,
                                                height: 16,
                                                min: 0.0, max: 1.0, value: 0.85,
                                                wheel_adjust: true,
                                                wheel_step: 0.05
                                            }
                                        }
                                    }

                                    lbl_m_rs_switch = <Label> {
                                        margin: { left: 4, top: 2 }
                                        draw_text: { color: #8A8EA2, text_style: <THEME_FONT_MEDIUM> { font_size: 7.8 } }
                                        text: "SWITCH OUTPUT TO"
                                    }

                                    rs_conn_local_btn = <FerricBtn> { width: Fill, height: 30, text: "This PC (Active)" }
                                    rs_conn_peer_0 = <FerricBtn> { visible: false, width: Fill, height: 30, text: "Device 1" }
                                    rs_conn_peer_1 = <FerricBtn> { visible: false, width: Fill, height: 30, text: "Device 2" }
                                    rs_conn_peer_2 = <FerricBtn> { visible: false, width: Fill, height: 30, text: "Device 3" }
                                    rs_conn_peer_3 = <FerricBtn> { visible: false, width: Fill, height: 30, text: "Device 4" }

                                    rs_conn_status_lbl = <Label> {
                                        width: Fill,
                                        margin: { left: 4, top: 4 }
                                        draw_text: { color: #8A8EA2, text_style: <THEME_FONT_REGULAR> { font_size: 8.0 } }
                                        text: "Sign in to Spotify or YouTube Music on Desktop & Android to auto-link across any network."
                                    }

                                    rs_conn_qr_btn = <FerricBtn> { width: Fill, height: 28, text: "QR Pairing & Network Info" }
                                }
                            }
                        }

                        // ============================================================
                        // 3. CENTERED 2-ROW AMBIENT BOTTOM PLAYER BAR (74px, GEOMETRIC SYMMETRY)
                        // ============================================================
                        bottom_bar = <AmbientBottomBar> {
                            // Left: 44x44 Cover Art + Title/Artist + Heart + Add Playlist + Copy Link
                            bottom_left_grp = <View> {
                                width: 300,
                                height: Fill,
                                padding: { left: 2, right: 4 }
                                flow: Right,
                                align: { y: 0.5 }
                                spacing: 5,

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
                                    np_artist_hit = <View> {
                                        width: Fill,
                                        height: Fit,
                                        cursor: Hand,
                                        np_artist_lbl = <Label> {
                                            width: Fill,
                                            flow: Right,
                                            padding: 0,
                                            draw_text: { color: #A2A6B8, text_style: <THEME_FONT_REGULAR> { font_size: 8.3, line_spacing: 1.0 } }
                                            text: "Pick a song to play"
                                        }
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
                                    width: 26, height: 26,
                                    icon_walk: { width: 15, height: 15 }
                                    draw_icon: {
                                        svg_path: "M19 13H13V19H11V13H5V11H11V5H13V11H19V13Z"
                                        color: #707382
                                    }
                                }
                                np_copy_link_btn = <FerricIconBtn> {
                                    width: 28, height: 26,
                                    padding: 0,
                                    align: { x: 0.5, y: 0.5 }
                                    icon_walk: { width: 16, height: 9 }
                                    draw_icon: {
                                        svg_path: "M3.9 12C3.9 10.29 5.29 8.9 7 8.9H11V7H7C4.24 7 2 9.24 2 12C2 14.76 4.24 17 7 17H11V15.1H7C5.29 15.1 3.9 13.71 3.9 12ZM8 13H16V11H8V13ZM17 7H13V8.9H17C18.71 8.9 20.1 10.29 20.1 12C20.1 13.71 18.71 15.1 17 15.1H13V17H17C19.76 17 22 14.76 22 12C22 9.24 19.76 7 17 7Z"
                                        color: #707382
                                    }
                                }
                            }

                            // Center (Stacked 2-Row Transport + Wide Scrubber, Dead-Center Symmetry)
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
                                    repeat_btn = <FerricRepeatBtn> {
                                        width: 28, height: 28,
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

                            // Right (Fit width guarantees zero clipping of codec pill, volume, or fullscreen icon at ANY zoom 65%-140%!)
                            bottom_right_grp = <View> {
                                width: Fit,
                                height: Fill,
                                flow: Right,
                                align: { x: 1.0, y: 0.5 }
                                spacing: 3,

                                speed_pill_btn = <FerricBtn> {
                                    height: 22,
                                    padding: { left: 7, right: 7 }
                                    text: "1.0x"
                                    draw_text: { color: #B4B6C4, color_hover: #FFFFFF, text_style: <THEME_FONT_MEDIUM> { font_size: 7.8 } }
                                }
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
                                bar_connect_btn = <FerricIconBtn> {
                                    width: 28, height: 28,
                                    icon_walk: { width: Fit, height: 15 }
                                    draw_icon: { svg_path: "M4 6H22V4H4C2.9 4 2 4.9 2 6V17H0V20H14V17H4V6ZM23 8H17C16.45 8 16 8.45 16 9V19C16 19.55 16.45 20 17 20H23C23.55 20 24 19.55 24 19V9C24 8.45 23.55 8 23 8ZM22 17H18V10H22V17Z" }
                                }
                                vol_mute_btn = <FerricVolumeBtn> {
                                    width: 28, height: 28,
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
                                    draw_text: { color: #FFFFFF9E, text_style: <THEME_FONT_MEDIUM> { font_size: 8.0 } }
                                    text: "Signal Path: WASAPI 48.0 kHz · 6.0s Bass-Swap"
                                }
                                fs_close_btn = <Button> {
                                    width: Fit, height: 28, padding: { left: 14, right: 14 },
                                    text: "Close (Esc)"
                                    draw_text: { color: #FFFFFFCC, color_hover: #FFFFFF, text_style: <THEME_FONT_MEDIUM> { font_size: 8.6 } }
                                    draw_bg: {
                                        instance hover: 0.0
                                        fn pixel(self) -> vec4 {
                                            let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                            let r = (self.rect_size.y - 1.0) * 0.25;
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

                                    // Hero Cover Art (304x304 rounded with cursor-tracked hover glare & zoom) OR Docked 16:9 Rounded Native Video Theater
                                    fs_media_box = <View> {
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
                                            spacing: 10,

                                            video_stage_frame = <View> {
                                                width: Fill,
                                                height: 319,
                                                padding: 0,
                                                align: { x: 0.5, y: 0.5 }
                                                show_bg: true,
                                                draw_bg: {
                                                    uniform reveal: 1.0
                                                    fn pixel(self) -> vec4 {
                                                        let p = self.pos * self.rect_size;
                                                        let sdf = Sdf2d::viewport(p);
                                                        let a = clamp(self.reveal, 0.0, 1.0);
                                                        sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 16.0);
                                                        sdf.fill(vec4(0.04, 0.05, 0.08, 0.44 * a));
                                                        return sdf.result;
                                                    }
                                                }
                                                video_stage_slot = <View> {
                                                    width: Fill,
                                                    height: Fill,
                                                    align: { x: 0.5, y: 0.5 }
                                                    vid_stage_hint_lbl = <Label> {
                                                        padding: 0,
                                                        draw_text: { color: #D2D6E4, text_style: <THEME_FONT_MEDIUM> { font_size: 9.2 } }
                                                        text: "Loading native in-app video clip..."
                                                    }
                                                }
                                            }

                                            fs_video_footer = <View> {
                                                width: Fill,
                                                height: 54,
                                                flow: Down,
                                                align: { x: 0.5, y: 0.0 }
                                                fs_video_footer_inner = <View> {
                                                    width: Fill,
                                                    height: Fit,
                                                    flow: Down,
                                                    align: { x: 0.5, y: 0.5 }
                                                    spacing: 8,
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
                                        }
                                    }

                                    // Clean Centered Wrapping Title + Like Heart & Artist directly below Cover Art (never clips long titles!)
                                    <View> {
                                        width: 344, height: Fit, flow: Down, align: { x: 0.5 }, spacing: 4,
                                        <View> {
                                            width: Fill, height: Fit, flow: Right, align: { x: 0.5, y: 0.5 }, spacing: 6,
                                            <View> { width: 24, height: 1 }
                                            fs_title_lbl = <Label> {
                                                width: Fill,
                                                padding: 0,
                                                align: { x: 0.5, y: 0.5 }
                                                draw_text: { wrap: Word, color: #FFFFFF, text_style: <THEME_FONT_DISPLAY> { font_size: 13.8, line_spacing: 1.14 } }
                                                text: "No track playing"
                                            }
                                            fs_like_btn = <FerricIconBtn> {
                                                width: 24, height: 24,
                                                draw_bg: { acrylic_on: 1.0, light_mode: 0.0 }
                                                draw_icon: {
                                                    svg_path: "M12 21.35L10.55 20.03C5.4 15.36 2 12.28 2 8.5C2 5.42 4.42 3 7.5 3C9.24 3 10.91 3.81 12 5.09C13.09 3.81 14.76 3 16.5 3C19.58 3 22 5.42 22 8.5C22 12.28 18.6 15.36 13.45 20.04L12 21.35Z"
                                                    color: #FFFFFFB8,
                                                    color_hover: #FFFFFF,
                                                    color_focus: #FFFFFF
                                                }
                                            }
                                        }
                                        fs_artist_lbl = <Label> {
                                            width: Fill,
                                            padding: 0,
                                            align: { x: 0.5, y: 0.5 }
                                            draw_text: { wrap: Word, color: #FFFFFF8C, text_style: <THEME_FONT_REGULAR> { font_size: 9.6, line_spacing: 1.12 } }
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

                                    // Row 1: Symmetrical Centered Transport Controls (Play/Pause dead-center)
                                    <View> {
                                        width: 312, height: 38, flow: Right, align: { x: 0.5, y: 0.5 }, spacing: 14,
                                        fs_shuffle_btn = <FerricIconBtn> {
                                            width: 28, height: 28,
                                            draw_bg: { acrylic_on: 1.0, light_mode: 0.0 }
                                            draw_icon: { svg_path: "M10.59 9.17L5.41 4L4 5.41L9.17 10.58L10.59 9.17ZM14.5 4L16.54 6.04L4 18.59L5.41 20L17.96 7.46L20 9.5V4H14.5ZM14.83 13.41L13.42 14.82L16.55 17.95L14.5 20H20V14.5L17.96 16.54L14.83 13.41Z", color: #FFFFFFB8, color_hover: #FFFFFF, color_focus: #FFFFFF }
                                        }
                                        fs_prev_btn = <FerricIconBtn> {
                                            width: 30, height: 30,
                                            draw_bg: { acrylic_on: 1.0, light_mode: 0.0 }
                                            draw_icon: { svg_path: "M6 6H8V18H6V6ZM9.5 12L18 18V6L9.5 12Z", color: #FFFFFFC8, color_hover: #FFFFFF, color_focus: #FFFFFF }
                                        }
                                        fs_play_pause_btn = <FerricPlayBtn> {
                                            width: 36, height: 36,
                                        }
                                        fs_next_btn = <FerricIconBtn> {
                                            width: 30, height: 30,
                                            draw_bg: { acrylic_on: 1.0, light_mode: 0.0 }
                                            draw_icon: { svg_path: "M6 18L14.5 12L6 6V18ZM16 6V18H18V6H16Z", color: #FFFFFFC8, color_hover: #FFFFFF, color_focus: #FFFFFF }
                                        }
                                        fs_repeat_btn = <FerricRepeatBtn> {
                                            width: 28, height: 28,
                                            draw_bg: { fg_col: #FFFFFFB8, fg_hover: #FFFFFF, acrylic_on: 1.0, light_mode: 0.0 }
                                        }
                                    }

                                    // Row 2: Dedicated Centered Volume Bar directly beneath Transport for exact symmetry
                                    <View> {
                                        width: 232, height: 26, flow: Right, align: { x: 0.5, y: 0.5 }, spacing: 8,
                                        fs_vol_mute_btn = <FerricVolumeBtn> {
                                            width: 26, height: 26,
                                            draw_bg: { fg_col: #FFFFFFB8, fg_hover: #FFFFFF, acrylic_on: 1.0, light_mode: 0.0 }
                                        }
                                        fs_vol_slider = <FerricBarSlider> {
                                            width: Fill, height: 18, min: 0.0, max: 1.0, value: 0.85,
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
                                    draw_text: { color: #FFFFFF9E, text_style: <THEME_FONT_MEDIUM> { font_size: 8.0 } }
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
                            width: 510,
                            padding: 14,
                            flow: Down,
                            spacing: 10,
                            <View> {
                                width: Fill, height: Fit, flow: Right, align: { y: 0.5 },
                                lbl_t_powerbar = <Label> { width: Fill, draw_text: { color: #F4F4F7, text_style: <THEME_FONT_BOLD> { font_size: 10.8 } }, text: "Ferric Powerbar" }
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
                            <View> {
                                width: Fill, height: Fit, flow: Right, spacing: 5,
                                cmdk_copy_link_btn = <FerricBtn> { height: 26, text: "Copy Song Link" }
                                cmdk_copy_pl_btn = <FerricBtn> { height: 26, text: "Copy Playlist Link" }
                                cmdk_save_q_btn = <FerricBtn> { height: 26, text: "Save Queue" }
                                cmdk_about_btn = <FerricBtn> { height: 26, text: "About & Updates" }
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
                                uniform acrylic_opacity: 0.72
                                uniform acrylic_tint: #0B0D14
                                uniform surface_col: #0F1016
                                uniform border_col: #252734
                                uniform accent_col: #E86A2C
                                uniform light_mode: 0.0
                                fn pixel(self) -> vec4 {
                                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                                    sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 8.0);
                                    let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                                    if (self.acrylic_on > 0.5) {
                                        let a = clamp(self.acrylic_opacity * 0.72 + 0.20, 0.48, 0.90);
                                        let lift = mix(vec3(0.055, 0.058, 0.068), vec3(-0.04, -0.04, -0.04), self.light_mode);
                                        let glass = clamp(self.acrylic_tint.xyz + lift, 0.0, 1.0);
                                        sdf.fill_keep(vec4(glass * a, a));
                                        sdf.stroke(vec4(ink, 0.18 + 0.08 * self.light_mode), 1.0);
                                    } else {
                                        let solid_fill = vec4(clamp(mix(self.surface_col.xyz * 1.15 + vec3(0.02, 0.02, 0.024), self.surface_col.xyz, self.light_mode), 0.0, 1.0), 0.96);
                                        sdf.fill_keep(solid_fill);
                                        sdf.stroke(mix(self.border_col, vec4(self.accent_col.xyz, 0.45), 0.25), 1.0);
                                    }
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
                            width: 480,
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
                                tm_lyrics_btn = <FerricBtn> { height: 26, text: "Lyrics" }
                            }

                            <View> {
                                width: Fill, height: Fit, flow: Right, spacing: 6,
                                tm_copy_link_btn = <FerricBtn> { height: 26, text: "Copy Link" }
                                tm_copy_btn = <FerricBtn> { height: 26, text: "Copy Title" }
                                tm_copy_alt_link_btn = <FerricBtn> { visible: false, height: 26, text: "Copy YT Link" }
                                tm_open_source_btn = <FerricBtn> { height: 26, text: "Open Source" }
                            }

                            tm_div_1 = <FerricDividerH> {}

                            tm_add_pl_hdr = <Label> { draw_text: { color: #9A9CA8, text_style: <THEME_FONT_BOLD> { font_size: 8.2 } }, text: "ADD TO PLAYLIST" }
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

                            tm_div_2 = <FerricDividerH> {}

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

type PaletteCacheKey = (
    bool,
    u16,
    (u8, u8, u8),
    (u8, u8, u8),
    (u8, u8, u8),
    (u8, u8, u8),
    (u8, u8, u8),
);

fn read_text_file_capped(path: &std::path::Path, max_bytes: u64) -> Result<String, String> {
    use std::io::Read;
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("Selected path is not a regular file".to_string());
    }
    if meta.len() > max_bytes {
        return Err(format!(
            "File is too large ({} KiB, max {} KiB)",
            meta.len() / 1024,
            max_bytes / 1024
        ));
    }
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::with_capacity(meta.len().min(max_bytes) as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > max_bytes {
        return Err(format!(
            "File exceeds max allowed size ({} KiB)",
            max_bytes / 1024
        ));
    }
    String::from_utf8(buf).map_err(|_| "File is not valid UTF-8 text".to_string())
}

fn friendly_error_message(raw: &str) -> String {
    let trimmed = raw
        .trim()
        .strip_prefix("network request failed: ")
        .unwrap_or(raw.trim());
    let first_line = trimmed.lines().next().unwrap_or(trimmed).trim();
    if first_line.is_empty() {
        return "Operation failed. Please try again.".to_string();
    }
    // Redact accidental local drive paths before displaying user-facing error text.
    let redacted = if first_line.contains(":\\") {
        first_line
            .split_whitespace()
            .map(|tok| {
                if tok.contains(":\\") {
                    "[local path]"
                } else {
                    tok
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        first_line.to_string()
    };
    clip_text(&clean_display_text(&redacted), 140)
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
    artwork_snapshot_dirty: bool,
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
    recommended_stations: Vec<RecommendedStation>,
    #[rust]
    stations_page: usize,
    #[rust]
    stations_request_seq: u64,
    #[rust]
    stations_rotation_epoch: usize,
    #[rust]
    home_loading_more: bool,
    #[rust]
    home_request_seq: u64,
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
    sidebar_filter_query: String,
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
    detail_filter_query: String,
    #[rust]
    detail_sort_order: DetailSortOrder,
    #[rust]
    output_devices: Vec<String>,
    #[rust]
    session_save_dirty_at: Option<Instant>,
    #[rust]
    last_fs_viz_playing: Option<bool>,
    #[rust]
    last_tick_pos_ms: u64,
    #[rust]
    last_tick_dur_ms: u64,
    #[rust]
    queue: PlayQueue,
    #[rust]
    queue_context: String,
    #[rust]
    last_radio_seed_id: Option<String>,
    #[rust]
    radio_loading: bool,
    #[rust]
    spotify_sync_in_flight: bool,
    #[rust]
    theme: ThemeConfig,
    #[rust]
    palette_cache: RefCell<Option<(PaletteCacheKey, AdaptivePalette)>>,
    #[rust]
    last_fs_backdrop_playing: Option<bool>,
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
    update_download_gen: u64,
    #[rust]
    waiting_for_radio_after_eof: bool,
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
    video_is_official_mv: bool,
    #[rust]
    icon_pulses: Vec<(&'static [LiveId], f32, f32)>,
    #[rust]
    last_video_sync: Option<Instant>,
    #[rust]
    displayed_accent: Vec4,
    #[rust]
    displayed_acrylic_tint: Vec4,
    #[rust]
    started_at: Option<Instant>,
    #[rust]
    startup_trim_stage: u8,
    #[rust]
    connect_engine: Option<Arc<ConnectEngine>>,
    #[rust]
    connect_snap: Option<ConnectSnapshot>,
    #[rust]
    connect_peer_ids: [String; 4],
    #[rust]
    connect_sync_ticks: u8,
    #[rust]
    last_connect_pushed_playing: Option<bool>,
    #[rust]
    pending_handoff_seek_ms: Option<u64>,
    #[rust]
    pending_handoff_paused: bool,
    #[rust]
    last_remote_mirror_key: String,
    #[rust]
    was_controlling_remote: bool,
    #[rust]
    pairing_uri: String,
    #[rust]
    lan_ip: String,
    #[rust]
    synced_lyrics: SyncedLyrics,
    #[rust]
    lyrics_offset_ms: i64,
    #[rust]
    last_finished_seq: u64,
    #[rust]
    active_filter: LibraryFilter,
    #[rust]
    playlist_source_filter: PlaylistSourceFilter,
    #[rust]
    playlist_sort: PlaylistSortOrder,
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
    right_connect_tab: bool,
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
    modal_dismissed_swallow_up: bool,
    #[rust]
    track_menu_target: Option<Track>,
    #[rust]
    songs_filter_query: String,
    #[rust]
    window_hidden_or_minimized: bool,
    #[rust]
    ui_dirty_while_hidden: bool,
    #[rust]
    tray_second_trim_at: Option<Instant>,
    #[rust]
    post_load_trim_at: Option<Instant>,
    #[rust]
    last_open_gpu_trim_at: Option<Instant>,
    #[rust]
    artwork_was_pending: bool,
    #[rust]
    artwork_cleared_for_tray: bool,
    #[rust]
    home_shown_track_ids: VecDeque<String>,
    #[rust]
    home_shown_card_ids: VecDeque<String>,
    #[rust]
    home_shown_station_artists: VecDeque<String>,
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
    fs_reveal_target: f32,
    #[rust]
    fs_video_morph_t: f32,
    #[rust]
    fs_video_morph_target: f32,
    #[rust]
    fs_video_bounds_sync_pending: bool,
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
    tick_counter: u64,
    #[rust]
    last_discord_tick_at: Option<Instant>,
    #[rust]
    last_conn_vol_sent_at: Option<Instant>,
    #[rust]
    last_remote_track_key: String,
    #[rust]
    last_discord_track_id: String,
    #[rust]
    last_discord_playing: Option<bool>,
    #[rust]
    last_taskbar_track_key: String,
    #[rust]
    last_taskbar_playing: Option<bool>,
    #[rust]
    last_win_max_check_at: Option<Instant>,
    #[rust]
    last_codec_txt: String,
    #[rust]
    sleep_timer: SleepTimerMode,
    #[rust]
    sleep_fade_base_vol: Option<f32>,
    #[rust]
    pending_stream_track: Option<Track>,
    #[rust]
    consecutive_stream_failures: u32,
    #[rust]
    discord_settings: DiscordRpcSettings,
    #[rust]
    discord_rpc: Option<DiscordRpcBridge>,
    #[rust]
    last_discord_status: String,
    #[rust]
    tick_timer: Timer,
    #[rust]
    tick_interval_ms: u32,
    #[rust]
    stream_request_seq: Option<Arc<AtomicU64>>,
    #[rust]
    lyrics_request_seq: Option<Arc<AtomicU64>>,
    #[rust]
    bg_rx: Option<Receiver<BgMessage>>,
    #[rust]
    bg_tx: Option<mpsc::Sender<BgMessage>>,
    #[rust]
    initialized: bool,
}

fn station_pastel_vec4(idx: u8) -> Vec4 {
    const PALETTE: [&str; 8] = [
        "#7CECD4", // Mint Aqua
        "#FFA3DC", // Bubblegum Pink
        "#FFD86B", // Warm Gold
        "#8FF0A4", // Fresh Mint Green
        "#D8A8FF", // Soft Lavender
        "#FFB088", // Coral Peach
        "#B9F25A", // Acid Lime
        "#92D6FF", // Sky Cyan
    ];
    rgba_vec4(PALETTE[(idx as usize) % PALETTE.len()])
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

    /// Trigger a crisp 250ms GPU micro-animation on a stateful `FerricIconBtn`
    /// (`kind`: 0.0 = Bounce, 1.0 = Spin, 2.0 = Shake, 3.0 = HeartPop). Zero CPU once settled.
    fn trigger_icon_pulse(&mut self, cx: &mut Cx, btn_id: &'static [LiveId], kind: f32) {
        if let Some(slot) = self.icon_pulses.iter_mut().find(|(id, _, _)| *id == btn_id) {
            slot.1 = 0.005;
            slot.2 = kind;
        } else {
            self.icon_pulses.push((btn_id, 0.005, kind));
        }
        self.ui.button(btn_id).apply_over(
            cx,
            live! {
                draw_bg: { pulse: 0.005 }
                draw_icon: { pulse: 0.005, pulse_kind: (kind) }
            },
        );
        self.start_motion(cx);
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
        let k_sidebar = (1.0 - (-11.5 * dt).exp()) as f32;
        let mut active = false;

        if self.page_reveal_t < 0.999 {
            self.page_reveal_t = (self.page_reveal_t + (dt as f32) / 0.22_f32).min(1.0);
            if self.page_reveal_t < 0.999 {
                active = true;
            }
            let ease = 1.0 - (1.0 - f64::from(self.page_reveal_t)).powi(3);
            let (base_top, base_bottom) = match self.active_page {
                MainPage::Home => (12.0, 26.0),
                MainPage::Albums | MainPage::Artists | MainPage::Playlists | MainPage::Settings => {
                    (14.0, 26.0)
                }
                _ => (14.0, 12.0),
            };
            let top_pad = base_top + 6.0 * (1.0 - ease);
            self.ui.view(self.active_page_view_id()).apply_over(
                cx,
                live! { padding: { left: 18.0, right: 18.0, top: (top_pad), bottom: (base_bottom) } },
            );
        }

        if (self.fs_reveal_target - self.fs_reveal_t).abs() > 0.004 {
            if self.fs_reveal_target > self.fs_reveal_t {
                // Keep Fullscreen Lyrics open choreography unchanged (graceful ~320ms 2-phase rise)
                let k_fs = (1.0 - (-11.5 * dt).exp()) as f32;
                self.fs_reveal_t = (self.fs_reveal_t
                    + (self.fs_reveal_target - self.fs_reveal_t) * k_fs)
                    .clamp(0.0, 1.0);
            } else {
                // Unhurried, silky ~460ms 2-phase close dissolve (linear progress into smoothstep01
                // so lyrics & cover glide out gently before the aurora backdrop dissolves away)
                self.fs_reveal_t = (self.fs_reveal_t - (dt as f32) / 0.46_f32).max(0.0);
            }
            if (self.fs_reveal_target - self.fs_reveal_t).abs() <= 0.006 {
                self.fs_reveal_t = self.fs_reveal_target;
                self.apply_fullscreen_reveal(cx);
                if self.fs_reveal_t >= 0.995 {
                    // Fully covered by Fullscreen Lyrics stage -> hide background shell to save GPU
                    self.ui.view(id!(shell_col)).set_visible(cx, false);
                    if self.video_open && self.fs_video_morph_t >= 0.99 {
                        self.fs_video_bounds_sync_pending = true;
                        active = true;
                    }
                } else {
                    // Close transition finished -> tear down Fullscreen overlay cleanly (0% idle GPU)
                    self.finish_fullscreen_close(cx);
                }
            } else {
                self.apply_fullscreen_reveal(cx);
                active = true;
            }
        }

        if self.fullscreen_open
            && (self.fs_video_morph_target - self.fs_video_morph_t).abs() > 0.004
        {
            let k_media = (1.0 - (-12.5 * dt).exp()) as f32;
            self.fs_video_morph_t += (self.fs_video_morph_target - self.fs_video_morph_t) * k_media;
            if (self.fs_video_morph_target - self.fs_video_morph_t).abs() <= 0.005 {
                self.fs_video_morph_t = self.fs_video_morph_target;
                if self.video_open && self.fs_video_morph_t >= 0.99 {
                    // Schedule one post-layout frame so `video_stage_slot` reports its settled final rect.
                    self.fs_video_bounds_sync_pending = true;
                    active = true;
                }
            } else {
                active = true;
            }
            self.sync_fullscreen_media_layout(cx);
        } else if self.fs_video_bounds_sync_pending && self.fs_reveal_t >= 0.995 {
            self.fs_video_bounds_sync_pending = false;
            self.sync_video_bounds(cx);
        }

        if self.cmdk_open && self.cmdk_reveal_t < 0.999 {
            self.cmdk_reveal_t = (self.cmdk_reveal_t + (dt as f32) / 0.20_f32).min(1.0);
            if self.cmdk_reveal_t < 0.999 {
                active = true;
            }
            let ease = 1.0 - (1.0 - f64::from(self.cmdk_reveal_t)).powi(3);
            let y_align = 0.20 + 0.012 * (1.0 - ease);
            self.ui
                .view(id!(cmdk_modal))
                .apply_over(cx, live! { align: { x: 0.5, y: (y_align) } });
        }

        if self.toast_until.is_some() && self.toast_reveal_t < 0.999 {
            self.toast_reveal_t = (self.toast_reveal_t + (dt as f32) / 0.22_f32).min(1.0);
            if self.toast_reveal_t < 0.999 {
                active = true;
            }
            let ease = 1.0 - (1.0 - f64::from(self.toast_reveal_t)).powi(3);
            let b_pad = 58.0 + 10.0 * ease;
            self.ui
                .view(id!(toast_wrap))
                .apply_over(cx, live! { padding: { right: 16.0, bottom: (b_pad) } });
        }

        if (self.left_w_target - self.left_w_cur).abs() > 0.5 {
            self.left_w_cur += (self.left_w_target - self.left_w_cur) * f64::from(k_sidebar);
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
            self.right_w_cur += (self.right_w_target - self.right_w_cur) * f64::from(k_sidebar);
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

        if !self.icon_pulses.is_empty() {
            let step = (dt as f32) / 0.32_f32;
            let mut idx = 0;
            while idx < self.icon_pulses.len() {
                let (btn_id, cur_t, kind) = self.icon_pulses[idx];
                let next_t = cur_t + step;
                if next_t >= 1.0 {
                    self.ui.button(btn_id).apply_over(
                        cx,
                        live! {
                            draw_bg: { pulse: 0.0 }
                            draw_icon: { pulse: 0.0, pulse_kind: (kind) }
                        },
                    );
                    self.icon_pulses.swap_remove(idx);
                } else {
                    self.icon_pulses[idx].1 = next_t;
                    self.ui.button(btn_id).apply_over(
                        cx,
                        live! {
                            draw_bg: { pulse: (next_t) }
                            draw_icon: { pulse: (next_t), pulse_kind: (kind) }
                        },
                    );
                    active = true;
                    idx += 1;
                }
            }
        }

        if active {
            self.motion_frame = cx.new_next_frame();
        } else {
            self.motion_last_t = 0.0;
        }
        self.ui.redraw(cx);
    }

    fn adaptive_palette(&self) -> AdaptivePalette {
        let raw = self.current_accent();
        let active_rgb = (
            (raw.x * 255.0).round().clamp(0.0, 255.0) as u8,
            (raw.y * 255.0).round().clamp(0.0, 255.0) as u8,
            (raw.z * 255.0).round().clamp(0.0, 255.0) as u8,
        );
        let key: PaletteCacheKey = (
            self.theme.acrylic_enabled,
            (self.theme.acrylic_opacity * 1000.0).round() as u16,
            active_rgb,
            parse_hex_rgb(&self.theme.canvas_hex).unwrap_or((10, 10, 14)),
            parse_hex_rgb(&self.theme.surface_hex).unwrap_or((15, 16, 22)),
            parse_hex_rgb(&self.theme.accent_hex).unwrap_or((232, 106, 44)),
            self.theme.acrylic_tint_rgb(),
        );
        if let Some((cached_key, cached_pal)) = *self.palette_cache.borrow()
            && cached_key == key
        {
            return cached_pal;
        }
        let pal = self.theme.adaptive_palette(active_rgb);
        *self.palette_cache.borrow_mut() = Some((key, pal));
        pal
    }

    fn style_chip_btn(&self, cx: &mut Cx, btn_id: &[LiveId], active: bool) {
        let a = if active { 1.0_f32 } else { 0.0_f32 };
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        let pal = self.adaptive_palette();
        let lm = if pal.is_light { 1.0_f32 } else { 0.0_f32 };
        let txt = rgba_to_vec4(pal.text_secondary);
        self.ui.button(btn_id).apply_over(
            cx,
            live! {
                draw_bg: { active: (a), acrylic_on: (acr), light_mode: (lm) }
                draw_text: { active: (a), color: (txt), light_mode: (lm) }
            },
        );
    }

    /// Fullscreen stage chips always sit over the dark blurred aurora backdrop and must never invert into light mode.
    fn style_fs_chip_btn(&self, cx: &mut Cx, btn_id: &[LiveId], active: bool) {
        let a = if active { 1.0_f32 } else { 0.0_f32 };
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        self.ui.button(btn_id).apply_over(
            cx,
            live! {
                draw_bg: { active: (a), acrylic_on: (acr), light_mode: 0.0 }
                draw_text: { active: (a), color: #C8CAD6, light_mode: 0.0 }
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

    fn target_acrylic_tint(&self) -> Vec4 {
        if self.theme.acrylic_enabled
            && self.theme.adaptive_acrylic_tint
            && let Some(cur) = self.queue.current()
            && let Some(art_cache) = &self.artwork
            && let Some(cached) = art_cache.get_for_track(cur)
        {
            let rgb = derive_acrylic_tint_from_accent(
                [cached.accent.x, cached.accent.y, cached.accent.z],
                self.theme.is_light_surface(),
            );
            return vec4(rgb[0], rgb[1], rgb[2], 1.0);
        }
        rgba_vec4(&self.theme.acrylic_tint_hex)
    }

    fn current_acrylic_tint(&self) -> Vec4 {
        if self.displayed_acrylic_tint.w > 0.5 {
            self.displayed_acrylic_tint
        } else {
            self.target_acrylic_tint()
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
            (236.0 * z).max(196.0)
        } else {
            0.0
        };
        self.left_w_cur = self.left_w_target;
        self.right_w_target = if self.right_sidebar_open {
            (260.0 * z).max(210.0)
        } else {
            0.0
        };
        self.right_w_cur = self.right_w_target;

        let bar_h = (74.0 * z).max(64.0);
        let left_grp_w = (300.0 * z).max(208.0);
        let btn_sz = (28.0 * z).max(22.0);
        let icon_h = (15.0 * z).max(12.0);
        let bar_pad_x = (14.0 * z).max(8.0);
        self.ui
            .view(id!(top_bar))
            .apply_over(cx, live! { height: ((36.0 * z).max(30.0)) });
        if self.left_sidebar_open {
            self.ui
                .view(id!(left_sidebar))
                .apply_over(cx, live! { width: ((236.0 * z).max(196.0)) });
        }
        if self.right_sidebar_open {
            self.ui
                .view(id!(right_sidebar))
                .apply_over(cx, live! { width: ((260.0 * z).max(210.0)) });
        }
        self.ui.view(id!(bottom_bar)).apply_over(
            cx,
            live! {
                height: (bar_h),
                padding: { left: (bar_pad_x), right: (bar_pad_x), top: 6.0, bottom: 6.0 },
                spacing: ((10.0 * z).max(6.0))
            },
        );
        self.ui.view(id!(toast_wrap)).apply_over(
            cx,
            live! { padding: { right: 16.0, bottom: (bar_h + 12.0) } },
        );
        self.ui
            .view(id!(bottom_left_grp))
            .apply_over(cx, live! { width: (left_grp_w) });
        self.ui
            .label(id!(pos_lbl))
            .apply_over(cx, live! { width: ((38.0 * z).max(30.0)) });
        self.ui
            .button(id!(dur_btn))
            .apply_over(cx, live! { width: ((40.0 * z).max(32.0)) });
        for pill_id in [id!(speed_pill_btn), id!(codec_pill_btn)] {
            self.ui.button(pill_id).apply_over(
                cx,
                live! {
                    height: ((22.0 * z).max(18.0)),
                    padding: { left: ((7.0 * z).max(4.5)), right: ((7.0 * z).max(4.5)) },
                    draw_text: { text_style: { font_size: ((7.8 * z).max(6.6)) } }
                },
            );
        }
        for b_id in [
            id!(shuffle_btn),
            id!(prev_btn),
            id!(next_btn),
            id!(bar_video_btn),
            id!(bar_lyrics_btn),
            id!(bar_queue_btn),
            id!(bar_fullscreen_btn),
        ] {
            self.ui.button(b_id).apply_over(
                cx,
                live! {
                    width: (btn_sz),
                    height: (btn_sz),
                    icon_walk: { width: Fit, height: (icon_h) }
                },
            );
        }
        self.ui
            .ferric_repeat_btn(id!(repeat_btn))
            .apply_over(cx, live! { width: (btn_sz), height: (btn_sz) });
        self.ui
            .ferric_volume_btn(id!(vol_mute_btn))
            .apply_over(cx, live! { width: (btn_sz), height: (btn_sz) });
        self.ui
            .ferric_bar_slider(id!(vol_slider))
            .apply_over(cx, live! { width: ((62.0 * z).max(46.0)) });
        let small_action_sz = (26.0 * z).max(22.0);
        let small_icon_sz = (15.0 * z).max(12.0);
        for np_btn in [id!(np_like_btn), id!(np_add_pl_btn)] {
            self.ui.button(np_btn).apply_over(
                cx,
                live! {
                    width: (small_action_sz),
                    height: (small_action_sz),
                    icon_walk: { width: (small_icon_sz), height: (small_icon_sz) }
                },
            );
        }
        self.ui.button(id!(np_copy_link_btn)).apply_over(
            cx,
            live! {
                width: ((28.0 * z).max(24.0)),
                height: (small_action_sz),
                icon_walk: { width: ((16.0 * z).max(12.0)), height: ((9.0 * z).max(7.0)) }
            },
        );
        let art_sz = (44.0 * z).max(34.0);
        let play_sz = (34.0 * z).max(28.0);
        self.ui
            .ferric_cover_art(id!(np_art))
            .apply_over(cx, live! { width: (art_sz), height: (art_sz) });
        self.ui
            .ferric_play_btn(id!(play_pause_btn))
            .apply_over(cx, live! { width: (play_sz), height: (play_sz) });
        self.ui.label(id!(np_title_lbl)).apply_over(
            cx,
            live! { draw_text: { text_style: { font_size: ((9.4 * z).max(7.6)) } } },
        );
        self.ui.label(id!(np_artist_lbl)).apply_over(
            cx,
            live! { draw_text: { text_style: { font_size: ((8.2 * z).max(6.8)) } } },
        );
        self.sync_fullscreen_media_layout(cx);

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
            let qv = self.ui.view(vid);
            qv.apply_over(
                cx,
                live! {
                    height: (48.0 * z),
                    art = { width: (36.0 * z), height: (36.0 * z) }
                },
            );
            qv.label(id!(title)).apply_over(
                cx,
                live! { draw_text: { text_style: { font_size: (9.4 * z) } } },
            );
            qv.label(id!(sub)).apply_over(
                cx,
                live! { draw_text: { text_style: { font_size: (8.2 * z) } } },
            );
        }

        for sid in [id!(st_c0), id!(st_c1), id!(st_c2), id!(st_c3), id!(st_c4)] {
            let sv = self.ui.view(sid);
            sv.apply_over(
                cx,
                live! {
                    height: (188.0 * z),
                    inner = { art_stage = { height: (124.0 * z) } }
                },
            );
            sv.ferric_cover_art(id!(art_center))
                .apply_over(cx, live! { width: (64.0 * z), height: (64.0 * z) });
            sv.ferric_cover_art(id!(art_left))
                .apply_over(cx, live! { width: (44.0 * z), height: (44.0 * z) });
            sv.ferric_cover_art(id!(art_right))
                .apply_over(cx, live! { width: (44.0 * z), height: (44.0 * z) });
            sv.label(id!(card_title)).apply_over(
                cx,
                live! { draw_text: { text_style: { font_size: (9.8 * z) } } },
            );
            sv.label(id!(title)).apply_over(
                cx,
                live! { draw_text: { text_style: { font_size: (9.0 * z) } } },
            );
            sv.label(id!(sub)).apply_over(
                cx,
                live! { draw_text: { text_style: { font_size: (7.8 * z) } } },
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
            let qmv = self.ui.view(qid);
            qmv.apply_over(
                cx,
                live! {
                    height: (44.0 * z),
                    art = { width: (32.0 * z), height: (32.0 * z) }
                },
            );
            qmv.label(id!(title)).apply_over(
                cx,
                live! { draw_text: { text_style: { font_size: (9.0 * z) } } },
            );
            qmv.label(id!(sub)).apply_over(
                cx,
                live! { draw_text: { text_style: { font_size: (8.0 * z) } } },
            );
        }

        let raw_accent = self.current_accent();
        let pal = self.adaptive_palette();
        let accent = rgba_to_vec4(pal.accent);
        self.ui.ferric_lyrics_view(id!(rs_lyrics_scroll)).set_style(
            cx,
            accent,
            z as f32,
            pal.is_light,
        );
        self.ui
            .ferric_lyrics_view(id!(fs_lyrics_scroll))
            .set_style(cx, raw_accent, z as f32, false);

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
            // Fallback geometry matching fs_left_card's expanded 16:9 video theater inside Fullscreen Lyrics Mode
            let left_px = (64.0 * scale).round() as i32;
            let top_px = (128.0 * scale).round() as i32;
            let width_px = (560.0 * scale).round() as i32;
            let height_px = (315.0 * scale).round() as i32;
            Some(VideoViewportRect {
                x: client.left + left_px,
                y: client.top + top_px,
                width: width_px.min((client.width - left_px * 2).max(280)),
                height: height_px.min((client.height - top_px - 180).max(160)),
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
        ] {
            self.style_fs_chip_btn(cx, btn, cur_q == q);
        }
        for (btn, q) in [
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
        let (pos_sec, playing) = if let Some(peer) = self.active_remote_peer() {
            (
                peer.interpolated_position_ms() as f64 / 1000.0,
                peer.is_playing,
            )
        } else {
            self.audio
                .as_ref()
                .map(|a| {
                    let s = a.telemetry();
                    (s.position_ms as f64 / 1000.0, s.is_playing)
                })
                .unwrap_or((0.0, true))
        };
        let mv_tag = if self.video_is_official_mv {
            "Official MV"
        } else {
            "Audio Cover (No Official MV)"
        };
        self.ui.label(id!(vid_status_lbl)).set_text(
            cx,
            &format!(
                "{mv_tag} · {} ({}p · {} kbps)",
                chosen.label, chosen.height, chosen.bitrate_kbps
            ),
        );
        self.ui.label(id!(vid_stage_hint_lbl)).set_text(cx, "");
        let initially_visible = self.fullscreen_open
            && self.fs_reveal_t >= 0.96
            && self.fs_video_morph_t >= 0.96
            && !self.cmdk_open
            && !self.track_menu_open
            && !self.update_popup_visible
            && !win32_is_window_hidden_or_minimized();
        let owner = win32_main_hwnd_raw();
        if let Some(vp) = &mut self.video_player {
            vp.open_or_load(owner, rect, chosen.url, pos_sec, playing, initially_visible);
        }
        if !initially_visible && self.fullscreen_open {
            self.fs_video_bounds_sync_pending = true;
            self.start_motion(cx);
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
            || self.fs_reveal_t < 0.96
            || self.fs_video_morph_t < 0.96
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

    fn smoothstep01(x: f32) -> f32 {
        let c = x.clamp(0.0, 1.0);
        c * c * (3.0 - 2.0 * c)
    }

    fn sync_fullscreen_media_layout(&self, cx: &mut Cx) {
        let z = self.theme.ui_zoom.clamp(0.65, 1.40) as f64;
        let t = self.fs_video_morph_t.clamp(0.0, 1.0);
        let tf = f64::from(t);

        let cover_card_w = 410.0 * z;
        let video_card_w = (600.0 * z).min(680.0);
        let card_w = cover_card_w + (video_card_w - cover_card_w) * tf;

        let cover_box = 316.0 * z;
        let video_box_w = (568.0 * z).min(640.0);
        let video_frame_h_final = (video_box_w * 9.0 / 16.0).round();
        let video_box_h = video_frame_h_final + 64.0;
        let box_w = cover_box + (video_box_w - cover_box) * tf;
        let box_h = cover_box + (video_box_h - cover_box) * tf;
        let cur_frame_h = (box_w * 9.0 / 16.0).round().max(110.0);

        let reveal_content = Self::smoothstep01((self.fs_reveal_t - 0.52) / 0.48);
        let reveal_scale = 0.92 + 0.08 * f64::from(reveal_content);
        let art_sz = (304.0 - 28.0 * tf) * z * reveal_scale;
        let art_alpha = Self::smoothstep01(1.0 - t / 0.62) * reveal_content;
        let art_radius = 28.0 - 8.0 * t;
        let vid_alpha = Self::smoothstep01((t - 0.14) / 0.86) * reveal_content;
        let hint_alpha = Self::smoothstep01((t - 0.30) / 0.70) * reveal_content;
        let ctrl_alpha = Self::smoothstep01((t - 0.48) / 0.52) * reveal_content;

        self.ui
            .view(id!(fs_left_card))
            .apply_over(cx, live! { width: (card_w) });
        self.ui
            .view(id!(fs_media_box))
            .apply_over(cx, live! { width: (box_w), height: (box_h) });

        let show_art = art_alpha > 0.008;
        self.ui.view(id!(fs_art_wrap)).set_visible(cx, show_art);
        if show_art {
            let art = self.ui.ferric_cover_art(id!(fs_art));
            art.apply_over(cx, live! { width: (art_sz), height: (art_sz) });
            art.set_morph_visual(cx, art_alpha, art_radius);
        }

        let show_vid = t > 0.04;
        self.ui.view(id!(fs_video_wrap)).set_visible(cx, show_vid);
        if show_vid {
            self.ui.view(id!(video_stage_frame)).apply_over(
                cx,
                live! { height: (cur_frame_h), draw_bg: { reveal: (vid_alpha) } },
            );
            let hint_col = vec4(0.824, 0.839, 0.894, hint_alpha);
            self.ui
                .label(id!(vid_stage_hint_lbl))
                .apply_over(cx, live! { draw_text: { color: (hint_col) } });

            let show_footer = t > 0.48;
            self.ui
                .view(id!(fs_video_footer_inner))
                .set_visible(cx, show_footer);
            if show_footer {
                let status_col = vec4(1.0, 1.0, 1.0, 0.48 * ctrl_alpha);
                self.ui
                    .label(id!(vid_status_lbl))
                    .apply_over(cx, live! { draw_text: { color: (status_col) } });
            }
        }
    }

    fn open_native_video_stage(&mut self, cx: &mut Cx) {
        let Some(cur) = self.effective_display_track() else {
            self.show_toast(cx, "Play a song first to watch its video clip");
            return;
        };
        let was_fullscreen = self.fullscreen_open;
        // Video Clip lives INSIDE Fullscreen Lyrics Mode replacing the Left Cover Art card
        if !self.fullscreen_open {
            self.set_fullscreen(cx, true);
        }
        self.video_open = true;
        self.fs_video_morph_target = 1.0;
        if !was_fullscreen || self.fs_video_morph_t >= 0.995 {
            self.fs_video_morph_t = 1.0;
            self.fs_video_bounds_sync_pending = true;
        }
        self.sync_fullscreen_media_layout(cx);
        self.start_motion(cx);
        self.style_fs_chip_btn(cx, id!(fs_mode_art_btn), false);
        self.style_fs_chip_btn(cx, id!(fs_mode_video_btn), true);
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
            self.last_video_sync = None;
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
                    let uri = crate::video_player::path_to_file_uri(&cand);
                    self.video_track_id = Some(cur.id.clone());
                    self.video_is_official_mv = true;
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
        self.video_is_official_mv = false;
        self.video_streams.clear();
        self.ui
            .label(id!(vid_status_lbl))
            .set_text(cx, "Searching for Official Music Video...");
        self.ui
            .label(id!(vid_stage_hint_lbl))
            .set_text(cx, "Matching official music video clip...");
        self.ui.redraw(cx);

        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let session = self.current_ytm_session();
        let track_id = cur.id;
        let artist = cur.artist;
        let title = cur.title;
        let track_dur_ms = cur.duration_ms;
        let existing_vid = cur.youtube_video_id;
        thread::spawn(move || {
            let res = resolve_music_video_streams(
                &session,
                &artist,
                &title,
                existing_vid.as_deref(),
                track_dur_ms,
            )
            .map_err(|e| e.to_string());
            match res {
                Ok(resolved) => {
                    let _ = tx.send(BgMessage::VideoStreamsLoaded {
                        track_id,
                        video_id: resolved.video_id,
                        is_official_mv: resolved.is_official_mv,
                        streams: resolved.streams,
                    });
                }
                Err(error) => {
                    let _ = tx.send(BgMessage::VideoStreamsFailed { track_id, error });
                }
            }
        });
    }

    fn close_native_video_stage(&mut self, cx: &mut Cx) {
        self.close_native_video_stage_inner(cx, self.fullscreen_open);
    }

    fn close_native_video_stage_inner(&mut self, cx: &mut Cx, animate: bool) {
        self.video_open = false;
        self.fs_video_morph_target = 0.0;
        self.fs_video_bounds_sync_pending = false;
        if let Some(vp) = &mut self.video_player {
            vp.close();
        }
        if !animate {
            self.fs_video_morph_t = 0.0;
        }
        self.sync_fullscreen_media_layout(cx);
        if animate && self.fs_video_morph_t > 0.004 {
            self.start_motion(cx);
        }
        self.style_fs_chip_btn(cx, id!(fs_mode_art_btn), true);
        self.style_fs_chip_btn(cx, id!(fs_mode_video_btn), false);
        let muted = rgba_to_vec4(self.adaptive_palette().icon_idle);
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
        self.active_page = MainPage::Home;
        self.active_settings_tab = SettingsTab::Accounts;
        self.nav_history = Some(NavHistory::new(MainPage::Home));
        self.left_sidebar_open = true;
        self.right_sidebar_open = true;
        self.right_lyrics_tab = false;
        self.page_reveal_t = 1.0;
        self.fs_reveal_t = 0.0;
        self.fs_reveal_target = 0.0;
        self.fs_video_morph_t = 0.0;
        self.fs_video_morph_target = 0.0;
        self.fs_video_bounds_sync_pending = false;
        self.cmdk_reveal_t = 1.0;
        self.toast_reveal_t = 1.0;
        self.missing_art_queue = Rc::new(RefCell::new(Vec::new()));
        self.pre_mute_volume = 0.85;
        self.queue_context = "Quick picks".to_string();
        self.artwork = Some(ArtworkCache::new());
        self.video_player = Some(NativeVideoPlayer::new(
            app_data_dir().join("webview2_video"),
        ));
        let db_path = app_data_dir().join("ferric.db");
        let mut db = LibraryDb::open(&db_path)
            .or_else(|_| LibraryDb::open_in_memory())
            .ok();

        if let Some(store) = &mut db {
            self.theme = store.load_theme().unwrap_or_default();
            // Upgrade deprecated Liquid Glass preset or corrupted hex configs while preserving valid custom themes
            let core_hex_ok = parse_hex_rgb(&self.theme.canvas_hex).is_ok()
                && parse_hex_rgb(&self.theme.surface_hex).is_ok()
                && parse_hex_rgb(&self.theme.border_hex).is_ok()
                && parse_hex_rgb(&self.theme.text_primary_hex).is_ok()
                && parse_hex_rgb(&self.theme.text_secondary_hex).is_ok();
            if self.theme.name == "Liquid Glass" || !core_hex_ok {
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
        }

        // Seed DWM Acrylic state BEFORE window creation (`Win32Window::init`) and BEFORE installing
        // Win32 window hooks so frame 0 renders with native blur immediately on cold launch.
        cx.set_win32_initial_acrylic(self.theme.acrylic_enabled);
        win32_set_native_acrylic(
            self.theme.acrylic_enabled,
            self.theme.acrylic_opacity,
            self.theme.acrylic_tint_rgb(),
            self.theme.is_light_surface(),
        );
        self.hide_native_makepad_caption(cx);
        win32_install_window_hooks();

        if let Some(store) = &mut db {
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
            let sp_oauth_signed_in = self.accounts.spotify_connected
                && !self.accounts.spotify_access_token.trim().is_empty();
            let sp_web_signed_in = self.accounts.spotify_web_connected
                && (!self.accounts.spotify_web_access_token.trim().is_empty()
                    || !self.accounts.spotify_web_cookie.trim().is_empty());
            if !sp_oauth_signed_in {
                let _ = store.delete_spotify_premium_synced_playlists();
            }
            if !sp_web_signed_in {
                let _ = store.delete_spotify_web_synced_playlists();
            }
            self.update_settings = store.load_update_settings().unwrap_or_default();
            self.session_state = store.load_session_state().unwrap_or_default();
            let _ = store.prune_ephemeral_discovery_bloat(&self.session_state.recent_track_ids);
            self.all_tracks = store.list_tracks("").unwrap_or_default();
            self.playlists = store.list_playlists().unwrap_or_default();
        }
        let init_z = self.theme.ui_zoom.clamp(0.65, 1.40) as f64;
        self.left_w_cur = 236.0 * init_z;
        self.left_w_target = 236.0 * init_z;
        self.right_w_cur = 260.0 * init_z;
        self.right_w_target = 260.0 * init_z;
        self.displayed_accent = rgba_vec4(&self.theme.accent_hex);
        self.displayed_acrylic_tint = rgba_vec4(&self.theme.acrylic_tint_hex);
        win32_set_close_to_tray(self.dsp.close_to_tray);
        self.queue.shuffle = self.session_state.shuffle;
        self.queue.repeat = self.session_state.repeat;
        self.db = db;

        self.output_devices = list_output_devices();
        let audio = AudioEngine::start_with_device(self.eq.clone(), self.dsp.output_device.clone())
            .unwrap_or_else(|_| AudioEngine::start_headless(self.eq.clone()));
        audio.send(EngineCommand::SetDspSettings(self.dsp.clone()));
        audio.send(EngineCommand::SetVolume(self.session_state.volume));
        self.audio = Some(audio);

        self.smtc = Some(SmtcBridge::try_init());
        self.discord_rpc = Some(DiscordRpcBridge::start(self.discord_settings.clone()));
        self.lan_ip = detect_local_lan_ip();

        let (tx, rx) = mpsc::channel();
        self.bg_tx = Some(tx.clone());
        self.bg_rx = Some(rx);

        let dev_id = persistent_connect_device_id();
        let pc_name = std::env::var("COMPUTERNAME")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Desktop-PC".to_string());
        let conn_tx = tx;
        if let Ok(engine) = ConnectEngine::start(
            ConnectConfig {
                device_id: dev_id,
                device_name: pc_name,
                device_kind: DeviceKind::Desktop,
                bind_ephemeral_udp: false,
            },
            Arc::new(move |ev| {
                let _ = conn_tx.send(BgMessage::Connect(Box::new(ev)));
                SignalToUI::set_ui_signal();
            }),
        ) {
            let engine = Arc::new(engine);
            self.connect_snap = Some(engine.snapshot());
            self.connect_engine = Some(engine);
            self.sync_connect_accounts();
        }

        if let Some(brand_buf) = decode_and_downscale_image(BRAND_ICON_PNG, 128) {
            let tex = brand_buf.into_new_texture(cx);
            tex.set_release_cpu_after_upload(cx, true);
            self.ui
                .image(id!(brand_icon))
                .set_texture(cx, Some(tex.clone()));
            self.ui
                .image(id!(about_brand_icon))
                .set_texture(cx, Some(tex));
        } else {
            let _ = self
                .ui
                .image(id!(brand_icon))
                .load_png_from_data(cx, BRAND_ICON_PNG);
            let _ = self
                .ui
                .image(id!(about_brand_icon))
                .load_png_from_data(cx, BRAND_ICON_PNG);
        }
        self.ui
            .label(id!(about_version_lbl))
            .set_text(cx, &format!("v{APP_VERSION} · Windows x64 · Native Rust"));
        self.ui
            .label(id!(lbl_t_st_update))
            .set_text(cx, &format!("Software Updates (Ferric v{APP_VERSION})"));

        self.regenerate_pairing_qr(cx);
        self.rebuild_library_views(cx);
        self.refresh_spotify_account_ui(cx);
        // Sync persisted connected status labels on cold start so Settings > Accounts never displays stale defaults.
        if self.accounts.youtube_connected && !self.accounts.youtube_sapisid.is_empty() {
            let yt_pl_n = self
                .playlists
                .iter()
                .filter(|p| PlaylistSourceFilter::YouTube.matches_playlist(p))
                .count();
            let name = self.accounts.youtube_channel_name.trim();
            self.ui.label(id!(oauth_status_lbl)).set_text(
                cx,
                &if name.is_empty() {
                    format!("Signed in with Google · {yt_pl_n} synced playlists.")
                } else {
                    format!("Signed in as {name} · {yt_pl_n} synced playlists.")
                },
            );
            if self.accounts.youtube_account_id.trim().is_empty() {
                self.sync_ytm_signed_in_library(cx);
            }
        }
        if self.is_spotify_web_session_active() {
            let web_pl_n = self
                .playlists
                .iter()
                .filter(|p| PlaylistSourceFilter::Spotify.matches_playlist(p))
                .count();
            let name = self.accounts.spotify_web_user_name.trim();
            self.ui.label(id!(spotify_web_status_lbl)).set_text(
                cx,
                &if name.is_empty() {
                    format!("Spotify connected · {web_pl_n} playlists synced.")
                } else {
                    format!("Spotify: {name} · {web_pl_n} playlists synced.")
                },
            );
            if self.accounts.spotify_user_id.trim().is_empty() {
                self.sync_spotify_library_for(cx, true, 400);
            }
        } else if self.is_spotify_oauth_session_active()
            && self.accounts.spotify_user_id.trim().is_empty()
        {
            self.sync_spotify_library_for(cx, false, 400);
        }
        if !self.all_tracks.is_empty() {
            self.queue.set_tracks(self.all_tracks.clone(), None);
        }
        self.sync_controls_from_state(cx);
        self.apply_theme_live(cx);
        self.apply_ui_zoom(cx);
        self.refresh_home_ui(cx);
        self.refresh_right_sidebar(cx);
        self.refresh_storage_stats(cx);
        self.tick_interval_ms = 100;
        self.tick_timer = cx.start_interval(0.10);

        if startup_load_demo() {
            self.load_and_play_demo_suite(cx);
        } else {
            self.spawn_ytm_rich_home(cx, 0, None, !self.accounts.youtube_connected);
            self.spawn_recommended_stations(cx, false);
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
            "about" => {
                self.navigate_to(cx, MainPage::Settings, false);
                self.switch_settings_tab(cx, SettingsTab::About);
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

    fn navigate_to_entry(&mut self, cx: &mut Cx, entry: NavEntry) {
        if entry.page == MainPage::Detail
            && let Some(det) = entry.detail
        {
            self.detail_filter_query.clear();
            self.detail_sort_order = DetailSortOrder::Original;
            self.ui
                .text_input(id!(detail_filter_input))
                .set_text(cx, "");
            self.detail_tracks = Rc::new(det.tracks.clone());
            self.detail_state = det;
            self.refresh_detail_hero(cx);
        }
        self.navigate_to(cx, entry.page, false);
    }

    #[inline]
    fn is_track_list_page(page: MainPage) -> bool {
        matches!(
            page,
            MainPage::Songs | MainPage::Recents | MainPage::Search | MainPage::Detail
        )
    }

    fn ensure_artwork_snapshot(&mut self) {
        if self.artwork_snapshot_dirty {
            if let Some(cache) = &self.artwork {
                self.artwork_snapshot = Rc::new(cache.snapshot());
            }
            self.artwork_snapshot_dirty = false;
        }
    }

    fn navigate_to(&mut self, cx: &mut Cx, target: MainPage, push_history: bool) {
        if self.video_open {
            self.close_native_video_stage_inner(cx, false);
        }
        let prev_page = self.active_page;
        if prev_page != target {
            if Self::is_track_list_page(prev_page) && !Self::is_track_list_page(target) {
                // Drop extra Texture handles held by the TrackList snapshot before reclaiming VRAM.
                self.artwork_snapshot = Rc::new(HashMap::new());
                self.artwork_snapshot_dirty = true;
            }
            self.unbind_page_cover_textures(cx, prev_page);
            cx.textures.free_unused_textures();
        }
        self.active_page = target;
        if Self::is_track_list_page(target) {
            self.ensure_artwork_snapshot();
        }
        if push_history && let Some(hist) = &mut self.nav_history {
            let det = if target == MainPage::Detail {
                Some(self.detail_state.clone())
            } else {
                None
            };
            hist.push(target, det);
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

        let pal = self.adaptive_palette();
        let bright = rgba_to_vec4(pal.text_primary);
        let muted = rgba_to_vec4(pal.text_secondary);
        let icon_muted = rgba_to_vec4(pal.icon_idle);
        let icon_hov = rgba_to_vec4(pal.icon_hover);
        let disabled_col = if pal.is_light {
            vec4(0.06, 0.08, 0.12, 0.34)
        } else {
            vec4(0.32, 0.34, 0.40, 1.0)
        };
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        let lm = if pal.is_light { 1.0_f32 } else { 0.0_f32 };
        for (p, btn_id) in [
            (MainPage::Home, id!(nav_home)),
            (MainPage::Search, id!(nav_search)),
            (MainPage::Recents, id!(nav_recents)),
        ] {
            let active = p == target;
            self.ui.button(btn_id).apply_over(
                cx,
                live! {
                    draw_bg: { active: (if active { 1.0 } else { 0.0 }), acrylic_on: (acr), light_mode: (lm) }
                    draw_icon: { color: (if active { bright } else { icon_muted }), color_hover: (icon_hov), color_focus: (icon_hov), color_down: (icon_hov) }
                },
            );
        }
        for (p, btn_id) in [
            (MainPage::Playlists, id!(nav_playlists)),
            (MainPage::Songs, id!(nav_songs)),
            (MainPage::Albums, id!(nav_albums)),
            (MainPage::Artists, id!(nav_artists)),
        ] {
            let active = p == target;
            self.ui.button(btn_id).apply_over(
                cx,
                live! {
                    draw_bg: { active: (if active { 1.0 } else { 0.0 }), acrylic_on: (acr), light_mode: (lm) }
                    draw_text: { active: (if active { 1.0 } else { 0.0 }), light_mode: (lm), color: (muted) }
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
            live! { draw_icon: { color: (if can_b { bright } else { disabled_col }) } },
        );
        self.ui.button(id!(nav_fwd_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if can_f { bright } else { disabled_col }) } },
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
        self.refresh_sidebar_library(cx);
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
            (SettingsTab::About, id!(tab_about), id!(st_about_btn)),
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
        } else if tab == SettingsTab::Connect {
            self.refresh_connect_panel(cx);
        }
        self.ui.redraw(cx);
    }

    fn sync_sidebar_toggle_buttons(&self, cx: &mut Cx) {
        let pal = self.adaptive_palette();
        let muted = rgba_to_vec4(pal.icon_idle);
        let bright = rgba_to_vec4(pal.text_primary);
        self.ui.button(id!(toggle_left_btn)).apply_over(
            cx,
            live! {
                draw_bg: { active: (if self.left_sidebar_open { 1.0 } else { 0.0 }) }
                draw_icon: { color: (if self.left_sidebar_open { bright } else { muted }) }
            },
        );
        self.ui.button(id!(toggle_right_btn)).apply_over(
            cx,
            live! {
                draw_bg: { active: (if self.right_sidebar_open { 1.0 } else { 0.0 }) }
                draw_icon: { color: (if self.right_sidebar_open { bright } else { muted }) }
            },
        );
    }

    fn set_right_sidebar(&mut self, cx: &mut Cx, open: bool, lyrics_tab: bool) {
        self.set_right_sidebar_mode(cx, open, lyrics_tab, false);
    }

    fn set_right_sidebar_mode(
        &mut self,
        cx: &mut Cx,
        open: bool,
        lyrics_tab: bool,
        connect_tab: bool,
    ) {
        let z = self.theme.ui_zoom.clamp(0.65, 1.40) as f64;
        self.right_sidebar_open = open;
        self.right_lyrics_tab = lyrics_tab && !connect_tab;
        self.right_connect_tab = connect_tab;
        let queue_tab = !self.right_lyrics_tab && !self.right_connect_tab;
        self.right_w_target = if open { 260.0 * z } else { 0.0 };
        if open {
            self.ui.view(id!(right_sidebar)).set_visible(cx, true);
            self.ui.view(id!(right_divider)).set_visible(cx, true);
        }
        self.ui.view(id!(rs_queue_view)).set_visible(cx, queue_tab);
        self.ui
            .view(id!(rs_lyrics_view))
            .set_visible(cx, self.right_lyrics_tab);
        self.ui
            .view(id!(rs_connect_view))
            .set_visible(cx, self.right_connect_tab);

        self.style_chip_btn(cx, id!(rs_tab_queue), queue_tab);
        self.style_chip_btn(cx, id!(rs_tab_lyrics), self.right_lyrics_tab);
        self.style_chip_btn(cx, id!(rs_tab_connect), self.right_connect_tab);

        let pal = self.adaptive_palette();
        let accent = rgba_to_vec4(pal.accent);
        let muted = rgba_to_vec4(pal.icon_idle);
        let has_peer = self
            .connect_snap
            .as_ref()
            .is_some_and(|s| s.peers.iter().any(|p| p.connected));
        self.sync_sidebar_toggle_buttons(cx);
        self.ui.button(id!(bar_lyrics_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if open && self.right_lyrics_tab { accent } else { muted }) } },
        );
        self.ui.button(id!(bar_queue_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if open && queue_tab { accent } else { muted }) } },
        );
        self.ui.button(id!(bar_connect_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if (open && self.right_connect_tab) || has_peer { accent } else { muted }) } },
        );
        if self.right_connect_tab {
            self.refresh_connect_panel(cx);
        }
        self.start_motion(cx);
        self.sync_video_bounds(cx);
        let pos_ms = self.active_remote_peer().map_or_else(
            || self.audio.as_ref().map_or(0, |a| a.telemetry().position_ms),
            |p| p.interpolated_position_ms(),
        );
        self.update_lyrics_display(cx, pos_ms);
        self.ui.redraw(cx);
    }

    fn apply_fullscreen_reveal(&self, cx: &mut Cx) {
        let t = self.fs_reveal_t.clamp(0.0, 1.0);
        // 2-phase choreography: aurora backdrop dissolves to 100% opaque first (0.0 -> 0.58),
        // then stage content glides up and blooms cleanly (0.52 -> 1.0) with zero UI ghosting over shell_col.
        let bg_alpha = Self::smoothstep01(t / 0.58);
        let content_alpha = Self::smoothstep01((t - 0.52) / 0.48);
        let slide_y = (1.0 - f64::from(content_alpha)) * 42.0;
        let top_pad = 14.0 + slide_y;
        let bottom_pad = (18.0 - slide_y * 0.25).max(0.0);

        self.ui
            .ferric_ambient_stage(id!(fs_backdrop))
            .set_opacity(cx, bg_alpha);

        let show_stage = t > 0.52;
        self.ui
            .view(id!(fs_stage_content))
            .set_visible(cx, show_stage);
        if show_stage {
            self.ui.view(id!(fs_stage_content)).apply_over(
                cx,
                live! { padding: { left: 48.0, right: 38.0, top: (top_pad), bottom: (bottom_pad) } },
            );
            self.ui
                .ferric_lyrics_view(id!(fs_lyrics_scroll))
                .set_reveal_alpha(cx, content_alpha);
            self.ui.label(id!(fs_title_lbl)).apply_over(
                cx,
                live! { draw_text: { color: (vec4(1.0, 1.0, 1.0, content_alpha)) } },
            );
            self.ui.label(id!(fs_artist_lbl)).apply_over(
                cx,
                live! { draw_text: { color: (vec4(1.0, 1.0, 1.0, 0.55 * content_alpha)) } },
            );
            self.ui.label(id!(signal_path_lbl)).apply_over(
                cx,
                live! { draw_text: { color: (vec4(1.0, 1.0, 1.0, 0.62 * content_alpha)) } },
            );
            self.ui.label(id!(fs_lyr_source)).apply_over(
                cx,
                live! { draw_text: { color: (vec4(1.0, 1.0, 1.0, 0.62 * content_alpha)) } },
            );
            self.sync_fullscreen_media_layout(cx);
        }
    }

    fn finish_fullscreen_close(&mut self, cx: &mut Cx) {
        self.fs_reveal_t = 0.0;
        self.fs_reveal_target = 0.0;
        self.fs_video_morph_t = 0.0;
        self.fs_video_morph_target = 0.0;
        self.fs_video_bounds_sync_pending = false;
        self.last_fs_backdrop_playing = None;
        self.ui.view(id!(fullscreen_overlay)).set_visible(cx, false);
        self.ui.view(id!(fs_stage_content)).set_visible(cx, true);
        self.ui.view(id!(shell_col)).set_visible(cx, true);
        let accent = self.current_accent();
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        let pos_ms = self.audio.as_ref().map_or(0, |a| a.telemetry().position_ms);
        self.ui
            .ferric_ambient_stage(id!(fs_backdrop))
            .set_state(cx, false, false, accent, acr);
        self.ui
            .ferric_ambient_stage(id!(fs_backdrop))
            .set_opacity(cx, 0.0);
        self.ui
            .ferric_lyrics_view(id!(fs_lyrics_scroll))
            .set_playback_state(cx, pos_ms, false);
        self.ui
            .ferric_lyrics_view(id!(fs_lyrics_scroll))
            .set_reveal_alpha(cx, 1.0);
        self.sync_fullscreen_media_layout(cx);
        if let Some(db) = &self.db {
            db.shrink_memory();
        }
        win32_trim_working_set();
    }

    fn set_fullscreen(&mut self, cx: &mut Cx, open: bool) {
        if !open && self.video_open {
            self.close_native_video_stage_inner(cx, false);
        }
        self.fullscreen_open = open;
        self.last_pos_txt.clear();
        self.last_dur_txt.clear();
        // Keep `shell_col` visible underneath during both Open & Close transitions so the
        // Fullscreen ambient sheet glides and dissolves smoothly over the main player shell.
        self.ui.view(id!(shell_col)).set_visible(cx, true);
        self.ui.view(id!(fullscreen_overlay)).set_visible(cx, true);
        let accent = self.current_accent();
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        let (is_playing, pos_ms) = if let Some(peer) = self.active_remote_peer() {
            (peer.is_playing, peer.interpolated_position_ms())
        } else {
            self.audio.as_ref().map_or((false, 0), |a| {
                let t = a.telemetry();
                (t.is_playing, t.position_ms)
            })
        };
        if open {
            let t0 = if self.video_open { 1.0 } else { 0.0 };
            self.fs_video_morph_t = t0;
            self.fs_video_morph_target = t0;
            self.fs_video_bounds_sync_pending = self.video_open;
            self.fs_reveal_target = 1.0;
            self.ui
                .ferric_ambient_stage(id!(fs_backdrop))
                .set_state(cx, true, is_playing, accent, acr);
            self.last_fs_backdrop_playing = Some(is_playing);
            self.sync_fullscreen_backdrop_artwork(cx);
            self.style_fs_chip_btn(cx, id!(fs_mode_art_btn), !self.video_open);
            self.style_fs_chip_btn(cx, id!(fs_mode_video_btn), self.video_open);
            self.apply_fullscreen_reveal(cx);
            self.start_motion(cx);
        } else {
            self.fs_video_bounds_sync_pending = false;
            self.fs_reveal_target = 0.0;
            if self.fs_reveal_t <= 0.01 {
                self.finish_fullscreen_close(cx);
            } else {
                // Keep backdrop alive for the ~220ms exit dissolve, then stop cleanly in `finish_fullscreen_close`
                self.ui
                    .ferric_ambient_stage(id!(fs_backdrop))
                    .set_state(cx, true, false, accent, acr);
                self.apply_fullscreen_reveal(cx);
                self.start_motion(cx);
            }
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
        if self.window_hidden_or_minimized {
            return;
        }
        if let Some(cache) = &mut self.artwork {
            for t in tracks.iter().take(14) {
                let _ = cache.request_for_track(t);
            }
        }
    }

    fn request_artwork_for_cards(&mut self, cards: &[YtmShelfCard]) {
        if self.window_hidden_or_minimized {
            return;
        }
        if let Some(cache) = &mut self.artwork {
            for c in cards.iter().take(6) {
                if let Some(url) = c.thumbnail_url.as_deref() {
                    let _ = cache.request_url(url);
                }
            }
        }
    }

    #[inline]
    fn trim_gpu_memory_now(cx: &Cx) {
        #[cfg(target_os = "windows")]
        cx.trim_gpu_memory();
        #[cfg(not(target_os = "windows"))]
        let _ = cx;
    }

    fn sync_tick_timer_rate(&mut self, cx: &mut Cx, desired_ms: u32) {
        if self.tick_interval_ms == desired_ms {
            return;
        }
        if !self.tick_timer.is_empty() {
            cx.stop_timer(self.tick_timer);
        }
        self.tick_interval_ms = desired_ms;
        self.tick_timer = cx.start_interval(f64::from(desired_ms) / 1_000.0);
    }

    fn clear_makepad_font_caches(cx: &mut Cx) {
        type FontsGlobal = Rc<RefCell<makepad_widgets::makepad_draw::text::fonts::Fonts>>;
        if cx.has_global::<FontsGlobal>() {
            cx.get_global::<FontsGlobal>().borrow_mut().clear_caches();
        }
    }

    fn unbind_page_cover_textures(&self, cx: &mut Cx, page: MainPage) {
        match page {
            MainPage::Albums => {
                for vid in [
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
                ] {
                    self.ui
                        .view(vid)
                        .ferric_cover_art(id!(art))
                        .set_texture(cx, None, 1.0);
                }
            }
            MainPage::Artists => {
                for vid in [
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
                ] {
                    self.ui
                        .view(vid)
                        .ferric_cover_art(id!(art))
                        .set_texture(cx, None, 1.0);
                }
            }
            MainPage::Playlists => {
                for vid in [
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
                    self.ui
                        .view(vid)
                        .ferric_cover_art(id!(art))
                        .set_texture(cx, None, 1.0);
                }
            }
            MainPage::Search => {
                for cid in [id!(sc_0), id!(sc_1), id!(sc_2), id!(sc_3), id!(sc_4)] {
                    self.ui
                        .view(cid)
                        .ferric_cover_art(id!(art))
                        .set_texture(cx, None, 1.0);
                }
                self.ui
                    .track_list(id!(search_track_list))
                    .clear_row_textures(cx);
            }
            MainPage::Songs => {
                self.ui
                    .track_list(id!(songs_track_list))
                    .clear_row_textures(cx);
            }
            MainPage::Recents => {
                self.ui
                    .track_list(id!(recents_track_list))
                    .clear_row_textures(cx);
            }
            MainPage::Detail => {
                self.ui
                    .ferric_cover_art(id!(detail_art))
                    .set_texture(cx, None, 1.0);
                self.ui
                    .track_list(id!(detail_track_list))
                    .clear_row_textures(cx);
            }
            MainPage::Settings => {
                self.ui
                    .ferric_cover_art(id!(discord_prev_art))
                    .set_texture(cx, None, 1.0);
            }
            MainPage::Home => {}
        }
    }

    fn unbind_all_cover_textures_for_tray(&self, cx: &mut Cx) {
        self.ui
            .ferric_cover_art(id!(np_art))
            .set_texture(cx, None, 1.0);
        self.ui
            .ferric_cover_art(id!(fs_art))
            .set_texture(cx, None, 1.0);
        self.ui
            .ferric_ambient_stage(id!(fs_backdrop))
            .set_aurora_zones(cx, None);
        for qid in [
            id!(qp_0),
            id!(qp_1),
            id!(qp_2),
            id!(qp_3),
            id!(qp_4),
            id!(qp_5),
            id!(qp_6),
            id!(qp_7),
        ] {
            self.ui
                .view(qid)
                .ferric_cover_art(id!(art))
                .set_texture(cx, None, 1.0);
        }
        for sid in [id!(st_c0), id!(st_c1), id!(st_c2), id!(st_c3), id!(st_c4)] {
            let sv = self.ui.view(sid);
            sv.ferric_cover_art(id!(full_art))
                .set_texture(cx, None, 1.0);
            let tb = sv.view(id!(triptych_box));
            tb.ferric_cover_art(id!(art_center))
                .set_texture(cx, None, 1.0);
            tb.ferric_cover_art(id!(art_left))
                .set_texture(cx, None, 1.0);
            tb.ferric_cover_art(id!(art_right))
                .set_texture(cx, None, 1.0);
        }
        for card_ids in [
            [
                id!(sh0_c0),
                id!(sh0_c1),
                id!(sh0_c2),
                id!(sh0_c3),
                id!(sh0_c4),
            ],
            [
                id!(sh1_c0),
                id!(sh1_c1),
                id!(sh1_c2),
                id!(sh1_c3),
                id!(sh1_c4),
            ],
            [
                id!(sh2_c0),
                id!(sh2_c1),
                id!(sh2_c2),
                id!(sh2_c3),
                id!(sh2_c4),
            ],
            [
                id!(sh3_c0),
                id!(sh3_c1),
                id!(sh3_c2),
                id!(sh3_c3),
                id!(sh3_c4),
            ],
            [
                id!(sh4_c0),
                id!(sh4_c1),
                id!(sh4_c2),
                id!(sh4_c3),
                id!(sh4_c4),
            ],
            [
                id!(sh5_c0),
                id!(sh5_c1),
                id!(sh5_c2),
                id!(sh5_c3),
                id!(sh5_c4),
            ],
            [
                id!(sh6_c0),
                id!(sh6_c1),
                id!(sh6_c2),
                id!(sh6_c3),
                id!(sh6_c4),
            ],
            [
                id!(sh7_c0),
                id!(sh7_c1),
                id!(sh7_c2),
                id!(sh7_c3),
                id!(sh7_c4),
            ],
        ] {
            for cid in card_ids {
                self.ui
                    .view(cid)
                    .ferric_cover_art(id!(art))
                    .set_texture(cx, None, 1.0);
            }
        }
        for rid in [
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
            self.ui
                .view(rid)
                .ferric_cover_art(id!(art))
                .set_texture(cx, None, 1.0);
        }
        for slot in sidebar_pin_slot_ids() {
            self.ui
                .view(slot)
                .ferric_cover_art(id!(art))
                .set_texture(cx, None, 1.0);
        }
        for page in [
            MainPage::Albums,
            MainPage::Artists,
            MainPage::Playlists,
            MainPage::Search,
            MainPage::Songs,
            MainPage::Recents,
            MainPage::Detail,
            MainPage::Settings,
        ] {
            self.unbind_page_cover_textures(cx, page);
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
        self.bind_cover_art_url_inner(
            cx, cover_ref, url_opt, track_opt, playing, level, circle, false,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_hero_cover_art_url(
        &self,
        cx: &mut Cx,
        cover_ref: FerricCoverArtRef,
        url_opt: Option<&str>,
        track_opt: Option<&Track>,
        playing: bool,
        level: f32,
        circle: bool,
    ) {
        self.bind_cover_art_url_inner(
            cx, cover_ref, url_opt, track_opt, playing, level, circle, true,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_cover_art_url_inner(
        &self,
        cx: &mut Cx,
        cover_ref: FerricCoverArtRef,
        url_opt: Option<&str>,
        track_opt: Option<&Track>,
        playing: bool,
        level: f32,
        circle: bool,
        prefer_hero: bool,
    ) {
        let accent = self.current_accent();
        let key_opt = track_opt.and_then(ArtworkCache::key_for_track).or_else(|| {
            url_opt
                .map(normalize_ytm_thumbnail_url)
                .filter(|s| !s.is_empty())
        });
        if let Some(key) = key_opt
            && let Some(cache) = &self.artwork
            && let Some(cached) = if prefer_hero {
                cache.get_hero(&key)
            } else {
                cache.get(&key)
            }
        {
            cover_ref.set_texture(cx, Some(cached.texture.clone()), cached.aspect);
        } else {
            cover_ref.set_texture(cx, None, 1.0);
        }
        cover_ref.set_state(cx, playing, level, accent, circle);
    }

    fn refresh_all_artwork_slots(&mut self, cx: &mut Cx) {
        self.refresh_all_artwork_slots_inner(cx, true);
    }

    fn refresh_all_artwork_slots_inner(&mut self, cx: &mut Cx, active_cover_changed: bool) {
        self.artwork_snapshot_dirty = true;
        if Self::is_track_list_page(self.active_page) {
            self.ensure_artwork_snapshot();
        }
        if active_cover_changed {
            self.apply_adaptive_accent_live(cx);
        }
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
        self.refresh_sidebar_library(cx);
        self.refresh_right_sidebar(cx);
        let is_playing = self
            .audio
            .as_ref()
            .is_some_and(|a| a.telemetry().is_playing);
        if let Some(cur) = self.queue.current().cloned() {
            self.bind_hero_cover_art_url(
                cx,
                self.ui.ferric_cover_art(id!(np_art)),
                cur.thumbnail_url.as_deref(),
                Some(&cur),
                is_playing,
                0.55,
                false,
            );
            self.bind_hero_cover_art_url(
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
        let cur_track = self.effective_display_track();
        let zones = cur_track.as_ref().and_then(|cur| {
            self.artwork
                .as_ref()
                .and_then(|c| c.get_for_track(cur))
                .map(|cached| cached.aurora_zones)
        });
        self.ui
            .ferric_ambient_stage(id!(fs_backdrop))
            .set_aurora_zones(cx, zones);
    }

    fn sync_transport_mode_icons(&self, cx: &mut Cx) {
        let raw_accent = self.current_accent();
        let pal = self.adaptive_palette();
        let accent = rgba_to_vec4(pal.accent);
        let muted = rgba_to_vec4(pal.icon_idle);
        let icon_hover = rgba_to_vec4(pal.icon_hover);
        let lm = if pal.is_light { 1.0_f32 } else { 0.0_f32 };
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        // Fullscreen always sits over its luminous liquid-aurora backdrop -> unified translucent white
        // idle icons and frosted-glass active pills across Shuffle, Prev, Next, Repeat, Volume, and Like.
        let fs_muted = vec4(1.0, 1.0, 1.0, 0.72);
        let fs_hover = vec4(1.0, 1.0, 1.0, 1.0);

        let eff_vol = self.effective_volume();
        let (eff_shuffle, eff_repeat) = self.effective_shuffle_repeat();

        let is_muted = eff_vol < 0.01;
        let vol_level_mode = if is_muted {
            0.0_f32
        } else if eff_vol < 0.50 {
            1.0_f32
        } else {
            2.0_f32
        };
        let mute_col = if is_muted { accent } else { muted };
        let mute_hov = if is_muted { accent } else { icon_hover };
        let fs_mute_col = if is_muted { raw_accent } else { fs_muted };
        let fs_mute_hov = if is_muted { raw_accent } else { fs_hover };

        self.ui.ferric_volume_btn(id!(vol_mute_btn)).set_state(
            cx,
            vol_level_mode,
            mute_col,
            mute_hov,
            lm,
            acr,
        );
        self.ui.ferric_volume_btn(id!(fs_vol_mute_btn)).set_state(
            cx,
            vol_level_mode,
            fs_mute_col,
            fs_mute_hov,
            0.0,
            1.0,
        );

        let shuf_act = if eff_shuffle { 1.0_f32 } else { 0.0_f32 };
        let shuf_col = if eff_shuffle { accent } else { muted };
        let shuf_hov = if eff_shuffle { accent } else { icon_hover };
        let fs_shuf_col = if eff_shuffle { raw_accent } else { fs_muted };
        let fs_shuf_hov = if eff_shuffle { raw_accent } else { fs_hover };
        self.ui.button(id!(shuffle_btn)).apply_over(
            cx,
            live! {
                draw_bg: { active: (shuf_act) }
                draw_icon: { color: (shuf_col), color_hover: (shuf_hov), color_focus: (shuf_hov) }
            },
        );
        self.ui.button(id!(fs_shuffle_btn)).apply_over(
            cx,
            live! {
                draw_bg: { active: (shuf_act), acrylic_on: 1.0, light_mode: 0.0 }
                draw_icon: { color: (fs_shuf_col), color_hover: (fs_shuf_hov), color_focus: (fs_shuf_hov) }
            },
        );

        let rep_active = eff_repeat != RepeatMode::Off;
        let rep_mode = match eff_repeat {
            RepeatMode::Off => 0.0_f32,
            RepeatMode::All => 1.0_f32,
            RepeatMode::One => 2.0_f32,
        };
        let rep_col = match eff_repeat {
            RepeatMode::Off => muted,
            RepeatMode::All | RepeatMode::One => accent,
        };
        let rep_hov = if rep_active { rep_col } else { icon_hover };
        let fs_rep_col = match eff_repeat {
            RepeatMode::Off => fs_muted,
            RepeatMode::All | RepeatMode::One => raw_accent,
        };
        let fs_rep_hov = if rep_active { fs_rep_col } else { fs_hover };
        self.ui
            .ferric_repeat_btn(id!(repeat_btn))
            .set_state(cx, rep_mode, rep_col, rep_hov, lm, acr);
        self.ui
            .ferric_repeat_btn(id!(fs_repeat_btn))
            .set_state(cx, rep_mode, fs_rep_col, fs_rep_hov, 0.0, 1.0);
    }

    /// Lightweight visible-only color lerp step (runs smoothly during track-switch accent transitions
    /// without mutating hidden Settings/EQ widgets or rebuilding sidebar library entries every tick).
    fn apply_adaptive_accent_lerp_step(&self, cx: &mut Cx) {
        let raw_accent = self.current_accent();
        let pal = self.adaptive_palette();
        let accent = rgba_to_vec4(pal.accent);
        let lm = if pal.is_light { 1.0_f32 } else { 0.0_f32 };
        let acrylic_tint = self.current_acrylic_tint();
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
        for pid in [
            id!(top_bar),
            id!(left_sidebar),
            id!(center_stage),
            id!(right_sidebar),
        ] {
            self.ui.view(pid).apply_over(
                cx,
                live! {
                    draw_bg: {
                        acrylic_on: (acr),
                        acrylic_opacity: (op),
                        acrylic_tint: (acrylic_tint)
                    }
                },
            );
        }
        self.ui.view(id!(bottom_bar)).apply_over(
            cx,
            live! {
                draw_bg: {
                    accent_col: (accent),
                    acrylic_on: (acr),
                    acrylic_opacity: (op),
                    acrylic_tint: (acrylic_tint),
                    light_mode: (lm),
                    glow_amt: (if self.effective_display_track().is_some() { 0.36 } else { 0.14 })
                }
            },
        );
        self.ui
            .ferric_bar_slider(id!(seek_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        self.ui
            .ferric_bar_slider(id!(vol_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        self.ui
            .ferric_bar_slider(id!(rs_conn_vol_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        self.ui
            .label(id!(rs_conn_active_lbl))
            .apply_over(cx, live! { draw_text: { color: (accent) } });
        self.ui.view(id!(top_search_box)).apply_over(
            cx,
            live! { draw_bg: { acrylic_on: (acr), accent_col: (accent), light_mode: (lm) } },
        );
        if self.active_page == MainPage::Home {
            for qid in [
                id!(qp_0),
                id!(qp_1),
                id!(qp_2),
                id!(qp_3),
                id!(qp_4),
                id!(qp_5),
                id!(qp_6),
                id!(qp_7),
            ] {
                self.ui.view(qid).apply_over(
                    cx,
                    live! { draw_bg: { accent_col: (accent), light_mode: (lm) } },
                );
            }
        }
        let icon_hover = rgba_to_vec4(pal.icon_hover);
        let muted = rgba_to_vec4(pal.icon_idle);
        let icon_muted = rgba_to_vec4(pal.icon_muted);
        let is_liked = self.effective_display_track().is_some_and(|cur| {
            self.all_tracks
                .iter()
                .find(|t| t.id == cur.id)
                .map_or(cur.liked, |t| t.liked)
        });
        self.ui.button(id!(np_like_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if is_liked { accent } else { icon_muted }) } },
        );
        self.sync_transport_mode_icons(cx);
        self.ui.button(id!(rs_radio_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if self.session_state.autoplay_radio { accent } else { icon_muted }) } },
        );
        let has_peer = self
            .connect_snap
            .as_ref()
            .is_some_and(|s| s.peers.iter().any(|p| p.connected));
        self.ui.button(id!(bar_lyrics_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if self.right_sidebar_open && self.right_lyrics_tab { accent } else { muted }) } },
        );
        self.ui.button(id!(bar_queue_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if self.right_sidebar_open && !self.right_lyrics_tab && !self.right_connect_tab { accent } else { muted }) } },
        );
        self.ui.button(id!(bar_connect_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if (self.right_sidebar_open && self.right_connect_tab) || has_peer { accent } else { muted }) } },
        );
        self.ui.button(id!(bar_video_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if self.video_open { accent } else { muted }) } },
        );
        self.ui.button(id!(top_account_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (accent), color_hover: (icon_hover), color_focus: (icon_hover), color_down: (accent) } },
        );
        for pill_id in [id!(speed_pill_btn), id!(codec_pill_btn)] {
            self.ui
                .button(pill_id)
                .apply_over(cx, live! { draw_bg: { accent_col: (accent) } });
        }
        if self.right_sidebar_open {
            if self.right_lyrics_tab {
                self.ui.ferric_lyrics_view(id!(rs_lyrics_scroll)).set_style(
                    cx,
                    accent,
                    z,
                    pal.is_light,
                );
            } else {
                self.ui.view(id!(q_now_row)).apply_over(
                    cx,
                    live! { draw_bg: { accent_col: (accent), acrylic_on: (acr), light_mode: (lm) } },
                );
                self.ui
                    .label(id!(q_now_header))
                    .apply_over(cx, live! { draw_text: { color: (accent) } });
            }
        }
        if self.fullscreen_open {
            self.ui
                .ferric_ambient_stage(id!(fs_backdrop))
                .set_state(cx, true, is_playing, raw_accent, acr);
            self.ui
                .ferric_bar_slider(id!(fs_seek_slider))
                .apply_over(cx, live! { draw_bg: { fill_col: (raw_accent) } });
            self.ui
                .ferric_bar_slider(id!(fs_vol_slider))
                .apply_over(cx, live! { draw_bg: { fill_col: (raw_accent) } });
            self.ui.button(id!(fs_like_btn)).apply_over(
                cx,
                live! {
                    draw_bg: { acrylic_on: 1.0, light_mode: 0.0 }
                    draw_icon: {
                        color: (if is_liked { raw_accent } else { vec4(1.0, 1.0, 1.0, 0.72) }),
                        color_hover: #FFFFFF,
                        color_focus: #FFFFFF
                    }
                },
            );
            self.ui.view(id!(np_canvas)).apply_over(
                cx,
                live! { draw_bg: { accent: (raw_accent), acrylic_on: (acr) } },
            );
            self.ui
                .ferric_lyrics_view(id!(fs_lyrics_scroll))
                .set_style(cx, raw_accent, z, false);
        }
        self.ui.redraw(cx);
    }

    fn theme_surface_tokens(&self) -> (Vec4, Vec4, Vec4, Vec4) {
        let surface = rgba_vec4(&self.theme.surface_hex);
        let border = rgba_vec4(&self.theme.border_hex);
        let tint = self.current_acrylic_tint();
        let is_amoled = self.theme.canvas_hex.eq_ignore_ascii_case("#000000");
        let mix_w = if is_amoled && !self.theme.adaptive_acrylic_tint {
            0.0_f32
        } else if self.theme.adaptive_acrylic_tint {
            0.22_f32
        } else {
            0.12_f32
        };
        let card_base = if is_amoled {
            vec4(0.052, 0.056, 0.076, 1.0)
        } else {
            vec4(
                (surface.x + 0.014).min(0.95),
                (surface.y + 0.015).min(0.95),
                (surface.z + 0.018).min(0.95),
                1.0,
            )
        };
        let card_col = vec4(
            card_base.x + (tint.x - card_base.x) * mix_w,
            card_base.y + (tint.y - card_base.y) * mix_w,
            card_base.z + (tint.z - card_base.z) * mix_w,
            1.0,
        );
        let btn_base = if is_amoled {
            vec4(0.078, 0.082, 0.114, 1.0)
        } else {
            vec4(
                (card_col.x + 0.032).min(0.96),
                (card_col.y + 0.033).min(0.96),
                (card_col.z + 0.038).min(0.96),
                1.0,
            )
        };
        let card_border = vec4(
            border.x + (tint.x - border.x) * (mix_w * 0.8),
            border.y + (tint.y - border.y) * (mix_w * 0.8),
            border.z + (tint.z - border.z) * (mix_w * 0.8),
            1.0,
        );
        (card_col, btn_base, card_border, tint)
    }

    fn apply_adaptive_accent_live(&self, cx: &mut Cx) {
        let raw_accent = self.current_accent();
        let pal = self.adaptive_palette();
        let accent = rgba_to_vec4(pal.accent);
        let brand_accent = rgba_to_vec4(pal.brand_accent);
        let lm = if pal.is_light { 1.0_f32 } else { 0.0_f32 };
        let (card_col, btn_col, card_border, acrylic_tint) = self.theme_surface_tokens();
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
        for pid in [
            id!(top_bar),
            id!(left_sidebar),
            id!(center_stage),
            id!(right_sidebar),
        ] {
            self.ui.view(pid).apply_over(
                cx,
                live! {
                    draw_bg: {
                        acrylic_on: (acr),
                        acrylic_opacity: (op),
                        acrylic_tint: (acrylic_tint)
                    }
                },
            );
        }
        self.ui.view(id!(bottom_bar)).apply_over(
            cx,
            live! {
                draw_bg: {
                    accent_col: (accent),
                    acrylic_on: (acr),
                    acrylic_opacity: (op),
                    acrylic_tint: (acrylic_tint),
                    light_mode: (lm),
                    glow_amt: (if self.effective_display_track().is_some() { 0.36 } else { 0.14 })
                }
            },
        );
        // Fullscreen overlay always sits over its own dark velvet aurora backdrop -> keep vivid raw cover accent
        self.ui.ferric_ambient_stage(id!(fs_backdrop)).set_state(
            cx,
            self.fullscreen_open,
            is_playing,
            raw_accent,
            acr,
        );
        self.ui
            .ferric_bar_slider(id!(seek_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        self.ui
            .ferric_bar_slider(id!(fs_seek_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (raw_accent) } });
        self.ui
            .ferric_bar_slider(id!(vol_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        self.ui
            .ferric_bar_slider(id!(rs_conn_vol_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        self.ui
            .label(id!(rs_conn_active_lbl))
            .apply_over(cx, live! { draw_text: { color: (accent) } });
        self.ui
            .ferric_bar_slider(id!(fs_vol_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (raw_accent) } });
        self.ui.view(id!(card_rs_conn_active)).apply_over(
            cx,
            live! {
                draw_bg: {
                    acrylic_on: (acr),
                    accent_col: (accent),
                    card_col: (card_col),
                    border_col: (card_border),
                    tint_col: (acrylic_tint),
                    light_mode: (lm)
                }
            },
        );
        for cid in [
            id!(rs_conn_local_btn),
            id!(rs_conn_peer_0),
            id!(rs_conn_peer_1),
            id!(rs_conn_peer_2),
            id!(rs_conn_peer_3),
            id!(rs_conn_qr_btn),
        ] {
            self.ui.button(cid).apply_over(
                cx,
                live! {
                    draw_bg: {
                        acrylic_on: (acr),
                        accent_col: (accent),
                        surface_col: (btn_col),
                        border_col: (card_border),
                        tint_col: (acrylic_tint),
                        light_mode: (lm)
                    }
                },
            );
        }
        self.ui.view(id!(top_search_box)).apply_over(
            cx,
            live! { draw_bg: { acrylic_on: (acr), accent_col: (accent), light_mode: (lm) } },
        );
        for qid in [
            id!(qp_0),
            id!(qp_1),
            id!(qp_2),
            id!(qp_3),
            id!(qp_4),
            id!(qp_5),
            id!(qp_6),
            id!(qp_7),
        ] {
            self.ui.view(qid).apply_over(
                cx,
                live! { draw_bg: { accent_col: (accent), light_mode: (lm) } },
            );
        }
        let muted = rgba_to_vec4(pal.icon_idle);
        let icon_muted = rgba_to_vec4(pal.icon_muted);
        let is_liked = self.effective_display_track().is_some_and(|cur| {
            self.all_tracks
                .iter()
                .find(|t| t.id == cur.id)
                .map_or(cur.liked, |t| t.liked)
        });
        let like_col = if is_liked { accent } else { icon_muted };
        let fs_like_col = if is_liked {
            raw_accent
        } else {
            vec4(1.0, 1.0, 1.0, 0.72)
        };
        self.ui
            .button(id!(np_like_btn))
            .apply_over(cx, live! { draw_icon: { color: (like_col) } });
        self.ui.button(id!(fs_like_btn)).apply_over(
            cx,
            live! {
                draw_bg: { acrylic_on: 1.0, light_mode: 0.0 }
                draw_icon: { color: (fs_like_col), color_hover: #FFFFFF, color_focus: #FFFFFF }
            },
        );
        self.sync_transport_mode_icons(cx);
        self.ui.button(id!(rs_radio_btn)).apply_over(
            cx,
            live! {
                draw_icon: { color: (if self.session_state.autoplay_radio { accent } else { icon_muted }) }
            },
        );
        let has_peer = self
            .connect_snap
            .as_ref()
            .is_some_and(|s| s.peers.iter().any(|p| p.connected));
        self.ui.button(id!(bar_lyrics_btn)).apply_over(
            cx,
            live! {
                draw_icon: { color: (if self.right_sidebar_open && self.right_lyrics_tab { accent } else { muted }) }
            },
        );
        self.ui.button(id!(bar_queue_btn)).apply_over(
            cx,
            live! {
                draw_icon: { color: (if self.right_sidebar_open && !self.right_lyrics_tab && !self.right_connect_tab { accent } else { muted }) }
            },
        );
        self.ui.button(id!(bar_connect_btn)).apply_over(
            cx,
            live! {
                draw_icon: { color: (if (self.right_sidebar_open && self.right_connect_tab) || has_peer { accent } else { muted }) }
            },
        );
        self.ui.button(id!(bar_video_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if self.video_open { accent } else { muted }) } },
        );
        let surface = rgba_vec4(&self.theme.surface_hex);
        let border = rgba_vec4(&self.theme.border_hex);
        self.ui.view(id!(toast_pill)).apply_over(
            cx,
            live! {
                draw_bg: {
                    acrylic_on: (acr),
                    acrylic_opacity: (op),
                    acrylic_tint: (acrylic_tint),
                    surface_col: (surface),
                    border_col: (border),
                    accent_col: (accent),
                    light_mode: (lm)
                }
            },
        );
        for sid in [
            id!(xfade_slider),
            id!(fadein_slider),
            id!(eq_preamp_slider),
            id!(acrylic_opacity_slider),
        ] {
            self.ui
                .ferric_bar_slider(sid)
                .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        }
        for (slider_id, _) in eq_band_ids() {
            self.ui
                .ferric_vert_slider(slider_id)
                .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
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
            id!(adaptive_acrylic_tint_toggle),
            id!(adaptive_art_toggle),
            id!(discord_enable_toggle),
            id!(discord_paused_toggle),
            id!(discord_cover_toggle),
            id!(discord_button_toggle),
            id!(discord_hide_details_toggle),
            id!(auto_update_toggle),
        ] {
            self.ui
                .ferric_toggle(tid)
                .apply_over(cx, live! { draw_bg: { on_col: (accent) } });
        }
        // Interactive readouts & stage kickers follow the live adaptive cover accent
        for lid in [
            id!(zoom_val_lbl),
            id!(acrylic_opacity_lbl),
            id!(acrylic_tint_lbl),
            id!(eq_preamp_lbl),
            id!(connect_lan_ip_lbl),
            id!(about_version_lbl),
            id!(pb_device_active_lbl),
            id!(xfade_val_lbl),
            id!(fadein_val_lbl),
            id!(detail_kind_lbl),
            id!(update_popup_sub),
        ] {
            self.ui
                .label(lid)
                .apply_over(cx, live! { draw_text: { color: (accent) } });
        }
        // Account & integration connection badges stay strictly Ferric Burnt Orange across all cards
        for lid in [
            id!(yt_state_lbl),
            id!(spotify_web_state_lbl),
            id!(discord_status_lbl),
        ] {
            self.ui
                .label(lid)
                .apply_over(cx, live! { draw_text: { color: (brand_accent) } });
        }
        // Input focus borders & cursors track live adaptive accent seamlessly
        for bare_inp in [id!(top_search_input), id!(sidebar_lib_filter_input)] {
            self.ui
                .text_input(bare_inp)
                .apply_over(cx, live! { draw_cursor: { color: (accent) } });
        }
        for iid in [
            id!(songs_filter_input),
            id!(search_page_input),
            id!(pl_new_input),
            id!(detail_filter_input),
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
            self.ui.text_input(iid).apply_over(
                cx,
                live! {
                    draw_bg: {
                        border_color_1_focus: (accent),
                        border_color_2_focus: (accent)
                    }
                    draw_cursor: { color: (accent) }
                },
            );
        }
        // Top account person icon & bottom speed/codec pills follow adaptive cover accent live
        let icon_hover = rgba_to_vec4(pal.icon_hover);
        self.ui.button(id!(top_account_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (accent), color_hover: (icon_hover), color_focus: (icon_hover), color_down: (accent) } },
        );
        for pill_id in [id!(speed_pill_btn), id!(codec_pill_btn)] {
            self.ui
                .button(pill_id)
                .apply_over(cx, live! { draw_bg: { accent_col: (accent) } });
        }
        // Active sidebar library highlights follow adaptive cover accent live
        let text_primary = rgba_to_vec4(pal.text_primary);
        let liked_act =
            self.active_page == MainPage::Songs && self.active_filter == LibraryFilter::Liked;
        let local_act =
            self.active_page == MainPage::Songs && self.active_filter == LibraryFilter::Local;
        self.ui
            .view(id!(pin_liked_row))
            .apply_over(cx, live! { draw_bg: { accent_col: (accent) } });
        self.ui
            .view(id!(pin_liked_row))
            .label(id!(title))
            .apply_over(
                cx,
                live! { draw_text: { color: (if liked_act { accent } else { text_primary }) } },
            );
        self.ui
            .view(id!(pin_local_row))
            .apply_over(cx, live! { draw_bg: { accent_col: (accent) } });
        self.ui
            .view(id!(pin_local_row))
            .label(id!(title))
            .apply_over(
                cx,
                live! { draw_text: { color: (if local_act { accent } else { text_primary }) } },
            );
        let entries = self.resolve_sidebar_library_entries();
        let active_detail_id =
            (self.active_page == MainPage::Detail).then_some(self.detail_state.id.as_str());
        for (i, slot) in sidebar_pin_slot_ids().into_iter().enumerate() {
            let is_act = entries
                .get(i)
                .is_some_and(|entry| active_detail_id == Some(entry.id.as_str()));
            self.ui
                .view(slot)
                .apply_over(cx, live! { draw_bg: { accent_col: (accent) } });
            if is_act {
                self.ui
                    .view(slot)
                    .label(id!(title))
                    .apply_over(cx, live! { draw_text: { color: (accent) } });
            }
        }
        self.ui.view(id!(q_now_row)).apply_over(
            cx,
            live! { draw_bg: { accent_col: (accent), acrylic_on: (acr), light_mode: (lm) } },
        );
        self.ui
            .label(id!(q_now_header))
            .apply_over(cx, live! { draw_text: { color: (accent) } });
        self.ui.view(id!(np_canvas)).apply_over(
            cx,
            live! { draw_bg: { accent: (raw_accent), acrylic_on: (acr) } },
        );
        self.ui
            .ferric_eq_graph(id!(eq_graph))
            .apply_over(cx, live! { draw_bg: { accent: (accent) } });
        self.ui
            .ferric_lyrics_view(id!(rs_lyrics_scroll))
            .set_style(cx, accent, z, pal.is_light);
        self.ui
            .ferric_lyrics_view(id!(fs_lyrics_scroll))
            .set_style(cx, raw_accent, z, false);
        self.refresh_spotify_account_ui(cx);
        self.ui.redraw(cx);
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

        let q_filter = self.songs_filter_query.trim();
        if !q_filter.is_empty() {
            list.retain(|t| {
                contains_ascii_icase(&t.title, q_filter)
                    || contains_ascii_icase(&t.artist, q_filter)
                    || contains_ascii_icase(&t.album, q_filter)
            });
        }

        let has_demo = self.all_tracks.iter().any(|t| t.id.starts_with("demo:"));
        self.ui
            .button(id!(songs_clear_demo_btn))
            .set_visible(cx, has_demo);

        match self.sort_field {
            SortField::Recent => {
                let recent_rank: HashMap<&str, usize> = self
                    .session_state
                    .recent_track_ids
                    .iter()
                    .enumerate()
                    .map(|(i, id)| (id.as_str(), i))
                    .collect();
                list.sort_by(|a, b| {
                    let ra = recent_rank
                        .get(a.id.as_str())
                        .copied()
                        .unwrap_or(usize::MAX);
                    let rb = recent_rank
                        .get(b.id.as_str())
                        .copied()
                        .unwrap_or(usize::MAX);
                    ra.cmp(&rb)
                        .then_with(|| b.liked.cmp(&a.liked))
                        .then_with(|| {
                            (b.source == TrackSource::Local).cmp(&(a.source == TrackSource::Local))
                        })
                        .then_with(|| cmp_ascii_icase(&a.title, &b.title))
                });
            }
            SortField::Artist => list.sort_by(|a, b| {
                cmp_ascii_icase(&a.artist, &b.artist)
                    .then_with(|| cmp_ascii_icase(&a.title, &b.title))
            }),
            SortField::Title => list.sort_by(|a, b| cmp_ascii_icase(&a.title, &b.title)),
            SortField::Album => list.sort_by(|a, b| {
                cmp_ascii_icase(&a.album, &b.album)
                    .then_with(|| cmp_ascii_icase(&a.title, &b.title))
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
        let mut seen_gallery: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for t in self
            .all_tracks
            .iter()
            .chain(self.rich_home.quick_picks.iter())
            .chain(self.rich_home.all_tracks.iter())
        {
            if contains_ascii_icase(&t.title, "minecraft")
                || contains_ascii_icase(&t.artist, "minecraft")
                || contains_ascii_icase(&t.artist, "watching")
            {
                continue;
            }
            if seen_gallery.insert(t.id.as_str()) {
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

        // Sidebar "Your Library" rich list (40x40 covers + pinned/saved playlists)
        self.refresh_sidebar_library(cx);

        // Top account status (reflects both Google/YouTube Music and Spotify sessions)
        self.refresh_top_account_btn(cx);
        let yt_ok = self.accounts.youtube_connected && !self.accounts.youtube_sapisid.is_empty();
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

    fn refresh_top_account_btn(&self, cx: &mut Cx) {
        let yt_ok = self.accounts.youtube_connected && !self.accounts.youtube_sapisid.is_empty();
        let sp_oauth_ok = self.is_spotify_oauth_session_active();
        let sp_web_ok = self.is_spotify_web_session_active();
        let sp_ok = sp_oauth_ok || sp_web_ok;
        let yt_name = self.accounts.youtube_channel_name.trim();
        let sp_name = if sp_oauth_ok && !self.accounts.spotify_user_name.trim().is_empty() {
            self.accounts.spotify_user_name.trim()
        } else if sp_web_ok && !self.accounts.spotify_web_user_name.trim().is_empty() {
            self.accounts.spotify_web_user_name.trim()
        } else {
            ""
        };
        let top_acct = if yt_ok && sp_ok {
            if !yt_name.is_empty() {
                clip_text(yt_name, 16)
            } else if !sp_name.is_empty() {
                clip_text(sp_name, 16)
            } else {
                "Connected".to_string()
            }
        } else if yt_ok {
            if yt_name.is_empty() {
                "Google".to_string()
            } else {
                clip_text(yt_name, 16)
            }
        } else if sp_ok {
            if sp_name.is_empty() {
                "Spotify".to_string()
            } else {
                clip_text(sp_name, 16)
            }
        } else {
            "Sign in".to_string()
        };
        let pal = self.adaptive_palette();
        let icon_col = rgba_to_vec4(pal.accent);
        let icon_hover = rgba_to_vec4(pal.icon_hover);
        self.ui.button(id!(top_account_btn)).set_text(cx, &top_acct);
        self.ui.button(id!(top_account_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (icon_col), color_hover: (icon_hover), color_focus: (icon_hover), color_down: (icon_col) } },
        );
    }

    fn filtered_playlists(&self) -> Vec<Playlist> {
        let mut out: Vec<Playlist> = self
            .playlists
            .iter()
            .filter(|pl| self.playlist_source_filter.matches_playlist(pl))
            .cloned()
            .collect();
        self.playlist_sort.apply(&mut out);
        out
    }

    fn sync_playlist_sort_ui(&self, cx: &mut Cx) {
        self.ui
            .button(id!(sb_pl_sort_btn))
            .set_text(cx, self.playlist_sort.label());
        self.style_chip_btn(
            cx,
            id!(sb_pl_sort_btn),
            self.playlist_sort != PlaylistSortOrder::Newest,
        );
        for (btn, order) in [
            (id!(pl_sort_newest_btn), PlaylistSortOrder::Newest),
            (id!(pl_sort_oldest_btn), PlaylistSortOrder::Oldest),
            (id!(pl_sort_az_btn), PlaylistSortOrder::NameAZ),
        ] {
            self.style_chip_btn(cx, btn, self.playlist_sort == order);
        }
    }

    fn set_playlist_sort(&mut self, cx: &mut Cx, order: PlaylistSortOrder) {
        self.playlist_sort = order;
        self.playlists_page = 0;
        self.refresh_sidebar_library(cx);
        self.refresh_playlists_grid(cx);
        self.ui.redraw(cx);
    }

    fn available_playlist_sources(&self) -> (bool, bool, bool) {
        let has_yt = (self.accounts.youtube_connected && !self.accounts.youtube_sapisid.is_empty())
            || self
                .playlists
                .iter()
                .any(|p| PlaylistSourceFilter::YouTube.matches_playlist(p));
        let has_sp = self.is_spotify_web_session_active()
            || self.is_spotify_oauth_session_active()
            || self
                .playlists
                .iter()
                .any(|p| PlaylistSourceFilter::Spotify.matches_playlist(p));
        let has_local = self
            .playlists
            .iter()
            .any(|p| PlaylistSourceFilter::Local.matches_playlist(p));
        (has_yt, has_sp, has_local)
    }

    fn sync_playlist_source_filter_ui(&mut self, cx: &mut Cx) {
        let (has_yt, has_sp, has_local) = self.available_playlist_sources();
        let active_selected_available = match self.playlist_source_filter {
            PlaylistSourceFilter::All => true,
            PlaylistSourceFilter::YouTube => has_yt,
            PlaylistSourceFilter::Spotify => has_sp,
            PlaylistSourceFilter::Local => has_local,
        };
        if !active_selected_available {
            self.playlist_source_filter = PlaylistSourceFilter::All;
        }
        let source_count = usize::from(has_yt) + usize::from(has_sp) + usize::from(has_local);
        let show_row =
            source_count >= 2 || self.playlist_source_filter != PlaylistSourceFilter::All;
        self.ui
            .view(id!(sidebar_pl_source_row))
            .set_visible(cx, show_row);
        self.ui
            .view(id!(pl_source_filter_row))
            .set_visible(cx, show_row);
        for (sb_btn, pl_btn, vis, kind) in [
            (
                id!(sb_src_all_btn),
                id!(pl_src_all_btn),
                true,
                PlaylistSourceFilter::All,
            ),
            (
                id!(sb_src_yt_btn),
                id!(pl_src_yt_btn),
                has_yt,
                PlaylistSourceFilter::YouTube,
            ),
            (
                id!(sb_src_sp_btn),
                id!(pl_src_sp_btn),
                has_sp,
                PlaylistSourceFilter::Spotify,
            ),
            (
                id!(sb_src_local_btn),
                id!(pl_src_local_btn),
                has_local,
                PlaylistSourceFilter::Local,
            ),
        ] {
            let is_act = self.playlist_source_filter == kind;
            self.ui.button(sb_btn).set_visible(cx, vis);
            self.ui.button(pl_btn).set_visible(cx, vis);
            if show_row && vis {
                self.style_chip_btn(cx, sb_btn, is_act);
                self.style_chip_btn(cx, pl_btn, is_act);
            }
        }
        self.sync_playlist_sort_ui(cx);
    }

    fn set_playlist_source_filter(&mut self, cx: &mut Cx, filter: PlaylistSourceFilter) {
        self.playlist_source_filter = filter;
        self.playlists_page = 0;
        self.refresh_sidebar_library(cx);
        self.refresh_playlists_grid(cx);
        self.ui.redraw(cx);
    }

    fn resolve_sidebar_library_entries(&self) -> Vec<SidebarLibraryEntry> {
        const MAX_SIDEBAR_SLOTS: usize = 40;
        let query = self.sidebar_filter_query.trim();
        let matches_query = |title: &str, sub: &str| {
            query.is_empty()
                || contains_ascii_icase(title, query)
                || contains_ascii_icase(sub, query)
        };
        let format_pl_sub = |pl: &Playlist, pinned: bool| {
            let prefix = if pinned { "Pinned · " } else { "" };
            let src = if pl.id.starts_with("spotify:premium:") {
                "Spotify Premium"
            } else {
                match pl.source {
                    TrackSource::Spotify => "Spotify",
                    TrackSource::Local => "Local",
                    TrackSource::Subsonic => "Subsonic",
                    _ => "YouTube",
                }
            };
            format!("{prefix}Playlist · {src} · {} songs", pl.track_count)
        };

        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for pin_id in &self.session_state.pinned_ids {
            if out.len() >= MAX_SIDEBAR_SLOTS {
                break;
            }
            if let Some(pl) = self.playlists.iter().find(|p| &p.id == pin_id) {
                if !self.playlist_source_filter.matches_playlist(pl) {
                    continue;
                }
                let sub = format_pl_sub(pl, true);
                if matches_query(&pl.name, &sub) && seen.insert(pl.id.clone()) {
                    out.push(SidebarLibraryEntry {
                        id: pl.id.clone(),
                        title: pl.name.clone(),
                        subtitle: sub,
                        thumbnail_url: pl.thumbnail_url.clone(),
                        is_circle: false,
                    });
                }
            } else if self.playlist_source_filter == PlaylistSourceFilter::All
                && let Some(alb) = self.album_groups.iter().find(|g| &g.id == pin_id)
            {
                let sub = format!("Pinned · Album · {}", alb.subtitle);
                if matches_query(&alb.title, &sub) && seen.insert(alb.id.clone()) {
                    out.push(SidebarLibraryEntry {
                        id: alb.id.clone(),
                        title: alb.title.clone(),
                        subtitle: sub,
                        thumbnail_url: alb.thumbnail_url.clone(),
                        is_circle: false,
                    });
                }
            } else if self.playlist_source_filter == PlaylistSourceFilter::All
                && let Some(art) = self.artist_groups.iter().find(|g| &g.id == pin_id)
            {
                let sub = "Pinned · Artist".to_string();
                if matches_query(&art.title, &sub) && seen.insert(art.id.clone()) {
                    out.push(SidebarLibraryEntry {
                        id: art.id.clone(),
                        title: art.title.clone(),
                        subtitle: sub,
                        thumbnail_url: art.thumbnail_url.clone(),
                        is_circle: true,
                    });
                }
            }
        }
        let mut sorted_pls: Vec<&Playlist> = self
            .playlists
            .iter()
            .filter(|pl| self.playlist_source_filter.matches_playlist(pl))
            .collect();
        match self.playlist_sort {
            PlaylistSortOrder::Newest => {}
            PlaylistSortOrder::Oldest => sorted_pls.reverse(),
            PlaylistSortOrder::NameAZ => {
                sorted_pls.sort_by(|a, b| {
                    cmp_ascii_icase(&a.name, &b.name).then_with(|| a.id.cmp(&b.id))
                });
            }
        }
        for pl in sorted_pls {
            if out.len() >= MAX_SIDEBAR_SLOTS {
                break;
            }
            let sub = format_pl_sub(pl, false);
            if matches_query(&pl.name, &sub) && seen.insert(pl.id.clone()) {
                out.push(SidebarLibraryEntry {
                    id: pl.id.clone(),
                    title: pl.name.clone(),
                    subtitle: sub,
                    thumbnail_url: pl.thumbnail_url.clone(),
                    is_circle: false,
                });
            }
        }
        out
    }

    fn refresh_sidebar_library(&mut self, cx: &mut Cx) {
        self.sync_playlist_source_filter_ui(cx);
        let entries = self.resolve_sidebar_library_entries();
        if !self.window_hidden_or_minimized
            && let Some(cache) = &mut self.artwork
        {
            for entry in &entries {
                if let Some(url) = entry.thumbnail_url.as_deref() {
                    let _ = cache.request_url(url);
                }
            }
        }
        let pal = self.adaptive_palette();
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        let lm = if pal.is_light { 1.0_f32 } else { 0.0_f32 };
        let accent = rgba_to_vec4(pal.accent);
        let title_col = rgba_to_vec4(pal.text_primary);
        let sub_col = rgba_to_vec4(pal.text_muted);

        let liked_count = self.all_tracks.iter().filter(|t| t.liked).count();
        let local_count = self
            .all_tracks
            .iter()
            .filter(|t| t.source == TrackSource::Local)
            .count();
        let query = self.sidebar_filter_query.trim();

        let liked_vis = self.playlist_source_filter == PlaylistSourceFilter::All
            && (query.is_empty() || contains_ascii_icase("Liked Songs", query));
        let liked_act =
            self.active_page == MainPage::Songs && self.active_filter == LibraryFilter::Liked;
        let liked_row = self.ui.view(id!(pin_liked_row));
        liked_row.set_visible(cx, liked_vis);
        liked_row.apply_over(
            cx,
            live! {
                draw_bg: {
                    active: (if liked_act { 1.0 } else { 0.0 }),
                    acrylic_on: (acr),
                    accent_col: (accent),
                    light_mode: (lm)
                }
            },
        );
        liked_row.label(id!(title)).set_text(cx, "Liked Songs");
        liked_row.label(id!(title)).apply_over(
            cx,
            live! { draw_text: { color: (if liked_act { accent } else { title_col }) } },
        );
        liked_row
            .label(id!(sub))
            .set_text(cx, &format!("Playlist · {liked_count} songs"));
        liked_row
            .label(id!(sub))
            .apply_over(cx, live! { draw_text: { color: (sub_col) } });

        let local_vis = matches!(
            self.playlist_source_filter,
            PlaylistSourceFilter::All | PlaylistSourceFilter::Local
        ) && (query.is_empty() || contains_ascii_icase("Local Lossless", query));
        let local_act =
            self.active_page == MainPage::Songs && self.active_filter == LibraryFilter::Local;
        let local_row = self.ui.view(id!(pin_local_row));
        local_row.set_visible(cx, local_vis);
        local_row.apply_over(
            cx,
            live! {
                draw_bg: {
                    active: (if local_act { 1.0 } else { 0.0 }),
                    acrylic_on: (acr),
                    accent_col: (accent),
                    light_mode: (lm)
                }
            },
        );
        local_row.label(id!(title)).set_text(cx, "Local Lossless");
        local_row.label(id!(title)).apply_over(
            cx,
            live! { draw_text: { color: (if local_act { accent } else { title_col }) } },
        );
        local_row
            .label(id!(sub))
            .set_text(cx, &format!("Local · {local_count} songs"));
        local_row
            .label(id!(sub))
            .apply_over(cx, live! { draw_text: { color: (sub_col) } });

        self.ui
            .label(id!(lbl_m_pinned))
            .set_text(cx, &format!("{}", entries.len()));
        let empty_wrap = self.ui.view(id!(sidebar_empty_wrap));
        let empty_lbl = self.ui.label(id!(sidebar_empty_lbl));
        if entries.is_empty() {
            empty_wrap.set_visible(cx, true);
            empty_lbl.set_text(
                cx,
                &if !query.is_empty() {
                    format!(
                        "No playlists matching \"{}\".",
                        self.sidebar_filter_query.trim()
                    )
                } else if self.playlist_source_filter != PlaylistSourceFilter::All {
                    format!(
                        "No {} playlists synced yet.",
                        self.playlist_source_filter.label()
                    )
                } else {
                    "No playlists yet. Sign in via Settings or click + Create.".to_string()
                },
            );
            empty_lbl.apply_over(cx, live! { draw_text: { color: (sub_col) } });
        } else {
            empty_wrap.set_visible(cx, false);
        }

        let active_detail_id =
            (self.active_page == MainPage::Detail).then_some(self.detail_state.id.as_str());
        for (i, slot) in sidebar_pin_slot_ids().into_iter().enumerate() {
            let row = self.ui.view(slot);
            if let Some(entry) = entries.get(i) {
                row.set_visible(cx, true);
                let is_act = active_detail_id == Some(entry.id.as_str());
                row.apply_over(
                    cx,
                    live! {
                        draw_bg: {
                            active: (if is_act { 1.0 } else { 0.0 }),
                            acrylic_on: (acr),
                            accent_col: (accent),
                            light_mode: (lm)
                        }
                    },
                );
                row.label(id!(title))
                    .set_text(cx, &clip_text(&clean_display_text(&entry.title), 22));
                row.label(id!(title)).apply_over(
                    cx,
                    live! { draw_text: { color: (if is_act { accent } else { title_col }) } },
                );
                row.label(id!(sub))
                    .set_text(cx, &clip_text(&clean_display_text(&entry.subtitle), 27));
                row.label(id!(sub))
                    .apply_over(cx, live! { draw_text: { color: (sub_col) } });
                self.bind_cover_art_url(
                    cx,
                    row.ferric_cover_art(id!(art)),
                    entry.thumbnail_url.as_deref(),
                    None,
                    false,
                    0.4,
                    entry.is_circle,
                );
            } else {
                row.set_visible(cx, false);
            }
        }
    }

    fn resolve_sidebar_collection(&self, target_id: &str) -> Option<CollectionGroup> {
        if let Some(pl) = self.playlists.iter().find(|p| p.id == target_id) {
            let tracks = self
                .db
                .as_ref()
                .and_then(|d| d.list_playlist_tracks(&pl.id).ok())
                .unwrap_or_default();
            return Some(CollectionGroup {
                id: pl.id.clone(),
                title: pl.name.clone(),
                subtitle: pl.description.clone(),
                kind: YtmCardKind::Playlist,
                thumbnail_url: pl.thumbnail_url.clone(),
                tracks,
            });
        }
        if let Some(alb) = self.album_groups.iter().find(|g| g.id == target_id) {
            return Some(alb.clone());
        }
        if let Some(art) = self.artist_groups.iter().find(|g| g.id == target_id) {
            return Some(art.clone());
        }
        None
    }

    fn home_quick_picks_pool(&self) -> Vec<Track> {
        let raw = if !self.rich_home.quick_picks.is_empty() {
            &self.rich_home.quick_picks
        } else {
            &self.all_tracks
        };
        raw.iter()
            .filter(|t| {
                !contains_ascii_icase(&t.album, "watching")
                    && !contains_ascii_icase(&t.artist, "watching")
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
        if !self.window_hidden_or_minimized
            && let Some(cache) = &mut self.artwork
        {
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
            let st_off = self.stations_page * 5;
            for st in self.recommended_stations.iter().skip(st_off).take(5) {
                if let Some(url) = st.center_art_url.as_deref() {
                    let _ = cache.request_url(url);
                }
                if let Some(url) = st.left_art_url.as_deref() {
                    let _ = cache.request_url(url);
                }
                if let Some(url) = st.right_art_url.as_deref() {
                    let _ = cache.request_url(url);
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
        let accent = rgba_to_vec4(self.adaptive_palette().accent);
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

        // Section 2: Spotify-Style Recommended Stations (5 cards per page, pastel 3-circle triptych or Spotify CDN cover)
        if !self.recommended_stations.is_empty() {
            self.ui.view(id!(stations_wrap)).set_visible(cx, true);
            let st_off = self.stations_page * 5;
            let st_ids = [id!(st_c0), id!(st_c1), id!(st_c2), id!(st_c3), id!(st_c4)];
            for (ci, sid) in st_ids.into_iter().enumerate() {
                let sv = self.ui.view(sid);
                sv.set_visible(cx, true);
                if let Some(st) = self.recommended_stations.get(st_off + ci).cloned() {
                    sv.view(id!(inner)).set_visible(cx, true);
                    sv.apply_over(
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
                    sv.label(id!(title))
                        .set_text(cx, &clip_text(&clean_display_text(&st.title), 18));
                    sv.label(id!(sub))
                        .set_text(cx, &clip_text(&clean_display_text(&st.subtitle), 26));
                    if st.is_spotify_prebuilt_art && st.center_art_url.is_some() {
                        sv.view(id!(full_art_wrap)).set_visible(cx, true);
                        sv.view(id!(triptych_box)).set_visible(cx, false);
                        self.bind_cover_art_url(
                            cx,
                            sv.ferric_cover_art(id!(full_art)),
                            st.center_art_url.as_deref(),
                            None,
                            false,
                            0.5,
                            false,
                        );
                    } else {
                        sv.view(id!(full_art_wrap)).set_visible(cx, false);
                        let tbox = sv.view(id!(triptych_box));
                        tbox.set_visible(cx, true);
                        let card_col = station_pastel_vec4(st.palette_idx);
                        tbox.apply_over(cx, live! { draw_bg: { card_col: (card_col) } });
                        tbox.label(id!(card_title))
                            .set_text(cx, &clip_text(&clean_display_text(&st.display_name), 15));
                        self.bind_cover_art_url(
                            cx,
                            tbox.ferric_cover_art(id!(art_center)),
                            st.center_art_url.as_deref(),
                            None,
                            false,
                            0.5,
                            true,
                        );
                        let left_url = st.left_art_url.as_deref().or(st.center_art_url.as_deref());
                        let right_url = st
                            .right_art_url
                            .as_deref()
                            .or(st.left_art_url.as_deref())
                            .or(st.center_art_url.as_deref());
                        tbox.view(id!(left_wrap))
                            .set_visible(cx, left_url.is_some());
                        tbox.view(id!(right_wrap))
                            .set_visible(cx, right_url.is_some());
                        self.bind_cover_art_url(
                            cx,
                            tbox.ferric_cover_art(id!(art_left)),
                            left_url,
                            None,
                            false,
                            0.5,
                            true,
                        );
                        self.bind_cover_art_url(
                            cx,
                            tbox.ferric_cover_art(id!(art_right)),
                            right_url,
                            None,
                            false,
                            0.5,
                            true,
                        );
                    }
                } else {
                    sv.view(id!(inner)).set_visible(cx, false);
                    sv.apply_over(
                        cx,
                        live! {
                            cursor: Default,
                            draw_bg: { ghost: 1.0 }
                        },
                    );
                }
            }
        } else {
            self.ui.view(id!(stations_wrap)).set_visible(cx, false);
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
        let is_remote = self.is_controlling_remote();
        let remote_peer = self.active_remote_peer().cloned();
        let is_playing = if let Some(ref peer) = remote_peer {
            peer.is_playing
        } else {
            self.audio
                .as_ref()
                .is_some_and(|a| a.telemetry().is_playing)
        };
        let accent = rgba_to_vec4(self.adaptive_palette().accent);
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };

        let remote_queue: Vec<Track> = if is_remote {
            self.connect_snap
                .as_ref()
                .map(|s| {
                    s.active_queue
                        .iter()
                        .cloned()
                        .map(QueueEntry::into_track)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let remote_idx = self
            .connect_snap
            .as_ref()
            .map_or(0, |s| s.active_queue_index)
            .min(remote_queue.len().saturating_sub(1));

        // History (last 3 played tracks excluding current; hidden when viewing remote speaker queue)
        let cur_track = self.effective_display_track();
        let cur_id = cur_track.as_ref().map(|t| t.id.clone());
        let hist: Vec<Track> = if is_remote {
            Vec::new()
        } else {
            self.session_state
                .recent_track_ids
                .iter()
                .filter(|id| cur_id.as_deref() != Some(id.as_str()))
                .filter_map(|id| self.all_tracks.iter().find(|t| &t.id == id).cloned())
                .take(3)
                .collect()
        };
        let upcoming: Vec<Track> = if is_remote {
            if remote_idx + 1 < remote_queue.len() {
                remote_queue[remote_idx + 1..]
                    .iter()
                    .take(10)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            self.queue.peek_upcoming(10).into_iter().cloned().collect()
        };
        if !self.window_hidden_or_minimized
            && let Some(cache) = &mut self.artwork
        {
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
        let row_lm = if self.adaptive_palette().is_light {
            1.0_f32
        } else {
            0.0_f32
        };
        for (i, rid) in [id!(qh_0), id!(qh_1), id!(qh_2)].into_iter().enumerate() {
            let rv = self.ui.view(rid);
            if let Some(t) = hist.get(i) {
                rv.set_visible(cx, true);
                rv.apply_over(
                    cx,
                    live! {
                        draw_bg: {
                            acrylic_on: (acr),
                            accent_col: (accent),
                            light_mode: (row_lm)
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
        let now_ctx = if let Some(ref peer) = remote_peer {
            format!("Playing on {}", clip_text(&peer.device_name, 18))
        } else {
            format!("Now playing · {}", clip_text(&self.queue_context, 18))
        };
        self.ui.label(id!(q_now_header)).set_text(cx, &now_ctx);
        let now_v = self.ui.view(id!(q_now_row));
        now_v.apply_over(
            cx,
            live! {
                draw_bg: {
                    active: 1.0,
                    acrylic_on: (acr),
                    accent_col: (accent),
                    light_mode: (row_lm)
                }
            },
        );
        if let Some(cur) = cur_track {
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
            let pal = self.adaptive_palette();
            let text_primary = rgba_to_vec4(pal.text_primary);
            now_v.set_visible(cx, true);
            now_v.label(id!(title)).set_text(cx, "Nothing playing");
            now_v
                .label(id!(title))
                .apply_over(cx, live! { draw_text: { color: (text_primary) } });
            now_v.label(id!(sub)).set_text(cx, "Pick any song");
            now_v.label(id!(dur)).set_text(cx, "");
            now_v.ferric_cover_art(id!(art)).set_texture(cx, None, 1.0);
        }

        // Up next (10 rows)
        let upnext_txt = if !is_remote && upcoming.is_empty() && self.radio_loading {
            "Up next (loading...)".to_string()
        } else if is_remote {
            format!("Up next ({}) · Remote", upcoming.len())
        } else {
            format!("Up next ({})", upcoming.len())
        };
        self.ui
            .label(id!(q_upnext_header))
            .set_text(cx, &upnext_txt);
        self.ui.label(id!(q_radio_status)).set_text(
            cx,
            if self.session_state.autoplay_radio {
                "Radio On"
            } else {
                "Radio Off"
            },
        );
        let radio_pal = self.adaptive_palette();
        let radio_idle = rgba_to_vec4(radio_pal.icon_muted);
        let radio_lm = if radio_pal.is_light { 1.0_f32 } else { 0.0_f32 };
        self.ui.button(id!(rs_radio_btn)).apply_over(
            cx,
            live! {
                draw_bg: { active: (if self.session_state.autoplay_radio { 1.0 } else { 0.0 }), acrylic_on: (acr), light_mode: (radio_lm) }
                draw_icon: { color: (if self.session_state.autoplay_radio { accent } else { radio_idle }) }
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
                            accent_col: (accent),
                            light_mode: (radio_lm)
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
                rv.button(id!(up_btn)).set_visible(cx, !is_remote && i > 0);
                rv.button(id!(rm_btn)).set_visible(cx, !is_remote);
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
        self.sync_playlist_source_filter_ui(cx);
        let filtered = self.filtered_playlists();
        let total = filtered.len();
        let max_pl_page = total.saturating_sub(1) / PER_PAGE;
        if self.playlists_page > max_pl_page {
            self.playlists_page = max_pl_page;
        }
        self.ui
            .view(id!(playlists_empty_card))
            .set_visible(cx, total == 0);
        if total == 0 {
            let empty_title = if self.playlist_source_filter == PlaylistSourceFilter::All {
                "No playlists yet".to_string()
            } else {
                format!("No {} playlists yet", self.playlist_source_filter.label())
            };
            self.ui
                .label(id!(lbl_t_pl_empty))
                .set_text(cx, &empty_title);
        }
        let start = self.playlists_page * PER_PAGE;
        if let Some(cache) = &mut self.artwork {
            for pl in filtered.iter().skip(start).take(PER_PAGE) {
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
            if let Some(pl) = filtered.get(start + i) {
                v.set_visible(cx, true);
                v.apply_over(cx, live! { draw_bg: { ghost: 0.0 } });
                v.view(id!(inner)).set_visible(cx, true);
                v.label(id!(title)).set_text(cx, &clip_text(&pl.name, 22));
                let badge = if pl.id.starts_with("spotify:premium:") {
                    "SPOTIFY PREMIUM"
                } else {
                    pl.source.as_badge()
                };
                v.label(id!(sub))
                    .set_text(cx, &format!("{badge} · {} songs", pl.track_count));
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

        // Filtered search cards (Albums / Artists / Playlists): priority artwork fetch
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
        let kind_str = if self.detail_state.is_radio_station {
            "RADIO STATION"
        } else {
            match self.detail_state.kind {
                YtmCardKind::Album => "ALBUM",
                YtmCardKind::Artist => "ARTIST",
                YtmCardKind::Playlist => "PLAYLIST",
                YtmCardKind::Song => "TRACK",
            }
        };
        self.ui.label(id!(detail_kind_lbl)).set_text(cx, kind_str);
        self.ui
            .label(id!(detail_title_lbl))
            .set_text(cx, &self.detail_state.title);
        let sub = if self.detail_state.loading {
            if self.detail_state.is_radio_station {
                "Building personalized radio station...".to_string()
            } else {
                "Loading tracks from YouTube Music...".to_string()
            }
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
        let has_tracks = !self.detail_state.tracks.is_empty();
        let has_share_url = self.detail_share_link().is_some();
        let can_push_cloud = is_saved_pl
            && self.detail_state.id.starts_with("local:pl:")
            && self
                .preferred_cloud_write_target(&self.detail_state.tracks)
                .is_some();
        self.ui
            .button(id!(detail_save_pl_btn))
            .set_visible(cx, !is_saved_pl && !self.detail_state.loading && has_tracks);
        self.ui
            .button(id!(detail_copy_link_btn))
            .set_visible(cx, has_share_url);
        self.ui
            .button(id!(detail_open_source_btn))
            .set_visible(cx, has_share_url);
        self.ui
            .button(id!(detail_cloud_push_btn))
            .set_visible(cx, can_push_cloud);
        self.ui
            .button(id!(detail_dedup_btn))
            .set_visible(cx, is_saved_pl && self.detail_state.tracks.len() >= 2);
        self.ui
            .button(id!(detail_copy_list_btn))
            .set_visible(cx, has_tracks);
        self.ui
            .button(id!(detail_delete_btn))
            .set_visible(cx, is_saved_pl);
        self.ui
            .view(id!(detail_tools_row))
            .set_visible(cx, is_saved_pl || has_tracks);
        self.ui
            .text_input(id!(detail_filter_input))
            .set_visible(cx, has_tracks);
        self.ui
            .button(id!(detail_sort_btn))
            .set_visible(cx, has_tracks);
        self.ui
            .button(id!(detail_sort_btn))
            .set_text(cx, self.detail_sort_order.label());
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
            .set_visible(cx, has_tracks);
        self.ui
            .button(id!(detail_export_csv_btn))
            .set_visible(cx, has_tracks);
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

    fn save_detail_as_playlist(&mut self, cx: &mut Cx) {
        if self.detail_state.tracks.is_empty() {
            self.show_toast(cx, "No tracks ready to save yet");
            return;
        }
        let title = self.detail_state.title.trim();
        let name = if title.is_empty() {
            "Saved Station".to_string()
        } else {
            title.to_string()
        };
        let pl_id = format!("local:pl:{}", csprng_hex(6));
        let tracks = self.detail_state.tracks.clone();
        let track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
        let thumb = self
            .detail_state
            .thumbnail_url
            .clone()
            .or_else(|| tracks.first().and_then(|t| t.thumbnail_url.clone()));
        let desc = if !self.detail_state.subtitle.trim().is_empty() {
            self.detail_state.subtitle.clone()
        } else if !self.detail_state.description.trim().is_empty() {
            self.detail_state.description.clone()
        } else {
            "Saved from Recommended Stations".to_string()
        };
        let playlist = Playlist {
            id: pl_id.clone(),
            name: name.clone(),
            source: TrackSource::Local,
            etag: None,
            track_count: track_ids.len(),
            description: desc.clone(),
            thumbnail_url: thumb.clone(),
        };
        if let Some(db) = &mut self.db {
            let _ = db.upsert_tracks(&tracks);
            let _ = db.upsert_playlist(&playlist, &track_ids);
            let _ = db.match_cloud_tracks_to_local();
            self.all_tracks = db.list_tracks("").unwrap_or_default();
            self.playlists = db.list_playlists().unwrap_or_default();
        }
        self.detail_state.id = pl_id.clone();
        self.detail_state.kind = YtmCardKind::Playlist;
        self.detail_state.is_radio_station = false;
        self.detail_state.description = desc.clone();
        self.detail_state.thumbnail_url = thumb;
        if let Some(hist) = &mut self.nav_history {
            hist.update_current_detail(&self.detail_state);
        }
        self.refresh_detail_hero(cx);
        self.rebuild_library_views(cx);
        self.refresh_playlists_grid(cx);
        self.spawn_cloud_create_playlist(pl_id, name.clone(), desc, tracks);
        self.show_toast(
            cx,
            &format!("Saved '{}' to Playlists ({} songs)", name, track_ids.len()),
        );
    }

    fn open_recommended_station(&mut self, cx: &mut Cx, station: RecommendedStation) {
        self.detail_filter_query.clear();
        self.detail_sort_order = DetailSortOrder::Original;
        self.ui
            .text_input(id!(detail_filter_input))
            .set_text(cx, "");
        let station_share_url = station
            .spotify_playlist_id
            .as_deref()
            .and_then(|pid| {
                collection_share_url(&format!("spotify:playlist:{pid}"), YtmCardKind::Playlist)
            })
            .map(|(_, u)| u);
        if let Some(url) = station.center_art_url.as_deref()
            && let Some(cache) = &mut self.artwork
        {
            let _ = cache.request_url(url);
        }
        if !station.cached_tracks.is_empty() {
            self.request_artwork_for_tracks(&station.cached_tracks);
            self.detail_tracks = Rc::new(station.cached_tracks.clone());
            self.detail_state = DetailViewState {
                id: station.id,
                title: station.title,
                subtitle: station.subtitle,
                description: String::new(),
                kind: YtmCardKind::Playlist,
                is_radio_station: true,
                thumbnail_url: station.center_art_url,
                share_url: station_share_url,
                loading: false,
                tracks: station.cached_tracks,
            };
            self.refresh_detail_hero(cx);
            self.navigate_to(cx, MainPage::Detail, true);
            return;
        }

        let station_id = station.id.clone();
        self.detail_tracks = Rc::new(Vec::new());
        self.detail_state = DetailViewState {
            id: station_id.clone(),
            title: station.title.clone(),
            subtitle: station.subtitle.clone(),
            description: String::new(),
            kind: YtmCardKind::Playlist,
            is_radio_station: true,
            thumbnail_url: station.center_art_url.clone(),
            share_url: station_share_url,
            loading: true,
            tracks: Vec::new(),
        };
        self.refresh_detail_hero(cx);
        self.navigate_to(cx, MainPage::Detail, true);

        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let session = self.current_ytm_session();
        let sp_token = if !self.accounts.spotify_web_access_token.trim().is_empty() {
            Some(self.accounts.spotify_web_access_token.trim().to_string())
        } else if !self.accounts.spotify_access_token.trim().is_empty() {
            Some(self.accounts.spotify_access_token.trim().to_string())
        } else {
            None
        };
        let liked_sp_ids: std::collections::HashSet<String> = self
            .all_tracks
            .iter()
            .filter(|t| t.liked)
            .filter_map(|t| t.spotify_id.clone())
            .collect();
        let library_pool: Vec<Track> = self.all_tracks.iter().take(600).cloned().collect();

        thread::spawn(move || {
            // Engine A: real Spotify Radio playlist (~50 tracks via Pathfinder or public embed)
            if let Some(pid) = station.spotify_playlist_id.as_deref() {
                if let Some(tok) = sp_token.as_deref()
                    && let Ok(bundle) = fetch_spotify_playlist_by_id(tok, pid, &liked_sp_ids)
                    && !bundle.tracks.is_empty()
                {
                    let sub = if bundle.playlist.description.trim().is_empty() {
                        station.subtitle.clone()
                    } else {
                        bundle.playlist.description
                    };
                    let _ = tx.send(BgMessage::StationTracksLoaded(Box::new(
                        StationTracksPayload {
                            station_id,
                            tracks: bundle.tracks,
                            subtitle: sub,
                            thumbnail_url: bundle
                                .playlist
                                .thumbnail_url
                                .or(station.center_art_url.clone()),
                            left_art: None,
                            right_art: None,
                        },
                    )));
                    return;
                }
                let pub_url = format!("https://open.spotify.com/playlist/{pid}");
                if let Ok((pl, tracks)) = import_spotify_public_url(&pub_url)
                    && !tracks.is_empty()
                {
                    let sub = if pl.description.trim().is_empty() {
                        station.subtitle.clone()
                    } else {
                        pl.description
                    };
                    let _ = tx.send(BgMessage::StationTracksLoaded(Box::new(
                        StationTracksPayload {
                            station_id,
                            tracks,
                            subtitle: sub,
                            thumbnail_url: pl.thumbnail_url.or(station.center_art_url.clone()),
                            left_art: None,
                            right_art: None,
                        },
                    )));
                    return;
                }
            }

            // Engine B: Algorithmic multi-hop Spotify-parity radio expansion
            match expand_recommended_station_tracks(&session, &station, &library_pool) {
                Ok(expanded) => {
                    let thumb = station.center_art_url.clone().or_else(|| {
                        expanded
                            .tracks
                            .first()
                            .and_then(|t| t.thumbnail_url.clone())
                    });
                    let _ = tx.send(BgMessage::StationTracksLoaded(Box::new(
                        StationTracksPayload {
                            station_id,
                            tracks: expanded.tracks,
                            subtitle: expanded.subtitle,
                            thumbnail_url: thumb,
                            left_art: expanded.left_art,
                            right_art: expanded.right_art,
                        },
                    )));
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::StationTracksFailed {
                        station_id,
                        error: err.to_string(),
                    });
                }
            }
        });
    }

    fn open_local_detail(&mut self, cx: &mut Cx, group: CollectionGroup) {
        self.detail_filter_query.clear();
        self.detail_sort_order = DetailSortOrder::Original;
        self.ui
            .text_input(id!(detail_filter_input))
            .set_text(cx, "");
        self.request_artwork_for_tracks(&group.tracks);
        self.detail_tracks = Rc::new(group.tracks.clone());
        self.detail_state = DetailViewState {
            id: group.id,
            title: group.title,
            subtitle: group.subtitle,
            description: String::new(),
            kind: group.kind,
            is_radio_station: false,
            thumbnail_url: group.thumbnail_url,
            share_url: None,
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
        self.detail_filter_query.clear();
        self.detail_sort_order = DetailSortOrder::Original;
        self.ui
            .text_input(id!(detail_filter_input))
            .set_text(cx, "");
        self.detail_tracks = Rc::new(Vec::new());
        self.detail_state = DetailViewState {
            id: bid.clone(),
            title: card.title,
            subtitle: card.subtitle,
            description: String::new(),
            kind: card.kind,
            is_radio_station: false,
            thumbnail_url: card.thumbnail_url,
            share_url: None,
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
                "Audio Stream Cache: {} / 512 MB (auto-clean) · Artwork Cache: {} / 128 MB (auto-clean)",
                format_bytes_mb(audio_sz),
                format_bytes_mb(art_sz)
            ),
        );
    }

    fn sync_controls_from_state(&self, cx: &mut Cx) {
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

        let spd = self.dsp.playback_speed;
        let spd_label = if (spd - 0.75).abs() < 0.05 {
            "0.75x"
        } else if (spd - 1.25).abs() < 0.05 {
            "1.25x"
        } else if (spd - 1.50).abs() < 0.05 {
            "1.5x"
        } else {
            "1.0x"
        };
        let spd_modified = (spd - 1.0).abs() >= 0.05;
        self.ui.button(id!(speed_pill_btn)).set_text(cx, spd_label);
        self.ui.button(id!(speed_pill_btn)).apply_over(
            cx,
            live! { draw_bg: { active: (if spd_modified { 1.0 } else { 0.0 }) } },
        );

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
            .ferric_toggle(id!(adaptive_acrylic_tint_toggle))
            .set_active(cx, self.theme.adaptive_acrylic_tint);
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
        self.ui
            .ferric_color_picker(id!(acrylic_color_picker))
            .set_hex(cx, &tint_hex);
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
            .ferric_toggle(id!(discord_button_toggle))
            .set_active(cx, self.discord_settings.show_listen_button);
        self.ui
            .ferric_toggle(id!(discord_hide_details_toggle))
            .set_active(cx, self.discord_settings.hide_details);
        self.refresh_output_device_ui(cx);
        self.ui
            .text_input(id!(discord_client_id_input))
            .set_text(cx, &self.discord_settings.client_id);
        self.refresh_discord_preview(cx);
        self.sync_playback_controls_only(cx);

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

    fn sync_playback_controls_only(&self, cx: &mut Cx) {
        let eff_vol = self.effective_volume();
        self.ui
            .ferric_bar_slider(id!(vol_slider))
            .set_value(cx, f64::from(eff_vol));
        self.ui
            .ferric_bar_slider(id!(fs_vol_slider))
            .set_value(cx, f64::from(eff_vol));
        self.ui
            .ferric_bar_slider(id!(rs_conn_vol_slider))
            .set_value(cx, f64::from(eff_vol));
        self.sync_transport_mode_icons(cx);
    }

    fn apply_theme_live(&self, cx: &mut Cx) {
        self.apply_theme_live_inner(cx, true);
    }

    fn apply_theme_live_inner(&self, cx: &mut Cx, save_db: bool) {
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        cx.set_win32_initial_acrylic(self.theme.acrylic_enabled);
        win32_set_native_acrylic(
            self.theme.acrylic_enabled,
            self.theme.acrylic_opacity,
            self.theme.acrylic_tint_rgb(),
            self.theme.is_light_surface(),
        );

        let pal = self.adaptive_palette();
        let lm = if pal.is_light { 1.0_f32 } else { 0.0_f32 };
        let text_primary = rgba_to_vec4(pal.text_primary);
        let text_secondary = rgba_to_vec4(pal.text_secondary);
        let text_muted = rgba_to_vec4(pal.text_muted);
        let icon_idle = rgba_to_vec4(pal.icon_idle);
        let icon_hover = rgba_to_vec4(pal.icon_hover);
        let icon_muted = rgba_to_vec4(pal.icon_muted);
        let divider_col = rgba_to_vec4(pal.divider);
        let accent = rgba_to_vec4(pal.accent);
        let danger_col = rgba_to_vec4(pal.danger_text);

        let canvas = rgba_vec4(&self.theme.canvas_hex);
        let surface = rgba_vec4(&self.theme.surface_hex);
        let border = rgba_vec4(&self.theme.border_hex);
        let (card_col, btn_col, card_border, acrylic_tint) = self.theme_surface_tokens();

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
        self.ui.view(id!(bottom_bar)).apply_over(
            cx,
            live! {
                draw_bg: {
                    base_col: (surface),
                    light_mode: (lm),
                    acrylic_on: (acr),
                    acrylic_opacity: (op),
                    acrylic_tint: (acrylic_tint)
                }
            },
        );

        // Propagate clean Native Acrylic / AMOLED / Light surface polarity to all cards
        for card_id in [
            id!(card_acct_yt),
            id!(card_acct_spotify_web),
            id!(card_acct_import),
            id!(card_acct_sub),
            id!(card_pb_device),
            id!(card_pb_xfade),
            id!(card_pb_quality),
            id!(card_pb_behavior),
            id!(card_pb_speed),
            id!(card_eq_main),
            id!(card_app_theme),
            id!(card_app_acrylic),
            id!(card_app_profiles),
            id!(card_discord_rpc),
            id!(card_st_cache),
            id!(card_st_update),
            id!(card_st_keys),
            id!(card_conn_main),
            id!(card_rs_conn_active),
            id!(card_ab_hero),
            id!(card_ab_links),
            id!(card_ab_support),
            id!(card_ab_system),
            id!(playlists_empty_card),
        ] {
            self.ui.view(card_id).apply_over(
                cx,
                live! {
                    draw_bg: {
                        acrylic_on: (acr),
                        accent_col: (accent),
                        card_col: (card_col),
                        border_col: (card_border),
                        tint_col: (acrylic_tint),
                        light_mode: (lm)
                    }
                },
            );
        }
        // Modal cards sit over a dark scrim, so keep them opaque solid matching dark/light polarity
        for modal_card_id in [id!(card_cmdk), id!(card_update_popup), id!(card_track_menu)] {
            self.ui.view(modal_card_id).apply_over(
                cx,
                live! {
                    draw_bg: {
                        acrylic_on: 0.0,
                        accent_col: (accent),
                        light_mode: (lm)
                    }
                },
            );
        }

        // Propagate surface polarity + adaptive title/sub text to all tiles, shelf cards & grid cards
        for card_id in [
            id!(qp_0),
            id!(qp_1),
            id!(qp_2),
            id!(qp_3),
            id!(qp_4),
            id!(qp_5),
            id!(qp_6),
            id!(qp_7),
            id!(st_c0),
            id!(st_c1),
            id!(st_c2),
            id!(st_c3),
            id!(st_c4),
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
        ] {
            let v = self.ui.view(card_id);
            v.apply_over(
                cx,
                live! {
                    draw_bg: {
                        acrylic_on: (acr),
                        accent_col: (accent),
                        light_mode: (lm)
                    }
                },
            );
            v.label(id!(title))
                .apply_over(cx, live! { draw_text: { color: (text_primary) } });
            v.label(id!(sub))
                .apply_over(cx, live! { draw_text: { color: (text_secondary) } });
        }

        for qrow_id in [
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
            let v = self.ui.view(qrow_id);
            v.apply_over(
                cx,
                live! {
                    draw_bg: {
                        acrylic_on: (acr),
                        accent_col: (accent),
                        light_mode: (lm)
                    }
                },
            );
            v.label(id!(title))
                .apply_over(cx, live! { draw_text: { color: (text_primary) } });
            v.label(id!(sub))
                .apply_over(cx, live! { draw_text: { color: (text_secondary) } });
            v.label(id!(dur))
                .apply_over(cx, live! { draw_text: { color: (text_muted) } });
            v.button(id!(up_btn)).apply_over(
                cx,
                live! { draw_icon: { color: (icon_muted), color_hover: (icon_hover) } },
            );
            v.button(id!(rm_btn))
                .apply_over(cx, live! { draw_icon: { color: (icon_muted) } });
        }

        self.ui.view(id!(toast_pill)).apply_over(
            cx,
            live! {
                draw_bg: {
                    acrylic_on: (acr),
                    acrylic_opacity: (op),
                    acrylic_tint: (acrylic_tint),
                    surface_col: (surface),
                    border_col: (border),
                    accent_col: (accent),
                    light_mode: (lm)
                }
            },
        );
        self.ui
            .label(id!(toast_lbl))
            .apply_over(cx, live! { draw_text: { color: (text_primary) } });
        self.ui.view(id!(discord_preview_box)).apply_over(
            cx,
            live! { draw_bg: { acrylic_on: (acr), light_mode: (lm) } },
        );
        self.ui
            .view(id!(acrylic_tint_preview))
            .apply_over(cx, live! { draw_bg: { light_mode: (lm) } });
        self.ui.view(id!(top_search_box)).apply_over(
            cx,
            live! { draw_bg: { acrylic_on: (acr), accent_col: (accent), light_mode: (lm) } },
        );
        self.ui
            .widget(id!(top_search_icon))
            .apply_over(cx, live! { draw_icon: { color: (icon_muted) } });
        let sel_col = if pal.is_light {
            vec4(accent.x, accent.y, accent.z, 0.28)
        } else {
            vec4(1.0, 1.0, 1.0, 0.25)
        };
        self.ui.text_input(id!(top_search_input)).apply_over(
            cx,
            live! {
                draw_text: { color: (text_primary), color_empty: (text_muted) }
                draw_cursor: { color: (accent) }
                draw_selection: { color: (sel_col), color_hover: (sel_col), color_focus: (sel_col) }
            },
        );
        self.ui.view(id!(sidebar_filter_box)).apply_over(
            cx,
            live! { draw_bg: { acrylic_on: (acr), light_mode: (lm) } },
        );
        self.ui
            .widget(id!(sidebar_filter_icon))
            .apply_over(cx, live! { draw_icon: { color: (icon_muted) } });
        self.ui
            .widget(id!(lib_hdr_icon))
            .apply_over(cx, live! { draw_icon: { color: (icon_idle) } });
        self.ui
            .text_input(id!(sidebar_lib_filter_input))
            .apply_over(
                cx,
                live! {
                    draw_text: { color: (text_primary), color_empty: (text_muted) }
                    draw_cursor: { color: (accent) }
                    draw_selection: { color: (sel_col), color_hover: (sel_col), color_focus: (sel_col) }
                },
            );

        let (inp_bg, inp_hov, inp_foc, inp_b, inp_bh) = if self.theme.acrylic_enabled {
            if pal.is_light {
                (
                    vec4(0.05, 0.06, 0.10, 0.085),
                    vec4(0.05, 0.06, 0.10, 0.13),
                    vec4(0.05, 0.06, 0.10, 0.16),
                    vec4(0.05, 0.06, 0.10, 0.20),
                    vec4(0.05, 0.06, 0.10, 0.34),
                )
            } else {
                (
                    vec4(1.0, 1.0, 1.0, 0.055),
                    vec4(1.0, 1.0, 1.0, 0.095),
                    vec4(1.0, 1.0, 1.0, 0.115),
                    vec4(1.0, 1.0, 1.0, 0.095),
                    vec4(1.0, 1.0, 1.0, 0.19),
                )
            }
        } else if pal.is_light {
            (
                rgba_vec4("#E6DFD1"),
                rgba_vec4("#DDD5C5"),
                rgba_vec4("#D7CEBC"),
                rgba_vec4("#C5BBA5"),
                rgba_vec4("#9E9480"),
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
            id!(detail_filter_input),
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
                    draw_text: { color: (text_primary), color_empty: (text_muted) }
                    draw_cursor: { color: (accent) }
                    draw_selection: { color: (sel_col), color_hover: (sel_col), color_focus: (sel_col) }
                },
            );
        }

        // Primary CTA buttons invert smoothly on light surfaces (#12151E bg + #FFFFFF text)
        let prim_fg = if pal.is_light {
            vec4(1.0, 1.0, 1.0, 1.0)
        } else {
            vec4(0.035, 0.035, 0.051, 1.0)
        };
        for pbtn_id in [
            id!(update_badge_btn),
            id!(search_go_btn),
            id!(recents_play_btn),
            id!(play_all_btn),
            id!(pl_create_btn),
            id!(pl_empty_focus_btn),
            id!(detail_play_btn),
            id!(yt_login_btn),
            id!(spotify_web_login_btn),
            id!(discord_save_id_btn),
            id!(update_now_btn),
            id!(tm_play_btn),
            id!(tm_new_pl_btn),
        ] {
            self.ui.button(pbtn_id).apply_over(
                cx,
                live! {
                    draw_bg: { light_mode: (lm) }
                    draw_text: { color: (prim_fg), color_hover: (prim_fg), color_focus: (prim_fg), color_down: (prim_fg) }
                    draw_icon: { color: (prim_fg), color_hover: (prim_fg), color_focus: (prim_fg), color_down: (prim_fg) }
                },
            );
        }

        for btn_id in [
            id!(top_account_btn),
            id!(sidebar_create_pl_btn),
            id!(home_retry_btn),
            id!(qp_refresh_btn),
            id!(home_play_all_btn),
            id!(stations_refresh_btn),
            id!(home_load_more_btn),
            id!(home_shuffle_feed_btn),
            id!(search_clear_btn),
            id!(recents_clear_btn),
            id!(hero_shuffle_btn),
            id!(add_folder_btn),
            id!(sort_cycle_btn),
            id!(pl_import_btn),
            id!(pl_empty_import_btn),
            id!(pl_empty_google_btn),
            id!(detail_shuffle_btn),
            id!(detail_save_pl_btn),
            id!(detail_radio_btn),
            id!(detail_copy_link_btn),
            id!(detail_pin_btn),
            id!(detail_queue_btn),
            id!(detail_copy_list_btn),
            id!(detail_open_source_btn),
            id!(detail_cloud_push_btn),
            id!(detail_dedup_btn),
            id!(detail_sort_btn),
            id!(detail_export_btn),
            id!(detail_export_csv_btn),
            id!(detail_rename_btn),
            id!(pb_device_cycle_btn),
            id!(pb_device_refresh_btn),
            id!(yt_sync_btn),
            id!(yt_logout_btn),
            id!(guest_refresh_btn),
            id!(spotify_web_sync_btn),
            id!(spotify_web_logout_btn),
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
            id!(check_updates_btn),
            id!(open_releases_btn),
            id!(connect_gen_pair_btn),
            id!(connect_copy_pair_btn),
            id!(rs_conn_local_btn),
            id!(rs_conn_peer_0),
            id!(rs_conn_peer_1),
            id!(rs_conn_peer_2),
            id!(rs_conn_peer_3),
            id!(rs_conn_qr_btn),
            id!(about_web_btn),
            id!(about_github_btn),
            id!(about_changelog_btn),
            id!(about_issues_btn),
            id!(about_license_btn),
            id!(about_coffee_btn),
            id!(about_saweria_btn),
            id!(about_copy_diag_btn),
            id!(rs_lyr_minus),
            id!(rs_lyr_plus),
            id!(rs_lyr_expand),
            id!(rs_lyr_copy),
            id!(rs_watch_video),
            id!(speed_pill_btn),
            id!(codec_pill_btn),
            id!(cmdk_close_btn),
            id!(cmdk_home_btn),
            id!(cmdk_search_btn),
            id!(cmdk_songs_btn),
            id!(cmdk_albums_btn),
            id!(cmdk_artists_btn),
            id!(cmdk_playlists_btn),
            id!(cmdk_eq_btn),
            id!(cmdk_fade_btn),
            id!(cmdk_google_btn),
            id!(cmdk_fs_btn),
            id!(cmdk_demo_btn),
            id!(cmdk_copy_link_btn),
            id!(cmdk_copy_pl_btn),
            id!(cmdk_save_q_btn),
            id!(cmdk_about_btn),
            id!(tm_close_btn),
            id!(tm_play_next_btn),
            id!(tm_add_queue_btn),
            id!(tm_radio_btn),
            id!(tm_like_btn),
            id!(tm_artist_btn),
            id!(tm_album_btn),
            id!(tm_lyrics_btn),
            id!(tm_copy_link_btn),
            id!(tm_copy_btn),
            id!(tm_copy_alt_link_btn),
            id!(tm_open_source_btn),
            id!(update_whats_new_btn),
            id!(update_popup_close_btn),
        ] {
            self.ui.button(btn_id).apply_over(
                cx,
                live! {
                    draw_bg: {
                        acrylic_on: (acr),
                        accent_col: (accent),
                        surface_col: (btn_col),
                        border_col: (card_border),
                        tint_col: (acrylic_tint),
                        light_mode: (lm)
                    }
                    draw_text: { color: (text_primary), color_hover: (icon_hover), color_focus: (icon_hover), color_down: (icon_hover) }
                },
            );
        }
        self.ui.button(id!(top_account_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (accent), color_hover: (icon_hover), color_focus: (icon_hover), color_down: (accent) } },
        );
        for danger_btn_id in [
            id!(songs_clear_demo_btn),
            id!(detail_delete_btn),
            id!(remove_demo_btn),
            id!(clear_local_btn),
            id!(tm_remove_btn),
        ] {
            self.ui.button(danger_btn_id).apply_over(
                cx,
                live! {
                    draw_bg: { acrylic_on: (acr), accent_col: (accent), light_mode: (lm) }
                    draw_text: { color: (danger_col), color_hover: (danger_col), color_focus: (danger_col), color_down: (danger_col) }
                },
            );
        }

        // Topbar icon nav pills (Home, Explore/Search, Recents)
        for (p, nid) in [
            (MainPage::Home, id!(nav_home)),
            (MainPage::Search, id!(nav_search)),
            (MainPage::Recents, id!(nav_recents)),
        ] {
            let active = p == self.active_page;
            self.ui.button(nid).apply_over(
                cx,
                live! {
                    draw_bg: { active: (if active { 1.0 } else { 0.0 }), acrylic_on: (acr), light_mode: (lm) }
                    draw_icon: { color: (if active { text_primary } else { icon_idle }), color_hover: (icon_hover), color_focus: (icon_hover), color_down: (icon_hover) }
                },
            );
        }
        // Left sidebar library category chips (Playlists, Songs, Albums, Artists)
        for (p, nid) in [
            (MainPage::Playlists, id!(nav_playlists)),
            (MainPage::Songs, id!(nav_songs)),
            (MainPage::Albums, id!(nav_albums)),
            (MainPage::Artists, id!(nav_artists)),
        ] {
            let active = p == self.active_page;
            self.ui.button(nid).apply_over(
                cx,
                live! {
                    draw_bg: { active: (if active { 1.0 } else { 0.0 }), acrylic_on: (acr), light_mode: (lm) }
                    draw_text: { active: (if active { 1.0 } else { 0.0 }), light_mode: (lm), color: (text_secondary) }
                },
            );
        }
        for row_id in [
            id!(pin_liked_row) as &[LiveId],
            id!(pin_local_row) as &[LiveId],
        ]
        .into_iter()
        .chain(sidebar_pin_slot_ids())
        {
            let row = self.ui.view(row_id);
            row.apply_over(
                cx,
                live! { draw_bg: { acrylic_on: (acr), accent_col: (accent), light_mode: (lm) } },
            );
            row.label(id!(title))
                .apply_over(cx, live! { draw_text: { color: (text_primary) } });
            row.label(id!(sub))
                .apply_over(cx, live! { draw_text: { color: (text_muted) } });
        }
        self.ui
            .label(id!(sidebar_empty_lbl))
            .apply_over(cx, live! { draw_text: { color: (text_muted) } });

        // All Chip buttons (moods, filters, tabs, tint presets, quality, queue/lyrics tabs)
        for chip_id in [
            id!(chip_0),
            id!(chip_1),
            id!(chip_2),
            id!(chip_3),
            id!(chip_4),
            id!(chip_5),
            id!(chip_6),
            id!(chip_7),
            id!(chip_8),
            id!(rsrch_0),
            id!(rsrch_1),
            id!(rsrch_2),
            id!(rsrch_3),
            id!(rsrch_4),
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
            id!(sb_src_all_btn),
            id!(sb_src_yt_btn),
            id!(sb_src_sp_btn),
            id!(sb_src_local_btn),
            id!(sb_pl_sort_btn),
            id!(pl_src_all_btn),
            id!(pl_src_yt_btn),
            id!(pl_src_sp_btn),
            id!(pl_src_local_btn),
            id!(pl_sort_newest_btn),
            id!(pl_sort_oldest_btn),
            id!(pl_sort_az_btn),
            id!(st_accounts_btn),
            id!(st_playback_btn),
            id!(st_eq_btn),
            id!(st_appearance_btn),
            id!(st_integrations_btn),
            id!(st_storage_btn),
            id!(st_connect_btn),
            id!(st_about_btn),
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
            id!(rs_tab_connect),
            id!(aq_low_btn),
            id!(aq_norm_btn),
            id!(aq_high_btn),
            id!(aq_best_btn),
            id!(svq_auto_btn),
            id!(svq_360_btn),
            id!(svq_480_btn),
            id!(svq_720_btn),
            id!(svq_1080_btn),
            id!(tm_pl_0),
            id!(tm_pl_1),
            id!(tm_pl_2),
            id!(tm_pl_3),
            id!(tm_pl_4),
            id!(tm_pl_5),
        ] {
            self.ui.button(chip_id).apply_over(
                cx,
                live! {
                    draw_bg: {
                        acrylic_on: (acr),
                        surface_col: (btn_col),
                        border_col: (card_border),
                        tint_col: (acrylic_tint),
                        light_mode: (lm)
                    }
                    draw_text: { color: (text_secondary), light_mode: (lm) }
                },
            );
        }

        // All shell Icon buttons (top bar, shelf arrows, pagination, right sidebar, bottom bar)
        for icon_btn_id in [
            id!(toggle_left_btn),
            id!(nav_back_btn),
            id!(nav_fwd_btn),
            id!(refresh_btn),
            id!(top_settings_btn),
            id!(toggle_right_btn),
            id!(top_search_clear_btn),
            id!(qp_prev_btn),
            id!(qp_next_btn),
            id!(stations_prev_btn),
            id!(stations_next_btn),
            id!(sh0_prev_btn),
            id!(sh0_next_btn),
            id!(sh1_prev_btn),
            id!(sh1_next_btn),
            id!(sh2_prev_btn),
            id!(sh2_next_btn),
            id!(sh3_prev_btn),
            id!(sh3_next_btn),
            id!(sh4_prev_btn),
            id!(sh4_next_btn),
            id!(sh5_prev_btn),
            id!(sh5_next_btn),
            id!(sh6_prev_btn),
            id!(sh6_next_btn),
            id!(sh7_prev_btn),
            id!(sh7_next_btn),
            id!(alb_prev_btn),
            id!(alb_next_btn),
            id!(art_prev_btn),
            id!(art_next_btn),
            id!(pl_prev_btn),
            id!(pl_next_btn),
            id!(rs_radio_btn),
            id!(rs_save_queue_btn),
            id!(rs_copy_queue_btn),
            id!(rs_refresh_queue_btn),
            id!(rs_clear_btn),
            id!(np_like_btn),
            id!(np_add_pl_btn),
            id!(np_copy_link_btn),
            id!(shuffle_btn),
            id!(prev_btn),
            id!(next_btn),
            id!(bar_video_btn),
            id!(bar_lyrics_btn),
            id!(bar_queue_btn),
            id!(bar_fullscreen_btn),
        ] {
            self.ui.button(icon_btn_id).apply_over(
                cx,
                live! {
                    draw_bg: { acrylic_on: (acr), light_mode: (lm) }
                    draw_icon: { color: (icon_idle), color_hover: (icon_hover), color_focus: (icon_hover), color_down: (accent) }
                },
            );
        }
        let can_b = self.nav_history.as_ref().is_some_and(NavHistory::can_back);
        let can_f = self
            .nav_history
            .as_ref()
            .is_some_and(NavHistory::can_forward);
        let disabled_nav = if pal.is_light {
            vec4(0.06, 0.08, 0.12, 0.34)
        } else {
            vec4(0.32, 0.34, 0.40, 1.0)
        };
        self.ui.button(id!(nav_back_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if can_b { text_primary } else { disabled_nav }) } },
        );
        self.ui.button(id!(nav_fwd_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if can_f { text_primary } else { disabled_nav }) } },
        );
        self.sync_sidebar_toggle_buttons(cx);

        // Window chrome controls (min / max / close) & Play/Pause button
        for win_btn_id in [id!(win_min_btn), id!(win_max_btn), id!(win_close_btn)] {
            self.ui.ferric_win_btn(win_btn_id).apply_over(
                cx,
                live! {
                    draw_bg: {
                        fg_col: (icon_idle),
                        fg_hover: (icon_hover),
                        light_mode: (lm)
                    }
                },
            );
        }
        let (play_bg, play_hov, play_down, play_aura, play_glyph) = if pal.is_light {
            (
                vec4(0.071, 0.082, 0.118, 1.0),
                vec4(0.137, 0.157, 0.220, 1.0),
                vec4(0.043, 0.051, 0.078, 1.0),
                vec4(0.06, 0.07, 0.11, 1.0),
                vec4(1.0, 1.0, 1.0, 1.0),
            )
        } else {
            (
                vec4(1.0, 1.0, 1.0, 1.0),
                vec4(1.0, 1.0, 1.0, 1.0),
                rgba_vec4("#D8DAE4"),
                vec4(1.0, 1.0, 1.0, 1.0),
                vec4(0.035, 0.035, 0.051, 1.0),
            )
        };
        self.ui.ferric_play_btn(id!(play_pause_btn)).apply_over(
            cx,
            live! {
                draw_bg: {
                    accent_col: (play_bg),
                    accent_hover: (play_hov),
                    accent_down: (play_down),
                    aura_col: (play_aura),
                    glyph_col: (play_glyph)
                }
            },
        );

        // Sliders, Toggles & EQ graph surfaces adapt to light/dark background
        let slider_track = if pal.is_light {
            vec4(0.06, 0.07, 0.11, 0.24)
        } else {
            rgba_vec4("#3E404C")
        };
        let slider_thumb = if pal.is_light {
            text_primary
        } else {
            rgba_vec4("#F4F4F7")
        };
        for sid in [
            id!(seek_slider),
            id!(vol_slider),
            id!(rs_conn_vol_slider),
            id!(xfade_slider),
            id!(fadein_slider),
            id!(eq_preamp_slider),
            id!(acrylic_opacity_slider),
        ] {
            self.ui.ferric_bar_slider(sid).apply_over(
                cx,
                live! { draw_bg: { track_col: (slider_track), thumb_col: (slider_thumb) } },
            );
        }
        let (vert_track, vert_tick) = if pal.is_light {
            (vec4(0.06, 0.07, 0.11, 0.22), vec4(0.06, 0.07, 0.11, 0.36))
        } else {
            (rgba_vec4("#262834"), rgba_vec4("#323544"))
        };
        for (slider_id, _) in eq_band_ids() {
            self.ui.ferric_vert_slider(slider_id).apply_over(
                cx,
                live! { draw_bg: { track_col: (vert_track), thumb_col: (slider_thumb), tick_col: (vert_tick) } },
            );
        }
        let (tog_off, tog_border) = if pal.is_light {
            (vec4(0.06, 0.07, 0.11, 0.44), vec4(0.06, 0.07, 0.11, 0.52))
        } else {
            (rgba_vec4("#222430"), rgba_vec4("#323544"))
        };
        for tid in [
            id!(close_to_tray_toggle),
            id!(radio_toggle),
            id!(gapless_lock_toggle),
            id!(trim_silence_toggle),
            id!(replay_gain_toggle),
            id!(mono_toggle),
            id!(eq_enable_toggle),
            id!(acrylic_toggle),
            id!(adaptive_acrylic_tint_toggle),
            id!(adaptive_art_toggle),
            id!(discord_enable_toggle),
            id!(discord_paused_toggle),
            id!(discord_cover_toggle),
            id!(discord_button_toggle),
            id!(discord_hide_details_toggle),
            id!(auto_update_toggle),
        ] {
            self.ui.ferric_toggle(tid).apply_over(
                cx,
                live! { draw_bg: { off_col: (tog_off), border_col: (tog_border) } },
            );
        }
        let (eq_bg, eq_border) = if pal.is_light {
            (rgba_vec4("#E6DFD1"), rgba_vec4("#C5BBA5"))
        } else {
            (rgba_vec4("#12131A"), rgba_vec4("#232530"))
        };
        self.ui.ferric_eq_graph(id!(eq_graph)).apply_over(
            cx,
            live! { draw_bg: { bg_col: (eq_bg), border_col: (eq_border), light_mode: (lm) } },
        );

        // Dividers adapt cleanly to light/dark background
        for div_id in [
            id!(div_top_l),
            id!(div_top_search),
            id!(div_top_r),
            id!(div_h_top),
            id!(left_divider),
            id!(right_divider),
            id!(div_h_recents),
            id!(div_h_songs),
            id!(div_h_detail),
            id!(div_h_app),
            id!(div_h_app_acr),
            id!(div_h_app_tint),
            id!(div_h_disc),
            id!(div_h_rs),
            id!(tm_div_1),
            id!(tm_div_2),
        ] {
            self.ui
                .widget(div_id)
                .apply_over(cx, live! { draw_bg: { color: (divider_col) } });
        }

        // All primary shell labels -> WCAG-verified primary ink
        for lid in [
            id!(brand_title_lbl),
            id!(lbl_m_library),
            id!(lbl_t_quick_picks),
            id!(lbl_t_stations),
            id!(sh0_title_lbl),
            id!(sh1_title_lbl),
            id!(sh2_title_lbl),
            id!(sh3_title_lbl),
            id!(sh4_title_lbl),
            id!(sh5_title_lbl),
            id!(sh6_title_lbl),
            id!(sh7_title_lbl),
            id!(lbl_t_top_results),
            id!(lbl_t_recents),
            id!(songs_title_lbl),
            id!(lbl_t_albums),
            id!(lbl_t_artists),
            id!(lbl_t_playlists),
            id!(lbl_t_pl_empty),
            id!(detail_title_lbl),
            id!(lbl_t_settings),
            id!(lbl_t_acct_yt),
            id!(lbl_t_acct_spotify_web),
            id!(lbl_t_acct_import),
            id!(lbl_t_acct_sub),
            id!(lbl_t_pb_dev),
            id!(lbl_t_pb_xfade),
            id!(lbl_t_pb_fadein),
            id!(lbl_t_pb_qual),
            id!(lbl_t_pb_aq),
            id!(lbl_t_pb_vq),
            id!(lbl_t_pb_beh),
            id!(lbl_t_pb_tray),
            id!(lbl_t_pb_radio),
            id!(lbl_t_pb_gapless),
            id!(lbl_t_pb_trim),
            id!(lbl_t_pb_rg),
            id!(lbl_t_pb_mono),
            id!(sleep_status_lbl),
            id!(lbl_t_eq_title),
            id!(lbl_t_eq_preamp),
            id!(eq_v0),
            id!(eq_v1),
            id!(eq_v2),
            id!(eq_v3),
            id!(eq_v4),
            id!(eq_v5),
            id!(eq_v6),
            id!(eq_v7),
            id!(eq_v8),
            id!(eq_v9),
            id!(lbl_t_app_title),
            id!(lbl_t_app_zoom),
            id!(lbl_t_app_acr),
            id!(lbl_t_app_acr_adaptive),
            id!(lbl_t_app_tint),
            id!(lbl_t_app_adaptive),
            id!(lbl_t_app_profiles),
            id!(lbl_t_disc_title),
            id!(discord_prev_title),
            id!(lbl_t_disc_en),
            id!(lbl_t_disc_fmt),
            id!(lbl_t_disc_paused),
            id!(lbl_t_disc_cover),
            id!(lbl_t_disc_btn),
            id!(lbl_t_disc_hide),
            id!(lbl_t_disc_id),
            id!(lbl_t_st_cache),
            id!(lbl_t_st_update),
            id!(lbl_t_st_keys),
            id!(lbl_t_conn_title),
            id!(lbl_t_ab_name),
            id!(lbl_t_ab_links),
            id!(lbl_t_ab_support),
            id!(lbl_t_ab_sys),
            id!(np_title_lbl),
            id!(rs_conn_now_lbl),
            id!(lbl_t_powerbar),
            id!(update_popup_title),
            id!(tm_title_lbl),
        ] {
            self.ui
                .label(lid)
                .apply_over(cx, live! { draw_text: { color: (text_primary) } });
        }

        // All secondary/muted shell labels -> WCAG-verified secondary/muted ink
        for lid in [
            id!(lbl_m_pinned),
            id!(lbl_m_stations),
            id!(home_status_lbl),
            id!(search_status_lbl),
            id!(lbl_m_recent),
            id!(recents_sub_lbl),
            id!(songs_sub_lbl),
            id!(albums_sub_lbl),
            id!(artists_sub_lbl),
            id!(lbl_m_pl_empty),
            id!(detail_sub_lbl),
            id!(lbl_m_acct_desc),
            id!(oauth_status_lbl),
            id!(lbl_m_spotify_web),
            id!(spotify_web_status_lbl),
            id!(lbl_m_pb_dev),
            id!(lbl_m_pb_curve),
            id!(eq_status_lbl),
            id!(eq_f0),
            id!(eq_f1),
            id!(eq_f2),
            id!(eq_f3),
            id!(eq_f4),
            id!(eq_f5),
            id!(eq_f6),
            id!(eq_f7),
            id!(eq_f8),
            id!(eq_f9),
            id!(lbl_m_app_theme_desc),
            id!(lbl_m_app_zoom_hint),
            id!(lbl_m_app_adaptive_hint),
            id!(lbl_m_app_acr_desc),
            id!(lbl_m_app_opacity),
            id!(lbl_m_app_acr_adaptive_hint),
            id!(lbl_m_app_presets),
            id!(lbl_m_app_profiles),
            id!(discord_prev_header),
            id!(discord_prev_artist),
            id!(discord_prev_album),
            id!(lbl_m_disc_hint),
            id!(cache_stats_lbl),
            id!(lbl_m_st_auto_upd),
            id!(update_status_lbl),
            id!(lbl_m_st_keys),
            id!(connect_peers_lbl),
            id!(lbl_m_ab_tagline),
            id!(lbl_m_ab_meta),
            id!(lbl_m_ab_links),
            id!(lbl_m_ab_support),
            id!(lbl_m_ab_sys),
            id!(lbl_m_history),
            id!(q_upnext_header),
            id!(q_radio_status),
            id!(lbl_m_rs_vol),
            id!(lbl_m_rs_switch),
            id!(rs_conn_status_lbl),
            id!(rs_lyr_source),
            id!(np_artist_lbl),
            id!(pos_lbl),
            id!(update_popup_notes),
            id!(tm_sub_lbl),
            id!(tm_add_pl_hdr),
        ] {
            self.ui
                .label(lid)
                .apply_over(cx, live! { draw_text: { color: (text_secondary) } });
        }
        self.ui.button(id!(dur_btn)).apply_over(
            cx,
            live! {
                draw_text: { color: (text_secondary), color_hover: (text_primary), color_focus: (text_primary), color_down: (text_primary) }
            },
        );

        // Fullscreen overlay chips keep acrylic_on sync without light_mode inversion (dark aurora stage)
        for fs_chip_id in [
            id!(fs_mode_art_btn),
            id!(fs_mode_video_btn),
            id!(vq_auto_btn),
            id!(vq_360_btn),
            id!(vq_480_btn),
            id!(vq_720_btn),
            id!(vq_1080_btn),
        ] {
            self.ui.button(fs_chip_id).apply_over(
                cx,
                live! {
                    draw_bg: { acrylic_on: (acr), light_mode: 0.0 }
                    draw_text: { color: #C8CAD6, light_mode: 0.0 }
                },
            );
        }

        self.apply_adaptive_accent_live(cx);
        self.refresh_output_device_ui(cx);
        if save_db && let Some(db) = &self.db {
            let _ = db.save_theme(&self.theme);
        }
        self.ui.redraw(cx);
    }

    fn apply_dsp_live(&mut self, cx: &mut Cx) {
        self.apply_dsp_live_inner(cx, true);
    }

    fn apply_dsp_live_inner(&mut self, cx: &mut Cx, save_db: bool) {
        if let Some(audio) = &self.audio {
            audio.send(EngineCommand::SetDspSettings(self.dsp.clone()));
        }
        if save_db && let Some(db) = &self.db {
            let _ = db.save_playback_dsp(&self.dsp);
        }
        self.sync_next_deck_to_engine();
        self.rebuild_library_views(cx);
    }

    fn apply_eq_live(&self, cx: &mut Cx) {
        self.apply_eq_live_inner(cx, true);
    }

    fn apply_eq_live_inner(&self, cx: &mut Cx, save_db: bool) {
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
        if save_db && let Some(db) = &self.db {
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
                rpc.update_playback(Some(Self::build_discord_snapshot(
                    cur,
                    snap.as_ref().map_or(0, |s| s.position_ms),
                    snap.as_ref()
                        .map_or(cur.duration_ms, |s| s.duration_ms.max(cur.duration_ms)),
                    snap.as_ref().is_some_and(|s| s.is_playing),
                )));
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
        if matches!(self.sleep_timer, SleepTimerMode::EndOfTrack) {
            audio.send(EngineCommand::QueueNextTrack {
                path: None,
                same_album_consecutive: false,
                replay_gain_db: 0,
                fallback_duration_ms: 0,
            });
            return;
        }
        let next_opt = if self.queue.repeat == RepeatMode::One {
            self.queue.current()
        } else {
            self.queue.peek_upcoming(1).into_iter().next()
        };
        if let Some(next_track) = next_opt {
            let valid_path = next_track.path.as_ref().filter(|p| p.is_file()).cloned();
            // Lock 0ms gapless transition for RepeatMode::One and consecutive Local album tracks.
            let same_album = self.queue.repeat == RepeatMode::One
                || self.queue.current().is_some_and(|cur| {
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
                replay_gain_db: next_track.replay_gain_db,
                fallback_duration_ms: next_track.duration_ms,
            });
        } else {
            audio.send(EngineCommand::QueueNextTrack {
                path: None,
                same_album_consecutive: false,
                replay_gain_db: 0,
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
            if self.queue.peek_upcoming(5).is_empty() {
                let fallback: Vec<Track> = self
                    .home_quick_picks_pool()
                    .into_iter()
                    .filter(|t| t.id != track.id)
                    .take(15)
                    .collect();
                if !fallback.is_empty() {
                    self.queue.append_tracks(fallback);
                    self.sync_next_deck_to_engine();
                    self.prefetch_next_online_track();
                }
            }
            return;
        };
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.last_radio_seed_id = Some(track.id.clone());
        self.radio_loading = true;
        let session = self.current_ytm_session();
        let seed_track_id = track.id.clone();
        let seed_title = track.title.clone();
        let seed_artist = track.artist.clone();
        let seed_album = track.album.clone();
        thread::spawn(move || {
            match fetch_radio_up_next_for_track(
                &session,
                &vid,
                &seed_title,
                &seed_artist,
                &seed_album,
                15,
                force,
            ) {
                Ok(tracks) if !tracks.is_empty() => {
                    let _ = tx.send(BgMessage::RadioLoaded {
                        seed_track_id,
                        seed_title,
                        tracks,
                    });
                }
                Ok(_) => {
                    let _ = tx.send(BgMessage::RadioFailed {
                        seed_track_id,
                        error: "Radio returned no tracks".to_string(),
                    });
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::RadioFailed {
                        seed_track_id,
                        error: err.to_string(),
                    });
                }
            }
        });
    }

    fn is_text_input_focused(&self, cx: &Cx) -> bool {
        for input_id in [
            id!(top_search_input),
            id!(sidebar_lib_filter_input),
            id!(search_page_input),
            id!(songs_filter_input),
            id!(pl_new_input),
            id!(detail_filter_input),
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

        let primary_link = track_primary_link(&track);
        let alt_yt_link = if primary_link.as_ref().is_some_and(|(p, _)| *p == "Spotify") {
            track_youtube_link(&track)
        } else {
            None
        };
        self.ui
            .button(id!(tm_copy_link_btn))
            .set_visible(cx, primary_link.is_some());
        if let Some((platform, _)) = &primary_link {
            let lbl = if *platform == "Spotify" {
                "Copy Spotify Link"
            } else {
                "Copy YT Link"
            };
            self.ui.button(id!(tm_copy_link_btn)).set_text(cx, lbl);
        }
        self.ui
            .button(id!(tm_copy_alt_link_btn))
            .set_visible(cx, alt_yt_link.is_some());
        let open_btn = self.ui.button(id!(tm_open_source_btn));
        if track.path.is_some() {
            open_btn.set_visible(cx, true);
            open_btn.set_text(cx, "Show File");
        } else if let Some((platform, _)) = &primary_link {
            open_btn.set_visible(cx, true);
            open_btn.set_text(
                cx,
                if *platform == "Spotify" {
                    "Open Spotify"
                } else {
                    "Open YouTube"
                },
            );
        } else {
            open_btn.set_visible(cx, false);
        }

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
            self.detail_state.tracks = updated;
            self.rebuild_detail_tracks_view();
            self.refresh_detail_hero(cx);
        }
        self.refresh_playlists_grid(cx);
        self.rebuild_library_views(cx);
        self.spawn_cloud_add_track(playlist_id.to_string(), track.clone());
        self.show_toast(
            cx,
            &format!(
                "Added '{}' to '{}'",
                clip_text(&track.title, 22),
                playlist_name
            ),
        );
    }

    fn clear_now_playing_ui(&mut self, cx: &mut Cx) {
        self.pending_stream_track_id = None;
        self.pending_stream_track = None;
        self.last_pos_txt.clear();
        self.last_dur_txt.clear();
        self.last_codec_txt.clear();
        self.synced_lyrics = SyncedLyrics::default();
        self.set_lyrics_center_message(cx, "Play a song to load synced lyrics.");
        self.ui.label(id!(np_title_lbl)).set_text(cx, "Not playing");
        self.ui
            .label(id!(np_artist_lbl))
            .set_text(cx, "Pick any song");
        self.ui
            .label(id!(fs_title_lbl))
            .set_text(cx, "No track playing");
        self.ui.label(id!(fs_artist_lbl)).set_text(cx, "");
        self.ui.button(id!(codec_pill_btn)).set_text(cx, "HI-FI");
        self.ui.label(id!(pos_lbl)).set_text(cx, "0:00");
        self.ui.button(id!(dur_btn)).set_text(cx, "0:00");
        self.ui.label(id!(fs_pos_lbl)).set_text(cx, "0:00");
        self.ui.label(id!(fs_dur_lbl)).set_text(cx, "0:00");
        self.ui
            .ferric_bar_slider(id!(seek_slider))
            .set_value(cx, 0.0);
        self.ui
            .ferric_bar_slider(id!(fs_seek_slider))
            .set_value(cx, 0.0);
        self.ui
            .ferric_play_btn(id!(play_pause_btn))
            .set_playing(cx, false);
        self.ui
            .ferric_play_btn(id!(fs_play_pause_btn))
            .set_playing(cx, false);
        self.ui
            .ferric_cover_art(id!(np_art))
            .set_texture(cx, None, 1.0);
        self.ui
            .ferric_cover_art(id!(fs_art))
            .set_texture(cx, None, 1.0);
        self.sync_fullscreen_backdrop_artwork(cx);
        if self.video_open {
            self.close_native_video_stage(cx);
        }
        if let Some(cache) = &mut self.artwork {
            cache.pin_active_track(None);
        }
        if let Some(smtc) = &mut self.smtc {
            smtc.set_stopped();
        }
        if let Some(rpc) = &mut self.discord_rpc {
            rpc.update_playback(None);
        }
        win32_update_tray_tooltip("Ferric - Music Player");
        win32_update_taskbar_playback("", "", false);
        self.refresh_discord_preview(cx);
        self.apply_adaptive_accent_live(cx);
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
                self.detail_state.tracks = updated;
                self.rebuild_detail_tracks_view();
            }
            self.refresh_detail_hero(cx);
            self.refresh_playlists_grid(cx);
            self.spawn_cloud_remove_track(pl_id, target.clone());
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

        let was_playing_target = self.queue.current().is_some_and(|cur| cur.id == target.id);
        let _ = self.queue.remove_track_by_id(&target.id);
        if was_playing_target {
            if let Some(audio) = &self.audio {
                audio.send(EngineCommand::Stop);
            }
            if let Some(next) = self.queue.current().cloned() {
                self.play_track(cx, next);
            } else {
                self.clear_now_playing_ui(cx);
            }
        }

        // Remove from Ferric SQLite library (never deletes user's own music files on disk;
        // removes generated demo `.wav` only for `demo:` tracks after engine handle is released).
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
        if self.active_page == MainPage::Search {
            let mut next_search: Vec<Track> = (*self.search_tracks).clone();
            next_search.retain(|t| t.id != target.id);
            self.search_tracks = Rc::new(next_search);
        } else if self.active_page == MainPage::Detail {
            self.detail_state.tracks.retain(|t| t.id != target.id);
            self.rebuild_detail_tracks_view();
            self.refresh_detail_hero(cx);
        }
        self.sync_next_deck_to_engine();
        self.rebuild_library_views(cx);
        self.refresh_right_sidebar(cx);
        self.show_toast(cx, &format!("Removed '{title}' from library"));
    }

    fn remove_all_demo_tracks(&mut self, cx: &mut Cx) {
        let playing_demo = self
            .queue
            .current()
            .is_some_and(|cur| cur.id.starts_with("demo:"));
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
        if playing_demo {
            if let Some(audio) = &self.audio {
                audio.send(EngineCommand::Stop);
            }
            if let Some(next) = self.queue.current().cloned() {
                self.play_track(cx, next);
            } else {
                self.clear_now_playing_ui(cx);
            }
        }
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
        self.sync_next_deck_to_engine();
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

    fn build_discord_snapshot(
        track: &Track,
        position_ms: u64,
        duration_ms: u64,
        is_playing: bool,
    ) -> DiscordTrackSnapshot {
        let listen_button =
            track_primary_link(track).map(|(platform, url)| (format!("Listen on {platform}"), url));
        DiscordTrackSnapshot {
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            cover_url: track.thumbnail_url.clone(),
            listen_button,
            position_ms,
            duration_ms,
            is_playing,
        }
    }

    fn mark_session_dirty(&mut self) {
        self.session_save_dirty_at = Some(Instant::now() + Duration::from_millis(400));
    }

    fn flush_session_state_now(&mut self) {
        if self.session_save_dirty_at.take().is_some()
            && let Some(db) = &self.db
        {
            let _ = db.save_session_state(&self.session_state);
        }
    }

    fn refresh_output_device_ui(&self, cx: &mut Cx) {
        let active_name = self
            .audio
            .as_ref()
            .map_or("System Default", AudioEngine::active_device_name);
        let pref = self.dsp.output_device.as_deref();
        self.ui
            .label(id!(pb_device_active_lbl))
            .set_text(cx, &format!("Active: {}", clip_text(active_name, 38)));
        self.ui.button(id!(pb_device_cycle_btn)).set_text(
            cx,
            &format!(
                "Output: {}",
                clip_text(pref.unwrap_or("System Default"), 34)
            ),
        );
        self.style_chip_btn(cx, id!(pb_dev_default), pref.is_none());
        for (i, cid) in [id!(pb_dev_0), id!(pb_dev_1), id!(pb_dev_2), id!(pb_dev_3)]
            .into_iter()
            .enumerate()
        {
            if let Some(dev_name) = self.output_devices.get(i) {
                self.ui.button(cid).set_visible(cx, true);
                self.ui.button(cid).set_text(cx, &clip_text(dev_name, 22));
                let is_sel = pref.is_some_and(|p| p.eq_ignore_ascii_case(dev_name));
                self.style_chip_btn(cx, cid, is_sel);
            } else {
                self.ui.button(cid).set_visible(cx, false);
            }
        }
    }

    fn select_output_device(&mut self, cx: &mut Cx, preferred: Option<String>) {
        let cleaned = preferred
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        self.dsp.output_device = cleaned.clone();
        if let Some(db) = &self.db {
            let _ = db.save_playback_dsp(&self.dsp);
        }
        let msg = if let Some(audio) = &mut self.audio {
            match audio.switch_output_device(cleaned) {
                Ok(active) => format!("Audio output: {}", clip_text(&active, 38)),
                Err(err) => format!("Audio output switch failed: {err}"),
            }
        } else {
            "Audio output preference saved".to_string()
        };
        self.refresh_output_device_ui(cx);
        self.show_toast(cx, &msg);
    }

    fn active_spotify_write_token(&self) -> Option<String> {
        if self.accounts.spotify_web_connected
            && !self.accounts.spotify_web_access_token.trim().is_empty()
        {
            return Some(self.accounts.spotify_web_access_token.trim().to_string());
        }
        if self.accounts.spotify_connected && !self.accounts.spotify_access_token.trim().is_empty()
        {
            return Some(self.accounts.spotify_access_token.trim().to_string());
        }
        None
    }

    fn preferred_cloud_write_target(&self, tracks: &[Track]) -> Option<TrackSource> {
        let has_sp = self.active_spotify_write_token().is_some();
        let has_yt = self.current_ytm_session().is_signed_in();
        match (has_sp, has_yt) {
            (false, false) => None,
            (true, false) => Some(TrackSource::Spotify),
            (false, true) => Some(TrackSource::YouTubeMusic),
            (true, true) => {
                let yt_count = tracks
                    .iter()
                    .filter(|t| t.source == TrackSource::YouTubeMusic && t.spotify_id.is_none())
                    .count();
                let sp_count = tracks
                    .iter()
                    .filter(|t| t.source == TrackSource::Spotify || t.spotify_id.is_some())
                    .count();
                if yt_count > sp_count {
                    Some(TrackSource::YouTubeMusic)
                } else {
                    Some(TrackSource::Spotify)
                }
            }
        }
    }

    fn spawn_cloud_create_playlist(
        &self,
        local_id: String,
        name: String,
        description: String,
        tracks: Vec<Track>,
    ) {
        let Some(target) = self.preferred_cloud_write_target(&tracks) else {
            return;
        };
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        match target {
            TrackSource::Spotify => {
                let Some(token) = self.active_spotify_write_token() else {
                    return;
                };
                thread::spawn(move || {
                    let mut uris = Vec::with_capacity(tracks.len());
                    let mut skipped = 0usize;
                    let mut search_lookups = 0usize;
                    for t in &tracks {
                        if let Some(sid) = t
                            .spotify_id
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            uris.push(sid.to_string());
                        } else if search_lookups < 25 {
                            search_lookups += 1;
                            match search_spotify_track_uri(&token, &t.title, &t.artist) {
                                Ok(Some(found)) => uris.push(found),
                                _ => skipped += 1,
                            }
                        } else {
                            skipped += 1;
                        }
                    }
                    match create_spotify_playlist(&token, &name, &description, &uris) {
                        Ok(remote_id) => {
                            let _ = tx.send(BgMessage::CloudPlaylistCreated {
                                local_id,
                                remote_id,
                                source: TrackSource::Spotify,
                                provider_label: "Spotify",
                                name,
                                pushed_tracks: uris.len(),
                                skipped_tracks: skipped,
                            });
                        }
                        Err(ferric_providers::spotify_web::SpotifyWebSyncError::AuthExpired) => {
                            let _ = tx.send(BgMessage::CloudPlaylistMutationNotice(
                                "Saved locally (Spotify session expired; sign in again to sync to account)".to_string(),
                            ));
                        }
                        Err(
                            ferric_providers::spotify_web::SpotifyWebSyncError::FallbackToRest(err),
                        ) => {
                            let _ = tx.send(BgMessage::CloudPlaylistMutationNotice(format!(
                                "Saved locally (Spotify sync failed: {})",
                                clip_text(&err, 60)
                            )));
                        }
                    }
                });
            }
            TrackSource::YouTubeMusic => {
                let session = self.current_ytm_session();
                if !session.is_signed_in() {
                    return;
                }
                thread::spawn(move || {
                    let mut vids = Vec::with_capacity(tracks.len());
                    let mut skipped = 0usize;
                    let mut lookups = 0usize;
                    for t in &tracks {
                        if let Some(vid) = t
                            .youtube_video_id
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            vids.push(vid.to_string());
                        } else if lookups < 25 {
                            lookups += 1;
                            let q = format!("{} {}", t.artist, t.title);
                            if let Ok(found) =
                                ferric_providers::ytmusic::search_music(&session, &q, 3)
                                && let Some(vid) =
                                    found.first().and_then(|x| x.youtube_video_id.clone())
                            {
                                vids.push(vid);
                            } else {
                                skipped += 1;
                            }
                        } else {
                            skipped += 1;
                        }
                    }
                    match create_ytm_playlist(&session, &name, &description, &vids) {
                        Ok(remote_id) => {
                            let _ = tx.send(BgMessage::CloudPlaylistCreated {
                                local_id,
                                remote_id,
                                source: TrackSource::YouTubeMusic,
                                provider_label: "YouTube Music",
                                name,
                                pushed_tracks: vids.len(),
                                skipped_tracks: skipped,
                            });
                        }
                        Err(err) => {
                            let _ = tx.send(BgMessage::CloudPlaylistMutationNotice(format!(
                                "Saved locally (YouTube sync failed: {})",
                                clip_text(&err.to_string(), 60)
                            )));
                        }
                    }
                });
            }
            _ => {}
        }
    }

    fn spawn_cloud_add_track(&self, playlist_id: String, track: Track) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        if playlist_id.starts_with("spotify:playlist:")
            || playlist_id.starts_with("spotify:premium:playlist:")
        {
            let Some(token) = self.active_spotify_write_token() else {
                return;
            };
            thread::spawn(move || {
                let uri = track
                    .spotify_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .or_else(|| {
                        search_spotify_track_uri(&token, &track.title, &track.artist)
                            .ok()
                            .flatten()
                    });
                let Some(uri) = uri else {
                    let _ = tx.send(BgMessage::CloudPlaylistMutationNotice(
                        "Added locally (no matching Spotify track ID found)".to_string(),
                    ));
                    return;
                };
                match add_tracks_to_spotify_playlist(&token, &playlist_id, &[uri]) {
                    Ok(()) => {}
                    Err(ferric_providers::spotify_web::SpotifyWebSyncError::AuthExpired) => {
                        let _ = tx.send(BgMessage::CloudPlaylistMutationNotice(
                            "Added locally (Spotify session expired; sign in again to sync)"
                                .to_string(),
                        ));
                    }
                    Err(ferric_providers::spotify_web::SpotifyWebSyncError::FallbackToRest(e)) => {
                        let _ = tx.send(BgMessage::CloudPlaylistMutationNotice(format!(
                            "Added locally (Spotify write failed: {})",
                            clip_text(&e, 54)
                        )));
                    }
                }
            });
        } else if playlist_id.starts_with("ytm:pl:") || playlist_id.starts_with("yt:playlist:") {
            let session = self.current_ytm_session();
            if !session.is_signed_in() {
                return;
            }
            thread::spawn(move || {
                let vid = track
                    .youtube_video_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .or_else(|| {
                        let q = format!("{} {}", track.artist, track.title);
                        ferric_providers::ytmusic::search_music(&session, &q, 3)
                            .ok()
                            .and_then(|v| v.first().and_then(|x| x.youtube_video_id.clone()))
                    });
                let Some(vid) = vid else {
                    return;
                };
                if let Err(err) = add_videos_to_ytm_playlist(&session, &playlist_id, &[vid]) {
                    let _ = tx.send(BgMessage::CloudPlaylistMutationNotice(format!(
                        "Added locally (YouTube sync failed: {})",
                        clip_text(&err.to_string(), 54)
                    )));
                }
            });
        }
    }

    fn spawn_cloud_remove_track(&self, playlist_id: String, track: Track) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        if playlist_id.starts_with("spotify:playlist:")
            || playlist_id.starts_with("spotify:premium:playlist:")
        {
            let Some(token) = self.active_spotify_write_token() else {
                return;
            };
            let Some(sid) = track.spotify_id.filter(|s| !s.trim().is_empty()) else {
                return;
            };
            thread::spawn(move || {
                if let Err(ferric_providers::spotify_web::SpotifyWebSyncError::AuthExpired) =
                    remove_tracks_from_spotify_playlist(&token, &playlist_id, &[sid])
                {
                    let _ = tx.send(BgMessage::CloudPlaylistMutationNotice(
                        "Removed locally (Spotify session expired; sign in again)".to_string(),
                    ));
                }
            });
        } else if playlist_id.starts_with("ytm:pl:") || playlist_id.starts_with("yt:playlist:") {
            let session = self.current_ytm_session();
            let Some(vid) = track.youtube_video_id.filter(|s| !s.trim().is_empty()) else {
                return;
            };
            if session.is_signed_in() {
                thread::spawn(move || {
                    let _ = remove_video_from_ytm_playlist(&session, &playlist_id, &vid);
                });
            }
        }
    }

    fn spawn_cloud_rename_playlist(&self, playlist_id: String, new_name: String) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        if playlist_id.starts_with("spotify:playlist:")
            || playlist_id.starts_with("spotify:premium:playlist:")
        {
            let Some(token) = self.active_spotify_write_token() else {
                return;
            };
            thread::spawn(move || {
                if let Err(ferric_providers::spotify_web::SpotifyWebSyncError::AuthExpired) =
                    rename_spotify_playlist(&token, &playlist_id, &new_name)
                {
                    let _ = tx.send(BgMessage::CloudPlaylistMutationNotice(
                        "Renamed locally (Spotify session expired; sign in again)".to_string(),
                    ));
                }
            });
        } else if playlist_id.starts_with("ytm:pl:") || playlist_id.starts_with("yt:playlist:") {
            let session = self.current_ytm_session();
            if session.is_signed_in() {
                thread::spawn(move || {
                    let _ = rename_ytm_playlist(&session, &playlist_id, &new_name);
                });
            }
        }
    }

    fn spawn_cloud_delete_playlist(&self, playlist_id: String) {
        if playlist_id.starts_with("spotify:playlist:")
            || playlist_id.starts_with("spotify:premium:playlist:")
        {
            if let Some(token) = self.active_spotify_write_token() {
                thread::spawn(move || {
                    let _ = unfollow_spotify_playlist(&token, &playlist_id);
                });
            }
        } else if playlist_id.starts_with("ytm:pl:") || playlist_id.starts_with("yt:playlist:") {
            let session = self.current_ytm_session();
            if session.is_signed_in() {
                thread::spawn(move || {
                    let _ = delete_ytm_playlist(&session, &playlist_id);
                });
            }
        }
    }

    fn spawn_cloud_like_track(&self, track: Track, liked: bool) {
        if let Some(sid) = track.spotify_id.filter(|s| !s.trim().is_empty())
            && let Some(token) = self.active_spotify_write_token()
        {
            thread::spawn(move || {
                let _ = set_spotify_track_liked(&token, &sid, liked);
            });
        } else if let Some(vid) = track.youtube_video_id.filter(|s| !s.trim().is_empty()) {
            let session = self.current_ytm_session();
            if session.is_signed_in() {
                thread::spawn(move || {
                    let _ = set_ytm_track_liked(&session, &vid, liked);
                });
            }
        }
    }

    fn collect_queue_tracks(&self) -> Vec<Track> {
        let mut out = Vec::new();
        if let Some(cur) = self.queue.current() {
            out.push(cur.clone());
        }
        for t in self.queue.peek_upcoming(100) {
            if !out.iter().any(|x| x.id == t.id) {
                out.push(t.clone());
            }
        }
        out
    }

    fn save_queue_as_playlist(&mut self, cx: &mut Cx) {
        let tracks = self.collect_queue_tracks();
        if tracks.is_empty() {
            self.show_toast(cx, "Queue is empty");
            return;
        }
        let ctx = self.queue_context.trim();
        let name = if ctx.is_empty() || ctx.eq_ignore_ascii_case("Queue") {
            "Saved Queue".to_string()
        } else {
            format!("Queue - {}", clip_text(ctx, 28))
        };
        let pl_id = format!("local:pl:{}", csprng_hex(6));
        let track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
        let thumb = tracks.first().and_then(|t| t.thumbnail_url.clone());
        let pl = Playlist {
            id: pl_id.clone(),
            name: name.clone(),
            source: TrackSource::Local,
            etag: None,
            track_count: tracks.len(),
            description: "Saved from Ferric Queue".to_string(),
            thumbnail_url: thumb,
        };
        if let Some(db) = &mut self.db {
            let _ = db.upsert_tracks(&tracks);
            let _ = db.upsert_playlist(&pl, &track_ids);
            self.all_tracks = db.list_tracks("").unwrap_or_default();
            self.playlists = db.list_playlists().unwrap_or_default();
        }
        self.refresh_playlists_grid(cx);
        self.rebuild_library_views(cx);
        self.spawn_cloud_create_playlist(
            pl_id,
            name.clone(),
            "Saved from Ferric Queue".to_string(),
            tracks.clone(),
        );
        self.show_toast(cx, &format!("Saved '{}' ({} songs)", name, tracks.len()));
    }

    fn copy_queue_tracklist(&mut self, cx: &mut Cx) {
        let tracks = self.collect_queue_tracks();
        if tracks.is_empty() {
            self.show_toast(cx, "Queue is empty");
            return;
        }
        let title = if self.queue_context.trim().is_empty() {
            "Ferric Queue"
        } else {
            self.queue_context.trim()
        };
        let txt = format_collection_tracklist(title, None, &tracks);
        let _ = copy_text_to_clipboard(&txt);
        self.show_toast(
            cx,
            &format!("Copied queue ({} songs) to clipboard", tracks.len()),
        );
    }

    fn open_artist_for_name(&mut self, cx: &mut Cx, artist: &str) {
        let trimmed = artist.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("Pick a song to play") {
            return;
        }
        if let Some(g) = self
            .artist_groups
            .iter()
            .find(|g| g.title.eq_ignore_ascii_case(trimmed))
            .cloned()
        {
            self.open_local_detail(cx, g);
        } else {
            self.spawn_rich_search(cx, trimmed.to_string());
        }
    }

    fn detail_share_link(&self) -> Option<(&'static str, String)> {
        if let Some(url) = self.detail_state.share_url.as_deref()
            && !url.trim().is_empty()
        {
            let platform = if url.starts_with("https://open.spotify.com/") {
                "Spotify"
            } else {
                "YouTube Music"
            };
            return Some((platform, url.to_string()));
        }
        collection_share_url(&self.detail_state.id, self.detail_state.kind)
    }

    fn rebuild_detail_tracks_view(&mut self) {
        let q = self.detail_filter_query.trim();
        let mut list: Vec<Track> = if q.is_empty() {
            self.detail_state.tracks.clone()
        } else {
            self.detail_state
                .tracks
                .iter()
                .filter(|t| {
                    contains_ascii_icase(&t.title, q)
                        || contains_ascii_icase(&t.artist, q)
                        || contains_ascii_icase(&t.album, q)
                })
                .cloned()
                .collect()
        };
        match self.detail_sort_order {
            DetailSortOrder::Original => {}
            DetailSortOrder::Title => {
                list.sort_by(|a, b| cmp_ascii_icase(&a.title, &b.title));
            }
            DetailSortOrder::Artist => {
                list.sort_by(|a, b| {
                    cmp_ascii_icase(&a.artist, &b.artist)
                        .then_with(|| cmp_ascii_icase(&a.title, &b.title))
                });
            }
            DetailSortOrder::Duration => {
                list.sort_by_key(|t| std::cmp::Reverse(t.duration_ms));
            }
        }
        self.detail_tracks = Rc::new(list);
    }

    fn copy_track_share_link(&mut self, cx: &mut Cx, track: &Track) {
        if let Some((platform, url)) = track_primary_link(track) {
            let _ = copy_text_to_clipboard(&url);
            self.show_toast(
                cx,
                &format!("Copied {platform} link: {}", clip_text(&track.title, 28)),
            );
        } else {
            let query = format!("{} - {}", track.artist, track.title);
            let _ = copy_text_to_clipboard(&query);
            self.show_toast(
                cx,
                &format!(
                    "Copied title (local track): {}",
                    clip_text(&track.title, 24)
                ),
            );
        }
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
            use std::fmt::Write as _;
            let mut out = String::with_capacity(64 + self.detail_state.tracks.len() * 96);
            out.push_str("#EXTM3U\n");
            for t in &self.detail_state.tracks {
                let secs = i64::try_from(t.duration_ms / 1000).unwrap_or(i64::MAX);
                let artist = t.artist.replace(['\r', '\n'], " ");
                let title = t.title.replace(['\r', '\n'], " ");
                let _ = writeln!(out, "#EXTINF:{secs},{artist} - {title}");
                if let Some(p) = &t.path {
                    let path_line = p.to_string_lossy().replace(['\r', '\n'], "");
                    if !path_line.is_empty() {
                        let _ = writeln!(out, "{path_line}");
                    }
                } else if let Some((_, url)) = track_primary_link(t) {
                    let _ = writeln!(out, "{url}");
                }
            }
            match std::fs::write(&dest, out) {
                Ok(()) => {
                    self.show_toast(cx, &format!("Exported '{}'", self.detail_state.title));
                }
                Err(err) => {
                    self.show_toast(cx, &format!("Playlist export failed: {err}"));
                }
            }
        }
    }

    fn export_detail_csv(&mut self, cx: &mut Cx) {
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
        let default_file = format!("{}.csv", safe_name.trim());
        if let Some(dest) = rfd::FileDialog::new()
            .set_title("Export Playlist (.csv)")
            .set_file_name(&default_file)
            .add_filter("CSV Spreadsheet", &["csv"])
            .save_file()
        {
            let out = format_collection_csv(&self.detail_state.tracks);
            match std::fs::write(&dest, out) {
                Ok(()) => {
                    self.show_toast(
                        cx,
                        &format!("Exported '{}' (.csv)", self.detail_state.title),
                    );
                }
                Err(err) => {
                    self.show_toast(cx, &format!("CSV export failed: {err}"));
                }
            }
        }
    }

    fn spawn_stream_resolve(&mut self, track: Track, auto_play: bool) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let seq_counter = Arc::clone(
            self.stream_request_seq
                .get_or_insert_with(|| Arc::new(AtomicU64::new(0))),
        );
        let expected_seq = if auto_play {
            Some(seq_counter.fetch_add(1, Ordering::AcqRel).wrapping_add(1))
        } else {
            None
        };
        let session = self.current_ytm_session();
        let cache_dir = app_data_dir().join("stream_cache");
        let audio_quality = self.dsp.audio_quality;
        let track_id = track.id;
        let artist = track.artist;
        let title = track.title;
        let existing_vid = track.youtube_video_id;

        thread::spawn(move || {
            let is_stale =
                || expected_seq.is_some_and(|mine| seq_counter.load(Ordering::Acquire) != mine);
            if is_stale() {
                return;
            }
            let outcome = (|| {
                let vid = match existing_vid {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => resolve_best_video_id_for_track(&session, &artist, &title)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| format!("No YouTube Music stream found for '{title}'"))?,
                };
                if is_stale() {
                    return Err(String::new());
                }
                let stream = resolve_and_cache_audio_stream_with_quality(
                    &vid,
                    &session,
                    &cache_dir,
                    audio_quality,
                )
                .map_err(|e| e.to_string())?;
                Ok::<_, String>((vid, stream))
            })();
            if is_stale() {
                return;
            }

            match outcome {
                Ok((video_id, stream)) => {
                    let _ = tx.send(BgMessage::StreamReady(Box::new(StreamReadyPayload {
                        track_id,
                        video_id,
                        stream,
                        auto_play,
                    })));
                }
                Err(error) => {
                    let _ = tx.send(BgMessage::StreamFailed {
                        track_id,
                        error,
                        auto_play,
                    });
                }
            }
            SignalToUI::set_ui_signal();
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
        // Manual track switches always start cleanly without crossfade; crossfade is exclusively
        // triggered automatically by the dual-deck engine when a track finishes naturally.
        audio.send(EngineCommand::LoadAndPlayTrack {
            path,
            replay_gain_db,
            fallback_duration_ms,
        });
    }

    fn play_track(&mut self, cx: &mut Cx, track: Track) {
        if self.is_controlling_remote()
            && let Some(engine) = &self.connect_engine
        {
            let entries: Vec<QueueEntry> = self
                .queue
                .tracks()
                .iter()
                .take(200)
                .map(QueueEntry::from_track)
                .collect();
            let idx = entries
                .iter()
                .position(|t| t.track_id == track.id)
                .unwrap_or(0);
            let tracks_to_send = if entries.is_empty() {
                vec![QueueEntry::from_track(&track)]
            } else {
                entries
            };
            engine.send_remote_action(RemoteAction::PlayQueue {
                tracks: tracks_to_send,
                start_index: idx,
                context: self.queue_context.clone(),
            });
            return;
        }
        self.play_track_inner(cx, track, true);
        self.push_connect_playback_snapshot();
    }

    fn play_track_inner(&mut self, cx: &mut Cx, track: Track, reset_failures: bool) {
        if reset_failures {
            self.consecutive_stream_failures = 0;
        }
        let is_hidden = self.window_hidden_or_minimized || win32_is_window_hidden_or_minimized();
        self.last_pos_txt.clear();
        self.last_dur_txt.clear();
        self.last_codec_txt.clear();
        if !is_hidden && let Some(cache) = &mut self.artwork {
            cache.pin_active_track(Some(&track));
        }
        if let Some(path) = track.path.as_ref().filter(|p| p.is_file()).cloned() {
            self.pending_stream_track_id = None;
            self.pending_stream_track = None;
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
            self.pending_stream_track = Some(track.clone());
            // Clear any preloaded Deck B while resolving a manual online stream so an old track
            // cannot crossfade mid-buffering.
            if let Some(audio) = &self.audio {
                audio.send(EngineCommand::QueueNextTrack {
                    path: None,
                    same_album_consecutive: false,
                    replay_gain_db: 0,
                    fallback_duration_ms: 0,
                });
            }
            if is_hidden {
                self.ui_dirty_while_hidden = true;
            } else {
                self.ui
                    .label(id!(np_title_lbl))
                    .set_text(cx, &clip_text(&track.title, 22));
                self.ui.label(id!(np_artist_lbl)).set_text(
                    cx,
                    &clip_text(&format!("{} · Buffering...", track.artist), 24),
                );
                self.ui
                    .label(id!(fs_title_lbl))
                    .set_text(cx, &clip_text(&clean_display_text(&track.title), 36));
                self.ui.label(id!(fs_artist_lbl)).set_text(
                    cx,
                    &clip_text(&format!("{} · Buffering...", track.artist), 44),
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
                self.bind_cover_art_url(
                    cx,
                    self.ui.ferric_cover_art(id!(fs_art)),
                    track.thumbnail_url.as_deref(),
                    Some(&track),
                    false,
                    0.5,
                    false,
                );
                self.sync_fullscreen_backdrop_artwork(cx);
                self.refresh_right_sidebar(cx);
                self.ui.redraw(cx);
            }
            self.spawn_stream_resolve(track.clone(), true);
            self.maybe_fetch_radio_for_track(&track, false);
            return;
        }

        if !is_hidden {
            self.show_toast(
                cx,
                &format!(
                    "'{}' has no local file. Search YouTube Music instead.",
                    track.title
                ),
            );
        }
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
            let pal = self.adaptive_palette();
            let accent = rgba_to_vec4(pal.accent);
            let raw_accent = self.current_accent();
            let like_col = if next_liked {
                accent
            } else {
                rgba_to_vec4(pal.icon_muted)
            };
            let fs_like_col = if next_liked {
                raw_accent
            } else {
                vec4(1.0, 1.0, 1.0, 0.72)
            };
            self.ui
                .button(id!(np_like_btn))
                .apply_over(cx, live! { draw_icon: { color: (like_col) } });
            self.ui.button(id!(fs_like_btn)).apply_over(
                cx,
                live! { draw_bg: { acrylic_on: 1.0, light_mode: 0.0 }, draw_icon: { color: (fs_like_col) } },
            );
        }
        self.rebuild_library_views(cx);
        self.spawn_cloud_like_track(saved.clone(), next_liked);
        if self.is_controlling_remote()
            && let Some(engine) = &self.connect_engine
        {
            engine.send_remote_action(RemoteAction::ToggleLike { track_id: saved.id });
        }
        next_liked
    }

    fn update_now_playing_metadata(&mut self, cx: &mut Cx, track: &Track) {
        self.persist_track_to_library(track);
        self.session_state
            .record_play_with_artist(&track.id, &track.artist, track.duration_ms);
        if let Some(db) = &self.db {
            let _ = db.save_session_state(&self.session_state);
        }

        win32_update_tray_tooltip(&format!("Ferric - {} · {}", track.title, track.artist));
        win32_update_taskbar_playback(&track.title, &track.artist, true);

        if let Some(smtc) = &mut self.smtc {
            smtc.set_metadata_for_track(track);
            smtc.sync_playback(true, 0, track.duration_ms);
        }
        if let Some(rpc) = &mut self.discord_rpc {
            rpc.update_playback(Some(Self::build_discord_snapshot(
                track,
                0,
                track.duration_ms,
                true,
            )));
        }

        self.lyrics_offset_ms = 0;

        // While hidden in System Tray / minimized, skip ALL GPU UI mutations, artwork decode bursts,
        // and hidden redraws: defer UI sync to `TrayEvent::ShowWindow` and trim transient working set.
        if self.window_hidden_or_minimized || win32_is_window_hidden_or_minimized() {
            self.ui_dirty_while_hidden = true;
            self.tray_second_trim_at = Some(Instant::now() + Duration::from_millis(1_800));
            return;
        }

        self.sync_now_playing_ui_widgets(cx, track, true);
    }

    fn sync_now_playing_ui_widgets(&mut self, cx: &mut Cx, track: &Track, is_playing: bool) {
        if let Some(cache) = &mut self.artwork {
            cache.pin_active_track(Some(track));
        }

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

        let pal = self.adaptive_palette();
        let accent = rgba_to_vec4(pal.accent);
        let raw_accent = self.current_accent();
        let is_liked = self
            .all_tracks
            .iter()
            .find(|t| t.id == track.id)
            .map_or(track.liked, |t| t.liked);
        let like_col = if is_liked {
            accent
        } else {
            rgba_to_vec4(pal.icon_muted)
        };
        let fs_like_col = if is_liked {
            raw_accent
        } else {
            vec4(1.0, 1.0, 1.0, 0.72)
        };
        self.ui
            .button(id!(np_like_btn))
            .apply_over(cx, live! { draw_icon: { color: (like_col) } });
        self.ui.button(id!(fs_like_btn)).apply_over(
            cx,
            live! { draw_bg: { acrylic_on: 1.0, light_mode: 0.0 }, draw_icon: { color: (fs_like_col) } },
        );
        self.ui
            .ferric_play_btn(id!(play_pause_btn))
            .set_playing(cx, is_playing);
        self.ui
            .ferric_play_btn(id!(fs_play_pause_btn))
            .set_playing(cx, is_playing);
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

        self.bind_hero_cover_art_url(
            cx,
            self.ui.ferric_cover_art(id!(np_art)),
            track.thumbnail_url.as_deref(),
            Some(track),
            is_playing,
            0.55,
            false,
        );
        self.bind_hero_cover_art_url(
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
        if self.active_page == MainPage::Settings {
            self.refresh_discord_preview(cx);
        }

        self.load_lyrics_for_track(cx, track);
        // Fast O(recents) recents refresh instead of rebuilding all Album/Artist groups on every Next press
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
        if self.active_page == MainPage::Songs {
            self.rebuild_library_views(cx);
        } else if self.active_page == MainPage::Home {
            self.refresh_home_ui(cx);
        }
        self.refresh_right_sidebar(cx);

        // If native in-app video clip stage is open, follow the newly playing track automatically
        if self.video_open && self.video_track_id.as_deref() != Some(track.id.as_str()) {
            self.open_native_video_stage(cx);
        }
        self.ui.redraw(cx);
    }

    fn load_lyrics_for_track(&mut self, cx: &mut Cx, track: &Track) {
        let seq_counter = Arc::clone(
            self.lyrics_request_seq
                .get_or_insert_with(|| Arc::new(AtomicU64::new(0))),
        );
        let my_seq = seq_counter.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        self.synced_lyrics = SyncedLyrics::default();
        self.set_lyrics_center_message(cx, "Searching synced lyrics (LRCLIB)...");

        if let Some(db) = &self.db
            && let Ok(Some(cached)) = db.get_cached_lyrics(&track.id)
            && (track.source == TrackSource::Local || !is_polluted_multilingual_lrc(&cached))
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
            if seq_counter.load(Ordering::Acquire) != my_seq {
                return;
            }
            let fetched =
                fetch_best_free_lyrics(&artist, &title, &album, dur_ms, local_path.as_deref());
            if seq_counter.load(Ordering::Acquire) != my_seq {
                return;
            }
            if let Ok(Some((parsed, raw))) = fetched {
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
            SignalToUI::set_ui_signal();
        });
    }

    fn set_lyrics_center_message(&self, cx: &mut Cx, msg: &str) {
        let raw_accent = self.current_accent();
        let pal = self.adaptive_palette();
        let accent = rgba_to_vec4(pal.accent);
        let z = self.theme.ui_zoom.clamp(0.65, 1.40);
        let rs = self.ui.ferric_lyrics_view(id!(rs_lyrics_scroll));
        rs.set_style(cx, accent, z, pal.is_light);
        rs.set_empty_message(cx, msg);
        let fs = self.ui.ferric_lyrics_view(id!(fs_lyrics_scroll));
        fs.set_style(cx, raw_accent, z, false);
        fs.set_empty_message(cx, msg);
    }

    fn push_lyrics_to_views(&self, cx: &mut Cx) {
        let raw_accent = self.current_accent();
        let pal = self.adaptive_palette();
        let accent = rgba_to_vec4(pal.accent);
        let z = self.theme.ui_zoom.clamp(0.65, 1.40);
        let items: Vec<(u64, String)> = self
            .synced_lyrics
            .lines
            .iter()
            .map(|l| (l.timestamp_ms, l.text.clone()))
            .collect();
        let rs = self.ui.ferric_lyrics_view(id!(rs_lyrics_scroll));
        rs.set_style(cx, accent, z, pal.is_light);
        rs.set_lyrics(cx, items.clone());
        let fs = self.ui.ferric_lyrics_view(id!(fs_lyrics_scroll));
        fs.set_style(cx, raw_accent, z, false);
        fs.set_lyrics(cx, items);
    }

    fn update_lyrics_display(&self, cx: &mut Cx, raw_pos_ms: u64) {
        if self.synced_lyrics.lines.is_empty() {
            return;
        }
        let shifted = (raw_pos_ms as i64 + self.lyrics_offset_ms).max(0) as u64;
        let is_playing = if let Some(peer) = self.active_remote_peer() {
            peer.is_playing
        } else {
            self.audio
                .as_ref()
                .is_some_and(|a| a.telemetry().is_playing)
        };
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

    fn push_bounded_ring(ring: &mut std::collections::VecDeque<String>, val: String, cap: usize) {
        if val.is_empty() || cap == 0 {
            return;
        }
        if let Some(pos) = ring.iter().position(|x| x == &val) {
            ring.remove(pos);
        }
        ring.push_back(val);
        while ring.len() > cap {
            ring.pop_front();
        }
    }

    fn record_home_shown_items(&mut self) {
        let pool = self.home_quick_picks_pool();
        let qp_start = self.qp_page * 8;
        for t in pool.iter().skip(qp_start).take(8) {
            Self::push_bounded_ring(&mut self.home_shown_track_ids, t.id.clone(), 48);
            if let Some(vid) = &t.youtube_video_id {
                Self::push_bounded_ring(&mut self.home_shown_track_ids, vid.clone(), 48);
            }
        }
        for (si, shelf) in self.rich_home.shelves.iter().take(8).enumerate() {
            let off = self.shelf_pages[si] * 5;
            for card in shelf.cards.iter().skip(off).take(5) {
                Self::push_bounded_ring(&mut self.home_shown_card_ids, card.id.clone(), 80);
            }
        }
    }

    fn record_stations_shown(&mut self) {
        let off = self.stations_page * 5;
        for st in self.recommended_stations.iter().skip(off).take(5) {
            let norm = primary_artist_key(&st.seed_artist).to_ascii_lowercase();
            if !norm.is_empty() {
                Self::push_bounded_ring(&mut self.home_shown_station_artists, norm, 20);
            }
        }
    }

    fn build_home_taste_profile(&self) -> YtmHomeTasteProfile {
        let mut by_id: HashMap<&str, &Track> = HashMap::new();
        for t in &self.all_tracks {
            by_id.insert(t.id.as_str(), t);
        }
        let mut scored_tracks: Vec<(i32, Track)> = Vec::new();
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Sublinear play-count weighting + strong exponential recency so recent listening shifts
        // recommendations cleanly instead of letting old high-play counts permanently lock slot 0..7.
        let play_weight = |plays: u32| -> i32 { ((1.0 + plays as f32).ln() * 22.0).round() as i32 };

        for (pos, id) in self.session_state.recent_track_ids.iter().enumerate() {
            if let Some(t) = by_id.get(id.as_str())
                && seen_ids.insert(t.id.clone())
            {
                let plays = self
                    .session_state
                    .track_play_counts
                    .get(&t.id)
                    .copied()
                    .unwrap_or(1);
                let recency = (44.0 * (-0.09 * pos as f32).exp()).round() as i32;
                let liked_bonus = if t.liked { 18 } else { 0 };
                scored_tracks.push((play_weight(plays) + recency + liked_bonus, (*t).clone()));
            }
        }
        for (id, &plays) in &self.session_state.track_play_counts {
            if let Some(t) = by_id.get(id.as_str())
                && seen_ids.insert(t.id.clone())
            {
                let liked_bonus = if t.liked { 18 } else { 0 };
                scored_tracks.push((play_weight(plays) + liked_bonus, (*t).clone()));
            }
        }
        for t in &self.all_tracks {
            if t.liked && seen_ids.insert(t.id.clone()) {
                scored_tracks.push((24, t.clone()));
            }
        }
        // Scan ALL cross-account library tracks (no alphabetical `.take(1500)` truncation) so every
        // playlist, letter, and locale contributes evenly to Home discovery & Refresh rotation.
        let mut lib_artist_freq: HashMap<String, (String, i32, Track)> = HashMap::new();
        for t in &self.all_tracks {
            let key = primary_artist_key(&t.artist);
            if key.is_empty() || key.eq_ignore_ascii_case("unknown artist") {
                continue;
            }
            let norm = key.to_ascii_lowercase();
            lib_artist_freq
                .entry(norm)
                .and_modify(|e| {
                    e.1 += if t.liked { 5 } else { 2 };
                    if e.2.youtube_video_id.is_none() && t.youtube_video_id.is_some() {
                        e.2 = t.clone();
                    }
                })
                .or_insert_with(|| (key, if t.liked { 10 } else { 5 }, t.clone()));
        }
        let mut lib_artists_sorted: Vec<(String, i32, Track)> =
            lib_artist_freq.into_values().collect();
        lib_artists_sorted.sort_by_key(|a| std::cmp::Reverse(a.1));
        for (_, freq_score, rep_track) in lib_artists_sorted.iter().take(64) {
            if seen_ids.insert(rep_track.id.clone()) {
                scored_tracks.push(((*freq_score).min(20), rep_track.clone()));
            }
        }

        scored_tracks.sort_by_key(|t| std::cmp::Reverse(t.0));
        let top_tracks: Vec<Track> = scored_tracks.into_iter().take(60).map(|(_, t)| t).collect();

        // Build a stratified sample of up to 400 library tracks across the full alphabet/playlists
        // (prioritizing YT-linked & liked tracks first, then evenly strided deeper library tracks)
        // so Refresh can sample distant seeds and surface "Rediscover your library".
        let mut library_tracks: Vec<Track> = Vec::with_capacity(400);
        let mut lib_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let total_lib = self.all_tracks.len();
        if total_lib > 0 {
            let stride = (total_lib / 240).max(1);
            for t in self.all_tracks.iter().step_by(stride) {
                if (t.youtube_video_id.is_some() || t.liked) && lib_seen.insert(t.id.clone()) {
                    library_tracks.push(t.clone());
                    if library_tracks.len() >= 240 {
                        break;
                    }
                }
            }
            if library_tracks.len() < 400 {
                let stride2 = (total_lib / 320).max(1);
                for t in self.all_tracks.iter().step_by(stride2) {
                    if lib_seen.insert(t.id.clone()) {
                        library_tracks.push(t.clone());
                        if library_tracks.len() >= 400 {
                            break;
                        }
                    }
                }
            }
        }

        let mut artist_scores: HashMap<String, (String, i32)> = HashMap::new();
        for (artist, &cnt) in &self.session_state.artist_play_counts {
            let key = primary_artist_key(artist);
            if !key.is_empty() {
                let norm = key.to_ascii_lowercase();
                let entry = artist_scores.entry(norm).or_insert((key, 0));
                entry.1 += play_weight(cnt);
            }
        }
        for (idx, t) in top_tracks.iter().enumerate() {
            let key = primary_artist_key(&t.artist);
            if !key.is_empty() {
                let norm = key.to_ascii_lowercase();
                let w =
                    28_i32.saturating_sub((idx as i32) / 2).max(4) + if t.liked { 10 } else { 0 };
                let entry = artist_scores.entry(norm).or_insert((key, 0));
                entry.1 += w;
            }
        }
        for (disp, freq_score, _) in lib_artists_sorted.into_iter().take(56) {
            let norm = disp.to_ascii_lowercase();
            let entry = artist_scores.entry(norm).or_insert((disp, 0));
            entry.1 += freq_score.min(24);
        }
        let mut artist_list: Vec<(String, i32)> = artist_scores.into_values().collect();
        artist_list.sort_by_key(|a| std::cmp::Reverse(a.1));
        let top_artists: Vec<String> = artist_list.into_iter().take(48).map(|(a, _)| a).collect();

        let refresh_mood_params = if !self.rich_home.chips.is_empty() {
            let idx = self.home_more_cycle % self.rich_home.chips.len();
            Some(self.rich_home.chips[idx].params.clone())
        } else {
            None
        };

        YtmHomeTasteProfile {
            top_tracks,
            top_artists,
            recent_searches: self.session_state.recent_searches.clone(),
            library_tracks,
            excluded_track_ids: self.home_shown_track_ids.iter().cloned().collect(),
            excluded_card_ids: self.home_shown_card_ids.iter().cloned().collect(),
            excluded_artists: self.home_shown_station_artists.iter().cloned().collect(),
            continuation_token: self.rich_home.continuation_token.clone(),
            refresh_mood_params,
        }
    }

    fn spawn_recommended_stations(&mut self, cx: &mut Cx, rotate: bool) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.stations_request_seq = self.stations_request_seq.wrapping_add(1);
        let seq = self.stations_request_seq;
        if rotate {
            self.record_stations_shown();
            self.stations_rotation_epoch = self.stations_rotation_epoch.wrapping_add(1);
        }
        let rot = self.stations_rotation_epoch;
        self.stations_page = 0;

        let taste = self.build_home_taste_profile();
        let home_tracks: Vec<Track> = self
            .rich_home
            .quick_picks
            .iter()
            .take(40)
            .cloned()
            .collect();

        // Immediately surface instant algorithmic stations from the FULL local/synced library
        // (no alphabetical `.take(1500)` cutoff) so Home feels zero-latency on startup & Shuffle.
        let instant = build_algorithmic_recommended_stations_rotated(
            &taste,
            &self.all_tracks,
            &home_tracks,
            15,
            rot,
        );
        let had_instant = !instant.is_empty();
        if had_instant && (self.recommended_stations.is_empty() || rotate) {
            self.recommended_stations = instant.clone();
            self.record_stations_shown();
            self.refresh_home_ui(cx);
            self.ui.redraw(cx);
        }

        let sp_token = if !self.accounts.spotify_web_access_token.trim().is_empty() {
            Some(self.accounts.spotify_web_access_token.trim().to_string())
        } else if !self.accounts.spotify_access_token.trim().is_empty() {
            Some(self.accounts.spotify_access_token.trim().to_string())
        } else {
            None
        };

        // When no Spotify token is active and Engine B already produced fresh stations synchronously,
        // skip spawning a redundant worker thread that would only re-run Engine B and overwrite them.
        let Some(tok) = sp_token else {
            return;
        };

        thread::spawn(move || {
            let Ok(mut sp_cards) = fetch_spotify_recommended_stations(&tok) else {
                // Keep the already-rendered instant Engine B stations intact on 401/network failure.
                return;
            };
            if sp_cards.is_empty() {
                return;
            }
            let mut merged: Vec<RecommendedStation> = Vec::new();
            let mut seen_names: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            if rot > 0 && sp_cards.len() > 2 {
                let shift = (rot * 3) % sp_cards.len();
                sp_cards.rotate_left(shift);
            }
            for (i, card) in sp_cards.into_iter().enumerate() {
                let key = card.display_name.trim().to_ascii_lowercase();
                if !key.is_empty() && seen_names.insert(key) {
                    merged.push(RecommendedStation::from_spotify_card(&card, (i % 8) as u8));
                    if merged.len() >= 15 {
                        break;
                    }
                }
            }
            for st in instant {
                if merged.len() >= 15 {
                    break;
                }
                let key = st.display_name.trim().to_ascii_lowercase();
                if !key.is_empty() && seen_names.insert(key) {
                    merged.push(st);
                }
            }
            if !merged.is_empty() {
                let _ = tx.send(BgMessage::RecommendedStationsLoaded {
                    seq,
                    stations: merged,
                });
            }
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
        self.home_request_seq = self.home_request_seq.wrapping_add(1);
        let seq = self.home_request_seq;
        self.active_mood_idx = mood_index;
        self.qp_page = 0;
        self.shelf_pages = [0; 8];
        self.home_loading_more = false;
        if fresh_seed {
            // Snapshot everything currently visible into the session exclusion rings BEFORE rotating,
            // then advance mood rotation and rotate cached quick_picks/shelves immediately for 0ms feedback.
            self.record_home_shown_items();
            self.home_more_cycle = self.home_more_cycle.wrapping_add(1);
        }
        let session = self.current_ytm_session();
        let taste = self.build_home_taste_profile();
        if fresh_seed {
            if self.rich_home.quick_picks.len() > 8 {
                let shift = 8.min(self.rich_home.quick_picks.len());
                self.rich_home.quick_picks.rotate_left(shift);
            }
            if self.rich_home.shelves.len() > 4 {
                self.rich_home.shelves.rotate_left(2);
            }
        }
        self.ui.button(id!(home_retry_btn)).set_visible(cx, false);
        self.ui
            .label(id!(home_status_lbl))
            .set_text(cx, "Refreshing YouTube Music...");
        self.refresh_home_ui(cx);
        self.ui.redraw(cx);

        thread::spawn(move || {
            match fetch_rich_home(&session, mood_params.as_deref(), fresh_seed, &taste) {
                Ok(home) => {
                    let _ = tx.send(BgMessage::YtmRichHomeLoaded {
                        seq,
                        mood_index,
                        home: Box::new(home),
                    });
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::YtmRichHomeFailed {
                        seq,
                        error: err.to_string(),
                    });
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
        let taste = self.build_home_taste_profile();
        let fallback_mood = if !self.rich_home.chips.is_empty() {
            let idx = self.home_more_cycle % self.rich_home.chips.len();
            self.home_more_cycle = self.home_more_cycle.wrapping_add(1);
            Some(self.rich_home.chips[idx].params.clone())
        } else {
            None
        };
        thread::spawn(move || {
            match fetch_home_continuation(
                &session,
                cont.as_deref(),
                fallback_mood.as_deref(),
                &taste,
            ) {
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

    fn clear_search_state(&mut self, cx: &mut Cx, focus_top_bar: bool) {
        self.pending_live_search = None;
        self.last_dispatched_search.clear();
        self.search_request_seq = self.search_request_seq.wrapping_add(1);
        self.ui.text_input(id!(search_page_input)).set_text(cx, "");
        self.ui.text_input(id!(top_search_input)).set_text(cx, "");
        self.ui
            .button(id!(top_search_clear_btn))
            .set_visible(cx, false);
        self.search_result = YtmRichSearch::default();
        self.search_tracks = Rc::new(Vec::new());
        self.ui
            .label(id!(search_status_lbl))
            .set_text(cx, "Instant YouTube Music + Local Search");
        self.refresh_search_ui(cx);
        if focus_top_bar {
            self.ui.widget(id!(top_search_input)).set_key_focus(cx);
        }
        self.ui.redraw(cx);
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
        self.ui
            .button(id!(top_search_clear_btn))
            .set_visible(cx, !raw.is_empty());

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

        // Instant zero-latency, zero-allocation local + Home feed filtering while live YouTube Music results load
        let mut instant_hits = Vec::new();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for t in self
            .all_tracks
            .iter()
            .chain(self.rich_home.quick_picks.iter())
            .chain(self.rich_home.all_tracks.iter())
        {
            if (contains_ascii_icase(&t.title, &q)
                || contains_ascii_icase(&t.artist, &q)
                || contains_ascii_icase(&t.album, &q))
                && seen.insert(t.id.as_str())
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
        self.ui
            .button(id!(top_search_clear_btn))
            .set_visible(cx, !query.is_empty());
        if self.active_page != MainPage::Search {
            self.navigate_to(cx, MainPage::Search, true);
        }
        self.ui
            .label(id!(search_status_lbl))
            .set_text(cx, &format!("Searching '{query}'..."));

        // Immediate local + cached matches while YouTube Music responds (zero-allocation filter)
        let mut local_hits = Vec::new();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for t in self
            .all_tracks
            .iter()
            .chain(self.rich_home.quick_picks.iter())
            .chain(self.rich_home.all_tracks.iter())
        {
            if (contains_ascii_icase(&t.title, &query)
                || contains_ascii_icase(&t.artist, &query)
                || contains_ascii_icase(&t.album, &query))
                && seen.insert(t.id.as_str())
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
        let q = query.trim();
        let tracks = match filter {
            SearchFilter::Local => self
                .all_tracks
                .iter()
                .filter(|t| {
                    t.source == TrackSource::Local
                        && (q.is_empty()
                            || contains_ascii_icase(&t.title, q)
                            || contains_ascii_icase(&t.artist, q))
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
                            q.is_empty()
                                || contains_ascii_icase(&t.title, q)
                                || contains_ascii_icase(&t.artist, q)
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

    fn start_spotify_web_login(&mut self, cx: &mut Cx) {
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        self.ui.label(id!(spotify_web_status_lbl)).set_text(
            cx,
            "Opening Spotify sign-in window... Sign in normally; no Client ID needed.",
        );
        let profile_dir = app_data_dir().join("spotify_web_profile");
        // Clear cookies if signed out so account switching works cleanly; keep active session otherwise.
        let clear_cookies = self.accounts.spotify_web_cookie.trim().is_empty();
        launch_spotify_login_window(profile_dir, clear_cookies, move |outcome| {
            let msg = match outcome {
                SpotifyLoginOutcome::SignedIn { sp_dc, token_json } => {
                    BgMessage::SpotifyWebLoginResult { sp_dc, token_json }
                }
                SpotifyLoginOutcome::Cancelled => {
                    BgMessage::SpotifyWebLoginFailed("Sign-in window closed".to_string())
                }
                SpotifyLoginOutcome::Failed(err) => BgMessage::SpotifyWebLoginFailed(err),
            };
            let _ = tx.send(msg);
        });
    }

    fn is_spotify_oauth_session_active(&self) -> bool {
        self.accounts.spotify_connected && !self.accounts.spotify_access_token.trim().is_empty()
    }

    fn is_spotify_web_session_active(&self) -> bool {
        self.accounts.spotify_web_connected
            && (!self.accounts.spotify_web_cookie.trim().is_empty()
                || !self.accounts.spotify_web_access_token.trim().is_empty())
    }

    fn refresh_spotify_account_ui(&self, cx: &mut Cx) {
        let pal = self.adaptive_palette();
        let status_col = rgba_to_vec4(pal.brand_accent);

        let web_active = self.is_spotify_web_session_active();
        let web_name = self.accounts.spotify_web_user_name.trim();

        let web_text = if web_active {
            if web_name.is_empty() {
                "Connected".to_string()
            } else {
                format!("Connected · {web_name}")
            }
        } else {
            "Not Connected".to_string()
        };
        self.ui
            .label(id!(spotify_web_state_lbl))
            .set_text(cx, &web_text);
        self.ui
            .label(id!(spotify_web_state_lbl))
            .apply_over(cx, live! { draw_text: { color: (status_col) } });

        self.refresh_top_account_btn(cx);
    }

    fn sync_spotify_library_for(&mut self, cx: &mut Cx, web_session: bool, initial_delay_ms: u64) {
        let status_lbl = id!(spotify_web_status_lbl);
        if self.spotify_sync_in_flight {
            self.ui.label(status_lbl).set_text(
                cx,
                "Spotify sync already in progress... Waiting for current request to finish.",
            );
            return;
        }
        let token = if web_session {
            self.accounts.spotify_web_access_token.trim().to_string()
        } else {
            self.accounts.spotify_access_token.trim().to_string()
        };
        if token.is_empty() {
            self.ui.label(status_lbl).set_text(
                cx,
                if web_session {
                    "Sign in via Web first to sync your Spotify Liked Songs and playlists."
                } else {
                    "Sign in with a Spotify Premium account first to sync your library."
                },
            );
            return;
        }
        let Some(tx) = self.bg_tx.clone() else {
            return;
        };
        let snapshot_prefix = if web_session {
            "spotify:playlist:"
        } else {
            "spotify:premium:playlist:"
        };
        let existing_snapshots: HashMap<String, String> = self
            .playlists
            .iter()
            .filter(|pl| pl.id.starts_with(snapshot_prefix))
            .filter_map(|pl| {
                pl.etag
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|snap| (pl.id.clone(), snap.to_string()))
            })
            .collect();
        self.spotify_sync_in_flight = true;
        self.ui.label(status_lbl).set_text(
            cx,
            if web_session {
                "Syncing your Spotify Web library..."
            } else {
                "Syncing your Spotify Premium library..."
            },
        );
        thread::spawn(move || {
            if initial_delay_ms > 0 {
                thread::sleep(Duration::from_millis(initial_delay_ms));
            }
            let msg =
                match sync_spotify_catalog_with_snapshots(&token, &existing_snapshots, web_session)
                {
                    Ok(report) => BgMessage::SpotifyCatalogSynced {
                        web_session,
                        report: Box::new(report),
                    },
                    Err(err) => BgMessage::SpotifySyncFailed(err.to_string()),
                };
            let _ = tx.send(msg);
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
        let (uri, port) = if let Some(engine) = &self.connect_engine {
            (engine.regenerate_qr_uri(), engine.local_tcp_port())
        } else {
            (String::new(), 43821)
        };
        self.pairing_uri = uri;
        if let Ok(qr) = QrMatrix::encode(&self.pairing_uri) {
            self.ui
                .ferric_qr_view(id!(connect_qr_view))
                .set_matrix(cx, qr.size, qr.modules);
        }
        let pc_name = self
            .connect_snap
            .as_ref()
            .map_or("Desktop-PC", |s| s.local_device_name.as_str());
        self.ui.label(id!(connect_lan_ip_lbl)).set_text(
            cx,
            &format!(
                "Cloud E2EE + Wi-Fi ({}:{port}) · {pc_name} Ready",
                self.lan_ip
            ),
        );
    }

    fn sync_connect_accounts(&self) {
        let Some(engine) = &self.connect_engine else {
            return;
        };
        let sp_id = if self.accounts.spotify_connected || self.accounts.spotify_web_connected {
            self.accounts.spotify_user_id.clone()
        } else {
            String::new()
        };
        let yt_id = if self.accounts.youtube_connected {
            self.accounts.youtube_account_id.clone()
        } else {
            String::new()
        };
        engine.update_accounts(sp_id, yt_id);
    }

    fn is_controlling_remote(&self) -> bool {
        self.connect_snap.as_ref().is_some_and(|s| {
            !s.is_local_active_speaker
                && s.peers
                    .iter()
                    .any(|p| p.connected && p.device_id == s.active_speaker_id)
        })
    }

    fn active_remote_peer(&self) -> Option<&ferric_sync::ConnectPeerInfo> {
        let snap = self.connect_snap.as_ref()?;
        if snap.is_local_active_speaker {
            return None;
        }
        snap.peers
            .iter()
            .find(|p| p.connected && p.device_id == snap.active_speaker_id)
    }

    fn active_remote_track(&self) -> Option<Track> {
        let snap = self.connect_snap.as_ref()?;
        let peer = self.active_remote_peer()?;
        if let Some(q_entry) = snap.active_queue.get(snap.active_queue_index)
            && (peer.active_track_id.is_none()
                || peer.active_track_id.as_deref() == Some(q_entry.track_id.as_str()))
        {
            return Some(q_entry.clone().into_track());
        }
        if peer.now_playing_title.is_empty() && peer.active_track_id.is_none() {
            return None;
        }
        let tid = peer
            .active_track_id
            .clone()
            .unwrap_or_else(|| format!("remote:{}", peer.device_id));
        let yt_vid = tid.strip_prefix("yt:video:").map(ToOwned::to_owned);
        Some(Track {
            id: tid,
            title: peer.now_playing_title.clone(),
            artist: peer.now_playing_artist.clone(),
            album: String::new(),
            duration_ms: peer.duration_ms,
            path: None,
            source: if yt_vid.is_some() {
                TrackSource::YouTubeMusic
            } else {
                TrackSource::Spotify
            },
            codec: if peer.via_relay {
                "CLOUD".to_string()
            } else {
                "WIFI".to_string()
            },
            sample_rate: 48_000,
            bit_depth: 16,
            bitrate_kbps: 128,
            isrc: None,
            spotify_id: None,
            youtube_video_id: yt_vid,
            thumbnail_url: peer.now_playing_thumbnail_url.clone(),
            replay_gain_db: 0,
            liked: false,
        })
    }

    fn effective_display_track(&self) -> Option<Track> {
        if self.is_controlling_remote() {
            self.active_remote_track()
        } else {
            self.queue.current().cloned()
        }
    }

    fn effective_volume(&self) -> f32 {
        self.active_remote_peer()
            .map_or(self.session_state.volume, |p| p.volume_norm)
            .clamp(0.0, 1.0)
    }

    fn effective_shuffle_repeat(&self) -> (bool, RepeatMode) {
        if let Some(peer) = self.active_remote_peer() {
            (peer.shuffle_enabled, peer.repeat_mode)
        } else {
            (self.queue.shuffle, self.queue.repeat)
        }
    }

    fn sync_remote_mirror_ui(&mut self, cx: &mut Cx) {
        let Some(peer) = self.active_remote_peer().cloned() else {
            return;
        };
        let remote_track = self.active_remote_track();
        let track_id = remote_track
            .as_ref()
            .map(|t| t.id.as_str())
            .unwrap_or_default();
        let (q_len, q_idx, q_next) = self
            .connect_snap
            .as_ref()
            .map(|s| {
                let next_id = s
                    .active_queue
                    .get(s.active_queue_index.saturating_add(1))
                    .map(|e| e.track_id.as_str())
                    .unwrap_or_default();
                (s.active_queue.len(), s.active_queue_index, next_id)
            })
            .unwrap_or((0, 0, ""));
        let mirror_key = format!(
            "{}|{}|{}|{}|{}|{:?}|{}|{:.2}|{}|{}|{}|{}",
            peer.device_id,
            track_id,
            peer.now_playing_title,
            peer.now_playing_artist,
            peer.is_playing,
            peer.repeat_mode,
            peer.shuffle_enabled,
            peer.volume_norm,
            peer.duration_ms,
            q_len,
            q_idx,
            q_next,
        );
        let track_key = format!(
            "{}|{}|{}|{:?}",
            track_id,
            peer.now_playing_title,
            peer.now_playing_artist,
            peer.now_playing_thumbnail_url
        );
        let track_changed = self.last_remote_track_key != track_key;
        self.last_remote_track_key = track_key;
        let meta_changed = self.last_remote_mirror_key != mirror_key;
        self.last_remote_mirror_key = mirror_key;
        self.was_controlling_remote = true;

        self.ui
            .ferric_play_btn(id!(play_pause_btn))
            .set_playing(cx, peer.is_playing);
        if self.fullscreen_open {
            self.ui
                .ferric_play_btn(id!(fs_play_pause_btn))
                .set_playing(cx, peer.is_playing);
            let acr = if self.theme.acrylic_enabled {
                1.0_f32
            } else {
                0.0_f32
            };
            self.ui.ferric_ambient_stage(id!(fs_backdrop)).set_state(
                cx,
                true,
                peer.is_playing,
                self.current_accent(),
                acr,
            );
            self.last_fs_backdrop_playing = Some(peer.is_playing);
        }
        self.sync_playback_controls_only(cx);

        if meta_changed {
            self.last_pos_txt.clear();
            self.last_dur_txt.clear();
            self.last_codec_txt.clear();

            let title = if peer.now_playing_title.is_empty() {
                "Connected Device".to_string()
            } else {
                peer.now_playing_title.clone()
            };
            let artist_sub = if peer.now_playing_artist.is_empty() {
                format!("Playing on {}", peer.device_name)
            } else {
                format!(
                    "{} · Playing on {}",
                    peer.now_playing_artist, peer.device_name
                )
            };
            let mode_badge = if peer.via_relay {
                format!("CLOUD · {}", clip_text(&peer.device_name, 14))
            } else {
                format!("WIFI · {}", clip_text(&peer.device_name, 14))
            };

            self.ui
                .label(id!(np_title_lbl))
                .set_text(cx, &clip_text(&clean_display_text(&title), 20));
            self.ui
                .label(id!(np_artist_lbl))
                .set_text(cx, &clip_text(&clean_display_text(&artist_sub), 26));
            self.ui
                .label(id!(fs_title_lbl))
                .set_text(cx, &clip_text(&clean_display_text(&title), 36));
            self.ui
                .label(id!(fs_artist_lbl))
                .set_text(cx, &clip_text(&clean_display_text(&artist_sub), 44));
            self.ui
                .button(id!(codec_pill_btn))
                .set_text(cx, &mode_badge);
            self.ui.label(id!(signal_path_lbl)).set_text(
                cx,
                &format!(
                    "Ferric Connect ({}) -> {} (E2EE ChaCha20-Poly1305)",
                    if peer.via_relay { "Cloud" } else { "Wi-Fi" },
                    peer.device_name
                ),
            );
            win32_update_tray_tooltip(&format!("Ferric · On {} - {title}", peer.device_name));
            win32_update_taskbar_playback(&title, &peer.now_playing_artist, peer.is_playing);

            if track_changed && let Some(ref tr) = remote_track {
                if let Some(cache) = &mut self.artwork {
                    cache.pin_active_track(Some(tr));
                }
                self.bind_hero_cover_art_url(
                    cx,
                    self.ui.ferric_cover_art(id!(np_art)),
                    tr.thumbnail_url.as_deref(),
                    Some(tr),
                    peer.is_playing,
                    0.55,
                    false,
                );
                self.bind_hero_cover_art_url(
                    cx,
                    self.ui.ferric_cover_art(id!(fs_art)),
                    tr.thumbnail_url.as_deref(),
                    Some(tr),
                    false,
                    0.55,
                    false,
                );
                self.sync_fullscreen_backdrop_artwork(cx);
                self.apply_adaptive_accent_live(cx);
                self.load_lyrics_for_track(cx, tr);
                if self.video_open && self.video_track_id.as_deref() != Some(tr.id.as_str()) {
                    self.open_native_video_stage(cx);
                }
            } else if !track_changed && remote_track.is_some() {
                let pal = self.adaptive_palette();
                let accent = rgba_to_vec4(pal.accent);
                self.ui.ferric_cover_art(id!(np_art)).set_state(
                    cx,
                    peer.is_playing,
                    0.55,
                    accent,
                    false,
                );
            }
            self.refresh_right_sidebar(cx);
        }
    }

    fn update_remote_timeline_tick(&mut self, cx: &mut Cx, window_hidden: bool) -> bool {
        let Some(peer) = self.active_remote_peer().cloned() else {
            return false;
        };
        let dur_ms = peer.duration_ms;
        let pos_ms = peer.interpolated_position_ms();

        self.ui
            .ferric_play_btn(id!(play_pause_btn))
            .set_playing(cx, peer.is_playing);
        if self.fullscreen_open {
            self.ui
                .ferric_play_btn(id!(fs_play_pause_btn))
                .set_playing(cx, peer.is_playing);
            if self.last_fs_backdrop_playing != Some(peer.is_playing) {
                self.last_fs_backdrop_playing = Some(peer.is_playing);
                let acr = if self.theme.acrylic_enabled {
                    1.0_f32
                } else {
                    0.0_f32
                };
                self.ui.ferric_ambient_stage(id!(fs_backdrop)).set_state(
                    cx,
                    true,
                    peer.is_playing,
                    self.current_accent(),
                    acr,
                );
            }
        }

        if !window_hidden && dur_ms > 0 {
            let ratio = (pos_ms as f64 / dur_ms as f64).clamp(0.0, 1.0);
            let pos_txt = format_compact_time_ms(pos_ms);
            let dur_txt = if self.show_remaining_time {
                format!("-{}", format_compact_time_ms(dur_ms.saturating_sub(pos_ms)))
            } else {
                format_compact_time_ms(dur_ms)
            };
            self.ui
                .ferric_bar_slider(id!(seek_slider))
                .set_value(cx, ratio);
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
            self.update_lyrics_display(cx, pos_ms);
        }

        if self.video_open
            && self
                .last_video_sync
                .is_none_or(|t| t.elapsed() >= Duration::from_millis(350))
        {
            self.last_video_sync = Some(Instant::now());
            if let Some(vp) = &self.video_player
                && vp.is_open()
            {
                vp.sync_playback(pos_ms as f64 / 1000.0, peer.is_playing);
            }
            self.sync_video_bounds(cx);
        }
        if let Some(smtc) = &mut self.smtc {
            smtc.sync_playback(peer.is_playing, pos_ms, dur_ms);
        }

        let desired = if window_hidden {
            if peer.is_playing { 500 } else { 1000 }
        } else if peer.is_playing {
            220
        } else {
            350
        };
        self.sync_tick_timer_rate(cx, desired);
        true
    }

    fn push_connect_playback_snapshot(&self) {
        let Some(engine) = &self.connect_engine else {
            return;
        };
        let tele = self
            .audio
            .as_ref()
            .map(|a| a.telemetry())
            .unwrap_or_default();
        let current_track = self.queue.current().map(QueueEntry::from_track);
        let queue: Vec<QueueEntry> = self
            .queue
            .tracks()
            .iter()
            .take(200)
            .map(QueueEntry::from_track)
            .collect();
        let current_index = current_track
            .as_ref()
            .and_then(|cur| queue.iter().position(|t| t.track_id == cur.track_id))
            .unwrap_or(0);
        let now_utc_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        engine.update_local_playback(LocalPlaybackSnapshot {
            is_playing: tele.is_playing,
            current_track,
            queue,
            current_index,
            position_ms: tele.position_ms,
            volume_norm: self.session_state.volume,
            repeat_mode: self.queue.repeat,
            shuffle_enabled: self.queue.shuffle,
            anchor_utc_ms: now_utc_ms,
        });
    }

    fn refresh_connect_panel(&mut self, cx: &mut Cx) {
        let Some(snap) = self.connect_snap.clone() else {
            return;
        };
        let active_peer = snap
            .peers
            .iter()
            .find(|p| p.connected && p.device_id == snap.active_speaker_id);

        let (active_lbl, now_lbl, slider_vol) =
            if snap.is_local_active_speaker || active_peer.is_none() {
                let now = self
                    .queue
                    .current()
                    .map(|t| format!("{} - {}", t.title, t.artist))
                    .unwrap_or_else(|| "Ready to play on this PC".to_string());
                (
                    format!("{} (Active)", snap.local_device_name),
                    now,
                    f64::from(self.session_state.volume.clamp(0.0, 1.0)),
                )
            } else if let Some(peer) = active_peer {
                let now = if !peer.now_playing_title.is_empty() {
                    format!("{} - {}", peer.now_playing_title, peer.now_playing_artist)
                } else if peer.via_relay {
                    "Linked via Cloud E2EE · Anywhere".to_string()
                } else {
                    "Connected over encrypted Wi-Fi".to_string()
                };
                (
                    format!("{} (Playing)", peer.device_name),
                    now,
                    f64::from(peer.volume_norm.clamp(0.0, 1.0)),
                )
            } else {
                (
                    snap.local_device_name.clone(),
                    "Ready".to_string(),
                    f64::from(self.session_state.volume.clamp(0.0, 1.0)),
                )
            };

        self.ui
            .label(id!(rs_conn_active_lbl))
            .set_text(cx, &active_lbl);
        self.ui
            .label(id!(rs_conn_now_lbl))
            .set_text(cx, &clip_text(&now_lbl, 34));
        self.ui
            .ferric_bar_slider(id!(rs_conn_vol_slider))
            .set_value(cx, slider_vol);
        self.ui.button(id!(rs_conn_local_btn)).set_text(
            cx,
            &if snap.is_local_active_speaker {
                format!("This PC · {} (Active)", snap.local_device_name)
            } else {
                format!("Switch to This PC ({})", snap.local_device_name)
            },
        );

        let peer_btn_ids = [
            id!(rs_conn_peer_0),
            id!(rs_conn_peer_1),
            id!(rs_conn_peer_2),
            id!(rs_conn_peer_3),
        ];
        self.connect_peer_ids = Default::default();
        for (idx, btn_id) in peer_btn_ids.iter().enumerate() {
            if let Some(peer) = snap.peers.get(idx) {
                self.connect_peer_ids[idx] = peer.device_id.clone();
                let tag = if peer.device_id == snap.active_speaker_id {
                    "Active"
                } else if peer.connected {
                    "Connected"
                } else {
                    "Found"
                };
                self.ui
                    .button(*btn_id)
                    .set_text(cx, &format!("{} ({tag})", clip_text(&peer.device_name, 18)));
                self.ui.button(*btn_id).set_visible(cx, true);
            } else {
                self.ui.button(*btn_id).set_visible(cx, false);
            }
        }

        let status = if snap.is_account_ready {
            if snap.peers.is_empty() {
                if snap.relay_connected {
                    "Cloud E2EE online · Open Ferric Android on the same account (any Wi-Fi/cellular)..."
                        .to_string()
                } else {
                    format!(
                        "Connecting Cloud E2EE & Wi-Fi ({}:{}) · Waiting for Ferric Android...",
                        snap.local_ip, snap.local_tcp_port
                    )
                }
            } else {
                let any_relay = snap.peers.iter().any(|p| p.connected && p.via_relay);
                let any_lan = snap.peers.iter().any(|p| p.connected && !p.via_relay);
                let transport_lbl = match (any_lan, any_relay) {
                    (true, true) => "Cloud E2EE & Wi-Fi Direct",
                    (true, false) => "Encrypted Wi-Fi Direct",
                    (false, true) => "Encrypted Cloud Relay (Anywhere)",
                    (false, false) => "Auto-Connect",
                };
                format!(
                    "Auto-Connect linked ({} device(s)) · {transport_lbl}",
                    snap.peers.len()
                )
            }
        } else {
            "Sign in to Spotify or YouTube Music on Desktop & Android for zero-pairing Connect anywhere."
                .to_string()
        };
        self.ui.label(id!(rs_conn_status_lbl)).set_text(cx, &status);

        let settings_peers = if snap.peers.is_empty() {
            status
        } else {
            snap.peers
                .iter()
                .map(|p| {
                    let st = if p.connected { "Connected" } else { "Found" };
                    format!("- {} ({st} · {})", p.device_name, p.addr)
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        self.ui
            .label(id!(connect_peers_lbl))
            .set_text(cx, &settings_peers);

        let pal = self.adaptive_palette();
        let accent = rgba_to_vec4(pal.accent);
        let muted = rgba_to_vec4(pal.icon_idle);
        let txt_primary = rgba_to_vec4(pal.text_primary);
        let txt_secondary = rgba_to_vec4(pal.text_secondary);
        let (card_col, btn_col, card_border, acrylic_tint) = self.theme_surface_tokens();
        let acr = if self.theme.acrylic_enabled {
            1.0_f32
        } else {
            0.0_f32
        };
        let lm = if pal.is_light { 1.0_f32 } else { 0.0_f32 };
        self.ui.view(id!(card_rs_conn_active)).apply_over(
            cx,
            live! {
                draw_bg: {
                    acrylic_on: (acr),
                    accent_col: (accent),
                    card_col: (card_col),
                    border_col: (card_border),
                    tint_col: (acrylic_tint),
                    light_mode: (lm)
                }
            },
        );
        self.ui
            .label(id!(rs_conn_active_lbl))
            .apply_over(cx, live! { draw_text: { color: (accent) } });
        self.ui
            .label(id!(rs_conn_now_lbl))
            .apply_over(cx, live! { draw_text: { color: (txt_primary) } });
        self.ui
            .label(id!(lbl_m_rs_vol))
            .apply_over(cx, live! { draw_text: { color: (txt_secondary) } });
        self.ui
            .label(id!(lbl_m_rs_switch))
            .apply_over(cx, live! { draw_text: { color: (txt_secondary) } });
        self.ui
            .label(id!(rs_conn_status_lbl))
            .apply_over(cx, live! { draw_text: { color: (txt_secondary) } });
        self.ui
            .ferric_bar_slider(id!(rs_conn_vol_slider))
            .apply_over(cx, live! { draw_bg: { fill_col: (accent) } });
        let local_act = if snap.is_local_active_speaker {
            1.0_f32
        } else {
            0.0_f32
        };
        let local_txt = if snap.is_local_active_speaker {
            accent
        } else {
            txt_primary
        };
        self.ui.button(id!(rs_conn_local_btn)).apply_over(
            cx,
            live! {
                draw_bg: {
                    active: (local_act),
                    acrylic_on: (acr),
                    accent_col: (accent),
                    surface_col: (btn_col),
                    border_col: (card_border),
                    tint_col: (acrylic_tint),
                    light_mode: (lm)
                }
                draw_text: { color: (local_txt) }
            },
        );
        for (idx, btn_id) in [
            id!(rs_conn_peer_0),
            id!(rs_conn_peer_1),
            id!(rs_conn_peer_2),
            id!(rs_conn_peer_3),
        ]
        .iter()
        .enumerate()
        {
            let is_act = snap
                .peers
                .get(idx)
                .is_some_and(|p| p.device_id == snap.active_speaker_id);
            let p_txt = if is_act { accent } else { txt_primary };
            self.ui.button(*btn_id).apply_over(
                cx,
                live! {
                    draw_bg: {
                        active: (if is_act { 1.0 } else { 0.0 }),
                        acrylic_on: (acr),
                        accent_col: (accent),
                        surface_col: (btn_col),
                        border_col: (card_border),
                        tint_col: (acrylic_tint),
                        light_mode: (lm)
                    }
                    draw_text: { color: (p_txt) }
                },
            );
        }
        self.ui.button(id!(rs_conn_qr_btn)).apply_over(
            cx,
            live! {
                draw_bg: {
                    acrylic_on: (acr),
                    accent_col: (accent),
                    surface_col: (btn_col),
                    border_col: (card_border),
                    tint_col: (acrylic_tint),
                    light_mode: (lm)
                }
                draw_text: { color: (txt_primary) }
            },
        );
        let has_peer = snap.peers.iter().any(|p| p.connected);
        self.ui.button(id!(bar_connect_btn)).apply_over(
            cx,
            live! { draw_icon: { color: (if (self.right_sidebar_open && self.right_connect_tab) || has_peer { accent } else { muted }) } },
        );
    }

    fn handle_connect_event(&mut self, cx: &mut Cx, ev: ConnectEvent) {
        match ev {
            ConnectEvent::StateUpdated(snap) => {
                self.connect_snap = Some(snap);
                self.refresh_connect_panel(cx);
                if self.is_controlling_remote() {
                    self.sync_remote_mirror_ui(cx);
                } else if self.was_controlling_remote {
                    self.was_controlling_remote = false;
                    self.last_remote_mirror_key.clear();
                    self.last_remote_track_key.clear();
                    self.last_pos_txt.clear();
                    self.last_dur_txt.clear();
                    self.last_codec_txt.clear();
                    if let Some(cur) = self.queue.current().cloned() {
                        let is_playing = self
                            .audio
                            .as_ref()
                            .is_some_and(|a| a.telemetry().is_playing);
                        self.sync_now_playing_ui_widgets(cx, &cur, is_playing);
                    } else {
                        self.ui
                            .label(id!(np_title_lbl))
                            .set_text(cx, "Nothing playing");
                        self.ui
                            .label(id!(np_artist_lbl))
                            .set_text(cx, "Pick any song to start");
                        self.ui
                            .label(id!(fs_title_lbl))
                            .set_text(cx, "Nothing playing");
                        self.ui
                            .label(id!(fs_artist_lbl))
                            .set_text(cx, "Pick any song to start");
                        self.ui.button(id!(codec_pill_btn)).set_text(cx, "READY");
                        self.ui
                            .ferric_play_btn(id!(play_pause_btn))
                            .set_playing(cx, false);
                        self.ui
                            .ferric_play_btn(id!(fs_play_pause_btn))
                            .set_playing(cx, false);
                        self.ui
                            .ferric_cover_art(id!(np_art))
                            .set_texture(cx, None, 1.0);
                        self.ui
                            .ferric_cover_art(id!(fs_art))
                            .set_texture(cx, None, 1.0);
                    }
                    self.sync_controls_from_state(cx);
                    self.refresh_right_sidebar(cx);
                }
                self.ui.redraw(cx);
            }
            ConnectEvent::ExecuteRemoteAction(action) => {
                self.apply_incoming_connect_action(cx, action);
            }
            ConnectEvent::BecameActiveSpeaker {
                state,
                queue,
                current_index,
            } => {
                self.was_controlling_remote = false;
                self.last_remote_mirror_key.clear();
                self.last_remote_track_key.clear();
                let mut tracks = self.hydrate_connect_tracks(queue);
                if tracks.is_empty()
                    && let Some(tid) = state.track_id.clone()
                {
                    tracks.push(
                        QueueEntry {
                            track_id: tid,
                            title: state.track_title.clone(),
                            artist: state.track_artist.clone(),
                            album: String::new(),
                            duration_ms: state.duration_ms,
                            spotify_id: None,
                            youtube_video_id: None,
                            thumbnail_url: state.track_thumbnail_url.clone(),
                        }
                        .into_track(),
                    );
                }
                if !tracks.is_empty() {
                    let idx = current_index.min(tracks.len() - 1);
                    let chosen = tracks[idx].clone();
                    self.queue.repeat = state.repeat_mode;
                    self.queue.set_shuffle(state.shuffle_enabled);
                    self.session_state.repeat = state.repeat_mode;
                    self.session_state.shuffle = state.shuffle_enabled;
                    self.queue.set_tracks(tracks, Some(idx));
                    let now_utc = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    let drift = if state.is_playing && state.anchor_utc_ms > 0 {
                        u64::try_from((now_utc - state.anchor_utc_ms).clamp(0, 5_000)).unwrap_or(0)
                    } else {
                        0
                    };
                    let seek_ms = state.base_position_ms.saturating_add(drift);
                    let has_local_file = chosen.path.as_ref().is_some_and(|p| p.is_file());
                    self.pending_handoff_seek_ms = Some(seek_ms);
                    self.pending_handoff_paused = !state.is_playing;
                    self.play_track_inner(cx, chosen, true);
                    if has_local_file {
                        self.pending_handoff_seek_ms = None;
                        self.pending_handoff_paused = false;
                        if seek_ms > 1_000
                            && let Some(audio) = &self.audio
                        {
                            audio.send(EngineCommand::SeekMs(seek_ms));
                        }
                        if !state.is_playing
                            && let Some(audio) = &self.audio
                        {
                            audio.send(EngineCommand::Pause);
                        }
                    }
                    self.sync_controls_from_state(cx);
                    self.refresh_right_sidebar(cx);
                    self.show_toast(cx, "Switched playback to Desktop PC");
                }
                self.refresh_connect_panel(cx);
                self.push_connect_playback_snapshot();
            }
            ConnectEvent::YieldedActiveSpeaker {
                new_speaker_name, ..
            } => {
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::Pause);
                }
                self.show_toast(cx, &format!("Playing on {new_speaker_name}"));
                self.refresh_connect_panel(cx);
                self.sync_remote_mirror_ui(cx);
            }
            ConnectEvent::Toast(msg) => {
                self.show_toast(cx, &msg);
            }
        }
    }

    fn apply_incoming_connect_action(&mut self, cx: &mut Cx, action: RemoteAction) {
        match action {
            RemoteAction::Play => {
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::Play);
                }
            }
            RemoteAction::Pause => {
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::Pause);
                }
            }
            RemoteAction::TogglePlayPause => {
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::TogglePlayPause);
                }
            }
            RemoteAction::SeekTo { position_ms } => {
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::SeekMs(position_ms));
                }
            }
            RemoteAction::SkipNext => {
                self.pending_handoff_seek_ms = None;
                self.pending_handoff_paused = false;
                if let Some(next) = self.queue.next_track_manual().cloned() {
                    self.play_track_inner(cx, next, true);
                }
            }
            RemoteAction::SkipPrev => {
                self.pending_handoff_seek_ms = None;
                self.pending_handoff_paused = false;
                if let Some(prev) = self.queue.prev_track().cloned() {
                    self.play_track_inner(cx, prev, true);
                }
            }
            RemoteAction::JumpToQueueIndex { index } => {
                self.pending_handoff_seek_ms = None;
                self.pending_handoff_paused = false;
                let tracks = self.queue.tracks().to_vec();
                if !tracks.is_empty() {
                    let idx = index.min(tracks.len() - 1);
                    let chosen = tracks[idx].clone();
                    self.queue.set_tracks(tracks, Some(idx));
                    self.play_track_inner(cx, chosen, true);
                }
            }
            RemoteAction::PlayQueue {
                tracks,
                start_index,
                ..
            } => {
                self.pending_handoff_seek_ms = None;
                self.pending_handoff_paused = false;
                let hydrated = self.hydrate_connect_tracks(tracks);
                if !hydrated.is_empty() {
                    let idx = start_index.min(hydrated.len() - 1);
                    let chosen = hydrated[idx].clone();
                    self.queue.set_tracks(hydrated, Some(idx));
                    self.play_track_inner(cx, chosen, true);
                }
            }
            RemoteAction::SetVolume { volume_norm, .. } => {
                let vol = volume_norm.clamp(0.0, 1.0);
                self.session_state.volume = vol;
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::SetVolume(vol));
                }
                self.sync_playback_controls_only(cx);
                self.mark_session_dirty();
            }
            RemoteAction::SetRepeat(mode) => {
                self.queue.repeat = mode;
                self.session_state.repeat = mode;
                self.mark_session_dirty();
                self.sync_next_deck_to_engine();
                self.sync_controls_from_state(cx);
                self.refresh_right_sidebar(cx);
            }
            RemoteAction::SetShuffle(enabled) => {
                self.queue.set_shuffle(enabled);
                self.session_state.shuffle = enabled;
                self.mark_session_dirty();
                self.sync_next_deck_to_engine();
                self.sync_controls_from_state(cx);
                self.refresh_right_sidebar(cx);
            }
            RemoteAction::ToggleLike { track_id } => {
                if let Some(target) = self
                    .all_tracks
                    .iter()
                    .find(|t| t.id == track_id)
                    .or_else(|| self.queue.tracks().iter().find(|t| t.id == track_id))
                    .cloned()
                {
                    let cur_liked = target.liked;
                    let next_liked = !cur_liked;
                    let mut saved = target;
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
                    self.sync_controls_from_state(cx);
                    self.rebuild_library_views(cx);
                }
            }
            RemoteAction::PlayNextTrack(entry) => {
                if let Some(track) = self.hydrate_connect_tracks(vec![entry]).into_iter().next() {
                    self.queue.insert_play_next(track);
                    self.sync_next_deck_to_engine();
                    self.prefetch_next_online_track();
                    self.refresh_right_sidebar(cx);
                }
            }
            RemoteAction::AppendQueueTrack(entry) => {
                if let Some(track) = self.hydrate_connect_tracks(vec![entry]).into_iter().next() {
                    self.queue.append_single_to_end(track);
                    self.sync_next_deck_to_engine();
                    self.refresh_right_sidebar(cx);
                }
            }
            RemoteAction::RemoveQueueAt(offset) => {
                if self.queue.remove_upcoming(offset).is_some() {
                    self.sync_next_deck_to_engine();
                    self.refresh_right_sidebar(cx);
                }
            }
            RemoteAction::ClearUpNext => {
                if let Some(cur) = self.queue.current().cloned() {
                    self.queue.set_tracks(vec![cur], Some(0));
                } else {
                    self.queue.set_tracks(Vec::new(), None);
                }
                self.sync_next_deck_to_engine();
                self.refresh_right_sidebar(cx);
            }
            RemoteAction::TransferActiveSpeaker { .. } | RemoteAction::SetSimulcastMode { .. } => {}
        }
        self.push_connect_playback_snapshot();
    }

    fn hydrate_connect_tracks(&self, entries: Vec<QueueEntry>) -> Vec<Track> {
        let local_map: HashMap<&str, &Track> =
            self.all_tracks.iter().map(|t| (t.id.as_str(), t)).collect();
        entries
            .into_iter()
            .map(|e| {
                if let Some(existing) = local_map.get(e.track_id.as_str()) {
                    (*existing).clone()
                } else {
                    e.into_track()
                }
            })
            .collect()
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
        self.update_download_gen = self.update_download_gen.wrapping_add(1);
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
        self.update_download_gen = self.update_download_gen.wrapping_add(1);
        let gen_id = self.update_download_gen;
        self.update_download_in_flight = true;
        let cancel = Arc::new(AtomicBool::new(false));
        self.update_cancel = Some(Arc::clone(&cancel));
        self.ui
            .button(id!(update_now_btn))
            .set_text(cx, "Downloading...");

        let dest_dir = app_data_dir().join("updates");
        thread::spawn(move || {
            let progress_tx = tx.clone();
            let res = download_verified_installer(
                &info.tag,
                APP_VERSION,
                &dest_dir,
                &cancel,
                move |pct| {
                    let _ = progress_tx.send(BgMessage::UpdateDownload {
                        gen_id,
                        event: DownloadEvent::Progress(pct),
                    });
                },
            );
            match res {
                Ok((path, sha256)) => {
                    let _ = tx.send(BgMessage::UpdateDownload {
                        gen_id,
                        event: DownloadEvent::Ready { path, sha256 },
                    });
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::UpdateDownload {
                        gen_id,
                        event: DownloadEvent::Failed(err),
                    });
                }
            }
        });
    }

    fn handle_tick(&mut self, cx: &mut Cx) {
        // 0a. Guarantee Win32 subclass hook is attached to the real MakepadWindow HWND
        // (no-op single atomic load once installed).
        win32_install_window_hooks();

        // 0b. Multi-stage & idle working-set / DXGI trim (releases DX11 shader/staging pools,
        // SQLite page cache, and artwork decode scratch so Ferric stays lean open and in tray).
        if let Some(due) = self.tray_second_trim_at
            && Instant::now() >= due
        {
            self.tray_second_trim_at = None;
            cx.textures.free_unused_textures();
            if let Some(db) = &self.db {
                db.shrink_memory();
            }
            Self::trim_gpu_memory_now(cx);
            if self.window_hidden_or_minimized || win32_is_window_hidden_or_minimized() {
                win32_trim_working_set();
            }
        }
        if let Some(due) = self.post_load_trim_at
            && Instant::now() >= due
        {
            let art_busy = self
                .artwork
                .as_ref()
                .is_some_and(ArtworkCache::has_pending_work);
            if !art_busy {
                self.post_load_trim_at = None;
                cx.textures.free_unused_textures();
                if let Some(db) = &self.db {
                    db.shrink_memory();
                }
                Self::trim_gpu_memory_now(cx);
                win32_trim_working_set();
            }
        }
        if let Some(started) = self.started_at {
            let elapsed = started.elapsed();
            if self.startup_trim_stage == 0 && elapsed >= Duration::from_millis(3_400) {
                let art_busy = self
                    .artwork
                    .as_ref()
                    .is_some_and(ArtworkCache::has_pending_work);
                if !art_busy || elapsed >= Duration::from_millis(6_500) {
                    self.startup_trim_stage = 1;
                    self.post_load_trim_at = None;
                    cx.textures.free_unused_textures();
                    if let Some(db) = &self.db {
                        db.shrink_memory();
                    }
                    Self::trim_gpu_memory_now(cx);
                    win32_trim_working_set();
                }
            }
        }
        self.tick_counter = self.tick_counter.wrapping_add(1);
        if self
            .session_save_dirty_at
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.flush_session_state_now();
        }

        // 0b. Poll System Tray & Win32 subclass events
        for tray_ev in win32_poll_tray_events() {
            match tray_ev {
                TrayEvent::Minimized => {
                    // Taskbar minimize: pause hidden 60 FPS animations without dropping VRAM cover cache
                    // or paging out DirectComposition so unminimizing restores Acrylic blur instantaneously.
                    self.flush_session_state_now();
                    self.window_hidden_or_minimized = true;
                    if self.video_open {
                        self.close_native_video_stage_inner(cx, false);
                    }
                    if !self.fullscreen_open && self.fs_reveal_t > 0.004 {
                        self.finish_fullscreen_close(cx);
                    }
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
                    if let Some(db) = &self.db {
                        db.shrink_memory();
                    }
                    Self::trim_gpu_memory_now(cx);
                }
                TrayEvent::HiddenToTray => {
                    self.flush_session_state_now();
                    self.window_hidden_or_minimized = true;
                    self.ui_dirty_while_hidden = true;
                    if self.video_open {
                        self.close_native_video_stage_inner(cx, false);
                    }
                    if !self.fullscreen_open && self.fs_reveal_t > 0.004 {
                        self.finish_fullscreen_close(cx);
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
                    // Unbind widget Texture handles + drop artwork_snapshot BEFORE clearing ArtworkCache
                    // so `free_unused_textures()` immediately releases every single GPU cover texture.
                    self.unbind_all_cover_textures_for_tray(cx);
                    self.artwork_snapshot = Rc::new(HashMap::new());
                    self.artwork_snapshot_dirty = true;
                    if let Some(cache) = &mut self.artwork {
                        cache.clear_all_for_tray(cx);
                        self.artwork_cleared_for_tray = true;
                    }
                    self.missing_art_queue.borrow_mut().clear();
                    self.missing_art_queue.borrow_mut().shrink_to_fit();
                    Self::clear_makepad_font_caches(cx);
                    if let Some(db) = &self.db {
                        db.shrink_memory_aggressive();
                    }
                    cx.shrink_swapchain_for_tray();
                    Self::trim_gpu_memory_now(cx);
                    win32_trim_working_set();
                    self.sync_tick_timer_rate(cx, 500);
                    // Schedule a second pass after DX11 finishes retiring frame buffers
                    self.tray_second_trim_at = Some(Instant::now() + Duration::from_millis(900));
                }
                TrayEvent::ShowWindow => {
                    self.window_hidden_or_minimized = false;
                    self.tray_second_trim_at = None;
                    self.sync_tick_timer_rate(cx, 100);
                    let (is_playing, pos_ms) = self.audio.as_ref().map_or((false, 0), |a| {
                        let t = a.telemetry();
                        (t.is_playing, t.position_ms)
                    });
                    if let Some(cache) = &mut self.artwork {
                        cache.resume_from_tray();
                    }
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
                    let was_dirty = self.ui_dirty_while_hidden;
                    self.ui_dirty_while_hidden = false;
                    let was_cleared = self.artwork_cleared_for_tray;
                    self.artwork_cleared_for_tray = false;
                    if was_dirty {
                        if !self.last_discord_status.is_empty() {
                            self.ui
                                .label(id!(discord_status_lbl))
                                .set_text(cx, &self.last_discord_status);
                        }
                        if !self.rich_home.quick_picks.is_empty()
                            || !self.rich_home.shelves.is_empty()
                        {
                            self.ui.label(id!(home_status_lbl)).set_text(
                                cx,
                                &format!(
                                    "{} songs · {} shelves",
                                    self.rich_home.quick_picks.len(),
                                    self.rich_home.shelves.len()
                                ),
                            );
                        }
                        if let Some(cur) = self.queue.current().cloned() {
                            self.sync_now_playing_ui_widgets(cx, &cur, is_playing);
                        } else {
                            self.rebuild_library_views(cx);
                            self.refresh_home_ui(cx);
                            self.refresh_right_sidebar(cx);
                        }
                    }
                    if was_cleared {
                        if let Some(cur) = self.queue.current().cloned()
                            && let Some(cache) = &mut self.artwork
                        {
                            cache.pin_active_track(Some(&cur));
                        }
                        self.refresh_all_artwork_slots(cx);
                    }
                    self.update_lyrics_display(cx, pos_ms);
                    self.sync_video_bounds(cx);
                    cx.redraw_all();
                }
                TrayEvent::TogglePlayPause => {
                    if self.is_controlling_remote() {
                        if let Some(engine) = &self.connect_engine {
                            engine.send_remote_action(RemoteAction::TogglePlayPause);
                        }
                    } else if let Some(a) = &self.audio {
                        a.send(EngineCommand::TogglePlayPause);
                    }
                }
                TrayEvent::NextTrack => {
                    if self.is_controlling_remote() {
                        if let Some(engine) = &self.connect_engine {
                            engine.send_remote_action(RemoteAction::SkipNext);
                        }
                    } else if let Some(next) = self.queue.next_track_manual().cloned() {
                        self.play_track(cx, next);
                    }
                }
                TrayEvent::PrevTrack => {
                    if self.is_controlling_remote() {
                        if let Some(engine) = &self.connect_engine {
                            engine.send_remote_action(RemoteAction::SkipPrev);
                        }
                    } else if let Some(prev) = self.queue.prev_track().cloned() {
                        self.play_track(cx, prev);
                    }
                }
                TrayEvent::QuitApp => {
                    self.flush_session_state_now();
                    if let Some(engine) = self.connect_engine.take() {
                        engine.shutdown();
                    }
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
        if !window_hidden && let Some(cache) = &mut self.artwork {
            let (changed, pinned_changed) = cache.poll(cx);
            let pending = cache.has_pending_work();
            if changed {
                self.refresh_all_artwork_slots_inner(cx, pinned_changed);
                if pinned_changed
                    && let Some(cur) = self.queue.current()
                    && let Some(smtc) = &mut self.smtc
                {
                    smtc.refresh_cover_if_cached(cur);
                }
            }
            if pending {
                self.artwork_was_pending = true;
            } else if self.artwork_was_pending {
                self.artwork_was_pending = false;
                self.post_load_trim_at = Some(Instant::now() + Duration::from_millis(1_600));
            }
        }

        // Periodic non-disruptive DXGI + SQLite trim while open (prevents multi-minute Intel iGPU
        // driver pool drift during continuous playback without touching Acrylic or paging active UI).
        if !window_hidden
            && !self.artwork_was_pending
            && self
                .last_open_gpu_trim_at
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(45))
        {
            self.last_open_gpu_trim_at = Some(Instant::now());
            cx.textures.free_unused_textures();
            if let Some(db) = &self.db {
                db.shrink_memory();
            }
            Self::trim_gpu_memory_now(cx);
        }

        // 0d. Smooth exponential accent + adaptive Acrylic glass tint lerp when switching tracks
        let mut color_lerping = false;
        if !window_hidden {
            let mut color_moved = false;
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
                color_moved = true;
            }
            let target_tint = self.target_acrylic_tint();
            if self.displayed_acrylic_tint.w < 0.5 {
                self.displayed_acrylic_tint = target_tint;
                color_moved = true;
            } else {
                let diff_tint = (target_tint.x - self.displayed_acrylic_tint.x).abs()
                    + (target_tint.y - self.displayed_acrylic_tint.y).abs()
                    + (target_tint.z - self.displayed_acrylic_tint.z).abs();
                if diff_tint > 0.004 {
                    let kt = 0.085_f32;
                    self.displayed_acrylic_tint = vec4(
                        self.displayed_acrylic_tint.x
                            + (target_tint.x - self.displayed_acrylic_tint.x) * kt,
                        self.displayed_acrylic_tint.y
                            + (target_tint.y - self.displayed_acrylic_tint.y) * kt,
                        self.displayed_acrylic_tint.z
                            + (target_tint.z - self.displayed_acrylic_tint.z) * kt,
                        1.0,
                    );
                    color_moved = true;
                }
            }
            if color_moved {
                color_lerping = true;
                self.apply_adaptive_accent_lerp_step(cx);
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
                    if self
                        .effective_display_track()
                        .as_ref()
                        .map(|t| t.id.as_str())
                        == Some(track_id.as_str())
                    {
                        if window_hidden {
                            self.synced_lyrics = parsed;
                            self.ui_dirty_while_hidden = true;
                        } else if parsed.lines.is_empty() {
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
                            let pos_ms = self.active_remote_peer().map_or_else(
                                || self.audio.as_ref().map_or(0, |a| a.telemetry().position_ms),
                                |p| p.interpolated_position_ms(),
                            );
                            self.update_lyrics_display(cx, pos_ms);
                        }
                    }
                }
                BgMessage::YtmRichHomeLoaded {
                    seq,
                    mood_index,
                    home,
                } => {
                    if seq != self.home_request_seq {
                        continue;
                    }
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
                    self.record_home_shown_items();

                    if self.queue.is_empty() && !self.rich_home.quick_picks.is_empty() {
                        self.queue
                            .set_tracks(self.rich_home.quick_picks.clone(), None);
                    }

                    if window_hidden {
                        self.ui_dirty_while_hidden = true;
                    } else {
                        self.ui
                            .label(id!(home_status_lbl))
                            .set_text(cx, &format!("{pick_n} songs · {shelf_n} shelves"));
                        self.rebuild_library_views(cx);
                        self.refresh_home_ui(cx);
                        self.refresh_right_sidebar(cx);
                        self.ui.redraw(cx);
                        self.post_load_trim_at =
                            Some(Instant::now() + Duration::from_millis(1_800));
                    }
                    if mood_index == 0 && self.recommended_stations.len() < 5 {
                        self.spawn_recommended_stations(cx, false);
                    }

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
                BgMessage::YtmRichHomeFailed { seq, error: err } => {
                    if seq != self.home_request_seq {
                        continue;
                    }
                    self.home_loading_more = false;
                    self.ui.button(id!(home_retry_btn)).set_visible(cx, true);
                    let has_cached = !self.home_quick_picks_pool().is_empty();
                    if has_cached {
                        let pick_n = self.home_quick_picks_pool().len();
                        let shelf_n = self.rich_home.shelves.len();
                        self.ui
                            .label(id!(home_status_lbl))
                            .set_text(cx, &format!("{pick_n} songs · {shelf_n} shelves"));
                        self.show_toast(cx, "YouTube Music timed out; showing cached feed");
                    } else {
                        self.ui
                            .label(id!(home_status_lbl))
                            .set_text(cx, "Connection timed out. Click Retry");
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
                    self.record_home_shown_items();
                    if window_hidden {
                        self.ui_dirty_while_hidden = true;
                    } else {
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
                        self.ui.redraw(cx);
                        self.show_toast(
                            cx,
                            &format!("Loaded {added_shelves} new shelves & {new_picks} songs"),
                        );
                    }
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
                            is_radio_station: false,
                            thumbnail_url: detail.thumbnail_url,
                            share_url: None,
                            loading: false,
                            tracks: detail.tracks,
                        };
                        self.rebuild_detail_tracks_view();
                        if let Some(hist) = &mut self.nav_history {
                            hist.update_current_detail(&self.detail_state);
                        }
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
                    self.radio_loading = false;
                    if self.queue.current().map(|t| t.id.as_str()) == Some(seed_track_id.as_str()) {
                        let added = self.queue.append_tracks(tracks);
                        if added > 0 {
                            self.queue_context = format!("{seed_title} radio");
                            if self.waiting_for_radio_after_eof {
                                self.waiting_for_radio_after_eof = false;
                                if let Some(next) = self.queue.next_track().cloned() {
                                    self.play_track(cx, next);
                                }
                            } else {
                                self.sync_next_deck_to_engine();
                                self.prefetch_next_online_track();
                            }
                        } else {
                            self.waiting_for_radio_after_eof = false;
                        }
                        if window_hidden {
                            self.ui_dirty_while_hidden = true;
                        } else {
                            self.refresh_right_sidebar(cx);
                        }
                    }
                }
                BgMessage::RadioFailed {
                    seed_track_id,
                    error: _err,
                } => {
                    self.radio_loading = false;
                    if self.queue.current().map(|t| t.id.as_str()) == Some(seed_track_id.as_str()) {
                        self.last_radio_seed_id = None;
                        let mut added = 0usize;
                        if self.queue.peek_upcoming(5).is_empty() {
                            let fallback: Vec<Track> = self
                                .home_quick_picks_pool()
                                .into_iter()
                                .filter(|t| t.id != seed_track_id)
                                .take(15)
                                .collect();
                            if !fallback.is_empty() {
                                added = self.queue.append_tracks(fallback);
                            }
                        }
                        if added > 0 && self.waiting_for_radio_after_eof {
                            self.waiting_for_radio_after_eof = false;
                            if let Some(next) = self.queue.next_track().cloned() {
                                self.play_track(cx, next);
                            }
                        } else {
                            self.waiting_for_radio_after_eof = false;
                            if added > 0 {
                                self.sync_next_deck_to_engine();
                                self.prefetch_next_online_track();
                            }
                        }
                        if window_hidden {
                            self.ui_dirty_while_hidden = true;
                        } else {
                            self.refresh_right_sidebar(cx);
                        }
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
                    if !lib.account_id.trim().is_empty() {
                        self.accounts.youtube_account_id = lib.account_id.clone();
                    }
                    if !lib.visitor_data.is_empty() {
                        self.accounts.youtube_visitor_data = lib.visitor_data;
                    }
                    self.accounts.youtube_playlist_count = lib.playlists.len();
                    self.sync_connect_accounts();
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
                    self.refresh_playlists_grid(cx);
                    self.spawn_ytm_rich_home(cx, 0, None, false);
                    self.spawn_recommended_stations(cx, false);
                }
                BgMessage::YtmAccountSyncFailed(err) => {
                    self.ui
                        .label(id!(oauth_status_lbl))
                        .set_text(cx, &format!("YouTube Music sync failed: {err}"));
                }
                BgMessage::SpotifyCatalogSynced {
                    web_session,
                    report,
                } => {
                    self.spotify_sync_in_flight = false;
                    let active_token = if web_session {
                        self.accounts.spotify_web_access_token.trim()
                    } else {
                        self.accounts.spotify_access_token.trim()
                    };
                    if active_token.is_empty() {
                        continue;
                    }
                    let report = *report;
                    if web_session {
                        self.accounts.spotify_web_connected = true;
                        if !report.display_name.trim().is_empty() {
                            self.accounts.spotify_web_user_name = report.display_name.clone();
                        }
                    } else {
                        self.accounts.spotify_connected = true;
                        if !report.display_name.trim().is_empty() {
                            self.accounts.spotify_user_name = report.display_name.clone();
                        }
                    }
                    if !report.account_id.trim().is_empty() {
                        self.accounts.spotify_user_id = report.account_id.clone();
                    }
                    self.sync_connect_accounts();
                    let liked_n = report.liked_tracks.len();
                    let pl_updated = report.playlists.len();
                    let etag_skipped = report.etag_skipped;
                    let rl_skipped = report.rate_limited_skipped;
                    if liked_n > 0 || rl_skipped == 0 {
                        self.accounts.spotify_liked_count = liked_n;
                    }
                    if let Some(db) = &mut self.db {
                        let _ = db.upsert_tracks(&report.liked_tracks);
                        for bundle in &report.playlists {
                            let _ = db.upsert_tracks(&bundle.tracks);
                            let ids: Vec<String> =
                                bundle.tracks.iter().map(|t| t.id.clone()).collect();
                            let _ = db.upsert_playlist(&bundle.playlist, &ids);
                        }
                        let _ = db.match_cloud_tracks_to_local();
                        self.all_tracks = db.list_tracks("").unwrap_or_default();
                        self.playlists = db.list_playlists().unwrap_or_default();
                    }
                    let total_spotify_pl = self
                        .playlists
                        .iter()
                        .filter(|p| p.source == TrackSource::Spotify)
                        .count();
                    self.accounts.spotify_playlist_count = total_spotify_pl.max(pl_updated);
                    if let Some(db) = &self.db {
                        let _ = db.save_accounts(&self.accounts);
                    }
                    let status_lbl = id!(spotify_web_status_lbl);
                    let status_msg = if rl_skipped > 0 {
                        format!(
                            "Spotify: {} · {liked_n} liked & {total_spotify_pl} playlists saved ({rl_skipped} deferred by HTTP 429; wait ~1 min and press Sync again to finish).",
                            report.display_name
                        )
                    } else if etag_skipped > 0 {
                        format!(
                            "Spotify: {} · {liked_n} liked, {pl_updated} updated ({etag_skipped} unchanged skipped).",
                            report.display_name
                        )
                    } else {
                        format!(
                            "Spotify: {} · {liked_n} liked songs & {total_spotify_pl} playlists synced.",
                            report.display_name
                        )
                    };
                    self.ui.label(status_lbl).set_text(cx, &status_msg);
                    self.refresh_spotify_account_ui(cx);
                    self.rebuild_library_views(cx);
                    self.refresh_playlists_grid(cx);
                    self.spawn_recommended_stations(cx, false);
                    self.show_toast(
                        cx,
                        &if rl_skipped > 0 {
                            format!(
                                "Spotify partially synced · {liked_n} liked, {total_spotify_pl} playlists ({rl_skipped} rate-limited)"
                            )
                        } else {
                            format!(
                                "Spotify library synced · {liked_n} liked, {total_spotify_pl} playlists"
                            )
                        },
                    );
                }
                BgMessage::SpotifySyncFailed(error) => {
                    self.spotify_sync_in_flight = false;
                    let clean = friendly_error_message(&error);
                    self.ui
                        .label(id!(spotify_web_status_lbl))
                        .set_text(cx, &format!("Spotify sync failed: {clean}"));
                }
                BgMessage::SpotifyWebLoginResult { sp_dc, token_json } => {
                    match ferric_providers::spotify_web::parse_web_token_json(&token_json) {
                        Ok(token) => {
                            self.accounts.spotify_web_cookie = sp_dc;
                            self.accounts.spotify_web_access_token = token;
                            self.accounts.spotify_web_connected = true;
                            if let Some(db) = &self.db {
                                let _ = db.save_accounts(&self.accounts);
                            }
                            self.refresh_spotify_account_ui(cx);
                            self.show_toast(
                                cx,
                                "Spotify Web session connected! Reading library...",
                            );
                            self.sync_spotify_library_for(cx, true, 500);
                        }
                        Err(err) => {
                            self.ui
                                .label(id!(spotify_web_status_lbl))
                                .set_text(cx, &format!("Spotify Web sign-in: {err}"));
                        }
                    }
                }
                BgMessage::SpotifyWebLoginFailed(err) => {
                    let clean = friendly_error_message(&err);
                    self.ui
                        .label(id!(spotify_web_status_lbl))
                        .set_text(cx, &format!("Spotify Web sign-in: {clean}"));
                }
                BgMessage::RecommendedStationsLoaded { seq, stations } => {
                    if seq != self.stations_request_seq {
                        continue;
                    }
                    if !stations.is_empty() {
                        // Preserve any cached_tracks from already-opened stations with matching IDs
                        let mut next = stations;
                        for st in &mut next {
                            if let Some(prev) = self
                                .recommended_stations
                                .iter()
                                .find(|p| p.id == st.id && !p.cached_tracks.is_empty())
                            {
                                st.cached_tracks = prev.cached_tracks.clone();
                            }
                        }
                        self.recommended_stations = next;
                        self.stations_page = 0;
                        self.record_stations_shown();
                        if window_hidden {
                            self.ui_dirty_while_hidden = true;
                        } else {
                            self.refresh_home_ui(cx);
                            self.ui.redraw(cx);
                        }
                    }
                }
                BgMessage::StationTracksLoaded(payload) => {
                    let StationTracksPayload {
                        station_id,
                        tracks,
                        subtitle,
                        thumbnail_url,
                        left_art,
                        right_art,
                    } = *payload;
                    if let Some(st) = self
                        .recommended_stations
                        .iter_mut()
                        .find(|s| s.id == station_id)
                    {
                        st.cached_tracks = tracks.clone();
                        if !subtitle.trim().is_empty() {
                            st.subtitle = subtitle.clone();
                        }
                        if let Some(la) = left_art {
                            st.left_art_url = Some(la);
                        }
                        if let Some(ra) = right_art {
                            st.right_art_url = Some(ra);
                        }
                        if st.center_art_url.is_none()
                            && let Some(tu) = thumbnail_url.clone()
                        {
                            st.center_art_url = Some(tu);
                        }
                    }
                    if self.detail_state.id == station_id {
                        self.request_artwork_for_tracks(&tracks);
                        if let Some(url) = thumbnail_url.as_deref()
                            && let Some(cache) = &mut self.artwork
                        {
                            let _ = cache.request_url(url);
                        }
                        self.detail_state.tracks = tracks;
                        self.rebuild_detail_tracks_view();
                        if !subtitle.trim().is_empty() {
                            self.detail_state.subtitle = subtitle;
                        }
                        if thumbnail_url.is_some() {
                            self.detail_state.thumbnail_url = thumbnail_url;
                        }
                        self.detail_state.loading = false;
                        if let Some(hist) = &mut self.nav_history {
                            hist.update_current_detail(&self.detail_state);
                        }
                        self.refresh_detail_hero(cx);
                    }
                    if self.active_page == MainPage::Home {
                        self.refresh_home_ui(cx);
                    }
                    self.ui.redraw(cx);
                }
                BgMessage::StationTracksFailed { station_id, error } => {
                    if self.detail_state.id == station_id {
                        self.detail_state.loading = false;
                        self.detail_state.subtitle = format!("Station load failed: {error}");
                        self.refresh_detail_hero(cx);
                        self.show_toast(cx, "Could not load station tracks; retry shortly");
                    }
                }
                BgMessage::CloudPlaylistCreated {
                    local_id,
                    remote_id,
                    source,
                    provider_label,
                    name,
                    pushed_tracks,
                    skipped_tracks,
                } => {
                    if let Some(db) = &mut self.db {
                        let _ = db.migrate_playlist_id(&local_id, &remote_id, source);
                        self.playlists = db.list_playlists().unwrap_or_default();
                    } else if let Some(pl) = self.playlists.iter_mut().find(|p| p.id == local_id) {
                        pl.id = remote_id.clone();
                        pl.source = source;
                    }
                    let mut pin_updated = false;
                    for pid in &mut self.session_state.pinned_ids {
                        if *pid == local_id {
                            *pid = remote_id.clone();
                            pin_updated = true;
                        }
                    }
                    if pin_updated {
                        self.mark_session_dirty();
                    }
                    if self.detail_state.id == local_id {
                        self.detail_state.id = remote_id.clone();
                        self.detail_state.share_url =
                            collection_share_url(&remote_id, YtmCardKind::Playlist).map(|(_, u)| u);
                        if let Some(hist) = &mut self.nav_history {
                            hist.update_current_detail(&self.detail_state);
                        }
                        self.refresh_detail_hero(cx);
                    }
                    if !window_hidden {
                        self.refresh_playlists_grid(cx);
                        self.rebuild_library_views(cx);
                        let msg = if pushed_tracks == 0 && skipped_tracks == 0 {
                            format!(
                                "Synced '{}' to {} account",
                                clip_text(&name, 26),
                                provider_label
                            )
                        } else if skipped_tracks == 0 {
                            format!(
                                "Synced '{}' to {} ({} songs)",
                                clip_text(&name, 24),
                                provider_label,
                                pushed_tracks
                            )
                        } else {
                            format!(
                                "Synced '{}' to {} ({} synced, {} local-only)",
                                clip_text(&name, 20),
                                provider_label,
                                pushed_tracks,
                                skipped_tracks
                            )
                        };
                        self.show_toast(cx, &msg);
                    } else {
                        self.ui_dirty_while_hidden = true;
                    }
                }
                BgMessage::CloudPlaylistMutationNotice(msg) => {
                    if !window_hidden {
                        self.show_toast(cx, &msg);
                    }
                }
                BgMessage::StreamReady(payload) => {
                    let StreamReadyPayload {
                        track_id,
                        video_id,
                        stream,
                        auto_play,
                    } = *payload;
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
                        self.consecutive_stream_failures = 0;
                        let pending_meta = self.pending_stream_track.take().map(|mut t| {
                            t.path = Some(stream.path.clone());
                            t.codec = stream.codec.clone();
                            if stream.bitrate_kbps > 0 {
                                t.bitrate_kbps = stream.bitrate_kbps;
                            }
                            if stream.duration_ms > 0 && t.duration_ms == 0 {
                                t.duration_ms = stream.duration_ms;
                            }
                            t
                        });
                        let resolved_track = updated_track
                            .or(pending_meta)
                            .or_else(|| self.queue.current().cloned());
                        let rg = resolved_track.as_ref().map_or(0, |t| t.replay_gain_db);
                        let dur_ms = resolved_track.as_ref().map_or(stream.duration_ms, |t| {
                            t.duration_ms.max(stream.duration_ms)
                        });
                        self.send_play_or_crossfade(stream.path, rg, dur_ms);
                        if let Some(seek_ms) = self.pending_handoff_seek_ms.take()
                            && seek_ms > 1_000
                            && let Some(audio) = &self.audio
                        {
                            audio.send(EngineCommand::SeekMs(seek_ms));
                        }
                        let start_paused = self.pending_handoff_paused;
                        self.pending_handoff_paused = false;
                        if start_paused && let Some(audio) = &self.audio {
                            audio.send(EngineCommand::Pause);
                        }
                        self.sync_next_deck_to_engine();
                        self.prefetch_next_online_track();
                        if let Some(tr) = resolved_track {
                            if start_paused {
                                self.sync_now_playing_ui_widgets(cx, &tr, false);
                            } else {
                                self.update_now_playing_metadata(cx, &tr);
                            }
                        }
                        self.push_connect_playback_snapshot();
                    } else {
                        self.sync_next_deck_to_engine();
                    }
                    if let Some(db) = &self.db {
                        db.shrink_memory();
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
                        self.pending_stream_track = None;
                        self.consecutive_stream_failures =
                            self.consecutive_stream_failures.saturating_add(1);
                        if self.consecutive_stream_failures <= 3
                            && let Some(next_candidate) = self.queue.next_track_manual().cloned()
                            && next_candidate.id != track_id
                        {
                            if !window_hidden {
                                self.show_toast(cx, "Skipping unavailable stream...");
                            }
                            self.play_track_inner(cx, next_candidate, false);
                        } else if window_hidden {
                            self.ui_dirty_while_hidden = true;
                        } else {
                            self.ui
                                .label(id!(np_artist_lbl))
                                .set_text(cx, "Stream unavailable");
                            self.ui.button(id!(codec_pill_btn)).set_text(cx, "OFFLINE");
                            let clean = friendly_error_message(&error);
                            self.show_toast(cx, &format!("Stream failed: {clean}"));
                        }
                    }
                }
                BgMessage::VideoStreamsLoaded {
                    track_id,
                    video_id,
                    is_official_mv,
                    streams,
                } => {
                    if let Some(t) = self.all_tracks.iter_mut().find(|x| x.id == track_id)
                        && t.youtube_video_id.is_none()
                    {
                        t.youtube_video_id = Some(video_id);
                    }
                    if self.video_open && self.video_track_id.as_deref() == Some(track_id.as_str())
                    {
                        self.video_is_official_mv = is_official_mv;
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
                BgMessage::UpdateDownload { gen_id, event } => {
                    if gen_id != self.update_download_gen || !self.update_popup_visible {
                        continue;
                    }
                    match event {
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
                        DownloadEvent::Ready { path, sha256 } => {
                            self.update_download_in_flight = false;
                            let updates_dir = app_data_dir().join("updates");
                            match launch_verified_installer(&path, &updates_dir, &sha256) {
                                Ok(()) => {
                                    self.flush_session_state_now();
                                    if let Some(engine) = self.connect_engine.take() {
                                        engine.shutdown();
                                    }
                                    self.close_native_video_stage(cx);
                                    win32_force_quit();
                                    cx.quit();
                                }
                                Err(err) => {
                                    self.ui.button(id!(update_now_btn)).set_text(cx, "Retry");
                                    self.ui.label(id!(update_popup_sub)).set_text(cx, &err);
                                    self.show_toast(cx, &err);
                                }
                            }
                        }
                        DownloadEvent::Failed(err) => {
                            self.update_download_in_flight = false;
                            self.ui.button(id!(update_now_btn)).set_text(cx, "Retry");
                            self.ui.label(id!(update_popup_sub)).set_text(cx, &err);
                        }
                    }
                }
                BgMessage::Connect(ev) => {
                    self.handle_connect_event(cx, *ev);
                }
            }
        }

        // 2. Poll Windows SMTC hardware media keys & third-party media overlay commands
        if let Some(smtc) = &mut self.smtc {
            smtc.try_attach_late();
        }
        let smtc_cmds = self
            .smtc
            .as_ref()
            .map(|s| s.poll_commands())
            .unwrap_or_default();
        for cmd in smtc_cmds {
            let remote_engine = if self.is_controlling_remote() {
                self.connect_engine.as_ref()
            } else {
                None
            };
            match cmd {
                SmtcCommand::Play => {
                    if let Some(engine) = remote_engine {
                        engine.send_remote_action(RemoteAction::Play);
                    } else if let Some(a) = &self.audio {
                        a.send(EngineCommand::Play);
                    }
                }
                SmtcCommand::Pause => {
                    if let Some(engine) = remote_engine {
                        engine.send_remote_action(RemoteAction::Pause);
                    } else if let Some(a) = &self.audio {
                        a.send(EngineCommand::Pause);
                    }
                }
                SmtcCommand::Toggle => {
                    if let Some(engine) = remote_engine {
                        engine.send_remote_action(RemoteAction::TogglePlayPause);
                    } else if let Some(a) = &self.audio {
                        a.send(EngineCommand::TogglePlayPause);
                    }
                }
                SmtcCommand::Next => {
                    if let Some(engine) = remote_engine {
                        engine.send_remote_action(RemoteAction::SkipNext);
                    } else if let Some(next) = self.queue.next_track_manual().cloned() {
                        self.play_track(cx, next);
                    }
                }
                SmtcCommand::Previous => {
                    if let Some(engine) = remote_engine {
                        engine.send_remote_action(RemoteAction::SkipPrev);
                    } else if let Some(prev) = self.queue.prev_track().cloned() {
                        self.play_track(cx, prev);
                    }
                }
                SmtcCommand::Stop => {
                    if let Some(a) = &self.audio {
                        a.send(EngineCommand::Stop);
                    }
                }
                SmtcCommand::SetPositionMs(target_ms) => {
                    if let Some(engine) = remote_engine {
                        engine.send_remote_action(RemoteAction::SeekTo {
                            position_ms: target_ms,
                        });
                    } else if let Some(a) = &self.audio {
                        a.send(EngineCommand::SeekMs(target_ms));
                    }
                }
                SmtcCommand::SeekRelativeMs(delta_ms) => {
                    if let Some(engine) = remote_engine
                        && let Some(peer) = self.active_remote_peer()
                    {
                        let cur_ms = peer.interpolated_position_ms() as i64;
                        let target_ms = (cur_ms + delta_ms).max(0) as u64;
                        engine.send_remote_action(RemoteAction::SeekTo {
                            position_ms: target_ms,
                        });
                    } else if let Some(a) = &self.audio {
                        let cur_ms = a.telemetry().position_ms as i64;
                        let target_ms = (cur_ms + delta_ms).max(0) as u64;
                        a.send(EngineCommand::SeekMs(target_ms));
                    }
                }
            }
        }

        // 3. Sleep timer countdown (with optional smooth 4-second fade-out)
        if let SleepTimerMode::Deadline(deadline) = self.sleep_timer {
            let now = Instant::now();
            if now >= deadline {
                self.sleep_timer = SleepTimerMode::Off;
                self.sleep_fade_base_vol = None;
                self.ui
                    .label(id!(sleep_status_lbl))
                    .set_text(cx, "Sleep Timer: Paused smoothly");
                if let Some(a) = &self.audio {
                    a.send(EngineCommand::Pause);
                    a.send(EngineCommand::SetVolume(self.session_state.volume));
                }
            } else {
                let rem = deadline.duration_since(now);
                let rem_s = rem.as_secs();
                if self.dsp.sleep_timer_fade_out && rem <= Duration::from_secs(4) {
                    let base = *self
                        .sleep_fade_base_vol
                        .get_or_insert(self.session_state.volume);
                    let frac = (rem.as_secs_f32() / 4.0).clamp(0.0, 1.0);
                    if let Some(a) = &self.audio {
                        a.send(EngineCommand::SetVolume(base * frac));
                    }
                }
                self.ui.label(id!(sleep_status_lbl)).set_text(
                    cx,
                    &format!("Sleep Timer: {}:{:02} left", rem_s / 60, rem_s % 60),
                );
            }
        }

        // 4. Periodic Ferric Connect playback state sync (~1 Hz while playing + immediate on state flip)
        if !self.is_controlling_remote()
            && let Some(audio) = &self.audio
        {
            let playing_now = audio.telemetry().is_playing;
            let state_flipped = self.last_connect_pushed_playing != Some(playing_now);
            self.connect_sync_ticks = self.connect_sync_ticks.wrapping_add(1);
            if state_flipped || (playing_now && self.connect_sync_ticks.is_multiple_of(10)) {
                self.last_connect_pushed_playing = Some(playing_now);
                self.push_connect_playback_snapshot();
            }
        }

        // 5. Sync Window Maximize / Restore Glyph (throttled to 1 Hz; instant on WM_SIZE/click)
        if !window_hidden
            && self
                .last_win_max_check_at
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
        {
            self.last_win_max_check_at = Some(Instant::now());
            self.ui
                .ferric_win_btn(id!(win_max_btn))
                .set_maximized(cx, win32_is_active_window_maximized());
        }

        // When another Ferric device is active speaker, mirror its timeline smoothly at 250ms and skip local seekbar overwrite.
        if self.update_remote_timeline_tick(cx, window_hidden) {
            return;
        }

        // 6. Audio Telemetry, Device Hot-Swap Recovery, Zero-Gap Crossfade Auto-Transition, Prefetch, Video Sync & Spectrum
        if !self.is_controlling_remote()
            && let Some(audio) = &mut self.audio
        {
            audio.poll_device_recovery();
        }
        if let Some(audio) = &self.audio {
            let snap = audio.telemetry();
            if snap.track_finished_seq != self.last_finished_seq {
                self.last_finished_seq = snap.track_finished_seq;
                if matches!(self.sleep_timer, SleepTimerMode::EndOfTrack) {
                    self.sleep_timer = SleepTimerMode::Off;
                    audio.send(EngineCommand::Stop);
                    if window_hidden {
                        self.ui_dirty_while_hidden = true;
                    } else {
                        self.ui
                            .label(id!(sleep_status_lbl))
                            .set_text(cx, "Sleep Timer: Stopped after song");
                    }
                } else if let Some(next) = self.queue.next_track().cloned() {
                    self.waiting_for_radio_after_eof = false;
                    if snap.auto_transitioned {
                        self.last_pos_txt.clear();
                        self.last_dur_txt.clear();
                        self.last_codec_txt.clear();
                        self.sync_next_deck_to_engine();
                        self.prefetch_next_online_track();
                        self.maybe_fetch_radio_for_track(&next, false);
                        self.update_now_playing_metadata(cx, &next);
                        if !window_hidden {
                            self.update_lyrics_display(cx, snap.position_ms);
                        }
                    } else {
                        self.play_track(cx, next);
                    }
                } else {
                    if self.session_state.autoplay_radio
                        && self.queue.repeat == RepeatMode::Off
                        && let Some(cur) = self.queue.current().cloned()
                    {
                        self.waiting_for_radio_after_eof = true;
                        if !self.radio_loading {
                            self.maybe_fetch_radio_for_track(&cur, true);
                        }
                        if let Some(next) = self.queue.next_track().cloned() {
                            self.waiting_for_radio_after_eof = false;
                            self.play_track(cx, next);
                            if window_hidden {
                                self.ui_dirty_while_hidden = true;
                            } else {
                                self.refresh_right_sidebar(cx);
                            }
                        } else if !window_hidden {
                            self.ui
                                .ferric_play_btn(id!(play_pause_btn))
                                .set_playing(cx, false);
                            self.ui
                                .ferric_play_btn(id!(fs_play_pause_btn))
                                .set_playing(cx, false);
                        }
                    } else {
                        self.waiting_for_radio_after_eof = false;
                        if !window_hidden {
                            self.ui
                                .ferric_play_btn(id!(play_pause_btn))
                                .set_playing(cx, false);
                            self.ui
                                .ferric_play_btn(id!(fs_play_pause_btn))
                                .set_playing(cx, false);
                        }
                    }
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
                let eff_dur = snap
                    .duration_ms
                    .max(self.queue.current().map_or(0, |t| t.duration_ms));
                smtc.sync_playback(snap.is_playing, snap.position_ms, eff_dur);
            }
            let cur_track_key = self
                .queue
                .current()
                .map(|c| c.id.as_str())
                .unwrap_or_default();
            let taskbar_changed = self.last_taskbar_track_key != cur_track_key
                || self.last_taskbar_playing != Some(snap.is_playing);
            if taskbar_changed {
                self.last_taskbar_track_key = cur_track_key.to_string();
                self.last_taskbar_playing = Some(snap.is_playing);
                if let Some(cur) = self.queue.current() {
                    win32_update_taskbar_playback(&cur.title, &cur.artist, snap.is_playing);
                } else {
                    win32_update_taskbar_playback("", "", false);
                }
            }
            let discord_changed = self.last_discord_track_id != cur_track_key
                || self.last_discord_playing != Some(snap.is_playing);
            if self.discord_settings.enabled
                && (discord_changed
                    || self
                        .last_discord_tick_at
                        .is_none_or(|t| t.elapsed() >= Duration::from_secs(1)))
            {
                self.last_discord_tick_at = Some(Instant::now());
                self.last_discord_track_id.clear();
                self.last_discord_track_id.push_str(cur_track_key);
                self.last_discord_playing = Some(snap.is_playing);
                if let Some(cur) = self.queue.current() {
                    if let Some(rpc) = &mut self.discord_rpc {
                        rpc.update_playback(Some(Self::build_discord_snapshot(
                            cur,
                            snap.position_ms,
                            snap.duration_ms.max(cur.duration_ms),
                            snap.is_playing,
                        )));
                    }
                } else if let Some(rpc) = &mut self.discord_rpc {
                    rpc.update_playback(None);
                }
                if let Some(rpc) = &self.discord_rpc {
                    let st = rpc.status_text();
                    if st != self.last_discord_status {
                        self.last_discord_status = st;
                        if window_hidden {
                            self.ui_dirty_while_hidden = true;
                        } else {
                            self.ui
                                .label(id!(discord_status_lbl))
                                .set_text(cx, &self.last_discord_status);
                        }
                    }
                }
            }

            // Skip GPU UI telemetry redraws when hidden in tray / minimized -> near-0% idle GPU/CPU
            if window_hidden {
                let desired = if snap.is_playing { 500 } else { 1000 };
                self.sync_tick_timer_rate(cx, desired);
                return;
            }
            let desired_open = if snap.is_playing
                || self.artwork_was_pending
                || color_lerping
                || self.pending_live_search.is_some()
                || self.toast_until.is_some()
                || self.pending_stream_track_id.is_some()
            {
                100
            } else {
                350
            };
            self.sync_tick_timer_rate(cx, desired_open);

            if snap.duration_ms > 0 {
                let time_changed = self.last_tick_pos_ms != snap.position_ms
                    || self.last_tick_dur_ms != snap.duration_ms
                    || self.last_dur_txt.is_empty();
                if snap.is_playing || time_changed {
                    self.last_tick_pos_ms = snap.position_ms;
                    self.last_tick_dur_ms = snap.duration_ms;
                    let ratio = (snap.position_ms as f64 / snap.duration_ms as f64).clamp(0.0, 1.0);
                    let pos_txt = format_compact_time_ms(snap.position_ms);
                    self.ui
                        .ferric_bar_slider(id!(seek_slider))
                        .set_value(cx, ratio);
                    let dur_txt = if self.show_remaining_time {
                        format!(
                            "-{}",
                            format_compact_time_ms(
                                snap.duration_ms.saturating_sub(snap.position_ms)
                            )
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
            }

            self.ui
                .ferric_play_btn(id!(play_pause_btn))
                .set_playing(cx, snap.is_playing);
            if self.fullscreen_open {
                self.ui
                    .ferric_play_btn(id!(fs_play_pause_btn))
                    .set_playing(cx, snap.is_playing);
                if self.last_fs_backdrop_playing != Some(snap.is_playing) {
                    self.last_fs_backdrop_playing = Some(snap.is_playing);
                    let acr = if self.theme.acrylic_enabled {
                        1.0_f32
                    } else {
                        0.0_f32
                    };
                    self.ui.ferric_ambient_stage(id!(fs_backdrop)).set_state(
                        cx,
                        true,
                        snap.is_playing,
                        self.current_accent(),
                        acr,
                    );
                }
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
                let norm = |i: usize| (snap.spectrum_bins[i] as f32 / 1_000.0).clamp(0.05, 1.0);
                if self.fullscreen_open {
                    self.last_fs_viz_playing = Some(true);
                    self.canvas_phase = (self.canvas_phase + 0.14) % 628.0;
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
                } else if self.tick_counter.is_multiple_of(5) {
                    // Calm ~2 Hz musical equalizer bar pulse strictly on visible cover slots
                    // to avoid triggering high-rate full-window D3D11 repaints while reading/browsing.
                    let raw_avg = (norm(0) + norm(1) + norm(2)) / 3.0;
                    let q_avg = (raw_avg * 10.0).round() / 10.0;
                    self.ui
                        .ferric_cover_art(id!(np_art))
                        .set_state(cx, true, q_avg, accent, false);
                    if self.right_sidebar_open && !self.right_lyrics_tab {
                        self.ui
                            .view(id!(q_now_row))
                            .ferric_cover_art(id!(art))
                            .set_state(cx, true, q_avg, accent, false);
                    }
                }
            } else {
                if self.last_fs_viz_playing != Some(false) {
                    self.last_fs_viz_playing = Some(false);
                    self.ui.view(id!(np_canvas)).apply_over(
                        cx,
                        live! {
                            draw_bg: {
                                mode: 1.0,
                                s0: 0.05, s1: 0.05, s2: 0.05, s3: 0.05,
                                s4: 0.05, s5: 0.05, s6: 0.05, s7: 0.05
                            }
                        },
                    );
                }
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
                self.flush_session_state_now();
                if let Some(engine) = self.connect_engine.take() {
                    engine.shutdown();
                }
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
                236.0 * z
            } else {
                0.0
            };
            if self.left_sidebar_open {
                self.ui.view(id!(left_sidebar)).set_visible(cx, true);
                self.ui.view(id!(left_divider)).set_visible(cx, true);
            }
            self.sync_sidebar_toggle_buttons(cx);
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
            self.navigate_to_entry(cx, prev);
        }
        if self.ui.button(id!(nav_fwd_btn)).clicked(actions)
            && let Some(next) = self.nav_history.as_mut().and_then(NavHistory::go_forward)
        {
            self.navigate_to_entry(cx, next);
        }
        if self.ui.button(id!(refresh_btn)).clicked(actions) {
            self.trigger_icon_pulse(cx, id!(refresh_btn), 1.0);
            let params = if self.active_mood_idx > 0 {
                self.rich_home
                    .chips
                    .get(self.active_mood_idx - 1)
                    .map(|c| c.params.clone())
            } else {
                None
            };
            self.spawn_ytm_rich_home(cx, self.active_mood_idx, params, true);
            self.spawn_recommended_stations(cx, true);
            self.show_toast(cx, "Refreshing Home & Recommended Stations...");
        }
        if self.ui.button(id!(top_search_clear_btn)).clicked(actions) {
            self.clear_search_state(cx, true);
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
        if self.ui.button(id!(top_settings_btn)).clicked(actions) {
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
        if self.ui.button(id!(sidebar_create_pl_btn)).clicked(actions) {
            self.navigate_to(cx, MainPage::Playlists, true);
            self.ui.widget(id!(pl_new_input)).set_key_focus(cx);
            self.show_toast(cx, "Type a playlist name and press Enter to create");
        }
        for (btn, filter) in [
            (id!(sb_src_all_btn), PlaylistSourceFilter::All),
            (id!(sb_src_yt_btn), PlaylistSourceFilter::YouTube),
            (id!(sb_src_sp_btn), PlaylistSourceFilter::Spotify),
            (id!(sb_src_local_btn), PlaylistSourceFilter::Local),
            (id!(pl_src_all_btn), PlaylistSourceFilter::All),
            (id!(pl_src_yt_btn), PlaylistSourceFilter::YouTube),
            (id!(pl_src_sp_btn), PlaylistSourceFilter::Spotify),
            (id!(pl_src_local_btn), PlaylistSourceFilter::Local),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.set_playlist_source_filter(cx, filter);
            }
        }
        if self.ui.button(id!(sb_pl_sort_btn)).clicked(actions) {
            let next = self.playlist_sort.next();
            self.set_playlist_sort(cx, next);
        }
        for (btn, order) in [
            (id!(pl_sort_newest_btn), PlaylistSortOrder::Newest),
            (id!(pl_sort_oldest_btn), PlaylistSortOrder::Oldest),
            (id!(pl_sort_az_btn), PlaylistSortOrder::NameAZ),
        ] {
            if self.ui.button(btn).clicked(actions) {
                self.set_playlist_sort(cx, order);
            }
        }
        if let Some(filter_txt) = self
            .ui
            .text_input(id!(sidebar_lib_filter_input))
            .changed(actions)
        {
            self.sidebar_filter_query = filter_txt;
            self.refresh_sidebar_library(cx);
        }
        if view_clicked(&self.ui.view(id!(pin_liked_row)), actions) {
            self.active_filter = LibraryFilter::Liked;
            self.rebuild_library_views(cx);
            self.navigate_to(cx, MainPage::Songs, true);
        }
        if view_clicked(&self.ui.view(id!(pin_local_row)), actions) {
            self.active_filter = LibraryFilter::Local;
            self.rebuild_library_views(cx);
            self.navigate_to(cx, MainPage::Songs, true);
        }
        let mut clicked_pin_idx = None;
        for (i, slot) in sidebar_pin_slot_ids().into_iter().enumerate() {
            let row = self.ui.view(slot);
            if row.ferric_cover_art(id!(art)).clicked(actions) || view_clicked(&row, actions) {
                clicked_pin_idx = Some(i);
                break;
            }
        }
        if let Some(i) = clicked_pin_idx {
            let entries = self.resolve_sidebar_library_entries();
            if let Some(entry) = entries.get(i)
                && let Some(group) = self.resolve_sidebar_collection(&entry.id)
            {
                self.open_local_detail(cx, group);
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
            self.spawn_recommended_stations(cx, true);
            self.show_toast(cx, "Discovering fresh albums, playlists & stations...");
        }
        if self.ui.button(id!(qp_refresh_btn)).clicked(actions) {
            let params = if self.active_mood_idx > 0 {
                self.rich_home
                    .chips
                    .get(self.active_mood_idx - 1)
                    .map(|c| c.params.clone())
            } else {
                None
            };
            self.spawn_ytm_rich_home(cx, self.active_mood_idx, params, true);
            self.show_toast(cx, "Refreshing Quick picks & Home...");
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
            if v.ferric_cover_art(id!(art)).clicked(actions) || view_clicked(&v, actions) {
                let pool = self.home_quick_picks_pool();
                let idx = self.qp_page * 8 + slot;
                if let Some(track) = pool.get(idx).cloned() {
                    if track.youtube_video_id.is_some() && self.session_state.autoplay_radio {
                        self.queue_context = format!("{} radio", track.title);
                        self.queue.set_tracks(vec![track.clone()], Some(0));
                        self.last_radio_seed_id = None;
                        self.play_track(cx, track);
                    } else {
                        self.queue_context = "Quick picks".to_string();
                        self.queue.set_tracks(pool, Some(idx));
                        self.play_track(cx, track);
                    }
                }
            }
        }

        // Recommended Stations paging, refresh & clicks
        if self.ui.button(id!(stations_refresh_btn)).clicked(actions) {
            self.spawn_recommended_stations(cx, true);
            self.show_toast(cx, "Shuffling Recommended Stations...");
        }
        if self.ui.button(id!(stations_prev_btn)).clicked(actions) && self.stations_page > 0 {
            self.stations_page -= 1;
            self.refresh_home_ui(cx);
        }
        if self.ui.button(id!(stations_next_btn)).clicked(actions)
            && (self.stations_page + 1) * 5 < self.recommended_stations.len()
        {
            self.stations_page += 1;
            self.refresh_home_ui(cx);
        }
        for (ci, sid) in [id!(st_c0), id!(st_c1), id!(st_c2), id!(st_c3), id!(st_c4)]
            .into_iter()
            .enumerate()
        {
            let sv = self.ui.view(sid);
            let tbox = sv.view(id!(triptych_box));
            if sv.ferric_cover_art(id!(full_art)).clicked(actions)
                || tbox.ferric_cover_art(id!(art_center)).clicked(actions)
                || tbox.ferric_cover_art(id!(art_left)).clicked(actions)
                || tbox.ferric_cover_art(id!(art_right)).clicked(actions)
                || view_clicked(&tbox, actions)
                || view_clicked(&sv, actions)
            {
                let idx = self.stations_page * 5 + ci;
                if let Some(st) = self.recommended_stations.get(idx).cloned() {
                    self.open_recommended_station(cx, st);
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
                if cv.ferric_cover_art(id!(art)).clicked(actions) || view_clicked(&cv, actions) {
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
            self.clear_search_state(cx, false);
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
            if cv.ferric_cover_art(id!(art)).clicked(actions) || view_clicked(&cv, actions) {
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
            let start_idx = fastrand::usize(..self.filtered_tracks.len());
            self.queue_context = "Library Shuffle".to_string();
            self.queue
                .set_tracks((*self.filtered_tracks).clone(), Some(start_idx));
            self.queue.set_shuffle(true);
            self.session_state.shuffle = true;
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
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
            if (v.ferric_cover_art(id!(art)).clicked(actions) || view_clicked(&v, actions))
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
            if (v.ferric_cover_art(id!(art)).clicked(actions) || view_clicked(&v, actions))
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
                let local_id = format!("local:pl:{}", csprng_hex(6));
                let pl = Playlist {
                    id: local_id.clone(),
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
                self.spawn_cloud_create_playlist(
                    local_id,
                    name.clone(),
                    "Custom playlist".to_string(),
                    Vec::new(),
                );
                self.show_toast(cx, &format!("Created playlist '{name}'"));
            }
        }
        if self.ui.button(id!(pl_empty_focus_btn)).clicked(actions) {
            self.ui.widget(id!(pl_new_input)).set_key_focus(cx);
            self.show_toast(cx, "Type a playlist name above and press Enter to create");
        }
        if self.ui.button(id!(pl_empty_google_btn)).clicked(actions) {
            self.navigate_to(cx, MainPage::Settings, true);
            self.switch_settings_tab(cx, SettingsTab::Accounts);
        }
        if (self.ui.button(id!(pl_import_btn)).clicked(actions)
            || self.ui.button(id!(pl_empty_import_btn)).clicked(actions))
            && let Some(file) = rfd::FileDialog::new()
                .set_title("Import Playlist (.m3u, .m3u8, .csv)")
                .add_filter("Playlist Files", &["m3u", "m3u8", "csv"])
                .pick_file()
        {
            match read_text_file_capped(&file, 4 * 1024 * 1024) {
                Ok(content) => {
                    let stem = file
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Imported Playlist");
                    let (pl, tracks) = parse_playlist_import_text(stem, &content, file.parent());
                    if tracks.is_empty() {
                        self.show_toast(cx, "No valid tracks found in playlist file");
                    } else {
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
                        self.show_toast(
                            cx,
                            &format!("Imported '{}' ({} tracks)", pl.name, tracks.len()),
                        );
                    }
                }
                Err(err) => {
                    self.show_toast(cx, &format!("Could not read playlist file: {err}"));
                }
            }
        }
        if self.ui.button(id!(pl_prev_btn)).clicked(actions) && self.playlists_page > 0 {
            self.playlists_page -= 1;
            self.refresh_playlists_grid(cx);
        }
        let filtered_playlists_view = self.filtered_playlists();
        if self.ui.button(id!(pl_next_btn)).clicked(actions)
            && (self.playlists_page + 1) * 20 < filtered_playlists_view.len()
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
            if (v.ferric_cover_art(id!(art)).clicked(actions) || view_clicked(&v, actions))
                && let Some(pl) = filtered_playlists_view
                    .get(self.playlists_page * 20 + i)
                    .cloned()
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

        // Detail Page Buttons (Play / Shuffle / + Queue respect active filter & sort!)
        if self.ui.button(id!(detail_play_btn)).clicked(actions) {
            let visible_tracks = (*self.detail_tracks).clone();
            if let Some(first) = visible_tracks.first().cloned() {
                self.queue_context = self.detail_state.title.clone();
                self.queue.set_tracks(visible_tracks, Some(0));
                self.play_track(cx, first);
            }
        }
        if self.ui.button(id!(detail_shuffle_btn)).clicked(actions)
            && !self.detail_tracks.is_empty()
        {
            let visible_tracks = (*self.detail_tracks).clone();
            let start_idx = fastrand::usize(..visible_tracks.len());
            self.queue_context = self.detail_state.title.clone();
            self.queue.set_tracks(visible_tracks, Some(start_idx));
            self.queue.set_shuffle(true);
            self.session_state.shuffle = true;
            self.mark_session_dirty();
            self.sync_controls_from_state(cx);
            if let Some(first) = self.queue.current().cloned() {
                self.play_track(cx, first);
            }
        }
        if self.ui.button(id!(detail_save_pl_btn)).clicked(actions) {
            self.save_detail_as_playlist(cx);
        }
        if self.ui.button(id!(detail_cloud_push_btn)).clicked(actions)
            && self.detail_state.id.starts_with("local:pl:")
        {
            let desc = if !self.detail_state.description.trim().is_empty() {
                self.detail_state.description.clone()
            } else {
                self.detail_state.subtitle.clone()
            };
            self.spawn_cloud_create_playlist(
                self.detail_state.id.clone(),
                self.detail_state.title.clone(),
                desc,
                self.detail_state.tracks.clone(),
            );
            self.show_toast(cx, "Syncing playlist to your signed-in account...");
        }
        if self.ui.button(id!(detail_radio_btn)).clicked(actions)
            && let Some(first) = self.detail_tracks.first().cloned()
        {
            self.queue_context = format!("{} radio", first.title);
            self.queue.set_tracks(vec![first.clone()], Some(0));
            self.last_radio_seed_id = None;
            self.play_track(cx, first.clone());
            if !self.session_state.autoplay_radio {
                self.maybe_fetch_radio_for_track(&first, true);
            }
            self.show_toast(cx, &format!("Started radio for '{}'", first.title));
        }
        if self.ui.button(id!(detail_pin_btn)).clicked(actions) && !self.detail_state.id.is_empty()
        {
            let pinned = self.session_state.toggle_pin(&self.detail_state.id);
            self.mark_session_dirty();
            self.refresh_detail_hero(cx);
            self.rebuild_library_views(cx);
            self.show_toast(
                cx,
                if pinned {
                    "Pinned to sidebar"
                } else {
                    "Unpinned"
                },
            );
        }
        if self.ui.button(id!(detail_queue_btn)).clicked(actions) && !self.detail_tracks.is_empty()
        {
            let added = self.queue.append_tracks((*self.detail_tracks).clone());
            self.sync_next_deck_to_engine();
            self.refresh_right_sidebar(cx);
            self.show_toast(cx, &format!("Added {added} songs to Up next"));
        }
        if self.ui.button(id!(detail_dedup_btn)).clicked(actions)
            && self.detail_state.kind == YtmCardKind::Playlist
            && !self.detail_state.id.is_empty()
        {
            let (deduped, removed) = deduplicate_tracks_in_order(&self.detail_state.tracks);
            if removed == 0 {
                self.show_toast(cx, "No duplicate songs found in this playlist");
            } else {
                let pl_id = self.detail_state.id.clone();
                let ids: Vec<String> = deduped.iter().map(|t| t.id.clone()).collect();
                if let Some(db) = &mut self.db {
                    let _ = db.replace_playlist_tracks(&pl_id, &ids);
                    self.playlists = db.list_playlists().unwrap_or_default();
                }
                self.detail_state.tracks = deduped.clone();
                self.rebuild_detail_tracks_view();
                self.refresh_detail_hero(cx);
                self.refresh_playlists_grid(cx);
                self.rebuild_library_views(cx);
                if (pl_id.starts_with("spotify:playlist:")
                    || pl_id.starts_with("spotify:premium:playlist:"))
                    && let Some(token) = self.active_spotify_write_token()
                    && let Some(tx) = self.bg_tx.clone()
                {
                    let uris: Vec<String> = deduped
                        .iter()
                        .filter_map(|t| t.spotify_id.clone())
                        .filter(|s| !s.trim().is_empty())
                        .collect();
                    if !uris.is_empty() {
                        thread::spawn(move || {
                            if let Err(
                                ferric_providers::spotify_web::SpotifyWebSyncError::AuthExpired,
                            ) = replace_spotify_playlist_tracks(&token, &pl_id, &uris)
                            {
                                let _ = tx.send(BgMessage::CloudPlaylistMutationNotice(
                                    "Deduplicated locally (Spotify session expired; sign in again)"
                                        .to_string(),
                                ));
                            }
                        });
                    }
                }
                self.show_toast(cx, &format!("Removed {removed} duplicate song(s)"));
            }
        }
        if self.ui.button(id!(detail_copy_link_btn)).clicked(actions) {
            if let Some((platform, url)) = self.detail_share_link() {
                let _ = copy_text_to_clipboard(&url);
                self.show_toast(
                    cx,
                    &format!(
                        "Copied {platform} link: {}",
                        clip_text(&self.detail_state.title, 26)
                    ),
                );
            } else {
                self.show_toast(cx, "Local playlist has no online URL; use Copy Tracklist");
            }
        }
        if self.ui.button(id!(detail_copy_list_btn)).clicked(actions)
            && !self.detail_state.tracks.is_empty()
        {
            let share_url = self.detail_share_link().map(|(_, u)| u);
            let txt = format_collection_tracklist(
                &self.detail_state.title,
                share_url.as_deref(),
                &self.detail_state.tracks,
            );
            let _ = copy_text_to_clipboard(&txt);
            self.show_toast(
                cx,
                &format!(
                    "Copied tracklist ({} songs) to clipboard",
                    self.detail_state.tracks.len()
                ),
            );
        }
        if self.ui.button(id!(detail_open_source_btn)).clicked(actions)
            && let Some((platform, url)) = self.detail_share_link()
        {
            match open_allowed_url(&url) {
                Ok(()) => self.show_toast(cx, &format!("Opened in {platform}")),
                Err(err) => self.show_toast(cx, &err),
            }
        }
        if self.ui.button(id!(detail_export_btn)).clicked(actions) {
            self.export_detail_m3u(cx);
        }
        if self.ui.button(id!(detail_export_csv_btn)).clicked(actions) {
            self.export_detail_csv(cx);
        }
        if let Some(q) = self
            .ui
            .text_input(id!(detail_filter_input))
            .changed(actions)
        {
            self.detail_filter_query = q;
            self.rebuild_detail_tracks_view();
            self.ui.redraw(cx);
        }
        if self.ui.button(id!(detail_sort_btn)).clicked(actions) {
            self.detail_sort_order = self.detail_sort_order.next();
            self.rebuild_detail_tracks_view();
            self.ui
                .button(id!(detail_sort_btn))
                .set_text(cx, self.detail_sort_order.label());
            self.ui.redraw(cx);
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
                self.spawn_cloud_rename_playlist(pl_id, next_name.clone());
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
            self.mark_session_dirty();
            if let Some(hist) = &mut self.nav_history {
                hist.prune_detail_id(&pl_id);
            }
            self.spawn_cloud_delete_playlist(pl_id);
            self.refresh_playlists_grid(cx);
            self.rebuild_library_views(cx);
            self.navigate_to(cx, MainPage::Playlists, false);
            self.show_toast(cx, &format!("Deleted playlist '{pl_title}'"));
        }

        // Add Local Music Folder
        if self.ui.button(id!(add_folder_btn)).clicked(actions)
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
                        if self.is_controlling_remote()
                            && let Some(engine) = &self.connect_engine
                        {
                            let ctx = if self.active_page == MainPage::Search
                                && clicked.youtube_video_id.is_some()
                                && self.session_state.autoplay_radio
                            {
                                format!("{} radio", clicked.title)
                            } else {
                                match self.active_page {
                                    MainPage::Search => "Search".to_string(),
                                    MainPage::Recents => "Recents".to_string(),
                                    MainPage::Detail => self.detail_state.title.clone(),
                                    _ => "Library".to_string(),
                                }
                            };
                            let (tracks_to_send, send_idx) = if self.active_page == MainPage::Search
                                && clicked.youtube_video_id.is_some()
                                && self.session_state.autoplay_radio
                            {
                                (vec![QueueEntry::from_track(&clicked)], 0)
                            } else {
                                let entries: Vec<QueueEntry> = active_list
                                    .iter()
                                    .take(200)
                                    .map(QueueEntry::from_track)
                                    .collect();
                                let clamped = idx.min(entries.len().saturating_sub(1));
                                (entries, clamped)
                            };
                            engine.send_remote_action(RemoteAction::PlayQueue {
                                tracks: tracks_to_send,
                                start_index: send_idx,
                                context: ctx,
                            });
                        } else if self.active_page == MainPage::Search
                            && clicked.youtube_video_id.is_some()
                            && self.session_state.autoplay_radio
                        {
                            self.queue_context = format!("{} radio", clicked.title);
                            self.queue.set_tracks(vec![clicked.clone()], Some(0));
                            self.last_radio_seed_id = None;
                            self.play_track(cx, clicked);
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

        // 6. RIGHT SIDEBAR (Queue / Lyrics / Connect Tabs + History + Up Next Jump!)
        if self.ui.button(id!(rs_tab_queue)).clicked(actions) {
            self.set_right_sidebar_mode(cx, true, false, false);
        }
        if self.ui.button(id!(rs_tab_lyrics)).clicked(actions) {
            self.set_right_sidebar_mode(cx, true, true, false);
        }
        if self.ui.button(id!(rs_tab_connect)).clicked(actions) {
            self.set_right_sidebar_mode(cx, true, false, true);
        }
        if self.ui.button(id!(rs_conn_local_btn)).clicked(actions)
            && let Some(engine) = &self.connect_engine
        {
            engine.transfer_active_speaker(engine.local_device_id().to_string());
        }
        for (idx, btn_id) in [
            id!(rs_conn_peer_0),
            id!(rs_conn_peer_1),
            id!(rs_conn_peer_2),
            id!(rs_conn_peer_3),
        ]
        .iter()
        .enumerate()
        {
            if self.ui.button(*btn_id).clicked(actions)
                && !self.connect_peer_ids[idx].is_empty()
                && let Some(engine) = &self.connect_engine
            {
                engine.transfer_active_speaker(self.connect_peer_ids[idx].clone());
            }
        }
        if let Some(cvol) = self
            .ui
            .ferric_bar_slider(id!(rs_conn_vol_slider))
            .changed(actions)
        {
            let vol = (cvol as f32).clamp(0.0, 1.0);
            self.ui
                .ferric_bar_slider(id!(vol_slider))
                .set_value(cx, f64::from(vol));
            self.ui
                .ferric_bar_slider(id!(fs_vol_slider))
                .set_value(cx, f64::from(vol));
            if self.is_controlling_remote() {
                if self
                    .last_conn_vol_sent_at
                    .is_none_or(|t| t.elapsed() >= Duration::from_millis(90))
                    && let Some(peer) = self.active_remote_peer().cloned()
                    && let Some(engine) = &self.connect_engine
                {
                    self.last_conn_vol_sent_at = Some(Instant::now());
                    engine.send_remote_action(RemoteAction::SetVolume {
                        target_device: peer.device_id,
                        volume_norm: vol,
                    });
                }
            } else {
                self.session_state.volume = vol;
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::SetVolume(vol));
                }
                self.sync_playback_controls_only(cx);
            }
        }
        if let Some(cvol) = self
            .ui
            .ferric_bar_slider(id!(rs_conn_vol_slider))
            .released(actions)
        {
            let vol = (cvol as f32).clamp(0.0, 1.0);
            if self.is_controlling_remote()
                && let Some(peer) = self.active_remote_peer().cloned()
                && let Some(engine) = &self.connect_engine
            {
                self.last_conn_vol_sent_at = Some(Instant::now());
                engine.send_remote_action(RemoteAction::SetVolume {
                    target_device: peer.device_id,
                    volume_norm: vol,
                });
            } else {
                self.session_state.volume = vol;
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::SetVolume(vol));
                }
                self.mark_session_dirty();
                self.push_connect_playback_snapshot();
            }
        }
        if self.ui.button(id!(rs_conn_qr_btn)).clicked(actions) {
            self.navigate_to(cx, MainPage::Settings, false);
            self.switch_settings_tab(cx, SettingsTab::Connect);
        }
        if self.ui.button(id!(rs_radio_btn)).clicked(actions) {
            self.trigger_icon_pulse(cx, id!(rs_radio_btn), 0.0);
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
        if self.ui.button(id!(rs_save_queue_btn)).clicked(actions) {
            self.save_queue_as_playlist(cx);
        }
        if self.ui.button(id!(rs_copy_queue_btn)).clicked(actions) {
            self.copy_queue_tracklist(cx);
        }
        if self.ui.button(id!(rs_clear_btn)).clicked(actions) {
            if self.is_controlling_remote() {
                if let Some(engine) = &self.connect_engine {
                    engine.send_remote_action(RemoteAction::ClearUpNext);
                }
                self.show_toast(cx, "Up next queue cleared");
            } else {
                if let Some(cur) = self.queue.current().cloned() {
                    self.queue.set_tracks(vec![cur], Some(0));
                } else {
                    self.queue.set_tracks(Vec::new(), None);
                }
                self.sync_next_deck_to_engine();
                self.refresh_right_sidebar(cx);
                self.push_connect_playback_snapshot();
                self.show_toast(cx, "Up next queue cleared");
            }
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
            if (rv.ferric_cover_art(id!(art)).clicked(actions) || view_clicked(&rv, actions))
                && let Some(t) = hist_tracks.get(i).cloned()
            {
                if self.queue.current().is_none() {
                    if t.youtube_video_id.is_some() && self.session_state.autoplay_radio {
                        self.queue_context = format!("{} radio", t.title);
                        self.queue.set_tracks(vec![t.clone()], Some(0));
                        self.last_radio_seed_id = None;
                    } else {
                        self.queue_context = "History".to_string();
                        let mut rebuilt = vec![t.clone()];
                        for cand in self.home_quick_picks_pool() {
                            if cand.id != t.id && !rebuilt.iter().any(|x| x.id == cand.id) {
                                rebuilt.push(cand);
                                if rebuilt.len() >= 25 {
                                    break;
                                }
                            }
                        }
                        self.queue.set_tracks(rebuilt, Some(0));
                        self.last_radio_seed_id = None;
                    }
                    self.play_track(cx, t);
                } else {
                    self.queue.insert_play_next(t);
                    if let Some(next) = self.queue.jump_to_upcoming(0).cloned() {
                        self.play_track(cx, next);
                    }
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
                if !self.is_controlling_remote() && self.queue.move_upcoming_up(i) {
                    self.sync_next_deck_to_engine();
                    self.refresh_right_sidebar(cx);
                    self.push_connect_playback_snapshot();
                }
                continue;
            }
            if rv.button(id!(rm_btn)).clicked(actions) {
                if self.is_controlling_remote() {
                    if let Some(engine) = &self.connect_engine {
                        engine.send_remote_action(RemoteAction::RemoveQueueAt(i));
                    }
                    self.show_toast(cx, "Removed track from remote queue");
                } else if let Some(removed) = self.queue.remove_upcoming(i) {
                    self.sync_next_deck_to_engine();
                    self.refresh_right_sidebar(cx);
                    self.push_connect_playback_snapshot();
                    self.show_toast(
                        cx,
                        &format!("Removed '{}' from queue", clip_text(&removed.title, 22)),
                    );
                }
                continue;
            }
            if rv.ferric_cover_art(id!(art)).clicked(actions) || view_clicked(&rv, actions) {
                if self.is_controlling_remote() {
                    if let Some(snap) = &self.connect_snap
                        && let Some(engine) = &self.connect_engine
                    {
                        let target_idx =
                            snap.active_queue_index.saturating_add(1).saturating_add(i);
                        engine.send_remote_action(RemoteAction::JumpToQueueIndex {
                            index: target_idx,
                        });
                    }
                } else if let Some(target) = self.queue.jump_to_upcoming(i).cloned() {
                    self.play_track(cx, target);
                }
            }
        }

        if self.ui.button(id!(np_add_pl_btn)).clicked(actions)
            && let Some(cur) = self.effective_display_track()
        {
            self.open_track_menu(cx, cur);
        }
        if self.ui.button(id!(np_copy_link_btn)).clicked(actions)
            && let Some(cur) = self.effective_display_track()
        {
            self.copy_track_share_link(cx, &cur);
        }
        if view_clicked(&self.ui.view(id!(np_artist_hit)), actions)
            && let Some(cur) = self.effective_display_track()
        {
            self.open_artist_for_name(cx, &cur.artist);
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
            self.trigger_icon_pulse(cx, id!(bar_lyrics_btn), 0.0);
            if self.right_sidebar_open && self.right_lyrics_tab {
                self.set_right_sidebar(cx, false, false);
            } else {
                self.set_right_sidebar(cx, true, true);
            }
        }
        if self.ui.button(id!(bar_queue_btn)).clicked(actions) {
            self.trigger_icon_pulse(cx, id!(bar_queue_btn), 0.0);
            if self.right_sidebar_open && !self.right_lyrics_tab && !self.right_connect_tab {
                self.set_right_sidebar_mode(cx, false, false, false);
            } else {
                self.set_right_sidebar_mode(cx, true, false, false);
            }
        }
        if self.ui.button(id!(bar_connect_btn)).clicked(actions) {
            self.trigger_icon_pulse(cx, id!(bar_connect_btn), 0.0);
            if self.right_sidebar_open && self.right_connect_tab {
                self.set_right_sidebar_mode(cx, false, false, false);
            } else {
                self.set_right_sidebar_mode(cx, true, false, true);
            }
        }
        if self
            .ui
            .ferric_play_btn(id!(play_pause_btn))
            .clicked(actions)
            || self
                .ui
                .ferric_play_btn(id!(fs_play_pause_btn))
                .clicked(actions)
        {
            if self.is_controlling_remote()
                && let Some(engine) = &self.connect_engine
            {
                engine.send_remote_action(RemoteAction::TogglePlayPause);
            } else if let Some(audio) = &self.audio {
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
                    self.push_connect_playback_snapshot();
                }
            }
        }
        if self.ui.button(id!(next_btn)).clicked(actions)
            || self.ui.button(id!(fs_next_btn)).clicked(actions)
        {
            if self.is_controlling_remote()
                && let Some(engine) = &self.connect_engine
            {
                engine.send_remote_action(RemoteAction::SkipNext);
            } else if let Some(next) = self.queue.next_track_manual().cloned() {
                self.play_track(cx, next);
            }
        }
        if self.ui.button(id!(prev_btn)).clicked(actions)
            || self.ui.button(id!(fs_prev_btn)).clicked(actions)
        {
            if self.is_controlling_remote()
                && let Some(engine) = &self.connect_engine
            {
                engine.send_remote_action(RemoteAction::SkipPrev);
            } else if let Some(prev) = self.queue.prev_track().cloned() {
                self.play_track(cx, prev);
            }
        }
        if self.ui.button(id!(shuffle_btn)).clicked(actions)
            || self.ui.button(id!(fs_shuffle_btn)).clicked(actions)
        {
            if self.is_controlling_remote()
                && let Some(peer) = self.active_remote_peer().cloned()
                && let Some(engine) = &self.connect_engine
            {
                engine.send_remote_action(RemoteAction::SetShuffle(!peer.shuffle_enabled));
                self.trigger_icon_pulse(cx, id!(shuffle_btn), 0.0);
                self.trigger_icon_pulse(cx, id!(fs_shuffle_btn), 0.0);
            } else {
                let next_shuf = !self.queue.shuffle;
                self.queue.set_shuffle(next_shuf);
                self.session_state.shuffle = next_shuf;
                self.mark_session_dirty();
                self.sync_next_deck_to_engine();
                self.sync_controls_from_state(cx);
                self.trigger_icon_pulse(cx, id!(shuffle_btn), 0.0);
                self.trigger_icon_pulse(cx, id!(fs_shuffle_btn), 0.0);
                self.refresh_right_sidebar(cx);
                self.push_connect_playback_snapshot();
            }
        }
        if self.ui.ferric_repeat_btn(id!(repeat_btn)).clicked(actions)
            || self
                .ui
                .ferric_repeat_btn(id!(fs_repeat_btn))
                .clicked(actions)
        {
            if self.is_controlling_remote()
                && let Some(peer) = self.active_remote_peer().cloned()
                && let Some(engine) = &self.connect_engine
            {
                let next_rep = peer.repeat_mode.next();
                engine.send_remote_action(RemoteAction::SetRepeat(next_rep));
                self.ui.ferric_repeat_btn(id!(repeat_btn)).trigger_anim(cx);
                self.ui
                    .ferric_repeat_btn(id!(fs_repeat_btn))
                    .trigger_anim(cx);
                self.show_toast(
                    cx,
                    match next_rep {
                        RepeatMode::Off => "Repeat Off",
                        RepeatMode::All => "Repeat All",
                        RepeatMode::One => "Repeat One",
                    },
                );
            } else {
                self.queue.repeat = self.queue.repeat.next();
                self.session_state.repeat = self.queue.repeat;
                self.mark_session_dirty();
                self.sync_next_deck_to_engine();
                self.sync_controls_from_state(cx);
                self.ui.ferric_repeat_btn(id!(repeat_btn)).trigger_anim(cx);
                self.ui
                    .ferric_repeat_btn(id!(fs_repeat_btn))
                    .trigger_anim(cx);
                self.push_connect_playback_snapshot();
                self.show_toast(
                    cx,
                    match self.queue.repeat {
                        RepeatMode::Off => "Repeat Off",
                        RepeatMode::All => "Repeat All",
                        RepeatMode::One => "Repeat One",
                    },
                );
                self.ui.redraw(cx);
            }
        }
        if self.ui.button(id!(dur_btn)).clicked(actions) {
            self.show_remaining_time = !self.show_remaining_time;
            self.last_dur_txt.clear();
        }
        if (self.ui.button(id!(np_like_btn)).clicked(actions)
            || self.ui.button(id!(fs_like_btn)).clicked(actions))
            && let Some(cur) = self.effective_display_track()
        {
            let next_liked = self.toggle_track_like(cx, &cur);
            self.trigger_icon_pulse(cx, id!(np_like_btn), 3.0);
            self.trigger_icon_pulse(cx, id!(fs_like_btn), 3.0);
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
        {
            if self.is_controlling_remote()
                && let Some(peer) = self.active_remote_peer().cloned()
                && let Some(engine) = &self.connect_engine
            {
                if peer.duration_ms > 0 {
                    let target_ms = (pos_ratio * peer.duration_ms as f64) as u64;
                    engine.send_remote_action(RemoteAction::SeekTo {
                        position_ms: target_ms,
                    });
                }
            } else if let Some(audio) = &self.audio {
                let snap = audio.telemetry();
                let dur = snap.duration_ms;
                if dur > 0 {
                    let target_ms = (pos_ratio * dur as f64) as u64;
                    audio.send(EngineCommand::SeekMs(target_ms));
                    self.push_connect_playback_snapshot();
                    if self.video_open
                        && let Some(vp) = &self.video_player
                        && vp.is_open()
                    {
                        vp.sync_playback(target_ms as f64 / 1000.0, snap.is_playing);
                    }
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
            if self.is_controlling_remote() {
                self.ui
                    .ferric_bar_slider(id!(vol_slider))
                    .set_value(cx, vol);
                self.ui
                    .ferric_bar_slider(id!(fs_vol_slider))
                    .set_value(cx, vol);
                self.ui
                    .ferric_bar_slider(id!(rs_conn_vol_slider))
                    .set_value(cx, vol);
                if self
                    .last_conn_vol_sent_at
                    .is_none_or(|t| t.elapsed() >= Duration::from_millis(90))
                    && let Some(peer) = self.active_remote_peer().cloned()
                    && let Some(engine) = &self.connect_engine
                {
                    self.last_conn_vol_sent_at = Some(Instant::now());
                    engine.send_remote_action(RemoteAction::SetVolume {
                        target_device: peer.device_id,
                        volume_norm: (vol as f32).clamp(0.0, 1.0),
                    });
                }
            } else {
                let was_muted = self.session_state.volume < 0.01;
                self.session_state.volume = vol as f32;
                let now_muted = self.session_state.volume < 0.01;
                if self.session_state.volume > 0.02 {
                    self.pre_mute_volume = self.session_state.volume;
                }
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::SetVolume(vol as f32));
                }
                self.sync_playback_controls_only(cx);
                if was_muted != now_muted {
                    self.ui
                        .ferric_volume_btn(id!(vol_mute_btn))
                        .trigger_anim(cx);
                    self.ui
                        .ferric_volume_btn(id!(fs_vol_mute_btn))
                        .trigger_anim(cx);
                }
            }
        }
        if let Some(vol) = self
            .ui
            .ferric_bar_slider(id!(vol_slider))
            .released(actions)
            .or_else(|| {
                self.ui
                    .ferric_bar_slider(id!(fs_vol_slider))
                    .released(actions)
            })
        {
            if self.is_controlling_remote()
                && let Some(peer) = self.active_remote_peer().cloned()
                && let Some(engine) = &self.connect_engine
            {
                engine.send_remote_action(RemoteAction::SetVolume {
                    target_device: peer.device_id,
                    volume_norm: (vol as f32).clamp(0.0, 1.0),
                });
            } else {
                self.session_state.volume = vol as f32;
                if self.session_state.volume > 0.02 {
                    self.pre_mute_volume = self.session_state.volume;
                }
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::SetVolume(vol as f32));
                }
                self.mark_session_dirty();
                self.push_connect_playback_snapshot();
            }
        }
        if self
            .ui
            .ferric_volume_btn(id!(vol_mute_btn))
            .clicked(actions)
            || self
                .ui
                .ferric_volume_btn(id!(fs_vol_mute_btn))
                .clicked(actions)
        {
            if self.is_controlling_remote()
                && let Some(peer) = self.active_remote_peer().cloned()
                && let Some(engine) = &self.connect_engine
            {
                let next_v = if peer.volume_norm > 0.01 {
                    0.0_f32
                } else {
                    0.85_f32
                };
                engine.send_remote_action(RemoteAction::SetVolume {
                    target_device: peer.device_id,
                    volume_norm: next_v,
                });
                self.ui
                    .ferric_volume_btn(id!(vol_mute_btn))
                    .trigger_anim(cx);
                self.ui
                    .ferric_volume_btn(id!(fs_vol_mute_btn))
                    .trigger_anim(cx);
            } else {
                if self.session_state.volume > 0.01 {
                    self.pre_mute_volume = self.session_state.volume;
                    self.session_state.volume = 0.0;
                } else {
                    self.session_state.volume = self.pre_mute_volume.max(0.5);
                }
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::SetVolume(self.session_state.volume));
                }
                self.mark_session_dirty();
                self.push_connect_playback_snapshot();
                self.sync_controls_from_state(cx);
                self.ui
                    .ferric_volume_btn(id!(vol_mute_btn))
                    .trigger_anim(cx);
                self.ui
                    .ferric_volume_btn(id!(fs_vol_mute_btn))
                    .trigger_anim(cx);
                self.ui.redraw(cx);
            }
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
        {
            if self.is_controlling_remote()
                && let Some(engine) = &self.connect_engine
            {
                engine.send_remote_action(RemoteAction::SeekTo {
                    position_ms: seek_ms,
                });
                self.update_lyrics_display(cx, seek_ms);
            } else if let Some(audio) = &self.audio {
                let playing = audio.telemetry().is_playing;
                audio.send(EngineCommand::SeekMs(seek_ms));
                self.push_connect_playback_snapshot();
                self.update_lyrics_display(cx, seek_ms);
                if self.video_open
                    && let Some(vp) = &self.video_player
                    && vp.is_open()
                {
                    vp.sync_playback(seek_ms as f64 / 1000.0, playing);
                }
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
            self.trigger_icon_pulse(cx, id!(bar_video_btn), 0.0);
            if self.video_open && self.fullscreen_open {
                self.close_native_video_stage(cx);
            } else {
                self.open_native_video_stage(cx);
            }
        }
        if (self.ui.button(id!(rs_watch_video)).clicked(actions)
            || self.ui.button(id!(fs_mode_video_btn)).clicked(actions))
            && !(self.fullscreen_open && self.video_open)
        {
            self.open_native_video_stage(cx);
        }
        if self.ui.button(id!(fs_mode_art_btn)).clicked(actions) && self.video_open {
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
        if self.ui.button(id!(cmdk_copy_link_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            if let Some(cur) = self.queue.current().cloned() {
                self.copy_track_share_link(cx, &cur);
            } else {
                self.show_toast(cx, "No song is currently playing");
            }
        }
        if self.ui.button(id!(cmdk_copy_pl_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            if let Some((platform, url)) = self.detail_share_link() {
                let _ = copy_text_to_clipboard(&url);
                self.show_toast(
                    cx,
                    &format!(
                        "Copied {platform} link: {}",
                        clip_text(&self.detail_state.title, 26)
                    ),
                );
            } else {
                self.show_toast(
                    cx,
                    "Open a Spotify or YouTube playlist first to copy its link",
                );
            }
        }
        if self.ui.button(id!(cmdk_save_q_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.save_queue_as_playlist(cx);
        }
        if self.ui.button(id!(cmdk_about_btn)).clicked(actions) {
            self.cmdk_open = false;
            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
            self.navigate_to(cx, MainPage::Settings, true);
            self.switch_settings_tab(cx, SettingsTab::About);
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
            (id!(st_about_btn), SettingsTab::About),
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
            self.accounts.zeroize_youtube_secrets();
            self.accounts.youtube_connected = false;
            self.accounts.youtube_channel_name.clear();
            self.accounts.youtube_account_id.clear();
            self.accounts.youtube_visitor_data.clear();
            self.accounts.youtube_playlist_count = 0;
            self.sync_connect_accounts();
            // Offload WebView2 login profile removal to a background thread so UI stays responsive.
            let profile_dir = app_data_dir().join("webview2_profile");
            thread::spawn(move || {
                let _ = std::fs::remove_dir_all(profile_dir);
            });
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
            self.show_toast(cx, "Signed out. Guest Mode active");
        }
        if self.ui.button(id!(spotify_web_login_btn)).clicked(actions) {
            self.start_spotify_web_login(cx);
        }
        if self.ui.button(id!(spotify_web_sync_btn)).clicked(actions) {
            if self.accounts.spotify_web_access_token.trim().is_empty() {
                self.start_spotify_web_login(cx);
            } else {
                self.sync_spotify_library_for(cx, true, 0);
            }
        }
        if self.ui.button(id!(spotify_web_logout_btn)).clicked(actions) {
            if !self.is_spotify_web_session_active() && !self.is_spotify_oauth_session_active() {
                self.ui
                    .label(id!(spotify_web_status_lbl))
                    .set_text(cx, "Spotify is not signed in.");
            } else {
                self.accounts.zeroize_spotify_web_secrets();
                self.accounts.zeroize_spotify_direct_secrets();
                self.accounts.spotify_web_connected = false;
                self.accounts.spotify_web_user_name.clear();
                self.accounts.spotify_connected = false;
                self.accounts.spotify_user_name.clear();
                self.accounts.spotify_user_id.clear();
                self.accounts.spotify_playlist_count = 0;
                self.accounts.spotify_liked_count = 0;
                self.sync_connect_accounts();
                // Clean up Spotify Web browser profile and any legacy capture file off the UI thread.
                let sp_profile_dir = app_data_dir().join("spotify_web_profile");
                let legacy_capture = std::env::temp_dir().join("ferric_spotify_capture.txt");
                thread::spawn(move || {
                    let _ = std::fs::remove_dir_all(sp_profile_dir);
                    let _ = std::fs::remove_file(legacy_capture);
                });
                if let Some(db) = &mut self.db {
                    let _ = db.save_accounts(&self.accounts);
                    let _ = db.delete_spotify_web_synced_playlists();
                    let _ = db.delete_spotify_premium_synced_playlists();
                    self.playlists = db.list_playlists().unwrap_or_default();
                } else {
                    self.playlists.retain(|pl| {
                        !pl.id.starts_with("spotify:playlist:")
                            && !pl.id.starts_with("spotify:premium:playlist:")
                    });
                }
                self.ui.label(id!(spotify_web_status_lbl)).set_text(
                    cx,
                    "Spotify signed out. Synced Spotify playlists removed; liked tracks kept.",
                );
                self.refresh_spotify_account_ui(cx);
                self.rebuild_library_views(cx);
                self.refresh_playlists_grid(cx);
                self.show_toast(cx, "Spotify signed out");
            }
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
        if self.ui.button(id!(rescan_btn)).clicked(actions) {
            let folders = self
                .db
                .as_ref()
                .and_then(|db| db.list_music_folders().ok())
                .unwrap_or_default();
            if folders.is_empty() {
                self.show_toast(cx, "No local music folders added yet");
            } else {
                self.show_toast(cx, &format!("Rescanning {} folder(s)...", folders.len()));
                self.spawn_folder_scan(folders);
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

        // Playback & Fade-In Tab
        if self.ui.button(id!(pb_device_refresh_btn)).clicked(actions) {
            self.output_devices = list_output_devices();
            self.refresh_output_device_ui(cx);
            self.show_toast(
                cx,
                &format!("Found {} audio output device(s)", self.output_devices.len()),
            );
        }
        if self.ui.button(id!(pb_device_cycle_btn)).clicked(actions) {
            if self.output_devices.is_empty() {
                self.output_devices = list_output_devices();
            }
            let next_dev = match self.dsp.output_device.as_deref() {
                None => self.output_devices.first().cloned(),
                Some(cur) => {
                    let pos = self
                        .output_devices
                        .iter()
                        .position(|d| d.eq_ignore_ascii_case(cur));
                    match pos {
                        Some(idx) if idx + 1 < self.output_devices.len() => {
                            Some(self.output_devices[idx + 1].clone())
                        }
                        _ => None,
                    }
                }
            };
            self.select_output_device(cx, next_dev);
        }
        if self.ui.button(id!(pb_dev_default)).clicked(actions) {
            self.select_output_device(cx, None);
        }
        for (i, cid) in [id!(pb_dev_0), id!(pb_dev_1), id!(pb_dev_2), id!(pb_dev_3)]
            .into_iter()
            .enumerate()
        {
            if self.ui.button(cid).clicked(actions)
                && let Some(dev_name) = self.output_devices.get(i).cloned()
            {
                self.select_output_device(cx, Some(dev_name));
            }
        }
        if let Some(sec) = self
            .ui
            .ferric_bar_slider(id!(xfade_slider))
            .changed(actions)
        {
            self.dsp.crossfade_ms = (sec * 1_000.0).round() as u32;
            self.ui
                .label(id!(xfade_val_lbl))
                .set_text(cx, &format!("{sec:.1} s"));
            self.apply_dsp_live_inner(cx, false);
        }
        if let Some(sec) = self
            .ui
            .ferric_bar_slider(id!(xfade_slider))
            .released(actions)
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
            self.apply_dsp_live_inner(cx, false);
        }
        if let Some(ms) = self
            .ui
            .ferric_bar_slider(id!(fadein_slider))
            .released(actions)
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
            if on && let Some(cur) = self.queue.current().cloned() {
                self.maybe_fetch_radio_for_track(&cur, true);
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
        if self.ui.button(id!(speed_pill_btn)).clicked(actions) {
            let next_spd = if (self.dsp.playback_speed - 0.75).abs() < 0.05 {
                1.0_f32
            } else if (self.dsp.playback_speed - 1.0).abs() < 0.05 {
                1.25_f32
            } else if (self.dsp.playback_speed - 1.25).abs() < 0.05 {
                1.5_f32
            } else {
                0.75_f32
            };
            self.dsp.playback_speed = next_spd;
            self.sync_controls_from_state(cx);
            self.apply_dsp_live(cx);
            let lbl = if (next_spd - 0.75).abs() < 0.05 {
                "0.75x"
            } else if (next_spd - 1.25).abs() < 0.05 {
                "1.25x"
            } else if (next_spd - 1.50).abs() < 0.05 {
                "1.5x"
            } else {
                "1.0x"
            };
            self.show_toast(cx, &format!("Playback speed: {lbl}"));
        }
        for (btn, mins_opt) in [
            (id!(slp_off_btn), None),
            (id!(slp_15_btn), Some(15_u64)),
            (id!(slp_30_btn), Some(30_u64)),
            (id!(slp_60_btn), Some(60_u64)),
        ] {
            if self.ui.button(btn).clicked(actions) {
                if self.sleep_fade_base_vol.take().is_some()
                    && let Some(a) = &self.audio
                {
                    a.send(EngineCommand::SetVolume(self.session_state.volume));
                }
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
                self.sync_next_deck_to_engine();
            }
        }
        if self.ui.button(id!(slp_eot_btn)).clicked(actions) {
            if self.sleep_fade_base_vol.take().is_some()
                && let Some(a) = &self.audio
            {
                a.send(EngineCommand::SetVolume(self.session_state.volume));
            }
            self.sleep_timer = SleepTimerMode::EndOfTrack;
            self.sync_next_deck_to_engine();
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
            self.apply_eq_live_inner(cx, false);
        }
        if let Some(pre) = self
            .ui
            .ferric_bar_slider(id!(eq_preamp_slider))
            .released(actions)
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
                self.apply_eq_live_inner(cx, false);
            }
            if let Some(val) = self.ui.ferric_vert_slider(slider_id).released(actions)
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
            } else {
                self.show_toast(cx, "No matching AutoEq headphone profile found");
            }
        }
        if self.ui.button(id!(eq_import_btn)).clicked(actions)
            && let Some(file) = rfd::FileDialog::new()
                .add_filter("Equalizer APO", &["txt"])
                .pick_file()
        {
            match read_text_file_capped(&file, 256 * 1024)
                .and_then(|raw| EqConfig::from_equalizer_apo(&raw).map_err(|e| e.to_string()))
            {
                Ok(parsed) => {
                    self.eq = parsed;
                    self.sync_controls_from_state(cx);
                    self.apply_eq_live(cx);
                    self.show_toast(cx, "Imported Equalizer APO profile");
                }
                Err(err) => {
                    self.show_toast(cx, &format!("EQ import failed: {err}"));
                }
            }
        }
        if self.ui.button(id!(eq_export_btn)).clicked(actions)
            && let Some(file) = rfd::FileDialog::new()
                .set_file_name("ferric-eq-apo.txt")
                .add_filter("Equalizer APO", &["txt"])
                .save_file()
        {
            match std::fs::write(file, self.eq.to_equalizer_apo()) {
                Ok(()) => self.show_toast(cx, "Exported Equalizer APO profile"),
                Err(err) => self.show_toast(cx, &format!("EQ export failed: {err}")),
            }
        }

        // Appearance Tab (AMOLED Black, Native Windows 11 Acrylic, Obsidian Studio, Warm Cream)
        if self.ui.button(id!(pal_amoled_btn)).clicked(actions) {
            let z = self.theme.ui_zoom;
            self.theme = ThemeConfig::pure_amoled();
            self.theme.ui_zoom = z;
            self.displayed_accent = self.target_accent();
            self.displayed_acrylic_tint = self.target_acrylic_tint();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(pal_acrylic_btn)).clicked(actions) {
            let z = self.theme.ui_zoom;
            self.theme = ThemeConfig::native_acrylic();
            self.theme.ui_zoom = z;
            self.displayed_accent = self.target_accent();
            self.displayed_acrylic_tint = self.target_acrylic_tint();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
            self.show_toast(cx, "Native Windows 11 Acrylic enabled");
        }
        if self.ui.button(id!(pal_obsidian_btn)).clicked(actions) {
            let z = self.theme.ui_zoom;
            self.theme = ThemeConfig::obsidian_slate();
            self.theme.ui_zoom = z;
            self.displayed_accent = self.target_accent();
            self.displayed_acrylic_tint = self.target_acrylic_tint();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if self.ui.button(id!(pal_cream_btn)).clicked(actions) {
            let z = self.theme.ui_zoom;
            self.theme = ThemeConfig::cream_light();
            self.theme.ui_zoom = z;
            self.displayed_accent = self.target_accent();
            self.displayed_acrylic_tint = self.target_acrylic_tint();
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
            self.displayed_accent = self.target_accent();
            self.apply_theme_live(cx);
            self.sync_controls_from_state(cx);
            self.refresh_home_ui(cx);
            self.show_toast(
                cx,
                if on {
                    "Adaptive Artwork Accent enabled"
                } else {
                    "Adaptive Artwork Accent disabled (Burnt Orange)"
                },
            );
        }
        if let Some(enabled) = self.ui.ferric_toggle(id!(acrylic_toggle)).toggled(actions) {
            self.theme.acrylic_enabled = enabled;
            if enabled {
                if self.theme.name != "Warm Cream Light" {
                    self.theme.name = "Native Acrylic".to_string();
                } else if self.theme.acrylic_tint_hex.eq_ignore_ascii_case("#0B0D14") {
                    self.theme.acrylic_tint_hex = "#EFE8DA".to_string();
                }
            } else if self.theme.name == "Native Acrylic" {
                self.theme.name = if self.theme.canvas_hex.eq_ignore_ascii_case("#F3EEE3") {
                    "Warm Cream Light".to_string()
                } else if self.theme.canvas_hex.eq_ignore_ascii_case("#0D0E12") {
                    "Obsidian Studio".to_string()
                } else {
                    "AMOLED Black".to_string()
                };
            }
            self.displayed_accent = self.target_accent();
            self.displayed_acrylic_tint = self.target_acrylic_tint();
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
        }
        if let Some(on) = self
            .ui
            .ferric_toggle(id!(adaptive_acrylic_tint_toggle))
            .toggled(actions)
        {
            self.theme.adaptive_acrylic_tint = on;
            if on && !self.theme.acrylic_enabled {
                self.theme.acrylic_enabled = true;
                self.theme.name = "Native Acrylic".to_string();
            }
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
            self.show_toast(
                cx,
                if on {
                    "Adaptive Acrylic Tint enabled: follows playing cover"
                } else {
                    "Adaptive Acrylic Tint disabled (using manual glass tint)"
                },
            );
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
            self.apply_theme_live_inner(cx, false);
        }
        if let Some(v) = self
            .ui
            .ferric_bar_slider(id!(acrylic_opacity_slider))
            .released(actions)
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
                self.theme.adaptive_acrylic_tint = false;
                self.displayed_acrylic_tint = rgba_vec4(hex);
                if !self.theme.acrylic_enabled {
                    self.theme.acrylic_enabled = true;
                    self.theme.name = "Native Acrylic".to_string();
                }
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
                self.show_toast(cx, &format!("Acrylic tint: {name} ({hex})"));
            }
        }
        if let Some(picked_hex) = self
            .ui
            .ferric_color_picker(id!(acrylic_color_picker))
            .changed(actions)
        {
            self.theme.acrylic_tint_hex = picked_hex.clone();
            self.theme.adaptive_acrylic_tint = false;
            if !self.theme.acrylic_enabled {
                self.theme.acrylic_enabled = true;
                self.theme.name = "Native Acrylic".to_string();
            }
            let tint_v4 = rgba_vec4(&picked_hex);
            self.displayed_acrylic_tint = tint_v4;
            self.ui
                .label(id!(acrylic_tint_lbl))
                .set_text(cx, &picked_hex);
            self.ui
                .view(id!(acrylic_tint_preview))
                .apply_over(cx, live! { draw_bg: { tint_col: (tint_v4) } });
            self.ui
                .text_input(id!(acrylic_tint_input))
                .set_text(cx, &picked_hex);
            self.apply_theme_live_inner(cx, false);
        }
        if let Some(picked_hex) = self
            .ui
            .ferric_color_picker(id!(acrylic_color_picker))
            .committed(actions)
        {
            self.theme.acrylic_tint_hex = picked_hex.clone();
            self.theme.adaptive_acrylic_tint = false;
            self.displayed_acrylic_tint = rgba_vec4(&picked_hex);
            if !self.theme.acrylic_enabled {
                self.theme.acrylic_enabled = true;
                self.theme.name = "Native Acrylic".to_string();
            }
            self.sync_controls_from_state(cx);
            self.apply_theme_live(cx);
            self.show_toast(cx, &format!("Acrylic tint: {picked_hex}"));
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
                self.theme.adaptive_acrylic_tint = false;
                self.displayed_acrylic_tint = rgba_vec4(&formatted);
                if !self.theme.acrylic_enabled {
                    self.theme.acrylic_enabled = true;
                    self.theme.name = "Native Acrylic".to_string();
                }
                self.sync_controls_from_state(cx);
                self.apply_theme_live(cx);
                self.show_toast(cx, &format!("Custom Acrylic tint applied: {formatted}"));
            } else {
                self.show_toast(cx, "Invalid hex color: use #RRGGBB (e.g. #2B140A)");
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
                    if !effective_client_id(&self.discord_settings).is_empty() {
                        "Discord Rich Presence enabled"
                    } else {
                        "Discord RPC enabled. Paste your Discord Application ID"
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
            .ferric_toggle(id!(discord_button_toggle))
            .toggled(actions)
        {
            self.discord_settings.show_listen_button = on;
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
                        "Using built-in Ferric Discord Application ID. Connecting..."
                    } else {
                        "Custom Discord Application ID saved. Connecting..."
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
            match self.theme.export_to_file(&file) {
                Ok(()) => self.show_toast(cx, "Exported theme JSON"),
                Err(err) => self.show_toast(cx, &format!("Theme export failed: {err}")),
            }
        }
        if self.ui.button(id!(theme_import_btn)).clicked(actions)
            && let Some(file) = rfd::FileDialog::new()
                .add_filter("Ferric Theme JSON", &["json"])
                .pick_file()
        {
            match ThemeConfig::import_from_file(&file) {
                Ok(loaded) => {
                    self.theme = loaded;
                    self.sync_controls_from_state(cx);
                    self.apply_theme_live(cx);
                    self.show_toast(cx, &format!("Imported theme '{}'", self.theme.name));
                }
                Err(err) => {
                    self.show_toast(cx, &format!("Theme import failed: {err}"));
                }
            }
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
            let playing_local = self
                .queue
                .current()
                .is_some_and(|t| t.source == TrackSource::Local);
            let local_ids: Vec<String> = self
                .queue
                .tracks()
                .iter()
                .filter(|t| t.source == TrackSource::Local)
                .map(|t| t.id.clone())
                .collect();
            let all_local_ids: std::collections::HashSet<String> = self
                .all_tracks
                .iter()
                .filter(|t| t.source == TrackSource::Local)
                .map(|t| t.id.clone())
                .collect();
            for lid in &local_ids {
                let _ = self.queue.remove_track_by_id(lid);
            }
            if playing_local {
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::Stop);
                }
                if let Some(next) = self.queue.current().cloned() {
                    self.play_track(cx, next);
                } else {
                    self.clear_now_playing_ui(cx);
                }
            }
            let removed = if let Some(db) = &mut self.db {
                let n = db.clear_music_folders_and_local_tracks().unwrap_or(0);
                self.all_tracks = db.list_tracks("").unwrap_or_default();
                self.playlists = db.list_playlists().unwrap_or_default();
                n
            } else {
                let before = self.all_tracks.len();
                self.all_tracks.retain(|t| t.source != TrackSource::Local);
                before.saturating_sub(self.all_tracks.len())
            };
            self.session_state
                .recent_track_ids
                .retain(|id| !all_local_ids.contains(id));
            if let Some(db) = &self.db {
                let _ = db.save_session_state(&self.session_state);
            }
            self.sync_next_deck_to_engine();
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

        // About Tab Links, Support & System Info
        for (btn_id, url) in [
            (id!(about_web_btn), "https://livelydesk.vercel.app/ferric"),
            (id!(about_github_btn), "https://github.com/Raflyf/Ferric"),
            (
                id!(about_changelog_btn),
                "https://github.com/Raflyf/Ferric/blob/main/CHANGELOG.md",
            ),
            (
                id!(about_issues_btn),
                "https://github.com/Raflyf/Ferric/issues",
            ),
            (
                id!(about_license_btn),
                "https://github.com/Raflyf/Ferric/blob/main/LICENSE",
            ),
            (id!(about_coffee_btn), "https://yapp.ink/raflyf"),
            (id!(about_saweria_btn), "https://saweria.co/BuyMatcha"),
        ] {
            if self.ui.button(btn_id).clicked(actions)
                && let Err(err) = open_allowed_url(url)
            {
                self.show_toast(cx, &format!("Could not open link: {err}"));
            }
        }
        if self.ui.button(id!(about_copy_diag_btn)).clicked(actions) {
            let diag = format!(
                "Ferric v{APP_VERSION} ({} {}) | Theme: {} | Acrylic: {} | UI Zoom: {:.0}% | Library: {} tracks, {} playlists",
                std::env::consts::OS,
                std::env::consts::ARCH,
                self.theme.name,
                if self.theme.acrylic_enabled {
                    "On"
                } else {
                    "Off"
                },
                self.theme.ui_zoom * 100.0,
                self.all_tracks.len(),
                self.playlists.len(),
            );
            let _ = copy_text_to_clipboard(&diag);
            self.show_toast(cx, "System info copied to clipboard");
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
                self.play_track(cx, target);
            } else if self.ui.button(id!(tm_play_next_btn)).clicked(actions) {
                self.close_track_menu(cx);
                let title = clip_text(&target.title, 22);
                if self.is_controlling_remote() {
                    if let Some(engine) = &self.connect_engine {
                        engine.send_remote_action(RemoteAction::PlayNextTrack(
                            QueueEntry::from_track(&target),
                        ));
                    }
                } else {
                    self.queue.insert_play_next(target);
                    self.sync_next_deck_to_engine();
                    self.prefetch_next_online_track();
                    self.refresh_right_sidebar(cx);
                    self.push_connect_playback_snapshot();
                }
                self.show_toast(cx, &format!("Playing next: '{title}'"));
            } else if self.ui.button(id!(tm_add_queue_btn)).clicked(actions) {
                self.close_track_menu(cx);
                let title = clip_text(&target.title, 22);
                if self.is_controlling_remote() {
                    if let Some(engine) = &self.connect_engine {
                        engine.send_remote_action(RemoteAction::AppendQueueTrack(
                            QueueEntry::from_track(&target),
                        ));
                    }
                } else {
                    self.queue.append_single_to_end(target);
                    self.sync_next_deck_to_engine();
                    self.refresh_right_sidebar(cx);
                    self.push_connect_playback_snapshot();
                }
                self.show_toast(cx, &format!("Added '{title}' to end of queue"));
            } else if self.ui.button(id!(tm_radio_btn)).clicked(actions) {
                self.close_track_menu(cx);
                self.queue_context = format!("{} radio", target.title);
                self.queue.set_tracks(vec![target.clone()], Some(0));
                self.last_radio_seed_id = None;
                self.play_track(cx, target.clone());
                if !self.session_state.autoplay_radio {
                    self.maybe_fetch_radio_for_track(&target, true);
                }
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
                self.open_artist_for_name(cx, &artist);
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
            } else if self.ui.button(id!(tm_lyrics_btn)).clicked(actions) {
                self.close_track_menu(cx);
                let is_current = self.queue.current().is_some_and(|cur| cur.id == target.id);
                if !is_current {
                    self.play_track(cx, target);
                }
                self.set_right_sidebar(cx, true, true);
            } else if self.ui.button(id!(tm_copy_link_btn)).clicked(actions) {
                self.copy_track_share_link(cx, &target);
                self.close_track_menu(cx);
            } else if self.ui.button(id!(tm_copy_alt_link_btn)).clicked(actions) {
                if let Some(yt_url) = track_youtube_link(&target) {
                    let _ = copy_text_to_clipboard(&yt_url);
                    self.show_toast(
                        cx,
                        &format!("Copied YouTube link: {}", clip_text(&target.title, 26)),
                    );
                }
                self.close_track_menu(cx);
            } else if self.ui.button(id!(tm_open_source_btn)).clicked(actions) {
                self.close_track_menu(cx);
                if let Some(path) = target.path.as_deref() {
                    match reveal_file_in_explorer(path) {
                        Ok(()) => self.show_toast(cx, "Opened file location in Explorer"),
                        Err(err) => self.show_toast(cx, &err),
                    }
                } else if let Some((platform, url)) = track_primary_link(&target) {
                    match open_allowed_url(&url) {
                        Ok(()) => self.show_toast(cx, &format!("Opened in {platform}")),
                        Err(err) => self.show_toast(cx, &err),
                    }
                }
            } else if self.ui.button(id!(tm_copy_btn)).clicked(actions) {
                let title_line = format!("{} - {}", target.artist, target.title);
                let _ = copy_text_to_clipboard(&title_line);
                self.close_track_menu(cx);
                self.show_toast(cx, "Copied Artist - Title to clipboard");
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
                        let local_id = format!("local:pl:{}", csprng_hex(6));
                        let pl = Playlist {
                            id: local_id.clone(),
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
                        self.spawn_cloud_create_playlist(
                            local_id,
                            name.clone(),
                            "Custom playlist".to_string(),
                            vec![target.clone()],
                        );
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
            win32_install_window_hooks();
            self.hide_native_makepad_caption(cx);
            let is_max = win32_is_active_window_maximized();
            self.ui
                .ferric_win_btn(id!(win_max_btn))
                .set_maximized(cx, is_max);
            self.sync_maximized_chrome_padding(cx, is_max);
            self.sync_video_bounds(cx);
        }

        // Modal Backdrop Isolation:
        // When any modal popup is open, block all pointer interaction with the background UI,
        // and dismiss the popup cleanly whenever the user clicks outside the modal card.
        let active_modal: Option<(&'static [LiveId], &'static [LiveId])> = if self.track_menu_open {
            Some((id!(track_menu_modal), id!(card_track_menu)))
        } else if self.cmdk_open {
            Some((id!(cmdk_modal), id!(card_cmdk)))
        } else if self.update_popup_visible {
            Some((id!(update_popup), id!(card_update_popup)))
        } else {
            None
        };

        if self.modal_dismissed_swallow_up && matches!(event, Event::MouseUp(_)) {
            self.modal_dismissed_swallow_up = false;
            return;
        }

        if let Some((modal_id, card_id)) = active_modal {
            match event {
                Event::WindowDragQuery(_) => {
                    // Never drag the window while a modal backdrop is active
                    return;
                }
                Event::MouseDown(md) => {
                    let card_r = self.ui.view(card_id).area().rect(cx);
                    if card_r.size.x > 0.0 && !card_r.contains(md.abs) {
                        if self.track_menu_open {
                            self.close_track_menu(cx);
                        } else if self.cmdk_open {
                            self.cmdk_open = false;
                            self.ui.view(id!(cmdk_modal)).set_visible(cx, false);
                            self.sync_video_bounds(cx);
                            self.ui.redraw(cx);
                        } else if self.update_popup_visible {
                            self.dismiss_update_popup(cx);
                        }
                        self.modal_dismissed_swallow_up = true;
                        return;
                    }
                    // Click is inside the modal card: route strictly to the modal overlay so
                    // background tracks/buttons underneath can never be clicked through.
                    let mut scope = Scope::empty();
                    self.ui.view(modal_id).handle_event(cx, event, &mut scope);
                    return;
                }
                Event::MouseUp(_) | Event::MouseMove(_) | Event::Scroll(_) => {
                    if let Event::Scroll(se) = event {
                        se.handled_y.set(true);
                        se.handled_x.set(true);
                    }
                    let mut scope = Scope::empty();
                    self.ui.view(modal_id).handle_event(cx, event, &mut scope);
                    return;
                }
                _ => {}
            }
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
                let pill_r = self.ui.view(id!(top_center_grp)).area().rect(cx);
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
                .ferric_volume_btn(id!(vol_mute_btn))
                .area()
                .rect(cx)
                .contains(se.abs);
            let on_fs_vol = self.fullscreen_open
                && self
                    .ui
                    .ferric_volume_btn(id!(fs_vol_mute_btn))
                    .area()
                    .rect(cx)
                    .contains(se.abs);
            if on_bottom_vol || on_fs_vol {
                se.handled_y.set(true);
                let was_muted = self.session_state.volume < 0.01;
                let delta = if se.scroll.y < 0.0 {
                    0.05_f32
                } else {
                    -0.05_f32
                };
                self.session_state.volume = (self.session_state.volume + delta).clamp(0.0, 1.0);
                let now_muted = self.session_state.volume < 0.01;
                if self.session_state.volume > 0.02 {
                    self.pre_mute_volume = self.session_state.volume;
                }
                if let Some(audio) = &self.audio {
                    audio.send(EngineCommand::SetVolume(self.session_state.volume));
                }
                self.mark_session_dirty();
                self.sync_controls_from_state(cx);
                if was_muted != now_muted {
                    self.ui
                        .ferric_volume_btn(id!(vol_mute_btn))
                        .trigger_anim(cx);
                    self.ui
                        .ferric_volume_btn(id!(fs_vol_mute_btn))
                        .trigger_anim(cx);
                }
                return;
            }
        }

        if let Some(nf) = self.motion_frame.is_event(event) {
            self.step_motion(cx, nf.time);
        }

        if self.tick_timer.is_event(event).is_some() || matches!(event, Event::Signal) {
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
            if ke.modifiers.control && ke.key_code == KeyCode::KeyP {
                self.cmdk_open = !self.cmdk_open;
                self.ui
                    .view(id!(cmdk_modal))
                    .set_visible(cx, self.cmdk_open);
                if self.cmdk_open {
                    self.cmdk_reveal_t = 0.0;
                    self.ui.widget(id!(cmdk_input)).set_key_focus(cx);
                    self.start_motion(cx);
                }
                self.sync_video_bounds(cx);
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
            if ke.modifiers.control && ke.modifiers.shift && ke.key_code == KeyCode::KeyC {
                if let Some(cur) = self.queue.current().cloned() {
                    self.copy_track_share_link(cx, &cur);
                } else if self.active_page == MainPage::Detail
                    && let Some((platform, url)) = self.detail_share_link()
                {
                    let _ = copy_text_to_clipboard(&url);
                    self.show_toast(
                        cx,
                        &format!(
                            "Copied {platform} link: {}",
                            clip_text(&self.detail_state.title, 26)
                        ),
                    );
                }
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
                self.navigate_to_entry(cx, prev);
                return;
            }
            if ke.modifiers.alt
                && ke.key_code == KeyCode::ArrowRight
                && let Some(next) = self.nav_history.as_mut().and_then(NavHistory::go_forward)
            {
                self.navigate_to_entry(cx, next);
                return;
            }
            if ke.modifiers.shift && ke.key_code == KeyCode::ArrowRight {
                if self.is_controlling_remote()
                    && let Some(engine) = &self.connect_engine
                {
                    engine.send_remote_action(RemoteAction::SkipNext);
                    return;
                } else if let Some(next) = self.queue.next_track_manual().cloned() {
                    self.play_track(cx, next);
                    return;
                }
            }
            if ke.modifiers.shift && ke.key_code == KeyCode::ArrowLeft {
                if self.is_controlling_remote()
                    && let Some(engine) = &self.connect_engine
                {
                    engine.send_remote_action(RemoteAction::SkipPrev);
                    return;
                } else if let Some(prev) = self.queue.prev_track().cloned() {
                    self.play_track(cx, prev);
                    return;
                }
            }
            if ke.modifiers.control && ke.key_code == KeyCode::ArrowUp {
                if self.is_controlling_remote()
                    && let Some(peer) = self.active_remote_peer().cloned()
                    && let Some(engine) = &self.connect_engine
                {
                    let next_v = (peer.volume_norm + 0.05).min(1.0);
                    engine.send_remote_action(RemoteAction::SetVolume {
                        target_device: peer.device_id,
                        volume_norm: next_v,
                    });
                    return;
                }
                self.session_state.volume = (self.session_state.volume + 0.05).min(1.0);
                if self.session_state.volume > 0.02 {
                    self.pre_mute_volume = self.session_state.volume;
                }
                if let Some(a) = &self.audio {
                    a.send(EngineCommand::SetVolume(self.session_state.volume));
                }
                self.mark_session_dirty();
                self.sync_playback_controls_only(cx);
                return;
            }
            if ke.modifiers.control && ke.key_code == KeyCode::ArrowDown {
                if self.is_controlling_remote()
                    && let Some(peer) = self.active_remote_peer().cloned()
                    && let Some(engine) = &self.connect_engine
                {
                    let next_v = (peer.volume_norm - 0.05).max(0.0);
                    engine.send_remote_action(RemoteAction::SetVolume {
                        target_device: peer.device_id,
                        volume_norm: next_v,
                    });
                    return;
                }
                self.session_state.volume = (self.session_state.volume - 0.05).max(0.0);
                if self.session_state.volume > 0.02 {
                    self.pre_mute_volume = self.session_state.volume;
                }
                if let Some(a) = &self.audio {
                    a.send(EngineCommand::SetVolume(self.session_state.volume));
                }
                self.mark_session_dirty();
                self.sync_playback_controls_only(cx);
                return;
            }
            let typing = self.is_text_input_focused(cx);
            let modal_open = self.cmdk_open || self.track_menu_open || self.update_popup_visible;
            if !typing
                && !modal_open
                && !ke.modifiers.control
                && !ke.modifiers.alt
                && !ke.modifiers.shift
            {
                if ke.key_code == KeyCode::Space {
                    if self.is_controlling_remote()
                        && let Some(engine) = &self.connect_engine
                    {
                        engine.send_remote_action(RemoteAction::TogglePlayPause);
                        return;
                    }
                    if let Some(audio) = &self.audio {
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
                        return;
                    }
                }
                if ke.key_code == KeyCode::ArrowRight {
                    if self.is_controlling_remote()
                        && let Some(peer) = self.active_remote_peer().cloned()
                        && let Some(engine) = &self.connect_engine
                    {
                        if peer.duration_ms > 0 {
                            let next_ms = peer
                                .interpolated_position_ms()
                                .saturating_add(5_000)
                                .min(peer.duration_ms);
                            engine.send_remote_action(RemoteAction::SeekTo {
                                position_ms: next_ms,
                            });
                            self.update_lyrics_display(cx, next_ms);
                        }
                        return;
                    }
                    if let Some(audio) = &self.audio {
                        let snap = audio.telemetry();
                        if snap.duration_ms > 0 {
                            let next_ms =
                                snap.position_ms.saturating_add(5_000).min(snap.duration_ms);
                            audio.send(EngineCommand::SeekMs(next_ms));
                            self.update_lyrics_display(cx, next_ms);
                        }
                        return;
                    }
                }
                if ke.key_code == KeyCode::ArrowLeft {
                    if self.is_controlling_remote()
                        && let Some(peer) = self.active_remote_peer().cloned()
                        && let Some(engine) = &self.connect_engine
                    {
                        if peer.duration_ms > 0 {
                            let next_ms = peer.interpolated_position_ms().saturating_sub(5_000);
                            engine.send_remote_action(RemoteAction::SeekTo {
                                position_ms: next_ms,
                            });
                            self.update_lyrics_display(cx, next_ms);
                        }
                        return;
                    }
                    if let Some(audio) = &self.audio {
                        let snap = audio.telemetry();
                        if snap.duration_ms > 0 {
                            let next_ms = snap.position_ms.saturating_sub(5_000);
                            audio.send(EngineCommand::SeekMs(next_ms));
                            self.update_lyrics_display(cx, next_ms);
                        }
                        return;
                    }
                }
                if ke.key_code == KeyCode::KeyM {
                    if self.is_controlling_remote()
                        && let Some(peer) = self.active_remote_peer().cloned()
                        && let Some(engine) = &self.connect_engine
                    {
                        let next_v = if peer.volume_norm > 0.01 { 0.0 } else { 0.85 };
                        engine.send_remote_action(RemoteAction::SetVolume {
                            target_device: peer.device_id,
                            volume_norm: next_v,
                        });
                        self.ui
                            .ferric_volume_btn(id!(vol_mute_btn))
                            .trigger_anim(cx);
                        self.ui
                            .ferric_volume_btn(id!(fs_vol_mute_btn))
                            .trigger_anim(cx);
                        return;
                    }
                    if self.session_state.volume > 0.01 {
                        self.pre_mute_volume = self.session_state.volume;
                        self.session_state.volume = 0.0;
                    } else {
                        self.session_state.volume = self.pre_mute_volume.max(0.5);
                    }
                    if let Some(audio) = &self.audio {
                        audio.send(EngineCommand::SetVolume(self.session_state.volume));
                    }
                    self.mark_session_dirty();
                    self.sync_playback_controls_only(cx);
                    self.ui
                        .ferric_volume_btn(id!(vol_mute_btn))
                        .trigger_anim(cx);
                    self.ui
                        .ferric_volume_btn(id!(fs_vol_mute_btn))
                        .trigger_anim(cx);
                    return;
                }
                if ke.key_code == KeyCode::KeyS {
                    if self.is_controlling_remote()
                        && let Some(peer) = self.active_remote_peer().cloned()
                        && let Some(engine) = &self.connect_engine
                    {
                        let next_s = !peer.shuffle_enabled;
                        engine.send_remote_action(RemoteAction::SetShuffle(next_s));
                        self.trigger_icon_pulse(cx, id!(shuffle_btn), 0.0);
                        self.trigger_icon_pulse(cx, id!(fs_shuffle_btn), 0.0);
                        self.show_toast(cx, if next_s { "Shuffle On" } else { "Shuffle Off" });
                        return;
                    }
                    let next_s = !self.session_state.shuffle;
                    self.session_state.shuffle = next_s;
                    self.queue.set_shuffle(next_s);
                    self.mark_session_dirty();
                    self.sync_next_deck_to_engine();
                    self.sync_playback_controls_only(cx);
                    self.trigger_icon_pulse(cx, id!(shuffle_btn), 0.0);
                    self.trigger_icon_pulse(cx, id!(fs_shuffle_btn), 0.0);
                    self.refresh_right_sidebar(cx);
                    self.show_toast(cx, if next_s { "Shuffle On" } else { "Shuffle Off" });
                    return;
                }
                if ke.key_code == KeyCode::KeyR {
                    if self.is_controlling_remote()
                        && let Some(peer) = self.active_remote_peer().cloned()
                        && let Some(engine) = &self.connect_engine
                    {
                        let next_r = peer.repeat_mode.next();
                        engine.send_remote_action(RemoteAction::SetRepeat(next_r));
                        self.ui.ferric_repeat_btn(id!(repeat_btn)).trigger_anim(cx);
                        self.ui
                            .ferric_repeat_btn(id!(fs_repeat_btn))
                            .trigger_anim(cx);
                        return;
                    }
                    let next_r = match self.session_state.repeat {
                        RepeatMode::Off => RepeatMode::All,
                        RepeatMode::All => RepeatMode::One,
                        RepeatMode::One => RepeatMode::Off,
                    };
                    self.session_state.repeat = next_r;
                    self.queue.repeat = next_r;
                    self.mark_session_dirty();
                    self.sync_next_deck_to_engine();
                    self.sync_playback_controls_only(cx);
                    self.ui.ferric_repeat_btn(id!(repeat_btn)).trigger_anim(cx);
                    self.ui
                        .ferric_repeat_btn(id!(fs_repeat_btn))
                        .trigger_anim(cx);
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
                    && let Some(cur) = self.effective_display_track()
                {
                    let next_liked = self.toggle_track_like(cx, &cur);
                    self.trigger_icon_pulse(cx, id!(np_like_btn), 3.0);
                    self.trigger_icon_pulse(cx, id!(fs_like_btn), 3.0);
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
                if ke.key_code == KeyCode::KeyF {
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

        let on_track_list = Self::is_track_list_page(self.active_page);
        if on_track_list {
            self.ensure_artwork_snapshot();
        }
        let snap_opt = if on_track_list {
            self.audio.as_ref().map(|a| a.telemetry())
        } else {
            None
        };
        let avg_lvl = snap_opt.map_or(0.5_f32, |s| {
            ((s.spectrum_bins[0] + s.spectrum_bins[1]) as f32 / 2_000.0).clamp(0.1, 1.0)
        });
        if !self.missing_art_queue.borrow().is_empty() {
            self.missing_art_queue.borrow_mut().clear();
        }
        let pal = self.adaptive_palette();
        let props = TrackListScope {
            tracks: self.active_track_list(),
            active_track_id: if on_track_list {
                self.queue.current().map(|t| t.id.clone())
            } else {
                None
            },
            is_playing: snap_opt.is_some_and(|s| s.is_playing),
            audio_level: avg_lvl,
            accent: rgba_to_vec4(pal.accent),
            acrylic_on: if self.theme.acrylic_enabled { 1.0 } else { 0.0 },
            light_mode: if pal.is_light { 1.0 } else { 0.0 },
            text_primary: rgba_to_vec4(pal.text_primary),
            text_secondary: rgba_to_vec4(pal.text_secondary),
            icon_idle: rgba_to_vec4(pal.icon_idle),
            icon_hover: rgba_to_vec4(pal.icon_hover),
            ui_zoom: self.theme.ui_zoom.clamp(0.65, 1.40),
            artworks: Rc::clone(&self.artwork_snapshot),
            missing_art_tracks: Rc::clone(&self.missing_art_queue),
        };
        {
            let mut scope = Scope::with_props(&props);
            self.ui.handle_event(cx, event, &mut scope);
        }

        // Drain lazily requested artwork for scrolled TrackList rows beyond initial batch
        if !self.missing_art_queue.borrow().is_empty() {
            let lazy_tracks: Vec<Track> = self.missing_art_queue.borrow_mut().drain(..).collect();
            if !self.window_hidden_or_minimized
                && let Some(cache) = &mut self.artwork
            {
                for t in &lazy_tracks {
                    let _ = cache.request_for_track(t);
                }
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
    fe.is_over && (fe.abs - fe.abs_start).length() < 24.0
}

/// Inspect EVERY action emitted for `view` in the current frame (Makepad's built-in
/// `ViewRef::finger_up` only checks the first matching action and can miss `FingerUp` when
/// `FingerHoverIn`/`FingerMove` was also emitted in the same batch).
fn view_clicked(view: &ViewRef, actions: &Actions) -> bool {
    let uid = view.widget_uid();
    actions
        .filter_widget_actions(uid)
        .any(|item| matches!(item.cast::<ViewAction>(), ViewAction::FingerUp(fe) if is_desktop_click(&fe)))
}

fn clean_display_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars() {
        let cp = c as u32;
        if matches!(
            cp,
            0x200D | 0xFE00..=0xFE0F | 0x2600..=0x27BF | 0x1F000..=0x1FFFF
        ) {
            continue;
        }
        if c.is_whitespace() {
            if !out.is_empty() {
                pending_space = true;
            }
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    out
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

struct SidebarLibraryEntry {
    id: String,
    title: String,
    subtitle: String,
    thumbnail_url: Option<String>,
    is_circle: bool,
}

fn sidebar_pin_slot_ids() -> [&'static [LiveId]; 40] {
    [
        id!(pin_slot_0),
        id!(pin_slot_1),
        id!(pin_slot_2),
        id!(pin_slot_3),
        id!(pin_slot_4),
        id!(pin_slot_5),
        id!(pin_slot_6),
        id!(pin_slot_7),
        id!(pin_slot_8),
        id!(pin_slot_9),
        id!(pin_slot_10),
        id!(pin_slot_11),
        id!(pin_slot_12),
        id!(pin_slot_13),
        id!(pin_slot_14),
        id!(pin_slot_15),
        id!(pin_slot_16),
        id!(pin_slot_17),
        id!(pin_slot_18),
        id!(pin_slot_19),
        id!(pin_slot_20),
        id!(pin_slot_21),
        id!(pin_slot_22),
        id!(pin_slot_23),
        id!(pin_slot_24),
        id!(pin_slot_25),
        id!(pin_slot_26),
        id!(pin_slot_27),
        id!(pin_slot_28),
        id!(pin_slot_29),
        id!(pin_slot_30),
        id!(pin_slot_31),
        id!(pin_slot_32),
        id!(pin_slot_33),
        id!(pin_slot_34),
        id!(pin_slot_35),
        id!(pin_slot_36),
        id!(pin_slot_37),
        id!(pin_slot_38),
        id!(pin_slot_39),
    ]
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

fn persistent_connect_device_id() -> String {
    let dir = app_data_dir();
    let id_path = dir.join("connect_device_id.txt");
    if let Ok(raw) = std::fs::read_to_string(&id_path) {
        let trimmed = raw.trim();
        if !trimmed.is_empty()
            && trimmed.len() <= 64
            && trimmed
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return trimmed.to_string();
        }
    }
    let generated = format!("ferric-pc-{}", csprng_hex(6));
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&id_path, &generated);
    generated
}

fn rgba_to_vec4(c: [f32; 4]) -> Vec4 {
    vec4(c[0], c[1], c[2], c[3])
}

impl Drop for App {
    fn drop(&mut self) {
        self.flush_session_state_now();
        if let Some(engine) = self.connect_engine.take() {
            engine.shutdown();
        }
    }
}

app_main!(App);
