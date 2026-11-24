# Design System & Visual Direction (`DESIGN.md`) — Ferric Music Player

- **Versi:** 2.0.0 (Strict 3-Color Identity + Unlimited Open Theme Studio)
- **Tanggal:** 24 November 2026
- **Mode Permukaan (Impeccable Mode):** `Operate` (Presisi instrumen audio, keterbacaan instan, nol dekorasi pelangi).
- **Design Read (`antislop`):** Aplikasi pemutar musik lintas platform (Windows Desktop & Android Mobile) dengan identitas default ketat **3 Warna LivelyDesk-Family di atas Pure OLED Pitch Black** (`#000000` / `#111111`, `#F3EEE3` Cream, `#D9541E` Burnt Orange), ditambah **Open Theme Studio** tanpa batas di menu Appearance agar pengguna bebas berkreasi sepenuhnya. Dial: **`ENERGY 2 / RHYTHM 2 / MOTION 2`**.
- **File Desain Live (FigCraft):** `MusicPlayer UI UX` (backup lokal: [ferric-ui-ux.figcraft](file:///d:/code/project/music%20player/ferric-ui-ux.figcraft)).
- **Aset Logo & App Icon Produksi (Terkunci):** Folder [assets/brand/](file:///d:/code/project/music%20player/assets/brand) — **Ferric Oxide Cassette** (ikon kaset pita magnetik *Type-I Ferric Oxide* 3D miring `-10°` berbezel tembaga `#AE7D57 -> #63321F`, sasis grafit `#443B35 -> #111111`, jendela reel kaca buram, gelombang sutra krem `#FFF1D1`, dan dua kumparan pita tembaga `#D9541E`, tanpa huruf agar 100% menyatu dengan lini `LivelyDesk`, `WallpaperLive`, dan `TransparentBar`). File siap produksi: [ferric.svg](file:///d:/code/project/music%20player/assets/brand/ferric.svg), [ferric.png](file:///d:/code/project/music%20player/assets/brand/ferric.png) (`512px` RGBA transparan), [ferric.ico](file:///d:/code/project/music%20player/assets/brand/ferric.ico) (multi-resolusi Windows `16/24/32/48/64/128/256px`), [ferric-icon-mono.svg](file:///d:/code/project/music%20player/assets/brand/ferric-icon-mono.svg) (Android 13+ Themed Icon), serta [ferric-wordmark-amoled.svg](file:///d:/code/project/music%20player/assets/brand/ferric-wordmark-amoled.svg) & [ferric-wordmark-cream.svg](file:///d:/code/project/music%20player/assets/brand/ferric-wordmark-cream.svg).

---

## 1. Identitas Default: Ketat 3 Warna (Anti-Slop R-29)

Seluruh layar default aplikasi (**tidak ada hijau neon, biru cyan, merah menyala, maupun merah muda di chrome UI**) hanya memakai **3 warna identitas produk** selaras dengan lini desain `LivelyDesk`:

| Peran | Nama Warna | Kode HEX Default | Turunan Netral / Aksen | Fungsi & Rasio Kontras (WCAG) |
| :--- | :--- | :--- | :--- | :--- |
| **1. Canvas & Ink Black** | **Pure OLED Black / Deep Charcoal** | **`#000000`** *(Base)*<br>**`#111111`** *(Surface 1)* | `#1A1A1A` *(Surface 2)*<br>`#262626` *(Hairline 1px)* | Latar utama AMOLED hemat daya (0% piksel menyala pada `#000000`), panel samping, kartu lagu, dan batas tipis tanpa *drop shadow* berlebihan. |
| **2. Cream (Sand White)** | **Warm Cream Ink** | **`#F3EEE3`** *(Primary)* | `#ADA79C` *(Secondary)*<br>`#8A847A` *(Muted)* | Judul lagu, heading editorial, ikon vektor, metadata sekunder, dan latar utama saat mode *Cream Light* dipilih. Kontras `#F3EEE3` di atas `#000000` = **17.9 : 1** (AAA); `#ADA79C` di atas `#111111` = **7.9 : 1** (AAA); `#8A847A` di atas `#1A1A1A` = **4.8 : 1** (AA). |
| **3. Burnt Orange** | **Vibrant Copper Accent** | **`#D9541E`** *(Accent)* | `#2A1309` *(Tinted Active Surface)* | **Satu-satunya warna aksen kromatik.** Digunakan terbatas hanya untuk titik fokus utama: tombol *Play/Pause*, garis progres *seekbar*, judul lagu yang sedang diputar, indikator perangkat aktif *Ferric Connect*, dan kurva Parametric EQ. Kontras `#D9541E` di atas `#000000` = **5.23 : 1** (AA); ikon `#000000` di atas `#D9541E` = **5.23 : 1** (AA). |

### Disiplin Anti-Slop pada Identitas Default (R-31)
- **Nol Badge Warna-Warni:** Tag `SPOTIFY`, `YT MUSIC`, `LOCAL FLAC`, `P2P HOME`, maupun `Connect` memakai **monokrom hangat** (`#1A1A1A` dengan hairline `#2A2A2A` dan teks `#ADA79C` / `#F3EEE3`). Warna `#D9541E` hanya menyala pada elemen yang benar-benar sedang aktif (*playing*, *selected*, atau *primary CTA*).
- **Tipografi Berkarakter:** `Nunito Sans` (keterbacaan tinggi pada ukuran `12–15px` lintas Windows & Android) + `Liberation Mono` (`tnum`) khusus data telemetri (`03:42`, `24-bit / 96 kHz`, `+3.5 dB`, `LAN 2 ms`).
- **Hierarki Radius Konsisten (R-11):** `4px` badge teknis, `8px` tombol sekunder & artwork kecil, `12px–16px` panel & sheet, *pill* (`999px`) khusus tombol Play/Pause utama & thumb slider.

---

## 2. Unlimited User Theme Studio (`Settings > Appearance`)

Identitas 3 warna di atas adalah **tampilan bawaan (default)** yang bersih dan berkarakter. Di dalam menu **Appearance Studio**, pengguna diberi **kebebasan penuh tanpa batas** untuk merancang ulang tampilan aplikasinya sendiri:

1. **Per-Token Full Color & Opacity Editor:**
   - Pengguna dapat memilih warna apa pun (HSV Color Wheel + HEX/RGB/HSL input + Alpha/Opacity) secara terpisah untuk: `Canvas Background`, `Panel Surface`, `Elevated Card`, `Hairline Border`, `Primary Text`, `Secondary Text`, dan `Accent Color`.
   - Dilengkapi **Live WCAG Contrast Guard** (menampilkan skor kontras real-time seperti `17.9:1 AAA` dan peringatan lembut bila kombinasi buatan user sulit terbaca).
2. **Custom Background, Wallpaper, Translucency & Liquid Glass Toggle (`OFF` Default / `ON` Opsional):**
   - Pilihan latar belakang bebas: **Solid Pure OLED (`#000000`)**, **Custom Gradient** (atur arah & stop warna bebas), **Custom Image / Wallpaper Lokal** (dengan slider *Dim Overlay* & *Blur `0–32px`*), atau **Windows 11 Mica / Acrylic Native Translucency** dengan kontrol *Panel Opacity `0%–100%`*.
   - **Toggle `Liquid Glass Theme (Refractive UI)` (`OFF` default / `ON` opsional di seluruh UI/UX Windows & Android):**
     - Saat **OFF (Default):** 100% Pure AMOLED `#000000`, `0%` GPU idle, ~28–32 MB RAM, hemat baterai OLED maksimal.
     - Saat **ON:** Mengaktifkan **Makepad SDF Optical Liquid Glass Fragment Shader** (kompilasi native ke HLSL DX11 di Windows & GLSL ES/Vulkan di Android) pada seluruh elemen mengambang lintas UI/UX: *floating sidebar rail*, *top search pill*, *floating bottom player dock*, *always-on-top mini-player widget*, panel *Ferric Connect* & *EQ*, *Android Now Playing transport deck*, *Android floating mini-player & bottom navigation capsule*, serta *Command Palette (`Ctrl+K`)*.
     - **6 Parameter Optik Bebas Diatur Real-Time:** `Refraction Index (IOR 1.00–2.00, default 1.48)`, `Chromatic Edge Dispersion (0%–100%, default 42%)`, `Interior Frost Blur (0–32px, default 10px)`, `Bevel Curvature Radius (8–36px, default 24px)`, `Specular Rim Catchlight (0%–100%, default 78%)`, dan `WCAG Readability Scrim (0%–60%, default 28% dengan penjaga kontras AA+ >= 4.5:1)`.
     - **Zero-GPU Safeguard Otomatis:** Shader hanya melakukan *pass* saat ada *dirty frame* (scroll, drag, hover, animasi -> `0.0%` GPU saat statis, tambahan VRAM ~3–4 MB) dan otomatis mundur ke mode AMOLED solid saat jendela diminimalkan, layar Android terkunci, atau *Battery Saver* aktif.
     - Lihat demonstrasi lengkap layar Desktop & Android mode ON pada **Page `05 Liquid Glass Mode (ON)`** di [ferric-ui-ux.figcraft](file:///d:/code/project/music%20player/ferric-ui-ux.figcraft) serta pratinjau visual di [assets/ui_previews/](file:///d:/code/project/music%20player/assets/ui_previews).
   - Opsional toggle **Adaptive Accent from Album Art** (ekstraksi warna otomatis dari sampul lagu aktif).
3. **Typography & Geometry Customizer:**
   - **Font Picker Bebas:** Pilih font sistem apa pun yang terinstal di Windows/Android, atau impor file `.ttf` / `.otf` kustom.
   - **Slider Geometri Penuh:** `Font Scale (85%–130%)`, `Corner Radius (0px Sharp – 24px Soft)`, `Track Row Height (36px – 72px)`, `Border Weight (0px – 2px)`, dan `Content Padding`.
4. **Modular Layout & Chrome Controls:**
   - Posisi Sidebar (`Left` / `Right` / `Collapsed` / `Hidden`), Gaya Player Bar (`Full Docked` / `Floating Bar` / `Minimal`), visibilitas Panel Kanan (`Connect & Queue`), dan visibilitas *Engine Telemetry Strip*.
5. **Theme Pack Import/Export & Sync Lintas Perangkat:**
   - Simpan preset buatan sendiri tanpa batas, **Export / Import `.ferric-theme.json`** untuk berbagi dengan komunitas, sinkronkan tema kustom otomatis antara PC Windows dan HP Android via `Ferric Connect`, dan tombol 1-klik **Reset to 3-Color Default**.
