use egui::Context;

use crate::app::Ctx;
use crate::bridge::Command;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ctx: &Context, cx: &mut Ctx) {
    let Some(mut dialog) = cx.ui_state.preset_save.clone() else {
        return;
    };
    let pal = cx.pal;
    let cfg = cx.snap.channel(dialog.channel).clone();

    let mut commit = false;
    let mut cancel = false;

    let dismissed = super::modal(
        ctx,
        pal,
        "preset_save",
        &format!("Save channel {}", dialog.channel.label()),
        400.0,
        |ui| {
            w::dim(
                ui,
                pal,
                "Stores this channel's frequency, intensity and modulation in presets/user/.",
            );
            ui.add_space(space::ITEM_GAP);

            ui.horizontal(|ui| {
                w::field_label(ui, pal, "Name", 72.0, true);
                let r = w::text_field(ui, pal, &mut dialog.name, "(random)", 280.0);
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    commit = true;
                }
            });
            ui.horizontal(|ui| {
                w::field_label(ui, pal, "Author", 72.0, true);
                w::text_field(ui, pal, &mut dialog.author, "(no author)", 280.0);
            });

            ui.add_space(space::ITEM_GAP);
            w::caption(ui, pal, "What gets saved");
            ui.add_space(2.0);
            let mods = cfg
                .freq_modulation
                .iter()
                .chain(cfg.intensity_modulation.iter())
                .filter(|m| m.is_some())
                .count();
            for line in [
                format!("Frequency  {:?}", cfg.frequency),
                format!("Intensity  {:?}", cfg.intensity),
                format!("Modulation {mods} slot(s)"),
            ] {
                ui.label(egui::RichText::new(line).font(font::small()).color(pal.dim));
            }
            ui.add_space(2.0);
            w::dim(ui, pal, "Power limits and zones are not part of a preset.");

            ui.add_space(space::ITEM_GAP);
            ui.horizontal(|ui| {
                if w::cta(ui, pal, "Save").clicked() {
                    commit = true;
                }
                if w::button(ui, pal, "Cancel").clicked() {
                    cancel = true;
                }
            });
        },
    );

    if dismissed {
        cancel = true;
    }

    if commit {
        let name = if dialog.name.trim().is_empty() {
            shocking_vrc_app::presets::random_preset_name(dialog.channel.label())
        } else {
            dialog.name.trim().to_string()
        };
        let author = dialog.author.trim().to_string();
        cx.ui_state.nickname = author.clone();
        if let Err(e) = shocking_vrc_app::prefs::update_ui_prefs(|p| p.nickname = author.clone()) {
            log::warn!("[prefs] Failed to save nickname: {e}");
        }
        cx.bridge.send(Command::SaveUserPreset {
            channel: dialog.channel,
            name,
            author,
        });
        cx.ui_state.preset_save = None;
    } else if cancel {
        cx.ui_state.preset_save = None;
    } else {
        cx.ui_state.preset_save = Some(dialog);
    }
}
