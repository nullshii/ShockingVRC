use std::collections::HashMap;

use shocking_vrc_core::modulation::config::ModulationConfig;

use super::{Channel, ModKind, ZonesPane};

#[derive(Debug, Clone)]
pub struct DeviceUiState {
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
    pub channels_scroll: u16,
    pub mod_slots_scroll: u16,
    pub mod_editor_scroll: u16,
    pub tuning_scroll: u16,
}

impl Default for DeviceUiState {
    fn default() -> Self {
        Self {
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
            channels_scroll: 0,
            mod_slots_scroll: 0,
            mod_editor_scroll: 0,
            tuning_scroll: 0,
        }
    }
}

pub type DeviceUiStateMap = HashMap<String, DeviceUiState>;
