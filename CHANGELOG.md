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

- **Native Rust Windows Desktop Player (`ferric.exe`):** Built on Makepad GPU rendering (DirectX 11), Symphonia 0.6 + pure-Rust Opus decoding, Rubato cubic resampler, 10-band parametric Biquad EQ with Equalizer APO `.txt` import/export, wait-free `rtrb` SPSC ring buffer, and `cpal` WASAPI output.
- **Official Ferric Oxide Cassette Brand Identity:** Custom tilted copper-beveled cassette icon embedded in `ferric.exe`, Start Menu shortcut, Add/Remove Programs, and WiX MSI installer banner/dialog artwork.
- **Open Theme Studio + Liquid Glass Toggle (ON/OFF):** Defaults to strict 3-color Pure OLED Pitch Black (`#000000`, `#0A0A0A`, `#222222`, `#F3EEE3`, `#D9541E`) with live toggleable optical Liquid Glass GPU SDF shader, IOR/Dispersion/Specular Rim/Corner Radius sliders, 4 optical presets, WCAG contrast guard, and `.ferric-theme.json` export/import.
- **Library, Lyrics & Windows Integration:** SQLite WAL persistent library, recursive local folder scanner (`lofty`), virtualized `PortalList` track browser, smart anti-back-to-back artist shuffle, sidecar `.lrc` + `lrclib.net` synced lyrics, Windows 11 SMTC hardware media keys, BYO OAuth 2.0 PKCE account settings, and Ferric Connect pairing URI protocol.
- **Windows MSI Installer & Verified In-App Auto-Updater:** WiX v3.14 per-user x64 installer (`ferricsetup-v0.1.0.msi`) with major-upgrade support, automatic startup update check with floating update popup (`Update Now`, `What's New`, `Later`, `Don't check on startup`), manual `Check for Updates`, and SHA-256 verified installer download + launch.

### Verification

- Automated formatting (`cargo fmt --check`), Clippy (`-D warnings`), workspace unit tests, release compilation, and `msi.dll` metadata verification run on `windows-latest` via GitHub Actions.
