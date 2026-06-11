pub mod sandbox;
pub mod steps;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

use crate::tui::app::{Action, App};
use crate::tui::theme;
use crate::tui::ui;

use steps::TutorialStep;

const GUIDE_BG: Color = Color::Rgb(16, 20, 30);
const GUIDE_BORDER: Color = Color::Rgb(100, 180, 255);
const GUIDE_TITLE: Color = Color::Rgb(100, 180, 255);
const STEP_INDICATOR: Color = Color::Rgb(80, 140, 200);

pub fn render_overlay(app: &mut App, frame: &mut Frame) {
    let step = app.tutorial_step;
    let area = frame.area();

    if let Some(tab) = step.tab() {
        if app.active_tab != tab {
            app.active_tab = tab;
        }
    }

    let panel_h = guide_height(step).min(area.height.saturating_sub(4));
    let panel_w = (area.width * 80 / 100).min(76).max(40);

    let y = area.height.saturating_sub(panel_h + 1);
    let x = (area.width.saturating_sub(panel_w)) / 2;

    let panel = Rect {
        x,
        y,
        width: panel_w,
        height: panel_h,
    };

    frame.render_widget(Clear, panel);

    let title = format!("  {}  ", step.title());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(GUIDE_BORDER))
        .title(title)
        .title_style(
            Style::default()
                .fg(GUIDE_TITLE)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(GUIDE_BG));
    let inner = block.inner(panel);
    frame.render_widget(block, panel);

    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);

    let body_lines: Vec<Line> = step
        .body()
        .iter()
        .map(|l| {
            if l.starts_with("**") && l.ends_with("**") {
                Line::from(Span::styled(
                    l.trim_matches('*').trim(),
                    Style::default()
                        .fg(theme::HIGHLIGHT)
                        .add_modifier(Modifier::BOLD),
                ))
            } else if l.starts_with("  ") {
                Line::from(Span::styled(*l, Style::default().fg(theme::TEXT)))
            } else if l.is_empty() {
                Line::from("")
            } else {
                Line::from(Span::styled(*l, Style::default().fg(theme::TEXT)))
            }
        })
        .collect();

    frame.render_widget(
        Paragraph::new(body_lines)
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(GUIDE_BG)),
        rows[0],
    );

    render_nav(app, frame, rows[1], step);
}

fn render_nav(app: &mut App, frame: &mut Frame, area: Rect, step: TutorialStep) {
    let is_first = step == TutorialStep::ALL[0];
    let is_last = step == *TutorialStep::ALL.last().unwrap();

    let total = TutorialStep::ALL.len();
    let current = step.index() + 1;
    let progress = format!("{current}/{total}");

    let cols = Layout::horizontal([
        Constraint::Length(10),
        Constraint::Min(0),
        Constraint::Length(10),
        Constraint::Length(1),
        Constraint::Length(10),
    ])
    .split(area);

    if !is_first {
        ui::button(frame, app, cols[0], "Back", false, Action::TutorialPrev);
    }

    frame.render_widget(
        Paragraph::new(progress)
            .alignment(Alignment::Center)
            .style(Style::default().fg(STEP_INDICATOR)),
        cols[1],
    );

    if is_last {
        ui::button(frame, app, cols[2], "Finish", false, Action::TutorialClose);
    } else {
        ui::button(frame, app, cols[2], "Next", false, Action::TutorialNext);
    }

    ui::button(frame, app, cols[4], "Skip", false, Action::TutorialClose);
}

fn guide_height(step: TutorialStep) -> u16 {
    let body = step.body().len() as u16;
    body + 4
}
