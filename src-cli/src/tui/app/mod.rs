mod apply;
mod controls;
mod helpers;
mod input;
mod prefs;
mod types;

pub use controls::{
    agg_name, aggregation_modes, channel_control_row, channel_controls, cycle_aggregation,
    cycle_mode, mod_controls_len, mod_function_list, mod_function_list_for, mod_kind_name,
    mod_slot_index, mod_source_list, tuning_control_row, tuning_controls, ChannelControl,
    TuningControl,
};
pub use types::{
    Action, Channel, Clickable, ClickKind, ModKind, ModParam, NormField, PresetSaveField,
    SliderKind, Tab, UkfField, ZonesPane,
};

use std::sync::Arc;
use std::time::{Duration, Instant};

use shocking_vrc_core::cli::{ChannelConfig, CliConfig, CliStatus};
use shocking_vrc_core::modulation::config::ModulationConfig;
use shocking_vrc_core::presets::PresetEntry;
use shocking_vrc_core::ZoneEvent;

use crate::app_state::AppState;
use crate::tui_logger::LogBuffer;

use helpers::default_status;
use prefs::{load_ui_prefs, save_ui_prefs_full};

pub struct App {
    pub state: Arc<AppState>,
    pub log_buffer: LogBuffer,

    pub config: CliConfig,
    pub status: CliStatus,
    pub avatar_zones: Vec<ZoneEvent>,
    pub vrchat_found: bool,
    pub osc_port: u16,
    pub osc_port_editing: bool,
    pub osc_port_input: String,
    pub device_battery: Option<u8>,

    pub active_tab: Tab,
    pub should_quit: bool,

    pub clickables: Vec<Clickable>,

    pub zones_pane: ZonesPane,
    pub sel_conf_a: usize,
    pub sel_conf_b: usize,
    pub sel_avatar: usize,

    pub channel_focus: usize,
    pub tuning_focus: usize,
    pub mod_focus: usize,

    pub mod_channel: Channel,
    pub mod_kind: ModKind,
    pub mod_seg: usize,
    pub mod_editor: ModulationConfig,

    pub mod_function_picker: bool,
    pub mod_func_pick_ix: usize,
    pub mod_func_pick_scroll: u16,
    pub mod_func_pick_viewport: u16,

    pub log_scroll: usize,

    pub channels_scroll: u16,
    pub channels_viewport_h: u16,
    pub mod_slots_scroll: u16,
    pub mod_slots_viewport_h: u16,
    pub mod_editor_scroll: u16,
    pub mod_editor_viewport_h: u16,
    pub tuning_scroll: u16,
    pub tuning_viewport_h: u16,
    pub mod_split_x: u16,

    pub button_flash: Option<(Action, Instant)>,

    pub auto_save: bool,

    pub preset_entries: Vec<PresetEntry>,
    pub sel_preset: usize,
    pub preset_scroll: u16,
    pub presets_viewport_h: u16,
    pub presets_loading: bool,
    pub presets_error: Option<String>,
    pub presets_source: Option<String>,

    pub preset_save_editing: bool,
    pub preset_save_channel: Channel,
    pub preset_save_input: String,
    pub preset_save_nickname: String,
    pub preset_save_field: PresetSaveField,

    pub preset_delete_confirm: Option<usize>,

    pub tutorial_active: bool,
    pub tutorial_step: crate::tui::tutorial::steps::TutorialStep,
    tutorial_saved_tab: Tab,
    tutorial_saved_config: Option<CliConfig>,
    tutorial_saved_zones: Vec<ZoneEvent>,

    last_refresh: Instant,
}

impl App {
    pub fn new(state: Arc<AppState>, log_buffer: LogBuffer) -> Self {
        use std::sync::atomic::Ordering;
        state.monitor_enabled.store(true, Ordering::Relaxed);
        let mut app = App {
            state,
            log_buffer,
            config: CliConfig::default(),
            status: default_status(),
            avatar_zones: Vec::new(),
            vrchat_found: false,
            osc_port: 9001,
            osc_port_editing: false,
            osc_port_input: String::new(),
            device_battery: None,
            active_tab: Tab::Status,
            should_quit: false,
            clickables: Vec::new(),
            zones_pane: ZonesPane::ConfiguredA,
            sel_conf_a: 0,
            sel_conf_b: 0,
            sel_avatar: 0,
            channel_focus: 0,
            tuning_focus: 0,
            mod_focus: 0,
            mod_channel: Channel::A,
            mod_kind: ModKind::Freq,
            mod_seg: 0,
            mod_editor: ModulationConfig::default(),
            mod_function_picker: false,
            mod_func_pick_ix: 0,
            mod_func_pick_scroll: 0,
            mod_func_pick_viewport: 0,
            log_scroll: 0,
            channels_scroll: 0,
            channels_viewport_h: 0,
            mod_slots_scroll: 0,
            mod_slots_viewport_h: 0,
            mod_editor_scroll: 0,
            mod_editor_viewport_h: 0,
            tuning_scroll: 0,
            tuning_viewport_h: 0,
            mod_split_x: 0,
            button_flash: None,
            auto_save: false,
            preset_entries: Vec::new(),
            sel_preset: 0,
            preset_scroll: 0,
            presets_viewport_h: 0,
            presets_loading: false,
            presets_error: None,
            presets_source: None,
            preset_save_editing: false,
            preset_save_channel: Channel::A,
            preset_save_input: String::new(),
            preset_save_nickname: String::new(),
            preset_save_field: PresetSaveField::Name,
            preset_delete_confirm: None,
            tutorial_active: false,
            tutorial_step: crate::tui::tutorial::steps::TutorialStep::Welcome,
            tutorial_saved_tab: Tab::Status,
            tutorial_saved_config: None,
            tutorial_saved_zones: Vec::new(),
            last_refresh: Instant::now(),
        };
        app.load_editor_from_config();
        let p = load_ui_prefs();
        app.auto_save = p.auto_save;
        app.preset_save_nickname = p.nickname;
        if !p.has_seen_tutorial {
            app.start_tutorial();
        }
        app
    }

    pub async fn refresh_all(&mut self) {
        self.config = self.state.engine.config().await;
        self.status = self.state.engine.current_status().await;
        self.avatar_zones = self.state.scanner.zones().await;
        self.vrchat_found = self.state.scanner.vrchat_address().await.is_some();
        self.osc_port = self.state.scanner.port().await;
        self.device_battery = self.state.battery_level.read().ok().and_then(|g| *g);
        self.last_refresh = Instant::now();
        self.load_editor_from_config();
    }

    pub async fn maybe_refresh(&mut self) {
        if self.last_refresh.elapsed() >= Duration::from_millis(800) {
            self.config = self.state.engine.config().await;
            self.avatar_zones = self.state.scanner.zones().await;
            self.vrchat_found = self.state.scanner.vrchat_address().await.is_some();
            self.device_battery = self.state.battery_level.read().ok().and_then(|g| *g);
            self.last_refresh = Instant::now();
        }
    }

    pub(super) async fn refresh_config(&mut self) {
        self.config = self.state.engine.config().await;
    }
}

impl App {
    pub fn push_click(&mut self, rect: ratatui::layout::Rect, action: Action) {
        self.clickables.push(Clickable {
            rect,
            kind: ClickKind::Act(action),
        });
    }

    pub fn push_slider(&mut self, rect: ratatui::layout::Rect, kind: SliderKind) {
        self.clickables.push(Clickable {
            rect,
            kind: ClickKind::Slider(kind),
        });
    }

    pub fn flash_button(&mut self, action: Action) {
        self.button_flash = Some((action, Instant::now()));
    }

    pub fn is_button_flashing(&self, action: &Action) -> bool {
        self.button_flash
            .as_ref()
            .is_some_and(|(a, t)| a == action && t.elapsed() < Duration::from_millis(200))
    }

    pub fn button_lit(&self, action: &Action, keyboard_focus: bool) -> bool {
        self.is_button_flashing(action) || keyboard_focus
    }

    pub fn choice_emphasis(&self, action: &Action, selected: bool, row_focused: bool) -> bool {
        self.is_button_flashing(action) || (row_focused && selected)
    }
}

impl App {
    pub fn channel_config(&self, ch: Channel) -> &ChannelConfig {
        match ch {
            Channel::A => &self.config.channel_a,
            Channel::B => &self.config.channel_b,
        }
    }

    pub fn mod_preview_base(&self) -> f32 {
        let ch = self.channel_config(self.mod_channel);
        match self.mod_kind {
            ModKind::Freq => ch.frequency[self.mod_seg] as f32,
            ModKind::Intensity => ch.intensity[self.mod_seg] as f32,
        }
    }

    pub(super) fn load_editor_from_config(&mut self) {
        let ch = self.channel_config(self.mod_channel);
        let slot = match self.mod_kind {
            ModKind::Freq => &ch.freq_modulation[self.mod_seg],
            ModKind::Intensity => &ch.intensity_modulation[self.mod_seg],
        };
        self.mod_editor = match slot {
            Some(c) => c.clone(),
            None => {
                let mut c = ModulationConfig::default();
                if self.mod_kind == ModKind::Intensity {
                    c.max_deviation = 10.0;
                }
                c
            }
        };
        self.mod_editor.sanitise(self.mod_kind == ModKind::Intensity);
    }

    pub(super) fn sanitise_mod_editor(&mut self) {
        self.mod_editor.sanitise(self.mod_kind == ModKind::Intensity);
    }

    pub(super) fn clamp_selections(&mut self) {
        let la = self.config.channel_a.zones.len();
        let lb = self.config.channel_b.zones.len();
        self.sel_conf_a = if la > 0 { self.sel_conf_a.min(la - 1) } else { 0 };
        self.sel_conf_b = if lb > 0 { self.sel_conf_b.min(lb - 1) } else { 0 };
    }
}

impl App {
    pub(super) fn sync_channels_scroll(&mut self) {
        let Some(ctrl) = channel_controls().get(self.channel_focus).copied() else { return };
        let Some(row) = channel_control_row(ctrl) else { return };
        self.channels_scroll =
            crate::tui::ui::scroll_to_row(self.channels_scroll, self.channels_viewport_h, row);
    }

    pub(super) fn sync_tuning_scroll(&mut self) {
        let Some(ctrl) = tuning_controls().get(self.tuning_focus).copied() else { return };
        let row = tuning_control_row(ctrl);
        self.tuning_scroll =
            crate::tui::ui::scroll_to_row(self.tuning_scroll, self.tuning_viewport_h, row);
    }

    pub(super) fn sync_mod_editor_scroll(&mut self) {
        let row = self.mod_focus as u16;
        self.mod_editor_scroll =
            crate::tui::ui::scroll_to_row(self.mod_editor_scroll, self.mod_editor_viewport_h, row);
    }

    pub(super) fn sync_mod_slots_scroll(&mut self) {
        let idx = mod_slot_index(self.mod_channel, self.mod_kind, self.mod_seg);
        self.mod_slots_scroll =
            crate::tui::ui::scroll_to_row(self.mod_slots_scroll, self.mod_slots_viewport_h, idx);
    }

    pub(super) fn sync_presets_scroll(&mut self) {
        let content_h = self.preset_entries.len() as u16;
        self.preset_scroll = crate::tui::ui::clamp_scroll(
            crate::tui::ui::scroll_to_row(
                self.preset_scroll,
                self.presets_viewport_h,
                self.sel_preset as u16,
            ),
            content_h,
            self.presets_viewport_h,
        );
    }
}

impl App {
    pub(super) fn cancel_preset_save_edit(&mut self) {
        self.preset_save_editing = false;
        self.preset_save_input.clear();
    }

    fn start_preset_save_edit(&mut self, ch: Channel) {
        self.preset_save_channel = ch;
        self.preset_save_input.clear();
        self.preset_save_field = PresetSaveField::Name;
        self.preset_save_editing = true;
    }

    async fn commit_preset_save_edit(&mut self) {
        let ch = self.preset_save_channel;
        let name = if self.preset_save_input.trim().is_empty() {
            crate::presets::random_preset_name(ch.label())
        } else {
            self.preset_save_input.trim().to_string()
        };
        let author = self.preset_save_nickname.trim().to_string();
        if !author.is_empty() {
            let mut p = load_ui_prefs();
            p.nickname = author.clone();
            let _ = save_ui_prefs_full(&p);
        }
        let ch_cfg = self.channel_config(ch).clone();
        match crate::presets::save_user_preset(&ch_cfg, &name, &author) {
            Ok(entry) => {
                log::info!(
                    "[presets] Saved \"{}\" by \"{}\" from ch {} → presets/user/",
                    entry.name,
                    author,
                    ch.label()
                );
                self.cancel_preset_save_edit();
                self.refresh_presets().await;
                if let Some(i) = self.preset_entries.iter().position(|e| e.id == entry.id) {
                    self.sel_preset = i;
                    self.sync_presets_scroll();
                }
            }
            Err(e) => log::error!("[presets] Save failed: {e}"),
        }
    }

    async fn handle_preset_save_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        use ratatui::crossterm::event::KeyCode;
        match key.code {
            KeyCode::Enter => self.commit_preset_save_edit().await,
            KeyCode::Tab => {
                self.preset_save_field = match self.preset_save_field {
                    PresetSaveField::Name => PresetSaveField::Nickname,
                    PresetSaveField::Nickname => PresetSaveField::Name,
                };
            }
            KeyCode::Backspace => match self.preset_save_field {
                PresetSaveField::Name => { self.preset_save_input.pop(); }
                PresetSaveField::Nickname => { self.preset_save_nickname.pop(); }
            },
            KeyCode::Char(c) if !c.is_control() => match self.preset_save_field {
                PresetSaveField::Name if self.preset_save_input.len() < 48 => {
                    self.preset_save_input.push(c);
                }
                PresetSaveField::Nickname if self.preset_save_nickname.len() < 32 => {
                    self.preset_save_nickname.push(c);
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub(super) fn cancel_preset_delete_confirm(&mut self) {
        self.preset_delete_confirm = None;
    }

    async fn confirm_preset_delete(&mut self) {
        let Some(i) = self.preset_delete_confirm.take() else { return };
        let Some(entry) = self.preset_entries.get(i) else { return };
        if !entry.user { return; }
        let id = entry.id.clone();
        let name = entry.name.clone();
        match crate::presets::delete_user_preset(&id) {
            Ok(()) => {
                log::info!("[presets] Deleted \"{name}\" from presets/user/");
                self.refresh_presets().await;
                if !self.preset_entries.is_empty() {
                    self.sel_preset = self.sel_preset.min(self.preset_entries.len() - 1);
                } else {
                    self.sel_preset = 0;
                }
                self.sync_presets_scroll();
            }
            Err(e) => log::error!("[presets] Delete failed: {e}"),
        }
    }

    pub async fn refresh_presets(&mut self) {
        self.presets_loading = true;
        self.presets_error = None;
        match crate::presets::load_catalog().await {
            Ok(result) => {
                let count = result.entries.len();
                self.preset_entries = result.entries;
                self.presets_source = Some(result.source);
                self.sel_preset = 0;
                self.preset_scroll = 0;
                if !self.preset_entries.is_empty() {
                    self.sel_preset = self.sel_preset.min(self.preset_entries.len() - 1);
                }
                log::info!("[presets] Loaded {count} preset(s)");
            }
            Err(e) => {
                self.presets_error = Some(e.clone());
                log::warn!("[presets] {e}");
            }
        }
        self.presets_loading = false;
    }

    async fn maybe_load_presets(&mut self) {
        if self.active_tab == Tab::Presets
            && self.preset_entries.is_empty()
            && !self.presets_loading
        {
            self.refresh_presets().await;
        }
    }

    async fn apply_selected_preset(&mut self, ch: Channel) {
        if self.presets_loading { return; }
        let Some(entry) = self.preset_entries.get(self.sel_preset).cloned() else {
            log::warn!("[presets] No preset selected");
            return;
        };
        let mut cfg = self.state.engine.config().await;
        let ch_cfg = match ch {
            Channel::A => &mut cfg.channel_a,
            Channel::B => &mut cfg.channel_b,
        };
        entry.preset.apply_to(ch_cfg);
        self.state.engine.set_config(cfg).await;
        log::info!("[ch-{}] Preset applied: {} (limits unchanged)", ch.label(), entry.name);
        self.refresh_config().await;
        self.load_editor_from_config();
    }
}

impl App {
    pub fn start_tutorial(&mut self) {
        use crate::tui::tutorial::{sandbox, steps::TutorialStep};
        self.tutorial_saved_tab = self.active_tab;
        self.tutorial_saved_config = Some(self.config.clone());
        self.tutorial_saved_zones = self.avatar_zones.clone();
        self.config = sandbox::demo_config();
        self.avatar_zones = sandbox::demo_avatar_zones();
        self.vrchat_found = true;
        self.device_battery = Some(75);
        self.status.device_connected = true;
        self.tutorial_active = true;
        self.tutorial_step = TutorialStep::Welcome;
        self.active_tab = Tab::Status;
    }

    pub(super) fn tutorial_next(&mut self) {
        use crate::tui::tutorial::steps::TutorialStep;
        let all = TutorialStep::ALL;
        let idx = self.tutorial_step.index();
        if idx + 1 < all.len() {
            self.tutorial_step = all[idx + 1];
            if let Some(tab) = self.tutorial_step.tab() {
                self.active_tab = tab;
            }
        } else {
            self.close_tutorial();
        }
    }

    pub(super) fn tutorial_prev(&mut self) {
        use crate::tui::tutorial::steps::TutorialStep;
        let all = TutorialStep::ALL;
        let idx = self.tutorial_step.index();
        if idx > 0 {
            self.tutorial_step = all[idx - 1];
            if let Some(tab) = self.tutorial_step.tab() {
                self.active_tab = tab;
            }
        }
    }

    pub(super) fn close_tutorial(&mut self) {
        self.tutorial_active = false;
        if let Some(cfg) = self.tutorial_saved_config.take() {
            self.config = cfg;
        }
        self.avatar_zones = std::mem::take(&mut self.tutorial_saved_zones);
        self.active_tab = self.tutorial_saved_tab;
        self.load_editor_from_config();
        let mut p = load_ui_prefs();
        p.has_seen_tutorial = true;
        let _ = save_ui_prefs_full(&p);
    }
}

impl App {
    pub(super) fn cancel_osc_port_edit(&mut self) {
        self.osc_port_editing = false;
        self.osc_port_input.clear();
    }

    fn start_osc_port_edit(&mut self) {
        self.osc_port_input = self.osc_port.to_string();
        self.osc_port_editing = true;
    }

    async fn commit_osc_port_edit(&mut self) {
        if self.osc_port_input.is_empty() {
            self.cancel_osc_port_edit();
            return;
        }
        let Ok(port) = self.osc_port_input.parse::<u16>() else {
            log::warn!("[osc] Invalid port — use digits only (1024–65535)");
            return;
        };
        if !(1024..=65535).contains(&port) {
            log::warn!("[osc] Port {port} out of range (1024–65535)");
            return;
        }
        self.set_osc_port(port).await;
        self.cancel_osc_port_edit();
    }

    async fn set_osc_port(&mut self, new_port: u16) {
        if new_port == self.osc_port { return; }
        match self.state.scanner.set_port(new_port).await {
            Ok(()) => {
                self.osc_port = new_port;
                log::info!("[osc] Listener restarted on UDP port {new_port}");
            }
            Err(e) => log::error!("[osc] Failed to change port: {e}"),
        }
    }

    async fn handle_osc_port_edit_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        use ratatui::crossterm::event::KeyCode;
        match key.code {
            KeyCode::Enter => self.commit_osc_port_edit().await,
            KeyCode::Backspace => { self.osc_port_input.pop(); }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                if self.osc_port_input.len() >= 5 { return; }
                self.osc_port_input.push(c);
                if self.osc_port_input.parse::<u32>().unwrap_or(0) > 65535 {
                    self.osc_port_input.pop();
                }
            }
            _ => {}
        }
    }
}

impl App {
    pub(super) fn close_mod_function_picker(&mut self) {
        self.mod_function_picker = false;
    }

    fn open_mod_function_picker(&mut self) {
        let list = mod_function_list_for(&self.mod_editor.function);
        self.mod_func_pick_ix = list
            .iter()
            .position(|f| f == &self.mod_editor.function)
            .unwrap_or(0);
        self.mod_func_pick_scroll = 0;
        self.mod_function_picker = true;
        self.sync_mod_func_pick_scroll();
    }

    pub(super) fn move_mod_func_pick(&mut self, delta: i32) {
        let len = mod_function_list_for(&self.mod_editor.function).len();
        if len == 0 { return; }
        self.mod_func_pick_ix = helpers::step_index(self.mod_func_pick_ix, delta, len);
        self.sync_mod_func_pick_scroll();
    }

    fn sync_mod_func_pick_scroll(&mut self) {
        self.mod_func_pick_scroll = crate::tui::ui::scroll_to_row(
            self.mod_func_pick_scroll,
            self.mod_func_pick_viewport,
            self.mod_func_pick_ix as u16,
        );
    }

    fn pick_mod_function(&mut self, index: usize) {
        let list = mod_function_list_for(&self.mod_editor.function);
        if let Some(f) = list.get(index) {
            self.mod_editor.function = f.clone();
        }
        self.close_mod_function_picker();
    }

    async fn handle_mod_function_picker_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        use ratatui::crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc => self.close_mod_function_picker(),
            KeyCode::Enter | KeyCode::Char(' ') => {
                let ix = self.mod_func_pick_ix;
                self.pick_mod_function(ix);
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_mod_func_pick(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_mod_func_pick(1),
            KeyCode::Home => {
                self.mod_func_pick_ix = 0;
                self.sync_mod_func_pick_scroll();
            }
            KeyCode::End => {
                let len = mod_function_list_for(&self.mod_editor.function).len();
                if len > 0 {
                    self.mod_func_pick_ix = len - 1;
                    self.sync_mod_func_pick_scroll();
                }
            }
            _ => {}
        }
    }
}
