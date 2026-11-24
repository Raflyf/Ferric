use crate::CoreResult;
use crate::eq::EqConfig;
use crate::models::{AccountSettings, Track, TrackSource, UpdateSettings};
use crate::theme::ThemeConfig;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

/// Persistent SQLite WAL database for tracks, settings, theme, EQ, and cached lyrics.
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
                replay_gain_db INTEGER NOT NULL DEFAULT 0,
                liked INTEGER NOT NULL DEFAULT 0,
                added_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );
            CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title);
            CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist);
            CREATE INDEX IF NOT EXISTS idx_tracks_isrc ON tracks(isrc);

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
                    spotify_id, youtube_video_id, replay_gain_db, liked
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
                ON CONFLICT(id) DO UPDATE SET
                    title = excluded.title,
                    artist = excluded.artist,
                    album = excluded.album,
                    duration_ms = excluded.duration_ms,
                    file_path = excluded.file_path,
                    codec = excluded.codec,
                    sample_rate = excluded.sample_rate,
                    bit_depth = excluded.bit_depth,
                    bitrate_kbps = excluded.bitrate_kbps
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
                "SELECT id, title, artist, album, duration_ms, file_path, source, codec, sample_rate, bit_depth, bitrate_kbps, isrc, spotify_id, youtube_video_id, replay_gain_db, liked FROM tracks ORDER BY artist COLLATE NOCASE, album COLLATE NOCASE, title COLLATE NOCASE",
            )?
        } else {
            self.conn.prepare(
                "SELECT id, title, artist, album, duration_ms, file_path, source, codec, sample_rate, bit_depth, bitrate_kbps, isrc, spotify_id, youtube_video_id, replay_gain_db, liked FROM tracks WHERE title LIKE ?1 OR artist LIKE ?1 OR album LIKE ?1 ORDER BY artist COLLATE NOCASE, title COLLATE NOCASE",
            )?
        };

        let mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<Track> {
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
                replay_gain_db: row.get(14)?,
                liked: row.get::<_, i64>(15)? != 0,
            })
        };

        let mut out = Vec::new();
        if q.is_empty() {
            let rows = stmt.query_map([], mapper)?;
            for item in rows {
                out.push(item?);
            }
        } else {
            let pat = format!("%{q}%");
            let rows = stmt.query_map([pat], mapper)?;
            for item in rows {
                out.push(item?);
            }
        }
        Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_wal_persists_tracks_and_liquid_glass_theme() {
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
            replay_gain_db: 0,
            liked: true,
        };
        assert_eq!(db.upsert_tracks(&[track]).expect("upsert"), 1);
        let found = db.list_tracks("Daft").expect("search");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Instant Crush");

        let mut theme = ThemeConfig::default();
        theme.material.liquid_glass = true;
        db.save_theme(&theme).expect("save theme");
        let loaded = db.load_theme().expect("load theme");
        assert!(loaded.material.liquid_glass);

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
}
