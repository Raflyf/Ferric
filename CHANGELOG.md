# Changelog

Release history for Ferric. Each version records verified shipped changes; earlier installers keep their original behavior.

## Downloads and Installation

- **Included:** Per-user Windows x64 MSI installer (`ferricsetup-vX.Y.Z.msi`), portable ZIP archive (`Ferric-vX.Y.Z-windows-x86_64.zip`), `SHA256SUMS.txt`, product icons, embedded Inter v4.1 + Makepad desktop fonts, Start Menu shortcut, and license notices.
- **Per-user setup:** Installs into `%LOCALAPPDATA%\Programs\Ferric` without administrator privileges while keeping user data in `%LOCALAPPDATA%\LivelyDesk\Ferric\data\ferric.db`.
- **Verified updates:** Built-in `Check for Updates` and optional startup update prompt verify `SHA256SUMS.txt` from official GitHub Releases before launching Windows Installer.

---

## Unreleased (Pre-Release `v0.1.0` Stabilization)

### Added

- **Frameless Native Rust 3-Pane Desktop Shell (`apps/ferric-desktop`):**
  - DirectX 11 Makepad 1.0 workspace with far-left brand bar, inline top search bar (no distracting modal popup), collapsible left navigation sidebar, collapsible right `Queue / Lyrics` sidebar, 6-tab Settings hub (`Accounts`, `Playback`, `Equalizer`, `Appearance`, `Storage`, `Connect`), and a centered stacked 2-row `74px` bottom player bar.
  - Native Windows 11 single-instance guard (`Local\FerricMusicPlayerSingleInstance_v1`) that restores and focuses the existing window instead of opening duplicate windows.
  - Mouse-wheel volume control, keyboard shortcuts (`Space`, `Left/Right`, `Shift+Left/Right`, `Ctrl+Up/Down`, `M`, `S`, `R`, `L`, `F`/`F11`, `Q`/`Ctrl+Q`, `Ctrl+L`, `Ctrl+K`, `Ctrl+F`, `Ctrl+,`, `Ctrl+E`, `Alt+Left/Right`, `Ctrl -/+/0`, `Esc`), and full track/playlist CRUD + context menu (`Play Next`, `Add to Queue`, `Start Radio`, `Like/Unlike`, `Add to Playlist`, `Remove`).
- **True Per-Pixel Native Windows 11 Acrylic & Theme Studio:**
  - Vendored `vendor/makepad-platform` DirectComposition composition swapchain (`WS_EX_NOREDIRECTIONBITMAP` + `CreateSwapChainForComposition` + `DXGI_ALPHA_MODE_PREMULTIPLIED` + `DWMSBT_TRANSIENTWINDOW`) so window translucency blurs the desktop behind Ferric while keeping all text, icons, controls, and artwork 100% opaque.
  - 4 theme presets (`AMOLED Black` default, `Native Acrylic`, `Obsidian Studio`, `Warm Cream Light`), unified light frosted glass cards/inputs under Acrylic, adjustable opacity (`18%–94%`), live adaptive artwork accent tinting, and `.ferric-theme.json` import/export.
- **Dual-Deck Seamless Automix & 10-Band Parametric EQ (`ferric-audio`):**
  - Dual-deck `Deck A + Deck B` pure-Rust decoder (`symphonia 0.6.1`, pure-Rust `symphonia-opus 0.1.3`, `rubato 5.1.0`, lock-free `rtrb 0.4.1`, `cpal 0.18.2` WASAPI).
  - `0–12 s` crossfade with `140 Hz` complementary **Bass-Swap Automix**, **S-Curve Velvet**, and **Equal-Power** curves, **Gapless Album Lock** (`0 ms` on consecutive local album tracks), intro/outro silence cuepoint trimming, Hermite micro-fade on Play/Pause/Seek, ReplayGain, Playback Speed (`0.75x–1.5x`), Mono downmix, Sleep Timer, and stereo demo suite (`--demo`).
  - Live GPU 10-band Biquad EQ response curve, **24 built-in AutoEq headphone profiles**, and **Equalizer APO `.txt`** import/export.
- **Smart Multi-Artist Radio Queue, Discovery & Clean Library (`ferric-providers` + `ferric-core`):**
  - Dynamic YouTube Music Home discovery (mood chips, Quick Picks, 8 scrollable shelves with continuation loading), rich search, optional Google WebView2 sign-in (with Guest Mode playlist isolation), OpenSubsonic / Navidrome sync, public playlist URL import, and `.m3u`/`.m3u8`/`.csv` import/export.
  - Locale-coherent, multi-artist **Smart Radio Queue** with second-hop `RDAMVM` artist expansion, strict max-1-track-per-artist cap, same-song cover/karaoke/pitch-shift/SEO spam filtering, and rotating **Refresh Queue** button.
  - Clean `Songs` library (`Sort: Recent` default, persisted only on play/like/playlist/local scan) with automatic one-time migration (`migration_pruned_ephemeral_v2`) pruning old ephemeral discovery/stream-cache bloat.
  - Expanded 20-card (`5x4`) paginated grids for `Albums`, `Artists`, and `Playlists` with ghost-spacer alignment and clean album grouping.
- **Interactive Fullscreen Lyrics & In-App HD Video Clip:**
  - 4-stage synced lyrics waterfall (sidecar `.lrc` -> embedded tags -> `LRCLIB` -> `Lyrics.ovh`), 60 FPS spring scrolling, click-to-seek, `-0.5s`/`+0.5s` sync offset, sub-pixel karaoke sweep, and 21-tap silky blurred aurora backdrop with dark velvet reading scrim.
  - Docked WebView2 in-app HD music video toggle inside Fullscreen Lyrics mode, synchronized to WASAPI audio playback.
- **Ferric Connect Foundation (`ferric-sync`) & Sub-50 MB Memory Discipline:**
  - Pure-Rust ISO/IEC 18004 QR pairing renderer, non-blocking UDP LAN peer discovery (`port 43821`), `postcard 1.2` binary frames, and `ActiveSpeakerLease` state tracking.
  - 128-texture LRU artwork cache with idempotent widget setters (zero playback card flicker), halted hidden redraws, and Win32 working-set trimming keeping RAM around `44 MB` active streaming / `7.6 MB` idle / `7.3 MB` minimized.

### Fixed

- Eliminated ghost Windows 11 DWM caption buttons (`—`, `❐`, `×`) under Native Acrylic by stripping `WS_CAPTION`, retaining full frameless `WM_NCCALCSIZE`, and intercepting caption `WM_NCHITTEST` codes.
- Fixed maximized viewport clipping on right/bottom edges (`get_inner_size()` + `rcWork` clamping) and eliminated rapid UI card flashing during playback.
- Removed redundant mini-mode chrome button and fixed search box text vertical alignment.

### Verification

- Automated `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked`, WiX MSI build, portable ZIP packaging, and 3/3 `SHA256SUMS.txt` verification via GitHub Actions on `windows-latest`.
- Live Windows 11 visual & telemetry verification on installed `ferric.exe` (`SHADER_ERROR_COUNT=0`, `WS_OPEN=7.6–44.1 MB`, `WS_MIN=7.3 MB`).
