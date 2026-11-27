//! Typed async event sink emitted by [`crate::FerricController`] to platform frontends (Kotlin StateFlow, tests).

use crate::types::{
    AccountStatusSnapshot, CollectionDetailSnapshot, HomeSnapshot, LyricsSnapshot, SearchSnapshot,
    VideoStreamsSnapshot,
};
use ferric_audio::engine::TelemetrySnapshot;
use ferric_core::models::Track;
use ferric_sync::ConnectSnapshot;

/// Platform event receiver invoked off the UI thread whenever controller state changes.
pub trait ControllerEventSink: Send + Sync {
    fn on_telemetry(&self, _telemetry: TelemetrySnapshot) {}
    fn on_now_playing(&self, _track: Option<Track>) {}
    fn on_queue_changed(&self, _up_next: Vec<Track>) {}
    fn on_library_changed(&self) {}
    fn on_home_updated(&self, _home: HomeSnapshot) {}
    fn on_search_results(&self, _results: SearchSnapshot) {}
    fn on_detail_loaded(&self, _detail: CollectionDetailSnapshot) {}
    fn on_accounts_changed(&self, _status: AccountStatusSnapshot) {}
    fn on_lyrics_loaded(&self, _lyrics: LyricsSnapshot) {}
    fn on_video_streams_loaded(&self, _video: VideoStreamsSnapshot) {}
    fn on_connect_state_changed(&self, _state: ConnectSnapshot) {}
    fn on_download_progress(&self, _track_id: String, _percent: u8) {}
    fn on_spotify_auth_expired(&self) {}
    fn on_toast(&self, _message: String) {}
}

/// Default no-op sink for headless utilities and tests.
pub struct NoopEventSink;

impl ControllerEventSink for NoopEventSink {}
