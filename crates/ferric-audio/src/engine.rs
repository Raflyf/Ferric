use crate::dsp::{
    BassSwapMixer, SpectrumAnalyzer8, StereoEqProcessor, db_to_linear, smooth_fade_envelope,
};
use crate::{AudioError, AudioResult};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ferric_core::eq::EqConfig;
use ferric_core::models::{FadeCurve, PlaybackDspSettings};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Adjustable, Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters, WindowFunction,
};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::common::Tier;
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
    pub(crate) flush_ring: AtomicBool,
}

impl AudioTelemetry {
    pub fn snapshot(&self) -> TelemetrySnapshot {
        let mut bins = [0_u32; 8];
        for (i, slot) in self.spectrum_bins.iter().enumerate() {
            bins[i] = slot.load(Ordering::Relaxed);
        }
        TelemetrySnapshot {
            is_playing: self.is_playing.load(Ordering::Acquire),
            position_ms: self.position_ms.load(Ordering::Relaxed),
            duration_ms: self.duration_ms.load(Ordering::Relaxed),
            sample_rate: self.sample_rate.load(Ordering::Relaxed),
            output_rate: self.output_rate.load(Ordering::Relaxed),
            track_finished_seq: self.track_finished_seq.load(Ordering::Acquire),
            auto_transitioned: self.auto_transitioned.load(Ordering::Acquire),
            is_crossfading: self.is_crossfading.load(Ordering::Relaxed),
            spectrum_bins: bins,
        }
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
        fallback_duration_ms: u64,
    },
    Play,
    Pause,
    TogglePlayPause,
    Stop,
    SeekMs(u64),
    SetVolume(f32),
    SetEq(EqConfig),
    SetCrossfadeMs(u32),
    SetDspSettings(PlaybackDspSettings),
    Shutdown,
}

pub struct AudioEngine {
    cmd_tx: Sender<EngineCommand>,
    telemetry: Arc<AudioTelemetry>,
    _output_stream: Option<cpal::Stream>,
}

impl AudioEngine {
    /// Start the real-time WASAPI/cpal output stream and dedicated dual-deck decode+DSP OS thread.
    pub fn start(initial_eq: EqConfig) -> AudioResult<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioError::NoOutputDevice)?;
        let default_cfg = device
            .default_output_config()
            .map_err(|e| AudioError::Device(e.to_string()))?;

        let out_rate = default_cfg.sample_rate();
        let out_channels = usize::from(default_cfg.channels()).max(1);
        let sample_format = default_cfg.sample_format();
        let stream_cfg: cpal::StreamConfig = default_cfg.into();

        // ~260 ms wait-free SPSC ring buffer of atomic stereo `[L, R]` frames — eliminates
        // L/R channel desync and absorbs disk/Deck-B pre-open jitter with zero underrun crackles.
        let ring_frames = ((out_rate as usize * 260) / 1000).max(4096);
        let (producer, mut consumer) = rtrb::RingBuffer::<[f32; 2]>::new(ring_frames);

        let telemetry = Arc::new(AudioTelemetry::default());
        telemetry.output_rate.store(out_rate, Ordering::Relaxed);
        let cb_telemetry = Arc::clone(&telemetry);

        let err_fn = |err| {
            eprintln!("ferric-audio stream error: {err}");
        };

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                stream_cfg,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if cb_telemetry.flush_ring.swap(false, Ordering::AcqRel) {
                        while consumer.pop().is_ok() {}
                    }
                    for frame in data.chunks_exact_mut(out_channels) {
                        let [l, r] = consumer.pop().unwrap_or([0.0, 0.0]);
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
                    if cb_telemetry.flush_ring.swap(false, Ordering::AcqRel) {
                        while consumer.pop().is_ok() {}
                    }
                    for frame in data.chunks_exact_mut(out_channels) {
                        let [l, r] = consumer.pop().unwrap_or([0.0, 0.0]);
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
                    if cb_telemetry.flush_ring.swap(false, Ordering::AcqRel) {
                        while consumer.pop().is_ok() {}
                    }
                    for frame in data.chunks_exact_mut(out_channels) {
                        let [l, r] = consumer.pop().unwrap_or([0.0, 0.0]);
                        let to_i32 = |s: f32| {
                            (f64::from(s.clamp(-1.0, 1.0)) * 2_147_483_647.0).round() as i32
                        };
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
                    if cb_telemetry.flush_ring.swap(false, Ordering::AcqRel) {
                        while consumer.pop().is_ok() {}
                    }
                    for frame in data.chunks_exact_mut(out_channels) {
                        let [l, r] = consumer.pop().unwrap_or([0.0, 0.0]);
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

        stream
            .play()
            .map_err(|e| AudioError::Device(e.to_string()))?;

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let thread_telemetry = Arc::clone(&telemetry);
        thread::Builder::new()
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

        Ok(Self {
            cmd_tx,
            telemetry,
            _output_stream: Some(stream),
        })
    }

    /// Headless no-device fallback (used when no sound card is attached, e.g., CI runners).
    pub fn start_headless(initial_eq: EqConfig) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let telemetry = Arc::new(AudioTelemetry::default());
        telemetry.output_rate.store(48_000, Ordering::Relaxed);
        let thread_telemetry = Arc::clone(&telemetry);
        let (producer, _consumer) = rtrb::RingBuffer::<[f32; 2]>::new(4096);
        let _ = thread::Builder::new()
            .name("ferric-audio-headless".to_string())
            .spawn(move || {
                decode_loop(cmd_rx, producer, 48_000, initial_eq, thread_telemetry, true);
            });
        Self {
            cmd_tx,
            telemetry,
            _output_stream: None,
        }
    }

    pub fn send(&self, cmd: EngineCommand) {
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
        let _ = self.cmd_tx.send(EngineCommand::Shutdown);
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
    pending_offset: usize,
    pos_ms_f64: f64,
    replay_gain_linear: f32,
    last_chunk_rms: f32,
    quiet_outro_frames: usize,
    eof: bool,
    tail_flushed: bool,
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
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
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
            (_, Some(frames)) if sample_rate > 0 => (frames * 1000) / u64::from(sample_rate),
            _ => 0,
        };
        let duration_ms = if probed_duration_ms > 0 {
            probed_duration_ms
        } else {
            fallback_duration_ms
        };

        let decoder = codecs
            .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
            .map_err(|e| AudioError::Decode(e.to_string()))?;

        let clamped_speed = playback_speed.clamp(0.5, 2.0);
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
            pending_offset: 0,
            pos_ms_f64: 0.0,
            replay_gain_linear,
            last_chunk_rms: 1.0,
            quiet_outro_frames: 0,
            eof: false,
            tail_flushed: false,
        })
    }

    fn set_playback_speed(&mut self, out_rate: u32, speed: f32) {
        let clamped = speed.clamp(0.5, 2.0);
        if let Some(res) = &mut self.resampler {
            res.set_speed(clamped);
        } else if (clamped - 1.0).abs() > 1e-4
            && let Ok(res) = StereoResampler::new(self.sample_rate, out_rate, clamped)
        {
            self.resampler = Some(res);
        }
    }

    fn seek_ms(&mut self, target_ms: u64) -> AudioResult<u64> {
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from_millis_u64(target_ms),
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
        let actual_ms = self
            .time_base
            .and_then(|tb| tb.calc_time(seeked.actual_ts))
            .map(|t| t.as_millis().max(0) as u64)
            .unwrap_or(target_ms);
        self.pos_ms_f64 = actual_ms as f64;
        Ok(actual_ms)
    }

    #[inline]
    fn current_pos_ms(&self) -> u64 {
        let raw = self.pos_ms_f64.max(0.0).round() as u64;
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
            (1_000.0 * f64::from(playback_speed.clamp(0.5, 2.0))) / f64::from(out_rate.max(8_000));
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
            (1_000.0 * f64::from(playback_speed.clamp(0.5, 2.0))) / f64::from(out_rate.max(8_000));
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
                        let mut stereo_in = Vec::with_capacity(raw_buf.len() * 2);
                        for &s in raw_buf.iter() {
                            stereo_in.push(s);
                            stereo_in.push(s);
                        }
                        if let Some(res) = &mut self.resampler {
                            res.process_stereo(&stereo_in, &mut self.pending_stereo);
                        } else {
                            self.pending_stereo = stereo_in;
                        }
                    } else {
                        let mut stereo_in = Vec::with_capacity((raw_buf.len() / ch) * 2);
                        for frame in raw_buf.chunks_exact(ch) {
                            stereo_in.push(frame[0]);
                            stereo_in.push(frame[1]);
                        }
                        if let Some(res) = &mut self.resampler {
                            res.process_stereo(&stereo_in, &mut self.pending_stereo);
                        } else {
                            self.pending_stereo = stereo_in;
                        }
                    }
                    if self.pending_stereo.len() >= 2 {
                        self.last_chunk_rms = compute_rms(&self.pending_stereo);
                        return true;
                    }
                }
                Err(SymphoniaError::DecodeError(_)) => continue,
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
                telemetry.flush_ring.store(true, Ordering::Release);
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
            Err(err) => {
                eprintln!("ferric-audio open failed ({}): {err}", path.display());
                telemetry.is_playing.store(false, Ordering::Release);
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
            Err(err) => {
                eprintln!(
                    "ferric-audio manual crossfade open failed ({}): {err}",
                    path.display()
                );
            }
        }
    }

    fn set_queued_next(
        &mut self,
        path: Option<PathBuf>,
        same_album_consecutive: bool,
        fallback_duration_ms: u64,
    ) {
        match path {
            Some(p) => {
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
                    replay_gain_db: 0,
                    fallback_duration_ms,
                });
            }
            None => {
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
    mut producer: rtrb::Producer<[f32; 2]>,
    out_rate: u32,
    mut eq_config: EqConfig,
    telemetry: Arc<AudioTelemetry>,
    headless: bool,
) {
    let mut rt = DecodeRuntime::new(out_rate, &eq_config);
    let mut target_volume = 0.85_f32;
    let mut smoothed_volume = 0.85_f32;
    let mut dsp_scratch = Vec::with_capacity(2048);

    let mut pending_cmd: Option<EngineCommand> = None;

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
                    rt.set_queued_next(path, false, 0);
                }
                EngineCommand::QueueNextTrack {
                    path,
                    same_album_consecutive,
                    fallback_duration_ms,
                } => {
                    rt.set_queued_next(path, same_album_consecutive, fallback_duration_ms);
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
                    telemetry.flush_ring.store(true, Ordering::Release);
                    telemetry.is_playing.store(false, Ordering::Release);
                    telemetry.auto_transitioned.store(false, Ordering::Release);
                    telemetry.position_ms.store(0, Ordering::Relaxed);
                    telemetry.store_spectrum([0; 8]);
                    rt.deck_a = None;
                    rt.deck_b = None;
                    rt.crossfade_state = None;
                    rt.micro_fade = MicroFade::Steady(0.0);
                    rt.bass_swap.reset();
                }
                EngineCommand::SeekMs(ms) => {
                    if rt
                        .crossfade_state
                        .is_some_and(|xf| xf.ui_handed_off || xf.is_manual_switch)
                    {
                        rt.promote_deck_b(&telemetry, false);
                    }
                    if let Some(deck) = &mut rt.deck_a
                        && let Ok(actual) = deck.seek_ms(ms)
                    {
                        telemetry.flush_ring.store(true, Ordering::Release);
                        telemetry.position_ms.store(actual, Ordering::Relaxed);
                        rt.eq.reset();
                        if rt.crossfade_state.is_some() {
                            rt.crossfade_state = None;
                            rt.deck_b = None;
                            telemetry.is_crossfading.store(false, Ordering::Relaxed);
                            rt.bass_swap.reset();
                        }
                        if rt.dsp.fade_in_play_ms > 0 {
                            rt.micro_fade = MicroFade::FadingIn {
                                start_gain: 0.0,
                                elapsed_frames: 0,
                                total_frames: ms_to_frames(rt.dsp.fade_in_play_ms, out_rate).max(1),
                            };
                        }
                    }
                }
                EngineCommand::SetVolume(v) => {
                    target_volume = v.clamp(0.0, 1.5);
                }
                EngineCommand::SetEq(cfg) => {
                    eq_config = cfg;
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
                EngineCommand::Shutdown => return,
            }
        }

        let is_playing = telemetry.is_playing.load(Ordering::Relaxed);
        if !is_playing || rt.deck_a.is_none() {
            let idle_bins = rt.spectrum.decay_idle();
            telemetry.store_spectrum(idle_bins);
            if idle_bins == [0; 8] {
                // Zero-CPU idle wait: wakes instantaneously (<0.1 ms) on next command instead of polling at 83 Hz.
                match cmd_rx.recv_timeout(Duration::from_millis(120)) {
                    Ok(cmd) => pending_cmd = Some(cmd),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            } else {
                thread::sleep(Duration::from_millis(16));
            }
            continue;
        }

        let max_frames = if headless {
            512
        } else {
            let slots = producer.slots();
            if slots < 256 {
                thread::sleep(Duration::from_millis(3));
                continue;
            }
            slots.min(1024)
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
                    Err(err) => {
                        eprintln!(
                            "ferric-audio Deck B pre-open failed ({}): {err}",
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
            let eff_xfade_ms = if album_locked { 0 } else { rt.dsp.crossfade_ms };
            let rem_ms = da.remaining_ms();
            let sustained_silence = da.quiet_outro_frames >= (out_rate as usize / 12).max(512)
                && da.last_chunk_rms < 0.003;

            let time_trigger =
                eff_xfade_ms > 0 && da.duration_ms > 0 && rem_ms <= u64::from(eff_xfade_ms);
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
                        Some(sample_a)
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
            rt.eq.process_interleaved_stereo_ramped(
                &mut dsp_scratch,
                &mut smoothed_volume,
                target_volume,
            );
            telemetry.store_spectrum(rt.spectrum.analyze_interleaved(&dsp_scratch));
            if !headless {
                for &[l, r] in dsp_scratch.as_chunks::<2>().0 {
                    if producer.push([l, r]).is_err() {
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
            fallback_duration_ms: dur0,
        });
        let seek_target = dur1.saturating_sub(2_400);
        engine.send(EngineCommand::SeekMs(seek_target));

        let mut saw_auto_xfade = false;
        let mut handed_off_during_xfade = false;
        let mut promoted = false;
        for _ in 0..450 {
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
}
