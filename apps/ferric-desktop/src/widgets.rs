use makepad_widgets::*;

live_design! {
    use link::theme::*;
    use link::shaders::*;
    use link::widgets::*;

    pub DrawFerricSlider = {{DrawFerricSlider}} {}
    pub DrawFerricToggle = {{DrawFerricToggle}} {}
    pub DrawFerricWinBtn = {{DrawFerricWinBtn}} {}
    pub DrawFerricPlayBtn = {{DrawFerricPlayBtn}} {}
    pub DrawFerricEqGraph = {{DrawFerricEqGraph}} {}
    pub DrawFerricCoverArt = {{DrawFerricCoverArt}} {}
    pub DrawLyricsPill = {{DrawLyricsPill}} {}

    pub FerricLyricsView = {{FerricLyricsView}} {
        width: Fill,
        height: Fill,
        compact: false,
        draw_pill: {
            accent: #A882FF
            alpha: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 8.0);
                let fill_col = vec4(self.accent.xyz, 0.13 * self.alpha);
                sdf.fill_keep(fill_col);
                sdf.stroke(vec4(self.accent.xyz, 0.24 * self.alpha), 1.0);
                // Crisp 3px left accent beacon inside active lyric pill
                sdf.box(4.0, 6.0, 3.0, max(self.rect_size.y - 12.0, 4.0), 1.5);
                sdf.fill(vec4(self.accent.xyz, 0.92 * self.alpha));
                return sdf.result;
            }
        }
        draw_line: {
            color: #9A9EAE,
            text_style: <THEME_FONT_MEDIUM> {
                font_size: 11.5,
                line_spacing: 1.15
            }
        }
        draw_active: {
            color: #FFFFFF,
            text_style: <THEME_FONT_DISPLAY> {
                font_size: 13.5,
                line_spacing: 1.12
            }
        }
    }

    pub FerricCoverArt = {{FerricCoverArt}} {
        width: 48,
        height: 48,
        draw_bg: {
            texture image: texture2d
            accent: #A882FF
            uniform fallback_col: #1C1D24
            uniform border_col: #2A2C38

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                let c = self.rect_size * 0.5;
                let min_dim = min(self.rect_size.x, self.rect_size.y);

                if (self.circle > 0.5) {
                    sdf.circle(c.x, c.y, min_dim * 0.5 - 0.5);
                } else {
                    let r = min(self.radius * 0.5, min_dim * 0.22);
                    sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);
                }

                let mut rgb = self.fallback_col.xyz;
                if (self.has_image > 0.5) {
                    let box_aspect = self.rect_size.x / max(self.rect_size.y, 1.0);
                    let mut uv = self.pos;
                    if (self.img_aspect > box_aspect) {
                        let scale = box_aspect / max(self.img_aspect, 0.01);
                        uv.x = (uv.x - 0.5) * scale + 0.5;
                    } else {
                        let scale = max(self.img_aspect, 0.01) / max(box_aspect, 0.01);
                        uv.y = (uv.y - 0.5) * scale + 0.5;
                    }
                    let sampled = sample2d(self.image, clamp(uv, vec2(0.002, 0.002), vec2(0.998, 0.998)));
                    rgb = sampled.xyz;
                } else {
                    // Subtle obsidian placeholder with gentle sheen while cover loads
                    let sheen = clamp(1.0 - (self.pos.x * 0.55 + self.pos.y * 0.75), 0.0, 1.0);
                    rgb = self.fallback_col.xyz + self.accent.xyz * (0.06 + 0.07 * sheen);
                }

                if (self.hover > 0.01) {
                    rgb = mix(rgb, vec3(1.0, 1.0, 1.0), 0.06 * self.hover);
                }

                if (self.playing > 0.5) {
                    rgb = mix(rgb, vec3(0.04, 0.04, 0.06), 0.44);
                }

                sdf.fill_keep(vec4(clamp(rgb, 0.0, 1.0), 1.0));
                sdf.stroke(mix(self.border_col, vec4(self.accent.xyz, 0.75), self.playing), 1.0);

                if (self.playing > 0.5) {
                    let bw = max(min_dim * 0.095, 2.5);
                    let gap = max(min_dim * 0.055, 2.0);
                    let base_y = self.rect_size.y * 0.76;
                    let h1 = min_dim * (0.20 + 0.28 * self.level);
                    let h2 = min_dim * (0.36 + 0.22 * (1.0 - self.level * 0.45));
                    let h3 = min_dim * (0.26 + 0.30 * self.level);

                    sdf.box(c.x - bw * 1.5 - gap, base_y - h1, bw, h1, 1.2);
                    sdf.fill(self.accent);
                    sdf.box(c.x - bw * 0.5, base_y - h2, bw, h2, 1.2);
                    sdf.fill(#FFFFFF);
                    sdf.box(c.x + bw * 0.5 + gap, base_y - h3, bw, h3, 1.2);
                    sdf.fill(self.accent);
                }

                return sdf.result;
            }
        }
    }

    pub FerricPlayBtn = {{FerricPlayBtn}} {
        width: 36,
        height: 36,
        draw_bg: {
            uniform accent_col: #F4F4F7
            uniform accent_hover: #FFFFFF
            uniform glyph_col: #0E0F13

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                let c = self.rect_size * 0.5;
                let r = min(c.x, c.y) - 1.0;
                sdf.circle(c.x, c.y, r);
                sdf.fill(mix(self.accent_col, self.accent_hover, self.hover));

                if (self.playing > 0.5) {
                    sdf.box(c.x - 4.8, c.y - 5.8, 3.3, 11.6, 0.7);
                    sdf.box(c.x + 1.5, c.y - 5.8, 3.3, 11.6, 0.7);
                    sdf.fill(self.glyph_col);
                } else {
                    sdf.move_to(c.x - 3.6, c.y - 6.0);
                    sdf.line_to(c.x + 6.0, c.y);
                    sdf.line_to(c.x - 3.6, c.y + 6.0);
                    sdf.close_path();
                    sdf.fill(self.glyph_col);
                }
                return sdf.result;
            }
        }
    }

    pub FerricBarSlider = {{FerricBarSlider}} {
        width: Fill,
        height: 20,
        draw_bg: {
            uniform track_col: #2A2C38
            uniform fill_col: #A882FF
            uniform thumb_col: #F4F4F7

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                let pad = 7.0;
                let usable = max(self.rect_size.x - pad * 2.0, 2.0);
                let cy = self.rect_size.y * 0.5;
                let t = clamp(self.norm_val, 0.0, 1.0);
                let tx = pad + usable * t;

                sdf.box(pad, cy - 1.5, usable, 3.0, 0.75);
                sdf.fill(self.track_col);

                if (tx > pad + 0.5) {
                    sdf.box(pad, cy - 1.5, tx - pad, 3.0, 0.75);
                    sdf.fill(self.fill_col);
                }

                let active_amt = max(self.hover, self.drag);
                if (active_amt > 0.01) {
                    sdf.circle(tx, cy, 8.5);
                    sdf.fill(vec4(self.fill_col.xyz, 0.24 * active_amt));
                }

                let r = mix(4.2, 5.8, active_amt);
                sdf.circle(tx, cy, r);
                sdf.fill_keep(self.thumb_col);
                sdf.stroke(#0E0F13, 0.9);
                return sdf.result;
            }
        }
    }

    pub FerricVertSlider = {{FerricVertSlider}} {
        width: 34,
        height: 148,
        min: -12.0,
        max: 12.0,
        value: 0.0,
        draw_bg: {
            uniform track_col: #262834
            uniform fill_col: #A882FF
            uniform thumb_col: #F4F4F7

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                let pad = 9.0;
                let usable = max(self.rect_size.y - pad * 2.0, 4.0);
                let cx = self.rect_size.x * 0.5;
                let t = clamp(self.norm_val, 0.0, 1.0);
                let ty = self.rect_size.y - pad - usable * t;
                let mid_y = self.rect_size.y * 0.5;

                sdf.rect(cx - 6.0, mid_y - 0.5, 12.0, 1.0);
                sdf.fill(#323544);

                sdf.box(cx - 1.75, pad, 3.5, usable, 0.85);
                sdf.fill(self.track_col);

                let bar_h = max((self.rect_size.y - pad) - ty, 1.0);
                sdf.box(cx - 1.75, ty, 3.5, bar_h, 0.85);
                sdf.fill(self.fill_col);

                let active_amt = max(self.hover, self.drag);
                if (active_amt > 0.01) {
                    sdf.circle(cx, ty, 9.5);
                    sdf.fill(vec4(self.fill_col.xyz, 0.24 * active_amt));
                }

                let r = mix(5.2, 6.4, active_amt);
                sdf.circle(cx, ty, r);
                sdf.fill_keep(self.thumb_col);
                sdf.stroke(#0E0F13, 1.1);
                return sdf.result;
            }
        }
    }

    pub FerricToggle = {{FerricToggle}} {
        width: 42,
        height: 23,
        draw_bg: {
            uniform on_col: #A882FF
            uniform off_col: #222430
            uniform border_col: #323544
            uniform knob_col: #F4F4F7

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                let h = self.rect_size.y;
                let box_r = (h - 2.0) * 0.25;
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, h - 2.0, box_r);
                let bg = mix(self.off_col, self.on_col, self.active);
                sdf.fill_keep(bg);
                let bcol = mix(self.border_col, self.on_col, self.active);
                sdf.stroke(bcol, 1.0);

                let kx = mix(h * 0.5, self.rect_size.x - h * 0.5, self.active);
                let kr = (h - 2.0) * 0.5 - 3.0 + self.hover * 0.5;
                sdf.circle(kx, h * 0.5, kr);
                sdf.fill(self.knob_col);
                return sdf.result;
            }
        }
    }

    pub FerricWinBtn = {{FerricWinBtn}} {
        width: 36,
        height: 28,
        draw_bg: {
            uniform fg_col: #9A9CA8
            uniform fg_hover: #F4F4F7

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                let c = self.rect_size * 0.5;

                if (self.hover > 0.01) {
                    sdf.box(2.0, 2.0, self.rect_size.x - 4.0, self.rect_size.y - 4.0, 3.5);
                    if self.kind > 1.5 && self.kind < 2.5 {
                        sdf.fill(vec4(0.80, 0.22, 0.20, 0.92 * self.hover));
                    } else {
                        sdf.fill(vec4(0.18, 0.19, 0.25, 0.90 * self.hover));
                    }
                }

                let stroke_col = mix(self.fg_col, self.fg_hover, self.hover);
                if self.kind < 0.5 {
                    sdf.move_to(c.x - 5.0, c.y + 0.5);
                    sdf.line_to(c.x + 5.0, c.y + 0.5);
                    sdf.stroke(stroke_col, 1.25);
                } else if self.kind < 1.5 {
                    if self.is_maximized > 0.5 {
                        sdf.rect(c.x - 3.0, c.y - 5.0, 7.5, 7.5);
                        sdf.stroke(stroke_col, 1.15);
                        sdf.rect(c.x - 5.0, c.y - 3.0, 7.5, 7.5);
                        sdf.fill_keep(#15161C);
                        sdf.stroke(stroke_col, 1.2);
                    } else {
                        sdf.rect(c.x - 4.5, c.y - 4.5, 9.0, 9.0);
                        sdf.stroke(stroke_col, 1.2);
                    }
                } else if self.kind < 2.5 {
                    sdf.move_to(c.x - 4.5, c.y - 4.5);
                    sdf.line_to(c.x + 4.5, c.y + 4.5);
                    sdf.move_to(c.x + 4.5, c.y - 4.5);
                    sdf.line_to(c.x - 4.5, c.y + 4.5);
                    sdf.stroke(stroke_col, 1.3);
                } else {
                    sdf.box(c.x - 6.0, c.y - 4.5, 12.0, 9.0, 1.5);
                    sdf.stroke(stroke_col, 1.15);
                    sdf.rect(c.x + 0.2, c.y - 0.2, 4.8, 3.8);
                    sdf.fill(#A882FF);
                }
                return sdf.result;
            }
        }
    }

    pub FerricArtTile = <View> {
        width: 44,
        height: 44,
        show_bg: true,
        draw_bg: {
            instance variant: 0.0
            instance playing: 0.0
            instance level: 0.55
            uniform radius: 8.0
            uniform accent: #D9541E

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                let r = min(self.radius * 0.5, min(self.rect_size.x, self.rect_size.y) * 0.14);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);

                let uv = self.pos;
                let c = self.rect_size * 0.5;
                let min_dim = min(self.rect_size.x, self.rect_size.y);

                if self.playing > 0.5 {
                    // Active track equalizer bars on warm dark copper surface (matching FigCraft D1)
                    let bg = mix(vec3(0.11, 0.05, 0.03), vec3(0.05, 0.03, 0.02), uv.y);
                    sdf.fill_keep(vec4(bg, 1.0));
                    sdf.stroke(vec4(self.accent.xyz, 0.55), 1.0);

                    let bw = min_dim * 0.11;
                    let gap = min_dim * 0.06;
                    let base_y = self.rect_size.y * 0.78;
                    let h1 = min_dim * (0.24 + 0.30 * self.level);
                    let h2 = min_dim * (0.42 + 0.22 * (1.0 - self.level * 0.5));
                    let h3 = min_dim * (0.30 + 0.34 * self.level);

                    sdf.box(c.x - bw * 1.5 - gap, base_y - h1, bw, h1, 1.5);
                    sdf.fill(self.accent);
                    sdf.box(c.x - bw * 0.5, base_y - h2, bw, h2, 1.5);
                    sdf.fill(self.accent);
                    sdf.box(c.x + bw * 0.5 + gap, base_y - h3, bw, h3, 1.5);
                    sdf.fill(#F3EEE3);
                    return sdf.result;
                }

                if self.variant < 0.5 {
                    // Sunset Horizon Disc (matching FigCraft LG1 & LG2)
                    let sky = mix(vec3(0.92, 0.44, 0.14), vec3(0.58, 0.16, 0.09), clamp(uv.y * 1.25, 0.0, 1.0));
                    let sea = mix(vec3(0.22, 0.06, 0.06), vec3(0.08, 0.03, 0.04), clamp((uv.y - 0.58) * 2.4, 0.0, 1.0));
                    let mut col = mix(sky, sea, step(0.60, uv.y));
                    if uv.y > 0.62 && uv.y < 0.66 { col = vec3(0.75, 0.25, 0.14); }
                    if uv.y > 0.70 && uv.y < 0.73 { col = vec3(0.55, 0.17, 0.11); }
                    if uv.y > 0.78 && uv.y < 0.805 { col = vec3(0.36, 0.11, 0.09); }
                    let sun_d = length(p - vec2(c.x, self.rect_size.y * 0.56));
                    let sun_r = min_dim * 0.21;
                    let sun_mask = clamp(sun_r - sun_d + 0.8, 0.0, 1.0);
                    let sun_col = mix(vec3(0.98, 0.95, 0.88), vec3(0.95, 0.76, 0.62), step(0.60, uv.y));
                    col = mix(col, sun_col, sun_mask);
                    sdf.fill_keep(vec4(col, 1.0));
                    sdf.stroke(#2A1A12, 1.0);
                    return sdf.result;
                } else {
                    // Oxide Vinyl Ring (matching FigCraft D1 Hero & Track Thumbnails)
                    let bg = mix(vec3(0.11, 0.05, 0.025), vec3(0.06, 0.03, 0.02), uv.y);
                    sdf.fill_keep(vec4(bg, 1.0));
                    sdf.stroke(#241812, 1.0);

                    sdf.circle(c.x, c.y, min_dim * 0.34);
                    sdf.stroke(self.accent, max(min_dim * 0.032, 1.3));
                    sdf.circle(c.x, c.y, min_dim * 0.12);
                    sdf.fill(self.accent);
                    return sdf.result;
                }
            }
        }
    }

    pub FerricEqGraph = {{FerricEqGraph}} {
        width: Fill,
        height: 176,
        draw_bg: {
            uniform accent: #A882FF
            uniform bg_col: #12131A
            uniform border_col: #232530

            fn eval_band_y(self, idx: float) -> float {
                if idx < 0.5 { return self.g0; }
                if idx < 1.5 { return self.g1; }
                if idx < 2.5 { return self.g2; }
                if idx < 3.5 { return self.g3; }
                if idx < 4.5 { return self.g4; }
                if idx < 5.5 { return self.g5; }
                if idx < 6.5 { return self.g6; }
                if idx < 7.5 { return self.g7; }
                if idx < 8.5 { return self.g8; }
                return self.g9;
            }

            fn sample_curve_db(self, x_norm: float) -> float {
                // Smooth Catmull-Rom / Cosine interpolation across the 10 octave bands
                let pos = clamp(x_norm, 0.0, 1.0) * 9.0;
                let idx = floor(pos);
                let frac = pos - idx;
                let smooth_t = (1.0 - cos(frac * 3.14159265)) * 0.5;
                let v0 = self.eval_band_y(idx);
                let v1 = self.eval_band_y(min(idx + 1.0, 9.0));
                return clamp((mix(v0, v1, smooth_t) + self.preamp * 0.5) * self.enabled, -14.0, 14.0);
            }

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 6.0);
                sdf.fill_keep(self.bg_col);
                sdf.stroke(self.border_col, 1.0);

                let left = 52.0;
                let right = self.rect_size.x - 22.0;
                let top = 20.0;
                let bot = self.rect_size.y - 20.0;
                let h = max(bot - top, 10.0);
                let w = max(right - left, 10.0);
                let mid_y = (top + bot) * 0.5;

                // Horizontal dB gridlines (+12, +6, 0, -6, -12 dB)
                for i in 0..5 {
                    let fi = float(i);
                    let gy = top + h * (fi / 4.0);
                    if (p.x >= left && p.x <= right && abs(p.y - gy) < 0.6) {
                        if (fi > 1.5 && fi < 2.5) {
                            return vec4(0.20, 0.20, 0.20, 1.0);
                        } else {
                            return vec4(0.11, 0.11, 0.11, 1.0);
                        }
                    }
                }

                if (p.x >= left && p.x <= right) {
                    let xn = (p.x - left) / w;
                    let db = self.sample_curve_db(xn);
                    let cy = mid_y - (db / 14.0) * (h * 0.5);
                    let dist = abs(p.y - cy);

                    // Subtle warm copper fill between curve and 0 dB axis
                    let mut rgb = self.bg_col.xyz;
                    if (p.y >= min(cy, mid_y) && p.y <= max(cy, mid_y)) {
                        let fade = 1.0 - clamp(abs(p.y - cy) / (h * 0.55), 0.0, 1.0);
                        rgb += self.accent.xyz * (0.13 * fade * max(self.enabled, 0.25));
                    }

                    // Smooth anti-aliased 2.2px response curve
                    let line_alpha = clamp(1.9 - dist, 0.0, 1.0);
                    let line_col = mix(vec3(0.38, 0.36, 0.34), self.accent.xyz, self.enabled);
                    rgb = mix(rgb, line_col, line_alpha);

                    // 10 glowing band anchor nodes
                    for bi in 0..10 {
                        let bx = left + w * (float(bi) / 9.0);
                        let bdb = clamp((self.eval_band_y(float(bi)) + self.preamp * 0.5) * self.enabled, -14.0, 14.0);
                        let by = mid_y - (bdb / 14.0) * (h * 0.5);
                        let nd = length(p - vec2(bx, by));
                        if nd < 4.5 {
                            if nd < 2.3 {
                                rgb = vec3(0.05, 0.05, 0.05);
                            } else {
                                rgb = line_col;
                            }
                        }
                    }
                    return vec4(rgb, 1.0);
                }

                return sdf.result;
            }
        }
    }

    pub FerricQrView = {{FerricQrView}} {
        width: 148,
        height: 148,
        draw_bg: { color: #F3EEE3 }
        draw_mod: { color: #000000 }
    }

    pub FerricNowPlayingCanvas = <View> {
        width: Fill,
        height: 96,
        show_bg: true,
        draw_bg: {
            instance mode: 1.0
            instance phase: 0.0
            instance s0: 0.35
            instance s1: 0.55
            instance s2: 0.48
            instance s3: 0.62
            instance s4: 0.42
            instance s5: 0.50
            instance s6: 0.30
            instance s7: 0.24
            uniform accent: #A882FF
            uniform acrylic_on: 0.0

            fn bin_val(self, i: float) -> float {
                if (i < 0.5) { return self.s0; }
                if (i < 1.5) { return self.s1; }
                if (i < 2.5) { return self.s2; }
                if (i < 3.5) { return self.s3; }
                if (i < 4.5) { return self.s4; }
                if (i < 5.5) { return self.s5; }
                if (i < 6.5) { return self.s6; }
                return self.s7;
            }

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                let uv = self.pos;
                let energy = (self.s0 + self.s1 + self.s2 + self.s3) * 0.25;

                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 10.0);
                let wave = clamp(0.5 + 0.5 * sin(uv.x * 4.0 + self.phase * 1.15), 0.0, 1.0);
                let base_dark = mix(vec3(0.035, 0.037, 0.048), vec3(0.08, 0.085, 0.115), self.acrylic_on);
                let ambient = self.accent.xyz * (0.08 + 0.14 * energy * wave);
                let acrylic_sheen = self.acrylic_on * smoothstep(0.32, 0.0, uv.y) * 0.06;
                let bg = clamp(base_dark + ambient + vec3(acrylic_sheen, acrylic_sheen, acrylic_sheen), 0.0, 1.0);
                sdf.fill_keep(vec4(bg, mix(0.96, 0.84, self.acrylic_on)));

                // Clean top-catchlight border
                let rim = mix(vec4(self.accent.xyz, 0.24), vec4(1.0, 1.0, 1.0, 0.22), self.acrylic_on * smoothstep(0.6, 0.0, uv.y));
                sdf.stroke(rim, 1.0);

                // Smooth 24-bar centered rounded capsule spectrum visualizer (no overlapping circle clutter)
                let pad_x = 18.0;
                let span_x = max(self.rect_size.x - pad_x * 2.0, 24.0);
                let cy = self.rect_size.y * 0.5;
                let max_half = self.rect_size.y * 0.36;
                let bars = 24.0;
                let cell_w = span_x / bars;
                let bar_w = max(cell_w * 0.44, 3.0);

                if (p.x >= pad_x && p.x <= (self.rect_size.x - pad_x)) {
                    let norm_x = (p.x - pad_x) / span_x;
                    let slot = floor(norm_x * bars);
                    let cell_local = (norm_x * bars - slot) * cell_w;
                    let sym_f = abs(slot - (bars * 0.5 - 0.5)) / (bars * 0.5) * 7.0;
                    let i0 = floor(sym_f);
                    let i1 = min(i0 + 1.0, 7.0);
                    let f = sym_f - i0;
                    let raw_bv = mix(self.bin_val(i0), self.bin_val(i1), f);
                    let pulse = 0.08 * sin(self.phase * 1.9 + slot * 0.45) * (0.35 + energy);
                    let bv = clamp(raw_bv * 1.15 + pulse, 0.08, 1.0);
                    let half_h = max(max_half * bv, 3.5);
                    let dx = abs(cell_local - cell_w * 0.5) - bar_w * 0.5;
                    let dy = abs(p.y - cy) - half_h;
                    let r_cap = bar_w * 0.5;
                    let qx = max(dx + r_cap, 0.0);
                    let qy = max(dy + r_cap, 0.0);
                    let dist = length(vec2(qx, qy)) - r_cap;
                    let alpha = 1.0 - smoothstep(-0.7, 0.7, dist);
                    if (alpha > 0.01) {
                        let t_v = 1.0 - clamp(abs(p.y - cy) / max(half_h, 1.0), 0.0, 1.0);
                        let bar_col = mix(self.accent.xyz, vec3(0.97, 0.97, 0.99), t_v * 0.52);
                        let out_rgb = mix(sdf.result.xyz, bar_col, alpha * 0.95);
                        return vec4(out_rgb, sdf.result.w);
                    }
                }

                return sdf.result;
            }
        }
    }
}

#[derive(Clone, Debug, DefaultNone)]
pub enum FerricSliderAction {
    Changed(f64),
    Released(f64),
    None,
}

#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
pub struct DrawFerricSlider {
    #[deref]
    draw_super: DrawQuad,
    #[live(0.0_f32)]
    norm_val: f32,
    #[live(0.0_f32)]
    hover: f32,
    #[live(0.0_f32)]
    drag: f32,
}

#[derive(Live, LiveHook, Widget)]
pub struct FerricBarSlider {
    #[walk]
    walk: Walk,
    #[redraw]
    #[live]
    draw_bg: DrawFerricSlider,
    #[live(0.0)]
    min: f64,
    #[live(1.0)]
    max: f64,
    #[live(0.0)]
    value: f64,
    #[rust]
    dragging: bool,
}

impl FerricBarSlider {
    fn update_from_abs(&mut self, cx: &mut Cx, abs_x: f64) -> f64 {
        let rect = self.draw_bg.area().rect(cx);
        let pad = 7.0;
        let w = (rect.size.x - pad * 2.0).max(2.0);
        let norm = ((abs_x - (rect.pos.x + pad)) / w).clamp(0.0, 1.0);
        self.value = self.min + norm * (self.max - self.min);
        self.draw_bg.norm_val = norm as f32;
        self.draw_bg.redraw(cx);
        self.value
    }
}

impl Widget for FerricBarSlider {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        let uid = self.widget_uid();
        match event.hits(cx, self.draw_bg.area()) {
            Hit::FingerHoverIn(_) => {
                cx.set_cursor(MouseCursor::Hand);
                self.draw_bg.hover = 1.0;
                self.draw_bg.redraw(cx);
            }
            Hit::FingerHoverOut(_) => {
                self.draw_bg.hover = 0.0;
                self.draw_bg.redraw(cx);
            }
            Hit::FingerDown(fe) => {
                self.dragging = true;
                self.draw_bg.drag = 1.0;
                let v = self.update_from_abs(cx, fe.abs.x);
                cx.widget_action(uid, &scope.path, FerricSliderAction::Changed(v));
            }
            Hit::FingerMove(fe) if self.dragging => {
                let v = self.update_from_abs(cx, fe.abs.x);
                cx.widget_action(uid, &scope.path, FerricSliderAction::Changed(v));
            }
            Hit::FingerUp(fe) if self.dragging => {
                self.dragging = false;
                self.draw_bg.drag = 0.0;
                let v = self.update_from_abs(cx, fe.abs.x);
                cx.widget_action(uid, &scope.path, FerricSliderAction::Released(v));
            }
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let span = (self.max - self.min).abs().max(1e-6);
        self.draw_bg.norm_val = ((self.value - self.min) / span).clamp(0.0, 1.0) as f32;
        let _ = self.draw_bg.draw_walk(cx, walk);
        DrawStep::done()
    }
}

impl FerricBarSliderRef {
    pub fn set_value(&self, cx: &mut Cx, val: f64) {
        if let Some(mut inner) = self.borrow_mut()
            && !inner.dragging
        {
            inner.value = val.clamp(inner.min, inner.max);
            inner.draw_bg.redraw(cx);
        }
    }

    pub fn changed(&self, actions: &Actions) -> Option<f64> {
        match actions.find_widget_action(self.widget_uid()).cast() {
            FerricSliderAction::Changed(v) => Some(v),
            _ => None,
        }
    }

    pub fn released(&self, actions: &Actions) -> Option<f64> {
        match actions.find_widget_action(self.widget_uid()).cast() {
            FerricSliderAction::Released(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Live, LiveHook, Widget)]
pub struct FerricVertSlider {
    #[walk]
    walk: Walk,
    #[redraw]
    #[live]
    draw_bg: DrawFerricSlider,
    #[live(-12.0)]
    min: f64,
    #[live(12.0)]
    max: f64,
    #[live(0.0)]
    value: f64,
    #[rust]
    dragging: bool,
}

impl FerricVertSlider {
    fn update_from_abs(&mut self, cx: &mut Cx, abs_y: f64) -> f64 {
        let rect = self.draw_bg.area().rect(cx);
        let pad = 9.0;
        let h = (rect.size.y - pad * 2.0).max(4.0);
        let norm = (1.0 - (abs_y - (rect.pos.y + pad)) / h).clamp(0.0, 1.0);
        self.value = self.min + norm * (self.max - self.min);
        self.draw_bg.norm_val = norm as f32;
        self.draw_bg.redraw(cx);
        self.value
    }
}

impl Widget for FerricVertSlider {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        let uid = self.widget_uid();
        match event.hits(cx, self.draw_bg.area()) {
            Hit::FingerHoverIn(_) => {
                cx.set_cursor(MouseCursor::Hand);
                self.draw_bg.hover = 1.0;
                self.draw_bg.redraw(cx);
            }
            Hit::FingerHoverOut(_) => {
                self.draw_bg.hover = 0.0;
                self.draw_bg.redraw(cx);
            }
            Hit::FingerDown(fe) => {
                if fe.tap_count >= 2 {
                    self.value = 0.0;
                    self.draw_bg.redraw(cx);
                    cx.widget_action(uid, &scope.path, FerricSliderAction::Changed(0.0));
                    return;
                }
                self.dragging = true;
                self.draw_bg.drag = 1.0;
                let v = self.update_from_abs(cx, fe.abs.y);
                cx.widget_action(uid, &scope.path, FerricSliderAction::Changed(v));
            }
            Hit::FingerMove(fe) if self.dragging => {
                let v = self.update_from_abs(cx, fe.abs.y);
                cx.widget_action(uid, &scope.path, FerricSliderAction::Changed(v));
            }
            Hit::FingerUp(fe) if self.dragging => {
                self.dragging = false;
                self.draw_bg.drag = 0.0;
                let v = self.update_from_abs(cx, fe.abs.y);
                cx.widget_action(uid, &scope.path, FerricSliderAction::Released(v));
            }
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let span = (self.max - self.min).abs().max(1e-6);
        self.draw_bg.norm_val = ((self.value - self.min) / span).clamp(0.0, 1.0) as f32;
        let _ = self.draw_bg.draw_walk(cx, walk);
        DrawStep::done()
    }
}

impl FerricVertSliderRef {
    pub fn set_value(&self, cx: &mut Cx, val: f64) {
        if let Some(mut inner) = self.borrow_mut()
            && !inner.dragging
        {
            inner.value = val.clamp(inner.min, inner.max);
            inner.draw_bg.redraw(cx);
        }
    }

    pub fn changed(&self, actions: &Actions) -> Option<f64> {
        match actions.find_widget_action(self.widget_uid()).cast() {
            FerricSliderAction::Changed(v) | FerricSliderAction::Released(v) => Some(v),
            FerricSliderAction::None => None,
        }
    }
}

#[derive(Clone, Debug, DefaultNone)]
pub enum FerricToggleAction {
    Toggled(bool),
    None,
}

#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
pub struct DrawFerricToggle {
    #[deref]
    draw_super: DrawQuad,
    #[live(0.0_f32)]
    active: f32,
    #[live(0.0_f32)]
    hover: f32,
}

#[derive(Live, LiveHook, Widget)]
pub struct FerricToggle {
    #[walk]
    walk: Walk,
    #[redraw]
    #[live]
    draw_bg: DrawFerricToggle,
    #[live(false)]
    is_on: bool,
    #[rust]
    visual_active: f32,
    #[rust]
    visual_hover: f32,
    #[rust]
    hover_target: f32,
    #[rust]
    visual_synced: bool,
    #[rust]
    anim_frame: NextFrame,
}

impl Widget for FerricToggle {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if self.anim_frame.is_event(event).is_some() {
            let target_a = if self.is_on { 1.0_f32 } else { 0.0_f32 };
            let target_h = self.hover_target;
            self.visual_active += (target_a - self.visual_active) * 0.28;
            self.visual_hover += (target_h - self.visual_hover) * 0.28;
            if (target_a - self.visual_active).abs() < 0.005 {
                self.visual_active = target_a;
            }
            if (target_h - self.visual_hover).abs() < 0.005 {
                self.visual_hover = target_h;
            }
            self.draw_bg.active = self.visual_active;
            self.draw_bg.hover = self.visual_hover;
            self.draw_bg.redraw(cx);
            if (target_a - self.visual_active).abs() > 0.005
                || (target_h - self.visual_hover).abs() > 0.005
            {
                self.anim_frame = cx.new_next_frame();
            }
        }
        let uid = self.widget_uid();
        match event.hits(cx, self.draw_bg.area()) {
            Hit::FingerHoverIn(_) => {
                cx.set_cursor(MouseCursor::Hand);
                self.hover_target = 1.0;
                self.anim_frame = cx.new_next_frame();
            }
            Hit::FingerHoverOut(_) => {
                self.hover_target = 0.0;
                self.anim_frame = cx.new_next_frame();
            }
            Hit::FingerUp(fe) if fe.is_over => {
                self.is_on = !self.is_on;
                self.visual_synced = true;
                self.anim_frame = cx.new_next_frame();
                cx.widget_action(uid, &scope.path, FerricToggleAction::Toggled(self.is_on));
            }
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        if !self.visual_synced {
            self.visual_synced = true;
            self.visual_active = if self.is_on { 1.0 } else { 0.0 };
        }
        self.draw_bg.active = self.visual_active;
        self.draw_bg.hover = self.visual_hover;
        let _ = self.draw_bg.draw_walk(cx, walk);
        DrawStep::done()
    }
}

impl FerricToggleRef {
    pub fn set_active(&self, cx: &mut Cx, active: bool) {
        if let Some(mut inner) = self.borrow_mut() {
            let changed = inner.is_on != active;
            inner.is_on = active;
            if !inner.visual_synced {
                inner.visual_synced = true;
                inner.visual_active = if active { 1.0 } else { 0.0 };
                inner.draw_bg.active = inner.visual_active;
                inner.draw_bg.redraw(cx);
            } else if changed {
                inner.anim_frame = cx.new_next_frame();
            }
        }
    }

    pub fn toggled(&self, actions: &Actions) -> Option<bool> {
        match actions.find_widget_action(self.widget_uid()).cast() {
            FerricToggleAction::Toggled(b) => Some(b),
            FerricToggleAction::None => None,
        }
    }
}

#[derive(Clone, Debug, DefaultNone)]
pub enum FerricWinBtnAction {
    Clicked,
    None,
}

#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
pub struct DrawFerricWinBtn {
    #[deref]
    draw_super: DrawQuad,
    #[live(0.0_f32)]
    kind: f32,
    #[live(0.0_f32)]
    is_maximized: f32,
    #[live(0.0_f32)]
    hover: f32,
}

#[derive(Live, LiveHook, Widget)]
pub struct FerricWinBtn {
    #[walk]
    walk: Walk,
    #[redraw]
    #[live]
    draw_bg: DrawFerricWinBtn,
    #[rust]
    hover_target: f32,
    #[rust]
    anim_frame: NextFrame,
}

impl Widget for FerricWinBtn {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if self.anim_frame.is_event(event).is_some() {
            self.draw_bg.hover += (self.hover_target - self.draw_bg.hover) * 0.30;
            if (self.hover_target - self.draw_bg.hover).abs() < 0.01 {
                self.draw_bg.hover = self.hover_target;
            } else {
                self.anim_frame = cx.new_next_frame();
            }
            self.draw_bg.redraw(cx);
        }
        let uid = self.widget_uid();
        match event.hits(cx, self.draw_bg.area()) {
            Hit::FingerHoverIn(_) => {
                cx.set_cursor(MouseCursor::Hand);
                self.hover_target = 1.0;
                self.anim_frame = cx.new_next_frame();
            }
            Hit::FingerHoverOut(_) => {
                self.hover_target = 0.0;
                self.anim_frame = cx.new_next_frame();
            }
            Hit::FingerUp(fe) if fe.is_over => {
                self.hover_target = 0.0;
                self.draw_bg.hover = 0.0;
                self.draw_bg.redraw(cx);
                cx.widget_action(uid, &scope.path, FerricWinBtnAction::Clicked);
            }
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let _ = self.draw_bg.draw_walk(cx, walk);
        DrawStep::done()
    }
}

impl FerricWinBtnRef {
    pub fn clicked(&self, actions: &Actions) -> bool {
        matches!(
            actions.find_widget_action(self.widget_uid()).cast(),
            FerricWinBtnAction::Clicked
        )
    }

    pub fn set_maximized(&self, cx: &mut Cx, maximized: bool) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.draw_bg.is_maximized = if maximized { 1.0 } else { 0.0 };
            inner.draw_bg.redraw(cx);
        }
    }
}

#[derive(Clone, Debug, DefaultNone)]
pub enum FerricPlayBtnAction {
    Clicked,
    None,
}

#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
pub struct DrawFerricPlayBtn {
    #[deref]
    draw_super: DrawQuad,
    #[live(0.0_f32)]
    playing: f32,
    #[live(0.0_f32)]
    hover: f32,
}

#[derive(Live, LiveHook, Widget)]
pub struct FerricPlayBtn {
    #[walk]
    walk: Walk,
    #[redraw]
    #[live]
    draw_bg: DrawFerricPlayBtn,
    #[rust]
    hover_target: f32,
    #[rust]
    anim_frame: NextFrame,
}

impl Widget for FerricPlayBtn {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if self.anim_frame.is_event(event).is_some() {
            self.draw_bg.hover += (self.hover_target - self.draw_bg.hover) * 0.28;
            if (self.hover_target - self.draw_bg.hover).abs() < 0.01 {
                self.draw_bg.hover = self.hover_target;
            } else {
                self.anim_frame = cx.new_next_frame();
            }
            self.draw_bg.redraw(cx);
        }
        let uid = self.widget_uid();
        match event.hits(cx, self.draw_bg.area()) {
            Hit::FingerHoverIn(_) => {
                cx.set_cursor(MouseCursor::Hand);
                self.hover_target = 1.0;
                self.anim_frame = cx.new_next_frame();
            }
            Hit::FingerHoverOut(_) => {
                self.hover_target = 0.0;
                self.anim_frame = cx.new_next_frame();
            }
            Hit::FingerUp(fe) if fe.is_over => {
                cx.widget_action(uid, &scope.path, FerricPlayBtnAction::Clicked);
            }
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let _ = self.draw_bg.draw_walk(cx, walk);
        DrawStep::done()
    }
}

impl FerricPlayBtnRef {
    pub fn clicked(&self, actions: &Actions) -> bool {
        matches!(
            actions.find_widget_action(self.widget_uid()).cast(),
            FerricPlayBtnAction::Clicked
        )
    }

    pub fn set_playing(&self, cx: &mut Cx, playing: bool) {
        if let Some(mut inner) = self.borrow_mut() {
            let next = if playing { 1.0 } else { 0.0 };
            if (inner.draw_bg.playing - next).abs() > 0.1 {
                inner.draw_bg.playing = next;
                inner.draw_bg.redraw(cx);
            }
        }
    }
}

#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
pub struct DrawFerricEqGraph {
    #[deref]
    draw_super: DrawQuad,
    #[live(1.0_f32)]
    enabled: f32,
    #[live(0.0_f32)]
    preamp: f32,
    #[live(0.0_f32)]
    g0: f32,
    #[live(0.0_f32)]
    g1: f32,
    #[live(0.0_f32)]
    g2: f32,
    #[live(0.0_f32)]
    g3: f32,
    #[live(0.0_f32)]
    g4: f32,
    #[live(0.0_f32)]
    g5: f32,
    #[live(0.0_f32)]
    g6: f32,
    #[live(0.0_f32)]
    g7: f32,
    #[live(0.0_f32)]
    g8: f32,
    #[live(0.0_f32)]
    g9: f32,
}

#[derive(Live, LiveHook, Widget)]
pub struct FerricEqGraph {
    #[walk]
    walk: Walk,
    #[redraw]
    #[live]
    draw_bg: DrawFerricEqGraph,
}

impl Widget for FerricEqGraph {
    fn handle_event(&mut self, _cx: &mut Cx, _event: &Event, _scope: &mut Scope) {}

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let _ = self.draw_bg.draw_walk(cx, walk);
        DrawStep::done()
    }
}

impl FerricEqGraphRef {
    pub fn set_bands(&self, cx: &mut Cx, enabled: bool, preamp_db: f32, gains: &[f32]) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.draw_bg.enabled = if enabled { 1.0 } else { 0.0 };
            inner.draw_bg.preamp = preamp_db;
            let g = |i: usize| gains.get(i).copied().unwrap_or(0.0);
            inner.draw_bg.g0 = g(0);
            inner.draw_bg.g1 = g(1);
            inner.draw_bg.g2 = g(2);
            inner.draw_bg.g3 = g(3);
            inner.draw_bg.g4 = g(4);
            inner.draw_bg.g5 = g(5);
            inner.draw_bg.g6 = g(6);
            inner.draw_bg.g7 = g(7);
            inner.draw_bg.g8 = g(8);
            inner.draw_bg.g9 = g(9);
            inner.draw_bg.redraw(cx);
        }
    }
}

#[derive(Live, LiveHook, Widget)]
pub struct FerricQrView {
    #[walk]
    walk: Walk,
    #[redraw]
    #[live]
    draw_bg: DrawColor,
    #[live]
    draw_mod: DrawColor,
    #[rust]
    qr_size: usize,
    #[rust]
    modules: Vec<bool>,
}

impl Widget for FerricQrView {
    fn handle_event(&mut self, _cx: &mut Cx, _event: &Event, _scope: &mut Scope) {}

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let rect = cx.walk_turtle(walk);
        self.draw_bg.draw_abs(cx, rect);
        if self.qr_size > 0 && self.modules.len() >= self.qr_size * self.qr_size {
            let quiet = 3usize;
            let total = (self.qr_size + quiet * 2) as f64;
            let side = rect.size.x.min(rect.size.y);
            let cell = (side / total).max(1.0);
            let ox = rect.pos.x + (rect.size.x - cell * total) * 0.5;
            let oy = rect.pos.y + (rect.size.y - cell * total) * 0.5;
            for y in 0..self.qr_size {
                for x in 0..self.qr_size {
                    if self.modules[y * self.qr_size + x] {
                        self.draw_mod.draw_abs(
                            cx,
                            Rect {
                                pos: dvec2(
                                    ox + (x + quiet) as f64 * cell,
                                    oy + (y + quiet) as f64 * cell,
                                ),
                                size: dvec2(cell + 0.35, cell + 0.35),
                            },
                        );
                    }
                }
            }
        }
        DrawStep::done()
    }
}

impl FerricQrViewRef {
    pub fn set_matrix(&self, cx: &mut Cx, size: usize, modules: Vec<bool>) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.qr_size = size;
            inner.modules = modules;
            inner.draw_bg.redraw(cx);
        }
    }
}

#[derive(Clone, Debug, DefaultNone)]
pub enum FerricCoverArtAction {
    Clicked,
    None,
}

#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
pub struct DrawFerricCoverArt {
    #[deref]
    draw_super: DrawQuad,
    #[live(0.0_f32)]
    has_image: f32,
    #[live(1.0_f32)]
    img_aspect: f32,
    #[live(12.0_f32)]
    radius: f32,
    #[live(0.0_f32)]
    circle: f32,
    #[live(0.0_f32)]
    playing: f32,
    #[live(0.5_f32)]
    level: f32,
    #[live(0.0_f32)]
    hover: f32,
    #[live]
    accent: Vec4,
}

#[derive(Live, LiveHook, Widget)]
pub struct FerricCoverArt {
    #[walk]
    walk: Walk,
    #[redraw]
    #[live]
    draw_bg: DrawFerricCoverArt,
    #[rust]
    texture: Option<Texture>,
    #[rust]
    hover_target: f32,
    #[rust]
    anim_frame: NextFrame,
}

impl Widget for FerricCoverArt {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if self.anim_frame.is_event(event).is_some() {
            self.draw_bg.hover += (self.hover_target - self.draw_bg.hover) * 0.25;
            if (self.hover_target - self.draw_bg.hover).abs() < 0.01 {
                self.draw_bg.hover = self.hover_target;
            } else {
                self.anim_frame = cx.new_next_frame();
            }
            self.draw_bg.redraw(cx);
        }
        let uid = self.widget_uid();
        match event.hits(cx, self.draw_bg.area()) {
            Hit::FingerHoverIn(_) => {
                cx.set_cursor(MouseCursor::Hand);
                self.hover_target = 1.0;
                self.anim_frame = cx.new_next_frame();
            }
            Hit::FingerHoverOut(_) => {
                self.hover_target = 0.0;
                self.anim_frame = cx.new_next_frame();
            }
            Hit::FingerUp(fe) if fe.is_over && (fe.abs - fe.abs_start).length() < 16.0 => {
                cx.widget_action(uid, &scope.path, FerricCoverArtAction::Clicked);
            }
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        if let Some(tex) = &self.texture {
            self.draw_bg.draw_vars.set_texture(0, tex);
            self.draw_bg.has_image = 1.0;
        } else {
            self.draw_bg.draw_vars.empty_texture(0);
            self.draw_bg.has_image = 0.0;
        }
        let _ = self.draw_bg.draw_walk(cx, walk);
        DrawStep::done()
    }
}

impl FerricCoverArtRef {
    pub fn clicked(&self, actions: &Actions) -> bool {
        matches!(
            actions.find_widget_action(self.widget_uid()).cast(),
            FerricCoverArtAction::Clicked
        )
    }

    pub fn set_texture(&self, cx: &mut Cx, texture: Option<Texture>, aspect: f32) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.texture = texture;
            inner.draw_bg.img_aspect = aspect.clamp(0.25, 4.0);
            inner.draw_bg.redraw(cx);
        }
    }

    pub fn set_state(&self, cx: &mut Cx, playing: bool, level: f32, accent: Vec4, circle: bool) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.draw_bg.playing = if playing { 1.0 } else { 0.0 };
            inner.draw_bg.level = level;
            inner.draw_bg.accent = accent;
            inner.draw_bg.circle = if circle { 1.0 } else { 0.0 };
            inner.draw_bg.redraw(cx);
        }
    }
}

#[derive(Clone, Debug, DefaultNone)]
pub enum FerricLyricsAction {
    SeekMs(u64),
    None,
}

#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
pub struct DrawLyricsPill {
    #[deref]
    draw_super: DrawQuad,
    #[live]
    accent: Vec4,
    #[live(0.0_f32)]
    alpha: f32,
}

/// 60 FPS GPU spring-animated synced lyrics stage with critically-damped vertical glide,
/// per-line smooth SDF scale/color focus bloom, feathered top/bottom viewport fade,
/// active accent beacon pill, mouse-wheel scrubbing, and click-any-line-to-seek.
#[derive(Live, LiveHook, Widget)]
pub struct FerricLyricsView {
    #[walk]
    walk: Walk,
    #[live(false)]
    compact: bool,
    #[redraw]
    #[live]
    draw_pill: DrawLyricsPill,
    #[redraw]
    #[live]
    draw_line: DrawText,
    #[redraw]
    #[live]
    draw_active: DrawText,
    #[rust]
    area: Area,
    #[rust]
    lines: Vec<(u64, String)>,
    #[rust]
    empty_msg: String,
    #[rust]
    active_idx: usize,
    #[rust]
    focus_vals: Vec<f32>,
    #[rust]
    scroll_y: f64,
    #[rust]
    scroll_vel: f64,
    #[rust]
    target_scroll_y: f64,
    #[rust]
    manual_scroll_until: f64,
    #[rust]
    accent: Vec4,
    #[rust]
    ui_zoom: f32,
    #[rust]
    anim_frame: NextFrame,
    #[rust]
    last_time: f64,
    #[rust]
    hovered_idx: Option<usize>,
    #[rust]
    line_rects: Vec<(usize, Rect)>,
    #[rust]
    last_viewport_h: f64,
}

impl FerricLyricsView {
    fn base_step(&self) -> f64 {
        let z = if self.ui_zoom > 0.4 {
            self.ui_zoom as f64
        } else {
            1.0
        };
        if self.compact { 34.0 * z } else { 48.0 * z }
    }

    fn recompute_target_scroll(&mut self) {
        if self.lines.is_empty() {
            self.target_scroll_y = 0.0;
            return;
        }
        let step = self.base_step();
        let vh = self
            .last_viewport_h
            .max(if self.compact { 220.0 } else { 360.0 });
        let anchor_y = if self.compact { vh * 0.34 } else { vh * 0.36 };
        let desired = (self.active_idx as f64 * step - anchor_y).max(0.0);
        let max_scroll = ((self.lines.len().saturating_sub(1)) as f64 * step - vh * 0.35).max(0.0);
        self.target_scroll_y = desired.min(max_scroll);
    }

    fn wake_anim(&mut self, cx: &mut Cx) {
        self.anim_frame = cx.new_next_frame();
        self.area.redraw(cx);
    }
}

impl Widget for FerricLyricsView {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if let Some(nf) = self.anim_frame.is_event(event) {
            let dt = if self.last_time > 0.0 {
                (nf.time - self.last_time).clamp(0.004, 0.045)
            } else {
                0.016
            };
            self.last_time = nf.time;

            // Critically damped spring for silky Apple-Music vertical glide (zero overshoot)
            let stiffness = 175.0_f64;
            let damping = 26.0_f64;
            let force =
                (self.target_scroll_y - self.scroll_y) * stiffness - self.scroll_vel * damping;
            self.scroll_vel += force * dt;
            self.scroll_y += self.scroll_vel * dt;

            let focus_k = (1.0 - (-12.5 * dt).exp()) as f32;
            let mut max_focus_err = 0.0_f32;
            for (i, fv) in self.focus_vals.iter_mut().enumerate() {
                let target = if i == self.active_idx { 1.0 } else { 0.0 };
                let diff = target - *fv;
                *fv += diff * focus_k;
                if diff.abs() < 0.004 {
                    *fv = target;
                } else {
                    max_focus_err = max_focus_err.max(diff.abs());
                }
            }

            let scroll_err = (self.target_scroll_y - self.scroll_y).abs();
            if scroll_err < 0.35 && self.scroll_vel.abs() < 1.2 && max_focus_err < 0.008 {
                self.scroll_y = self.target_scroll_y;
                self.scroll_vel = 0.0;
            } else {
                self.anim_frame = cx.new_next_frame();
            }
            self.area.redraw(cx);
        }

        let uid = self.widget_uid();
        match event.hits(cx, self.area) {
            Hit::FingerScroll(fs) => {
                let step = self.base_step();
                let max_s = (self.lines.len().saturating_sub(1) as f64 * step).max(0.0);
                self.target_scroll_y = (self.target_scroll_y + fs.scroll.y).clamp(0.0, max_s);
                self.manual_scroll_until = fs.time + 3.0;
                self.wake_anim(cx);
            }
            Hit::FingerHoverIn(fh) | Hit::FingerHoverOver(fh) => {
                let hit_i = self
                    .line_rects
                    .iter()
                    .find(|(_, r)| r.contains(fh.abs))
                    .map(|(i, _)| *i);
                if hit_i.is_some() {
                    cx.set_cursor(MouseCursor::Hand);
                } else {
                    cx.set_cursor(MouseCursor::Default);
                }
                if hit_i != self.hovered_idx {
                    self.hovered_idx = hit_i;
                    self.area.redraw(cx);
                }
            }
            Hit::FingerHoverOut(_) => {
                if self.hovered_idx.is_some() {
                    self.hovered_idx = None;
                    self.area.redraw(cx);
                }
            }
            Hit::FingerUp(fe) if fe.is_over && (fe.abs - fe.abs_start).length() < 16.0 => {
                if let Some((idx, _)) = self.line_rects.iter().find(|(_, r)| r.contains(fe.abs))
                    && let Some((ts_ms, _)) = self.lines.get(*idx)
                {
                    self.active_idx = *idx;
                    self.manual_scroll_until = 0.0;
                    self.recompute_target_scroll();
                    self.wake_anim(cx);
                    cx.widget_action(uid, &scope.path, FerricLyricsAction::SeekMs(*ts_ms));
                }
            }
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        cx.begin_turtle(
            walk,
            Layout {
                clip_x: true,
                clip_y: true,
                ..Default::default()
            },
        );
        let rect = cx.turtle().inner_rect();
        self.last_viewport_h = rect.size.y.max(120.0);
        self.line_rects.clear();

        let z = if self.ui_zoom > 0.4 {
            self.ui_zoom
        } else {
            1.0
        };
        let accent = if self.accent.w > 0.5 {
            self.accent
        } else {
            vec4(0.659, 0.510, 1.0, 1.0)
        };

        if self.lines.is_empty() {
            let msg = if self.empty_msg.is_empty() {
                "Play a song to view synced lyrics"
            } else {
                self.empty_msg.as_str()
            };
            let fs = if self.compact { 10.0 * z } else { 13.0 * z };
            self.draw_line.text_style.font_size = fs;
            self.draw_line.font_scale = 1.0;
            self.draw_line.color = vec4(0.56, 0.58, 0.66, 1.0);
            let max_w = (rect.size.x - 16.0).max(80.0) as f32;
            let laid =
                self.draw_line
                    .layout(cx, 0.0, 0.0, Some(max_w), true, Align::default(), msg);
            let lh = laid.size_in_lpxs.height as f64;
            let y = rect.pos.y + (rect.size.y - lh) * 0.42;
            let _ = self.draw_line.draw_walk_laidout(
                cx,
                Walk {
                    abs_pos: Some(dvec2(rect.pos.x + 8.0, y)),
                    width: Size::Fixed(max_w as f64),
                    height: Size::Fixed(lh),
                    margin: Margin::default(),
                },
                &laid,
            );
            cx.end_turtle_with_area(&mut self.area);
            return DrawStep::done();
        }

        let step = self.base_step();
        let top_pad = if self.compact { 10.0 } else { 16.0 };
        let base_fs = if self.compact { 9.8 * z } else { 13.2 * z };
        let max_w = (rect.size.x - 28.0).max(80.0) as f32;

        // First measure actual cumulative y offsets with smooth spring scroll
        let mut cur_y = rect.pos.y + top_pad - self.scroll_y;
        for (i, (_, text)) in self.lines.iter().enumerate() {
            let focus = self.focus_vals.get(i).copied().unwrap_or(0.0);
            let is_hov = self.hovered_idx == Some(i);
            let line_h_est = step + (focus as f64) * (if self.compact { 8.0 } else { 14.0 });

            // Cull lines far outside viewport for O(visible) GPU cost
            if cur_y + line_h_est < rect.pos.y - 60.0 {
                cur_y += line_h_est;
                continue;
            }
            if cur_y > rect.pos.y + rect.size.y + 60.0 {
                break;
            }

            // Smooth feathered edge fade at top & bottom 14% of viewport
            let rel_y = (cur_y + line_h_est * 0.5 - rect.pos.y) / rect.size.y.max(1.0);
            let edge_fade = if rel_y < 0.14 {
                (rel_y / 0.14).clamp(0.0, 1.0) as f32
            } else if rel_y > 0.86 {
                ((1.0 - rel_y) / 0.14).clamp(0.0, 1.0) as f32
            } else {
                1.0
            };

            let dist = i.abs_diff(self.active_idx) as f32;
            let base_alpha = (0.72 - dist * 0.11).clamp(0.28, 0.72);
            let alpha = (base_alpha + (1.0 - base_alpha) * focus + if is_hov { 0.16 } else { 0.0 })
                .clamp(0.0, 1.0)
                * edge_fade;

            let scale = 1.0 + focus * if self.compact { 0.15 } else { 0.24 };
            let x_shift = (focus as f64) * if self.compact { 6.0 } else { 10.0 };
            let pad_x = if self.compact { 10.0 } else { 14.0 };

            let display_str = if text.trim().is_empty() {
                "···"
            } else {
                text.as_str()
            };
            let draw = if focus > 0.45 {
                &mut self.draw_active
            } else {
                &mut self.draw_line
            };
            draw.text_style.font_size = base_fs;
            draw.font_scale = scale;
            let text_rgb = if focus > 0.02 {
                let mix_r = 0.72 + (1.0 - 0.72) * focus;
                let mix_g = 0.74 + (1.0 - 0.74) * focus;
                let mix_b = 0.80 + (1.0 - 0.80) * focus;
                // Blend clean bright white with subtle adaptive accent warmth on active line
                vec3(
                    mix_r * (1.0 - 0.28 * focus) + accent.x * (0.28 * focus),
                    mix_g * (1.0 - 0.28 * focus) + accent.y * (0.28 * focus),
                    mix_b * (1.0 - 0.28 * focus) + accent.z * (0.28 * focus),
                )
            } else if is_hov {
                vec3(0.92, 0.93, 0.96)
            } else {
                vec3(0.68, 0.70, 0.78)
            };
            draw.color = vec4(text_rgb.x, text_rgb.y, text_rgb.z, alpha);

            let laid = draw.layout(
                cx,
                0.0,
                0.0,
                Some((max_w - x_shift as f32).max(60.0)),
                true,
                Align::default(),
                display_str,
            );
            let actual_h = (laid.size_in_lpxs.height * scale) as f64;
            let pill_pad_y = if self.compact { 5.0 } else { 7.0 };

            // Luminous active-line highlight pill + accent beacon bar
            if focus > 0.03 {
                self.draw_pill.accent = accent;
                self.draw_pill.alpha = focus * edge_fade;
                self.draw_pill.draw_abs(
                    cx,
                    Rect {
                        pos: dvec2(rect.pos.x + 2.0, cur_y - pill_pad_y),
                        size: dvec2((rect.size.x - 4.0).max(20.0), actual_h + pill_pad_y * 2.0),
                    },
                );
            }

            let draw_rect = draw.draw_walk_laidout(
                cx,
                Walk {
                    abs_pos: Some(dvec2(rect.pos.x + pad_x + x_shift, cur_y)),
                    width: Size::Fixed((max_w - x_shift as f32).max(60.0) as f64),
                    height: Size::Fixed(actual_h),
                    margin: Margin::default(),
                },
                &laid,
            );

            self.line_rects.push((
                i,
                Rect {
                    pos: dvec2(rect.pos.x, cur_y - pill_pad_y),
                    size: dvec2(
                        rect.size.x,
                        (actual_h + pill_pad_y * 2.0).max(draw_rect.size.y),
                    ),
                },
            ));

            let gap = if self.compact {
                12.0 * z as f64
            } else {
                18.0 * z as f64
            };
            cur_y += actual_h + gap;
        }

        cx.end_turtle_with_area(&mut self.area);
        DrawStep::done()
    }
}

impl FerricLyricsViewRef {
    pub fn seek_ms(&self, actions: &Actions) -> Option<u64> {
        if let FerricLyricsAction::SeekMs(ms) = actions.find_widget_action(self.widget_uid()).cast()
        {
            return Some(ms);
        }
        None
    }

    pub fn set_empty_message(&self, cx: &mut Cx, msg: &str) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.lines.clear();
            inner.focus_vals.clear();
            inner.empty_msg = msg.to_string();
            inner.active_idx = 0;
            inner.scroll_y = 0.0;
            inner.scroll_vel = 0.0;
            inner.target_scroll_y = 0.0;
            inner.area.redraw(cx);
        }
    }

    pub fn set_lyrics(&self, cx: &mut Cx, items: Vec<(u64, String)>) {
        if let Some(mut inner) = self.borrow_mut() {
            let n = items.len();
            inner.lines = items;
            inner.focus_vals = vec![0.0; n];
            if n > 0 {
                inner.focus_vals[0] = 1.0;
            }
            inner.active_idx = 0;
            inner.scroll_y = 0.0;
            inner.scroll_vel = 0.0;
            inner.target_scroll_y = 0.0;
            inner.manual_scroll_until = 0.0;
            inner.recompute_target_scroll();
            inner.wake_anim(cx);
        }
    }

    pub fn set_active_index(&self, cx: &mut Cx, idx: usize) {
        if let Some(mut inner) = self.borrow_mut() {
            if inner.lines.is_empty() {
                return;
            }
            let clamped = idx.min(inner.lines.len() - 1);
            if clamped != inner.active_idx {
                inner.active_idx = clamped;
                if inner.last_time >= inner.manual_scroll_until {
                    inner.recompute_target_scroll();
                }
                inner.wake_anim(cx);
            }
        }
    }

    pub fn set_style(&self, cx: &mut Cx, accent: Vec4, ui_zoom: f32) {
        if let Some(mut inner) = self.borrow_mut() {
            let changed = (inner.accent.x - accent.x).abs() > 0.005
                || (inner.accent.y - accent.y).abs() > 0.005
                || (inner.accent.z - accent.z).abs() > 0.005
                || (inner.ui_zoom - ui_zoom).abs() > 0.005;
            inner.accent = accent;
            inner.ui_zoom = ui_zoom.clamp(0.65, 1.40);
            if changed {
                inner.recompute_target_scroll();
                inner.area.redraw(cx);
            }
        }
    }
}
