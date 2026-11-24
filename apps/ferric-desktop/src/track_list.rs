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
        height: 58,
        padding: { left: 16, right: 18, top: 7, bottom: 7 }
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
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, self.radius);
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
            draw_bg: { radius: 6.0 }
        }

        info_col = <View> {
            width: Fill,
            height: Fit,
            flow: Down,
            spacing: 3,
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
            width: 210,
            draw_text: {
                color: #9E988E,
                text_style: { font_size: 12.0 }
            }
            text: "Album"
        }

        like_btn = <Button> {
            width: 54,
            height: 28,
            padding: { left: 8, right: 8 }
            align: { x: 0.5, y: 0.5 }
            text: "Save"
            draw_text: {
                color: #888279,
                color_hover: #F3EEE3,
                text_style: { font_size: 11.0 }
            }
            draw_bg: {
                instance hover: 0.0
                fn pixel(self) -> vec4 {
                    let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                    sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 14.0);
                    sdf.fill(vec4(0.14, 0.14, 0.14, 0.75 * self.hover));
                    return sdf.result;
                }
            }
        }

        dur_lbl = <Label> {
            width: 52,
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
        height: 240,
        padding: { left: 28, right: 28, top: 28, bottom: 28 }
        margin: { top: 8 }
        flow: Down,
        align: { x: 0.0, y: 0.5 }
        spacing: 12,
        show_bg: true,
        draw_bg: {
            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, 14.0);
                sdf.fill_keep(#0B0B0B);
                sdf.stroke(#1F1F1F, 1.0);
                return sdf.result;
            }
        }
        <Label> {
            draw_text: { color: #F3EEE3, text_style: { font_size: 20.0 } }
            text: "Mulai koleksi musik Anda"
        }
        <Label> {
            width: Fill,
            draw_text: { color: #9E988E, text_style: { font_size: 13.0 } }
            text: "Pilih folder musik lokal (FLAC, MP3, Opus, WAV), hubungkan akun Spotify atau YouTube Music, atau putar lagu demo bawaan untuk mencoba transisi tanpa jeda (Smooth Fade-In)."
        }
        <View> {
            width: Fill,
            height: Fit,
            margin: { top: 8 }
            flow: Right,
            spacing: 10,
            empty_add_folder_btn = <Button> {
                width: Fit, height: 38, padding: { left: 18, right: 18 }
                text: "Add Music Folder"
                draw_text: { color: #000000, color_hover: #000000, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 19.0);
                        sdf.fill(mix(#D9541E, #EB642C, self.hover));
                        return sdf.result;
                    }
                }
            }
            empty_spotify_btn = <Button> {
                width: Fit, height: 38, padding: { left: 18, right: 18 }
                text: "Sign in with Spotify"
                draw_text: { color: #F3EEE3, color_hover: #FFFFFF, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 19.0);
                        sdf.fill_keep(mix(#141414, #1E1E1E, self.hover));
                        sdf.stroke(#2C2C2C, 1.0);
                        return sdf.result;
                    }
                }
            }
            empty_youtube_btn = <Button> {
                width: Fit, height: 38, padding: { left: 18, right: 18 }
                text: "Sign in with YouTube"
                draw_text: { color: #F3EEE3, color_hover: #FFFFFF, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 19.0);
                        sdf.fill_keep(mix(#141414, #1E1E1E, self.hover));
                        sdf.stroke(#2C2C2C, 1.0);
                        return sdf.result;
                    }
                }
            }
            empty_demo_btn = <Button> {
                width: Fit, height: 38, padding: { left: 18, right: 18 }
                text: "Play Demo Tracks"
                draw_text: { color: #D9541E, color_hover: #F3EEE3, text_style: { font_size: 12.5 } }
                draw_bg: {
                    instance hover: 0.0
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        sdf.box(0.5, 0.5, self.rect_size.x - 1.0, self.rect_size.y - 1.0, 19.0);
                        sdf.fill_keep(mix(#160C07, #24120A, self.hover));
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
                            vec4(0.533, 0.510, 0.475, 1.0)
                        };
                        let art_var = if item_id % 2 == 0 { 0.0 } else { 1.0 };
                        let art_play = if is_active && is_playing { 1.0 } else { 0.0 };

                        row.apply_over(
                            cx,
                            live! {
                                draw_bg: { active: (if is_active { 1.0 } else { 0.0 }) }
                                idx_lbl = { draw_text: { color: (idx_col) } }
                                title_lbl = { draw_text: { color: (title_col) } }
                                like_btn = { draw_text: { color: (like_col) } }
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
                        row.button(id!(like_btn))
                            .set_text(cx, if track.liked { "Saved" } else { "Save" });
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
