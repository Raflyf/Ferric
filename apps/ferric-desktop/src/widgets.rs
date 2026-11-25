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
    pub DrawLyricsText = {{DrawLyricsText}} {}
    pub DrawAmbientStage = {{DrawAmbientStage}} {}

    pub FerricAmbientStage = {{FerricAmbientStage}} {
        width: Fill,
        height: Fill,
        draw_bg: {
            texture image: texture2d
            accent: #A882FF
            has_image: 0.0
            phase: 0.0
            mouse_x: 0.5
            mouse_y: 0.5
            hover_amt: 0.0
            acrylic_on: 0.0

            fn sample_blur(self, uv: vec2) -> vec3 {
                let c0 = sample2d(self.image, clamp(uv, vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz;
                let r1 = 0.075;
                let r2 = 0.155;
                let mut acc = c0 * 0.16;
                acc += sample2d(self.image, clamp(uv + vec2(r1, 0.0), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.09;
                acc += sample2d(self.image, clamp(uv - vec2(r1, 0.0), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.09;
                acc += sample2d(self.image, clamp(uv + vec2(0.0, r1), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.09;
                acc += sample2d(self.image, clamp(uv - vec2(0.0, r1), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.09;
                acc += sample2d(self.image, clamp(uv + vec2(r1 * 0.72, r1 * 0.72), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.07;
                acc += sample2d(self.image, clamp(uv + vec2(-r1 * 0.72, r1 * 0.72), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.07;
                acc += sample2d(self.image, clamp(uv + vec2(r1 * 0.72, -r1 * 0.72), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.07;
                acc += sample2d(self.image, clamp(uv + vec2(-r1 * 0.72, -r1 * 0.72), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.07;
                acc += sample2d(self.image, clamp(uv + vec2(r2, 0.05), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.05;
                acc += sample2d(self.image, clamp(uv - vec2(r2, 0.05), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.05;
                acc += sample2d(self.image, clamp(uv + vec2(0.05, r2), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.05;
                acc += sample2d(self.image, clamp(uv - vec2(0.05, r2), vec2(0.01, 0.01), vec2(0.99, 0.99))).xyz * 0.05;
                return acc;
            }

            fn pixel(self) -> vec4 {
                let uv = self.pos;
                let drift = vec2(sin(self.phase * 0.28) * 0.042, cos(self.phase * 0.21) * 0.036);
                let mut col = mix(vec3(0.024, 0.025, 0.032), vec3(0.045, 0.047, 0.062), self.acrylic_on);

                if self.has_image > 0.5 {
                    let uv1 = (uv - 0.5) * 0.76 + 0.5 + drift;
                    let uv2 = (vec2(1.0 - uv.x, uv.y) - 0.5) * 0.88 + 0.5 - drift * 0.85;
                    let b1 = self.sample_blur(uv1);
                    let b2 = self.sample_blur(uv2);
                    let blended = mix(b1, b2, 0.34);
                    // Rich cinematic dark ambient wash with guaranteed contrast for white lyrics
                    let luma = dot(blended, vec3(0.299, 0.587, 0.114));
                    let sat = mix(vec3(luma, luma, luma), blended, 1.42);
                    col = mix(vec3(0.028, 0.026, 0.030), clamp(sat, 0.0, 1.0) * 0.42, 0.84);
                } else {
                    let d1 = length((uv - (vec2(0.26, 0.44) + drift)) * vec2(0.95, 1.15));
                    let d2 = length((uv - (vec2(0.76, 0.62) - drift)) * vec2(1.05, 1.2));
                    let g1 = exp(-d1 * d1 * 2.4) * 0.26;
                    let g2 = exp(-d2 * d2 * 2.8) * 0.16;
                    col += self.accent.xyz * g1 + vec3(0.28, 0.22, 0.38) * g2;
                }

                // Subtle interactive mouse-tracked ambient spotlight across the stage
                let dm = length((uv - vec2(self.mouse_x, self.mouse_y)) * vec2(1.35, 1.0));
                let spot = exp(-dm * dm * 6.5) * 0.095 * self.hover_amt;
                let spot_col = mix(vec3(1.0, 0.96, 0.92), self.accent.xyz, 0.42);
                col += spot_col * spot;

                // Soft atmospheric edge vignette so cover art and lyrics float in clean depth
                let dv = length((uv - vec2(0.50, 0.48)) * vec2(1.08, 1.22));
                let vig = clamp(1.0 - dv * dv * 0.52, 0.42, 1.0);
                col *= vig;
                return vec4(clamp(col, 0.0, 1.0), 1.0);
            }
        }
    }

    pub FerricLyricsView = {{FerricLyricsView}} {
        width: Fill,
        height: Fill,
        compact: false,
        draw_pill: {
            accent: #A882FF
            alpha: 0.0
            hover_mode: 0.0
            mouse_x: 0.5
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                if self.hover_mode > 0.5 {
                    // Frosted interactive hover glass container with subtle pointer sheen
                    sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 13.0);
                    let dx = self.pos.x - self.mouse_x;
                    let sheen = exp(-dx * dx * 7.5) * 0.048;
                    let fill_a = (0.072 + sheen) * self.alpha;
                    sdf.fill_keep(vec4(1.0, 1.0, 1.0, fill_a));
                    sdf.stroke(vec4(1.0, 1.0, 1.0, 0.115 * self.alpha), 1.0);
                    return sdf.result;
                }
                sdf.box(0.0, 0.0, self.rect_size.x, self.rect_size.y, 8.0);
                let fill_col = vec4(self.accent.xyz, 0.13 * self.alpha);
                sdf.fill_keep(fill_col);
                sdf.stroke(vec4(self.accent.xyz, 0.24 * self.alpha), 1.0);
                sdf.box(4.0, 6.0, 3.0, max(self.rect_size.y - 12.0, 4.0), 1.5);
                sdf.fill(vec4(self.accent.xyz, 0.92 * self.alpha));
                return sdf.result;
            }
        }
        draw_line: {
            color: #FFFFFF,
            blur_amt: 0.0,
            karaoke_on: 0.0,
            sweep_x: 0.0,
            sweep_y_min: 0.0,
            sweep_y_max: 0.0,
            dim_col: #FFFFFF6E,
            glow_amt: 0.0,
            text_style: <THEME_FONT_DISPLAY> {
                font_size: 19.5,
                line_spacing: 1.22
            }
            fn pixel(self) -> vec4 {
                let dxt = length(dFdx(self.t));
                let dyt = length(dFdy(self.t));
                if self.texture_index == 0 {
                    let base_scale = max((dxt + dyt) * self.grayscale_atlas_size.x * 0.5, 0.001);
                    let blur = clamp(self.blur_amt, 0.0, 1.0);
                    let eff_scale = base_scale * (1.0 + blur * 5.2);
                    let mut s = self.sdf(eff_scale, self.t.xy);
                    if blur > 0.02 {
                        let r_uv = (self.t_max - self.t_min) * (0.048 * blur);
                        let s1 = self.sdf(eff_scale, clamp(self.t.xy + vec2(r_uv.x, 0.0), self.t_min, self.t_max));
                        let s2 = self.sdf(eff_scale, clamp(self.t.xy - vec2(r_uv.x, 0.0), self.t_min, self.t_max));
                        let s3 = self.sdf(eff_scale, clamp(self.t.xy + vec2(0.0, r_uv.y), self.t_min, self.t_max));
                        let s4 = self.sdf(eff_scale, clamp(self.t.xy - vec2(0.0, r_uv.y), self.t_min, self.t_max));
                        s = mix(s, s * 0.36 + (s1 + s2 + s3 + s4) * 0.16, blur);
                    }
                    let mut c = self.get_color();
                    if self.karaoke_on > 0.5 {
                        let mut lit = 0.0;
                        if self.world.y < self.sweep_y_min {
                            lit = 1.0;
                        } else if self.world.y <= self.sweep_y_max {
                            let lo = self.sweep_x - 24.0;
                            let hi = self.sweep_x + 14.0;
                            lit = 1.0 - smoothstep(lo, hi, self.world.x);
                        }
                        c = mix(self.dim_col, c, lit);
                        if self.glow_amt > 0.01 && lit > 0.05 {
                            let halo = clamp(((sample2d(self.grayscale_texture, self.t.xy).x - (1.0 - self.cutoff)) * self.radius / (base_scale * 4.2) + 0.5), 0.0, 1.0);
                            s = max(s, halo * 0.24 * lit * self.glow_amt);
                        }
                    }
                    return s * vec4(c.rgb * c.a, c.a);
                } else {
                    let c = sample2d(self.color_texture, self.t);
                    return vec4(c.rgb * c.a, c.a);
                }
            }
        }
        draw_active: {
            color: #FFFFFF,
            blur_amt: 0.0,
            karaoke_on: 1.0,
            sweep_x: 0.0,
            sweep_y_min: 0.0,
            sweep_y_max: 0.0,
            dim_col: #FFFFFF6E,
            glow_amt: 1.0,
            text_style: <THEME_FONT_DISPLAY> {
                font_size: 20.5,
                line_spacing: 1.20
            }
            fn pixel(self) -> vec4 {
                let dxt = length(dFdx(self.t));
                let dyt = length(dFdy(self.t));
                if self.texture_index == 0 {
                    let base_scale = max((dxt + dyt) * self.grayscale_atlas_size.x * 0.5, 0.001);
                    let mut s = self.sdf(base_scale, self.t.xy);
                    let mut c = self.get_color();
                    if self.karaoke_on > 0.5 {
                        let mut lit = 0.0;
                        if self.world.y < self.sweep_y_min {
                            lit = 1.0;
                        } else if self.world.y <= self.sweep_y_max {
                            let lo = self.sweep_x - 24.0;
                            let hi = self.sweep_x + 14.0;
                            lit = 1.0 - smoothstep(lo, hi, self.world.x);
                        }
                        c = mix(self.dim_col, c, lit);
                        if self.glow_amt > 0.01 && lit > 0.05 {
                            let halo = clamp(((sample2d(self.grayscale_texture, self.t.xy).x - (1.0 - self.cutoff)) * self.radius / (base_scale * 4.0) + 0.5), 0.0, 1.0);
                            s = max(s, halo * 0.26 * lit * self.glow_amt);
                        }
                    }
                    return s * vec4(c.rgb * c.a, c.a);
                } else {
                    let c = sample2d(self.color_texture, self.t);
                    return vec4(c.rgb * c.a, c.a);
                }
            }
        }
        draw_ts: {
            color: #FFFFFF88,
            text_style: <THEME_FONT_MEDIUM> {
                font_size: 8.5,
                line_spacing: 1.0
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
                    // Smooth interactive zoom + cursor-tracked micro-parallax on hero cover hover
                    if (self.hero > 0.5 && self.hover > 0.005) {
                        let zoom_s = mix(1.0, 0.962, self.hover);
                        let par = (vec2(self.hover_x, self.hover_y) - 0.5) * (0.022 * self.hover);
                        uv = (uv - 0.5) * zoom_s + 0.5 - par;
                    }
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
                    if (self.hero > 0.5) {
                        let dg = length(self.pos - vec2(self.hover_x, self.hover_y));
                        let glare = exp(-dg * dg * 4.5) * 0.14 * self.hover;
                        rgb = mix(rgb, vec3(1.0, 1.0, 1.0), glare);
                    } else {
                        rgb = mix(rgb, vec3(1.0, 1.0, 1.0), 0.06 * self.hover);
                    }
                }

                if (self.playing > 0.5) {
                    rgb = mix(rgb, vec3(0.04, 0.04, 0.06), 0.44);
                }

                sdf.fill_keep(vec4(clamp(rgb, 0.0, 1.0), 1.0));
                let rim_col = if (self.hero > 0.5) {
                    mix(vec4(1.0, 1.0, 1.0, 0.12), vec4(1.0, 1.0, 1.0, 0.30), self.hover)
                } else {
                    mix(self.border_col, vec4(self.accent.xyz, 0.75), self.playing)
                };
                sdf.stroke(rim_col, 1.0);

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
                let energy = (self.s0 + self.s1 + self.s2 + self.s3) * 0.25;

                // Floating translucent 24-bar centered rounded capsule spectrum (zero boxy frame)
                let pad_x = 12.0;
                let span_x = max(self.rect_size.x - pad_x * 2.0, 24.0);
                let cy = self.rect_size.y * 0.5;
                let max_half = self.rect_size.y * 0.42;
                let bars = 24.0;
                let cell_w = span_x / bars;
                let bar_w = max(cell_w * 0.40, 2.5);

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
                    let bv = clamp(raw_bv * 1.15 + pulse, 0.10, 1.0);
                    let half_h = max(max_half * bv, 2.2);
                    let dx = abs(cell_local - cell_w * 0.5) - bar_w * 0.5;
                    let dy = abs(p.y - cy) - half_h;
                    let r_cap = bar_w * 0.5;
                    let qx = max(dx + r_cap, 0.0);
                    let qy = max(dy + r_cap, 0.0);
                    let dist = length(vec2(qx, qy)) - r_cap;
                    let alpha = 1.0 - smoothstep(-0.7, 0.7, dist);
                    if (alpha > 0.01) {
                        let t_v = 1.0 - clamp(abs(p.y - cy) / max(half_h, 1.0), 0.0, 1.0);
                        let bar_col = mix(vec3(1.0, 1.0, 1.0), self.accent.xyz, 0.25 * (1.0 - t_v));
                        let out_a = alpha * (0.38 + 0.34 * bv);
                        return vec4(bar_col * out_a, out_a);
                    }
                }

                return vec4(0.0, 0.0, 0.0, 0.0);
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
    #[live(0.5_f32)]
    hover_x: f32,
    #[live(0.5_f32)]
    hover_y: f32,
    #[live(0.0_f32)]
    hero: f32,
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
    hover_tx: f32,
    #[rust]
    hover_ty: f32,
    #[rust]
    anim_frame: NextFrame,
}

impl Widget for FerricCoverArt {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if self.anim_frame.is_event(event).is_some() {
            self.draw_bg.hover += (self.hover_target - self.draw_bg.hover) * 0.22;
            self.draw_bg.hover_x += (self.hover_tx - self.draw_bg.hover_x) * 0.22;
            self.draw_bg.hover_y += (self.hover_ty - self.draw_bg.hover_y) * 0.22;
            let done = (self.hover_target - self.draw_bg.hover).abs() < 0.008
                && (self.hover_tx - self.draw_bg.hover_x).abs() < 0.008
                && (self.hover_ty - self.draw_bg.hover_y).abs() < 0.008;
            if done {
                self.draw_bg.hover = self.hover_target;
                self.draw_bg.hover_x = self.hover_tx;
                self.draw_bg.hover_y = self.hover_ty;
            } else {
                self.anim_frame = cx.new_next_frame();
            }
            self.draw_bg.redraw(cx);
        }
        let uid = self.widget_uid();
        match event.hits(cx, self.draw_bg.area()) {
            Hit::FingerHoverIn(fh) | Hit::FingerHoverOver(fh) => {
                cx.set_cursor(MouseCursor::Hand);
                let rect = self.draw_bg.area().rect(cx);
                if rect.size.x > 1.0 && rect.size.y > 1.0 {
                    self.hover_tx = ((fh.abs.x - rect.pos.x) / rect.size.x).clamp(0.0, 1.0) as f32;
                    self.hover_ty = ((fh.abs.y - rect.pos.y) / rect.size.y).clamp(0.0, 1.0) as f32;
                }
                self.hover_target = 1.0;
                self.anim_frame = cx.new_next_frame();
            }
            Hit::FingerHoverOut(_) => {
                self.hover_target = 0.0;
                self.hover_tx = 0.5;
                self.hover_ty = 0.5;
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

#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
pub struct DrawAmbientStage {
    #[deref]
    draw_super: DrawQuad,
    #[live]
    accent: Vec4,
    #[live(0.0_f32)]
    has_image: f32,
    #[live(0.0_f32)]
    phase: f32,
    #[live(0.5_f32)]
    mouse_x: f32,
    #[live(0.5_f32)]
    mouse_y: f32,
    #[live(0.0_f32)]
    hover_amt: f32,
    #[live(0.0_f32)]
    acrylic_on: f32,
}

/// Full-bleed blurred album artwork ambient stage with smooth slow drift
/// and subtle cursor-following light response for immersive fullscreen lyrics.
#[derive(Live, LiveHook, Widget)]
pub struct FerricAmbientStage {
    #[walk]
    walk: Walk,
    #[redraw]
    #[live]
    draw_bg: DrawAmbientStage,
    #[rust]
    texture: Option<Texture>,
    #[rust]
    target_mx: f32,
    #[rust]
    target_my: f32,
    #[rust]
    target_hover: f32,
    #[rust]
    playing: bool,
    #[rust]
    active: bool,
    #[rust]
    last_time: f64,
    #[rust]
    anim_frame: NextFrame,
}

impl Widget for FerricAmbientStage {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, _scope: &mut Scope) {
        if let Event::MouseMove(mm) = event
            && self.active
        {
            let rect = self.draw_bg.area().rect(cx);
            if rect.size.x > 10.0 && rect.size.y > 10.0 && rect.contains(mm.abs) {
                self.target_mx = ((mm.abs.x - rect.pos.x) / rect.size.x).clamp(0.0, 1.0) as f32;
                self.target_my = ((mm.abs.y - rect.pos.y) / rect.size.y).clamp(0.0, 1.0) as f32;
                self.target_hover = 1.0;
                self.anim_frame = cx.new_next_frame();
            } else if self.target_hover > 0.01 {
                self.target_hover = 0.0;
                self.anim_frame = cx.new_next_frame();
            }
        }
        if let Some(nf) = self.anim_frame.is_event(event) {
            if !self.active {
                return;
            }
            let dt = if self.last_time > 0.0 {
                (nf.time - self.last_time).clamp(0.004, 0.045) as f32
            } else {
                0.016
            };
            self.last_time = nf.time;
            let k = 1.0 - (-8.0 * dt).exp();
            self.draw_bg.mouse_x += (self.target_mx - self.draw_bg.mouse_x) * k;
            self.draw_bg.mouse_y += (self.target_my - self.draw_bg.mouse_y) * k;
            self.draw_bg.hover_amt += (self.target_hover - self.draw_bg.hover_amt) * k;
            if self.playing {
                self.draw_bg.phase = (self.draw_bg.phase + dt * 0.65) % 628.3;
            }
            let moving = (self.target_mx - self.draw_bg.mouse_x).abs() > 0.003
                || (self.target_my - self.draw_bg.mouse_y).abs() > 0.003
                || (self.target_hover - self.draw_bg.hover_amt).abs() > 0.005;
            if self.playing || moving {
                self.anim_frame = cx.new_next_frame();
            }
            self.draw_bg.redraw(cx);
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

impl FerricAmbientStageRef {
    pub fn set_texture(&self, cx: &mut Cx, texture: Option<Texture>) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.texture = texture;
            inner.draw_bg.redraw(cx);
        }
    }

    pub fn set_state(&self, cx: &mut Cx, active: bool, playing: bool, accent: Vec4, acrylic: f32) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.active = active;
            inner.playing = playing;
            inner.draw_bg.accent = accent;
            inner.draw_bg.acrylic_on = acrylic;
            if active && playing {
                inner.anim_frame = cx.new_next_frame();
            }
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
    #[live(0.0_f32)]
    hover_mode: f32,
    #[live(0.5_f32)]
    mouse_x: f32,
}

#[derive(Live, LiveHook, LiveRegister)]
#[repr(C)]
pub struct DrawLyricsText {
    #[deref]
    draw_super: DrawText,
    #[live(0.0_f32)]
    blur_amt: f32,
    #[live(0.0_f32)]
    karaoke_on: f32,
    #[live(0.0_f32)]
    sweep_x: f32,
    #[live(0.0_f32)]
    sweep_y_min: f32,
    #[live(0.0_f32)]
    sweep_y_max: f32,
    #[live]
    dim_col: Vec4,
    #[live(0.0_f32)]
    glow_amt: f32,
}

/// 60 FPS GPU interactive synced lyrics stage:
/// - Staggered bottom-to-top critically-damped wave cascade on line change
/// - True SDF optical depth-of-field blur on inactive lines, smoothly un-blurring on hover
/// - Frosted cursor-tracked glass hover pill + timestamp hint + click-any-line-to-seek
/// - Continuous 60 FPS sub-pixel word-by-word karaoke highlight sweep across wrapped rows
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
    draw_line: DrawLyricsText,
    #[redraw]
    #[live]
    draw_active: DrawLyricsText,
    #[redraw]
    #[live]
    draw_ts: DrawText,
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
    hover_vals: Vec<f32>,
    #[rust]
    cascade_y: Vec<f64>,
    #[rust]
    cascade_vel: Vec<f64>,
    #[rust]
    scroll_y: f64,
    #[rust]
    scroll_vel: f64,
    #[rust]
    target_scroll_y: f64,
    #[rust]
    manual_scroll_until: f64,
    #[rust]
    playback_pos_ms: u64,
    #[rust]
    playback_sync_time: f64,
    #[rust]
    is_playing: bool,
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
    pressed_idx: Option<usize>,
    #[rust]
    mouse_abs: DVec2,
    #[rust]
    line_rects: Vec<(usize, Rect)>,
    #[rust]
    line_heights: Vec<f64>,
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
        if self.compact { 34.0 * z } else { 64.0 * z }
    }

    fn line_gap(&self) -> f64 {
        let z = if self.ui_zoom > 0.4 {
            self.ui_zoom as f64
        } else {
            1.0
        };
        if self.compact { 12.0 * z } else { 24.0 * z }
    }

    fn cumulative_y_of(&self, target_idx: usize) -> f64 {
        let step = self.base_step();
        let gap = self.line_gap();
        let mut y = 0.0_f64;
        for i in 0..target_idx.min(self.lines.len()) {
            let h = self.line_heights.get(i).copied().unwrap_or(0.0);
            y += if h > 1.0 { h + gap } else { step };
        }
        y
    }

    fn total_content_h(&self) -> f64 {
        self.cumulative_y_of(self.lines.len())
    }

    fn recompute_target_scroll(&mut self) {
        if self.lines.is_empty() {
            self.target_scroll_y = 0.0;
            return;
        }
        let vh = self
            .last_viewport_h
            .max(if self.compact { 220.0 } else { 420.0 });
        let anchor_y = if self.compact { vh * 0.34 } else { vh * 0.39 };
        let active_y = self.cumulative_y_of(self.active_idx);
        let desired = (active_y - anchor_y).max(0.0);
        let max_scroll = (self.total_content_h() - vh * 0.42).max(0.0);
        self.target_scroll_y = desired.min(max_scroll);
    }

    fn trigger_bottom_up_cascade(&mut self, old_idx: usize, new_idx: usize) {
        if self.compact || self.lines.is_empty() || old_idx == new_idx {
            return;
        }
        let dir = if new_idx > old_idx { 1.0_f64 } else { -1.0_f64 };
        let jump = (new_idx.abs_diff(old_idx) as f64).min(3.0);
        let amp = (14.0 + jump * 5.0) * dir;
        for i in 0..self.cascade_y.len() {
            let rel = i as isize - new_idx as isize;
            if (-4..=6).contains(&rel) {
                // Staggered bottom-to-top liquid wave: lower lines lag slightly and glide up into place
                let lag = ((rel + 4) as f64 / 10.0).clamp(0.0, 1.0);
                self.cascade_y[i] =
                    (self.cascade_y[i] + amp * (0.35 + lag * 0.95)).clamp(-46.0, 46.0);
            }
        }
    }

    fn wake_anim(&mut self, cx: &mut Cx) {
        self.anim_frame = cx.new_next_frame();
        self.area.redraw(cx);
    }

    fn estimated_pos_ms(&self) -> f64 {
        if self.is_playing
            && self.playback_sync_time > 0.0
            && self.last_time >= self.playback_sync_time
        {
            let extra_ms = ((self.last_time - self.playback_sync_time) * 1000.0).clamp(0.0, 420.0);
            self.playback_pos_ms as f64 + extra_ms
        } else {
            self.playback_pos_ms as f64
        }
    }
}

fn format_lyric_ts(ms: u64) -> String {
    let total_s = ms / 1000;
    format!("{}:{:02}", total_s / 60, total_s % 60)
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

            // Auto-resume synced scroll smoothly after manual mouse-wheel exploration expires
            if self.manual_scroll_until > 0.0 && nf.time >= self.manual_scroll_until {
                self.manual_scroll_until = 0.0;
                self.recompute_target_scroll();
            }

            // Silky critically-damped vertical spring (smooth bottom-to-top glide, zero harsh bounce)
            let stiffness = if self.compact { 170.0_f64 } else { 128.0_f64 };
            let damping = if self.compact { 25.0_f64 } else { 22.5_f64 };
            let force =
                (self.target_scroll_y - self.scroll_y) * stiffness - self.scroll_vel * damping;
            self.scroll_vel += force * dt;
            self.scroll_y += self.scroll_vel * dt;

            // Per-line staggered wave cascade spring
            let mut max_cascade = 0.0_f64;
            for i in 0..self.cascade_y.len() {
                let dist = i.abs_diff(self.active_idx) as f64;
                let k_line = (145.0 - dist * 11.0).max(78.0);
                let d_line = (23.0 - dist * 1.1).max(16.0);
                let f = -self.cascade_y[i] * k_line - self.cascade_vel[i] * d_line;
                self.cascade_vel[i] += f * dt;
                self.cascade_y[i] += self.cascade_vel[i] * dt;
                if self.cascade_y[i].abs() < 0.08 && self.cascade_vel[i].abs() < 0.4 {
                    self.cascade_y[i] = 0.0;
                    self.cascade_vel[i] = 0.0;
                } else {
                    max_cascade = max_cascade.max(self.cascade_y[i].abs());
                }
            }

            let focus_k = (1.0 - (-10.5 * dt).exp()) as f32;
            let hover_k = (1.0 - (-15.0 * dt).exp()) as f32;
            let mut max_err = 0.0_f32;
            for (i, fv) in self.focus_vals.iter_mut().enumerate() {
                let target = if i == self.active_idx { 1.0 } else { 0.0 };
                let diff = target - *fv;
                *fv += diff * focus_k;
                if diff.abs() < 0.004 {
                    *fv = target;
                } else {
                    max_err = max_err.max(diff.abs());
                }
            }
            for (i, hv) in self.hover_vals.iter_mut().enumerate() {
                let target = if self.hovered_idx == Some(i) {
                    1.0
                } else {
                    0.0
                };
                let diff = target - *hv;
                *hv += diff * hover_k;
                if diff.abs() < 0.004 {
                    *hv = target;
                } else {
                    max_err = max_err.max(diff.abs());
                }
            }

            let scroll_err = (self.target_scroll_y - self.scroll_y).abs();
            let waiting_manual = self.manual_scroll_until > nf.time;
            let keep_alive = self.is_playing
                || waiting_manual
                || scroll_err >= 0.30
                || self.scroll_vel.abs() >= 1.0
                || max_cascade >= 0.08
                || max_err >= 0.006;
            if !keep_alive {
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
                let max_s = (self.total_content_h() - self.last_viewport_h * 0.35).max(0.0);
                self.target_scroll_y =
                    (self.target_scroll_y + fs.scroll.y * 1.15).clamp(0.0, max_s);
                self.manual_scroll_until = fs.time + 3.6;
                self.wake_anim(cx);
            }
            Hit::FingerHoverIn(fh) | Hit::FingerHoverOver(fh) => {
                self.mouse_abs = fh.abs;
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
                }
                self.wake_anim(cx);
            }
            Hit::FingerHoverOut(_) => {
                self.pressed_idx = None;
                if self.hovered_idx.is_some() {
                    self.hovered_idx = None;
                    self.wake_anim(cx);
                }
            }
            Hit::FingerDown(fe) => {
                self.pressed_idx = self
                    .line_rects
                    .iter()
                    .find(|(_, r)| r.contains(fe.abs))
                    .map(|(i, _)| *i);
                if self.pressed_idx.is_some() {
                    self.area.redraw(cx);
                }
            }
            Hit::FingerUp(fe) if fe.is_over && (fe.abs - fe.abs_start).length() < 16.0 => {
                self.pressed_idx = None;
                let clicked = self
                    .line_rects
                    .iter()
                    .find(|(_, r)| r.contains(fe.abs))
                    .and_then(|(idx, _)| self.lines.get(*idx).map(|(ts_ms, _)| (*idx, *ts_ms)));
                if let Some((idx, ts_ms)) = clicked {
                    let old_idx = self.active_idx;
                    self.active_idx = idx;
                    self.playback_pos_ms = ts_ms;
                    self.playback_sync_time = self.last_time;
                    self.manual_scroll_until = 0.0;
                    self.trigger_bottom_up_cascade(old_idx, idx);
                    self.recompute_target_scroll();
                    self.wake_anim(cx);
                    cx.widget_action(uid, &scope.path, FerricLyricsAction::SeekMs(ts_ms));
                }
            }
            Hit::FingerUp(_) if self.pressed_idx.is_some() => {
                self.pressed_idx = None;
                self.area.redraw(cx);
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
        let rect = cx.turtle().rect();
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
            let fs = if self.compact { 10.0 * z } else { 15.0 * z };
            self.draw_line.text_style.font_size = fs;
            self.draw_line.font_scale = 1.0;
            self.draw_line.blur_amt = 0.0;
            self.draw_line.karaoke_on = 0.0;
            self.draw_line.glow_amt = 0.0;
            self.draw_line.color = vec4(0.72, 0.74, 0.80, 0.65);
            let max_w = (rect.size.x - 24.0).max(80.0) as f32;
            let laid =
                self.draw_line
                    .layout(cx, 0.0, 0.0, Some(max_w), true, Align::default(), msg);
            let lh = laid.size_in_lpxs.height as f64;
            let y = rect.pos.y + (rect.size.y - lh) * 0.44;
            let _ = self.draw_line.draw_walk_laidout(
                cx,
                Walk {
                    abs_pos: Some(dvec2(rect.pos.x + 12.0, y)),
                    width: Size::Fixed(max_w as f64),
                    height: Size::Fixed(lh),
                    margin: Margin::default(),
                },
                &laid,
            );
            cx.end_turtle_with_area(&mut self.area);
            return DrawStep::done();
        }

        let top_pad = if self.compact { 12.0 } else { 44.0 };
        let base_fs = if self.compact { 9.8 * z } else { 19.2 * z };
        let base_step = self.base_step();
        let gap = self.line_gap();
        let pad_x = if self.compact { 10.0 } else { 20.0 };
        let max_w =
            (rect.size.x - pad_x * 2.0 - if self.compact { 8.0 } else { 36.0 }).max(80.0) as f32;
        let est_pos_ms = self.estimated_pos_ms();
        let line_count = self.lines.len();
        if self.line_heights.len() != line_count {
            self.line_heights = vec![0.0; line_count];
        }

        let mut cur_y = rect.pos.y + top_pad - self.scroll_y;
        let mut heights_updated = false;

        for i in 0..line_count {
            let (ts_ms, ref text) = self.lines[i];
            let focus = self.focus_vals.get(i).copied().unwrap_or(0.0);
            let hover_v = self.hover_vals.get(i).copied().unwrap_or(0.0);
            let wave_y = self.cascade_y.get(i).copied().unwrap_or(0.0);
            let is_pressed = self.pressed_idx == Some(i);

            let cached_h = self.line_heights[i];
            let est_h = if cached_h > 1.0 {
                cached_h
            } else {
                base_step - gap
            };
            let draw_y = cur_y + wave_y;

            // Cull lines far outside viewport while preserving exact cumulative height
            if draw_y + est_h < rect.pos.y - 120.0 {
                cur_y += est_h + gap;
                continue;
            }
            if draw_y > rect.pos.y + rect.size.y + 120.0 {
                break;
            }

            // Smooth wide feathered edge fade (top 18% & bottom 18% melt seamlessly into backdrop)
            let rel_y = (draw_y + est_h * 0.5 - rect.pos.y) / rect.size.y.max(1.0);
            let fade_zone = if self.compact { 0.14 } else { 0.19 };
            let edge_fade = if rel_y < fade_zone {
                let t = (rel_y / fade_zone).clamp(0.0, 1.0) as f32;
                t * t * (3.0 - 2.0 * t)
            } else if rel_y > 1.0 - fade_zone {
                let t = ((1.0 - rel_y) / fade_zone).clamp(0.0, 1.0) as f32;
                t * t * (3.0 - 2.0 * t)
            } else {
                1.0
            };

            let dist = i.abs_diff(self.active_idx) as f32;
            let press_scale = if is_pressed { 0.985_f32 } else { 1.0_f32 };
            let scale = (1.0
                + focus * if self.compact { 0.14 } else { 0.065 }
                + hover_v * (1.0 - focus) * if self.compact { 0.02 } else { 0.018 })
                * press_scale;

            // Depth-of-field optical blur on inactive lines, snapping to razor sharp on mouse hover!
            let base_blur = if self.compact {
                0.0
            } else {
                ((dist - 0.15).max(0.0) * 0.34).clamp(0.0, 0.96)
            };
            let blur_amt = base_blur * (1.0 - focus) * (1.0 - hover_v);

            // Clean luminance hierarchy: active 100%, hover 86%, nearby 34%, distant 18%
            let base_alpha = if self.compact {
                (0.72 - dist * 0.10).clamp(0.28, 0.72)
            } else {
                (0.42 - dist * 0.065).clamp(0.16, 0.42)
            };
            let target_alpha = base_alpha
                + (1.0 - base_alpha) * focus
                + (0.86 - base_alpha).max(0.0) * hover_v * (1.0 - focus);
            let alpha = target_alpha.clamp(0.0, 1.0) * edge_fade;

            let x_shift = if self.compact {
                (focus as f64) * 6.0 + (hover_v as f64) * 3.0
            } else {
                (hover_v as f64) * 6.0 * (1.0 - focus as f64)
            };
            let draw_x = rect.pos.x + pad_x + x_shift;

            let display_str = if text.trim().is_empty() {
                "···"
            } else {
                text.as_str()
            };

            // Measure layout first to get exact wrapped height & row geometry for karaoke sweep
            let use_active_draw = focus > 0.35;
            let laid = if use_active_draw {
                self.draw_active.text_style.font_size = base_fs;
                self.draw_active.font_scale = scale;
                self.draw_active.layout(
                    cx,
                    0.0,
                    0.0,
                    Some((max_w - x_shift as f32).max(60.0)),
                    true,
                    Align::default(),
                    display_str,
                )
            } else {
                self.draw_line.text_style.font_size = base_fs;
                self.draw_line.font_scale = scale;
                self.draw_line.layout(
                    cx,
                    0.0,
                    0.0,
                    Some((max_w - x_shift as f32).max(60.0)),
                    true,
                    Align::default(),
                    display_str,
                )
            };

            let actual_h = (laid.size_in_lpxs.height * scale) as f64;
            if (self.line_heights[i] - actual_h).abs() > 1.0 {
                self.line_heights[i] = actual_h;
                heights_updated = true;
            }

            let pill_pad_y = if self.compact { 5.0 } else { 12.0 };
            let pill_rect = Rect {
                pos: dvec2(rect.pos.x + 2.0, draw_y - pill_pad_y),
                size: dvec2((rect.size.x - 10.0).max(24.0), actual_h + pill_pad_y * 2.0),
            };

            // 1. Frosted interactive glass hover pill (fullscreen) OR subtle active accent pill (compact)
            if !self.compact && hover_v > 0.015 {
                let mx_norm = ((self.mouse_abs.x - pill_rect.pos.x) / pill_rect.size.x.max(1.0))
                    .clamp(0.0, 1.0) as f32;
                self.draw_pill.accent = accent;
                self.draw_pill.hover_mode = 1.0;
                self.draw_pill.mouse_x = mx_norm;
                self.draw_pill.alpha = hover_v * edge_fade;
                self.draw_pill.draw_abs(cx, pill_rect);
            } else if self.compact && (focus > 0.03 || hover_v > 0.03) {
                self.draw_pill.accent = accent;
                self.draw_pill.hover_mode = if focus > 0.25 { 0.0 } else { 1.0 };
                self.draw_pill.mouse_x = 0.5;
                self.draw_pill.alpha = focus.max(hover_v * 0.7) * edge_fade;
                self.draw_pill.draw_abs(cx, pill_rect);
            }

            // 2. Compute smooth sub-pixel word-by-word karaoke sweep across wrapped rows on active line
            let (karaoke_on, sweep_x, sweep_y_min, sweep_y_max) =
                if focus > 0.45 && !laid.rows.is_empty() {
                    let next_ts = self
                        .lines
                        .get(i + 1)
                        .map(|(t, _)| *t)
                        .unwrap_or(ts_ms + 3_500);
                    let span_ms = next_ts.saturating_sub(ts_ms).clamp(900, 12_000) as f64;
                    let raw_t = ((est_pos_ms - ts_ms as f64) / (span_ms * 0.90)).clamp(0.0, 1.0);
                    // Smooth natural phrasing easing
                    let prog = (0.04 + raw_t * 0.96).clamp(0.0, 1.0) as f32;
                    let total_w: f32 = laid
                        .rows
                        .iter()
                        .map(|r| r.width_in_lpxs.max(1.0))
                        .sum::<f32>()
                        .max(1.0);
                    let mut target_w = prog * total_w;
                    let mut chosen = laid.rows.last().unwrap();
                    for r in &laid.rows {
                        let rw = r.width_in_lpxs.max(1.0);
                        if target_w <= rw {
                            chosen = r;
                            break;
                        }
                        target_w -= rw;
                    }
                    let local_x = target_w.clamp(0.0, chosen.width_in_lpxs.max(1.0));
                    let sx = draw_x as f32 + (chosen.origin_in_lpxs.x + local_x) * scale;
                    let sy_min =
                        draw_y as f32 + (chosen.origin_in_lpxs.y - chosen.ascender_in_lpxs) * scale;
                    let sy_max = draw_y as f32
                        + (chosen.origin_in_lpxs.y - chosen.descender_in_lpxs) * scale
                        + 2.0;
                    (1.0_f32, sx, sy_min, sy_max)
                } else {
                    (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32)
                };

            let bright_col = vec4(1.0, 1.0, 1.0, alpha);
            let dim_active_col = vec4(0.92, 0.93, 0.96, (0.44 * edge_fade).clamp(0.0, 1.0));

            let draw_rect = if use_active_draw {
                self.draw_active.blur_amt = 0.0;
                self.draw_active.karaoke_on = karaoke_on;
                self.draw_active.sweep_x = sweep_x;
                self.draw_active.sweep_y_min = sweep_y_min;
                self.draw_active.sweep_y_max = sweep_y_max;
                self.draw_active.dim_col = dim_active_col;
                self.draw_active.glow_amt = focus * edge_fade;
                self.draw_active.color = bright_col;
                self.draw_active.draw_walk_laidout(
                    cx,
                    Walk {
                        abs_pos: Some(dvec2(draw_x, draw_y)),
                        width: Size::Fixed((max_w - x_shift as f32).max(60.0) as f64),
                        height: Size::Fixed(actual_h),
                        margin: Margin::default(),
                    },
                    &laid,
                )
            } else {
                self.draw_line.blur_amt = blur_amt;
                self.draw_line.karaoke_on = 0.0;
                self.draw_line.glow_amt = 0.0;
                self.draw_line.color = if hover_v > 0.05 {
                    vec4(0.96, 0.97, 1.0, alpha)
                } else {
                    vec4(0.88, 0.90, 0.94, alpha)
                };
                self.draw_line.draw_walk_laidout(
                    cx,
                    Walk {
                        abs_pos: Some(dvec2(draw_x, draw_y)),
                        width: Size::Fixed((max_w - x_shift as f32).max(60.0) as f64),
                        height: Size::Fixed(actual_h),
                        margin: Margin::default(),
                    },
                    &laid,
                )
            };

            // 3. Subtle timestamp badge on hover in fullscreen mode so users can seek precisely
            if !self.compact && hover_v > 0.05 {
                let ts_txt = format_lyric_ts(ts_ms);
                self.draw_ts.text_style.font_size = 8.4 * z;
                self.draw_ts.color = vec4(1.0, 1.0, 1.0, 0.48 * hover_v * edge_fade);
                self.draw_ts.draw_abs(
                    cx,
                    dvec2(
                        (pill_rect.pos.x + pill_rect.size.x - 42.0).max(draw_x + 20.0),
                        draw_y + (actual_h * 0.5) - 6.0,
                    ),
                    &ts_txt,
                );
            }

            self.line_rects.push((
                i,
                Rect {
                    pos: pill_rect.pos,
                    size: dvec2(
                        pill_rect.size.x,
                        pill_rect.size.y.max(draw_rect.size.y + pill_pad_y),
                    ),
                },
            ));

            cur_y += actual_h + gap;
        }

        if heights_updated && self.manual_scroll_until == 0.0 {
            self.recompute_target_scroll();
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
            inner.hover_vals.clear();
            inner.cascade_y.clear();
            inner.cascade_vel.clear();
            inner.line_heights.clear();
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
            inner.hover_vals = vec![0.0; n];
            inner.cascade_y = vec![0.0; n];
            inner.cascade_vel = vec![0.0; n];
            inner.line_heights = vec![0.0; n];
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
                let old_idx = inner.active_idx;
                inner.active_idx = clamped;
                inner.trigger_bottom_up_cascade(old_idx, clamped);
                if inner.last_time >= inner.manual_scroll_until {
                    inner.recompute_target_scroll();
                }
                inner.wake_anim(cx);
            }
        }
    }

    pub fn set_playback_state(&self, cx: &mut Cx, pos_ms: u64, is_playing: bool) {
        if let Some(mut inner) = self.borrow_mut() {
            let was_playing = inner.is_playing;
            inner.playback_pos_ms = pos_ms;
            inner.playback_sync_time = inner.last_time;
            inner.is_playing = is_playing;
            if is_playing && (!was_playing || !inner.lines.is_empty()) {
                inner.anim_frame = cx.new_next_frame();
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
                inner.line_heights.fill(0.0);
                inner.recompute_target_scroll();
                inner.area.redraw(cx);
            }
        }
    }
}
