use crate::AudioResult;
use ferric_core::models::{Track, TrackSource};
use std::f32::consts::TAU;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

const DEMO_SAMPLE_RATE: u32 = 44_100;

struct DemoSpec {
    id: &'static str,
    file_name: &'static str,
    title: &'static str,
    artist: &'static str,
    album: &'static str,
    duration_secs: u32,
    kind: SynthProgram,
}

#[derive(Clone, Copy)]
enum SynthProgram {
    SlowRefraction,
    OxideMidnight,
    VelvetAutomixPt1,
    VelvetAutomixPt2,
}

const DEMO_SPECS: [DemoSpec; 4] = [
    DemoSpec {
        id: "demo:01_slow_refraction",
        file_name: "01_slow_refraction.wav",
        title: "Slow Refraction",
        artist: "Mira Vale",
        album: "Liquid Horizon",
        duration_secs: 16,
        kind: SynthProgram::SlowRefraction,
    },
    DemoSpec {
        id: "demo:02_oxide_midnight",
        file_name: "02_oxide_midnight.wav",
        title: "Oxide Midnight",
        artist: "Ferric Tape Lab",
        album: "Type-I Sessions",
        duration_secs: 17,
        kind: SynthProgram::OxideMidnight,
    },
    DemoSpec {
        id: "demo:03_velvet_automix_pt1",
        file_name: "03_velvet_automix_pt1.wav",
        title: "Velvet Automix, Pt. 1",
        artist: "Analog Drift",
        album: "Seamless Suite",
        duration_secs: 15,
        kind: SynthProgram::VelvetAutomixPt1,
    },
    DemoSpec {
        id: "demo:04_velvet_automix_pt2",
        file_name: "04_velvet_automix_pt2.wav",
        title: "Velvet Automix, Pt. 2",
        artist: "Analog Drift",
        album: "Seamless Suite",
        duration_secs: 16,
        kind: SynthProgram::VelvetAutomixPt2,
    },
];

/// Ensure 4 musical 16-bit / 44.1 kHz stereo analog-pad WAV tracks exist in `target_dir`
/// for out-of-the-box testing of 0ms Gapless Album Lock, 6s Bass-Swap Crossfade, and 10-band EQ.
pub fn ensure_demo_tracks(target_dir: &Path) -> AudioResult<Vec<Track>> {
    std::fs::create_dir_all(target_dir)?;
    let mut tracks = Vec::with_capacity(DEMO_SPECS.len());

    for spec in &DEMO_SPECS {
        let wav_path = target_dir.join(spec.file_name);
        let needs_render = match std::fs::metadata(&wav_path) {
            Ok(meta) => meta.len() < 4_096,
            Err(_) => true,
        };
        if needs_render {
            render_demo_wav(&wav_path, spec.duration_secs, spec.kind)?;
        }

        tracks.push(Track {
            id: spec.id.to_string(),
            title: spec.title.to_string(),
            artist: spec.artist.to_string(),
            album: spec.album.to_string(),
            duration_ms: u64::from(spec.duration_secs) * 1_000,
            path: Some(wav_path),
            source: TrackSource::Local,
            codec: "WAV".to_string(),
            sample_rate: DEMO_SAMPLE_RATE,
            bit_depth: 16,
            bitrate_kbps: 1_411,
            isrc: None,
            spotify_id: None,
            youtube_video_id: None,
            replay_gain_db: 0,
            liked: false,
        });
    }

    Ok(tracks)
}

fn render_demo_wav(path: &Path, duration_secs: u32, program: SynthProgram) -> AudioResult<()> {
    let total_frames = (DEMO_SAMPLE_RATE as usize) * (duration_secs as usize);
    let data_bytes_len = total_frames * 4;
    let mut pcm_bytes = Vec::with_capacity(44 + data_bytes_len);

    // Standard 44-byte RIFF/WAVE 16-bit stereo PCM header.
    let riff_size = 36_u32.saturating_add(data_bytes_len as u32);
    pcm_bytes.extend_from_slice(b"RIFF");
    pcm_bytes.extend_from_slice(&riff_size.to_le_bytes());
    pcm_bytes.extend_from_slice(b"WAVE");
    pcm_bytes.extend_from_slice(b"fmt ");
    pcm_bytes.extend_from_slice(&16_u32.to_le_bytes());
    pcm_bytes.extend_from_slice(&1_u16.to_le_bytes()); // Linear PCM
    pcm_bytes.extend_from_slice(&2_u16.to_le_bytes()); // Stereo
    pcm_bytes.extend_from_slice(&DEMO_SAMPLE_RATE.to_le_bytes());
    pcm_bytes.extend_from_slice(&(DEMO_SAMPLE_RATE * 4).to_le_bytes());
    pcm_bytes.extend_from_slice(&4_u16.to_le_bytes()); // Block align
    pcm_bytes.extend_from_slice(&16_u16.to_le_bytes()); // 16-bit
    pcm_bytes.extend_from_slice(b"data");
    pcm_bytes.extend_from_slice(&(data_bytes_len as u32).to_le_bytes());

    let inv_sr = 1.0_f32 / (DEMO_SAMPLE_RATE as f32);
    let dur = duration_secs as f32;

    for frame_idx in 0..total_frames {
        let t = (frame_idx as f32) * inv_sr;
        let (l, r) = match program {
            SynthProgram::SlowRefraction => synth_slow_refraction(t, dur),
            SynthProgram::OxideMidnight => synth_oxide_midnight(t, dur),
            SynthProgram::VelvetAutomixPt1 => synth_velvet_suite_pt1(t, dur),
            SynthProgram::VelvetAutomixPt2 => synth_velvet_suite_pt2(t, dur),
        };
        let li = (l.clamp(-0.96, 0.96) * 32_767.0) as i16;
        let ri = (r.clamp(-0.96, 0.96) * 32_767.0) as i16;
        pcm_bytes.extend_from_slice(&li.to_le_bytes());
        pcm_bytes.extend_from_slice(&ri.to_le_bytes());
    }

    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    writer.write_all(&pcm_bytes)?;
    writer.flush()?;
    Ok(())
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() < 1e-6 {
        return 1.0;
    }
    let u = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

#[inline]
fn warm_pad_voice(t: f32, freq: f32, detune_hz: f32) -> (f32, f32) {
    let fl = freq - detune_hz;
    let fr = freq + detune_hz;
    let c = (TAU * freq * t).sin();
    let l = 0.52 * (TAU * fl * t).sin()
        + 0.28 * c
        + 0.14 * (TAU * (fl * 2.0) * t).sin()
        + 0.06 * (TAU * (freq * 3.0) * t).sin();
    let r = 0.52 * (TAU * fr * t).sin()
        + 0.28 * c
        + 0.14 * (TAU * (fr * 2.0) * t).sin()
        + 0.06 * (TAU * (freq * 3.0) * t + 0.4).sin();
    (l, r)
}

fn synth_slow_refraction(t: f32, dur: f32) -> (f32, f32) {
    // Dmaj9 -> Bm11 -> Gmaj9 -> Asus4 warm glass pad with sub-bass & crystalline bell shimmer.
    let chord: [f32; 4] = if t < 4.0 {
        [146.83, 220.00, 277.18, 329.63]
    } else if t < 8.0 {
        [123.47, 185.00, 220.00, 293.66]
    } else if t < 12.0 {
        [98.00, 196.00, 246.94, 293.66]
    } else {
        [110.00, 220.00, 293.66, 329.63]
    };
    let bass_f = chord[0] * 0.5;
    let sub = 0.26 * (TAU * bass_f * t).sin() + 0.09 * (TAU * bass_f * 2.0 * t).sin();

    let mut l = sub;
    let mut r = sub;
    for (i, &f) in chord.iter().enumerate() {
        let (vl, vr) = warm_pad_voice(t, f, 0.35 + (i as f32) * 0.12);
        l += vl * 0.14;
        r += vr * 0.14;
    }

    // Gentle 8th-note harmonic bell overtones for 4 kHz - 8 kHz EQ auditioning.
    let step = ((t * 2.0).floor() as usize) % 4;
    let local = (t * 2.0).fract();
    let bell_env = (-local * 5.5).exp() * smoothstep(0.0, 0.015, local);
    let bell_f = chord[step] * 4.0;
    let bell = 0.07 * bell_env * (TAU * bell_f * t).sin();
    let pan = 0.5 + 0.35 * (TAU * 0.25 * t).sin();
    l += bell * pan;
    r += bell * (1.0 - pan);

    let env = smoothstep(0.0, 0.45, t) * smoothstep(dur, dur - 2.6, t);
    (l * env * 0.85, r * env * 0.85)
}

fn synth_oxide_midnight(t: f32, dur: f32) -> (f32, f32) {
    // Deep warm analog tape progression (Cm9 -> Abmaj7 -> Fm9 -> G7sus4) with rhythmic bass pulse.
    let chord: [f32; 4] = if t < 4.25 {
        [130.81, 196.00, 233.08, 293.66]
    } else if t < 8.5 {
        [103.83, 155.56, 196.00, 261.63]
    } else if t < 12.75 {
        [87.31, 174.61, 207.65, 261.63]
    } else {
        [98.00, 174.61, 196.00, 261.63]
    };

    let pulse_phase = (t * 2.0).fract();
    let pulse_env = 0.45 + 0.55 * (-pulse_phase * 3.8).exp() * smoothstep(0.0, 0.012, pulse_phase);
    let bass_f = chord[0] * 0.5;
    let bass = pulse_env * (0.30 * (TAU * bass_f * t).sin() + 0.11 * (TAU * chord[0] * t).sin());

    let wow = 1.0 + 0.0012 * (TAU * 0.55 * t).sin();
    let mut l = bass;
    let mut r = bass;
    for (i, &f) in chord.iter().enumerate() {
        let (vl, vr) = warm_pad_voice(t, f * wow, 0.42 + (i as f32) * 0.15);
        l += vl * 0.13;
        r += vr * 0.13;
    }

    let env = smoothstep(0.0, 0.35, t) * smoothstep(dur, dur - 2.8, t);
    (l * env * 0.85, r * env * 0.85)
}

/// Shared continuous harmonic bridge voice at the exact seam `t_abs = 15.0s` between
/// `Velvet Automix, Pt. 1` (duration `15.0s`) and `Velvet Automix, Pt. 2` (`t_abs = 15.0 + t`).
fn seamless_suite_bridge(t_abs: f32) -> (f32, f32) {
    let bridge_chord = [110.00_f32, 164.81, 220.00, 261.63, 329.63]; // Am9 lush drone
    let sub = 0.24 * (TAU * 55.0 * t_abs).sin() + 0.08 * (TAU * 110.0 * t_abs).sin();
    let mut l = sub;
    let mut r = sub;
    for (i, &f) in bridge_chord.iter().enumerate() {
        let (vl, vr) = warm_pad_voice(t_abs, f, 0.28 + (i as f32) * 0.10);
        l += vl * 0.115;
        r += vr * 0.115;
    }
    (l * 0.82, r * 0.82)
}

fn synth_velvet_suite_pt1(t: f32, dur: f32) -> (f32, f32) {
    // Morphs smoothly into the phase-locked `seamless_suite_bridge(t)` over the final 3 seconds,
    // ending at full continuous sustain with ZERO drop to silence so 0ms Gapless Album Lock is audible.
    let chord: [f32; 4] = if t < 5.0 {
        [174.61, 220.00, 261.63, 329.63] // Fmaj7
    } else if t < 10.0 {
        [146.83, 220.00, 261.63, 329.63] // Dm9
    } else {
        [110.00, 164.81, 220.00, 329.63] // Am9
    };

    let sub = 0.24 * (TAU * (chord[0] * 0.5) * t).sin();
    let mut body_l = sub;
    let mut body_r = sub;
    for (i, &f) in chord.iter().enumerate() {
        let (vl, vr) = warm_pad_voice(t, f, 0.30 + (i as f32) * 0.11);
        body_l += vl * 0.135;
        body_r += vr * 0.135;
    }

    let morph_to_bridge = smoothstep(dur - 3.5, dur - 0.1, t);
    let (br_l, br_r) = seamless_suite_bridge(t);
    let intro = smoothstep(0.0, 0.45, t);
    let l = ((1.0 - morph_to_bridge) * body_l * 0.82 + morph_to_bridge * br_l) * intro;
    let r = ((1.0 - morph_to_bridge) * body_r * 0.82 + morph_to_bridge * br_r) * intro;
    (l, r)
}

fn synth_velvet_suite_pt2(t: f32, dur: f32) -> (f32, f32) {
    // Starts at `t_abs = 15.0 + t` phase-locked to the exact final sample of Pt. 1,
    // then blossoms into a shimmering Cmaj9 / G6 resolution.
    let t_abs = 15.0 + t;
    let (br_l, br_r) = seamless_suite_bridge(t_abs);

    let chord: [f32; 4] = if t < 5.5 {
        [130.81, 196.00, 246.94, 293.66] // Cmaj9
    } else if t < 11.0 {
        [98.00, 196.00, 246.94, 329.63] // G6
    } else {
        [174.61, 220.00, 261.63, 392.00] // Fmaj9
    };

    let sub = 0.25 * (TAU * (chord[0] * 0.5) * t_abs).sin();
    let mut body_l = sub;
    let mut body_r = sub;
    for (i, &f) in chord.iter().enumerate() {
        let (vl, vr) = warm_pad_voice(t_abs, f, 0.32 + (i as f32) * 0.12);
        body_l += vl * 0.135;
        body_r += vr * 0.135;
    }

    let step = ((t * 3.0).floor() as usize) % 4;
    let local = (t * 3.0).fract();
    let arp = 0.06
        * (-local * 6.0).exp()
        * smoothstep(0.0, 0.01, local)
        * (TAU * chord[step] * 2.0 * t_abs).sin();

    let morph_from_bridge = smoothstep(0.1, 3.2, t);
    let outro = smoothstep(dur, dur - 2.8, t);
    let l = ((1.0 - morph_from_bridge) * br_l + morph_from_bridge * (body_l * 0.82 + arp)) * outro;
    let r = ((1.0 - morph_from_bridge) * br_r + morph_from_bridge * (body_r * 0.82 + arp)) * outro;
    (l, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seamless_suite_bridge_is_continuous_across_pt1_and_pt2_boundary() {
        let dt = 1.0 / (DEMO_SAMPLE_RATE as f32);
        let end_pt1 = synth_velvet_suite_pt1(15.0 - dt, 15.0);
        let start_pt2 = synth_velvet_suite_pt2(0.0, 16.0);
        assert!((end_pt1.0 - start_pt2.0).abs() < 0.02);
        assert!((end_pt1.1 - start_pt2.1).abs() < 0.02);
    }
}
