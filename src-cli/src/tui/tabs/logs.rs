use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::app::App;
use crate::tui::theme;



pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = theme::panel_block("Event log — scroll with wheel or ↑↓");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let height = inner.height as usize;
    let lines: Vec<String> = {
        let buf = app.log_buffer.lock().unwrap_or_else(|e| e.into_inner());
        buf.iter().cloned().collect()
    };

    let total = lines.len();
    let max_scroll = total.saturating_sub(height);
    if app.log_scroll > max_scroll {
        app.log_scroll = max_scroll;
    }

    let end = total.saturating_sub(app.log_scroll);
    let start = end.saturating_sub(height);

    if lines.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  Waiting for events…",
                Style::default().fg(theme::TEXT_DIM),
            )),
            inner,
        );
        return;
    }

    let rendered: Vec<Line> = lines[start..end]
        .iter()
        .map(|l| Line::from(Span::styled(l.clone(), color_for(l))))
        .collect();
    frame.render_widget(Paragraph::new(rendered), inner);
}



fn color_for(line: &str) -> Style {
    if line.starts_with("[ERROR]") {
        Style::default().fg(theme::ERROR)
    } else if line.starts_with("[WARN") {
        Style::default().fg(theme::WARNING)
    } else if line.starts_with("[DEBUG]") || line.starts_with("[TRACE]") {
        Style::default().fg(theme::TEXT_DIM)
    } else {
        Style::default().fg(theme::TEXT)
    }
}