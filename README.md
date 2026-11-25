<p align="center">
  <img src="assets/brand/ferric.png" width="128" height="128" alt="Ferric Oxide Cassette Icon" />
</p>

# Ferric (`v0.1.0` Pre-Release)

Ultra-lightweight, 100% native Rust desktop Hi-Fi music player for **Windows 10/11** (`x86_64`), with shared core crates designed for future Android expansion.

Part of the **LivelyDesk** visual product family (`LivelyDesk` · `WallpaperLive` · `TransparentBar` · `Ferric`), built with **AMOLED Black** (`#000000`) default, **True Per-Pixel Native Windows 11 Acrylic** via DirectComposition, crisp **Inter v4.1** typography, and sub-50 MB live RAM usage.

## Status

Windows Desktop `v0.1.0` is **feature-complete and in final bug-fix / polish stabilization** prior to tagging the first release. Measured on installed Windows 11 MSI builds:

- **RAM (Idle Open):** `~7.6 MB` Working Set
- **RAM (Active Online Streaming + Artwork + Lyrics):** `~44.1 MB` Working Set
- **RAM (Minimized / System Tray):** `~7.3 MB` Working Set
- **GPU (Idle / Hidden):** `0%` (event-driven Makepad DX11 redraws halted when hidden/idle)

## Core Capabilities

- **Native Rust GPU UI (`makepad-widgets 1.0.0`, DX11 + vendored DirectComposition Acrylic):**
  - Frameless 3-pane desktop layout (`36px` top chrome, collapsible `168px` left sidebar, 9 content pages, collapsible `260px` right Queue/History & Lyrics panel, centered 2-row `74px` bottom player bar).
  - 4 theme presets (`AMOLED Black`, `Native Acrylic`, `Obsidian Studio`, `Warm Cream Light`), adaptive artwork accent tinting, `.ferric-theme.json` import/export, and `65%–140%` live UI zoom (`Ctrl - / + / 0`).
  - True per-pixel Windows 11 Acrylic (`WS_EX_NOREDIRECTIONBITMAP` + `CreateSwapChainForComposition` + `DWMSBT_TRANSIENTWINDOW`) with unified frosted glass cards/inputs while text, icons, and covers stay 100% opaque.
  - 20-card (`5x4`) visual galleries for `Albums`, `Artists`, and `Playlists`; clean `Songs` library sorted by `Recent` by default; `Recents` history; inline top search bar; and full track context menu / playlist CRUD.
- **Pure-Rust Dual-Deck Audiophile Engine (`ferric-audio`):**
  - Decodes FLAC, MP3, AAC/M4A, ALAC, Ogg Vorbis, Opus, WAV, and AIFF (`symphonia 0.6.1` + pure-Rust `symphonia-opus 0.1.3`).
  - Lock-free `rtrb 0.4.1` ring buffer -> `cpal 0.18.2` WASAPI output with `rubato 5.1.0` resampling.
  - Dual-deck `Deck A + Deck B` pre-buffered transitions (`0–12 s` crossfade, `140 Hz` complementary **Bass-Swap Automix**, **S-Curve Velvet**, **Equal-Power**, **Gapless Album Lock**, intro/outro silence trimming, Hermite micro-fade on play/pause/seek, ReplayGain, `0.75x–1.5x` speed, mono downmix, Sleep Timer).
  - Live **10-band Parametric EQ** (`biquad 0.6.0`), **24 built-in AutoEq headphone profiles**, and **Equalizer APO `.txt`** import/export.
- **Discovery, Smart Multi-Artist Radio, Lyrics & Video (`ferric-providers`):**
  - Local folder scanning (`lofty 0.25.5` + `walkdir`), `.m3u`/`.m3u8`/`.csv` playlist import/export, OpenSubsonic / Navidrome sync, public playlist URL import, and optional Google WebView2 sign-in.
  - Locale-aware, multi-artist **Smart Radio Queue** with second-hop artist expansion, strict 1-track-per-artist diversity cap, cover/karaoke/pitch-shift filtering, and rotating Refresh Queue.
  - 4-stage synced lyrics waterfall (sidecar `.lrc` -> embedded tags -> `LRCLIB` -> `Lyrics.ovh`), spring-animated karaoke sweep in Fullscreen Lyrics mode with 21-tap blurred velvet aurora backdrop, and docked in-app WebView2 HD Music Video toggle.
- **Windows Integration & Verified Delivery:**
  - Single-instance guard (`Local\FerricMusicPlayerSingleInstance_v1`) that activates/restores the existing window instead of spawning duplicates.
  - Native Windows 11 SMTC media flyout & hardware keys (`souvlaki 0.9.3`), system tray minimize/close-to-tray, scroll-wheel volume adjustment, per-user WiX MSI installer (`%LOCALAPPDATA%\Programs\Ferric`), and SHA-256 verified update checker.

## Workspace Layout

```text
ferric/
├── Cargo.toml                  # Workspace manifest (v0.1.0, Edition 2024, LTO thin + strip)
├── apps/
│   └── ferric-desktop/         # Windows Makepad 1.0 UI, Win32 shell/tray/Acrylic, SMTC, WebView2 video/login
├── crates/
│   ├── ferric-core/            # SQLite WAL store, PlayQueue, ThemeConfig, EqConfig, domain models
│   ├── ferric-audio/           # Dual-deck Symphonia + Opus + Rubato + 10-band Biquad EQ + rtrb + cpal
│   ├── ferric-providers/       # Local scanner, YTM Home/Search/Radio/Stream, Lyrics, AutoEq, Subsonic, Updater
│   └── ferric-sync/            # Binary postcard frames, UDP LAN beacon (43821), pure-Rust QR pairing, Active Speaker Lease
├── vendor/
│   └── makepad-platform/       # Vendored patch for DirectComposition premultiplied-alpha Acrylic & exact maximized geometry
├── scripts/
│   └── Installer.ps1           # WiX v3 per-user MSI packaging script
├── assets/brand/               # ferric.svg, ferric.png, ferric.ico, wordmarks, monochrome glyph
└── docs/                       # PRD.md, DESIGN.md, SYNC_AND_AUDIO_SPEC.md, RESEARCH_AND_RE_FINDINGS.md, AGENT_HANDOFF.md
```

## Build Locally (Windows MSVC)

```powershell
rustup default stable-x86_64-pc-windows-msvc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo build --release -p ferric-desktop --locked
```

## Automated CI & Packaging

Every code push to `main` runs `.github/workflows/ci-desktop.yml` (`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace --locked`, release build, WiX MSI packaging, portable ZIP packaging, and `SHA256SUMS.txt` generation) without creating a git release tag until manually pushed via `v*` tag (`.github/workflows/release.yml`).
