use crate::CoreResult;
use crate::eq::EqConfig;
use crate::models::{
    AccountSettings, PlaybackDspSettings, Playlist, SessionResumeState, Track, TrackSource,
    UpdateSettings,
};
use crate::theme::ThemeConfig;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Persistent SQLite WAL database for tracks, playlists, settings, theme, EQ, and cached lyrics.
pub struct LibraryDb {
    conn: Connection,
}

impl LibraryDb {
    pub fn open(path: &Path) -> CoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> CoreResult<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> CoreResult<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS tracks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                file_path TEXT,
                source TEXT NOT NULL,
                codec TEXT NOT NULL,
                sample_rate INTEGER NOT NULL DEFAULT 0,
                bit_depth INTEGER NOT NULL DEFAULT 0,
                bitrate_kbps INTEGER NOT NULL DEFAULT 0,
                isrc TEXT,
                spotify_id TEXT,
                youtube_video_id TEXT,
                thumbnail_url TEXT,
                replay_gain_db INTEGER NOT NULL DEFAULT 0,
                liked INTEGER NOT NULL DEFAULT 0,
                added_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );
            CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title);
            CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist);
            CREATE INDEX IF NOT EXISTS idx_tracks_isrc ON tracks(isrc);

            CREATE TABLE IF NOT EXISTS playlists (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                source TEXT NOT NULL,
                etag TEXT,
                description TEXT NOT NULL DEFAULT '',
                thumbnail_url TEXT,
                updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );

            CREATE TABLE IF NOT EXISTS playlist_tracks (
                playlist_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                track_id TEXT NOT NULL,
                PRIMARY KEY (playlist_id, position)
            );

            CREATE TABLE IF NOT EXISTS music_folders (
                path TEXT PRIMARY KEY,
                added_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS lyrics_cache (
                track_key TEXT PRIMARY KEY,
                synced_lrc TEXT NOT NULL,
                updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );

            CREATE TABLE IF NOT EXISTS paired_devices (
                device_id TEXT PRIMARY KEY,
                device_name TEXT NOT NULL,
                last_seen_utc INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );
            "#,
        )?;
        // Safe idempotent schema upgrades for existing user databases.
        let _ = self
            .conn
            .execute("ALTER TABLE tracks ADD COLUMN thumbnail_url TEXT", []);
        let _ = self
            .conn
            .execute("ALTER TABLE playlists ADD COLUMN thumbnail_url TEXT", []);
        Ok(())
    }

    pub fn upsert_tracks(&mut self, tracks: &[Track]) -> CoreResult<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0usize;
        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO tracks (
                    id, title, artist, album, duration_ms, file_path, source,
                    codec, sample_rate, bit_depth, bitrate_kbps, isrc,
                    spotify_id, youtube_video_id, replay_gain_db, liked, thumbnail_url
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
                ON CONFLICT(id) DO UPDATE SET
                    title = excluded.title,
                    artist = excluded.artist,
                    album = excluded.album,
                    duration_ms = CASE
                        WHEN excluded.duration_ms > 0 THEN excluded.duration_ms
                        ELSE tracks.duration_ms
                    END,
                    file_path = COALESCE(excluded.file_path, tracks.file_path),
                    codec = CASE
                        WHEN excluded.file_path IS NULL AND tracks.file_path IS NOT NULL AND tracks.codec != '' THEN tracks.codec
                        ELSE excluded.codec
                    END,
                    sample_rate = CASE
                        WHEN excluded.sample_rate > 0 THEN excluded.sample_rate
                        ELSE tracks.sample_rate
                    END,
                    bit_depth = CASE
                        WHEN excluded.bit_depth > 0 THEN excluded.bit_depth
                        ELSE tracks.bit_depth
                    END,
                    bitrate_kbps = CASE
                        WHEN excluded.bitrate_kbps > 0 THEN excluded.bitrate_kbps
                        ELSE tracks.bitrate_kbps
                    END,
                    isrc = COALESCE(excluded.isrc, tracks.isrc),
                    spotify_id = COALESCE(excluded.spotify_id, tracks.spotify_id),
                    youtube_video_id = COALESCE(excluded.youtube_video_id, tracks.youtube_video_id),
                    replay_gain_db = CASE
                        WHEN excluded.replay_gain_db != 0 THEN excluded.replay_gain_db
                        ELSE tracks.replay_gain_db
                    END,
                    liked = MAX(tracks.liked, excluded.liked),
                    thumbnail_url = COALESCE(excluded.thumbnail_url, tracks.thumbnail_url)
                "#,
            )?;
            for t in tracks {
                let path_str = t.path.as_ref().map(|p| p.to_string_lossy().into_owned());
                stmt.execute(params![
                    t.id,
                    t.title,
                    t.artist,
                    t.album,
                    t.duration_ms as i64,
                    path_str,
                    t.source.as_badge(),
                    t.codec,
                    i64::from(t.sample_rate),
                    i64::from(t.bit_depth),
                    i64::from(t.bitrate_kbps),
                    t.isrc,
                    t.spotify_id,
                    t.youtube_video_id,
                    t.replay_gain_db,
                    i64::from(t.liked),
                    t.thumbnail_url,
                ])?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    pub fn list_tracks(&self, search_query: &str) -> CoreResult<Vec<Track>> {
        let q = search_query.trim();
        let mut stmt = if q.is_empty() {
            self.conn.prepare(
                "SELECT id, title, artist, album, duration_ms, file_path, source, codec, sample_rate, bit_depth, bitrate_kbps, isrc, spotify_id, youtube_video_id, replay_gain_db, liked, thumbnail_url FROM tracks ORDER BY artist COLLATE NOCASE, album COLLATE NOCASE, title COLLATE NOCASE",
            )?
        } else {
            self.conn.prepare(
                "SELECT id, title, artist, album, duration_ms, file_path, source, codec, sample_rate, bit_depth, bitrate_kbps, isrc, spotify_id, youtube_video_id, replay_gain_db, liked, thumbnail_url FROM tracks WHERE title LIKE ?1 OR artist LIKE ?1 OR album LIKE ?1 ORDER BY artist COLLATE NOCASE, title COLLATE NOCASE",
            )?
        };

        let mut out = Vec::new();
        if q.is_empty() {
            let rows = stmt.query_map([], row_to_track)?;
            for item in rows {
                out.push(item?);
            }
        } else {
            let pat = format!("%{q}%");
            let rows = stmt.query_map([pat], row_to_track)?;
            for item in rows {
                out.push(item?);
            }
        }
        Ok(out)
    }

    pub fn set_track_liked(&self, track_id: &str, liked: bool) -> CoreResult<()> {
        self.conn.execute(
            "UPDATE tracks SET liked = ?1 WHERE id = ?2",
            params![i64::from(liked), track_id],
        )?;
        Ok(())
    }

    pub fn upsert_playlist(&mut self, playlist: &Playlist, track_ids: &[String]) -> CoreResult<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            r#"
            INSERT INTO playlists (id, name, source, etag, description, thumbnail_url, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%s','now'))
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                source = excluded.source,
                etag = excluded.etag,
                description = excluded.description,
                thumbnail_url = COALESCE(excluded.thumbnail_url, playlists.thumbnail_url),
                updated_at = strftime('%s','now')
            "#,
            params![
                playlist.id,
                playlist.name,
                playlist.source.as_badge(),
                playlist.etag,
                playlist.description,
                playlist.thumbnail_url,
            ],
        )?;
        tx.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
            params![playlist.id],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO playlist_tracks (playlist_id, position, track_id) VALUES (?1, ?2, ?3)",
            )?;
            for (pos, track_id) in track_ids.iter().enumerate() {
                stmt.execute(params![playlist.id, pos as i64, track_id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn add_track_to_playlist(&mut self, playlist_id: &str, track_id: &str) -> CoreResult<()> {
        let next_pos: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_tracks WHERE playlist_id = ?1",
            [playlist_id],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO playlist_tracks (playlist_id, position, track_id) VALUES (?1, ?2, ?3)",
            params![playlist_id, next_pos, track_id],
        )?;
        Ok(())
    }

    pub fn delete_playlist(&mut self, playlist_id: &str) -> CoreResult<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
            [playlist_id],
        )?;
        tx.execute("DELETE FROM playlists WHERE id = ?1", [playlist_id])?;
        tx.commit()?;
        Ok(())
    }

    /// Remove synced Google / YouTube Music account playlists (`ytm:pl:*`) and auto-generated
    /// `ytm:home:discover` rows when signed out, strictly preserving manually created (`local:pl:*`)
    /// and manually imported (`spotify:*`, `yt:playlist:*`) playlists.
    pub fn delete_account_synced_playlists(&mut self) -> CoreResult<usize> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id LIKE 'ytm:pl:%' OR playlist_id = 'ytm:home:discover'",
            [],
        )?;
        let removed = tx.execute(
            "DELETE FROM playlists WHERE id LIKE 'ytm:pl:%' OR id = 'ytm:home:discover'",
            [],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    pub fn list_playlists(&self) -> CoreResult<Vec<Playlist>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT
                p.id,
                p.name,
                p.source,
                p.etag,
                p.description,
                (SELECT COUNT(*) FROM playlist_tracks pt WHERE pt.playlist_id = p.id) AS track_count,
                p.thumbnail_url
            FROM playlists p
            ORDER BY p.name COLLATE NOCASE ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            let source_str: String = row.get(2)?;
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                source: TrackSource::from_db_str(&source_str),
                etag: row.get(3)?,
                description: row.get(4)?,
                track_count: row.get::<_, i64>(5)?.max(0) as usize,
                thumbnail_url: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item?);
        }
        Ok(out)
    }

    pub fn list_playlist_tracks(&self, playlist_id: &str) -> CoreResult<Vec<Track>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT
                t.id, t.title, t.artist, t.album, t.duration_ms, t.file_path, t.source,
                t.codec, t.sample_rate, t.bit_depth, t.bitrate_kbps, t.isrc,
                t.spotify_id, t.youtube_video_id, t.replay_gain_db, t.liked, t.thumbnail_url
            FROM playlist_tracks pt
            INNER JOIN tracks t ON t.id = pt.track_id
            WHERE pt.playlist_id = ?1
            ORDER BY pt.position ASC
            "#,
        )?;
        let rows = stmt.query_map([playlist_id], row_to_track)?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item?);
        }
        Ok(out)
    }

    /// Match cloud catalog tracks (`file_path IS NULL`) against local tracks (`file_path IS NOT NULL`)
    /// first by case-insensitive ISRC, then by normalized lowercase `(title, artist)`, copying
    /// `file_path`, `codec`, `sample_rate`, `bit_depth`, and `bitrate_kbps` onto matched rows.
    pub fn match_cloud_tracks_to_local(&mut self) -> CoreResult<usize> {
        struct LocalCandidate {
            file_path: String,
            codec: String,
            sample_rate: i64,
            bit_depth: i64,
            bitrate_kbps: i64,
        }

        let mut locals = Vec::new();
        let mut by_isrc: HashMap<String, usize> = HashMap::new();
        let mut by_title_artist: HashMap<(String, String), usize> = HashMap::new();

        {
            let mut stmt = self.conn.prepare(
                "SELECT file_path, codec, sample_rate, bit_depth, bitrate_kbps, isrc, title, artist FROM tracks WHERE file_path IS NOT NULL AND file_path != ''",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })?;

            for item in rows {
                let (file_path, codec, sample_rate, bit_depth, bitrate_kbps, isrc, title, artist) =
                    item?;
                let idx = locals.len();
                if let Some(code) = isrc {
                    let clean_isrc = code.trim().to_ascii_uppercase();
                    if !clean_isrc.is_empty() {
                        by_isrc.entry(clean_isrc).or_insert(idx);
                    }
                }
                let norm_title = normalize_match_token(&title);
                let norm_artist = normalize_match_token(&artist);
                if !norm_title.is_empty() && !norm_artist.is_empty() {
                    by_title_artist
                        .entry((norm_title, norm_artist))
                        .or_insert(idx);
                }
                locals.push(LocalCandidate {
                    file_path,
                    codec,
                    sample_rate,
                    bit_depth,
                    bitrate_kbps,
                });
            }
        }

        if locals.is_empty() {
            return Ok(0);
        }

        let mut cloud_rows = Vec::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, isrc, title, artist FROM tracks WHERE file_path IS NULL OR file_path = ''",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            for item in rows {
                cloud_rows.push(item?);
            }
        }

        if cloud_rows.is_empty() {
            return Ok(0);
        }

        let tx = self.conn.transaction()?;
        let mut matched = 0usize;
        {
            let mut update_stmt = tx.prepare(
                "UPDATE tracks SET file_path = ?1, codec = ?2, sample_rate = ?3, bit_depth = ?4, bitrate_kbps = ?5 WHERE id = ?6",
            )?;
            for (cloud_id, isrc, title, artist) in cloud_rows {
                let candidate_idx = isrc
                    .as_deref()
                    .map(|s| s.trim().to_ascii_uppercase())
                    .filter(|s| !s.is_empty())
                    .and_then(|s| by_isrc.get(&s).copied())
                    .or_else(|| {
                        let key = (
                            normalize_match_token(&title),
                            normalize_match_token(&artist),
                        );
                        by_title_artist.get(&key).copied()
                    });

                if let Some(idx) = candidate_idx {
                    let local = &locals[idx];
                    update_stmt.execute(params![
                        local.file_path,
                        local.codec,
                        local.sample_rate,
                        local.bit_depth,
                        local.bitrate_kbps,
                        cloud_id,
                    ])?;
                    matched += 1;
                }
            }
        }
        tx.commit()?;
        Ok(matched)
    }

    pub fn add_music_folder(&self, folder: &Path) -> CoreResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO music_folders (path) VALUES (?1)",
            [folder.to_string_lossy().as_ref()],
        )?;
        Ok(())
    }

    pub fn list_music_folders(&self) -> CoreResult<Vec<PathBuf>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM music_folders ORDER BY added_at ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for p in rows {
            out.push(PathBuf::from(p?));
        }
        Ok(out)
    }

    pub fn save_theme(&self, theme: &ThemeConfig) -> CoreResult<()> {
        self.set_setting("theme", theme)
    }

    pub fn load_theme(&self) -> CoreResult<ThemeConfig> {
        Ok(self.get_setting("theme")?.unwrap_or_default())
    }

    pub fn save_eq(&self, eq: &EqConfig) -> CoreResult<()> {
        self.set_setting("eq", eq)
    }

    pub fn load_eq(&self) -> CoreResult<EqConfig> {
        Ok(self.get_setting("eq")?.unwrap_or_default())
    }

    pub fn save_playback_dsp(&self, dsp: &PlaybackDspSettings) -> CoreResult<()> {
        self.set_setting("playback_dsp", &dsp.sanitized())
    }

    pub fn load_playback_dsp(&self) -> CoreResult<PlaybackDspSettings> {
        let loaded: PlaybackDspSettings = self.get_setting("playback_dsp")?.unwrap_or_default();
        Ok(loaded.sanitized())
    }

    pub fn save_accounts(&self, accounts: &AccountSettings) -> CoreResult<()> {
        self.set_setting("accounts", accounts)
    }

    pub fn load_accounts(&self) -> CoreResult<AccountSettings> {
        Ok(self.get_setting("accounts")?.unwrap_or_default())
    }

    pub fn save_update_settings(&self, settings: &UpdateSettings) -> CoreResult<()> {
        self.set_setting("updates", settings)
    }

    pub fn load_update_settings(&self) -> CoreResult<UpdateSettings> {
        Ok(self.get_setting("updates")?.unwrap_or_default())
    }

    pub fn save_session_state(&self, state: &SessionResumeState) -> CoreResult<()> {
        self.set_setting("session_resume", state)
    }

    pub fn load_session_state(&self) -> CoreResult<SessionResumeState> {
        Ok(self.get_setting("session_resume")?.unwrap_or_default())
    }

    pub fn get_cached_lyrics(&self, track_key: &str) -> CoreResult<Option<String>> {
        let val = self
            .conn
            .query_row(
                "SELECT synced_lrc FROM lyrics_cache WHERE track_key = ?1",
                [track_key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(val)
    }

    pub fn save_cached_lyrics(&self, track_key: &str, lrc: &str) -> CoreResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO lyrics_cache (track_key, synced_lrc) VALUES (?1, ?2)",
            params![track_key, lrc],
        )?;
        Ok(())
    }

    fn set_setting<T: serde::Serialize>(&self, key: &str, value: &T) -> CoreResult<()> {
        let json = serde_json::to_string(value)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO app_settings (key, value_json) VALUES (?1, ?2)",
            params![key, json],
        )?;
        Ok(())
    }

    fn get_setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> CoreResult<Option<T>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        match raw {
            Some(txt) => Ok(Some(serde_json::from_str(&txt)?)),
            None => Ok(None),
        }
    }
}

fn row_to_track(row: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    let file_path: Option<String> = row.get(5)?;
    let source_str: String = row.get(6)?;
    Ok(Track {
        id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        album: row.get(3)?,
        duration_ms: row.get::<_, i64>(4)?.max(0) as u64,
        path: file_path.map(PathBuf::from),
        source: TrackSource::from_db_str(&source_str),
        codec: row.get(7)?,
        sample_rate: row.get::<_, i64>(8)?.max(0) as u32,
        bit_depth: row.get::<_, i64>(9)?.max(0) as u16,
        bitrate_kbps: row.get::<_, i64>(10)?.max(0) as u32,
        isrc: row.get(11)?,
        spotify_id: row.get(12)?,
        youtube_video_id: row.get(13)?,
        thumbnail_url: row.get(16)?,
        replay_gain_db: row.get(14)?,
        liked: row.get::<_, i64>(15)? != 0,
    })
}

fn normalize_match_token(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for word in raw.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&word.to_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_wal_persists_tracks_and_native_acrylic_theme() {
        let mut db = LibraryDb::open_in_memory().expect("open memory db");
        let track = Track {
            id: "local:1".to_string(),
            title: "Instant Crush".to_string(),
            artist: "Daft Punk".to_string(),
            album: "Random Access Memories".to_string(),
            duration_ms: 337_000,
            path: Some(PathBuf::from("C:/Music/instant_crush.flac")),
            source: TrackSource::Local,
            codec: "FLAC".to_string(),
            sample_rate: 96_000,
            bit_depth: 24,
            bitrate_kbps: 2800,
            isrc: Some("USQX91300105".to_string()),
            spotify_id: None,
            youtube_video_id: None,
            thumbnail_url: Some("https://example.com/ram.jpg".to_string()),
            replay_gain_db: 0,
            liked: true,
        };
        assert_eq!(db.upsert_tracks(&[track]).expect("upsert"), 1);
        let found = db.list_tracks("Daft").expect("search");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Instant Crush");
        assert_eq!(
            found[0].thumbnail_url.as_deref(),
            Some("https://example.com/ram.jpg")
        );

        let theme = ThemeConfig {
            acrylic_enabled: true,
            acrylic_opacity: 0.80,
            ..ThemeConfig::default()
        };
        db.save_theme(&theme).expect("save theme");
        let loaded = db.load_theme().expect("load theme");
        assert!(loaded.acrylic_enabled);
        assert!((loaded.acrylic_opacity - 0.80).abs() < 1e-4);

        // Verify signed-out cleanup removes account playlists while keeping manual playlists intact
        let manual_pl = Playlist {
            id: "local:pl:manual1".to_string(),
            name: "Manual Mix".to_string(),
            source: TrackSource::Local,
            etag: None,
            track_count: 0,
            description: "User created".to_string(),
            thumbnail_url: None,
        };
        let acct_pl = Playlist {
            id: "ytm:pl:VL12345".to_string(),
            name: "Account Playlist".to_string(),
            source: TrackSource::YouTubeMusic,
            etag: None,
            track_count: 0,
            description: "Synced".to_string(),
            thumbnail_url: None,
        };
        db.upsert_playlist(&manual_pl, &[]).expect("manual pl");
        db.upsert_playlist(&acct_pl, &[]).expect("acct pl");
        assert_eq!(db.list_playlists().expect("list").len(), 2);
        let removed = db.delete_account_synced_playlists().expect("cleanup");
        assert_eq!(removed, 1);
        let after = db.list_playlists().expect("list after");
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].id, "local:pl:manual1");

        assert!(
            db.load_update_settings()
                .expect("default updates")
                .auto_check_on_startup
        );
        db.save_update_settings(&UpdateSettings {
            auto_check_on_startup: false,
        })
        .expect("save updates");
        assert!(
            !db.load_update_settings()
                .expect("loaded updates")
                .auto_check_on_startup
        );
    }

    #[test]
    fn playlists_dsp_and_cloud_to_local_matching_work_end_to_end() {
        let mut db = LibraryDb::open_in_memory().expect("open memory db");
        let local = Track {
            id: "local:daft".to_string(),
            title: "Instant Crush".to_string(),
            artist: "Daft Punk".to_string(),
            album: "Random Access Memories".to_string(),
            duration_ms: 337_000,
            path: Some(PathBuf::from("C:/Music/instant_crush.flac")),
            source: TrackSource::Local,
            codec: "FLAC".to_string(),
            sample_rate: 96_000,
            bit_depth: 24,
            bitrate_kbps: 2800,
            isrc: Some("USQX91300105".to_string()),
            spotify_id: None,
            youtube_video_id: None,
            thumbnail_url: None,
            replay_gain_db: -2,
            liked: false,
        };
        let cloud = Track {
            id: "spotify:track:2cGxRwrMyEAp8dEbuZaVv6".to_string(),
            title: "Instant Crush".to_string(),
            artist: "Daft Punk".to_string(),
            album: "Random Access Memories".to_string(),
            duration_ms: 337_000,
            path: None,
            source: TrackSource::Spotify,
            codec: "SPOTIFY".to_string(),
            sample_rate: 0,
            bit_depth: 0,
            bitrate_kbps: 0,
            isrc: Some("usqx91300105".to_string()),
            spotify_id: Some("2cGxRwrMyEAp8dEbuZaVv6".to_string()),
            youtube_video_id: None,
            thumbnail_url: None,
            replay_gain_db: 0,
            liked: true,
        };
        db.upsert_tracks(&[local, cloud]).expect("upsert");

        let matched = db.match_cloud_tracks_to_local().expect("match");
        assert_eq!(matched, 1);

        let pl = Playlist {
            id: "spotify:pl:1".to_string(),
            name: "Late Night Hi-Fi".to_string(),
            source: TrackSource::Spotify,
            etag: Some("etag-1".to_string()),
            track_count: 1,
            description: "Synced audiophile favorites".to_string(),
            thumbnail_url: None,
        };
        db.upsert_playlist(&pl, &["spotify:track:2cGxRwrMyEAp8dEbuZaVv6".to_string()])
            .expect("upsert playlist");

        let playlists = db.list_playlists().expect("list playlists");
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].track_count, 1);

        let pl_tracks = db
            .list_playlist_tracks("spotify:pl:1")
            .expect("list playlist tracks");
        assert_eq!(pl_tracks.len(), 1);
        assert_eq!(pl_tracks[0].codec, "FLAC");
        assert!(pl_tracks[0].path.is_some());

        db.set_track_liked("local:daft", true).expect("set liked");
        let dsp = PlaybackDspSettings::default();
        db.save_playback_dsp(&dsp).expect("save dsp");
        assert_eq!(db.load_playback_dsp().expect("load dsp"), dsp);
    }
}
