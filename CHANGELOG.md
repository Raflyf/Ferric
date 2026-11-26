# Changelog

Release history for Ferric. Each version records verified shipped changes; earlier installers keep their original behavior.

## Downloads and Installation

- **Included:** Per-user Windows x64 MSI installer (`ferricsetup-vX.Y.Z.msi`), portable ZIP archive (`Ferric-vX.Y.Z-windows-x86_64.zip`), `SHA256SUMS.txt`, product icons, embedded Inter v4.1 + Makepad desktop fonts, Start Menu shortcut (`AppUserModelID: Raflyf.Ferric`), and license notices.
- **Per-user setup:** Installs into `%LOCALAPPDATA%\Programs\Ferric` without administrator privileges while keeping user data in `%LOCALAPPDATA%\LivelyDesk\Ferric\data\ferric.db`.
- **Verified updates:** Built-in `Check for Updates` and optional startup update prompt verify `SHA256SUMS.txt` from official GitHub Releases and re-verify the SHA-256 digest immediately before launching `%SystemRoot%\System32\msiexec.exe`.

---

## Unreleased (Pre-Release `v0.1.0` Stabilization)

### Added

- **Frameless Native Rust 3-Pane Desktop Shell (`apps/ferric-desktop`):**
  - DirectX 11 Makepad 1.0 workspace with far-left brand bar, live debounced inline top search bar with clear button, unified recent search chips, collapsible left navigation sidebar, collapsible right `Queue / Lyrics` sidebar (synchronized toggle state), 6-tab Settings hub (`Accounts`, `Playback`, `Equalizer`, `Appearance`, `Storage`, `Connect`), theme-matched toast notifications, modal outside-click dismissal with background pointer blocking, and a zoom-safe centered 2-row `74px` bottom player bar.
  - Native Windows 11 single-instance guard (`Local\FerricMusicPlayerSingleInstance_v1`), deterministic AppUserModelID (`Raflyf.Ferric`), clean `Ferric` Task Manager & MuiCache naming, dynamic now-playing window title, **Windows Taskbar Thumbnail Media Buttons** (`ITaskbarList3`: Previous / Play-Pause / Next), and crisp HD vector icons.
  - Mouse-wheel volume control, keyboard shortcuts (`Space`, `Left/Right`, `Shift+Left/Right`, `Ctrl+Up/Down`, `M`, `S`, `R`, `L`, `F`/`F11`, `Q`/`Ctrl+Q`, `Ctrl+L`, `Ctrl+K`, `Ctrl+F`, `Ctrl+,`, `Ctrl+E`, `Alt+Left/Right`, `Ctrl -/+/0`, `Esc`), and full track/playlist CRUD + context menu (`Play Next`, `Add to Queue`, `Start Radio`, `Like/Unlike`, `Add to Playlist`, `Remove`).
- **True Per-Pixel Native Windows 11 Acrylic, Burnt Orange Default & Theme Studio:**
  - Vendored `vendor/makepad-platform` DirectComposition composition swapchain (`WS_EX_NOREDIRECTIONBITMAP` + `CreateSwapChainForComposition` + `DXGI_ALPHA_MODE_PREMULTIPLIED` + `DWMSBT_TRANSIENTWINDOW`) initialized strictly prior to `CreateTargetForHwnd` and `ShowWindow` so cold-start transparency renders reliably on first launch while all text, icons, controls, and artwork stay 100% opaque.
  - 4 theme presets (`AMOLED Black` fresh-install default with `#000000` canvas, `#E86A2C` Ferric Burnt Orange accent, and `acrylic_enabled: false`; `Native Acrylic`; `Obsidian Studio`; `Warm Cream Light`), **GPU HSV Color Picker** (`FerricColorPicker`), customizable Acrylic glass tint color (`acrylic_tint_hex`), adjustable opacity (`18%–94%`), **WCAG 2.1 adaptive foreground/icon contrast** (`AdaptivePalette`), HSV hue/saturation-preserving adaptive artwork accent extraction, and `.ferric-theme.json` import/export.
- **60 FPS Motion Choreography, Synced Lyrics & In-App HD Video Clip:**
  - **2-Phase Fullscreen Lyrics Open & Close Animation:** Phase 1 dissolves the 21-tap liquid aurora backdrop to 100% opaque over the live desktop shell; Phase 2 glides fullscreen controls and lyrics upward `42px` without UI ghosting, reversing smoothly (`~240ms`) on close.
  - **60 FPS Cover <-> Video Clip Morph:** Smooth crossfade and morph between album cover art and docked WebView2 HD music video with a fixed `54px` anti-jump footer, pre-buffered WebView2 `#stage` fade-in, official music video resolution, and a single DPI-exact `16px` rounded border (`CreateRoundRectRgn`).
  - **Animated GPU SDF Stateful Controls:** Hardware-rendered SDF Repeat (`Off`/`All`/`One`) and Volume/Mute buttons, symmetrical Fullscreen Lyrics controls with dedicated volume row, and smooth wave-spring scrolling.
  - **Zero-Bloat International Typography & Clean Lyrics Selection:** Virtual OS fallback font loader in `vendor/makepad-platform/src/os/cx_native.rs` (`YuGothM.ttc`, `malgunsl.ttf`, `msyh.ttc`, `LeelawUI.ttf` shared once via `Rc<Font>`, `0` bytes binary bloat) for Japanese, Korean, Chinese, and Thai glyphs, paired with a 4-stage synced lyrics waterfall (sidecar `.lrc` -> embedded tags -> `LRCLIB` -> `Lyrics.ovh`) that penalizes multi-language fan-sub / mojibake uploads and auto-heals stale `lyrics_cache` entries.
- **Dual-Deck Seamless Automix & 10-Band Parametric EQ (`ferric-audio`):**
  - Dual-deck `Deck A + Deck B` pure-Rust decoder (`symphonia 0.6.1`, pure-Rust `symphonia-opus 0.1.3`, `rubato 5.1.0`, lock-free `rtrb 0.4.1`, `cpal 0.18.2` WASAPI).
  - `0–12 s` auto-only crossfade (manual skips hand off immediately without visual lag) with `140 Hz` complementary **Bass-Swap Automix**, **S-Curve Velvet**, and **Equal-Power** curves, **Gapless Album Lock** (`0 ms` on consecutive local album tracks), intro/outro silence cuepoint trimming, Hermite micro-fade on Play/Pause/Seek, ReplayGain, Playback Speed (`0.75x–1.5x`), Mono downmix, Sleep Timer, and stereo demo suite (`--demo`).
  - Live GPU 10-band Biquad EQ response curve, **24 built-in AutoEq headphone profiles**, and **Equalizer APO `.txt`** import/export.
- **Smart Multi-Artist Radio Queue, Personalized Home & Native Discord RPC:**
  - Taste-driven personalized YouTube Music Home discovery (mood chips, Quick Picks, 8 scrollable shelves with continuation loading), rich search, optional Google WebView2 sign-in (with Guest Mode playlist isolation), OpenSubsonic / Navidrome sync, public playlist URL import, official Spotify / YouTube OAuth 2.0 PKCE read-only metadata sync, and `.m3u`/`.m3u8`/`.csv` import/export.
  - Locale-coherent, multi-artist **Smart Radio Queue** with second-hop `RDAMVM` artist expansion, strict max-1-track-per-artist cap, same-song cover/karaoke/pitch-shift/SEO spam filtering, and rotating **Refresh Queue** button.
  - Clean `Songs` library (`Sort: Recent` default, persisted only on play/like/playlist/local scan) with one-time migration (`migration_pruned_ephemeral_v2`) pruning legacy discovery bloat, plus 20-card (`5x4`) paginated grids for `Albums`, `Artists`, and `Playlists`.
  - Native zero-dependency **Discord Rich Presence** (`apps/ferric-desktop/src/discord_rpc.rs`) over Windows named pipe (`\\.\pipe\discord-ipc-0..9`) with Activity Type `2` (`Listening to Ferric`), built-in Ferric Discord App ID (`1575176866413940806`), cassette icon badge (`small_image`), and clean metadata suppression when album equals title.
- **Ferric Connect Foundation (`ferric-sync`) & Sub-50 MB Memory Discipline:**
  - Pure-Rust ISO/IEC 18004 QR pairing renderer, non-blocking UDP LAN peer discovery (`port 43821`), `postcard 1.2` binary frames, and `ActiveSpeakerLease` state tracking.
  - 128-texture LRU artwork cache with idempotent widget setters, halted hidden redraws, and Win32 working-set trimming keeping RAM around `41–44 MB` active streaming / `7.6 MB` idle / `7.3 MB` minimized.

### Security & Hardening

- **DPAPI Credential Protection & OS CSPRNG (`crates/ferric-core/src/secrets.rs`):** Account OAuth tokens, refresh tokens, cookies, SAPISID, and Subsonic passwords are encrypted at rest with Windows DPAPI (`dpapi:v1:` via `crypt32.dll` + `BCryptGenRandom`), automatically migrating legacy plaintext rows on load.
- **Win32 Anti-Injection & Process Mitigations (`apps/ferric-desktop/src/shell.rs`):** Enforces `SetDllDirectoryW("")`, `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)`, `HeapSetInformation(HEAP_ENABLE_TERMINATION_ON_CORRUPTION)`, and `SetProcessMitigationPolicy` (`PROCESS_IMAGE_LOAD_POLICY` NoRemote/NoLowLabel/PreferSystem32 + `PROCESS_EXTENSION_POINT_DISABLE_POLICY`) on startup.
- **Strict WebView2 Sandboxing (`video_player.rs`, `google_login.rs`):** Applies strict CSP, `no-referrer`, disables DevTools/context menus/host objects/web messages/accelerator keys/autofill/swipe navigation, denies all permissions, blocks popup windows, enforces strict `NavigationStarting` URL allowlists, escapes injected JS via `js_quote_safe`, and rejects path traversal/UNC paths on local `file:///` video URIs.
- **TOCTOU-Safe Updater & Network Guards (`shell.rs`, `updater.rs`, `lyrics.rs`):** Re-verifies canonical updates-directory containment, regular-file type, and cryptographic SHA-256 digest right before invoking `%SystemRoot%\System32\msiexec.exe` via `CreateProcessW`; enforces HTTPS-only allowlisted external URLs, `max_redirects(0)`, and response byte limits.
- **Reproducible Anti-RE PE Hardening (`Cargo.toml`, `.github/workflows/*.yml`, `scripts/verify_pe_hardening.py`):** Compiles releases with `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`, `panic = "abort"`, `/GUARD:CF`, `/CETCOMPAT`, `/DYNAMICBASE`, `/HIGHENTROPYVA`, `/NXCOMPAT`, `/DEBUG:NONE`, `/NOCOFFGRPINFO`, `/BREPRO`, neutral `CARGO_HOME=C:\c`, `subst S:`, `--remap-path-prefix`, and `/d1trimfile`, verified by CI for zero PDB or host path leaks.

### Fixed

- Fixed cold-start Windows 11 DWM Acrylic turning solid black after exit/End Task by seeding `INITIAL_ACRYLIC_BACKDROP` on `Event::Startup`, applying DWM backdrop attributes inside `Win32Window::init` before `CreateTargetForHwnd`/`ShowWindow`, committing DirectComposition before show, forwarding `WM_NCACTIVATE` (`LPARAM(-1)`) and `WM_ACTIVATE` to `DefWindowProcW`, and calling `SWP_FRAMECHANGED` + `DwmFlush()` on toggle.
- Fixed active lyrics line wrapping split by illuminating the entire active wrapped lyric line uniformly across all rows rather than clipping halfway through multi-line verses.
- Fixed Japanese/international lyrics displaying missing glyph boxes or mixed Japanese+Korean community fan-subs via runtime OS fallback fonts and LRCLIB content-pollution scoring + SQLite cache auto-heal.
- Fixed stacked/double rounded borders around the docked Video Clip stage by consolidating clipping to a single DPI-exact `16px` `CreateRoundRectRgn` and removing competing DWM corner frames and outer border strokes.
- Eliminated ghost Windows 11 DWM caption buttons (Minimize, Maximize, Close) under Native Acrylic by stripping `WS_CAPTION`, retaining full frameless `WM_NCCALCSIZE`, and intercepting caption `WM_NCHITTEST` codes.
- Fixed maximized viewport clipping on right/bottom edges (`get_inner_size()` + `rcWork` clamping), icon atlas pointer ABA collisions, rapid UI card flashing during playback, and Subsonic RFC 1321 MD5 test digest expectation.

### Verification

- Automated `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked`, PowerShell parser check, `scripts/verify_pe_hardening.py` (`GUARD_CF=True`, `CETCOMPAT=True`, `BREPRO=True`, zero path/PDB leaks), WiX MSI build, portable ZIP packaging, and 3/3 `SHA256SUMS.txt` verification via GitHub Actions on `windows-latest`.
- Live Windows 11 visual & telemetry verification on installed `ferric.exe` (`5c81b11`, `SHADER_ERROR_COUNT=0`, `APPLY_ERR=0`, `RES_MISS=0`, `WS_OPEN=7.6–44.1 MB`, `WS_CLOSE=41.34 MB`, `WS_MIN=7.3 MB`, `AFTER_CLOSE_CPU=0.52%`, clean healed Japanese LRCLIB cache).
