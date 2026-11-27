//! Typed async event sink emitted by [`crate::FerricController`] to platform frontends (Kotlin StateFlow, tests).

use ferric_audio::engine::TelemetrySnapshot;
use ferric_core::models::Track;

/// Platform event receiver invoked off the UI thread whenever controller state changes.
pub trait ControllerEventSink: Send + Sync {
    fn on_telemetry(&self, _telemetry: TelemetrySnapshot) {}
    fn on_now_playing(&self, _track: Option<Track>) {}
    fn on_queue_changed(&self, _up_next: Vec<Track>) {}
    fn on_library_changed(&self) {}
    fn on_toast(&self, _message: String) {}
}

/// Default no-op sink for headless utilities and tests.
pub struct NoopEventSink;

impl ControllerEventSink for NoopEventSink {}
