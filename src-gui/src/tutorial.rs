use egui::Context;

use shocking_vrc_core::cli::EngineMode;

use crate::app::UiState;
use crate::model::Section;
use crate::theme::Palette;
use crate::theme::widgets::{self as w, font, space};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Welcome,
    StatusOverview,
    GraphIntro,
    ZonesIntro,
    ZonesAddZone,
    ZonesDgbTip,
    ChannelsFreq,
    ChannelsIntensity,
    ChannelsLimits,
    ChannelsAggregation,
    ModulationIntro,
    ModulationApply,
    TuningIntro,
    PresetsIntro,
    PresetsApply,
    SetupOsc,
    SetupAutoSave,
    Done,
}

impl Step {
    pub const ALL: [Step; 18] = [
        Step::Welcome,
        Step::StatusOverview,
        Step::GraphIntro,
        Step::ZonesIntro,
        Step::ZonesAddZone,
        Step::ZonesDgbTip,
        Step::ChannelsFreq,
        Step::ChannelsIntensity,
        Step::ChannelsLimits,
        Step::ChannelsAggregation,
        Step::ModulationIntro,
        Step::ModulationApply,
        Step::TuningIntro,
        Step::PresetsIntro,
        Step::PresetsApply,
        Step::SetupOsc,
        Step::SetupAutoSave,
        Step::Done,
    ];

    #[cfg(test)]
    fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    fn applies(self, mode: EngineMode) -> bool {
        self.section().is_none_or(|s| s.in_mode(mode))
    }

    fn for_mode(mode: EngineMode) -> Vec<Step> {
        Self::ALL.into_iter().filter(|s| s.applies(mode)).collect()
    }

    fn section(self) -> Option<Section> {
        match self {
            Step::Welcome | Step::Done => None,
            Step::StatusOverview => Some(Section::Status),
            Step::GraphIntro => Some(Section::Graph),
            Step::ZonesIntro | Step::ZonesAddZone | Step::ZonesDgbTip => Some(Section::Zones),
            Step::ChannelsFreq
            | Step::ChannelsIntensity
            | Step::ChannelsLimits
            | Step::ChannelsAggregation => Some(Section::Channels),
            Step::ModulationIntro | Step::ModulationApply => Some(Section::Modulation),
            Step::TuningIntro => Some(Section::Tuning),
            Step::PresetsIntro | Step::PresetsApply => Some(Section::Presets),
            Step::SetupOsc | Step::SetupAutoSave => Some(Section::Setup),
        }
    }

    fn title(self) -> &'static str {
        match self {
            Step::Welcome => "Welcome to ShockingVRC",
            Step::StatusOverview => "Status",
            Step::GraphIntro => "Graph mode",
            Step::ZonesIntro => "Zones — overview",
            Step::ZonesAddZone => "Zones — adding a zone",
            Step::ZonesDgbTip => "DGB contacts — Unity setup",
            Step::ChannelsFreq => "Channels — frequency",
            Step::ChannelsIntensity => "Channels — intensity",
            Step::ChannelsLimits => "Channels — power limits",
            Step::ChannelsAggregation => "Channels — zone mixing",
            Step::ModulationIntro => "Modulation — overview",
            Step::ModulationApply => "Modulation — applying",
            Step::TuningIntro => "Tuning — filter & normalisation",
            Step::PresetsIntro => "Presets — the catalogue",
            Step::PresetsApply => "Presets — apply & save",
            Step::SetupOsc => "Setup — OSC & OSCQuery",
            Step::SetupAutoSave => "Setup — auto-save",
            Step::Done => "All done",
        }
    }

    fn body(self) -> &'static [&'static str] {
        match self {
            Step::Welcome => &[
                "This tour walks through every section of the app.",
                "",
                "It steers the real interface as it goes — whatever is being",
                "described is on screen behind this panel, and you can click",
                "anything at any time.",
                "",
                "Next to begin, Esc to leave.",
            ],
            Step::StatusOverview => &[
                "The Status section shows, in real time:",
                "",
                "  · Coyote connection and battery",
                "  · Whether VRChat has found the OSC link",
                "  · Live power output for channels A and B",
                "  · Which zones are active and how hard",
                "",
                "With more than one Coyote, a device list appears — click a row",
                "to switch. Each device keeps its own configuration.",
            ],
            Step::GraphIntro => &[
                "In Graph mode you wire the signal yourself, like a shader editor.",
                "",
                "  · Right-click or Shift+A adds a node; drag from an output",
                "    square to an input to connect it",
                "  · Every Coyote gets its own Device node with Power,",
                "    Frequency and Intensity pins for channels A and B",
                "  · Minimal graph: Contact → Device · Channel A · Power",
                "",
                "Whatever the graph asks for, the Graph cap in Setup is the",
                "ceiling. PANIC (or F12) holds every device at 0.",
            ],
            Step::ZonesIntro => &[
                "Zones are avatar contact areas that drive the device.",
                "",
                "The two left columns are the zones mapped to channels A and B.",
                "The right column lists everything your avatar is reporting.",
                "",
                "Each mapped zone has a mode — depth, speed, acc or recoil —",
                "which decides what about the contact drives the output.",
            ],
            Step::ZonesAddZone => &[
                "To map a zone, find it in the avatar list and click the small",
                "→A or →B button on its row. Add all → A maps every zone at once.",
                "",
                "Click a mapped zone to select it; the Calibrate panel below then",
                "lets you set its mode, trim its contribution with Scale, and",
                "narrow the depth window with the two threshold sliders.",
                "",
                "Right-click a mapped zone to remove it.",
            ],
            Step::ZonesDgbTip => &[
                "To use custom DGB contacts you need to set up a VRC Contact",
                "Receiver in Unity:",
                "",
                "  1.  Add a VRC Contact Receiver component to your avatar",
                "  2.  Set Receiver Type to Proximity",
                "  3.  Set Parameter to DGB/YourContactName",
                "      (for example DGB/Chest, DGB/TouchAreaA)",
                "  4.  Upload the avatar",
                "",
                "The app discovers DGB zones automatically over OSC.",
            ],
            Step::ChannelsFreq => &[
                "Each channel plays a four-segment waveform.",
                "",
                "Frequency sets the pulse rate of each segment, shown in Hz.",
                "Higher is faster and sharper; lower is slower and deeper.",
                "",
                "Drag a slider to change it. The small plot at the top of the",
                "panel shows the resulting shape.",
            ],
            Step::ChannelsIntensity => &[
                "Intensity is how strong each segment feels, from 0 to 100%.",
                "",
                "Together, the four frequency and four intensity values make a",
                "repeating pattern — that pattern is what the preset catalogue",
                "stores and restores.",
            ],
            Step::ChannelsLimits => &[
                "Power limits are your safety net.",
                "",
                "  Min  — the floor, usually 0",
                "  Max  — the hardest the device is ever allowed to go",
                "",
                "The ceiling is pushed to the hardware itself, so nothing can",
                "exceed it. Presets and zone sets never change it.",
                "",
                "Start at 20–30 and raise it slowly.",
            ],
            Step::ChannelsAggregation => &[
                "When several zones are active at once, Zone mix decides how",
                "their levels combine:",
                "",
                "  Max      — the strongest zone wins",
                "  Sum      — they add up, capped at 100%",
                "  Average  — the mean of the active zones",
            ],
            Step::ModulationIntro => &[
                "Modulation gives frequency and intensity movement over time,",
                "so the sensation does not sit still.",
                "",
                "There are sixteen slots: two channels × frequency and intensity",
                "× four segments. Pick one on the left to edit it on the right.",
                "",
                "Choose a function for the shape, a source for what drives it,",
                "then tune speed, phase and deviation.",
            ],
            Step::ModulationApply => &[
                "The graph above the editor is a live preview, computed with the",
                "same evaluator the engine runs — what you see is what the device",
                "will do.",
                "",
                "  Apply       — send the slot to the device",
                "  Revert      — discard edits, reload the stored slot",
                "  Clear slot  — turn that one slot off",
                "",
                "Edits are not live until you press Apply.",
            ],
            Step::TuningIntro => &[
                "Tuning holds the signal-processing controls.",
                "",
                "The Kalman filter smooths noisy contact data. Lower Q is",
                "calmer but slower to react.",
                "",
                "Motion normalisation sets the divisors for speed, acceleration",
                "and recoil modes — raise one to make that mode less sensitive.",
                "",
                "The live readout underneath shows what the engine is deriving.",
                "If a bar pins at 100%, its divisor is too low.",
            ],
            Step::PresetsIntro => &[
                "Presets carry frequency, intensity and modulation settings.",
                "",
                "The official catalogue is fetched from GitHub; your own presets",
                "live in presets/user/ and are tagged 'mine'.",
                "",
                "Search and the All/Mine filter narrow the list; the panel on the",
                "right shows what the selected preset will actually write.",
            ],
            Step::PresetsApply => &[
                "Select a preset, then:",
                "",
                "  Apply → A / Apply → B   send it to that channel",
                "  Save A / Save B         store the current channel as your own",
                "",
                "A preset never touches your power limits or your zones — only",
                "the waveform and its modulation.",
            ],
            Step::SetupOsc => &[
                "OSC and OSCQuery configure themselves.",
                "",
                "A free port is chosen at startup, used for both OSC over UDP and",
                "OSCQuery over HTTP, then advertised to VRChat with mDNS.",
                "",
                "Just enable OSC in VRChat — it will find this app on its own.",
            ],
            Step::SetupAutoSave => &[
                "Auto-save writes this device's configuration to devices/ every",
                "time you change something. Turn it off if you would rather save",
                "by hand with the button in the Channels section.",
                "",
                "The same panel unlocks the Alarm section, which drives every",
                "connected device on a schedule.",
            ],
            Step::Done => &[
                "That is the whole app.",
                "",
                "You can reopen this tour any time from Setup → Help.",
                "",
                "Worth remembering:",
                "  · Start with a low power ceiling, 20–30",
                "  · Set up DGB contacts in Unity for custom zones",
                "  · Presets for waveforms, zone sets for mappings",
                "  · Press 1–9 to jump between sections, T for themes",
            ],
        }
    }
}

pub struct Tutorial {
    pub active: bool,
    step: Step,
}

impl Default for Tutorial {
    fn default() -> Self {
        Self::new()
    }
}

impl Tutorial {
    pub fn new() -> Self {
        Self {
            active: false,
            step: Step::Welcome,
        }
    }

    pub fn start(&mut self) {
        self.active = true;
        self.step = Step::Welcome;
    }

    fn advance(&mut self, delta: i32, state: &mut UiState) {
        let steps = Step::for_mode(state.mode);
        let i = steps.iter().position(|s| *s == self.step).unwrap_or(0) as i32 + delta;
        if i < 0 {
            return;
        }
        match steps.get(i as usize) {
            Some(s) => {
                self.step = *s;
                if let Some(section) = s.section()
                    && state.section_visible(section) {
                        state.section = section;
                    }
            }
            None => self.finish(),
        }
    }

    fn finish(&mut self) {
        self.active = false;
        if let Err(e) = shocking_vrc_app::prefs::update_ui_prefs(|p| p.has_seen_tutorial = true) {
            log::warn!("[prefs] Failed to record tutorial completion: {e}");
        }
    }

    pub fn show(&mut self, ctx: &Context, pal: &Palette, state: &mut UiState) {
        let step = self.step;
        let steps = Step::for_mode(state.mode);
        let n = steps.len();
        let i = steps.iter().position(|s| *s == step).unwrap_or(0);

        let mut next = false;
        let mut prev = false;
        let mut close = false;

        egui::Window::new("tour")
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-24.0, -44.0))
            .frame(
                egui::Frame::NONE
                    .fill(pal.base)
                    .inner_margin(egui::Margin::same(16)),
            )
            .default_width(430.0)
            .show(ctx, |ui| {
                let outer = ui.max_rect().expand(16.0);
                ui.painter().rect_filled(outer, w::SQUARE, pal.base);
                w::dashed_rect(ui, outer, pal.accent);

                w::heading_with_suffix(ui, pal, step.title(), Some(&format!("{}/{n}", i + 1)));

                for line in step.body() {
                    if line.is_empty() {
                        ui.add_space(6.0);
                    } else {
                        ui.label(
                            egui::RichText::new(*line)
                                .font(font::small())
                                .color(pal.contrast),
                        );
                    }
                }

                ui.add_space(space::ITEM_GAP);

                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 3.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(rect, w::SQUARE, pal.well);
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(rect.width() * (i + 1) as f32 / n as f32, rect.height()),
                    ),
                    w::SQUARE,
                    pal.accent,
                );

                ui.add_space(space::ITEM_GAP);
                ui.horizontal(|ui| {
                    if w::cta(
                        ui,
                        pal,
                        if step == Step::Done { "Finish" } else { "Next" },
                    )
                    .clicked()
                    {
                        next = true;
                    }
                    if w::button_enabled(ui, pal, "Back", i > 0).clicked() {
                        prev = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if w::button(ui, pal, "Skip tour").clicked() {
                            close = true;
                        }
                    });
                });
            });

        ctx.input(|inp| {
            for ev in &inp.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    ..
                } = ev
                {
                    match key {
                        egui::Key::Enter | egui::Key::ArrowRight | egui::Key::Space => next = true,
                        egui::Key::Backspace | egui::Key::ArrowLeft => prev = true,
                        egui::Key::Escape => close = true,
                        _ => {}
                    }
                }
            }
        });

        if close {
            self.finish();
        } else if next {
            self.advance(1, state);
        } else if prev {
            self.advance(-1, state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_has_a_title_and_body() {
        for s in Step::ALL {
            assert!(!s.title().is_empty());
            assert!(!s.body().is_empty(), "{:?} has no body", s);
        }
    }

    #[test]
    fn steps_are_indexed_in_order() {
        for (i, s) in Step::ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
        }
    }

    #[test]
    fn the_tour_visits_every_section_that_has_one() {
        let visited: Vec<Section> = Step::ALL.iter().filter_map(|s| s.section()).collect();
        for section in [
            Section::Status,
            Section::Graph,
            Section::Zones,
            Section::Channels,
            Section::Tuning,
            Section::Modulation,
            Section::Presets,
            Section::Setup,
        ] {
            assert!(
                visited.contains(&section),
                "{:?} is never covered by the tour",
                section
            );
        }
    }

    #[test]
    fn finishing_past_the_last_step_closes_the_tour() {
        let mut t = Tutorial::new();
        t.start();
        let mut state = UiState::for_test();
        for _ in 0..Step::ALL.len() {
            t.advance(1, &mut state);
        }
        assert!(!t.active, "the tour ends after the last step");
    }

    #[test]
    fn the_tour_follows_the_mode() {
        let classic = Step::for_mode(EngineMode::Classic);
        let graph = Step::for_mode(EngineMode::Graph);
        assert!(!classic.contains(&Step::GraphIntro), "the Classic tour is unchanged");
        assert_eq!(classic.len(), Step::ALL.len() - 1);
        assert!(graph.contains(&Step::GraphIntro));
        assert!(!graph.contains(&Step::ZonesIntro));
    }

    #[test]
    fn back_stops_at_the_first_step() {
        let mut t = Tutorial::new();
        t.start();
        let mut state = UiState::for_test();
        t.advance(-1, &mut state);
        assert_eq!(t.step, Step::Welcome);
        assert!(t.active);
    }
}
