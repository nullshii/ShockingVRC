use egui::{Align2, Rect, Sense, Ui, vec2};

use crate::app::Ctx;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let lines: Vec<String> = cx
        .log
        .lock()
        .map(|b| b.iter().cloned().collect())
        .unwrap_or_default();

    let filter = cx.ui_state.log_filter.to_lowercase();
    let visible: Vec<&String> = lines
        .iter()
        .filter(|l| cx.ui_state.log_levels.accepts(l))
        .filter(|l| filter.is_empty() || l.to_lowercase().contains(&filter))
        .collect();

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(
            ui,
            pal,
            "Event log",
            Some(&format!("{} / {} lines", visible.len(), lines.len())),
        );

        ui.horizontal(|ui| {
            w::text_field(ui, pal, &mut cx.ui_state.log_filter, "filter…", 240.0);
            ui.add_space(8.0);

            let lv = &mut cx.ui_state.log_levels;
            for (label, flag) in [
                ("ERROR", &mut lv.error),
                ("WARN", &mut lv.warn),
                ("INFO", &mut lv.info),
                ("DEBUG", &mut lv.debug),
            ] {
                let color = if label == "DEBUG" {
                    pal.contrast
                } else {
                    level_color(pal, label)
                };
                let font = font::tiny();
                let gw = ui
                    .painter()
                    .layout_no_wrap(label.to_string(), font.clone(), color)
                    .size()
                    .x;
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(gw + 16.0, 22.0), Sense::click());
                let on = *flag;
                if on {
                    ui.painter().rect_filled(rect, w::SQUARE, pal.faded(color, 0.75));
                }
                ui.painter().rect_stroke(
                    rect,
                    w::SQUARE,
                    egui::Stroke::new(1.0, if on { color } else { pal.hair }),
                    egui::StrokeKind::Inside,
                );
                w::text_in(
                    ui,
                    rect,
                    label,
                    font,
                    if on { color } else { pal.dim },
                    Align2::CENTER_CENTER,
                );
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    *flag = !on;
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::button(ui, pal, "Copy")
                    .on_hover_text("Copy the visible lines to the clipboard")
                    .clicked()
                {
                    let text = visible
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.ctx().copy_text(text);
                    cx.bridge.shared.notify(
                        crate::bridge::ToastKind::Info,
                        format!("Copied {} line(s)", visible.len()),
                    );
                }
                let mut follow = cx.ui_state.log_follow;
                if w::segmented(
                    ui,
                    pal,
                    &["Follow", "Hold"],
                    if follow { 0 } else { 1 },
                )
                .is_some_and(|i| {
                    follow = i == 0;
                    true
                }) {
                    cx.ui_state.log_follow = follow;
                }
            });
        });

        ui.add_space(space::ROW_GAP);

        let mut area = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("log_lines");
        if cx.ui_state.log_follow {
            area = area.stick_to_bottom(true);
        }
        area.show_rows(ui, 17.0, visible.len().max(1), |ui, range| {
            ui.set_min_height(320.0);
            if visible.is_empty() {
                w::empty_state(
                    ui,
                    pal,
                    if lines.is_empty() {
                        "waiting for events…"
                    } else {
                        "nothing matches the current filter"
                    },
                );
                return;
            }
            for line in &visible[range.start.min(visible.len())..range.end.min(visible.len())] {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 17.0), Sense::hover());
                let (tag, rest) = split_tag(line);
                let color = level_color(pal, tag);
                w::text_in(
                    ui,
                    Rect::from_min_size(rect.min, vec2(52.0, rect.height())),
                    tag,
                    font::tiny(),
                    color,
                    Align2::LEFT_CENTER,
                );
                w::text_in(
                    ui,
                    Rect::from_min_max(
                        egui::pos2(rect.left() + 56.0, rect.top()),
                        rect.max,
                    ),
                    rest,
                    font::small(),
                    if tag == "ERROR" || tag == "WARN" {
                        color
                    } else {
                        pal.contrast
                    },
                    Align2::LEFT_CENTER,
                );
            }
        });
    });
}

fn split_tag(line: &str) -> (&str, &str) {
    for tag in ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"] {
        let prefix = format!("[{tag}]");
        if let Some(rest) = line.strip_prefix(&prefix) {
            return (tag, rest.trim_start());
        }
        let padded = format!("[{tag} ]");
        if let Some(rest) = line.strip_prefix(&padded) {
            return (tag, rest.trim_start());
        }
    }
    ("INFO", line)
}

fn level_color(pal: &crate::theme::Palette, tag: &str) -> egui::Color32 {
    match tag {
        "ERROR" => pal.error,
        "WARN" => pal.warning,
        "DEBUG" | "TRACE" => pal.dim,
        _ => pal.accent,
    }
}
