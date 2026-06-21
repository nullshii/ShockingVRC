use shocking_vrc_core::cli::{CliConfig, MotionNorms, PowerLimits, UkfConfig, ZoneEntry, ZoneId};

use super::App;
use super::{Action, Channel, ModKind};
use super::controls::{agg_name, cycle_aggregation, cycle_mode, mod_function_list_for, mod_kind_name};
use super::helpers::{apply_norm_step, apply_ukf_step, step_u8};
use super::prefs::{save_config, save_ui_prefs, should_persist_config, CONFIG_FILE};

impl App {
    pub async fn apply(&mut self, action: Action) {
        let persist = should_persist_config(&action);
        match action {
            Action::Quit => self.should_quit = true,
            Action::SwitchTab(t) => {
                self.cancel_osc_port_edit();
                self.cancel_preset_save_edit();
                self.cancel_preset_delete_confirm();
                self.close_mod_function_picker();
                self.active_tab = t;
                self.channels_scroll = 0;
                self.mod_slots_scroll = 0;
                self.mod_editor_scroll = 0;
                self.tuning_scroll = 0;
                self.preset_scroll = 0;
            }
            Action::StartOscPortEdit => self.start_osc_port_edit(),
            Action::StepOscPort(delta) => {
                self.cancel_osc_port_edit();
                let new_port = (self.osc_port as i32 + delta).clamp(1024, 65535) as u16;
                self.set_osc_port(new_port).await;
            }
            Action::FocusZonesPane(p) => self.zones_pane = p,
            Action::SelectConfigured(ch, i) => match ch {
                Channel::A => {
                    self.zones_pane = super::ZonesPane::ConfiguredA;
                    self.sel_conf_a = i;
                }
                Channel::B => {
                    self.zones_pane = super::ZonesPane::ConfiguredB;
                    self.sel_conf_b = i;
                }
            },
            Action::SelectAvatar(i) => {
                self.zones_pane = super::ZonesPane::Avatar;
                self.sel_avatar = i;
            }
            Action::RemoveZone(ch, i) => {
                let id = self.channel_config(ch).zones.get(i).map(|e| e.id.clone());
                if let Some(id) = id {
                    match ch {
                        Channel::A => { self.state.engine.remove_zone_a(&id).await; }
                        Channel::B => { self.state.engine.remove_zone_b(&id).await; }
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
                log::info!("[ch-{}] Aggregation set to {}", ch.label(), agg_name(&mode));
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
            Action::SetAutoSave(on) => {
                self.auto_save = on;
                match save_ui_prefs(self.auto_save) {
                    Ok(()) => {
                        log::info!("[prefs] Auto-save {}", if on { "enabled" } else { "disabled" })
                    }
                    Err(e) => log::error!("[prefs] Failed to save: {e}"),
                }
                if on {
                    self.auto_save_config().await;
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
                let cur = list.iter().position(|f| f == &self.mod_editor.function).unwrap_or(0) as i32;
                let next = (cur + d).rem_euclid(list.len() as i32) as usize;
                self.mod_editor.function = list[next].clone();
            }
            Action::CycleModSource(d) => {
                use super::controls::mod_source_list;
                let srcs = mod_source_list();
                let cur = srcs.iter().position(|s| *s == self.mod_editor.source).unwrap_or(0) as i32;
                let next = (cur + d).rem_euclid(srcs.len() as i32) as usize;
                self.mod_editor.source = srcs[next];
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
            Action::SelectPreset(i) => {
                if i < self.preset_entries.len() {
                    self.sel_preset = i;
                    self.sync_presets_scroll();
                }
            }
            Action::ApplyPreset(ch) => self.apply_selected_preset(ch).await,
            Action::StartSavePreset(ch) => self.start_preset_save_edit(ch),
            Action::CommitSavePreset => self.commit_preset_save_edit().await,
            Action::CancelSavePreset => self.cancel_preset_save_edit(),
            Action::RequestDeletePreset(i) => {
                if self.preset_entries.get(i).is_some_and(|e| e.user) {
                    self.preset_delete_confirm = Some(i);
                }
            }
            Action::ConfirmDeletePreset => self.confirm_preset_delete().await,
            Action::CancelDeletePreset => self.cancel_preset_delete_confirm(),
            Action::RefreshPresets => self.start_refresh_presets(),
            Action::ConfirmUpdate => self.start_update_download(),
            Action::DismissUpdate => self.dismiss_update_popup(),
            Action::SkipUpdateVersion => self.skip_update_version(),
            Action::FocusNext => self.move_focus(1),
            Action::FocusPrev => self.move_focus(-1),
            Action::AdjustFocused(_) => {}
            Action::ActivateFocused => {}
            Action::TutorialStart => self.start_tutorial(),
            Action::TutorialNext => self.tutorial_next(),
            Action::TutorialPrev => self.tutorial_prev(),
            Action::TutorialClose => self.close_tutorial(),
        }

        if persist {
            self.auto_save_config().await;
        }
    }
    pub(super) async fn auto_save_config(&mut self) {
        if !self.auto_save || self.tutorial_active {
            return;
        }
        let cfg = self.state.engine.config().await;
        match save_config(CONFIG_FILE, &cfg) {
            Ok(()) => log::debug!("[config] Auto-saved to {CONFIG_FILE}"),
            Err(e) => log::error!("[config] Auto-save failed: {e}"),
        }
    }

    pub(super) async fn set_freq(&mut self, ch: Channel, f: [u8; 4]) {
        match ch {
            Channel::A => self.state.engine.set_frequency_a(f).await,
            Channel::B => self.state.engine.set_frequency_b(f).await,
        }
        self.refresh_config().await;
    }

    pub(super) async fn set_intensity(&mut self, ch: Channel, it: [u8; 4]) {
        match ch {
            Channel::A => self.state.engine.set_intensity_a(it).await,
            Channel::B => self.state.engine.set_intensity_b(it).await,
        }
        self.refresh_config().await;
    }

    pub(super) async fn set_limits(&mut self, ch: Channel, lim: PowerLimits) {
        match ch {
            Channel::A => self.state.engine.set_limits_a(lim).await,
            Channel::B => self.state.engine.set_limits_b(lim).await,
        }
        self.refresh_config().await;
    }
}
