use egui_kittest::Harness;
use shocking_vrc_gui::app::{Ctx, UiState};
use shocking_vrc_gui::bridge::{Bridge, Snapshot};
use shocking_vrc_gui::fixtures;
use shocking_vrc_gui::model::Section;
use shocking_vrc_gui::theme::{self, Palette, ThemeState};
use shocking_vrc_gui::views;

const W: f32 = 1280.0;
const H: f32 = 900.0;

struct Scene {
    bridge: Bridge,
    _rx: tokio::sync::mpsc::UnboundedReceiver<shocking_vrc_gui::bridge::Command>,
    snap: Snapshot,
    log: shocking_vrc_app::logger::LogBuffer,
}

impl Scene {
    fn new(snap: Snapshot) -> Self {
        let (bridge, rx) = fixtures::detached_bridge();
        bridge.set_snapshot(snap.clone());
        fixtures::fill_shared(&bridge.shared);

        let log: shocking_vrc_app::logger::LogBuffer = Default::default();
        if let Ok(mut b) = log.lock() {
            for line in [
                "[INFO ] [cli] Ready.",
                "[INFO ] [ble] Device slot created for AA:BB:CC:DD:EE:FF",
                "[WARN ] [ble] Device 11:22:33:44:55:66 disconnected — coordinator will reconnect",
                "[INFO ] [osc] Port 33777 selected (OSC UDP + OSCQuery HTTP).",
                "[ERROR] [config] Save failed: permission denied",
                "[DEBUG] [cli] Skipped 3 zone events (lagged)",
            ] {
                b.push_back(line.to_string());
            }
        }

        Self {
            bridge,
            _rx: rx,
            snap,
            log,
        }
    }

    fn render_section(&self, section: Section, theme_id: &str) -> image::RgbaImage {
        self.render_section_with(section, theme_id, |_| {})
    }

    fn render_section_with(
        &self,
        section: Section,
        theme_id: &str,
        prep: impl Fn(&mut UiState),
    ) -> image::RgbaImage {
        let theme = ThemeState::new(theme_id);
        let pal: Palette = *theme.palette();
        let mut state = UiState::default_for(section);
        prep(&mut state);

        let mut harness = Harness::builder()
            .with_size(egui::vec2(W, H))
            .wgpu()
            .build_ui(|ui| {
                theme::install_style(ui.ctx(), &pal);
                ui.painter()
                    .rect_filled(ui.max_rect(), theme::widgets::SQUARE, pal.base);
                let mut cx = Ctx {
                    pal: &pal,
                    snap: &self.snap,
                    bridge: &self.bridge,
                    ui_state: &mut state,
                    log: &self.log,
                };
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match section {
                        Section::Status => views::status::show(ui, &mut cx),
                        Section::Zones => views::zones::show(ui, &mut cx),
                        Section::Channels => views::channels::show(ui, &mut cx),
                        Section::Tuning => views::tuning::show(ui, &mut cx),
                        Section::Modulation => views::modulation::show(ui, &mut cx),
                        Section::Log => views::log::show(ui, &mut cx),
                        Section::Presets => views::presets::show(ui, &mut cx),
                        Section::Setup => {
                            views::setup::show(ui, &mut cx);
                        }
                        Section::Alarm => views::alarm::show(ui, &mut cx),
                        Section::Graph => views::graph::show(ui, &mut cx),
                    });
            });

        harness.run();
        harness.render().expect("render")
    }
}

impl Scene {
    fn render_overlay(&self, open: impl Fn(&mut UiState)) -> image::RgbaImage {
        let theme = ThemeState::new("terminal");
        let pal: Palette = *theme.palette();
        let mut state = UiState::default_for(Section::Status);
        open(&mut state);

        let mut harness = Harness::builder()
            .with_size(egui::vec2(W, H))
            .wgpu()
            .build_ui(|ui| {
                theme::install_style(ui.ctx(), &pal);
                ui.painter()
                    .rect_filled(ui.max_rect(), theme::widgets::SQUARE, pal.base);
                let ctx = ui.ctx().clone();
                let mut cx = Ctx {
                    pal: &pal,
                    snap: &self.snap,
                    bridge: &self.bridge,
                    ui_state: &mut state,
                    log: &self.log,
                };
                views::status::show(ui, &mut cx);

                if cx.ui_state.show_zone_presets {
                    shocking_vrc_gui::overlays::zone_presets::show(&ctx, &mut cx);
                }
                if cx.ui_state.show_function_picker {
                    shocking_vrc_gui::overlays::function_picker::show(&ctx, &mut cx);
                }
                if cx.ui_state.preset_save.is_some() {
                    shocking_vrc_gui::overlays::preset_save::show(&ctx, &mut cx);
                }
                shocking_vrc_gui::overlays::confirm::show(&ctx, &mut cx);
                shocking_vrc_gui::overlays::graph_dialogs::show(&ctx, &mut cx);
                shocking_vrc_gui::overlays::update::show(&ctx, &mut cx);
                if cx.snap.alarm_status.phase.is_ringing() {
                    shocking_vrc_gui::overlays::alarm_ring::show(&ctx, &mut cx);
                }
            });

        harness.run();
        harness.render().expect("render")
    }
}

fn out_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::var("SHOCKINGVRC_SHOTS").ok()?;
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn save(img: &image::RgbaImage, name: &str) {
    if let Some(dir) = out_dir() {
        let path = dir.join(format!("{name}.png"));
        if let Err(e) = img.save(&path) {
            eprintln!("could not write {}: {e}", path.display());
        }
    }
}

fn assert_not_blank(img: &image::RgbaImage, what: &str) {
    let first = img.get_pixel(0, 0);
    let distinct = img.pixels().any(|p| p != first);
    assert!(distinct, "{what} rendered as a flat, empty frame");
}

#[test]
fn every_section_renders() {
    let scene = Scene::new(fixtures::demo_snapshot());
    for section in Section::ALL {
        let img = scene.render_section(section, "terminal");
        assert_not_blank(&img, section.title());
        save(&img, &format!("section-{}", section.title().to_lowercase()));
    }
}

#[test]
fn every_section_renders_with_nothing_connected() {
    let mut snap = Snapshot::empty_for_test();
    snap.osc_port = 9001;
    let scene = Scene::new(snap);
    for section in Section::ALL {
        let img = scene.render_section(section, "terminal");
        assert_not_blank(&img, section.title());
    }
    save(
        &scene.render_section(Section::Status, "terminal"),
        "empty-status",
    );
}

#[test]
fn every_palette_renders() {
    let scene = Scene::new(fixtures::demo_snapshot());
    for t in theme::THEMES {
        let img = scene.render_section(Section::Status, t.id);
        assert_not_blank(&img, t.name);
        save(&img, &format!("palette-{}", t.id));
    }
}

#[test]
fn the_graph_section_renders() {
    let scene = Scene::new(fixtures::graph_snapshot());
    for theme_id in ["terminal", theme::THEMES[theme::THEMES.len() - 1].id] {
        let img = scene.render_section(Section::Graph, theme_id);
        assert_not_blank(&img, "graph");
        save(&img, &format!("section-graph-{theme_id}"));
    }
}

fn set_graph(scene: &Scene, graph: shocking_vrc_core::graph::Graph) {
    if let Ok(mut d) = scene.bridge.shared.graph_doc.write() {
        d.graph = std::sync::Arc::new(graph);
        d.rev += 1;
    }
}

#[test]
fn the_graph_stays_sharp_at_every_zoom() {
    let scene = Scene::new(fixtures::graph_snapshot());
    for zoom in [0.5f32, 1.0, 1.8] {
        let mut g = fixtures::demo_graph();
        g.view.zoom = zoom;
        g.view.pan = [20.0, 120.0 * zoom];
        set_graph(&scene, g);
        let img = scene.render_section(Section::Graph, "terminal");
        assert_not_blank(&img, "zoomed graph");
        save(&img, &format!("graph-zoom-{:03}", (zoom * 100.0) as u32));
    }
}

#[test]
fn every_built_in_graph_preset_renders() {
    let scene = Scene::new(fixtures::graph_snapshot());
    let macs = vec!["AA:BB:CC:DD:EE:FF".to_string(), "11:22:33:44:55:66".to_string()];
    for e in shocking_vrc_app::graph_presets::builtin_presets() {
        set_graph(&scene, e.preset.instantiate(&macs));
        let img = scene.render_section_with(Section::Graph, "terminal", |s| s.graph.frame_request = true);
        assert_not_blank(&img, &e.id);
        save(&img, &format!("preset-{}", e.id));
    }
}

#[test]
fn the_device_inspector_renders() {
    use shocking_vrc_core::graph::{NodeId, NodeKind};
    let scene = Scene::new(fixtures::graph_snapshot());
    let mut g = fixtures::demo_graph();
    if let Some(NodeKind::Device(d)) = g.node_mut(NodeId(106)).map(|n| &mut n.kind) {
        for ch in &mut d.channels {
            ch.set_expanded("freq", true);
            ch.set_expanded("int", true);
        }
    }
    set_graph(&scene, g);
    let img = scene.render_section_with(Section::Graph, "terminal", |s| {
        s.graph.sync(&scene.bridge.shared.graph_doc());
        s.graph.selection = [NodeId(106)].into_iter().collect();
    });
    assert_not_blank(&img, "device inspector");
    save(&img, "graph-device-inspector");
}

#[test]
fn the_graph_section_renders_with_nothing_connected() {
    let mut snap = Snapshot::empty_for_test();
    snap.mode = shocking_vrc_core::cli::EngineMode::Graph;
    let scene = Scene::new(snap);
    let img = scene.render_section(Section::Graph, "terminal");
    assert_not_blank(&img, "empty graph");
}

#[test]
fn a_ringing_alarm_renders() {
    let scene = Scene::new(fixtures::ringing_snapshot());
    let img = scene.render_section(Section::Alarm, "terminal");
    assert_not_blank(&img, "ringing alarm");
    save(&img, "alarm-ringing");
}

type OverlayCase = (&'static str, fn(&mut UiState));

#[test]
fn every_overlay_renders() {
    let scene = Scene::new(fixtures::demo_snapshot());
    let cases: [OverlayCase; 5] = [
        ("zone-presets", |s| s.show_zone_presets = true),
        ("function-picker", |s| s.show_function_picker = true),
        ("preset-save", |s| {
            s.preset_save = Some(shocking_vrc_gui::app::PresetSaveDialog {
                channel: shocking_vrc_gui::model::Channel::A,
                name: String::new(),
                author: "Amigo_FPS".into(),
            })
        }),
        ("confirm-preset-delete", |s| s.preset_delete_confirm = Some(0)),
        ("confirm-zoneset-delete", |s| {
            s.zone_preset_delete_confirm = Some(0)
        }),
    ];

    for (name, open) in cases {
        let img = scene.render_overlay(open);
        assert_not_blank(&img, name);
        save(&img, &format!("overlay-{name}"));
    }
}

#[test]
fn every_graph_dialog_renders() {
    use shocking_vrc_gui::overlays::graph_dialogs::GraphDialog;
    let scene = Scene::new(fixtures::graph_snapshot());
    let cases: [OverlayCase; 5] = [
        ("mode-confirm", |s| {
            s.mode_confirm = Some(shocking_vrc_core::cli::EngineMode::Graph)
        }),
        ("graph-preset-save", |s| {
            s.graph_dialog = Some(GraphDialog::save_preset("Amigo_FPS", false))
        }),
        ("graph-preset-apply", |s| {
            s.graph_dialog = Some(GraphDialog::ConfirmApply {
                id: "simple-depth-power".into(),
            })
        }),
        ("graph-preset-delete", |s| {
            s.graph_dialog = Some(GraphDialog::ConfirmDelete {
                id: "my-evening".into(),
                name: "My evening".into(),
            })
        }),
        ("graph-import", |s| {
            s.graph_dialog = Some(GraphDialog::ImportGraph { path: String::new() })
        }),
    ];
    for (name, open) in cases {
        let img = scene.render_overlay(open);
        assert_not_blank(&img, name);
        save(&img, &format!("overlay-{name}"));
    }
}

#[test]
fn the_alarm_ring_renders() {
    let scene = Scene::new(fixtures::ringing_snapshot());
    let img = scene.render_overlay(|_| {});
    assert_not_blank(&img, "alarm ring");
    save(&img, "overlay-alarm-ring");
}
