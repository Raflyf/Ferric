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

const WORKER_THREADS: usize = 4;

#[derive(Clone)]
pub struct CachedArtwork {
    pub texture: Texture,
    pub aspect: f32,
    pub accent: Vec4,
}

enum ArtworkJob {
    Url { key: String, url: String },
    LocalFile { key: String, path: PathBuf },
}

enum ArtworkDone {
    Loaded {
        key: String,
        buffer: ImageBuffer,
        accent_rgb: [f32; 3],
    },
    Failed {
        key: String,
    },
}

/// Asynchronous multi-worker album cover fetcher, disk cache, JPEG/PNG decoder,
/// dominant vibrant accent extractor, and GPU texture registry.
pub struct ArtworkCache {
    entries: HashMap<String, CachedArtwork>,
    in_flight: HashSet<String>,
    failed: HashSet<String>,
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

                    let (key, raw_bytes) = match job {
                        ArtworkJob::Url { key, url } => {
                            let bytes = download_artwork_bytes(&url, &dir).ok();
                            (key, bytes)
                        }
                        ArtworkJob::LocalFile { key, path } => {
                            let bytes = read_embedded_cover_bytes(&path);
                            (key, bytes)
                        }
                    };

                    let decoded = raw_bytes.and_then(|bytes| decode_image_buffer(&bytes));
                    match decoded {
                        Some(buffer) if buffer.width > 0 && buffer.height > 0 => {
                            let accent_rgb = extract_vibrant_accent_rgb(&buffer);
                            let _ = tx.send(ArtworkDone::Loaded {
                                key,
                                buffer,
                                accent_rgb,
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
            entries: HashMap::new(),
            in_flight: HashSet::new(),
            failed: HashSet::new(),
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

    pub fn request_url(&mut self, raw_url: &str) -> Option<String> {
        let norm = normalize_ytm_thumbnail_url(raw_url);
        if norm.is_empty() {
            return None;
        }
        if !self.entries.contains_key(&norm)
            && !self.in_flight.contains(&norm)
            && !self.failed.contains(&norm)
        {
            self.in_flight.insert(norm.clone());
            let _ = self.job_tx.send(ArtworkJob::Url {
                key: norm.clone(),
                url: norm.clone(),
            });
        }
        Some(norm)
    }

    pub fn request_for_track(&mut self, track: &Track) -> Option<String> {
        let key = Self::key_for_track(track)?;
        if self.entries.contains_key(&key)
            || self.in_flight.contains(&key)
            || self.failed.contains(&key)
        {
            return Some(key);
        }
        self.in_flight.insert(key.clone());
        if key.starts_with("http://") || key.starts_with("https://") {
            let _ = self.job_tx.send(ArtworkJob::Url {
                key: key.clone(),
                url: key.clone(),
            });
        } else if let Some(path) = track.path.clone() {
            let _ = self.job_tx.send(ArtworkJob::LocalFile {
                key: key.clone(),
                path,
            });
        }
        Some(key)
    }

    pub fn poll(&mut self, cx: &mut Cx) -> bool {
        let mut changed = false;
        for _ in 0..16 {
            let Ok(msg) = self.done_rx.try_recv() else {
                break;
            };
            match msg {
                ArtworkDone::Loaded {
                    key,
                    buffer,
                    accent_rgb,
                } => {
                    self.in_flight.remove(&key);
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
                        },
                    );
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
        self.failed.clear();
    }
}

fn decode_image_buffer(bytes: &[u8]) -> Option<ImageBuffer> {
    if bytes.len() < 16 {
        return None;
    }
    if bytes[0] == 0x89 && bytes[1] == 0x50 && bytes[2] == 0x4E && bytes[3] == 0x47 {
        ImageBuffer::from_png(bytes).ok()
    } else if bytes[0] == 0xFF && bytes[1] == 0xD8 {
        ImageBuffer::from_jpg(bytes).ok()
    } else {
        ImageBuffer::from_jpg(bytes)
            .or_else(|_| ImageBuffer::from_png(bytes))
            .ok()
    }
}

/// Extract a refined, WCAG-legible vibrant accent RGB from decoded `VecBGRAu8_32` album artwork.
pub fn extract_vibrant_accent_rgb(buffer: &ImageBuffer) -> [f32; 3] {
    const FALLBACK: [f32; 3] = [0.659, 0.510, 1.0]; // Sonora Midnight soft violet (#A882FF)

    let len = buffer.data.len();
    if len == 0 {
        return FALLBACK;
    }

    // 12 hue buckets weighted by saturation & mid-luminance so album cover accents look cohesive
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

    // Lift dark accents so they stay WCAG-legible (>= 5:1 contrast) on #0E0F13 obsidian canvas,
    // and gently tame hyper-saturated neon reds/greens for refined Sonora taste.
    let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if lum < 0.42 {
        let lift = ((0.46 - lum) / (1.0 - lum).max(0.1)).clamp(0.0, 0.65);
        r += (1.0 - r) * lift;
        g += (1.0 - g) * lift;
        b += (1.0 - b) * lift;
    }
    let avg = (r + g + b) / 3.0;
    let desat = 0.14;
    r = (r * (1.0 - desat) + avg * desat).clamp(0.18, 0.96);
    g = (g * (1.0 - desat) + avg * desat).clamp(0.18, 0.96);
    b = (b * (1.0 - desat) + avg * desat).clamp(0.18, 0.96);

    [r, g, b]
}
