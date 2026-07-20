use crate::tui::app::Tab;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TutorialStep {
    Welcome,
    StatusOverview,
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

impl TutorialStep {
    pub const ALL: &'static [TutorialStep] = &[
        TutorialStep::Welcome,
        TutorialStep::StatusOverview,
        TutorialStep::ZonesIntro,
        TutorialStep::ZonesAddZone,
        TutorialStep::ZonesDgbTip,
        TutorialStep::ChannelsFreq,
        TutorialStep::ChannelsIntensity,
        TutorialStep::ChannelsLimits,
        TutorialStep::ChannelsAggregation,
        TutorialStep::ModulationIntro,
        TutorialStep::ModulationApply,
        TutorialStep::TuningIntro,
        TutorialStep::PresetsIntro,
        TutorialStep::PresetsApply,
        TutorialStep::SetupOsc,
        TutorialStep::SetupAutoSave,
        TutorialStep::Done,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    pub fn tab(self) -> Option<Tab> {
        match self {
            Self::Welcome | Self::Done => None,
            Self::StatusOverview => Some(Tab::Status),
            Self::ZonesIntro | Self::ZonesAddZone | Self::ZonesDgbTip => Some(Tab::Zones),
            Self::ChannelsFreq
            | Self::ChannelsIntensity
            | Self::ChannelsLimits
            | Self::ChannelsAggregation => Some(Tab::Channels),
            Self::ModulationIntro | Self::ModulationApply => Some(Tab::Modulation),
            Self::TuningIntro => Some(Tab::Tuning),
            Self::PresetsIntro | Self::PresetsApply => Some(Tab::Presets),
            Self::SetupOsc | Self::SetupAutoSave => Some(Tab::Setup),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome to ShockingVRC!",
            Self::StatusOverview => "Status tab",
            Self::ZonesIntro => "Zones tab — overview",
            Self::ZonesAddZone => "Zones — adding a zone",
            Self::ZonesDgbTip => "DGB contacts — Unity setup",
            Self::ChannelsFreq => "Channels — frequency",
            Self::ChannelsIntensity => "Channels — intensity",
            Self::ChannelsLimits => "Channels — power limits",
            Self::ChannelsAggregation => "Channels — aggregation",
            Self::ModulationIntro => "Modulation — overview",
            Self::ModulationApply => "Modulation — applying",
            Self::TuningIntro => "Tuning — UKF & normalization",
            Self::PresetsIntro => "Presets — catalog",
            Self::PresetsApply => "Presets — apply & save",
            Self::SetupOsc => "Setup — OSC & OSCQuery",
            Self::SetupAutoSave => "Setup — auto-save",
            Self::Done => "All done!",
        }
    }

    pub fn body(self) -> &'static [&'static str] {
        match self {
            Self::Welcome => &[
                "This interactive guide will walk you through every tab.",
                "The app below is a sandbox — feel free to click anything!",
                "",
                "Navigation:",
                "  [Enter] or click  Next  — go to the next step",
                "  [Backspace]  or  Back   — go back",
                "  [Esc]                   — exit tutorial",
                "",
                "Press Enter to begin.",
            ],
            Self::StatusOverview => &[
                "This is the Status tab. It shows:",
                "  - Coyote device connection & battery level",
                "  - VRChat OSC status and live frequency",
                "  - Real-time channel A / B power output",
                "  - Active zones with their stimulation level",
            ],
            Self::ZonesIntro => &[
                "Zones are avatar contact areas that drive the device.",
                "Left columns show zones mapped to channel A and B.",
                "Right column shows zones discovered from your avatar.",
                "",
                "Each zone has a mode: depth, speed, acc, or recoil.",
            ],
            Self::ZonesAddZone => &[
                "Try clicking a zone in the avatar list on the right,",
                "then click  Add -> A  or  Add -> B  to map it.",
                "",
                "You can also use  Add all -> A  to map every zone.",
                "Click  Cycle mode  to change between depth/speed/acc.",
                "Click  Remove  to remove a mapped zone.",
            ],
            Self::ZonesDgbTip => &[
                "** DGB Contacts — Unity setup **",
                "",
                "To use DGB (custom) contacts you need to set up",
                "VRC Contact Receiver in Unity:",
                "",
                "  1. Add a VRC Contact Receiver component to your avatar",
                "  2. Set  Receiver Type  to  Proximity",
                "  3. Set  Parameter  to  DGB/YourContactName",
                "     (e.g. DGB/Chest, DGB/TouchAreaA)",
                "  4. Upload the avatar",
                "",
                "The app will discover DGB zones automatically via OSC.",
            ],
            Self::ChannelsFreq => &[
                "Each channel has 4 frequency segments (seg 0-3).",
                "Frequency controls the Hz of the DG-Lab waveform.",
                "Range: 10-255 (lower = faster pulsing).",
                "",
                "Use  <- ->  arrows or drag the slider to adjust.",
                "Hold  Shift  for +/-10 steps.",
            ],
            Self::ChannelsIntensity => &[
                "Intensity controls how strong each segment feels.",
                "Range: 0-100%.",
                "",
                "Together with frequency, the 4 segments create a",
                "repeating waveform pattern on the device.",
            ],
            Self::ChannelsLimits => &[
                "** Power limits are your safety net! **",
                "",
                "  Limit Min  — minimum power level (usually 0)",
                "  Limit Max  — maximum power the device will output",
                "",
                "Presets NEVER change your limits.",
                "Start low (20-30) and increase carefully!",
            ],
            Self::ChannelsAggregation => &[
                "When multiple zones are active, aggregation decides",
                "how their values combine:",
                "",
                "  Max     — strongest zone wins",
                "  Sum     — all zones add up (capped at 100%)",
                "  Average — average of all active zones",
                "",
                "Click the mode button to cycle through them.",
            ],
            Self::ModulationIntro => &[
                "Modulation adds movement to frequency or intensity.",
                "Each channel has 4 freq + 4 intensity slots (16 total).",
                "",
                "Click a slot on the left to edit it on the right.",
                "Pick a function (sin, triangle, saw, etc.),",
                "a source (depth, speed, acc, recoil),",
                "and tweak parameters like speed, phase, deviation.",
            ],
            Self::ModulationApply => &[
                "After editing a modulation slot:",
                "  Click  Apply  to save it to the config",
                "  Click  Clear  to disable that slot",
                "  Click  Clear All  to remove all modulation",
                "",
                "The preview graph shows how the waveform will look.",
            ],
            Self::TuningIntro => &[
                "Tuning has advanced DSP parameters:",
                "",
                "  UKF (Unscented Kalman Filter) — smooths noisy",
                "  contact data. Lower Q = smoother but slower.",
                "",
                "  Motion norms — scale factors for speed, acc,",
                "  and recoil modes. Higher = less sensitive.",
                "",
                "Click  Reset  to restore defaults.",
            ],
            Self::PresetsIntro => &[
                "Presets load frequency, intensity, and modulation",
                "settings from a catalog.",
                "",
                "Official presets are fetched from GitHub.",
                "Your own presets are saved locally in presets/user/.",
                "",
                "Press  R  to refresh the catalog.",
            ],
            Self::PresetsApply => &[
                "Select a preset from the list, then:",
                "  -> A  — apply to channel A",
                "  -> B  — apply to channel B",
                "",
                "  Save A / Save B  — save current channel as preset",
                "",
                "Presets NEVER touch your power limits or zones!",
            ],
            Self::SetupOsc => &[
                "OSC & OSCQuery are configured automatically.",
                "A free port is picked at runtime and used for both",
                "OSC (UDP) and OSCQuery (HTTP), then advertised to",
                "VRChat over mDNS.",
                "",
                "Just enable OSC in VRChat — it discovers this app",
            ],
            Self::SetupAutoSave => &[
                "Auto-save writes cli_config.json whenever you",
                "change any setting. Turn OFF if you want manual",
                "save only (use Save button on Channels tab).",
                "",
                "That's everything! You're ready to use ShockingVRC.",
            ],
            Self::Done => &[
                "You've completed the tutorial!",
                "",
                "You can always re-open it from the Setup tab.",
                "",
                "Key tips:",
                "  - Start with LOW power limits (20-30)",
                "  - Set up DGB contacts in Unity for custom zones",
                "  - Use presets for quick waveform configs",
                "  - Tab / number keys to switch tabs",
                "",
                "Press  Enter  or  Esc  to close.",
            ],
        }
    }
}
