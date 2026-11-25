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
        height: 220,
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

            fn bin_val(self, i: float) -> float {
                if i < 0.5 { return self.s0; }
                if i < 1.5 { return self.s1; }
                if i < 2.5 { return self.s2; }
                if i < 3.5 { return self.s3; }
                if i < 4.5 { return self.s4; }
                if i < 5.5 { return self.s5; }
                if i < 6.5 { return self.s6; }
                return self.s7;
            }

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 8.0);

                let uv = self.pos;
                let c = self.rect_size * 0.5;
                let energy = (self.s0 + self.s1 + self.s2 + self.s3) * 0.25;

                // Mode 0: Static Minimalist Studio Artwork Card
                if self.mode < 0.5 {
                    sdf.fill_keep(#080808);
                    sdf.stroke(#222222, 1.0);
                    let r_outer = min(self.rect_size.x, self.rect_size.y) * 0.34;
                    sdf.circle(c.x, c.y, r_outer);
                    sdf.fill_keep(#140A06);
                    sdf.stroke(self.accent, 2.2);
                    sdf.circle(c.x, c.y, r_outer * 0.32);
                    sdf.fill(self.accent);
                    return sdf.result;
                }

                // Mode 1: Animated Audio-Reactive Motion Canvas + 8-Band Spectrum (Spotify Canvas / Sonora style)
                let wave1 = sin(uv.x * 5.2 + self.phase * 1.4) * cos(uv.y * 3.8 - self.phase * 0.9);
                let wave2 = cos(uv.x * 3.4 - self.phase * 1.1 + uv.y * 4.2);
                let glow = clamp(0.5 + 0.5 * wave1, 0.0, 1.0) * (0.20 + 0.38 * energy);
                let deep = clamp(0.5 + 0.5 * wave2, 0.0, 1.0) * 0.14;
                let mut rgb = vec3(0.03, 0.025, 0.025)
                    + self.accent.xyz * glow
                    + vec3(0.95, 0.72, 0.52) * (deep * energy);

                // Spinning vinyl / magnetic reel rings in center
                let d = length(p - c);
                let ring_r = min(self.rect_size.x, self.rect_size.y) * (0.24 + 0.04 * energy);
                let ring_line = smoothstep(2.2, 0.4, abs(d - ring_r));
                let inner_line = smoothstep(1.8, 0.3, abs(d - ring_r * 0.42));
                rgb = mix(rgb, self.accent.xyz, ring_line * 0.85);
                rgb = mix(rgb, vec3(0.95, 0.93, 0.89), inner_line * 0.75);

                // Bottom 16-bar mirrored audio spectrum visualizer
                let margin_x = self.rect_size.x * 0.14;
                let span_x = max(self.rect_size.x - margin_x * 2.0, 20.0);
                if (p.x >= margin_x && p.x <= (self.rect_size.x - margin_x)) {
                    let norm_x = (p.x - margin_x) / span_x;
                    let slot = floor(norm_x * 16.0);
                    let cell_x = fract(norm_x * 16.0);
                    let sym_idx = abs(slot - 7.5);
                    let bv = clamp(self.bin_val(min(sym_idx, 7.0)), 0.06, 1.0);
                    let bar_h = self.rect_size.y * 0.34 * bv;
                    let base_y = self.rect_size.y - 18.0;
                    if (cell_x > 0.18 && cell_x < 0.82 && p.y <= base_y && p.y >= (base_y - bar_h)) {
                        let t_bar = (base_y - p.y) / max(bar_h, 1.0);
                        rgb = mix(self.accent.xyz, vec3(0.96, 0.93, 0.88), t_bar * 0.55);
                    }
                }

                sdf.fill_keep(vec4(clamp(rgb, 0.0, 1.0), 1.0));
                sdf.stroke(vec4(self.accent.xyz, 0.45), 1.0);
                return sdf.result;
            }
        }
    }

    pub LiquidGlassPreview = <View> {
        width: Fill,
        height: 410,
        show_bg: true,
        draw_bg: {
            uniform ior: 1.48
            uniform dispersion: 0.42
            uniform blur_amt: 0.35
            uniform rim_amt: 0.78
            uniform scrim_amt: 0.28
            uniform radius_px: 24.0
            uniform glass_on: 1.0

            // Procedural high-contrast editorial bands behind the floating glass lens
            // matching LG2_Desktop_Appearance_ON.png and the user's reference showcase.
            fn sample_backdrop(self, uv: vec2) -> vec3 {
                let u = clamp(uv, vec2(0.0, 0.0), vec2(1.0, 1.0));
                let mut bg = vec3(0.02, 0.02, 0.025);

                // Crisp horizontal hairline rules
                let rule1 = smoothstep(0.004, 0.0, abs(u.y - 0.29));
                let rule2 = smoothstep(0.004, 0.0, abs(u.y - 0.47));
                let rule3 = smoothstep(0.004, 0.0, abs(u.y - 0.65));
                let rule4 = smoothstep(0.004, 0.0, abs(u.y - 0.81));
                bg += vec3(0.28, 0.14, 0.08) * rule1;
                bg += vec3(0.10, 0.20, 0.42) * rule2;
                bg += vec3(0.36, 0.10, 0.20) * rule3;
                bg += vec3(0.16, 0.20, 0.28) * rule4;

                // Bold typographic color blocks ("FERRIC / LIQUID / GLASS / SOUND / RUST")
                let gx = fract(u.x * 6.2);
                let stem = smoothstep(0.12, 0.17, gx) * (1.0 - smoothstep(0.74, 0.80, gx));
                let crossbar = smoothstep(0.36, 0.42, fract(u.y * 16.0));
                let glyph = clamp(stem * (0.62 + 0.38 * crossbar), 0.0, 1.0);
                let margin_mask = smoothstep(0.08, 0.13, u.x) * (1.0 - smoothstep(0.87, 0.92, u.x));

                if u.y > 0.12 && u.y < 0.26 {
                    bg = mix(bg, vec3(0.85, 0.33, 0.12), glyph * margin_mask);
                } else if u.y > 0.31 && u.y < 0.45 {
                    bg = mix(bg, vec3(0.95, 0.93, 0.89), glyph * margin_mask);
                } else if u.y > 0.49 && u.y < 0.63 {
                    bg = mix(bg, vec3(0.22, 0.49, 0.98), glyph * margin_mask);
                } else if u.y > 0.66 && u.y < 0.79 {
                    bg = mix(bg, vec3(0.88, 0.24, 0.48), glyph * margin_mask);
                } else if u.y > 0.82 && u.y < 0.94 {
                    bg = mix(bg, vec3(0.42, 0.52, 0.70), glyph * margin_mask * 0.75);
                }
                return bg;
            }

            fn pixel(self) -> vec4 {
                let p = self.pos * self.rect_size;
                let sdf = Sdf2d::viewport(p);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 8.0);
                sdf.fill_keep(#050506);
                sdf.stroke(#1D1D1D, 1.0);

                let uv = self.pos;
                let mut col = self.sample_backdrop(uv);

                // Floating Mini-Player Glass Lens bounds in the center
                let card_c = self.rect_size * vec2(0.5, 0.52);
                let card_half = vec2(min(self.rect_size.x * 0.34, 132.0), min(self.rect_size.y * 0.34, 136.0));
                let r = clamp(self.radius_px, 8.0, 38.0);
                let rel = p - card_c;
                let q = abs(rel) - (card_half - vec2(r, r));
                let sd = length(max(q, vec2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;

                // Soft drop shadow beneath floating mini-player
                if sd > 0.0 && sd < 22.0 {
                    let sh = exp(-sd * 0.16) * 0.55;
                    col *= 1.0 - sh;
                }

                if sd <= 0.0 {
                    if self.glass_on < 0.5 {
                        // Pure AMOLED 3-Color Solid Card when Liquid Glass is OFF
                        col = vec3(0.05, 0.05, 0.05);
                        if sd > -1.4 {
                            col = vec3(0.20, 0.20, 0.20);
                        }
                    } else {
                        // Snell's Law curved bevel refraction + RGB dispersion + frost blur
                        let depth = -sd;
                        let bevel = 28.0;
                        let edge_t = clamp(1.0 - depth / bevel, 0.0, 1.0);
                        let n2 = normalize(rel / max(card_half, vec2(1.0, 1.0)) + vec2(0.0001, 0.0001));
                        let bend = pow(edge_t, 1.75) * clamp(self.ior - 1.0, 0.05, 0.95) * 0.18;
                        let spread = bend * self.dispersion * 0.55;
                        let frost = self.blur_amt * 0.014;

                        let uv_r = uv - n2 * (bend + spread);
                        let uv_g = uv - n2 * bend;
                        let uv_b = uv - n2 * (bend - spread);

                        let r_ch = (self.sample_backdrop(uv_r).x + self.sample_backdrop(uv_r + vec2(frost, 0.0)).x) * 0.5;
                        let g_ch = (self.sample_backdrop(uv_g).y + self.sample_backdrop(uv_g + vec2(0.0, frost)).y) * 0.5;
                        let b_ch = (self.sample_backdrop(uv_b).z + self.sample_backdrop(uv_b - vec2(frost, frost)).z) * 0.5;
                        let mut refr = vec3(r_ch, g_ch, b_ch);

                        // Prismatic edge caustic rainbow boost at high dispersion
                        let ang = atan(n2.y, n2.x);
                        let caustic = vec3(
                            0.5 + 0.5 * cos(ang),
                            0.5 + 0.5 * cos(ang - 2.094),
                            0.5 + 0.5 * cos(ang + 2.094)
                        ) * pow(edge_t, 2.4) * self.dispersion * 0.72;
                        refr += caustic;

                        // Interior WCAG readability scrim
                        refr = mix(refr, vec3(0.02, 0.02, 0.03), clamp(self.scrim_amt, 0.05, 0.85));

                        // 1.5px Specular Rim Catchlight
                        let rim_band = clamp(1.0 - depth / 2.2, 0.0, 1.0);
                        let tl = clamp(-(n2.x * 0.7 + n2.y * 0.7), 0.0, 1.0);
                        let br = clamp(n2.x * 0.7 + n2.y * 0.7, 0.0, 1.0);
                        refr += vec3(0.98, 0.96, 0.92) * rim_band * (0.30 + 0.75 * tl + 0.45 * br) * self.rim_amt;
                        col = clamp(refr, 0.0, 1.0);
                    }

                    // Mini-player inner UI elements (Sunset Album Art + Seekbar + Circular Pause Btn)
                    let local_p = p - (card_c - card_half);
                    let card_sz = card_half * 2.0;

                    // Top-left Sunset Art square inside mini-player
                    let art_q = abs(local_p - vec2(40.0, 40.0)) - vec2(20.0, 20.0);
                    let art_sd = length(max(art_q, vec2(0.0, 0.0))) + min(max(art_q.x, art_q.y), 0.0) - 8.0;
                    if art_sd <= 0.0 {
                        let auv = (local_p - vec2(12.0, 12.0)) / 56.0;
                        let mut acol = mix(vec3(0.92, 0.44, 0.14), vec3(0.28, 0.08, 0.07), clamp(auv.y, 0.0, 1.0));
                        if length(local_p - vec2(40.0, 43.0)) < 11.0 {
                            acol = vec3(0.98, 0.94, 0.86);
                        }
                        col = acol;
                    }

                    // Seekbar inside mini-player
                    let sy = card_sz.y - 62.0;
                    if (local_p.x >= 20.0 && local_p.x <= (card_sz.x - 20.0) && abs(local_p.y - sy) < 1.8) {
                        let prog_x = 20.0 + (card_sz.x - 40.0) * 0.42;
                        if (local_p.x <= prog_x) {
                            col = vec3(0.96, 0.94, 0.90);
                        } else {
                            col = vec3(0.35, 0.35, 0.38);
                        }
                    }
                    if length(local_p - vec2(20.0 + (card_sz.x - 40.0) * 0.42, sy)) < 5.0 {
                        col = vec3(0.98, 0.96, 0.92);
                    }

                    // Bottom center circular Pause button inside mini-player
                    let pb_c = vec2(card_sz.x * 0.5, card_sz.y - 28.0);
                    let pb_d = length(local_p - pb_c);
                    if pb_d < 16.5 {
                        col = vec3(0.96, 0.94, 0.90);
                        if abs(abs(local_p.x - pb_c.x) - 3.6) < 1.5 && abs(local_p.y - pb_c.y) < 5.5 {
                            col = vec3(0.04, 0.04, 0.05);
                        }
                    }
                }

                return vec4(col, 1.0);
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
}

impl Widget for FerricToggle {
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
            Hit::FingerUp(fe) if fe.is_over => {
                self.is_on = !self.is_on;
                self.draw_bg.active = if self.is_on { 1.0 } else { 0.0 };
                self.draw_bg.redraw(cx);
                cx.widget_action(uid, &scope.path, FerricToggleAction::Toggled(self.is_on));
            }
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        self.draw_bg.active = if self.is_on { 1.0 } else { 0.0 };
        let _ = self.draw_bg.draw_walk(cx, walk);
        DrawStep::done()
    }
}

impl FerricToggleRef {
    pub fn set_active(&self, cx: &mut Cx, active: bool) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.is_on = active;
            inner.draw_bg.active = if active { 1.0 } else { 0.0 };
            inner.draw_bg.redraw(cx);
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
}

impl Widget for FerricWinBtn {
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
            Hit::FingerUp(fe) if fe.is_over => {
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
}

impl Widget for FerricPlayBtn {
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
}

impl Widget for FerricCoverArt {
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
            Hit::FingerUp(fe) if fe.is_over && fe.was_tap() => {
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
