use std::sync::Arc;

use shocking_vrc_app::app_state::DeviceSlotInfo;
use shocking_vrc_app::graph_runtime::{DeviceOutput, GraphStatus};
use shocking_vrc_core::cli::{EngineMode, GraphChannelOutput};
use shocking_vrc_core::graph::kind::{
    ContactParams, CurveParams, DeviceParams, FrameParams, LfoParams, NoParams, UkfNodeParams,
};
use shocking_vrc_core::graph::safety::{AlarmOverlay, safe_channel};
use shocking_vrc_core::graph::{
    Curve, CurvePreset, ExternalSample, Graph, GraphEvaluator, GraphPreset, GraphPresetEntry, Node,
    NodeId, NodeKind, PinRef,
};
use shocking_vrc_core::cli::{
    AggregationMode, AlarmChannels, AlarmConfig, AlarmPhase, AlarmStatus, ChannelConfig,
    ChannelStatus, CliConfig, CliStatus, ContactMode, PowerLimits, ZoneEntry, ZoneId,
};
use shocking_vrc_core::modulation::config::{
    ModulationConfig, ModulationFunction, ModulationSource,
};
use shocking_vrc_core::modulation::evaluator::KinematicsInput;
use shocking_vrc_core::presets::{ChannelPreset, PresetEntry, ZonePreset, ZonePresetEntry};
use shocking_vrc_core::{OldZoneType, ZoneEvent};

use crate::bridge::{
    Bridge, GraphDocShared, GraphPresetState, PresetState, Shared, Snapshot, ZonePresetState,
};

pub fn detached_bridge() -> (Bridge, tokio::sync::mpsc::UnboundedReceiver<crate::bridge::Command>)
{
    Bridge::detached()
}

fn zone(t: OldZoneType, name: &str, mode: ContactMode) -> ZoneEntry {
    ZoneEntry::new(ZoneId::new(t, name), mode)
}

fn modulation(function: ModulationFunction, source: ModulationSource) -> ModulationConfig {
    let mut c = ModulationConfig {
        function,
        source,
        base_speed: 1.4,
        max_deviation: 35.0,
        ..ModulationConfig::default()
    };
    c.sanitise(false);
    c
}

pub fn demo_config() -> CliConfig {
    let mut a = ChannelConfig {
        zones: vec![
            zone(OldZoneType::Orf, "Pussy", ContactMode::Depth),
            {
                let mut z = zone(OldZoneType::DGB, "TouchAreaA", ContactMode::Speed);
                z.scale = 70;
                z.min_threshold = 12;
                z.max_threshold = 88;
                z
            },
        ],
        frequency: [30, 200, 30, 200],
        intensity: [100, 90, 100, 85],
        limits: PowerLimits::new(0, 30),
        aggregation: AggregationMode::Max,
        ..ChannelConfig::default()
    };
    a.freq_modulation[0] = Some(modulation(
        ModulationFunction::Sin,
        ModulationSource::Depth,
    ));
    a.intensity_modulation[2] = Some({
        let mut c = modulation(ModulationFunction::Triangle, ModulationSource::Speed);
        c.max_deviation = 12.0;
        c.clamp_min = 0.0;
        c.clamp_max = 100.0;
        c.sanitise(true);
        c
    });

    let b = ChannelConfig {
        zones: vec![zone(OldZoneType::Pen, "Cock", ContactMode::Depth)],
        frequency: [30, 220, 60, 140],
        intensity: [100, 100, 100, 100],
        limits: PowerLimits::new(0, 42),
        aggregation: AggregationMode::Sum,
        ..ChannelConfig::default()
    };

    let mut cfg = CliConfig {
        channel_a: a,
        channel_b: b,
        ..CliConfig::default()
    };
    cfg.sanitise();
    cfg
}

fn avatar_zone(t: OldZoneType, id: &str, level: f32) -> ZoneEvent {
    ZoneEvent {
        zone_type: t,
        id: id.to_string(),
        is_tps: false,
        level,
        velocity: level * 2.0,
        acceleration: level * 6.0,
        recoil: level * 20.0,
    }
}

pub fn demo_snapshot() -> Snapshot {
    let config = demo_config();
    let status = CliStatus {
        channel_a: ChannelStatus {
            raw_level: 0.62,
            strength: 19,
            frequency: [34, 200, 30, 200],
            active_zones: vec![
                (ZoneId::new(OldZoneType::Orf, "Pussy"), 0.62),
                (ZoneId::new(OldZoneType::DGB, "TouchAreaA"), 0.28),
            ],
            kinematics: KinematicsInput {
                depth: 0.62,
                speed: 0.31,
                acc: 0.18,
                recoil: 0.05,
            },
        },
        channel_b: ChannelStatus {
            raw_level: 0.0,
            strength: 0,
            frequency: [30, 220, 60, 140],
            active_zones: Vec::new(),
            kinematics: KinematicsInput::default(),
        },
        device_connected: true,
    };

    Snapshot {
        config,
        status,
        alarm: AlarmConfig {
            enabled: true,
            hour: 7,
            minute: 30,
            channels: AlarmChannels::Both,
            ..AlarmConfig::default()
        },
        alarm_status: AlarmStatus {
            phase: AlarmPhase::Idle,
            next_fire_in_mins: 265,
            ..AlarmStatus::default()
        },
        avatar_zones: {
            let mut zones = vec![
                avatar_zone(OldZoneType::Orf, "Pussy", 0.62),
                avatar_zone(OldZoneType::Pen, "Cock", 0.0),
                avatar_zone(OldZoneType::DGB, "TouchAreaA", 0.28),
                avatar_zone(OldZoneType::DGB, "Chest", 0.0),
                avatar_zone(OldZoneType::Touch, "HeadPat", 0.0),
            ];
            for i in 0..18 {
                zones.push(avatar_zone(
                    OldZoneType::DGB,
                    &format!("Contact_{i:02}"),
                    0.0,
                ));
            }
            zones
        },
        vrchat_found: true,
        osc_port: 33777,
        oscquery_port: Some(33777),
        devices: vec![
            DeviceSlotInfo {
                mac: "AA:BB:CC:DD:EE:FF".into(),
                name: "DG-Lab Coyote V3".into(),
                battery: Some(75),
                connected: true,
                reconnecting: false,
            },
            DeviceSlotInfo {
                mac: "11:22:33:44:55:66".into(),
                name: "Coyote V3 (2)".into(),
                battery: Some(38),
                connected: false,
                reconnecting: true,
            },
        ],
        active_device: 0,
        revision: 1,
        mode: EngineMode::Classic,
        graph: demo_graph_status(),
    }
}

pub fn graph_snapshot() -> Snapshot {
    let mut s = demo_snapshot();
    s.mode = EngineMode::Graph;
    s
}

fn place(g: &mut Graph, id: u64, kind: NodeKind, pos: [f32; 2], title: &str) -> NodeId {
    let mut n = Node::new(kind, pos);
    n.id = NodeId(id);
    n.title = title.to_string();
    g.add(n)
}

pub fn demo_graph() -> Graph {
    let mut g = Graph::default();
    let contact = place(
        &mut g,
        101,
        NodeKind::Contact(ContactParams {
            zone: ZoneId::new(OldZoneType::Orf, "Pussy"),
            ..ContactParams::default()
        }),
        [20.0, 40.0],
        "",
    );
    let ukf = place(&mut g, 102, NodeKind::Ukf(UkfNodeParams::default()), [270.0, 150.0], "");
    let curve = place(
        &mut g,
        103,
        NodeKind::Curve(CurveParams {
            curve: Curve::preset(CurvePreset::EaseOut),
        }),
        [530.0, 230.0],
        "",
    );
    let lfo = place(&mut g, 104, NodeKind::Lfo(LfoParams::default()), [270.0, 470.0], "");
    let metro = place(&mut g, 108, NodeKind::Metronome(NoParams {}), [530.0, 470.0], "");
    let dev = place(
        &mut g,
        105,
        NodeKind::Device(DeviceParams::bound_to("AA:BB:CC:DD:EE:FF")),
        [800.0, 10.0],
        "DG-Lab Coyote V3 EEFF",
    );
    let offline = place(
        &mut g,
        106,
        NodeKind::Device(DeviceParams::bound_to("11:22:33:44:55:66")),
        [800.0, 560.0],
        "Coyote V3 (2) 5566",
    );
    place(
        &mut g,
        107,
        NodeKind::Frame(FrameParams {
            size: [230.0, 90.0],
            text: "Speed from the UKF shapes intensity on A.".into(),
        }),
        [10.0, -130.0],
        "Notes",
    );
    for (from, out, to, input) in [
        (contact, "level", dev, "a.power"),
        (contact, "level", ukf, "in"),
        (ukf, "speed_n", curve, "in"),
        (curve, "out", dev, "a.int"),
        (lfo, "out", dev, "a.freq"),
        (metro, "gate", dev, "b.power"),
        (contact, "level", offline, "b.power"),
    ] {
        let _ = g.connect(PinRef::new(from, out), PinRef::new(to, input));
    }
    g.view.zoom = 0.7;
    g.view.pan = [20.0, 110.0];
    g
}

pub fn demo_graph_status() -> GraphStatus {
    let g = demo_graph();
    let mut ev = GraphEvaluator::new(&g);
    for k in 0..48 {
        let level = 0.35 + 0.27 * (k as f32 * 0.2).sin();
        for s in ev.external_mut() {
            *s = ExternalSample {
                value: level,
                fresh: true,
            };
        }
        ev.step(0.025);
    }
    let frame = ev
        .frames()
        .iter()
        .find(|f| f.binding.mac() == Some("AA:BB:CC:DD:EE:FF"))
        .map(|f| f.channels);
    let channels = frame
        .map(|c| {
            c.map(|ch| GraphChannelOutput {
                power: ch.power,
                safe: safe_channel(&ch, 50, AlarmOverlay::default(), 1.0),
            })
        })
        .unwrap_or_default();
    GraphStatus {
        mode: EngineMode::Graph,
        tick: ev.tick(),
        probe: Arc::new(ev.probe()),
        issues: ev.issues().to_vec(),
        valid: ev.is_valid(),
        outputs: vec![
            DeviceOutput {
                mac: "AA:BB:CC:DD:EE:FF".into(),
                name: "DG-Lab Coyote V3".into(),
                connected: true,
                battery: Some(75),
                channels,
            },
            DeviceOutput {
                mac: "11:22:33:44:55:66".into(),
                name: "Coyote V3 (2)".into(),
                connected: false,
                battery: Some(38),
                channels: Default::default(),
            },
        ],
        panic: false,
        ui_panic: false,
        caps: [50, 50],
        seen_params: Arc::new(vec![
            ("/avatar/parameters/GestureLeft".into(), 0.0),
            ("/avatar/parameters/MaxPower".into(), 0.4),
            ("/avatar/parameters/OGB/Orf/Pussy/PenOthers".into(), 0.62),
        ]),
        doc_rev: 1,
        notes: Vec::new(),
    }
}

pub fn ringing_snapshot() -> Snapshot {
    let mut s = demo_snapshot();
    s.alarm_status = AlarmStatus {
        phase: AlarmPhase::Ringing,
        strength: 22,
        elapsed_secs: 74,
        auto_stop_in_secs: 226,
        attempt: 1,
        ..AlarmStatus::default()
    };
    s
}

fn preset(id: &str, name: &str, desc: &str, author: &str, user: bool) -> PresetEntry {
    PresetEntry {
        id: id.into(),
        name: name.into(),
        description: desc.into(),
        preset: ChannelPreset {
            name: name.into(),
            description: desc.into(),
            author: author.into(),
            frequency: [180, 50, 160, 70],
            intensity: [75, 85, 80, 70],
            freq_modulation: Default::default(),
            intensity_modulation: Default::default(),
        },
        user,
    }
}

pub fn fill_shared(shared: &Arc<Shared>) {
    if let Ok(mut p) = shared.presets.write() {
        *p = PresetState {
            entries: vec![
                preset("my-warm-up", "Warm Up", "Gentle opener", "", true),
                preset("steady-hum", "Steady Hum", "A calm constant buzz", "nullshii", false),
                preset("thunder-roll", "Thunder Roll", "Rolling low pulses", "nullshii", false),
                preset("velvet-purr", "Velvet Purr", "Soft and slow", "Amigo_FPS", false),
            ],
            loading: false,
            error: None,
            source: "GitHub + mine".into(),
        };
    }
    if let Ok(mut d) = shared.graph_doc.write() {
        *d = GraphDocShared {
            graph: Arc::new(demo_graph()),
            rev: 1,
            replaced: 0,
        };
    }
    if let Ok(mut p) = shared.graph_presets.write() {
        let mut entries = vec![GraphPresetEntry {
            id: "my-evening".into(),
            preset: GraphPreset::from_graph("My evening", "Amigo_FPS", &demo_graph()),
            user: true,
        }];
        entries.extend(shocking_vrc_app::graph_presets::builtin_presets());
        *p = GraphPresetState { entries };
    }
    if let Ok(mut k) = shared.known_devices.write() {
        *k = vec!["AA:BB:CC:DD:EE:FF".into(), "C0:FF:EE:00:12:34".into()];
    }
    if let Ok(mut z) = shared.zone_presets.write() {
        *z = ZonePresetState {
            entries: vec![
                ZonePresetEntry {
                    id: "my-avatar".into(),
                    preset: ZonePreset::from_config("My Avatar", &demo_config()),
                },
                ZonePresetEntry {
                    id: "minimal".into(),
                    preset: ZonePreset::from_config("Minimal", &CliConfig::default()),
                },
            ],
        };
    }
}
