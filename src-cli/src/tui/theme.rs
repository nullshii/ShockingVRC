use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders};

pub const ACCENT: Color = Color::Rgb(72, 196, 220);
pub const ACCENT_DIM: Color = Color::Rgb(48, 110, 130);
pub const HIGHLIGHT: Color = Color::Rgb(255, 190, 90);
pub const TEXT: Color = Color::Rgb(225, 228, 238);
pub const TEXT_DIM: Color = Color::Rgb(115, 122, 140);
pub const SUCCESS: Color = Color::Rgb(86, 210, 130);
pub const WARNING: Color = Color::Rgb(240, 185, 70);
pub const ERROR: Color = Color::Rgb(240, 95, 95);
pub const SURFACE: Color = Color::Rgb(24, 28, 38);
pub const SURFACE_ELEVATED: Color = Color::Rgb(34, 40, 54);

pub fn panel_block(title: impl Into<String>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT_DIM))
        .title(title.into())
        .title_style(
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(SURFACE))
}

pub fn tab_block() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT_DIM))
        .title(" ShockingVRC ")
        .title_style(
            Style::default()
                .fg(HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(SURFACE))
}

pub fn focused_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .fg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn inactive_tab_style() -> Style {
    Style::default().fg(TEXT_DIM)
}

pub fn active_tab_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn label_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(HIGHLIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TEXT)
    }
}

pub fn button_style(focused: bool) -> Style {
    if focused {
        focused_style()
    } else {
        Style::default()
            .fg(TEXT)
            .bg(SURFACE_ELEVATED)
            .add_modifier(Modifier::BOLD)
    }
}

pub fn section_header_style() -> Style {
    Style::default()
        .fg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn hint_style() -> Style {
    Style::default()
        .fg(TEXT_DIM)
        .add_modifier(Modifier::ITALIC)
}

fn rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (128, 128, 128),
    }
}

fn lerp_rgb(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let (ar, ag, ab) = rgb(a);
    let (br, bg, bb) = rgb(b);
    Color::Rgb(
        (ar as f32 + (br as f32 - ar as f32) * t) as u8,
        (ag as f32 + (bg as f32 - ag as f32) * t) as u8,
        (ab as f32 + (bb as f32 - ab as f32) * t) as u8,
    )
}

pub fn battery_gradient_color(t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        lerp_rgb(ERROR, WARNING, t * 2.0)
    } else {
        lerp_rgb(WARNING, SUCCESS, (t - 0.5) * 2.0)
    }
}

pub fn status_badge(ok: bool) -> (Style, Style) {
    if ok {
        (
            Style::default().fg(TEXT_DIM),
            Style::default()
                .fg(Color::Black)
                .bg(SUCCESS)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default().fg(TEXT_DIM),
            Style::default()
                .fg(Color::Black)
                .bg(ERROR)
                .add_modifier(Modifier::BOLD),
        )
    }
}
