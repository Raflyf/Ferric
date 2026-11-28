//! M3 Synced Lyrics (LRCLIB / Lyrics.ovh / Local `.lrc` / Embedded `content://` tags),
//! Official Music Video stream resolution, 10-band Parametric EQ + AutoEq + APO, and Theme Studio JSON.

use crate::accounts::active_ytm_session;
use crate::types::{
    AutoEqProfileSnapshot, LyricLineSnapshot, LyricsSnapshot, VideoStreamOptionSnapshot,
    VideoStreamsSnapshot,
};
use crate::{ControllerError, ControllerResult, FerricController};
use ferric_core::eq::EqConfig;
use ferric_core::models::Track;
use ferric_core::theme::ThemeConfig;
use ferric_providers::autoeq::{AUTOEQ_PROFILES, search_autoeq_profiles};
use ferric_providers::lyrics::{SyncedLyrics, fetch_best_free_lyrics};
use ferric_providers::ytmusic::resolve_music_video_streams;
use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use lofty::tag::ItemKey;
use std::io::BufReader;
use std::sync::Arc;
use std::thread;

pub const EQ_PRESET_NAMES: &[&str] = &[
    "Flat",
    "Warm Copper",
    "Deep Bass",
    "Bass Reducer",
    "Treble Boost",
    "Studio Ref",
    "Vocal Clarity",
    "Rock",
    "Pop",
    "Jazz",
    "Classical",
    "Electronic",
    "Acoustic",
    "Loudness",
];

fn read_embedded_lyrics_via_content_opener(uri: &str) -> Option<String> {
    let file = ferric_audio::content::open_audio_file(std::path::Path::new(uri)).ok()?;
    let opts = lofty::config::ParseOptions::new()
        .read_cover_art(false)
        .read_properties(false);
    let tagged = Probe::new(BufReader::new(file))
        .guess_file_type()
        .ok()?
        .options(opts)
        .read()
        .ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    tag.get_string(ItemKey::Lyrics)
        .or_else(|| tag.get_string(ItemKey::UnsyncLyrics))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn lyrics_cache_key(track: &Track) -> String {
    let artist = track.artist.trim().to_ascii_lowercase();
    let title = track.title.trim().to_ascii_lowercase();
    if track.duration_ms > 0 {
        let dur_bucket = track.duration_ms / 5_000;
        format!("{artist}|{title}|d{dur_bucket}")
    } else {
        format!("{artist}|{title}")
    }
}

fn to_lyrics_snapshot(track_id: String, parsed: SyncedLyrics, offset_ms: i64) -> LyricsSnapshot {
    LyricsSnapshot {
        track_id,
        lines: parsed
            .lines
            .into_iter()
            .map(|l| LyricLineSnapshot {
                timestamp_ms: l.timestamp_ms,
                text: l.text,
            })
            .collect(),
        source_name: parsed.source_name,
        is_synced: parsed.is_synced,
        offset_ms,
        is_loading: false,
    }
}

impl FerricController {
    /// Load synced or plain lyrics for `track` (SQLite cache -> Android `content://` embedded tag ->
    /// local `.lrc`/embedded -> LRCLIB -> Lyrics.ovh) off the main thread.
    pub fn load_lyrics(self: &Arc<Self>, track: Track) {
        let (gen_id, cached_lrc, persisted_offset_ms) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.lyrics_gen = st.lyrics_gen.wrapping_add(1);
            let persisted = st
                .db
                .get_track_lyrics_offset(&track.id)
                .unwrap_or(0)
                .clamp(-10_000, 10_000);
            st.lyrics_offset_ms = persisted;
            let key = lyrics_cache_key(&track);
            let cached = st.db.get_cached_lyrics(&key).ok().flatten();
            (st.lyrics_gen, cached, persisted)
        };

        if let Some(raw) = cached_lrc {
            let parsed = SyncedLyrics::parse_with_context(&raw, track.duration_ms, "Cached");
            if !parsed.lines.is_empty() {
                let snap = to_lyrics_snapshot(track.id, parsed, persisted_offset_ms);
                {
                    let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    st.lyrics_cache = snap.clone();
                }
                self.sink.on_lyrics_loaded(snap);
                return;
            }
        }

        let loading = LyricsSnapshot {
            track_id: track.id.clone(),
            lines: Vec::new(),
            source_name: String::new(),
            is_synced: false,
            offset_ms: persisted_offset_ms,
            is_loading: true,
        };
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.lyrics_cache = loading.clone();
        }
        self.sink.on_lyrics_loaded(loading);

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-lyrics".to_string())
            .spawn(move || {
                // Check Android Scoped Storage `content://` embedded lyrics first.
                if let Some(raw_uri) = track
                    .path
                    .as_ref()
                    .and_then(|p| p.to_str())
                    .filter(|s| s.starts_with("content://"))
                    && let Some(embedded) = read_embedded_lyrics_via_content_opener(raw_uri)
                {
                    let parsed = SyncedLyrics::parse_with_context(
                        &embedded,
                        track.duration_ms,
                        "Embedded Tag",
                    );
                    if !parsed.lines.is_empty() {
                        let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                        if st.lyrics_gen == gen_id {
                            let snap =
                                to_lyrics_snapshot(track.id.clone(), parsed, st.lyrics_offset_ms);
                            let _ = st
                                .db
                                .save_cached_lyrics(&lyrics_cache_key(&track), &embedded);
                            st.lyrics_cache = snap.clone();
                            drop(st);
                            this.sink.on_lyrics_loaded(snap);
                        }
                        return;
                    }
                }

                let local_fs_path = track
                    .path
                    .as_deref()
                    .filter(|p| p.to_str().is_none_or(|s| !s.starts_with("content://")));
                match fetch_best_free_lyrics(
                    &track.artist,
                    &track.title,
                    &track.album,
                    track.duration_ms,
                    local_fs_path,
                ) {
                    Ok(Some((parsed, raw_lrc))) => {
                        let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                        if st.lyrics_gen == gen_id {
                            let snap =
                                to_lyrics_snapshot(track.id.clone(), parsed, st.lyrics_offset_ms);
                            let _ = st
                                .db
                                .save_cached_lyrics(&lyrics_cache_key(&track), &raw_lrc);
                            st.lyrics_cache = snap.clone();
                            drop(st);
                            this.sink.on_lyrics_loaded(snap);
                        }
                    }
                    Ok(None) | Err(_) => {
                        let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                        if st.lyrics_gen == gen_id {
                            let empty = LyricsSnapshot {
                                track_id: track.id,
                                lines: Vec::new(),
                                source_name: "Unavailable".to_string(),
                                is_synced: false,
                                offset_ms: st.lyrics_offset_ms,
                                is_loading: false,
                            };
                            st.lyrics_cache = empty.clone();
                            drop(st);
                            this.sink.on_lyrics_loaded(empty);
                        }
                    }
                }
            });
    }

    /// Shift synced lyrics timing by `delta_ms` (e.g. `-500` or `+500`), clamped to `[-10s, +10s]`.
    pub fn nudge_lyrics_offset_ms(&self, delta_ms: i64) -> i64 {
        let snap = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.lyrics_offset_ms = st
                .lyrics_offset_ms
                .saturating_add(delta_ms)
                .clamp(-10_000, 10_000);
            st.lyrics_cache.offset_ms = st.lyrics_offset_ms;
            let track_id = st.lyrics_cache.track_id.clone();
            if !track_id.is_empty() {
                let _ = st
                    .db
                    .set_track_lyrics_offset(&track_id, st.lyrics_offset_ms);
            }
            st.lyrics_cache.clone()
        };
        self.sink.on_lyrics_loaded(snap.clone());
        snap.offset_ms
    }

    /// Resolve official music video streams (360p/480p/720p/1080p) off-thread for muted clock-synced playback.
    pub fn resolve_video_streams(self: &Arc<Self>, track: Track) {
        let (gen_id, session) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.video_gen = st.video_gen.wrapping_add(1);
            (st.video_gen, active_ytm_session(&st.accounts))
        };

        self.sink.on_video_streams_loaded(VideoStreamsSnapshot {
            track_id: track.id.clone(),
            video_id: track.youtube_video_id.clone().unwrap_or_default(),
            is_official_mv: false,
            options: Vec::new(),
            is_loading: true,
        });

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-video-res".to_string())
            .spawn(move || {
                match resolve_music_video_streams(
                    &session,
                    &track.artist,
                    &track.title,
                    track.youtube_video_id.as_deref(),
                    track.duration_ms,
                ) {
                    Ok(resolved) => {
                        let still_active = {
                            let st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                            st.video_gen == gen_id
                        };
                        if still_active {
                            this.sink.on_video_streams_loaded(VideoStreamsSnapshot {
                                track_id: track.id,
                                video_id: resolved.video_id,
                                is_official_mv: resolved.is_official_mv,
                                options: resolved
                                    .streams
                                    .into_iter()
                                    .map(|o| VideoStreamOptionSnapshot {
                                        quality: o.quality,
                                        label: o.label,
                                        width: o.width,
                                        height: o.height,
                                        bitrate_kbps: o.bitrate_kbps,
                                        url: o.url,
                                        mime: o.mime,
                                    })
                                    .collect(),
                                is_loading: false,
                            });
                        }
                    }
                    Err(err) => {
                        let still_active = {
                            let st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                            st.video_gen == gen_id
                        };
                        if still_active {
                            this.sink.on_video_streams_loaded(VideoStreamsSnapshot {
                                track_id: track.id,
                                video_id: String::new(),
                                is_official_mv: false,
                                options: Vec::new(),
                                is_loading: false,
                            });
                            this.sink.on_toast(format!("Video clip unavailable: {err}"));
                        }
                    }
                }
            });
    }

    // --- EQ, AutoEq & Equalizer APO ---

    #[must_use]
    pub fn list_eq_presets(&self) -> Vec<String> {
        EQ_PRESET_NAMES.iter().map(|s| (*s).to_string()).collect()
    }

    pub fn apply_eq_preset(&self, preset_name: &str) -> ControllerResult<EqConfig> {
        let cfg = EqConfig::preset(preset_name);
        self.set_eq(cfg.clone())?;
        Ok(cfg)
    }

    #[must_use]
    pub fn search_autoeq(&self, query: &str) -> Vec<AutoEqProfileSnapshot> {
        search_autoeq_profiles(query)
            .into_iter()
            .map(|p| AutoEqProfileSnapshot {
                name: p.name.to_string(),
                category: p.category.to_string(),
                preamp_db: p.preamp_db,
            })
            .collect()
    }

    pub fn apply_autoeq_profile(&self, profile_name: &str) -> ControllerResult<EqConfig> {
        let Some(profile) = AUTOEQ_PROFILES
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(profile_name.trim()))
        else {
            return Err(ControllerError::State(format!(
                "AutoEq profile not found: {profile_name}"
            )));
        };
        let cfg = profile.to_eq_config();
        self.set_eq(cfg.clone())?;
        self.sink
            .on_toast(format!("Applied AutoEq: {}", profile.name));
        Ok(cfg)
    }

    pub fn import_eq_apo(&self, apo_text: &str) -> ControllerResult<EqConfig> {
        let cfg = EqConfig::from_equalizer_apo(apo_text)?;
        self.set_eq(cfg.clone())?;
        self.sink.on_toast(format!(
            "Imported Equalizer APO ({} bands)",
            cfg.bands.len()
        ));
        Ok(cfg)
    }

    #[must_use]
    pub fn export_eq_apo(&self) -> String {
        self.get_eq().to_equalizer_apo()
    }

    // --- Theme Studio & Desktop-Compatible `.ferric-theme.json` ---

    pub fn apply_theme_preset(&self, preset_name: &str) -> ControllerResult<ThemeConfig> {
        let prev = self.get_theme();
        let mut next = match preset_name.trim().to_ascii_lowercase().as_str() {
            "liquid glass" | "liquid_glass" | "native acrylic" => ThemeConfig::liquid_glass(),
            "obsidian studio" | "obsidian" => ThemeConfig::obsidian_slate(),
            "warm cream light" | "warm cream" | "light" => ThemeConfig::cream_light(),
            _ => ThemeConfig::pure_amoled(),
        };
        next.editorial_mode = prev.editorial_mode;
        next.font_family = prev.font_family;
        self.set_theme(next.clone())?;
        Ok(next)
    }

    pub fn export_theme_json(&self) -> ControllerResult<String> {
        let theme = self.get_theme();
        serde_json::to_string_pretty(&theme).map_err(|e| ControllerError::State(e.to_string()))
    }

    pub fn import_theme_json(&self, raw_json: &str) -> ControllerResult<ThemeConfig> {
        let parsed: ThemeConfig = serde_json::from_str(raw_json)
            .map_err(|e| ControllerError::State(format!("Invalid .ferric-theme.json: {e}")))?;
        let sanitized = parsed.sanitized();
        self.set_theme(sanitized.clone())?;
        self.sink
            .on_toast(format!("Imported theme \"{}\"", sanitized.name));
        Ok(sanitized)
    }
}
