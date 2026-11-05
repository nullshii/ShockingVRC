use egui::Context;

use crate::app::Ctx;
use crate::bridge::Command;
use crate::model::format_secs;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ctx: &Context, cx: &mut Ctx) {
    let pal = cx.pal;
    let cfg = cx.snap.alarm.clone();
    let st = cx.snap.alarm_status;

    super::modal(
        ctx,
        pal,
        "alarm_ring",
        if st.test { "Test ring" } else { "Alarm" },
        420.0,
        |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(format!("WAKE UP — {}", cfg.time_label()))
                        .font(font::display())
                        .color(pal.error),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "power {} / {}   ·   ringing for {}",
                        st.strength,
                        cfg.peak_strength,
                        format_secs(st.elapsed_secs as i32)
                    ))
                    .font(font::body())
                    .color(pal.contrast),
                );
            });

            ui.add_space(space::ROW_GAP);
            let peak = cfg.peak_strength.max(1) as f32;
            w::gauge(ui, pal, st.strength as f32 / peak, pal.error, 14.0);
            ui.add_space(space::ROW_GAP);

            ui.vertical_centered(|ui| {
                let tail = if st.attempt < cfg.repeats {
                    format!(
                        "Attempt {}/{} — retries in {} min if you do not answer",
                        st.attempt.max(1),
                        cfg.repeats,
                        cfg.snooze_mins
                    )
                } else {
                    format!(
                        "Attempt {}/{} — gives up in {}",
                        st.attempt.max(1),
                        cfg.repeats,
                        format_secs(st.auto_stop_in_secs as i32)
                    )
                };
                ui.label(egui::RichText::new(tail).font(font::small()).color(pal.dim));

                if cx.snap.devices.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("No device connected — nothing is being driven.")
                            .font(font::tiny())
                            .color(pal.warning),
                    );
                }

                ui.add_space(space::ITEM_GAP);
                ui.horizontal(|ui| {
                    if w::cta(ui, pal, "Stop").clicked() {
                        cx.bridge.send(Command::AlarmStop);
                    }
                    if w::button(ui, pal, "Snooze").clicked() {
                        cx.bridge.send(Command::AlarmSnooze);
                    }
                });
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Enter or Esc — stop   ·   S — snooze")
                        .font(font::tiny())
                        .color(pal.dim),
                );
            });
        },
    );
}
