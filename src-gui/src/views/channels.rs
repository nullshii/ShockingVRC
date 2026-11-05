use egui::{Align2, Rect, Sense, Ui, vec2};

use shocking_vrc_core::cli::AggregationMode;
use shocking_vrc_core::raw_to_hz;

use crate::app::Ctx;
use crate::bridge::Command;
use crate::model::Channel;
use crate::theme::widgets::{self as w, font, space};

const AGGREGATIONS: [AggregationMode; 3] = [AggregationMode::Max, AggregationMode::Sum, AggregationMode::Average];

pub fn show(ui: &mut Ui, cx: &mut Ctx) {
    let gap = space::SHEET_GAP;
    let col_w = (ui.available_width() - gap) * 0.5;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for ch in Channel::ALL {
            w::column(ui, col_w, |ui| channel(ui, cx, ch));
        }
    });

    ui.add_space(space::SHEET_GAP);
    settings_file(ui, cx);
}

fn channel(ui: &mut Ui, cx: &mut Ctx, ch: Channel) {
    let pal = cx.pal;
    let cfg = cx.snap.channel(ch).clone();
    let live = cx.snap.channel_status(ch).clone();

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(
            ui,
            pal,
            &format!("Channel {}", ch.label()),
            Some(&format!("ceiling {}", cfg.limits.max)),
        );

        w::caption(ui, pal, "Waveform");
        ui.add_space(2.0);
        waveform_preview(ui, cx, ch);
        ui.add_space(space::ITEM_GAP);

        w::caption(ui, pal, "Frequency — Hz per segment");
        ui.add_space(2.0);
        for seg in 0..4 {
            let raw = cfg.frequency[seg];
            let modulated = live.frequency[seg] != raw && live.frequency[seg] != 0;
            if let Some(v) = w::slider_row(
                ui,
                pal,
                &format!("Seg {seg}"),
                52.0,
                w::Slider::new(raw as f32, 10.0, 255.0).inverted(true),
                &format!("{:.0} Hz", raw_to_hz(raw)),
                66.0,
                Some(if modulated { pal.accent } else { pal.contrast }),
            ) {
                cx.send(Command::SetFrequency(ch, seg, v.round() as u8));
            }
        }
        ui.add_space(space::ITEM_GAP);

        w::caption(ui, pal, "Intensity — amplitude per segment");
        ui.add_space(2.0);
        for seg in 0..4 {
            let v0 = cfg.intensity[seg];
            if let Some(v) = w::slider_row(
                ui,
                pal,
                &format!("Seg {seg}"),
                52.0,
                w::Slider::new(v0 as f32, 0.0, 100.0),
                &format!("{v0}%"),
                66.0,
                None,
            ) {
                cx.send(Command::SetIntensity(ch, seg, v.round() as u8));
            }
        }
        ui.add_space(space::ITEM_GAP);

        w::caption(ui, pal, "Power limits");
        ui.add_space(2.0);
        if let Some(v) = w::slider_row(
            ui,
            pal,
            "Min",
            52.0,
            w::Slider::new(cfg.limits.min as f32, 0.0, 200.0),
            &cfg.limits.min.to_string(),
            66.0,
            None,
        ) {
            cx.send(Command::SetLimitMin(ch, v.round() as u8));
        }
        let danger = cfg.limits.max > 80;
        if let Some(v) = w::slider_row(
            ui,
            pal,
            "Max",
            52.0,
            w::Slider::new(cfg.limits.max as f32, 0.0, 200.0).fill(if danger { pal.error } else { pal.accent }),
            &cfg.limits.max.to_string(),
            66.0,
            Some(if danger { pal.error } else { pal.contrast }),
        ) {
            cx.send(Command::SetLimitMax(ch, v.round() as u8));
        }
        ui.add_space(2.0);
        if danger {
            ui.label(
                egui::RichText::new(
                    "High ceiling — raise this only in small steps, and only if you know how it feels.",
                )
                .font(font::tiny())
                .color(pal.error),
            );
        } else {
            w::dim(ui, pal, "The ceiling is your safety net. Presets never change it.");
        }
        ui.add_space(space::ITEM_GAP);

        w::caption(ui, pal, "Zone mix");
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let cur = AGGREGATIONS.iter().position(|m| *m == cfg.aggregation).unwrap_or(0);
            if let Some(p) = w::segmented(ui, pal, &["Max", "Sum", "Average"], cur) {
                cx.send(Command::SetAggregation(ch, AGGREGATIONS[p]));
            }
        });
        w::dim(
            ui,
            pal,
            match cfg.aggregation {
                AggregationMode::Max => "Strongest active zone wins.",
                AggregationMode::Sum => "All active zones add up, capped at 100%.",
                AggregationMode::Average => "Mean of every active zone.",
            },
        );
    });
}

fn waveform_preview(ui: &mut Ui, cx: &mut Ctx, ch: Channel) {
    let pal = cx.pal;
    let cfg = cx.snap.channel(ch);
    let live = cx.snap.channel_status(ch);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::hover());
    w::dashed_rect(ui, rect, pal.hair);
    let inner = rect.shrink(5.0);

    let label_h = 12.0;
    let plot = Rect::from_min_max(egui::pos2(inner.left(), inner.top() + label_h), inner.max);

    let periods: [f32; 4] = std::array::from_fn(|i| {
        let hz = raw_to_hz(cfg.frequency[i]).max(1.0);
        1.0 / hz
    });
    let total: f32 = periods.iter().sum::<f32>().max(1e-6);
    let driven = live.strength > 0;
    let fill = if driven { pal.accent } else { pal.faded(pal.accent, 0.5) };

    let mut x = plot.left();
    for (seg, (period, intensity)) in periods.iter().zip(cfg.intensity.iter()).enumerate() {
        let width = plot.width() * (period / total);
        let amp = (*intensity as f32 / 100.0).clamp(0.0, 1.0);
        let h = plot.height() * amp;

        let slot = Rect::from_min_size(egui::pos2(x + 1.0, plot.top()), vec2((width - 2.0).max(1.0), plot.height()));
        ui.painter().rect_filled(slot, w::SQUARE, pal.well);
        ui.painter().rect_filled(
            Rect::from_min_size(egui::pos2(slot.left(), plot.bottom() - h), vec2(slot.width(), h)),
            w::SQUARE,
            fill,
        );

        if width > 18.0 {
            w::text_in(
                ui,
                Rect::from_min_size(egui::pos2(x + 3.0, inner.top()), vec2(width, label_h)),
                &format!("s{seg}"),
                font::tiny(),
                pal.dim,
                Align2::LEFT_CENTER,
            );
        }
        x += width;
    }
}

fn settings_file(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Settings file");
        ui.horizontal(|ui| {
            if w::cta(ui, pal, "Save settings")
                .on_hover_text("Write to devices/<mac>.json and cli_config.json")
                .clicked()
            {
                cx.send(Command::SaveConfig);
            }
            if w::button(ui, pal, "Reload from disk")
                .on_hover_text("Discard unsaved changes and reload this device's file")
                .clicked()
            {
                cx.send(Command::LoadConfig);
            }
            ui.add_space(6.0);
            w::dim(
                ui,
                pal,
                if cx.ui_state.auto_save {
                    "Auto-save is on — every change is written as you make it."
                } else {
                    "Auto-save is off — changes live only in memory until you save."
                },
            );
        });
    });
}
