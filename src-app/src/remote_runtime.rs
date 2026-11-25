use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use shocking_vrc_core::graph::device::MAX_CHANNELS;
use shocking_vrc_core::graph::kind::{self, ConstantParams, FaderParams, PartnerDeviceParams, XyPadParams};
use shocking_vrc_core::graph::safety::RemoteChannelPolicy;
use shocking_vrc_core::graph::{AlarmSample, Graph, GraphEvaluator, GraphIssue, GraphProbe, NodeKind, PinRef};
use shocking_vrc_core::osc::{OutputNodeStatus, OutputSource};
use shocking_vrc_remote::protocol::{OutputReport, PolicySnapshot, SlotFrame, SlotInfo, WireChannel};
use shocking_vrc_remote::service::{RemoteConfig, RemoteService, RemoteStatus};

use crate::app_state::AppState;
use crate::graph_runtime::{GRAPH_TICK, sample};
use crate::graph_store;

pub const REMOTE_SETTINGS_FILE: &str = "remote_settings.json";
pub const REMOTE_GRAPH_FILE: &str = "remote_graph.json";
pub const REMOTE_LOG_FILE: &str = "remote_session_log.txt";
pub const MAX_REMOTE_SLOTS: usize = 8;
const AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(1500);
const PUBLISH_EVERY: u64 = 2;
const OUTPUT_EVERY: u64 = 8;
const POLICY_EVERY: u64 = 40;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemoteSettings {
    pub config: RemoteConfig,
    pub slot_channels: [[RemoteChannelPolicy; MAX_CHANNELS]; MAX_REMOTE_SLOTS],
    pub slots: Vec<u8>,
    pub stop_param: String,
    pub scheme_asked: bool,
}

impl Default for RemoteSettings {
    fn default() -> Self {
        Self {
            config: RemoteConfig::default(),
            slot_channels: [[RemoteChannelPolicy::default(); MAX_CHANNELS]; MAX_REMOTE_SLOTS],
            slots: vec![0],
            stop_param: "/avatar/parameters/RemoteStop".into(),
            scheme_asked: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct RemoteSettingsRaw {
    config: RemoteConfig,
    slot_channels: Option<Vec<[RemoteChannelPolicy; MAX_CHANNELS]>>,
    channels: Option<[RemoteChannelPolicy; MAX_CHANNELS]>,
    slots: Vec<u8>,
    stop_param: String,
    scheme_asked: bool,
}

impl Default for RemoteSettingsRaw {
    fn default() -> Self {
        let def = RemoteSettings::default();
        Self {
            config: def.config,
            slot_channels: None,
            channels: None,
            slots: def.slots,
            stop_param: def.stop_param,
            scheme_asked: def.scheme_asked,
        }
    }
}

impl<'de> Deserialize<'de> for RemoteSettings {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RemoteSettingsRaw::deserialize(deserializer)?;
        let fallback = raw.channels.unwrap_or([RemoteChannelPolicy::default(); MAX_CHANNELS]);
        let mut slot_channels = [fallback; MAX_REMOTE_SLOTS];
        if let Some(list) = raw.slot_channels {
            for (dst, src) in slot_channels.iter_mut().zip(list) {
                *dst = src;
            }
        }
        Ok(Self {
            config: raw.config,
            slot_channels,
            slots: raw.slots,
            stop_param: raw.stop_param,
            scheme_asked: raw.scheme_asked,
        })
    }
}

impl RemoteSettings {
    pub fn sanitized(mut self) -> Self {
        for chs in &mut self.slot_channels {
            *chs = chs.map(RemoteChannelPolicy::sanitized);
        }
        self.slots.sort_unstable();
        self.slots.dedup();
        self.slots.retain(|s| (*s as usize) < MAX_REMOTE_SLOTS);
        self.config.jitter_ms = self.config.jitter_ms.min(500);
        self
    }

    pub fn allows(&self, slot: usize) -> bool {
        self.slots.iter().any(|s| *s as usize == slot)
    }

    pub fn slot_channels(&self, slot: usize) -> [RemoteChannelPolicy; MAX_CHANNELS] {
        self.slot_channels
            .get(slot)
            .copied()
            .unwrap_or([RemoteChannelPolicy::default(); MAX_CHANNELS])
    }

    pub fn slot_channels_mut(&mut self, slot: usize) -> &mut [RemoteChannelPolicy; MAX_CHANNELS] {
        let idx = slot.min(MAX_REMOTE_SLOTS - 1);
        &mut self.slot_channels[idx]
    }
}

pub fn load_settings() -> RemoteSettings {
    std::fs::read_to_string(REMOTE_SETTINGS_FILE)
        .ok()
        .and_then(|s| serde_json::from_str::<RemoteSettings>(&s).ok())
        .unwrap_or_default()
        .sanitized()
}

pub fn save_settings(s: &RemoteSettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(REMOTE_SETTINGS_FILE, json).map_err(|e| e.to_string())
}

pub fn starter_graph() -> Graph {
    let mut g = Graph::default();
    let fader = g.add_kind(
        NodeKind::Fader(FaderParams {
            latch: true,
            ..FaderParams::default()
        }),
        [0.0, 0.0],
    );
    let pad = g.add_kind(
        NodeKind::XyPad(XyPadParams {
            spring: false,
            y: 0.8,
            ..XyPadParams::default()
        }),
        [0.0, 260.0],
    );
    let one = g.add_kind(NodeKind::Constant(ConstantParams { value: 1.0 }), [0.0, -140.0]);
    let partner = g.add_kind(NodeKind::PartnerDevice(PartnerDeviceParams::default()), [380.0, 0.0]);
    let wires = [
        (one, "out", fader, "level"),
        (fader, "out", partner, "a.power"),
        (fader, "out", partner, "b.power"),
        (pad, "x", partner, "a.freq"),
        (pad, "y", partner, "a.int"),
    ];
    for (from, out, to, input) in wires {
        let _ = g.connect(PinRef::new(from, out), PinRef::new(to, input));
    }
    g
}

pub fn load_remote_graph() -> graph_store::LoadedGraph {
    let path = std::path::Path::new(REMOTE_GRAPH_FILE);
    if !path.exists() {
        return graph_store::LoadedGraph {
            graph: starter_graph(),
            notes: Vec::new(),
        };
    }
    match graph_store::load_graph_from(path) {
        Ok(graph) => graph_store::LoadedGraph {
            graph,
            notes: Vec::new(),
        },
        Err(e) => {
            let _ = std::fs::copy(path, path.with_extension("json.bak"));
            graph_store::LoadedGraph {
                graph: starter_graph(),
                notes: vec![format!("{REMOTE_GRAPH_FILE} could not be read: {e}")],
            }
        }
    }
}

pub fn register_url_scheme() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let command = format!("\"{}\" \"%1\"", exe.display());
        let key = r"HKCU\Software\Classes\shockingvrc";
        let steps: [Vec<&str>; 3] = [
            vec!["add", key, "/ve", "/d", "URL:ShockingVRC remote link", "/f"],
            vec!["add", key, "/v", "URL Protocol", "/d", "", "/f"],
            vec![
                "add",
                r"HKCU\Software\Classes\shockingvrc\shell\open\command",
                "/ve",
                "/d",
                command.as_str(),
                "/f",
            ],
        ];
        for args in steps {
            let status = std::process::Command::new("reg")
                .args(&args)
                .creation_flags(CREATE_NO_WINDOW)
                .status()
                .map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("reg {} failed", args.join(" ")));
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Opening links directly is only set up on Windows — paste the link instead".into())
    }
}

pub fn link_from_args() -> Option<String> {
    std::env::args()
        .skip(1)
        .find(|a| a.to_ascii_lowercase().starts_with("shockingvrc:"))
}

#[derive(Debug, Clone, Default)]
pub struct RemoteRuntimeStatus {
    pub enabled: bool,
    pub service: RemoteStatus,
    pub probe: Arc<GraphProbe>,
    pub issues: Vec<GraphIssue>,
    pub valid: bool,
    pub doc_rev: u64,
    pub my_remote_output: Vec<(usize, [u8; MAX_CHANNELS])>,
    pub osc_statuses: BTreeMap<u64, OutputNodeStatus>,
}

struct Doc {
    graph: Graph,
    rev: u64,
}

struct Inner {
    enabled: AtomicBool,
    service: RwLock<Option<RemoteService>>,
    settings: RwLock<RemoteSettings>,
    doc: RwLock<Doc>,
    eval: Mutex<GraphEvaluator>,
    status: RwLock<RemoteRuntimeStatus>,
    dirty_at: Mutex<Option<Instant>>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pending_link: Mutex<Option<String>>,
    auto_save: AtomicBool,
}

#[derive(Clone)]
pub struct RemoteRuntime {
    inner: Arc<Inner>,
}

impl Default for RemoteRuntime {
    fn default() -> Self {
        Self::new(RemoteSettings::default(), Graph::default(), false)
    }
}

impl RemoteRuntime {
    pub fn new(settings: RemoteSettings, graph: Graph, auto_save: bool) -> Self {
        let eval = GraphEvaluator::new(&graph);
        Self {
            inner: Arc::new(Inner {
                enabled: AtomicBool::new(false),
                service: RwLock::new(None),
                settings: RwLock::new(settings.sanitized()),
                doc: RwLock::new(Doc { graph, rev: 1 }),
                eval: Mutex::new(eval),
                status: RwLock::new(RemoteRuntimeStatus::default()),
                dirty_at: Mutex::new(None),
                task: Mutex::new(None),
                pending_link: Mutex::new(None),
                auto_save: AtomicBool::new(auto_save),
            }),
        }
    }

    pub fn load_graph_from_disk(&self) {
        let loaded = load_remote_graph();
        for note in &loaded.notes {
            log::warn!("[remote] {note}");
        }
        self.replace_graph(loaded.graph, false);
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.enabled.load(Ordering::Acquire)
    }

    pub fn service(&self) -> Option<RemoteService> {
        self.inner.service.read().ok().and_then(|s| s.clone())
    }

    pub fn settings(&self) -> RemoteSettings {
        self.inner.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn stop_param(&self) -> String {
        self.inner
            .settings
            .read()
            .map(|s| s.stop_param.clone())
            .unwrap_or_default()
    }

    pub fn set_settings(&self, settings: RemoteSettings) {
        let settings = settings.sanitized();
        if let Ok(mut s) = self.inner.settings.write() {
            *s = settings.clone();
        }
        if let Some(svc) = self.service() {
            svc.set_config(settings.config.clone());
        }
        if let Err(e) = save_settings(&settings) {
            log::warn!("[remote] Could not save {REMOTE_SETTINGS_FILE}: {e}");
        }
    }

    pub fn set_auto_save(&self, on: bool) {
        self.inner.auto_save.store(on, Ordering::Release);
    }

    pub fn set_pending_link(&self, link: Option<String>) {
        if let Ok(mut p) = self.inner.pending_link.lock() {
            *p = link;
        }
    }

    pub fn take_pending_link(&self) -> Option<String> {
        self.inner.pending_link.lock().ok().and_then(|mut p| p.take())
    }

    pub fn status(&self) -> RemoteRuntimeStatus {
        if !self.is_enabled() {
            return RemoteRuntimeStatus::default();
        }
        let mut s = self.inner.status.read().map(|s| s.clone()).unwrap_or_default();
        s.enabled = true;
        if let Some(svc) = self.service() {
            s.service = svc.status();
        }
        s
    }

    pub fn graph(&self) -> (Graph, u64) {
        match self.inner.doc.read() {
            Ok(d) => (d.graph.clone(), d.rev),
            Err(_) => (Graph::default(), 0),
        }
    }

    pub fn doc_rev(&self) -> u64 {
        self.inner.doc.read().map(|d| d.rev).unwrap_or(0)
    }

    fn replace_graph(&self, mut graph: Graph, dirty: bool) -> u64 {
        graph.sanitize();
        let rev = match self.inner.doc.write() {
            Ok(mut d) => {
                if let Ok(mut ev) = self.inner.eval.lock() {
                    ev.rebuild(&graph);
                }
                d.graph = graph;
                d.rev += 1;
                d.rev
            }
            Err(_) => return 0,
        };
        if dirty && let Ok(mut d) = self.inner.dirty_at.lock() {
            *d = Some(Instant::now());
        }
        rev
    }

    pub fn set_graph(&self, graph: Graph) -> u64 {
        self.replace_graph(graph, true)
    }

    pub fn save_now(&self) -> Result<(), String> {
        let (graph, _) = self.graph();
        graph_store::save_graph_to(std::path::Path::new(REMOTE_GRAPH_FILE), &graph)?;
        if let Ok(mut d) = self.inner.dirty_at.lock() {
            *d = None;
        }
        Ok(())
    }

    pub fn flush_on_exit(&self) {
        let dirty = self.inner.dirty_at.lock().map(|d| d.is_some()).unwrap_or(false);
        if self.is_enabled()
            && dirty
            && self.inner.auto_save.load(Ordering::Acquire)
            && let Err(e) = self.save_now()
        {
            log::error!("[remote] Saving {REMOTE_GRAPH_FILE} on exit failed: {e}");
        }
    }

    fn maybe_autosave(&self) {
        if !self.inner.auto_save.load(Ordering::Acquire) {
            return;
        }
        let due = self
            .inner
            .dirty_at
            .lock()
            .map(|d| d.is_some_and(|t| t.elapsed() >= AUTOSAVE_DEBOUNCE))
            .unwrap_or(false);
        if !due {
            return;
        }
        if let Ok(mut d) = self.inner.dirty_at.lock() {
            *d = None;
        }
        let (graph, _) = self.graph();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = graph_store::save_graph_to(std::path::Path::new(REMOTE_GRAPH_FILE), &graph) {
                log::error!("[remote] Auto-save failed: {e}");
            }
        });
    }

    pub fn enable(&self, state: &Arc<AppState>) {
        if self.inner.enabled.swap(true, Ordering::AcqRel) {
            return;
        }
        kind::set_remote_nodes_enabled(true);
        let (mut local, _) = state.graph.graph();
        if local.revive_remote() > 0 {
            state.graph.set_graph(local);
        }
        self.load_graph_from_disk();
        let settings = self.settings();
        let svc = RemoteService::start(
            settings.config.clone(),
            Some(std::path::PathBuf::from(shocking_vrc_remote::store::TRUST_FILE)),
        );
        if let Ok(mut s) = self.inner.service.write() {
            *s = Some(svc);
        }
        let me = self.clone();
        let weak = Arc::downgrade(state);
        let task = tokio::spawn(async move { me.run(weak).await });
        if let Ok(mut t) = self.inner.task.lock() {
            *t = Some(task);
        }
        log::info!("[remote] Remote control enabled");
    }

    pub fn disable(&self, state: &Arc<AppState>) {
        if !self.inner.enabled.swap(false, Ordering::AcqRel) {
            return;
        }
        self.flush_on_exit_forced();
        if let Some(svc) = self.inner.service.write().ok().and_then(|mut s| s.take()) {
            svc.shutdown();
        }
        if let Some(t) = self.inner.task.lock().ok().and_then(|mut t| t.take()) {
            t.abort();
        }
        state.scanner.release_outputs(OutputSource::Remote);
        for e in state.engines() {
            e.set_remote_active(false);
        }
        kind::set_remote_nodes_enabled(false);
        let (local, _) = state.graph.graph();
        if local.has_remote_nodes()
            && let Ok(json) = serde_json::to_string(&local)
            && let Ok(inert) = shocking_vrc_core::graph::parse_graph(&json)
        {
            state.graph.set_graph(inert);
        }
        if let Ok(mut s) = self.inner.status.write() {
            *s = RemoteRuntimeStatus::default();
        }
        log::info!("[remote] Remote control disabled");
    }

    fn flush_on_exit_forced(&self) {
        let dirty = self.inner.dirty_at.lock().map(|d| d.is_some()).unwrap_or(false);
        if dirty && let Err(e) = self.save_now() {
            log::error!("[remote] Saving {REMOTE_GRAPH_FILE} failed: {e}");
        }
    }

    pub fn shutdown(&self) {
        self.flush_on_exit();
        if let Some(svc) = self.service() {
            svc.shutdown();
        }
    }

    pub fn stop_all(&self, state: &AppState, reason: &str) {
        if let Some(svc) = self.service() {
            svc.stop_all(reason);
        }
        state.scanner.release_outputs(OutputSource::Remote);
        for e in state.engines() {
            e.set_remote_active(false);
        }
    }

    async fn run(self, state: Weak<AppState>) {
        let mut ticker = tokio::time::interval(GRAPH_TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last = Instant::now();
        let mut n: u64 = 0;
        let mut stop_was_on = true;
        loop {
            ticker.tick().await;
            let Some(state) = state.upgrade() else { break };
            let Some(svc) = self.service() else { break };
            n = n.wrapping_add(1);
            let now = Instant::now();
            let dt = now.saturating_duration_since(last).as_secs_f32();
            last = now;
            let settings = self.settings();
            self.maybe_autosave();

            let engines = state.engines();
            let receiving = svc.receiving();
            for (i, e) in engines.iter().enumerate() {
                e.set_remote_policy(settings.slot_channels(i));
                e.set_remote_active(receiving && settings.allows(i));
            }
            if let Some(frame) = svc.take_inbound() {
                for sf in &frame.slots {
                    let slot = sf.slot as usize;
                    if receiving
                        && settings.allows(slot)
                        && let Some(e) = engines.get(slot)
                    {
                        e.submit_remote_frame(sf.channels());
                    }
                }
            }
            let mine: Vec<(usize, [u8; MAX_CHANNELS])> = engines
                .iter()
                .enumerate()
                .filter(|(i, _)| receiving && settings.allows(*i))
                .map(|(i, e)| (i, e.remote_output().map(|c| c.strength)))
                .collect();
            if receiving && n.is_multiple_of(OUTPUT_EVERY) {
                svc.set_output_report(OutputReport {
                    slots: mine.iter().map(|(i, s)| (*i as u8, *s)).collect(),
                });
            }
            if n % POLICY_EVERY == 1 {
                let names = state.slot_infos();
                let fallback_channels = settings
                    .slots
                    .first()
                    .map(|s| settings.slot_channels(*s as usize))
                    .unwrap_or_default();
                svc.set_policy(PolicySnapshot {
                    slots: settings
                        .slots
                        .iter()
                        .map(|s| SlotInfo {
                            slot: *s,
                            name: names.get(*s as usize).map(|i| i.name.clone()).unwrap_or_default(),
                            channels: Some(settings.slot_channels(*s as usize)),
                        })
                        .collect(),
                    channels: fallback_channels,
                });
            }

            let addr = settings.stop_param.trim();
            if !addr.is_empty() {
                let on = state.scanner.with_raw_inputs(|raw| {
                    raw.params.get(addr).is_some_and(|p| {
                        p.value > 0.5 && now.saturating_duration_since(p.updated) < Duration::from_secs(2)
                    })
                });
                if on && !stop_was_on {
                    log::warn!("[remote] Stop parameter {addr} turned on");
                    self.stop_all(&state, "stop parameter");
                }
                stop_was_on = on;
            }

            let controlling = svc.controlling();
            let ui_panic = state.graph.ui_panic();
            let (active_out, osc_outs, chatboxes, published) = {
                let Ok(mut ev) = self.inner.eval.lock() else { continue };
                let guard = shocking_vrc_core::cli::spoof_guard_enabled();
                state
                    .scanner
                    .with_raw_inputs(|raw| ev.fill_external(|req| sample(req, raw, now, guard)));
                ev.set_alarm(AlarmSample::default());
                ev.set_partner(svc.partner_context());
                ev.step(dt);
                let ok = ev.is_valid() && !ev.panic_requested();
                if controlling {
                    let slots: Vec<SlotFrame> = if ok {
                        ev.partner_frames()
                            .iter()
                            .map(|f| SlotFrame {
                                slot: f.slot,
                                a: Some(WireChannel::from_frame(&f.channels[0])),
                                b: Some(WireChannel::from_frame(&f.channels[1])),
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    svc.send_frame(slots);
                }
                let active_out = controlling && !ui_panic && ok;
                let (osc_outs, chatboxes) = if active_out {
                    (ev.osc_out_frames().to_vec(), ev.chatbox_frames().to_vec())
                } else {
                    (Vec::new(), Vec::new())
                };
                let published = n
                    .is_multiple_of(PUBLISH_EVERY)
                    .then(|| (ev.probe(), ev.issues().to_vec(), ev.is_valid()));
                (active_out, osc_outs, chatboxes, published)
            };
            if active_out {
                state
                    .scanner
                    .submit_outputs(OutputSource::Remote, osc_outs, chatboxes, Some(&settings.stop_param));
            } else {
                state.scanner.release_outputs(OutputSource::Remote);
            }
            if let Some((probe, issues, valid)) = published {
                let rev = self.doc_rev();
                let osc_statuses = state.scanner.output_statuses(OutputSource::Remote);
                if let Ok(mut s) = self.inner.status.write() {
                    s.probe = Arc::new(probe);
                    s.issues = issues;
                    s.valid = valid;
                    s.doc_rev = rev;
                    s.my_remote_output = mine;
                    s.osc_statuses = osc_statuses;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starter_graph_drives_a_partner_device() {
        let g = starter_graph();
        let mut ev = GraphEvaluator::new(&g);
        assert!(ev.is_valid());
        for _ in 0..4 {
            ev.step(0.025);
        }
        assert_eq!(ev.partner_frames().len(), 1);
    }

    #[test]
    fn settings_are_sanitised_and_round_trip() {
        let mut s = RemoteSettings {
            slots: vec![3, 1, 1, 99],
            ..RemoteSettings::default()
        };
        s.slot_channels_mut(1)[0].cap = 55;
        s.slot_channels_mut(3)[1].allowed = false;
        s.slot_channels_mut(3)[0].freq_bounds = [40, 120];
        s.slot_channels_mut(3)[0].int_bounds = [20, 80];
        let s = s.sanitized();

        assert_eq!(s.slots, vec![1, 3]);
        assert!(s.allows(3) && !s.allows(0));
        assert_eq!(s.slot_channels(1)[0].cap, 55);
        assert_eq!(s.slot_channels(3)[0].cap, 30, "slot 1 edits do not leak to slot 3");
        assert!(!s.slot_channels(3)[1].allowed);
        assert!(s.slot_channels(1)[1].allowed);
        assert_eq!(s.slot_channels(3)[0].freq_bounds, [10, 240]);
        assert_eq!(s.slot_channels(3)[0].int_bounds, [0, 100]);

        let back: RemoteSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let old: RemoteSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.slots, vec![0], "a missing file means slot 1 only");
    }

    #[test]
    fn legacy_settings_channels_seed_all_slots_and_reset_bounds() {
        let legacy_json = r#"{
            "channels": [
                {"allowed": true, "cap": 65, "freq_bounds": [30, 90], "int_bounds": [15, 70], "ramp_per_s": 25.0, "merge": "RemoteOnly"},
                {"allowed": false, "cap": 10, "freq_bounds": [20, 80], "int_bounds": [5, 50], "ramp_per_s": 0.0, "merge": "LocalOnly"}
            ],
            "slots": [0, 1]
        }"#;
        let loaded: RemoteSettings = serde_json::from_str(legacy_json).unwrap();
        let loaded = loaded.sanitized();
        assert_eq!(loaded.slots, vec![0, 1]);
        for slot in 0..MAX_REMOTE_SLOTS {
            assert_eq!(loaded.slot_channels(slot)[0].cap, 65);
            assert_eq!(loaded.slot_channels(slot)[0].freq_bounds, [10, 240]);
            assert_eq!(loaded.slot_channels(slot)[0].int_bounds, [0, 100]);
            assert!(!loaded.slot_channels(slot)[1].allowed);
            assert_eq!(loaded.slot_channels(slot)[1].cap, 10);
        }
    }

    #[test]
    fn a_disabled_runtime_reports_nothing() {
        let rt = RemoteRuntime::default();
        assert!(!rt.is_enabled());
        assert!(rt.service().is_none());
        let s = rt.status();
        assert!(!s.enabled && s.service.sessions.is_empty());
    }
}
