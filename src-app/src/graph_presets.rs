use std::path::{Path, PathBuf};

use shocking_vrc_core::graph::{GraphPreset, GraphPresetEntry, parse_graph_preset};
use shocking_vrc_core::presets::parse_manifest;

use crate::presets::slugify;

const USER_GRAPH_PRESETS_DIR: &str = "graph-presets/user";
const EXPORT_DIR: &str = "graphs";

const BUILTIN_MANIFEST: &str = include_str!("../../graph-presets/manifest.json");
const BUILTIN_FILES: &[(&str, &str)] = &[
    ("simple-depth-power.json", include_str!("../../graph-presets/simple-depth-power.json")),
    ("depth-lfo-frequency.json", include_str!("../../graph-presets/depth-lfo-frequency.json")),
    ("speed-intensity-smoothing.json", include_str!("../../graph-presets/speed-intensity-smoothing.json")),
    ("two-zone-blend.json", include_str!("../../graph-presets/two-zone-blend.json")),
    ("two-devices-one-contact.json", include_str!("../../graph-presets/two-devices-one-contact.json")),
    ("guarded-soft-start.json", include_str!("../../graph-presets/guarded-soft-start.json")),
    ("heartbeat-touch.json", include_str!("../../graph-presets/heartbeat-touch.json")),
    ("four-segment-texture.json", include_str!("../../graph-presets/four-segment-texture.json")),
    ("tease-timer.json", include_str!("../../graph-presets/tease-timer.json")),
    ("echo-tail.json", include_str!("../../graph-presets/echo-tail.json")),
    ("breathing-noise.json", include_str!("../../graph-presets/breathing-noise.json")),
    ("ping-pong-ab.json", include_str!("../../graph-presets/ping-pong-ab.json")),
    ("classic-functions.json", include_str!("../../graph-presets/classic-functions.json")),
    ("speed-to-frequency.json", include_str!("../../graph-presets/speed-to-frequency.json")),
    ("touch-envelope.json", include_str!("../../graph-presets/touch-envelope.json")),
    ("recoil-spike.json", include_str!("../../graph-presets/recoil-spike.json")),
    ("avatar-slider-max.json", include_str!("../../graph-presets/avatar-slider-max.json")),
    ("hotkey-modes.json", include_str!("../../graph-presets/hotkey-modes.json")),
    ("long-four-voice-drift.json", include_str!("../../graph-presets/long-four-voice-drift.json")),
    ("long-slow-escalation.json", include_str!("../../graph-presets/long-slow-escalation.json")),
    ("long-breathing-cathedral.json", include_str!("../../graph-presets/long-breathing-cathedral.json")),
    ("long-drifting-polyrhythm.json", include_str!("../../graph-presets/long-drifting-polyrhythm.json")),
    ("long-depth-ceiling.json", include_str!("../../graph-presets/long-depth-ceiling.json")),
    ("long-glitch-rain.json", include_str!("../../graph-presets/long-glitch-rain.json")),
    ("high-rolling-buzz.json", include_str!("../../graph-presets/high-rolling-buzz.json")),
    ("high-hummingbird.json", include_str!("../../graph-presets/high-hummingbird.json")),
    ("nightmare-no-rhythm.json", include_str!("../../graph-presets/nightmare-no-rhythm.json")),
    ("nightmare-ratchet.json", include_str!("../../graph-presets/nightmare-ratchet.json")),
    ("low-thump.json", include_str!("../../graph-presets/low-thump.json")),
    ("low-deep-rumble.json", include_str!("../../graph-presets/low-deep-rumble.json")),
    ("mid-roll.json", include_str!("../../graph-presets/mid-roll.json")),
    ("low-mid-wander.json", include_str!("../../graph-presets/low-mid-wander.json")),
    ("soft-feather.json", include_str!("../../graph-presets/soft-feather.json")),
    ("soft-warm-tide.json", include_str!("../../graph-presets/soft-warm-tide.json")),
    ("drop-freefall.json", include_str!("../../graph-presets/drop-freefall.json")),
    ("drop-undertow.json", include_str!("../../graph-presets/drop-undertow.json")),
    ("drop-fault-line.json", include_str!("../../graph-presets/drop-fault-line.json")),
    ("fragment-ukf-speed.json", include_str!("../../graph-presets/fragment-ukf-speed.json")),
    ("fragment-stale-safe-osc.json", include_str!("../../graph-presets/fragment-stale-safe-osc.json")),
    ("fragment-soft-start.json", include_str!("../../graph-presets/fragment-soft-start.json")),
];

pub fn user_graph_preset_dir() -> PathBuf {
    PathBuf::from(USER_GRAPH_PRESETS_DIR)
}

pub fn load_graph_presets() -> Vec<GraphPresetEntry> {
    let mut entries = builtin_presets();
    entries.extend(load_user_from(&user_graph_preset_dir()));
    sort_graph_preset_entries(&mut entries);
    entries
}

pub fn builtin_presets() -> Vec<GraphPresetEntry> {
    let manifest = match parse_manifest(BUILTIN_MANIFEST) {
        Ok(m) => m,
        Err(e) => {
            log::error!("[graph-presets] Built-in manifest: {e}");
            return Vec::new();
        }
    };
    manifest
        .presets
        .iter()
        .filter_map(|r| {
            let (_, text) = BUILTIN_FILES.iter().find(|(f, _)| *f == r.file)?;
            match parse_graph_preset(text) {
                Ok(mut preset) => {
                    if preset.name.is_empty() {
                        preset.name = r.name.clone();
                    }
                    if preset.description.is_empty() {
                        preset.description = r.description.clone();
                    }
                    Some(GraphPresetEntry {
                        id: r.id.clone(),
                        preset,
                        user: false,
                    })
                }
                Err(e) => {
                    log::error!("[graph-presets] Built-in {}: {e}", r.file);
                    None
                }
            }
        })
        .collect()
}

pub fn sort_graph_preset_entries(entries: &mut [GraphPresetEntry]) {
    entries.sort_by(|a, b| {
        b.user
            .cmp(&a.user)
            .then_with(|| a.name().to_lowercase().cmp(&b.name().to_lowercase()))
    });
}

pub fn save_user_graph_preset(preset: &GraphPreset) -> Result<GraphPresetEntry, String> {
    save_user_in(&user_graph_preset_dir(), preset)
}

pub fn delete_user_graph_preset(id: &str) -> Result<(), String> {
    delete_user_in(&user_graph_preset_dir(), id)
}

pub fn rename_user_graph_preset(id: &str, new_name: &str) -> Result<GraphPresetEntry, String> {
    rename_user_in(&user_graph_preset_dir(), id, new_name)
}

pub fn import_graph_preset(path: &Path) -> Result<GraphPresetEntry, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut preset = parse_graph_preset(&text)?;
    if preset.name.trim().is_empty() {
        preset.name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported")
            .to_string();
    }
    save_user_graph_preset(&preset)
}

pub fn export_graph_preset(entry: &GraphPresetEntry) -> Result<PathBuf, String> {
    let dir = PathBuf::from(EXPORT_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{}.graph-preset.json", entry.id));
    let json = serde_json::to_string_pretty(&entry.preset).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub fn random_graph_preset_name() -> String {
    use rand::RngExt;
    let n: u32 = rand::rng().random_range(10_000..99_999);
    format!("graph-{n}")
}

fn load_user_from(dir: &Path) -> Vec<GraphPresetEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for item in read.flatten() {
        let path = item.path();
        if path.is_dir() || path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match parse_graph_preset(&text) {
            Ok(preset) => entries.push(GraphPresetEntry {
                id,
                preset,
                user: true,
            }),
            Err(e) => log::warn!("[graph-presets] Skipping {}: {e}", path.display()),
        }
    }
    entries
}

fn save_user_in(dir: &Path, preset: &GraphPreset) -> Result<GraphPresetEntry, String> {
    let name = preset.name.trim();
    if name.is_empty() {
        return Err("Preset name cannot be empty".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let id = slugify(name);
    let json = serde_json::to_string_pretty(preset).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{id}.json"));
    std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(GraphPresetEntry {
        id,
        preset: preset.clone(),
        user: true,
    })
}

fn delete_user_in(dir: &Path, id: &str) -> Result<(), String> {
    let path = dir.join(format!("{id}.json"));
    if !path.exists() {
        return Err(format!("preset file '{}' not found", path.display()));
    }
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))
}

fn rename_user_in(dir: &Path, id: &str, new_name: &str) -> Result<GraphPresetEntry, String> {
    let path = dir.join(format!("{id}.json"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut preset = parse_graph_preset(&text)?;
    preset.name = new_name.trim().to_string();
    let entry = save_user_in(dir, &preset)?;
    if entry.id != id {
        let _ = std::fs::remove_file(&path);
    }
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shocking_vrc_core::graph::{Graph, GraphPresetKind, NodeKind, Severity, kind::NoParams};

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shockingvrc-graph-presets-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn sample(name: &str) -> GraphPreset {
        let mut g = Graph::default();
        g.add_kind(NodeKind::Time(NoParams {}), [0.0, 0.0]);
        GraphPreset::from_graph(name, "me", &g)
    }

    #[test]
    fn every_built_in_preset_parses_and_is_listed() {
        let builtins = builtin_presets();
        assert_eq!(builtins.len(), BUILTIN_FILES.len(), "manifest and embedded files disagree");
        for e in &builtins {
            assert!(!e.user);
            assert!(e.preset.node_count() > 0, "{} is empty", e.id);
            assert!(e.preset.graph.missing_types().is_empty(), "{} uses unknown nodes", e.id);
            let mut g = e.preset.instantiate(&["AA:AA".into(), "BB:BB".into()]);
            assert_eq!(g.prune_dangling_edges(), 0, "{} has wires to pins that do not exist", e.id);
            let ev = shocking_vrc_core::graph::GraphEvaluator::new(&g);
            assert!(ev.is_valid(), "{} does not compile", e.id);
            let bad: Vec<_> = ev
                .issues()
                .iter()
                .filter(|i| match e.preset.kind {
                    GraphPresetKind::Graph => i.severity != Severity::Info,
                    GraphPresetKind::Fragment => i.severity == Severity::Error,
                })
                .collect();
            assert!(bad.is_empty(), "{}: {bad:?}", e.id);
        }
    }

    #[test]
    fn every_built_in_graph_drives_a_device_when_touched() {
        use shocking_vrc_core::graph::ExternalSample;
        for e in builtin_presets().iter().filter(|e| e.preset.kind == GraphPresetKind::Graph) {
            let g = e.preset.instantiate(&["AA:AA".into(), "BB:BB".into()]);
            let mut ev = shocking_vrc_core::graph::GraphEvaluator::new(&g);
            let mut peak = 0.0f32;
            for k in 0..80 {
                let level = 0.3 + 0.4 * ((k as f32) * 0.3).sin().abs();
                ev.fill_external(|_| ExternalSample { value: level, fresh: true });
                for f in ev.step(0.025) {
                    for ch in &f.channels {
                        peak = peak.max(ch.power);
                    }
                }
            }
            assert!(peak > 0.05, "{} never asks for power (peak {peak})", e.id);
        }
    }

    #[test]
    fn long_form_presets_use_every_segment_and_unusual_limits() {
        use shocking_vrc_core::graph::{ExternalSample, NodeKind};
        let long: Vec<_> = builtin_presets().into_iter().filter(|e| e.id.starts_with("long-")).collect();
        assert_eq!(long.len(), 6);
        for e in &long {
            let g = e.preset.instantiate(&["AA:AA".into()]);
            let dev = g.nodes.iter().find(|n| n.kind.is_device()).expect("a device");
            if let NodeKind::Device(p) = &dev.kind {
                for ch in &p.channels {
                    assert!(ch.is_expanded("freq") && ch.is_expanded("int"), "{}: not ×4", e.id);
                }
            }
            let mut ev = shocking_vrc_core::graph::GraphEvaluator::new(&g);
            let (mut spread_f, mut spread_i) = ([0.0f32; 2], [0.0f32; 2]);
            let (mut lo_max, mut hi_max, mut hi_min) = (f32::MAX, 0.0f32, 0.0f32);
            for k in 0..2400 {
                let level = 0.35 + 0.3 * ((k as f32) * 0.01).sin();
                ev.fill_external(|_| ExternalSample { value: level, fresh: true });
                let f = &ev.step(0.025)[0];
                for (c, ch) in f.channels.iter().enumerate() {
                    let range = |v: [f32; 4]| v.iter().fold(f32::MIN, |a, b| a.max(*b)) - v.iter().fold(f32::MAX, |a, b| a.min(*b));
                    spread_f[c] = spread_f[c].max(range(ch.freq));
                    spread_i[c] = spread_i[c].max(range(ch.intensity));
                    lo_max = lo_max.min(ch.limit_max);
                    hi_max = hi_max.max(ch.limit_max);
                    hi_min = hi_min.max(ch.limit_min);
                }
            }
            for c in 0..2 {
                assert!(spread_f[c] > 5.0, "{} ch {c}: frequency segments never differ", e.id);
                assert!(spread_i[c] > 5.0, "{} ch {c}: intensity segments never differ", e.id);
            }
            let moving = hi_max - lo_max > 2.0;
            assert!(moving || hi_min > 0.0 || hi_max != 30.0, "{}: plain 0–30 limits", e.id);
        }
    }

    #[test]
    fn slow_escalation_climbs_for_ten_minutes() {
        use shocking_vrc_core::graph::ExternalSample;
        let e = builtin_presets().into_iter().find(|e| e.id == "long-slow-escalation").unwrap();
        let mut ev = shocking_vrc_core::graph::GraphEvaluator::new(&e.preset.instantiate(&["AA:AA".into()]));
        let at = |secs: f32, ev: &mut shocking_vrc_core::graph::GraphEvaluator| {
            for _ in 0..(secs / 0.025) as usize {
                ev.fill_external(|_| ExternalSample { value: 0.6, fresh: true });
                ev.step(0.025);
            }
            ev.frames()[0].channels[0]
        };
        let start = at(5.0, &mut ev);
        let mid = at(300.0, &mut ev);
        let end = at(320.0, &mut ev);
        assert_eq!((start.limit_min, start.limit_max), (0.0, 14.0), "still in the 10 s hold");
        assert!(mid.limit_max > 20.0 && mid.limit_max < 32.0, "half-way: {}", mid.limit_max);
        assert!((end.limit_max - 38.0).abs() < 0.01 && (end.limit_min - 12.0).abs() < 0.01);
    }

    #[test]
    fn built_in_ids_are_unique_and_fragments_carry_no_devices() {
        let builtins = builtin_presets();
        let mut ids: Vec<&str> = builtins.iter().map(|e| e.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), builtins.len());
        for e in builtins.iter().filter(|e| e.preset.kind == GraphPresetKind::Fragment) {
            assert_eq!(e.preset.device_slots(), 0, "{} would bind devices", e.id);
        }
    }

    #[test]
    fn save_rename_delete_round_trip() {
        let dir = temp("round-trip");
        let saved = save_user_in(&dir, &sample("My Graph")).expect("save");
        assert_eq!(saved.id, "my-graph");
        let renamed = rename_user_in(&dir, "my-graph", "Better Name").expect("rename");
        assert_eq!(renamed.id, "better-name");
        assert!(!dir.join("my-graph.json").exists());
        let listed = load_user_from(&dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name(), "Better Name");
        delete_user_in(&dir, "better-name").expect("delete");
        assert!(load_user_from(&dir).is_empty());
        assert!(delete_user_in(&dir, "better-name").is_err());
    }

    #[test]
    fn mine_sort_first_and_junk_is_skipped() {
        let dir = temp("sorting");
        save_user_in(&dir, &sample("zeta")).unwrap();
        std::fs::write(dir.join("broken.json"), "{").unwrap();
        let mut entries = builtin_presets();
        entries.extend(load_user_from(&dir));
        sort_graph_preset_entries(&mut entries);
        assert!(entries[0].user, "user presets first, like Classic");
        assert!(save_user_in(&dir, &sample("   ")).is_err());
        assert!(random_graph_preset_name().starts_with("graph-"));
    }

    struct Strokes {
        phase: f32,
        rate: f32,
        rng: u64,
        t: f32,
        next_pull: f32,
        mid: f32,
        steady: bool,
    }

    impl Strokes {
        fn new(seed: u64, mid: f32) -> Self {
            Self { phase: 0.0, rate: 1.0, rng: seed.max(1), t: 0.0, next_pull: 8.0, mid, steady: false }
        }

        fn rand(&mut self) -> f32 {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 7;
            self.rng ^= self.rng << 17;
            (self.rng >> 40) as f32 / (1u64 << 24) as f32
        }

        fn next(&mut self, dt: f32) -> f32 {
            self.t += dt;
            if self.steady {
                self.phase += std::f32::consts::TAU * dt;
                return (self.mid + 0.3 * self.phase.sin()).clamp(0.0, 1.0);
            }
            self.rate = (self.rate + (self.rand() - 0.5) * 0.08).clamp(0.4, 2.2);
            self.phase += std::f32::consts::TAU * self.rate * dt;
            let mut level = self.mid + 0.3 * self.phase.sin();
            if self.t >= self.next_pull {
                if self.t < self.next_pull + 0.15 {
                    level = 0.06;
                } else {
                    self.next_pull = self.t + 7.0 + 5.0 * self.rand();
                }
            }
            level.clamp(0.0, 1.0)
        }
    }

    fn preset(id: &str) -> GraphPresetEntry {
        builtin_presets().into_iter().find(|e| e.id == id).unwrap_or_else(|| panic!("no preset {id}"))
    }

    fn play(g: &Graph, secs: f32, strokes: &mut Strokes) -> Vec<[shocking_vrc_core::graph::device::ChannelFrame; 2]> {
        use shocking_vrc_core::graph::ExternalSample;
        let mut ev = shocking_vrc_core::graph::GraphEvaluator::new(g);
        (0..(secs / 0.025) as usize)
            .map(|_| {
                let level = strokes.next(0.025);
                ev.fill_external(|_| ExternalSample { value: level, fresh: true });
                let f = &ev.step(0.025)[0];
                [f.channels[0], f.channels[1]]
            })
            .collect()
    }

    fn hz(raw: f32) -> f32 {
        100.0 + (1.0 - 100.0) / 245.0 * (raw - 10.0)
    }

    const DROPS: [&str; 3] = ["drop-freefall", "drop-undertow", "drop-fault-line"];

    fn set_slider(g: &mut Graph, title: &str, value: f32) {
        let n = g.nodes.iter_mut().find(|n| n.title == title).unwrap_or_else(|| panic!("no slider {title}"));
        let NodeKind::Slider(p) = &mut n.kind else { panic!("{title} is not a slider") };
        p.value = value;
    }

    fn hz_int(frames: &[[shocking_vrc_core::graph::device::ChannelFrame; 2]]) -> Vec<(f32, f32)> {
        frames.iter().skip(200).map(|f| (hz(f[0].freq[3]), f[0].intensity[3])).collect()
    }

    fn fall_starts(a: &[(f32, f32)]) -> Vec<usize> {
        (2..a.len().saturating_sub(10))
            .filter(|&t| a[t - 1].0 - a[t].0 >= 8.0 && a[t - 2].0 - a[t - 1].0 < 8.0)
            .collect()
    }

    #[test]
    fn drop_presets_have_both_sliders_and_start_at_power_6() {
        for id in DROPS {
            let g = preset(id).preset.instantiate(&["AA:AA".into()]);
            let sliders: Vec<(&str, f32)> = g
                .nodes
                .iter()
                .filter_map(|n| match &n.kind {
                    NodeKind::Slider(p) => Some((n.title.as_str(), p.value)),
                    _ => None,
                })
                .collect();
            assert!(sliders.iter().any(|(t, _)| *t == "Max frequency drop"), "{id}: {sliders:?}");
            assert!(sliders.contains(&("Max device power", 6.0)), "{id}: {sliders:?}");
            for f in play(&g, 5.0, &mut Strokes::new(3, 0.5)) {
                assert!(f.iter().all(|ch| ch.limit_max == 6.0), "{id}: limit max {}", f[0].limit_max);
            }
        }
    }

    #[test]
    fn drop_presets_fall_abruptly_softer_and_within_the_slider() {
        for id in DROPS {
            let mut deepest = Vec::new();
            for max_drop in [60.0f32, 25.0] {
                let mut g = preset(id).preset.instantiate(&["AA:AA".into()]);
                set_slider(&mut g, "Max frequency drop", max_drop);
                let a = hz_int(&play(&g, 180.0, &mut Strokes::new(11, 0.5)));
                let starts = fall_starts(&a);
                assert!(starts.len() >= 40, "{id}: only {} falls in 3 min", starts.len());
                let (mut abrupt, mut softer, mut deepest_fall) = (0, 0, 0.0f32);
                for (k, &t) in starts.iter().enumerate() {
                    let end = starts.get(k + 1).map_or(t + 8, |&next| next.min(t + 8));
                    let before = a[t - 1];
                    let bottom = a[t..end].iter().map(|x| x.0).fold(f32::MAX, f32::min);
                    let depth = before.0 - bottom;
                    deepest_fall = deepest_fall.max(depth);
                    if before.0 - a[t].0.min(a[t + 1].0) >= 0.8 * depth {
                        abrupt += 1;
                    }
                    if a[t + 6].1 < before.1 {
                        softer += 1;
                    }
                }
                let n = starts.len() as f32;
                assert!(abrupt as f32 >= 0.9 * n, "{id}: {abrupt} of {n} falls are abrupt");
                assert!(softer as f32 >= 0.75 * n, "{id}: intensity sank in only {softer} of {n} falls");
                assert!(deepest_fall <= max_drop + 6.0, "{id}: fell {deepest_fall} Hz with a {max_drop} Hz cap");
                if max_drop == 60.0 {
                    let high = a.iter().filter(|x| x.0 >= 65.0).count() as f32 / a.len() as f32;
                    assert!(high >= 0.6, "{id}: high only {:.0} % of the time", high * 100.0);
                }
                deepest.push(deepest_fall);
            }
            assert!(deepest[0] > deepest[1] + 10.0, "{id}: the slider does not change the drop ({deepest:?})");
        }
    }

    #[test]
    fn drop_patterns_never_repeat() {
        for id in DROPS {
            let g = preset(id).preset.instantiate(&["AA:AA".into()]);
            let events = |mid: f32| {
                let mut strokes = Strokes::new(5, mid);
                strokes.steady = true;
                let a = hz_int(&play(&g, 600.0, &mut strokes));
                let starts = fall_starts(&a);
                starts
                    .windows(2)
                    .map(|w| {
                        let bottom = a[w[1]..w[1] + 8].iter().map(|x| x.0).fold(f32::MAX, f32::min);
                        (w[1] - w[0], ((a[w[1] - 2].0 - bottom) / 2.0).round() as i32)
                    })
                    .collect::<Vec<_>>()
            };
            let ev = events(0.5);
            assert!(ev.len() >= 200, "{id}: only {} drops in 10 minutes", ev.len());
            let mut seen = std::collections::HashSet::new();
            for w in ev.windows(5) {
                assert!(seen.insert(w.to_vec()), "{id}: the drop pattern {w:?} came back");
            }
            let other = events(0.51);
            assert_ne!(ev[..20], other[..20], "{id}: a different stroke played the same drops");
        }
    }

    fn downstream(g: &Graph, node: shocking_vrc_core::graph::NodeId, pin: &str) -> Vec<(NodeKind, String)> {
        use shocking_vrc_core::graph::NodeId;
        let mut todo: Vec<(Vec<NodeId>, NodeId, String)> = vec![(Vec::new(), node, pin.to_string())];
        let mut seen = std::collections::HashSet::new();
        let mut reached = Vec::new();
        while let Some((path, from, out)) = todo.pop() {
            if !seen.insert((path.clone(), from, out.clone())) {
                continue;
            }
            let scope = g.scope(&path).expect("scope");
            for e in scope.edges.iter().filter(|e| e.from.node == from && e.from.pin == out) {
                let to = scope.node(e.to.node).expect("node");
                reached.push((to.kind.clone(), e.to.pin.clone()));
                match &to.kind {
                    NodeKind::Group(p) => {
                        if let Some(gi) = p.graph.nodes.iter().find(|n| matches!(n.kind, NodeKind::GroupInput(_))) {
                            let mut inner = path.clone();
                            inner.push(to.id);
                            todo.push((inner, gi.id, e.to.pin.clone()));
                        }
                    }
                    NodeKind::GroupOutput(_) => {
                        let mut outer = path.clone();
                        if let Some(grp) = outer.pop() {
                            todo.push((outer, grp, e.to.pin.clone()));
                        }
                    }
                    kind => {
                        for o in kind.outputs() {
                            todo.push((path.clone(), to.id, o.key.to_string()));
                        }
                    }
                }
            }
        }
        reached
    }

    #[test]
    fn drop_presets_use_all_four_motion_values_everywhere() {
        for id in DROPS {
            let g = preset(id).preset.instantiate(&["AA:AA".into()]);
            let ukf = g.nodes.iter().find(|n| matches!(n.kind, NodeKind::Ukf(_))).expect("a UKF");
            for pin in ["level", "speed_n", "acc_n", "recoil_n"] {
                let reached = downstream(&g, ukf.id, pin);
                let device = |prefix: &str| {
                    reached.iter().any(|(k, p)| matches!(k, NodeKind::Device(_)) && p.starts_with(prefix))
                };
                assert!(device("a.freq"), "{id}: {pin} never reaches the frequency");
                assert!(device("a.int"), "{id}: {pin} never reaches the intensity");
                let timing = reached
                    .iter()
                    .any(|(k, _)| matches!(k, NodeKind::Metronome(_) | NodeKind::Schmitt(_) | NodeKind::Compare(_)));
                assert!(timing, "{id}: {pin} does not move the timing");
            }
        }
    }

    #[test]
    fn range_presets_stay_in_their_bands() {
        for (id, lo, hi) in [
            ("high-rolling-buzz", 85.0, 100.0),
            ("high-hummingbird", 95.0, 100.0),
            ("low-thump", 3.0, 12.0),
            ("low-deep-rumble", 1.0, 6.0),
            ("mid-roll", 25.0, 50.0),
            ("low-mid-wander", 6.0, 40.0),
            ("soft-feather", 55.0, 75.0),
            ("soft-warm-tide", 30.0, 45.0),
        ] {
            let g = preset(id).preset.instantiate(&["AA:AA".into()]);
            let frames = play(&g, 90.0, &mut Strokes::new(13, 0.5));
            for f in frames.iter().skip(200) {
                for ch in f {
                    for raw in ch.freq {
                        let h = hz(raw);
                        assert!(h >= lo - 0.6 && h <= hi + 0.6, "{id}: {h:.1} Hz outside {lo}–{hi}");
                    }
                }
            }
            let ints: Vec<f32> = frames.iter().skip(200).map(|f| f[0].intensity[3]).collect();
            let spread = ints.iter().fold(f32::MIN, |a, b| a.max(*b)) - ints.iter().fold(f32::MAX, |a, b| a.min(*b));
            if id.starts_with("high-") {
                assert!(spread >= 30.0, "{id}: intensity barely varies ({spread:.0} %)");
            }
            if id.starts_with("soft-") {
                assert!(frames.iter().all(|f| f[0].limit_max <= 14.0), "{id}: not gentle");
                assert!(ints.iter().all(|i| *i <= 65.0), "{id}: intensity above 65 %");
                let jump = frames.windows(2).map(|w| (w[1][0].power - w[0][0].power).abs()).fold(0.0f32, f32::max);
                assert!(jump <= 0.02, "{id}: power jumps by {jump:.3} in one tick");
            }
        }
    }

    #[test]
    fn nightmare_presets_roam_and_push_the_floor() {
        for id in ["nightmare-no-rhythm", "nightmare-ratchet"] {
            let g = preset(id).preset.instantiate(&["AA:AA".into()]);
            let frames = play(&g, 120.0, &mut Strokes::new(17, 0.5));
            let hs: Vec<f32> = frames.iter().map(|f| hz(f[0].freq[3])).collect();
            let span = hs.iter().fold(f32::MIN, |a, b| a.max(*b)) - hs.iter().fold(f32::MAX, |a, b| a.min(*b));
            assert!(span >= 60.0, "{id}: pitch spans only {span:.0} Hz");
            let floor = frames.iter().map(|f| f[0].limit_min).fold(0.0f32, f32::max);
            assert!(floor >= 12.0, "{id}: the floor never rises (max {floor})");
        }
    }
}
