# Changelog

All notable changes to Ferric are documented in this file.

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
