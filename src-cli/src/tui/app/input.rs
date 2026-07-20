use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use super::App;
use super::{Action, Channel, ModParam, Tab, ZonesPane};
use super::controls::{channel_controls, mod_controls_len, tuning_controls};
use super::helpers::{point_in, slider_value_action, step_multiplier, HitResult};

impl App {
    pub async fn handle_key(&mut self, key: KeyEvent) {
        if self.update_popup {
            if self.update_downloading {
                return;
            }
            if self.update_error.is_some() {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.dismiss_update_popup();
                    }
                    KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                        self.flash_button(Action::ConfirmUpdate);
                        self.start_update_download();
                    }
                    _ => {}
                }
                return;
            }
            match key.code {
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.dismiss_update_popup();
                }
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.flash_button(Action::ConfirmUpdate);
                    self.start_update_download();
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    self.skip_update_version();
                }
                _ => {}
            }
            return;
        }

        if self.tutorial_active {
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.close_tutorial();
                    self.should_quit = true;
                }
                KeyCode::Esc | KeyCode::Char('q') => self.close_tutorial(),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ') => self.tutorial_next(),
                KeyCode::Backspace | KeyCode::Left => self.tutorial_prev(),
                _ => {}
            }
            return;
        }

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
                if self.preset_delete_confirm.is_some() {
                    self.cancel_preset_delete_confirm();
                    return;
                }
                if self.preset_save_editing {
                    self.cancel_preset_save_edit();
                    return;
                }
                if self.update_popup {
                    self.dismiss_update_popup();
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

        if self.preset_delete_confirm.is_some() && self.active_tab == Tab::Presets {
            match key.code {
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.cancel_preset_delete_confirm();
                }
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.flash_button(Action::ConfirmDeletePreset);
                    self.confirm_preset_delete().await;
                }
                _ => {}
            }
            return;
        }

        if self.preset_save_editing && self.active_tab == Tab::Presets {
            self.handle_preset_save_key(key).await;
            return;
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
            KeyCode::Char(c @ '1'..='9') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let idx = c as usize - '1' as usize;
                if idx < self.device_infos.len() {
                    self.flash_button(Action::SwitchDevice(idx));
                    self.apply(Action::SwitchDevice(idx)).await;
                }
                return;
            }
            KeyCode::Char('[') => {
                self.flash_button(Action::CycleDevice(-1));
                self.apply(Action::CycleDevice(-1)).await;
                return;
            }
            KeyCode::Char(']') => {
                self.flash_button(Action::CycleDevice(1));
                self.apply(Action::CycleDevice(1)).await;
                return;
            }
            KeyCode::Char(c @ '1'..='8') => {
                self.close_mod_function_picker();
                let idx = c as usize - '1' as usize;
                if let Some(t) = Tab::ALL.get(idx) {
                    self.active_tab = *t;
                    self.channels_scroll = 0;
                    self.mod_slots_scroll = 0;
                    self.mod_editor_scroll = 0;
                    self.tuning_scroll = 0;
                    self.preset_scroll = 0;
                }
                return;
            }
            _ => {}
        }

        if self.active_tab == Tab::Log {
            match key.code {
                KeyCode::Up => { self.on_scroll_at(-1, None); return; }
                KeyCode::Down => { self.on_scroll_at(1, None); return; }
                _ => {}
            }
        }

        if let Some(action) = self.map_key_to_action(key) {
            let concrete = match action {
                Action::FocusNext => { self.move_focus(1); None }
                Action::FocusPrev => { self.move_focus(-1); None }
                Action::AdjustFocused(d) => self.focused_adjust_action(d),
                Action::ActivateFocused => self.focused_activate_action(),
                other => Some(other),
            };
            if let Some(a) = concrete {
                self.flash_button(a.clone());
                self.apply(a).await;
            }
        }
    }

    pub async fn handle_mouse(&mut self, m: MouseEvent) {
        if self.tutorial_active {
            if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
                let hit = self.clickables.iter().rev()
                    .find(|c| point_in(c.rect, m.column, m.row))
                    .and_then(|c| match &c.kind {
                        super::ClickKind::Act(a @ Action::TutorialNext)
                        | super::ClickKind::Act(a @ Action::TutorialPrev)
                        | super::ClickKind::Act(a @ Action::TutorialClose) => Some(a.clone()),
                        _ => None,
                    });
                if let Some(a) = hit {
                    self.flash_button(a.clone());
                    self.apply(a).await;
                }
            }
            return;
        }

        if self.preset_save_editing {
            if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
                let hit = self.clickables.iter().rev()
                    .find(|c| point_in(c.rect, m.column, m.row))
                    .and_then(|c| match &c.kind {
                        super::ClickKind::Act(a @ Action::CommitSavePreset)
                        | super::ClickKind::Act(a @ Action::CancelSavePreset) => Some(a.clone()),
                        _ => None,
                    });
                if let Some(a) = hit {
                    self.flash_button(a.clone());
                    self.apply(a).await;
                }
            }
            return;
        }

        if self.preset_delete_confirm.is_some() {
            if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
                let hit = self.clickables.iter().rev()
                    .find(|c| point_in(c.rect, m.column, m.row))
                    .and_then(|c| match &c.kind {
                        super::ClickKind::Act(Action::ConfirmDeletePreset) => {
                            Some(Action::ConfirmDeletePreset)
                        }
                        super::ClickKind::Act(Action::CancelDeletePreset) => {
                            Some(Action::CancelDeletePreset)
                        }
                        _ => None,
                    });
                if let Some(a) = hit {
                    self.flash_button(a.clone());
                    self.apply(a).await;
                }
            }
            return;
        }

        match m.kind {
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left) => {
                let hit = self.clickables.iter().rev()
                    .find(|c| point_in(c.rect, m.column, m.row))
                    .map(|c| match &c.kind {
                        super::ClickKind::Act(a) => HitResult::Action(a.clone()),
                        super::ClickKind::Slider(k) => HitResult::Slider(*k, c.rect),
                    });
                if let Some(hit) = hit {
                    match hit {
                        HitResult::Action(a) => {
                            if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
                                self.flash_button(a.clone());
                                self.apply(a).await;
                            }
                        }
                        HitResult::Slider(kind, rect) => {
                            let action = slider_value_action(kind, rect, m.column);
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

    fn map_key_to_action(&self, key: KeyEvent) -> Option<Action> {
        match self.active_tab {
            Tab::Status => None,
            Tab::Setup => None,
            Tab::Zones => match key.code {
                KeyCode::Up => Some(Action::FocusPrev),
                KeyCode::Down => Some(Action::FocusNext),
                KeyCode::Left => Some(Action::FocusZonesPane(self.zones_pane.prev())),
                KeyCode::Right => Some(Action::FocusZonesPane(self.zones_pane.next())),
                KeyCode::Char('a') => Some(Action::FocusZonesPane(ZonesPane::ConfiguredA)),
                KeyCode::Char('b') => Some(Action::FocusZonesPane(ZonesPane::ConfiguredB)),
                KeyCode::Char('v') => Some(Action::FocusZonesPane(ZonesPane::Avatar)),
                KeyCode::Char('m') => Some(Action::ActivateFocused),
                KeyCode::Char('x') | KeyCode::Delete => Some(match self.zones_pane {
                    ZonesPane::ConfiguredA => Action::RemoveZone(Channel::A, self.sel_conf_a),
                    ZonesPane::ConfiguredB => Action::RemoveZone(Channel::B, self.sel_conf_b),
                    ZonesPane::Avatar => Action::AddAvatarZone(Channel::B, self.sel_avatar),
                }),
                KeyCode::Char('-') | KeyCode::Char('_') => match self.zones_pane {
                    ZonesPane::ConfiguredA => Some(Action::StepZoneScale(
                        Channel::A, self.sel_conf_a,
                        if key.modifiers.contains(KeyModifiers::SHIFT) { -1 } else { -5 },
                    )),
                    ZonesPane::ConfiguredB => Some(Action::StepZoneScale(
                        Channel::B, self.sel_conf_b,
                        if key.modifiers.contains(KeyModifiers::SHIFT) { -1 } else { -5 },
                    )),
                    ZonesPane::Avatar => None,
                },
                KeyCode::Char('+') | KeyCode::Char('=') => match self.zones_pane {
                    ZonesPane::ConfiguredA => Some(Action::StepZoneScale(
                        Channel::A, self.sel_conf_a,
                        if key.modifiers.contains(KeyModifiers::SHIFT) { 1 } else { 5 },
                    )),
                    ZonesPane::ConfiguredB => Some(Action::StepZoneScale(
                        Channel::B, self.sel_conf_b,
                        if key.modifiers.contains(KeyModifiers::SHIFT) { 1 } else { 5 },
                    )),
                    ZonesPane::Avatar => None,
                },
                KeyCode::Char('r') => match self.zones_pane {
                    ZonesPane::ConfiguredA => Some(Action::SetZoneScale(Channel::A, self.sel_conf_a, 100)),
                    ZonesPane::ConfiguredB => Some(Action::SetZoneScale(Channel::B, self.sel_conf_b, 100)),
                    ZonesPane::Avatar => None,
                },
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
                KeyCode::Char(']') => Some(Action::SelectModSlot(
                    self.mod_channel,
                    self.mod_kind,
                    (self.mod_seg + 1) % 4,
                )),
                KeyCode::Enter | KeyCode::Char(' ') => Some(Action::ActivateFocused),
                _ => None,
            },
            Tab::Presets => match key.code {
                KeyCode::Up => Some(Action::FocusPrev),
                KeyCode::Down => Some(Action::FocusNext),
                KeyCode::Char('r') | KeyCode::Char('R') => Some(Action::RefreshPresets),
                KeyCode::Char('a') | KeyCode::Char('A') => Some(Action::ApplyPreset(Channel::A)),
                KeyCode::Char('b') | KeyCode::Char('B') => Some(Action::ApplyPreset(Channel::B)),
                _ => None,
            },
            Tab::Log => None,
        }
    }

    pub(super) fn on_scroll_at(&mut self, delta: i32, mouse_col: Option<u16>) {
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
                self.channels_scroll =
                    crate::tui::ui::apply_scroll_delta(self.channels_scroll, delta, 1);
            }
            Tab::Tuning => {
                self.tuning_scroll =
                    crate::tui::ui::apply_scroll_delta(self.tuning_scroll, delta, 1);
            }
            Tab::Modulation => {
                if self.mod_function_picker {
                    self.move_mod_func_pick(delta);
                } else {
                    let scroll_editor = mouse_col.map(|c| c >= self.mod_split_x).unwrap_or(true);
                    if scroll_editor {
                        self.mod_editor_scroll =
                            crate::tui::ui::apply_scroll_delta(self.mod_editor_scroll, delta, 1);
                    } else {
                        self.mod_slots_scroll =
                            crate::tui::ui::apply_scroll_delta(self.mod_slots_scroll, delta, 1);
                    }
                }
            }
            Tab::Presets => {
                let content_h = self.preset_entries.len() as u16;
                if crate::tui::ui::max_scroll(content_h, self.presets_viewport_h) == 0 {
                    return;
                }
                self.preset_scroll = crate::tui::ui::clamp_scroll(
                    crate::tui::ui::apply_scroll_delta(self.preset_scroll, delta, 1),
                    content_h,
                    self.presets_viewport_h,
                );
            }
            _ => {}
        }
    }

    pub(super) fn move_focus(&mut self, delta: i32) {
        use super::helpers::step_index;
        match self.active_tab {
            Tab::Zones => match self.zones_pane {
                ZonesPane::ConfiguredA => {
                    self.sel_conf_a =
                        step_index(self.sel_conf_a, delta, self.config.channel_a.zones.len());
                }
                ZonesPane::ConfiguredB => {
                    self.sel_conf_b =
                        step_index(self.sel_conf_b, delta, self.config.channel_b.zones.len());
                }
                ZonesPane::Avatar => {
                    self.sel_avatar =
                        step_index(self.sel_avatar, delta, self.avatar_zones.len());
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
            Tab::Presets => {
                if !self.preset_entries.is_empty() {
                    self.sel_preset =
                        step_index(self.sel_preset, delta, self.preset_entries.len());
                    self.sync_presets_scroll();
                }
            }
            _ => {}
        }
    }

    pub(super) fn switch_tab_relative(&mut self, delta: i32) {
        self.cancel_preset_save_edit();
        self.cancel_preset_delete_confirm();
        self.close_mod_function_picker();
        let n = Tab::ALL.len() as i32;
        let idx = (self.active_tab.index() as i32 + delta).rem_euclid(n) as usize;
        self.active_tab = Tab::ALL[idx];
        self.channels_scroll = 0;
        self.mod_slots_scroll = 0;
        self.mod_editor_scroll = 0;
        self.tuning_scroll = 0;
        self.preset_scroll = 0;
    }

    pub(super) fn focused_adjust_action(&self, d: i32) -> Option<Action> {
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

    pub(super) fn focused_activate_action(&self) -> Option<Action> {
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
