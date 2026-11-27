# Design System & Visual Direction (`DESIGN.md`) - Ferric Desktop

- **Versi Dokumen:** 3.2.0 (Diselaraskan dengan UI Produksi Aktual `v0.1.0`)
- **Tanggal Pembaruan:** 26 November 2026
- **Implementasi Sumber:** `crates/ferric-core/src/theme.rs`, `apps/ferric-desktop/src/theme.rs`, `apps/ferric-desktop/src/app.rs`, `apps/ferric-desktop/src/widgets.rs`, `apps/ferric-desktop/src/track_list.rs`, `apps/ferric-desktop/src/shell.rs`
- **Ikon Produk Resmi (Terkunci):** **Ferric Oxide Cassette** di `assets/brand/` (`ferric.svg`, `ferric.png`, `ferric.ico`, `ferric-icon-mono.svg`, wordmark).

---

## 1. Sistem Tema Aktual (4 Preset + GPU HSV Picker + WCAG Adaptive Contrast)

Ferric menggunakan **4 preset tema** yang didefinisikan langsung di `crates/ferric-core/src/theme.rs` dan dapat diekspor/diimpor melalui `.ferric-theme.json`:

| Preset | Canvas | Surface | Border | Teks Utama / Sekunder | Aksen Default | Catatan Perilaku |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`AMOLED Black` (Default Instalasi Awal)** | `#000000` | `#09090D` | `#1C1D25` | `#F4F4F7` / `#9A9CA8` | `#E86A2C` (Ferric Burnt Orange) | Kontras > 19:1 (WCAG AAA), `acrylic_enabled: false`, `0%` GPU saat idle, mendukung **Adaptive Artwork Accent** aktif bawaan. |
| **`Native Acrylic`** | `#0B0C11` *(per-pixel alpha)* | `#11131B` *(frosted glass)* | `#252836` | `#F4F4F7` / `#9A9CA8` | `#E86A2C` (+ Adaptive) | Mengaktifkan blur OS Windows 11 nyata (`DWMSBT_TRANSIENTWINDOW`) via DirectComposition premultiplied-alpha sejak *cold start*. Opacity `18%–94%` (`default 72%`), tint kustom `acrylic_tint_hex` (`default #0B0D14`), serta toggle **Adaptive Acrylic Tint** yang menggeser warna kaca transparan secara halus mengikuti sampul lagu aktif. |
| **`Obsidian Studio`** | `#0D0E12` | `#14151C` | `#232530` | `#F4F4F7` / `#9A9CA8` | `#E86A2C` (+ Adaptive) | Mode gelap studio berkontras lembut tanpa transparansi OS. |
| **`Warm Cream Light`** | `#F3EEE3` | `#EAE3D4` | `#D5CCB9` | `#111111` / `#524B42` | `#D9541E` (Burnt Orange) | Mode terang kertas hangat LivelyDesk-family (Adaptive Accent dimatikan bawaan). |

### Kontras Adaptif WCAG 2.1, Warna Kustom & Penyeragaman Kaca Acrylic
- **GPU HSV Color Picker (`FerricColorPicker`):** Di `Settings > Appearance`, pengguna dapat memilih warna kustom untuk Canvas, Accent, dan Native Acrylic Glass Tint (`acrylic_tint_hex`) secara interaktif.
- **WCAG 2.1 Adaptive Ink (`ThemeConfig::adaptive_palette`):** Setiap perubahan warna latar atau tint Acrylic secara otomatis mengevaluasi rasio kontras W3C WCAG 2.1 (`is_light_surface()`) sehingga teks utama, teks sekunder, ikon, garis batas, dan aksen otomatis beralih antara tinta gelap dan terang agar keterbacaan minimal 4.5:1 tetap terjaga.
- **Ekstraksi Aksen Sampul Presisi HSV (`artwork.rs`):** Saat mengangkat warna dari sampul album gelap, konversi mempertahankan rona (*hue*) dan saturasi HSV agar warna aksen tetap hidup tanpa berubah pucat atau abu-abu.
- **Teks & Konten 100% Opaque di `Native Acrylic`:** Transparansi bekerja per-piksel pada swapchain DirectComposition (`WS_EX_NOREDIRECTIONBITMAP` + `DXGI_ALPHA_MODE_PREMULTIPLIED`), sehingga menurunkan opacity latar **tidak pernah** memudarkan teks, ikon, tombol, atau sampul lagu.
- **Cold-Start Native Acrylic & Pemulihan Fokus/Restore:** DWM backdrop (`DWMWA_USE_IMMERSIVE_DARK_MODE`, `DWMWCP_ROUND`, `DwmExtendFrameIntoClientArea(-1)`, `DWMWA_SYSTEMBACKDROP_TYPE`) diterapkan di `Win32Window::init` sebelum `CreateTargetForHwnd` dan `ShowWindow`. `WM_NCACTIVATE` meneruskan `wparam` asli dengan `LPARAM(-1)` serta menegaskan ulang backdrop saat aktif (`WM_NCACTIVATE`/`WM_ACTIVATE`), restore (`WM_SIZE`), dan perubahan tema/komposisi (`WM_THEMECHANGED`/`WM_DWMCOMPOSITIONCHANGED`) sehingga blur tidak pernah macet pada warna solid.
- **Adaptive Acrylic Tint (`adaptive_acrylic_tint`):** Saat diaktifkan di `Settings > Appearance`, kaca Acrylic seluruh panel (`body`, `top_bar`, `left_sidebar`, `center_stage`, `right_sidebar`, `bottom_bar`, `toast_pill`) berinterpolasi halus (~1,8 dtk) mengikuti warna sampul lagu yang sedang diputar via `derive_acrylic_tint_from_accent` sambil menjaga kontras teks WCAG AAA (`>= 7:1`). Memilih warna tint manual otomatis menonaktifkan mode adaptif agar pilihan pengguna langsung berlaku.
- **Kualitas Sampul Tajam (`artwork.rs` + `d3d11.rs`):** Sampul kartu di-downscale menggunakan *area-box filter* ke `160px` (`256px` untuk hero/fullscreen) sebagai tekstur `D3D11_USAGE_IMMUTABLE` dengan hardware D3D11 bilinear sampling (`D3D11_FILTER_MIN_MAG_MIP_LINEAR`), bebas artefak kotak-kotak di kartu besar maupun ikon sidebar `40px` tanpa overhead RAM driver.
- **Menu Klik Kanan System Tray Mengikuti Tema Perangkat:** `shell.rs` menyinkronkan tema menu popup Win32 dengan tema sistem Windows (`SystemUsesLightTheme`) via `uxtheme.dll` ordinal `#135`/`#136` sehingga klik kanan ikon tray tampil dalam Dark Mode bawaan Windows 11 (atau Light Mode bila perangkat memakai tema terang).
- **Keseragaman Kartu & Input Kaca:** Seluruh permukaan interaktif (`TrackRow`, `QuickPickTile`, `ShelfCard`, `GridCard`, `FerricCard`, `QueueMiniRow`, `top_search_box`, dan seluruh `FerricInput`) memakai resep kaca tipis yang seragam:
  - Idle: `rgba(255, 255, 255, 0.024 .. 0.036)` dengan border halus `rgba(255, 255, 255, 0.055 .. 0.095)`
  - Hover: `rgba(255, 255, 255, 0.090 .. 0.095)` + sheen atas halus
  - Active / Playing: tint aksen `mix(vec3(0.14, 0.14, 0.22), accent, 0.34)` alpha `0.36` + border aksen.
- **Catatan Arsitektur:** Tema lama *Liquid Glass refraction shader* (`IOR`/`Dispersion`/`Rim`) dan tombol *Compact Mini-Mode* telah **dihapus sepenuhnya** demi kebersihan visual dan efisiensi GPU.

---

## 2. Tipografi, Font Fallback OS Internasional & Skala Antarmuka

- **Font Latin Tertanam:** **Inter v4.1** (`Inter-Regular.ttf`, `Inter-Medium.ttf`, `Inter-SemiBold.ttf`, `Inter-Bold.ttf` di `apps/ferric-desktop/resources/fonts/`) ditambah `LiberationMono-Regular.ttf` untuk kode/teknis dan ikon vektor/SDF.
- **Zero-Bloat OS International Font Fallback (`apps/ferric-desktop/src/theme.rs` + `vendor/makepad-platform/src/os/cx_native.rs`):**
  - Seluruh 7 keluarga font UI (`THEME_FONT_LABEL`, `REGULAR`, `MEDIUM`, `BOLD`, `DISPLAY`, `ITALIC`, `BOLD_ITALIC`) mendefinisikan rantai fallback virtual:
    - `__sys_cjk_jp.ttf` -> `YuGothM.ttc` (fallback `YuGothR.ttc` / `meiryo.ttc` / `msgothic.ttc`)
    - `__sys_cjk_kr.ttf` -> `malgunsl.ttf` (fallback `malgun.ttf` / `gulim.ttc`)
    - `__sys_cjk_cn.ttf` -> `msyh.ttc` (fallback `msyhl.ttc` / `simsun.ttc`)
    - `__sys_thai.ttf` -> `LeelawUI.ttf` (fallback `leelawad.ttf` / `tahoma.ttf`)
  - Dimuat langsung dari `%SystemRoot%\Fonts` pada runtime dengan validasi header SFNT/TTC (`<= 32 MiB`), dibagikan satu kali melalui `Rc<Font>`, **menambah 0 byte ke ukuran `ferric.exe`/MSI**, dan jatuh aman ke `Inter-Regular.ttf` bila font sistem tidak tersedia.
- **Skala Dasar (`ui_zoom = 1.0`):**
  - Display / Judul Halaman: `15.5–16.0 pt` Bold
  - Judul Kartu / Baris Lagu: `9.0–9.4 pt` Bold (mendukung judul terbungkus rapi)
  - Subjudul / Metadata: `8.0–8.6 pt` Regular/Medium
  - Lirik Fullscreen: **iluminasi penuh seragam di seluruh baris aktif** (termasuk lirik panjang yang membungkus beberapa baris, menggantikan sapuan setengah baris lama), dengan baris aktif putih penuh (`#FFFFFF`) serta baris tetangga ber-alpha `0.28–0.64` dan blur kedalaman halus (`0.0–0.48`).
- **Live UI Zoom:** Mendukung `65%–140%` (`Ctrl -`, `Ctrl +`, `Ctrl 0`, atau `Settings > Appearance`) dengan bottom bar zoom-safe (`Fit`) sehingga kontrol tidak pernah terpotong pada skala besar.

---

## 3. Tata Letak 3-Pane Desktop Shell & Koreografi Motion 60 FPS

```text
+--------------------------------------------------------------------------------------+
| [Icon Ferric] [< > R]      [ Search music... (Ctrl+K) x ]   [Sign in][Cfg][Side] [-][Max][x] |  36px Top Chrome
+-----------+------------------------------------------------------+-------------------+
| Home      |  Main Stage (9 Halaman)                              | [Queue][Lyrics] [Rad][Ref][Clr]
| Search    |  - Home (Mood Chips + Quick Picks 2x4 + 8 Shelves)   | HISTORY           |
| Recents   |  - Search (Live Debounce + Chips + Filters + Cards)  | (3 items)         |
|           |  - Recents / Songs (Virtualized TrackList)           | NOW PLAYING       |
| LIBRARY   |  - Albums / Artists / Playlists (Grid 5x4 = 20/hal)  | UP NEXT           |
| Songs     |  - Detail View (Hero + Actions + TrackList)          | (10 rows          |
| Albums    |  - Settings (6 Tabs)                                 |  + Up / Remove)   |
| Artists   |                                                      |                   |
| Playlists |                                                      |                   |
| PINNED    |                                                      |                   |
| Settings  |                                                      |                   |
+-----------+------------------------------------------------------+-------------------+
| [Art] Title/Artist [Like][+]  [Shuf][Prev](Play)[Next][Rep]   [CODEC][Vid][Lyr][Q][Vol ==o][FS] |  74px Stacked 2-Row
|                               0:11 ========o---------- 4:31                          |       Bottom Bar
+--------------------------------------------------------------------------------------+
```

### Rincian Geometri, Chrome & Kontrol SDF
- **Top Bar (`36px`):** Logo + nama `Ferric` di pojok paling kiri; area drag jendela Windows 11 + Aero Snap; pencarian inline *live debounced* dengan tombol clear (`x`); tombol akun, Settings, toggle sidebar kanan tersinkron, dan satu set tombol jendela kustom (`Minimize`, `Maximize`, `Close`).
- **Win32 Window Style & Taskbar:** Memakai `WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU` (**tanpa `WS_CAPTION`**) dengan kliping maximize presisi ke `rcWork`, judul jendela dinamis *now-playing*, nama Task Manager bersih `Ferric`, dan tombol media pratinjau Taskbar Windows (`Prev` / `Play-Pause` / `Next`).
- **Bottom Player Bar (`74px`, Zoom-Safe `Fit`):** Blok tengah (`[Shuffle][Prev](Play)[Next][Repeat]` di baris atas, `Elapsed ━━━○─── Duration` di baris bawah) terjaga presisi di tengah layar. Tombol Repeat (`FerricRepeatBtn`: `Off` / `All` / `One`) dan Volume (`FerricVolumeBtn`: mute / level) dirender sebagai **ikon GPU SDF beranimasi**.
- **Modal & Toast:** Popup modal (context menu, playlist picker, update dialog) memblokir pointer ke lapisan belakang dan menutup otomatis saat klik di luar area; notifikasi toast mengikuti palet tema aktif.

### Koreografi Animasi 60 FPS di Fullscreen Lyrics Mode (`F` / `F11`)
1. **Fullscreen Open & Close 2-Fase (`fs_reveal_t`):**
   - **Buka:** Fase 1 (`t = 0.0 -> 0.58`) melarutkan tabir aurora 21-tap (`FerricAmbientStage.opacity`) hingga 100% opak di atas shell desktop yang masih hidup; Fase 2 (`t = 0.52 -> 1.0`) meluncurkan konten fullscreen naik `42px` dan menaikkan opasitas lirik (`FerricLyricsView.reveal_alpha`) tanpa efek *UI ghosting*.
   - **Tutup (~240ms):** Fase 2 membalik (konten meluncur turun dan pudar terlebih dulu), diikuti Fase 1 melarutkan tabir aurora hingga terbuka bersih ke shell, lalu menyembunyikan overlay dan memangkas working set.
2. **Cover <-> Video Clip Morph (`fs_video_morph_t`):**
   - Perpindahan mode `Cover` dan `Video` pada kolom kiri Fullscreen menjalankan morph 60 FPS (`DrawFerricCoverArt.opacity` + penyesuaian layout), menggunakan kontainer bawah tinggi tetap `54px` (`fs_video_footer`) agar kontrol transport tidak melompat, serta *pre-buffered* WebView2 CSS `#stage` fade-in `220ms` dengan **satu sudut membulat `16px` DPI-presisi (`CreateRoundRectRgn`)** tanpa bingkai ganda.
3. **Kolom Kiri Simetris & Lirik Interaktif:**
   - Kolom kiri Fullscreen menempatkan Play/Pause presisi di tengah di antara Shuffle/Prev/Next/Repeat, diikuti baris volume tersendiri dan visualizer spektrum.
   - Latar `FerricAmbientStage` menggunakan **21-tap liquid aurora blur** + scrim velvet gelap berkontras tinggi; kolom kanan menampilkan `FerricLyricsView` dengan *smooth wave spring*, hover pill kaca, stempel waktu saat hover, dan klik baris untuk seek.

---

## 4. Pintasan Keyboard Aktual (`apps/ferric-desktop/src/app.rs`)

| Pintasan | Aksi |
| :--- | :--- |
| `Space` | Play / Pause (saat tidak mengetik di input teks) |
| `Left` / `Right` | Seek mundur / maju 5 detik |
| `Shift + Left` / `Shift + Right` | Lagu sebelumnya / berikutnya |
| `Ctrl + Up` / `Ctrl + Down` | Naikkan / turunkan volume 5% (juga mendukung scroll di area volume) |
| `M` | Mute / Unmute |
| `S` / `R` / `L` | Toggle Shuffle / siklus Repeat (`Off -> All -> One`) / Like lagu aktif |
| `F` / `F11` | Buka / tutup Fullscreen Lyrics Mode (dengan animasi 2-fase 60 FPS) |
| `Q` / `Ctrl + Q` | Tampilkan / sembunyikan Right Sidebar (`Queue`; `Q` saat Fullscreen tertutup) |
| `Ctrl + L` | Buka / toggle tab `Lyrics` di Right Sidebar |
| `Ctrl + K` / `Ctrl + F` | Fokus pencarian inline / buka halaman Search |
| `Ctrl + ,` / `Ctrl + E` | Buka Settings / langsung ke tab Equalizer |
| `Alt + Left` / `Alt + Right` | Navigasi Riwayat Halaman Mundur / Maju |
| `Ctrl + -` / `Ctrl + +` / `Ctrl + 0` | Perkecil / perbesar / reset UI Zoom (`65%–140%`) |
| `Esc` | Tutup menu konteks / video klip / Fullscreen Lyrics / popup update |
