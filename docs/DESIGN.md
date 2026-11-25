# Design System & Visual Direction (`DESIGN.md`) — Ferric Desktop

- **Versi Dokumen:** 3.0.0 (Diselaraskan dengan UI Produksi Aktual `v0.1.0`)
- **Tanggal Pembaruan:** 25 November 2026
- **Implementasi Sumber:** [theme.rs](file:///d:/code/project/music%20player/ferric/crates/ferric-core/src/theme.rs), [app.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/app.rs), [widgets.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/widgets.rs), [track_list.rs](file:///d:/code/project/music%20player/ferric/apps/ferric-desktop/src/track_list.rs)
- **Ikon Produk Resmi (Terkunci):** **Ferric Oxide Cassette** di [assets/brand/](file:///d:/code/project/music%20player/ferric/assets/brand) (`ferric.svg`, `ferric.png`, `ferric.ico`, `ferric-icon-mono.svg`, wordmark).

---

## 1. Sistem Tema Aktual (4 Preset + Import/Export JSON)

Ferric menggunakan **4 preset tema** yang didefinisikan langsung di `crates/ferric-core/src/theme.rs` dan dapat diekspor/diimpor melalui `.ferric-theme.json`:

| Preset | Canvas | Surface | Border | Teks Utama / Sekunder | Aksen Default | Catatan Perilaku |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`AMOLED Black` (Default)** | `#000000` | `#09090D` | `#1C1D25` | `#F4F4F7` / `#9A9CA8` | `#A882FF` (Soft Violet) | Kontras > 19:1 (WCAG AAA), `0%` GPU saat idle, mendukung **Adaptive Artwork Accent** aktif bawaan. |
| **`Native Acrylic`** | `#0B0C11` *(per-pixel alpha)* | `#11131B` *(frosted glass)* | `#252836` | `#F4F4F7` / `#9A9CA8` | `#A882FF` (+ Adaptive) | Mengaktifkan blur OS Windows 11 nyata (`DWMSBT_TRANSIENTWINDOW`) via DirectComposition premultiplied-alpha. Opacity diatur `18%–94%` (`default 72%`). |
| **`Obsidian Studio`** | `#0D0E12` | `#14151C` | `#232530` | `#F4F4F7` / `#9A9CA8` | `#A882FF` (+ Adaptive) | Mode gelap studio berkontras lembut tanpa transparansi OS. |
| **`Warm Cream Light`** | `#F3EEE3` | `#EAE3D4` | `#D5CCB9` | `#111111` / `#524B42` | `#D9541E` (Burnt Orange) | Mode terang kertas hangat LivelyDesk-family (Adaptive Accent dimatikan bawaan). |

### Penyeragaman Permukaan Kaca di Mode `Native Acrylic`
- **Teks & Konten 100% Opaque:** Karena transparansi bekerja per-piksel pada swapchain DirectComposition (`WS_EX_NOREDIRECTIONBITMAP` + `DXGI_ALPHA_MODE_PREMULTIPLIED`, bukan `SetLayeredWindowAttributes`), menurunkan opacity latar **tidak pernah** membuat teks, ikon, tombol, atau sampul lagu menjadi pudar.
- **Keseragaman Kartu & Input:** Seluruh permukaan interaktif (`TrackRow`, `QuickPickTile`, `ShelfCard`, `GridCard`, `FerricCard`, `QueueMiniRow`, `top_search_box`, dan seluruh `FerricInput`) menggunakan resep kaca tipis yang seragam:
  - Idle: `rgba(255, 255, 255, 0.024 .. 0.036)` dengan border halus `rgba(255, 255, 255, 0.055 .. 0.095)`
  - Hover: `rgba(255, 255, 255, 0.090 .. 0.095)` + sheen atas halus
  - Active / Playing: tint aksen `mix(vec3(0.14, 0.14, 0.22), accent, 0.34)` alpha `0.36` + border aksen.
- **Catatan Arsitektur:** Tema lama *Liquid Glass refraction shader* (`IOR`/`Dispersion`/`Rim`) dan tombol *Compact Mini-Mode* telah **dihapus sepenuhnya** agar antarmuka tetap bersih, ringan, dan bebas konflik visual.

---

## 2. Tipografi & Skala Antarmuka

- **Font Tertanam:** **Inter v4.1** (`Inter-Regular.ttf`, `Inter-Medium.ttf`, `Inter-SemiBold.ttf`, `Inter-Bold.ttf` di `apps/ferric-desktop/resources/fonts/`).
- **Skala Dasar (`ui_zoom = 1.0`):**
  - Display / Judul Halaman: `15.5–16.0 pt` Bold
  - Judul Kartu / Baris Lagu: `9.0–9.4 pt` Bold
  - Subjudul / Metadata: `8.0–8.6 pt` Regular/Medium
  - Lirik Fullscreen: skala dinamis dengan fokus baris aktif, sapuan karaoke putih murni (`#FFFFFF` di atas teks belum tersapu alpha `0.58`), serta baris tetangga ber-alpha `0.28–0.64` dan blur kedalaman halus (`0.0–0.48`).
- **Live UI Zoom:** Mendukung `65%–140%` (`Ctrl -`, `Ctrl +`, `Ctrl 0`, atau kontrol di `Settings > Appearance`) yang menskalakan seluruh tinggi baris, sidebar, kartu, dan ukuran font secara proporsional.

---

## 3. Tata Letak 3-Pane Desktop Shell

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ [Icon Ferric] [|< > ↻]        [ Search music... (Ctrl+K) ]     [Sign in][⚙][◫] [—][❐][×] │  36px Top Chrome
├───────────┬──────────────────────────────────────────────────────┬───────────┤
│ Home      │  Main Stage (9 Halaman)                              │ [Queue][Lyrics] [📻][↻][🗑]
│ Search    │  - Home (Mood Chips + Quick Picks 2x4 + 8 Shelves)   │ HISTORY   │
│ Recents   │  - Search (Filters + Cards + Songs)                  │ (3 items) │
│           │  - Recents / Songs (Virtualized TrackList)           │ NOW PLAY  │
│ LIBRARY   │  - Albums / Artists / Playlists (Grid 5x4 = 20/hal)  │ UP NEXT   │
│ Songs     │  - Detail View (Hero + Actions + TrackList)          │ (10 rows  │
│ Albums    │  - Settings (6 Tabs)                                 │  + ↑ / ×) │
│ Artists   │                                                      │           │
│ Playlists │                                                      │           │
│ PINNED    │                                                      │           │
│ Settings  │                                                      │           │
├───────────┴──────────────────────────────────────────────────────┴───────────┤
│ [Art] Title/Artist [♡][+]    [🔀][⏮]( ▶ )[⏭][🔁]       [CODEC][🎬][🎤][☰][🔊 ━━○][⛶] │  74px Stacked 2-Row
│                              0:11 ━━━━━━━━○────────── 4:31                   │       Bottom Bar
└──────────────────────────────────────────────────────────────────────────────┘
```

### Rincian Geometri & Aturan Chrome
- **Top Bar (`36px`):** Logo + nama `Ferric` di pojok paling kiri; area drag jendela Windows 11 + Aero Snap di ruang kosong; input pencarian inline di tengah; tombol akun, Settings, toggle sidebar kanan, dan tepat satu set tombol jendela kustom (`—`, `❐`, `×`) di kanan.
- **Win32 Window Style:** Selalu menggunakan `WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU` (**tanpa `WS_CAPTION`**) agar Windows 11 DWM tidak pernah menggambar tombol caption native ganda di atas mode Acrylic, sekaligus memotong ukuran maximize presisi ke `rcWork` (di atas Taskbar, nol kliping sisi kanan/bawah).
- **Bottom Player Bar (`74px`):** Grup kiri (`272px` fixed) dan grup kanan (`272px` fixed) simetris agar blok tengah (`[Shuffle][Prev](Play)[Next][Repeat]` di baris atas, `Elapsed ━━━○─── Duration` di baris bawah) selalu presisi di tengah layar. Volume slider mendukung drag maupun **scroll roda mouse**.
- **Fullscreen Lyrics Overlay (`F` / `F11`):**
  - Latar `FerricAmbientStage` menggunakan **21-tap ultra-wide blur** (`r1=0.14`, `r2=0.28`, `r3=0.42`) + scrim gelap halus (`0.28 -> 0.145` di kolom lirik) sehingga tekstur/teks sampul album melebur sempurna menjadi awan warna tenang tanpa mengganggu keterbacaan lirik.
  - Kolom kiri menampilkan Cover Art besar (atau Video Klip HD WebView2 saat mode `Video` dipilih) + transport; kolom kanan menampilkan `FerricLyricsView` interaktif (hover pill kaca, stempel waktu saat hover, klik untuk seek).

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
| `F` / `F11` | Buka / tutup Fullscreen Lyrics Mode |
| `Q` / `Ctrl + Q` | Tampilkan / sembunyikan Right Sidebar (`Queue`) |
| `Ctrl + L` | Buka / toggle tab `Lyrics` di Right Sidebar |
| `Ctrl + K` / `Ctrl + F` | Fokus pencarian inline / buka halaman Search |
| `Ctrl + ,` / `Ctrl + E` | Buka Settings / langsung ke tab Equalizer |
| `Alt + Left` / `Alt + Right` | Navigasi Riwayat Halaman Mundur / Maju |
| `Ctrl + -` / `Ctrl + +` / `Ctrl + 0` | Perkecil / perbesar / reset UI Zoom (`65%–140%`) |
| `Esc` | Tutup menu konteks / video klip / Fullscreen Lyrics / popup update |
