# Changelog

All notable changes to Ferric are documented in this file.

---

## v0.1.3 (2026-11-28)

### Added

- **Synchronized Connect Queue, Like & Handoff Continuity (Desktop & Android):** Mirrored like state, `Play Next`, `Add to Queue`, queue item removal, shuffled queue windows, and `Clear Up Next` actions bidirectionally over Ferric Connect, preserved live playback position and paused state across rapid speaker handoffs without audio bleed, and added version 1..=10 multi-group Reed-Solomon QR pairing support.

### Fixed

- **Ferric Connect Failover, Shuffle Parity & Desktop UI Sync:** Drained queued `SyncFrame::Goodbye` frames on relay shutdown, aligned `RELAY_PEER_TTL` (`35s`) above background heartbeat (`15s`), fixed remote shuffle/repeat mutations and unshuffled `JumpToQueueIndex` regressions, preserved paused-seek Connect snapshots, synced Desktop cards/buttons (`FerricCard`, `FerricBtn`, `FerricChipBtn`) to active Solid/Acrylic/Adaptive theme tokens, and fixed bottom-bar Copy Link clipping.

### Security & Hardening

- Scrubbed `AccountSettings`/`SubsonicCredentials` secrets on drop, enforced PE hardening flags and stripped host path references in release binaries, and separated Android APK update discovery (`android-v*` / `.apk` assets) from Desktop installers.

---

## Android v0.1.0 (Initial Android Release) (2026-11-28)

### Added

- **Overhanging Optical Convex Magnifier Pill & Reconfigurable Glass Widgets:** Added an optical 3D convex magnifying lens bottom navigation pill with balanced edge-to-center crystal refraction and prism dispersion, plus `FerricWidgetConfigActivity` (`android:configure`, reconfigurable) supporting `Glass`, `Clear glass`, `Dark`, `Light`, `Cover colour`, dynamic "From the cover" accent extraction across chromatic and monochrome artwork, refined cassette fallback cover rendering, persistent last-played track display after stop or app close, spacious evenly distributed transport controls paired with true Small/Medium/Large cover, typography, and control sizing, and an amber dual-tone flame streak badge pill.
- **Standalone About Sheet & Theme-Synced Startup Update Popup:** Separated `About` from `Settings` into a dedicated Liquid Glass sheet with Android-scoped GitHub release checks, `Notify on app launch` toggle, `Raflyf/Ferric` & `livelydesk.vercel.app/ferric` links, changelog/issue shortcuts, creator support links, a compact `You` hub, and an in-window theme-synced startup update dialog (`FerricUpdateDialogOverlay`) with per-version skip persistence.

### Fixed & Hardening

- **Rich `MediaStyle` Notification, Service Lifecycle & Player Polish:** Bound `MediaSession` directly in `FerricPlaybackService`, eliminated `ForegroundServiceDidNotStartInTimeException` races and bare non-MediaStyle overwrites, added shared `fullPlayerBackdrop` capture, fixed Warm Cream Light contrast and `ShareCardSheet` storage permissions on Android 9, stripped debug/verbose logging in R8 release builds, and enforced release keystore checks in CI.
- **60 FPS Playback, Smooth 720p/1080p Video Sync, Responsive EQ Sliders & Full-Surface Optical Liquid Glass:** Replaced dual-layer downsampled backdrop blurring with a single-pass padded `RenderEffect` optical chain (`ColorMatrixColorFilter` saturation -> 1:1 `BlurEffect` -> full-surface AGSL convex lens refraction + smooth square/sheet diagonal normals + symmetrical prism dispersion + specular rim) matched to iOS 26 crystal optics across the Navigation Pill, sliding magnifier bubble, compact orb, Mini-Player, Snackbar, Bottom Sheets, dialogs, and Full Player chrome without partial transparency flicker; added bounded 2 MiB ranged HTTP video streaming (`ChunkedVideoDataSource`) and micro-rate clock steering to eliminate 720p/1080p CDN throttling and keyframe re-seek stutters; added precomputed zero-allocation `EqCurveMath` evaluation and conflict-free immediate vertical fader tracking in `EqualizerSheet`; eliminated `FerricPlaybackService.onStartCommand` self-retrigger loops; isolated `MiniProgressBar` GPU transforms and `0.25x` static `AuroraBackdrop` caching; bounded `sample_library_pool_bounded` and avoided redundant string/telemetry atomic reads in Rust; and aligned simultaneous non-bouncing `FERRIC_MOTION_MS` tab transitions.

---

## v0.1.2 (2026-11-27)

### Added

- **Two-Way Cloud Account Playlist & Like Sync (Spotify & YouTube Music):** Creating playlists (`+ Create`, `Save Playlist`, `Save Queue`, Track Menu `+ Create`), adding or removing tracks, renaming or deleting playlists, deduplicating Spotify playlists, and toggling Liked Songs now synchronize automatically back to your signed-in Spotify or YouTube Music account so changes persist across sign-outs, uninstalls, and other devices. Existing local playlists can also be pushed to your signed-in account with one click via **Sync to Account**.
- **WASAPI Audio Output Device Selector + Live Hot-Switch:** Choose `System Default` or switch live between headphones, speakers, USB DACs, and Bluetooth outputs inside **Settings > Playback** (`Refresh`, cycle button, and device chips) without restarting playback or losing song position, with automatic fallback to `System Default` if a device is disconnected.
- **Discord Rich Presence Listen Button:** Added an optional **Show Listen Button** toggle inside **Settings > Integrations** (enabled by default, automatically omitted under **Hide Track Details**) that attaches a `Listen on Spotify` or `Listen on YouTube Music` link button verified against a strict HTTPS allowlist.
- **Right Sidebar Queue Tools & Clickable Bottom-Bar Artist:** Save the active song and upcoming queue straight to a playlist (`Save Queue`) or copy the queue tracklist (`Copy Queue`), and click the artist name in the bottom player bar to open the artist detail page or search.
- **Playlist Detail Sort, Deduplicate & Filtered Playback:** Playlist Detail **Play**, **Shuffle**, and **+ Queue** now follow active text filtering and sort order, alongside dedicated **Sort** (`Original`, `Title`, `Artist`, `Duration`) and O(N) **Deduplicate** controls.
- **Expanded Powerbar (`Ctrl+P`) & Shortcut Documentation:** Added `Copy Song Link`, `Copy Playlist Link`, `Save Queue`, and `About & Updates` quick actions plus `Ctrl+Shift+C` shortcut documentation in **Settings > Storage**.

### Improved

- **Workspace-Wide Zero-Allocation Search, Filter & Sort:** Added a shared `ferric_core::text` module (`find_ascii_icase`, `contains_ascii_icase`, `starts_with_ascii_icase`, `ends_with_ascii_icase`, `cmp_ascii_icase`) and replaced temporary `.to_ascii_lowercase()` heap allocations across library filters, instant search, playlist/album/artist sorting, AutoEq lookup, lyrics scoring, Spotify/YouTube parsers, updater host checks, and WASAPI device sorting, paired with O(1) `HashMap` lookup for `Recent` sorting.
- **Debounced Session State Persistence & Flush Guarantees:** Coalesced rapid volume slider, wheel, mute, shuffle, and repeat writes into a 400 ms debounce window while guaranteeing immediate SQLite flush on minimize, hide-to-tray, updater launch, tray quit, and app exit (`Drop`).
- **Paused Fullscreen Lyrics Idle CPU/GPU Reduction:** Stopped 60 FPS hover redraws on paused ambient backdrops, flattened the visualizer once when pausing, and skipped redundant seek slider/lyrics UI updates when paused with unchanged timestamps.

---

## v0.1.1 (2026-11-27)

### Added

- **Settings > About Tab:** Dedicated About page displaying Ferric's version, SHA-256 verified Software Updates controls (`Check for Updates`, startup auto-check toggle, release notes), project links (LivelyDesk website, GitHub, Changelog, Issues, MIT License), optional support links (Buy Me a Coffee, Saweria), engine/privacy summary, and one-click `Copy System Info`.
- **Unified Spotify Sign-In:** Consolidated Spotify integration in `Settings > Accounts` into a single **Spotify Account (Free & Premium)** sign-in card and unified `Spotify` playlist filter, removing the redundant Premium-only OAuth card.

### Improved

- **Sub-100 MB Memory Architecture:** Re-architected artwork caching, GPU texture lifecycles, SQLite memory limits, and shared HTTP/TLS connection pools so Ferric stays under 100 MB Private Working Set across idle, active streaming playback, Fullscreen Lyrics, and system tray residency.
- **Zero Per-Track RAM Growth:** Eliminated memory creep across song transitions (both while open and minimized to the system tray) by clearing hidden cover textures, trimming swapchain buffers when hidden, and compacting Makepad font/glyph caches cleanly.
- **Home, Quick Picks & Recommended Stations Refresh:** Replaced static ordering with weighted multi-locale rotation and recent-impression exclusion so clicking Refresh deals genuinely fresh songs, albums, and radio stations.
- **Rust Hot-Path & Allocation Audit:** Eliminated per-frame heap allocations in track list rendering, audio decoding, locale detection, and radio filtering; streamed Subsonic responses without intermediate JSON DOM trees; and enabled stricter workspace Clippy performance lints.

### Fixed

- Fixed SMTC cover staging reading full artwork files into UI-thread memory and added exponential backoff on systems where Windows SMTC activation is unavailable.
- Fixed Discord Rich Presence timestamp boundary jitter that caused redundant IPC roundtrips, skipped snapshot building when RPC is disabled, and bounded the background IPC channel.
- Fixed concurrent WebView2 script execution during Spotify login, removed excess CDP network traffic, and added session/command deadlines so closed login windows never lock future sign-in attempts.
- Fixed docked Video Clip command bursts during resize/morph animations by coalescing geometry and playback sync updates, and cleared stale window handles on early close.
- Fixed taskbar thumbnail icon handle cleanup via RAII `Drop` and eliminated unnecessary UTF-16 heap allocations in Win32 window and tray hooks.
- Fixed MSI uninstall/upgrade auto-closing running or tray-minimized Ferric instances cleanly (`WM_ENDSESSION` handling + pre-`InstallValidate` process-tree termination), expanded full uninstall cleanup to remove custom install paths and temporary browser profiles (`%TEMP%\ferric-spotify-browser-*`), and updated the installer banner art to reflect `AMOLED + Native Acrylic`.
- Fixed initial desktop window placement on first launch so Ferric opens centered inside the active monitor's usable work area (with DPI-aware clamping above the taskbar) instead of cascading downward from `CW_USEDEFAULT`.

---

## v0.1.0 (2026-11-27)

### Added

- **Native Rust Desktop Shell:** Frameless DirectX 11 desktop interface with collapsible Library and Queue/Lyrics sidebars, live inline search, command palette (`Ctrl+K`), keyboard shortcuts, mouse-wheel volume, and a centered bottom player bar with one-click Playback Speed (`0.75x`–`1.5x`) and Codec badges.
- **Windows 11 Acrylic & Theme Studio:** Four built-in themes (`AMOLED Black` default, `Native Acrylic`, `Obsidian Studio`, `Warm Cream Light`), customizable Acrylic opacity and glass tint with a GPU HSV color picker, Adaptive Artwork Accent and Adaptive Acrylic Tint with WCAG AA contrast, and `.ferric-theme.json` import/export.
- **Pure-Rust Dual-Deck Audio Engine:** Plays local lossless and compressed formats (`FLAC`, `MP3`, `AAC`/`M4A`, `ALAC`, `Ogg Vorbis`, `Opus`, `WAV`, `AIFF`) alongside online streams with gapless album playback, auto-crossfade (`0–12 s`, Bass-Swap Automix, S-Curve Velvet, Equal-Power), ReplayGain, Mono downmix, Sleep Timer, and a 10-band parametric EQ with 24 built-in AutoEq profiles and Equalizer APO `.txt` import/export.
- **Synced Lyrics, Fullscreen Lyrics & Video Clips:** Four-stage synced lyrics waterfall (`.lrc` sidecar, embedded tags, `LRCLIB`, `Lyrics.ovh`) with timing offset controls, Fullscreen Lyrics ambient aurora stage, docked HD music video clip morph, and zero-bloat OS fallback typography for Japanese, Korean, Chinese, and Thai scripts.
- **Unified Library, Discovery & Integrations:** YouTube Music Home, Search, Smart Radio Queue, and Recommended Stations; optional Google, Spotify (Web & OAuth PKCE), and OpenSubsonic/Navidrome sync; public playlist URL and `.m3u`/`.csv` import/export; native Discord Rich Presence; and Ferric Connect LAN QR discovery.
- **Native Windows 11 Experience:** Full Windows SMTC integration (cover thumbnails, live timeline position, seek support for Quick Settings and third-party overlays), taskbar thumbnail media buttons (`Previous` / `Play-Pause` / `Next`), device-theme-aware dark/light system tray menu, built-in SHA-256 verified auto-updater, and clean root-level per-user MSI uninstallation.

### Security

- **Encrypted Local Credentials:** Tokens, cookies, and server passwords are encrypted at rest with Windows DPAPI (`CryptProtectData`), zeroized in memory on sign-out, and backed by SQLite `PRAGMA secure_delete = ON`.
- **Hardened Release Builds & Sandboxing:** Compiled with LTO, Control Flow Guard (`/GUARD:CF`), CET shadow-stack compatibility (`/CETCOMPAT`), strict System32 DLL search policy (`/DEPENDENTLOADFLAG:0x800`), reproducible builds (`/BREPRO`), strict WebView2 sandboxing and URL allowlists, SSRF protection, and TOCTOU-safe SHA-256 installer verification.

### Fixed

- Fixed Windows 11 Acrylic background flashes across startup, focus changes, and taskbar restore, and removed ghost DWM caption buttons under frameless Acrylic mode.
- Fixed Warm Cream Light bottom bar contrast, clipped Settings labels, active multi-line lyric highlighting, blocky thumbnail downscaling, and right-clicking the system tray raising the main window.
- Fixed `Repeat One` manual skip handling, mouse-wheel volume persistence, SMTC timeline sync for third-party overlays, and signed-in YouTube Music stream fallbacks.
