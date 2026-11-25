pub mod backdrop;
pub mod palette;
pub mod widgets;

pub use backdrop::Backdrop;
pub use palette::{Palette, THEMES, ThemeDef, theme_by_id};

use std::time::Instant;

const FADE_SECS: f32 = 0.45;

pub struct ThemeState {
    current: Palette,
    from: Palette,
    target: Palette,
    started: Option<Instant>,
    pub id: String,
}

impl ThemeState {
    pub fn new(id: &str) -> Self {
        let pal = Palette::from_theme(theme_by_id(id));
        Self {
            current: pal,
            from: pal,
            target: pal,
            started: None,
            id: theme_by_id(id).id.to_string(),
        }
    }

    pub fn palette(&self) -> &Palette {
        &self.current
    }

    pub fn set(&mut self, id: &str, animate: bool) {
        let def = theme_by_id(id);
        if self.id == def.id {
            return;
        }
        self.id = def.id.to_string();
        self.target = Palette::from_theme(def);
        if animate {
            self.from = self.current;
            self.started = Some(Instant::now());
        } else {
            self.current = self.target;
            self.started = None;
        }
    }

    pub fn is_fading(&self) -> bool {
        self.started.is_some()
    }

    pub fn snap(&mut self) -> bool {
        if self.started.take().is_some() {
            self.current = self.target;
            true
        } else {
            false
        }
    }

    pub fn tick(&mut self) -> bool {
        let Some(started) = self.started else {
            return false;
        };
        let t = (started.elapsed().as_secs_f32() / FADE_SECS).clamp(0.0, 1.0);
        let e = t * t * (3.0 - 2.0 * t);
        self.current = self.from.lerp_to(&self.target, e);
        if t >= 1.0 {
            self.current = self.target;
            self.started = None;
            return false;
        }
        true
    }
}

pub fn install_style(ctx: &egui::Context, pal: &Palette) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();

    style.visuals.dark_mode = !pal.light;
    style.visuals.panel_fill = pal.base;
    style.visuals.window_fill = pal.base;
    style.visuals.extreme_bg_color = pal.well;
    style.visuals.faint_bg_color = pal.sheet;
    style.visuals.override_text_color = Some(pal.contrast);
    style.visuals.window_stroke = egui::Stroke::new(1.0, pal.hair);
    style.visuals.window_corner_radius = widgets::SQUARE;
    style.visuals.menu_corner_radius = widgets::SQUARE;
    style.visuals.window_shadow = egui::epaint::Shadow::NONE;
    style.visuals.popup_shadow = egui::epaint::Shadow::NONE;
    style.visuals.selection.bg_fill = pal.faded(pal.accent, 0.45);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, pal.contrast);
    style.visuals.hyperlink_color = pal.accent;

    for w in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        w.corner_radius = widgets::SQUARE;
        w.fg_stroke.color = pal.contrast;
        w.expansion = 0.0;
    }
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, pal.hair);
    style.visuals.widgets.inactive.weak_bg_fill = pal.sheet;
    style.visuals.widgets.hovered.weak_bg_fill = pal.well;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, pal.accent);
    style.visuals.widgets.active.weak_bg_fill = pal.accent;

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.scroll = egui::style::ScrollStyle::thin();
    style.spacing.interact_size.y = 22.0;

    for (ts, font) in [
        (egui::TextStyle::Body, widgets::font::body()),
        (egui::TextStyle::Button, widgets::font::body()),
        (egui::TextStyle::Small, widgets::font::small()),
        (egui::TextStyle::Monospace, widgets::font::body()),
        (egui::TextStyle::Heading, widgets::font::heading()),
    ] {
        style.text_styles.insert(ts, font);
    }

    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style);
    ctx.set_theme(if pal.light {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    });
}
