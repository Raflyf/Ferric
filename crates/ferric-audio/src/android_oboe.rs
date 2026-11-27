//! Android native Oboe (`AAudio` with automatic `OpenSL ES` fallback) output backend.
//! Compiled exclusively on `target_os = "android"` so Windows Desktop `cpal` remains untouched.

use crate::engine::{AudioTelemetry, RingFrame, pop_epoch_frame};
use crate::{AudioError, AudioResult};
use oboe::{
    AudioOutputCallback, AudioOutputStream, AudioOutputStreamSafe, AudioStream, AudioStreamAsync,
    AudioStreamBase, AudioStreamBuilder, ContentType, DataCallbackResult, Output,
    PerformanceMode, SharingMode, Stereo, Usage,
};
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub(crate) struct OboeRenderCallback {
    consumer: rtrb::Consumer<RingFrame>,
    telemetry: Arc<AudioTelemetry>,
    max_skip: usize,
}

impl AudioOutputCallback for OboeRenderCallback {
    type FrameType = (f32, Stereo);

    fn on_audio_ready(
        &mut self,
        _stream: &mut dyn AudioOutputStreamSafe,
        frames: &mut [(f32, f32)],
    ) -> DataCallbackResult {
        self.telemetry.cb_heartbeat.fetch_add(1, Ordering::Relaxed);
        let cur_epoch = self.telemetry.active_epoch.load(Ordering::Acquire);
        for frame in frames.iter_mut() {
            let [l, r] = pop_epoch_frame(&mut self.consumer, cur_epoch, self.max_skip);
            *frame = (l, r);
        }
        DataCallbackResult::Continue
    }

    fn on_error_after_close(
        &mut self,
        _stream: &mut dyn AudioOutputStreamSafe,
        _error: oboe::Error,
    ) {
        self.telemetry.device_lost.store(true, Ordering::Release);
    }
}

pub(crate) struct AndroidOutputStream {
    stream: AudioStreamAsync<Output, OboeRenderCallback>,
}

impl AndroidOutputStream {
    pub(crate) fn play(&mut self) -> Result<(), String> {
        self.stream.start().map_err(|e| format!("{e:?}"))
    }

    pub(crate) fn pause(&mut self) -> Result<(), String> {
        self.stream.pause().map_err(|e| format!("{e:?}"))
    }
}

pub(crate) fn list_output_devices() -> Vec<String> {
    vec!["Android System Output".to_string()]
}

pub(crate) fn build_output_stream_and_producer(
    telemetry: &Arc<AudioTelemetry>,
    _preferred_device: Option<&str>,
    start_playing: bool,
) -> AudioResult<(AndroidOutputStream, rtrb::Producer<RingFrame>, u32, String)> {
    // Allocate a ~260 ms stereo ring buffer sized for up to 48 kHz (expanded dynamically if higher).
    let ring_frames: usize = 16_384;
    let (producer, consumer) = rtrb::RingBuffer::<RingFrame>::new(ring_frames);

    let cb = OboeRenderCallback {
        consumer,
        telemetry: Arc::clone(telemetry),
        max_skip: ring_frames.min(16_384),
    };

    let mut stream = AudioStreamBuilder::default()
        .set_performance_mode(PerformanceMode::None)
        .set_sharing_mode(SharingMode::Shared)
        .set_usage(Usage::Media)
        .set_content_type(ContentType::Music)
        .set_format::<f32>()
        .set_channel_count::<Stereo>()
        .set_callback(cb)
        .open_stream()
        .map_err(|e| AudioError::Device(format!("oboe open_stream failed: {e:?}")))?;

    let raw_rate = stream.get_sample_rate();
    let out_rate = u32::try_from(raw_rate).unwrap_or(48_000).max(8_000);
    telemetry.output_rate.store(out_rate, Ordering::Relaxed);

    if start_playing {
        stream
            .start()
            .map_err(|e| AudioError::Device(format!("oboe start failed: {e:?}")))?;
    }

    Ok((
        AndroidOutputStream { stream },
        producer,
        out_rate,
        "Android System Output".to_string(),
    ))
}
