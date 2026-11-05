use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, vec2};

use super::Palette;

pub const SQUARE: CornerRadius = CornerRadius::ZERO;

pub mod font {
    use egui::FontId;

    pub fn body() -> FontId {
        FontId::monospace(13.5)
    }
    pub fn small() -> FontId {
        FontId::monospace(12.0)
    }
    pub fn tiny() -> FontId {
        FontId::monospace(11.0)
    }
    pub fn heading() -> FontId {
        FontId::monospace(15.0)
    }
    pub fn display() -> FontId {
        FontId::monospace(26.0)
    }
    pub fn mono_big() -> FontId {
        FontId::monospace(18.0)
    }
}

pub mod space {
    pub const SHEET_PAD: f32 = 14.0;
    pub const SHEET_GAP: f32 = 14.0;
    pub const ROW_GAP: f32 = 6.0;
    pub const ITEM_GAP: f32 = 8.0;
}

pub fn dashed_rect(ui: &Ui, rect: Rect, color: Color32) {
    dashed_rect_with(ui, rect, color, 4.0, 4.0);
}

pub fn dashed_rect_with(ui: &Ui, rect: Rect, color: Color32, dash: f32, gap: f32) {
    let p = ui.painter();
    let stroke = Stroke::new(1.0, color);
    let r = rect.shrink(0.5);
    dashed_line(p, r.left_top(), r.right_top(), stroke, dash, gap);
    dashed_line(p, r.right_top(), r.right_bottom(), stroke, dash, gap);
    dashed_line(p, r.right_bottom(), r.left_bottom(), stroke, dash, gap);
    dashed_line(p, r.left_bottom(), r.left_top(), stroke, dash, gap);
}

pub fn dashed_line(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke, dash: f32, gap: f32) {
    let delta = to - from;
    let len = delta.length();
    if len <= 0.5 {
        return;
    }
    let dir = delta / len;
    let step = dash + gap;
    let mut t = 0.0;
    while t < len {
        let end = (t + dash).min(len);
        painter.line_segment([from + dir * t, from + dir * end], stroke);
        t += step;
    }
}

pub fn dashed_hr(ui: &mut Ui, color: Color32) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    dashed_line(
        ui.painter(),
        rect.left_center(),
        rect.right_center(),
        Stroke::new(1.0, color),
        4.0,
        4.0,
    );
}

pub fn hatch(ui: &Ui, rect: Rect, color: Color32, spacing: f32) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let p = ui.painter().with_clip_rect(rect);
    let stroke = Stroke::new(1.0, color);
    let span = rect.width() + rect.height();
    let mut x = rect.left() - rect.height();
    while x < rect.left() + span {
        p.line_segment([Pos2::new(x, rect.bottom()), Pos2::new(x + rect.height(), rect.top())], stroke);
        x += spacing;
    }
}

pub fn focus_corners(ui: &Ui, rect: Rect, color: Color32) {
    let p = ui.painter();
    let s = Stroke::new(1.0, color);
    let n = 6.0;
    let r = rect.expand(1.0);
    for (corner, dx, dy) in [
        (r.left_top(), 1.0, 1.0),
        (r.right_top(), -1.0, 1.0),
        (r.left_bottom(), 1.0, -1.0),
        (r.right_bottom(), -1.0, -1.0),
    ] {
        p.line_segment([corner, corner + vec2(n * dx, 0.0)], s);
        p.line_segment([corner, corner + vec2(0.0, n * dy)], s);
    }
}

pub fn sheet<R>(ui: &mut Ui, pal: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    sheet_tinted(ui, pal, pal.hair, add)
}

pub fn sheet_tinted<R>(ui: &mut Ui, pal: &Palette, border: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let outer = egui::Frame::NONE
        .fill(pal.sheet)
        .inner_margin(egui::Margin::same(space::SHEET_PAD as i8));
    let resp = outer.show(ui, add);
    dashed_rect(ui, resp.response.rect, border);
    resp.inner
}

pub fn heading(ui: &mut Ui, pal: &Palette, text: &str) {
    heading_with_suffix(ui, pal, text, None);
}

pub fn heading_with_suffix(ui: &mut Ui, pal: &Palette, text: &str, suffix: Option<&str>) {
    let label = text.to_uppercase();
    let font = font::heading();
    let galley = ui.painter().layout_no_wrap(label.clone(), font.clone(), pal.on_solid());
    let chip_w = galley.size().x + 18.0;
    let h = galley.size().y + 10.0;

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let p = ui.painter();

    p.rect_stroke(rect, SQUARE, Stroke::new(1.0, pal.contrast), StrokeKind::Inside);

    let chip = Rect::from_min_size(rect.min, vec2(chip_w.min(rect.width()), h)).shrink(3.0);
    p.rect_filled(chip, SQUARE, pal.contrast);
    p.text(chip.center(), Align2::CENTER_CENTER, &label, font, pal.on_solid());

    let tail = Rect::from_min_max(Pos2::new(rect.min.x + chip_w, rect.min.y), rect.max);
    if tail.width() > 2.0 {
        p.vline(tail.left(), tail.y_range(), Stroke::new(1.0, pal.contrast));
        hatch(ui, tail.shrink(1.0), pal.faded(pal.contrast, 0.55), 10.0);
        if let Some(suffix) = suffix {
            let sf = font::small();
            let sg = ui.painter().layout_no_wrap(suffix.to_string(), sf.clone(), pal.dim);
            let pad = 8.0;
            let box_rect = Rect::from_min_size(
                Pos2::new(tail.right() - sg.size().x - pad * 2.0, tail.top() + 2.0),
                vec2(sg.size().x + pad * 2.0, tail.height() - 4.0),
            );
            if box_rect.left() > tail.left() + 6.0 {
                ui.painter().rect_filled(box_rect, SQUARE, pal.sheet);
                ui.painter()
                    .text(box_rect.center(), Align2::CENTER_CENTER, suffix, sf, pal.dim);
            }
        }
    }
    ui.add_space(space::ITEM_GAP);
}

pub fn body(ui: &mut Ui, pal: &Palette, text: impl Into<String>) {
    ui.label(egui::RichText::new(text).font(font::body()).color(pal.contrast));
}

pub fn dim(ui: &mut Ui, pal: &Palette, text: impl Into<String>) {
    ui.label(egui::RichText::new(text).font(font::small()).color(pal.dim));
}

pub fn caption(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .font(font::tiny())
            .color(pal.dim),
    );
}

pub fn marked(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(font::body())
            .color(pal.contrast)
            .background_color(pal.marker),
    );
}

pub fn badge(ui: &mut Ui, pal: &Palette, text: &str, fill: Color32) -> Response {
    let font = font::small();
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), pal.on_solid());
    let size = galley.size() + vec2(14.0, 5.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, SQUARE, fill);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, text, font, pal.on_solid());
    resp
}

pub fn tag(ui: &mut Ui, _pal: &Palette, text: &str, color: Color32) -> Response {
    let font = font::small();
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), color);
    let size = galley.size() + vec2(14.0, 5.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_stroke(rect, SQUARE, Stroke::new(1.0, color), StrokeKind::Inside);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, text, font, color);
    resp
}

pub fn cta(ui: &mut Ui, pal: &Palette, text: &str) -> Response {
    button_inner(ui, pal, text, ButtonStyle::Cta, true)
}

pub fn button(ui: &mut Ui, pal: &Palette, text: &str) -> Response {
    button_inner(ui, pal, text, ButtonStyle::Outline, true)
}

pub fn button_enabled(ui: &mut Ui, pal: &Palette, text: &str, enabled: bool) -> Response {
    button_inner(ui, pal, text, ButtonStyle::Outline, enabled)
}

pub fn danger_button(ui: &mut Ui, pal: &Palette, text: &str, enabled: bool) -> Response {
    button_inner(ui, pal, text, ButtonStyle::Danger, enabled)
}

#[derive(Clone, Copy, PartialEq)]
enum ButtonStyle {
    Cta,
    Outline,
    Danger,
}

fn button_inner(ui: &mut Ui, pal: &Palette, text: &str, style: ButtonStyle, enabled: bool) -> Response {
    let font = font::body();
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), pal.contrast);
    let size = vec2(galley.size().x + 20.0, galley.size().y + 9.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let hovered = enabled && resp.hovered();
    let p = ui.painter();

    let (fill, fg, border) = match (style, enabled, hovered) {
        (_, false, _) => (Color32::TRANSPARENT, pal.faded(pal.dim, 0.4), pal.faded(pal.hair, 0.4)),
        (ButtonStyle::Cta, _, false) => (pal.accent, pal.on_solid(), pal.accent),
        (ButtonStyle::Cta, _, true) => (pal.contrast, pal.on_solid(), pal.contrast),
        (ButtonStyle::Outline, _, false) => (Color32::TRANSPARENT, pal.contrast, pal.hair),
        (ButtonStyle::Outline, _, true) => (pal.accent, pal.on_solid(), pal.accent),
        (ButtonStyle::Danger, _, false) => (Color32::TRANSPARENT, pal.error, pal.error),
        (ButtonStyle::Danger, _, true) => (pal.error, pal.on_solid(), pal.error),
    };

    if fill != Color32::TRANSPARENT {
        p.rect_filled(rect, SQUARE, fill);
    }
    p.rect_stroke(rect, SQUARE, Stroke::new(1.0, border), StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, text, font, fg);
    if resp.has_focus() {
        focus_corners(ui, rect, pal.accent);
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub fn segmented(ui: &mut Ui, pal: &Palette, options: &[&str], selected: usize) -> Option<usize> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, label) in options.iter().enumerate() {
            let font = font::small();
            let galley = ui
                .painter()
                .layout_no_wrap(label.to_string(), font.clone(), pal.contrast);
            let size = vec2(galley.size().x + 18.0, galley.size().y + 8.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let on = i == selected;
            let hovered = resp.hovered();
            let p = ui.painter();
            let (fill, fg) = match (on, hovered) {
                (true, _) => (pal.accent, pal.on_solid()),
                (false, true) => (pal.well, pal.contrast),
                (false, false) => (Color32::TRANSPARENT, pal.dim),
            };
            if fill != Color32::TRANSPARENT {
                p.rect_filled(rect, SQUARE, fill);
            }
            p.rect_stroke(
                rect,
                SQUARE,
                Stroke::new(1.0, if on { pal.accent } else { pal.hair }),
                StrokeKind::Inside,
            );
            p.text(rect.center(), Align2::CENTER_CENTER, *label, font, fg);
            if resp.clicked() {
                picked = Some(i);
            }
            if hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
    });
    picked
}

pub fn toggle(ui: &mut Ui, pal: &Palette, on: bool) -> Option<bool> {
    segmented(ui, pal, &["ON", "OFF"], if on { 0 } else { 1 }).map(|i| i == 0)
}
pub struct Slider<'a> {
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub inverted: bool,
    pub width: f32,
    pub fill: Option<Color32>,
    pub label: Option<&'a str>,
}

impl<'a> Slider<'a> {
    pub fn new(value: f32, min: f32, max: f32) -> Self {
        Self {
            value,
            min,
            max,
            inverted: false,
            width: 0.0,
            fill: None,
            label: None,
        }
    }
    pub fn inverted(mut self, yes: bool) -> Self {
        self.inverted = yes;
        self
    }
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
    pub fn fill(mut self, c: Color32) -> Self {
        self.fill = Some(c);
        self
    }

    pub fn show(self, ui: &mut Ui, pal: &Palette) -> Option<f32> {
        let w = if self.width > 0.0 {
            self.width
        } else {
            ui.available_width()
        };
        let (rect, _) = ui.allocate_exact_size(vec2(w, 16.0), Sense::hover());
        self.show_in(ui, pal, rect)
    }

    pub fn show_in(self, ui: &mut Ui, pal: &Palette, rect: Rect) -> Option<f32> {
        let resp = ui.interact(
            rect,
            ui.id().with(("slider", rect.left() as i32, rect.top() as i32)),
            Sense::click_and_drag(),
        );
        let track = Rect::from_center_size(rect.center(), vec2(rect.width(), 10.0));
        let p = ui.painter();

        p.rect_filled(track, SQUARE, pal.well);
        hatch(ui, track.shrink(1.0), pal.faded(pal.contrast, 0.7), 6.0);

        let span = (self.max - self.min).max(1e-6);
        let mut frac = ((self.value - self.min) / span).clamp(0.0, 1.0);
        if self.inverted {
            frac = 1.0 - frac;
        }

        let accent = self.fill.unwrap_or(pal.accent);
        let filled = if self.inverted {
            Rect::from_min_max(
                Pos2::new(track.left() + frac * track.width(), track.top()),
                track.right_bottom(),
            )
        } else {
            Rect::from_min_max(track.left_top(), Pos2::new(track.left() + frac * track.width(), track.bottom()))
        };
        if filled.width() > 0.5 {
            ui.painter().rect_filled(filled, SQUARE, pal.faded(accent, 0.35));
        }
        ui.painter().rect_stroke(
            track,
            SQUARE,
            Stroke::new(1.0, if resp.hovered() { accent } else { pal.hair }),
            StrokeKind::Inside,
        );

        let tx = track.left() + frac * track.width();
        let thumb = Rect::from_center_size(Pos2::new(tx, track.center().y), vec2(11.0, 16.0));
        ui.painter().rect_filled(thumb, SQUARE, pal.contrast);
        ui.painter()
            .rect_stroke(thumb, SQUARE, Stroke::new(1.0, pal.base), StrokeKind::Inside);

        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }

        if (resp.dragged() || resp.clicked())
            && let Some(pos) = resp.interact_pointer_pos()
        {
            let denom = track.width().max(1.0);
            let mut f = ((pos.x - track.left()) / denom).clamp(0.0, 1.0);
            if self.inverted {
                f = 1.0 - f;
            }
            return Some(self.min + f * span);
        }
        None
    }
}

#[allow(clippy::too_many_arguments)]
pub fn slider_row(
    ui: &mut Ui,
    pal: &Palette,
    label: &str,
    label_w: f32,
    slider: Slider<'_>,
    value: &str,
    value_w: f32,
    value_color: Option<Color32>,
) -> Option<f32> {
    let h = 22.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());

    let gap = 8.0;
    let label_w = label_w.min(rect.width() * 0.5);
    let value_w = value_w.min(rect.width() * 0.35);
    let track_w = (rect.width() - label_w - value_w - gap * 2.0).max(24.0);

    let label_rect = Rect::from_min_size(rect.min, vec2(label_w, h));
    let track_rect = Rect::from_min_size(
        egui::pos2(rect.left() + label_w + gap, rect.center().y - 8.0),
        vec2(track_w, 16.0),
    );
    let value_rect = Rect::from_min_size(egui::pos2(rect.right() - value_w, rect.top()), vec2(value_w, h));

    text_in(ui, label_rect, label, font::body(), pal.dim, Align2::LEFT_CENTER);
    let changed = slider.show_in(ui, pal, track_rect);
    text_in(
        ui,
        value_rect,
        value,
        font::body(),
        value_color.unwrap_or(pal.contrast),
        Align2::RIGHT_CENTER,
    );
    changed
}

pub fn gauge(ui: &mut Ui, pal: &Palette, ratio: f32, color: Color32, height: f32) -> Rect {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
    ui.painter().rect_filled(rect, SQUARE, pal.well);
    let r = ratio.clamp(0.0, 1.0);
    if r > 0.0 {
        let filled = Rect::from_min_size(rect.min, vec2(rect.width() * r, rect.height()));
        ui.painter().rect_filled(filled, SQUARE, color);
    }
    ui.painter()
        .rect_stroke(rect, SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
    rect
}

#[allow(clippy::too_many_arguments)]
pub fn gauge_row(
    ui: &mut Ui,
    pal: &Palette,
    label: &str,
    label_w: f32,
    ratio: f32,
    color: Color32,
    value: &str,
    value_w: f32,
) {
    let h = 20.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let gap = 8.0;
    let label_w = label_w.min(rect.width() * 0.5);
    let value_w = value_w.min(rect.width() * 0.35);
    let bar_w = (rect.width() - label_w - value_w - gap * 2.0).max(20.0);

    text_in(
        ui,
        Rect::from_min_size(rect.min, vec2(label_w, h)),
        label,
        font::body(),
        pal.dim,
        Align2::LEFT_CENTER,
    );

    let bar = Rect::from_min_size(
        egui::pos2(rect.left() + label_w + gap, rect.center().y - 5.0),
        vec2(bar_w, 10.0),
    );
    ui.painter().rect_filled(bar, SQUARE, pal.well);
    let r = ratio.clamp(0.0, 1.0);
    if r > 0.0 {
        ui.painter()
            .rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * r, bar.height())), SQUARE, color);
    }
    ui.painter()
        .rect_stroke(bar, SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);

    text_in(
        ui,
        Rect::from_min_size(egui::pos2(rect.right() - value_w, rect.top()), vec2(value_w, h)),
        value,
        font::body(),
        if ratio > 0.0 { pal.contrast } else { pal.dim },
        Align2::RIGHT_CENTER,
    );
}

pub fn stepper(ui: &mut Ui, pal: &Palette, value: &str, width: f32) -> Option<i32> {
    let mut delta = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if button(ui, pal, "−").clicked() {
            delta = Some(-1);
        }
        let font = font::body();
        let (rect, _) = ui.allocate_exact_size(vec2(width, 25.0), Sense::hover());
        ui.painter()
            .rect_stroke(rect, SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
        ui.painter()
            .text(rect.center(), Align2::CENTER_CENTER, value, font, pal.contrast);
        if button(ui, pal, "+").clicked() {
            delta = Some(1);
        }
    });
    delta
}

pub fn row(ui: &mut Ui, pal: &Palette, selected: bool, height: f32) -> (Rect, Response) {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, height), Sense::click());
    let hovered = resp.hovered();
    if selected {
        ui.painter().rect_filled(rect, SQUARE, pal.faded(pal.accent, 0.82));
        ui.painter()
            .vline(rect.left() + 1.0, rect.y_range(), Stroke::new(2.0, pal.accent));
    } else if hovered {
        ui.painter().rect_filled(rect, SQUARE, pal.well);
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (rect, resp)
}

pub fn text_in(ui: &Ui, rect: Rect, text: &str, font: FontId, color: Color32, align: Align2) {
    let p = ui.painter().with_clip_rect(rect);
    let pos = match align {
        Align2::LEFT_CENTER => rect.left_center(),
        Align2::RIGHT_CENTER => rect.right_center(),
        _ => rect.center(),
    };
    p.text(pos, align, text, font, color);
}

pub fn stat_cell(ui: &mut Ui, pal: &Palette, label: &str, value: &str, value_color: Color32) {
    let size = vec2(ui.available_width(), 46.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    dashed_rect(ui, rect, pal.hair);
    let inner = rect.shrink2(vec2(10.0, 6.0));
    text_in(
        ui,
        Rect::from_min_size(inner.min, vec2(inner.width(), 12.0)),
        &label.to_uppercase(),
        font::tiny(),
        pal.dim,
        Align2::LEFT_CENTER,
    );
    text_in(
        ui,
        Rect::from_min_max(Pos2::new(inner.left(), inner.top() + 13.0), inner.max),
        value,
        font::mono_big(),
        value_color,
        Align2::LEFT_CENTER,
    );
}

pub fn field_label(ui: &mut Ui, pal: &Palette, text: &str, width: f32, emphasis: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    text_in(
        ui,
        rect,
        text,
        font::body(),
        if emphasis { pal.contrast } else { pal.dim },
        Align2::LEFT_CENTER,
    );
}

pub fn value_label(ui: &mut Ui, pal: &Palette, text: &str, width: f32, color: Option<Color32>) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    text_in(
        ui,
        rect,
        text,
        font::body(),
        color.unwrap_or(pal.contrast),
        Align2::RIGHT_CENTER,
    );
}

pub fn text_field(ui: &mut Ui, pal: &Palette, buf: &mut String, hint: &str, width: f32) -> Response {
    let before = ui.style().clone();
    {
        let s = ui.style_mut();
        s.visuals.extreme_bg_color = pal.well;
        s.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, pal.hair);
        s.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, pal.dim);
        s.visuals.widgets.active.bg_stroke = Stroke::new(1.0, pal.accent);
        s.visuals.selection.bg_fill = pal.faded(pal.accent, 0.4);
        s.visuals.selection.stroke = Stroke::new(1.0, pal.contrast);
        s.visuals.widgets.inactive.corner_radius = SQUARE;
        s.visuals.widgets.hovered.corner_radius = SQUARE;
        s.visuals.widgets.active.corner_radius = SQUARE;
    }
    let resp = ui.add_sized(
        vec2(width, 26.0),
        egui::TextEdit::singleline(buf)
            .font(font::body())
            .text_color(pal.contrast)
            .hint_text(egui::RichText::new(hint).color(pal.dim).font(font::body()))
            .margin(egui::Margin::symmetric(8, 4)),
    );
    ui.set_style(before);
    resp
}

pub fn column<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let width = width.max(1.0);
    let origin = ui.cursor().min;
    let height = ui.available_height().max(4096.0);
    let max = Rect::from_min_size(origin, vec2(width, height));

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(max)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let inner = add(&mut child);
    let used = child.min_rect().height();

    ui.allocate_rect(Rect::from_min_size(origin, vec2(width, used)), Sense::hover());
    inner
}

pub fn empty_state(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(text).font(font::small()).color(pal.dim).italics());
    });
    ui.add_space(6.0);
}

pub fn ch(ui: &Ui, n: f32) -> f32 {
    ui.fonts_mut(|f| f.glyph_width(&font::body(), '0')) * n
}

pub fn vec_size(w: f32, h: f32) -> Vec2 {
    vec2(w, h)
}
