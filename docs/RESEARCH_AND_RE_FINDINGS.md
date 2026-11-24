# Laporan Riset Ekosistem Rust (Akhir 2026) & Analisis Reverse Engineering

Dokumen ini menyimpan seluruh bukti hasil pembongkaran arsitektur (*static reverse engineering*) terhadap dua artefak referensi (`Sonora-Setup.exe` dan `auralys.4.1.0.final.apk`) menggunakan MCP `rea` (`reverse-engineer-anything`) dan inspeksi biner statis, serta hasil riset ekosistem crate Rust terbaru (akhir 2026) untuk pengembangan aplikasi pemutar musik native lintas platform (Windows + Android).

> [!IMPORTANT]
> **Batasan Keamanan, Hak Cipta, dan Kepatuhan Teknis:**
> Analisis pada dokumen ini dibatasi murni pada **pemetaan arsitektur perangkat lunak, daftar pustaka (*crate/library inventory*), pipeline audio, mekanisme sinkronisasi, dan profil penggunaan memori/CPU** untuk keperluan perancangan *clean-room*. Proyek ini **tidak menyalin kode sumber secara verbatim**, **tidak mengekstrak kunci rahasia/kredensial**, serta **tidak mengimplementasikan pembobolan DRM (Widevine CENC), pemalsuan token internal (`clienttoken`), pembajakan cipher `QuickJS`/`youtubei` maupun gerbang anti-bot (`po_token`), atau bypass iklan pada stream terproteksi milik Spotify maupun YouTube**. Lihat [PRD.md](file:///d:/code/project/music%20player/PRD.md) Bagian 2 untuk arsitektur legal berbasis *Unified Metadata-Audio Decoupling*.

---

## 1. Hasil Analisis Artefak 1: `Sonora-Setup.exe` (Desktop Windows)

### 1.1 Identitas & Struktur Paket
- **Path File:** `d:\code\project\music player\Sonora-Setup.exe`
- **Ukuran Installer:** `23.594.913` byte (~22,5 MB)
- **SHA-256 (`Sonora-Setup.exe`):** `510744f80c9b1e5fd2f0f5282666eb28fded3f549fc5a7f72fce965902be35de`
- **REA Evidence ID:** `ev_1c3f2da42fedbef84e9b6ab983b05b9c1bb8b4bd63e6cc57f227228ab7b0241c` (`inspect_pe_resources` & `open_binary`)
- **Format Installer:** **Inno Setup 6.7.0** (`pe32` loader stub + tabel offset pada resource `RT_RCDATA` ID `11111`: stream header LZMA1 di offset `22390144` dan stream payload solid LZMA2 `zlb\x1a` di offset `849408`).
- **Isi Payload Terdekompresi (`89.139.153` byte / ~85,0 MB):**
  1. `sonora.exe` (`88.900.608` byte / ~84,78 MB — executable utama PE64 native Rust)
  2. `sonora-build.json` (`110` byte — `version: "0.50.3"`, `channel: "stable"`, `commit: "1cb2c2c5044b38c9c99a04eaf91e4411801f71f9"`)
  3. `COPYING` (`35.149` byte — Lisensi **GNU General Public License v3.0 or later**, repositori hulu: `https://github.com/sonorahq/sonora`)
  4. `THIRD_PARTY_LICENSES` (`203.286` byte — 687 crate MIT, 37 Apache-2.0, 19 MPL-2.0, 21 Unicode-3.0)

### 1.2 Pembedahan Tech Stack `sonora.exe` (246 Crate Rust)
Hasil ekstraksi simbol path kompilasi Cargo membuktikan bahwa **Sonora sudah ditulis menggunakan 100% Native Rust** tanpa Electron/Tauri WebView untuk UI utamanya:

| Lapisan | Teknologi / Crate di Sonora v0.50.3 | Analisis Teknis |
| :--- | :--- | :--- |
| **UI Framework** | **GPUI** (`crates\gpui`, `gpui_windows`, `gpui_wgpu` dari editor Zed) + `taffy 0.13.0` + `sum_tree` | Dirender langsung via **DirectX 11 & DirectWrite** (`directx_renderer.rs`, `directx_atlas.rs`, `vsync.rs`). Sangat responsif di Windows, menggunakan virtual list (`uniform_list.rs`). |
| **2D Vector & Image** | `lyon_tessellation 1.0.23`, `tiny-skia 0.12.0`, `resvg 0.49.0`, `etagere 0.2.15`, `zune-jpeg 0.5.15`, `image-webp 0.2.4` | Atlas tekstur GPU (`etagere`) menjaga penggunaan VRAM tetap terkendali; decoding JPEG dipercepat SIMD (`zune-jpeg`). |
| **Audio Decoding** | `symphonia 0.5.5` (MP3, FLAC, AAC, ALAC, Vorbis, ISO/MP4, MKV, RIFF) + `symphonia-adapter-libopus 0.2.9` (`opusic-sys` C binding) + `rusty_aac 1.1.2` | Mendukung hampir semua format audio populer termasuk `.wv` (WavPack) dan `.ape` (Monkey's Audio) pada asosiasi file Windows. |
| **Audio Output & DSP** | `cpal 0.18.2` + `audio_thread_priority 0.35.1` + `rubato 3.0.0` + `realfft 3.5.1` / `rustfft 6.4.1` | Memiliki **Parametric Equalizer** dengan dukungan kompatibilitas preset **Equalizer APO** (`crates\music\src\equalizer\apo.rs`), pemotong hening (*silence trim* `trim.rs`), dan *spectrum visualizer* real-time. |
| **Penyimpanan & Cache** | `rusqlite 0.43.0` (`libsqlite3-sys 0.40.0`) + `moka 0.12.16` + `lofty 0.25.5` + `notify 8.2.2` | Indeks pustaka lokal berbasis SQLite, pembacaan tag ID3/Vorbis/MP4 via `lofty`, dan pemantauan perubahan folder otomatis via `notify`. |
| **Integrasi OS Windows** | `souvlaki 0.9.3` (SMTC), `tray-icon 0.26.1`, `muda 0.21.2`, `accesskit_windows 0.34.0`, `thumbbar.rs` | Mendukung tombol media hardware Windows 11, *Taskbar Thumbnail Toolbar* (`thumbbar.rs`), System Tray native, dan pembaca layar aksesibilitas (`AccessKit`). |
| **Lirik & Bahasa** | `lrclib`, `kugou`, `netease`, `musixmatch`, `binimum` + `kakasi 0.1.0` | Multi-provider lirik sinkron per baris dan per kata (*syllable/word-synced*) + romanisasi lirik Jepang ringan (`kakasi`, jauh lebih kecil dari kamus Kuromoji 33 MB milik Auralys). |
| **Sumber & Ekosistem** | Local Files, **OpenSubsonic (`opensubsonic 0.7.0`)**, **Jellyfin** (`crates\music\src\jellyfin`), YouTube Music (`rquickjs-core 0.12.2`), Scrobbling (`Last.fm`, `ListenBrainz`, `Maloja`), `SponsorBlock`, `Discord Rich Presence` | Arsitektur konektor multi-sumber yang sangat lengkap. |
| **Sesi Jarak Jauh** | `crates\music\src\soloist` (`daemon.rs`, `pair.rs`, `player.rs`, `binary.rs`) + `crates\state\src\listening` (`jam.rs`) + `qrcodegen 1.8.0` + `tokio-tungstenite 0.30.0` | Memiliki fondasi pairing QR dan ruang dengar bersama (*Jam*), namun belum memiliki klien mobile native dan belum memakai P2P NAT traversal seperti `iroh`. |

### 1.3 Kelemahan Utama Sonora yang Harus Diperbaiki di Proyek Kita
1. **Tidak Bisa Jalan di Android (`GPUI` Lock-in):** `gpui` hanya memiliki backend desktop (`gpui_windows`, `gpui_macos`, `gpui_linux`). Tidak ada backend Android resmi pada `gpui`, sehingga kode UI Sonora mustahil dikompilasi menjadi APK Android native.
2. **Pembengkakan Ukuran Binary (`88,9 MB` `.exe`):** Mengemas 5 set ikon sekaligus (Lucide, Iconoir, Remix, Solar, Simple Icons) ke dalam binary dan memiliki banyak duplikasi versi crate (`reqwest 0.12` & `0.13`, `png 0.17` & `0.18`, `tiny-skia 0.11` & `0.12`, `tokio-tungstenite 0.28` & `0.30`, `hashbrown 0.16` & `0.17`). Dengan *deduplication* Cargo + *icon subsetting* + LTO (`lto = "fat"`, `codegen-units = 1`, `strip = true`), binary Rust setara dapat ditekan menjadi **8–15 MB**!
3. **Ketergantungan Rapuh & Risiko Hukum pada Modul DRM/Internal Endpoint:** Sonora memuat modul `crates\widevine` dan memanggil endpoint internal tidak resmi yang bergantung pada Cloudflare Worker eksternal untuk hash GraphQL. Pendekatan tersebut melanggar ketentuan layanan/anti-circumvention dan sangat sering rusak ketika penyedia layanan memutar versi klien. Proyek kita wajib menggantinya dengan **Official OAuth PKCE (`rspotify`) + Clean-Room Audio Resolver**.

---

## 2. Hasil Analisis Artefak 2: `auralys.4.1.0.final.apk` (Mobile Android)

### 2.1 Identitas & Struktur Paket
- **Path File:** `d:\code\project\music player\auralys.4.1.0.final.apk`
- **Ukuran APK:** `36.015.514` byte (~34,35 MB), berisi `857` entri ZIP
- **SHA-256:** `011479a95386ad048368682c088bc13960851d8efc10f3cc5124818fdbf34e43`
- **Application ID:** `com.auralyst.app` (Versi `4.1.0`)
- **Identitas Kode Asli (Hasil Dekompilasi Header DEX):** Merupakan *rebrand/fork* dari proyek open-source Android **Metrolist** (`com.metrolist.music`, `https://github.com/MetrolistGroup/Metrolist`, berlisensi GPL-3.0).

### 2.2 Pembedahan Tech Stack `auralys.4.1.0.final.apk` (13.019 Class di 3 File DEX)
- **Bahasa & UI:** **Kotlin + Jetpack Compose + Material 3** (`androidx.compose.ui`, `foundation`, `material3`, `sh.calvin.reorderable`).
- **Audio Engine:** **AndroidX Media3 ExoPlayer** (`MusicService`, `ExoDownloadService`, `MediaLibraryService`, mendukung Android Auto `com.google.android.gms.car.application` dan Android TV `LEANBACK_LAUNCHER`).
- **Sumber Streaming:** Klien **YouTube Music (`com.metrolist.innertube` & `innertubex`)** menggunakan **Ktor Client + OkHttp** serta engine JavaScript native C **`libquickjs.so`** (~925 KB di `arm64-v8a`) untuk mengeksekusi `assets/solver/yt.solver.core.js` dan `assets/po_token.html`.
- **Fitur Unggulan yang Patut Diambil Inspirasinya (Secara Clean-Room):**
  1. **Integrasi Profil Headphone AutoEq (`com.metrolist.music.eq`):** Mengambil kurva koreksi frekuensi headphone langsung dari repositori terbuka `AutoEq`!
  2. **Listen Together Berbasis Protobuf (`com.metrolist.music.listentogether`):** Sinkronisasi ruang dengar menggunakan skema Protobuf yang rapi (`CreateRoomPayload`, `PlaybackActionPayload`, `BufferWaitPayload`, `BufferReadyPayload` — menunggu semua peserta selesai *buffering* sebelum lagu mulai diputar serentak!).
  3. **Multi-Provider Synced Lyrics + AI Translation:** Menggabungkan `LRCLIB`, `KuGou`, dan `Paxsenix` dengan opsi terjemahan lirik.
  4. **Audio Normalization, Tempo/Pitch Adjustment, SponsorBlock, Scrobbling (`ws.audioscrobbler.com`), Android Quick Settings Tile (`BIND_QUICK_SETTINGS_TILE`), dan App Widgets.**

### 2.3 Kelemahan Utama Auralys yang Harus Diperbaiki di Proyek Kita
1. **Bukan Native Rust & Boros RAM di Android (~140–250 MB RAM):** Kombinasi runtime ART/JVM + pohon komposisi Jetpack Compose + ExoPlayer + Ktor + `libquickjs.so` + **kamus Jepang Kuromoji `ipadic` sebesar ~33,4 MB tidak terkompresi di memori** membuat konsumsi RAM tinggi di HP kelas *entry-level*.
2. **Tidak Ada Aplikasi Desktop Native & Tidak Ada Fitur "Spotify Connect" Pribadi:** Fitur `ListenTogether` di Auralys bergantung pada server relay terpusat (`metrolist.cc`) untuk ruang sosial, bukan protokol *remote control & active speaker handoff* instan antara PC Windows dan HP Android milik satu pengguna di jaringan lokal (LAN) maupun jarak jauh (P2P).
3. **Tidak Mendukung Sinkronisasi Akun/Playlist Spotify Resmi:** Murni ekosistem YouTube Music (`innertube`).

---

## 3. Riset Ekosistem Native Rust Terbaru (Akhir 2026) untuk Windows + Android

Target utama kita adalah **1 codebase Rust penuh** yang berjalan native di **Windows (x86_64/ARM64)** dan **Android (arm64-v8a / armeabi-v7a / x86_64)** tanpa WebView (tanpa Tauri/Electron/Chromium), dengan target **RAM idle 20–45 MB** dan **CPU/GPU 0% saat diminimize atau layar HP mati**.

### 3.1 Matriks Perbandingan GUI Toolkit Native Rust (Windows + Android)

| Toolkit | Versi (Akhir 2026) | Backend Windows | Kesiapan Android | RAM Idle (Desktop) | Virtual List 10.000+ Lagu | Efek Blur / Animasi 120fps | Lisensi |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Makepad** (`makepad-widgets`) | **`1.0.0`** | Native **DirectX 11** (+ DX12/Vulkan) | **Production-Ready** (`cargo-makepad`, GLES/Vulkan, ekosistem Project Robius) | **20–45 MB** | Kelas satu (`PortalList`, hanya me-render baris yang terlihat di layar) | Shader SDF GPU kustom (`live_design!`), transisi miniplayer-ke-fullscreen 120fps super mulus | **MIT / Apache-2.0** |
| **Slint** (`slint`) | **`1.19.0`** | `FemtoVG-wgpu` (DX12), `Skia` (D3D), atau Software | **Production-Ready** (`android-activity 0.6.1`, safe-area, fisika sentuh AOSP `OverScroller`) | **25–40 MB** (`FemtoVG`) / **15 MB** (Software) | Kelas satu (`ListView` + `slint::Model` lazy) | Native `backdrop-blur` (sejak 1.19), animasi properti deklaratif | **GPLv3** atau **Royalty-Free** (wajib atribut) |
| **Dioxus Native** (`dioxus` + `blitz`) | `0.7.10` (`0.8-alpha`) | `Blitz` + `Vello` (`wgpu` compute) | **Beta** (IME & scroll Android masih berkembang) | **45–85 MB** (buffer compute `Vello` cukup berat di GPU murah) | Kustom di atas `blitz-dom` | Subset CSS via Stylo | MIT / Apache-2.0 |
| **Iced** (`iced`) | `0.14.0` | `iced_wgpu` + `tiny-skia` | **Tidak Didukung Resmi** (mobile bukan fokus upstream) | 25–45 MB | Butuh kustomisasi viewport | Terbatas | MIT |
| **GPUI** (`gpui` — dipakai Sonora) | `0.2.2` | Native DirectX 11 | **Tidak Ada Resmi** (hanya fork eksperimental komunitas) | 40–70 MB | Sangat cepat di desktop (`uniform_list`) | Cepat, khusus desktop | Apache-2.0 / GPLv3 |

**Keputusan Rekomendasi UI:**
- **Pilihan Utama (Rekomendasi #1): `Makepad 1.0.0` (`makepad-widgets`)** — Lisensi **100% MIT/Apache-2.0**, sudah teruji di produksi pada Windows (DirectX 11 yang ringan bahkan di laptop iGPU tua) dan Android (Project Robius seperti *Robrix* & *Moly*), memiliki `PortalList` setara `RecyclerView`, *event-driven rendering* (**0% CPU & 0% GPU** saat tidak ada interaksi/animasi), dan memungkinkan shader gradien sampul album dinamis ala Apple Music/Spotify dengan beban sangat ringan.
- **Pilihan Alternatif: `Slint 1.19.0` (`renderer-femtovg-wgpu`)** — Sangat cocok bila tim menginginkan bahasa markup deklaratif `.slint` yang lebih mudah dipelajari dan proyek dirilis sebagai open-source GPLv3 (atau menampilkan badge atribusi Royalty-Free).

### 3.2 Stack Audio, Kontrol Media OS, & Sinkronisasi Antar-Perangkat (Akhir 2026)

| Komponen | Crate & Versi | Alasan Pemilihan & Keunggulan Efisiensi |
| :--- | :--- | :--- |
| **Audio Decoder** | `symphonia 0.6.1` + **`symphonia-opus 0.1.x`** | Di v0.6, `symphonia-opus` adalah decoder Opus **100% pure-Rust (`#![forbid(unsafe_code)]`, zero-allocation)** sehingga kita tidak lagi butuh kompilasi C `libopus` dengan Android NDK seperti yang masih dilakukan Sonora v0.50.3! |
| **Resampler & Ring Buffer** | `rubato 5.1.0` + `rtrb 0.3.1` | SPSC *wait-free ring buffer* (`rtrb`) memisahkan thread jaringan/disk dari callback real-time audio tanpa alokasi memori atau *mutex lock*. Mengalahkan `rodio` (kurang presisi untuk crossfade dua stream) dan `kira` (boros RAM karena memuat seluruh file ke memori). |
| **Output Audio Windows** | `cpal 0.18.2` + opsional `wasapi 0.27.0` | Mode standar memakai WASAPI Shared *event-driven*; pengguna audiophile dapat menyalakan **WASAPI Exclusive Mode** bit-perfect langsung ke DAC. |
| **Output Audio Android** | `oboe 0.6.1` (`PerformanceMode::PowerSaving`) | Menggunakan Android **AAudio** dalam mode **`PowerSaving`** (bukan `LowLatency`!), memungkinkan hardware DSP memproses buffer besar sehingga hemat baterai drastis saat layar mati. |
| **OS Media Controls** | Windows: `souvlaki 0.9.3` (SMTC)<br>Android: Thin Kotlin `MediaSessionService` (~150 baris) + `jni 0.22.4` (`robius-android-env 0.2.0`) | Di Android 14/15/16, sistem operasi akan mematikan proses background dalam <60 detik kecuali diikat ke `ForegroundService` bertipe `mediaPlayback`. Saat layar HP mati atau aplikasi di-minimize, **seluruh surface GPU UI dihancurkan (`0 MB` VRAM, `0%` GPU)** sementara engine Rust tetap memutar musik di background (~15 MB RAM)! |
| **Sinkronisasi PC <-> HP** | **`iroh 1.4.1`** + `iroh-blobs` + `iroh-gossip` + `mdns-sd 0.22.0` + `postcard` | **Tanpa biaya server pusat!** Di Wi-Fi/LAN yang sama, `mdns-sd` menghubungkan PC dan HP dalam <5 ms secara offline. Saat HP memakai data seluler 4G/5G di luar rumah, `iroh` menembus NAT secara P2P terenkripsi Ed25519/QUIC untuk mengontrol musik di PC atau men-stream koleksi FLAC dari PC rumah ke HP! |
| **Integrasi Spotify & YouTube Legal** | `rspotify 0.16.1` + `oauth2 5.0` + `reqwest 0.13` (YouTube Data API v3) | Mendukung login akun **Spotify (Free/Premium)** dan **Google / YouTube & YouTube Music (Free/Premium)** secara bersamaan via OAuth 2.0 PKCE resmi + *short-circuit* `ETag` (`304 Not Modified`) tanpa perlu mengemas `libquickjs.so` (~1,5 MB beban cipher JS). |

---

## 4. Ringkasan Perbandingan: Sonora vs Auralys vs Aplikasi Kita (Codename: **Ferric**)

| Aspek | Sonora v0.50.3 (`.exe`) | Auralys v4.1.0 (`.apk`) | Desain Aplikasi Kita (Full Native Rust) |
| :--- | :--- | :--- | :--- |
| **Platform** | Hanya Desktop (Windows/macOS/Linux) | Hanya Android | **Windows + Android dari 1 Core Rust** |
| **Bahasa & UI** | Rust + GPUI | Kotlin + Jetpack Compose | **Rust + Makepad 1.0 (DX11 di Win, Vulkan/GLES di Android)** |
| **Ukuran Installer / APK** | 22,5 MB setup / **88,9 MB `.exe`** | **36,0 MB `.apk`** (+33 MB kamus di dalam) | Target **< 12 MB `.exe`** & **< 15 MB `.apk`** (per ABI) |
| **RAM Saat Memutar Musik** | ~55–95 MB | ~140–250 MB | **25–45 MB (UI aktif)** / **12–20 MB (minimized / layar mati)** |
| **CPU / GPU Saat Idle/Background** | Rendah (~0–1%) | Sedang (GC ART/JVM) | **0,0% GPU & < 0,5% CPU** (surface GPU dilepas saat background) |
| **Login Akun Musik** | Endpoint internal tidak resmi (`pathfinder` / `innertube` + `rquickjs`) | Hanya YT Music tidak resmi (`innertube` + `libquickjs.so` + `po_token`) | **Multi-Akun Resmi via OAuth 2.0 PKCE: Spotify (Free/Premium) + YouTube & YT Music (Free/Premium) + Bridge Playlist 2 Arah** |
| **Kontrol Silang PC <-> HP** | Terbatas (`soloist` lokal, tanpa aplikasi HP) | Hanya grup sosial `ListenTogether` via server pusat | **Native Bidirectional "Connect" (`iroh 1.4` LAN + P2P Internet) + Direct PC-to-Phone Streaming** |
| **Equalizer & DSP** | Parametric EQ + Equalizer APO | System EQ + AutoEq | **10-Band Parametric Biquad EQ di Rust + Import AutoEq & Equalizer APO + ReplayGain + Crossfade** |


---

## 5. Deep Feature & DSP Audit Lintas 3 Artefak Referensi (`Sonora` + `Auralys` + `Spotify`)

Bagian ini mendokumentasikan hasil ekstraksi fitur tingkat layar UI, kunci konfigurasi (*preference keys*), dan parameter transisi audio menggunakan MCP `rea` (`open_binary`, `inspect_artifact`, `inspect_pe_resources`) dan parser statis DEX/LZMA2 terhadap ketiga artefak referensi di `reference/`:
- **`Spotify 9.1.30.1993 [ReVanced + AB] (Merged).apk`** — SHA-256: `30a41a84cc5847fff66708054b5a4e88b89e7b98b9b09421a5d287889be9b220` (`4.141` entri ZIP, `12` file DEX, `26` pustaka native)
- **`auralys.4.1.0.final.apk`** — SHA-256: `011479a95386ad048368682c088bc13960851d8efc10f3cc5124818fdbf34e43` (`857` entri ZIP, `3` file DEX, `93` komponen layar UI, `363` preference keys)
- **`Sonora-Setup.exe`** — SHA-256: `510744f80c9b1e5fd2f0f5282666eb28fded3f549fc5a7f72fce965902be35de` (payload `sonora.exe` v0.50.3 `89.139.153` byte)

### 5.1 Arsitektur Transisi Tanpa Jeda (*Seamless Fade-In, Crossfade & Automix*)
Dari pembongkaran parameter internal Spotify (`audio.crossfade_v2`, `audio.fade_in_curves`, `audio.fade_in_duration`, `audio.fade_in_start_time`, `BEST_FADE_IN_CUEPOINT_FIELD_NUMBER`, `BEST_FADE_OUT_CUEPOINT_FIELD_NUMBER`, `BassCrossfade`, `automix.auto_transition_recipe`), Auralys (`CrossfadeEnabledKey`, `CrossfadeDurationKey`, `CrossfadeGaplessKey`, `SkipSilenceKey`, `SleepTimerFadeOutKey`), dan Sonora (`Gapless playback`, `trim_silence`, `ReplayGain`), ditemukan 5 mekanisme kunci yang membuat perpindahan lagu terasa menyatu tanpa jeda (*zero-gap fade-in*):

1. **Dual-Deck Pre-Buffering (`Deck A` + `Deck B`):** Lagu berikutnya sudah dibuka, didekode ke PCM `f32`, dan di-resample ke sample rate hardware sebelum lagu aktif mencapai jendela akhir (`T - max(crossfade_ms + 4000, 8000)`), sehingga tidak ada jeda I/O disk/jaringan saat pergantian lagu.
2. **Deteksi Cuepoint Hening (*Intro/Outro Silence Trim*):** Jika bagian akhir `Deck A` turun di bawah ambang hening (`-50 dBFS`) dalam 4,5 detik menjelang jendela transisi, `Deck B` langsung mulai *fade-in* tanpa menunggu sisa keheningan file habis; sampel hening di awal `Deck B` (`-52 dBFS`) juga langsung dilewati.
3. **Bass-Swap Automix (`BassCrossfade` < 140 Hz):** Saat dua lagu bertumpuk selama crossfade, penjumlahan frekuensi rendah dari dua kick/bass dapat memicu *muddy boom* (+6 dB). Filter crossover satu kutub `140 Hz` menahan bass lagu baru hingga titik tengah transisi lalu menukarnya secara halus dengan bass lagu lama, sementara vokal/instrumen mengikuti kurva *Equal-Power* ($\cos^2 + \sin^2 = 1$) atau *Hermite S-Curve*.
4. **Album-Aware Gapless Lock (`CrossfadeGaplessKey`):** Jika dua lagu berurutan berasal dari album yang sama, crossfade dilewati (`0 ms`) agar sambungan album konsep/live tetap presisi hingga tingkat sampel.
5. **Micro-Fade pada Play, Pause, Seek, dan Sleep Timer:** Envelope halus `240 ms` saat *Play/Seek* dan `120 ms` saat *Pause* menghilangkan bunyi klik transien, ditambah *fade-out* bertahap 5 detik saat *Sleep Timer* berakhir.

### 5.2 Matriks Paritas Fitur Lengkap Ferric Desktop v0.1.0
| Kategori Fitur | Bukti di Spotify / Auralys / Sonora | Implementasi Clean-Room di Ferric |
| :--- | :--- | :--- |
| **Seamless Fade-In & Automix** | `audio.fade_in_curves`, `BassCrossfade`, `CrossfadeGaplessKey`, `SkipSilenceKey` | Dual-deck `Deck A + Deck B` pre-buffered overlap (`0–12 dtk`), Play/Seek micro-fade (`0–1500 ms`), 3 kurva (`Equal-Power`, `S-Curve Velvet`, `Bass-Swap Automix`), `Gapless Album Lock`, `Trim Intro/Outro Silence` |
| **Login Akun Resmi & Sync** | Spotify Connect/Library, YT Music Library, Sonora OpenSubsonic | Halaman khusus **Accounts & Cloud Sync** dengan OAuth 2.0 PKCE + Loopback HTTP Callback (`127.0.0.1:43824/callback`) untuk **Spotify**, **Google / YouTube & YT Music** (`ETag 304` + `categoryId=10`), dan **OpenSubsonic / Navidrome**, ditambah pencocokan ISRC otomatis ke file FLAC/Opus lokal |
| **Parametric EQ & AutoEq** | Auralys `GitHubAutoEqSearch` + `BiquadFilter`, Sonora `Equalizer APO` | Grafik respons frekuensi Biquad interaktif (`20 Hz–20 kHz`), 10 fader vertikal (`32 Hz–16 kHz`), Preamp, pencarian **24 profil headphone AutoEq**, Import/Export `.txt` Equalizer APO |
| **Kontrol Ekstra Pemutaran** | `SleepTimerFadeOutKey`, `VarispeedKey`, `mono_audio`, `AudioNormalizationKey` | **Sleep Timer** (`15m`, `30m`, `45m`, `60m`, `End of Track` dengan fade-out), **Playback Speed** (`0.75x–1.5x`), **ReplayGain**, **Mono Audio**, **Floating Compact Mini-Player**, dan **Command Palette (`Ctrl+K`)** |
| **Ferric Connect & Lyrics** | Spotify `Connect` + `Jam`, Auralys `ListenTogether` + `LRCLIB`, Sonora `qrcodegen` | Encoder matriks **QR Code ISO/IEC 18004** pure-Rust di kanvas UI, penemuan peer **UDP LAN Discovery**, Active Speaker Lease, serta **Synced Lyrics (LRCLIB + `.lrc` lokal)** |
