use std::time::Instant;

use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui, vec2};

use shocking_vrc_app::logger::LogBuffer;
use shocking_vrc_app::prefs;
use shocking_vrc_core::cli::EngineMode;
use shocking_vrc_core::modulation::config::ModulationConfig;

use crate::bridge::{Bridge, Command, Snapshot, ToastKind};
use crate::model::{Channel, ModKind, Section, default_mod_config};
use crate::theme::widgets::{self as w, font, space};
use crate::theme::{Backdrop, Palette, ThemeState};
use crate::tutorial::Tutorial;
use crate::views;

pub struct UiState {
    pub section: Section,

    pub sel_zone: [usize; 2],
    pub sel_avatar: usize,
    pub avatar_filter: String,

    pub mod_channel: Channel,
    pub mod_kind: ModKind,
    pub mod_seg: usize,
    pub mod_editor: ModulationConfig,
    mod_loaded_from: (Channel, ModKind, usize),
    pub mod_editor_dirty: bool,
    pub show_function_picker: bool,
    pub function_filter: String,

    pub sel_preset: usize,
    pub preset_filter: String,
    pub preset_show_mine_only: bool,
    pub preset_save: Option<PresetSaveDialog>,
    pub preset_delete_confirm: Option<usize>,

    pub show_zone_presets: bool,
    pub sel_zone_preset: usize,
    pub zone_preset_save: Option<String>,
    pub zone_preset_delete_confirm: Option<usize>,

    pub log_filter: String,
    pub log_follow: bool,
    pub log_levels: LogLevels,

    pub auto_save: bool,
    pub alarm_section_visible: bool,
    pub spoof_guard: bool,
    pub nickname: String,

    pub show_theme_panel: bool,
    pub show_about: bool,

    pub mode: EngineMode,
    pub mode_confirm: Option<EngineMode>,
    pub graph_caps: [u8; 2],
    pub graph: crate::graph_editor::GraphEditor,
    pub graph_dialog: Option<crate::overlays::graph_dialogs::GraphDialog>,
}

#[derive(Clone)]
pub struct PresetSaveDialog {
    pub channel: Channel,
    pub name: String,
    pub author: String,
}

#[derive(Clone, Copy)]
pub struct LogLevels {
    pub error: bool,
    pub warn: bool,
    pub info: bool,
    pub debug: bool,
}

impl Default for LogLevels {
    fn default() -> Self {
        Self {
            error: true,
            warn: true,
            info: true,
            debug: true,
        }
    }
}

impl LogLevels {
    pub fn accepts(&self, line: &str) -> bool {
        if line.starts_with("[ERROR]") {
            self.error
        } else if line.starts_with("[WARN") {
            self.warn
        } else if line.starts_with("[DEBUG]") || line.starts_with("[TRACE]") {
            self.debug
        } else {
            self.info
        }
    }
}

impl UiState {
    fn new(p: &prefs::UiPrefs) -> Self {
        Self {
            section: Section::Status,
            sel_zone: [0, 0],
            sel_avatar: 0,
            avatar_filter: String::new(),
            mod_channel: Channel::A,
            mod_kind: ModKind::Freq,
            mod_seg: 0,
            mod_editor: default_mod_config(ModKind::Freq),
            mod_loaded_from: (Channel::A, ModKind::Freq, usize::MAX),
            mod_editor_dirty: false,
            show_function_picker: false,
            function_filter: String::new(),
            sel_preset: 0,
            preset_filter: String::new(),
            preset_show_mine_only: false,
            preset_save: None,
            preset_delete_confirm: None,
            show_zone_presets: false,
            sel_zone_preset: 0,
            zone_preset_save: None,
            zone_preset_delete_confirm: None,
            log_filter: String::new(),
            log_follow: true,
            log_levels: LogLevels::default(),
            auto_save: p.auto_save,
            alarm_section_visible: p.alarm_tab,
            spoof_guard: p.spoof_guard,
            nickname: p.nickname.clone(),
            show_theme_panel: false,
            show_about: false,
            mode: p.mode,
            mode_confirm: None,
            graph_caps: p.graph_power_cap,
            graph: crate::graph_editor::GraphEditor::default(),
            graph_dialog: None,
        }
    }

    pub fn sel_zone_for(&self, ch: Channel) -> usize {
        self.sel_zone[match ch {
            Channel::A => 0,
            Channel::B => 1,
        }]
    }

    pub fn set_sel_zone(&mut self, ch: Channel, i: usize) {
        self.sel_zone[match ch {
            Channel::A => 0,
            Channel::B => 1,
        }] = i;
    }

    pub fn sync_mod_editor(&mut self, snap: &Snapshot) {
        let key = (self.mod_channel, self.mod_kind, self.mod_seg);
        if key == self.mod_loaded_from && self.mod_editor_dirty {
            return;
        }
        if key == self.mod_loaded_from {
            return;
        }
        self.mod_loaded_from = key;
        self.mod_editor_dirty = false;
        self.mod_editor = match snap.mod_slot(self.mod_channel, self.mod_kind, self.mod_seg) {
            Some(c) => {
                let mut c = c.clone();
                c.sanitise(self.mod_kind.is_intensity());
                c
            }
            None => default_mod_config(self.mod_kind),
        };
    }

    pub fn reload_mod_editor(&mut self, snap: &Snapshot) {
        self.mod_loaded_from = (self.mod_channel, self.mod_kind, usize::MAX);
        self.mod_editor_dirty = false;
        self.sync_mod_editor(snap);
    }

    pub fn select_mod_slot(&mut self, ch: Channel, kind: ModKind, seg: usize) {
        self.mod_channel = ch;
        self.mod_kind = kind;
        self.mod_seg = seg.min(3);
        self.show_function_picker = false;
    }

    pub fn section_visible(&self, s: Section) -> bool {
        s.in_mode(self.mode)
            && match s {
                Section::Alarm => self.alarm_section_visible,
                _ => true,
            }
    }

    pub fn follow_mode(&mut self, mode: EngineMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if !self.section_visible(self.section) {
            self.section = if mode == EngineMode::Graph {
                Section::Graph
            } else {
                Section::Status
            };
        }
    }

    #[cfg(test)]
    pub fn for_test() -> Self {
        Self::new(&prefs::UiPrefs::default())
    }

    pub fn default_for(section: Section) -> Self {
        let mut s = Self::new(&prefs::UiPrefs {
            alarm_tab: true,
            ..prefs::UiPrefs::default()
        });
        if section == Section::Graph {
            s.mode = EngineMode::Graph;
        }
        s.section = section;
        s
    }
}

pub struct Ctx<'a> {
    pub pal: &'a Palette,
    pub snap: &'a Snapshot,
    pub bridge: &'a Bridge,
    pub ui_state: &'a mut UiState,
    pub log: &'a LogBuffer,
}

impl Ctx<'_> {
    pub fn send(&self, cmd: Command) {
        self.bridge.send(cmd);
    }
}

pub struct GuiApp {
    bridge: Bridge,
    log: LogBuffer,
    theme: ThemeState,
    backdrop: Backdrop,
    state: UiState,
    tutorial: Tutorial,
    last_frame: Instant,
    pub quit: bool,
}

impl GuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, bridge: Bridge, log: LogBuffer) -> Self {
        let p = prefs::load_ui_prefs();
        let theme_id = p.theme.clone().unwrap_or_else(|| "terminal".to_string());
        let theme = ThemeState::new(&theme_id);
        crate::theme::install_style(&cc.egui_ctx, theme.palette());

        let mut backdrop = Backdrop::new();
        backdrop.speed = p.backdrop_speed.unwrap_or(1.0);
        backdrop.density = p.backdrop_density.unwrap_or(1.0);

        let mut tutorial = Tutorial::new();
        if !p.has_seen_tutorial {
            tutorial.start();
        }

        Self {
            bridge,
            log,
            theme,
            backdrop,
            state: UiState::new(&p),
            tutorial,
            last_frame: Instant::now(),
            quit: false,
        }
    }

    fn save_prefs(&self) {
        let theme = self.theme.id.clone();
        let speed = self.backdrop.speed;
        let density = self.backdrop.density;
        let (auto_save, alarm_tab, spoof_guard, nickname) = (
            self.state.auto_save,
            self.state.alarm_section_visible,
            self.state.spoof_guard,
            self.state.nickname.clone(),
        );
        if let Err(e) = prefs::update_ui_prefs(|p| {
            p.theme = Some(theme);
            p.backdrop_speed = Some(speed);
            p.backdrop_density = Some(density);
            p.auto_save = auto_save;
            p.alarm_tab = alarm_tab;
            p.spoof_guard = spoof_guard;
            p.nickname = nickname;
        }) {
            log::warn!("[prefs] Failed to save: {e}");
        }
    }
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;

        let animating = self.theme.tick();
        if animating {
            crate::theme::install_style(ui.ctx(), self.theme.palette());
        }
        let pal = *self.theme.palette();

        let snap = self.bridge.snapshot();
        self.state.follow_mode(snap.mode);
        self.state.sync_mod_editor(&snap);
        self.clamp_selections(&snap);

        let full = ui.max_rect();
        ui.painter().rect_filled(full, w::SQUARE, pal.base);
        self.backdrop.paint(ui, full, &pal, dt);

        self.handle_shortcuts(ui, &snap);

        self.topbar(ui, &pal, &snap);
        let footer_h = 26.0;
        let body = Rect::from_min_max(
            egui::pos2(full.left(), ui.min_rect().bottom()),
            egui::pos2(full.right(), full.bottom() - footer_h),
        );
        self.body(ui, body, &pal, &snap);
        self.statusbar(ui, &pal, &snap, footer_h);

        self.overlays(ui, &pal, &snap);

        ui.ctx().request_repaint_after(std::time::Duration::from_millis(
            if self.backdrop.speed > 0.0 { 16 } else { 60 },
        ));

        if ui.ctx().input(|i| i.viewport().close_requested()) {
            self.quit = true;
            self.save_prefs();
        }
    }
}

impl GuiApp {
    fn clamp_selections(&mut self, snap: &Snapshot) {
        for ch in Channel::ALL {
            let n = snap.channel(ch).zones.len();
            let cur = self.state.sel_zone_for(ch);
            self.state
                .set_sel_zone(ch, if n == 0 { 0 } else { cur.min(n - 1) });
        }
        let zones = snap.avatar_zones.len();
        if zones == 0 {
            self.state.sel_avatar = 0;
        } else {
            self.state.sel_avatar = self.state.sel_avatar.min(zones - 1);
        }
    }

    fn handle_shortcuts(&mut self, ui: &Ui, snap: &Snapshot) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let alarm_ringing = snap.alarm_status.phase.is_ringing();
        ui.ctx().input(|i| {
            for ev in &i.events {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = ev
                else {
                    continue;
                };
                if modifiers.command || modifiers.alt {
                    continue;
                }

                if alarm_ringing {
                    match key {
                        egui::Key::S => self.bridge.send(Command::AlarmSnooze),
                        egui::Key::Enter | egui::Key::Escape | egui::Key::Space => {
                            self.bridge.send(Command::AlarmStop)
                        }
                        _ => {}
                    }
                    continue;
                }

                if *key == egui::Key::Escape {
                    self.dismiss_topmost();
                    continue;
                }

                if let Some(n) = digit_of(*key)
                    && let Some(s) = Section::ALL.iter().copied().find(|s| {
                        s.key().to_digit(10) == Some(n) && self.state.section_visible(*s)
                    })
                    {
                        self.state.section = s;
                    }
                if *key == egui::Key::T {
                    self.state.show_theme_panel = !self.state.show_theme_panel;
                }
            }
        });
    }

    fn dismiss_topmost(&mut self) {
        let s = &mut self.state;
        if s.mode_confirm.take().is_some() || s.graph_dialog.take().is_some() {
            return;
        }
        if s.graph.dismiss() {
            return;
        }
        if s.preset_delete_confirm.take().is_some() {
            return;
        }
        if s.zone_preset_delete_confirm.take().is_some() {
            return;
        }
        if s.preset_save.take().is_some() {
            return;
        }
        if s.zone_preset_save.take().is_some() {
            return;
        }
        if s.show_function_picker {
            s.show_function_picker = false;
            return;
        }
        if s.show_zone_presets {
            s.show_zone_presets = false;
            return;
        }
        if s.show_about {
            s.show_about = false;
            return;
        }
        if s.show_theme_panel {
            s.show_theme_panel = false;
        }
    }

    fn topbar(&mut self, ui: &mut Ui, pal: &Palette, snap: &Snapshot) {
        let full = ui.max_rect();
        let h = 52.0;
        let bar = Rect::from_min_size(full.min, vec2(full.width(), h));
        ui.painter().rect_filled(bar, w::SQUARE, pal.sheet);
        w::dashed_line(
            ui.painter(),
            bar.left_bottom(),
            bar.right_bottom(),
            Stroke::new(1.0, pal.hair),
            4.0,
            4.0,
        );
        ui.allocate_rect(bar, Sense::hover());

        let inner = bar.shrink2(vec2(18.0, 0.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut child;

        ui.label(
            egui::RichText::new("coyote@vrc:~$ ")
                .font(font::small())
                .color(pal.dim),
        );
        let cmd = self.state.section.title().to_lowercase();
        ui.label(
            egui::RichText::new(&cmd)
                .font(font::small())
                .color(pal.contrast),
        );
        let blink = (ui.input(|i| i.time) * 0.9).fract() < 0.5;
        let (cur, _) = ui.allocate_exact_size(vec2(7.0, 13.0), Sense::hover());
        if blink {
            ui.painter().rect_filled(cur, w::SQUARE, pal.accent);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 14.0;

            if nav_link(ui, pal, 'T', "Theme", self.state.show_theme_panel) {
                self.state.show_theme_panel = !self.state.show_theme_panel;
            }

            for s in Section::ALL.iter().rev() {
                if !self.state.section_visible(*s) {
                    continue;
                }
                let current = self.state.section == *s;
                let mut label = s.title().to_string();
                if *s == Section::Alarm && snap.alarm_status.phase.is_ringing() {
                    label.push('!');
                }
                if nav_link(ui, pal, s.key(), &label, current) {
                    self.state.section = *s;
                }
            }
        });
    }

    fn body(&mut self, ui: &mut Ui, rect: Rect, pal: &Palette, snap: &Snapshot) {
        if rect.height() < 40.0 {
            return;
        }
        if self.state.section == Section::Graph {
            self.graph_body(ui, rect, pal, snap);
            return;
        }
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut child;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let max_w = 1180.0_f32.min(ui.available_width() - 36.0);
                let pad = ((ui.available_width() - max_w) * 0.5).max(18.0);
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.add_space(pad);
                    w::column(ui, max_w, |ui| {
                        self.section_header(ui, pal);
                        let mut cx = Ctx {
                            pal,
                            snap,
                            bridge: &self.bridge,
                            ui_state: &mut self.state,
                            log: &self.log,
                        };
                        match cx.ui_state.section {
                            Section::Status => views::status::show(ui, &mut cx),
                            Section::Zones => views::zones::show(ui, &mut cx),
                            Section::Channels => views::channels::show(ui, &mut cx),
                            Section::Tuning => views::tuning::show(ui, &mut cx),
                            Section::Modulation => views::modulation::show(ui, &mut cx),
                            Section::Log => views::log::show(ui, &mut cx),
                            Section::Presets => views::presets::show(ui, &mut cx),
                            Section::Setup => {
                                let restart = views::setup::show(ui, &mut cx);
                                if restart {
                                    self.tutorial.start();
                                }
                            }
                            Section::Alarm => views::alarm::show(ui, &mut cx),
                            Section::Graph => views::graph::show(ui, &mut cx),
                        }
                        ui.add_space(28.0);
                    });
                });
            });
    }

    fn graph_body(&mut self, ui: &mut Ui, rect: Rect, pal: &Palette, snap: &Snapshot) {
        let pad = 18.0;
        let inner = Rect::from_min_max(
            egui::pos2(rect.left() + pad, rect.top() + 20.0),
            egui::pos2((rect.right() - pad).max(rect.left() + pad + 1.0), rect.bottom() - 14.0),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut child;
        self.section_header(ui, pal);
        let mut cx = Ctx {
            pal,
            snap,
            bridge: &self.bridge,
            ui_state: &mut self.state,
            log: &self.log,
        };
        views::graph::show(ui, &mut cx);
    }

    fn section_header(&self, ui: &mut Ui, pal: &Palette) {
        let s = self.state.section;
        ui.label(
            egui::RichText::new(s.title().to_uppercase())
                .font(font::display())
                .color(pal.contrast),
        );
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(s.blurb())
                .font(font::small())
                .color(pal.dim),
        );
        ui.add_space(space::SHEET_GAP);
    }

    fn statusbar(&mut self, ui: &mut Ui, pal: &Palette, snap: &Snapshot, h: f32) {
        let full = ui.max_rect();
        let bar = Rect::from_min_max(
            egui::pos2(full.left(), full.bottom() - h),
            full.max,
        );
        ui.painter().rect_filled(bar, w::SQUARE, pal.sheet);
        w::dashed_line(
            ui.painter(),
            bar.left_top(),
            bar.right_top(),
            Stroke::new(1.0, pal.hair),
            4.0,
            4.0,
        );

        let inner = bar.shrink2(vec2(18.0, 0.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut child;

        let dot = |ui: &mut Ui, on: bool, label: &str| {
            let c = if on { pal.success } else { pal.error };
            let (r, _) = ui.allocate_exact_size(vec2(7.0, 7.0), Sense::hover());
            ui.painter().rect_filled(r, w::SQUARE, c);
            ui.label(
                egui::RichText::new(label)
                    .font(font::tiny())
                    .color(pal.dim),
            );
        };

        ui.spacing_mut().item_spacing.x = 6.0;
        let connected = snap.active().map(|d| d.connected).unwrap_or(false);
        dot(
            ui,
            connected,
            &match snap.active() {
                Some(d) if d.connected => format!("{}  ·  {}", d.name, short_mac(&d.mac)),
                Some(d) if d.reconnecting => "Reconnecting…".to_string(),
                Some(_) => "Coyote offline".to_string(),
                None => "Scanning for Coyote…".to_string(),
            },
        );
        ui.add_space(14.0);
        dot(ui, snap.vrchat_found, "VRChat");
        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(format!(
                "OSC :{}  ·  {} zone(s)",
                snap.osc_port,
                snap.avatar_zones.len()
            ))
            .font(font::tiny())
            .color(pal.dim),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("v{}", shocking_vrc_core::VERSION))
                    .font(font::tiny())
                    .color(pal.dim),
            );
            let toast = self.bridge.shared.toast.read().ok().and_then(|t| t.clone());
            if let Some(t) = toast {
                let c = match t.kind {
                    ToastKind::Success => pal.success,
                    ToastKind::Error => pal.error,
                    ToastKind::Info => pal.accent,
                };
                ui.add_space(16.0);
                ui.label(egui::RichText::new(&t.text).font(font::tiny()).color(c));
            }
        });
    }

    fn overlays(&mut self, ui: &mut Ui, pal: &Palette, snap: &Snapshot) {
        let ctx = ui.ctx().clone();

        if self.state.show_theme_panel {
            self.theme_panel(&ctx, pal);
        }
        if self.state.show_about {
            crate::overlays::about::show(&ctx, pal, &mut self.state.show_about);
        }

        {
            let mut cx = Ctx {
                pal,
                snap,
                bridge: &self.bridge,
                ui_state: &mut self.state,
                log: &self.log,
            };

            if cx.ui_state.show_zone_presets {
                crate::overlays::zone_presets::show(&ctx, &mut cx);
            }
            if cx.ui_state.show_function_picker {
                crate::overlays::function_picker::show(&ctx, &mut cx);
            }
            if cx.ui_state.preset_save.is_some() {
                crate::overlays::preset_save::show(&ctx, &mut cx);
            }
            crate::overlays::confirm::show(&ctx, &mut cx);
            crate::overlays::graph_dialogs::show(&ctx, &mut cx);
            crate::overlays::update::show(&ctx, &mut cx);

            if snap.alarm_status.phase.is_ringing() {
                crate::overlays::alarm_ring::show(&ctx, &mut cx);
            }
        }

        if self.tutorial.active {
            self.tutorial.show(&ctx, pal, &mut self.state);
        }
    }

    fn theme_panel(&mut self, ctx: &egui::Context, pal: &Palette) {
        let mut open = true;
        egui::Window::new("theme")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::RIGHT_TOP, vec2(-18.0, 58.0))
            .frame(
                egui::Frame::NONE
                    .fill(pal.base)
                    .inner_margin(egui::Margin::same(14)),
            )
            .default_width(300.0)
            .show(ctx, |ui| {
                let outer = ui.max_rect().expand(14.0);
                w::dashed_rect(ui, outer, pal.hair);

                ui.horizontal(|ui| {
                    w::caption(ui, pal, "Palette");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if w::button(ui, pal, "Close").clicked() {
                            open = false;
                        }
                    });
                });
                ui.add_space(6.0);

                for t in crate::theme::THEMES {
                    let active = self.theme.id == t.id;
                    let (rect, resp) = w::row(ui, pal, active, 24.0);
                    let inner = rect.shrink2(vec2(8.0, 0.0));
                    w::text_in(
                        ui,
                        inner,
                        t.name,
                        font::body(),
                        if active { pal.accent } else { pal.contrast },
                        Align2::LEFT_CENTER,
                    );
                    let mut x = inner.right();
                    for c in [t.accent, t.contrast, t.base] {
                        let sw = Rect::from_min_size(
                            egui::pos2(x - 16.0, inner.center().y - 8.0),
                            vec2(16.0, 16.0),
                        );
                        ui.painter().rect_filled(
                            sw.shrink(2.0),
                            w::SQUARE,
                            crate::theme::palette::oklch_to_color(c),
                        );
                        ui.painter().rect_stroke(
                            sw,
                            w::SQUARE,
                            Stroke::new(1.0, if active { pal.accent } else { pal.hair }),
                            StrokeKind::Inside,
                        );
                        x -= 20.0;
                    }
                    if resp.clicked() {
                        self.theme.set(t.id, true);
                        self.save_prefs();
                    }
                }

                ui.add_space(10.0);
                w::dashed_hr(ui, pal.hair);
                ui.add_space(10.0);

                let mut changed = false;
                for (label, value, max) in [
                    ("Speed", &mut self.backdrop.speed, 3.0f32),
                    ("Density", &mut self.backdrop.density, 2.0f32),
                ] {
                    ui.horizontal(|ui| {
                        w::field_label(ui, pal, label, 68.0, false);
                        if let Some(v) =
                            w::Slider::new(*value, 0.0, max).width(160.0).show(ui, pal)
                        {
                            *value = (v * 10.0).round() / 10.0;
                            changed = true;
                        }
                        w::value_label(ui, pal, &format!("{value:.1}"), 34.0, None);
                    });
                }
                if changed {
                    self.save_prefs();
                }

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "Nine palettes, three OKLCH colours each — the same set the website uses.",
                    )
                    .font(font::tiny())
                    .color(pal.dim),
                );
            });
        if !open {
            self.state.show_theme_panel = false;
        }
    }
}

fn nav_link(ui: &mut Ui, pal: &Palette, key: char, label: &str, current: bool) -> bool {
    let key_txt = format!("[{key}] ");
    let font = font::small();
    let kw = ui
        .painter()
        .layout_no_wrap(key_txt.clone(), font.clone(), pal.dim)
        .size()
        .x;
    let lw = ui
        .painter()
        .layout_no_wrap(label.to_uppercase(), font.clone(), pal.contrast)
        .size()
        .x;

    let (rect, resp) = ui.allocate_exact_size(vec2(kw + lw, 20.0), Sense::click());
    let hovered = resp.hovered();
    let color = if current || hovered {
        pal.accent
    } else {
        pal.contrast
    };
    let p = ui.painter();
    p.text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        &key_txt,
        font.clone(),
        if hovered { color } else { pal.dim },
    );
    p.text(
        egui::pos2(rect.left() + kw, rect.center().y),
        Align2::LEFT_CENTER,
        label.to_uppercase(),
        font,
        color,
    );
    if current {
        p.hline(
            rect.x_range(),
            rect.bottom() - 1.0,
            Stroke::new(1.0, pal.accent),
        );
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn digit_of(key: egui::Key) -> Option<u32> {
    use egui::Key::*;
    Some(match key {
        Num1 => 1,
        Num2 => 2,
        Num3 => 3,
        Num4 => 4,
        Num5 => 5,
        Num6 => 6,
        Num7 => 7,
        Num8 => 8,
        Num9 => 9,
        _ => return None,
    })
}

pub fn short_mac(mac: &str) -> String {
    let clean: String = mac.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let tail = &clean[clean.len().saturating_sub(6)..];
    tail.to_uppercase()
}

pub fn level_color(pal: &Palette, ratio: f32) -> Color32 {
    if ratio <= 0.0 {
        pal.dim
    } else {
        pal.gradient(1.0 - ratio.clamp(0.0, 1.0))
    }
}
