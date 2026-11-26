# Riset Teknis Ekosistem Rust & Benchmark Arsitektur (`RESEARCH_AND_RE_FINDINGS.md`)

- **Versi Dokumen:** 2.2.0 (Rekonsiliasi Riset Awal vs Implementasi Nyata Ferric `v0.1.0`)
- **Tanggal Pembaruan:** 26 November 2026

Dokumen ini merangkum dasar pemilihan stack teknis Rust pada proyek **Ferric**, pelajaran arsitektur Win32/DX11/keamanan yang ditemukan selama pengembangan, serta perbandingan hasil implementasi aktual `v0.1.0` terhadap tolok ukur pemutar musik desktop modern.

---

## 1. Alasan Pemilihan Stack Native Rust di Ferric

| Komponen | Pilihan Terimplementasi di Ferric | Alasan Teknis & Hasil Nyata |
| :--- | :--- | :--- |
| **UI Framework** | **`makepad-widgets 1.0.0`** + patch lokal `vendor/makepad-platform` | Lisensi MIT/Apache-2.0, rendering DX11 native di Windows tanpa Chromium untuk UI utama, `PortalList` tervirtualisasi, shader SDF kustom, DirectComposition premultiplied-alpha Acrylic sejak *cold start*, dan virtual OS fallback font loader (`cx_native.rs`). |
| **Audio Decoder** | **`symphonia 0.6.1`** + **`symphonia-opus 0.1.3`** | 100% pure-Rust (termasuk Opus tanpa binding C `libopus`), mendukung FLAC, MP3, AAC/M4A, ALAC, Ogg Vorbis, Opus, WAV, AIFF dengan nol dependensi native tambahan. |
| **Resampler, EQ & Buffer** | **`rubato 5.1.0`** + **`biquad 0.6.0`** + **`rtrb 0.4.1`** + **`cpal 0.18.2`** | Dual-deck A+B dengan wait-free SPSC ring buffer, crossover Bass-Swap 140 Hz, auto-only crossfade, 10-band Parametric EQ real-time, dan output WASAPI bebas alokasi di thread audio. |
| **HTTP & Serialisasi** | **`ureq 3.5`** + **`serde_json`** + **`postcard 1.2`** | Menggunakan klien HTTP sinkron pada thread pekerja ringan (menghindari overhead memori runtime async berat) dan framing biner ringkas untuk Ferric Connect. |
| **Database & Metadata** | **`rusqlite 0.43` (`bundled`)** + **`lofty 0.25.5`** + **`aho-corasick 1.1.5`** | SQLite mode WAL tertanam tanpa DLL eksternal, pembacaan tag lokal cepat, dan pembersihan judul YouTube/YT Music deterministik. |
| **Keamanan & Integrasi OS** | **`windows 0.62`** + **`souvlaki 0.9.3`** + **`rfd 0.18.0`** + **`webview2-com 0.39.1`** + **DPAPI/BCrypt native** | Mitigasi proses & anti-DLL hijacking, enkripsi DPAPI lokal, DWM Acrylic cold-start, Taskbar `ITaskbarList3`, SMTC, Discord RPC via named pipe tanpa dependensi, WebView2 tersandbox ketat, dan `verify_pe_hardening.py`. |

---

## 2. Pelajaran Arsitektur & Temuan Teknis Utama

1. **Mengapa bukan Electron / Chromium UI:** Pemutar berbasis Chromium umumnya memakan `300–700 MB` RAM saat terbuka maupun di tray. Dengan Makepad DX11 native + penghentian loop redraw saat tersembunyi + trim working set Win32, Ferric mencapai **`41,3–44,1 MB`** saat streaming aktif, **`7,6 MB`** saat idle terbuka, dan **`7,3 MB`** saat diminimalkan.
2. **Mengapa bukan GPUI saja:** `GPUI` berfokus pada desktop; `Makepad 1.0` memberi jalur berbagi crate inti (`ferric-core`, `ferric-audio`, `ferric-providers`, `ferric-sync`) dan pola UI menuju Android di fase pasca-`v0.1.0`.
3. **Mengapa `symphonia-opus` pure-Rust + Dual-Deck:** Menghilangkan kompleksitas build C `libopus` lintas platform sekaligus memungkinkan pre-buffering `Deck B`, *Gapless Album Lock*, pemotongan hening awal/akhir, dan *Bass-Swap Automix 140 Hz*.
4. **Mengapa DirectComposition Vendored Patch & Urutan Cold-Start Diperlukan:**
   - Pada Windows 11, DXGI Flip-Model mengabaikan `LWA_COLORKEY`, sementara `SetLayeredWindowAttributes` memudarkan teks dan artwork bersamaan dengan latar. Patch pada `vendor/makepad-platform` (`WS_EX_NOREDIRECTIONBITMAP` + `CreateSwapChainForComposition` + `DXGI_ALPHA_MODE_PREMULTIPLIED`) membuka transparansi per-piksel sejati dengan teks 100% tajam.
   - Penghapusan `WS_CAPTION` mencegah Windows 11 DWM menggambar tombol caption ganda di atas chrome kustom.
   - **Temuan Cold-Start DWM Acrylic:** Menerapkan `DWMWA_SYSTEMBACKDROP_TYPE` setelah `CreateTargetForHwnd` dan `ShowWindow` membuat kompositor Windows 11 melewatkan backdrop Acrylic (tampil hitam solid hingga jendela di-minimize/restore). Menyemai `INITIAL_ACRYLIC_BACKDROP` saat `Event::Startup`, memanggil `DwmExtendFrameIntoClientArea(-1)` + `DWMWA_SYSTEMBACKDROP_TYPE` di `Win32Window::init` sebelum target DirectComposition dibuat, memanggil `dcomp_dev.Commit()` sebelum `show()`, serta meneruskan `WM_NCACTIVATE` (`LPARAM(-1)`) dan `WM_ACTIVATE` ke `DefWindowProcW` menyelesaikan cacat ini 100%.
5. **Mengapa Zero-Bloat OS Fallback Fonts Lebih Unggul daripada Bundling CJK:** Membundel paket font CJK/Emoji Makepad bawaan membengkakkan biner hingga `~47 MB`. Dengan slot font virtual (`__sys_cjk_jp.ttf`, `__sys_cjk_kr.ttf`, `__sys_cjk_cn.ttf`, `__sys_thai.ttf`) di `cx_native.rs` yang memuat font sistem OS (`YuGothM.ttc`, `malgunsl.ttf`, `msyh.ttc`, `LeelawUI.ttf`) satu kali ke `Rc<Font>`, seluruh aksara internasional tampil tajam dengan **0 byte penambahan ukuran biner/MSI**.
6. **Mengapa Kualitas Lirik Membutuhkan Filter Polusi Konten:** Di database komunitas terbuka seperti `LRCLIB`, entri `/api/get` untuk lagu Jepang terkadang berupa fan-sub 3 kolom campuran Jepang + pelafalan/terjemahan Korea atau mojibake `?`. Penalti konten deterministik (`lyrics_content_penalty`) dengan *fallback* ke `/api/search` dan *auto-heal* cache SQLite menjaga lirik asli tetap bersih tanpa merusak lagu dwibahasa yang sah.
7. **Mengapa Reproducible Hardened PE & Defense-in-Depth Diterapkan Sebelum `v0.1.0`:** Kompilasi Rust standar meninggalkan path mesin build, informasi PDB, dan tanpa CET/CFG. Menggabungkan `lto = "fat"`, `strip = "symbols"`, `panic = "abort"`, `/GUARD:CF`, `/CETCOMPAT`, `/BREPRO`, `CARGO_HOME=C:\c`, `subst S:`, `--remap-path-prefix`, dan gerbang otomatis `scripts/verify_pe_hardening.py` menghasilkan biner `~14–15 MB` yang deterministik, bebas kebocoran path, serta terlindungi dari injeksi DLL, pembajakan WebView2, dan TOCTOU installer.
8. **Mengapa Direct3D 11 Hemat-RAM (Integrated GPU Pertama, Tanpa Opsi Dedicated GPU & Bukan D3D12) Lebih Optimal untuk UI 2D Ferric:**
   - Makepad 1.0 menggunakan backend D3D11 + DirectComposition native di Windows. Untuk aplikasi UI 2D/SDF (~20–50 draw call per frame, 0–2 FPS saat idle/playback), D3D12 justru menambah beban baseline RAM driver `~40–80 MB` (command allocators, descriptor heaps, PSO caches) tanpa manfaat throughput nyata dibanding game 3D multi-threaded (10.000+ draw call).
   - Benchmark empiris membuktikan mengaktifkan dGPU diskret (`nvwgf2umx.dll` + Optimus staging) menaikkan commit RAM ke `~360 MB` dan RAM tray ke `~55 MB` sekaligus membangunkan dGPU. Oleh karena itu Ferric meniadakan pemaksaan/opsi dGPU sepenuhnya: selalu memprioritaskan Integrated GPU hemat-RAM terlebih dahulu (tanpa mengenumerasi dGPU bila iGPU sudah di slot 0), menggunakan tekstur `D3D11_USAGE_IMMUTABLE`, melepas `ID3DBlob` pasca-kompilasi, dan berbagi satu `ureq::Agent` per transport agar RAM, CPU, dan GPU tetap seminimal mungkin tanpa mengorbankan kualitas UI/UX 60 FPS.

---

## 3. Benchmark Aktual Ferric Desktop `v0.1.0` vs Tolok Ukur Umum

| Aspek | Pemutar Web/Electron Umum | Pemutar Native Desktop Umum | **Ferric Desktop `v0.1.0` (Terukur Aktual `5c81b11`)** |
| :--- | :--- | :--- | :--- |
| **UI Runtime** | Chromium + JS | Native C++/Rust | **100% Native Rust (`Makepad 1.0` DX11 + DirectComposition)** |
| **Ukuran Binary Utama** | `120–250 MB` | `20–60 MB` | **`14,0 MB`** (`ferric.exe`, fat LTO + stripped + hardened PE) |
| **RAM Idle (Terbuka)** | `250–450 MB` | `45–95 MB` | **`7,6 MB`** Working Set |
| **RAM Streaming Aktif** | `350–700 MB` | `55–110 MB` | **`41,3–44,1 MB`** Working Set |
| **RAM Minimized / Tray** | `200–400 MB` | `30–65 MB` | **`7,3 MB`** Working Set |
| **CPU Settled / GPU Hidden** | `1–5%` GPU | `0–1%` GPU | **`0,52%` CPU settled / `0%` GPU saat tersembunyi** |
| **Transparansi Windows 11** | CSS palsu / buram seluruh jendela | Jarang per-piksel | **Cold-Start Native Windows 11 DWM Acrylic per-piksel (teks 100% opaque)** |
| **Tipografi & Lirik Internasional** | Font browser berat | Sering kotak glyph CJK | **Inter v4.1 + Zero-Bloat OS CJK/Thai Fallbacks + Filter Polusi LRCLIB** |
| **Audio Transition & EQ** | Crossfade dasar | Gapless + EQ | **Dual-Deck Auto-Only Bass-Swap 140 Hz + S-Curve + 10-Band Biquad EQ + 24 AutoEq + APO** |
| **Smart Radio Queue** | Sering ulang artis/cover | Bervariasi | **Second-Hop Multi-Artis (maks 1/artis) + filter cover/karaoke/pitch-shift** |
| **Integrasi Desktop** | Terbatas | SMTC saja | **SMTC + Taskbar Thumbnail Controls + Native Discord RPC + System Tray** |
| **Keamanan & Installer** | Installer besar tanpa hash check | Manual / Squirrel | **DPAPI + Win32 Mitigations + WebView2 Sandbox + `GUARD_CF`/`CETCOMPAT`/`BREPRO` + WiX MSI & TOCTOU-Safe SHA-256** |
