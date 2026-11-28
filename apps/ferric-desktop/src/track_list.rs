use crate::artwork::{ArtworkCache, CachedArtwork};
use crate::state::clean_clip_into;
use crate::widgets::FerricCoverArtWidgetRefExt;
use ferric_core::models::{Track, TrackSource};
use makepad_widgets::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

live_design! {
    use link::theme::*;
    use link::shaders::*;
    use link::widgets::*;
    use crate::widgets::FerricCoverArt;

    TrackRow = <View> {
        width: Fill,
        height: 46,
        padding: { left: 10, right: 12, top: 4, bottom: 4 }
        margin: { bottom: 2 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 10,
        show_bg: true,
        cursor: Hand,

        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            uniform base_col: #00000000
            uniform hover_col: #161822
            uniform active_col: #261810
            uniform accent_col: #E86A2C
            uniform acrylic_on: 0.0
            uniform light_mode: 0.0

            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 7.0);

                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.04, 0.05, 0.09), self.light_mode);
                let solid_hov = mix(self.hover_col, vec4(0.04, 0.05, 0.09, 0.08), self.light_mode);
                let solid_idle = mix(self.base_col, solid_hov, self.hover);
                let acrylic_idle = mix(
                    vec4(ink, 0.025 + 0.015 * self.light_mode),
                    vec4(ink, 0.09 + 0.02 * self.light_mode),
                    self.hover
                );
                let idle_or_hover = mix(solid_idle, acrylic_idle, self.acrylic_on);

                let active_solid = mix(
                    mix(self.active_col, vec4(self.accent_col.xyz, 0.16), self.light_mode),
                    vec4(self.accent_col.xyz, 0.24),
                    0.36
                );
                let active_acrylic = vec4(
                    mix(mix(vec3(0.14, 0.14, 0.20), vec3(0.05, 0.06, 0.10), self.light_mode), self.accent_col.xyz, 0.34),
                    0.26 + 0.06 * self.hover
                );
                let bg = mix(idle_or_hover, mix(active_solid, active_acrylic, self.acrylic_on), self.active);
                let pressed = mix(bg, vec4(ink, 0.16), self.down * 0.22);
                sdf.fill_keep(pressed);

                let idle_border = vec4(ink, (0.055 * self.acrylic_on) + (0.09 * self.hover));
                let act_border = vec4(self.accent_col.xyz, 0.60);
                sdf.stroke(mix(idle_border, act_border, self.active), 1.0);
                return sdf.result;
            }
        }
        animator: {
            hover = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.16 } }
                    apply: { draw_bg: { hover: 0.0 } }
                }
                on = {
                    from: { all: Forward { duration: 0.12 } }
                    apply: { draw_bg: { hover: 1.0 } }
                }
            }
            down = {
                default: off,
                off = {
                    from: { all: Forward { duration: 0.12 } }
                    apply: { draw_bg: { down: 0.0 } }
                }
                on = {
                    from: { all: Forward { duration: 0.06 } }
                    apply: { draw_bg: { down: 1.0 } }
                }
            }
        }

        idx_lbl = <Label> {
            width: 22,
            flow: Right,
            padding: 0,
            align: { x: 0.5, y: 0.5 }
            draw_text: {
                color: #707382,
                text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 }
            }
            text: "1"
        }

        art_tile = <FerricCoverArt> {
            width: 34,
            height: 34,
            draw_bg: { radius: 6.0 }
        }

        info_col = <View> {
            width: Fill,
            height: Fit,
            flow: Down,
            spacing: 2,
            title_lbl = <Label> {
                width: Fill,
                flow: Right,
                padding: 0,
                draw_text: {
                    color: #F4F4F7,
                    text_style: <THEME_FONT_BOLD> { font_size: 9.6, line_spacing: 1.0 }
                }
                text: "Song Title"
            }
            artist_lbl = <Label> {
                width: Fill,
                flow: Right,
                padding: 0,
                draw_text: {
                    color: #9A9CA8,
                    text_style: <THEME_FONT_REGULAR> { font_size: 8.4, line_spacing: 1.0 }
                }
                text: "Artist"
            }
        }

        album_lbl = <Label> {
            width: 148,
            flow: Right,
            padding: 0,
            draw_text: {
                color: #888B99,
                text_style: <THEME_FONT_REGULAR> { font_size: 8.5, line_spacing: 1.0 }
            }
            text: "Album"
        }

        queue_add_btn = <Button> {
            width: 28,
            height: 28,
            padding: 0,
            spacing: 0,
            align: { x: 0.5, y: 0.5 }
            text: ""
            label_walk: { width: 0, height: 0 }
            icon_walk: { width: 14, height: 14 }
            draw_icon: {
                curve: 1.15,
                linearize: 0.08,
                brightness: 1.05,
                svg_path: "M14 10H2V12H14V10ZM14 6H2V8H14V6ZM18 14V10H16V14H12V16H16V20H18V16H22V14H18ZM2 16H10V14H2V16Z"
                color: #6E7180
                color_hover: #F4F4F7
                color_down: #E86A2C
            }
            draw_bg: {
                instance hover: 0.0
                uniform light_mode: 0.0
                fn pixel(self) -> vec4 {
                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                    sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 5.0);
                    let hov_dark = vec4(0.18, 0.19, 0.25, 0.80 * self.hover);
                    let hov_light = vec4(0.05, 0.06, 0.10, 0.12 * self.hover);
                    sdf.fill(mix(hov_dark, hov_light, self.light_mode));
                    return sdf.result;
                }
            }
            animator: {
                hover = {
                    default: off,
                    off = { from: { all: Forward { duration: 0.15 } } apply: { draw_bg: { hover: 0.0 }, draw_icon: { hover: 0.0 } } }
                    on = { from: { all: Forward { duration: 0.12 } } apply: { draw_bg: { hover: 1.0 }, draw_icon: { hover: 1.0 } } }
                }
            }
        }

        like_btn = <Button> {
            width: 26,
            height: 26,
            padding: 0,
            spacing: 0,
            align: { x: 0.5, y: 0.5 }
            text: ""
            label_walk: { width: 0, height: 0 }
            icon_walk: { width: 14, height: 14 }
            draw_icon: {
                curve: 1.15,
                linearize: 0.08,
                brightness: 1.05,
                svg_path: "M12 21.35L10.55 20.03C5.4 15.36 2 12.28 2 8.5C2 5.42 4.42 3 7.5 3C9.24 3 10.91 3.81 12 5.09C13.09 3.81 14.76 3 16.5 3C19.58 3 22 5.42 22 8.5C22 12.28 18.6 15.36 13.45 20.04L12 21.35Z"
                color: #5C5F6E
                color_hover: #F4F4F7
                color_down: #E86A2C
            }
            draw_bg: {
                instance hover: 0.0
                uniform light_mode: 0.0
                fn pixel(self) -> vec4 {
                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                    sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 5.0);
                    let hov_dark = vec4(0.18, 0.19, 0.25, 0.80 * self.hover);
                    let hov_light = vec4(0.05, 0.06, 0.10, 0.12 * self.hover);
                    sdf.fill(mix(hov_dark, hov_light, self.light_mode));
                    return sdf.result;
                }
            }
            animator: {
                hover = {
                    default: off,
                    off = { from: { all: Forward { duration: 0.15 } } apply: { draw_bg: { hover: 0.0 }, draw_icon: { hover: 0.0 } } }
                    on = { from: { all: Forward { duration: 0.12 } } apply: { draw_bg: { hover: 1.0 }, draw_icon: { hover: 1.0 } } }
                }
            }
        }

        pl_add_btn = <Button> {
            width: 26,
            height: 26,
            padding: 0,
            spacing: 0,
            align: { x: 0.5, y: 0.5 }
            text: ""
            label_walk: { width: 0, height: 0 }
            icon_walk: { width: 14, height: 14 }
            draw_icon: {
                curve: 1.15,
                linearize: 0.08,
                brightness: 1.05,
                svg_path: "M19 13H13V19H11V13H5V11H11V5H13V11H19V13Z"
                color: #6E7180
                color_hover: #F4F4F7
                color_down: #E86A2C
            }
            draw_bg: {
                instance hover: 0.0
                uniform light_mode: 0.0
                fn pixel(self) -> vec4 {
                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                    sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 5.0);
                    let hov_dark = vec4(0.18, 0.19, 0.25, 0.80 * self.hover);
                    let hov_light = vec4(0.05, 0.06, 0.10, 0.12 * self.hover);
                    sdf.fill(mix(hov_dark, hov_light, self.light_mode));
                    return sdf.result;
                }
            }
            animator: {
                hover = {
                    default: off,
                    off = { from: { all: Forward { duration: 0.15 } } apply: { draw_bg: { hover: 0.0 }, draw_icon: { hover: 0.0 } } }
                    on = { from: { all: Forward { duration: 0.12 } } apply: { draw_bg: { hover: 1.0 }, draw_icon: { hover: 1.0 } } }
                }
            }
        }

        delete_btn = <Button> {
            width: 26,
            height: 26,
            padding: 0,
            spacing: 0,
            align: { x: 0.5, y: 0.5 }
            text: ""
            label_walk: { width: 0, height: 0 }
            icon_walk: { width: 14, height: 14 }
            draw_icon: {
                curve: 1.15,
                linearize: 0.08,
                brightness: 1.05,
                svg_path: "M6 19C6 20.1 6.9 21 8 21H16C17.1 21 18 20.1 18 19V7H6V19ZM19 4H15.5L14.5 3H9.5L8.5 4H5V6H19V4Z"
                color: #646778
                color_hover: #FF6B7A
                color_down: #FF4D5E
            }
            draw_bg: {
                instance hover: 0.0
                uniform light_mode: 0.0
                fn pixel(self) -> vec4 {
                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                    sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 5.0);
                    let d_dark = vec4(0.86, 0.22, 0.26, 0.24 * self.hover);
                    let d_light = vec4(0.75, 0.14, 0.18, 0.16 * self.hover);
                    sdf.fill(mix(d_dark, d_light, self.light_mode));
                    return sdf.result;
                }
            }
            animator: {
                hover = {
                    default: off,
                    off = { from: { all: Forward { duration: 0.15 } } apply: { draw_bg: { hover: 0.0 }, draw_icon: { hover: 0.0 } } }
                    on = { from: { all: Forward { duration: 0.11 } } apply: { draw_bg: { hover: 1.0 }, draw_icon: { hover: 1.0 } } }
                }
            }
        }

        dur_lbl = <Label> {
            width: 44,
            flow: Right,
            padding: 0,
            align: { x: 0.5, y: 0.5 }
            draw_text: {
                color: #888B99,
                text_style: <THEME_FONT_MEDIUM> { font_size: 8.5 }
            }
            text: "03:45"
        }
    }

    EmptyLibraryState = <View> {
        width: Fill,
        height: 164,
        padding: { left: 18, right: 18, top: 16, bottom: 16 }
        margin: { top: 6 }
        flow: Down,
        align: { x: 0.0, y: 0.5 }
        spacing: 8,
        show_bg: true,
        draw_bg: {
            uniform acrylic_on: 0.0
            uniform light_mode: 0.0
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 8.0);
                let ink = mix(vec3(1.0, 1.0, 1.0), vec3(0.05, 0.06, 0.10), self.light_mode);
                let solid_fill = mix(vec4(0.071, 0.078, 0.110, 1.0), vec4(0.91, 0.88, 0.83, 1.0), self.light_mode);
                let acr_fill = vec4(ink, 0.045 + 0.045 * self.light_mode);
                sdf.fill_keep(mix(solid_fill, acr_fill, self.acrylic_on));
                let solid_b = mix(vec4(0.137, 0.145, 0.188, 1.0), vec4(0.08, 0.09, 0.14, 0.20), self.light_mode);
                let acr_b = vec4(ink, 0.09 + 0.08 * self.light_mode);
                sdf.stroke(mix(solid_b, acr_b, self.acrylic_on), 1.0);
                return sdf.result;
            }
        }
        empty_title_lbl = <Label> {
            padding: 0,
            draw_text: { color: #F4F4F7, text_style: <THEME_FONT_DISPLAY> { font_size: 12.5 } }
            text: "No tracks here yet"
        }
        empty_sub_lbl = <Label> {
            width: Fill,
            padding: 0,
            draw_text: { color: #9A9CA8, text_style: <THEME_FONT_REGULAR> { font_size: 9.0 } }
            text: "Browse YouTube Music immediately in Guest Mode, add a local folder (FLAC, MP3, Opus, WAV), or sign in with Google."
        }
        <View> {
            width: Fill,
            height: Fit,
            margin: { top: 4 }
            flow: Right,
            spacing: 8,
            empty_discover_btn = <Button> {
                width: Fit, height: 28, padding: { left: 12, right: 13 }, spacing: 6,
                text: "Refresh Discover"
                icon_walk: { width: 12, height: Fit }
                draw_icon: {
                    svg_path: "M12 3V13.55C11.41 13.21 10.73 13 10 13C7.79 13 6 14.79 6 17C6 19.21 7.79 21 10 21C12.21 21 14 19.21 14 17V7H18V3H12Z"
                    color: #09090D
                    color_hover: #09090D
                    color_focus: #09090D
                    color_down: #09090D
                    color_disabled: #09090D
                    fn get_color(self) -> vec4 {
                        return #09090D;
                    }
                }
                draw_text: {
                    color: #09090D,
                    color_hover: #09090D,
                    color_focus: #09090D,
                    color_down: #09090D,
                    color_disabled: #09090D,
                    text_style: <THEME_FONT_BOLD> { font_size: 8.8 }
                    fn get_color(self) -> vec4 {
                        return #09090D;
                    }
                }
                draw_bg: {
                    instance hover: 0.0
                    instance down: 0.0
                    uniform light_mode: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        let inset = 0.5;
                        let r = (self.rect_size.y - inset * 2.0) * 0.25;
                        sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, r);
                        let dark_col = mix(mix(#F2F3F7, #FFFFFF, self.hover), #D6D9E4, self.down);
                        let light_col = mix(mix(#FFFFFF, #F5F1EA, self.hover), #E4DEC8, self.down);
                        sdf.fill_keep(mix(dark_col, light_col, self.light_mode));
                        sdf.stroke(vec4(0.08, 0.09, 0.14, 0.22 * self.light_mode), 1.0);
                        return sdf.result;
                    }
                }
                animator: {
                    hover = {
                        default: off,
                        off = { from: { all: Forward { duration: 0.15 } } apply: { draw_bg: { hover: 0.0, down: 0.0 } } }
                        on = { from: { all: Forward { duration: 0.11 } } apply: { draw_bg: { hover: 1.0, down: 0.0 } } }
                        down = { from: { all: Forward { duration: 0.05 } } apply: { draw_bg: { hover: 1.0, down: 1.0 } } }
                    }
                }
            }
            empty_add_folder_btn = <Button> {
                width: Fit, height: 28, padding: { left: 11, right: 12 }, spacing: 6,
                text: "Add Folder"
                icon_walk: { width: 12, height: Fit }
                draw_icon: {
                    svg_path: "M20 6H12L10 4H4C2.89 4 2 4.89 2 6V18C2 19.11 2.89 20 4 20H20C21.11 20 22 19.11 22 18V8C22 6.89 21.11 6 20 6ZM19 14H16V17H14V14H11V12H14V9H16V12H19V14Z"
                    color: #E86A2C
                    color_hover: #F4F4F7
                    color_focus: #F4F4F7
                    color_down: #F4F4F7
                }
                draw_text: {
                    color: #F4F4F7,
                    color_hover: #FFFFFF,
                    color_focus: #FFFFFF,
                    color_down: #FFFFFF,
                    text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 }
                }
                draw_bg: {
                    instance hover: 0.0
                    instance down: 0.0
                    uniform light_mode: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        let inset = 0.5;
                        sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 6.0);
                        let dark_bg = mix(mix(#181A22, #252836, self.hover), #2D3042, self.down);
                        let light_bg = mix(mix(#E7E0D1, #DDD5C4, self.hover), #CFC6B2, self.down);
                        let dark_b = mix(#2A2C38, #444861, self.hover);
                        let light_b = mix(vec4(0.08, 0.09, 0.14, 0.18), vec4(0.08, 0.09, 0.14, 0.30), self.hover);
                        sdf.fill_keep(mix(dark_bg, light_bg, self.light_mode));
                        sdf.stroke(mix(dark_b, light_b, self.light_mode), 1.0);
                        return sdf.result;
                    }
                }
                animator: {
                    hover = {
                        default: off,
                        off = { from: { all: Forward { duration: 0.15 } } apply: { draw_bg: { hover: 0.0, down: 0.0 }, draw_icon: { hover: 0.0 }, draw_text: { hover: 0.0 } } }
                        on = { from: { all: Forward { duration: 0.11 } } apply: { draw_bg: { hover: 1.0, down: 0.0 }, draw_icon: { hover: 1.0 }, draw_text: { hover: 1.0 } } }
                        down = { from: { all: Forward { duration: 0.05 } } apply: { draw_bg: { hover: 1.0, down: 1.0 }, draw_icon: { hover: 1.0 }, draw_text: { hover: 1.0 } } }
                    }
                }
            }
            empty_youtube_btn = <Button> {
                width: Fit, height: 28, padding: { left: 11, right: 12 }, spacing: 6,
                text: "Sign in with Google"
                icon_walk: { width: 12, height: Fit }
                draw_icon: {
                    svg_path: "M12 12C14.21 12 16 10.21 16 8C16 5.79 14.21 4 12 4C9.79 4 8 5.79 8 8C8 10.21 9.79 12 12 12ZM12 14C9.33 14 4 15.34 4 18V20H20V18C20 15.34 14.67 14 12 14Z"
                    color: #E86A2C
                    color_hover: #F4F4F7
                    color_focus: #F4F4F7
                    color_down: #F4F4F7
                }
                draw_text: {
                    color: #F4F4F7,
                    color_hover: #FFFFFF,
                    color_focus: #FFFFFF,
                    color_down: #FFFFFF,
                    text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 }
                }
                draw_bg: {
                    instance hover: 0.0
                    instance down: 0.0
                    uniform light_mode: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        let inset = 0.5;
                        sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 6.0);
                        let dark_bg = mix(mix(#181A22, #252836, self.hover), #2D3042, self.down);
                        let light_bg = mix(mix(#E7E0D1, #DDD5C4, self.hover), #CFC6B2, self.down);
                        let dark_b = mix(#2A2C38, #444861, self.hover);
                        let light_b = mix(vec4(0.08, 0.09, 0.14, 0.18), vec4(0.08, 0.09, 0.14, 0.30), self.hover);
                        sdf.fill_keep(mix(dark_bg, light_bg, self.light_mode));
                        sdf.stroke(mix(dark_b, light_b, self.light_mode), 1.0);
                        return sdf.result;
                    }
                }
                animator: {
                    hover = {
                        default: off,
                        off = { from: { all: Forward { duration: 0.15 } } apply: { draw_bg: { hover: 0.0, down: 0.0 }, draw_icon: { hover: 0.0 }, draw_text: { hover: 0.0 } } }
                        on = { from: { all: Forward { duration: 0.11 } } apply: { draw_bg: { hover: 1.0, down: 0.0 }, draw_icon: { hover: 1.0 }, draw_text: { hover: 1.0 } } }
                        down = { from: { all: Forward { duration: 0.05 } } apply: { draw_bg: { hover: 1.0, down: 1.0 }, draw_icon: { hover: 1.0 }, draw_text: { hover: 1.0 } } }
                    }
                }
            }
            empty_demo_btn = <Button> {
                width: Fit, height: 28, padding: { left: 11, right: 12 }, spacing: 6,
                text: "Offline Demo"
                icon_walk: { width: 11, height: Fit }
                draw_icon: {
                    svg_path: "M8 5V19L19 12L8 5Z"
                    color: #E86A2C
                    color_hover: #F4F4F7
                    color_focus: #F4F4F7
                    color_down: #F4F4F7
                }
                draw_text: {
                    color: #F5A473,
                    color_hover: #FFFFFF,
                    color_focus: #FFFFFF,
                    color_down: #FFFFFF,
                    text_style: <THEME_FONT_MEDIUM> { font_size: 8.8 }
                }
                draw_bg: {
                    instance hover: 0.0
                    instance down: 0.0
                    uniform light_mode: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        let inset = 0.5;
                        sdf.box(inset, inset, self.rect_size.x - inset * 2.0, self.rect_size.y - inset * 2.0, 6.0);
                        let dark_bg = mix(mix(#241710, #382216, self.hover), #472A1B, self.down);
                        let light_bg = mix(mix(#F3DFD3, #ECCFBE, self.hover), #E4BEA9, self.down);
                        let dark_b = mix(#6E3920, #B85A2C, self.hover);
                        let light_b = mix(#C67B52, #B35526, self.hover);
                        sdf.fill_keep(mix(dark_bg, light_bg, self.light_mode));
                        sdf.stroke(mix(dark_b, light_b, self.light_mode), 1.0);
                        return sdf.result;
                    }
                }
                animator: {
                    hover = {
                        default: off,
                        off = { from: { all: Forward { duration: 0.15 } } apply: { draw_bg: { hover: 0.0, down: 0.0 }, draw_icon: { hover: 0.0 }, draw_text: { hover: 0.0 } } }
                        on = { from: { all: Forward { duration: 0.11 } } apply: { draw_bg: { hover: 1.0, down: 0.0 }, draw_icon: { hover: 1.0 }, draw_text: { hover: 1.0 } } }
                        down = { from: { all: Forward { duration: 0.05 } } apply: { draw_bg: { hover: 1.0, down: 1.0 }, draw_icon: { hover: 1.0 }, draw_text: { hover: 1.0 } } }
                    }
                }
            }
        }
    }

    pub TrackList = {{TrackList}} {
        width: Fill,
        height: Fill,
        list = <PortalList> {
            width: Fill,
            height: Fill,
            flow: Down,
            drag_scrolling: false,
            capture_overload: false,
            Row = <TrackRow> {}
            Empty = <EmptyLibraryState> {}
        }
    }
}

#[derive(Clone, Debug, DefaultNone)]
pub enum TrackListAction {
    PlayIndex(usize),
    ToggleLike(usize),
    AddToQueue(usize),
    AddToPlaylist(usize),
    RemoveTrack(usize),
    ContextMenu(usize),
    LoadDiscoverFeed,
    AddLocalFolder,
    OpenYouTubeLogin,
    LoadDemoSuite,
    None,
}

pub struct TrackListScope {
    pub tracks: Rc<Vec<Track>>,
    pub active_track_id: Option<String>,
    pub is_playing: bool,
    pub audio_level: f32,
    pub accent: Vec4,
    pub acrylic_on: f32,
    pub light_mode: f32,
    pub text_primary: Vec4,
    pub text_secondary: Vec4,
    pub icon_idle: Vec4,
    pub icon_hover: Vec4,
    pub ui_zoom: f32,
    pub artworks: Rc<HashMap<String, CachedArtwork>>,
    pub missing_art_tracks: Rc<RefCell<Vec<Track>>>,
}

#[derive(Live, LiveHook, Widget)]
pub struct TrackList {
    #[deref]
    view: View,
    #[rust]
    smooth_rem_y: f64,
    #[rust]
    smooth_abs: DVec2,
    #[rust]
    smooth_window_id: Option<WindowId>,
    #[rust]
    smooth_frame: NextFrame,
    #[rust]
    smooth_last_t: f64,
    #[rust]
    drawn_item_ids: Vec<usize>,
    #[rust]
    row_scratch_a: String,
    #[rust]
    row_scratch_b: String,
}

impl TrackListRef {
    /// Unbind `FerricCoverArt` texture handles across all materialized `PortalList` rows so
    /// `cx.textures.free_unused_textures()` immediately releases GPU VRAM on tray hide / page leave.
    pub fn clear_row_textures(&self, cx: &mut Cx) {
        if let Some(mut inner) = self.borrow_mut() {
            let list = inner.view.portal_list(id!(list));
            let first = list.first_id();
            let vis = list.visible_items().max(24);
            for id in inner.drawn_item_ids.drain(..) {
                if let Some((_, item)) = list.get_item(id) {
                    item.ferric_cover_art(id!(art_tile))
                        .set_texture(cx, None, 1.0);
                }
            }
            for id in first.saturating_sub(4)..first.saturating_add(vis + 8) {
                if let Some((_, item)) = list.get_item(id) {
                    item.ferric_cover_art(id!(art_tile))
                        .set_texture(cx, None, 1.0);
                }
            }
        }
    }
}

impl Widget for TrackList {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        let uid = self.widget_uid();

        // 60 FPS exponential momentum smooth wheel scrolling over Makepad PortalList
        if let Event::Scroll(se) = event
            && se.is_mouse
            && !se.handled_y.get()
            && self.view.area().rect(cx).contains(se.abs)
        {
            se.handled_y.set(true);
            self.smooth_rem_y += se.scroll.y * 1.15;
            self.smooth_abs = se.abs;
            self.smooth_window_id = Some(se.window_id);
            self.smooth_frame = cx.new_next_frame();
            return;
        }
        if let Some(nf) = self.smooth_frame.is_event(event) {
            let dt = if self.smooth_last_t > 0.0 {
                (nf.time - self.smooth_last_t).clamp(0.004, 0.045)
            } else {
                0.016
            };
            self.smooth_last_t = nf.time;
            if self.smooth_rem_y.abs() > 0.45
                && let Some(win_id) = self.smooth_window_id
            {
                let k = 1.0 - (-16.0 * dt).exp();
                let step = self.smooth_rem_y * k;
                self.smooth_rem_y -= step;
                let synth = Event::Scroll(makepad_widgets::makepad_platform::event::ScrollEvent {
                    window_id: win_id,
                    scroll: dvec2(0.0, step),
                    abs: self.smooth_abs,
                    modifiers: KeyModifiers::default(),
                    handled_x: std::cell::Cell::new(false),
                    handled_y: std::cell::Cell::new(false),
                    is_mouse: false,
                    time: nf.time,
                });
                self.view.handle_event(cx, &synth, scope);
                if self.smooth_rem_y.abs() > 0.45 {
                    self.smooth_frame = cx.new_next_frame();
                } else {
                    self.smooth_rem_y = 0.0;
                }
            } else {
                self.smooth_rem_y = 0.0;
            }
            return;
        }

        let actions = cx.capture_actions(|cx| {
            self.view.handle_event(cx, event, scope);
        });
        let list = self.view.portal_list(id!(list));
        for (item_id, item) in list.items_with_actions(&actions) {
            if item.button(id!(empty_discover_btn)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::LoadDiscoverFeed);
                continue;
            }
            if item.button(id!(empty_add_folder_btn)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::AddLocalFolder);
                continue;
            }
            if item.button(id!(empty_youtube_btn)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::OpenYouTubeLogin);
                continue;
            }
            if item.button(id!(empty_demo_btn)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::LoadDemoSuite);
                continue;
            }
            if item.button(id!(queue_add_btn)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::AddToQueue(item_id));
                continue;
            }
            if item.button(id!(like_btn)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::ToggleLike(item_id));
                continue;
            }
            if item.button(id!(pl_add_btn)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::AddToPlaylist(item_id));
                continue;
            }
            if item.button(id!(delete_btn)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::RemoveTrack(item_id));
                continue;
            }
            if item.ferric_cover_art(id!(art_tile)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::PlayIndex(item_id));
                break;
            }
            if let Some(fe) = item.as_view().finger_up(&actions)
                && fe.is_over
                && (fe.abs - fe.abs_start).length() < 16.0
            {
                if fe.device.mouse_button() == Some(MouseButton::SECONDARY) {
                    cx.widget_action(uid, &scope.path, TrackListAction::ContextMenu(item_id));
                } else {
                    cx.widget_action(uid, &scope.path, TrackListAction::PlayIndex(item_id));
                }
                break;
            }
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        let (
            tracks,
            active_id,
            is_playing,
            level,
            accent,
            acrylic_on,
            light_mode,
            text_primary,
            text_secondary,
            icon_idle,
            icon_hover,
            zoom,
            artworks,
            missing_art,
        ) = scope
            .props
            .get::<TrackListScope>()
            .map(|d| {
                (
                    Rc::clone(&d.tracks),
                    d.active_track_id.clone(),
                    d.is_playing,
                    d.audio_level,
                    d.accent,
                    d.acrylic_on,
                    d.light_mode,
                    d.text_primary,
                    d.text_secondary,
                    d.icon_idle,
                    d.icon_hover,
                    d.ui_zoom.clamp(0.65, 1.40),
                    Rc::clone(&d.artworks),
                    Some(Rc::clone(&d.missing_art_tracks)),
                )
            })
            .unwrap_or_else(|| {
                (
                    Rc::new(Vec::new()),
                    None,
                    false,
                    0.5,
                    vec4(0.910, 0.416, 0.173, 1.0),
                    0.0,
                    0.0,
                    vec4(0.957, 0.957, 0.969, 1.0),
                    vec4(0.604, 0.612, 0.659, 1.0),
                    vec4(0.431, 0.443, 0.502, 1.0),
                    vec4(0.957, 0.957, 0.969, 1.0),
                    1.0,
                    Rc::new(HashMap::new()),
                    None,
                )
            });
        let count = tracks.len();
        let row_h = (46.0 * zoom) as f64;
        let art_sz = (34.0 * zoom) as f64;
        let idx_w = (22.0 * zoom).max(16.0) as f64;
        let album_w = (148.0 * zoom).max(90.0) as f64;
        let dur_w = (44.0 * zoom).max(34.0) as f64;
        let row_btn_sz = (26.0 * zoom).max(20.0) as f64;
        let row_icon_sz = (14.0 * zoom).max(11.0) as f64;
        let title_fs = (9.6 * zoom) as f64;
        let sub_fs = (8.4 * zoom) as f64;
        let del_hover = if light_mode > 0.5 {
            vec4(0.75, 0.14, 0.18, 1.0)
        } else {
            vec4(1.0, 0.42, 0.48, 1.0)
        };

        while let Some(item) = self.view.draw_walk(cx, scope, walk).step() {
            if let Some(mut list) = item.as_portal_list().borrow_mut() {
                if count == 0 {
                    list.set_item_range(cx, 0, 1);
                    while let Some(item_id) = list.next_visible_item(cx) {
                        if item_id == 0 {
                            let empty = list.item(cx, 0, live_id!(Empty));
                            empty.apply_over(
                                cx,
                                live! {
                                    draw_bg: {
                                        acrylic_on: (acrylic_on),
                                        light_mode: (light_mode)
                                    }
                                    empty_title_lbl = { draw_text: { color: (text_primary) } }
                                    empty_sub_lbl = { draw_text: { color: (text_secondary) } }
                                },
                            );
                            // Action buttons live inside an anonymous child <View>, so apply directly via WidgetRef lookup.
                            empty
                                .button(id!(empty_discover_btn))
                                .apply_over(cx, live! { draw_bg: { light_mode: (light_mode) } });
                            let btn_text_col = text_primary;
                            let btn_hover_col = icon_hover;
                            for btn_id in [id!(empty_add_folder_btn), id!(empty_youtube_btn)] {
                                empty.button(btn_id).apply_over(
                                    cx,
                                    live! {
                                        draw_bg: { light_mode: (light_mode) }
                                        draw_text: {
                                            color: (btn_text_col),
                                            color_hover: (btn_hover_col),
                                            color_focus: (btn_hover_col),
                                            color_down: (btn_hover_col)
                                        }
                                        draw_icon: {
                                            color: (accent),
                                            color_hover: (btn_hover_col),
                                            color_focus: (btn_hover_col),
                                            color_down: (btn_hover_col)
                                        }
                                    },
                                );
                            }
                            let demo_txt = if light_mode > 0.5 {
                                accent
                            } else {
                                vec4(0.961, 0.643, 0.451, 1.0)
                            };
                            empty.button(id!(empty_demo_btn)).apply_over(
                                cx,
                                live! {
                                    draw_bg: { light_mode: (light_mode) }
                                    draw_text: {
                                        color: (demo_txt),
                                        color_hover: (btn_hover_col),
                                        color_focus: (btn_hover_col),
                                        color_down: (btn_hover_col)
                                    }
                                    draw_icon: {
                                        color: (accent),
                                        color_hover: (btn_hover_col),
                                        color_focus: (btn_hover_col),
                                        color_down: (btn_hover_col)
                                    }
                                },
                            );
                            empty.draw_all(cx, scope);
                        }
                    }
                } else {
                    self.drawn_item_ids.clear();
                    list.set_item_range(cx, 0, count);
                    while let Some(item_id) = list.next_visible_item(cx) {
                        let Some(track) = tracks.get(item_id) else {
                            continue;
                        };
                        self.drawn_item_ids.push(item_id);
                        let row = list.item(cx, item_id, live_id!(Row));
                        let is_active = active_id.as_deref() == Some(track.id.as_str());
                        let title_col = if is_active { accent } else { text_primary };
                        let idx_col = if is_active { accent } else { text_secondary };
                        let like_col = if track.liked { accent } else { icon_idle };

                        row.apply_over(
                            cx,
                            live! {
                                height: (row_h),
                                padding: { left: (10.0 * zoom), right: (12.0 * zoom), top: 4, bottom: 4 }
                                spacing: (10.0 * zoom),
                                draw_bg: {
                                    active: (if is_active { 1.0 } else { 0.0 }),
                                    accent_col: (accent),
                                    acrylic_on: (acrylic_on),
                                    light_mode: (light_mode)
                                }
                                art_tile = { width: (art_sz), height: (art_sz) }
                                idx_lbl = { width: (idx_w), draw_text: { color: (idx_col), text_style: { font_size: (sub_fs) } } }
                                album_lbl = { width: (album_w), padding: { right: 6 }, draw_text: { color: (text_secondary), text_style: { font_size: (sub_fs) } } }
                                dur_lbl = { width: (dur_w), draw_text: { color: (text_secondary), text_style: { font_size: (sub_fs) } } }
                                queue_add_btn = {
                                    width: (row_btn_sz), height: (row_btn_sz),
                                    icon_walk: { width: (row_icon_sz), height: (row_icon_sz) }
                                    draw_bg: { light_mode: (light_mode) }
                                    draw_icon: { color: (icon_idle), color_hover: (icon_hover) }
                                }
                                like_btn = {
                                    width: (row_btn_sz), height: (row_btn_sz),
                                    icon_walk: { width: (row_icon_sz), height: (row_icon_sz) }
                                    draw_bg: { light_mode: (light_mode) }
                                    draw_icon: { color: (like_col), color_hover: (icon_hover) }
                                }
                                pl_add_btn = {
                                    width: (row_btn_sz), height: (row_btn_sz),
                                    icon_walk: { width: (row_icon_sz), height: (row_icon_sz) }
                                    draw_bg: { light_mode: (light_mode) }
                                    draw_icon: { color: (icon_idle), color_hover: (icon_hover) }
                                }
                                delete_btn = {
                                    width: (row_btn_sz), height: (row_btn_sz),
                                    icon_walk: { width: (row_icon_sz), height: (row_icon_sz) }
                                    draw_bg: { light_mode: (light_mode) }
                                    draw_icon: { color: (icon_idle), color_hover: (del_hover), color_down: (del_hover) }
                                }
                            },
                        );
                        // `title_lbl` and `artist_lbl` are nested inside `info_col`, so direct `row.apply_over`
                        // skips them in Makepad 1.0; style them directly for Warm Cream Light & UI Zoom.
                        row.label(id!(title_lbl)).apply_over(
                            cx,
                            live! { draw_text: { color: (title_col), text_style: { font_size: (title_fs) } } },
                        );
                        row.label(id!(artist_lbl)).apply_over(
                            cx,
                            live! { draw_text: { color: (text_secondary), text_style: { font_size: (sub_fs) } } },
                        );

                        let art_ref = row.ferric_cover_art(id!(art_tile));
                        if let Some(key) = ArtworkCache::key_for_track(track) {
                            if let Some(cached) = artworks.get(&key) {
                                art_ref.set_texture(
                                    cx,
                                    Some(cached.texture.clone()),
                                    cached.aspect,
                                );
                            } else {
                                art_ref.set_texture(cx, None, 1.0);
                                if let Some(ref queue) = missing_art
                                    && let Ok(mut q) = queue.try_borrow_mut()
                                    && q.len() < 16
                                {
                                    q.push(track.clone());
                                }
                            }
                        } else {
                            art_ref.set_texture(cx, None, 1.0);
                        }
                        art_ref.set_state(cx, is_active && is_playing, level, accent, false);

                        if is_active {
                            row.label(id!(idx_lbl)).set_text(cx, "•");
                        } else {
                            self.row_scratch_a.clear();
                            let _ = write!(&mut self.row_scratch_a, "{}", item_id + 1);
                            row.label(id!(idx_lbl)).set_text(cx, &self.row_scratch_a);
                        }
                        clean_clip_into(&track.title, 38, Some("Unknown"), &mut self.row_scratch_a);
                        row.label(id!(title_lbl)).set_text(cx, &self.row_scratch_a);

                        match track.source {
                            TrackSource::Local => {
                                clean_clip_into(
                                    &track.artist,
                                    42,
                                    Some("Unknown"),
                                    &mut self.row_scratch_a,
                                );
                            }
                            other => {
                                clean_clip_into(&track.artist, 42, None, &mut self.row_scratch_b);
                                self.row_scratch_b.push_str(" · ");
                                self.row_scratch_b.push_str(other.as_badge());
                                clean_clip_into(
                                    &self.row_scratch_b,
                                    42,
                                    Some("Unknown"),
                                    &mut self.row_scratch_a,
                                );
                            }
                        }
                        row.label(id!(artist_lbl)).set_text(cx, &self.row_scratch_a);

                        clean_clip_into(&track.album, 24, Some("Unknown"), &mut self.row_scratch_a);
                        row.label(id!(album_lbl)).set_text(cx, &self.row_scratch_a);

                        if track.duration_ms > 0 {
                            let total_secs = track.duration_ms / 1000;
                            let mins = total_secs / 60;
                            let secs = total_secs % 60;
                            self.row_scratch_a.clear();
                            let _ = write!(&mut self.row_scratch_a, "{mins}:{secs:02}");
                            row.label(id!(dur_lbl)).set_text(cx, &self.row_scratch_a);
                        } else {
                            row.label(id!(dur_lbl)).set_text(cx, "-");
                        }
                        row.draw_all(cx, scope);
                    }
                }
            }
        }
        DrawStep::done()
    }
}
