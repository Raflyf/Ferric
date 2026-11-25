# Riset Teknis Ekosistem Rust & Benchmark Arsitektur (`RESEARCH_AND_RE_FINDINGS.md`)

- **Versi Dokumen:** 2.0.0 (Rekonsiliasi Riset Awal vs Implementasi Nyata Ferric `v0.1.0`)
- **Tanggal Pembaruan:** 25 November 2026

Dokumen ini merangkum dasar pemilihan stack teknis Rust pada proyek **Ferric** beserta perbandingan hasil implementasi aktual `v0.1.0` terhadap tolok ukur pemutar musik desktop dan mobile modern.

---

## 1. Alasan Pemilihan Stack Native Rust di Ferric

| Komponen | Pilihan Terimplementasi di Ferric | Alasan Teknis & Hasil Nyata |
| :--- | :--- | :--- |
| **UI Framework** | **`makepad-widgets 1.0.0`** + patch lokal `vendor/makepad-platform` | Lisensi MIT/Apache-2.0, rendering DX11 native di Windows tanpa WebView/Chromium untuk UI utama, `PortalList` tervirtualisasi, shader SDF kustom, dan DirectComposition premultiplied-alpha untuk Native Windows 11 Acrylic. |
| **Audio Decoder** | **`symphonia 0.6.1`** + **`symphonia-opus 0.1.3`** | 100% pure-Rust (termasuk Opus tanpa binding C `libopus`), mendukung FLAC, MP3, AAC/M4A, ALAC, Ogg Vorbis, Opus, WAV, AIFF dengan nol dependensi native tambahan. |
| **Resampler, EQ & Buffer** | **`rubato 5.1.0`** + **`biquad 0.6.0`** + **`rtrb 0.4.1`** + **`cpal 0.18.2`** | Dual-deck A+B dengan wait-free SPSC ring buffer, crossover Bass-Swap 140 Hz, 10-band Parametric EQ real-time, dan output WASAPI bebas alokasi di thread audio. |
| **HTTP & Serialisasi** | **`ureq 3.5`** + **`serde_json`** + **`postcard 1.2`** | Menggunakan klien HTTP sinkron pada thread pekerja ringan (menghindari overhead memori runtime async berat) dan framing biner ringkas untuk Ferric Connect. |
| **Database & Metadata** | **`rusqlite 0.43` (`bundled`)** + **`lofty 0.25.5`** + **`aho-corasick 1.1.5`** | SQLite mode WAL tertanam tanpa DLL eksternal, pembacaan tag lokal cepat, dan pembersihan judul YouTube/YT Music deterministik. |
| **Integrasi Windows** | **`windows 0.62`** + **`souvlaki 0.9.3`** + **`rfd 0.18.0`** + **`webview2-com 0.39.1`** | Subclass Win32 untuk frameless maximize/tray/single-instance/DWM Acrylic, SMTC media keys, dialog file native, serta WebView2 terisolasi khusus login Google opsional dan video klip HD dalam mode fullscreen. |

---

## 2. Pelajaran Arsitektur dari Evaluasi Referensi

1. **Mengapa bukan Electron / Chromium UI:** Pemutar berbasis Chromium umumnya memakan `300–700 MB` RAM saat terbuka maupun di tray. Dengan Makepad DX11 native + penghentian loop redraw saat tersembunyi + trim working set Win32, Ferric mencapai **`44,1 MB`** saat streaming aktif dan **`7,3 MB`** saat diminimalkan.
2. **Mengapa bukan GPUI saja:** `GPUI` hanya menarget desktop; `Makepad 1.0` memberi jalur berbagi crate inti dan pola UI menuju Android di fase pasca-`v0.1.0`.
3. **Mengapa `symphonia-opus` pure-Rust + Dual-Deck:** Menghilangkan kesulitan build C `libopus` lintas platform sekaligus memungkinkan pre-buffering `Deck B`, *Gapless Album Lock*, pemotongan hening awal/akhir, dan *Bass-Swap Automix 140 Hz*.
4. **Mengapa DirectComposition Vendored Patch Diperlukan:** Pada Windows 11, DXGI Flip-Model mengabaikan `LWA_COLORKEY`, sementara `SetLayeredWindowAttributes` memudarkan teks dan artwork bersamaan dengan latar. Patch minimal pada `vendor/makepad-platform` (`WS_EX_NOREDIRECTIONBITMAP` + `CreateSwapChainForComposition` + `DXGI_ALPHA_MODE_PREMULTIPLIED`) membuka transparansi per-piksel sejati dengan teks 100% tajam, dan penghapusan `WS_CAPTION` mencegah Windows 11 DWM menggambar tombol caption ganda.
5. **Mengapa Lirik & Metadata Tanpa Kamus Besar:** Mengandalkan waterfall 4 tahap (`.lrc` lokal -> embedded tag -> `LRCLIB` -> `Lyrics.ovh`) menjaga ukuran paket tetap ringkas tanpa kamus puluhan megabyte di memori.

---

## 3. Benchmark Aktual Ferric Desktop `v0.1.0` vs Tolok Ukur Umum

| Aspek | Pemutar Web/Electron Umum | Pemutar Native Desktop Umum | **Ferric Desktop `v0.1.0` (Terukur Aktual)** |
| :--- | :--- | :--- | :--- |
| **UI Runtime** | Chromium + JS | Native C++/Rust | **100% Native Rust (`Makepad 1.0` DX11 + DirectComposition)** |
| **RAM Idle (Terbuka)** | `250–450 MB` | `45–95 MB` | **`7,6 MB`** Working Set |
| **RAM Streaming Aktif** | `350–700 MB` | `55–110 MB` | **`44,1 MB`** Working Set |
| **RAM Minimized / Tray** | `200–400 MB` | `30–65 MB` | **`7,3 MB`** Working Set |
| **GPU Idle / Tersembunyi** | `1–5%` | `0–1%` | **`0%`** (redraw terhenti saat tersembunyi) |
| **Transparansi Windows 11** | CSS palsu / buram seluruh jendela | Jarang per-piksel | **Native Windows 11 DWM Acrylic per-piksel (teks 100% opaque)** |
| **Audio Transition & EQ** | Crossfade dasar | Gapless + EQ | **Dual-Deck Bass-Swap 140 Hz + S-Curve + 10-Band Biquad EQ + 24 AutoEq + APO** |
| **Smart Radio Queue** | Sering ulang artis/cover | Bervariasi | **Second-Hop Multi-Artis (maks 1/artis) + filter cover/karaoke/pitch-shift** |
| **Installer & Update** | Installer besar tanpa hash check | Manual / Squirrel | **Per-user WiX MSI (`%LOCALAPPDATA%\Programs\Ferric`) + cek SHA-256 otomatis** |
