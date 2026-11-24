use crate::dsp::{StereoEqProcessor, equal_power_gains};
use crate::{AudioError, AudioResult};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ferric_core::eq::EqConfig;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, PolynomialDegree, Resampler};
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
}

impl AudioTelemetry {
    pub fn snapshot(&self) -> TelemetrySnapshot {
        TelemetrySnapshot {
            is_playing: self.is_playing.load(Ordering::Relaxed),
            position_ms: self.position_ms.load(Ordering::Relaxed),
            duration_ms: self.duration_ms.load(Ordering::Relaxed),
            sample_rate: self.sample_rate.load(Ordering::Relaxed),
            output_rate: self.output_rate.load(Ordering::Relaxed),
            track_finished_seq: self.track_finished_seq.load(Ordering::Relaxed),
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
}

#[derive(Debug)]
pub enum EngineCommand {
    LoadAndPlay(PathBuf),
    QueueNext(Option<PathBuf>),
    Play,
    Pause,
    TogglePlayPause,
    Stop,
    SeekMs(u64),
    SetVolume(f32),
    SetEq(EqConfig),
    SetCrossfadeMs(u32),
    Shutdown,
}

pub struct AudioEngine {
    cmd_tx: Sender<EngineCommand>,
    telemetry: Arc<AudioTelemetry>,
    _output_stream: Option<cpal::Stream>,
}

impl AudioEngine {
    /// Start the real-time WASAPI/cpal output stream and dedicated decode+DSP OS thread.
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
        let stream_cfg: cpal::StreamConfig = default_cfg.into();

        // ~350 ms wait-free SPSC ring buffer (stereo interleaved f32).
        let ring_samples = ((out_rate as usize * 350) / 1000).max(4096) * 2;
        let (producer, mut consumer) = rtrb::RingBuffer::<f32>::new(ring_samples);

        let telemetry = Arc::new(AudioTelemetry::default());
        telemetry.output_rate.store(out_rate, Ordering::Relaxed);
        let playing_flag = Arc::clone(&telemetry);

        let stream = device
            .build_output_stream(
                &stream_cfg,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if !playing_flag.is_playing.load(Ordering::Relaxed) {
                        data.fill(0.0);
                        return;
                    }
                    // Zero-allocation real-time audio hardware callback.
                    for frame in data.chunks_exact_mut(out_channels) {
                        let l = consumer.pop().unwrap_or(0.0);
                        let r = consumer.pop().unwrap_or(0.0);
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
                |err| {
                    eprintln!("ferric-audio stream error: {err}");
                },
                None,
            )
            .map_err(|e| AudioError::Device(e.to_string()))?;

        stream
            .play()
            .map_err(|e| AudioError::Device(e.to_string()))?;

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let thread_telemetry = Arc::clone(&telemetry);
        thread::Builder::new()
            .name("ferric-audio-dsp".to_string())
            .spawn(move || {
                decode_loop(cmd_rx, producer, out_rate, initial_eq, thread_telemetry);
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
        let thread_telemetry = Arc::clone(&telemetry);
        let (producer, _consumer) = rtrb::RingBuffer::<f32>::new(8192);
        let _ = thread::Builder::new()
            .name("ferric-audio-headless".to_string())
            .spawn(move || {
                decode_loop(cmd_rx, producer, 48_000, initial_eq, thread_telemetry);
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
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(EngineCommand::Shutdown);
    }
}

struct ActiveDeck {
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
    eof: bool,
}

impl ActiveDeck {
    fn open(path: &Path, out_rate: u32, codecs: &CodecRegistry) -> AudioResult<Self> {
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
                FormatOptions {
                    enable_gapless: true,
                    ..Default::default()
                },
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
        let duration_ms = match (time_base, num_frames) {
            (Some(tb), Some(frames)) => tb
                .calc_duration(SymDuration::from(frames))
                .map(|t| t.as_millis().max(0) as u64)
                .unwrap_or(0),
            (_, Some(frames)) if sample_rate > 0 => (frames * 1000) / u64::from(sample_rate),
            _ => 0,
        };

        let decoder = codecs
            .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
            .map_err(|e| AudioError::Decode(e.to_string()))?;

        let resampler = if sample_rate != out_rate {
            StereoResampler::new(sample_rate, out_rate).ok()
        } else {
            None
        };

        Ok(Self {
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
            eof: false,
        })
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
        if let Some(res) = &mut self.resampler {
            res.reset();
        }
        let actual_ms = self
            .time_base
            .and_then(|tb| tb.calc_time(seeked.actual_ts))
            .map(|t| t.as_millis().max(0) as u64)
            .unwrap_or(target_ms);
        Ok(actual_ms)
    }

    /// Decode next chunk into stereo `f32` samples at hardware `out_rate`.
    fn decode_next_chunk(&mut self, raw_buf: &mut Vec<f32>) -> Option<u64> {
        if self.eof {
            return None;
        }
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(pkt)) => pkt,
                Ok(None) => {
                    self.eof = true;
                    return None;
                }
                Err(SymphoniaError::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    self.eof = true;
                    return None;
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(_) => {
                    self.eof = true;
                    return None;
                }
            };

            if packet.track_id != self.track_id {
                continue;
            }

            let pos_ms = self
                .time_base
                .and_then(|tb| tb.calc_time(packet.pts))
                .map(|t| t.as_millis().max(0) as u64);

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
                    if !self.pending_stereo.is_empty() {
                        return pos_ms;
                    }
                }
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(_) => {
                    self.eof = true;
                    return None;
                }
            }
        }
    }
}

struct StereoResampler {
    inner: Async<f32>,
    in_fifo: Vec<f32>,
    out_scratch: Vec<f32>,
}

impl StereoResampler {
    fn new(in_rate: u32, out_rate: u32) -> AudioResult<Self> {
        let ratio = f64::from(out_rate) / f64::from(in_rate.max(8_000));
        let chunk_frames = 512;
        let inner = Async::<f32>::new_poly(
            ratio,
            1.1,
            PolynomialDegree::Cubic,
            chunk_frames,
            2,
            FixedAsync::Input,
        )
        .map_err(|e| AudioError::Decode(e.to_string()))?;
        let out_max = inner.output_frames_max() * 2 + 64;
        Ok(Self {
            inner,
            in_fifo: Vec::with_capacity(chunk_frames * 4),
            out_scratch: vec![0.0; out_max],
        })
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
            if let (Ok(in_buf), Ok(mut out_buf)) = (in_adapter, out_adapter) {
                if let Ok((_, frames_out)) =
                    self.inner.process_into_buffer(&in_buf, &mut out_buf, None)
                {
                    output.extend_from_slice(&self.out_scratch[..frames_out * 2]);
                }
            }
            self.in_fifo.drain(..needed_samples);
        }
    }
}

fn build_codec_registry() -> CodecRegistry {
    let mut codecs = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut codecs);
    codecs.register_audio_decoder_at_tier::<OpusDecoder>(Tier::Fallback);
    codecs
}

fn decode_loop(
    cmd_rx: Receiver<EngineCommand>,
    mut producer: rtrb::Producer<f32>,
    out_rate: u32,
    mut eq_config: EqConfig,
    telemetry: Arc<AudioTelemetry>,
) {
    let codecs = build_codec_registry();
    let mut eq = StereoEqProcessor::new(out_rate, &eq_config);
    let mut deck_a: Option<ActiveDeck> = None;
    let mut queued_next_path: Option<PathBuf> = None;
    let mut volume = 0.85_f32;
    let mut crossfade_ms = 0_u32;
    let mut raw_scratch = Vec::with_capacity(8192);
    let mut dsp_scratch = Vec::with_capacity(2048);

    loop {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                EngineCommand::LoadAndPlay(path) => {
                    match ActiveDeck::open(&path, out_rate, &codecs) {
                        Ok(deck) => {
                            telemetry
                                .duration_ms
                                .store(deck.duration_ms, Ordering::Relaxed);
                            telemetry
                                .sample_rate
                                .store(deck.sample_rate, Ordering::Relaxed);
                            telemetry.position_ms.store(0, Ordering::Relaxed);
                            telemetry.is_playing.store(true, Ordering::Relaxed);
                            eq.reset();
                            deck_a = Some(deck);
                        }
                        Err(err) => {
                            eprintln!("ferric-audio open failed ({}): {err}", path.display());
                            telemetry.is_playing.store(false, Ordering::Relaxed);
                        }
                    }
                }
                EngineCommand::QueueNext(next) => {
                    queued_next_path = next;
                }
                EngineCommand::Play => {
                    if deck_a.is_some() {
                        telemetry.is_playing.store(true, Ordering::Relaxed);
                    }
                }
                EngineCommand::Pause => {
                    telemetry.is_playing.store(false, Ordering::Relaxed);
                }
                EngineCommand::TogglePlayPause => {
                    let cur = telemetry.is_playing.load(Ordering::Relaxed);
                    if deck_a.is_some() {
                        telemetry.is_playing.store(!cur, Ordering::Relaxed);
                    }
                }
                EngineCommand::Stop => {
                    telemetry.is_playing.store(false, Ordering::Relaxed);
                    telemetry.position_ms.store(0, Ordering::Relaxed);
                    deck_a = None;
                }
                EngineCommand::SeekMs(ms) => {
                    if let Some(deck) = &mut deck_a {
                        if let Ok(actual) = deck.seek_ms(ms) {
                            telemetry.position_ms.store(actual, Ordering::Relaxed);
                            eq.reset();
                        }
                    }
                }
                EngineCommand::SetVolume(v) => {
                    volume = v.clamp(0.0, 1.5);
                }
                EngineCommand::SetEq(cfg) => {
                    eq_config = cfg;
                    eq.apply_config(&eq_config);
                }
                EngineCommand::SetCrossfadeMs(ms) => {
                    crossfade_ms = ms.min(12_000);
                }
                EngineCommand::Shutdown => return,
            }
        }

        let is_playing = telemetry.is_playing.load(Ordering::Relaxed);
        if !is_playing || deck_a.is_none() {
            thread::sleep(Duration::from_millis(12));
            continue;
        }

        let slots = producer.slots();
        if slots < 512 {
            thread::sleep(Duration::from_millis(4));
            continue;
        }

        let mut track_ended = false;
        if let Some(deck) = &mut deck_a {
            if deck.pending_offset >= deck.pending_stereo.len() {
                if let Some(pos_ms) = deck.decode_next_chunk(&mut raw_scratch) {
                    telemetry.position_ms.store(pos_ms, Ordering::Relaxed);
                } else if deck.eof {
                    track_ended = true;
                }
            }

            if !track_ended && deck.pending_offset < deck.pending_stereo.len() {
                let avail = deck.pending_stereo.len() - deck.pending_offset;
                let to_take = avail.min(slots & !1).min(2048);
                dsp_scratch.clear();
                dsp_scratch.extend_from_slice(
                    &deck.pending_stereo[deck.pending_offset..deck.pending_offset + to_take],
                );
                deck.pending_offset += to_take;

                // Subtle equal-power tail fade when crossfade is active near track end.
                if crossfade_ms > 0 && deck.duration_ms > u64::from(crossfade_ms) {
                    let pos = telemetry.position_ms.load(Ordering::Relaxed);
                    let rem = deck.duration_ms.saturating_sub(pos);
                    if rem < u64::from(crossfade_ms) {
                        let prog = 1.0 - (rem as f32 / crossfade_ms as f32);
                        let (ga, _) = equal_power_gains(prog);
                        for s in &mut dsp_scratch {
                            *s *= ga;
                        }
                    }
                }

                eq.process_interleaved_stereo(&mut dsp_scratch, volume);
                for &sample in &dsp_scratch {
                    if producer.push(sample).is_err() {
                        break;
                    }
                }
            }
        }

        if track_ended {
            // Gapless transition into pre-queued next track if available; otherwise signal UI queue.
            if let Some(next_path) = queued_next_path.take() {
                if let Ok(next_deck) = ActiveDeck::open(&next_path, out_rate, &codecs) {
                    telemetry
                        .duration_ms
                        .store(next_deck.duration_ms, Ordering::Relaxed);
                    telemetry
                        .sample_rate
                        .store(next_deck.sample_rate, Ordering::Relaxed);
                    telemetry.position_ms.store(0, Ordering::Relaxed);
                    deck_a = Some(next_deck);
                    telemetry.track_finished_seq.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            }
            deck_a = None;
            telemetry.is_playing.store(false, Ordering::Relaxed);
            telemetry.track_finished_seq.fetch_add(1, Ordering::Relaxed);
        }
    }
}
