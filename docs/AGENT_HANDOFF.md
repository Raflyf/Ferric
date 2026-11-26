# Panduan Handoff & Pemeliharaan (`AGENT_HANDOFF.md`) — Ferric

- **Versi Dokumen:** 2.1.0 (Pre-Release `v0.1.0` Ready-for-Tag)
- **Tanggal Pembaruan:** 26 November 2026
- **Repositori:** `d:\code\project\music player\ferric` (branch `main`, remote `https://github.com/Raflyf/Ferric.git`)
- **Kode Terverifikasi Live Terakhir:** `5c81b11` (CI run `45734632596` hijau, MSI terpasang & diaudit live)
- **Binary Terpasang:** `C:\Users\RaflyF\AppData\Local\Programs\Ferric\ferric.exe` (`14.008.320` byte)
- **Database Pengguna:** `C:\Users\RaflyF\AppData\Local\LivelyDesk\Ferric\data\ferric.db`

---

## 1. Status Proyek Saat Ini

**Aplikasi desktop Windows Ferric `v0.1.0` sudah rampung secara fitur, animasi 60 FPS, keamanan, pengerasan PE anti-RE, dan verifikasi live.** Seluruh CI check (`fmt`, `clippy -D warnings`, `cargo test`, `verify_pe_hardening.py`, WiX MSI, ZIP, `SHA256SUMS.txt`) hijau. Pekerjaan berikutnya hanya perbaikan minor tambahan jika diminta pengguna, lalu penandaan dan publikasi git tag `v0.1.0` setelah persetujuan eksplisit pengguna.

---

## 2. Peta Dokumen & Kode Sumber Kunci

### Dokumen
- [README.md](file:///d:/code/project/music%20player/ferric/README.md) — ringkasan produk, kapabilitas, keamanan/PE hardening, dan perintah build.
- [CHANGELOG.md](file:///d:/code/project/music%20player/ferric/CHANGELOG.md) — riwayat perubahan terverifikasi pre-`v0.1.0`.
- [PRD.md](file:///d:/code/project/music%20player/ferric/docs/PRD.md) — cakupan fitur aktual, metrik live terukur, keamanan, batasan resmi Spotify, skema SQLite, dan roadmap pasca-`v0.1.0`.
- [DESIGN.md](file:///d:/code/project/music%20player/ferric/docs/DESIGN.md) — 4 preset tema (default AMOLED Black + Burnt Orange `#E86A2C`), GPU HSV picker, WCAG Adaptive Contrast, font OS internasional, koreografi 60 FPS, dan pintasan keyboard.
- [SYNC_AND_AUDIO_SPEC.md](file:///d:/code/project/music%20player/ferric/docs/SYNC_AND_AUDIO_SPEC.md) — detail implementasi `ferric-audio`, `ferric-providers`, `ferric-sync`, cold-start DWM Acrylic, DPAPI, mitigasi Win32, WebView2 sandbox, Discord RPC, dan PE hardening.
- [RESEARCH_AND_RE_FINDINGS.md](file:///d:/code/project/music%20player/ferric/docs/RESEARCH_AND_RE_FINDINGS.md) — alasan stack Rust, temuan teknis Win32/font/keamanan, dan benchmark aktual.

### Kode Sumber Utama
| Berkas | Tanggung Jawab |
| :--- | :--- |
| [apps/ferric-desktop/src/main.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/main.rs) & [build.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/build.rs) | Entrypoint startup (`win32_harden_process`, AppUserModelID, single-instance), PE version info `Ferric`, manifest PerMonitorV2 UTF-8 |
| [apps/ferric-desktop/src/app.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/app.rs) | Layout `live_design!`, 9 halaman UI, 6 tab Settings, animasi 60 FPS (`fs_reveal_t` 2-fase, `fs_video_morph_t`), theme live propagation, zoom, shortcut |
| [apps/ferric-desktop/src/theme.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/theme.rs) | Token Makepad lokal, Inter v4.1 + 4 slot font virtual OS internasional (`__sys_cjk_jp/kr/cn`, `__sys_thai`) |
| [apps/ferric-desktop/src/widgets.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/widgets.rs) | Widget kustom: `FerricAmbientStage` (21-tap aurora + opacity), `FerricLyricsView` (uniform active line + `reveal_alpha`), `FerricCoverArt` (morph), `FerricRepeatBtn` & `FerricVolumeBtn` (SDF), `FerricColorPicker` (HSV), `FerricBarSlider`, `FerricPlayBtn`, `FerricWinBtn`, `FerricEqGraph`, `FerricQrView` |
| [apps/ferric-desktop/src/track_list.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/track_list.rs) | `PortalList` daftar lagu tervirtualisasi dengan frosted glass `TrackRow` |
| [apps/ferric-desktop/src/shell.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/shell.rs) | Win32 hardening, subclass (`WM_GETMINMAXINFO`, `WM_NCCALCSIZE`, `WM_NCHITTEST`, `WM_NCACTIVATE`, `WM_SIZE`, `WM_CLOSE`), DWM Acrylic, Taskbar `ITaskbarList3`, System Tray, single-instance, TOCTOU-safe `msiexec`, RAM trim |
| [apps/ferric-desktop/src/video_player.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/video_player.rs) & [google_login.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/google_login.rs) | Host WebView2 tersandbox ketat, `#stage` fade-in 220ms, kliping tunggal `16px` `CreateRoundRectRgn`, login Google opsional |
| [apps/ferric-desktop/src/discord_rpc.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/discord_rpc.rs) | Klien IPC named-pipe Discord Rich Presence zero-dependency (`Listening to Ferric`, App ID bawaan, ikon kaset) |
| [apps/ferric-desktop/src/artwork.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/artwork.rs) & [state.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/state.rs) | Cache tekstur LRU (`MAX_CACHED_TEXTURES = 128`), ekstraksi warna HSV, state halaman/filter/sort (`SortField::Recent` default) |
| [vendor/makepad-platform/src/](file:///d:/code/project/music%20player/ferric/vendor/makepad-platform/src) | Patch lokal: `d3d11.rs` (DirectComposition premultiplied-alpha + Commit), `win32_window.rs` (cold-start DWM Acrylic sebelum `CreateTargetForHwnd`, `WM_NCACTIVATE`/`WM_ACTIVATE`, `get_inner_size()`), `cx.rs`, `os/cx_native.rs` (OS CJK/Thai font loader) |
| [crates/ferric-core/src/db.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-core/src/db.rs), [theme.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-core/src/theme.rs), [secrets.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-core/src/secrets.rs) | SQLite WAL, tema 4 preset + WCAG `AdaptivePalette`, enkripsi DPAPI (`dpapi:v1:`) + CSPRNG `BCryptGenRandom` |
| [crates/ferric-providers/src/ytmusic.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-providers/src/ytmusic.rs), [lyrics.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-providers/src/lyrics.rs), [oauth.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-providers/src/oauth.rs) | Taste-driven Home, Smart Multi-Artist Radio, Official Video Resolver, LRCLIB clean filter & auto-heal, OAuth 2.0 PKCE read-only |
| [crates/ferric-audio/src/engine.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-audio/src/engine.rs) & [dsp.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-audio/src/dsp.rs) | Dual-deck auto-only crossfade, Bass-Swap 140 Hz, 10-band Biquad EQ, spectrum analyzer |
| [scripts/verify_pe_hardening.py](file:///d:/code/project/music%20player/ferric/scripts/verify_pe_hardening.py) & [.github/workflows/](file:///d:/code/project/music%20player/ferric/.github/workflows) | Gerbang verifikasi PE (`GUARD_CF`, `CETCOMPAT`, `BREPRO`, 0 leak), CI build & manual tag release |

---

## 3. Invarian Kritis (Jangan Rusak Saat Memperbaiki Bug)

1. **Urutan Cold-Start DWM Acrylic & Anti-Ghost Caption:**
   - Pertahankan penyemaian `INITIAL_ACRYLIC_BACKDROP` saat `Event::Startup` dan penerapan atribut DWM di `Win32Window::init` **sebelum `CreateTargetForHwnd` dan `ShowWindow`**, serta `dcomp_dev.Commit()` sebelum `show()`.
   - Jangan tambahkan `WS_CAPTION` di `shell.rs`, jangan jatuh ke `DefWindowProcW` pada `WM_NCCALCSIZE`, dan pertahankan penerusan `WM_NCACTIVATE` (`LPARAM(-1)`) + `WM_ACTIVATE` ke `DefWindowProcW` agar Acrylic tidak pernah menjadi solid saat cold start maupun menggambar tombol caption ganda.
2. **Default Instalasi Awal AMOLED Black:** `ThemeConfig::default()` wajib mempertahankan `AMOLED Black` (`#000000`, aksen `#E86A2C`, `acrylic_enabled: false`) untuk pengguna instalasi baru.
3. **Satu Sudut Membulat Video Klip & Anti-Ghosting Fullscreen:** Pertahankan satu `CreateRoundRectRgn` (`16px` DPI-exact) pada jendela WebView2 video (jangan tambahkan `DWMWCP_ROUND` atau stroke border luar bertumpuk) dan urutan 2-fase pada `fs_reveal_t` (`bg_alpha: t/0.58`, `show_stage: t > 0.52`).
4. **Zero-Bloat OS Fonts & Filter Polusi LRCLIB:** Jangan membundel font CJK puluhan megabyte ke dalam repo/installer; pertahankan slot virtual `__sys_*` di `theme.rs` + `cx_native.rs` serta `is_polluted_multilingual_lrc` auto-heal pada `lyrics_cache`.
5. **Keamanan, DPAPI & Gerbang PE Hardening:** Pertahankan `win32_harden_process()`, enkripsi `dpapi:v1:` pada `accounts`, sandbox WebView2 ketat, re-hash SHA-256 tahan-TOCTOU sebelum `CreateProcessW(msiexec.exe)`, serta kelulusan 100% `scripts/verify_pe_hardening.py` (nol `.pdb`, `runneradmin`, `C:\Users\`, `D:\a\`).
6. **Anti-Kedip Kartu & Kebersihan Tabel `Songs`:** Pertahankan `MAX_CACHED_TEXTURES = 128`, setter idempoten, dan larangan `db.upsert_tracks` pada hasil browse Home/Search/Detail/Radio.
7. **Anggaran RAM (< 50 MB aktif, < 10 MB tray/idle):** Jangan menambah loop redraw 60 FPS permanen saat animasi selesai atau jendela tersembunyi; pertahankan trim working set.
8. **Zero Nama Merek Kompetitor, DRM Compliance & Aturan Tag Rilis:** Jangan tulis nama aplikasi kompetitor di kode/UI/docs; jangan implementasikan mekanisme bypass DRM/API privat; dorong ke `origin main` saja dan **jangan buat atau push git tag rilis (`v*`)** tanpa persetujuan eksplisit pengguna.
9. **Keamanan Penghapusan File:** Jangan gunakan `rm -rf` atau `Remove-Item -Recurse -Force`; gunakan Recycle Bin (`safe-delete.ps1`).

---

## 4. Alur Build, Verifikasi & Audit Standar

> **Catatan Lingkungan Host:** Toolchain MSVC lokal di mesin pengembangan saat ini tidak sehat; lakukan `cargo fmt` dan validasi lockfile/skrip secara lokal, lalu andalkan GitHub Actions (`ci-desktop.yml` pada `windows-latest`) untuk kompilasi release, Clippy, unit test, `verify_pe_hardening.py`, serta pembuatan MSI & ZIP.

1. Format & cek lockfile lokal sebelum push:
   ```powershell
   cargo fmt --all
   cargo fmt --all -- --check
   cargo metadata --locked --format-version 1 > $null
   ```
   *(Catatan: `ci-desktop.yml` mengabaikan `docs/**`, `**.md`, dan `assets/ui_previews/**` pada `paths-ignore`, sehingga commit dokumentasi murni tidak memicu rebuild MSI; commit kode Rust/installer otomatis menjalankan full check, Clippy `-D warnings`, unit tests, build hardened terisolasi `CARGO_HOME=C:\c` + `subst S:`, `verify_pe_hardening.py`, WiX MSI, ZIP, dan `SHA256SUMS.txt`.)*
2. Setelah CI hijau (`gh run watch <run_id> --exit-status`):
   - Unduh artefak (`gh run download <run_id> -D <scratch_dir>`).
   - Verifikasi **SHA-256 3/3** (`.msi`, `.zip`, dan `ferric.exe` terpasang vs isi ZIP).
   - Pasang diam-diam: `msiexec /i ferricsetup-v0.1.0.msi /qn /norestart`.
   - Jalankan verifikasi visual & telemetri langsung (`SHADER_ERROR_COUNT=0`, `APPLY_ERR=0`, `RES_MISS=0`, RAM open/close/minimized, CPU settled, serta tangkapan layar UI terkait) sebelum melapor ke pengguna.
