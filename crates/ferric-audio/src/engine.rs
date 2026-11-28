use crate::dsp::{
    BassSwapMixer, SpectrumAnalyzer8, StereoEqProcessor, db_to_linear, smooth_fade_envelope,
};
use crate::{AudioError, AudioResult};
#[cfg(not(target_os = "android"))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ferric_core::eq::EqConfig;
use ferric_core::models::{FadeCurve, PlaybackDspSettings};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Adjustable, Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters, WindowFunction,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::common::{Limit, Tier};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Duration as SymDuration, Time};
use symphonia_opus::OpusDecoder;

/// Lock-free atomic snapshot read by the UI thread with zero mutex contention.
#[derive(Debug, Default)]
pub struct AudioTelemetry {
    pub is_playing: AtomicBool,
    pub position_ms: AtomicU64,
    pub duration_ms: AtomicU64,
    pub sample_rate: AtomicU32,
    pub output_rate: AtomicU32,
    pub track_finished_seq: AtomicU64,
    /// `true` when `Deck B` was already promoted gaplessly/crossfaded to `Deck A` on the most
    /// recent `track_finished_seq` increment so the UI updates metadata without cutting audio.
    pub auto_transitioned: AtomicBool,
    /// `true` while Deck A and Deck B are actively overlapping in a crossfade transition.
    pub is_crossfading: AtomicBool,
    /// Lock-free 8-band visualizer energy levels (`0..=1000`).
    pub spectrum_bins: [AtomicU32; 8],
    /// Monotonic epoch incremented on seek/load/stop so the RT callback drops stale frames
    /// without a race between `flush_ring` and newly pushed samples.
    pub(crate) active_epoch: AtomicU32,
    /// Set to `true` when WASAPI/cpal reports a stream disconnect or device stall.
    pub(crate) device_lost: AtomicBool,
    /// Incremented by the audio output callback on every render burst to detect silent stalls.
    pub(crate) cb_heartbeat: AtomicU64,
    /// Set to `true` once paused/stopped playback has finished microfade + ring-buffer drain so
    /// the host WASAPI stream can pause at 0 Hz CPU until the next play command.
    pub(crate) idle_silent: AtomicBool,
}

impl AudioTelemetry {
    pub fn snapshot(&self) -> TelemetrySnapshot {
        // Acquire-load `track_finished_seq` FIRST so subsequent loads observe the metadata and
        // `auto_transitioned` writes published before `track_finished_seq.fetch_add(1, AcqRel)`.
        let track_finished_seq = self.track_finished_seq.load(Ordering::Acquire);
        let auto_transitioned = self.auto_transitioned.load(Ordering::Acquire);
        let is_playing = self.is_playing.load(Ordering::Acquire);
        let mut bins = [0_u32; 8];
        for (i, slot) in self.spectrum_bins.iter().enumerate() {
            bins[i] = slot.load(Ordering::Relaxed);
        }
        TelemetrySnapshot {
            is_playing,
            position_ms: self.position_ms.load(Ordering::Relaxed),
            duration_ms: self.duration_ms.load(Ordering::Relaxed),
            sample_rate: self.sample_rate.load(Ordering::Relaxed),
            output_rate: self.output_rate.load(Ordering::Relaxed),
            track_finished_seq,
            auto_transitioned,
            is_crossfading: self.is_crossfading.load(Ordering::Relaxed),
            spectrum_bins: bins,
        }
    }

    #[inline]
    pub(crate) fn bump_epoch(&self) -> u32 {
        self.active_epoch
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }

    fn store_spectrum(&self, bins: [u32; 8]) {
        for (slot, val) in self.spectrum_bins.iter().zip(bins) {
            slot.store(val, Ordering::Relaxed);
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TelemetrySnapshot {
    pub is_playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub output_rate: u32,
    pub track_finished_seq: u64,
    pub auto_transitioned: bool,
    pub is_crossfading: bool,
    pub spectrum_bins: [u32; 8],
}

#[derive(Debug)]
pub enum EngineCommand {
    LoadAndPlay(PathBuf),
    LoadAndPlayTrack {
        path: PathBuf,
        replay_gain_db: i32,
        fallback_duration_ms: u64,
    },
    CrossfadeToTrack {
        path: PathBuf,
        replay_gain_db: i32,
        fallback_duration_ms: u64,
        overlap_ms: u32,
    },
    QueueNext(Option<PathBuf>),
    QueueNextTrack {
        path: Option<PathBuf>,
        same_album_consecutive: bool,
        replay_gain_db: i32,
        fallback_duration_ms: u64,
    },
    Play,
    Pause,
    TogglePlayPause,
    Stop,
    SeekMs(u64),
    SetVolume(f32),
    /// Multiplicative audio-focus ducking (`true` ramps smoothly to `0.25x`, `false` restores `1.0x`)
    /// without altering the user's persisted volume setting.
    SetDucking(bool),
    SetEq(EqConfig),
    SetCrossfadeMs(u32),
    SetDspSettings(PlaybackDspSettings),
    #[doc(hidden)]
    ReplaceOutputRing {
        producer: rtrb::Producer<(u32, [f32; 2])>,
        out_rate: u32,
    },
    Shutdown,
}

#[inline]
pub(crate) fn pop_epoch_frame(
    consumer: &mut rtrb::Consumer<(u32, [f32; 2])>,
    active_epoch: u32,
    max_skip: usize,
) -> [f32; 2] {
    for _ in 0..max_skip {
        match consumer.pop() {
            Ok((ep, frame)) if ep == active_epoch => return frame,
            Ok(_) => continue,
            Err(_) => return [0.0, 0.0],
        }
    }
    [0.0, 0.0]
}

pub(crate) type RingFrame = (u32, [f32; 2]);

/// Smooth multiplicative gain ramp for Android transient audio-focus ducking.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DuckingState {
    ducked: bool,
    current_gain: f32,
}

impl Default for DuckingState {
    fn default() -> Self {
        Self {
            ducked: false,
            current_gain: 1.0,
        }
    }
}

impl DuckingState {
    const DUCK_TARGET_GAIN: f32 = 0.25;
    const RAMP_SECS: f32 = 0.12;

    pub(crate) fn set_ducked(&mut self, ducked: bool) {
        self.ducked = ducked;
    }

    #[cfg(test)]
    pub(crate) fn current_gain(self) -> f32 {
        self.current_gain
    }

    pub(crate) fn step_block(&mut self, frames: usize, out_rate: u32) {
        let target = if self.ducked {
            Self::DUCK_TARGET_GAIN
        } else {
            1.0
        };
        let diff = target - self.current_gain;
        if diff.abs() < 1e-4 {
            self.current_gain = target;
            return;
        }
        let ramp_frames = (out_rate.max(8_000) as f32 * Self::RAMP_SECS).max(1.0);
        let max_delta = (frames as f32 / ramp_frames).clamp(0.005, 1.0);
        if diff > 0.0 {
            self.current_gain = (self.current_gain + max_delta).min(target);
        } else {
            self.current_gain = (self.current_gain - max_delta).max(target);
        }
    }

    #[inline]
    pub(crate) fn effective_volume(self, user_volume: f32) -> f32 {
        (user_volume * self.current_gain).clamp(0.0, 1.5)
    }
}

#[cfg(not(target_os = "android"))]
struct PlatformOutputStream(cpal::Stream);

#[cfg(not(target_os = "android"))]
impl PlatformOutputStream {
    fn play(&mut self) -> Result<(), String> {
        self.0.play().map_err(|e| e.to_string())
    }

    fn pause(&mut self) -> Result<(), String> {
        self.0.pause().map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "android")]
struct PlatformOutputStream(crate::android_oboe::AndroidOutputStream);

#[cfg(target_os = "android")]
impl PlatformOutputStream {
    fn play(&mut self) -> Result<(), String> {
        self.0.play()
    }

    fn pause(&mut self) -> Result<(), String> {
        self.0.pause()
    }
}

type OutputStreamBundle = (PlatformOutputStream, rtrb::Producer<RingFrame>, u32, String);

/// Enumerate available audio output device names on the host (deduplicated, sorted).
pub fn list_output_devices() -> Vec<String> {
    #[cfg(target_os = "android")]
    {
        crate::android_oboe::list_output_devices()
    }
    #[cfg(not(target_os = "android"))]
    {
        let host = cpal::default_host();
        let Ok(devices) = host.output_devices() else {
            return Vec::new();
        };
        let mut names = Vec::new();
        for dev in devices {
            if let Ok(desc) = dev.description() {
                let name = desc.name().trim().to_string();
                if !name.is_empty() && !names.iter().any(|existing: &String| existing == &name) {
                    names.push(name);
                }
            }
        }
        names.sort_by(|a, b| ferric_core::text::cmp_ascii_icase(a, b));
        names
    }
}

#[cfg(not(target_os = "android"))]
fn select_output_device(
    host: &cpal::Host,
    preferred_name: Option<&str>,
) -> AudioResult<(cpal::Device, String)> {
    if let Some(target) = preferred_name.map(str::trim).filter(|s| !s.is_empty())
        && let Ok(devices) = host.output_devices()
    {
        for dev in devices {
            if let Ok(desc) = dev.description()
                && desc.name().trim().eq_ignore_ascii_case(target)
            {
                let resolved = desc.name().trim().to_string();
                return Ok((dev, resolved));
            }
        }
    }
    let device = host
        .default_output_device()
        .ok_or(AudioError::NoOutputDevice)?;
    let active_name = device
        .description()
        .ok()
        .map(|d| d.name().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "System Default".to_string());
    Ok((device, active_name))
}

#[cfg(target_os = "android")]
fn build_output_stream_and_producer(
    telemetry: &Arc<AudioTelemetry>,
    preferred_device: Option<&str>,
    start_playing: bool,
) -> AudioResult<OutputStreamBundle> {
    let (stream, producer, out_rate, name) = crate::android_oboe::build_output_stream_and_producer(
        telemetry,
        preferred_device,
        start_playing,
    )?;
    Ok((PlatformOutputStream(stream), producer, out_rate, name))
}

#[cfg(not(target_os = "android"))]
fn build_output_stream_and_producer(
    telemetry: &Arc<AudioTelemetry>,
    preferred_device: Option<&str>,
    start_playing: bool,
) -> AudioResult<OutputStreamBundle> {
    let host = cpal::default_host();
    let (device, active_name) = select_output_device(&host, preferred_device)?;
    let default_cfg = device
        .default_output_config()
        .map_err(|e| AudioError::Device(e.to_string()))?;

    let out_rate = default_cfg.sample_rate().max(8_000);
    let out_channels = usize::from(default_cfg.channels()).max(1);
    let sample_format = default_cfg.sample_format();
    let stream_cfg: cpal::StreamConfig = default_cfg.into();

    // ~260 ms wait-free SPSC ring buffer of epoch-tagged atomic stereo `[L, R]` frames.
    let ring_frames = ((out_rate as usize * 260) / 1000).max(4096);
    let (producer, mut consumer) = rtrb::RingBuffer::<(u32, [f32; 2])>::new(ring_frames);

    telemetry.output_rate.store(out_rate, Ordering::Relaxed);
    let cb_telemetry = Arc::clone(telemetry);
    let err_telemetry = Arc::clone(telemetry);

    let err_fn = move |_err| {
        #[cfg(debug_assertions)]
        eprintln!("ferric-audio stream error: {_err}");
        err_telemetry.device_lost.store(true, Ordering::Release);
    };

    let max_skip = ring_frames.min(16_384);
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            stream_cfg,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                cb_telemetry.cb_heartbeat.fetch_add(1, Ordering::Relaxed);
                let cur_epoch = cb_telemetry.active_epoch.load(Ordering::Acquire);
                for frame in data.chunks_exact_mut(out_channels) {
                    let [l, r] = pop_epoch_frame(&mut consumer, cur_epoch, max_skip);
                    if out_channels == 1 {
                        frame[0] = 0.5 * (l + r);
                    } else {
                        frame[0] = l;
                        frame[1] = r;
                        for ch in frame.iter_mut().skip(2) {
                            *ch = 0.0;
                        }
                    }
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            stream_cfg,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                cb_telemetry.cb_heartbeat.fetch_add(1, Ordering::Relaxed);
                let cur_epoch = cb_telemetry.active_epoch.load(Ordering::Acquire);
                for frame in data.chunks_exact_mut(out_channels) {
                    let [l, r] = pop_epoch_frame(&mut consumer, cur_epoch, max_skip);
                    let to_i16 = |s: f32| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                    if out_channels == 1 {
                        frame[0] = to_i16(0.5 * (l + r));
                    } else {
                        frame[0] = to_i16(l);
                        frame[1] = to_i16(r);
                        for ch in frame.iter_mut().skip(2) {
                            *ch = 0;
                        }
                    }
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_output_stream(
            stream_cfg,
            move |data: &mut [i32], _: &cpal::OutputCallbackInfo| {
                cb_telemetry.cb_heartbeat.fetch_add(1, Ordering::Relaxed);
                let cur_epoch = cb_telemetry.active_epoch.load(Ordering::Acquire);
                for frame in data.chunks_exact_mut(out_channels) {
                    let [l, r] = pop_epoch_frame(&mut consumer, cur_epoch, max_skip);
                    let to_i32 =
                        |s: f32| (f64::from(s.clamp(-1.0, 1.0)) * 2_147_483_647.0).round() as i32;
                    if out_channels == 1 {
                        frame[0] = to_i32(0.5 * (l + r));
                    } else {
                        frame[0] = to_i32(l);
                        frame[1] = to_i32(r);
                        for ch in frame.iter_mut().skip(2) {
                            *ch = 0;
                        }
                    }
                }
            },
            err_fn,
            None,
        ),
        _ => device.build_output_stream(
            stream_cfg,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                cb_telemetry.cb_heartbeat.fetch_add(1, Ordering::Relaxed);
                let cur_epoch = cb_telemetry.active_epoch.load(Ordering::Acquire);
                for frame in data.chunks_exact_mut(out_channels) {
                    let [l, r] = pop_epoch_frame(&mut consumer, cur_epoch, max_skip);
                    if out_channels == 1 {
                        frame[0] = 0.5 * (l + r);
                    } else {
                        frame[0] = l;
                        frame[1] = r;
                        for ch in frame.iter_mut().skip(2) {
                            *ch = 0.0;
                        }
                    }
                }
            },
            err_fn,
            None,
        ),
    }
    .map_err(|e| AudioError::Device(e.to_string()))?;

    let mut wrapped = PlatformOutputStream(stream);
    if start_playing {
        wrapped.play().map_err(AudioError::Device)?;
    }

    Ok((wrapped, producer, out_rate, active_name))
}

struct OutputSlotState {
    stream: Option<PlatformOutputStream>,
    preferred_device: Option<String>,
    active_device_name: String,
    stream_paused: bool,
    last_recovery_attempt: Option<std::time::Instant>,
}

fn poll_recovery_inner(
    slot_lock: &Mutex<OutputSlotState>,
    telemetry: &Arc<AudioTelemetry>,
    cmd_tx: &Sender<EngineCommand>,
) -> bool {
    let mut slot = slot_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !slot.stream_paused
        && telemetry.idle_silent.load(Ordering::Acquire)
        && !telemetry.is_playing.load(Ordering::Acquire)
        && let Some(stream) = slot.stream.as_mut()
    {
        let _ = stream.pause();
        slot.stream_paused = true;
    }
    if !telemetry.device_lost.load(Ordering::Acquire) {
        return false;
    }
    let now = std::time::Instant::now();
    if let Some(prev) = slot.last_recovery_attempt
        && now.duration_since(prev) < Duration::from_millis(1_500)
    {
        return false;
    }
    slot.last_recovery_attempt = Some(now);
    slot.stream = None;
    let want_play = telemetry.is_playing.load(Ordering::Acquire);
    match build_output_stream_and_producer(telemetry, slot.preferred_device.as_deref(), want_play) {
        Ok((new_stream, new_producer, new_rate, active_name)) => {
            slot.stream = Some(new_stream);
            slot.stream_paused = !want_play;
            telemetry.device_lost.store(false, Ordering::Release);
            slot.active_device_name = active_name;
            let _ = cmd_tx.send(EngineCommand::ReplaceOutputRing {
                producer: new_producer,
                out_rate: new_rate,
            });
            true
        }
        Err(_) => {
            // Preserve telemetry.is_playing so the next recovery retry resumes automatically
            // once Bluetooth/USB audio routing settles.
            slot.stream_paused = true;
            telemetry.device_lost.store(true, Ordering::Release);
            false
        }
    }
}

pub struct AudioEngine {
    cmd_tx: Sender<EngineCommand>,
    telemetry: Arc<AudioTelemetry>,
    output_slot: Arc<Mutex<OutputSlotState>>,
    decode_handle: Option<thread::JoinHandle<()>>,
    supervisor_stop: Arc<AtomicBool>,
    supervisor_wake: Arc<(Mutex<bool>, Condvar)>,
    supervisor_handle: Option<thread::JoinHandle<()>>,
    headless: bool,
    preferred_device: Option<String>,
    active_device_name: String,
}

impl AudioEngine {
    /// Start the real-time output stream on the system default device.
    pub fn start(initial_eq: EqConfig) -> AudioResult<Self> {
        Self::start_with_device(initial_eq, None)
    }

    /// Start the real-time output stream on `preferred_device` with an internal background
    /// supervisor thread that handles idle-silent suspension and Bluetooth/headset disconnect
    /// recovery even when no UI frame loop is running (recommended for Android background service).
    pub fn start_supervised(
        initial_eq: EqConfig,
        preferred_device: Option<String>,
    ) -> AudioResult<Self> {
        Self::start_internal(initial_eq, preferred_device, true)
    }

    /// Start the real-time output stream on `preferred_device` (falling back to the
    /// system default device when `None` or unavailable) and dedicated dual-deck decode+DSP thread.
    pub fn start_with_device(
        initial_eq: EqConfig,
        preferred_device: Option<String>,
    ) -> AudioResult<Self> {
        Self::start_internal(initial_eq, preferred_device, false)
    }

    fn start_internal(
        initial_eq: EqConfig,
        preferred_device: Option<String>,
        with_supervisor: bool,
    ) -> AudioResult<Self> {
        let telemetry = Arc::new(AudioTelemetry::default());
        telemetry.idle_silent.store(true, Ordering::Relaxed);
        let cleaned_pref = preferred_device
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let (stream, producer, out_rate, active_device_name) =
            build_output_stream_and_producer(&telemetry, cleaned_pref.as_deref(), false)?;

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let thread_telemetry = Arc::clone(&telemetry);
        let handle = thread::Builder::new()
            .name("ferric-audio-dsp".to_string())
            .spawn(move || {
                decode_loop(
                    cmd_rx,
                    producer,
                    out_rate,
                    initial_eq,
                    thread_telemetry,
                    false,
                );
            })?;

        let output_slot = Arc::new(Mutex::new(OutputSlotState {
            stream: Some(stream),
            preferred_device: cleaned_pref.clone(),
            active_device_name: active_device_name.clone(),
            stream_paused: true,
            last_recovery_attempt: None,
        }));
        let supervisor_stop = Arc::new(AtomicBool::new(false));
        let supervisor_wake = Arc::new((Mutex::new(false), Condvar::new()));
        let supervisor_handle = if with_supervisor {
            let sup_slot = Arc::clone(&output_slot);
            let sup_tel = Arc::clone(&telemetry);
            let sup_tx = cmd_tx.clone();
            let sup_stop = Arc::clone(&supervisor_stop);
            let sup_wake = Arc::clone(&supervisor_wake);
            thread::Builder::new()
                .name("ferric-audio-sup".to_string())
                .spawn(move || {
                    let (wake_lock, wake_cv) = &*sup_wake;
                    while !sup_stop.load(Ordering::Acquire) {
                        poll_recovery_inner(&sup_slot, &sup_tel, &sup_tx);
                        let active = sup_tel.is_playing.load(Ordering::Acquire)
                            || sup_tel.device_lost.load(Ordering::Acquire)
                            || !sup_tel.idle_silent.load(Ordering::Acquire);
                        let mut guard = wake_lock.lock().unwrap_or_else(|p| p.into_inner());
                        if sup_stop.load(Ordering::Acquire) {
                            break;
                        }
                        if *guard {
                            *guard = false;
                            continue;
                        }
                        if active {
                            let (g, _) = wake_cv
                                .wait_timeout(guard, Duration::from_millis(450))
                                .unwrap_or_else(|p| p.into_inner());
                            let mut g = g;
                            *g = false;
                        } else {
                            // Standby while paused & drained: sleep indefinitely until Play/Drop wakes us.
                            let (g, _) = wake_cv
                                .wait_timeout(guard, Duration::from_secs(30))
                                .unwrap_or_else(|p| p.into_inner());
                            let mut g = g;
                            *g = false;
                        }
                    }
                })
                .ok()
        } else {
            None
        };

        Ok(Self {
            cmd_tx,
            telemetry,
            output_slot,
            decode_handle: Some(handle),
            supervisor_stop,
            supervisor_wake,
            supervisor_handle,
            headless: false,
            preferred_device: cleaned_pref,
            active_device_name,
        })
    }

    /// Headless no-device fallback (used when no sound card is attached, e.g., CI runners).
    pub fn start_headless(initial_eq: EqConfig) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let telemetry = Arc::new(AudioTelemetry::default());
        telemetry.output_rate.store(48_000, Ordering::Relaxed);
        let thread_telemetry = Arc::clone(&telemetry);
        let (producer, _consumer) = rtrb::RingBuffer::<(u32, [f32; 2])>::new(4096);
        let handle = thread::Builder::new()
            .name("ferric-audio-headless".to_string())
            .spawn(move || {
                decode_loop(cmd_rx, producer, 48_000, initial_eq, thread_telemetry, true);
            })
            .ok();
        Self {
            cmd_tx,
            telemetry,
            output_slot: Arc::new(Mutex::new(OutputSlotState {
                stream: None,
                preferred_device: None,
                active_device_name: "Headless (No Audio Device)".to_string(),
                stream_paused: true,
                last_recovery_attempt: None,
            })),
            decode_handle: handle,
            supervisor_stop: Arc::new(AtomicBool::new(true)),
            supervisor_wake: Arc::new((Mutex::new(false), Condvar::new())),
            supervisor_handle: None,
            headless: true,
            preferred_device: None,
            active_device_name: "Headless (No Audio Device)".to_string(),
        }
    }

    /// Currently active host audio output device name.
    #[must_use]
    pub fn active_device_name(&self) -> &str {
        &self.active_device_name
    }

    /// Configured preferred device (`None` means System Default).
    #[must_use]
    pub fn preferred_device(&self) -> Option<&str> {
        self.preferred_device.as_deref()
    }

    /// Hot-switch the live output stream to a specific device name (`None` = System Default)
    /// without restarting the application or losing playback position.
    pub fn switch_output_device(
        &mut self,
        preferred_device: Option<String>,
    ) -> AudioResult<String> {
        let cleaned_pref = preferred_device
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        self.preferred_device = cleaned_pref.clone();
        if self.headless {
            return Ok(self.active_device_name.clone());
        }
        let want_play = self.telemetry.is_playing.load(Ordering::Acquire);
        let mut slot = self
            .output_slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        slot.preferred_device = cleaned_pref;
        // Drop old stream first so exclusive/shared audio sessions release cleanly before re-opening.
        slot.stream = None;
        match build_output_stream_and_producer(
            &self.telemetry,
            slot.preferred_device.as_deref(),
            want_play,
        ) {
            Ok((new_stream, new_producer, new_rate, active_name)) => {
                slot.stream = Some(new_stream);
                slot.stream_paused = !want_play;
                self.telemetry.device_lost.store(false, Ordering::Release);
                slot.active_device_name = active_name.clone();
                self.active_device_name = active_name.clone();
                let _ = self.cmd_tx.send(EngineCommand::ReplaceOutputRing {
                    producer: new_producer,
                    out_rate: new_rate,
                });
                Ok(active_name)
            }
            Err(err) => {
                self.telemetry.is_playing.store(false, Ordering::Release);
                slot.stream_paused = true;
                Err(err)
            }
        }
    }

    /// Detect audio device disconnects/stalls (e.g., USB DAC or Bluetooth headset unplugged)
    /// and rebuild the output stream on the preferred or default device without wedging decode.
    /// Also suspends the host output stream once paused/stopped audio finishes draining so idle
    /// CPU drops to 0 Hz.
    pub fn poll_device_recovery(&mut self) -> bool {
        if self.headless {
            return false;
        }
        let recovered = poll_recovery_inner(&self.output_slot, &self.telemetry, &self.cmd_tx);
        if recovered {
            let slot = self
                .output_slot
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.active_device_name = slot.active_device_name.clone();
        }
        recovered
    }

    fn wake_supervisor(&self) {
        let (lock, cv) = &*self.supervisor_wake;
        let mut flag = lock.lock().unwrap_or_else(|p| p.into_inner());
        *flag = true;
        cv.notify_all();
    }

    pub fn send(&self, cmd: EngineCommand) {
        let needs_audio_wake = matches!(
            cmd,
            EngineCommand::LoadAndPlay(_)
                | EngineCommand::LoadAndPlayTrack { .. }
                | EngineCommand::CrossfadeToTrack { .. }
                | EngineCommand::Play
                | EngineCommand::TogglePlayPause
        );
        if needs_audio_wake {
            self.telemetry.idle_silent.store(false, Ordering::Release);
            {
                let mut slot = self
                    .output_slot
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if slot.stream_paused {
                    slot.stream_paused = false;
                    if let Some(stream) = slot.stream.as_mut()
                        && stream.play().is_err()
                    {
                        slot.stream_paused = true;
                        self.telemetry.device_lost.store(true, Ordering::Release);
                    }
                }
            }
            self.wake_supervisor();
        }
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn telemetry(&self) -> TelemetrySnapshot {
        self.telemetry.snapshot()
    }

    pub fn telemetry_arc(&self) -> &Arc<AudioTelemetry> {
        &self.telemetry
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.supervisor_stop.store(true, Ordering::Release);
        self.wake_supervisor();
        let _ = self.cmd_tx.send(EngineCommand::Shutdown);
        if let Some(sup) = self.supervisor_handle.take() {
            let _ = sup.join();
        }
        {
            let mut slot = self
                .output_slot
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            slot.stream = None;
        }
        if let Some(handle) = self.decode_handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Debug, Clone)]
struct QueuedTrackSpec {
    path: PathBuf,
    same_album_consecutive: bool,
    replay_gain_db: i32,
    fallback_duration_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct ActiveCrossfade {
    elapsed_frames: u64,
    total_frames: u64,
    curve: FadeCurve,
    is_manual_switch: bool,
    ui_handed_off: bool,
}

#[derive(Debug, Clone, Copy)]
enum MicroFade {
    Steady(f32),
    FadingIn {
        start_gain: f32,
        elapsed_frames: u64,
        total_frames: u64,
    },
    Pausing {
        start_gain: f32,
        elapsed_frames: u64,
        total_frames: u64,
    },
}

impl MicroFade {
    #[inline]
    fn current_gain(self) -> f32 {
        match self {
            Self::Steady(g) => g,
            Self::FadingIn {
                start_gain,
                elapsed_frames,
                total_frames,
            } => {
                let t = (elapsed_frames as f32 / total_frames.max(1) as f32).clamp(0.0, 1.0);
                start_gain + (1.0 - start_gain) * smooth_fade_envelope(t)
            }
            Self::Pausing {
                start_gain,
                elapsed_frames,
                total_frames,
            } => {
                let t = (elapsed_frames as f32 / total_frames.max(1) as f32).clamp(0.0, 1.0);
                start_gain * (1.0 - smooth_fade_envelope(t))
            }
        }
    }

    #[inline]
    fn step_frame(&mut self) -> (f32, bool) {
        match self {
            Self::Steady(g) => (*g, false),
            Self::FadingIn {
                start_gain,
                elapsed_frames,
                total_frames,
            } => {
                let total = (*total_frames).max(1);
                let t = (*elapsed_frames as f32 / total as f32).clamp(0.0, 1.0);
                let g = *start_gain + (1.0 - *start_gain) * smooth_fade_envelope(t);
                *elapsed_frames += 1;
                if *elapsed_frames >= total {
                    *self = Self::Steady(1.0);
                }
                (g, false)
            }
            Self::Pausing {
                start_gain,
                elapsed_frames,
                total_frames,
            } => {
                let total = (*total_frames).max(1);
                let t = (*elapsed_frames as f32 / total as f32).clamp(0.0, 1.0);
                let g = *start_gain * (1.0 - smooth_fade_envelope(t));
                *elapsed_frames += 1;
                if *elapsed_frames >= total {
                    *self = Self::Steady(0.0);
                    return (0.0, true);
                }
                (g, false)
            }
        }
    }
}

struct ActiveDeck {
    path: PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    sample_rate: u32,
    channels: usize,
    duration_ms: u64,
    time_base: Option<symphonia::core::units::TimeBase>,
    resampler: Option<StereoResampler>,
    pending_stereo: Vec<f32>,
    stereo_scratch: Vec<f32>,
    pending_offset: usize,
    pos_ms_f64: f64,
    replay_gain_db: i32,
    replay_gain_linear: f32,
    last_chunk_rms: f32,
    quiet_outro_frames: usize,
    eof: bool,
    tail_flushed: bool,
}

#[inline]
fn safe_speed(speed: f32) -> f32 {
    if speed.is_finite() {
        speed.clamp(0.5, 2.0)
    } else {
        1.0
    }
}

fn read_ebml_vint(buf: &[u8], pos: usize, keep_marker: bool) -> Option<(u64, usize)> {
    let first = *buf.get(pos)?;
    if first == 0 {
        return None;
    }
    let lz = first.leading_zeros() as usize;
    let len = lz + 1;
    if len > 8 || pos + len > buf.len() {
        return None;
    }
    let mut val = if keep_marker {
        u64::from(first)
    } else {
        u64::from(first & (0x7f_u8 >> lz))
    };
    for &b in &buf[pos + 1..pos + len] {
        val = (val << 8) | u64::from(b);
    }
    Some((val, len))
}

fn parse_ebml_info_duration_ms(buf: &[u8]) -> Option<u64> {
    // Locate Matroska Segment/Info element (ID 0x1549A966) and read TimecodeScale + Duration.
    const INFO_ID: [u8; 4] = [0x15, 0x49, 0xA9, 0x66];
    let mut offset = 0usize;
    while let Some(rel) = buf[offset..].windows(4).position(|w| w == INFO_ID) {
        let start = offset + rel;
        if let Some((info_size, size_len)) = read_ebml_vint(buf, start + 4, false)
            && (4..=4096).contains(&info_size)
        {
            let body_start = start + 4 + size_len;
            let body_end = body_start.saturating_add(info_size as usize).min(buf.len());
            let mut cur = body_start;
            let mut timecode_scale_ns: u64 = 1_000_000;
            let mut duration_units: Option<f64> = None;

            while cur < body_end {
                let Some((id, id_len)) = read_ebml_vint(buf, cur, true) else {
                    break;
                };
                cur += id_len;
                let Some((sz, sz_len)) = read_ebml_vint(buf, cur, false) else {
                    break;
                };
                cur += sz_len;
                let end = cur.saturating_add(sz as usize);
                if end > body_end {
                    break;
                }
                let payload = &buf[cur..end];
                match id {
                    0x2AD7B1 if !payload.is_empty() && payload.len() <= 8 => {
                        let mut v = 0u64;
                        for &b in payload {
                            v = (v << 8) | u64::from(b);
                        }
                        if v > 0 {
                            timecode_scale_ns = v;
                        }
                    }
                    0x4489 if payload.len() == 4 => {
                        let d = f64::from(f32::from_be_bytes([
                            payload[0], payload[1], payload[2], payload[3],
                        ]));
                        if d.is_finite() && d > 0.0 {
                            duration_units = Some(d);
                        }
                    }
                    0x4489 if payload.len() == 8 => {
                        let d = f64::from_be_bytes([
                            payload[0], payload[1], payload[2], payload[3], payload[4], payload[5],
                            payload[6], payload[7],
                        ]);
                        if d.is_finite() && d > 0.0 {
                            duration_units = Some(d);
                        }
                    }
                    _ => {}
                }
                cur = end;
            }
            if let Some(d) = duration_units {
                let ms = (d * (timecode_scale_ns as f64) / 1_000_000.0).round();
                if ms.is_finite() && ms > 0.0 {
                    return Some(ms as u64);
                }
            }
        }
        offset = start + 1;
    }
    None
}

fn probe_webm_duration_ms(path: &Path) -> Option<u64> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if ext != "webm" && ext != "mkv" {
        return None;
    }
    use std::io::Read;
    let mut f = crate::content::open_audio_file(path).ok()?;
    let mut buf = vec![0u8; 8192];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    parse_ebml_info_duration_ms(&buf)
}

impl ActiveDeck {
    fn open(
        path: &Path,
        out_rate: u32,
        playback_speed: f32,
        replay_gain_db: i32,
        fallback_duration_ms: u64,
        codecs: &CodecRegistry,
    ) -> AudioResult<Self> {
        let file = crate::content::open_audio_file(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let meta_opts = MetadataOptions::default()
            .limit_tag_bytes(Limit::Maximum(0))
            .limit_visual_bytes(Limit::Maximum(0));
        let format = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), meta_opts)
            .map_err(|e| AudioError::Decode(e.to_string()))?;

        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| AudioError::Decode("no audio track found".to_string()))?;
        let track_id = track.id;
        let time_base = track.time_base;
        let num_frames = track.num_frames;
        let codec_params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| AudioError::Decode("missing audio codec parameters".to_string()))?
            .clone();

        let sample_rate = codec_params.sample_rate.unwrap_or(44_100);
        let channels = codec_params
            .channels
            .as_ref()
            .map(|c| c.count())
            .unwrap_or(2)
            .max(1);
        let probed_duration_ms = match (time_base, num_frames) {
            (Some(tb), Some(frames)) => tb
                .calc_duration(SymDuration::from(frames))
                .map(|t| t.as_millis().max(0) as u64)
                .unwrap_or(0),
            (_, Some(frames)) if sample_rate > 0 => {
                frames.saturating_mul(1_000) / u64::from(sample_rate)
            }
            _ => 0,
        };
        let duration_ms = if probed_duration_ms > 0 {
            probed_duration_ms
        } else if let Some(wd) = probe_webm_duration_ms(path).filter(|&d| d > 0) {
            wd
        } else {
            fallback_duration_ms
        };

        let decoder = codecs
            .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
            .map_err(|e| AudioError::Decode(e.to_string()))?;

        let clamped_speed = safe_speed(playback_speed);
        let needs_resampler = sample_rate != out_rate || (clamped_speed - 1.0).abs() > 1e-4;
        let resampler = if needs_resampler {
            StereoResampler::new(sample_rate, out_rate, clamped_speed).ok()
        } else {
            None
        };

        let rg_clamped = replay_gain_db.clamp(-12, 6);
        let replay_gain_linear = db_to_linear(rg_clamped as f32);

        Ok(Self {
            path: path.to_path_buf(),
            format,
            decoder,
            track_id,
            sample_rate,
            channels,
            duration_ms,
            time_base,
            resampler,
            pending_stereo: Vec::with_capacity(8192),
            stereo_scratch: Vec::new(),
            pending_offset: 0,
            pos_ms_f64: 0.0,
            replay_gain_db: rg_clamped,
            replay_gain_linear,
            last_chunk_rms: 1.0,
            quiet_outro_frames: 0,
            eof: false,
            tail_flushed: false,
        })
    }

    fn set_playback_speed(&mut self, out_rate: u32, speed: f32) {
        let clamped = safe_speed(speed);
        if let Some(res) = &mut self.resampler {
            res.set_speed(clamped);
        } else if (clamped - 1.0).abs() > 1e-4
            && let Ok(res) = StereoResampler::new(self.sample_rate, out_rate, clamped)
        {
            self.resampler = Some(res);
        }
    }

    fn update_output_rate(&mut self, out_rate: u32, speed: f32) {
        let clamped = safe_speed(speed);
        let needs_resampler = self.sample_rate != out_rate || (clamped - 1.0).abs() > 1e-4;
        self.resampler = if needs_resampler {
            StereoResampler::new(self.sample_rate, out_rate, clamped).ok()
        } else {
            None
        };
    }

    fn seek_ms(&mut self, target_ms: u64) -> AudioResult<u64> {
        let clamped_ms = if self.duration_ms > 50 {
            target_ms.min(self.duration_ms.saturating_sub(50))
        } else {
            target_ms
        };
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from_millis_u64(clamped_ms),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| AudioError::Decode(e.to_string()))?;
        self.decoder.reset();
        self.pending_stereo.clear();
        self.pending_offset = 0;
        self.eof = false;
        self.tail_flushed = false;
        self.last_chunk_rms = 1.0;
        self.quiet_outro_frames = 0;
        if let Some(res) = &mut self.resampler {
            res.reset();
        }
        // Symphonia demuxers land on the preceding keyframe/Cue/Ogg page (`actual_ts <= required_ts`).
        // Fast-forward decode and discard intermediate packets so WebM/Opus/Ogg/MP3 seeks land
        // accurately instead of snapping backward by 2-8 seconds.
        let mut landed_ts = seeked.actual_ts;
        if seeked.actual_ts < seeked.required_ts {
            for _ in 0..256 {
                match self.format.next_packet() {
                    Ok(Some(packet)) => {
                        if packet.track_id != self.track_id {
                            continue;
                        }
                        let pkt_end = packet.pts.saturating_add(packet.dur);
                        let _ = self.decoder.decode(&packet);
                        landed_ts = pkt_end;
                        if pkt_end >= seeked.required_ts {
                            break;
                        }
                    }
                    Ok(None) | Err(_) => break,
                }
            }
        }
        let actual_ms = self
            .time_base
            .and_then(|tb| tb.calc_time(landed_ts))
            .map(|t| t.as_millis().max(0) as u64)
            .unwrap_or(clamped_ms);
        self.pos_ms_f64 = actual_ms as f64;
        Ok(actual_ms)
    }

    #[inline]
    fn current_pos_ms(&self) -> u64 {
        let raw = if self.pos_ms_f64.is_finite() {
            self.pos_ms_f64.max(0.0).round() as u64
        } else {
            0
        };
        if self.duration_ms > 0 {
            raw.min(self.duration_ms)
        } else {
            raw
        }
    }

    #[inline]
    fn remaining_ms(&self) -> u64 {
        self.duration_ms.saturating_sub(self.current_pos_ms())
    }

    /// Pre-decode the initial chunk and optionally skip leading digital silence (`|sample| < 0.0025`,
    /// up to `max_trim_secs` seconds) so `Deck B` enters with zero dead gap.
    fn prime_first_chunk(
        &mut self,
        raw_buf: &mut Vec<f32>,
        trim_leading_silence: bool,
        max_trim_secs: f32,
        out_rate: u32,
        playback_speed: f32,
    ) {
        if !trim_leading_silence {
            if self.pending_offset >= self.pending_stereo.len() {
                self.decode_next_chunk(raw_buf);
            }
            return;
        }

        let max_trim_frames = ((out_rate.max(8_000) as f32) * max_trim_secs).round() as usize;
        let step_ms =
            (1_000.0 * f64::from(safe_speed(playback_speed))) / f64::from(out_rate.max(8_000));
        let mut trimmed = 0usize;

        while trimmed < max_trim_frames {
            if self.pending_offset + 1 >= self.pending_stereo.len()
                && !self.decode_next_chunk(raw_buf)
            {
                break;
            }
            while self.pending_offset + 1 < self.pending_stereo.len() && trimmed < max_trim_frames {
                let l = self.pending_stereo[self.pending_offset];
                let r = self.pending_stereo[self.pending_offset + 1];
                if l.abs() < 0.0025 && r.abs() < 0.0025 {
                    self.pending_offset += 2;
                    self.pos_ms_f64 += step_ms;
                    trimmed += 1;
                } else {
                    // Reset quiet-outro counter after priming so intro level starts clean.
                    self.quiet_outro_frames = 0;
                    return;
                }
            }
        }
        self.quiet_outro_frames = 0;
    }

    #[inline]
    fn next_stereo_frame(
        &mut self,
        raw_buf: &mut Vec<f32>,
        out_rate: u32,
        playback_speed: f32,
        apply_replay_gain: bool,
    ) -> Option<(f32, f32)> {
        if self.pending_offset + 1 >= self.pending_stereo.len() && !self.decode_next_chunk(raw_buf)
        {
            return None;
        }
        if self.pending_offset + 1 >= self.pending_stereo.len() {
            return None;
        }
        let mut l = self.pending_stereo[self.pending_offset];
        let mut r = self.pending_stereo[self.pending_offset + 1];
        self.pending_offset += 2;

        let step_ms =
            (1_000.0 * f64::from(safe_speed(playback_speed))) / f64::from(out_rate.max(8_000));
        self.pos_ms_f64 += step_ms;

        if self.last_chunk_rms < 0.003 && l.abs() < 0.004 && r.abs() < 0.004 {
            self.quiet_outro_frames = self.quiet_outro_frames.saturating_add(1);
        } else {
            self.quiet_outro_frames = 0;
        }

        if apply_replay_gain {
            l *= self.replay_gain_linear;
            r *= self.replay_gain_linear;
        }
        Some((l, r))
    }

    /// Decode next chunk into stereo `f32` samples at hardware `out_rate`.
    /// Returns `true` if `pending_stereo` now contains unread frames.
    fn decode_next_chunk(&mut self, raw_buf: &mut Vec<f32>) -> bool {
        if self.eof {
            if !self.tail_flushed {
                self.tail_flushed = true;
                if let Some(res) = &mut self.resampler {
                    self.pending_stereo.clear();
                    self.pending_offset = 0;
                    res.flush_tail(&mut self.pending_stereo);
                    if self.pending_stereo.len() >= 2 {
                        self.last_chunk_rms = compute_rms(&self.pending_stereo);
                        return true;
                    }
                }
            }
            return false;
        }

        const MAX_CONSECUTIVE_DECODE_ERRORS: usize = 64;
        const MAX_CONSECUTIVE_RESETS: usize = 8;
        let mut consecutive_decode_errors = 0usize;
        let mut consecutive_resets = 0usize;

        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(pkt)) => pkt,
                Ok(None) => {
                    self.eof = true;
                    return self.decode_next_chunk(raw_buf);
                }
                Err(SymphoniaError::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    self.eof = true;
                    return self.decode_next_chunk(raw_buf);
                }
                Err(SymphoniaError::ResetRequired) => {
                    consecutive_resets += 1;
                    if consecutive_resets > MAX_CONSECUTIVE_RESETS {
                        self.eof = true;
                        return false;
                    }
                    self.decoder.reset();
                    continue;
                }
                Err(_) => {
                    self.eof = true;
                    return false;
                }
            };

            if packet.track_id != self.track_id {
                continue;
            }

            match self.decoder.decode(&packet) {
                Ok(audio_slice) => {
                    consecutive_decode_errors = 0;
                    consecutive_resets = 0;
                    if audio_slice.frames() == 0 {
                        continue;
                    }
                    let ch = audio_slice.num_planes().max(1);
                    self.channels = ch;
                    raw_buf.clear();
                    audio_slice.copy_to_vec_interleaved::<f32>(raw_buf);

                    self.pending_stereo.clear();
                    self.pending_offset = 0;
                    if ch == 2 {
                        if let Some(res) = &mut self.resampler {
                            res.process_stereo(raw_buf, &mut self.pending_stereo);
                        } else {
                            self.pending_stereo.extend_from_slice(raw_buf);
                        }
                    } else if ch == 1 {
                        if let Some(res) = &mut self.resampler {
                            self.stereo_scratch.clear();
                            self.stereo_scratch.reserve(raw_buf.len() * 2);
                            for &s in raw_buf.iter() {
                                self.stereo_scratch.push(s);
                                self.stereo_scratch.push(s);
                            }
                            res.process_stereo(&self.stereo_scratch, &mut self.pending_stereo);
                        } else {
                            self.pending_stereo.reserve(raw_buf.len() * 2);
                            for &s in raw_buf.iter() {
                                self.pending_stereo.push(s);
                                self.pending_stereo.push(s);
                            }
                        }
                    } else if let Some(res) = &mut self.resampler {
                        self.stereo_scratch.clear();
                        self.stereo_scratch.reserve((raw_buf.len() / ch) * 2);
                        for frame in raw_buf.chunks_exact(ch) {
                            let c = if ch >= 3 {
                                frame[2] * 0.707_106_77
                            } else {
                                0.0
                            };
                            self.stereo_scratch.push((frame[0] + c).clamp(-1.0, 1.0));
                            self.stereo_scratch.push((frame[1] + c).clamp(-1.0, 1.0));
                        }
                        res.process_stereo(&self.stereo_scratch, &mut self.pending_stereo);
                    } else {
                        self.pending_stereo.reserve((raw_buf.len() / ch) * 2);
                        for frame in raw_buf.chunks_exact(ch) {
                            let c = if ch >= 3 {
                                frame[2] * 0.707_106_77
                            } else {
                                0.0
                            };
                            self.pending_stereo.push((frame[0] + c).clamp(-1.0, 1.0));
                            self.pending_stereo.push((frame[1] + c).clamp(-1.0, 1.0));
                        }
                    }
                    if self.pending_stereo.len() >= 2 {
                        self.last_chunk_rms = compute_rms(&self.pending_stereo);
                        return true;
                    }
                }
                Err(SymphoniaError::DecodeError(_)) => {
                    consecutive_decode_errors += 1;
                    if consecutive_decode_errors > MAX_CONSECUTIVE_DECODE_ERRORS {
                        self.eof = true;
                        return false;
                    }
                    continue;
                }
                Err(_) => {
                    self.eof = true;
                    return false;
                }
            }
        }
    }
}

#[inline]
fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0_f32;
    for &s in samples {
        sum += s * s;
    }
    (sum / (samples.len() as f32)).sqrt()
}

struct StereoResampler {
    inner: Async<f32>,
    in_fifo: Vec<f32>,
    out_scratch: Vec<f32>,
}

impl StereoResampler {
    fn new(in_rate: u32, out_rate: u32, playback_speed: f32) -> AudioResult<Self> {
        let base_ratio = f64::from(out_rate.max(8_000)) / f64::from(in_rate.max(8_000));
        let chunk_frames = 512;
        let sinc_params = SincInterpolationParameters::new(128, WindowFunction::BlackmanHarris2);
        let mut inner = Async::<f32>::new_sinc(
            base_ratio,
            2.2,
            &sinc_params,
            chunk_frames,
            2,
            FixedAsync::Input,
        )
        .map_err(|e| AudioError::Decode(e.to_string()))?;
        let rel = 1.0 / f64::from(playback_speed.clamp(0.5, 2.0));
        let _ = inner.set_resample_ratio_relative(rel, false);
        let out_max = inner.output_frames_max() * 2 + 64;
        Ok(Self {
            inner,
            in_fifo: Vec::with_capacity(chunk_frames * 4),
            out_scratch: vec![0.0; out_max],
        })
    }

    fn set_speed(&mut self, playback_speed: f32) {
        let rel = 1.0 / f64::from(playback_speed.clamp(0.5, 2.0));
        let _ = self.inner.set_resample_ratio_relative(rel, true);
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.in_fifo.clear();
    }

    fn process_stereo(&mut self, input: &[f32], output: &mut Vec<f32>) {
        self.in_fifo.extend_from_slice(input);
        loop {
            let needed_frames = self.inner.input_frames_next();
            let needed_samples = needed_frames * 2;
            if self.in_fifo.len() < needed_samples {
                break;
            }
            let out_needed_frames = self.inner.output_frames_max();
            if self.out_scratch.len() < out_needed_frames * 2 {
                self.out_scratch.resize(out_needed_frames * 2, 0.0);
            }
            let in_adapter =
                InterleavedSlice::new(&self.in_fifo[..needed_samples], 2, needed_frames);
            let out_adapter = InterleavedSlice::new_mut(
                &mut self.out_scratch[..out_needed_frames * 2],
                2,
                out_needed_frames,
            );
            if let (Ok(in_buf), Ok(mut out_buf)) = (in_adapter, out_adapter)
                && let Ok((_, frames_out)) =
                    self.inner.process_into_buffer(&in_buf, &mut out_buf, None)
            {
                output.extend_from_slice(&self.out_scratch[..frames_out * 2]);
            }
            self.in_fifo.drain(..needed_samples);
        }
    }

    /// Flush remaining `< 512` frames at stream EOF so 0ms gapless album transitions lose zero tail samples.
    fn flush_tail(&mut self, output: &mut Vec<f32>) {
        let rem_frames = self.in_fifo.len() / 2;
        if rem_frames == 0 {
            return;
        }
        let needed_frames = self.inner.input_frames_next();
        let needed_samples = needed_frames * 2;
        if self.in_fifo.len() < needed_samples {
            self.in_fifo.resize(needed_samples, 0.0);
        }
        let out_needed_frames = self.inner.output_frames_max();
        if self.out_scratch.len() < out_needed_frames * 2 {
            self.out_scratch.resize(out_needed_frames * 2, 0.0);
        }
        let indexing = Indexing::new().partial_len(rem_frames);
        let in_adapter = InterleavedSlice::new(&self.in_fifo[..needed_samples], 2, needed_frames);
        let out_adapter = InterleavedSlice::new_mut(
            &mut self.out_scratch[..out_needed_frames * 2],
            2,
            out_needed_frames,
        );
        if let (Ok(in_buf), Ok(mut out_buf)) = (in_adapter, out_adapter)
            && let Ok((_, frames_out)) =
                self.inner
                    .process_into_buffer(&in_buf, &mut out_buf, Some(&indexing))
        {
            let expected = ((rem_frames as f64) * self.inner.resample_ratio()).round() as usize;
            let valid_frames = frames_out.min(expected.max(1));
            output.extend_from_slice(&self.out_scratch[..valid_frames * 2]);
        }
        self.in_fifo.clear();
    }
}

#[inline]
fn ms_to_frames(ms: u32, sample_rate: u32) -> u64 {
    (u64::from(ms) * u64::from(sample_rate.max(8_000))) / 1_000
}

fn build_codec_registry() -> CodecRegistry {
    let mut codecs = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut codecs);
    codecs.register_audio_decoder_at_tier::<OpusDecoder>(Tier::Fallback);
    codecs
}

fn promote_deck_b_to_a(
    deck_a: &mut Option<ActiveDeck>,
    deck_b: &mut Option<ActiveDeck>,
    crossfade_state: &mut Option<ActiveCrossfade>,
    bass_swap: &mut BassSwapMixer,
    telemetry: &AudioTelemetry,
    emit_finished_event: bool,
) -> bool {
    let Some(next_deck) = deck_b.take() else {
        return false;
    };
    *crossfade_state = None;
    bass_swap.reset();
    telemetry
        .duration_ms
        .store(next_deck.duration_ms, Ordering::Relaxed);
    telemetry
        .sample_rate
        .store(next_deck.sample_rate, Ordering::Relaxed);
    telemetry
        .position_ms
        .store(next_deck.current_pos_ms(), Ordering::Relaxed);
    telemetry.is_playing.store(true, Ordering::Release);
    if emit_finished_event {
        telemetry.auto_transitioned.store(true, Ordering::Release);
        telemetry.track_finished_seq.fetch_add(1, Ordering::AcqRel);
    }
    *deck_a = Some(next_deck);
    true
}

struct DecodeRuntime {
    codecs: CodecRegistry,
    eq: StereoEqProcessor,
    bass_swap: BassSwapMixer,
    spectrum: SpectrumAnalyzer8,
    dsp: PlaybackDspSettings,
    deck_a: Option<ActiveDeck>,
    deck_b: Option<ActiveDeck>,
    deck_b_same_album: bool,
    queued_next: Option<QueuedTrackSpec>,
    crossfade_state: Option<ActiveCrossfade>,
    micro_fade: MicroFade,
    raw_scratch: Vec<f32>,
}

impl DecodeRuntime {
    fn new(out_rate: u32, eq_config: &EqConfig) -> Self {
        Self {
            codecs: build_codec_registry(),
            eq: StereoEqProcessor::new(out_rate, eq_config),
            bass_swap: BassSwapMixer::new(out_rate),
            spectrum: SpectrumAnalyzer8::new(out_rate),
            dsp: PlaybackDspSettings::default(),
            deck_a: None,
            deck_b: None,
            deck_b_same_album: false,
            queued_next: None,
            crossfade_state: None,
            micro_fade: MicroFade::Steady(1.0),
            raw_scratch: Vec::with_capacity(8192),
        }
    }

    fn load_primary(
        &mut self,
        path: &Path,
        replay_gain_db: i32,
        fallback_duration_ms: u64,
        out_rate: u32,
        telemetry: &AudioTelemetry,
    ) {
        match ActiveDeck::open(
            path,
            out_rate,
            self.dsp.playback_speed,
            replay_gain_db,
            fallback_duration_ms,
            &self.codecs,
        ) {
            Ok(mut deck) => {
                if self.dsp.trim_silence {
                    deck.prime_first_chunk(
                        &mut self.raw_scratch,
                        true,
                        0.5,
                        out_rate,
                        self.dsp.playback_speed,
                    );
                }
                telemetry.bump_epoch();
                telemetry.auto_transitioned.store(false, Ordering::Release);
                telemetry.is_crossfading.store(false, Ordering::Relaxed);
                telemetry
                    .duration_ms
                    .store(deck.duration_ms, Ordering::Relaxed);
                telemetry
                    .sample_rate
                    .store(deck.sample_rate, Ordering::Relaxed);
                telemetry
                    .position_ms
                    .store(deck.current_pos_ms(), Ordering::Relaxed);
                telemetry.is_playing.store(true, Ordering::Release);
                self.eq.reset();
                self.bass_swap.reset();
                self.crossfade_state = None;
                self.deck_b = None;
                self.queued_next = None;
                self.micro_fade = if self.dsp.fade_in_play_ms > 0 {
                    MicroFade::FadingIn {
                        start_gain: 0.0,
                        elapsed_frames: 0,
                        total_frames: ms_to_frames(self.dsp.fade_in_play_ms, out_rate).max(1),
                    }
                } else {
                    MicroFade::Steady(1.0)
                };
                self.deck_a = Some(deck);
            }
            Err(_err) => {
                #[cfg(debug_assertions)]
                eprintln!("ferric-audio open failed ({}): {_err}", path.display());
                telemetry.is_playing.store(false, Ordering::Release);
                telemetry.is_crossfading.store(false, Ordering::Relaxed);
                self.deck_a = None;
                self.deck_b = None;
                self.crossfade_state = None;
                self.bass_swap.reset();
            }
        }
    }

    /// Seamlessly crossfade from currently playing `Deck A` directly into a newly chosen track
    /// without cutting Deck A abruptly (used when user clicks Next or selects another song).
    fn crossfade_to_primary(
        &mut self,
        path: &Path,
        replay_gain_db: i32,
        fallback_duration_ms: u64,
        overlap_ms: u32,
        out_rate: u32,
        telemetry: &AudioTelemetry,
    ) {
        let is_active = self.deck_a.is_some()
            && telemetry.is_playing.load(Ordering::Acquire)
            && overlap_ms >= 200;
        if !is_active {
            self.load_primary(
                path,
                replay_gain_db,
                fallback_duration_ms,
                out_rate,
                telemetry,
            );
            return;
        }
        match ActiveDeck::open(
            path,
            out_rate,
            self.dsp.playback_speed,
            replay_gain_db,
            fallback_duration_ms,
            &self.codecs,
        ) {
            Ok(mut next_deck) => {
                next_deck.prime_first_chunk(
                    &mut self.raw_scratch,
                    self.dsp.trim_silence,
                    2.5,
                    out_rate,
                    self.dsp.playback_speed,
                );
                // If already mid-crossfade, promote current B to A first so the new track overlaps cleanly
                if self.crossfade_state.is_some()
                    && let Some(old_b) = self.deck_b.take()
                {
                    self.deck_a = Some(old_b);
                }
                telemetry
                    .duration_ms
                    .store(next_deck.duration_ms, Ordering::Relaxed);
                telemetry
                    .sample_rate
                    .store(next_deck.sample_rate, Ordering::Relaxed);
                telemetry
                    .position_ms
                    .store(next_deck.current_pos_ms(), Ordering::Relaxed);
                self.deck_b = Some(next_deck);
                self.deck_b_same_album = false;
                self.queued_next = None;
                self.bass_swap.reset();
                self.crossfade_state = Some(ActiveCrossfade {
                    elapsed_frames: 0,
                    total_frames: ms_to_frames(overlap_ms.min(6_000), out_rate).max(1),
                    curve: self.dsp.fade_curve,
                    is_manual_switch: true,
                    ui_handed_off: true,
                });
                telemetry.is_crossfading.store(true, Ordering::Relaxed);
                telemetry.is_playing.store(true, Ordering::Release);
                self.micro_fade = MicroFade::Steady(1.0);
            }
            Err(_err) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "ferric-audio manual crossfade open failed ({}): {_err}",
                    path.display()
                );
                telemetry.is_playing.store(false, Ordering::Release);
                telemetry.is_crossfading.store(false, Ordering::Relaxed);
                self.deck_a = None;
                self.deck_b = None;
                self.queued_next = None;
                self.crossfade_state = None;
                self.bass_swap.reset();
            }
        }
    }

    fn set_queued_next(
        &mut self,
        path: Option<PathBuf>,
        same_album_consecutive: bool,
        replay_gain_db: i32,
        fallback_duration_ms: u64,
        telemetry: &AudioTelemetry,
    ) {
        let abort_pre_handoff_if_mismatched = |rt: &mut Self, incoming: Option<&PathBuf>| {
            if let Some(xf) = rt.crossfade_state
                && !xf.is_manual_switch
                && !xf.ui_handed_off
            {
                let same_target = incoming
                    .zip(rt.deck_b.as_ref())
                    .is_some_and(|(p, db)| db.path == *p);
                if !same_target {
                    rt.crossfade_state = None;
                    rt.deck_b = None;
                    rt.bass_swap.reset();
                    telemetry.is_crossfading.store(false, Ordering::Relaxed);
                }
            }
        };
        match path {
            Some(p) => {
                abort_pre_handoff_if_mismatched(self, Some(&p));
                if self.crossfade_state.is_none()
                    && let Some(existing_b) = &self.deck_b
                    && existing_b.path == p
                {
                    self.deck_b_same_album = same_album_consecutive;
                    self.queued_next = None;
                    return;
                }
                if self.crossfade_state.is_none() {
                    self.deck_b = None;
                }
                self.queued_next = Some(QueuedTrackSpec {
                    path: p,
                    same_album_consecutive,
                    replay_gain_db,
                    fallback_duration_ms,
                });
            }
            None => {
                abort_pre_handoff_if_mismatched(self, None);
                self.queued_next = None;
                if self.crossfade_state.is_none() {
                    self.deck_b = None;
                }
            }
        }
    }

    fn promote_deck_b(&mut self, telemetry: &AudioTelemetry, emit_finished_event: bool) -> bool {
        telemetry.is_crossfading.store(false, Ordering::Relaxed);
        promote_deck_b_to_a(
            &mut self.deck_a,
            &mut self.deck_b,
            &mut self.crossfade_state,
            &mut self.bass_swap,
            telemetry,
            emit_finished_event,
        )
    }
}

fn decode_loop(
    cmd_rx: Receiver<EngineCommand>,
    mut producer: rtrb::Producer<(u32, [f32; 2])>,
    mut out_rate: u32,
    mut eq_config: EqConfig,
    telemetry: Arc<AudioTelemetry>,
    headless: bool,
) {
    let mut rt = DecodeRuntime::new(out_rate, &eq_config);
    let mut target_volume = 1.0_f32;
    let mut smoothed_volume = 1.0_f32;
    let mut ducking = DuckingState::default();
    let mut dsp_scratch = Vec::with_capacity(2048);

    let mut pending_cmd: Option<EngineCommand> = None;
    let mut last_hb = telemetry.cb_heartbeat.load(Ordering::Relaxed);
    let mut stall_ticks = 0u32;

    loop {
        while let Some(cmd) = pending_cmd.take().or_else(|| cmd_rx.try_recv().ok()) {
            match cmd {
                EngineCommand::LoadAndPlay(path) => {
                    rt.load_primary(&path, 0, 0, out_rate, &telemetry);
                }
                EngineCommand::LoadAndPlayTrack {
                    path,
                    replay_gain_db,
                    fallback_duration_ms,
                } => {
                    rt.load_primary(
                        &path,
                        replay_gain_db,
                        fallback_duration_ms,
                        out_rate,
                        &telemetry,
                    );
                }
                EngineCommand::CrossfadeToTrack {
                    path,
                    replay_gain_db,
                    fallback_duration_ms,
                    overlap_ms,
                } => {
                    rt.crossfade_to_primary(
                        &path,
                        replay_gain_db,
                        fallback_duration_ms,
                        overlap_ms,
                        out_rate,
                        &telemetry,
                    );
                }
                EngineCommand::QueueNext(path) => {
                    rt.set_queued_next(path, false, 0, 0, &telemetry);
                }
                EngineCommand::QueueNextTrack {
                    path,
                    same_album_consecutive,
                    replay_gain_db,
                    fallback_duration_ms,
                } => {
                    rt.set_queued_next(
                        path,
                        same_album_consecutive,
                        replay_gain_db,
                        fallback_duration_ms,
                        &telemetry,
                    );
                }
                EngineCommand::Play => {
                    if rt.deck_a.is_some() {
                        let start_gain = if telemetry.is_playing.load(Ordering::Relaxed) {
                            rt.micro_fade.current_gain()
                        } else {
                            0.0
                        };
                        telemetry.is_playing.store(true, Ordering::Release);
                        rt.micro_fade = if rt.dsp.fade_in_play_ms > 0 {
                            MicroFade::FadingIn {
                                start_gain,
                                elapsed_frames: 0,
                                total_frames: ms_to_frames(rt.dsp.fade_in_play_ms, out_rate).max(1),
                            }
                        } else {
                            MicroFade::Steady(1.0)
                        };
                    }
                }
                EngineCommand::Pause => {
                    if rt.deck_a.is_some() && telemetry.is_playing.load(Ordering::Relaxed) {
                        if !matches!(rt.micro_fade, MicroFade::Pausing { .. }) {
                            rt.micro_fade = MicroFade::Pausing {
                                start_gain: rt.micro_fade.current_gain(),
                                elapsed_frames: 0,
                                total_frames: ms_to_frames(120, out_rate).max(1),
                            };
                        }
                    } else {
                        telemetry.is_playing.store(false, Ordering::Release);
                        rt.micro_fade = MicroFade::Steady(0.0);
                    }
                }
                EngineCommand::TogglePlayPause => {
                    if rt.deck_a.is_some() {
                        let actively_playing = telemetry.is_playing.load(Ordering::Relaxed)
                            && !matches!(rt.micro_fade, MicroFade::Pausing { .. });
                        if actively_playing {
                            rt.micro_fade = MicroFade::Pausing {
                                start_gain: rt.micro_fade.current_gain(),
                                elapsed_frames: 0,
                                total_frames: ms_to_frames(120, out_rate).max(1),
                            };
                        } else {
                            let start_gain = if telemetry.is_playing.load(Ordering::Relaxed) {
                                rt.micro_fade.current_gain()
                            } else {
                                0.0
                            };
                            telemetry.is_playing.store(true, Ordering::Release);
                            rt.micro_fade = if rt.dsp.fade_in_play_ms > 0 {
                                MicroFade::FadingIn {
                                    start_gain,
                                    elapsed_frames: 0,
                                    total_frames: ms_to_frames(rt.dsp.fade_in_play_ms, out_rate)
                                        .max(1),
                                }
                            } else {
                                MicroFade::Steady(1.0)
                            };
                        }
                    }
                }
                EngineCommand::Stop => {
                    telemetry.bump_epoch();
                    telemetry.is_playing.store(false, Ordering::Release);
                    telemetry.is_crossfading.store(false, Ordering::Relaxed);
                    telemetry.auto_transitioned.store(false, Ordering::Release);
                    telemetry.position_ms.store(0, Ordering::Relaxed);
                    telemetry.store_spectrum([0; 8]);
                    rt.deck_a = None;
                    rt.deck_b = None;
                    rt.queued_next = None;
                    rt.crossfade_state = None;
                    rt.micro_fade = MicroFade::Steady(0.0);
                    rt.bass_swap.reset();
                }
                EngineCommand::SeekMs(ms) => {
                    let seek_deck_b = rt
                        .crossfade_state
                        .is_some_and(|xf| xf.ui_handed_off || xf.is_manual_switch)
                        && rt.deck_b.is_some();
                    let seek_res = if seek_deck_b {
                        rt.deck_b
                            .as_mut()
                            .and_then(|db| db.seek_ms(ms).ok())
                            .inspect(|_| {
                                rt.promote_deck_b(&telemetry, false);
                            })
                    } else {
                        rt.deck_a.as_mut().and_then(|da| da.seek_ms(ms).ok())
                    };
                    if let Some(actual) = seek_res {
                        telemetry.bump_epoch();
                        telemetry.position_ms.store(actual, Ordering::Relaxed);
                        rt.eq.reset();
                        if rt.crossfade_state.is_some() {
                            rt.crossfade_state = None;
                            if let Some(old_b) = rt.deck_b.take()
                                && rt.queued_next.is_none()
                            {
                                rt.queued_next = Some(QueuedTrackSpec {
                                    path: old_b.path,
                                    same_album_consecutive: rt.deck_b_same_album,
                                    replay_gain_db: old_b.replay_gain_db,
                                    fallback_duration_ms: old_b.duration_ms,
                                });
                            }
                            telemetry.is_crossfading.store(false, Ordering::Relaxed);
                            rt.bass_swap.reset();
                        }
                        if rt.dsp.fade_in_play_ms > 0
                            && telemetry.is_playing.load(Ordering::Relaxed)
                            && !matches!(rt.micro_fade, MicroFade::Pausing { .. })
                        {
                            rt.micro_fade = MicroFade::FadingIn {
                                start_gain: 0.0,
                                elapsed_frames: 0,
                                total_frames: ms_to_frames(rt.dsp.fade_in_play_ms, out_rate).max(1),
                            };
                        }
                    }
                }
                EngineCommand::SetVolume(v) => {
                    if v.is_finite() {
                        target_volume = v.clamp(0.0, 1.5);
                    }
                }
                EngineCommand::SetDucking(ducked) => {
                    ducking.set_ducked(ducked);
                }
                EngineCommand::SetEq(cfg) => {
                    eq_config = cfg.sanitized();
                    rt.eq.apply_config(&eq_config);
                }
                EngineCommand::SetCrossfadeMs(ms) => {
                    rt.dsp.crossfade_ms = ms.min(12_000);
                }
                EngineCommand::SetDspSettings(new_dsp) => {
                    let sanitized = new_dsp.sanitized();
                    let speed_changed =
                        (sanitized.playback_speed - rt.dsp.playback_speed).abs() > 1e-4;
                    rt.dsp = sanitized;
                    if speed_changed {
                        if let Some(da) = &mut rt.deck_a {
                            da.set_playback_speed(out_rate, rt.dsp.playback_speed);
                        }
                        if let Some(db) = &mut rt.deck_b {
                            db.set_playback_speed(out_rate, rt.dsp.playback_speed);
                        }
                    }
                }
                EngineCommand::ReplaceOutputRing {
                    producer: new_producer,
                    out_rate: new_rate,
                } => {
                    producer = new_producer;
                    stall_ticks = 0;
                    last_hb = telemetry.cb_heartbeat.load(Ordering::Relaxed);
                    telemetry.bump_epoch();
                    if new_rate != out_rate {
                        out_rate = new_rate.max(8_000);
                        rt.eq.set_sample_rate(out_rate, &eq_config);
                        rt.bass_swap.set_sample_rate(out_rate);
                        rt.spectrum = SpectrumAnalyzer8::new(out_rate);
                        if let Some(da) = &mut rt.deck_a {
                            da.update_output_rate(out_rate, rt.dsp.playback_speed);
                        }
                        if let Some(db) = &mut rt.deck_b {
                            db.update_output_rate(out_rate, rt.dsp.playback_speed);
                        }
                    }
                }
                EngineCommand::Shutdown => return,
            }
        }

        let is_playing = telemetry.is_playing.load(Ordering::Relaxed);
        if !is_playing || rt.deck_a.is_none() {
            let idle_bins = rt.spectrum.decay_idle();
            telemetry.store_spectrum(idle_bins);
            if idle_bins == [0; 8] {
                if !telemetry.idle_silent.load(Ordering::Acquire) {
                    // Let the 260 ms SPSC output ring finish draining cleanly before pausing WASAPI.
                    match cmd_rx.recv_timeout(Duration::from_millis(280)) {
                        Ok(cmd) => {
                            pending_cmd = Some(cmd);
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            telemetry.idle_silent.store(true, Ordering::Release);
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                // True 0 Hz idle wait: blocks kernel-side until next command arrives (<0.1 ms wakeup).
                match cmd_rx.recv() {
                    Ok(cmd) => pending_cmd = Some(cmd),
                    Err(_) => return,
                }
            } else {
                thread::sleep(Duration::from_millis(16));
            }
            continue;
        }
        telemetry.idle_silent.store(false, Ordering::Relaxed);

        let max_frames = if headless {
            512
        } else {
            let slots = producer.slots();
            if slots < 1024 {
                let hb = telemetry.cb_heartbeat.load(Ordering::Relaxed);
                if hb == last_hb {
                    stall_ticks = stall_ticks.saturating_add(1);
                    // ~450 ms with zero output callback progress => WASAPI stream silently died.
                    if stall_ticks >= 45 {
                        telemetry.device_lost.store(true, Ordering::Release);
                        stall_ticks = 0;
                    }
                } else {
                    last_hb = hb;
                    stall_ticks = 0;
                }
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            stall_ticks = 0;
            last_hb = telemetry.cb_heartbeat.load(Ordering::Relaxed);
            slots.min(2048)
        };

        // 1. Proactively pre-open and prime Deck B within max(eff_xfade_ms + 8000, 12000) ms of Deck A end.
        if rt.deck_b.is_none()
            && let Some(da) = &rt.deck_a
            && let Some(spec) = &rt.queued_next
        {
            let eff_xfade_ms = if rt.dsp.gapless_album_lock && spec.same_album_consecutive {
                0
            } else {
                rt.dsp.crossfade_ms
            };
            let preopen_window_ms = u64::from(eff_xfade_ms.saturating_add(8_000).max(12_000));
            if (da.duration_ms == 0 || da.remaining_ms() <= preopen_window_ms)
                && let Some(s) = rt.queued_next.take()
            {
                match ActiveDeck::open(
                    &s.path,
                    out_rate,
                    rt.dsp.playback_speed,
                    s.replay_gain_db,
                    s.fallback_duration_ms,
                    &rt.codecs,
                ) {
                    Ok(mut next_deck) => {
                        let trim_lead = rt.dsp.trim_silence
                            && !(rt.dsp.gapless_album_lock && s.same_album_consecutive);
                        next_deck.prime_first_chunk(
                            &mut rt.raw_scratch,
                            trim_lead,
                            4.0,
                            out_rate,
                            rt.dsp.playback_speed,
                        );
                        rt.deck_b_same_album = s.same_album_consecutive;
                        rt.deck_b = Some(next_deck);
                    }
                    Err(_err) => {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "ferric-audio Deck B pre-open failed ({}): {_err}",
                            s.path.display()
                        );
                    }
                }
            }
        }

        // 2. Evaluate Smart Trailing-Silence Trigger + Crossfade Overlap Start.
        if rt.crossfade_state.is_none()
            && rt.deck_b.is_some()
            && let Some(da) = &rt.deck_a
        {
            let album_locked = rt.dsp.gapless_album_lock && rt.deck_b_same_album;
            let raw_xfade_ms = if album_locked { 0 } else { rt.dsp.crossfade_ms };
            // Cap overlap to at most 1/3 of track length so short tracks/intros never skip on frame 0.
            let eff_xfade_ms = if da.duration_ms > 0 {
                raw_xfade_ms.min(u32::try_from(da.duration_ms / 3).unwrap_or(u32::MAX))
            } else {
                raw_xfade_ms
            };
            let rem_ms = da.remaining_ms();
            let sustained_silence = da.quiet_outro_frames >= (out_rate as usize / 12).max(512)
                && da.last_chunk_rms < 0.003;

            let time_trigger = eff_xfade_ms > 0
                && da.duration_ms >= 2_000
                && da.current_pos_ms() >= 1_000
                && rem_ms <= u64::from(eff_xfade_ms);
            let silence_trigger = !album_locked
                && rt.dsp.trim_silence
                && da.current_pos_ms() > 2_000
                && (da.duration_ms == 0 || rem_ms <= u64::from(eff_xfade_ms) + 4_500)
                && sustained_silence;

            if time_trigger || silence_trigger {
                let overlap_ms = if eff_xfade_ms > 0 { eff_xfade_ms } else { 180 };
                rt.bass_swap.reset();
                rt.crossfade_state = Some(ActiveCrossfade {
                    elapsed_frames: 0,
                    total_frames: ms_to_frames(overlap_ms, out_rate).max(1),
                    curve: rt.dsp.fade_curve,
                    is_manual_switch: false,
                    ui_handed_off: false,
                });
                telemetry.is_crossfading.store(true, Ordering::Relaxed);
            }
        }

        // 3. Render stereo frames seamlessly across Deck A / Deck B with zero gap.
        dsp_scratch.clear();
        let mut unqueued_track_ended = false;
        let mut pause_completed = false;

        for _ in 0..max_frames {
            let (env_gain, finished_pause) = rt.micro_fade.step_frame();
            if finished_pause {
                pause_completed = true;
                break;
            }

            let mixed_frame = if let Some(mut xfade) = rt.crossfade_state {
                let sample_a = rt
                    .deck_a
                    .as_mut()
                    .and_then(|da| {
                        da.next_stereo_frame(
                            &mut rt.raw_scratch,
                            out_rate,
                            rt.dsp.playback_speed,
                            rt.dsp.replay_gain,
                        )
                    })
                    .unwrap_or((0.0, 0.0));

                let sample_b = rt.deck_b.as_mut().and_then(|db| {
                    db.next_stereo_frame(
                        &mut rt.raw_scratch,
                        out_rate,
                        rt.dsp.playback_speed,
                        rt.dsp.replay_gain,
                    )
                });

                match sample_b {
                    Some(sb) => {
                        let progress = (xfade.elapsed_frames as f32
                            / xfade.total_frames.max(1) as f32)
                            .clamp(0.0, 1.0);
                        let (ml, mr) = rt.bass_swap.mix_frame(sample_a, sb, progress, xfade.curve);
                        xfade.elapsed_frames += 1;
                        // Adaptively hand off UI metadata & clock to incoming Deck B once it enters
                        // (~15% into the user's chosen crossfade duration) instead of waiting until
                        // the full crossfade tail finishes.
                        if !xfade.is_manual_switch && !xfade.ui_handed_off && progress >= 0.15 {
                            xfade.ui_handed_off = true;
                            if let Some(db) = &rt.deck_b {
                                telemetry
                                    .duration_ms
                                    .store(db.duration_ms, Ordering::Relaxed);
                                telemetry
                                    .sample_rate
                                    .store(db.sample_rate, Ordering::Relaxed);
                                telemetry
                                    .position_ms
                                    .store(db.current_pos_ms(), Ordering::Relaxed);
                            }
                            telemetry.auto_transitioned.store(true, Ordering::Release);
                            telemetry.track_finished_seq.fetch_add(1, Ordering::AcqRel);
                        }
                        if xfade.elapsed_frames >= xfade.total_frames {
                            let emit_now = !xfade.is_manual_switch && !xfade.ui_handed_off;
                            rt.promote_deck_b(&telemetry, emit_now);
                        } else {
                            rt.crossfade_state = Some(xfade);
                        }
                        Some((ml, mr))
                    }
                    None => {
                        rt.crossfade_state = None;
                        rt.deck_b = None;
                        telemetry.is_crossfading.store(false, Ordering::Relaxed);
                        if xfade.ui_handed_off {
                            unqueued_track_ended = true;
                            None
                        } else {
                            Some(sample_a)
                        }
                    }
                }
            } else {
                let primary = rt.deck_a.as_mut().and_then(|da| {
                    da.next_stereo_frame(
                        &mut rt.raw_scratch,
                        out_rate,
                        rt.dsp.playback_speed,
                        rt.dsp.replay_gain,
                    )
                });

                match primary {
                    Some(frame) => Some(frame),
                    None => {
                        if rt.deck_b.is_none()
                            && let Some(s) = rt.queued_next.take()
                            && let Ok(mut next_deck) = ActiveDeck::open(
                                &s.path,
                                out_rate,
                                rt.dsp.playback_speed,
                                s.replay_gain_db,
                                s.fallback_duration_ms,
                                &rt.codecs,
                            )
                        {
                            let trim_lead = rt.dsp.trim_silence
                                && !(rt.dsp.gapless_album_lock && s.same_album_consecutive);
                            next_deck.prime_first_chunk(
                                &mut rt.raw_scratch,
                                trim_lead,
                                4.0,
                                out_rate,
                                rt.dsp.playback_speed,
                            );
                            rt.deck_b_same_album = s.same_album_consecutive;
                            rt.deck_b = Some(next_deck);
                        }

                        if rt.promote_deck_b(&telemetry, true) {
                            rt.deck_a.as_mut().and_then(|da| {
                                da.next_stereo_frame(
                                    &mut rt.raw_scratch,
                                    out_rate,
                                    rt.dsp.playback_speed,
                                    rt.dsp.replay_gain,
                                )
                            })
                        } else {
                            unqueued_track_ended = true;
                            None
                        }
                    }
                }
            };

            let Some((mut l, mut r)) = mixed_frame else {
                break;
            };
            l *= env_gain;
            r *= env_gain;
            if rt.dsp.mono_audio {
                let mono = 0.5 * (l + r);
                l = mono;
                r = mono;
            }
            dsp_scratch.push(l);
            dsp_scratch.push(r);
        }

        if let Some(xf) = rt.crossfade_state
            && (xf.is_manual_switch || xf.ui_handed_off)
            && let Some(db) = &rt.deck_b
        {
            telemetry
                .position_ms
                .store(db.current_pos_ms(), Ordering::Relaxed);
        } else if let Some(da) = &rt.deck_a {
            telemetry
                .position_ms
                .store(da.current_pos_ms(), Ordering::Relaxed);
        }

        if !dsp_scratch.is_empty() {
            ducking.step_block(dsp_scratch.len() / 2, out_rate);
            let effective_target = ducking.effective_volume(target_volume);
            rt.eq.process_interleaved_stereo_ramped(
                &mut dsp_scratch,
                &mut smoothed_volume,
                effective_target,
            );
            telemetry.store_spectrum(rt.spectrum.analyze_interleaved(&dsp_scratch));
            if !headless {
                let push_epoch = telemetry.active_epoch.load(Ordering::Acquire);
                for &[l, r] in dsp_scratch.as_chunks::<2>().0 {
                    if producer.push((push_epoch, [l, r])).is_err() {
                        break;
                    }
                }
            } else {
                thread::sleep(Duration::from_millis(8));
            }
        }

        if pause_completed {
            telemetry.is_playing.store(false, Ordering::Release);
            telemetry.store_spectrum([0; 8]);
        }

        if unqueued_track_ended {
            rt.deck_a = None;
            rt.crossfade_state = None;
            telemetry.is_playing.store(false, Ordering::Release);
            telemetry.auto_transitioned.store(false, Ordering::Release);
            telemetry.store_spectrum([0; 8]);
            telemetry.track_finished_seq.fetch_add(1, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo_synth::ensure_demo_tracks;

    #[test]
    fn dual_deck_manual_and_auto_crossfade_overlap_with_zero_gap() {
        let dir = std::env::temp_dir().join(format!("ferric_xfade_test_{}", std::process::id()));
        let tracks = ensure_demo_tracks(&dir).expect("generate demo wav tracks");
        assert!(tracks.len() >= 2);
        let p0 = tracks[0].path.clone().expect("track 0 path");
        let p1 = tracks[1].path.clone().expect("track 1 path");
        let dur0 = tracks[0].duration_ms;
        let dur1 = tracks[1].duration_ms;

        let engine = AudioEngine::start_headless(EqConfig::default());
        let dsp = PlaybackDspSettings {
            crossfade_ms: 2_000,
            gapless_album_lock: false,
            trim_silence: false,
            ..Default::default()
        };
        engine.send(EngineCommand::SetDspSettings(dsp));

        // 1. Start track 0 and verify playback begins
        engine.send(EngineCommand::LoadAndPlayTrack {
            path: p0.clone(),
            replay_gain_db: 0,
            fallback_duration_ms: dur0,
        });
        let mut started = false;
        for _ in 0..80 {
            let s = engine.telemetry();
            if s.is_playing && s.duration_ms > 0 {
                started = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(started, "track 0 should start playing in headless engine");

        // 2. Manual CrossfadeToTrack must overlap Deck A -> Deck B without cutting playback
        engine.send(EngineCommand::CrossfadeToTrack {
            path: p1,
            replay_gain_db: 0,
            fallback_duration_ms: dur1,
            overlap_ms: 800,
        });
        let mut saw_manual_xfade = false;
        let mut manual_done = false;
        for _ in 0..250 {
            let s = engine.telemetry();
            if s.is_crossfading {
                saw_manual_xfade = true;
            } else if saw_manual_xfade && s.is_playing {
                manual_done = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            saw_manual_xfade,
            "manual CrossfadeToTrack should enter dual-deck overlap"
        );
        assert!(
            manual_done,
            "manual crossfade should promote Deck B cleanly while staying playing"
        );
        assert_eq!(
            engine.telemetry().track_finished_seq,
            0,
            "manual crossfade switch must not emit spurious track_finished_seq"
        );

        // 3. Queue next track and seek near end to verify automatic overlap crossfade before EOF
        engine.send(EngineCommand::QueueNextTrack {
            path: Some(p0),
            same_album_consecutive: false,
            replay_gain_db: 0,
            fallback_duration_ms: dur0,
        });
        let seek_target = dur1.saturating_sub(2_400);
        engine.send(EngineCommand::SeekMs(seek_target));

        let mut saw_auto_xfade = false;
        let mut handed_off_during_xfade = false;
        let mut promoted = false;
        for _ in 0..900 {
            let s = engine.telemetry();
            if s.is_crossfading {
                saw_auto_xfade = true;
                if s.track_finished_seq == 1 && s.auto_transitioned {
                    handed_off_during_xfade = true;
                }
            }
            if saw_auto_xfade
                && !s.is_crossfading
                && s.track_finished_seq == 1
                && s.auto_transitioned
            {
                promoted = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            saw_auto_xfade,
            "automatic end-of-track crossfade must start overlapping before Deck A ends"
        );
        assert!(
            handed_off_during_xfade,
            "UI must adaptively hand off to incoming Deck B during crossfade without waiting for tail completion"
        );
        assert!(
            promoted,
            "Deck B must finish promoting cleanly with exactly one track_finished_seq increment"
        );

        engine.send(EngineCommand::Shutdown);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ducking_attenuates_independently_and_restores_exact_volume() {
        let mut duck = DuckingState::default();
        assert!((duck.current_gain() - 1.0).abs() < 1e-6);
        assert!((duck.effective_volume(0.85) - 0.85).abs() < 1e-6);

        duck.set_ducked(true);
        for _ in 0..120 {
            duck.step_block(256, 48_000);
        }
        assert!(duck.current_gain() <= 0.27 && duck.current_gain() >= 0.23);
        // Changing user volume while ducked must scale proportionally without corrupting stored volume.
        assert!((duck.effective_volume(0.60) - 0.15).abs() < 0.02);

        duck.set_ducked(false);
        for _ in 0..160 {
            duck.step_block(256, 48_000);
        }
        assert!((duck.current_gain() - 1.0).abs() < 1e-3);
        assert!((duck.effective_volume(0.60) - 0.60).abs() < 1e-3);
    }

    #[test]
    fn content_opener_decodes_content_uri_in_headless_engine() {
        use crate::content::{ContentOpener, register_scoped_content_opener_for_test};

        let dir = std::env::temp_dir().join(format!("ferric_content_uri_{}", std::process::id()));
        let tracks = ensure_demo_tracks(&dir).expect("demo wav");
        let wav_path = tracks[0].path.clone().expect("wav path");

        struct MapOpener(PathBuf);
        impl ContentOpener for MapOpener {
            fn open_file(&self, uri: &str) -> std::io::Result<std::fs::File> {
                if uri == "content://ferric-test/track0.wav" {
                    std::fs::File::open(&self.0)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "unknown test content uri",
                    ))
                }
            }
        }

        let _guard = register_scoped_content_opener_for_test(Arc::new(MapOpener(wav_path)));
        let engine = AudioEngine::start_headless(EqConfig::default());
        engine.send(EngineCommand::SetDucking(true));
        engine.send(EngineCommand::LoadAndPlay(PathBuf::from(
            "content://ferric-test/track0.wav",
        )));

        let mut started = false;
        for _ in 0..100 {
            let s = engine.telemetry();
            if s.is_playing && s.duration_ms > 0 {
                started = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            started,
            "content:// URI must decode via registered ContentOpener"
        );
        engine.send(EngineCommand::SetDucking(false));
        engine.send(EngineCommand::Shutdown);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duration_parsers_decode_webm_ebml_and_duration_tags() {
        // Synthetic Matroska Segment/Info containing TimecodeScale (1_000_000 ns) + float64 Duration (184_500.0 ms)
        let mut info_body = Vec::new();
        info_body.extend_from_slice(&[0x2A, 0xD7, 0xB1, 0x83, 0x0F, 0x42, 0x40]);
        info_body.extend_from_slice(&[0x44, 0x89, 0x88]);
        info_body.extend_from_slice(&184_500.0_f64.to_be_bytes());
        let mut ebml = vec![0x1A, 0x45, 0xDF, 0xA3, 0x15, 0x49, 0xA9, 0x66];
        ebml.push(0x80 | (info_body.len() as u8));
        ebml.extend_from_slice(&info_body);
        assert_eq!(parse_ebml_info_duration_ms(&ebml), Some(184_500));
    }
}
