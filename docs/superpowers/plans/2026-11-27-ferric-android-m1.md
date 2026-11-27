# Ferric Android Milestone M1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the complete Milestone M1 vertical slice of Ferric Android: cross-platform Rust core adaptations, Android `oboe-rs` audio engine + `ContentOpener` FD bridge + `SetDucking`, headless `ferric-controller`, UniFFI `ferric-mobile-ffi` bindings, native Kotlin Jetpack Compose skeleton (`FerricPlaybackService`, `KeystoreVault`, `MediaRouter`, AMOLED player), space-safe Windows build script, and Linux GitHub Actions CI building verified dual APKs (`arm64-v8a` + `universal`).

**Architecture:** Option A Hybrid Monorepo inside `d:\code\project\music player\ferric`. Rust owns SQLite WAL storage, hardware-abstracted secret envelopes, dual-deck decoding/DSP/EQ (`oboe-rs` on Android, `cpal` unchanged on Windows), and headless playback/library orchestration (`crates/ferric-controller`), exposed to Kotlin via UniFFI proc-macros (`crates/ferric-mobile-ffi`). Kotlin Jetpack Compose (`apps/ferric-android`) owns Android UI, `MediaSessionService`, Keystore AES-256-GCM cipher, SAF `ContentOpener` FD bridge, AudioFocus/Noisy handling, and dual-ABI Gradle packaging.

**Tech Stack:** Rust 1.100 (edition 2024), `rusqlite` 0.43, `oboe` 0.6.1 (Android), `cpal` 0.18.2 (Desktop), `uniffi` 0.33.0, `jni` 0.22.4, `ndk-context` 0.1.1, `cargo-ndk` 4.1.2, Kotlin 2.x, Jetpack Compose BOM, Material 3, AndroidX Media3, JNA >= 5.15 (`5.19.1@aar`), Gradle 9.x / AGP, Android NDK r28+ (16 KB ELF page alignment).

**Spec:** [2026-11-27-ferric-android-design.md](file:///d:/code/project/music%20player/ferric/docs/superpowers/specs/2026-11-27-ferric-android-design.md)

## Global Constraints

- Monorepo root is `d:\code\project\music player\ferric`; never touch `LivelyDesk` or break Windows `ferric-desktop` `v0.1.2`.
- Zero em dashes (`U+2014`), zero emojis, zero telemetry/analytics SDKs, zero `unwrap()` in production Rust paths (`rust-skills`: `err-no-unwrap-prod`, `unsafe-safety-comment`, `unsafe-no-mangle-unsafe`, `own-slice-over-vec`).
- At-rest secret protection must fail closed (`CoreError::SecretProtection`); never silently overwrite credentials on decrypt failure.
- Android `minSdk = 26`, `targetSdk = 35`; never request `MANAGE_EXTERNAL_STORAGE`; never use VoIP `AudioManager.setCommunicationDevice` for media routing.
- Dual APK release output: `ferric-vX.Y.Z-android-arm64-v8a.apk` and `ferric-vX.Y.Z-android-universal.apk` (`arm64-v8a`, `armeabi-v7a`, `x86_64`) with 16 KB ELF page alignment (`-Wl,-z,max-page-size=16384`) and JNA/UniFFI R8 keep rules.
- `AudioEngine` starts lazily inside `FerricPlaybackService` (never in `FerricApp.onCreate()`).

## Review Focus

1. **Corrupted Keystore envelope (`keystore:v1:BAD`)**: Must fail closed (`plaintext == ""`, `failed == true`, `needs_migration == false`) so `LibraryDb::load_accounts()` never overwrites stored credentials during auto-migration (pinned in Task 1).
2. **Legacy Desktop `v0.1.2` `.ferric-theme.json` interoperability**: Android `ThemeConfig::liquid_glass()` must serialize with `background_mode = NativeAcrylic` and additive `#[serde(default)]` fields so Desktop `v0.1.2` parses it cleanly, and Desktop JSON lacking new fields deserializes on Android with sensible defaults (pinned in Task 1).
3. **Audio Focus ducking vs user volume**: `EngineCommand::SetDucking(true)` must attenuate gain independently without altering persisted `target_volume`, and `SetDucking(false)` must restore the exact prior volume even if volume changed while ducked (pinned in Task 2).
4. **Scoped Storage `content://` URI playback**: `ActiveDeck::open` must open and decode audio via registered `ContentOpener` FD callback without hitting `std::fs::File::open("content://...")` (pinned in Task 2).
5. **Lazy audio lifecycle & qualified `play_history` streak**: `FerricController` must construct and read SQLite/library without starting `AudioEngine`, qualify plays strictly at `>= 30s` (or `>= 50%` for tracks `< 60s`), and count consecutive local calendar days without double-counting multiple plays on the same day (pinned in Tasks 1 & 3).

---

### Task 1: `crates/ferric-core` Cross-Platform Storage, Keystore Hook, Schemas, Theme Compat & Shared Helpers

**Files:**
- Modify: `crates/ferric-core/src/lib.rs:1-42`
- Modify: `crates/ferric-core/src/secrets.rs:1-462`
- Modify: `crates/ferric-core/src/models.rs:356-518`
- Modify: `crates/ferric-core/src/theme.rs:72-300`
- Modify: `crates/ferric-core/src/db.rs:17-160, 870-1011`
- Create: `crates/ferric-core/src/collections.rs`
- Modify: `apps/ferric-desktop/src/state.rs:50-240, 804-991` (re-export moved pure helpers from `ferric_core::collections` without changing Desktop behavior)

**Interfaces:**
- Produces:
  - `ferric_core::set_app_data_dir_override(dir: PathBuf)` & `ferric_core::app_data_dir() -> PathBuf`
  - `ferric_core::LibraryDb::open_in_dir(dir: &Path) -> CoreResult<LibraryDb>`
  - `ferric_core::secrets::{KEYSTORE_PREFIX, SecretCipher, register_platform_secret_cipher(Arc<dyn SecretCipher>)}`
  - `AccountSettings::{spotify_user_id: String, youtube_account_id: String}`
  - `ferric_core::models::{OfflineDownloadRecord, PlayHistoryEntry, ListeningStatsSummary}`
  - `LibraryDb::{upsert_offline_download, list_offline_downloads, get_offline_download, delete_offline_download, clear_offline_downloads, offline_downloads_total_bytes}`
  - `LibraryDb::{record_play_history, get_listening_stats}` + `ferric_core::models::qualifies_as_play(listened_ms: u64, duration_ms: u64) -> bool`
  - `ThemeConfig::{editorial_mode: bool, font_family: String, player_layout: String, liquid_glass() -> Self}`
  - `ferric_core::collections::{PlaylistSortOrder, PlaylistSourceFilter, DetailSortOrder, deduplicate_tracks_in_order, track_primary_link, track_youtube_link, format_collection_tracklist, format_collection_csv}`

- [ ] **Step 1: Write failing tests in `ferric-core`**
  - In `lib.rs` / `secrets.rs` / `theme.rs` / `db.rs` / `collections.rs` unit test modules, add failing tests:
    - `app_data_dir_respects_explicit_override`
    - `custom_secret_cipher_roundtrips_and_fails_closed_on_tamper` (verifying `keystore:v1:` envelope, `failed == true` on `keystore:v1:BAD`, and `needs_migration == true` for plaintext when cipher registered)
    - `account_settings_persists_stable_ids_and_redacts_secrets`
    - `offline_downloads_crud_and_total_bytes`
    - `play_history_records_qualified_plays_and_computes_calendar_streak` (3 consecutive days with 2 plays on day 2 -> streak 3; short `< 30s` play on 180s track rejected; `20s` play on `35s` track accepted)
    - `liquid_glass_theme_preserves_desktop_v012_json_compatibility`
    - `collections_dedup_sort_and_share_links_match_desktop`

- [ ] **Step 2: Run tests to verify they fail**
  - Run: `cargo test -p ferric-core`
  - Expected: FAIL (missing symbols/methods).

- [ ] **Step 3: Implement Task 1 changes across `ferric-core` and re-export in `apps/ferric-desktop/src/state.rs`**
  - `lib.rs`: add `pub mod collections;`, `RwLock<Option<PathBuf>>` override checked first by `app_data_dir()`.
  - `secrets.rs`: define `pub const KEYSTORE_PREFIX: &str = "keystore:v1:";`, `pub trait SecretCipher: Send + Sync { fn prefix(&self) -> &'static str; fn encrypt(&self, plain: &[u8]) -> Result<Vec<u8>, String>; fn decrypt(&self, cipher: &[u8]) -> Result<Vec<u8>, String>; }`, `register_platform_secret_cipher(Arc<dyn SecretCipher>)` using `RwLock<Option<Arc<dyn SecretCipher>>>` (so tests can swap cleanly under a test mutex or scoped helper), update `protect_secret` and `unprotect_secret_checked` to dispatch registered cipher before Windows DPAPI fallback, fail closed on unknown/corrupt `keystore:v1:` payloads.
  - `models.rs`: add `#[serde(default)] pub spotify_user_id: String` and `#[serde(default)] pub youtube_account_id: String` to `AccountSettings` (include in `Debug` impl cleanly), plus `OfflineDownloadRecord`, `PlayHistoryEntry`, `ListeningStatsSummary`, and `pub fn qualifies_as_play(listened_ms: u64, duration_ms: u64) -> bool`.
  - `theme.rs`: add `#[serde(default)] pub editorial_mode: bool`, `#[serde(default = "default_font_family")] pub font_family: String`, `#[serde(default = "default_player_layout")] pub player_layout: String`, sanitize allowed values, add `ThemeConfig::liquid_glass()` using `BackgroundMode::NativeAcrylic`, `acrylic_enabled: true`, `player_layout: "liquid_glass"`.
  - `db.rs`: add `LibraryDb::open_in_dir(dir: &Path)` opening `dir.join("ferric.db")`, add `CREATE TABLE IF NOT EXISTS offline_downloads (...)` and `CREATE TABLE IF NOT EXISTS play_history (...)` + `idx_play_history_played_at` in `migrate()`, implement CRUD & streak aggregation.
  - `collections.rs`: move `PlaylistSortOrder`, `PlaylistSourceFilter`, `DetailSortOrder`, `deduplicate_tracks_in_order`, `track_primary_link`, `track_youtube_link`, `format_collection_tracklist`, `format_collection_csv` cleanly from `apps/ferric-desktop/src/state.rs`, and `pub use` them in `state.rs` so Desktop compiles with zero behavior drift.

- [ ] **Step 4: Run tests to verify all pass (including Desktop workspace tests)**
  - Run: `cargo test --workspace`
  - Expected: PASS (all existing 100+ tests + all new Task 1 tests, 0 warnings).

- [ ] **Step 5: Commit**
  - Commit message: `feat(core): add cross-platform data dir, keystore cipher hook, offline/history tables, and shared helpers`

---

### Task 2: `crates/ferric-audio` & `crates/ferric-providers` Android Readiness (`oboe-rs`, `ContentOpener`, `SetDucking`, Supervisor, IPv6, Stable Account IDs)

**Files:**
- Modify: `Cargo.toml` (add workspace dependencies `oboe = "0.6.1"`)
- Modify: `crates/ferric-audio/Cargo.toml` (gate `cpal` to `#[cfg(not(target_os = "android"))]` and `oboe` to `#[cfg(target_os = "android")]`)
- Create: `crates/ferric-audio/src/content.rs`
- Create: `crates/ferric-audio/src/android_oboe.rs`
- Modify: `crates/ferric-audio/src/lib.rs`
- Modify: `crates/ferric-audio/src/engine.rs`
- Modify: `crates/ferric-providers/src/lib.rs:37-53`
- Modify: `crates/ferric-providers/src/spotify_web.rs:675-695, 885-915`
- Modify: `crates/ferric-providers/src/oauth.rs:1030-1145`
- Modify: `crates/ferric-providers/src/ytmusic.rs:3758-3845`

**Interfaces:**
- Consumes: `ferric_core::eq::EqConfig`, `ferric_core::models::PlaybackDspSettings`
- Produces:
  - `ferric_audio::content::{ContentOpener, register_content_opener(Arc<dyn ContentOpener>), open_audio_file(path: &Path) -> std::io::Result<std::fs::File>}`
  - `ferric_audio::engine::EngineCommand::SetDucking(bool)`
  - `ferric_audio::engine::AudioEngine::start_supervised(initial_eq: EqConfig, preferred_device: Option<String>) -> AudioResult<Self>` (spawns internal supervisor thread polling recovery & idle suspend every 500 ms while alive)
  - `ferric_providers::spotify_web::parse_profile_user_id(val: &serde_json::Value) -> String`
  - `ferric_providers::oauth::CloudSyncReport::account_id: String`
  - `ferric_providers::ytmusic::YtmAccountLibrary::account_id: String` & `find_youtube_account_id(root: &serde_json::Value) -> Option<String>`

- [ ] **Step 1: Write failing tests in `ferric-audio` and `ferric-providers`**
  - `ferric-audio/src/engine.rs`:
    - `ducking_attenuates_independently_and_restores_exact_volume` (verifies `DuckingState` smooth ramp: `0.25x` when ducked, preserves `target_volume` across volume changes while ducked, restores `1.0x` cleanly on `SetDucking(false)`).
    - `content_opener_decodes_content_uri_in_headless_engine` (registers a test `ContentOpener` mapping `"content://ferric-test/demo.wav"` to a real temp WAV file, plays it in `AudioEngine::start_headless`, asserts `is_playing && duration_ms > 0`).
  - `ferric-providers`:
    - `parse_profile_user_id_extracts_stable_username_before_display_name`
    - `find_youtube_account_id_prefers_email_or_channel_handle_over_display_name`

- [ ] **Step 2: Run tests to verify they fail**
  - Run: `cargo test -p ferric-audio -p ferric-providers`
  - Expected: FAIL (missing `content`, `SetDucking`, `parse_profile_user_id`, `find_youtube_account_id`).

- [ ] **Step 3: Implement Task 2 changes**
  - `crates/ferric-audio/src/content.rs`: define `ContentOpener` trait (`fn open_file(&self, uri: &str) -> std::io::Result<std::fs::File>`), `RwLock<Option<Arc<dyn ContentOpener>>>`, `open_audio_file(path: &Path)`: if `path.to_str()` starts with `"content://"`, call registered opener (fail with `NotFound`/`Unsupported` if unregistered); otherwise `File::open(path)`.
  - `crates/ferric-audio/src/android_oboe.rs`: `#[cfg(target_os = "android")]` wrapper struct `AndroidOutputStream` holding `oboe::AudioStreamAsync<oboe::Output, OboeCallback>` opened with `Usage::Media`, `ContentType::Music`, `SharingMode::Shared`, `PerformanceMode::None`, `Stereo`, `f32`, pulling wait-free epoch-tagged stereo frames from `rtrb::Consumer`, incrementing `cb_heartbeat`, and marking `device_lost` on `on_error_after_close`.
  - `crates/ferric-audio/src/engine.rs`:
    - Abstract `PlatformOutputStream` (`cpal::Stream` on non-Android, `android_oboe::AndroidOutputStream` on Android).
    - Wire `ActiveDeck::open` to `crate::content::open_audio_file(path)?`.
    - Add `EngineCommand::SetDucking(bool)` + `DuckingRamp` (`target_duck = 0.25` when ducked, `1.0` normal, smoothed per block multiplied cleanly into `effective_target_volume = target_volume * duck_gain` without ever mutating `target_volume`).
    - Add internal supervisor thread in `AudioEngine` (enabled via `start_supervised`, cleanly signaled via `Arc<AtomicBool>` and joined in `Drop`, using `Mutex<Option<PlatformOutputStream>>` so Desktop's existing `poll_device_recovery(&mut self)` continues working seamlessly).
  - `crates/ferric-providers/src/lib.rs`: gate `.ip_family(ureq::config::IpFamily::Ipv4Only)` to `#[cfg(target_os = "windows")]` inside `shared_http_agent_ipv4()`.
  - `crates/ferric-providers/src/spotify_web.rs`, `oauth.rs`, `ytmusic.rs`: add `account_id: String` to `CloudSyncReport` and `YtmAccountLibrary`, extract stable `spotify_user_id` (`data.me.profile.username` / `/v1/me` `id`) and `youtube_account_id` (email / `channelHandle` / `UC...` browseId from `account_menu`).

- [ ] **Step 4: Run tests and verify**
  - Run: `cargo test --workspace`
  - Expected: PASS (all workspace tests green, 0 clippy/compiler warnings).

- [ ] **Step 5: Commit**
  - Commit message: `feat(audio,providers): add Android oboe backend, ContentOpener FD bridge, SetDucking, IPv6 support, and stable account IDs`

---

### Task 3: `crates/ferric-controller` Headless Playback, Library & Persistence Orchestrator

**Files:**
- Modify: `Cargo.toml` (add `"crates/ferric-controller"` to workspace members)
- Create: `crates/ferric-controller/Cargo.toml`
- Create: `crates/ferric-controller/src/lib.rs`
- Create: `crates/ferric-controller/src/events.rs`
- Create: `crates/ferric-controller/src/playback.rs`

**Interfaces:**
- Consumes: `ferric-core`, `ferric-audio`, `ferric-providers`
- Produces:
  - `ControllerConfig { data_dir: PathBuf, cache_dir: PathBuf, headless_audio: bool }`
  - `ControllerEventSink: Send + Sync` (`on_telemetry`, `on_now_playing`, `on_queue_changed`, `on_library_changed`, `on_toast`)
  - `FerricController::new(cfg: ControllerConfig, sink: Arc<dyn ControllerEventSink>) -> ControllerResult<Arc<Self>>`
  - Methods: `ensure_audio_started()`, `suspend_audio_if_idle()`, `ensure_demo_seed()`, `play_track(track_id: &str)`, `play_queue(tracks: Vec<Track>, start_index: usize, context: String)`, `toggle_play_pause()`, `play()`, `pause()`, `next()`, `prev()`, `seek_ms(pos_ms: u64)`, `set_volume(vol: f32)`, `set_ducking(duck: bool)`, `toggle_shuffle() -> bool`, `cycle_repeat() -> RepeatMode`, `telemetry() -> TelemetrySnapshot`, `current_track() -> Option<Track>`, `queue_snapshot() -> (Option<Track>, Vec<Track>)`, `list_songs(query: &str) -> Vec<Track>`, `list_playlists() -> Vec<Playlist>`, `get_theme() -> ThemeConfig`, `set_theme(ThemeConfig)`, `get_eq() -> EqConfig`, `set_eq(EqConfig)`, `get_dsp() -> PlaybackDspSettings`, `set_dsp(PlaybackDspSettings)`, `listening_stats() -> ListeningStatsSummary`, `trim_memory()`, `shutdown()`

- [ ] **Step 1: Write failing tests in `crates/ferric-controller/src/lib.rs`**
  - `controller_starts_lazily_without_audio_engine_and_loads_demo_tracks`
  - `controller_plays_queue_advances_tracks_and_emits_events`
  - `controller_records_qualified_play_history_and_persists_session_on_shutdown`

- [ ] **Step 2: Run tests to verify failure**
  - Run: `cargo test -p ferric-controller`
  - Expected: FAIL (crate/methods not yet implemented).

- [ ] **Step 3: Implement `crates/ferric-controller`**
  - Clean lock-ordered internal state (`Mutex<ControllerState>` for queue/settings/db; separate `Mutex<Option<AudioEngine>>` for lazy audio startup so `AudioEngine` is NEVER created in `new()`, only on `ensure_audio_started()` or playback command).
  - Background monitor thread ticks every 200 ms while playing (and sleeps on condvar when paused/idle for 0% idle battery drain), emitting telemetry, detecting `track_finished_seq` auto-transitions, pre-queuing Deck B, and recording qualified `play_history` rows (`>= 30s` or `>= 50%`).
  - Clean `shutdown()` flushes `SessionResumeState`, stops monitor thread, and drops `AudioEngine`.

- [ ] **Step 4: Run tests and verify**
  - Run: `cargo test --workspace`
  - Expected: PASS (0 warnings, all controller + workspace tests green).

- [ ] **Step 5: Commit**
  - Commit message: `feat(controller): add headless FerricController crate for mobile playback and persistence`

---

### Task 4: `crates/ferric-mobile-ffi` UniFFI Proc-Macro Crate, Android JNI Context Init & Bindgen CLI

**Files:**
- Modify: `Cargo.toml` (add `"crates/ferric-mobile-ffi"`, workspace dep `uniffi = "0.33.0"`)
- Create: `crates/ferric-mobile-ffi/Cargo.toml` (`crate-type = ["cdylib", "rlib"]`, `uniffi = { workspace = true, features = ["cli"] }`, Android-only `jni = "0.22.4"`, `ndk-context = "0.1.1"`)
- Create: `crates/ferric-mobile-ffi/src/lib.rs`
- Create: `crates/ferric-mobile-ffi/src/android_jni.rs`
- Create: `crates/ferric-mobile-ffi/src/bin/uniffi_bindgen.rs`

**Interfaces:**
- Consumes: `ferric-controller`, `ferric-core`, `ferric-audio`
- Produces:
  - UniFFI namespace `ferric_mobile_ffi` (`uniffi::setup_scaffolding!()`)
  - Exported UniFFI traits: `FfiSecretCipher`, `FfiContentOpener`, `FfiEventSink`
  - Exported UniFFI object: `FerricMobileCore` (`new`, `ensure_audio_started`, `suspend_audio_if_idle`, `seed_demo_if_empty`, `list_songs`, `list_playlists`, `play_track`, `play_queue`, `toggle_play_pause`, `play`, `pause`, `next`, `prev`, `seek_ms`, `set_volume`, `set_ducking`, `toggle_shuffle`, `cycle_repeat`, `get_telemetry`, `get_current_track`, `get_up_next`, `get_theme`, `set_theme`, `get_eq`, `set_eq`, `get_dsp`, `set_dsp`, `get_listening_stats`, `trim_memory`, `shutdown`)
  - Exported Android JNI function: `Java_dev_livelydesk_ferric_core_NativeBridge_initAndroidContext` using `#[unsafe(no_mangle)]` and documented `// SAFETY:` invariants
  - Binary `uniffi-bindgen` (`cargo run -p ferric-mobile-ffi --bin uniffi-bindgen -- generate --library ... --language kotlin --out-dir ...`)

- [ ] **Step 1: Write failing Rust integration tests for `FerricMobileCore`**
  - In `crates/ferric-mobile-ffi/tests/ffi_smoke.rs`: test `FerricMobileCore::new_headless_for_test` with mock `FfiSecretCipher`, `FfiContentOpener`, and `FfiEventSink`, seeding demo tracks, calling `play_track`, checking `get_telemetry().is_playing == true`, updating `FfiThemeConfig`, and verifying clean `shutdown()`.

- [ ] **Step 2: Run tests to verify failure**
  - Run: `cargo test -p ferric-mobile-ffi`
  - Expected: FAIL (crate not implemented).

- [ ] **Step 3: Implement `crates/ferric-mobile-ffi`**
  - Implement all UniFFI records (`#[derive(uniffi::Record)]`), enums (`#[derive(uniffi::Enum)]`), error (`#[derive(Debug, thiserror::Error, uniffi::Error)]`), callback interfaces (`#[uniffi::export(callback_interface)]`), and `#[uniffi::export]` methods on `FerricMobileCore`.
  - Register `FfiSecretCipher` adapter into `ferric_core::secrets::register_platform_secret_cipher` and `FfiContentOpener` adapter into `ferric_audio::content::register_content_opener` (using `std::os::fd::FromRawFd` on Unix/Android with documented `// SAFETY:` comment validating `fd >= 0`) BEFORE opening `FerricController` / `LibraryDb`.
  - Implement `android_jni.rs` (`#[cfg(target_os = "android")]`) initializing `ndk_context` once via `Once` guard with `GlobalRef` leaked intentionally for process lifetime (documented `// SAFETY:`).

- [ ] **Step 4: Run tests & verify UniFFI Kotlin binding generation on host library**
  - Run: `cargo test -p ferric-mobile-ffi` and `cargo build -p ferric-mobile-ffi` and run `uniffi-bindgen generate --library target/debug/ferric_mobile_ffi.dll --language kotlin --out-dir target/uniffi-test`
  - Expected: PASS, generates valid `uniffi/ferric_mobile_ffi/ferric_mobile_ffi.kt` with zero errors.

- [ ] **Step 5: Commit**
  - Commit message: `feat(mobile-ffi): add UniFFI proc-macro bridge, JNI ndk_context init, and bindgen CLI`

---

### Task 5: `apps/ferric-android` Native Kotlin Jetpack Compose Skeleton, Keystore, `ContentOpener`, `FerricPlaybackService` & Dual-ABI Gradle Project

**Files:**
- Create: `apps/ferric-android/settings.gradle.kts`
- Create: `apps/ferric-android/build.gradle.kts`
- Create: `apps/ferric-android/gradle.properties`
- Create: `apps/ferric-android/gradle/libs.versions.toml`
- Create: `apps/ferric-android/gradle/wrapper/gradle-wrapper.properties` + `gradlew` / `gradlew.bat` / `gradle-wrapper.jar`
- Create: `apps/ferric-android/app/build.gradle.kts` (dual ABI splits `arm64-v8a`, `armeabi-v7a`, `x86_64`, `isUniversalApk = true`; R8 release minify/shrink; `JNA 5.19.1@aar`)
- Create: `apps/ferric-android/app/proguard-rules.pro` (JNA + `uniffi.**` + callback keep rules)
- Create: `apps/ferric-android/app/src/main/AndroidManifest.xml` (`allowBackup="false"`, `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_MEDIA_PLAYBACK`, `POST_NOTIFICATIONS`, `INTERNET`, `ACCESS_NETWORK_STATE`, `FerricPlaybackService`)
- Create: `apps/ferric-android/app/src/main/java/dev/livelydesk/ferric/FerricApp.kt`
- Create: `apps/ferric-android/app/src/main/java/dev/livelydesk/ferric/core/NativeBridge.kt`
- Create: `apps/ferric-android/app/src/main/java/dev/livelydesk/ferric/core/KeystoreVault.kt`
- Create: `apps/ferric-android/app/src/main/java/dev/livelydesk/ferric/core/AndroidContentOpener.kt`
- Create: `apps/ferric-android/app/src/main/java/dev/livelydesk/ferric/core/CoreStateStore.kt`
- Create: `apps/ferric-android/app/src/main/java/dev/livelydesk/ferric/playback/FerricPlaybackService.kt`
- Create: `apps/ferric-android/app/src/main/java/dev/livelydesk/ferric/ui/theme/FerricTheme.kt`
- Create: `apps/ferric-android/app/src/main/java/dev/livelydesk/ferric/MainActivity.kt`
- Create: `apps/ferric-android/app/src/main/res/` (strings, launcher adaptive icon vector, themes.xml)
- Create: `apps/ferric-android/app/src/test/java/dev/livelydesk/ferric/KeystoreEnvelopeTest.kt`
- Create: `apps/ferric-android/app/src/test/java/dev/livelydesk/ferric/PlaybackStateReducerTest.kt`

**Interfaces:**
- Consumes: generated `uniffi.ferric_mobile_ffi.*` bindings + `jniLibs/{arm64-v8a,armeabi-v7a,x86_64}/libferric_mobile_ffi.so`
- Produces: runnable Ferric Android app (M1 AMOLED Black Home with demo/quick-picks playback via Rust Oboe engine, working MediaSession notification + AudioFocus duck/pause + Noisy unplug pause + System Output Switcher button, hardware Keystore vault, dual release APKs).

- [ ] **Step 1: Ensure Android SDK + NDK r28+ are installed via `android` CLI**
  - Run `android sdk install` for `platforms/android-35`, `build-tools/35.0.0`, `platform-tools`, and latest `ndk` (`28.x` / `29.x`).
  - Verify `ANDROID_HOME` and NDK path exist.

- [ ] **Step 2: Scaffold Gradle project, `libs.versions.toml`, `build.gradle.kts` (dual ABI + R8), `proguard-rules.pro`, and unit tests**
  - Write JVM unit tests first (`KeystoreEnvelopeTest` verifying 12-byte IV framing & tamper/short-payload rejection; `PlaybackStateReducerTest` verifying StateFlow telemetry/track/queue updates).

- [ ] **Step 3: Implement Kotlin core bridge, KeystoreVault, ContentOpener, `FerricPlaybackService`, and M1 Compose UI**
  - `FerricApp.kt`: calls `NativeBridge.initAndroidContext(this)` and holds lazy `FerricRuntime` (NEVER starts audio engine in `onCreate()`).
  - `KeystoreVault.kt`: implements `FfiSecretCipher` backed by `AndroidKeyStore` AES-256-GCM (`12-byte IV || ciphertext+tag`), failing closed on any Keystore error.
  - `AndroidContentOpener.kt`: implements `FfiContentOpener` validating `content://` scheme and detaching read-only `ParcelFileDescriptor`.
  - `FerricPlaybackService.kt`: `MediaSessionService` wrapping a custom `SimpleBasePlayer` (Media3) bridged directly to `FerricMobileCore` so Android lockscreen, Bluetooth AVRCP, notification controls, `AudioFocusRequest` (`DUCK`/`PAUSE`/`LOSS`), `ACTION_AUDIO_BECOMING_NOISY`, and Android System Output Switcher (`MediaRouter` / `SystemOutputSwitcher`) control the real Rust `oboe` audio engine cleanly!
  - `MainActivity.kt` & `FerricTheme.kt`: AMOLED Black (`#000000` canvas, `#09090D` surface, `#E86A2C` Burnt Orange, `WCAG AA >= 4.5:1`, `>= 48dp` touch targets), displaying M1 Home (`Quick Picks` playable demo tracks), live MiniPlayer + Play/Pause/Prev/Next/Seek + System Output Switcher button.

- [ ] **Step 4: Run Gradle unit tests & lint**
  - Run: `.\gradlew testDebugUnitTest lintDebug` (via `subst` drive if needed)
  - Expected: PASS (0 test failures, 0 lint errors).

- [ ] **Step 5: Commit**
  - Commit message: `feat(android): scaffold Jetpack Compose app, KeystoreVault, MediaSessionService, and dual-ABI Gradle config`

---

### Task 6: Space-Safe Build Script (`scripts/build-android.ps1`), GitHub Actions CI (`.github/workflows/ci-android.yml`) & End-to-End Dual-APK Verification

**Files:**
- Create: `scripts/build-android.ps1`
- Create: `.github/workflows/ci-android.yml`
- Modify: `.gitignore` (ignore `apps/ferric-android/app/src/main/jniLibs/`, `apps/ferric-android/local.properties`, `apps/ferric-android/.gradle/`, `apps/ferric-android/**/build/`)

**Interfaces:**
- Consumes: `crates/ferric-mobile-ffi`, `apps/ferric-android`
- Produces:
  - `dist/android/ferric-v<ver>-android-arm64-v8a.apk`
  - `dist/android/ferric-v<ver>-android-universal.apk`
  - `dist/android/SHA256SUMS.txt`

- [ ] **Step 1: Write `scripts/build-android.ps1` and `.github/workflows/ci-android.yml`**
  - `scripts/build-android.ps1`: maps temp `subst` drive (matching `scripts/build-release.ps1` so space in `music player` never breaks NDK/CMake/Gradle), runs `cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64` with `RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"`, generates UniFFI Kotlin bindings via `uniffi-bindgen`, runs Gradle `testDebugUnitTest assembleRelease`, copies & renames both APKs into `dist/android/`, verifies `LOAD` 16 KB (`0x4000`) alignment via NDK `llvm-readelf`, and writes `SHA256SUMS.txt`.
  - `.github/workflows/ci-android.yml`: Linux `ubuntu-latest` workflow checking Rust tests, 3-ABI `cargo-ndk`, UniFFI bindings, Gradle unit tests + lint + release dual APKs + 16 KB alignment check + artifact upload.

- [ ] **Step 2: Run full end-to-end M1 verification**
  - Run: `.\scripts\build-android.ps1`
  - Verify BOTH APKs exist, check their sizes (`arm64-v8a` vs `universal`), verify `llvm-readelf -l` shows `Align 0x4000` (16 KB) on `libferric_mobile_ffi.so` inside the APK, and run `cargo test --workspace` + `cargo check -p ferric-desktop` confirming zero regressions on Desktop `v0.1.2`.

- [ ] **Step 3: Commit & push Milestone M1**
  - Commit message: `build(android): add space-safe dual-APK build script and GitHub Actions Android CI`
  - Run fresh verification before claiming M1 complete, then inform the user they can plug in their phone if they want to live-test M1 APK via `adb` (or proceed straight to M2).
