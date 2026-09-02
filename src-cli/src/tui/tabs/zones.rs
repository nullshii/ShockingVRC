use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use shocking_vrc_core::cli::ContactMode;

use crate::tui::app::{Action, App, Channel, SliderKind, ZonesPane};
use crate::tui::theme;
use crate::tui::ui;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Min(4), Constraint::Length(1)]).split(area);
    let cols = Layout::horizontal([
        Constraint::Percentage(33),
        Constraint::Percentage(33),
        Constraint::Percentage(34),
    ])
    .split(rows[0]);
    render_configured(app, frame, cols[0], Channel::A);
    render_configured(app, frame, cols[1], Channel::B);
    render_avatar(app, frame, cols[2]);
    render_footer(app, frame, rows[1]);
}

fn render_footer(app: &mut App, frame: &mut Frame, area: Rect) {
    let saved = app.zone_presets.len();
    let cols = Layout::horizontal([Constraint::Length(13), Constraint::Min(0)])
        .split(area);
    ui::button(frame, app, cols[0], "Zone sets", false, Action::OpenZonePresets);
    let hint = match saved {
        0 => "  z — save the current zones as a reusable set".to_string(),
        1 => "  z — 1 saved set".to_string(),
        n => format!("  z — {n} saved sets"),
    };
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(theme::TEXT_DIM)),
        cols[1],
    );
}

fn render_configured(app: &mut App, frame: &mut Frame, area: Rect, ch: Channel) {
    let pane = match ch {
        Channel::A => ZonesPane::ConfiguredA,
        Channel::B => ZonesPane::ConfiguredB,
    };
    let focused = app.zones_pane == pane;
    let block = theme::panel_block(format!("Channel {} — mapped zones", ch.label()))
        .border_style(if focused {
            Style::default().fg(theme::ACCENT)
        } else {
            Style::default().fg(theme::ACCENT_DIM)
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::vertical([
        Constraint::Min(1),    // mapped-zone list
        Constraint::Length(1), // Cycle mode / Remove buttons
        Constraint::Length(1), // Scale slider
        Constraint::Length(1), // Min threshold slider
        Constraint::Length(1), // Max threshold slider
    ])
    .split(inner);
    let list_area = layout[0];
    let btn_row = layout[1];
    let slider_row_area = layout[2];

    let selected = match ch {
        Channel::A => app.sel_conf_a,
        Channel::B => app.sel_conf_b,
    };

    let entries: Vec<(String, u8)> = app
        .channel_config(ch)
        .zones
        .iter()
        .map(|e| {
            let mode_str = match e.mode {
                ContactMode::Depth => "depth",
                ContactMode::Speed => "speed",
                ContactMode::Acc => "acc",
                ContactMode::Recoil => "recoil",
            };
            let scale_badge = if e.scale != 100 {
                format!(" ·{}%", e.scale)
            } else {
                String::new()
            };
            let thr_badge = if e.min_threshold != 1 || e.max_threshold != 100 {
                format!(
                    " ·thr {:.2}-{:.2}",
                    e.min_threshold as f32 / 100.0,
                    e.max_threshold as f32 / 100.0
                )
            } else {
                String::new()
            };
            (
                format!("{} · {}{}{}", e.id, mode_str, scale_badge, thr_badge),
                e.scale,
            )
        })
        .collect();

    render_rows(
        frame,
        app,
        list_area,
        &entries.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>(),
        selected,
        focused,
        &|i| Action::SelectConfigured(ch, i),
    );

    let btns = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(btn_row);
    ui::button(frame, app, btns[0], "Cycle mode", false, Action::CycleMode(ch, selected));
    ui::button(frame, app, btns[1], "Remove", false, Action::RemoveZone(ch, selected));


    let current_scale = entries.get(selected).map(|(_, s)| *s).unwrap_or(100);

    let label_cols = Layout::horizontal([
        Constraint::Length(7),
        Constraint::Min(8),
        Constraint::Length(5),
    ])
    .split(slider_row_area);

    let label_style = if focused {
        Style::default().fg(theme::HIGHLIGHT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };

    frame.render_widget(
        Paragraph::new("Scale ").style(label_style),
        label_cols[0],
    );

    let ratio = current_scale as f64 / 100.0;
    let bar_color = scale_bar_color(current_scale);
    let gauge = ratatui::widgets::Gauge::default()
        .gauge_style(Style::default().fg(bar_color).bg(theme::SURFACE_ELEVATED))
        .ratio(ratio)
        .label("");
    frame.render_widget(gauge, label_cols[1]);
    app.push_slider(label_cols[1], SliderKind::ZoneScale(ch, selected));

    let val_style = if current_scale != 100 {
        Style::default()
            .fg(bar_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{:>3}%", current_scale),
            val_style,
        ))),
        label_cols[2],
    );

    // Threshold-range sliders (stored in hundredths, shown 0.01–1.00).
    let (thr_min, thr_max) = app
        .channel_config(ch)
        .zones
        .get(selected)
        .map(|e| (e.min_threshold, e.max_threshold))
        .unwrap_or((1, 100));

    threshold_row(
        frame,
        app,
        layout[3],
        "Min thr",
        thr_min,
        SliderKind::ZoneThresholdMin(ch, selected),
        focused,
    );
    threshold_row(
        frame,
        app,
        layout[4],
        "Max thr",
        thr_max,
        SliderKind::ZoneThresholdMax(ch, selected),
        focused,
    );
}

/// Render a single threshold slider row: label, gauge (0.01–1.00), and the
/// current value shown as a decimal in real time. Mirrors the Scale gauge so it
/// blends into the existing Zone menu styling.
#[allow(clippy::too_many_arguments)]
fn threshold_row(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    label: &str,
    value: u8,
    kind: SliderKind,
    focused: bool,
) {
    let cols = Layout::horizontal([
        Constraint::Length(10),
        Constraint::Min(6),
        Constraint::Length(5),
    ])
    .split(area);

    let label_style = if focused {
        Style::default().fg(theme::HIGHLIGHT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    frame.render_widget(Paragraph::new(label.to_string()).style(label_style), cols[0]);

    let bar_color = if focused { theme::HIGHLIGHT } else { theme::ACCENT };
    let gauge = ratatui::widgets::Gauge::default()
        .gauge_style(Style::default().fg(bar_color).bg(theme::SURFACE_ELEVATED))
        .ratio((value as f64 / 100.0).clamp(0.0, 1.0))
        .label("");
    frame.render_widget(gauge, cols[1]);
    app.push_slider(cols[1], kind);

    let val_style = Style::default().fg(bar_color).add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{:>4.2}", value as f32 / 100.0),
            val_style,
        ))),
        cols[2],
    );
}

fn scale_bar_color(scale: u8) -> ratatui::style::Color {
    use ratatui::style::Color;
    match scale {
        0 => Color::DarkGray,
        1..=29 => theme::ERROR,
        30..=59 => theme::WARNING,
        60..=89 => Color::Rgb(160, 210, 100),
        90..=99 => Color::Rgb(100, 200, 160),
        100 => theme::ACCENT,
        _ => theme::ACCENT,
    }
}

fn render_avatar(app: &mut App, frame: &mut Frame, area: Rect) {
    let focused = app.zones_pane == ZonesPane::Avatar;
    let block = theme::panel_block("Avatar — discovered touch zones")
        .border_style(if focused {
            Style::default().fg(theme::ACCENT)
        } else {
            Style::default().fg(theme::ACCENT_DIM)
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let layout = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    let rows: Vec<String> = app
        .avatar_zones
        .iter()
        .map(|z| {
            let pct = (z.level * 100.0) as u32;
            let bar = if pct > 0 {
                format!("  {pct:>3}%")
            } else {
                "     ".into()
            };
            format!("{:<6} {}{bar}", z.zone_type, z.id)
        })
        .collect();

    render_rows(
        frame,
        app,
        layout[0],
        &rows,
        app.sel_avatar,
        focused,
        &Action::SelectAvatar,
    );

    let sel = app.sel_avatar;
    let add = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(layout[1]);
    ui::button(frame, app, add[0], "Add → A", false, Action::AddAvatarZone(Channel::A, sel));
    ui::button(frame, app, add[1], "Add → B", false, Action::AddAvatarZone(Channel::B, sel));

    let all = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(layout[2]);
    ui::button(frame, app, all[0], "Add all → A", false, Action::AddAllZones(Channel::A));
    ui::button(frame, app, all[1], "Add all → B", false, Action::AddAllZones(Channel::B));
}

fn render_rows(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    rows: &[String],
    selected: usize,
    focused: bool,
    make_action: &dyn Fn(usize) -> Action,
) {
    if area.height == 0 {
        return;
    }
    let height = area.height as usize;
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("  No zones yet — pick from avatar list →")
                .style(Style::default().fg(theme::TEXT_DIM)),
            area,
        );
        return;
    }
    let offset = selected.saturating_sub(height.saturating_sub(1));
    for (vis, idx) in (offset..rows.len()).enumerate() {
        if vis >= height {
            break;
        }
        let rect = Rect {
            x: area.x,
            y: area.y + vis as u16,
            width: area.width,
            height: 1,
        };
        let is_sel = idx == selected;
        let style = if is_sel && focused {
            theme::focused_style()
        } else if is_sel {
            theme::selected_style()
        } else {
            Style::default().fg(theme::TEXT)
        };
        frame.render_widget(Paragraph::new(format!(" {}", rows[idx])).style(style), rect);
        app.push_click(rect, make_action(idx));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex, RwLock};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use shocking_vrc_core::AvatarScanner;
    use shocking_vrc_core::cli::{AlarmConfig, AlarmController, CliConfig, ZoneEntry, ZoneId};
    use shocking_vrc_core::presets::{ZonePreset, ZonePresetEntry};
    use shocking_vrc_core::{OldZoneType, ZoneEvent};

    use crate::app_state::AppState;
    use crate::tui::app::{Action, App, ClickKind, Tab};

    fn test_app() -> App {
        let state = Arc::new(AppState {
            scanner: AvatarScanner::new(None),
            monitor_enabled: Arc::new(AtomicBool::new(false)),
            default_config: CliConfig::default(),
            device_slots: Arc::new(RwLock::new(Vec::new())),
            alarm: AlarmController::new(AlarmConfig::default()),
        });
        let mut app = App::new(state, Arc::new(Mutex::new(Default::default())));
        app.tutorial_active = false;
        app.auto_save = false;
        app.config = CliConfig::default();
        app.active_tab = Tab::Zones;
        app.zone_presets = vec![set("Alpha", 2, 1), set("Bravo", 0, 0)];
        app.sel_zone_preset = 0;
        app
    }

    fn set(name: &str, zones_a: usize, zones_b: usize) -> ZonePresetEntry {
        let zone = |i: usize| {
            ZoneEntry::with_default_mode(ZoneId::new(OldZoneType::DGB, format!("Zone{i}")))
        };
        let mut cfg = CliConfig::default();
        cfg.channel_a.zones = (0..zones_a).map(zone).collect();
        cfg.channel_b.zones = (0..zones_b).map(zone).collect();
        ZonePresetEntry {
            id: name.to_lowercase(),
            preset: ZonePreset::from_config(name, &cfg),
        }
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

    async fn type_str(app: &mut App, text: &str) {
        for c in text.chars() {
            app.handle_key(key(KeyCode::Char(c))).await;
        }
    }

    async fn open_overlay(app: &mut App) {
        app.handle_key(key(KeyCode::Char('z'))).await;
        app.zone_presets = vec![set("Alpha", 2, 1), set("Bravo", 0, 0)];
        app.sel_zone_preset = 0;
    }

    fn click_at(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn rect_of(app: &App, action: &Action) -> Option<ratatui::layout::Rect> {
        app.clickables
            .iter()
            .find(|c| matches!(&c.kind, ClickKind::Act(a) if a == action))
            .map(|c| c.rect)
    }

    #[tokio::test]
    async fn z_opens_the_overlay_and_lists_saved_sets() {
        let mut app = test_app();
        assert!(!app.zone_presets_open);

        open_overlay(&mut app).await;
        assert!(app.zone_presets_open);

        let screen = draw_to_text(&mut app, 120, 30);
        assert!(screen.contains("Zone sets"), "{screen}");
        assert!(screen.contains("Alpha"), "{screen}");
        assert!(screen.contains("A: 2"), "zone counts are shown: {screen}");
    }

    #[tokio::test]
    async fn the_zones_tab_offers_a_button_for_it() {
        let mut app = test_app();
        draw_at(&mut app, 120, 30);
        let rect = rect_of(&app, &Action::OpenZonePresets).expect("footer button");

        app.handle_mouse(click_at(rect.x + 1, rect.y)).await;
        assert!(app.zone_presets_open);
    }

    #[tokio::test]
    async fn arrows_and_clicks_move_between_sets() {
        let mut app = test_app();
        open_overlay(&mut app).await;

        app.handle_key(key(KeyCode::Down)).await;
        assert_eq!(app.sel_zone_preset, 1);
        app.handle_key(key(KeyCode::Up)).await;
        assert_eq!(app.sel_zone_preset, 0);

        draw_at(&mut app, 120, 30);
        let rect = rect_of(&app, &Action::SelectZonePreset(1)).expect("second row");
        app.handle_mouse(click_at(rect.x + 1, rect.y)).await;
        assert_eq!(app.sel_zone_preset, 1);
    }

    #[tokio::test]
    async fn typing_a_name_swallows_the_global_hotkeys() {
        let mut app = test_app();
        open_overlay(&mut app).await;
        app.handle_key(key(KeyCode::Char('s'))).await;
        assert!(app.zone_preset_save_editing);

        type_str(&mut app, "quest 3 avatar").await;

        assert_eq!(app.zone_preset_save_input, "quest 3 avatar");
        assert!(!app.should_quit, "q must type, not quit");
        assert_eq!(app.active_tab, Tab::Zones, "3 must type, not switch tabs");

        app.handle_key(key(KeyCode::Backspace)).await;
        assert_eq!(app.zone_preset_save_input, "quest 3 avata");
    }

    #[tokio::test]
    async fn esc_backs_out_one_layer_at_a_time() {
        let mut app = test_app();
        open_overlay(&mut app).await;

        app.handle_key(key(KeyCode::Char('s'))).await;
        app.handle_key(key(KeyCode::Esc)).await;
        assert!(!app.zone_preset_save_editing, "name field closes first");
        assert!(app.zone_presets_open, "overlay stays up");

        app.handle_key(key(KeyCode::Char('x'))).await;
        assert_eq!(app.zone_preset_delete_confirm, Some(0));
        app.handle_key(key(KeyCode::Char('n'))).await;
        assert_eq!(app.zone_preset_delete_confirm, None, "N cancels the delete");

        app.handle_key(key(KeyCode::Esc)).await;
        assert!(!app.zone_presets_open, "then the overlay closes");
        assert!(!app.should_quit, "without quitting the app");
    }

    #[tokio::test]
    async fn the_overlay_shields_the_tab_below() {
        let mut app = test_app();
        app.avatar_zones = vec![
            ZoneEvent {
                zone_type: OldZoneType::DGB,
                id: "FrontR".into(),
                is_tps: false,
                level: 0.0,
                velocity: 0.0,
                acceleration: 0.0,
                recoil: 0.0,
            },
            ZoneEvent {
                zone_type: OldZoneType::DGB,
                id: "FrontL".into(),
                is_tps: false,
                level: 0.0,
                velocity: 0.0,
                acceleration: 0.0,
                recoil: 0.0,
            },
        ];
        draw_at(&mut app, 120, 30);
        let avatar_row = rect_of(&app, &Action::SelectAvatar(1)).expect("avatar row");

        open_overlay(&mut app).await;
        draw_at(&mut app, 120, 30);
        app.handle_mouse(click_at(avatar_row.x + 1, avatar_row.y)).await;

        assert_eq!(app.sel_avatar, 0, "the click never reached the avatar list");
    }

    #[tokio::test]
    async fn leaving_the_tab_closes_the_overlay() {
        let mut app = test_app();
        open_overlay(&mut app).await;
        app.handle_key(key(KeyCode::Char('s'))).await;

        app.apply(Action::SwitchTab(Tab::Status)).await;

        assert!(!app.zone_presets_open);
        assert!(!app.zone_preset_save_editing);
    }

    #[tokio::test]
    async fn the_overlay_renders_at_any_size() {
        let mut app = test_app();
        open_overlay(&mut app).await;
        for (w, h) in [(120, 40), (80, 24), (60, 16), (30, 10), (20, 6)] {
            draw_at(&mut app, w, h);
        }

        app.zone_preset_save_editing = true;
        for (w, h) in [(120, 40), (60, 16), (20, 6)] {
            draw_at(&mut app, w, h);
        }

        app.zone_preset_save_editing = false;
        app.zone_preset_delete_confirm = Some(1);
        for (w, h) in [(120, 40), (60, 16), (20, 6)] {
            draw_at(&mut app, w, h);
        }

        app.zone_preset_delete_confirm = None;
        app.zone_presets.clear();
        for (w, h) in [(120, 40), (60, 16), (20, 6)] {
            draw_at(&mut app, w, h);
        }
    }
}
