use ferric_core::app_data_dir;
use ferric_core::models::Track;
use ferric_providers::local_scanner::read_embedded_cover_bytes;
use ferric_providers::ytmusic::{download_artwork_bytes, normalize_ytm_thumbnail_url};
use makepad_widgets::image_cache::ImageBuffer;
use makepad_widgets::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const FAILED_TTL: Duration = Duration::from_secs(120);
const MAX_FAILED_ENTRIES: usize = 256;

/// Single lean sequential decoder thread avoids parallel full-res JPEG/PNG decode RAM spikes
/// while decoding disk-cached covers rapidly.
const WORKER_THREADS: usize = 1;
/// Hard LRU cap for 160x160 grid/sidebar/queue thumbnails (~100 KB each, <= 11.2 MB VRAM total;
/// CPU `Vec<u32>` freed immediately on D3D11 upload). Hero 256x256 artwork lives in its own
/// dedicated single slot (`hero_entry`) so high-res covers never accumulate across long sessions.
const MAX_CACHED_TEXTURES: usize = 112;
/// Crisp card thumbnail size (area-box downscaled + hardware D3D11 linear filtering: sharp on
/// 200px Grid/Shelf/Station cards without stair-stepping or memory bloat).
const THUMB_MAX_DIM: usize = 160;
/// High-detail size reserved solely for the currently playing hero/fullscreen artwork.
const HERO_MAX_DIM: usize = 256;

#[derive(Clone)]
pub struct CachedArtwork {
    pub texture: Texture,
    pub aspect: f32,
    pub accent: Vec4,
    /// Precomputed 4-quadrant liquid aurora pigments (identical to `FerricAmbientStage`'s 9-tap
    /// sampling + `enrich_tone`, computed once on CPU instead of 36 GPU texture fetches per pixel).
    pub aurora_zones: [[f32; 3]; 4],
    last_used: u64,
}

enum ArtworkJob {
    Url {
        key: String,
        url: String,
        max_dim: usize,
    },
    LocalFile {
        key: String,
        path: PathBuf,
        max_dim: usize,
    },
}

enum ArtworkDone {
    Loaded {
        key: String,
        buffer: ImageBuffer,
        accent_rgb: [f32; 3],
        aurora_zones: [[f32; 3]; 4],
        high_res: bool,
    },
    Failed {
        key: String,
        high_res: bool,
    },
}

/// Memory-bounded LRU album cover fetcher, disk cache, downscaling decoder,
/// dominant WCAG-safe accent extractor, and GPU texture registry.
pub struct ArtworkCache {
    entries: HashMap<String, CachedArtwork>,
    /// Single dedicated 256x256 high-resolution slot for the active Now Playing / Fullscreen cover.
    hero_entry: Option<(String, CachedArtwork)>,
    in_flight: HashSet<(String, bool)>,
    failed: HashMap<String, Instant>,
    pinned_key: Option<String>,
    paused_flag: Arc<AtomicBool>,
    clock: u64,
    job_queue: Arc<(Mutex<VecDeque<ArtworkJob>>, Condvar)>,
    done_rx: mpsc::Receiver<ArtworkDone>,
}

impl ArtworkCache {
    pub fn new() -> Self {
        let job_queue: Arc<(Mutex<VecDeque<ArtworkJob>>, Condvar)> =
            Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let (done_tx, done_rx) = mpsc::channel::<ArtworkDone>();
        let paused_flag = Arc::new(AtomicBool::new(false));
        let cache_dir = app_data_dir().join("artwork_cache");

        for _ in 0..WORKER_THREADS {
            let queue = Arc::clone(&job_queue);
            let tx = done_tx.clone();
            let dir = cache_dir.clone();
            let paused = Arc::clone(&paused_flag);
            thread::spawn(move || {
                loop {
                    let job = {
                        let (lock, cvar) = &*queue;
                        let Ok(mut guard) = lock.lock() else {
                            break;
                        };
                        while guard.is_empty() {
                            match cvar.wait(guard) {
                                Ok(g) => guard = g,
                                Err(_) => return,
                            }
                        }
                        match guard.pop_front() {
                            Some(j) => j,
                            None => continue,
                        }
                    };
                    if paused.load(Ordering::Acquire) {
                        continue;
                    }

                    let (key, max_dim, raw_bytes) = match job {
                        ArtworkJob::Url { key, url, max_dim } => {
                            let bytes = download_artwork_bytes(&url, &dir).ok();
                            (key, max_dim, bytes)
                        }
                        ArtworkJob::LocalFile { key, path, max_dim } => {
                            let bytes = read_embedded_cover_bytes(&path);
                            (key, max_dim, bytes)
                        }
                    };
                    if paused.load(Ordering::Acquire) {
                        continue;
                    }

                    let high_res = max_dim >= HERO_MAX_DIM;
                    let decoded = raw_bytes.and_then(|bytes| decode_and_downscale(&bytes, max_dim));
                    if paused.load(Ordering::Acquire) {
                        continue;
                    }
                    match decoded {
                        Some(buffer) if buffer.width > 0 && buffer.height > 0 => {
                            let accent_rgb = extract_vibrant_accent_rgb(&buffer);
                            let aurora_zones = extract_aurora_zones(&buffer);
                            let _ = tx.send(ArtworkDone::Loaded {
                                key,
                                buffer,
                                accent_rgb,
                                aurora_zones,
                                high_res,
                            });
                            SignalToUI::set_ui_signal();
                        }
                        _ => {
                            let _ = tx.send(ArtworkDone::Failed { key, high_res });
                        }
                    }
                }
            });
        }

        Self {
            entries: HashMap::with_capacity(MAX_CACHED_TEXTURES + 4),
            hero_entry: None,
            in_flight: HashSet::new(),
            failed: HashMap::new(),
            pinned_key: None,
            paused_flag,
            clock: 0,
            job_queue,
            done_rx,
        }
    }

    fn enqueue_job(&self, job: ArtworkJob) {
        let (lock, cvar) = &*self.job_queue;
        if let Ok(mut guard) = lock.lock() {
            guard.push_back(job);
            cvar.notify_one();
        }
    }

    fn is_recently_failed(&mut self, key: &str) -> bool {
        if let Some(&when) = self.failed.get(key) {
            if when.elapsed() < FAILED_TTL {
                return true;
            }
            self.failed.remove(key);
        }
        false
    }

    pub fn key_for_track(track: &Track) -> Option<String> {
        if let Some(url) = track
            .thumbnail_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let norm = normalize_ytm_thumbnail_url(url);
            if !norm.is_empty() {
                return Some(norm);
            }
        }
        if let Some(vid) = track
            .youtube_video_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| track.id.strip_prefix("yt:video:").filter(|s| !s.is_empty()))
        {
            return Some(format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg"));
        }
        // Avoid synchronous `Path::is_file()` syscalls during UI render; worker thread verifies existence.
        if let Some(path) = track.path.as_ref().filter(|p| !p.as_os_str().is_empty()) {
            return Some(format!("local:{}", path.to_string_lossy()));
        }
        None
    }

    /// Pin the active Now Playing track cover so LRU eviction never drops it and upgrade to 256px.
    pub fn pin_active_track(&mut self, track: Option<&Track>) {
        self.pinned_key = track.and_then(Self::key_for_track);
        if let Some(t) = track {
            self.request_for_track_sized(t, HERO_MAX_DIM);
        }
    }

    pub fn request_url(&mut self, raw_url: &str) -> Option<String> {
        self.request_url_sized(raw_url, THUMB_MAX_DIM)
    }

    pub fn request_url_sized(&mut self, raw_url: &str, max_dim: usize) -> Option<String> {
        if self.paused_flag.load(Ordering::Acquire) {
            return None;
        }
        let norm = normalize_ytm_thumbnail_url(raw_url);
        if norm.is_empty() {
            return None;
        }
        self.clock = self.clock.wrapping_add(1);
        let want_hi = max_dim >= HERO_MAX_DIM;
        if want_hi {
            if let Some((hk, hv)) = &mut self.hero_entry
                && *hk == norm
            {
                hv.last_used = self.clock;
                return Some(norm);
            }
        } else if let Some(existing) = self.entries.get_mut(&norm) {
            existing.last_used = self.clock;
            return Some(norm);
        }
        let inflight_key = (norm.clone(), want_hi);
        if !self.in_flight.contains(&inflight_key) && !self.is_recently_failed(&norm) {
            self.in_flight.insert(inflight_key);
            self.enqueue_job(ArtworkJob::Url {
                key: norm.clone(),
                url: norm.clone(),
                max_dim,
            });
        }
        Some(norm)
    }

    pub fn request_for_track(&mut self, track: &Track) -> Option<String> {
        self.request_for_track_sized(track, THUMB_MAX_DIM)
    }

    pub fn request_for_track_sized(&mut self, track: &Track, max_dim: usize) -> Option<String> {
        if self.paused_flag.load(Ordering::Acquire) {
            return None;
        }
        let key = Self::key_for_track(track)?;
        self.clock = self.clock.wrapping_add(1);
        let want_hi = max_dim >= HERO_MAX_DIM;
        if want_hi {
            if let Some((hk, hv)) = &mut self.hero_entry
                && *hk == key
            {
                hv.last_used = self.clock;
                return Some(key);
            }
        } else if let Some(existing) = self.entries.get_mut(&key) {
            existing.last_used = self.clock;
            return Some(key);
        }
        let inflight_key = (key.clone(), want_hi);
        if self.in_flight.contains(&inflight_key) || self.is_recently_failed(&key) {
            return Some(key);
        }
        self.in_flight.insert(inflight_key);
        if key.starts_with("http://") || key.starts_with("https://") {
            self.enqueue_job(ArtworkJob::Url {
                key: key.clone(),
                url: key.clone(),
                max_dim,
            });
        } else if let Some(path) = track.path.clone() {
            self.enqueue_job(ArtworkJob::LocalFile {
                key: key.clone(),
                path,
                max_dim,
            });
        }
        Some(key)
    }

    /// Poll completed artwork decodes. Returns `(any_changed, pinned_hero_changed)`.
    pub fn poll(&mut self, cx: &mut Cx) -> (bool, bool) {
        let mut changed = false;
        let mut pinned_changed = false;
        let mut needs_free = false;
        for _ in 0..24 {
            let Ok(msg) = self.done_rx.try_recv() else {
                break;
            };
            match msg {
                ArtworkDone::Loaded {
                    key,
                    buffer,
                    accent_rgb,
                    aurora_zones,
                    high_res,
                } => {
                    self.in_flight.remove(&(key.clone(), high_res));
                    self.clock = self.clock.wrapping_add(1);
                    let aspect =
                        (buffer.width as f32 / buffer.height.max(1) as f32).clamp(0.25, 4.0);
                    let texture = buffer.into_new_texture(cx);
                    texture.set_release_cpu_after_upload(cx, true);
                    let accent = vec4(accent_rgb[0], accent_rgb[1], accent_rgb[2], 1.0);
                    let is_pinned = self.pinned_key.as_deref() == Some(key.as_str());
                    let item = CachedArtwork {
                        texture,
                        aspect,
                        accent,
                        aurora_zones,
                        last_used: self.clock,
                    };
                    if high_res {
                        if self.hero_entry.replace((key, item)).is_some() {
                            needs_free = true;
                        }
                        if is_pinned {
                            pinned_changed = true;
                        }
                    } else {
                        if self.entries.insert(key, item).is_some() {
                            needs_free = true;
                        }
                        if is_pinned {
                            pinned_changed = true;
                        }
                        self.evict_lru_if_needed(cx);
                    }
                    changed = true;
                }
                ArtworkDone::Failed { key, high_res } => {
                    self.in_flight.remove(&(key.clone(), high_res));
                    if self.failed.len() >= MAX_FAILED_ENTRIES {
                        self.failed.retain(|_, t| t.elapsed() < FAILED_TTL);
                        if self.failed.len() >= MAX_FAILED_ENTRIES
                            && let Some(oldest) = self
                                .failed
                                .iter()
                                .min_by_key(|(_, t)| **t)
                                .map(|(k, _)| k.clone())
                        {
                            self.failed.remove(&oldest);
                        }
                    }
                    self.failed.insert(key, Instant::now());
                }
            }
        }
        if needs_free {
            cx.textures.free_unused_textures();
        }
        (changed, pinned_changed)
    }

    fn evict_lru_if_needed(&mut self, cx: &mut Cx) {
        let mut evicted = false;
        while self.entries.len() > MAX_CACHED_TEXTURES {
            let oldest_key = self
                .entries
                .iter()
                .filter(|(k, _)| self.pinned_key.as_deref() != Some(k.as_str()))
                .min_by_key(|(_, v)| v.last_used)
                .map(|(k, _)| k.clone());
            let Some(victim) = oldest_key else {
                break;
            };
            self.entries.remove(&victim);
            evicted = true;
        }
        if evicted {
            cx.textures.free_unused_textures();
        }
    }

    pub fn has_pending_work(&self) -> bool {
        !self.in_flight.is_empty()
    }

    /// Release every decoded cover texture while hidden in System Tray and pause background
    /// artwork decoding so hidden track changes never pile up unread RGBA buffers in `done_rx`.
    pub fn clear_all_for_tray(&mut self, cx: &mut Cx) {
        self.paused_flag.store(true, Ordering::Release);
        let (lock, _) = &*self.job_queue;
        if let Ok(mut guard) = lock.lock() {
            guard.clear();
            guard.shrink_to_fit();
        }
        self.hero_entry = None;
        self.entries.clear();
        self.entries.shrink_to_fit();
        self.in_flight.clear();
        self.in_flight.shrink_to_fit();
        self.failed.clear();
        self.failed.shrink_to_fit();
        while self.done_rx.try_recv().is_ok() {}
        cx.textures.free_unused_textures();
    }

    /// Resume artwork decoding when restoring the window from System Tray.
    pub fn resume_from_tray(&mut self) {
        let (lock, _) = &*self.job_queue;
        if let Ok(mut guard) = lock.lock() {
            guard.clear();
        }
        while self.done_rx.try_recv().is_ok() {}
        self.paused_flag.store(false, Ordering::Release);
    }

    pub fn get(&self, key: &str) -> Option<&CachedArtwork> {
        self.entries.get(key).or_else(|| {
            self.hero_entry
                .as_ref()
                .and_then(|(k, v)| (k == key).then_some(v))
        })
    }

    /// Prefer the 256x256 high-resolution hero entry for the active Now Playing / Fullscreen cover,
    /// falling back to the 160x160 grid thumbnail until hero decoding finishes.
    pub fn get_hero(&self, key: &str) -> Option<&CachedArtwork> {
        if let Some((hk, hv)) = &self.hero_entry
            && hk == key
        {
            return Some(hv);
        }
        self.entries.get(key)
    }

    pub fn get_for_track(&self, track: &Track) -> Option<&CachedArtwork> {
        let key = Self::key_for_track(track)?;
        self.get_hero(&key)
    }

    pub fn snapshot(&self) -> HashMap<String, CachedArtwork> {
        self.entries.clone()
    }

    pub fn clear_memory(&mut self) {
        self.hero_entry = None;
        self.entries.clear();
        self.entries.shrink_to_fit();
        self.failed.clear();
    }
}

const MAX_DECODE_DIM: u32 = 4096;

/// Pre-check PNG/JPEG header dimensions before allocating full decoded pixel buffers (blocks decompression bombs).
fn check_image_header_dimensions(bytes: &[u8]) -> bool {
    if bytes.len() >= 24 && bytes[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return w > 0 && h > 0 && w <= MAX_DECODE_DIM && h <= MAX_DECODE_DIM;
    }
    if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        let mut i = 2usize;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            while i < bytes.len() && bytes[i] == 0xFF {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let marker = bytes[i];
            i += 1;
            if marker == 0xD9 || marker == 0xDA {
                break;
            }
            if marker == 0x01 || (0xD0..=0xD7).contains(&marker) || marker == 0x00 {
                continue;
            }
            if i + 2 > bytes.len() {
                break;
            }
            let seg_len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
            if seg_len < 2 || i + seg_len > bytes.len() {
                break;
            }
            if matches!(
                marker,
                0xC0 | 0xC1
                    | 0xC2
                    | 0xC3
                    | 0xC5
                    | 0xC6
                    | 0xC7
                    | 0xC9
                    | 0xCA
                    | 0xCB
                    | 0xCD
                    | 0xCE
                    | 0xCF
            ) && seg_len >= 7
            {
                let h = u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                return w > 0 && h > 0 && w <= MAX_DECODE_DIM && h <= MAX_DECODE_DIM;
            }
            i += seg_len;
        }
        return false;
    }
    true
}

/// Decode and area-box downscale PNG/JPEG bytes (also used to keep static app chrome icons lean).
pub fn decode_and_downscale_image(bytes: &[u8], max_dim: usize) -> Option<ImageBuffer> {
    decode_and_downscale(bytes, max_dim)
}

/// Precompute the 4 enriched liquid-aurora pigments once on CPU from decoded cover art.
/// Matches `FerricAmbientStage`'s 9-tap quadrant sampling + `enrich_tone`, eliminating 36 GPU
/// texture fetches per pixel at 30 FPS with identical color output.
pub fn extract_aurora_zones(buffer: &ImageBuffer) -> [[f32; 3]; 4] {
    let w = buffer.width;
    let h = buffer.height;
    if w == 0 || h == 0 || buffer.data.len() < w * h {
        return [[0.0; 3]; 4];
    }

    let sample_uv = |u: f32, v: f32| -> [f32; 3] {
        let cu = u.clamp(0.04, 0.96);
        let cv = v.clamp(0.04, 0.96);
        let x = ((cu * (w as f32 - 1.0)).round() as usize).min(w - 1);
        let y = ((cv * (h as f32 - 1.0)).round() as usize).min(h - 1);
        let px = buffer.data[y * w + x];
        [
            ((px >> 16) & 0xFF) as f32 / 255.0,
            ((px >> 8) & 0xFF) as f32 / 255.0,
            (px & 0xFF) as f32 / 255.0,
        ]
    };

    let sample_zone = |cx: f32, cy: f32| -> [f32; 3] {
        let r = 0.14_f32;
        let taps: [(f32, f32, f32); 9] = [
            (0.0, 0.0, 0.24),
            (r, 0.0, 0.11),
            (-r, 0.0, 0.11),
            (0.0, r, 0.11),
            (0.0, -r, 0.11),
            (r * 0.72, r * 0.72, 0.08),
            (-r * 0.72, r * 0.72, 0.08),
            (r * 0.72, -r * 0.72, 0.08),
            (-r * 0.72, -r * 0.72, 0.08),
        ];
        let mut acc = [0.0_f32; 3];
        for (dx, dy, wt) in taps {
            let s = sample_uv(cx + dx, cy + dy);
            acc[0] += s[0] * wt;
            acc[1] += s[1] * wt;
            acc[2] += s[2] * wt;
        }
        // Match `enrich_tone` in `FerricAmbientStage`
        let luma = acc[0] * 0.299 + acc[1] * 0.587 + acc[2] * 0.114;
        let br = (luma + (acc[0] - luma) * 1.68).clamp(0.0, 1.0);
        let bg = (luma + (acc[1] - luma) * 1.68).clamp(0.0, 1.0);
        let bb = (luma + (acc[2] - luma) * 1.68).clamp(0.0, 1.0);
        let peak = br.max(bg).max(bb);
        let lift = (0.68 / peak.max(0.18)).clamp(0.85, 2.1);
        [
            (br * lift).clamp(0.0, 1.0),
            (bg * lift).clamp(0.0, 1.0),
            (bb * lift).clamp(0.0, 1.0),
        ]
    };

    [
        sample_zone(0.24, 0.24),
        sample_zone(0.76, 0.26),
        sample_zone(0.26, 0.76),
        sample_zone(0.76, 0.74),
    ]
}

fn decode_and_downscale(bytes: &[u8], max_dim: usize) -> Option<ImageBuffer> {
    if bytes.len() < 16 || !check_image_header_dimensions(bytes) {
        return None;
    }
    let decoded = if bytes[0] == 0x89 && bytes[1] == 0x50 && bytes[2] == 0x4E && bytes[3] == 0x47 {
        ImageBuffer::from_png(bytes).ok()?
    } else if bytes[0] == 0xFF && bytes[1] == 0xD8 {
        ImageBuffer::from_jpg(bytes).ok()?
    } else {
        ImageBuffer::from_jpg(bytes)
            .or_else(|_| ImageBuffer::from_png(bytes))
            .ok()?
    };
    if decoded.width == 0
        || decoded.height == 0
        || decoded
            .width
            .checked_mul(decoded.height)
            .is_none_or(|expected| decoded.data.len() < expected)
    {
        return None;
    }
    if decoded.width <= max_dim && decoded.height <= max_dim {
        return Some(decoded);
    }
    Some(downscale_area_box(&decoded, max_dim))
}

/// High-quality aspect-preserving area/box-filter downscaler over packed BGRA `u32` pixels.
/// Integrates every source pixel inside each destination pixel footprint with exact fractional
/// edge weights — eliminates aliasing and blocky stair-steps on high-contrast graphic covers.
fn downscale_area_box(src: &ImageBuffer, max_dim: usize) -> ImageBuffer {
    let sw = src.width.max(1);
    let sh = src.height.max(1);
    let (dw, dh) = if sw >= sh {
        let h = ((sh as f64 * max_dim as f64) / sw as f64).round().max(1.0) as usize;
        (max_dim, h.min(max_dim))
    } else {
        let w = ((sw as f64 * max_dim as f64) / sh as f64).round().max(1.0) as usize;
        (w.min(max_dim), max_dim)
    };

    let mut dst = vec![0u32; dw * dh];
    let scale_x = sw as f32 / dw as f32;
    let scale_y = sh as f32 / dh as f32;

    for y in 0..dh {
        let sy0 = y as f32 * scale_y;
        let sy1 = ((y + 1) as f32 * scale_y).min(sh as f32);
        let iy0 = (sy0.floor() as usize).min(sh - 1);
        let iy1 = ((sy1.ceil() as usize).max(iy0 + 1)).min(sh);
        let dst_row = y * dw;

        for x in 0..dw {
            let sx0 = x as f32 * scale_x;
            let sx1 = ((x + 1) as f32 * scale_x).min(sw as f32);
            let ix0 = (sx0.floor() as usize).min(sw - 1);
            let ix1 = ((sx1.ceil() as usize).max(ix0 + 1)).min(sw);

            let mut acc_b = 0.0_f32;
            let mut acc_g = 0.0_f32;
            let mut acc_r = 0.0_f32;
            let mut acc_a = 0.0_f32;
            let mut acc_w = 0.0_f32;

            for sy in iy0..iy1 {
                let wy = (sy1.min((sy + 1) as f32) - sy0.max(sy as f32)).max(0.0);
                if wy <= 0.0 {
                    continue;
                }
                let row_off = sy * sw;
                for sx in ix0..ix1 {
                    let wx = (sx1.min((sx + 1) as f32) - sx0.max(sx as f32)).max(0.0);
                    let w = wx * wy;
                    if w <= 0.0 {
                        continue;
                    }
                    let px = src.data[row_off + sx];
                    acc_b += (px & 0xFF) as f32 * w;
                    acc_g += ((px >> 8) & 0xFF) as f32 * w;
                    acc_r += ((px >> 16) & 0xFF) as f32 * w;
                    acc_a += ((px >> 24) & 0xFF) as f32 * w;
                    acc_w += w;
                }
            }

            let inv = if acc_w > 1e-6 { 1.0 / acc_w } else { 1.0 };
            let b = (acc_b * inv).round().clamp(0.0, 255.0) as u32;
            let g = (acc_g * inv).round().clamp(0.0, 255.0) as u32;
            let r = (acc_r * inv).round().clamp(0.0, 255.0) as u32;
            let a = (acc_a * inv).round().clamp(0.0, 255.0) as u32;
            dst[dst_row + x] = (a << 24) | (r << 16) | (g << 8) | b;
        }
    }

    ImageBuffer {
        width: dw,
        height: dh,
        data: dst,
        animation: None,
    }
}

/// Extract a faithful, WCAG-legible dominant accent RGB from decoded `VecBGRAu8_32` album artwork.
/// Uses 24 fine 15-degree hue bins with circular neighbor smoothing and area-balanced chroma weighting
/// so the true visual dominant color wins (never hijacked by tiny stickers or JPEG compression noise).
/// Truly monochrome / black-and-white / silver covers extract sleek neutral platinum-silver instead
/// of injecting an unrelated warm orange fallback.
pub fn extract_vibrant_accent_rgb(buffer: &ImageBuffer) -> [f32; 3] {
    const MONO_SILVER: [f32; 3] = [0.80, 0.82, 0.86];

    let len = buffer.data.len();
    if len == 0 {
        return MONO_SILVER;
    }

    let step = (len / 900).max(1);
    let mut total_samples: u32 = 0;
    let mut chromatic_samples: u32 = 0;
    let mut subtle_samples: u32 = 0;
    let mut sum_r = 0.0_f32;
    let mut sum_g = 0.0_f32;
    let mut sum_b = 0.0_f32;

    let mut vivid_score = [0.0_f32; 24];
    let mut vivid_r = [0.0_f32; 24];
    let mut vivid_g = [0.0_f32; 24];
    let mut vivid_b = [0.0_f32; 24];

    let mut subtle_score = [0.0_f32; 24];
    let mut subtle_r = [0.0_f32; 24];
    let mut subtle_g = [0.0_f32; 24];
    let mut subtle_b = [0.0_f32; 24];

    for &px in buffer.data.iter().step_by(step) {
        let r = ((px >> 16) & 0xFF) as f32 / 255.0;
        let g = ((px >> 8) & 0xFF) as f32 / 255.0;
        let b = (px & 0xFF) as f32 / 255.0;
        total_samples += 1;
        sum_r += r;
        sum_g += g;
        sum_b += b;

        let max_c = r.max(g).max(b);
        let min_c = r.min(g).min(b);
        let delta = max_c - min_c;
        let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        if delta < 0.035 || !(0.035..=0.965).contains(&lum) {
            continue;
        }
        let sat = delta / max_c.max(1e-4);
        if sat < 0.065 {
            continue;
        }

        let hue = if (max_c - r).abs() < 1e-4 {
            ((g - b) / delta).rem_euclid(6.0)
        } else if (max_c - g).abs() < 1e-4 {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        let bi = ((hue * 4.0).floor() as usize).min(23);
        let lum_weight = 0.40 + (1.0 - (lum - 0.48).abs() * 1.15).max(0.12);
        // Area-balanced chroma weighting: dominant cover regions beat tiny 2% neon stickers
        let w = (0.35 + 0.65 * sat) * lum_weight;

        subtle_samples += 1;
        subtle_score[bi] += w;
        subtle_r[bi] += r * w;
        subtle_g[bi] += g * w;
        subtle_b[bi] += b * w;

        if delta >= 0.075 && sat >= 0.14 {
            chromatic_samples += 1;
            vivid_score[bi] += w;
            vivid_r[bi] += r * w;
            vivid_g[bi] += g * w;
            vivid_b[bi] += b * w;
        }
    }

    if total_samples == 0 {
        return MONO_SILVER;
    }

    let pick_dominant =
        |score: &[f32; 24], br: &[f32; 24], bg: &[f32; 24], bb: &[f32; 24]| -> Option<[f32; 3]> {
            let mut best_i = 0usize;
            let mut best_smoothed = 0.0_f32;
            for i in 0..24 {
                let prev = (i + 23) % 24;
                let next = (i + 1) % 24;
                let smoothed = score[i] + 0.35 * (score[prev] + score[next]);
                if smoothed > best_smoothed {
                    best_smoothed = smoothed;
                    best_i = i;
                }
            }
            if best_smoothed < 0.08 {
                return None;
            }
            let prev = (best_i + 23) % 24;
            let next = (best_i + 1) % 24;
            let w_tot = score[best_i] + 0.35 * (score[prev] + score[next]);
            if w_tot <= 1e-4 {
                return None;
            }
            let r = (br[best_i] + 0.35 * (br[prev] + br[next])) / w_tot;
            let g = (bg[best_i] + 0.35 * (bg[prev] + bg[next])) / w_tot;
            let b = (bb[best_i] + 0.35 * (bb[prev] + bb[next])) / w_tot;
            Some([r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)])
        };

    let chroma_ratio = chromatic_samples as f32 / total_samples as f32;
    let subtle_ratio = subtle_samples as f32 / total_samples as f32;

    // Pass 1: Vivid or deep chromatic covers (>= 4% real colored area -> beats tiny 2% stickers)
    // Pass 2: Subtle/pastel covers (sage green, dusty rose, slate blue -> requires >= 18% tinted area)
    // Pass 3: Genuine monochrome / B&W / silver / noir covers -> refined neutral platinum/ivory silver
    let (mut r, mut g, mut b, boost_sat) = if chroma_ratio >= 0.04
        && let Some([r, g, b]) = pick_dominant(&vivid_score, &vivid_r, &vivid_g, &vivid_b)
    {
        (r, g, b, false)
    } else if subtle_ratio >= 0.18
        && let Some([r, g, b]) = pick_dominant(&subtle_score, &subtle_r, &subtle_g, &subtle_b)
    {
        (r, g, b, true)
    } else {
        // Authentic monochrome / grayscale cover: preserve subtle cool silver vs warm ivory tone
        let n = total_samples as f32;
        let avg_r = sum_r / n;
        let avg_g = sum_g / n;
        let avg_b = sum_b / n;
        let mean = (avg_r + avg_g + avg_b) / 3.0;
        let dr = (avg_r - mean).clamp(-0.025, 0.025);
        let dg = (avg_g - mean).clamp(-0.025, 0.025);
        let db = (avg_b - mean).clamp(-0.025, 0.025);
        return [
            (0.81 + dr).clamp(0.74, 0.88),
            (0.82 + dg).clamp(0.74, 0.88),
            (0.85 + db).clamp(0.74, 0.90),
        ];
    };

    // Lift brightness multiplicatively (preserving HSV hue & saturation faithfully without washing out!)
    let max_c = r.max(g).max(b).max(1e-4);
    let target_peak = 0.92_f32;
    if max_c < target_peak {
        let gain = (target_peak / max_c).min(6.0);
        r = (r * gain).clamp(0.0, 1.0);
        g = (g * gain).clamp(0.0, 1.0);
        b = (b * gain).clamp(0.0, 1.0);
    }

    // Gentle chromatic enrichment for muted/pastel covers while keeping authentic cover hue
    let peak = r.max(g).max(b).max(1e-4);
    let valley = r.min(g).min(b);
    let sat_now = (peak - valley) / peak;
    if boost_sat || sat_now < 0.42 {
        let avg = (r + g + b) / 3.0;
        let sat_boost = if boost_sat { 1.55 } else { 1.28 };
        r = (avg + (r - avg) * sat_boost).clamp(0.0, 1.0);
        g = (avg + (g - avg) * sat_boost).clamp(0.0, 1.0);
        b = (avg + (b - avg) * sat_boost).clamp(0.0, 1.0);
    }

    // Final luminance floor for WCAG AA on dark surfaces (e.g. pure deep indigo/crimson)
    let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if lum < 0.42 {
        let lift = ((0.44 - lum) / (1.0 - lum).max(0.1)).clamp(0.0, 0.45);
        r += (1.0 - r) * lift;
        g += (1.0 - g) * lift;
        b += (1.0 - b) * lift;
    }

    [
        r.clamp(0.10, 0.98),
        g.clamp(0.10, 0.98),
        b.clamp(0.10, 0.98),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_buffer(r: u8, g: u8, b: u8) -> ImageBuffer {
        let px = (0xFF_u32 << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
        ImageBuffer {
            width: 32,
            height: 32,
            data: vec![px; 32 * 32],
            animation: None,
        }
    }

    #[test]
    fn adaptive_accent_extracts_deep_ocean_blue_and_muted_sage_green() {
        let ocean = extract_vibrant_accent_rgb(&solid_buffer(14, 38, 58));
        assert!(
            ocean[2] > ocean[0] + 0.12 && ocean[2] > 0.55,
            "deep ocean cover should extract blue accent, got {ocean:?}"
        );

        let sage = extract_vibrant_accent_rgb(&solid_buffer(108, 128, 100));
        assert!(
            sage[1] > sage[0] + 0.05 && sage[1] > sage[2] + 0.05,
            "muted sage cover should extract green accent, got {sage:?}"
        );

        let mono = extract_vibrant_accent_rgb(&solid_buffer(120, 120, 120));
        assert!(
            (mono[0] - mono[1]).abs() < 0.04 && (mono[1] - mono[2]).abs() < 0.05,
            "pure monochrome B&W cover should extract sleek neutral silver, not orange, got {mono:?}"
        );

        // Monochrome cover with a tiny 2% orange badge must remain monochrome silver, not orange
        let mut mono_with_badge = solid_buffer(96, 96, 96);
        for px in mono_with_badge.data.iter_mut().take(20) {
            *px = 0xFF_E86A2C;
        }
        let mono_badge_accent = extract_vibrant_accent_rgb(&mono_with_badge);
        assert!(
            (mono_badge_accent[0] - mono_badge_accent[1]).abs() < 0.05,
            "tiny 2% sticker on monochrome cover must not hijack dominant silver accent, got {mono_badge_accent:?}"
        );
    }

    #[test]
    fn area_box_downscale_averages_checkerboard_smoothly_without_aliasing() {
        let mut data = Vec::with_capacity(64 * 64);
        for y in 0..64 {
            for x in 0..64 {
                let px = if (x + y) % 2 == 0 {
                    0xFFFF_FFFF
                } else {
                    0xFF00_0000
                };
                data.push(px);
            }
        }
        let src = ImageBuffer {
            width: 64,
            height: 64,
            data,
            animation: None,
        };
        let out = downscale_area_box(&src, 16);
        assert_eq!(out.width, 16);
        assert_eq!(out.height, 16);
        // Every 4x4 block has 8 black + 8 white pixels -> exact mid-gray (~128), zero checkerboard aliasing.
        for px in &out.data {
            let r = (*px >> 16) & 0xFF;
            assert!(
                (126..=130).contains(&r),
                "expected smooth mid-gray, got {r}"
            );
        }
    }
}
