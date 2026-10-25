pub mod about;
pub mod alarm_ring;
pub mod confirm;
pub mod function_picker;
pub mod graph_dialogs;
pub mod preset_save;
pub mod update;
pub mod zone_presets;

use egui::{Context, Ui};

use crate::theme::Palette;
use crate::theme::widgets as w;

pub fn modal<R>(
    ctx: &Context,
    pal: &Palette,
    id: &str,
    title: &str,
    width: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> bool {
    let backdrop = egui::Color32::from_rgba_unmultiplied(
        pal.base.r(),
        pal.base.g(),
        pal.base.b(),
        205,
    );

    let resp = egui::Modal::new(egui::Id::new(id))
        .backdrop_color(backdrop)
        .frame(
            egui::Frame::NONE
                .fill(pal.base)
                .inner_margin(egui::Margin::same(w::space::SHEET_PAD as i8 + 4)),
        )
        .show(ctx, |ui| {
            ui.set_width(width);
            let outer = ui.max_rect().expand(w::space::SHEET_PAD + 4.0);
            w::dashed_rect(ui, outer, pal.hair);
            w::heading(ui, pal, title);
            add(ui)
        });

    resp.backdrop_response.clicked()
}
