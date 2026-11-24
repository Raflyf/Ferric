use ferric_core::models::{Track, TrackSource, format_duration_ms};
use makepad_widgets::*;
use std::rc::Rc;

live_design! {
    use link::theme::*;
    use link::shaders::*;
    use link::widgets::*;
    use crate::widgets::FerricArtTile;

    TrackRow = <View> {
        width: Fill,
        height: 56,
        padding: { left: 16, right: 18, top: 6, bottom: 6 }
        margin: { bottom: 2 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 14,
        show_bg: true,
        cursor: Hand,

        draw_bg: {
            instance active: 0.0
            uniform base_col: #00000000
            uniform active_col: #190D08
            uniform active_border: #D9541E
            uniform radius: 8.0

            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, self.radius * 0.5);
                let bg = mix(self.base_col, self.active_col, self.active);
                sdf.fill_keep(bg);
                let border = mix(vec4(0.0, 0.0, 0.0, 0.0), self.active_border, self.active * 0.55);
                sdf.stroke(border, 1.0);
                return sdf.result;
            }
        }

        idx_lbl = <Label> {
            width: 26,
            draw_text: {
                color: #888279,
                text_style: { font_size: 12.0 }
            }
            text: "1"
        }

        art_tile = <FerricArtTile> {
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
                draw_text: {
                    color: #F3EEE3,
                    text_style: { font_size: 13.5 }
                }
                text: "Song Title"
            }
            artist_lbl = <Label> {
                width: Fill,
                draw_text: {
                    color: #9E988E,
                    text_style: { font_size: 11.5 }
                }
                text: "Artist"
            }
        }

        album_lbl = <Label> {
            width: 220,
            draw_text: {
                color: #9E988E,
                text_style: { font_size: 12.0 }
            }
            text: "Album"
        }

        like_btn = <Button> {
            width: 34,
            height: 30,
            padding: 0,
            spacing: 0,
            align: { x: 0.5, y: 0.5 }
            text: ""
            label_walk: { width: 0, height: 0 }
            icon_walk: { width: 15, height: Fit }
            draw_icon: {
                svg_path: "M12 21.35L10.55 20.03C5.4 15.36 2 12.28 2 8.5C2 5.42 4.42 3 7.5 3C9.24 3 10.91 3.81 12 5.09C13.09 3.81 14.76 3 16.5 3C19.58 3 22 5.42 22 8.5C22 12.28 18.6 15.36 13.45 20.04L12 21.35Z"
                color: #5A5650
                color_hover: #F3EEE3
                color_down: #D9541E
            }
            draw_bg: {
                instance hover: 0.0
                fn pixel(self) -> vec4 {
                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                    sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 4.0);
                    sdf.fill(vec4(0.14, 0.14, 0.14, 0.80 * self.hover));
                    return sdf.result;
                }
            }
        }

        dur_lbl = <Label> {
            width: 68,
            padding: { right: 4 }
            align: { x: 1.0, y: 0.5 }
            draw_text: {
                color: #9E988E,
                text_style: { font_size: 12.0 }
            }
            text: "03:45"
        }
    }

    EmptyLibraryState = <View> {
        width: Fill,
        height: 236,
        padding: { left: 28, right: 28, top: 26, bottom: 26 }
        margin: { top: 8 }
        flow: Down,
        align: { x: 0.0, y: 0.5 }
        spacing: 12,
        show_bg: true,
        draw_bg: {
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 6.0);
                sdf.fill_keep(#0A0A0A);
                sdf.stroke(#1E1E1E, 1.0);
                return sdf.result;
            }
        }
        <Label> {
            draw_text: { color: #F3EEE3, text_style: { font_size: 19.0 } }
            text: "Mulai koleksi musik Anda"
        }
        <Label> {
            width: Fill,
            draw_text: { color: #9E988E, text_style: { font_size: 13.0 } }
            text: "Pilih folder musik lokal (FLAC, MP3, Opus, WAV), import playlist Spotify / YouTube Music secara instan, atau putar lagu demo bawaan untuk mencoba transisi tanpa jeda (Smooth Fade-In)."
        }
        <View> {
            width: Fill,
            height: Fit,
            margin: { top: 8 }
            flow: Right,
            spacing: 10,
            empty_add_folder_btn = <Button> {
                width: Fit, height: 38, padding: { left: 16, right: 18 }, spacing: 8,
                text: "Add Music Folder"
                icon_walk: { width: 15, height: Fit }
                draw_icon: {
                    svg_path: "M20 6H12L10 4H4C2.89 4 2 4.89 2 6V18C2 19.11 2.89 20 4 20H20C21.11 20 22 19.11 22 18V8C22 6.89 21.11 6 20 6ZM19 14H16V17H14V14H11V12H14V9H16V12H19V14Z"
                    color: #080808
                    color_hover: #080808
                }
                draw_text: { color: #080808, color_hover: #080808, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        let r = (self.rect_size.y - 1.0) * 0.25;
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, r);
                        sdf.fill(mix(#D9541E, #EB642C, self.hover));
                        return sdf.result;
                    }
                }
            }
            empty_spotify_btn = <Button> {
                width: Fit, height: 38, padding: { left: 14, right: 16 }, spacing: 8,
                text: "Spotify Sync & Login"
                icon_walk: { width: 15, height: Fit }
                draw_icon: {
                    svg_path: "M19.35 10.04C18.67 6.59 15.64 4 12 4C9.11 4 6.6 5.64 5.35 8.04C2.34 8.36 0 10.91 0 14C0 17.31 2.69 20 6 20H19C21.76 20 24 17.76 24 15C24 12.36 21.95 10.22 19.35 10.04Z"
                    color: #D9541E
                    color_hover: #F3EEE3
                }
                draw_text: { color: #F3EEE3, color_hover: #FFFFFF, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 4.0);
                        sdf.fill_keep(mix(#121212, #1C1C1C, self.hover));
                        sdf.stroke(#2A2A2A, 1.0);
                        return sdf.result;
                    }
                }
            }
            empty_youtube_btn = <Button> {
                width: Fit, height: 38, padding: { left: 14, right: 16 }, spacing: 8,
                text: "YouTube Music Sync"
                icon_walk: { width: 15, height: Fit }
                draw_icon: {
                    svg_path: "M17 10.5V7C17 6.45 16.55 6 16 6H4C3.45 6 3 6.45 3 7V17C3 17.55 3.45 18 4 18H16C16.55 18 17 17.55 17 17V13.5L21 17.5V6.5L17 10.5Z"
                    color: #D9541E
                    color_hover: #F3EEE3
                }
                draw_text: { color: #F3EEE3, color_hover: #FFFFFF, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 4.0);
                        sdf.fill_keep(mix(#121212, #1C1C1C, self.hover));
                        sdf.stroke(#2A2A2A, 1.0);
                        return sdf.result;
                    }
                }
            }
            empty_demo_btn = <Button> {
                width: Fit, height: 38, padding: { left: 14, right: 16 }, spacing: 8,
                text: "Play Demo Tracks"
                icon_walk: { width: 13, height: Fit }
                draw_icon: {
                    svg_path: "M8 5V19L19 12L8 5Z"
                    color: #D9541E
                    color_hover: #F3EEE3
                }
                draw_text: { color: #D9541E, color_hover: #F3EEE3, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 4.0);
                        sdf.fill_keep(mix(#150B06, #231109, self.hover));
                        sdf.stroke(#D9541E, 1.0);
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
    AddLocalFolder,
    OpenSpotifyLogin,
    OpenYouTubeLogin,
    LoadDemoSuite,
    None,
}

pub struct TrackListScope {
    pub tracks: Rc<Vec<Track>>,
    pub active_track_id: Option<String>,
    pub is_playing: bool,
    pub audio_level: f32,
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
            if item.button(id!(empty_add_folder_btn)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::AddLocalFolder);
                continue;
            }
            if item.button(id!(empty_spotify_btn)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::OpenSpotifyLogin);
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
            if item.button(id!(like_btn)).clicked(&actions) {
                cx.widget_action(uid, &scope.path, TrackListAction::ToggleLike(item_id));
                continue;
            }
            if let Some(fe) = item.as_view().finger_up(&actions)
                && fe.is_over
                && fe.was_tap()
            {
                cx.widget_action(uid, &scope.path, TrackListAction::PlayIndex(item_id));
            }
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        let (tracks, active_id, is_playing, level) = scope
            .props
            .get::<TrackListScope>()
            .map(|d| {
                (
                    Rc::clone(&d.tracks),
                    d.active_track_id.clone(),
                    d.is_playing,
                    d.audio_level,
                )
            })
            .unwrap_or_else(|| (Rc::new(Vec::new()), None, false, 0.5));
        let count = tracks.len();

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
                            vec4(0.851, 0.329, 0.118, 1.0)
                        } else {
                            vec4(0.953, 0.933, 0.890, 1.0)
                        };
                        let idx_col = if is_active {
                            vec4(0.851, 0.329, 0.118, 1.0)
                        } else {
                            vec4(0.533, 0.510, 0.475, 1.0)
                        };
                        let like_col = if track.liked {
                            vec4(0.851, 0.329, 0.118, 1.0)
                        } else {
                            vec4(0.353, 0.337, 0.314, 1.0)
                        };
                        let art_var = if item_id % 2 == 0 { 0.0 } else { 1.0 };
                        let art_play = if is_active && is_playing { 1.0 } else { 0.0 };

                        row.apply_over(
                            cx,
                            live! {
                                draw_bg: { active: (if is_active { 1.0 } else { 0.0 }) }
                                idx_lbl = { draw_text: { color: (idx_col) } }
                                title_lbl = { draw_text: { color: (title_col) } }
                                like_btn = { draw_icon: { color: (like_col) } }
                                art_tile = {
                                    draw_bg: {
                                        variant: (art_var),
                                        playing: (art_play),
                                        level: (level)
                                    }
                                }
                            },
                        );

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
                        row.label(id!(title_lbl)).set_text(cx, &track.title);
                        row.label(id!(artist_lbl)).set_text(cx, &artist_sub);
                        row.label(id!(album_lbl)).set_text(cx, &track.album);
                        row.label(id!(dur_lbl))
                            .set_text(cx, &format_duration_ms(track.duration_ms));
                        row.draw_all(cx, scope);
                    }
                }
            }
        }
        DrawStep::done()
    }
}
