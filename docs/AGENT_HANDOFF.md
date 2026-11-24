# Panduan Handoff untuk AI Agent Berikutnya (`AGENT_HANDOFF.md`)

Selamat datang di direktori kerja proyek pemutar musik lintas platform **Full Native Rust** (Windows + Android), Codename **Ferric**. Dokumen ini disiapkan agar AI agent mana pun (Claude Code, Codex, Cursor, Gemini, Antigravity, Hermes, OpenCode) dapat langsung melanjutkan eksekusi kode dari tahap riset tanpa harus mengulang analisis dari nol.

---

## 1. Peta Dokumen di Folder Ini

| File | Isi & Kapan Harus Dibaca |
| :--- | :--- |
| [PRD.md](file:///d:/code/project/music%20player/PRD.md) | **Dokumen Utama (Wajib Baca Pertama):** Visi produk, batasan kepatuhan legal (*Metadata-Audio Decoupling* tanpa pembobolan DRM), target efisiensi RAM/CPU/GPU, struktur Cargo workspace, daftar crate Rust (akhir 2026), fitur lengkap, inovasi di atas kompetitor, skema SQLite, dan roadmap 5 fase. |
| [DESIGN.md](file:///d:/code/project/music%20player/DESIGN.md) | **Panduan Sistem Desain UI/UX & Brand:** Arah visual **3 Warna LivelyDesk-Family di atas Pure OLED Pitch Black (`#000000`)** + **Open Theme Studio** tanpa batas + toggle **Liquid Glass Theme (`OFF` default / `ON` Makepad SDF Refraction Shader)** di seluruh UI/UX Windows & Android, ikon resmi **Ferric Oxide Cassette** ([ferric.svg](file:///d:/code/project/music%20player/assets/brand/ferric.svg), [ferric.png](file:///d:/code/project/music%20player/assets/brand/ferric.png), [ferric.ico](file:///d:/code/project/music%20player/assets/brand/ferric.ico)), tipografi `Nunito Sans` + `Liberation Mono`, dan peta 6 halaman desain FigCraft ([ferric-ui-ux.figcraft](file:///d:/code/project/music%20player/ferric-ui-ux.figcraft)). |
| [RESEARCH_AND_RE_FINDINGS.md](file:///d:/code/project/music%20player/RESEARCH_AND_RE_FINDINGS.md) | **Bukti Riset & Reverse Engineering:** Hasil bedah statis `Sonora-Setup.exe` (`v0.50.3` — Rust + `GPUI` desktop) dan `auralys.4.1.0.final.apk` (`v4.1.0` — fork Kotlin/Compose `Metrolist`), alasan teknis kelemahan masing-masing, serta benchmark komparatif toolkit GUI Rust (`Makepad 1.0.0`, `Slint 1.19.0`, `Dioxus 0.7.10`, `Iced 0.14.0`, `GPUI 0.2.2`). |
| [SYNC_AND_AUDIO_SPEC.md](file:///d:/code/project/music%20player/SYNC_AND_AUDIO_SPEC.md) | **Spesifikasi Implementasi Teknis:** Protokol biner `iroh 1.4.1` + `mdns-sd` + `postcard` untuk kontrol bolak-balik PC <-> HP (*Ferric Connect*), rumus *gapless/equal-power crossfade* pada `symphonia 0.6.1` + `rubato 5.1.0` + `rtrb`, kontrak trait `AudioResolver`, dan manajemen *Zero-GPU Background Mode* di Android & Windows. |
| `Sonora-Setup.exe` & `auralys.4.1.0.final.apk` | Dua file biner referensi yang diberikan pengguna (jangan dihapus atau dimodifikasi; hasil bedah lengkapnya sudah didokumentasikan di `RESEARCH_AND_RE_FINDINGS.md`). |

---

## 2. Keputusan Arsitektur Kunci yang Sudah Final

1. **Jangan Pakai Electron, Tauri/WebView, Flutter, maupun `GPUI`:**
   - WebView/Chromium melanggar target hemat RAM (< 45 MB aktif, < 20 MB background).
   - `GPUI` (yang dipakai Sonora) tidak memiliki backend Android resmi.
   - Gunakan **`makepad-widgets = "1.0.0"`** (MIT/Apache-2.0, DirectX 11 di Windows + OpenGL ES/Vulkan di Android, `PortalList` virtualisasi list, 0% CPU/GPU saat idle) — atau `slint = "1.19.0"` (`renderer-femtovg-wgpu`) hanya jika pengguna secara eksplisit meminta bahasa markup `.slint`.
2. **Jangan Pakai C `libopus` (`symphonia-adapter-libopus`) atau `rodio`/`kira`:**
   - Gunakan **`symphonia = "0.6.1"` + `symphonia-opus`** (100% safe pure-Rust Opus decoder sehingga cross-compile ke Android NDK bebas masalah C toolchain) + `rubato = "5.1.0"` + `rtrb = "0.3.1"`.
   - Output audio: `cpal = "0.18.2"` (+ opsional `wasapi = "0.27.0"` Exclusive Mode) di Windows, dan **`oboe = "0.6.1"` dengan `PerformanceMode::PowerSaving`** di Android.
3. **Sinkronisasi Desktop <-> Mobile Tanpa Biaya Server:**
   - Gunakan **`iroh = "1.4.1"` + `iroh-blobs` + `mdns-sd = "0.22.0"` + `postcard`**. Satu protokol menangani penemuan instan di Wi-Fi lokal maupun kontrol jarak jauh / streaming koleksi lagu PC rumah ke HP via 4G/5G.
4. **Kepatuhan Ketat Terkait Spotify, YouTube / YouTube Music, & DRM:**
   - **Dilarang keras** mengimplementasikan dekripsi Widevine/DRM, pencurian kunci CDM, spoofing `clienttoken` internal Spotify, pembajakan cipher `QuickJS`/`youtubei/v1/player`, bypass anti-bot `po_token`, maupun penghapusan iklan dari stream server terproteksi Spotify/YouTube.
   - Login akun mendukung **Multi-Akun Resmi via OAuth 2.0 PKCE** dengan skema BYO Client ID:
     1. **Spotify (Free & Premium):** Menggunakan **Official Spotify Web API (`rspotify = "0.16.1"`)** + sinkronisasi inkremental `snapshot_id`.
     2. **Google untuk YouTube & YouTube Music (Free & Premium):** Menggunakan **Official YouTube Data API v3 (`oauth2 = "5.0"` + klien REST `reqwest` ringan tanpa crate `google-youtube3` yang berat)** + sinkronisasi inkremental header `ETag` (`304 Not Modified`), playlist `LL` (*Liked Songs*), filter `videoCategoryId = 10`, pembersih judul `yt_metadata_cleaner`, dan jembatan migrasi/sinkronisasi 2 arah **Spotify <-> YouTube Music**.
   - Seluruh katalog dari kedua akun dipetakan melalui trait `AudioResolver` ke file lokal, streaming P2P dari PC rumah (`iroh-blobs`), server `OpenSubsonic`/`Jellyfin`, katalog musik terbuka bebas royalti, atau plugin resolver eksternal berpasir WASM.

---

## 3. Langkah Eksekusi Selanjutnya (Siap Dijalankan Agent Berikutnya)

Jika pengguna meminta untuk **mulai menulis kode (Fase 1)**, jalankan urutan berikut:

1. **Verifikasi Toolchain Lokal:**
   - Periksa `rustc --version`, `cargo --version`, target `aarch64-linux-android`, serta ketersediaan Android SDK/NDK dan `cargo-makepad`.
2. **Scaffold Cargo Workspace di `d:\code\project\music player\` (atau subfolder `ferric/`):**
   ```text
   Cargo.toml                  # Workspace root dengan [profile.release] lto="fat", codegen-units=1, strip="symbols"
   crates/
     ferric-core/              # SQLite WAL, model domain multi-akun, queue, ISRC & YT metadata matcher, event bus
     ferric-audio/             # Dual-deck symphonia 0.6.1 + rubato 5.1 + biquad EQ + rtrb + cpal/oboe
     ferric-sync/              # Protokol Ferric Connect (iroh 1.4.1 + mdns-sd + postcard + QR pairing)
     ferric-providers/         # Spotify & YouTube/YT Music OAuth PKCE, playlist bridge, lofty, OpenSubsonic, LRCLIB, AudioResolver
     ferric-ui/                # Komponen UI lintas platform Makepad 1.0 (adaptive desktop 3-pane & mobile sheet)
   apps/
     ferric-desktop/           # Entrypoint Windows + souvlaki SMTC + tray-icon + minimize VRAM saver
     ferric-mobile/            # Entrypoint Android + thin Kotlin MediaSessionService + JNI bridge
   ```
3. **Bangun & Uji Vertikal Slice Pertama (Fase 1):**
   - Pemindaian folder musik lokal -> simpan di SQLite -> tampilkan di `PortalList` -> putar dengan *gapless + crossfade + 10-band EQ* -> ukur penggunaan RAM di Task Manager Windows (< 35 MB).
