<p align="center">
  <img src="assets/brand/ferric.png" width="128" height="128" alt="Ferric Oxide Cassette Icon" />
</p>

# Ferric (`v0.1.0` Pre-Release)

Ultra-lightweight, 100% native Rust desktop Hi-Fi music player for **Windows 10/11** (`x86_64`), with shared core crates designed for future Android expansion.

Part of the **LivelyDesk** visual product family (`LivelyDesk` · `WallpaperLive` · `TransparentBar` · `Ferric`), shipping with **AMOLED Black** (`#000000`, Burnt Orange `#E86A2C`) by default, optional **True Per-Pixel Native Windows 11 Acrylic** via DirectComposition, crisp **Inter v4.1** typography with zero-bloat OS international font fallbacks, hardened PE security, and sub-50 MB live RAM usage.

## Status

Windows Desktop `v0.1.0` is **feature-complete, security-hardened, and live-verified** prior to tagging the first release. Measured on installed Windows 11 MSI builds (`5c81b11`):

- **RAM (Idle Open):** `~7.6 MB` Working Set
- **RAM (Active Online Streaming + Artwork + Lyrics):** `~41.3–44.1 MB` Working Set
- **RAM (Minimized / System Tray):** `~7.3 MB` Working Set
- **CPU (Settled / Post-Close):** `~0.5%`
- **GPU (Idle / Hidden):** `0%` (event-driven Makepad DX11 redraws halted when hidden/idle)
- **Hardened Binary Size:** `~14.0 MB` (`ferric.exe`, fat LTO + stripped symbols + CFG + CET)

## Core Capabilities

- **Native Rust GPU UI (`makepad-widgets 1.0.0`, DX11 + vendored DirectComposition Acrylic):**
  - Frameless 3-pane desktop layout (`36px` top chrome, collapsible `168px` left sidebar, 9 content pages, collapsible `260px` right Queue/History & Lyrics panel, zoom-safe centered 2-row `74px` bottom player bar).
  - 4 theme presets (`AMOLED Black` fresh-install default with `acrylic_enabled: false`, `Native Acrylic`, `Obsidian Studio`, `Warm Cream Light`), **GPU HSV color picker**, customizable Acrylic tint color (`acrylic_tint_hex`), **WCAG 2.1 adaptive foreground/icon contrast** (`AdaptivePalette`), HSV-preserving adaptive artwork accent tinting, `.ferric-theme.json` import/export, and `65%–140%` live UI zoom (`Ctrl - / + / 0`).
  - Cold-start safe **True Per-Pixel Windows 11 Acrylic** (`WS_EX_NOREDIRECTIONBITMAP` + `CreateSwapChainForComposition` + `DWMSBT_TRANSIENTWINDOW` initialized prior to `CreateTargetForHwnd` and `ShowWindow`, with `WM_NCACTIVATE`/`WM_ACTIVATE` forwarding) so window translucency works immediately on fresh launch while text, icons, and covers stay 100% opaque.
  - Zero-bloat **runtime OS international font fallbacks** (Japanese `YuGothM.ttc`, Korean `malgunsl.ttf`, Chinese `msyh.ttc`, Thai `LeelawUI.ttf`) shared once via `Rc<Font>` across all UI font families (`0` bytes added to `ferric.exe`/MSI).
  - 20-card (`5x4`) visual galleries for `Albums`, `Artists`, and `Playlists`; clean `Songs` library sorted by `Recent` by default; `Recents` history; live debounced inline top search bar with clear button; and full track context menu / playlist CRUD.
- **Pure-Rust Dual-Deck Audiophile Engine (`ferric-audio`):**
  - Decodes FLAC, MP3, AAC/M4A, ALAC, Ogg Vorbis, Opus, WAV, and AIFF (`symphonia 0.6.1` + pure-Rust `symphonia-opus 0.1.3`).
  - Lock-free `rtrb 0.4.1` ring buffer -> `cpal 0.18.2` WASAPI output with `rubato 5.1.0` resampling.
  - Dual-deck `Deck A + Deck B` pre-buffered transitions (`0–12 s` auto-only crossfade with immediate manual Next/Prev handoff, `140 Hz` complementary **Bass-Swap Automix**, **S-Curve Velvet**, **Equal-Power**, **Gapless Album Lock**, intro/outro silence trimming, Hermite micro-fade on play/pause/seek, ReplayGain, `0.75x–1.5x` speed, mono downmix, Sleep Timer).
  - Live **10-band Parametric EQ** (`biquad 0.6.0`), **24 built-in AutoEq headphone profiles**, and **Equalizer APO `.txt`** import/export.
- **Discovery, Smart Multi-Artist Radio, Synced Lyrics & HD Video (`ferric-providers`):**
  - Local folder scanning (`lofty 0.25.5` + `walkdir`), `.m3u`/`.m3u8`/`.csv` playlist import/export, OpenSubsonic / Navidrome sync, public playlist/album/track URL import (works out-of-the-box without login), official Spotify / Google YouTube OAuth 2.0 PKCE read-only metadata sync (`oauth.rs`, no DRM bypass), and optional Google WebView2 sign-in.
  - Taste-driven personalized Home discovery and locale-aware, multi-artist **Smart Radio Queue** with second-hop artist expansion, strict 1-track-per-artist diversity cap, cover/karaoke/pitch-shift filtering, and rotating Refresh Queue.
  - 4-stage synced lyrics waterfall (sidecar `.lrc` -> embedded tags -> `LRCLIB` -> `Lyrics.ovh`) with **clean original LRCLIB selection** (penalizing polluted multi-language fan-sub columns and mojibake while auto-healing stale SQLite `lyrics_cache` entries).
  - **60 FPS Fullscreen Lyrics Mode:** 2-phase aurora dissolve + `42px` glide open/close animation (zero shell ghosting), uniform full-line active illumination across wrapped lines, 21-tap liquid aurora backdrop, animated GPU SDF Repeat/Volume controls, **60 FPS Cover <-> Video Clip morph**, official music video resolver, and docked WebView2 HD video theater with a single crisp DPI-exact `16px` rounded border.
- **Windows Integration, Discord RPC & Security Hardening:**
  - Single-instance guard (`Local\FerricMusicPlayerSingleInstance_v1`), deterministic AppUserModelID (`Raflyf.Ferric`), clean `Ferric` Task Manager naming, dynamic now-playing window title, **Windows Taskbar Thumbnail Media Buttons** (`Prev` / `Play-Pause` / `Next`), Windows 11 SMTC media flyout & hardware keys (`souvlaki 0.9.3`), system tray (`Close to Tray`), and scroll-wheel volume adjustment.
  - Built-in zero-dependency **Discord Rich Presence** (`\\.\pipe\discord-ipc-0..9`, Activity Type `Listening`, built-in Ferric Discord App ID, cassette icon badge, clean album metadata handling).
  - **Defense-in-Depth & Anti-RE Hardening:** Windows DPAPI encryption (`dpapi:v1:`) for stored account tokens/cookies/passwords, OS CSPRNG (`BCryptGenRandom`), Win32 DLL hijacking & process mitigations (`SetDllDirectoryW`, `SetDefaultDllDirectories`, `HeapSetInformation`, `SetProcessMitigationPolicy`), strict WebView2 sandboxing (CSP, DevTools/popups/permissions disabled, URL allowlists), TOCTOU-safe SHA-256 MSI re-verification prior to `CreateProcessW` (`msiexec.exe`), and reproducible hardened PE builds (`GUARD_CF`, `CETCOMPAT`, `DYNAMICBASE`, `HIGHENTROPYVA`, `NXCOMPAT`, `BREPRO`, zero PDB or host path leaks verified by `scripts/verify_pe_hardening.py`).

## Workspace Layout

```text
ferric/
├── Cargo.toml                  # Workspace manifest (v0.1.0, Edition 2024, fat LTO, strip symbols, panic=abort)
├── apps/
│   └── ferric-desktop/         # Makepad 1.0 UI, Win32 shell/hardening/tray/Acrylic/taskbar, SMTC, Discord RPC, WebView2 video/login, build.rs
├── crates/
│   ├── ferric-core/            # SQLite WAL store, PlayQueue, ThemeConfig + AdaptivePalette, EqConfig, DPAPI secrets, models
│   ├── ferric-audio/           # Dual-deck Symphonia + Opus + Rubato + 10-band Biquad EQ + rtrb + cpal
│   ├── ferric-providers/       # Local scanner, YTM Home/Search/Radio/Video/Stream, Lyrics + clean filter, OAuth PKCE, Public Sync, AutoEq, Subsonic, Updater
│   └── ferric-sync/            # Binary postcard frames, UDP LAN beacon (43821), pure-Rust QR pairing, Active Speaker Lease
├── vendor/
│   └── makepad-platform/       # Vendored patches: DirectComposition cold-start Acrylic, WM_NCACTIVATE forwarding, exact maximize geometry, OS CJK/Thai virtual fonts
├── installer/                  # WiX v3 Ferric.wxs & ONLINE-NOTICE.txt
├── scripts/
│   ├── Installer.ps1           # WiX v3 per-user MSI packaging script
│   └── verify_pe_hardening.py  # Post-build PE security flags & zero path/PDB leak verification gate
├── assets/brand/               # ferric.svg, ferric.png, ferric.ico, wordmarks, monochrome glyph, WiX bitmaps
└── docs/                       # PRD.md, DESIGN.md, SYNC_AND_AUDIO_SPEC.md, RESEARCH_AND_RE_FINDINGS.md, AGENT_HANDOFF.md
```

## Build Locally (Windows MSVC)

```powershell
rustup default stable-x86_64-pc-windows-msvc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo build --release -p ferric-desktop --locked
python .\scripts\verify_pe_hardening.py .\target\release\ferric.exe
```

## Automated CI & Packaging

Every code push to `main` runs `.github/workflows/ci-desktop.yml`: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace --locked`, PowerShell syntax validation, isolated reproducible hardened release build (`CARGO_HOME=C:\c`, `subst S:`, `/GUARD:CF`, `/CETCOMPAT`, `/BREPRO`, `/DEBUG:NONE`, `--remap-path-prefix`), `scripts/verify_pe_hardening.py`, WiX MSI packaging, portable ZIP packaging, and `SHA256SUMS.txt` generation. GitHub Releases are only published when a `v*` tag is manually pushed (`.github/workflows/release.yml`).
