use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use shocking_vrc_core::cli::{
    AggregationMode, CliConfig, CliStatus, ContactMode, MotionNorms, PowerLimits, UkfConfig,
    ZoneEntry, ZoneId,
};
use shocking_vrc_core::modulation::config::{
    ModulationConfig, ModulationFunction, ModulationSource,
};
use shocking_vrc_core::ZoneEvent;

use crate::app_state::AppState;
use crate::tui_logger::LogBuffer;

const CONFIG_FILE: &str = "cli_config.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    A,
    B,
}

impl Channel {
    pub fn label(self) -> &'static str {
        match self {
            Channel::A => "A",
            Channel::B => "B",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Status,
    Zones,
    Channels,
    Tuning,
    Modulation,
    Log,
    Setup,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Status,
        Tab::Zones,
        Tab::Channels,
        Tab::Tuning,
        Tab::Modulation,
        Tab::Log,
        Tab::Setup,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Status => "Status",
            Tab::Zones => "Zones",
            Tab::Channels => "Channels",
            Tab::Tuning => "Tuning",
            Tab::Modulation => "Modulation",
            Tab::Log => "Log",
            Tab::Setup => "Setup",
        }
    }

    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZonesPane {
    ConfiguredA,
    ConfiguredB,
    Avatar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKind {
    Freq,
    Intensity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UkfField {
    Q,
    R,
    Alpha,
    Beta,
    Kappa,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormField {
    Speed,
    Acc,
    Recoil,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModParam {
    BaseSpeed,
    Sensitivity,
    MaxDeviation,
    Phase,
    FreqMul,
    Offset,
    Power,
    ClampMin,
    ClampMax,
}

impl ModParam {
    pub const ALL: [ModParam; 9] = [
        ModParam::BaseSpeed,
        ModParam::Sensitivity,
        ModParam::MaxDeviation,
        ModParam::Phase,
        ModParam::FreqMul,
        ModParam::Offset,
        ModParam::Power,
        ModParam::ClampMin,
        ModParam::ClampMax,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ModParam::BaseSpeed => "base_speed",
            ModParam::Sensitivity => "sensitivity",
            ModParam::MaxDeviation => "max_deviation",
            ModParam::Phase => "phase",
            ModParam::FreqMul => "freq_mul",
            ModParam::Offset => "offset",
            ModParam::Power => "power",
            ModParam::ClampMin => "clamp_min",
            ModParam::ClampMax => "clamp_max",
        }
    }

    pub fn step(self) -> f32 {
        match self {
            ModParam::BaseSpeed
            | ModParam::Sensitivity
            | ModParam::Phase
            | ModParam::FreqMul
            | ModParam::Power => 0.1,
            ModParam::MaxDeviation
            | ModParam::Offset
            | ModParam::ClampMin
            | ModParam::ClampMax => 1.0,
        }
    }

    pub fn get(self, c: &ModulationConfig) -> f32 {
        match self {
            ModParam::BaseSpeed => c.base_speed,
            ModParam::Sensitivity => c.sensitivity,
            ModParam::MaxDeviation => c.max_deviation,
            ModParam::Phase => c.phase,
            ModParam::FreqMul => c.frequency_multiplier,
            ModParam::Offset => c.offset,
            ModParam::Power => c.power,
            ModParam::ClampMin => c.clamp_min,
            ModParam::ClampMax => c.clamp_max,
        }
    }

    pub fn set(self, c: &mut ModulationConfig, v: f32) {
        match self {
            ModParam::BaseSpeed => c.base_speed = v,
            ModParam::Sensitivity => c.sensitivity = v,
            ModParam::MaxDeviation => c.max_deviation = v,
            ModParam::Phase => c.phase = v,
            ModParam::FreqMul => c.frequency_multiplier = v,
            ModParam::Offset => c.offset = v,
            ModParam::Power => c.power = v,
            ModParam::ClampMin => c.clamp_min = v,
            ModParam::ClampMax => c.clamp_max = v,
        }
    }

    pub fn clamp_value(self, v: f32, kind: ModKind) -> f32 {
        let intensity = kind == ModKind::Intensity;
        let (lo_limit, hi_limit) = ModulationConfig::output_bounds(intensity);
        match self {
            ModParam::Offset => v.clamp(-hi_limit, hi_limit),
            ModParam::ClampMin | ModParam::ClampMax => v.clamp(lo_limit, hi_limit),
            ModParam::BaseSpeed
            | ModParam::Sensitivity
            | ModParam::MaxDeviation
            | ModParam::Power => v.max(0.0),
            ModParam::Phase => v,
            ModParam::FreqMul => v.max(0.001),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SliderKind {
    Freq(Channel, usize),
    Intensity(Channel, usize),
    LimitMin(Channel),
    LimitMax(Channel),
}

impl SliderKind {
    pub fn range(self) -> (i32, i32) {
        match self {
            SliderKind::Freq(..) => (10, 255),
            SliderKind::Intensity(..) => (0, 100),
            SliderKind::LimitMin(..) | SliderKind::LimitMax(..) => (0, 200),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Quit,
    SwitchTab(Tab),
    ToggleMonitor,
    StepOscPort(i32),
    StartOscPortEdit,

    FocusZonesPane(ZonesPane),
    SelectConfigured(Channel, usize),
    SelectAvatar(usize),
    RemoveZone(Channel, usize),
    CycleMode(Channel, usize),
    AddAvatarZone(Channel, usize),
    AddAllZones(Channel),

    SetFreq(Channel, usize, u8),
    StepFreq(Channel, usize, i32),
    SetIntensity(Channel, usize, u8),
    StepIntensity(Channel, usize, i32),
    SetLimitMin(Channel, u8),
    SetLimitMax(Channel, u8),
    StepLimitMin(Channel, i32),
    StepLimitMax(Channel, i32),
    CycleAggregation(Channel),
    SetAggregation(Channel, AggregationMode),
    SaveConfig,
    LoadConfig,

    StepUkf(UkfField, i32),
    ResetUkf,
    StepNorm(NormField, i32),
    ResetNorms,

    SelectModSlot(Channel, ModKind, usize),
    OpenModFunctionPicker,
    PickModFunction(usize),
    CloseModFunctionPicker,
    SetModSource(ModulationSource),
    CycleModFunction(i32),
    CycleModSource(i32),
    StepModParam(ModParam, i32),
    ApplyMod,
    ClearMod,
    ClearAllMod(Channel),

    FocusNext,
    FocusPrev,
    AdjustFocused(i32),
    ActivateFocused,
}

pub struct Clickable {
    pub rect: Rect,
    pub kind: ClickKind,
}

pub enum ClickKind {
    Act(Action),
    Slider(SliderKind),
}

pub fn mod_function_list() -> Vec<ModulationFunction> {
    use ModulationFunction::*;
    vec![
        None,
        Sin,
        Cos,
        Tan,
        SinCos,
        Sin2,
        Cos2,
        SinPlusCos,
        Sinh,
        Cosh,
        Tanh,
        Square,
        Cube,
        Pow4,
        Sqrt,
        Cbrt,
        Abs,
        Sign,
        Exp,
        ExpNeg,
        Pow2x,
        Pow10x,
        Ln,
        Log2,
        Log10,
        Triangle,
        Saw,
        ReverseSaw,
        SquareWave,
        Pulse,
        Bounce,
        Sigmoid,
        SmoothStep,
        SmootherStep,
        Logistic,
        SoftSign,
        Perlin,
        Simplex,
        Fractal,
        ValueNoise,
        SinPlusNoise,
        SinTimesNoise,
        TrianglePlusSin,
        SquareTimesSigmoid,
    ]
}

pub fn mod_function_list_for(current: &ModulationFunction) -> Vec<ModulationFunction> {
    let mut list = mod_function_list();
    if !list.iter().any(|f| f == current) {
        list.insert(1, current.clone());
    }
    list
}

const MOD_SOURCES: [ModulationSource; 4] = [
    ModulationSource::Depth,
    ModulationSource::Speed,
    ModulationSource::Acc,
    ModulationSource::Recoil,
];

pub fn mod_source_list() -> &'static [ModulationSource; 4] {
    &MOD_SOURCES
}

fn default_status() -> CliStatus {
    use shocking_vrc_core::cli::ChannelStatus;
    let empty = || ChannelStatus {
        raw_level: 0.0,
        strength: 0,
        frequency: [0; 4],
        active_zones: Vec::new(),
        kinematics: Default::default(),
    };
    CliStatus {
        channel_a: empty(),
        channel_b: empty(),
        device_connected: false,
    }
}

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
    pub monitor: bool,

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

    last_refresh: Instant,
}

impl App {
    pub fn new(state: Arc<AppState>, log_buffer: LogBuffer) -> Self {
        let monitor = state.monitor_enabled.load(Ordering::Relaxed);
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
            monitor,
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
            last_refresh: Instant::now(),
        };
        app.load_editor_from_config();
        app
    }

    pub async fn refresh_all(&mut self) {
        self.config = self.state.engine.config().await;
        self.status = self.state.engine.current_status().await;
        self.avatar_zones = self.state.scanner.zones().await;
        self.vrchat_found = self.state.scanner.vrchat_address().await.is_some();
        self.osc_port = self.state.scanner.port().await;
        self.device_battery = self
            .state
            .battery_level
            .read()
            .ok()
            .and_then(|g| *g);
        self.last_refresh = Instant::now();
        self.load_editor_from_config();
    }

    pub async fn maybe_refresh(&mut self) {
        if self.last_refresh.elapsed() >= Duration::from_millis(800) {
            self.config = self.state.engine.config().await;
            self.avatar_zones = self.state.scanner.zones().await;
            self.vrchat_found = self.state.scanner.vrchat_address().await.is_some();
            self.device_battery = self
            .state
            .battery_level
            .read()
            .ok()
            .and_then(|g| *g);
            self.last_refresh = Instant::now();
        }
    }

    async fn refresh_config(&mut self) {
        self.config = self.state.engine.config().await;
    }

    fn cancel_osc_port_edit(&mut self) {
        self.osc_port_editing = false;
        self.osc_port_input.clear();
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
        if new_port == self.osc_port {
            return;
        }
        match self.state.scanner.set_port(new_port).await {
            Ok(()) => {
                self.osc_port = new_port;
                log::info!("[osc] Listener restarted on UDP port {new_port}");
            }
            Err(e) => log::error!("[osc] Failed to change port: {e}"),
        }
    }

    fn start_osc_port_edit(&mut self) {
        self.osc_port_input = self.osc_port.to_string();
        self.osc_port_editing = true;
    }

    async fn handle_osc_port_edit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.commit_osc_port_edit().await,
            KeyCode::Backspace => {
                self.osc_port_input.pop();
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                if self.osc_port_input.len() >= 5 {
                    return;
                }
                self.osc_port_input.push(c);
                if self.osc_port_input.parse::<u32>().unwrap_or(0) > 65535 {
                    self.osc_port_input.pop();
                }
            }
            _ => {}
        }
    }

    fn close_mod_function_picker(&mut self) {
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

    fn move_mod_func_pick(&mut self, delta: i32) {
        let len = mod_function_list_for(&self.mod_editor.function).len();
        if len == 0 {
            return;
        }
        self.mod_func_pick_ix = step_index(self.mod_func_pick_ix, delta, len);
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

    async fn handle_mod_function_picker_key(&mut self, key: KeyEvent) {
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

    pub fn push_click(&mut self, rect: Rect, action: Action) {
        self.clickables.push(Clickable {
            rect,
            kind: ClickKind::Act(action),
        });
    }

    pub fn push_slider(&mut self, rect: Rect, kind: SliderKind) {
        self.clickables.push(Clickable {
            rect,
            kind: ClickKind::Slider(kind),
        });
    }

    pub fn channel_config(&self, ch: Channel) -> &shocking_vrc_core::cli::ChannelConfig {
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

    fn load_editor_from_config(&mut self) {
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
        self.mod_editor
            .sanitise(self.mod_kind == ModKind::Intensity);
    }

    fn sanitise_mod_editor(&mut self) {
        self.mod_editor
            .sanitise(self.mod_kind == ModKind::Intensity);
    }

    pub async fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            KeyCode::Esc => {
                if self.mod_function_picker {
                    self.close_mod_function_picker();
                    return;
                }
                if self.osc_port_editing {
                    self.cancel_osc_port_edit();
                    return;
                }
                self.should_quit = true;
                return;
            }
            _ => {}
        }

        if self.mod_function_picker {
            self.handle_mod_function_picker_key(key).await;
            return;
        }

        if self.osc_port_editing {
            match key.code {
                KeyCode::Tab => {
                    self.switch_tab_relative(1);
                    return;
                }
                KeyCode::BackTab => {
                    self.switch_tab_relative(-1);
                    return;
                }
                _ => {
                    self.handle_osc_port_edit_key(key).await;
                    return;
                }
            }
        }

        match key.code {
            KeyCode::Tab => {
                self.switch_tab_relative(1);
                return;
            }
            KeyCode::BackTab => {
                self.switch_tab_relative(-1);
                return;
            }
            KeyCode::Char(c @ '1'..='7') => {
                self.close_mod_function_picker();
                let idx = c as usize - '1' as usize;
                if let Some(t) = Tab::ALL.get(idx) {
                    self.active_tab = *t;
                    self.channels_scroll = 0;
                    self.mod_slots_scroll = 0;
                    self.mod_editor_scroll = 0;
                    self.tuning_scroll = 0;
                }
                return;
            }
            _ => {}
        }

        if self.active_tab == Tab::Log {
            match key.code {
                KeyCode::Up => {
                    self.on_scroll_at(-1, None);
                    return;
                }
                KeyCode::Down => {
                    self.on_scroll_at(1, None);
                    return;
                }
                _ => {}
            }
        }

        if let Some(action) = self.map_key_to_action(key) {
            let concrete = match action {
                Action::FocusNext => {
                    self.move_focus(1);
                    None
                }
                Action::FocusPrev => {
                    self.move_focus(-1);
                    None
                }
                Action::AdjustFocused(d) => self.focused_adjust_action(d),
                Action::ActivateFocused => self.focused_activate_action(),
                other => Some(other),
            };
            if let Some(a) = concrete {
                self.apply(a).await;
            }
        }
    }

    fn switch_tab_relative(&mut self, delta: i32) {
        self.cancel_osc_port_edit();
        self.close_mod_function_picker();
        let n = Tab::ALL.len() as i32;
        let idx = (self.active_tab.index() as i32 + delta).rem_euclid(n) as usize;
        self.active_tab = Tab::ALL[idx];
    }

    fn map_key_to_action(&self, key: KeyEvent) -> Option<Action> {
        match self.active_tab {
            Tab::Status => match key.code {
                KeyCode::Char('m') => Some(Action::ToggleMonitor),
                _ => None,
            },
            Tab::Setup => match key.code {
                KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char('E') => {
                    Some(Action::StartOscPortEdit)
                }
                KeyCode::Char('[') => Some(Action::StepOscPort(-1)),
                KeyCode::Char(']') => Some(Action::StepOscPort(1)),
                _ => None,
            },
            Tab::Zones => match key.code {
                KeyCode::Up => Some(Action::FocusPrev),
                KeyCode::Down => Some(Action::FocusNext),
                KeyCode::Left => Some(Action::FocusZonesPane(ZonesPane::ConfiguredA)),
                KeyCode::Right => Some(Action::FocusZonesPane(ZonesPane::Avatar)),
                KeyCode::Char('a') => Some(Action::FocusZonesPane(ZonesPane::ConfiguredA)),
                KeyCode::Char('b') => Some(Action::FocusZonesPane(ZonesPane::ConfiguredB)),
                KeyCode::Char('v') => Some(Action::FocusZonesPane(ZonesPane::Avatar)),
                KeyCode::Char('m') => Some(Action::ActivateFocused), // cycle mode / add
                KeyCode::Char('x') | KeyCode::Delete => Some(match self.zones_pane {
                    ZonesPane::ConfiguredA => Action::RemoveZone(Channel::A, self.sel_conf_a),
                    ZonesPane::ConfiguredB => Action::RemoveZone(Channel::B, self.sel_conf_b),
                    ZonesPane::Avatar => Action::AddAvatarZone(Channel::B, self.sel_avatar),
                }),
                KeyCode::Enter | KeyCode::Char(' ') => Some(Action::ActivateFocused),
                _ => None,
            },
            Tab::Channels | Tab::Tuning => match key.code {
                KeyCode::Up => Some(Action::FocusPrev),
                KeyCode::Down => Some(Action::FocusNext),
                KeyCode::Left | KeyCode::Char('-') => {
                    Some(Action::AdjustFocused(-step_multiplier(&key)))
                }
                KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('=') => {
                    Some(Action::AdjustFocused(step_multiplier(&key)))
                }
                KeyCode::Enter | KeyCode::Char(' ') => Some(Action::ActivateFocused),
                _ => None,
            },
            Tab::Modulation => match key.code {
                KeyCode::Up => Some(Action::FocusPrev),
                KeyCode::Down => Some(Action::FocusNext),
                KeyCode::Left | KeyCode::Char('-') => {
                    Some(Action::AdjustFocused(-step_multiplier(&key)))
                }
                KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('=') => {
                    Some(Action::AdjustFocused(step_multiplier(&key)))
                }
                KeyCode::Char('[') => Some(Action::SelectModSlot(
                    self.mod_channel,
                    self.mod_kind,
                    self.mod_seg.wrapping_sub(1).min(3),
                )),
                KeyCode::Char(']') => {
                    Some(Action::SelectModSlot(self.mod_channel, self.mod_kind, (self.mod_seg + 1) % 4))
                }
                KeyCode::Enter | KeyCode::Char(' ') => Some(Action::ActivateFocused),
                _ => None,
            },
            Tab::Log => None,
        }
    }

    pub async fn handle_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left) => {
                let col = m.column;
                let row = m.row;
                let hit = self
                    .clickables
                    .iter()
                    .rev()
                    .find(|c| point_in(c.rect, col, row))
                    .map(|c| match &c.kind {
                        ClickKind::Act(a) => HitResult::Action(a.clone()),
                        ClickKind::Slider(k) => HitResult::Slider(*k, c.rect),
                    });
                if let Some(hit) = hit {
                    match hit {
                        HitResult::Action(a) => {
                            if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
                                self.apply(a).await;
                            }
                        }
                        HitResult::Slider(kind, rect) => {
                            let action = slider_value_action(kind, rect, col);
                            self.apply(action).await;
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown => self.on_scroll_at(1, Some(m.column)),
            MouseEventKind::ScrollUp => self.on_scroll_at(-1, Some(m.column)),
            _ => {}
        }
    }

    fn on_scroll_at(&mut self, delta: i32, mouse_col: Option<u16>) {
        match self.active_tab {
            Tab::Log => {
                if delta < 0 {
                    self.log_scroll = self.log_scroll.saturating_add(3);
                } else {
                    self.log_scroll = self.log_scroll.saturating_sub(3);
                }
            }
            Tab::Zones => {
                if delta > 0 {
                    self.move_focus(1);
                } else {
                    self.move_focus(-1);
                }
            }
            Tab::Channels => {
                self.channels_scroll = crate::tui::ui::apply_scroll_delta(self.channels_scroll, delta, 1);
            }
            Tab::Tuning => {
                self.tuning_scroll = crate::tui::ui::apply_scroll_delta(self.tuning_scroll, delta, 1);
            }
            Tab::Modulation => {
                if self.mod_function_picker {
                    self.move_mod_func_pick(delta);
                } else {
                    let scroll_editor = mouse_col
                        .map(|c| c >= self.mod_split_x)
                        .unwrap_or(true);
                    if scroll_editor {
                        self.mod_editor_scroll =
                            crate::tui::ui::apply_scroll_delta(self.mod_editor_scroll, delta, 1);
                    } else {
                        self.mod_slots_scroll =
                            crate::tui::ui::apply_scroll_delta(self.mod_slots_scroll, delta, 1);
                    }
                }
            }
            _ => {}
        }
    }

    fn move_focus(&mut self, delta: i32) {
        match self.active_tab {
            Tab::Zones => match self.zones_pane {
                ZonesPane::ConfiguredA => {
                    self.sel_conf_a = step_index(self.sel_conf_a, delta, self.config.channel_a.zones.len());
                }
                ZonesPane::ConfiguredB => {
                    self.sel_conf_b = step_index(self.sel_conf_b, delta, self.config.channel_b.zones.len());
                }
                ZonesPane::Avatar => {
                    self.sel_avatar = step_index(self.sel_avatar, delta, self.avatar_zones.len());
                }
            },
            Tab::Channels => {
                self.channel_focus = step_index(self.channel_focus, delta, channel_controls().len());
                self.sync_channels_scroll();
            }
            Tab::Tuning => {
                self.tuning_focus = step_index(self.tuning_focus, delta, tuning_controls().len());
                self.sync_tuning_scroll();
            }
            Tab::Modulation => {
                self.mod_focus = step_index(self.mod_focus, delta, mod_controls_len());
                self.sync_mod_editor_scroll();
            }
            _ => {}
        }
    }

    fn sync_channels_scroll(&mut self) {
        let Some(ctrl) = channel_controls().get(self.channel_focus).copied() else {
            return;
        };
        let Some(row) = channel_control_row(ctrl) else {
            return;
        };
        self.channels_scroll = crate::tui::ui::scroll_to_row(
            self.channels_scroll,
            self.channels_viewport_h,
            row,
        );
    }

    fn sync_tuning_scroll(&mut self) {
        let Some(ctrl) = tuning_controls().get(self.tuning_focus).copied() else {
            return;
        };
        let row = tuning_control_row(ctrl);
        self.tuning_scroll = crate::tui::ui::scroll_to_row(
            self.tuning_scroll,
            self.tuning_viewport_h,
            row,
        );
    }

    fn sync_mod_editor_scroll(&mut self) {
        let row = self.mod_focus as u16;
        self.mod_editor_scroll = crate::tui::ui::scroll_to_row(
            self.mod_editor_scroll,
            self.mod_editor_viewport_h,
            row,
        );
    }

    fn sync_mod_slots_scroll(&mut self) {
        let idx = mod_slot_index(self.mod_channel, self.mod_kind, self.mod_seg);
        self.mod_slots_scroll = crate::tui::ui::scroll_to_row(
            self.mod_slots_scroll,
            self.mod_slots_viewport_h,
            idx,
        );
    }

    pub async fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::SwitchTab(t) => {
                self.cancel_osc_port_edit();
                self.close_mod_function_picker();
                self.active_tab = t;
                self.channels_scroll = 0;
                self.mod_slots_scroll = 0;
                self.mod_editor_scroll = 0;
                self.tuning_scroll = 0;
            }
            Action::ToggleMonitor => {
                self.monitor = !self.monitor;
                self.state.monitor_enabled.store(self.monitor, Ordering::Relaxed);
            }
            Action::StartOscPortEdit => self.start_osc_port_edit(),
            Action::StepOscPort(delta) => {
                self.cancel_osc_port_edit();
                let new_port =
                    (self.osc_port as i32 + delta).clamp(1024, 65535) as u16;
                self.set_osc_port(new_port).await;
            }

            Action::FocusZonesPane(p) => self.zones_pane = p,
            Action::SelectConfigured(ch, i) => {
                match ch {
                    Channel::A => {
                        self.zones_pane = ZonesPane::ConfiguredA;
                        self.sel_conf_a = i;
                    }
                    Channel::B => {
                        self.zones_pane = ZonesPane::ConfiguredB;
                        self.sel_conf_b = i;
                    }
                }
            }
            Action::SelectAvatar(i) => {
                self.zones_pane = ZonesPane::Avatar;
                self.sel_avatar = i;
            }
            Action::RemoveZone(ch, i) => {
                let id = self.channel_config(ch).zones.get(i).map(|e| e.id.clone());
                if let Some(id) = id {
                    match ch {
                        Channel::A => {
                            self.state.engine.remove_zone_a(&id).await;
                        }
                        Channel::B => {
                            self.state.engine.remove_zone_b(&id).await;
                        }
                    }
                    log::info!("[ch-{}] Zone removed: {id}", ch.label());
                    self.refresh_config().await;
                    self.clamp_selections();
                }
            }
            Action::CycleMode(ch, i) => {
                let entry = self.channel_config(ch).zones.get(i).cloned();
                if let Some(entry) = entry {
                    let next = cycle_mode(entry.mode);
                    let found = match ch {
                        Channel::A => self.state.engine.set_zone_mode_a(&entry.id, next).await,
                        Channel::B => self.state.engine.set_zone_mode_b(&entry.id, next).await,
                    };
                    if found {
                        log::info!("[ch-{}] Mode for {} set to {next}", ch.label(), entry.id);
                    }
                    self.refresh_config().await;
                }
            }
            Action::AddAvatarZone(ch, i) => {
                if let Some(z) = self.avatar_zones.get(i) {
                    let id = ZoneId::new(z.zone_type, &z.id);
                    let entry = ZoneEntry::with_default_mode(id.clone());
                    match ch {
                        Channel::A => self.state.engine.add_zone_entry_a(entry).await,
                        Channel::B => self.state.engine.add_zone_entry_b(entry).await,
                    }
                    log::info!("[ch-{}] Zone added: {id} [depth]", ch.label());
                    self.refresh_config().await;
                }
            }
            Action::AddAllZones(ch) => {
                let zones = self.avatar_zones.clone();
                let mut added = 0usize;
                for z in &zones {
                    let id = ZoneId::new(z.zone_type, &z.id);
                    let already = self
                        .channel_config(ch)
                        .zones
                        .iter()
                        .any(|e| e.id.matches(&id));
                    if !already {
                        let entry = ZoneEntry::with_default_mode(id.clone());
                        match ch {
                            Channel::A => self.state.engine.add_zone_entry_a(entry).await,
                            Channel::B => self.state.engine.add_zone_entry_b(entry).await,
                        }
                        added += 1;
                        self.refresh_config().await;
                    }
                }
                log::info!("[ch-{}] Added {added} zone(s) from avatar", ch.label());
            }

            Action::SetFreq(ch, seg, v) => {
                let mut f = self.channel_config(ch).frequency;
                f[seg] = v.clamp(10, 255);
                self.set_freq(ch, f).await;
            }
            Action::StepFreq(ch, seg, d) => {
                let mut f = self.channel_config(ch).frequency;
                f[seg] = step_u8(f[seg], d, 10, 255);
                self.set_freq(ch, f).await;
            }
            Action::SetIntensity(ch, seg, v) => {
                let mut it = self.channel_config(ch).intensity;
                it[seg] = v.min(100);
                self.set_intensity(ch, it).await;
            }
            Action::StepIntensity(ch, seg, d) => {
                let mut it = self.channel_config(ch).intensity;
                it[seg] = step_u8(it[seg], d, 0, 100);
                self.set_intensity(ch, it).await;
            }
            Action::SetLimitMin(ch, v) => {
                let cur = self.channel_config(ch).limits.clone();
                self.set_limits(ch, PowerLimits::new(v, cur.max)).await;
            }
            Action::SetLimitMax(ch, v) => {
                let cur = self.channel_config(ch).limits.clone();
                self.set_limits(ch, PowerLimits::new(cur.min, v)).await;
            }
            Action::StepLimitMin(ch, d) => {
                let cur = self.channel_config(ch).limits.clone();
                let v = step_u8(cur.min, d, 0, 200);
                self.set_limits(ch, PowerLimits::new(v, cur.max)).await;
            }
            Action::StepLimitMax(ch, d) => {
                let cur = self.channel_config(ch).limits.clone();
                let v = step_u8(cur.max, d, 0, 200);
                self.set_limits(ch, PowerLimits::new(cur.min, v)).await;
            }
            Action::CycleAggregation(ch) => {
                let mut cfg = self.state.engine.config().await;
                let target = match ch {
                    Channel::A => &mut cfg.channel_a.aggregation,
                    Channel::B => &mut cfg.channel_b.aggregation,
                };
                *target = cycle_aggregation(target);
                let name = agg_name(match ch {
                    Channel::A => &cfg.channel_a.aggregation,
                    Channel::B => &cfg.channel_b.aggregation,
                });
                self.state.engine.set_config(cfg).await;
                log::info!("[ch-{}] Aggregation set to {name}", ch.label());
                self.refresh_config().await;
            }
            Action::SetAggregation(ch, mode) => {
                let mut cfg = self.state.engine.config().await;
                match ch {
                    Channel::A => cfg.channel_a.aggregation = mode,
                    Channel::B => cfg.channel_b.aggregation = mode,
                };
                self.state.engine.set_config(cfg).await;
                log::info!(
                    "[ch-{}] Aggregation set to {}",
                    ch.label(),
                    agg_name(&mode)
                );
                self.refresh_config().await;
            }
            Action::SaveConfig => {
                let cfg = self.state.engine.config().await;
                match save_config(CONFIG_FILE, &cfg) {
                    Ok(_) => log::info!("[config] Saved to {CONFIG_FILE}"),
                    Err(e) => log::error!("[config] Save failed: {e}"),
                }
            }
            Action::LoadConfig => {
                match std::fs::read_to_string(CONFIG_FILE)
                    .map_err(|e| e.to_string())
                    .and_then(|j| serde_json::from_str::<CliConfig>(&j).map_err(|e| e.to_string()))
                {
                    Ok(cfg) => {
                        self.state.engine.set_config(cfg).await;
                        self.state.engine.sync_hardware_limits().await;
                        log::info!("[config] Loaded from {CONFIG_FILE}");
                        self.refresh_config().await;
                        self.clamp_selections();
                        self.load_editor_from_config();
                    }
                    Err(e) => log::error!("[config] Load failed: {e}"),
                }
            }

            Action::StepUkf(field, d) => {
                let mut p = self.state.engine.ukf_params().await;
                apply_ukf_step(&mut p, field, d);
                self.state.engine.set_ukf_params(p).await;
                self.refresh_config().await;
            }
            Action::ResetUkf => {
                self.state.engine.set_ukf_params(UkfConfig::default()).await;
                log::info!("[ukf] Reset to defaults");
                self.refresh_config().await;
            }
            Action::StepNorm(field, d) => {
                let mut n = self.state.engine.norms().await;
                apply_norm_step(&mut n, field, d);
                self.state.engine.set_norms(n).await;
                self.refresh_config().await;
            }
            Action::ResetNorms => {
                self.state.engine.set_norms(MotionNorms::default()).await;
                log::info!("[norms] Reset to defaults");
                self.refresh_config().await;
            }

            Action::SelectModSlot(ch, kind, seg) => {
                self.close_mod_function_picker();
                self.mod_channel = ch;
                self.mod_kind = kind;
                self.mod_seg = seg.min(3);
                self.load_editor_from_config();
                self.sync_mod_slots_scroll();
            }
            Action::OpenModFunctionPicker => self.open_mod_function_picker(),
            Action::PickModFunction(i) => self.pick_mod_function(i),
            Action::CloseModFunctionPicker => self.close_mod_function_picker(),
            Action::SetModSource(src) => self.mod_editor.source = src,
            Action::CycleModFunction(d) => {
                let list = mod_function_list_for(&self.mod_editor.function);
                let cur = list
                    .iter()
                    .position(|f| f == &self.mod_editor.function)
                    .unwrap_or(0) as i32;
                let next = (cur + d).rem_euclid(list.len() as i32) as usize;
                self.mod_editor.function = list[next].clone();
            }
            Action::CycleModSource(d) => {
                let cur = MOD_SOURCES
                    .iter()
                    .position(|s| *s == self.mod_editor.source)
                    .unwrap_or(0) as i32;
                let next = (cur + d).rem_euclid(MOD_SOURCES.len() as i32) as usize;
                self.mod_editor.source = MOD_SOURCES[next];
            }
            Action::StepModParam(param, d) => {
                let v = param.get(&self.mod_editor) + d as f32 * param.step();
                let v = (v * 1000.0).round() / 1000.0;
                let v = param.clamp_value(v, self.mod_kind);
                param.set(&mut self.mod_editor, v);
                self.sanitise_mod_editor();
            }
            Action::ApplyMod => {
                self.sanitise_mod_editor();
                let mut cfg = self.state.engine.config().await;
                let ch_cfg = match self.mod_channel {
                    Channel::A => &mut cfg.channel_a,
                    Channel::B => &mut cfg.channel_b,
                };
                let target = match self.mod_kind {
                    ModKind::Freq => &mut ch_cfg.freq_modulation,
                    ModKind::Intensity => &mut ch_cfg.intensity_modulation,
                };
                target[self.mod_seg] = Some(self.mod_editor.clone());
                self.state.engine.set_config(cfg).await;
                log::info!(
                    "[ch-{}] {}[{}] modulation set: {}",
                    self.mod_channel.label(),
                    mod_kind_name(self.mod_kind),
                    self.mod_seg,
                    self.mod_editor
                );
                self.refresh_config().await;
            }
            Action::ClearMod => {
                let mut cfg = self.state.engine.config().await;
                let ch_cfg = match self.mod_channel {
                    Channel::A => &mut cfg.channel_a,
                    Channel::B => &mut cfg.channel_b,
                };
                match self.mod_kind {
                    ModKind::Freq => ch_cfg.freq_modulation[self.mod_seg] = None,
                    ModKind::Intensity => ch_cfg.intensity_modulation[self.mod_seg] = None,
                }
                self.state.engine.set_config(cfg).await;
                log::info!(
                    "[ch-{}] Modulation seg[{}] disabled ({})",
                    self.mod_channel.label(),
                    self.mod_seg,
                    mod_kind_name(self.mod_kind)
                );
                self.refresh_config().await;
            }
            Action::ClearAllMod(ch) => {
                let mut cfg = self.state.engine.config().await;
                let ch_cfg = match ch {
                    Channel::A => &mut cfg.channel_a,
                    Channel::B => &mut cfg.channel_b,
                };
                for i in 0..4 {
                    ch_cfg.freq_modulation[i] = None;
                    ch_cfg.intensity_modulation[i] = None;
                }
                self.state.engine.set_config(cfg).await;
                log::info!("[ch-{}] All modulation disabled (both)", ch.label());
                self.refresh_config().await;
            }

            Action::FocusNext => self.move_focus(1),
            Action::FocusPrev => self.move_focus(-1),
            Action::AdjustFocused(_) => {}
            Action::ActivateFocused => {}
        }
    }

    async fn set_freq(&mut self, ch: Channel, f: [u8; 4]) {
        match ch {
            Channel::A => self.state.engine.set_frequency_a(f).await,
            Channel::B => self.state.engine.set_frequency_b(f).await,
        }
        self.refresh_config().await;
    }

    async fn set_intensity(&mut self, ch: Channel, it: [u8; 4]) {
        match ch {
            Channel::A => self.state.engine.set_intensity_a(it).await,
            Channel::B => self.state.engine.set_intensity_b(it).await,
        }
        self.refresh_config().await;
    }

    async fn set_limits(&mut self, ch: Channel, lim: PowerLimits) {
        match ch {
            Channel::A => self.state.engine.set_limits_a(lim).await,
            Channel::B => self.state.engine.set_limits_b(lim).await,
        }
        self.refresh_config().await;
    }

    fn clamp_selections(&mut self) {
        let la = self.config.channel_a.zones.len();
        let lb = self.config.channel_b.zones.len();
        if la > 0 {
            self.sel_conf_a = self.sel_conf_a.min(la - 1);
        } else {
            self.sel_conf_a = 0;
        }
        if lb > 0 {
            self.sel_conf_b = self.sel_conf_b.min(lb - 1);
        } else {
            self.sel_conf_b = 0;
        }
    }

    fn focused_adjust_action(&self, d: i32) -> Option<Action> {
        match self.active_tab {
            Tab::Channels => channel_controls()
                .get(self.channel_focus)
                .copied()
                .and_then(|c| c.adjust_action(d)),
            Tab::Tuning => tuning_controls()
                .get(self.tuning_focus)
                .copied()
                .and_then(|c| c.adjust_action(d)),
            Tab::Modulation => self.mod_control_adjust(self.mod_focus, d),
            _ => None,
        }
    }

    fn focused_activate_action(&self) -> Option<Action> {
        match self.active_tab {
            Tab::Zones => Some(match self.zones_pane {
                ZonesPane::ConfiguredA => Action::CycleMode(Channel::A, self.sel_conf_a),
                ZonesPane::ConfiguredB => Action::CycleMode(Channel::B, self.sel_conf_b),
                ZonesPane::Avatar => Action::AddAvatarZone(Channel::A, self.sel_avatar),
            }),
            Tab::Channels => channel_controls()
                .get(self.channel_focus)
                .copied()
                .and_then(|c| c.activate_action()),
            Tab::Tuning => tuning_controls()
                .get(self.tuning_focus)
                .copied()
                .and_then(|c| c.activate_action()),
            Tab::Modulation => self.mod_control_activate(self.mod_focus),
            _ => None,
        }
    }

    fn mod_control_adjust(&self, focus: usize, d: i32) -> Option<Action> {
        match focus {
            0 => None,
            1 => Some(Action::CycleModSource(d)),
            f if (2..2 + ModParam::ALL.len()).contains(&f) => {
                Some(Action::StepModParam(ModParam::ALL[f - 2], d))
            }
            _ => None,
        }
    }

    fn mod_control_activate(&self, focus: usize) -> Option<Action> {
        let base = 2 + ModParam::ALL.len();
        match focus {
            0 => Some(Action::OpenModFunctionPicker),
            1 => None,
            f if f == base => Some(Action::ApplyMod),
            f if f == base + 1 => Some(Action::ClearMod),
            f if f == base + 2 => Some(Action::ClearAllMod(self.mod_channel)),
            _ => None,
        }
    }
}

enum HitResult {
    Action(Action),
    Slider(SliderKind, Rect),
}

fn point_in(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

fn slider_value_action(kind: SliderKind, rect: Rect, col: u16) -> Action {
    let (min, max) = kind.range();
    let w = rect.width.max(1);
    let rel = col.saturating_sub(rect.x) as f32;
    let denom = (w.saturating_sub(1)).max(1) as f32;
    let frac = (rel / denom).clamp(0.0, 1.0);
    let val = (min as f32 + frac * (max - min) as f32).round() as i32;
    match kind {
        SliderKind::Freq(ch, seg) => Action::SetFreq(ch, seg, val.clamp(min, max) as u8),
        SliderKind::Intensity(ch, seg) => Action::SetIntensity(ch, seg, val.clamp(min, max) as u8),
        SliderKind::LimitMin(ch) => Action::SetLimitMin(ch, val.clamp(min, max) as u8),
        SliderKind::LimitMax(ch) => Action::SetLimitMax(ch, val.clamp(min, max) as u8),
    }
}

fn step_index(cur: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let n = len as i32;
    (cur as i32 + delta).rem_euclid(n) as usize
}

fn step_u8(cur: u8, delta: i32, min: u8, max: u8) -> u8 {
    let v = cur as i32 + delta;
    v.clamp(min as i32, max as i32) as u8
}

fn step_multiplier(key: &KeyEvent) -> i32 {
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        10
    } else {
        1
    }
}

pub fn cycle_mode(m: ContactMode) -> ContactMode {
    match m {
        ContactMode::Depth => ContactMode::Speed,
        ContactMode::Speed => ContactMode::Acc,
        ContactMode::Acc => ContactMode::Recoil,
        ContactMode::Recoil => ContactMode::Depth,
    }
}

pub fn cycle_aggregation(m: &AggregationMode) -> AggregationMode {
    match m {
        AggregationMode::Max => AggregationMode::Sum,
        AggregationMode::Sum => AggregationMode::Average,
        AggregationMode::Average => AggregationMode::Max,
    }
}

pub fn agg_name(m: &AggregationMode) -> &'static str {
    match m {
        AggregationMode::Max => "max",
        AggregationMode::Sum => "sum",
        AggregationMode::Average => "avg",
    }
}

pub fn aggregation_modes() -> &'static [AggregationMode; 3] {
    &[
        AggregationMode::Max,
        AggregationMode::Sum,
        AggregationMode::Average,
    ]
}

pub fn mod_kind_name(k: ModKind) -> &'static str {
    match k {
        ModKind::Freq => "freq",
        ModKind::Intensity => "int",
    }
}

fn apply_ukf_step(p: &mut UkfConfig, field: UkfField, d: i32) {
    let f = d as f32;
    match field {
        UkfField::Q => p.q = (p.q + f * 0.001).max(0.0001),
        UkfField::R => p.r = (p.r + f * 0.001).max(0.0001),
        UkfField::Alpha => p.alpha = (p.alpha + f * 0.05).max(0.001),
        UkfField::Beta => p.beta = (p.beta + f * 0.1).max(0.0),
        UkfField::Kappa => p.kappa += f * 0.1,
    }
    p.q = round3(p.q);
    p.r = round3(p.r);
    p.alpha = round3(p.alpha);
    p.beta = round3(p.beta);
    p.kappa = round3(p.kappa);
}

fn apply_norm_step(n: &mut MotionNorms, field: NormField, d: i32) {
    let f = d as f32;
    match field {
        NormField::Speed => n.speed = (n.speed + f * 0.5).max(0.001),
        NormField::Acc => n.acc = (n.acc + f * 1.0).max(0.001),
        NormField::Recoil => n.recoil = (n.recoil + f * 5.0).max(0.001),
    }
    n.speed = round3(n.speed);
    n.acc = round3(n.acc);
    n.recoil = round3(n.recoil);
}

fn round3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

fn save_config(path: &str, config: &CliConfig) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(path, json)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelControl {
    Freq(Channel, usize),
    Intensity(Channel, usize),
    LimitMin(Channel),
    LimitMax(Channel),
    Aggregation(Channel),
    Save,
    Load,
}

impl ChannelControl {
    fn adjust_action(self, d: i32) -> Option<Action> {
        match self {
            ChannelControl::Freq(ch, seg) => Some(Action::StepFreq(ch, seg, d)),
            ChannelControl::Intensity(ch, seg) => Some(Action::StepIntensity(ch, seg, d)),
            ChannelControl::LimitMin(ch) => Some(Action::StepLimitMin(ch, d)),
            ChannelControl::LimitMax(ch) => Some(Action::StepLimitMax(ch, d)),
            ChannelControl::Aggregation(ch) => Some(Action::CycleAggregation(ch)),
            _ => None,
        }
    }

    fn activate_action(self) -> Option<Action> {
        match self {
            ChannelControl::Aggregation(ch) => Some(Action::CycleAggregation(ch)),
            ChannelControl::Save => Some(Action::SaveConfig),
            ChannelControl::Load => Some(Action::LoadConfig),
            _ => None,
        }
    }
}

pub fn channel_controls() -> Vec<ChannelControl> {
    let mut v = Vec::new();
    for ch in [Channel::A, Channel::B] {
        for seg in 0..4 {
            v.push(ChannelControl::Freq(ch, seg));
        }
        for seg in 0..4 {
            v.push(ChannelControl::Intensity(ch, seg));
        }
        v.push(ChannelControl::LimitMin(ch));
        v.push(ChannelControl::LimitMax(ch));
        v.push(ChannelControl::Aggregation(ch));
    }
    v.push(ChannelControl::Save);
    v.push(ChannelControl::Load);
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuningControl {
    Ukf(UkfField),
    UkfReset,
    Norm(NormField),
    NormReset,
}

impl TuningControl {
    fn adjust_action(self, d: i32) -> Option<Action> {
        match self {
            TuningControl::Ukf(f) => Some(Action::StepUkf(f, d)),
            TuningControl::Norm(f) => Some(Action::StepNorm(f, d)),
            _ => None,
        }
    }

    fn activate_action(self) -> Option<Action> {
        match self {
            TuningControl::UkfReset => Some(Action::ResetUkf),
            TuningControl::NormReset => Some(Action::ResetNorms),
            _ => None,
        }
    }
}

pub fn tuning_controls() -> Vec<TuningControl> {
    vec![
        TuningControl::Ukf(UkfField::Q),
        TuningControl::Ukf(UkfField::R),
        TuningControl::Ukf(UkfField::Alpha),
        TuningControl::Ukf(UkfField::Beta),
        TuningControl::Ukf(UkfField::Kappa),
        TuningControl::UkfReset,
        TuningControl::Norm(NormField::Speed),
        TuningControl::Norm(NormField::Acc),
        TuningControl::Norm(NormField::Recoil),
        TuningControl::NormReset,
    ]
}

pub fn mod_controls_len() -> usize {
    2 + ModParam::ALL.len() + 3
}

pub fn mod_slot_index(ch: Channel, kind: ModKind, seg: usize) -> u16 {
    let ch_off = match ch {
        Channel::A => 0,
        Channel::B => 8,
    };
    let kind_off = match kind {
        ModKind::Freq => 0,
        ModKind::Intensity => 4,
    };
    ch_off + kind_off + seg as u16
}

pub fn channel_control_row(ctrl: ChannelControl) -> Option<u16> {
    match ctrl {
        ChannelControl::Freq(_, seg) => Some(1 + seg as u16),
        ChannelControl::Intensity(_, seg) => Some(6 + seg as u16),
        ChannelControl::LimitMin(_) => Some(11),
        ChannelControl::LimitMax(_) => Some(12),
        ChannelControl::Aggregation(_) => Some(13),
        ChannelControl::Save | ChannelControl::Load => None,
    }
}

pub fn tuning_control_row(ctrl: TuningControl) -> u16 {
    match ctrl {
        TuningControl::Ukf(UkfField::Q) => 0,
        TuningControl::Ukf(UkfField::R) => 1,
        TuningControl::Ukf(UkfField::Alpha) => 2,
        TuningControl::Ukf(UkfField::Beta) => 3,
        TuningControl::Ukf(UkfField::Kappa) => 4,
        TuningControl::UkfReset => 5,
        TuningControl::Norm(NormField::Speed) => 0,
        TuningControl::Norm(NormField::Acc) => 1,
        TuningControl::Norm(NormField::Recoil) => 2,
        TuningControl::NormReset => 3,
    }
}
