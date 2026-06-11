use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};

use crate::tui::app::{
    tuning_control_row, tuning_controls, Action, App, NormField, TuningControl, UkfField,
};
use crate::tui::theme;
use crate::tui::ui;

const UKF_ROWS: u16 = 6;
const NORM_ROWS: u16 = 4;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let viewport_h = area.height.saturating_sub(2);
    app.tuning_viewport_h = viewport_h;
    app.tuning_scroll = ui::clamp_scroll(
        app.tuning_scroll,
        UKF_ROWS.max(NORM_ROWS),
        viewport_h,
    );
    if let Some(ctrl) = tuning_controls().get(app.tuning_focus).copied() {
        app.tuning_scroll = ui::scroll_to_row(
            app.tuning_scroll,
            viewport_h,
            tuning_control_row(ctrl),
        );
    }

    let focus = tuning_controls().get(app.tuning_focus).copied();
    render_ukf(app, frame, cols[0], focus);
    render_norms(app, frame, cols[1], focus);
}

fn render_ukf(app: &mut App, frame: &mut Frame, area: Rect, focus: Option<TuningControl>) {
    let block = theme::panel_block("UKF — Unscented Kalman Filter");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    ui::clear_panel_inner(frame, inner);

    let p = app.config.ukf;
    let scroll = app.tuning_scroll;
    let fields = [
        (UkfField::Q, "Q (process)", format!("{:.4}", p.q)),
        (UkfField::R, "R (measure)", format!("{:.4}", p.r)),
        (UkfField::Alpha, "Alpha (σ spread)", format!("{:.3}", p.alpha)),
        (UkfField::Beta, "Beta (prior)", format!("{:.3}", p.beta)),
        (UkfField::Kappa, "Kappa", format!("{:.3}", p.kappa)),
    ];

    for (i, (field, name, val)) in fields.into_iter().enumerate() {
        let Some(rect) = ui::scrolled_row_rect(inner, scroll, i as u16, 1) else {
            continue;
        };
        ui::stepper_row(
            frame,
            app,
            rect,
            &format!("{name:<18} {val}"),
            focus == Some(TuningControl::Ukf(field)),
            Action::StepUkf(field, -1),
            Action::StepUkf(field, 1),
        );
    }

    if let Some(rect) = ui::scrolled_row_rect(inner, scroll, 5, 1) {
        ui::button(
            frame,
            app,
            Rect {
                x: rect.x,
                y: rect.y,
                width: 20.min(rect.width),
                height: 1,
            },
            "Reset defaults",
            focus == Some(TuningControl::UkfReset),
            Action::ResetUkf,
        );
    }
}

fn render_norms(app: &mut App, frame: &mut Frame, area: Rect, focus: Option<TuningControl>) {
    let block = theme::panel_block("Normalization — contact sensitivity");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    ui::clear_panel_inner(frame, inner);

    let n = app.config.norms;
    let scroll = app.tuning_scroll;
    let fields = [
        (NormField::Speed, "Speed ÷", format!("{:.3}", n.speed)),
        (NormField::Acc, "Accel ÷", format!("{:.3}", n.acc)),
        (NormField::Recoil, "Recoil ÷", format!("{:.3}", n.recoil)),
    ];

    for (i, (field, name, val)) in fields.into_iter().enumerate() {
        let Some(rect) = ui::scrolled_row_rect(inner, scroll, i as u16, 1) else {
            continue;
        };
        ui::stepper_row(
            frame,
            app,
            rect,
            &format!("{name:<16} {val}"),
            focus == Some(TuningControl::Norm(field)),
            Action::StepNorm(field, -1),
            Action::StepNorm(field, 1),
        );
    }

    if let Some(rect) = ui::scrolled_row_rect(inner, scroll, 3, 1) {
        ui::button(
            frame,
            app,
            Rect {
                x: rect.x,
                y: rect.y,
                width: 20.min(rect.width),
                height: 1,
            },
            "Reset defaults",
            focus == Some(TuningControl::NormReset),
            Action::ResetNorms,
        );
    }
}
