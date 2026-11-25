# Panduan Handoff & Pemeliharaan (`AGENT_HANDOFF.md`) — Ferric

- **Versi Dokumen:** 2.0.0 (Pre-Release `v0.1.0` Stabilization)
- **Tanggal Pembaruan:** 25 November 2026
- **Repositori:** `d:\code\project\music player\ferric` (branch `main`, remote `https://github.com/Raflyf/Ferric.git`)
- **Binary Terpasang:** `C:\Users\RaflyF\AppData\Local\Programs\Ferric\ferric.exe`
- **Database Pengguna:** `C:\Users\RaflyF\AppData\Local\LivelyDesk\Ferric\data\ferric.db`

---

## 1. Status Proyek Saat Ini

**Aplikasi desktop Windows Ferric sudah hampir rampung (`~95%` untuk `v0.1.0`).** Fase scaffolding awal sudah lama selesai; seluruh fitur inti, paket installer MSI, dan pipeline CI sudah berjalan hijau. Pekerjaan selanjutnya berfokus pada **perbaikan bug spesifik, penyempurnaan UX/performa, dan persiapan rilis `v0.1.0` setelah persetujuan pengguna**.

---

## 2. Peta Dokumen & Kode Sumber Kunci

### Dokumen
- [README.md](file:///d:/code/project/music%20player/ferric/README.md) — ringkasan produk, fitur, dan perintah build lokal.
- [CHANGELOG.md](file:///d:/code/project/music%20player/ferric/CHANGELOG.md) — riwayat perubahan terverifikasi `v0.1.0`.
- [PRD.md](file:///d:/code/project/music%20player/ferric/docs/PRD.md) — cakupan fitur aktual, metrik terukur, skema SQLite, dan batas roadmap pasca-`v0.1.0`.
- [DESIGN.md](file:///d:/code/project/music%20player/ferric/docs/DESIGN.md) — 4 preset tema aktual, resep kaca Acrylic seragam, tata letak 3-pane, dan pintasan keyboard.
- [SYNC_AND_AUDIO_SPEC.md](file:///d:/code/project/music%20player/ferric/docs/SYNC_AND_AUDIO_SPEC.md) — detail implementasi `ferric-audio`, `ferric-providers`, `ferric-sync`, dan Win32 DirectComposition.
- [RESEARCH_AND_RE_FINDINGS.md](file:///d:/code/project/music%20player/ferric/docs/RESEARCH_AND_RE_FINDINGS.md) — alasan stack Rust dan benchmark aktual.

### Kode Sumber Utama
| Berkas | Tanggung Jawab |
| :--- | :--- |
| [apps/ferric-desktop/src/app.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/app.rs) | Layout `live_design!`, 9 halaman UI, 6 tab Settings, handler aksi, theme live propagation, zoom, shortcut |
| [apps/ferric-desktop/src/widgets.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/widgets.rs) | Widget kustom: `FerricAmbientStage` (21-tap blur), `FerricLyricsView` (karaoke spring), `FerricCoverArt`, `FerricBarSlider`, `FerricPlayBtn`, `FerricWinBtn`, `FerricEqGraph`, `FerricQrCanvas` |
| [apps/ferric-desktop/src/track_list.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/track_list.rs) | `PortalList` daftar lagu tervirtualisasi dengan frosted glass `TrackRow` |
| [apps/ferric-desktop/src/shell.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/shell.rs) | Win32 subclass (`WM_GETMINMAXINFO`, `WM_NCCALCSIZE`, `WM_NCHITTEST`, `WM_SIZE`, `WM_CLOSE`), DWM Acrylic, System Tray, single-instance guard, RAM trim |
| [apps/ferric-desktop/src/artwork.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/artwork.rs) | Cache tekstur sampul LRU (`MAX_CACHED_TEXTURES = 128`) + ekstraksi warna aksen adaptif |
| [apps/ferric-desktop/src/state.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/state.rs) | Enum halaman/filter/sort (`SortField::Recent` default), `BgMessage`, grouping Album/Artis |
| [vendor/makepad-platform/src/os/windows/](file:///d:/code/project/music%20player/ferric/vendor/makepad-platform/src/os/windows) | Patch lokal `d3d11.rs` (DirectComposition premultiplied-alpha) & `win32_window.rs` (`WM_NCCALCSIZE` + `get_inner_size()`) |
| [crates/ferric-providers/src/ytmusic.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-providers/src/ytmusic.rs) | Home/Search/Detail/Stream YouTube Music + Smart Multi-Artist Radio Queue |
| [crates/ferric-core/src/db.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-core/src/db.rs) | SQLite WAL, CRUD lagu/playlist, migrasi pembersihan `prune_ephemeral_discovery_bloat` |
| [crates/ferric-audio/src/engine.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-audio/src/engine.rs) & [dsp.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-audio/src/dsp.rs) | Dual-deck playback, Bass-Swap 140 Hz, 10-band Biquad EQ, spectrum analyzer |

---

## 3. Invarian Kritis (Jangan Rusak Saat Memperbaiki Bug)

1. **Jangan tambahkan `WS_CAPTION` di `shell.rs` dan jangan jatuh ke `DefWindowProcW` pada `WM_NCCALCSIZE`:** Penambahan `WS_CAPTION` bersama `DWMSBT_TRANSIENTWINDOW` menyebabkan Windows 11 DWM menggambar tombol caption native ganda (`—`, `❐`, `×`) di atas chrome kustom Ferric.
2. **Pertahankan patch `vendor/makepad-platform` dan `[patch.crates-io]` di `Cargo.toml`:** Jangan kembali ke `SetLayeredWindowAttributes` (memudarkan teks) atau `get_outer_size()` saat maximize (memotong 18px sisi kanan/bawah).
3. **Anti-Kedip Kartu:** Pertahankan `MAX_CACHED_TEXTURES = 128` di `artwork.rs` serta setter idempoten di `widgets.rs` dan diff string di `handle_tick`.
4. **Kebersihan Tabel `Songs`:** Jangan memanggil `db.upsert_tracks` pada `YtmRichHomeLoaded`, `YtmHomeMoreLoaded`, `YtmBrowseDetailLoaded`, `YtmRichSearchCompleted`, atau `RadioLoaded`. Hanya simpan lagu ke SQLite saat dipindai lokal, diputar (`persist_track_to_library`), di-like (`toggle_track_like`), atau dimasukkan ke playlist/sync akun.
5. **Anggaran RAM (< 100 MB terbuka, < 20 MB tray/minimized):** Jangan menambah loop redraw 60 FPS permanen saat jendela tersembunyi atau idle; pertahankan pemangkasan memori di `WM_SIZE`/`WM_CLOSE` dan pasca-`StreamReady`.
6. **Zero Nama Merek Kompetitor & Aturan Tag Rilis:** Jangan tulis nama aplikasi kompetitor di kode, UI, commit, atau dokumentasi. Selalu dorong ke `origin main` saja; **jangan buat atau push git tag rilis (`v*`)** tanpa persetujuan eksplisit pengguna setelah mencoba build terpasang.
7. **Keamanan Penghapusan File:** Jangan gunakan `rm -rf` atau `Remove-Item -Recurse -Force`; gunakan Recycle Bin (`safe-delete.ps1`).

---

## 4. Alur Build, Verifikasi & Audit Standar

1. Format & cek lockfile lokal sebelum push:
   ```powershell
   cargo fmt --all
   cargo fmt --all -- --check
   cargo metadata --locked --format-version 1 > $null
   ```
   *(Catatan: `ci-desktop.yml` memiliki `paths-ignore` untuk `docs/**` dan `**.md`, sehingga commit dokumentasi murni tidak memicu rebuild MSI; commit kode Rust otomatis menjalankan full check, Clippy `-D warnings`, unit tests, MSI, ZIP, dan `SHA256SUMS.txt`.)*
2. Setelah CI hijau (`gh run watch <run_id> --exit-status`):
   - Unduh artefak (`gh run download <run_id> -D <scratch_dir>`).
   - Verifikasi **SHA-256 3/3** (`.msi`, `.zip`, dan `ferric.exe` terpasang vs isi ZIP).
   - Pasang diam-diam: `msiexec /i ferricsetup-v0.1.0.msi /qn /norestart`.
   - Jalankan verifikasi visual langsung (`--page home/songs/albums/artists/settings`, `--play-first`, RAM open/minimized, `SHADER_ERROR_COUNT=0`) dan inspeksi hasil tangkapan layar sebelum melapor ke pengguna.
