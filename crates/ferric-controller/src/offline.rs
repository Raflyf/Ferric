//! Persistent offline track & playlist downloads isolated under `data_dir/offline_downloads/`
//! plus storage diagnostics and cache cleaners.

use crate::types::StorageStatsSnapshot;
use crate::{ControllerError, ControllerResult, FerricController, unix_now_secs};
use ferric_core::models::{OfflineDownloadRecord, Track};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

const OFFLINE_DIR_NAME: &str = "offline_downloads";

fn offline_dir_for(data_dir: &Path) -> PathBuf {
    data_dir.join(OFFLINE_DIR_NAME)
}

fn dir_size_and_count(root: &Path) -> (u64, usize) {
    let mut bytes = 0u64;
    let mut count = 0usize;
    let Ok(entries) = std::fs::read_dir(root) else {
        return (0, 0);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Ok(meta) = entry.metadata() {
                bytes = bytes.saturating_add(meta.len());
                count = count.saturating_add(1);
            }
        } else if path.is_dir() {
            let (sub_b, sub_c) = dir_size_and_count(&path);
            bytes = bytes.saturating_add(sub_b);
            count = count.saturating_add(sub_c);
        }
    }
    (bytes, count)
}

fn clear_directory_contents(root: &Path) -> u64 {
    let (before_bytes, _) = dir_size_and_count(root);
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            let _ = std::fs::remove_dir_all(&p);
        } else {
            let _ = std::fs::remove_file(&p);
        }
    }
    before_bytes
}

fn safe_offline_filename(track_id: &str, codec: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(track_id.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(32);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &b in &digest[..16] {
        hex.push(char::from(HEX[usize::from(b >> 4)]));
        hex.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    let ext = match codec.trim().to_ascii_lowercase().as_str() {
        "flac" => "flac",
        "mp3" => "mp3",
        "wav" => "wav",
        "ogg" | "vorbis" => "ogg",
        "opus" | "webm" => "webm",
        "aac" | "m4a" | "alac" | "mp4" => "m4a",
        _ => "audio",
    };
    format!("{hex}.{ext}")
}

fn is_path_inside_dir(candidate: &Path, parent_dir: &Path) -> bool {
    let Ok(canon_parent) = std::fs::canonicalize(parent_dir) else {
        return false;
    };
    let Ok(canon_cand) = std::fs::canonicalize(candidate) else {
        return false;
    };
    canon_cand.starts_with(&canon_parent)
}

impl FerricController {
    /// Download a single track into persistent `data_dir/offline_downloads/` off the UI thread.
    pub fn download_track(self: &Arc<Self>, track: Track) {
        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-offline-dl".to_string())
            .spawn(move || match this.download_track_blocking(&track) {
                Ok(size) => {
                    this.sink.on_library_changed();
                    this.sink.on_toast(format!(
                        "Downloaded '{}' ({:.1} MB)",
                        track.title,
                        size as f64 / (1024.0 * 1024.0)
                    ));
                }
                Err(err) => {
                    this.sink
                        .on_toast(format!("Offline download failed: {err}"));
                }
            });
    }

    /// Download all online/uncached tracks of a playlist sequentially on a single background worker.
    pub fn download_playlist(self: &Arc<Self>, playlist_id: String) -> ControllerResult<()> {
        let tracks = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db.list_playlist_tracks(&playlist_id)?
        };
        if tracks.is_empty() {
            return Err(ControllerError::State("Playlist has no tracks".to_string()));
        }
        self.download_tracks(tracks);
        Ok(())
    }

    /// Download any batch of tracks (Album, Artist, Liked, Station, or Playlist) sequentially
    /// into isolated persistent `data_dir/offline_downloads/`.
    pub fn download_tracks(self: &Arc<Self>, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-batch-dl".to_string())
            .spawn(move || {
                let total = tracks.len();
                let mut ok_count = 0usize;
                for track in tracks {
                    let already = {
                        let st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                        st.db
                            .get_offline_download(&track.id)
                            .ok()
                            .flatten()
                            .is_some_and(|r| Path::new(&r.local_path).is_file())
                    };
                    if already {
                        ok_count += 1;
                        continue;
                    }
                    if this.download_track_blocking(&track).is_ok() {
                        ok_count += 1;
                    }
                }
                this.sink.on_library_changed();
                this.sink.on_toast(format!(
                    "Offline ready: {ok_count}/{total} songs downloaded"
                ));
            });
    }

    pub(crate) fn download_track_blocking(&self, track: &Track) -> Result<u64, String> {
        self.sink.on_download_progress(track.id.clone(), 5);
        let offline_dir = offline_dir_for(&self.cfg.data_dir);
        std::fs::create_dir_all(&offline_dir).map_err(|e| e.to_string())?;

        // 1. Check if already on disk as a regular local file or already cached in stream_cache.
        let (src_path, codec) = if let Some(p) = track.path.as_ref().filter(|p| p.is_file()) {
            (p.clone(), track.codec.clone())
        } else {
            self.sink.on_download_progress(track.id.clone(), 20);
            let (cached_path, _vid, resolved_codec, _kbps, _dur) =
                self.resolve_stream_path_blocking(track)?;
            (cached_path, resolved_codec)
        };

        self.sink.on_download_progress(track.id.clone(), 75);
        let dest_name = safe_offline_filename(&track.id, &codec);
        let dest_path = offline_dir.join(&dest_name);
        let nonce = fastrand::u64(..);
        let tmp_path = offline_dir.join(format!("{dest_name}.{nonce:016x}.part"));

        if let Err(err) = std::fs::copy(&src_path, &tmp_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(err.to_string());
        }
        let size_bytes = std::fs::metadata(&tmp_path).map(|m| m.len()).unwrap_or(0);
        if dest_path.exists() {
            let _ = std::fs::remove_file(&dest_path);
        }
        if let Err(err) = std::fs::rename(&tmp_path, &dest_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(err.to_string());
        }

        let quality_label = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.dsp.audio_quality.label().to_string()
        };

        let rec = OfflineDownloadRecord {
            track_id: track.id.clone(),
            local_path: dest_path.to_string_lossy().into_owned(),
            size_bytes,
            codec,
            quality: quality_label,
            downloaded_at: unix_now_secs(),
        };

        let db_res = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let _ = st.db.upsert_tracks(std::slice::from_ref(track));
            st.db.upsert_offline_download(&rec)
        };
        if let Err(err) = db_res {
            let _ = std::fs::remove_file(&dest_path);
            return Err(err.to_string());
        }

        self.sink.on_download_progress(track.id.clone(), 100);
        Ok(size_bytes)
    }

    pub fn list_offline_track_ids(&self) -> ControllerResult<Vec<String>> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let rows = st.db.list_offline_downloads()?;
        Ok(rows
            .into_iter()
            .filter(|r| Path::new(&r.local_path).is_file())
            .map(|r| r.track_id)
            .collect())
    }

    pub fn list_offline_tracks(&self) -> ControllerResult<Vec<Track>> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let rows = st.db.list_offline_tracks_joined()?;
        Ok(rows
            .into_iter()
            .filter(|t| t.path.as_ref().is_some_and(|p| p.is_file()))
            .collect())
    }

    pub fn delete_offline_track(&self, track_id: &str) -> ControllerResult<()> {
        let offline_dir = offline_dir_for(&self.cfg.data_dir);
        let existing = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let rec = st.db.get_offline_download(track_id)?;
            st.db.delete_offline_download(track_id)?;
            rec
        };
        if let Some(rec) = existing {
            let candidate = PathBuf::from(&rec.local_path);
            if candidate.is_file() && is_path_inside_dir(&candidate, &offline_dir) {
                let _ = std::fs::remove_file(candidate);
            }
        }
        self.sink.on_library_changed();
        Ok(())
    }

    pub fn delete_all_offline_downloads(&self) -> ControllerResult<usize> {
        let offline_dir = offline_dir_for(&self.cfg.data_dir);
        let removed = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db.clear_offline_downloads()?
        };
        let _ = clear_directory_contents(&offline_dir);
        self.sink.on_library_changed();
        self.sink
            .on_toast(format!("Removed {removed} offline download(s)"));
        Ok(removed)
    }

    pub fn get_storage_stats(&self) -> ControllerResult<StorageStatsSnapshot> {
        let (offline_count, offline_bytes) = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let rows = st.db.list_offline_downloads()?;
            let valid: Vec<_> = rows
                .into_iter()
                .filter(|r| Path::new(&r.local_path).is_file())
                .collect();
            let sum: u64 = valid.iter().map(|r| r.size_bytes).sum();
            (valid.len(), sum)
        };

        let (stream_cache_bytes, stream_cache_files) =
            dir_size_and_count(&self.cfg.cache_dir.join("stream_cache"));
        let (artwork_cache_bytes, _) =
            dir_size_and_count(&self.cfg.cache_dir.join("artwork_cache"));
        let db_bytes = std::fs::metadata(self.cfg.data_dir.join("ferric.db"))
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(StorageStatsSnapshot {
            offline_track_count: offline_count,
            offline_bytes,
            stream_cache_bytes,
            stream_cache_files,
            artwork_cache_bytes,
            database_bytes: db_bytes,
        })
    }

    pub fn clear_audio_cache(&self) -> u64 {
        let freed = clear_directory_contents(&self.cfg.cache_dir.join("stream_cache"));
        self.sink.on_toast(format!(
            "Cleared audio cache ({:.1} MB freed)",
            freed as f64 / (1024.0 * 1024.0)
        ));
        freed
    }

    pub fn clear_artwork_cache(&self) -> u64 {
        let freed = clear_directory_contents(&self.cfg.cache_dir.join("artwork_cache"));
        self.sink.on_toast(format!(
            "Cleared artwork cache ({:.1} MB freed)",
            freed as f64 / (1024.0 * 1024.0)
        ));
        freed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ControllerConfig;
    use crate::events::NoopEventSink;
    use ferric_sync::DeviceKind;

    #[test]
    fn offline_download_survives_cache_clear_and_cleans_up_safely() {
        let root = std::env::temp_dir().join(format!(
            "ferric_m5_offline_{}_{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        let cfg = ControllerConfig {
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            device_id: "m5_test".to_string(),
            device_name: "M5 Test".to_string(),
            device_kind: DeviceKind::Phone,
            enable_connect: false,
            headless_audio: true,
        };
        let ctrl = FerricController::new(cfg, Arc::new(NoopEventSink)).expect("ctrl");
        ctrl.ensure_demo_seed().expect("seed");
        let songs = ctrl.list_songs("").expect("songs");
        let first = songs[0].clone();

        // Write a dummy file in stream_cache to verify clear_audio_cache touches ONLY cacheDir
        let stream_cache = root.join("cache").join("stream_cache");
        std::fs::create_dir_all(&stream_cache).expect("mkdir");
        std::fs::write(stream_cache.join("temp_stream.webm"), vec![7u8; 4096]).expect("write");

        let size = ctrl
            .download_track_blocking(&first)
            .expect("download local demo track");
        assert!(size > 0);
        assert_eq!(
            ctrl.list_offline_track_ids().expect("ids"),
            vec![first.id.clone()]
        );
        assert_eq!(ctrl.list_offline_tracks().expect("tracks").len(), 1);

        let stats_before = ctrl.get_storage_stats().expect("stats before");
        assert_eq!(stats_before.offline_track_count, 1);
        assert_eq!(stats_before.offline_bytes, size);
        assert!(stats_before.stream_cache_bytes >= 4096);

        // Clearing stream cache must NOT affect offline_downloads!
        let freed = ctrl.clear_audio_cache();
        assert!(freed >= 4096);
        assert_eq!(ctrl.list_offline_tracks().expect("still offline").len(), 1);

        ctrl.delete_offline_track(&first.id).expect("delete one");
        assert!(ctrl.list_offline_tracks().expect("empty").is_empty());

        ctrl.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }
}
