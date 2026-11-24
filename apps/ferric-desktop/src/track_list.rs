use ferric_core::models::{Track, format_duration_ms};
use makepad_widgets::*;

live_design! {
    use link::theme::*;
    use link::shaders::*;
    use link::widgets::*;

    TrackRow = <View> {
        width: Fill,
        height: 52,
        padding: { left: 14, right: 14, top: 6, bottom: 6 }
        margin: { bottom: 2 }
        flow: Right,
        align: { y: 0.5 }
        spacing: 12,
        show_bg: true,
        cursor: Hand,

        draw_bg: {
            instance active: 0.0
            uniform base_col: #00000000
            uniform active_col: #240E05
            uniform active_border: #D9541E
            uniform radius: 8.0

            fn pixel(self) -> vec4 {
                let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                sdf.box(1.0, 1.0, self.rect_size.x - 2.0, self.rect_size.y - 2.0, self.radius);
                let bg = mix(self.base_col, self.active_col, self.active);
                sdf.fill_keep(bg);
                let border = mix(vec4(0.0, 0.0, 0.0, 0.0), self.active_border, self.active * 0.72);
                sdf.stroke(border, 1.0);
                return sdf.result;
            }
        }

        idx_lbl = <Label> {
            width: 30,
            draw_text: {
                color: #888279,
                text_style: { font_size: 11.0 }
            }
            text: "01"
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
                    text_style: { font_size: 13.0 }
                }
                text: "Track Title"
            }
            artist_lbl = <Label> {
                width: Fill,
                draw_text: {
                    color: #ABA59B,
                    text_style: { font_size: 11.0 }
                }
                text: "Artist - Album"
            }
        }

        origin_pill = <RoundedView> {
            width: 78,
            height: 22,
            align: { x: 0.5, y: 0.5 }
            draw_bg: {
                color: #141414,
                border_radius: 5.0,
                border_size: 1.0,
                border_color: #242424
            }
            origin_lbl = <Label> {
                draw_text: { color: #ABA59B, text_style: { font_size: 9.5 } }
                text: "LOCAL"
            }
        }

        codec_pill = <RoundedView> {
            width: 82,
            height: 22,
            align: { x: 0.5, y: 0.5 }
            draw_bg: {
                color: #141414,
                border_radius: 5.0,
                border_size: 1.0,
                border_color: #242424
            }
            codec_lbl = <Label> {
                draw_text: { color: #F3EEE3, text_style: { font_size: 9.5 } }
                text: "FLAC"
            }
        }

        dur_lbl = <Label> {
            width: 48,
            align: { x: 1.0, y: 0.5 }
            draw_text: {
                color: #ABA59B,
                text_style: { font_size: 11.0 }
            }
            text: "03:45"
        }
    }

    EmptyLibraryState = <View> {
        width: Fill,
        height: 220,
        flow: Down,
        align: { x: 0.5, y: 0.5 }
        spacing: 10,
        <Label> {
            draw_text: { color: #F3EEE3, text_style: { font_size: 16.0 } }
            text: "Your Ferric Library is Empty"
        }
        <Label> {
            draw_text: { color: #ABA59B, text_style: { font_size: 12.5 } }
            text: "Click '+ Add Folder' in the top bar to scan local FLAC, Opus, MP3, AAC, ALAC, or WAV files."
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
    None,
}

pub struct TrackListScope<'a> {
    pub tracks: &'a [Track],
    pub active_track_id: Option<&'a str>,
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
            if let Some(fe) = item.as_view().finger_up(&actions)
                && fe.is_over
                && fe.was_tap()
            {
                cx.widget_action(uid, &scope.path, TrackListAction::PlayIndex(item_id));
            }
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        while let Some(item) = self.view.draw_walk(cx, scope, walk).step() {
            if let Some(mut list) = item.as_portal_list().borrow_mut() {
                let data = scope.props.get::<TrackListScope<'_>>();
                let count = data.map_or(0, |d| d.tracks.len());
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
                        let Some(d) = data else { continue };
                        let Some(track) = d.tracks.get(item_id) else {
                            continue;
                        };
                        let row = list.item(cx, item_id, live_id!(Row));
                        let is_active = d.active_track_id == Some(track.id.as_str());
                        row.apply_over(
                            cx,
                            live! {
                                draw_bg: { active: (if is_active { 1.0 } else { 0.0 }) }
                            },
                        );
                        let idx_txt = if is_active {
                            ">".to_string()
                        } else {
                            format!("{:02}", item_id + 1)
                        };
                        row.label(id!(idx_lbl)).set_text(cx, &idx_txt);
                        row.label(id!(title_lbl)).set_text(cx, &track.title);
                        row.label(id!(artist_lbl))
                            .set_text(cx, &format!("{} · {}", track.artist, track.album));
                        row.label(id!(origin_lbl))
                            .set_text(cx, track.source.as_badge());
                        row.label(id!(codec_lbl))
                            .set_text(cx, &track.quality_badge());
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
