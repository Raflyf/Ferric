# Panduan Handoff & Pemeliharaan (`AGENT_HANDOFF.md`) — Ferric

- **Versi Dokumen:** 2.2.0 (Pre-Release `v0.1.0` Ready-for-Tag)
- **Tanggal Pembaruan:** 26 November 2026
- **Repositori:** `https://github.com/Raflyf/Ferric.git` (branch `main`)
- **Kode Terverifikasi Live Terakhir:** Pre-release `v0.1.0` full security, GPU/VRAM, SMTC, Acrylic & clean-uninstall audit
- **Binary Terpasang:** `%LOCALAPPDATA%\Programs\Ferric\ferric.exe`
- **Database Pengguna:** `%LOCALAPPDATA%\LivelyDesk\Ferric\data\ferric.db`

---

## 1. Status Proyek Saat Ini

**Aplikasi desktop Windows Ferric `v0.1.0` sudah rampung secara fitur, animasi 60 FPS, kebijakan GPU DX11 hemat-RAM (prioritas iGPU tanpa membangunkan dGPU, fallback GPU utama/WARP), integrasi SMTC penuh (thumbnail + timeline + seek), Adaptive Acrylic Tint, menu tray dark/light mengikuti sistem, clean uninstall MSI sampai akar, keamanan, pengerasan PE anti-RE, dan verifikasi live.** Seluruh CI check (`fmt`, `clippy -D warnings`, `cargo test`, `verify_pe_hardening.py`, WiX MSI, ZIP, `SHA256SUMS.txt`) hijau dan bebas kode debug/flag pengujian di profil release. Pekerjaan berikutnya hanya penandaan dan publikasi git tag `v0.1.0` setelah persetujuan eksplisit pengguna.

---

## 2. Peta Dokumen & Kode Sumber Kunci

### Dokumen
- [README.md](../README.md) — ringkasan produk, kapabilitas, keamanan/PE hardening, dan perintah build.
- [CHANGELOG.md](../CHANGELOG.md) — riwayat perubahan terverifikasi pre-`v0.1.0`.
- [PRD.md](./PRD.md) — cakupan fitur aktual, metrik live terukur, keamanan, batasan resmi Spotify, skema SQLite, dan roadmap pasca-`v0.1.0`.
- [DESIGN.md](./DESIGN.md) — 4 preset tema (default AMOLED Black + Burnt Orange `#E86A2C`), GPU HSV picker, WCAG Adaptive Contrast, font OS internasional, koreografi 60 FPS, dan pintasan keyboard.
- [SYNC_AND_AUDIO_SPEC.md](./SYNC_AND_AUDIO_SPEC.md) — detail implementasi `ferric-audio`, `ferric-providers`, `ferric-sync`, cold-start DWM Acrylic, DPAPI, mitigasi Win32, WebView2 sandbox, Discord RPC, dan PE hardening.
- [RESEARCH_AND_RE_FINDINGS.md](./RESEARCH_AND_RE_FINDINGS.md) — alasan stack Rust, temuan teknis Win32/font/keamanan, dan benchmark aktual.

### Kode Sumber Utama
| Berkas | Tanggung Jawab |
| :--- | :--- |
| `apps/ferric-desktop/src/main.rs` & `build.rs` | Entrypoint startup (`win32_harden_process`, AppUserModelID, single-instance), PE version info `Ferric`, manifest PerMonitorV2 UTF-8 |
| `apps/ferric-desktop/src/app.rs` | Layout `live_design!`, 9 halaman UI, 7 tab Settings, animasi 60 FPS (`fs_reveal_t` 2-fase, `fs_video_morph_t`), theme live propagation, zoom, shortcut |
| `apps/ferric-desktop/src/theme.rs` | Token Makepad lokal, Inter v4.1 + 4 slot font virtual OS internasional (`__sys_cjk_jp/kr/cn`, `__sys_thai`) |
| `apps/ferric-desktop/src/widgets.rs` | Widget kustom: `FerricAmbientStage` (21-tap aurora + opacity), `FerricLyricsView` (uniform active line + `reveal_alpha`), `FerricCoverArt` (morph), `FerricRepeatBtn` & `FerricVolumeBtn` (SDF), `FerricColorPicker` (HSV), `FerricBarSlider`, `FerricPlayBtn`, `FerricWinBtn`, `FerricEqGraph`, `FerricQrView` |
| `apps/ferric-desktop/src/track_list.rs` | `PortalList` daftar lagu tervirtualisasi dengan frosted glass `TrackRow` |
| `apps/ferric-desktop/src/shell.rs` | Win32 hardening, subclass (`WM_GETMINMAXINFO`, `WM_NCCALCSIZE`, `WM_NCHITTEST`, `WM_NCACTIVATE`, `WM_ACTIVATE`, `WM_SIZE`, `WM_THEMECHANGED`, `WM_DWMCOMPOSITIONCHANGED`, `WM_CLOSE`), DWM Acrylic + pemulihan aktif/restore, menu tray dark/light mengikuti tema Windows (`uxtheme.dll` `#135`/`#136`), registrasi AUMID `Raflyf.Ferric`, Taskbar `ITaskbarList3`, System Tray, single-instance, TOCTOU-safe `msiexec`, RAM trim |
| `apps/ferric-desktop/src/smtc.rs` | Windows SMTC penuh (`souvlaki 0.9.3`): thumbnail sampul lokal `file://` + fallback HTTPS, timeline `Position`/`EndTime` real-time (1 Hz & saat seek/ganti status), serta handler `SetPosition` / `Seek` untuk Windows 11 Quick Settings dan overlay pihak ketiga (Nusa Pro, NotchPixel, ModernFlyouts) |
| `apps/ferric-desktop/src/video_player.rs`, `google_login.rs`, `spotify_browser_login.rs` | Host WebView2 tersandbox ketat, `#stage` fade-in 220ms, kliping tunggal `16px` `CreateRoundRectRgn`, login Google & Spotify Web opsional |
| `apps/ferric-desktop/src/discord_rpc.rs` | Klien IPC named-pipe Discord Rich Presence zero-dependency (`Listening to Ferric`, App ID bawaan, ikon kaset) |
| `apps/ferric-desktop/src/artwork.rs` & `state.rs` | Cache tekstur LRU (`MAX_CACHED_TEXTURES = 144`), downscaler *area-box* (`160px` kartu / `256px` hero), ekstraksi warna HSV, state halaman/filter sumber akun/sort (`Newest`/`Oldest`/`A-Z`, `SortField::Recent` default) |
| `vendor/makepad-platform/src/` | Patch lokal: `d3d11.rs` (kebijakan hemat-RAM iGPU-pertama tanpa membangunkan dGPU + fallback GPU utama -> WARP CPU, sampler bilinear hardware eksplisit, tekstur sampul `D3D11_USAGE_IMMUTABLE`, bebas CPU `Vec<u32>` & `ID3DBlob` pasca-upload/kompilasi, DirectComposition premultiplied-alpha + Commit), `id_pool.rs` & `texture.rs` (`free_unused_textures`), `win32_window.rs` (cold-start DWM Acrylic sebelum `CreateTargetForHwnd`, penerusan `WM_NCACTIVATE`/`WM_ACTIVATE` asli, `get_inner_size()`), `cx.rs`, `os/cx_native.rs` (OS CJK/Thai font loader) |
| `installer/Ferric.wxs` & `scripts/Installer.ps1` | WiX v3 per-user MSI: shortcut Start Menu + registrasi AUMID `Raflyf.Ferric`, serta `CleanUninstallUserData` (`WixQuietExec64`, kondisi `REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE`) untuk bersih total sampai akar saat uninstall tanpa mengganggu data saat upgrade |
| `crates/ferric-core/src/{db,theme,secrets}.rs` | SQLite WAL, tema 4 preset + WCAG `AdaptivePalette`, enkripsi DPAPI (`dpapi:v1:`) + CSPRNG `BCryptGenRandom` + zeroization |
| `crates/ferric-providers/src/{ytmusic,lyrics,oauth,spotify_web,subsonic,updater}.rs` | Taste-driven Home, Smart Multi-Artist Radio, Official Video Resolver, LRCLIB clean filter & auto-heal, OAuth 2.0 PKCE Premium-only gate, Spotify Web read-only sync, Subsonic HTTPS, SHA-256 updater |
| `crates/ferric-audio/src/{engine,dsp}.rs` | Dual-deck auto-only crossfade, Bass-Swap 140 Hz, 10-band Biquad EQ, epoch-tagged lock-free ring buffer, WASAPI hot-swap recovery, NaN/Inf DSP firewall |
| `scripts/{build-release.ps1,verify_pe_hardening.py,Installer.ps1}` & `.github/workflows/` | Build terisolasi & gerbang verifikasi PE (`GUARD_CF`, `CFTable`, `DependentLoadFlags=0x0800`, `CETCOMPAT`, `BREPRO`, 0 RWX, 0 leak), SHA-pinned CI & split release |

---

## 3. Invarian Kritis (Jangan Rusak Saat Memperbaiki Bug)

1. **Urutan Cold-Start DWM Acrylic & Anti-Ghost Caption:**
   - Pertahankan penyemaian `INITIAL_ACRYLIC_BACKDROP` saat `Event::Startup` dan penerapan atribut DWM di `Win32Window::init` **sebelum `CreateTargetForHwnd` dan `ShowWindow`**, serta `dcomp_dev.Commit()` sebelum `show()`.
   - Jangan tambahkan `WS_CAPTION` di `shell.rs`, jangan jatuh ke `DefWindowProcW` pada `WM_NCCALCSIZE`, dan pertahankan penerusan `WM_NCACTIVATE` (`LPARAM(-1)`) + `WM_ACTIVATE` ke `DefWindowProcW` agar Acrylic tidak pernah menjadi solid saat cold start maupun menggambar tombol caption ganda.
2. **Default Instalasi Awal AMOLED Black:** `ThemeConfig::default()` wajib mempertahankan `AMOLED Black` (`#000000`, aksen `#E86A2C`, `acrylic_enabled: false`) untuk pengguna instalasi baru.
3. **Satu Sudut Membulat Video Klip & Anti-Ghosting Fullscreen:** Pertahankan satu `CreateRoundRectRgn` (`16px` DPI-exact) pada jendela WebView2 video (jangan tambahkan `DWMWCP_ROUND` atau stroke border luar bertumpuk) dan urutan 2-fase pada `fs_reveal_t` (`bg_alpha: t/0.58`, `show_stage: t > 0.52`).
4. **Zero-Bloat OS Fonts & Filter Polusi LRCLIB:** Jangan membundel font CJK puluhan megabyte ke dalam repo/installer; pertahankan slot virtual `__sys_*` di `theme.rs` + `cx_native.rs` serta `is_polluted_multilingual_lrc` auto-heal pada `lyrics_cache`.
5. **Keamanan, DPAPI & Gerbang PE Hardening:** Pertahankan `win32_harden_process()`, enkripsi `dpapi:v1:` pada `accounts`, sandbox WebView2 ketat, re-hash SHA-256 tahan-TOCTOU sebelum `CreateProcessW(msiexec.exe)`, serta kelulusan 100% `scripts/verify_pe_hardening.py` (nol `.pdb`, `runneradmin`, `C:\Users\`, `D:\a\`).
6. **Anti-Kedip Kartu & Kebersihan Tabel `Songs`:** Pertahankan `MAX_CACHED_TEXTURES = 144` (melebihi total slot Home + Sidebar + Queue), downscaler area-box (`160px`/`256px`) dengan `D3D11_USAGE_IMMUTABLE` + pembebasan CPU `Vec<u32>` pasca-upload di `d3d11.rs`, setter idempoten, dan larangan `db.upsert_tracks` pada hasil browse Home/Search/Detail/Radio.
7. **Disiplin RAM (hemat-RAM iGPU-pertama):** Jangan menambah loop redraw 60 FPS permanen saat animasi selesai atau jendela tersembunyi; pertahankan prioritas iGPU tanpa membangunkan dGPU, `free_unused_textures()`, dan trim working set saat masuk system tray (`~35 MB` idle UI / `~7.4 MB` tray).
8. **Zero Nama Merek Kompetitor, DRM Compliance & Aturan Tag Rilis:** Jangan tulis nama aplikasi kompetitor di kode/UI/docs; jangan implementasikan mekanisme bypass DRM/API privat; dorong ke `origin main` saja dan **jangan buat atau push git tag rilis (`v*`)** tanpa persetujuan eksplisit pengguna.
9. **Keamanan Penghapusan File & Clean Uninstall:** Jangan gunakan `rm -rf` atau `Remove-Item -Recurse -Force` di skrip dev; pertahankan kondisi ketat `REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE` pada `CleanUninstallUserData` di `installer/Ferric.wxs`.

---

## 4. Alur Build, Verifikasi & Audit Standar

1. Format, Clippy, unit tests, build hardened terisolasi, verifikasi PE, dan MSI lokal:
   ```powershell
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --locked -- -D warnings
   cargo test --workspace --locked
   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-release.ps1
   python .\scripts\verify_pe_hardening.py .\target\release\ferric.exe
   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\Installer.ps1
   ```
2. Pada GitHub Actions (`ci-desktop.yml`), setiap push kode ke `main` menjalankan seluruh gerbang yang sama plus paket Portable ZIP dan `SHA256SUMS.txt`. *(Catatan: `docs/**` dan `**.md` berada di `paths-ignore`.)*
3. Publikasi GitHub Release hanya berjalan saat tag `v*` di-push secara eksplisit melalui `.github/workflows/release.yml`.
