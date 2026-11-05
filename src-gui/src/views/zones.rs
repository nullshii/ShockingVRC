use egui::{Align2, Rect, Sense, Ui, vec2};

use shocking_vrc_core::cli::{ContactMode, ZoneEntry, ZoneId};

use crate::app::Ctx;
use crate::bridge::Command;
use crate::model::Channel;
use crate::theme::widgets::{self as w, font, space};

const MODES: [ContactMode; 4] = [
    ContactMode::Depth,
    ContactMode::Speed,
    ContactMode::Acc,
    ContactMode::Recoil,
];

pub fn show(ui: &mut Ui, cx: &mut Ctx) {
    toolbar(ui, cx);
    ui.add_space(space::SHEET_GAP);

    let gap = space::SHEET_GAP;
    let total = ui.available_width() - gap * 2.0;
    let mapped_w = total * 0.34;
    let avatar_w = total - mapped_w * 2.0;

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        w::column(ui, mapped_w, |ui| mapped_panel(ui, cx, Channel::A));
        w::column(ui, mapped_w, |ui| mapped_panel(ui, cx, Channel::B));
        w::column(ui, avatar_w, |ui| avatar_panel(ui, cx));
    });

    ui.add_space(space::SHEET_GAP);
    calibration(ui, cx);
}

fn toolbar(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    w::sheet(ui, pal, |ui| {
        ui.horizontal(|ui| {
            let saved = cx
                .bridge
                .shared
                .zone_presets
                .read()
                .map(|z| z.entries.len())
                .unwrap_or(0);
            if w::cta(ui, pal, "Zone sets").clicked() {
                cx.send(Command::ReloadZonePresets);
                cx.ui_state.show_zone_presets = true;
            }
            ui.add_space(4.0);
            w::dim(
                ui,
                pal,
                match saved {
                    0 => "Save the current mapping as a reusable set".to_string(),
                    1 => "1 saved set".to_string(),
                    n => format!("{n} saved sets"),
                },
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for ch in [Channel::B, Channel::A] {
                    if w::button_enabled(
                        ui,
                        pal,
                        &format!("Add all → {}", ch.label()),
                        !cx.snap.avatar_zones.is_empty(),
                    )
                    .on_hover_text("Map every zone the avatar is reporting")
                    .clicked()
                    {
                        cx.send(Command::AddAllAvatarZones(ch));
                    }
                }
            });
        });
    });
}

fn mapped_panel(ui: &mut Ui, cx: &mut Ctx, ch: Channel) {
    let pal = cx.pal;
    let zones = cx.snap.channel(ch).zones.clone();
    let selected = cx.ui_state.sel_zone_for(ch);
    let active: Vec<String> = cx
        .snap
        .channel_status(ch)
        .active_zones
        .iter()
        .map(|(id, _)| id.to_string())
        .collect();

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(
            ui,
            pal,
            &format!("Channel {}", ch.label()),
            Some(&format!("{} mapped", zones.len())),
        );

        if zones.is_empty() {
            w::empty_state(ui, pal, "no zones — add one from the avatar list");
            return;
        }

        let mut remove = None;
        for (i, e) in zones.iter().enumerate() {
            let is_selected = i == selected;
            let (rect, resp) = w::row(ui, pal, is_selected, 38.0);
            let inner = rect.shrink2(vec2(8.0, 4.0));
            let live = active.contains(&e.id.to_string());

            w::text_in(
                ui,
                Rect::from_min_size(inner.min, vec2(inner.width() - 18.0, 15.0)),
                &e.id.to_string(),
                font::body(),
                if live { pal.accent } else { pal.contrast },
                Align2::LEFT_CENTER,
            );

            let mut sub = e.mode.to_string();
            if e.scale != 100 {
                sub.push_str(&format!(" · {}%", e.scale));
            }
            if e.min_threshold != ZoneEntry::THRESHOLD_MIN || e.max_threshold != ZoneEntry::THRESHOLD_MAX {
                sub.push_str(&format!(
                    " · thr {:.2}–{:.2}",
                    e.min_threshold as f32 / 100.0,
                    e.max_threshold as f32 / 100.0
                ));
            }
            w::text_in(
                ui,
                Rect::from_min_size(inner.min + vec2(0.0, 16.0), vec2(inner.width(), 13.0)),
                &sub,
                font::tiny(),
                pal.dim,
                Align2::LEFT_CENTER,
            );

            if live {
                let dot = Rect::from_center_size(egui::pos2(inner.right() - 4.0, inner.top() + 7.0), vec2(6.0, 6.0));
                ui.painter().rect_filled(dot, w::SQUARE, pal.accent);
            }

            if resp.clicked() {
                cx.ui_state.set_sel_zone(ch, i);
            }
            resp.context_menu(|ui| {
                if ui.button("Remove zone").clicked() {
                    remove = Some(i);
                    ui.close();
                }
            });
        }
        if let Some(i) = remove {
            cx.send(Command::RemoveZone(ch, i));
        }
    });
}

fn avatar_panel(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, "Avatar", Some(&format!("{} seen", cx.snap.avatar_zones.len())));

        if cx.snap.avatar_zones.is_empty() {
            w::empty_state(ui, pal, "nothing yet — enable OSC in VRChat and touch a contact");
            return;
        }

        w::text_field(ui, pal, &mut cx.ui_state.avatar_filter, "filter…", ui.available_width());
        ui.add_space(space::ROW_GAP);

        let filter = cx.ui_state.avatar_filter.to_lowercase();
        let mut add: Option<(Channel, ZoneId)> = None;

        egui::ScrollArea::vertical()
            .max_height(280.0)
            .auto_shrink([false, true])
            .id_salt("avatar_zones")
            .show(ui, |ui| {
                let mut shown = 0usize;
                for (i, z) in cx.snap.avatar_zones.iter().enumerate() {
                    let name = format!("{}/{}", z.zone_type, z.id);
                    if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                        continue;
                    }
                    shown += 1;
                    let is_sel = i == cx.ui_state.sel_avatar;
                    let (rect, resp) = w::row(ui, pal, is_sel, 30.0);
                    let inner = rect.shrink2(vec2(8.0, 0.0));

                    let f = z.level.clamp(0.0, 1.0);
                    if f > 0.0 {
                        ui.painter().rect_filled(
                            Rect::from_min_size(
                                egui::pos2(rect.left(), rect.bottom() - 2.0),
                                vec2(rect.width() * f, 2.0),
                            ),
                            w::SQUARE,
                            pal.accent,
                        );
                    }

                    w::text_in(
                        ui,
                        Rect::from_min_max(inner.min, egui::pos2(inner.right() - 74.0, inner.bottom())),
                        &name,
                        font::small(),
                        if f > 0.0 { pal.accent } else { pal.contrast },
                        Align2::LEFT_CENTER,
                    );

                    let mut bx = inner.right();
                    for ch in [Channel::B, Channel::A] {
                        let bw = 32.0;
                        let brect = Rect::from_min_size(egui::pos2(bx - bw, inner.center().y - 10.0), vec2(bw, 20.0));
                        let br = ui.interact(brect, ui.id().with(("addzone", i, ch.label())), Sense::click());
                        let hot = br.hovered();
                        ui.painter()
                            .rect_filled(brect, w::SQUARE, if hot { pal.accent } else { pal.well });
                        w::text_in(
                            ui,
                            brect,
                            &format!("→{}", ch.label()),
                            font::tiny(),
                            if hot { pal.on_solid() } else { pal.dim },
                            Align2::CENTER_CENTER,
                        );
                        if hot {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if br.clicked() {
                            add = Some((ch, ZoneId::new(z.zone_type, &z.id)));
                        }
                        bx -= bw + 4.0;
                    }

                    if resp.clicked() {
                        cx.ui_state.sel_avatar = i;
                    }
                }
                if shown == 0 {
                    w::empty_state(ui, pal, "nothing matches that filter");
                }
            });

        if let Some((ch, id)) = add {
            cx.send(Command::AddZone(ch, id));
        }
    });
}

fn calibration(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let gap = space::SHEET_GAP;
    let col_w = (ui.available_width() - gap) * 0.5;

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for ch in Channel::ALL {
            w::column(ui, col_w, |ui| {
                let i = cx.ui_state.sel_zone_for(ch);
                let entry = cx.snap.channel(ch).zones.get(i).cloned();

                w::sheet(ui, pal, |ui| {
                    match &entry {
                        Some(e) => w::heading_with_suffix(
                            ui,
                            pal,
                            &format!("Calibrate {}", ch.label()),
                            Some(&e.id.to_string()),
                        ),
                        None => {
                            w::heading(ui, pal, &format!("Calibrate {}", ch.label()));
                        }
                    }

                    let Some(e) = entry else {
                        w::empty_state(ui, pal, "select a mapped zone to tune it");
                        return;
                    };

                    ui.horizontal(|ui| {
                        w::field_label(ui, pal, "Mode", 92.0, true);
                        let labels: Vec<String> = MODES.iter().map(|m| m.to_string()).collect();
                        let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
                        let cur = MODES.iter().position(|m| *m == e.mode).unwrap_or(0);
                        if let Some(p) = w::segmented(ui, pal, &refs, cur) {
                            cx.send(Command::SetZoneMode(ch, i, MODES[p]));
                        }
                    });
                    w::dim(ui, pal, mode_hint(e.mode));
                    ui.add_space(space::ROW_GAP);

                    slider_row(
                        ui,
                        cx,
                        "Scale",
                        e.scale as f32,
                        0.0,
                        100.0,
                        &format!("{}%", e.scale),
                        "Trims this zone's contribution to the channel",
                        |v| Command::SetZoneScale(ch, i, v.round() as u8),
                    );

                    slider_row(
                        ui,
                        cx,
                        "Min thr",
                        e.min_threshold as f32,
                        1.0,
                        100.0,
                        &format!("{:.2}", e.min_threshold as f32 / 100.0),
                        "Contact shallower than this reads as no contact",
                        |v| Command::SetZoneThresholdMin(ch, i, v.round() as u8),
                    );
                    slider_row(
                        ui,
                        cx,
                        "Max thr",
                        e.max_threshold as f32,
                        1.0,
                        100.0,
                        &format!("{:.2}", e.max_threshold as f32 / 100.0),
                        "Contact deeper than this counts as fully engaged",
                        |v| Command::SetZoneThresholdMax(ch, i, v.round() as u8),
                    );

                    ui.add_space(space::ROW_GAP);
                    ui.horizontal(|ui| {
                        if w::button(ui, pal, "Reset calibration").clicked() {
                            cx.send(Command::SetZoneScale(ch, i, 100));
                            cx.send(Command::SetZoneThresholdMin(ch, i, ZoneEntry::THRESHOLD_MIN));
                            cx.send(Command::SetZoneThresholdMax(ch, i, ZoneEntry::THRESHOLD_MAX));
                        }
                        if w::danger_button(ui, pal, "Remove zone", true).clicked() {
                            cx.send(Command::RemoveZone(ch, i));
                        }
                    });
                });
            });
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn slider_row(
    ui: &mut Ui,
    cx: &mut Ctx,
    label: &str,
    value: f32,
    min: f32,
    max: f32,
    display: &str,
    hint: &str,
    to_cmd: impl Fn(f32) -> Command,
) {
    let pal = cx.pal;
    let changed = w::slider_row(ui, pal, label, 92.0, w::Slider::new(value, min, max), display, 56.0, None);
    if let Some(v) = changed {
        cx.send(to_cmd(v));
    }
    let _ = hint;
}

fn mode_hint(m: ContactMode) -> &'static str {
    match m {
        ContactMode::Depth => "Output follows how deep the contact is.",
        ContactMode::Speed => "Output follows how fast the contact moves.",
        ContactMode::Acc => "Output follows how sharply the motion changes.",
        ContactMode::Recoil => "Output fires on pull-out rather than push-in.",
    }
}
