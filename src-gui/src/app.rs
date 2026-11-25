use std::sync::OnceLock;
use std::time::Instant;

use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui, vec2};

use shocking_vrc_app::logger::LogBuffer;
use shocking_vrc_app::prefs;
use shocking_vrc_core::cli::{AlarmPhase, EngineMode};
use shocking_vrc_core::modulation::config::ModulationConfig;

use crate::bridge::{Bridge, Command, Snapshot, ToastKind};
use crate::graphics::{
    self, Decision as GraphicsDecision, FrameGuard, Mode as GraphicsMode, PowerWatcher,
    Preference as GraphicsPreference, RepaintNeed, Runtime as GraphicsRuntime, WindowVisibility,
};
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
    pub nickname_dirty: bool,
    pub has_seen_tutorial: bool,

    pub show_theme_panel: bool,
    pub show_about: bool,
    pub graphics_mode: GraphicsMode,
    pub window_visible: bool,

    pub mode: EngineMode,
    pub mode_confirm: Option<EngineMode>,
    pub graph_caps: [u8; 2],
    pub graph: crate::graph_editor::GraphEditor,
    pub graph_dialog: Option<crate::overlays::graph_dialogs::GraphDialog>,
    pub remote_enabled: bool,
    pub remote_graph: crate::graph_editor::GraphEditor,
    pub graph_tab: GraphTab,
    pub remote_form: RemoteForm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphTab {
    #[default]
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RemoteTab {
    #[default]
    Connect,
    Limits,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InviteMode {
    #[default]
    Link,
    WebCode,
}

#[derive(Clone)]
pub struct RemoteForm {
    pub tab: RemoteTab,
    pub invite_mode: InviteMode,
    pub role: shocking_vrc_remote::link::OfferedRole,
    pub lifetime: shocking_vrc_remote::store::Lifetime,
    pub passphrase: String,
    pub label: String,
    pub join_link: String,
    pub join_pass: String,
    pub web_lifetime: shocking_vrc_remote::store::Lifetime,
    pub trusted: bool,
    pub web_trusted: bool,
    pub connection: Option<shocking_vrc_remote::service::RemoteConfig>,
    pub stop_param: Option<String>,
    pub show_log: bool,
    pub limit_slot: u8,
}

impl Default for RemoteForm {
    fn default() -> Self {
        Self {
            tab: RemoteTab::Connect,
            invite_mode: InviteMode::Link,
            role: shocking_vrc_remote::link::OfferedRole::Receiver,
            lifetime: shocking_vrc_remote::store::Lifetime::SingleUse,
            passphrase: String::new(),
            label: String::new(),
            join_link: String::new(),
            join_pass: String::new(),
            web_lifetime: shocking_vrc_remote::store::Lifetime::Hour1,
            trusted: false,
            web_trusted: false,
            connection: None,
            stop_param: None,
            show_log: false,
            limit_slot: 0,
        }
    }
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
            nickname_dirty: false,
            has_seen_tutorial: p.has_seen_tutorial,
            show_theme_panel: false,
            show_about: false,
            graphics_mode: GraphicsMode::Full,
            window_visible: true,
            mode: p.mode,
            mode_confirm: None,
            graph_caps: p.graph_power_cap,
            graph: crate::graph_editor::GraphEditor::default(),
            graph_dialog: None,
            remote_enabled: false,
            remote_graph: crate::graph_editor::GraphEditor::for_remote(),
            graph_tab: GraphTab::Local,
            remote_form: RemoteForm::default(),
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
                Section::Remote => self.remote_enabled,
                _ => true,
            }
    }

    pub fn follow_remote(&mut self, on: bool) {
        if self.remote_enabled == on {
            return;
        }
        self.remote_enabled = on;
        if !on {
            self.graph_tab = GraphTab::Local;
            if self.section == Section::Remote {
                self.section = Section::Setup;
            }
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
    started_at: Instant,
    visibility: WindowVisibility,
    graphics_pref: GraphicsPreference,
    graphics_runtime: GraphicsRuntime,
    graphics_decision: GraphicsDecision,
    applied_mode: Option<GraphicsMode>,
    default_animation_time: f32,
    power_watcher: PowerWatcher,
    frame_guard: FrameGuard,
    pub quit: bool,
}

impl GuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, bridge: Bridge, log: LogBuffer, visibility: WindowVisibility) -> Self {
        let p = prefs::load_ui_prefs();
        let theme_id = p.theme.clone().unwrap_or_else(|| "terminal".to_string());
        let theme = ThemeState::new(&theme_id);
        let default_animation_time = cc.egui_ctx.style_of(egui::Theme::Dark).animation_time;
        crate::theme::install_style(&cc.egui_ctx, theme.palette());

        let graphics_pref = p.graphics;
        let graphics_runtime = GraphicsRuntime::detect(cc);
        let initial_machine = graphics::machine();
        let power_watcher = PowerWatcher::spawn(initial_machine, visibility.handle(), cc.egui_ctx.clone());
        let graphics_decision = graphics::decide(graphics_pref, initial_machine, &graphics_runtime);
        log::info!(
            "[graphics] {} (renderer: {})",
            graphics_decision.status_line(graphics_pref),
            if graphics_runtime.renderer.is_empty() {
                "unknown"
            } else {
                &graphics_runtime.renderer
            }
        );

        graphics::apply_mode_to_ctx(&cc.egui_ctx, graphics_decision.mode, default_animation_time);
        bridge.shared.set_graphics_mode(graphics_decision.mode);

        let mut backdrop = Backdrop::new();
        backdrop.speed = p.backdrop_speed.unwrap_or(1.0);
        backdrop.density = p.backdrop_density.unwrap_or(1.0);

        let mut tutorial = Tutorial::new();
        if !p.has_seen_tutorial {
            tutorial.start();
        }

        let mut state = UiState::new(&p);
        state.graphics_mode = graphics_decision.mode;

        let initial_mode = graphics_decision.mode;
        let now = Instant::now();
        Self {
            bridge,
            log,
            theme,
            backdrop,
            state,
            tutorial,
            last_frame: now,
            started_at: now,
            visibility,
            graphics_pref,
            graphics_runtime,
            graphics_decision,
            applied_mode: Some(initial_mode),
            default_animation_time,
            power_watcher,
            frame_guard: FrameGuard::new(),
            quit: false,
        }
    }

    fn recompute_graphics(&mut self, ctx: &egui::Context, machine: graphics::Machine) {
        let next = graphics::decide(self.graphics_pref, machine, &self.graphics_runtime);
        let mode_changed = self.applied_mode != Some(next.mode);
        self.graphics_decision = next;
        self.state.graphics_mode = self.graphics_decision.mode;
        if mode_changed {
            self.applied_mode = Some(self.graphics_decision.mode);
            graphics::apply_mode_to_ctx(ctx, self.graphics_decision.mode, self.default_animation_time);
            self.bridge.shared.set_graphics_mode(self.graphics_decision.mode);
            self.frame_guard.reset();
            log::info!("[graphics] {}", self.graphics_decision.status_line(self.graphics_pref));
        }
    }

    fn save_prefs(&self) {
        let theme = self.theme.id.clone();
        let speed = self.backdrop.speed;
        let density = self.backdrop.density;
        let graphics = self.graphics_pref;
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
            p.graphics = graphics;
        }) {
            log::warn!("[prefs] Failed to save: {e}");
        }
    }
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let frame_start = Instant::now();
        let dt = (frame_start - self.last_frame).as_secs_f32();
        self.last_frame = frame_start;

        let (visible, just_restored) = self.visibility.sync_from_ctx(ui.ctx());
        self.state.window_visible = visible;

        let latest_machine = self.power_watcher.latest();
        if latest_machine != self.graphics_decision.machine {
            self.recompute_graphics(ui.ctx(), latest_machine);
        }

        let mode = self.graphics_decision.mode;
        let theme_animating = if mode == GraphicsMode::Full && visible {
            let was_fading = self.theme.is_fading();
            let still = self.theme.tick();
            if was_fading {
                crate::theme::install_style(ui.ctx(), self.theme.palette());
            }
            still
        } else {
            if self.theme.snap() {
                crate::theme::install_style(ui.ctx(), self.theme.palette());
            }
            false
        };
        let pal = *self.theme.palette();

        let snap = self.bridge.snapshot();
        self.state.follow_remote(snap.remote_enabled);
        self.state.follow_mode(snap.mode);
        self.state.sync_mod_editor(&snap);
        self.clamp_selections(&snap);

        let full = ui.max_rect();
        ui.painter().rect_filled(full, w::SQUARE, pal.base);
        self.backdrop
            .paint_mode(ui, full, &pal, if visible { dt } else { 0.0 }, mode);

        self.handle_shortcuts(ui, &snap);

        let topbar_bottom = self.topbar(ui, &pal, &snap);
        let footer_h = 26.0;
        let body = Rect::from_min_max(
            egui::pos2(full.left(), topbar_bottom),
            egui::pos2(full.right(), full.bottom() - footer_h),
        );
        self.body(ui, body, &pal, &snap);
        self.statusbar(ui, &pal, &snap, footer_h);

        self.overlays(ui, &pal, &snap, topbar_bottom);

        let backdrop_anim = self.backdrop.is_animating(mode);
        let wire_anim = mode == GraphicsMode::Full
            && self.state.section == Section::Graph
            && has_lit_graph_probe(&snap, self.state.graph_tab);
        let animating = mode == GraphicsMode::Full && (theme_animating || backdrop_anim || wire_anim);

        if self.graphics_pref == GraphicsPreference::Auto && mode == GraphicsMode::Full && visible && animating {
            let frame_cost_ms = frame
                .info()
                .cpu_usage
                .map(|secs| secs * 1000.0)
                .unwrap_or_else(|| frame_start.elapsed().as_secs_f32() * 1000.0);
            if let Some(avg_ms) = self.frame_guard.record(self.started_at.elapsed(), frame_cost_ms) {
                log::warn!(
                    "[graphics] Average frame time {avg_ms:.1} ms exceeded {:.0} ms over 5 s; switching Auto to Lite",
                    graphics::SLOW_FRAME_MS
                );
                self.graphics_runtime.slow_frames = true;
                let machine = self.graphics_decision.machine;
                self.recompute_graphics(ui.ctx(), machine);
            }
        } else if !visible || !animating {
            self.frame_guard.reset();
        }

        let mode = self.graphics_decision.mode;
        let periodic_telemetry = has_active_telemetry(&snap, &self.bridge);
        if just_restored {
            ui.ctx().request_repaint();
        } else if let Some(delay) = graphics::next_repaint(RepaintNeed {
            visible,
            mode,
            animating: mode == GraphicsMode::Full && animating,
            periodic_telemetry,
        }) {
            ui.ctx().request_repaint_after(delay);
        }

        if ui.ctx().input(|i| i.viewport().close_requested()) {
            self.quit = true;
            self.save_prefs();
        }

        if !self.quit && self.bridge.shared.update.read().is_ok_and(|u| u.pending.is_some()) {
            log::info!("[update] Closing to install the update");
            self.quit = true;
            self.save_prefs();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl GuiApp {
    fn clamp_selections(&mut self, snap: &Snapshot) {
        for ch in Channel::ALL {
            let n = snap.channel(ch).zones.len();
            let cur = self.state.sel_zone_for(ch);
            self.state.set_sel_zone(ch, if n == 0 { 0 } else { cur.min(n - 1) });
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
                        egui::Key::Enter | egui::Key::Escape | egui::Key::Space => self.bridge.send(Command::AlarmStop),
                        _ => {}
                    }
                    continue;
                }

                if *key == egui::Key::Escape {
                    self.dismiss_topmost();
                    continue;
                }

                if let Some(n) = digit_of(*key)
                    && let Some(s) = Section::ALL
                        .iter()
                        .copied()
                        .find(|s| s.key().to_digit(10) == Some(n) && self.state.section_visible(*s))
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
        if s.graph.dismiss() || s.remote_graph.dismiss() {
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

    fn topbar(&mut self, ui: &mut Ui, pal: &Palette, snap: &Snapshot) -> f32 {
        let full = ui.max_rect();
        let inner_w = (full.width() - 36.0).max(1.0);
        let cmd = self.state.section.title().to_lowercase();
        let blink = if self.graphics_decision.mode == GraphicsMode::Lite || self.backdrop.speed <= 0.0 {
            true
        } else {
            (ui.input(|i| i.time) * 0.9).fract() < 0.5
        };

        let mut nav_items: Vec<(Option<Section>, char, String, bool)> = Vec::with_capacity(Section::ALL.len() + 1);
        for &s in &Section::ALL {
            if !self.state.section_visible(s) {
                continue;
            }
            let mut label = s.title().to_string();
            if s == Section::Alarm && snap.alarm_status.phase.is_ringing() {
                label.push('!');
            }
            nav_items.push((Some(s), s.key(), label, self.state.section == s));
        }
        nav_items.push((None, 'T', "Theme".to_string(), self.state.show_theme_panel));

        let prompt_w = ui
            .painter()
            .layout_no_wrap("coyote@vrc:~$ ".to_string(), font::small(), pal.dim)
            .size()
            .x
            + ui.painter()
                .layout_no_wrap(cmd.clone(), font::small(), pal.contrast)
                .size()
                .x
            + 7.0
            + ui.spacing().item_spacing.x * 2.0;
        let nav_sum_w: f32 = nav_items
            .iter()
            .map(|(_, key, label, _)| nav_link_width(ui, *key, label))
            .sum();
        let gaps = nav_items.len().saturating_sub(1) as f32;
        let single_row_spacing = if prompt_w + 16.0 + nav_sum_w + gaps * 14.0 <= inner_w {
            Some(14.0)
        } else if prompt_w + 12.0 + nav_sum_w + gaps * 8.0 <= inner_w {
            Some(8.0)
        } else {
            None
        };

        let draw_prompt = |ui: &mut Ui| {
            ui.label(egui::RichText::new("coyote@vrc:~$ ").font(font::small()).color(pal.dim));
            ui.label(egui::RichText::new(&cmd).font(font::small()).color(pal.contrast));
            let (cur, _) = ui.allocate_exact_size(vec2(7.0, 13.0), Sense::hover());
            if blink {
                ui.painter().rect_filled(cur, w::SQUARE, pal.accent);
            }
        };

        let mut clicked_section = None;
        let mut toggle_theme = false;

        let nav_bar = if let Some(spacing) = single_row_spacing {
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
            draw_prompt(&mut child);
            child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = spacing;
                for (sec, key, label, current) in nav_items.iter().rev() {
                    if nav_link(ui, pal, *key, label, *current) {
                        match sec {
                            Some(s) => clicked_section = Some(*s),
                            None => toggle_theme = true,
                        }
                    }
                }
            });
            bar
        } else {
            let bg_idx = ui.painter().add(egui::Shape::Noop);
            let content_origin = egui::pos2(full.left() + 18.0, full.top() + 8.0);
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_size(content_origin, vec2(inner_w, 200.0)))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.horizontal(|ui| draw_prompt(ui));
            child.add_space(6.0);
            child.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(12.0, 4.0);
                for (sec, key, label, current) in &nav_items {
                    if nav_link(ui, pal, *key, label, *current) {
                        match sec {
                            Some(s) => clicked_section = Some(*s),
                            None => toggle_theme = true,
                        }
                    }
                }
            });
            let bar_h = (child.min_rect().height() + 16.0).max(52.0);
            let bar = Rect::from_min_size(full.min, vec2(full.width(), bar_h));
            ui.painter()
                .set(bg_idx, egui::Shape::rect_filled(bar, w::SQUARE, pal.sheet));
            w::dashed_line(
                ui.painter(),
                bar.left_bottom(),
                bar.right_bottom(),
                Stroke::new(1.0, pal.hair),
                4.0,
                4.0,
            );
            ui.advance_cursor_after_rect(bar);
            bar
        };

        if let Some(s) = clicked_section {
            self.state.section = s;
        }
        if toggle_theme {
            self.state.show_theme_panel = !self.state.show_theme_panel;
        }

        let mut bottom = nav_bar.bottom();
        if snap.remote_enabled && crate::views::remote::has_topbar_badge(snap) {
            let accent = crate::views::remote::topbar_accent(pal, snap);
            let bg_idx = ui.painter().add(egui::Shape::Noop);
            let pad_y = 5.0;
            let content_origin = egui::pos2(full.left() + 18.0, bottom + pad_y);
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_size(content_origin, vec2(inner_w, 160.0)))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            let on_remote = self.state.section == Section::Remote;
            if crate::views::remote::topbar_badge(&mut child, pal, snap, &self.bridge, on_remote) {
                self.state.section = Section::Remote;
            }
            let strip_h = (child.min_rect().height() + pad_y * 2.0).max(32.0);
            let strip = Rect::from_min_size(egui::pos2(full.left(), bottom), vec2(full.width(), strip_h));
            ui.painter()
                .set(bg_idx, egui::Shape::rect_filled(strip, w::SQUARE, pal.faded(accent, 0.91)));
            let rail = Rect::from_min_size(strip.min, vec2(3.0, strip_h));
            ui.painter().rect_filled(rail, w::SQUARE, accent);
            w::dashed_line(
                ui.painter(),
                strip.left_bottom(),
                strip.right_bottom(),
                Stroke::new(1.0, pal.faded(accent, 0.55)),
                4.0,
                4.0,
            );
            ui.advance_cursor_after_rect(strip);
            bottom = strip.bottom();
        }
        bottom
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

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
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
                        Section::Remote => views::remote::show(ui, &mut cx),
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
        ui.label(egui::RichText::new(s.blurb()).font(font::small()).color(pal.dim));
        ui.add_space(space::SHEET_GAP);
    }

    fn statusbar(&mut self, ui: &mut Ui, pal: &Palette, snap: &Snapshot, h: f32) {
        let full = ui.max_rect();
        let bar = Rect::from_min_max(egui::pos2(full.left(), full.bottom() - h), full.max);
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
            ui.label(egui::RichText::new(label).font(font::tiny()).color(pal.dim));
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
            egui::RichText::new(format!("OSC :{}  ·  {} zone(s)", snap.osc_port, snap.avatar_zones.len()))
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

    fn overlays(&mut self, ui: &mut Ui, pal: &Palette, snap: &Snapshot, topbar_bottom: f32) {
        let ctx = ui.ctx().clone();

        if self.state.show_theme_panel {
            self.theme_panel(&ctx, pal, topbar_bottom);
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
            if snap.remote_enabled {
                crate::overlays::remote::show(&ctx, &mut cx);
            }
        }

        if self.tutorial.active {
            self.tutorial.show(&ctx, pal, &mut self.state);
            if !self.tutorial.active {
                self.state.has_seen_tutorial = true;
            }
        }
    }

    fn theme_panel(&mut self, ctx: &egui::Context, pal: &Palette, topbar_bottom: f32) {
        let mut open = true;
        let swatches = cached_theme_swatches();
        egui::Window::new("theme")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::RIGHT_TOP, vec2(-18.0, topbar_bottom + 6.0))
            .frame(egui::Frame::NONE.fill(pal.base).inner_margin(egui::Margin::same(14)))
            .default_width(320.0)
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

                for (t, sw_colors) in crate::theme::THEMES.iter().zip(swatches.iter()) {
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
                    for &c in sw_colors {
                        let sw = Rect::from_min_size(egui::pos2(x - 16.0, inner.center().y - 8.0), vec2(16.0, 16.0));
                        ui.painter().rect_filled(sw.shrink(2.0), w::SQUARE, c);
                        ui.painter().rect_stroke(
                            sw,
                            w::SQUARE,
                            Stroke::new(1.0, if active { pal.accent } else { pal.hair }),
                            StrokeKind::Inside,
                        );
                        x -= 20.0;
                    }
                    if resp.clicked() {
                        self.theme.set(t.id, self.graphics_decision.mode == GraphicsMode::Full);
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
                        if let Some(v) = w::Slider::new(*value, 0.0, max).width(168.0).show(ui, pal) {
                            *value = (v * 10.0).round() / 10.0;
                            changed = true;
                        }
                        w::value_label(ui, pal, &format!("{value:.1}"), 34.0, None);
                    });
                }
                if changed {
                    self.save_prefs();
                }

                ui.add_space(10.0);
                w::dashed_hr(ui, pal.hair);
                ui.add_space(10.0);

                w::caption(ui, pal, "Graphics");
                ui.add_space(4.0);
                let prev_pref = self.graphics_pref;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.graphics_pref, GraphicsPreference::Auto, "Auto");
                    ui.selectable_value(&mut self.graphics_pref, GraphicsPreference::Full, "Full");
                    ui.selectable_value(&mut self.graphics_pref, GraphicsPreference::Lite, "Lite");
                });
                if self.graphics_pref != prev_pref {
                    let machine = self.power_watcher.latest();
                    self.recompute_graphics(ctx, machine);
                    self.save_prefs();
                }
                ui.add_space(4.0);
                let status = self.graphics_decision.status_line(self.graphics_pref);
                ui.label(
                    egui::RichText::new(status)
                        .font(font::small())
                        .color(if self.graphics_decision.mode == GraphicsMode::Full {
                            pal.accent
                        } else {
                            pal.contrast
                        }),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "Lite replaces the animated 3D backdrop with a static dot grid, keeps indicators steady, \
                         disables UI transitions, and throttles passive telemetry redraws. \
                         Auto picks Lite on battery, energy saver, OS animation effects off, software rendering, or slow frames.",
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

fn cached_theme_swatches() -> &'static [[Color32; 3]; crate::theme::THEMES.len()] {
    static SWATCHES: OnceLock<[[Color32; 3]; crate::theme::THEMES.len()]> = OnceLock::new();
    SWATCHES.get_or_init(|| {
        std::array::from_fn(|i| {
            let t = &crate::theme::THEMES[i];
            [
                crate::theme::palette::oklch_to_color(t.accent),
                crate::theme::palette::oklch_to_color(t.contrast),
                crate::theme::palette::oklch_to_color(t.base),
            ]
        })
    })
}

fn has_lit_graph_probe(snap: &Snapshot, tab: GraphTab) -> bool {
    let g = if snap.remote_enabled && tab == GraphTab::Remote {
        &snap.remote_graph
    } else {
        &snap.graph
    };
    g.probe
        .nodes
        .values()
        .any(|p| p.outputs.iter().any(|&v| v.abs() > 0.02))
}

fn has_active_telemetry(snap: &Snapshot, bridge: &Bridge) -> bool {
    let has_toast = bridge.shared.toast.read().ok().is_some_and(|t| t.is_some());
    let downloading = bridge.shared.update.read().ok().is_some_and(|u| u.downloading);
    let device_online = snap.devices.iter().any(|d| d.connected || d.reconnecting);
    device_online
        || snap.vrchat_found
        || snap.alarm_status.phase != AlarmPhase::Idle
        || !snap.remote.service.sessions.is_empty()
        || snap.remote.service.connecting.is_some()
        || has_toast
        || downloading
}

fn nav_link_width(ui: &Ui, key: char, label: &str) -> f32 {
    let key_txt = format!("[{key}] ");
    let font = font::small();
    let kw = ui
        .painter()
        .layout_no_wrap(key_txt, font.clone(), Color32::WHITE)
        .size()
        .x;
    let lw = ui
        .painter()
        .layout_no_wrap(label.to_uppercase(), font, Color32::WHITE)
        .size()
        .x;
    kw + lw
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
    let color = if current || hovered { pal.accent } else { pal.contrast };
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
        p.hline(rect.x_range(), rect.bottom() - 1.0, Stroke::new(1.0, pal.accent));
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn digit_of(key: egui::Key) -> Option<u32> {
    use egui::Key::*;
    Some(match key {
        Num0 => 0,
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
