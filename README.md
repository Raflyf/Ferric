<p align="center">
  <img src="assets/brand/ferric.png" width="92" height="92" alt="Ferric logo">
</p>

<h1 align="center">Ferric</h1>
<p align="center">
  <strong>Native music player for Windows and Android powered by a shared Rust audio core.</strong><br>
  Local lossless playback, Spotify (Free &amp; Premium) &amp; YouTube Music sync, synced lyrics, and cross-device handoff.
</p>

<p align="center">
  <a href="https://github.com/Raflyf/Ferric/releases/tag/v0.1.3"><img src="https://img.shields.io/github/v/release/Raflyf/Ferric?label=desktop&color=E86A2C" alt="Desktop release"></a>
  <a href="https://github.com/Raflyf/Ferric/releases/tag/android-v0.1.0"><img src="https://img.shields.io/badge/android-v0.1.0-3DDC84" alt="Android release"></a>
  <a href="https://github.com/Raflyf/Ferric/actions/workflows/ci-desktop.yml"><img src="https://github.com/Raflyf/Ferric/actions/workflows/ci-desktop.yml/badge.svg" alt="Desktop CI"></a>
  <a href="https://github.com/Raflyf/Ferric/actions/workflows/ci-android.yml"><img src="https://github.com/Raflyf/Ferric/actions/workflows/ci-android.yml/badge.svg" alt="Android CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-586575" alt="MIT license"></a>
</p>

<p align="center">
  <a href="#download"><strong>Download</strong></a>
  &nbsp;&middot;&nbsp; <a href="#minimum-requirements">Requirements</a>
  &nbsp;&middot;&nbsp; <a href="#core-highlights">Highlights</a>
  &nbsp;&middot;&nbsp; <a href="#documentation">Documentation</a>
  &nbsp;&middot;&nbsp; <a href="CHANGELOG.md">Changelog</a>
  &nbsp;&middot;&nbsp; <a href="https://livelydesk.vercel.app/ferric">Website</a>
</p>

Ferric pairs a native DirectX 11 Windows client (`makepad-widgets`) and a Jetpack Compose Android client on top of a shared Rust engine for decoding, DSP, library indexing, streaming providers, and device synchronization.

<p align="center">
  <img src="assets/screenshots/ferric-home-v0.1.1.png" width="1100" alt="Ferric Windows desktop workspace showing synced playlists, Quick Picks, Queue, and player bar">
</p>

*Album artwork and track metadata shown in previews belong to their respective owners and are not bundled with Ferric.*

## Core Highlights

- **Shared Rust Audio Engine:** Dual-deck gapless playback over `WASAPI` (Windows) and `Oboe` (Android), crossfade transitions, 10-band parametric EQ, and support for `FLAC`, `MP3`, `AAC`/`M4A`, `ALAC`, `Opus`, `Ogg Vorbis`, `WAV`, and `AIFF`.
- **Accounts & Offline Library:** Two-way library and playlist sync for Spotify (Free & Premium) and YouTube Music, OpenSubsonic/Navidrome server support, and fast local folder scanning backed by SQLite.
- **Lyrics & Video:** Time-synced lyrics (`LRCLIB`, embedded tags, local `.lrc`) with synchronized HD music video playback.
- **Ferric Connect:** Live queue sharing and playback handoff across Windows and Android over LAN or an encrypted cloud relay.

## Download

| Platform | Package | Notes |
| :--- | :--- | :--- |
| Windows Installer | [ferricsetup-v0.1.3.msi](<https://github.com/Raflyf/Ferric/releases/download/v0.1.3/ferricsetup-v0.1.3.msi>) | Per-user installer (no administrator rights required) |
| Windows Portable | [Ferric-v0.1.3-windows-x86_64.zip](<https://github.com/Raflyf/Ferric/releases/download/v0.1.3/Ferric-v0.1.3-windows-x86_64.zip>) | Standalone portable archive (`SHA256SUMS.txt` on release) |
| Android APK (`arm64-v8a`) | [ferric-v0.1.0-android-arm64-v8a.apk](<https://github.com/Raflyf/Ferric/releases/download/android-v0.1.0/ferric-v0.1.0-android-arm64-v8a.apk>) | Recommended for modern 64-bit ARM devices (`15.85 MB`) |
| Android APK (Universal) | [ferric-v0.1.0-android-universal.apk](<https://github.com/Raflyf/Ferric/releases/download/android-v0.1.0/ferric-v0.1.0-android-universal.apk>) | Includes `arm64-v8a`, `armeabi-v7a`, and `x86_64` (`34.6 MB`) |

Browse [all GitHub releases](<https://github.com/Raflyf/Ferric/releases>) or report bugs on [GitHub Issues](<https://github.com/Raflyf/Ferric/issues>).

## Minimum Requirements

| Component | Windows Desktop | Android |
| :--- | :--- | :--- |
| **OS Version** | Windows 10 or Windows 11 (Native Acrylic works best on Windows 11) | Android 9.0 (API 28) or higher |
| **CPU Architecture** | 64-bit (`x86_64`) | `arm64-v8a`, `armeabi-v7a`, or `x86_64` |
| **Graphics / UI** | DirectX 11 compatible GPU | Android GPU supporting OpenGL ES / Compose |
| **Audio Output** | Windows Audio (`WASAPI`) | Android Audio (`Oboe` / `AudioTrack`) |
| **Account Sign-In** | Microsoft Edge WebView2 Runtime (built into Windows 11) | Android System WebView (built-in) |
| **Privileges** | Standard user account (per-user install, no admin required) | Standard APK installation permission |
| **Network** | Optional for local files; required for streaming, lyrics, and cloud relay | Optional for local files; required for streaming, lyrics, and cloud relay |

## Privacy & Signing Notes

- **Local-First & Zero Analytics:** Ferric sends no telemetry or usage tracking to developer servers. Local library databases, caches, and encrypted credentials stay on-device (encrypted via Windows DPAPI on Desktop and Android Keystore on Android).
- **Signing Status:** Official binaries are built transparently via [GitHub Actions](<.github/workflows/release.yml>) with `SHA256SUMS.txt` verification manifests. Android APKs are release-signed; Windows `v0.1.3` installers and ZIPs are currently unsigned (Windows SmartScreen may display an unknown-publisher prompt on first launch).

## Documentation

Full documentation (user guide, keyboard shortcuts, Ferric Connect setup, DSP tuning, and build-from-source guides) is currently being prepared and will follow in an upcoming update. Release history is tracked in [CHANGELOG.md](CHANGELOG.md).

## Support & License

Part of [LivelyDesk](<https://livelydesk.vercel.app>). Optional project support: [Buy me a coffee](<https://yapp.ink/raflyf>) &middot; [Saweria](<https://saweria.co/BuyMatcha>).

Licensed under [MIT](LICENSE) &copy; Rafly Firmansyah.
