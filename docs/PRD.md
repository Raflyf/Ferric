# Product Requirements Document (PRD): Ferric Native Rust Music Player

- **Versi Dokumen:** 2.2.0 (Diselaraskan dengan Implementasi Produksi Pre-Release `v0.1.0`)
- **Tanggal Pembaruan:** 26 November 2026
- **Status Proyek:** **Windows Desktop `v0.1.0` Siap Rilis (Feature-Complete & Security-Hardened)** — seluruh fitur inti, mesin audio dual-deck, tampilan default AMOLED Black & cold-start Native Acrylic dengan **Adaptive Acrylic Tint**, kebijakan DX11 hemat-RAM (prioritas Integrated GPU -> fallback GPU utama / WARP CPU tanpa membangunkan dGPU), animasi GPU 60 FPS (Fullscreen Lyrics 2-fase & Cover-to-Video morph), Recommended Stations + Home exploration kaya, font internasional OS tanpa bloat, filter lirik LRCLIB + auto-heal, SMTC penuh (thumbnail + timeline + seek), menu tray dark/light mengikuti sistem, Discord Rich Presence, Taskbar thumbnail controls, keamanan DPAPI/WebView2/Win32, pengerasan PE anti-RE, dan paket installer MSI dengan **clean uninstall sampai akar** sudah berjalan dan terverifikasi; menunggu persetujuan eksplisit pengguna untuk menandai rilis `v0.1.0`. Ekspansi Android Mobile menjadi fase pasca-`v0.1.0`.
- **Target Platform Saat Ini:** Windows 10/11 (`x86_64`, MSI per-user & Portable ZIP)
- **Target Platform Lanjutan:** Android 8.0–16+ (`arm64-v8a`) berbagi crate inti `ferric-core`, `ferric-audio`, `ferric-providers`, dan `ferric-sync`
- **Bahasa & Runtime:** 100% Native Rust (Edition 2024) + `makepad-widgets 1.0.0` (DirectX 11 + DirectComposition)
- **Dokumen Terkait:** [DESIGN.md](./DESIGN.md), [SYNC_AND_AUDIO_SPEC.md](./SYNC_AND_AUDIO_SPEC.md), [RESEARCH_AND_RE_FINDINGS.md](./RESEARCH_AND_RE_FINDINGS.md), [AGENT_HANDOFF.md](./AGENT_HANDOFF.md)

---

## 1. Visi Produk & Posisi Saat Ini

**Ferric** adalah pemutar musik desktop modern berarsitektur **100% Native Rust** tanpa Electron/Chromium untuk UI utamanya, dirancang untuk menggabungkan pengalaman streaming & pustaka musik lengkap (pemutaran lokal lossless, katalog daring, rekomendasi Home personal, radio otomatis multi-artis, lirik sinkron internasional, video klip HD dalam aplikasi, 10-band Parametric EQ + AutoEq, Discord Rich Presence, dan sinkronisasi lintas perangkat) dengan jejak memori **di bawah 50 MB saat memutar musik**, **di bawah 10 MB saat idle/minimized**, serta standar keamanan dan pengerasan biner setara aplikasi produksi.

Ferric merupakan bagian dari keluarga visual **LivelyDesk** (`LivelyDesk` · `WallpaperLive` · `TransparentBar` · `Ferric`), menggunakan default instalasi awal **AMOLED Black (`#000000`, aksen Burnt Orange `#E86A2C`, `acrylic_enabled: false`)** serta tema opsional **Native Windows 11 Acrylic** per-piksel berbasis DirectComposition yang langsung transparan sejak *cold start*.

---

## 2. Target & Hasil Terukur Performa & Keamanan (Windows 11 Terverifikasi)

| Metrik | Target Anggaran | Hasil Terukur Aktual (`5c81b11`) | Mekanisme Implementasi |
| :--- | :--- | :--- | :--- |
| **RAM (Idle, Jendela Terbuka)** | `< 45 MB` | **`7,6 MB`** Working Set | SQLite WAL ringan + `shrink_memory()` + Win32 working-set trim pasca-inisialisasi |
| **RAM (Streaming Aktif + Artwork + Lirik)** | `< 100 MB` | **`41,3–44,1 MB`** Working Set | Ring buffer `rtrb` streaming + LRU 128 tekstur (`artwork.rs`) + font OS via `Rc<Font>` |
| **RAM (Minimized / System Tray)** | `< 25 MB` | **`7,3 MB`** Working Set | Menghentikan loop redraw UI saat tersembunyi + eviksi tekstur non-aktif + trim working set |
| **CPU (Settled / Pasca-Tutup Fullscreen)** | `< 2%` | **`0,52%`** | Loop animasi 60 FPS berhenti penuh segera setelah `fs_reveal_t` / `fs_video_morph_t` tuntas |
| **GPU (Idle / Tersembunyi)** | `0%` | **`0%`** (event-driven) | Loop redraw DX11 berhenti penuh saat jendela tersembunyi atau tidak ada animasi aktif |
| **Ukuran Binary Hardened (`ferric.exe`)** | `< 20 MB` | **`14,0 MB`** (`14.008.320` byte) | `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`, `panic = "abort"`, 0 byte font CJK tertanam |
| **PE Hardening & Zero Leak Gate** | 100% lulus | **Lulus (`verify_pe_hardening.py`)** | `GUARD_CF`, `CETCOMPAT`, `DYNAMICBASE`, `HIGHENTROPYVA`, `NXCOMPAT`, `BREPRO`, 0 `.pdb`/path host |
| **Shader / Apply / Resource Error** | `0` | **`0` / `0` / `0`** | Seluruh shader Makepad DX11 dan virtual font OS tervalidasi tanpa error |
| **Single-Instance & Identitas OS** | 1 proses, nama bersih | **Terverifikasi (`Ferric`)** | Mutex `Local\FerricMusicPlayerSingleInstance_v1` + `AppUserModelID = Raflyf.Ferric` |

---

## 3. Arsitektur Workspace Cargo Aktual

```text
ferric/
├── Cargo.toml                  # Workspace root (v0.1.0, Edition 2024, lto="fat", codegen-units=1, strip="symbols", panic="abort")
├── crates/
│   ├── ferric-core/            # db.rs (SQLite WAL), models.rs, queue.rs, theme.rs (4 preset + WCAG AdaptivePalette),
│   │                           # eq.rs, secrets.rs (Windows DPAPI + OS CSPRNG)
│   ├── ferric-audio/           # engine.rs (Dual-Deck A+B), dsp.rs (Bass-Swap/EQ/Spectrum), demo_synth.rs
│   ├── ferric-providers/       # ytmusic.rs, yt_cleaner.rs, lyrics.rs (filter multi-bahasa + auto-heal), local_scanner.rs,
│   │                           # public_sync.rs, oauth.rs, subsonic.rs, autoeq.rs, resolver.rs, updater.rs
│   └── ferric-sync/            # lib.rs (postcard frames, ActiveSpeakerLease), lan.rs (UDP 43821), qr.rs
├── apps/
│   └── ferric-desktop/         # main.rs, app.rs, widgets.rs, track_list.rs, state.rs, theme.rs, icons.rs,
│                               # artwork.rs, shell.rs, smtc.rs, discord_rpc.rs, google_login.rs, video_player.rs, build.rs
├── vendor/
│   └── makepad-platform/       # Patch lokal: DirectComposition cold-start Acrylic, WM_NCACTIVATE forwarding,
│                               # geometri maximize presisi, dan loader font OS internasional (cx_native.rs)
├── installer/                  # Ferric.wxs (WiX v3 per-user), ONLINE-NOTICE.txt
└── scripts/
    ├── Installer.ps1           # Paket WiX v3 per-user MSI
    └── verify_pe_hardening.py  # Gerbang verifikasi flag PE & nol kebocoran PDB/path
```

### Dependensi Inti Terkunci (`Cargo.toml`)

| Lapisan | Crate & Versi Aktual | Fungsi |
| :--- | :--- | :--- |
| **UI Native GPU** | `makepad-widgets 1.0.0` + `vendor/makepad-platform` | Rendering DX11 + DirectComposition per-pixel Acrylic, `PortalList`, shader SDF, virtual OS font loader |
| **Audio Decode** | `symphonia 0.6.1` + `symphonia-opus 0.1.3` | Dekoder pure-Rust (FLAC, MP3, AAC/M4A, ALAC, Ogg Vorbis, Opus, WAV, AIFF) |
| **DSP & Resampling** | `rubato 5.1.0` + `biquad 0.6.0` + `rtrb 0.4.1` | Resampler, 10-band Parametric Biquad EQ, wait-free SPSC ring buffer |
| **Audio Output** | `cpal 0.18.2` | Output WASAPI Windows dengan callback real-time bebas alokasi |
| **Database & Tag** | `rusqlite 0.43` (`bundled`) + `lofty 0.25.5` + `walkdir 2.5` | SQLite mode WAL + pembaca tag audio lokal & pemindaian folder |
| **Jaringan & Sync** | `ureq 3.5` (`json`) + `postcard 1.2` + `sha2 0.10` + `aho-corasick 1.1.5` | Klien HTTP sinkron ringan, framing biner Ferric Connect, verifikasi SHA-256, normalisasi judul |
| **Integrasi Windows** | `windows 0.62` + `souvlaki 0.9.3` + `rfd 0.18.0` + `webview2-com 0.39.1` + `winresource 0.1.34` | Subclass Win32, mitigasi proses, DWM Acrylic, System Tray, Taskbar ThumbBar, SMTC, dialog native, WebView2 sandbox, PE manifest & icon |

---

## 4. Fitur yang Sudah Diimplementasikan (Desktop `v0.1.0`)

### 4.1 Navigasi, Tema & Manajemen Pustaka (9 Halaman)
1. **Home:** Rekomendasi personal berlapis (*taste-driven* hingga 60 lagu / 32 artis lintas akun), mood filter chips, `Quick Picks` (2x4 tile), rak **Recommended Stations** (mesin ganda live Pathfinder + kartu radio triptych pastel 3-lingkaran algoritmis dengan tombol `Refresh` dan `+ Save Playlist`), serta hingga 8 rak rekomendasi kaya (menggabungkan Home, Explore, 3 radio artis, dan pencarian album/playlist/artis terdiversifikasi + `Load More` personal tak terbatas), `Play All` & `Shuffle`.
2. **Search:** Pencarian inline *live debounced* di top bar lengkap dengan tombol clear (`x`), chip riwayat pencarian terpadu, filter (`All`, `Songs`, `Albums`, `Artists`, `Playlists`, `Local`), dan kartu Album/Artis/Playlist.
3. **Recents:** Riwayat putar kronologis lengkap dengan statistik total pemutaran dan durasi mendengar, plus `Play All` dan `Clear History`.
4. **Songs:** Daftar lagu tervirtualisasi (`TrackList`) dengan default `Sort: Recent`, filter sumber (`All`, `Liked`, `Local`, `Spotify`, `YouTube`), filter teks kaca transparan, `Add Folder`, dan aksi per baris (`Play Next`, `Like`, `Menu`/`Add to Playlist`, `Delete`). Hasil jelajah/pencarian/radio sementara tidak mengotori tabel `Songs`.
5. **Albums, Artists & Playlists:** Grid visual 5 kolom x 4 baris (**20 kartu per halaman**) dengan pagination (`Page X / Y`), spacer `ghost`, filter sumber akun (`All` / `YouTube` / `Spotify` / `Premium` / `Local`), urutan **terbaru di paling atas** (`Newest` / `Oldest` / `A-Z`) tersinkron dengan sidebar kiri, normalisasi nama album bersih, empty state terstruktur, dan importer playlist (`.m3u`, `.m3u8`, `.csv`).
6. **Detail View:** Hero header + aksi (`Play`, `Shuffle`, `Start Radio`, `Pin`, `+ Save Playlist` untuk koleksi/stasiun daring, `+ Queue`, `Export M3U`, `Rename`, `Delete`) + `TrackList`.
7. **Settings Hub (7 Tab):** `Accounts` (Google/YouTube, Spotify Premium OAuth PKCE, Spotify Web Sign-In, Public URL Import, Subsonic), `Playback`, `Equalizer`, `Appearance` (4 preset + GPU HSV Color Picker + Acrylic tint/opacity + **Adaptive Acrylic Tint** toggle + WCAG contrast + UI zoom + JSON import/export), `Integrations` (Discord Rich Presence), `Storage`, dan `Connect`.

### 4.2 Mesin Audio, DSP & Smart Radio Queue
- **Dual-Deck `Deck A + Deck B`:** Pre-buffering lagu berikutnya, transisi **auto-only crossfade (`0–12 dtk`)** (skip manual Next/Prev langsung berpindah instan tanpa tertahan crossfade), pilihan kurva **Bass-Swap Automix (`140 Hz`)**, **S-Curve Velvet**, dan **Equal-Power**, **Gapless Album Lock** (`0 ms` pada album lokal berurutan), pemotong hening awal/akhir, micro-fade Hermite pada Play/Pause/Seek, ReplayGain, Playback Speed (`0.75x–1.5x`), Mono Audio, dan Sleep Timer (`15/30/45/60m` / `End of Track`).
- **10-Band Parametric EQ:** Grafik respons frekuensi GPU interaktif, 10 fader (`32 Hz–16 kHz`), Preamp, pencarian **24 profil headphone AutoEq**, dan import/export **Equalizer APO `.txt`**.
- **Smart Multi-Artist Radio Queue:** Ekspansi radio dua lompatan (`RDAMVM`), koherensi bahasa/vibe, batas ketat maksimal 1 lagu per artis (artis lagu aktif maksimal 1 kali dan tidak pernah di posisi #1), penyaringan ketat terhadap variasi lagu yang sama/cover/karaoke/pitch-shift/SEO spam, serta tombol **Refresh Queue** berotasi.

### 4.3 Lirik Fullscreen 60 FPS, Video Klip HD, Font Internasional & Integrasi Desktop
- **Animasi 60 FPS Fullscreen Lyrics (`F` / `F11`):** Transisi buka/tutup **2-fase** (`0.0 -> 0.58` tabir aurora 21-tap mencapai opak penuh di atas shell aktif; `0.52 -> 1.0` konten meluncur naik `42px` tanpa efek *ghosting*; tutup membalik ~240ms lalu memangkas working set).
- **Lirik Sinkron Internasional Bersih:** Waterfall 4 tahap (`.lrc` lokal -> embedded tag -> `LRCLIB` -> `Lyrics.ovh`) dengan **penalti polusi fan-sub multi-bahasa & mojibake** (`lyrics_content_penalty` + `is_polluted_multilingual_lrc`) dan *auto-heal* cache SQLite, iluminasi penuh seragam di seluruh baris terbungkus (*uniform full-line active illumination*), klik baris untuk seek, offset `-0.5s`/`+0.5s`, serta **font fallback OS tanpa bloat** (Jepang, Korea, Mandarin, Thailand).
- **In-App HD Video Clip & Cover-to-Video Morph:** Resolusi video musik resmi, animasi morph 60 FPS antara Cover Art dan Video Klip dengan footer tinggi tetap `54px`, WebView2 `#stage` fade-in 220ms, dan satu sudut membulat `16px` DPI-presisi (`CreateRoundRectRgn`) tersinkron dengan audio WASAPI.
- **Integrasi Windows & Discord:** Single-instance guard, registrasi AppUserModelID (`Raflyf.Ferric`), Task Manager & MuiCache bernama bersih `Ferric`, judul jendela dinamis *now-playing*, **Windows Taskbar Thumbnail Media Buttons** (`Prev`/`Play-Pause`/`Next`), **Windows 11 SMTC penuh** (thumbnail sampul lokal `file://` + fallback HTTPS, timeline `Position`/`EndTime` 1 Hz + seek dari overlay pihak ketiga seperti Nusa Pro, NotchPixel, dan ModernFlyouts), System Tray dengan menu klik kanan Dark/Light mengikuti tema Windows (`Close to Tray`), scroll-wheel volume, **clean root-level MSI uninstall** (menghapus biner, shortcut, registry, DB SQLite, cache artwork/stream, profil WebView2, dan berkas temp hanya pada full uninstall), serta **Discord Rich Presence** native zero-dependency (`Listening to Ferric`, App ID bawaan, lencana ikon kaset, menyembunyikan `large_text` duplikat saat album sama dengan judul).

### 4.4 Keamanan, Privasi, PE Hardening & Batasan Integrasi Eksternal
- **Proteksi Kredensial Lokal (DPAPI):** Seluruh token OAuth, refresh token, cookie, SAPISID, dan kata sandi Subsonic di `app_settings.accounts` dienkripsi di disk menggunakan Windows DPAPI (`dpapi:v1:` via `crypt32.dll` + CSPRNG `BCryptGenRandom`), dengan migrasi otomatis untuk data plaintext lama.
- **Mitigasi Proses Win32 & Sandbox WebView2:** Hardening startup (`SetDllDirectoryW("")`, `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)`, `HeapSetInformation`, `SetProcessMitigationPolicy`), WebView2 terkunci (CSP ketat, `no-referrer`, DevTools/context-menu/host-object/web-message/popup/izin dimatikan, allowlist URL ketat, validasi path `file:///` lokal tanpa traversal/UNC), serta validasi ulang SHA-256 tahan-TOCTOU sebelum meluncurkan `%SystemRoot%\System32\msiexec.exe`.
- **Reproducible Anti-RE PE Build:** Fat LTO, `strip = "symbols"`, `panic = "abort"`, `/GUARD:CF`, `/CETCOMPAT`, `/DYNAMICBASE`, `/HIGHENTROPYVA`, `/NXCOMPAT`, `/DEBUG:NONE`, `/NOCOFFGRPINFO`, `/BREPRO`, serta pemetaan ulang path build (`CARGO_HOME=C:\c`, `subst S:`, `--remap-path-prefix`, `/d1trimfile`) yang diaudit otomatis oleh `scripts/verify_pe_hardening.py`.
- **Cakupan Resmi Integrasi Spotify:** Impor URL playlist/album/track publik bekerja langsung tanpa login (`public_sync.rs`). Sinkronisasi akun resmi Spotify melalui OAuth 2.0 PKCE (`oauth.rs`, scope baca-saja `user-read-private playlist-read-private playlist-read-collaborative user-library-read` dengan Client ID developer resmi pengguna) melayani sinkronisasi metadata pustaka/playlist untuk akun Free maupun Premium; pemutaran audio tetap berasal dari berkas lokal atau aliran publik resmi tanpa melewati mekanisme DRM.

---

## 5. Skema Database SQLite Aktual

Lokasi berkas: `%LOCALAPPDATA%\LivelyDesk\Ferric\data\ferric.db` (`PRAGMA journal_mode = WAL; synchronous = NORMAL; foreign_keys = ON;`).

| Tabel | Kunci Utama & Fungsi Aktual (`crates/ferric-core/src/db.rs`) |
| :--- | :--- |
| `tracks` | `id` — metadata lagu, `file_path`, `source`, `codec`, `sample_rate`, `bit_depth`, `bitrate_kbps`, `isrc`, `spotify_id`, `youtube_video_id`, `thumbnail_url`, `replay_gain_db`, `liked`, `added_at` |
| `playlists` | `id` — `name`, `source`, `etag`, `description`, `thumbnail_url`, `created_at` (ms, urutan terbaru di atas), `updated_at` |
| `playlist_tracks` | `(playlist_id, position)` — relasi urutan `track_id` di dalam playlist |
| `music_folders` | `path` — daftar folder lokal yang dipantau/dipindai |
| `app_settings` | `key` — JSON untuk `theme`, `eq`, `playback_dsp`, `accounts` (rahasia terenkripsi `dpapi:v1:`), `updates`, `session_resume`, `discord_rpc`, dan flag migrasi (`migration_pruned_ephemeral_v2`) |
| `lyrics_cache` | `track_key` — cache lirik `.lrc` offline (dengan *auto-heal* bila entri lama terdeteksi tercemar fan-sub multi-bahasa/mojibake) |
| `paired_devices` | `device_id` — daftar perangkat pasangan Ferric Connect |

---

## 6. Sisa Pekerjaan & Roadmap Terstruktur

### A. Penutupan Rilis `v0.1.0` (Prioritas Sekarang)
- Seluruh perbaikan visual, animasi 60 FPS, cold-start Acrylic, font internasional OS, kebersihan lirik LRCLIB, keamanan, dan PE hardening sudah tuntas dan terverifikasi live pada `5c81b11`.
- Menunggu konfirmasi akhir pengguna untuk menandai dan memublikasikan git tag `v0.1.0` ke GitHub Releases.

### B. Rencana Pasca-`v0.1.0` (Belum Diimplementasikan — Jangan Dicampur dengan Status `v0.1.0`)
- **Ferric Connect Full Transport:** Mengaktifkan transport jarak jauh lintas jaringan (QUIC P2P / handoff output aktif penuh) di atas framing `postcard` dan `LanDiscoveryBeacon` yang sudah tersedia di `ferric-sync`.
- **Android Mobile App (`apps/ferric-mobile`):** Klien Android native Makepad (`arm64-v8a`) + `MediaSessionService` tipis dengan `oboe` (`PowerSaving`) yang berbagi crate `ferric-core`, `ferric-audio`, `ferric-providers`, dan `ferric-sync`.
- **Sinkronisasi Lintas Perangkat:** Sinkronisasi tema, profil EQ, dan streaming pustaka lokal PC -> HP.
