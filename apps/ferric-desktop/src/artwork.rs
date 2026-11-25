use ferric_core::app_data_dir;
use ferric_core::models::Track;
use ferric_providers::local_scanner::read_embedded_cover_bytes;
use ferric_providers::ytmusic::{download_artwork_bytes, normalize_ytm_thumbnail_url};
use makepad_widgets::image_cache::ImageBuffer;
use makepad_widgets::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

/// Two worker threads balance fast artwork loading with minimal thread-stack/TLS RAM overhead.
const WORKER_THREADS: usize = 2;
/// Hard LRU cap: 28 covers x 128x128 BGRA (64 KB) = ~1.8 MB CPU + ~1.8 MB VRAM max!
const MAX_CACHED_TEXTURES: usize = 28;
/// Standard cover art thumbnail size (crisp for 40..160 px cards, 56x smaller than 1280x720).
const THUMB_MAX_DIM: usize = 128;
/// Higher detail size reserved solely for the currently playing hero/fullscreen artwork (256 KB).
const HERO_MAX_DIM: usize = 256;

#[derive(Clone)]
pub struct CachedArtwork {
    pub texture: Texture,
    pub aspect: f32,
    pub accent: Vec4,
    pub high_res: bool,
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
        high_res: bool,
    },
    Failed {
        key: String,
    },
}

/// Memory-bounded LRU album cover fetcher, disk cache, downscaling decoder,
/// dominant WCAG-safe accent extractor, and GPU texture registry.
pub struct ArtworkCache {
    entries: HashMap<String, CachedArtwork>,
    in_flight: HashSet<String>,
    failed: HashSet<String>,
    pinned_key: Option<String>,
    clock: u64,
    job_tx: mpsc::Sender<ArtworkJob>,
    done_rx: mpsc::Receiver<ArtworkDone>,
}

impl ArtworkCache {
    pub fn new() -> Self {
        let (job_tx, job_rx) = mpsc::channel::<ArtworkJob>();
        let (done_tx, done_rx) = mpsc::channel::<ArtworkDone>();
        let shared_rx = Arc::new(Mutex::new(job_rx));
        let cache_dir = app_data_dir().join("artwork_cache");

        for _ in 0..WORKER_THREADS {
            let rx = Arc::clone(&shared_rx);
            let tx = done_tx.clone();
            let dir = cache_dir.clone();
            thread::spawn(move || {
                loop {
                    let job = {
                        let Ok(guard) = rx.lock() else {
                            break;
                        };
                        match guard.recv() {
                            Ok(j) => j,
                            Err(_) => break,
                        }
                    };

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

                    let decoded = raw_bytes.and_then(|bytes| decode_and_downscale(&bytes, max_dim));
                    match decoded {
                        Some(buffer) if buffer.width > 0 && buffer.height > 0 => {
                            let accent_rgb = extract_vibrant_accent_rgb(&buffer);
                            let _ = tx.send(ArtworkDone::Loaded {
                                key,
                                buffer,
                                accent_rgb,
                                high_res: max_dim >= HERO_MAX_DIM,
                            });
                        }
                        _ => {
                            let _ = tx.send(ArtworkDone::Failed { key });
                        }
                    }
                }
            });
        }

        Self {
            entries: HashMap::with_capacity(MAX_CACHED_TEXTURES + 4),
            in_flight: HashSet::new(),
            failed: HashSet::new(),
            pinned_key: None,
            clock: 0,
            job_tx,
            done_rx,
        }
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
        if let Some(path) = track.path.as_ref().filter(|p| p.is_file()) {
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
        let norm = normalize_ytm_thumbnail_url(raw_url);
        if norm.is_empty() {
            return None;
        }
        self.clock = self.clock.wrapping_add(1);
        let want_hi = max_dim >= HERO_MAX_DIM;
        if let Some(existing) = self.entries.get_mut(&norm) {
            existing.last_used = self.clock;
            if !want_hi || existing.high_res {
                return Some(norm);
            }
        }
        if !self.in_flight.contains(&norm) && !self.failed.contains(&norm) {
            self.in_flight.insert(norm.clone());
            let _ = self.job_tx.send(ArtworkJob::Url {
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
        let key = Self::key_for_track(track)?;
        self.clock = self.clock.wrapping_add(1);
        let want_hi = max_dim >= HERO_MAX_DIM;
        if let Some(existing) = self.entries.get_mut(&key) {
            existing.last_used = self.clock;
            if !want_hi || existing.high_res {
                return Some(key);
            }
        }
        if self.in_flight.contains(&key) || self.failed.contains(&key) {
            return Some(key);
        }
        self.in_flight.insert(key.clone());
        if key.starts_with("http://") || key.starts_with("https://") {
            let _ = self.job_tx.send(ArtworkJob::Url {
                key: key.clone(),
                url: key.clone(),
                max_dim,
            });
        } else if let Some(path) = track.path.clone() {
            let _ = self.job_tx.send(ArtworkJob::LocalFile {
                key: key.clone(),
                path,
                max_dim,
            });
        }
        Some(key)
    }

    pub fn poll(&mut self, cx: &mut Cx) -> bool {
        let mut changed = false;
        for _ in 0..12 {
            let Ok(msg) = self.done_rx.try_recv() else {
                break;
            };
            match msg {
                ArtworkDone::Loaded {
                    key,
                    buffer,
                    accent_rgb,
                    high_res,
                } => {
                    self.in_flight.remove(&key);
                    self.clock = self.clock.wrapping_add(1);
                    let aspect =
                        (buffer.width as f32 / buffer.height.max(1) as f32).clamp(0.25, 4.0);
                    let texture = buffer.into_new_texture(cx);
                    let accent = vec4(accent_rgb[0], accent_rgb[1], accent_rgb[2], 1.0);
                    self.entries.insert(
                        key,
                        CachedArtwork {
                            texture,
                            aspect,
                            accent,
                            high_res,
                            last_used: self.clock,
                        },
                    );
                    self.evict_lru_if_needed();
                    changed = true;
                }
                ArtworkDone::Failed { key } => {
                    self.in_flight.remove(&key);
                    self.failed.insert(key);
                }
            }
        }
        changed
    }

    fn evict_lru_if_needed(&mut self) {
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
        }
    }

    /// Release all non-active cover textures when minimized / hidden in System Tray to keep RAM minimal.
    pub fn trim_non_active(&mut self) {
        if let Some(pin) = self.pinned_key.clone() {
            self.entries.retain(|k, _| k == &pin);
        } else {
            self.entries.clear();
        }
        self.entries.shrink_to(8);
    }

    pub fn get(&self, key: &str) -> Option<&CachedArtwork> {
        self.entries.get(key)
    }

    pub fn get_for_track(&self, track: &Track) -> Option<&CachedArtwork> {
        let key = Self::key_for_track(track)?;
        self.entries.get(&key)
    }

    pub fn snapshot(&self) -> HashMap<String, CachedArtwork> {
        self.entries.clone()
    }

    pub fn clear_memory(&mut self) {
        self.entries.clear();
        self.entries.shrink_to_fit();
        self.failed.clear();
    }
}

fn decode_and_downscale(bytes: &[u8], max_dim: usize) -> Option<ImageBuffer> {
    if bytes.len() < 16 {
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
    if decoded.width == 0 || decoded.height == 0 {
        return None;
    }
    if decoded.width <= max_dim && decoded.height <= max_dim {
        return Some(decoded);
    }
    Some(downscale_bilinear(&decoded, max_dim))
}

/// Fast aspect-preserving bilinear downscaler over packed BGRA `u32` pixels.
/// Shrinks 1280x720 (3.52 MB) down to 128x72 (36 KB) or 128x128 (64 KB) before GPU upload.
fn downscale_bilinear(src: &ImageBuffer, max_dim: usize) -> ImageBuffer {
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
        let fy = ((y as f32 + 0.5) * scale_y - 0.5).clamp(0.0, (sh - 1) as f32);
        let y0 = fy.floor() as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let wy = fy - y0 as f32;
        let wy0 = 1.0 - wy;

        let row0 = y0 * sw;
        let row1 = y1 * sw;
        let dst_row = y * dw;

        for x in 0..dw {
            let fx = ((x as f32 + 0.5) * scale_x - 0.5).clamp(0.0, (sw - 1) as f32);
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let wx = fx - x0 as f32;
            let wx0 = 1.0 - wx;

            let p00 = src.data[row0 + x0];
            let p10 = src.data[row0 + x1];
            let p01 = src.data[row1 + x0];
            let p11 = src.data[row1 + x1];

            let w00 = wx0 * wy0;
            let w10 = wx * wy0;
            let w01 = wx0 * wy;
            let w11 = wx * wy;

            let blend = |shift: u32| -> u32 {
                let c00 = ((p00 >> shift) & 0xFF) as f32;
                let c10 = ((p10 >> shift) & 0xFF) as f32;
                let c01 = ((p01 >> shift) & 0xFF) as f32;
                let c11 = ((p11 >> shift) & 0xFF) as f32;
                (c00 * w00 + c10 * w10 + c01 * w01 + c11 * w11)
                    .round()
                    .clamp(0.0, 255.0) as u32
            };

            let b = blend(0);
            let g = blend(8);
            let r = blend(16);
            let a = blend(24);
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

/// Extract a refined, WCAG-legible vibrant accent RGB from decoded `VecBGRAu8_32` album artwork.
pub fn extract_vibrant_accent_rgb(buffer: &ImageBuffer) -> [f32; 3] {
    const FALLBACK: [f32; 3] = [0.659, 0.510, 1.0]; // Soft violet (#A882FF)

    let len = buffer.data.len();
    if len == 0 {
        return FALLBACK;
    }

    let mut bucket_score = [0.0_f32; 12];
    let mut bucket_r = [0.0_f32; 12];
    let mut bucket_g = [0.0_f32; 12];
    let mut bucket_b = [0.0_f32; 12];

    let step = (len / 256).max(1);
    for &px in buffer.data.iter().step_by(step) {
        let r = ((px >> 16) & 0xFF) as f32 / 255.0;
        let g = ((px >> 8) & 0xFF) as f32 / 255.0;
        let b = (px & 0xFF) as f32 / 255.0;
        let max_c = r.max(g).max(b);
        let min_c = r.min(g).min(b);
        let delta = max_c - min_c;
        let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        if delta < 0.14 || !(0.12..=0.90).contains(&lum) {
            continue;
        }
        let sat = delta / max_c.max(1e-4);
        if sat < 0.22 {
            continue;
        }
        let hue = if (max_c - r).abs() < 1e-4 {
            ((g - b) / delta).rem_euclid(6.0)
        } else if (max_c - g).abs() < 1e-4 {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        let bi = ((hue * 2.0).floor() as usize).min(11);
        let w = sat * sat * (1.0 - (lum - 0.52).abs() * 1.4).max(0.15);
        bucket_score[bi] += w;
        bucket_r[bi] += r * w;
        bucket_g[bi] += g * w;
        bucket_b[bi] += b * w;
    }

    let Some((best_i, &best_w)) = bucket_score
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
    else {
        return FALLBACK;
    };
    if best_w < 0.15 {
        return FALLBACK;
    }

    let mut r = (bucket_r[best_i] / best_w).clamp(0.0, 1.0);
    let mut g = (bucket_g[best_i] / best_w).clamp(0.0, 1.0);
    let mut b = (bucket_b[best_i] / best_w).clamp(0.0, 1.0);

    // Ensure WCAG AA+ legibility (>= 5:1 contrast) on pure AMOLED #000000 canvas.
    let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if lum < 0.44 {
        let lift = ((0.48 - lum) / (1.0 - lum).max(0.1)).clamp(0.0, 0.68);
        r += (1.0 - r) * lift;
        g += (1.0 - g) * lift;
        b += (1.0 - b) * lift;
    }
    let avg = (r + g + b) / 3.0;
    let desat = 0.14;
    r = (r * (1.0 - desat) + avg * desat).clamp(0.20, 0.96);
    g = (g * (1.0 - desat) + avg * desat).clamp(0.20, 0.96);
    b = (b * (1.0 - desat) + avg * desat).clamp(0.20, 0.96);

    [r, g, b]
}
