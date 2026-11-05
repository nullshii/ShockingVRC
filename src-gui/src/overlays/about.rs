use egui::Context;

use crate::theme::Palette;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ctx: &Context, pal: &Palette, open: &mut bool) {
    let mut close = false;
    let dismissed = super::modal(ctx, pal, "about", "ShockingVRC", 420.0, |ui| {
        ui.label(
            egui::RichText::new(format!("v{}", shocking_vrc_core::VERSION))
                .font(font::mono_big())
                .color(pal.accent),
        );
        ui.add_space(4.0);
        w::body(ui, pal, "Bridges DG-Lab Coyote hardware and VRChat over OSC.");

        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Files");
        ui.add_space(2.0);
        for (path, what) in [
            ("cli_config.json", "default configuration"),
            ("devices/<mac>.json", "per-device configuration"),
            ("alarm.json", "alarm schedule"),
            ("shockingvrc_prefs.json", "preferences and theme"),
            ("presets/user/", "your waveform presets"),
            ("presets/zones/", "your zone sets"),
        ] {
            ui.horizontal(|ui| {
                w::field_label(ui, pal, path, 180.0, false);
                w::dim(ui, pal, what);
            });
        }

        ui.add_space(space::ITEM_GAP);
        ui.hyperlink_to(
            egui::RichText::new("github.com/nullshii/ShockingVRC")
                .font(font::small())
                .color(pal.accent),
            "https://github.com/nullshii/ShockingVRC",
        );

        ui.add_space(space::ITEM_GAP);
        if w::button(ui, pal, "Close").clicked() {
            close = true;
        }
    });
    if close || dismissed {
        *open = false;
    }
}
