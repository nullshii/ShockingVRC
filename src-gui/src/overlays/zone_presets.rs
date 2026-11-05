use egui::{Align2, Context, Rect, vec2};

use crate::app::Ctx;
use crate::bridge::Command;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ctx: &Context, cx: &mut Ctx) {
    let pal = cx.pal;
    let entries = cx
        .bridge
        .shared
        .zone_presets
        .read()
        .map(|z| z.entries.clone())
        .unwrap_or_default();

    let mut close = false;
    let mut apply = None;
    let mut delete = None;

    let dismissed = super::modal(ctx, pal, "zone_presets", "Zone sets", 520.0, |ui| {
        w::dim(
            ui,
            pal,
            "A zone set stores which contacts are mapped to each channel, with their \
             modes and calibration. Loading one replaces the current mapping and leaves \
             the waveform alone.",
        );
        ui.add_space(space::ITEM_GAP);

        ui.horizontal(|ui| {
            let mut name = cx.ui_state.zone_preset_save.clone().unwrap_or_default();
            w::field_label(ui, pal, "New set", 68.0, true);
            let r = w::text_field(ui, pal, &mut name, "(auto-named)", 250.0);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            cx.ui_state.zone_preset_save = Some(name.clone());

            let (a, b) = (cx.snap.config.channel_a.zones.len(), cx.snap.config.channel_b.zones.len());
            if (w::cta(ui, pal, "Save current").clicked() || enter) && cx.snap.has_device() {
                let final_name = if name.trim().is_empty() {
                    shocking_vrc_app::zone_presets::default_zone_set_name()
                } else {
                    name.trim().to_string()
                };
                cx.bridge.send(Command::SaveZonePreset(final_name));
                cx.ui_state.zone_preset_save = Some(String::new());
            }
            ui.add_space(4.0);
            w::dim(ui, pal, format!("A: {a}, B: {b}"));
        });

        ui.add_space(space::ITEM_GAP);
        w::dashed_hr(ui, pal.hair);
        ui.add_space(space::ITEM_GAP);

        if entries.is_empty() {
            w::empty_state(ui, pal, "no saved sets yet");
        } else {
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .auto_shrink([false, true])
                .id_salt("zone_set_list")
                .show(ui, |ui| {
                    for (i, e) in entries.iter().enumerate() {
                        let selected = i == cx.ui_state.sel_zone_preset;
                        let (rect, resp) = w::row(ui, pal, selected, 34.0);
                        let inner = rect.shrink2(vec2(8.0, 3.0));
                        let (a, b) = e.preset.counts();

                        w::text_in(
                            ui,
                            Rect::from_min_size(inner.min, vec2(inner.width() - 150.0, 15.0)),
                            e.name(),
                            font::body(),
                            if selected { pal.accent } else { pal.contrast },
                            Align2::LEFT_CENTER,
                        );
                        w::text_in(
                            ui,
                            Rect::from_min_size(inner.min + vec2(0.0, 15.0), vec2(inner.width() - 150.0, 13.0)),
                            &format!("A: {a} zone(s) · B: {b} zone(s)"),
                            font::tiny(),
                            pal.dim,
                            Align2::LEFT_CENTER,
                        );

                        if resp.clicked() {
                            cx.ui_state.sel_zone_preset = i;
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Load this set").clicked() {
                                apply = Some(i);
                                ui.close();
                            }
                            if ui.button("Delete").clicked() {
                                delete = Some(i);
                                ui.close();
                            }
                        });
                    }
                });
        }

        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            let any = !entries.is_empty();
            if w::cta(ui, pal, "Load selected").clicked() && any && cx.snap.has_device() {
                apply = Some(cx.ui_state.sel_zone_preset);
            }
            if w::danger_button(ui, pal, "Delete", any).clicked() {
                delete = Some(cx.ui_state.sel_zone_preset);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::button(ui, pal, "Close").clicked() {
                    close = true;
                }
            });
        });

        if !cx.snap.has_device() {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Connect a Coyote before loading or saving a set.")
                    .font(font::tiny())
                    .color(pal.warning),
            );
        }
    });

    if dismissed {
        close = true;
    }

    if let Some(i) = apply {
        cx.bridge.send(Command::ApplyZonePreset(i));
        cx.ui_state.show_zone_presets = false;
    }
    if let Some(i) = delete {
        cx.ui_state.zone_preset_delete_confirm = Some(i);
    }
    if close {
        cx.ui_state.show_zone_presets = false;
    }
}
