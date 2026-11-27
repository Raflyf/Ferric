# Ferric Android Milestone M2 Implementation Plan

> **For agentic workers:** Follow strict TDD (RED -> GREEN), apply `/rust-skills`, keep Ferric Desktop `v0.1.2` regression-free, and verify with fresh Cargo + Gradle + dual-APK checks before committing.

**Goal:** Deliver 100% core Desktop parity on Ferric Android: `Home` (Greeting, Mood Chips, Quick Picks, Recommended Stations, shelves, weighted Refresh), `Search` (instant local + debounced YTM + URL import + history), `Library` (`Songs`, `Albums`, `Artists`, `Playlists`, `Liked`, `Recents`, Local Lossless via `MediaStore` + SAF `ContentOpener`), `Collection Detail` (Sort, Deduplicate, Copy Link/CSV, `Sync to Account`), `Queue` bottom sheet (reorder, remove, save, copy), isolated WebView sign-in + silent Spotify token refresh, two-way Spotify & YTM sync, and OpenSubsonic real stream playback.

**Architecture:** All state aggregation, 2-way cloud sync, search ranking, local/Subsonic/YTM stream resolution, and collection helpers live in `crates/ferric-providers` and `crates/ferric-controller`, exposed cleanly over UniFFI in `crates/ferric-mobile-ffi`. `apps/ferric-android` renders native Jetpack Compose tabs with Coil 3 artwork loading, SAF/`MediaStore` local scanning, and hardened isolated WebView auth.

---

### Task 1: OpenSubsonic Real Stream Caching + Android Homelab CA Support (`crates/ferric-providers`)
- Add `download_and_cache_subsonic_stream(creds, song_id, suffix_hint, cache_dir)` in `crates/ferric-providers/src/subsonic.rs` using atomic `.part` download, size cap (`128 MiB`), container validation, and LRU pruning.
- Add `subsonic_http_agent()` that trusts Mozilla roots (`webpki-roots`) plus Android user-installed CA certificates (`/data/misc/user/0/cacerts-added`) on `#[cfg(target_os = "android")]` so homelab Navidrome/Jellyfin TLS certs work fail-closed without insecure cert skipping.
- Add unit tests verifying stream download caching, cache hit reuse, and SSRF/size-limit enforcement.

### Task 2: Expand `crates/ferric-controller` for Full M2 Parity
- Expand `ControllerEventSink` (`on_home_updated`, `on_search_results`, `on_detail_loaded`, `on_accounts_changed`, `on_spotify_auth_expired`).
- Add M2 controller modules cleanly (`crates/ferric-controller/src/{home.rs, library.rs, accounts.rs}`):
  - **Home:** `load_home(force_refresh, mood_idx)` (taste profile from `play_history` + liked + library + recents, `fetch_rich_home`, Spotify Pathfinder stations + algorithmic stations, weighted refresh exclusions), `open_station(station_id)`, `open_browse_detail(browse_id, kind)`.
  - **Search & URL Import:** `instant_search_local(query)` (zero-alloc `ferric_core::text::contains_ascii_icase`), `search_online(query)`, `import_public_url(url)`, recent search history persistence.
  - **Library, Local Entries & Collections:** `list_albums()`, `list_artists()`, `list_liked_songs()`, `list_recent_songs()`, `list_local_songs()`, `list_playlists_filtered(source, sort)`, `get_playlist_tracks(playlist_id, sort)`, `scan_local_entries(entries)` (upsert Android `MediaStore` / SAF `content://` tracks), `create_playlist`, `rename_playlist`, `delete_playlist`, `add_track_to_playlist`, `remove_track_from_playlist`, `deduplicate_playlist`, `push_local_playlist_to_cloud`, `toggle_like`, `format_share_tracklist`, `format_share_csv`, `track_share_link`, `import_playlist_text` (`.m3u`/`.csv`).
  - **Queue Operations:** `move_queue_item(from, to)`, `remove_queue_item(index)`, `clear_up_next()`, `save_queue_as_playlist(name)`, `copy_queue_tracklist()`, `play_next(track)`, `append_to_queue(track)`.
  - **Accounts & 2-Way Sync:** `get_account_status()`, `set_spotify_session(...)`, `sync_spotify_account()`, `logout_spotify()`, `set_ytm_session(...)`, `sync_ytm_account()`, `logout_ytm()`, `sync_subsonic(url, user, pass)`, `logout_subsonic()`, plus Subsonic playback branch inside `resolve_online_track_worker` and background Deck B stream prefetch!
- Add comprehensive controller unit tests (`instant_search_local`, `scan_local_entries`, `playlist_crud_dedup_export`, `queue_reorder_remove_save`, `subsonic_stream_playback_headless`).

### Task 3: Expand `crates/ferric-mobile-ffi` UniFFI API Surface
- Export all M2 records (`FfiHomeState`, `FfiMoodChip`, `FfiShelf`, `FfiShelfCard`, `FfiStationCard`, `FfiSearchResults`, `FfiCollectionDetail`, `FfiAlbumSummary`, `FfiArtistSummary`, `FfiAccountStatus`, `FfiLocalScanEntry`) and enums (`FfiCardKind`, `FfiPlaylistSort`, `FfiPlaylistSourceFilter`, `FfiDetailSort`).
- Expand `FfiEventSink` and `FerricMobileCore` methods.
- Verify with `cargo test -p ferric-mobile-ffi`, `cargo clippy --workspace --all-targets -- -D warnings`, and clean UniFFI Kotlin generation.

### Task 4: Android Kotlin Auth, Local Scanner (`MediaStore` + SAF), Coil 3 & Compose M2 UI (`apps/ferric-android`)
- Add `io.coil-kt.coil3:coil-compose:3.6.3`, `coil-network-okhttp:3.6.3`, `androidx.webkit:webkit:1.15.0` (verified compatible with compileSdk 36), `androidx.documentfile:documentfile:1.1.0` to `libs.versions.toml` and `app/build.gradle.kts`.
- Configure Coil 3 memory (32 MB) + disk (80 MB) `ImageLoader` in `FerricApp.kt`.
- Implement `auth/LoginHostAllowlist.kt` (unit-tested!), `auth/WebLoginActivity.kt` (isolated Spotify & Google/YTM WebView login with `WebViewCompat.addDocumentStartJavaScript` token capture and strict host allowlist), and `auth/SilentSpotifyRefresher.kt` (automatic offscreen bearer refresh on `onSpotifyAuthExpired`).
- Implement `local/LocalMediaScanner.kt` (`MediaStore.Audio` scanner + SAF `DocumentFile` folder walker returning `FfiLocalScanEntry` list with `content://` URIs).
- Build native Jetpack Compose screens & components:
  - Bottom Navigation (`Home`, `Search`, `Library`) + persistent swipeable `MiniPlayer` (left/right skip, tap/up expand to Queue/Controls) + top bar quick actions (`Accounts` / `Sync` / `Output`).
  - `HomeScreen.kt`: Time-based Greeting, Mood Chips, `Quick Picks` (`Play All` / `Shuffle`), `Recommended Stations`, horizontal personalized shelves, weighted `Refresh`.
  - `SearchScreen.kt`: Instant local results, debounced live YouTube Music search, search history chips, category filter pills (`All`, `Songs`, `Albums`, `Artists`, `Playlists`, `Local`), and direct Spotify/YouTube URL import bar.
  - `LibraryScreen.kt`: Sub-tabs (`Songs`, `Albums`, `Artists`, `Playlists`, `Liked`, `Recents`, `Local`), playlist source/sort chips, `+ New Playlist`, `Scan Phone Music` (`MediaStore` permission flow), `Pick Music Folder` (SAF `ACTION_OPEN_DOCUMENT_TREE`), `.m3u`/`.csv` import.
  - `DetailScreen.kt`: Unified detail view for Playlists, Albums, Artists, Liked Songs, Recommended Stations, and online YTM albums/playlists/artists (`Play All`, `Shuffle`, `Sort`, `Deduplicate`, `Copy Link`, `Copy Tracklist`, `Export CSV`, `Sync to Account`, clickable artist navigation).
  - `QueueSheet.kt`: ModalBottomSheet showing Now Playing + Up Next with move up/down reorder, remove, clear, `Save Queue`, and `Copy Queue`.
  - `AccountsSheet.kt`: Spotify (WebView Login + Sync + Logout), YouTube Music (WebView Login + Sync + Logout), and OpenSubsonic/Navidrome form (`URL`, `Username`, `Password`, `Connect & Sync`, `Disconnect`).
- Add JVM unit tests (`LoginHostAllowlistTest.kt` + expanded `PlaybackStateReducerTest.kt`).

### Task 5: Full End-to-End Verification & Commit M2
- Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Run full 3-ABI `scripts/build-android.ps1` (verifying `testDebugUnitTest`, `lintDebug`, R8 `assembleRelease`, 16 KB ELF alignment, dual APKs + `SHA256SUMS.txt`).
- Commit cleanly and push `main`.
