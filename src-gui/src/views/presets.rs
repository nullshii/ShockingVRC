use egui::{Align2, Rect, Ui, vec2};

use shocking_vrc_core::raw_to_hz;

use crate::app::{Ctx, PresetSaveDialog};
use crate::bridge::Command;
use crate::model::Channel;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ui: &mut Ui, cx: &mut Ctx) {
    let gap = space::SHEET_GAP;
    let total = ui.available_width() - gap;
    let list_w = total * 0.58;
    let detail_w = total - list_w;

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        w::column(ui, list_w, |ui| catalog(ui, cx));
        w::column(ui, detail_w, |ui| detail(ui, cx));
    });
}

fn filtered(cx: &Ctx) -> Vec<usize> {
    let state = cx.bridge.shared.presets.read();
    let Ok(state) = state else { return Vec::new() };
    let needle = cx.ui_state.preset_filter.to_lowercase();
    state
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !cx.ui_state.preset_show_mine_only || e.user)
        .filter(|(_, e)| {
            needle.is_empty()
                || e.name.to_lowercase().contains(&needle)
                || e.description.to_lowercase().contains(&needle)
                || e.preset.author.to_lowercase().contains(&needle)
        })
        .map(|(i, _)| i)
        .collect()
}

fn catalog(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let (entries, loading, error, source) = {
        let Ok(s) = cx.bridge.shared.presets.read() else {
            return;
        };
        (s.entries.clone(), s.loading, s.error.clone(), s.source.clone())
    };
    let visible = filtered(cx);

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(
            ui,
            pal,
            "Catalogue",
            Some(&if loading {
                "loading…".to_string()
            } else if entries.is_empty() {
                "empty".to_string()
            } else {
                format!("{} preset(s) · {source}", entries.len())
            }),
        );

        ui.horizontal(|ui| {
            w::text_field(ui, pal, &mut cx.ui_state.preset_filter, "search…", 220.0);
            let mine = cx.ui_state.preset_show_mine_only;
            if let Some(i) = w::segmented(ui, pal, &["All", "Mine"], if mine { 1 } else { 0 }) {
                cx.ui_state.preset_show_mine_only = i == 1;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::button_enabled(ui, pal, "Refresh", !loading)
                    .on_hover_text("Re-fetch the catalogue from GitHub")
                    .clicked()
                {
                    cx.send(Command::RefreshPresets);
                }
            });
        });
        ui.add_space(space::ROW_GAP);

        if loading {
            w::empty_state(ui, pal, "fetching the catalogue…");
            return;
        }
        if let Some(err) = &error {
            ui.label(egui::RichText::new(err).font(font::small()).color(pal.error));
            ui.add_space(4.0);
            w::dim(ui, pal, "Check your connection, or drop .json files into ./presets/.");
            return;
        }
        if visible.is_empty() {
            w::empty_state(
                ui,
                pal,
                if entries.is_empty() {
                    "no presets — press Refresh"
                } else {
                    "nothing matches those filters"
                },
            );
            return;
        }

        let mut delete = None;
        egui::ScrollArea::vertical()
            .max_height(430.0)
            .auto_shrink([false, true])
            .id_salt("preset_list")
            .show(ui, |ui| {
                for &i in &visible {
                    let e = &entries[i];
                    let selected = i == cx.ui_state.sel_preset;
                    let (rect, resp) = w::row(ui, pal, selected, 40.0);
                    let inner = rect.shrink2(vec2(8.0, 4.0));

                    w::text_in(
                        ui,
                        Rect::from_min_size(inner.min, vec2(inner.width() - 90.0, 15.0)),
                        &e.name,
                        font::body(),
                        if selected { pal.accent } else { pal.contrast },
                        Align2::LEFT_CENTER,
                    );
                    let sub = if e.description.is_empty() {
                        byline(e).to_string()
                    } else {
                        format!("{}  ·  {}", byline(e), e.description)
                    };
                    w::text_in(
                        ui,
                        Rect::from_min_size(inner.min + vec2(0.0, 17.0), vec2(inner.width() - 90.0, 13.0)),
                        &sub,
                        font::tiny(),
                        pal.dim,
                        Align2::LEFT_CENTER,
                    );

                    if e.user {
                        let t =
                            Rect::from_min_size(egui::pos2(inner.right() - 40.0, inner.top() + 1.0), vec2(40.0, 15.0));
                        ui.painter().rect_stroke(
                            t,
                            w::SQUARE,
                            egui::Stroke::new(1.0, pal.accent),
                            egui::StrokeKind::Inside,
                        );
                        w::text_in(ui, t, "mine", font::tiny(), pal.accent, Align2::CENTER_CENTER);
                    }

                    if resp.clicked() {
                        cx.ui_state.sel_preset = i;
                    }
                    if e.user {
                        resp.context_menu(|ui| {
                            if ui.button("Delete preset").clicked() {
                                delete = Some(i);
                                ui.close();
                            }
                        });
                    }
                }
            });
        if let Some(i) = delete {
            cx.ui_state.preset_delete_confirm = Some(i);
        }
    });
}

fn byline(e: &shocking_vrc_core::presets::PresetEntry) -> String {
    if e.user && e.preset.author.trim().is_empty() {
        "mine".to_string()
    } else if e.preset.author.trim().is_empty() {
        "official".to_string()
    } else {
        format!("by {}", e.preset.author.trim())
    }
}

fn detail(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let entry = cx
        .bridge
        .shared
        .presets
        .read()
        .ok()
        .and_then(|s| s.entries.get(cx.ui_state.sel_preset).cloned());

    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Preset");

        let Some(e) = entry else {
            w::empty_state(ui, pal, "select a preset to see what it does");
            return;
        };

        ui.label(egui::RichText::new(&e.name).font(font::mono_big()).color(pal.contrast));
        w::dim(ui, pal, byline(&e));
        if !e.description.is_empty() {
            ui.add_space(4.0);
            w::body(ui, pal, e.description.clone());
        }
        ui.add_space(space::ITEM_GAP);

        w::caption(ui, pal, "Frequency");
        ui.add_space(2.0);
        for seg in 0..4 {
            let f = 1.0 - (e.preset.frequency[seg] as f32 - 10.0) / 245.0;
            w::gauge_row(
                ui,
                pal,
                &format!("s{seg}"),
                34.0,
                f,
                pal.accent,
                &format!("{:.0} Hz", raw_to_hz(e.preset.frequency[seg])),
                66.0,
            );
        }
        ui.add_space(space::ROW_GAP);

        w::caption(ui, pal, "Intensity");
        ui.add_space(2.0);
        for seg in 0..4 {
            w::gauge_row(
                ui,
                pal,
                &format!("s{seg}"),
                34.0,
                e.preset.intensity[seg] as f32 / 100.0,
                pal.accent,
                &format!("{}%", e.preset.intensity[seg]),
                66.0,
            );
        }

        let mods = e
            .preset
            .freq_modulation
            .iter()
            .chain(e.preset.intensity_modulation.iter())
            .filter(|m| m.is_some())
            .count();
        ui.add_space(space::ROW_GAP);
        w::dim(
            ui,
            pal,
            if mods == 0 {
                "No modulation in this preset.".to_string()
            } else {
                format!("Carries {mods} modulation slot(s).")
            },
        );

        ui.add_space(space::ITEM_GAP);
        w::dashed_hr(ui, pal.hair);
        ui.add_space(space::ITEM_GAP);

        let has_device = cx.snap.has_device();
        ui.horizontal(|ui| {
            for ch in Channel::ALL {
                if w::cta(ui, pal, &format!("Apply → {}", ch.label()))
                    .on_hover_text("Writes frequency, intensity and modulation. Limits and zones are untouched.")
                    .clicked()
                    && has_device
                {
                    cx.send(Command::ApplyPreset(ch, cx.ui_state.sel_preset));
                }
            }
        });
        ui.add_space(4.0);
        w::dim(ui, pal, "Applying never changes your power limits or zones.");

        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Save current settings");
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            for ch in Channel::ALL {
                if w::button_enabled(ui, pal, &format!("Save {}", ch.label()), has_device)
                    .on_hover_text("Store this channel's waveform as your own preset")
                    .clicked()
                {
                    cx.ui_state.preset_save = Some(PresetSaveDialog {
                        channel: ch,
                        name: String::new(),
                        author: cx.ui_state.nickname.clone(),
                    });
                }
            }
            if e.user && w::danger_button(ui, pal, "Delete", true).clicked() {
                cx.ui_state.preset_delete_confirm = Some(cx.ui_state.sel_preset);
            }
        });
    });
}
