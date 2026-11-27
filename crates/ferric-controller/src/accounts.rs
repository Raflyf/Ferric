//! Account persistence, two-way Spotify/YouTube Music sync, and OpenSubsonic integration.

use crate::types::AccountStatusSnapshot;
use crate::{ControllerError, ControllerResult, FerricController};
use ferric_core::models::AccountSettings;
use ferric_providers::spotify_web::{SpotifyWebSyncError, sync_spotify_web_catalog};
use ferric_providers::subsonic::{SubsonicCredentials, sync_subsonic_catalog};
use ferric_providers::ytmusic::{YtmSession, fetch_account_library};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

pub(crate) fn account_status_from_settings(a: &AccountSettings) -> AccountStatusSnapshot {
    let spotify_connected = a.spotify_web_connected || a.spotify_connected;
    let spotify_user_name = if !a.spotify_web_user_name.is_empty() {
        a.spotify_web_user_name.clone()
    } else {
        a.spotify_user_name.clone()
    };
    AccountStatusSnapshot {
        spotify_connected,
        spotify_user_name,
        spotify_user_id: a.spotify_user_id.clone(),
        spotify_playlist_count: a.spotify_playlist_count,
        spotify_liked_count: a.spotify_liked_count,
        youtube_connected: a.youtube_connected,
        youtube_channel_name: a.youtube_channel_name.clone(),
        youtube_account_id: a.youtube_account_id.clone(),
        youtube_playlist_count: a.youtube_playlist_count,
        subsonic_connected: a.subsonic_connected,
        subsonic_url: a.subsonic_url.clone(),
        subsonic_username: a.subsonic_username.clone(),
    }
}

pub(crate) fn active_spotify_token(a: &AccountSettings) -> Option<String> {
    if a.spotify_web_connected && !a.spotify_web_access_token.trim().is_empty() {
        Some(a.spotify_web_access_token.clone())
    } else if a.spotify_connected && !a.spotify_access_token.trim().is_empty() {
        Some(a.spotify_access_token.clone())
    } else {
        None
    }
}

pub(crate) fn active_ytm_session(a: &AccountSettings) -> YtmSession {
    if a.youtube_connected && !a.youtube_cookie.is_empty() && !a.youtube_sapisid.is_empty() {
        YtmSession {
            cookie: a.youtube_cookie.clone(),
            sapisid: a.youtube_sapisid.clone(),
            visitor_data: a.youtube_visitor_data.clone(),
        }
    } else {
        YtmSession::guest(a.youtube_visitor_data.clone())
    }
}

impl FerricController {
    #[must_use]
    pub fn get_account_status(&self) -> AccountStatusSnapshot {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        account_status_from_settings(&st.accounts)
    }

    /// Persist a Spotify Web Player session (from isolated WebView sign-in or silent refresh)
    /// and optionally trigger an immediate catalog sync.
    pub fn set_spotify_session(
        self: &Arc<Self>,
        access_token: String,
        cookie: String,
        user_name: String,
        user_id: String,
        trigger_sync: bool,
    ) -> ControllerResult<()> {
        let clean_token = access_token.trim().to_string();
        if clean_token.is_empty() {
            return Err(ControllerError::State(
                "Spotify access token cannot be empty".to_string(),
            ));
        }
        let status = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.accounts.spotify_web_connected = true;
            st.accounts.spotify_web_access_token = clean_token;
            if !cookie.trim().is_empty() {
                st.accounts.spotify_web_cookie = cookie.trim().to_string();
            }
            if !user_name.trim().is_empty() {
                st.accounts.spotify_web_user_name = user_name.trim().to_string();
            }
            if !user_id.trim().is_empty() {
                st.accounts.spotify_user_id = user_id.trim().to_string();
            }
            st.db.save_accounts(&st.accounts)?;
            account_status_from_settings(&st.accounts)
        };
        self.sink.on_accounts_changed(status);
        if trigger_sync {
            self.sync_spotify_account();
        }
        Ok(())
    }

    /// Synchronize Spotify Liked Songs and playlists off the main thread.
    pub fn sync_spotify_account(self: &Arc<Self>) {
        let (token, snapshots) = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let Some(tok) = active_spotify_token(&st.accounts) else {
                self.sink.on_toast("Sign in to Spotify first".to_string());
                return;
            };
            let mut map = HashMap::new();
            if let Ok(pls) = st.db.list_playlists() {
                for pl in pls {
                    if let Some(etag) = pl.etag {
                        map.insert(pl.id, etag);
                    }
                }
            }
            (tok, map)
        };

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-sp-sync".to_string())
            .spawn(move || match sync_spotify_web_catalog(&token, &snapshots) {
                Ok(report) => {
                    let status = {
                        let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                        let _ = st.db.delete_demo_tracks();
                        let _ = st.db.upsert_tracks(&report.liked_tracks);
                        for bundle in &report.playlists {
                            let _ = st.db.upsert_tracks(&bundle.tracks);
                            let ids: Vec<String> =
                                bundle.tracks.iter().map(|t| t.id.clone()).collect();
                            let _ = st.db.upsert_playlist(&bundle.playlist, &ids);
                        }
                        let _ = st.db.match_cloud_tracks_to_local();

                        if !report.display_name.trim().is_empty() {
                            st.accounts.spotify_web_user_name = report.display_name.clone();
                        }
                        if !report.account_id.trim().is_empty() {
                            st.accounts.spotify_user_id = report.account_id.clone();
                        }
                        st.accounts.spotify_web_connected = true;
                        st.accounts.spotify_liked_count = report.liked_tracks.len();
                        st.accounts.spotify_playlist_count = report.playlists.len();
                        let _ = st.db.save_accounts(&st.accounts);
                        account_status_from_settings(&st.accounts)
                    };
                    this.sink.on_accounts_changed(status);
                    this.sink.on_library_changed();
                    this.sink.on_toast(format!(
                        "Spotify synced: {} playlists, {} liked songs",
                        report.playlists.len(),
                        report.liked_tracks.len()
                    ));
                }
                Err(SpotifyWebSyncError::AuthExpired) => {
                    this.sink.on_spotify_auth_expired();
                    this.sink.on_toast(
                        "Spotify token expired; refreshing session in background...".to_string(),
                    );
                }
                Err(SpotifyWebSyncError::FallbackToRest(msg)) => {
                    this.sink.on_toast(format!("Spotify sync failed: {msg}"));
                }
            });
    }

    pub fn logout_spotify(&self) -> ControllerResult<()> {
        let status = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.accounts.zeroize_spotify_direct_secrets();
            st.accounts.zeroize_spotify_web_secrets();
            st.accounts.spotify_connected = false;
            st.accounts.spotify_web_connected = false;
            st.accounts.spotify_user_name.clear();
            st.accounts.spotify_web_user_name.clear();
            st.accounts.spotify_user_id.clear();
            st.accounts.spotify_playlist_count = 0;
            st.accounts.spotify_liked_count = 0;
            let _ = st.db.delete_spotify_synced_playlists();
            st.db.save_accounts(&st.accounts)?;
            account_status_from_settings(&st.accounts)
        };
        self.sink.on_accounts_changed(status);
        self.sink.on_library_changed();
        self.sink.on_toast("Signed out of Spotify".to_string());
        Ok(())
    }

    /// Persist a Google / YouTube Music WebView cookie session and sync library.
    pub fn set_ytm_session(
        self: &Arc<Self>,
        cookie: String,
        sapisid: String,
        visitor_data: String,
        channel_name: String,
        account_id: String,
    ) -> ControllerResult<()> {
        let clean_cookie = cookie.trim().to_string();
        let clean_sapisid = if sapisid.trim().is_empty() {
            ferric_providers::ytmusic::extract_sapisid_from_cookie(&clean_cookie)
                .unwrap_or_default()
        } else {
            sapisid.trim().to_string()
        };
        if clean_cookie.is_empty() || clean_sapisid.is_empty() {
            return Err(ControllerError::State(
                "YouTube Music session requires valid SAPISID cookie".to_string(),
            ));
        }
        let status = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.accounts.youtube_connected = true;
            st.accounts.youtube_cookie = clean_cookie;
            st.accounts.youtube_sapisid = clean_sapisid;
            if !visitor_data.trim().is_empty() {
                st.accounts.youtube_visitor_data = visitor_data.trim().to_string();
            }
            if !channel_name.trim().is_empty() {
                st.accounts.youtube_channel_name = channel_name.trim().to_string();
            }
            if !account_id.trim().is_empty() {
                st.accounts.youtube_account_id = account_id.trim().to_string();
            }
            st.db.save_accounts(&st.accounts)?;
            account_status_from_settings(&st.accounts)
        };
        self.sink.on_accounts_changed(status);
        self.sync_ytm_account();
        Ok(())
    }

    /// Synchronize YouTube Music Liked Songs, playlists, and stable `youtube_account_id`.
    pub fn sync_ytm_account(self: &Arc<Self>) {
        let session = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let sess = active_ytm_session(&st.accounts);
            if !sess.is_signed_in() {
                self.sink
                    .on_toast("Sign in to YouTube Music first".to_string());
                return;
            }
            sess
        };

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-ytm-sync".to_string())
            .spawn(move || match fetch_account_library(&session) {
                Ok(lib) => {
                    let pl_count = lib.playlists.len();
                    let liked_count = lib.liked_tracks.len();
                    let status = {
                        let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                        let _ = st.db.delete_demo_tracks();
                        let _ = st.db.upsert_tracks(&lib.liked_tracks);
                        for (pl, tracks) in &lib.playlists {
                            let _ = st.db.upsert_tracks(tracks);
                            let ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
                            let _ = st.db.upsert_playlist(pl, &ids);
                        }
                        let _ = st.db.match_cloud_tracks_to_local();

                        if !lib.display_name.trim().is_empty() {
                            st.accounts.youtube_channel_name = lib.display_name;
                        }
                        if !lib.account_id.trim().is_empty() {
                            st.accounts.youtube_account_id = lib.account_id;
                        }
                        if !lib.visitor_data.trim().is_empty() {
                            st.accounts.youtube_visitor_data = lib.visitor_data;
                        }
                        st.accounts.youtube_connected = true;
                        st.accounts.youtube_playlist_count = pl_count;
                        let _ = st.db.save_accounts(&st.accounts);
                        account_status_from_settings(&st.accounts)
                    };
                    this.sink.on_accounts_changed(status);
                    this.sink.on_library_changed();
                    this.sink.on_toast(format!(
                        "YouTube Music synced: {pl_count} playlists, {liked_count} liked songs"
                    ));
                }
                Err(err) => {
                    this.sink
                        .on_toast(format!("YouTube Music sync failed: {err}"));
                }
            });
    }

    pub fn logout_ytm(&self) -> ControllerResult<()> {
        let status = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.accounts.zeroize_youtube_secrets();
            st.accounts.youtube_connected = false;
            st.accounts.youtube_channel_name.clear();
            st.accounts.youtube_account_id.clear();
            st.accounts.youtube_playlist_count = 0;
            let _ = st.db.delete_account_synced_playlists();
            st.db.save_accounts(&st.accounts)?;
            account_status_from_settings(&st.accounts)
        };
        self.sink.on_accounts_changed(status);
        self.sink.on_library_changed();
        self.sink
            .on_toast("Signed out of YouTube Music".to_string());
        Ok(())
    }

    /// Save OpenSubsonic/Navidrome credentials and synchronize catalog + playlists off-thread.
    pub fn sync_subsonic(
        self: &Arc<Self>,
        base_url: String,
        username: String,
        password: String,
    ) -> ControllerResult<()> {
        let clean_url = base_url.trim().trim_end_matches('/').to_string();
        let clean_user = username.trim().to_string();
        if clean_url.is_empty() || clean_user.is_empty() || password.is_empty() {
            return Err(ControllerError::State(
                "Subsonic URL, username, and password are required".to_string(),
            ));
        }

        let creds = SubsonicCredentials {
            base_url: clean_url.clone(),
            username: clean_user.clone(),
            password: password.clone(),
        };
        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-subsonic-sync".to_string())
            .spawn(move || match sync_subsonic_catalog(&creds) {
                Ok((tracks, bundles)) => {
                    let song_count = tracks.len();
                    let pl_count = bundles.len();
                    let status = {
                        let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                        let _ = st.db.delete_demo_tracks();
                        let _ = st.db.upsert_tracks(&tracks);
                        for bundle in &bundles {
                            let _ = st.db.upsert_tracks(&bundle.tracks);
                            let ids: Vec<String> =
                                bundle.tracks.iter().map(|t| t.id.clone()).collect();
                            let _ = st.db.upsert_playlist(&bundle.playlist, &ids);
                        }
                        st.accounts.subsonic_url = clean_url;
                        st.accounts.subsonic_username = clean_user;
                        st.accounts.subsonic_password = password;
                        st.accounts.subsonic_connected = true;
                        let _ = st.db.save_accounts(&st.accounts);
                        account_status_from_settings(&st.accounts)
                    };
                    this.sink.on_accounts_changed(status);
                    this.sink.on_library_changed();
                    this.sink.on_toast(format!(
                        "Subsonic synced: {song_count} songs, {pl_count} playlists"
                    ));
                }
                Err(err) => {
                    this.sink.on_toast(format!("Subsonic sync failed: {err}"));
                }
            });
        Ok(())
    }

    pub fn logout_subsonic(&self) -> ControllerResult<()> {
        let status = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.accounts.zeroize_subsonic_secrets();
            st.accounts.subsonic_connected = false;
            st.accounts.subsonic_url.clear();
            st.accounts.subsonic_username.clear();
            st.db.save_accounts(&st.accounts)?;
            account_status_from_settings(&st.accounts)
        };
        self.sink.on_accounts_changed(status);
        self.sink
            .on_toast("Disconnected Subsonic server".to_string());
        Ok(())
    }
}
