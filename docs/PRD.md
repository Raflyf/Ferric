# Product Requirements Document (PRD): Ferric Native Rust Music Player

- **Versi Dokumen:** 2.0.0 (Diselaraskan dengan Implementasi Aktual Pre-Release `v0.1.0`)
- **Tanggal Pembaruan:** 25 November 2026
- **Status Proyek:** **Windows Desktop `v0.1.0` Hampir Rampung (~95%)** — seluruh fitur inti, mesin audio, tampilan AMOLED & Native Acrylic, discovery, radio queue, lirik fullscreen, video klip, EQ, database SQLite, dan paket installer MSI sudah berjalan dan terverifikasi; fokus saat ini hanya pada stabilisasi bug tersisa dan penyesuaian akhir sebelum tag rilis `v0.1.0`. Ekspansi Android Mobile menjadi fase pasca-`v0.1.0`.
- **Target Platform Saat Ini:** Windows 10/11 (`x86_64`, MSI per-user & Portable ZIP)
- **Target Platform Lanjutan:** Android 8.0–16+ (`arm64-v8a`) berbagi crate inti `ferric-core`, `ferric-audio`, `ferric-providers`, dan `ferric-sync`
- **Bahasa & Runtime:** 100% Native Rust (Edition 2024) + `makepad-widgets 1.0.0` (DirectX 11 + DirectComposition)
- **Dokumen Terkait:** [DESIGN.md](file:///d:/code/project/music%20player/ferric/docs/DESIGN.md), [SYNC_AND_AUDIO_SPEC.md](file:///d:/code/project/music%20player/ferric/docs/SYNC_AND_AUDIO_SPEC.md), [RESEARCH_AND_RE_FINDINGS.md](file:///d:/code/project/music%20player/ferric/docs/RESEARCH_AND_RE_FINDINGS.md), [AGENT_HANDOFF.md](file:///d:/code/project/music%20player/ferric/docs/AGENT_HANDOFF.md)

---

## 1. Visi Produk & Posisi Saat Ini

**Ferric** adalah pemutar musik desktop modern berarsitektur **100% Native Rust** tanpa Electron/Chromium untuk UI utamanya, dirancang untuk menggabungkan pengalaman streaming & pustaka musik lengkap (pemutaran lokal lossless, katalog daring, radio otomatis multi-artis, lirik sinkron karaoke, video klip dalam aplikasi, 10-band Parametric EQ + AutoEq, dan sinkronisasi lintas perangkat) dengan jejak memori **di bawah 50 MB saat memutar musik** dan **di bawah 10 MB saat idle/minimized**.

Ferric merupakan bagian dari keluarga visual **LivelyDesk** (`LivelyDesk` · `WallpaperLive` · `TransparentBar` · `Ferric`), menggunakan default **AMOLED Black (`#000000`)** serta tema opsional **Native Windows 11 Acrylic** per-piksel berbasis DirectComposition.

---

## 2. Target & Hasil Terukur Performa (Windows 11 Terverifikasi)

| Metrik | Target Anggaran | Hasil Terukur Aktual (`6fb7cff`) | Mekanisme Implementasi |
| :--- | :--- | :--- | :--- |
| **RAM (Idle, Jendela Terbuka)** | `< 45 MB` | **`7,6 MB`** Working Set | SQLite WAL ringan + `shrink_memory()` + Win32 working-set trim pasca-inisialisasi |
| **RAM (Streaming Aktif + Artwork + Lirik)** | `< 100 MB` | **`44,1 MB`** Working Set | Ring buffer `rtrb` streaming (tanpa memuat seluruh file ke RAM) + LRU 128 tekstur (`artwork.rs`) |
| **RAM (Minimized / System Tray)** | `< 25 MB` | **`7,3 MB`** Working Set | Menghentikan loop redraw UI saat tersembunyi + eviksi tekstur non-aktif + trim working set |
| **GPU (Idle / Tersembunyi)** | `0%` | **`0%`** (event-driven) | Loop animasi lirik/motion berhenti penuh saat jendela tersembunyi atau tidak aktif |
| **Shader & Render Error** | `0` | **`0`** | Seluruh shader Makepad DX11 tervalidasi tanpa peringatan kompilasi |
| **Single-Instance Guard** | 1 proses aktif | **Terverifikasi (1 proses)** | Named mutex `Local\FerricMusicPlayerSingleInstance_v1` + `RegisterWindowMessageW` aktivasi jendela lama |

---

## 3. Arsitektur Workspace Cargo Aktual

```text
ferric/
├── Cargo.toml                  # Workspace root (v0.1.0, Edition 2024, lto="thin", strip="symbols")
├── crates/
│   ├── ferric-core/            # db.rs (SQLite WAL), models.rs, queue.rs, theme.rs, eq.rs
│   ├── ferric-audio/           # engine.rs (Dual-Deck A+B), dsp.rs (Bass-Swap/EQ/Spectrum), demo_synth.rs
│   ├── ferric-providers/       # ytmusic.rs, yt_cleaner.rs, lyrics.rs, local_scanner.rs,
│   │                           # public_sync.rs, oauth.rs, subsonic.rs, autoeq.rs, resolver.rs, updater.rs
│   └── ferric-sync/            # lib.rs (postcard frames, ActiveSpeakerLease), lan.rs (UDP 43821), qr.rs
├── apps/
│   └── ferric-desktop/         # app.rs, widgets.rs, track_list.rs, state.rs, theme.rs, icons.rs,
│                               # artwork.rs, shell.rs, smtc.rs, google_login.rs, video_player.rs
├── vendor/
│   └── makepad-platform/       # Patch lokal DirectComposition Acrylic & geometri maximize presisi
└── scripts/
    └── Installer.ps1           # Paket WiX v3 per-user MSI
```

### Dependensi Inti Terkunci (`Cargo.toml`)

| Lapisan | Crate & Versi Aktual | Fungsi |
| :--- | :--- | :--- |
| **UI Native GPU** | `makepad-widgets 1.0.0` + `vendor/makepad-platform` | Rendering DX11 + DirectComposition per-pixel Acrylic, `PortalList`, shader SDF |
| **Audio Decode** | `symphonia 0.6.1` + `symphonia-opus 0.1.3` | Dekoder pure-Rust (FLAC, MP3, AAC/M4A, ALAC, Ogg Vorbis, Opus, WAV, AIFF) |
| **DSP & Resampling** | `rubato 5.1.0` + `biquad 0.6.0` + `rtrb 0.4.1` | Resampler, 10-band Parametric Biquad EQ, wait-free SPSC ring buffer |
| **Audio Output** | `cpal 0.18.2` | Output WASAPI Windows dengan callback real-time bebas alokasi |
| **Database & Tag** | `rusqlite 0.43` (`bundled`) + `lofty 0.25.5` + `walkdir 2.5` | SQLite mode WAL + pembaca tag audio lokal & pemindaian folder |
| **Jaringan & Sync** | `ureq 3.5` (`json`) + `postcard 1.2` + `sha2 0.10` + `aho-corasick 1.1.5` | Klien HTTP sinkron ringan tanpa overhead async runtime berat, framing biner, verifikasi SHA-256 |
| **Integrasi Windows** | `windows 0.62` + `souvlaki 0.9.3` + `rfd 0.18.0` + `webview2-com 0.39.1` | Subclass Win32, DWM Acrylic, System Tray, SMTC, dialog file native, WebView2 login & video klip |

---

## 4. Fitur yang Sudah Diimplementasikan (Desktop `v0.1.0`)

### 4.1 Navigasi & Manajemen Pustaka (9 Halaman)
1. **Home:** Mood filter chips, `Quick Picks` (2x4 tile), hingga 8 rak rekomendasi dinamis dengan tombol `Load More Recommendations` (continuation token), serta tombol `Play All` & `Shuffle`.
2. **Search:** Pencarian langsung dari top bar (inline tanpa popup mengganggu) atau halaman Search dengan filter (`All`, `Songs`, `Albums`, `Artists`, `Playlists`, `Local`), riwayat pencarian terbaru, dan kartu Album/Artis/Playlist.
3. **Recents:** Riwayat putar kronologis lengkap dengan statistik total pemutaran dan durasi mendengar, plus `Play All` dan `Clear History`.
4. **Songs:** Daftar lagu tervirtualisasi (`TrackList`) dengan default `Sort: Recent` (siklus `Recent` / `Artist` / `Title` / `Album` / `Duration`), filter sumber (`All`, `Liked`, `Local`, `Spotify`, `YouTube`), filter teks kaca transparan, `Add Folder`, dan aksi per baris (`Play Next`, `Like`, `Menu`/`Add to Playlist`, `Delete`). Hanya menyimpan lagu lokal, lagu yang diputar/di-like, dan isi playlist — hasil jelajah/pencarian/radio sementara tidak lagi mengotori tabel `Songs`.
5. **Albums, Artists & Playlists:** Grid visual 5 kolom x 4 baris (**20 kartu per halaman**) dengan pagination (`Page X / Y`), spacer `ghost` agar baris terakhir tidak melar, normalisasi nama album bersih, dan kreator/importer playlist (`.m3u`, `.m3u8`, `.csv`).
6. **Detail View:** Tampilan detail Album/Artis/Playlist lengkap dengan `Play`, `Shuffle`, `Start Radio`, `Pin`, `+ Queue`, `Export M3U`, `Rename`, dan `Delete`.
7. **Settings Hub (6 Tab):** `Accounts`, `Playback`, `Equalizer`, `Appearance`, `Storage`, dan `Connect`.

### 4.2 Mesin Audio, DSP & Smart Radio Queue
- **Dual-Deck `Deck A + Deck B`:** Pre-buffering lagu berikutnya sebelum lagu aktif habis, transisi `0–12 dtk`, pilihan kurva **Bass-Swap Automix (`140 Hz`)**, **S-Curve Velvet**, dan **Equal-Power**, **Gapless Album Lock** (`0 ms` pada album lokal berurutan), pemotong hening awal/akhir, micro-fade Hermite pada Play/Pause/Seek, ReplayGain, Playback Speed (`0.75x–1.5x`), Mono Audio, dan Sleep Timer (`15/30/45/60m` / `End of Track`).
- **10-Band Parametric EQ:** Grafik respons frekuensi GPU interaktif, 10 fader (`32 Hz–16 kHz`), Preamp, pencarian **24 profil headphone AutoEq**, dan import/export **Equalizer APO `.txt`**.
- **Smart Multi-Artist Radio Queue:** Ekspansi radio dua lompatan (`RDAMVM`), koherensi bahasa/vibe, batas ketat maksimal 1 lagu per artis (artis lagu aktif maksimal 1 kali dan tidak pernah di posisi #1), penyaringan ketat terhadap variasi lagu yang sama/cover/karaoke/pitch-shift (`+4Key`, `Mini Pop Kids`, `Metal Version`, `Remix`, `Extended`, kanal SEO `Todays Top Hits`), serta tombol **Refresh Queue** yang merotasi tuner untuk menghasilkan susunan artis baru.

### 4.3 Lirik Fullscreen, Video Klip & Integrasi Windows
- **Synced Lyrics:** Waterfall 4 tahap (file `.lrc` lokal -> embedded tag -> `LRCLIB` -> `Lyrics.ovh`), panel lirik kanan + mode **Fullscreen Lyrics** dengan animasi spring 60 FPS, sapuan karaoke per kata, klik baris untuk seek, offset `-0.5s`/`+0.5s`, dan latar blur 21-tap dengan scrim velvet gelap ber kontras tinggi.
- **In-App HD Video Clip:** Toggle `Cover` / `Video` di dalam mode Fullscreen yang menempelkan host WebView2 native MP4 tersinkron dengan posisi & status pemutaran WASAPI.
- **Kontrol Sistem & Pembaruan:** Single-instance guard, System Tray (`Close to Tray`), Windows 11 SMTC & tombol media keyboard, scroll-wheel volume, serta pengecek pembaruan GitHub Releases dengan verifikasi `SHA256SUMS.txt` sebelum meluncurkan installer MSI.

---

## 5. Skema Database SQLite Aktual

Lokasi berkas: `%LOCALAPPDATA%\LivelyDesk\Ferric\data\ferric.db` (`PRAGMA journal_mode = WAL; synchronous = NORMAL; foreign_keys = ON;`).

| Tabel | Kunci Utama & Fungsi Aktual (`crates/ferric-core/src/db.rs`) |
| :--- | :--- |
| `tracks` | `id` — metadata lagu, `file_path`, `source`, `codec`, `sample_rate`, `bit_depth`, `bitrate_kbps`, `isrc`, `spotify_id`, `youtube_video_id`, `thumbnail_url`, `replay_gain_db`, `liked`, `added_at` |
| `playlists` | `id` — `name`, `source`, `etag`, `description`, `thumbnail_url`, `updated_at` |
| `playlist_tracks` | `(playlist_id, position)` — relasi urutan `track_id` di dalam playlist |
| `music_folders` | `path` — daftar folder lokal yang dipantau/dipindai |
| `app_settings` | `key` — JSON untuk `theme`, `eq`, `playback_dsp`, `accounts`, `updates`, `session_resume`, dan flag migrasi (`migration_pruned_ephemeral_v2`) |
| `lyrics_cache` | `track_key` — cache lirik `.lrc` offline |
| `paired_devices` | `device_id` — daftar perangkat pasangan Ferric Connect |

---

## 6. Sisa Pekerjaan & Roadmap Terstruktur

### A. Stabilisasi Akhir Sebelum Tag Rilis `v0.1.0` (Prioritas Sekarang)
- Pengujian pemakaian harian pengguna pada build terpasang (verifikasi edge case lintas tema `AMOLED Black` / `Native Acrylic` / `Obsidian Studio` / `Warm Cream Light`, sinkronisasi akun, dan stabilitas jangka panjang).
- Penyelesaian bug visual/UX minor yang ditemukan selama pengujian langsung pengguna.
- Finalisasi catatan rilis dan penandaan git tag `v0.1.0` (hanya setelah persetujuan eksplisit pengguna).

### B. Rencana Pasca-`v0.1.0` (Belum Diimplementasikan — Jangan Dicampur dengan Status v0.1.0)
- **Ferric Connect Full Transport:** Mengaktifkan transport jarak jauh lintas jaringan (QUIC P2P / handoff output aktif penuh) di atas framing `postcard` dan `LanDiscoveryBeacon` yang sudah tersedia di `ferric-sync`.
- **Android Mobile App (`apps/ferric-mobile`):** Klien Android native Makepad (`arm64-v8a`) + `MediaSessionService` tipis dengan `oboe` (`PowerSaving`) yang berbagi crate `ferric-core`, `ferric-audio`, `ferric-providers`, dan `ferric-sync`.
- **Sinkronisasi Lintas Perangkat:** Sinkronisasi tema, profil EQ, dan streaming pustaka lokal PC -> HP.
