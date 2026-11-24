# Changelog

Release history for Ferric. Each version records its own changes; earlier installers keep their original behavior.

## Downloads and installation

- **Included:** Windows x64 MSI installer (`ferricsetup-vX.Y.Z.msi`), portable ZIP archive (`Ferric-vX.Y.Z-windows-x86_64.zip`), product icons, Makepad desktop font resources, Start Menu shortcut, and license notices.
- **Per-user setup:** Installs into `%LOCALAPPDATA%\Programs\Ferric` without administrator privileges.
- **Verified updates:** Built-in Check for Updates and startup update popup verify `SHA256SUMS.txt` against GitHub Releases before launching the Windows Installer.

---

## Unreleased

---

## v0.1.0

### Added

- **Clean Spotify & Sonora-Inspired Native Desktop Shell (`ferric.exe`):** Spacious frameless DirectX 11 Makepad workspace with native Windows 11 titlebar drag & Aero Snap, working custom Minimize / Maximize / Restore / Exit / Compact Mini-Player (`Ctrl+M`) controls, calm left library navigation, collapsible right Queue & Connect drawer, `Ctrl+K` Command Palette, and a clean 4-tab Settings studio.
- **Dual-Deck Seamless Fade-In & Bass-Swap Automix (`ferric-audio`):** Pre-buffered `Deck A + Deck B` audio engine (`Symphonia 0.6` + pure-Rust Opus + `Rubato` cubic resampler + `cpal` WASAPI) delivering `0-12 s` gapless crossfade, `140 Hz` complementary Bass-Swap Automix crossover, `240 ms` Hermite velvet fade-in on Play/Pause/Seek, Gapless Album Lock (`0 ms` on consecutive album tracks), intro/outro silence cuepoint trimming, ReplayGain, Playback Speed (`0.75x-1.5x`), Sleep Timer, and built-in stereo WAV demo suite.
- **Official Cloud OAuth 2.0 PKCE Login & Subsonic Sync (`ferric-providers`):** Loopback HTTP callback (`http://127.0.0.1:43824/callback`) + RFC 7636 S256 PKCE sign-in for Spotify Web API v1 and Google / YouTube Data API v3 (`If-None-Match` ETag `304` + `categoryId=10` music filter), OpenSubsonic / Navidrome sync, `.m3u`/`.m3u8`/Exportify `.csv` playlist import, and ISRC/title-artist local lossless matching.
- **Multi-Provider Free Time-Synced Lyrics & Now Playing Stage:** 4-stage zero-key lyrics waterfall (sidecar `.lrc` -> embedded audio tags -> `LRCLIB` exact & fuzzy search -> `Lyrics.ovh` fallback with auto-pacing), `-0.5s`/`+0.5s` sync offset, Motion Canvas + 16-bar live spectrum / Cover Card toggle, `Watch Video Clip` launcher, and full Audio Signal Path readout.
- **10-Band Parametric EQ, 24 AutoEq Headphone Profiles & Ferric Connect:** Live GPU RBJ Biquad frequency response graph, 24 built-in Harman/AutoEq headphone & IEM profiles, Equalizer APO `.txt` import/export, pure-Rust ISO/IEC 18004 QR code pairing renderer, and non-blocking UDP LAN peer discovery (`port 43821`).
- **Liquid Glass Appearance Studio & Verified Windows MSI Auto-Updater:** Pure OLED Pitch Black (`#000000`) default with live toggleable optical Liquid Glass GPU SDF shader (`IOR`, `Dispersion`, `Rim`, `Radius`, live mini-player preview, `.ferric-theme.json` import/export), plus WiX v3.14 per-user `ferricsetup-v0.1.0.msi`, `Check for Updates`, and SHA-256 verified startup update popup.

### Verification

- Automated formatting (`cargo fmt --check`), Clippy (`-D warnings`), workspace unit tests, release compilation, and `msi.dll` metadata verification run on `windows-latest` via GitHub Actions.
