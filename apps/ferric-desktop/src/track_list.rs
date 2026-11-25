use crate::artwork::{ArtworkCache, CachedArtwork};
use crate::state::clip_text;
use crate::widgets::FerricCoverArtWidgetRefExt;
use ferric_core::models::{Track, TrackSource, format_duration_ms};
use makepad_widgets::*;
use std::collections::HashMap;
use std::rc::Rc;

live_design! {
    use link::theme::*;
    use link::shaders::*;
    use link::widgets::*;
    use crate::widgets::FerricCoverArt;

    TrackRow = <View> {
        width: Fill,
        height: 58,
        padding: { left: 12, right: 14, top: 6, bottom: 6 }
        margin: { bottom: 3 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 12,
        show_bg: true,
        cursor: Hand,

        draw_bg: {
            instance active: 0.0
            instance hover: 0.0
            instance down: 0.0
            uniform base_col: #00000000
            uniform hover_col: #13151D
            uniform active_col: #181726
            uniform accent_col: #A882FF
            uniform glass_on: 0.0
            uniform dispersion: 0.42
            uniform rim_amt: 0.78
            uniform scrim_amt: 0.28
            uniform radius: 10.0

            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                let r = mix(7.0, clamp(self.radius * 0.55, 7.0, 14.0), self.glass_on);
                sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);

                let uv = self.pos;
                let top_sheen = smoothstep(0.45, 0.0, uv.y) * (0.09 + 0.06 * self.hover);
                let glass_base = clamp(
                    mix(vec3(0.11, 0.12, 0.18), self.accent_col.xyz, 0.08 + 0.06 * self.hover)
                        + vec3(top_sheen, top_sheen, top_sheen),
                    0.0,
                    1.0
                );
                let glass_alpha = clamp(0.28 + self.scrim_amt * 0.34 + self.hover * 0.14 - self.down * 0.04, 0.24, 0.78);
                let glass_idle = vec4(glass_base, glass_alpha);
                let solid_idle = mix(self.base_col, self.hover_col, self.hover);
                let idle_or_hover = mix(solid_idle, glass_idle, self.glass_on);

                let active_solid = mix(self.active_col, vec4(self.accent_col.xyz, 0.24), 0.40);
                let active_glass = vec4(
                    clamp(mix(vec3(0.14, 0.14, 0.22), self.accent_col.xyz, 0.32) + vec3(top_sheen, top_sheen, top_sheen), 0.0, 1.0),
                    clamp(0.46 + self.scrim_amt * 0.30, 0.40, 0.84)
                );
                let bg = mix(idle_or_hover, mix(active_solid, active_glass, self.glass_on), self.active);
                sdf.fill_keep(bg);

                let tl = smoothstep(0.65, 0.0, uv.y * 0.8 + uv.x * 0.2);
                let prism = vec3(
                    0.82 + 0.18 * cos(uv.x * 6.283),
                    0.82 + 0.18 * cos(uv.x * 6.283 - 2.094),
                    0.82 + 0.18 * cos(uv.x * 6.283 + 2.094)
                );
                let rim_rgb = mix(vec3(1.0, 1.0, 1.0), prism, self.dispersion * 0.65);
                let g_border = vec4(rim_rgb, (0.12 + (0.28 * tl + 0.12 * self.hover) * self.rim_amt) * self.glass_on);
                let s_border = vec4(1.0, 1.0, 1.0, 0.06 * self.hover);
                let a_border = vec4(self.accent_col.xyz, 0.62 * self.active);
                let border = mix(mix(s_border, g_border, self.glass_on), a_border, self.active);
                sdf.stroke(border, 1.0);
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
                    from: { all: Forward { duration: 0.13 } }
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
            width: 24,
            padding: 0,
            align: { x: 0.5, y: 0.5 }
            draw_text: {
                color: #707382,
                text_style: <THEME_FONT_MEDIUM> { font_size: 11.5 }
            }
            text: "1"
        }

        art_tile = <FerricCoverArt> {
            width: 40,
            height: 40,
            draw_bg: { radius: 8.0 }
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
                    text_style: <THEME_FONT_BOLD> { font_size: 13.0, line_spacing: 1.0 }
                }
                text: "Song Title"
            }
            artist_lbl = <Label> {
                width: Fill,
                flow: Right,
                padding: 0,
                draw_text: {
                    color: #9A9CA8,
                    text_style: <THEME_FONT_REGULAR> { font_size: 11.5, line_spacing: 1.0 }
                }
                text: "Artist"
            }
        }

        album_lbl = <Label> {
            width: 150,
            flow: Right,
            padding: 0,
            draw_text: {
                color: #888B99,
                text_style: <THEME_FONT_REGULAR> { font_size: 11.5, line_spacing: 1.0 }
            }
            text: "Album"
        }

        queue_add_btn = <Button> {
            width: 30,
            height: 28,
            padding: 0,
            spacing: 0,
            align: { x: 0.5, y: 0.5 }
            text: ""
            label_walk: { width: 0, height: 0 }
            icon_walk: { width: 14, height: 14 }
            draw_icon: {
                svg_path: "M14 10H2V12H14V10ZM14 6H2V8H14V6ZM18 14V10H16V14H12V16H16V20H18V16H22V14H18ZM2 16H10V14H2V16Z"
                color: #6E7180
                color_hover: #F4F4F7
                color_down: #A882FF
            }
            draw_bg: {
                instance hover: 0.0
                fn pixel(self) -> vec4 {
                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                    sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 5.0);
                    sdf.fill(vec4(0.18, 0.19, 0.25, 0.80 * self.hover));
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
            width: 30,
            height: 28,
            padding: 0,
            spacing: 0,
            align: { x: 0.5, y: 0.5 }
            text: ""
            label_walk: { width: 0, height: 0 }
            icon_walk: { width: 14, height: 14 }
            draw_icon: {
                svg_path: "M12 21.35L10.55 20.03C5.4 15.36 2 12.28 2 8.5C2 5.42 4.42 3 7.5 3C9.24 3 10.91 3.81 12 5.09C13.09 3.81 14.76 3 16.5 3C19.58 3 22 5.42 22 8.5C22 12.28 18.6 15.36 13.45 20.04L12 21.35Z"
                color: #5C5F6E
                color_hover: #F4F4F7
                color_down: #A882FF
            }
            draw_bg: {
                instance hover: 0.0
                fn pixel(self) -> vec4 {
                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                    sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 5.0);
                    sdf.fill(vec4(0.18, 0.19, 0.25, 0.80 * self.hover));
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

        dur_lbl = <Label> {
            width: 58,
            padding: 0,
            align: { x: 0.5, y: 0.5 }
            draw_text: {
                color: #888B99,
                text_style: <THEME_FONT_MEDIUM> { font_size: 11.5 }
            }
            text: "03:45"
        }
    }

    EmptyLibraryState = <View> {
        width: Fill,
        height: 210,
        padding: { left: 26, right: 26, top: 24, bottom: 24 }
        margin: { top: 8 }
        flow: Down,
        align: { x: 0.0, y: 0.5 }
        spacing: 10,
        show_bg: true,
        draw_bg: {
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 8.0);
                sdf.fill_keep(#15161C);
                sdf.stroke(#232530, 1.0);
                return sdf.result;
            }
        }
        <Label> {
            draw_text: { color: #F4F4F7, text_style: { font_size: 18.0 } }
            text: "No tracks here yet"
        }
        <Label> {
            width: Fill,
            draw_text: { color: #9A9CA8, text_style: { font_size: 12.5 } }
            text: "Browse YouTube Music immediately in Guest Mode, add a local folder (FLAC, MP3, Opus, WAV), or sign in with Google."
        }
        <View> {
            width: Fill,
            height: Fit,
            margin: { top: 6 }
            flow: Right,
            spacing: 10,
            empty_discover_btn = <Button> {
                width: Fit, height: 36, padding: { left: 16, right: 18 }, spacing: 8,
                text: "Refresh Discover"
                icon_walk: { width: 14, height: Fit }
                draw_icon: {
                    svg_path: "M12 3V13.55C11.41 13.21 10.73 13 10 13C7.79 13 6 14.79 6 17C6 19.21 7.79 21 10 21C12.21 21 14 19.21 14 17V7H18V3H12Z"
                    color: #0E0F13
                    color_hover: #0E0F13
                }
                draw_text: { color: #0E0F13, color_hover: #0E0F13, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        let r = (self.rect_size.y - 1.0) * 0.25;
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);
                        sdf.fill(mix(#F4F4F7, #FFFFFF, self.hover));
                        return sdf.result;
                    }
                }
            }
            empty_add_folder_btn = <Button> {
                width: Fit, height: 36, padding: { left: 14, right: 16 }, spacing: 8,
                text: "Add Folder"
                icon_walk: { width: 14, height: Fit }
                draw_icon: {
                    svg_path: "M20 6H12L10 4H4C2.89 4 2 4.89 2 6V18C2 19.11 2.89 20 4 20H20C21.11 20 22 19.11 22 18V8C22 6.89 21.11 6 20 6ZM19 14H16V17H14V14H11V12H14V9H16V12H19V14Z"
                    color: #A882FF
                    color_hover: #F4F4F7
                }
                draw_text: { color: #F4F4F7, color_hover: #FFFFFF, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 5.0);
                        sdf.fill_keep(mix(#1C1D24, #252732, self.hover));
                        sdf.stroke(#2A2C38, 1.0);
                        return sdf.result;
                    }
                }
            }
            empty_youtube_btn = <Button> {
                width: Fit, height: 36, padding: { left: 14, right: 16 }, spacing: 8,
                text: "Sign in with Google"
                icon_walk: { width: 14, height: Fit }
                draw_icon: {
                    svg_path: "M12 12C14.21 12 16 10.21 16 8C16 5.79 14.21 4 12 4C9.79 4 8 5.79 8 8C8 10.21 9.79 12 12 12ZM12 14C9.33 14 4 15.34 4 18V20H20V18C20 15.34 14.67 14 12 14Z"
                    color: #A882FF
                    color_hover: #F4F4F7
                }
                draw_text: { color: #F4F4F7, color_hover: #FFFFFF, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 5.0);
                        sdf.fill_keep(mix(#1C1D24, #252732, self.hover));
                        sdf.stroke(#2A2C38, 1.0);
                        return sdf.result;
                    }
                }
            }
            empty_demo_btn = <Button> {
                width: Fit, height: 36, padding: { left: 14, right: 16 }, spacing: 8,
                text: "Offline Demo"
                icon_walk: { width: 12, height: Fit }
                draw_icon: {
                    svg_path: "M8 5V19L19 12L8 5Z"
                    color: #A882FF
                    color_hover: #F4F4F7
                }
                draw_text: { color: #C5B2FF, color_hover: #FFFFFF, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 5.0);
                        sdf.fill_keep(mix(#1C1A29, #27233B, self.hover));
                        sdf.stroke(#443B66, 1.0);
                        return sdf.result;
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
    pub glass_on: f32,
    pub glass_dispersion: f32,
    pub glass_rim: f32,
    pub glass_scrim: f32,
    pub glass_radius: f32,
    pub ui_zoom: f32,
    pub artworks: Rc<HashMap<String, CachedArtwork>>,
}

#[derive(Live, LiveHook, Widget)]
pub struct TrackList {
    #[deref]
    view: View,
}

impl Widget for TrackList {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        let uid = self.widget_uid();
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
            if item.ferric_cover_art(id!(art_tile)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::PlayIndex(item_id));
                continue;
            }
            if let Some(fe) = item.as_view().finger_up(&actions)
                && fe.is_over
                && (fe.abs - fe.abs_start).length() < 16.0
            {
                cx.widget_action(uid, &scope.path, TrackListAction::PlayIndex(item_id));
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
            glass_on,
            dispersion,
            rim_amt,
            scrim_amt,
            radius,
            zoom,
            artworks,
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
                    d.glass_on,
                    d.glass_dispersion,
                    d.glass_rim,
                    d.glass_scrim,
                    d.glass_radius,
                    d.ui_zoom.clamp(0.8, 1.4),
                    Rc::clone(&d.artworks),
                )
            })
            .unwrap_or_else(|| {
                (
                    Rc::new(Vec::new()),
                    None,
                    false,
                    0.5,
                    vec4(0.659, 0.510, 1.0, 1.0),
                    0.0,
                    0.42,
                    0.78,
                    0.28,
                    12.0,
                    1.0,
                    Rc::new(HashMap::new()),
                )
            });
        let count = tracks.len();
        let row_h = (58.0 * zoom) as f64;
        let art_sz = (40.0 * zoom) as f64;
        let title_fs = (13.0 * zoom) as f64;
        let sub_fs = (11.5 * zoom) as f64;

        while let Some(item) = self.view.draw_walk(cx, scope, walk).step() {
            if let Some(mut list) = item.as_portal_list().borrow_mut() {
                if count == 0 {
                    list.set_item_range(cx, 0, 1);
                    while let Some(item_id) = list.next_visible_item(cx) {
                        if item_id == 0 {
                            let empty = list.item(cx, 0, live_id!(Empty));
                            empty.draw_all(cx, scope);
                        }
                    }
                } else {
                    list.set_item_range(cx, 0, count);
                    while let Some(item_id) = list.next_visible_item(cx) {
                        let Some(track) = tracks.get(item_id) else {
                            continue;
                        };
                        let row = list.item(cx, item_id, live_id!(Row));
                        let is_active = active_id.as_deref() == Some(track.id.as_str());
                        let title_col = if is_active {
                            accent
                        } else {
                            vec4(0.957, 0.957, 0.969, 1.0)
                        };
                        let idx_col = if is_active {
                            accent
                        } else {
                            vec4(0.439, 0.451, 0.510, 1.0)
                        };
                        let like_col = if track.liked {
                            accent
                        } else {
                            vec4(0.361, 0.373, 0.431, 1.0)
                        };

                        row.apply_over(
                            cx,
                            live! {
                                height: (row_h),
                                draw_bg: {
                                    active: (if is_active { 1.0 } else { 0.0 }),
                                    accent_col: (accent),
                                    glass_on: (glass_on),
                                    dispersion: (dispersion),
                                    rim_amt: (rim_amt),
                                    scrim_amt: (scrim_amt),
                                    radius: (radius)
                                }
                                art_tile = { width: (art_sz), height: (art_sz) }
                                idx_lbl = { draw_text: { color: (idx_col), text_style: { font_size: (sub_fs) } } }
                                title_lbl = { draw_text: { color: (title_col), text_style: { font_size: (title_fs) } } }
                                artist_lbl = { draw_text: { text_style: { font_size: (sub_fs) } } }
                                album_lbl = { draw_text: { text_style: { font_size: (sub_fs) } } }
                                dur_lbl = { draw_text: { text_style: { font_size: (sub_fs) } } }
                                like_btn = { draw_icon: { color: (like_col) } }
                            },
                        );

                        let art_ref = row.ferric_cover_art(id!(art_tile));
                        if let Some(key) = ArtworkCache::key_for_track(track)
                            && let Some(cached) = artworks.get(&key)
                        {
                            art_ref.set_texture(cx, Some(cached.texture.clone()), cached.aspect);
                        } else {
                            art_ref.set_texture(cx, None, 1.0);
                        }
                        art_ref.set_state(cx, is_active && is_playing, level, accent, false);

                        let idx_txt = if is_active {
                            "•".to_string()
                        } else {
                            format!("{}", item_id + 1)
                        };
                        let artist_sub = match track.source {
                            TrackSource::Local => track.artist.clone(),
                            other => format!("{} · {}", track.artist, other.as_badge()),
                        };
                        row.label(id!(idx_lbl)).set_text(cx, &idx_txt);
                        row.label(id!(title_lbl))
                            .set_text(cx, &clip_text(&track.title, 28));
                        row.label(id!(artist_lbl))
                            .set_text(cx, &clip_text(&artist_sub, 30));
                        row.label(id!(album_lbl))
                            .set_text(cx, &clip_text(&track.album, 18));
                        let dur_txt = if track.duration_ms > 0 {
                            format_duration_ms(track.duration_ms)
                        } else {
                            "--:--".to_string()
                        };
                        row.label(id!(dur_lbl)).set_text(cx, &dur_txt);
                        row.draw_all(cx, scope);
                    }
                }
            }
        }
        DrawStep::done()
    }
}
