use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use shocking_vrc_core::cli::{AlarmChannels, AlarmPhase};

use crate::tui::app::{
    alarm_control_row, alarm_controls, format_secs, Action, AlarmControl, AlarmField, App,
    ALARM_PATTERN_FIELDS, ALARM_SCHEDULE_FIELDS,
};
use crate::tui::theme;
use crate::tui::ui;

const SCHEDULE_ROWS: u16 = 4;
const PATTERN_ROWS: u16 = ALARM_PATTERN_FIELDS.len() as u16 + 1;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(5)]).split(area);

    let viewport_h = rows[0].height.saturating_sub(2);
    app.alarm_viewport_h = viewport_h;
    app.alarm_scroll = ui::clamp_scroll(app.alarm_scroll, SCHEDULE_ROWS.max(PATTERN_ROWS), viewport_h);
    if let Some(row) = alarm_controls()
        .get(app.alarm_focus)
        .copied()
        .and_then(alarm_control_row)
    {
        app.alarm_scroll = ui::scroll_to_row(app.alarm_scroll, viewport_h, row);
    }

    let focus = alarm_controls().get(app.alarm_focus).copied();
    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);
    render_schedule(app, frame, cols[0], focus);
    render_pattern(app, frame, cols[1], focus);
    render_state(app, frame, rows[1], focus);
}

fn field_row(
    app: &mut App,
    frame: &mut Frame,
    rect: Rect,
    field: AlarmField,
    focus: Option<AlarmControl>,
) {
    let value = field.value_label(app.alarm_config());
    ui::stepper_row(
        frame,
        app,
        rect,
        &format!("{:<14} {value}", field.label()),
        focus == Some(AlarmControl::Field(field)),
        Action::StepAlarmField(field, -1),
        Action::StepAlarmField(field, 1),
    );
}

fn render_schedule(app: &mut App, frame: &mut Frame, area: Rect, focus: Option<AlarmControl>) {
    let block = theme::panel_block("Alarm — all devices in this session");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    ui::clear_panel_inner(frame, inner);

    let scroll = app.alarm_scroll;
    let enabled = app.alarm_config().enabled;

    if let Some(rect) = ui::scrolled_row_rect(inner, scroll, 0, 1) {
        let cols = Layout::horizontal([
            Constraint::Length(15),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Min(0),
        ])
        .split(rect);
        frame.render_widget(
            Paragraph::new("Alarm")
                .style(theme::label_style(focus == Some(AlarmControl::Enabled))),
            cols[0],
        );
        ui::choice_button(
            frame,
            app,
            cols[1],
            "ON",
            enabled,
            focus == Some(AlarmControl::Enabled),
            Action::SetAlarmEnabled(true),
        );
        ui::choice_button(
            frame,
            app,
            cols[2],
            "OFF",
            !enabled,
            focus == Some(AlarmControl::Enabled),
            Action::SetAlarmEnabled(false),
        );
        let time = app.alarm_config().time_label();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  rings at ", Style::default().fg(theme::TEXT_DIM)),
                Span::styled(
                    time,
                    Style::default()
                        .fg(if enabled { theme::HIGHLIGHT } else { theme::TEXT_DIM })
                        .add_modifier(Modifier::BOLD),
                ),
            ])),
            cols[3],
        );
    }

    for (i, field) in ALARM_SCHEDULE_FIELDS.into_iter().enumerate() {
        if let Some(rect) = ui::scrolled_row_rect(inner, scroll, 1 + i as u16, 1) {
            field_row(app, frame, rect, field, focus);
        }
    }

    if let Some(rect) = ui::scrolled_row_rect(inner, scroll, 3, 1) {
        let cols = Layout::horizontal([
            Constraint::Length(15),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Min(0),
        ])
        .split(rect);
        frame.render_widget(
            Paragraph::new("Channel")
                .style(theme::label_style(focus == Some(AlarmControl::Channels))),
            cols[0],
        );
        let current = app.alarm_config().channels;
        for (i, mode) in AlarmChannels::ALL.into_iter().enumerate() {
            ui::choice_button(
                frame,
                app,
                cols[i + 1],
                mode.label(),
                current == mode,
                focus == Some(AlarmControl::Channels),
                Action::SetAlarmChannels(mode),
            );
        }
    }
}

fn render_pattern(app: &mut App, frame: &mut Frame, area: Rect, focus: Option<AlarmControl>) {
    let block = theme::panel_block("Wake pattern");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    ui::clear_panel_inner(frame, inner);

    let scroll = app.alarm_scroll;
    let mut fields = ALARM_PATTERN_FIELDS.to_vec();
    fields.push(AlarmField::Snooze);
    for (i, field) in fields.into_iter().enumerate() {
        let Some(rect) = ui::scrolled_row_rect(inner, scroll, i as u16, 1) else {
            continue;
        };
        field_row(app, frame, rect, field, focus);
    }
}

fn render_state(app: &mut App, frame: &mut Frame, area: Rect, focus: Option<AlarmControl>) {
    let block = theme::panel_block("Now");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    ui::clear_panel_inner(frame, inner);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    frame.render_widget(Paragraph::new(state_line(app)), rows[0]);
    frame.render_widget(
        Paragraph::new(limit_line(app)).style(Style::default().fg(theme::TEXT_DIM)),
        rows[1],
    );

    let cols = Layout::horizontal([
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Min(0),
    ])
    .split(rows[2]);
    ui::button(
        frame,
        app,
        cols[0],
        "Test",
        focus == Some(AlarmControl::Test),
        Action::AlarmTest,
    );
    ui::button(
        frame,
        app,
        cols[1],
        "Stop",
        focus == Some(AlarmControl::Stop),
        Action::AlarmStop,
    );
    ui::button(
        frame,
        app,
        cols[2],
        "Snooze",
        focus == Some(AlarmControl::Snooze),
        Action::AlarmSnooze,
    );
    frame.render_widget(
        Paragraph::new("  T test  ·  X stop  ·  S snooze")
            .style(theme::hint_style()),
        cols[3],
    );
}

fn state_line(app: &App) -> Line<'static> {
    let st = app.alarm_status;
    let cfg = app.alarm_config();
    let attempts = format!("attempt {}/{}", st.attempt.max(1), cfg.repeats);
    match st.phase {
        AlarmPhase::Ringing => Line::from(vec![
            Span::styled(
                if st.test { " TEST RING " } else { " RINGING " },
                Style::default()
                    .fg(ratatui::style::Color::Black)
                    .bg(theme::ERROR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  power {}  ·  {} elapsed  ·  {attempts} ends in {}",
                    st.strength,
                    format_secs(st.elapsed_secs as i32),
                    format_secs(st.auto_stop_in_secs as i32)
                ),
                Style::default().fg(theme::TEXT),
            ),
        ]),
        AlarmPhase::Snoozed => Line::from(vec![
            Span::styled(
                if st.retrying { " RETRYING " } else { " SNOOZED " },
                Style::default()
                    .fg(ratatui::style::Color::Black)
                    .bg(theme::WARNING)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  rings again in {}  ·  {attempts}",
                    format_secs(st.snooze_left_secs as i32)
                ),
                Style::default().fg(theme::TEXT),
            ),
        ]),
        AlarmPhase::Idle if !cfg.enabled => Line::from(Span::styled(
            "Alarm is off — turn it ON to arm it",
            Style::default().fg(theme::TEXT_DIM),
        )),
        AlarmPhase::Idle => Line::from(vec![
            Span::styled("Next ring ", Style::default().fg(theme::TEXT_DIM)),
            Span::styled(
                cfg.time_label(),
                Style::default()
                    .fg(theme::HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  —  in {}", format_mins(st.next_fire_in_mins)),
                Style::default().fg(theme::TEXT),
            ),
        ]),
    }
}

fn limit_line(app: &App) -> String {
    let cfg = app.alarm_config();
    let retries = if cfg.repeats > 1 {
        format!("  ·  retries every {} min", cfg.snooze_mins)
    } else {
        String::new()
    };
    let devices = match app.device_infos.len() {
        0 => "no device yet".to_string(),
        1 => "1 device".to_string(),
        n => format!("all {n} devices"),
    };
    format!(
        "Channel {}  ·  {devices}  ·  own ceiling {} — channel limits do not apply{retries}",
        cfg.channels.label(),
        cfg.peak_strength
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex, RwLock};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use shocking_vrc_core::AvatarScanner;
    use shocking_vrc_core::cli::{AlarmConfig, AlarmController, CliConfig};

    use crate::app_state::AppState;
    use crate::tui::app::{Action, App, ClickKind, Tab};

    fn test_app() -> App {
        let alarm = AlarmController::new(AlarmConfig {
            enabled: true,
            repeats: 3,
            ..AlarmConfig::default()
        });
        let state = Arc::new(AppState {
            scanner: AvatarScanner::new(None),
            monitor_enabled: Arc::new(AtomicBool::new(false)),
            default_config: CliConfig::default(),
            device_slots: Arc::new(RwLock::new(Vec::new())),
            alarm,
        });
        let mut app = App::new(state, Arc::new(Mutex::new(Default::default())));
        app.tutorial_active = false;
        app.config = CliConfig::default();
        app.alarm_tab_visible = true;
        app.active_tab = Tab::Alarm;
        app
    }

    fn draw_at(app: &mut App, w: u16, h: u16) {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|frame| crate::tui::ui::draw(frame, app))
            .unwrap();
    }

    fn draw_to_text(app: &mut App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|frame| crate::tui::ui::draw(frame, app))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[tokio::test]
    async fn hidden_alarm_tab_cannot_be_opened() {
        let mut app = test_app();
        app.alarm_tab_visible = false;
        app.active_tab = Tab::Status;

        app.handle_key(key(KeyCode::Char('9'))).await;
        assert_eq!(app.active_tab, Tab::Status, "number key must not open it");

        app.apply(Action::SwitchTab(Tab::Alarm)).await;
        assert_eq!(app.active_tab, Tab::Status, "a stray click must not open it");

        // Tab-cycling forward from the last visible tab wraps past Alarm.
        app.active_tab = Tab::Setup;
        app.handle_key(key(KeyCode::Tab)).await;
        assert_eq!(app.active_tab, Tab::Status, "cycling must skip it");

        let screen = draw_to_text(&mut app, 120, 30);
        assert!(!screen.contains("9 Alarm"), "no chip in the tab bar");
        assert!(
            !app.clickables.iter().any(|c| matches!(
                &c.kind,
                ClickKind::Act(Action::SwitchTab(Tab::Alarm))
            )),
            "no clickable target for a hidden tab"
        );
    }

    #[tokio::test]
    async fn enabling_it_in_setup_makes_the_tab_reachable() {
        let mut app = test_app();
        app.alarm_tab_visible = false;
        app.active_tab = Tab::Status;

        app.alarm_tab_visible = true;
        app.handle_key(key(KeyCode::Char('9'))).await;
        assert_eq!(app.active_tab, Tab::Alarm);
        assert!(draw_to_text(&mut app, 120, 30).contains("9 Alarm"));
    }

    /// A tab you cannot reach must not be able to wake you at 7 a.m.
    #[tokio::test]
    async fn hiding_the_tab_disarms_the_alarm_and_stops_a_ring() {
        let mut app = test_app();
        app.state.alarm.test();
        app.poll_alarm();
        assert!(app.alarm_ringing());

        app.apply(Action::SetAlarmTabVisible(false)).await;

        assert!(!app.alarm_tab_visible);
        assert!(!app.alarm_ringing(), "the ring is silenced");
        assert!(!app.alarm.enabled, "the schedule is disarmed");
        assert_ne!(app.active_tab, Tab::Alarm, "and we are moved off the tab");
    }

    /// Cramped terminals must clip, not panic — this tab can appear at 3 a.m.
    #[tokio::test]
    async fn alarm_tab_renders_at_any_size() {
        let mut app = test_app();
        for (w, h) in [(120, 40), (80, 24), (60, 16), (30, 10), (20, 6)] {
            draw_at(&mut app, w, h);
        }
    }

    #[tokio::test]
    async fn ringing_overlay_renders_at_any_size() {
        let mut app = test_app();
        app.alarm_status.phase = shocking_vrc_core::AlarmPhase::Ringing;
        app.alarm_status.strength = 18;
        app.alarm_status.elapsed_secs = 42;
        app.alarm_status.auto_stop_in_secs = 258;
        app.alarm_status.attempt = 2;
        for (w, h) in [(120, 40), (80, 24), (40, 12), (20, 6)] {
            draw_at(&mut app, w, h);
        }
        assert!(app.alarm_ringing());
    }

    #[tokio::test]
    async fn every_alarm_control_is_reachable_by_keyboard() {
        use crate::tui::app::alarm_controls;
        let mut app = test_app();
        let controls = alarm_controls();
        for i in 0..controls.len() {
            app.alarm_focus = i;
            draw_at(&mut app, 100, 30);
        }
    }
}

fn format_mins(mins: u32) -> String {
    if mins == 0 {
        "less than a minute".to_string()
    } else if mins < 60 {
        format!("{mins} min")
    } else {
        let h = mins / 60;
        let m = mins % 60;
        if m == 0 {
            format!("{h} h")
        } else {
            format!("{h} h {m} min")
        }
    }
}
