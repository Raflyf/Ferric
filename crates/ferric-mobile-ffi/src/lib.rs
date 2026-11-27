//! UniFFI proc-macro FFI bridge exposing Ferric's Rust core & controller cleanly to Kotlin Android.

#[cfg(target_os = "android")]
mod android_jni;

use ferric_audio::content::{ContentOpener, register_content_opener};
use ferric_audio::engine::TelemetrySnapshot;
use ferric_controller::events::ControllerEventSink;
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

    fn on_toast(&self, message: String) {
        self.0.on_toast(message);
    }
}

#[derive(uniffi::Object)]
pub struct FerricMobileCore {
    controller: Arc<FerricController>,
}

impl FerricMobileCore {
    fn build(
        data_dir: String,
        cache_dir: String,
        cipher: Box<dyn FfiSecretCipher>,
        content_opener: Box<dyn FfiContentOpener>,
        sink: Box<dyn FfiEventSink>,
        headless_audio: bool,
    ) -> Result<Arc<Self>, FfiError> {
        register_platform_secret_cipher(Arc::new(SecretCipherBridge(cipher)));
        register_content_opener(Arc::new(ContentOpenerBridge(content_opener)));
        let cfg = ControllerConfig {
            data_dir: PathBuf::from(data_dir),
            cache_dir: PathBuf::from(cache_dir),
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
        Self::build(data_dir, cache_dir, cipher, content_opener, sink, true)
    }
}

#[uniffi::export]
impl FerricMobileCore {
    #[uniffi::constructor]
    pub fn new(
        data_dir: String,
        cache_dir: String,
        cipher: Box<dyn FfiSecretCipher>,
        content_opener: Box<dyn FfiContentOpener>,
        sink: Box<dyn FfiEventSink>,
    ) -> Result<Arc<Self>, FfiError> {
        Self::build(data_dir, cache_dir, cipher, content_opener, sink, false)
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

    pub fn shutdown(&self) {
        self.controller.shutdown();
    }
}
