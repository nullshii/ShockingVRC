use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use shocking_vrc_core::OldZoneType;
use shocking_vrc_core::cli::{CliEngine, EngineMode, GraphChannelOutput, spoof_guard_enabled};
use shocking_vrc_core::graph::compile::ExternalRequest;
use shocking_vrc_core::graph::device::{ChannelFrame, MAX_CHANNELS};
use shocking_vrc_core::graph::{
    AlarmSample, ExternalSample, Graph, GraphEvaluator, GraphIssue, GraphProbe,
};
use shocking_vrc_core::osc::RawInputs;

use crate::app_state::AppState;
use crate::graph_store;

pub const GRAPH_TICK: Duration = Duration::from_millis(25);
const AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(1500);
const PUBLISH_EVERY: u64 = 2;
const DEVICE_REFRESH_EVERY: u64 = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceOutput {
    pub mac: String,
    pub name: String,
    pub connected: bool,
    pub battery: Option<u8>,
    pub channels: [GraphChannelOutput; MAX_CHANNELS],
}

#[derive(Debug, Clone, Default)]
pub struct GraphStatus {
    pub mode: EngineMode,
    pub tick: u64,
    pub probe: Arc<GraphProbe>,
    pub issues: Vec<GraphIssue>,
    pub valid: bool,
    pub outputs: Vec<DeviceOutput>,
    pub panic: bool,
    pub ui_panic: bool,
    pub caps: [u8; MAX_CHANNELS],
    pub seen_params: Arc<Vec<(String, f32)>>,
    pub doc_rev: u64,
    pub notes: Vec<String>,
}

struct Doc {
    graph: Graph,
    rev: u64,
}

struct Inner {
    doc: RwLock<Doc>,
    eval: Mutex<GraphEvaluator>,
    mode: AtomicU8,
    ui_panic: AtomicBool,
    caps: Mutex<[u8; MAX_CHANNELS]>,
    auto_save: AtomicBool,
    dirty_at: Mutex<Option<Instant>>,
    ramp_pending: AtomicBool,
    status: RwLock<GraphStatus>,
    notes: Mutex<Vec<String>>,
}

#[derive(Clone)]
pub struct GraphRuntime {
    inner: Arc<Inner>,
}

impl Default for GraphRuntime {
    fn default() -> Self {
        Self::new(
            Graph::default(),
            EngineMode::Classic,
            [shocking_vrc_core::graph::safety::DEFAULT_POWER_CAP; MAX_CHANNELS],
            false,
            Vec::new(),
        )
    }
}

fn mode_byte(m: EngineMode) -> u8 {
    match m {
        EngineMode::Classic => 0,
        EngineMode::Graph => 1,
    }
}

impl GraphRuntime {
    pub fn new(
        graph: Graph,
        mode: EngineMode,
        caps: [u8; MAX_CHANNELS],
        auto_save: bool,
        notes: Vec<String>,
    ) -> Self {
        let eval = GraphEvaluator::new(&graph);
        Self {
            inner: Arc::new(Inner {
                doc: RwLock::new(Doc { graph, rev: 1 }),
                eval: Mutex::new(eval),
                mode: AtomicU8::new(mode_byte(mode)),
                ui_panic: AtomicBool::new(false),
                caps: Mutex::new(caps.map(|c| c.min(200))),
                auto_save: AtomicBool::new(auto_save),
                dirty_at: Mutex::new(None),
                ramp_pending: AtomicBool::new(false),
                status: RwLock::new(GraphStatus {
                    mode,
                    caps,
                    ..GraphStatus::default()
                }),
                notes: Mutex::new(notes),
            }),
        }
    }

    pub fn mode(&self) -> EngineMode {
        match self.inner.mode.load(Ordering::Acquire) {
            1 => EngineMode::Graph,
            _ => EngineMode::Classic,
        }
    }

    pub fn caps(&self) -> [u8; MAX_CHANNELS] {
        self.inner.caps.lock().map(|c| *c).unwrap_or([0; MAX_CHANNELS])
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

    pub fn status(&self) -> GraphStatus {
        self.inner
            .status
            .read()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn set_graph(&self, mut graph: Graph) -> u64 {
        graph.sanitize();
        let rev = match self.inner.doc.write() {
            Ok(mut d) => {
                let rebuild = match self.inner.eval.lock() {
                    Ok(mut ev) => ev.rebuild(&graph),
                    Err(_) => return d.rev,
                };
                if rebuild.topology_changed {
                    self.inner.ramp_pending.store(true, Ordering::Release);
                }
                d.graph = graph;
                d.rev += 1;
                d.rev
            }
            Err(_) => return 0,
        };
        self.mark_dirty();
        rev
    }

    fn mark_dirty(&self) {
        if let Ok(mut d) = self.inner.dirty_at.lock() {
            *d = Some(Instant::now());
        }
    }

    pub fn set_ui_panic(&self, on: bool) {
        let was = self.inner.ui_panic.swap(on, Ordering::AcqRel);
        if was != on {
            if on {
                log::warn!("[graph] PANIC — every device output forced to 0");
            } else {
                log::info!("[graph] Panic released — ramping back in");
            }
        }
    }

    pub fn ui_panic(&self) -> bool {
        self.inner.ui_panic.load(Ordering::Acquire)
    }

    pub fn set_auto_save(&self, on: bool) {
        self.inner.auto_save.store(on, Ordering::Release);
    }

    pub fn save_now(&self) -> Result<(), String> {
        let (graph, _) = self.graph();
        graph_store::save_graph(&graph)?;
        if let Ok(mut d) = self.inner.dirty_at.lock() {
            *d = None;
        }
        Ok(())
    }

    pub fn flush_on_exit(&self) {
        let dirty = self.inner.dirty_at.lock().map(|d| d.is_some()).unwrap_or(false);
        if dirty && self.inner.auto_save.load(Ordering::Acquire) {
            match self.save_now() {
                Ok(()) => log::info!("[graph] Saved {} on exit", graph_store::GRAPH_FILE),
                Err(e) => log::error!("[graph] Save on exit failed: {e}"),
            }
        }
    }

    pub async fn prepare_engine(&self, engine: &CliEngine) {
        engine.set_graph_caps(self.caps());
        if self.mode() != engine.mode() {
            engine.set_mode(self.mode()).await;
        }
    }

    pub async fn set_mode(&self, mode: EngineMode, engines: &[CliEngine]) {
        if mode == EngineMode::Graph
            && let (Ok(d), Ok(mut ev)) = (self.inner.doc.read(), self.inner.eval.lock())
        {
            *ev = GraphEvaluator::new(&d.graph);
        }
        self.inner.mode.store(mode_byte(mode), Ordering::Release);
        for e in engines {
            e.set_mode(mode).await;
        }
        if let Ok(mut s) = self.inner.status.write() {
            s.mode = mode;
            s.outputs.clear();
        }
    }

    pub fn set_caps(&self, caps: [u8; MAX_CHANNELS], engines: &[CliEngine]) {
        let caps = caps.map(|c| c.min(200));
        if let Ok(mut c) = self.inner.caps.lock() {
            *c = caps;
        }
        for e in engines {
            e.set_graph_caps(caps);
        }
    }

    fn ensure_device_nodes(&self, devices: &[(String, String)]) {
        let Ok(mut d) = self.inner.doc.write() else {
            return;
        };
        let added = d.graph.ensure_device_nodes(devices);
        if added.is_empty() {
            return;
        }
        if let Ok(mut ev) = self.inner.eval.lock() {
            ev.rebuild(&d.graph);
        }
        d.rev += 1;
        drop(d);
        log::info!("[graph] Added {} Device node(s) for connected devices", added.len());
        self.mark_dirty();
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
        tokio::task::spawn_blocking(move || match graph_store::save_graph(&graph) {
            Ok(()) => log::debug!("[graph] Auto-saved {}", graph_store::GRAPH_FILE),
            Err(e) => log::error!("[graph] Auto-save failed: {e}"),
        });
    }

    pub fn spawn(&self, state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move { me.run(state).await })
    }

    async fn run(self, state: Arc<AppState>) {
        let mut ticker = tokio::time::interval(GRAPH_TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last = Instant::now();
        let mut engines: Vec<(String, CliEngine)> = Vec::new();
        let mut frames: Vec<Option<[ChannelFrame; MAX_CHANNELS]>> = Vec::new();
        let mut outs: Vec<[GraphChannelOutput; MAX_CHANNELS]> = Vec::new();
        let mut was_panic = false;
        let mut n: u64 = 0;

        loop {
            ticker.tick().await;
            n = n.wrapping_add(1);
            let now = Instant::now();
            let dt = now.saturating_duration_since(last).as_secs_f32();
            last = now;
            self.maybe_autosave();

            if n % DEVICE_REFRESH_EVERY == 1 {
                let params = state.scanner.seen_params();
                if let Ok(mut s) = self.inner.status.write() {
                    s.seen_params = Arc::new(params);
                }
            }
            if self.mode() != EngineMode::Graph {
                continue;
            }

            let slot_count = state.device_slots.read().map(|s| s.len()).unwrap_or(0);
            if n % DEVICE_REFRESH_EVERY == 1 || slot_count != engines.len() {
                engines = state
                    .device_slots
                    .read()
                    .map(|slots| {
                        slots
                            .iter()
                            .map(|s| (s.mac.clone(), s.engine.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                let named: Vec<(String, String)> = state
                    .slot_infos()
                    .into_iter()
                    .map(|i| (i.mac, i.name))
                    .collect();
                self.ensure_device_nodes(&named);
            }

            let guard = spoof_guard_enabled();
            let alarm = AlarmSample {
                strength: state.alarm.strength() as f32 / 200.0,
                active: state.alarm.is_active(),
            };
            let publish = n.is_multiple_of(PUBLISH_EVERY);
            let mut published = None;
            let panic = {
                let Ok(mut ev) = self.inner.eval.lock() else {
                    continue;
                };
                state
                    .scanner
                    .with_raw_inputs(|raw| ev.fill_external(|req| sample(req, raw, now, guard)));
                ev.set_alarm(alarm);
                ev.step(dt);
                frames.clear();
                for (mac, _) in &engines {
                    let f = if ev.is_valid() {
                        ev.frames()
                            .iter()
                            .find(|f| f.binding.mac().is_some_and(|m| m.eq_ignore_ascii_case(mac)))
                            .map(|f| f.channels)
                    } else {
                        None
                    };
                    frames.push(f);
                }
                if publish {
                    published = Some((ev.probe(), ev.issues().to_vec(), ev.is_valid()));
                }
                ev.panic_requested() || self.ui_panic()
            };

            let ramp = self.inner.ramp_pending.swap(false, Ordering::AcqRel) || (was_panic && !panic);
            was_panic = panic;
            if ramp {
                for (_, e) in &engines {
                    e.restart_graph_ramp().await;
                }
            }

            outs.clear();
            for ((_, e), f) in engines.iter().zip(&frames) {
                outs.push(e.submit_graph_frame(f.as_ref(), panic).await);
            }

            if let Some((probe, issues, valid)) = published {
                let infos = state.slot_infos();
                let outputs = infos
                    .into_iter()
                    .map(|i| {
                        let channels = engines
                            .iter()
                            .position(|(m, _)| m.eq_ignore_ascii_case(&i.mac))
                            .and_then(|k| outs.get(k).copied())
                            .unwrap_or_default();
                        DeviceOutput {
                            mac: i.mac,
                            name: i.name,
                            connected: i.connected,
                            battery: i.battery,
                            channels,
                        }
                    })
                    .collect();
                let notes = self.inner.notes.lock().map(|n| n.clone()).unwrap_or_default();
                let doc_rev = self.doc_rev();
                if let Ok(mut s) = self.inner.status.write() {
                    s.mode = EngineMode::Graph;
                    s.tick = probe.tick;
                    s.probe = Arc::new(probe);
                    s.issues = issues;
                    s.valid = valid;
                    s.outputs = outputs;
                    s.panic = panic;
                    s.ui_panic = self.ui_panic();
                    s.caps = self.caps();
                    s.doc_rev = doc_rev;
                    s.notes = notes;
                }
            }
        }
    }
}

pub fn sample(req: &ExternalRequest, raw: &RawInputs, now: Instant, guard: bool) -> ExternalSample {
    match req {
        ExternalRequest::Contact(p) => {
            let stale = Duration::from_millis(p.stale_ms as u64);
            let mut level = 0.0f32;
            let mut fresh = false;
            for c in &raw.contacts {
                let type_ok = p.zone.zone_type == OldZoneType::Any || p.zone.zone_type == c.zone_type;
                let name_ok = p.zone.name == "*" || p.zone.name.eq_ignore_ascii_case(&c.id);
                if !type_ok || !name_ok || now.saturating_duration_since(c.updated) > stale {
                    continue;
                }
                if guard && p.spoof_guard && c.blocked {
                    continue;
                }
                level = level.max(c.level);
                fresh |= c.level > 0.0;
            }
            ExternalSample { value: level, fresh }
        }
        ExternalRequest::Osc(p) => match raw.params.get(p.address.as_str()) {
            Some(s) => ExternalSample {
                value: s.value,
                fresh: now.saturating_duration_since(s.updated)
                    <= Duration::from_millis(p.stale_ms as u64),
            },
            None => ExternalSample::default(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shocking_vrc_core::cli::config::ZoneId;
    use shocking_vrc_core::graph::kind::{ContactParams, OscFloatParams};
    use shocking_vrc_core::osc::{ParamSample, RawContact};

    fn raw(now: Instant) -> RawInputs {
        let mut r = RawInputs::default();
        for (t, id, level, blocked) in [
            (OldZoneType::Pen, "Cock", 0.4, false),
            (OldZoneType::Orf, "Pussy", 0.7, false),
            (OldZoneType::DGB, "Spoof", 0.95, true),
        ] {
            r.contacts.push(RawContact {
                zone_type: t,
                id: id.into(),
                level,
                updated: now,
                blocked,
            });
        }
        r.params.insert(
            "/avatar/parameters/Max".into(),
            ParamSample {
                value: 0.25,
                updated: now - Duration::from_secs(10),
            },
        );
        r
    }

    fn contact(t: OldZoneType, name: &str) -> ExternalRequest {
        ExternalRequest::Contact(ContactParams {
            zone: ZoneId::new(t, name),
            ..ContactParams::default()
        })
    }

    #[test]
    fn wildcards_take_the_strongest_match() {
        let now = Instant::now();
        let r = raw(now);
        assert_eq!(sample(&contact(OldZoneType::Pen, "*"), &r, now, true).value, 0.4);
        assert_eq!(sample(&contact(OldZoneType::Any, "*"), &r, now, true).value, 0.7);
        assert_eq!(sample(&contact(OldZoneType::Orf, "pussy"), &r, now, true).value, 0.7);
        assert_eq!(sample(&contact(OldZoneType::Touch, "*"), &r, now, true).value, 0.0);
    }

    #[test]
    fn the_spoof_guard_applies_only_when_on() {
        let now = Instant::now();
        let r = raw(now);
        let req = contact(OldZoneType::DGB, "Spoof");
        assert_eq!(sample(&req, &r, now, true).value, 0.0);
        assert_eq!(sample(&req, &r, now, false).value, 0.95);
    }

    #[test]
    fn stale_contacts_read_zero_but_osc_values_persist() {
        let now = Instant::now();
        let r = raw(now);
        let later = now + Duration::from_secs(2);
        assert_eq!(sample(&contact(OldZoneType::Pen, "Cock"), &r, later, true).value, 0.0);
        let osc = ExternalRequest::Osc(OscFloatParams {
            address: "/avatar/parameters/Max".into(),
            stale_ms: 5000,
        });
        let s = sample(&osc, &r, now, true);
        assert_eq!(s.value, 0.25, "last value is kept");
        assert!(!s.fresh, "but reported stale");
    }

    #[tokio::test]
    async fn set_graph_bumps_the_revision_and_flags_topology_changes() {
        let rt = GraphRuntime::new(Graph::default(), EngineMode::Graph, [40, 40], false, Vec::new());
        let r0 = rt.doc_rev();
        let mut g = Graph::default();
        g.add_kind(
            shocking_vrc_core::graph::NodeKind::Time(Default::default()),
            [0.0, 0.0],
        );
        let r1 = rt.set_graph(g);
        assert!(r1 > r0);
        assert!(rt.inner.ramp_pending.load(Ordering::Acquire));
        assert_eq!(rt.graph().0.nodes.len(), 1);
    }

    #[tokio::test]
    async fn an_osc_slider_on_limit_max_is_capped_end_to_end() {
        use shocking_vrc_core::cli::{AlarmConfig, AlarmController, CliConfig};
        use shocking_vrc_core::graph::kind::{ConstantParams, DeviceParams, MathOp, MathParams};
        use shocking_vrc_core::graph::{NodeKind, PinRef};

        let mut g = Graph::default();
        let osc = g.add_kind(
            NodeKind::OscFloat(OscFloatParams {
                address: "/avatar/parameters/MaxPower".into(),
                stale_ms: 5000,
            }),
            [0.0, 0.0],
        );
        let scale = g.add_kind(NodeKind::Math(MathParams { op: MathOp::Multiply }), [0.0, 0.0]);
        g.node_mut(scale).unwrap().set_input_default("b", 200.0);
        let one = g.add_kind(NodeKind::Constant(ConstantParams { value: 1.0 }), [0.0, 0.0]);
        let dev = g.add_kind(NodeKind::Device(DeviceParams::bound_to("AA")), [0.0, 0.0]);
        g.connect(PinRef::new(osc, "value"), PinRef::new(scale, "a")).unwrap();
        g.connect(PinRef::new(scale, "out"), PinRef::new(dev, "a.limit_max")).unwrap();
        g.connect(PinRef::new(one, "out"), PinRef::new(dev, "a.power")).unwrap();

        let engine = CliEngine::new(CliConfig::default(), AlarmController::new(AlarmConfig::default()));
        engine.set_mode(EngineMode::Graph).await;
        engine.set_graph_caps([40, 40]);
        let mut ev = GraphEvaluator::new(&g);

        let mut last = 0;
        for (slider, expect_max) in [(0.1, 20u8), (0.15, 30), (0.9, 40), (1.0, 40)] {
            let now = Instant::now();
            let mut raw = RawInputs::default();
            raw.params.insert(
                "/avatar/parameters/MaxPower".into(),
                ParamSample { value: slider, updated: now },
            );
            ev.fill_external(|req| sample(req, &raw, now, true));
            ev.step(0.025);
            let frame = ev.frames()[0].channels;
            let out = engine.submit_graph_frame(Some(&frame), false).await;
            assert_eq!(out[0].safe.limit_max, expect_max, "slider {slider}");
            assert!(out[0].safe.strength <= 40 && out[0].safe.ceiling <= 40);
            last = out[0].safe.limit_max;
        }
        assert_eq!(last, 40, "the cap holds at full slider");
    }

    #[tokio::test]
    async fn the_tick_loop_adds_device_nodes_and_feeds_engines() {
        use crate::app_state::DeviceSlot;
        use shocking_vrc_core::AvatarScanner;
        use shocking_vrc_core::cli::{AlarmConfig, AlarmController, CliConfig};
        use shocking_vrc_core::graph::kind::{ConstantParams, DeviceParams};
        use shocking_vrc_core::graph::{NodeKind, PinRef};

        let mut g = Graph::default();
        let one = g.add_kind(NodeKind::Constant(ConstantParams { value: 1.0 }), [0.0, 0.0]);
        let dev = g.add_kind(NodeKind::Device(DeviceParams::bound_to("AA:AA")), [0.0, 0.0]);
        g.connect(PinRef::new(one, "out"), PinRef::new(dev, "a.power")).unwrap();

        let alarm = AlarmController::new(AlarmConfig::default());
        let rt = GraphRuntime::new(g, EngineMode::Graph, [20, 20], false, Vec::new());
        let scanner = AvatarScanner::new(Some(0));
        let mut slots = Vec::new();
        for mac in ["AA:AA", "BB:BB"] {
            let engine = CliEngine::new(CliConfig::default(), alarm.clone());
            rt.prepare_engine(&engine).await;
            let stop_handle = engine.start(&scanner).await;
            slots.push(DeviceSlot {
                mac: mac.into(),
                name: "Coyote".into(),
                engine,
                stop_handle,
                battery: None,
                connected: true,
                reconnecting: false,
            });
        }
        let state = Arc::new(AppState {
            scanner,
            monitor_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            default_config: CliConfig::default(),
            device_slots: Arc::new(RwLock::new(slots)),
            alarm,
            graph: rt.clone(),
        });
        let task = rt.spawn(Arc::clone(&state));
        tokio::time::sleep(Duration::from_millis(700)).await;

        let (graph, _) = rt.graph();
        assert!(graph.device_node_for("BB:BB").is_some(), "a node for the second device");
        let a = state.engine_for_mac("AA:AA").unwrap().graph_output();
        assert!(a[0].safe.strength > 0, "frames reach the engine");
        assert!(a[0].safe.strength < 20, "still ramping in after 0.7 s: {}", a[0].safe.strength);
        let b = state.engine_for_mac("BB:BB").unwrap().graph_output();
        assert_eq!(b[0].safe.strength, 0, "the unwired device stays at 0");
        let status = rt.status();
        assert_eq!(status.outputs.len(), 2);
        assert!(status.tick > 10);

        rt.set_ui_panic(true);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let a = state.engine_for_mac("AA:AA").unwrap().graph_output();
        assert_eq!(a[0].safe.strength, 0, "panic silences it");
        task.abort();
    }

    #[tokio::test]
    async fn mode_switch_reaches_every_engine() {
        use shocking_vrc_core::cli::{AlarmConfig, AlarmController, CliConfig};
        let alarm = AlarmController::new(AlarmConfig::default());
        let engines = vec![
            CliEngine::new(CliConfig::default(), alarm.clone()),
            CliEngine::new(CliConfig::default(), alarm),
        ];
        let rt = GraphRuntime::new(Graph::default(), EngineMode::Classic, [40, 40], false, Vec::new());
        rt.set_mode(EngineMode::Graph, &engines).await;
        assert!(engines.iter().all(|e| e.mode() == EngineMode::Graph));
        rt.set_caps([12, 34], &engines);
        assert!(engines.iter().all(|e| e.graph_caps() == [12, 34]));
        rt.set_mode(EngineMode::Classic, &engines).await;
        assert!(engines.iter().all(|e| e.mode() == EngineMode::Classic));
    }
}
