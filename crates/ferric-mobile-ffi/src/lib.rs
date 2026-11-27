//! UniFFI proc-macro FFI bridge exposing Ferric's Rust core & controller cleanly to Kotlin Android.

#[cfg(target_os = "android")]
mod android_jni;
pub mod m2_types;
pub mod m3_types;
pub mod m4_types;

pub use m2_types::*;
pub use m3_types::*;
pub use m4_types::*;

use ferric_audio::content::{ContentOpener, register_content_opener};
use ferric_audio::engine::TelemetrySnapshot;
use ferric_controller::events::ControllerEventSink;
use ferric_controller::types::{
    AccountStatusSnapshot, CollectionDetailSnapshot, HomeSnapshot, LyricsSnapshot, SearchSnapshot,
    VideoStreamsSnapshot,
};
use ferric_controller::{ControllerConfig, ControllerError, FerricController};
use ferric_core::eq::{EqBand, EqConfig, EqFilterKind};
use ferric_core::models::{
    AudioQuality, FadeCurve, ListeningStatsSummary, PlaybackDspSettings, Playlist, RepeatMode,
    Track, TrackSource, VideoQuality,
};
use ferric_core::secrets::{KEYSTORE_PREFIX, SecretCipher, register_platform_secret_cipher};
use ferric_core::theme::{BackgroundMode, ThemeConfig};
use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

uniffi::setup_scaffolding!();

#[derive(Debug, Error, uniffi::Error)]
pub enum FfiError {
    #[error("{reason}")]
    Internal { reason: String },
    #[error("{reason}")]
    SecretProtection { reason: String },
    #[error("{reason}")]
    Io { reason: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for FfiError {
    fn from(err: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Internal { reason: err.reason }
    }
}

impl From<ControllerError> for FfiError {
    fn from(err: ControllerError) -> Self {
        match err {
            ControllerError::Core(ferric_core::CoreError::SecretProtection(reason)) => {
                Self::SecretProtection { reason }
            }
            ControllerError::Io(e) => Self::Io {
                reason: e.to_string(),
            },
            other => Self::Internal {
                reason: other.to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiTrackSource {
    Local,
    Spotify,
    YouTubeMusic,
    Subsonic,
    P2pHome,
}

impl From<TrackSource> for FfiTrackSource {
    fn from(src: TrackSource) -> Self {
        match src {
            TrackSource::Local => Self::Local,
            TrackSource::Spotify => Self::Spotify,
            TrackSource::YouTubeMusic => Self::YouTubeMusic,
            TrackSource::Subsonic => Self::Subsonic,
            TrackSource::P2pHome => Self::P2pHome,
        }
    }
}

impl From<FfiTrackSource> for TrackSource {
    fn from(src: FfiTrackSource) -> Self {
        match src {
            FfiTrackSource::Local => Self::Local,
            FfiTrackSource::Spotify => Self::Spotify,
            FfiTrackSource::YouTubeMusic => Self::YouTubeMusic,
            FfiTrackSource::Subsonic => Self::Subsonic,
            FfiTrackSource::P2pHome => Self::P2pHome,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiRepeatMode {
    Off,
    All,
    One,
}

impl From<RepeatMode> for FfiRepeatMode {
    fn from(mode: RepeatMode) -> Self {
        match mode {
            RepeatMode::Off => Self::Off,
            RepeatMode::All => Self::All,
            RepeatMode::One => Self::One,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiBackgroundMode {
    AmoledSolid,
    NativeAcrylic,
    ObsidianSolid,
    WarmCream,
}

impl From<BackgroundMode> for FfiBackgroundMode {
    fn from(mode: BackgroundMode) -> Self {
        match mode {
            BackgroundMode::AmoledSolid => Self::AmoledSolid,
            BackgroundMode::NativeAcrylic => Self::NativeAcrylic,
            BackgroundMode::ObsidianSolid => Self::ObsidianSolid,
            BackgroundMode::WarmCream => Self::WarmCream,
        }
    }
}

impl From<FfiBackgroundMode> for BackgroundMode {
    fn from(mode: FfiBackgroundMode) -> Self {
        match mode {
            FfiBackgroundMode::AmoledSolid => Self::AmoledSolid,
            FfiBackgroundMode::NativeAcrylic => Self::NativeAcrylic,
            FfiBackgroundMode::ObsidianSolid => Self::ObsidianSolid,
            FfiBackgroundMode::WarmCream => Self::WarmCream,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiEqFilterKind {
    Peaking,
    LowShelf,
    HighShelf,
}

impl From<EqFilterKind> for FfiEqFilterKind {
    fn from(kind: EqFilterKind) -> Self {
        match kind {
            EqFilterKind::Peaking => Self::Peaking,
            EqFilterKind::LowShelf => Self::LowShelf,
            EqFilterKind::HighShelf => Self::HighShelf,
        }
    }
}

impl From<FfiEqFilterKind> for EqFilterKind {
    fn from(kind: FfiEqFilterKind) -> Self {
        match kind {
            FfiEqFilterKind::Peaking => Self::Peaking,
            FfiEqFilterKind::LowShelf => Self::LowShelf,
            FfiEqFilterKind::HighShelf => Self::HighShelf,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FfiEqBand {
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
    pub kind: FfiEqFilterKind,
    pub enabled: bool,
}

impl From<EqBand> for FfiEqBand {
    fn from(b: EqBand) -> Self {
        Self {
            freq_hz: b.freq_hz,
            gain_db: b.gain_db,
            q: b.q,
            kind: b.kind.into(),
            enabled: b.enabled,
        }
    }
}

impl From<FfiEqBand> for EqBand {
    fn from(b: FfiEqBand) -> Self {
        Self {
            freq_hz: b.freq_hz,
            gain_db: b.gain_db,
            q: b.q,
            kind: b.kind.into(),
            enabled: b.enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FfiEqConfig {
    pub enabled: bool,
    pub preamp_db: f32,
    pub bands: Vec<FfiEqBand>,
    pub preset_name: String,
}

impl From<EqConfig> for FfiEqConfig {
    fn from(cfg: EqConfig) -> Self {
        Self {
            enabled: cfg.enabled,
            preamp_db: cfg.preamp_db,
            bands: cfg.bands.into_iter().map(Into::into).collect(),
            preset_name: cfg.preset_name,
        }
    }
}

impl From<FfiEqConfig> for EqConfig {
    fn from(cfg: FfiEqConfig) -> Self {
        Self {
            enabled: cfg.enabled,
            preamp_db: cfg.preamp_db,
            bands: cfg.bands.into_iter().map(Into::into).collect(),
            preset_name: cfg.preset_name,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiFadeCurve {
    EqualPower,
    SCurveVelvet,
    BassSwapAutomix,
}

impl From<FadeCurve> for FfiFadeCurve {
    fn from(c: FadeCurve) -> Self {
        match c {
            FadeCurve::EqualPower => Self::EqualPower,
            FadeCurve::SCurveVelvet => Self::SCurveVelvet,
            FadeCurve::BassSwapAutomix => Self::BassSwapAutomix,
        }
    }
}

impl From<FfiFadeCurve> for FadeCurve {
    fn from(c: FfiFadeCurve) -> Self {
        match c {
            FfiFadeCurve::EqualPower => Self::EqualPower,
            FfiFadeCurve::SCurveVelvet => Self::SCurveVelvet,
            FfiFadeCurve::BassSwapAutomix => Self::BassSwapAutomix,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiAudioQuality {
    Low,
    Normal,
    High,
    Best,
}

impl From<AudioQuality> for FfiAudioQuality {
    fn from(q: AudioQuality) -> Self {
        match q {
            AudioQuality::Low => Self::Low,
            AudioQuality::Normal => Self::Normal,
            AudioQuality::High => Self::High,
            AudioQuality::Best => Self::Best,
        }
    }
}

impl From<FfiAudioQuality> for AudioQuality {
    fn from(q: FfiAudioQuality) -> Self {
        match q {
            FfiAudioQuality::Low => Self::Low,
            FfiAudioQuality::Normal => Self::Normal,
            FfiAudioQuality::High => Self::High,
            FfiAudioQuality::Best => Self::Best,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiVideoQuality {
    Auto,
    P360,
    P480,
    P720,
    P1080,
}

impl From<VideoQuality> for FfiVideoQuality {
    fn from(q: VideoQuality) -> Self {
        match q {
            VideoQuality::Auto => Self::Auto,
            VideoQuality::P360 => Self::P360,
            VideoQuality::P480 => Self::P480,
            VideoQuality::P720 => Self::P720,
            VideoQuality::P1080 => Self::P1080,
        }
    }
}

impl From<FfiVideoQuality> for VideoQuality {
    fn from(q: FfiVideoQuality) -> Self {
        match q {
            FfiVideoQuality::Auto => Self::Auto,
            FfiVideoQuality::P360 => Self::P360,
            FfiVideoQuality::P480 => Self::P480,
            FfiVideoQuality::P720 => Self::P720,
            FfiVideoQuality::P1080 => Self::P1080,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FfiDspSettings {
    pub crossfade_ms: u32,
    pub fade_in_play_ms: u32,
    pub fade_curve: FfiFadeCurve,
    pub gapless_album_lock: bool,
    pub trim_silence: bool,
    pub replay_gain: bool,
    pub mono_audio: bool,
    pub playback_speed: f32,
    pub sleep_timer_fade_out: bool,
    pub audio_quality: FfiAudioQuality,
    pub video_quality: FfiVideoQuality,
}

impl From<PlaybackDspSettings> for FfiDspSettings {
    fn from(d: PlaybackDspSettings) -> Self {
        Self {
            crossfade_ms: d.crossfade_ms,
            fade_in_play_ms: d.fade_in_play_ms,
            fade_curve: d.fade_curve.into(),
            gapless_album_lock: d.gapless_album_lock,
            trim_silence: d.trim_silence,
            replay_gain: d.replay_gain,
            mono_audio: d.mono_audio,
            playback_speed: d.playback_speed,
            sleep_timer_fade_out: d.sleep_timer_fade_out,
            audio_quality: d.audio_quality.into(),
            video_quality: d.video_quality.into(),
        }
    }
}

impl From<FfiDspSettings> for PlaybackDspSettings {
    fn from(d: FfiDspSettings) -> Self {
        Self {
            crossfade_ms: d.crossfade_ms,
            fade_in_play_ms: d.fade_in_play_ms,
            fade_curve: d.fade_curve.into(),
            gapless_album_lock: d.gapless_album_lock,
            trim_silence: d.trim_silence,
            replay_gain: d.replay_gain,
            mono_audio: d.mono_audio,
            playback_speed: d.playback_speed,
            sleep_timer_fade_out: d.sleep_timer_fade_out,
            audio_quality: d.audio_quality.into(),
            video_quality: d.video_quality.into(),
            close_to_tray: false,
            output_device: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub path: Option<String>,
    pub source: FfiTrackSource,
    pub codec: String,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub bitrate_kbps: u32,
    pub spotify_id: Option<String>,
    pub youtube_video_id: Option<String>,
    pub thumbnail_url: Option<String>,
    pub replay_gain_db: i32,
    pub liked: bool,
}

impl From<Track> for FfiTrack {
    fn from(t: Track) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_ms: t.duration_ms,
            path: t.path.map(|p| p.to_string_lossy().into_owned()),
            source: t.source.into(),
            codec: t.codec,
            sample_rate: t.sample_rate,
            bit_depth: t.bit_depth,
            bitrate_kbps: t.bitrate_kbps,
            spotify_id: t.spotify_id,
            youtube_video_id: t.youtube_video_id,
            thumbnail_url: t.thumbnail_url,
            replay_gain_db: t.replay_gain_db,
            liked: t.liked,
        }
    }
}

impl From<FfiTrack> for Track {
    fn from(t: FfiTrack) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_ms: t.duration_ms,
            path: t.path.map(PathBuf::from),
            source: t.source.into(),
            codec: t.codec,
            sample_rate: t.sample_rate,
            bit_depth: t.bit_depth,
            bitrate_kbps: t.bitrate_kbps,
            isrc: None,
            spotify_id: t.spotify_id,
            youtube_video_id: t.youtube_video_id,
            thumbnail_url: t.thumbnail_url,
            replay_gain_db: t.replay_gain_db,
            liked: t.liked,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiPlaylist {
    pub id: String,
    pub name: String,
    pub source: FfiTrackSource,
    pub track_count: u32,
    pub description: String,
    pub thumbnail_url: Option<String>,
}

impl From<Playlist> for FfiPlaylist {
    fn from(p: Playlist) -> Self {
        Self {
            id: p.id,
            name: p.name,
            source: p.source.into(),
            track_count: u32::try_from(p.track_count).unwrap_or(u32::MAX),
            description: p.description,
            thumbnail_url: p.thumbnail_url,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiTelemetry {
    pub is_playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub output_rate: u32,
    pub is_crossfading: bool,
    pub spectrum_bins: Vec<u32>,
}

impl From<TelemetrySnapshot> for FfiTelemetry {
    fn from(s: TelemetrySnapshot) -> Self {
        Self {
            is_playing: s.is_playing,
            position_ms: s.position_ms,
            duration_ms: s.duration_ms,
            sample_rate: s.sample_rate,
            output_rate: s.output_rate,
            is_crossfading: s.is_crossfading,
            spectrum_bins: s.spectrum_bins.to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FfiThemeConfig {
    pub name: String,
    pub canvas_hex: String,
    pub surface_hex: String,
    pub border_hex: String,
    pub text_primary_hex: String,
    pub text_secondary_hex: String,
    pub accent_hex: String,
    pub background_mode: FfiBackgroundMode,
    pub corner_radius_px: f32,
    pub panel_opacity: f32,
    pub adaptive_album_art: bool,
    pub acrylic_enabled: bool,
    pub acrylic_opacity: f32,
    pub acrylic_tint_hex: String,
    pub adaptive_acrylic_tint: bool,
    pub editorial_mode: bool,
    pub font_family: String,
    pub player_layout: String,
}

impl From<ThemeConfig> for FfiThemeConfig {
    fn from(t: ThemeConfig) -> Self {
        Self {
            name: t.name,
            canvas_hex: t.canvas_hex,
            surface_hex: t.surface_hex,
            border_hex: t.border_hex,
            text_primary_hex: t.text_primary_hex,
            text_secondary_hex: t.text_secondary_hex,
            accent_hex: t.accent_hex,
            background_mode: t.background_mode.into(),
            corner_radius_px: t.corner_radius_px,
            panel_opacity: t.panel_opacity,
            adaptive_album_art: t.adaptive_album_art,
            acrylic_enabled: t.acrylic_enabled,
            acrylic_opacity: t.acrylic_opacity,
            acrylic_tint_hex: t.acrylic_tint_hex,
            adaptive_acrylic_tint: t.adaptive_acrylic_tint,
            editorial_mode: t.editorial_mode,
            font_family: t.font_family,
            player_layout: t.player_layout,
        }
    }
}

impl From<FfiThemeConfig> for ThemeConfig {
    fn from(t: FfiThemeConfig) -> Self {
        Self {
            name: t.name,
            canvas_hex: t.canvas_hex,
            surface_hex: t.surface_hex,
            border_hex: t.border_hex,
            text_primary_hex: t.text_primary_hex,
            text_secondary_hex: t.text_secondary_hex,
            accent_hex: t.accent_hex,
            background_mode: t.background_mode.into(),
            corner_radius_px: t.corner_radius_px,
            panel_opacity: t.panel_opacity,
            adaptive_album_art: t.adaptive_album_art,
            acrylic_enabled: t.acrylic_enabled,
            acrylic_opacity: t.acrylic_opacity,
            acrylic_tint_hex: t.acrylic_tint_hex,
            adaptive_acrylic_tint: t.adaptive_acrylic_tint,
            editorial_mode: t.editorial_mode,
            font_family: t.font_family,
            player_layout: t.player_layout,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiArtistStat {
    pub artist: String,
    pub plays: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiListeningStats {
    pub total_plays: u64,
    pub total_listened_ms: u64,
    pub current_streak_days: u32,
    pub longest_streak_days: u32,
    pub top_artists: Vec<FfiArtistStat>,
}

impl From<ListeningStatsSummary> for FfiListeningStats {
    fn from(s: ListeningStatsSummary) -> Self {
        Self {
            total_plays: s.total_plays,
            total_listened_ms: s.total_listened_ms,
            current_streak_days: s.current_streak_days,
            longest_streak_days: s.longest_streak_days,
            top_artists: s
                .top_artists
                .into_iter()
                .map(|(artist, plays)| FfiArtistStat { artist, plays })
                .collect(),
        }
    }
}

#[uniffi::export(callback_interface)]
pub trait FfiSecretCipher: Send + Sync {
    fn encrypt(&self, plain: Vec<u8>) -> Result<Vec<u8>, FfiError>;
    fn decrypt(&self, cipher: Vec<u8>) -> Result<Vec<u8>, FfiError>;
}

#[uniffi::export(callback_interface)]
pub trait FfiContentOpener: Send + Sync {
    /// Open a read-only Android `content://` URI via `ContentResolver.openFileDescriptor(uri, "r")?.detachFd()`.
    /// Rust takes ownership of the detached POSIX file descriptor and closes it when done.
    fn open_fd(&self, uri: String) -> Result<i32, FfiError>;
}

#[uniffi::export(callback_interface)]
pub trait FfiEventSink: Send + Sync {
    fn on_playback_telemetry(&self, telemetry: FfiTelemetry);
    fn on_now_playing_changed(&self, track: Option<FfiTrack>);
    fn on_queue_changed(&self, up_next: Vec<FfiTrack>);
    fn on_library_changed(&self);
    fn on_home_updated(&self, home: FfiHomeState);
    fn on_search_results(&self, results: FfiSearchResults);
    fn on_detail_loaded(&self, detail: FfiCollectionDetail);
    fn on_accounts_changed(&self, status: FfiAccountStatus);
    fn on_lyrics_loaded(&self, lyrics: FfiLyricsState);
    fn on_video_streams_loaded(&self, video: FfiVideoStreamsState);
    fn on_connect_state_changed(&self, state: FfiConnectState);
    fn on_spotify_auth_expired(&self);
    fn on_toast(&self, message: String);
}

struct SecretCipherBridge(Box<dyn FfiSecretCipher>);

impl SecretCipher for SecretCipherBridge {
    fn prefix(&self) -> &'static str {
        KEYSTORE_PREFIX
    }

    fn encrypt(&self, plain: &[u8]) -> Result<Vec<u8>, String> {
        self.0.encrypt(plain.to_vec()).map_err(|e| e.to_string())
    }

    fn decrypt(&self, cipher: &[u8]) -> Result<Vec<u8>, String> {
        self.0.decrypt(cipher.to_vec()).map_err(|e| e.to_string())
    }
}

struct ContentOpenerBridge(Box<dyn FfiContentOpener>);

impl ContentOpener for ContentOpenerBridge {
    fn open_file(&self, uri: &str) -> io::Result<File> {
        let fd = self
            .0
            .open_fd(uri.to_string())
            .map_err(|e| io::Error::other(e.to_string()))?;
        if fd < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid negative file descriptor from Android ContentResolver",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::fd::FromRawFd;
            // SAFETY: `fd` is validated non-negative and exclusively owned after `ParcelFileDescriptor.detachFd()`.
            Ok(unsafe { File::from_raw_fd(fd) })
        }
        #[cfg(not(unix))]
        {
            let _ = fd;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "POSIX file descriptor content bridge is only supported on Unix/Android targets",
            ))
        }
    }
}

struct EventSinkBridge(Box<dyn FfiEventSink>);

impl ControllerEventSink for EventSinkBridge {
    fn on_telemetry(&self, telemetry: TelemetrySnapshot) {
        self.0.on_playback_telemetry(telemetry.into());
    }

    fn on_now_playing(&self, track: Option<Track>) {
        self.0.on_now_playing_changed(track.map(Into::into));
    }

    fn on_queue_changed(&self, up_next: Vec<Track>) {
        self.0
            .on_queue_changed(up_next.into_iter().map(Into::into).collect());
    }

    fn on_library_changed(&self) {
        self.0.on_library_changed();
    }

    fn on_home_updated(&self, home: HomeSnapshot) {
        self.0.on_home_updated(home.into());
    }

    fn on_search_results(&self, results: SearchSnapshot) {
        self.0.on_search_results(results.into());
    }

    fn on_detail_loaded(&self, detail: CollectionDetailSnapshot) {
        self.0.on_detail_loaded(detail.into());
    }

    fn on_accounts_changed(&self, status: AccountStatusSnapshot) {
        self.0.on_accounts_changed(status.into());
    }

    fn on_lyrics_loaded(&self, lyrics: LyricsSnapshot) {
        self.0.on_lyrics_loaded(lyrics.into());
    }

    fn on_video_streams_loaded(&self, video: VideoStreamsSnapshot) {
        self.0.on_video_streams_loaded(video.into());
    }

    fn on_connect_state_changed(&self, state: ferric_sync::ConnectSnapshot) {
        self.0.on_connect_state_changed(state.into());
    }

    fn on_spotify_auth_expired(&self) {
        self.0.on_spotify_auth_expired();
    }

    fn on_toast(&self, message: String) {
        self.0.on_toast(message);
    }
}

#[derive(uniffi::Object)]
pub struct FerricMobileCore {
    controller: Arc<FerricController>,
}

impl FerricMobileCore {
    #[allow(clippy::too_many_arguments)]
    fn build(
        data_dir: String,
        cache_dir: String,
        device_id: String,
        device_name: String,
        cipher: Box<dyn FfiSecretCipher>,
        content_opener: Box<dyn FfiContentOpener>,
        sink: Box<dyn FfiEventSink>,
        headless_audio: bool,
    ) -> Result<Arc<Self>, FfiError> {
        register_platform_secret_cipher(Arc::new(SecretCipherBridge(cipher)));
        register_content_opener(Arc::new(ContentOpenerBridge(content_opener)));
        let clean_id = if device_id.trim().is_empty() {
            format!("android_{}", ferric_core::secrets::csprng_hex(6))
        } else {
            device_id.trim().to_string()
        };
        let clean_name = if device_name.trim().is_empty() {
            "Ferric Android".to_string()
        } else {
            device_name.trim().to_string()
        };
        let cfg = ControllerConfig {
            data_dir: PathBuf::from(data_dir),
            cache_dir: PathBuf::from(cache_dir),
            device_id: clean_id,
            device_name: clean_name,
            device_kind: ferric_sync::DeviceKind::Phone,
            enable_connect: true,
            headless_audio,
        };
        let controller = FerricController::new(cfg, Arc::new(EventSinkBridge(sink)))?;
        Ok(Arc::new(Self { controller }))
    }

    /// Headless constructor used by host-side Rust integration tests without a sound device.
    pub fn new_headless_for_test(
        data_dir: String,
        cache_dir: String,
        cipher: Box<dyn FfiSecretCipher>,
        content_opener: Box<dyn FfiContentOpener>,
        sink: Box<dyn FfiEventSink>,
    ) -> Result<Arc<Self>, FfiError> {
        Self::build(
            data_dir,
            cache_dir,
            "test_android_01".to_string(),
            "Test Android".to_string(),
            cipher,
            content_opener,
            sink,
            true,
        )
    }
}

#[uniffi::export]
impl FerricMobileCore {
    #[uniffi::constructor]
    pub fn new(
        data_dir: String,
        cache_dir: String,
        device_id: String,
        device_name: String,
        cipher: Box<dyn FfiSecretCipher>,
        content_opener: Box<dyn FfiContentOpener>,
        sink: Box<dyn FfiEventSink>,
    ) -> Result<Arc<Self>, FfiError> {
        Self::build(
            data_dir,
            cache_dir,
            device_id,
            device_name,
            cipher,
            content_opener,
            sink,
            false,
        )
    }

    pub fn ensure_audio_started(&self) -> Result<(), FfiError> {
        self.controller.ensure_audio_started()?;
        Ok(())
    }

    pub fn is_audio_started(&self) -> bool {
        self.controller.is_audio_started()
    }

    pub fn suspend_audio_if_idle(&self) {
        self.controller.suspend_audio_if_idle();
    }

    pub fn seed_demo_if_empty(&self) -> Result<u32, FfiError> {
        let count = self.controller.ensure_demo_seed()?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    pub fn list_songs(&self, query: String) -> Result<Vec<FfiTrack>, FfiError> {
        let songs = self.controller.list_songs(&query)?;
        Ok(songs.into_iter().map(Into::into).collect())
    }

    pub fn list_playlists(&self) -> Result<Vec<FfiPlaylist>, FfiError> {
        let playlists = self.controller.list_playlists()?;
        Ok(playlists.into_iter().map(Into::into).collect())
    }

    pub fn play_track(&self, track_id: String) -> Result<(), FfiError> {
        self.controller.play_track(&track_id)?;
        Ok(())
    }

    pub fn play_queue(
        &self,
        tracks: Vec<FfiTrack>,
        start_index: u32,
        context: String,
    ) -> Result<(), FfiError> {
        let converted: Vec<Track> = tracks.into_iter().map(Into::into).collect();
        self.controller
            .play_queue(converted, start_index as usize, context)?;
        Ok(())
    }

    pub fn toggle_play_pause(&self) -> Result<(), FfiError> {
        self.controller.toggle_play_pause()?;
        Ok(())
    }

    pub fn play(&self) -> Result<(), FfiError> {
        self.controller.play()?;
        Ok(())
    }

    pub fn pause(&self) {
        self.controller.pause();
    }

    pub fn next(&self) -> Result<(), FfiError> {
        self.controller.next()?;
        Ok(())
    }

    pub fn prev(&self) -> Result<(), FfiError> {
        self.controller.prev()?;
        Ok(())
    }

    pub fn seek_ms(&self, pos_ms: u64) {
        self.controller.seek_ms(pos_ms);
    }

    pub fn set_volume(&self, vol: f32) {
        self.controller.set_volume(vol);
    }

    pub fn set_ducking(&self, duck: bool) {
        self.controller.set_ducking(duck);
    }

    pub fn toggle_shuffle(&self) -> bool {
        self.controller.toggle_shuffle()
    }

    pub fn cycle_repeat(&self) -> FfiRepeatMode {
        self.controller.cycle_repeat().into()
    }

    pub fn get_telemetry(&self) -> FfiTelemetry {
        self.controller.telemetry().into()
    }

    pub fn get_current_track(&self) -> Option<FfiTrack> {
        self.controller.current_track().map(Into::into)
    }

    pub fn get_up_next(&self, limit: u32) -> Vec<FfiTrack> {
        self.controller
            .up_next(limit as usize)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn get_theme(&self) -> FfiThemeConfig {
        self.controller.get_theme().into()
    }

    pub fn set_theme(&self, theme: FfiThemeConfig) -> Result<(), FfiError> {
        self.controller.set_theme(theme.into())?;
        Ok(())
    }

    pub fn get_eq(&self) -> FfiEqConfig {
        self.controller.get_eq().into()
    }

    pub fn set_eq(&self, eq: FfiEqConfig) -> Result<(), FfiError> {
        self.controller.set_eq(eq.into())?;
        Ok(())
    }

    pub fn get_dsp(&self) -> FfiDspSettings {
        self.controller.get_dsp().into()
    }

    pub fn set_dsp(&self, dsp: FfiDspSettings) -> Result<(), FfiError> {
        self.controller.set_dsp(dsp.into())?;
        Ok(())
    }

    pub fn get_listening_stats(&self, tz_offset_secs: i64) -> Result<FfiListeningStats, FfiError> {
        let stats = self.controller.get_listening_stats(tz_offset_secs)?;
        Ok(stats.into())
    }

    pub fn trim_memory(&self) {
        self.controller.trim_memory();
    }

    // --- M2 Home, Search & Discovery ---

    pub fn get_cached_home(&self) -> FfiHomeState {
        self.controller.get_cached_home().into()
    }

    pub fn load_home(&self, force_refresh: bool, mood_idx: Option<u32>) {
        self.controller
            .load_home(force_refresh, mood_idx.map(|i| i as usize));
    }

    pub fn open_station(&self, station_id: String) -> Result<(), FfiError> {
        self.controller.open_station(&station_id)?;
        Ok(())
    }

    pub fn open_browse_detail(
        &self,
        browse_id: String,
        kind_hint: FfiCardKind,
    ) -> Result<(), FfiError> {
        self.controller
            .open_browse_detail(browse_id, kind_hint.into())?;
        Ok(())
    }

    pub fn instant_search_local(&self, query: String) -> Result<Vec<FfiTrack>, FfiError> {
        let hits = self.controller.instant_search_local(&query)?;
        Ok(hits.into_iter().map(Into::into).collect())
    }

    pub fn search_online(&self, query: String) -> Result<(), FfiError> {
        self.controller.search_online(query)?;
        Ok(())
    }

    pub fn recent_searches(&self) -> Vec<String> {
        self.controller.recent_searches()
    }

    pub fn clear_recent_searches(&self) -> Result<(), FfiError> {
        self.controller.clear_recent_searches()?;
        Ok(())
    }

    pub fn import_public_url(&self, url: String) -> Result<(), FfiError> {
        self.controller.import_public_url(url)?;
        Ok(())
    }

    // --- M2 Library, Local Scan, Collections & Queue ---

    pub fn scan_local_entries(&self, entries: Vec<FfiLocalScanEntry>) -> Result<u32, FfiError> {
        let converted = entries.into_iter().map(Into::into).collect();
        let count = self.controller.scan_local_entries(converted)?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    pub fn list_liked_songs(&self) -> Result<Vec<FfiTrack>, FfiError> {
        Ok(self
            .controller
            .list_liked_songs()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn list_local_songs(&self) -> Result<Vec<FfiTrack>, FfiError> {
        Ok(self
            .controller
            .list_local_songs()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn list_recent_songs(&self) -> Result<Vec<FfiTrack>, FfiError> {
        Ok(self
            .controller
            .list_recent_songs()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn list_albums(&self) -> Result<Vec<FfiAlbumSummary>, FfiError> {
        Ok(self
            .controller
            .list_albums()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn list_artists(&self) -> Result<Vec<FfiArtistSummary>, FfiError> {
        Ok(self
            .controller
            .list_artists()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn list_playlists_filtered(
        &self,
        source: FfiPlaylistSourceFilter,
        sort: FfiPlaylistSort,
    ) -> Result<Vec<FfiPlaylist>, FfiError> {
        Ok(self
            .controller
            .list_playlists_filtered(source.into(), sort.into())?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn get_playlist_detail(
        &self,
        playlist_id: String,
        sort: FfiDetailSort,
    ) -> Result<FfiCollectionDetail, FfiError> {
        Ok(self
            .controller
            .get_playlist_detail(&playlist_id, sort.into())?
            .into())
    }

    pub fn get_album_detail(
        &self,
        album_name: String,
        artist_name: String,
        sort: FfiDetailSort,
    ) -> Result<FfiCollectionDetail, FfiError> {
        Ok(self
            .controller
            .get_album_detail(&album_name, &artist_name, sort.into())?
            .into())
    }

    pub fn get_artist_detail(
        &self,
        artist_name: String,
        sort: FfiDetailSort,
    ) -> Result<FfiCollectionDetail, FfiError> {
        Ok(self
            .controller
            .get_artist_detail(&artist_name, sort.into())?
            .into())
    }

    pub fn get_liked_detail(&self, sort: FfiDetailSort) -> Result<FfiCollectionDetail, FfiError> {
        Ok(self.controller.get_liked_detail(sort.into())?.into())
    }

    pub fn create_playlist(
        &self,
        name: String,
        description: String,
        track_ids: Vec<String>,
    ) -> Result<FfiPlaylist, FfiError> {
        Ok(self
            .controller
            .create_playlist(name, description, track_ids)?
            .into())
    }

    pub fn rename_playlist(&self, playlist_id: String, new_name: String) -> Result<(), FfiError> {
        self.controller.rename_playlist(playlist_id, new_name)?;
        Ok(())
    }

    pub fn delete_playlist(&self, playlist_id: String) -> Result<(), FfiError> {
        self.controller.delete_playlist(playlist_id)?;
        Ok(())
    }

    pub fn add_track_to_playlist(
        &self,
        playlist_id: String,
        track: FfiTrack,
    ) -> Result<(), FfiError> {
        self.controller
            .add_track_to_playlist(playlist_id, track.into())?;
        Ok(())
    }

    pub fn remove_track_from_playlist(
        &self,
        playlist_id: String,
        track_id: String,
    ) -> Result<(), FfiError> {
        self.controller
            .remove_track_from_playlist(playlist_id, track_id)?;
        Ok(())
    }

    pub fn deduplicate_playlist(&self, playlist_id: String) -> Result<u32, FfiError> {
        let removed = self.controller.deduplicate_playlist(playlist_id)?;
        Ok(u32::try_from(removed).unwrap_or(u32::MAX))
    }

    pub fn push_local_playlist_to_cloud(&self, playlist_id: String) -> Result<(), FfiError> {
        self.controller.push_local_playlist_to_cloud(playlist_id)?;
        Ok(())
    }

    pub fn toggle_like(&self, track_id: String) -> Result<bool, FfiError> {
        Ok(self.controller.toggle_like(&track_id)?)
    }

    pub fn track_share_link(&self, track: FfiTrack) -> Option<String> {
        let t: Track = track.into();
        self.controller.track_share_link(&t)
    }

    pub fn format_share_tracklist(
        &self,
        title: String,
        share_url: Option<String>,
        tracks: Vec<FfiTrack>,
    ) -> String {
        let converted: Vec<Track> = tracks.into_iter().map(Into::into).collect();
        self.controller
            .format_share_tracklist(&title, share_url.as_deref(), &converted)
    }

    pub fn format_share_csv(&self, tracks: Vec<FfiTrack>) -> String {
        let converted: Vec<Track> = tracks.into_iter().map(Into::into).collect();
        self.controller.format_share_csv(&converted)
    }

    pub fn import_playlist_text(
        &self,
        playlist_name: String,
        raw_text: String,
    ) -> Result<FfiPlaylist, FfiError> {
        Ok(self
            .controller
            .import_playlist_text(playlist_name, raw_text)?
            .into())
    }

    pub fn play_next(&self, track: FfiTrack) {
        self.controller.play_next(track.into());
    }

    pub fn append_to_queue(&self, track: FfiTrack) {
        self.controller.append_to_queue(track.into());
    }

    pub fn jump_to_upcoming(&self, offset: u32) -> Result<(), FfiError> {
        self.controller.jump_to_upcoming(offset as usize)?;
        Ok(())
    }

    pub fn move_queue_item_up(&self, offset: u32) -> bool {
        self.controller.move_queue_item_up(offset as usize)
    }

    pub fn move_queue_item_down(&self, offset: u32) -> bool {
        self.controller.move_queue_item_down(offset as usize)
    }

    pub fn remove_queue_item(&self, offset: u32) -> bool {
        self.controller.remove_queue_item(offset as usize)
    }

    pub fn clear_up_next(&self) {
        self.controller.clear_up_next();
    }

    pub fn save_queue_as_playlist(&self, name: String) -> Result<FfiPlaylist, FfiError> {
        Ok(self.controller.save_queue_as_playlist(name)?.into())
    }

    pub fn copy_queue_tracklist(&self) -> String {
        self.controller.copy_queue_tracklist()
    }

    // --- M2 Accounts & Two-Way Sync ---

    pub fn get_account_status(&self) -> FfiAccountStatus {
        self.controller.get_account_status().into()
    }

    pub fn set_spotify_session(
        &self,
        access_token: String,
        cookie: String,
        user_name: String,
        user_id: String,
        trigger_sync: bool,
    ) -> Result<(), FfiError> {
        self.controller.set_spotify_session(
            access_token,
            cookie,
            user_name,
            user_id,
            trigger_sync,
        )?;
        Ok(())
    }

    pub fn sync_spotify_account(&self) {
        self.controller.sync_spotify_account();
    }

    pub fn logout_spotify(&self) -> Result<(), FfiError> {
        self.controller.logout_spotify()?;
        Ok(())
    }

    pub fn set_ytm_session(
        &self,
        cookie: String,
        sapisid: String,
        visitor_data: String,
        channel_name: String,
        account_id: String,
    ) -> Result<(), FfiError> {
        self.controller
            .set_ytm_session(cookie, sapisid, visitor_data, channel_name, account_id)?;
        Ok(())
    }

    pub fn sync_ytm_account(&self) {
        self.controller.sync_ytm_account();
    }

    pub fn logout_ytm(&self) -> Result<(), FfiError> {
        self.controller.logout_ytm()?;
        Ok(())
    }

    pub fn sync_subsonic(
        &self,
        base_url: String,
        username: String,
        password: String,
    ) -> Result<(), FfiError> {
        self.controller
            .sync_subsonic(base_url, username, password)?;
        Ok(())
    }

    pub fn logout_subsonic(&self) -> Result<(), FfiError> {
        self.controller.logout_subsonic()?;
        Ok(())
    }

    // --- M3 Synced Lyrics, Video Clip, EQ/AutoEq/APO & Theme Studio ---

    pub fn load_lyrics(&self, track: FfiTrack) {
        self.controller.load_lyrics(track.into());
    }

    pub fn nudge_lyrics_offset_ms(&self, delta_ms: i64) -> i64 {
        self.controller.nudge_lyrics_offset_ms(delta_ms)
    }

    pub fn resolve_video_streams(&self, track: FfiTrack) {
        self.controller.resolve_video_streams(track.into());
    }

    pub fn list_eq_presets(&self) -> Vec<String> {
        self.controller.list_eq_presets()
    }

    pub fn apply_eq_preset(&self, preset_name: String) -> Result<FfiEqConfig, FfiError> {
        Ok(self.controller.apply_eq_preset(&preset_name)?.into())
    }

    pub fn search_autoeq(&self, query: String) -> Vec<FfiAutoEqProfile> {
        self.controller
            .search_autoeq(&query)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn apply_autoeq_profile(&self, profile_name: String) -> Result<FfiEqConfig, FfiError> {
        Ok(self.controller.apply_autoeq_profile(&profile_name)?.into())
    }

    pub fn import_eq_apo(&self, apo_text: String) -> Result<FfiEqConfig, FfiError> {
        Ok(self.controller.import_eq_apo(&apo_text)?.into())
    }

    pub fn export_eq_apo(&self) -> String {
        self.controller.export_eq_apo()
    }

    pub fn apply_theme_preset(&self, preset_name: String) -> Result<FfiThemeConfig, FfiError> {
        Ok(self.controller.apply_theme_preset(&preset_name)?.into())
    }

    pub fn export_theme_json(&self) -> Result<String, FfiError> {
        Ok(self.controller.export_theme_json()?)
    }

    pub fn import_theme_json(&self, raw_json: String) -> Result<FfiThemeConfig, FfiError> {
        Ok(self.controller.import_theme_json(&raw_json)?.into())
    }

    // --- M4 Zero-Pairing Ferric Connect ---

    pub fn get_connect_state(&self) -> FfiConnectState {
        self.controller.get_connect_state().into()
    }

    pub fn notify_mdns_peer(
        &self,
        device_id: String,
        device_name: String,
        device_kind: FfiDeviceKind,
        ip: String,
        tcp_port: u16,
        room_tags_hex: String,
    ) {
        self.controller.notify_mdns_peer(
            device_id,
            device_name,
            device_kind.into(),
            ip,
            tcp_port,
            room_tags_hex,
        );
    }

    pub fn connect_switch_speaker(&self, target_device_id: String) {
        self.controller.connect_switch_speaker(target_device_id);
    }

    pub fn connect_set_remote_volume(&self, target_device_id: String, volume_norm: f32) {
        self.controller
            .connect_set_remote_volume(target_device_id, volume_norm);
    }

    pub fn connect_pair_qr(&self, uri: String) {
        self.controller.connect_pair_qr(uri);
    }

    pub fn connect_get_qr_code(&self, regenerate: bool) -> FfiQrCode {
        let (uri, size, modules) = self.controller.connect_qr_matrix(regenerate);
        FfiQrCode {
            uri,
            size: u32::try_from(size).unwrap_or(0),
            modules,
        }
    }

    pub fn shutdown(&self) {
        self.controller.shutdown();
    }
}
