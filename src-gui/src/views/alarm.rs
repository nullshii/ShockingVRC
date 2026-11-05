use egui::{Sense, Ui, vec2};

use shocking_vrc_core::cli::{AlarmChannels, AlarmPhase};

use crate::app::Ctx;
use crate::bridge::Command;
use crate::model::{AlarmField, format_mins, format_secs};
use crate::theme::widgets::{self as w, font, space};

pub fn show(ui: &mut Ui, cx: &mut Ctx) {
    now_panel(ui, cx);
    ui.add_space(space::SHEET_GAP);

    let gap = space::SHEET_GAP;
    let col_w = (ui.available_width() - gap) * 0.5;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        w::column(ui, col_w, |ui| schedule(ui, cx));
        w::column(ui, col_w, |ui| pattern(ui, cx));
    });
}

fn commit(cx: &mut Ctx, edit: impl FnOnce(&mut shocking_vrc_core::cli::AlarmConfig)) {
    let mut cfg = cx.snap.alarm.clone();
    edit(&mut cfg);
    cx.send(Command::SetAlarmConfig(Box::new(cfg)));
}

fn now_panel(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let cfg = cx.snap.alarm.clone();
    let st = cx.snap.alarm_status;

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(
            ui,
            pal,
            "Alarm",
            Some(match st.phase {
                AlarmPhase::Ringing => "ringing",
                AlarmPhase::Snoozed => "snoozed",
                AlarmPhase::Idle if cfg.enabled => "armed",
                AlarmPhase::Idle => "off",
            }),
        );

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(cfg.time_label())
                    .font(font::display())
                    .color(if cfg.enabled { pal.accent } else { pal.dim }),
            );
            ui.add_space(14.0);

            ui.vertical(|ui| {
                ui.add_space(3.0);
                if let Some(next) = w::toggle(ui, pal, cfg.enabled)
                    && next != cfg.enabled
                {
                    commit(cx, |c| c.enabled = next);
                }
                ui.add_space(2.0);
                let line = match st.phase {
                    AlarmPhase::Ringing => format!(
                        "power {} · {} elapsed · attempt {}/{}",
                        st.strength,
                        format_secs(st.elapsed_secs as i32),
                        st.attempt.max(1),
                        cfg.repeats
                    ),
                    AlarmPhase::Snoozed => format!(
                        "rings again in {} · attempt {}/{}",
                        format_secs(st.snooze_left_secs as i32),
                        st.attempt.max(1),
                        cfg.repeats
                    ),
                    AlarmPhase::Idle if cfg.enabled => {
                        format!("next ring in {}", format_mins(st.next_fire_in_mins))
                    }
                    AlarmPhase::Idle => "turn it on to arm the schedule".to_string(),
                };
                ui.label(
                    egui::RichText::new(line)
                        .font(font::small())
                        .color(if st.phase == AlarmPhase::Ringing {
                            pal.error
                        } else {
                            pal.dim
                        }),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::button_enabled(ui, pal, "Snooze", st.phase == AlarmPhase::Ringing).clicked() {
                    cx.send(Command::AlarmSnooze);
                }
                if w::button_enabled(ui, pal, "Stop", st.phase != AlarmPhase::Idle).clicked() {
                    cx.send(Command::AlarmStop);
                }
                if w::button_enabled(ui, pal, "Test ring", st.phase == AlarmPhase::Idle)
                    .on_hover_text("Ring now, without changing the schedule")
                    .clicked()
                {
                    cx.send(Command::AlarmTest);
                }
            });
        });

        if st.phase == AlarmPhase::Ringing {
            ui.add_space(space::ROW_GAP);
            let peak = cfg.peak_strength.max(1) as f32;
            w::gauge(ui, pal, st.strength as f32 / peak, pal.error, 10.0);
        }

        ui.add_space(space::ITEM_GAP);
        let devices = match cx.snap.devices.len() {
            0 => "no device yet".to_string(),
            1 => "1 device".to_string(),
            n => format!("all {n} devices"),
        };
        w::dim(
            ui,
            pal,
            format!(
                "Channel {} · {devices} · own ceiling {} — the channel power limits do not apply here.",
                cfg.channels.label(),
                cfg.peak_strength
            ),
        );
        if cx.snap.devices.is_empty() {
            ui.label(
                egui::RichText::new("No Coyote connected — the alarm will still fire, but nothing will feel it.")
                    .font(font::tiny())
                    .color(pal.warning),
            );
        }
    });
}

fn schedule(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let cfg = cx.snap.alarm.clone();

    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Schedule");

        for f in AlarmField::SCHEDULE {
            let (min, max) = f.range();
            let changed = w::slider_row(
                ui,
                pal,
                f.label(),
                110.0,
                w::Slider::new(f.get(&cfg) as f32, min as f32, max as f32),
                &f.value_label(&cfg),
                84.0,
                None,
            );
            if let Some(v) = changed {
                commit(cx, |c| f.set(c, v.round() as i32));
            }
        }

        ui.add_space(space::ROW_GAP);
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Channel", 110.0, true);
            let all = AlarmChannels::ALL;
            let cur = all.iter().position(|c| *c == cfg.channels).unwrap_or(0);
            let labels: Vec<&str> = all.iter().map(|c| c.label()).collect();
            if let Some(p) = w::segmented(ui, pal, &labels, cur) {
                cx.send(Command::SetAlarmChannels(all[p]));
            }
        });

        ui.add_space(space::ROW_GAP);
        let f = AlarmField::Snooze;
        let (min, max) = f.range();
        let changed = w::slider_row(
            ui,
            pal,
            f.label(),
            110.0,
            w::Slider::new(f.get(&cfg) as f32, min as f32, max as f32),
            &f.value_label(&cfg),
            84.0,
            None,
        );
        if let Some(v) = changed {
            commit(cx, |c| f.set(c, v.round() as i32));
        }
    });
}

fn pattern(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let cfg = cx.snap.alarm.clone();

    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Wake pattern");

        for f in AlarmField::PATTERN {
            let (min, max) = f.range();
            let danger = matches!(f, AlarmField::PeakStrength) && f.get(&cfg) > 80;
            let mut s = w::Slider::new(f.get(&cfg) as f32, min as f32, max as f32);
            if danger {
                s = s.fill(pal.error);
            }
            let changed = w::slider_row(
                ui,
                pal,
                f.label(),
                110.0,
                s,
                &f.value_label(&cfg),
                92.0,
                danger.then_some(pal.error),
            );
            if let Some(v) = changed {
                commit(cx, |c| f.set(c, v.round() as i32));
            }
        }

        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            w::caption(ui, pal, "One attempt");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                w::dim(ui, pal, format_secs(cfg.max_duration_secs as i32));
            });
        });
        ui.add_space(2.0);
        pattern_preview(ui, cx);
        ui.add_space(4.0);
        w::dim(
            ui,
            pal,
            format!(
                "One attempt: climbs from {} to {} over {}, then retries every {} min, {} time(s).",
                cfg.start_strength,
                cfg.peak_strength,
                format_secs(cfg.ramp_secs as i32),
                cfg.snooze_mins,
                cfg.repeats
            ),
        );
    });
}

fn pattern_preview(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let cfg = &cx.snap.alarm;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::hover());
    w::dashed_rect(ui, rect, pal.hair);
    let plot = rect.shrink2(vec2(8.0, 8.0));

    let duration = cfg.max_duration_secs.max(1) as f32;
    let peak = cfg.peak_strength.max(1) as f32;
    let cols = plot.width().max(1.0) as usize;

    for i in 0..cols {
        let t = i as f32 / cols as f32 * duration;
        let elapsed = std::time::Duration::from_secs_f32(t);
        let open = cfg.pulse_open(elapsed);
        if !open {
            continue;
        }
        let strength = cfg.ramp_strength(elapsed) as f32 / peak;
        let h = plot.height() * strength.clamp(0.0, 1.0);
        let x = plot.left() + i as f32;
        ui.painter().rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, plot.bottom() - h), vec2(1.0, h)),
            w::SQUARE,
            pal.error,
        );
    }
}
