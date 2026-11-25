use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub use shocking_vrc_app::prefs::GraphicsPreference as Preference;

pub const POWER_POLL_INTERVAL: Duration = Duration::from_secs(10);
pub const ANIMATION_INTERVAL: Duration = Duration::from_millis(33);
pub const TELEMETRY_INTERVAL_FULL: Duration = Duration::from_secs(1);
pub const TELEMETRY_INTERVAL_LITE: Duration = Duration::from_secs(5);

pub const GUARD_WINDOW: Duration = Duration::from_secs(5);
pub const GUARD_MAX_GAP: Duration = Duration::from_millis(250);
pub const SLOW_FRAME_MS: f32 = 20.0;

const SOFTWARE_RENDERERS: &[&str] = &[
    "basic render driver",
    "swiftshader",
    "llvmpipe",
    "softpipe",
    "software adapter",
    "gdi generic",
    "apple software",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Full,
    Lite,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Lite => "Lite",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Machine {
    pub on_battery: Option<bool>,
    pub battery_present: Option<bool>,
    pub energy_saver: bool,
    pub animations_off: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Runtime {
    pub renderer: String,
    pub is_cpu_adapter: bool,
    pub slow_frames: bool,
}

impl Runtime {
    pub fn detect(cc: &eframe::CreationContext<'_>) -> Self {
        let mut runtime = Self::default();
        if let Some(gl) = cc.gl.as_ref() {
            use eframe::glow::HasContext as _;
            let renderer = unsafe { gl.get_parameter_string(eframe::glow::RENDERER) };
            runtime.renderer = renderer.trim().to_string();
        } else {
            runtime.is_cpu_adapter = true;
        }
        runtime
    }

    pub fn from_adapter_info(renderer: impl Into<String>, is_cpu_adapter: bool) -> Self {
        Self {
            renderer: renderer.into(),
            is_cpu_adapter,
            slow_frames: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub mode: Mode,
    pub reason: String,
    pub machine: Machine,
}

impl Decision {
    pub fn status_line(&self, preference: Preference) -> String {
        let source = match preference {
            Preference::Auto => "Auto",
            Preference::Full | Preference::Lite => "Set",
        };
        format!("{source} -> {}: {}", self.mode.label(), self.reason)
    }
}

pub fn is_software_renderer(renderer: &str) -> bool {
    let lower = renderer.to_ascii_lowercase();
    SOFTWARE_RENDERERS.iter().any(|needle| lower.contains(needle))
}

pub fn decide(preference: Preference, machine: Machine, runtime: &Runtime) -> Decision {
    let (mode, reason) = match preference {
        Preference::Full => (Mode::Full, "chosen".to_string()),
        Preference::Lite => (Mode::Lite, "chosen".to_string()),
        Preference::Auto => {
            if machine.animations_off {
                let msg = if cfg!(windows) {
                    "Windows animation effects are off"
                } else {
                    "animation effects are off"
                };
                (Mode::Lite, msg.to_string())
            } else if runtime.is_cpu_adapter {
                (Mode::Lite, "no hardware acceleration".to_string())
            } else if is_software_renderer(&runtime.renderer) {
                (Mode::Lite, format!("software rendering ({})", runtime.renderer))
            } else if machine.energy_saver {
                (Mode::Lite, "energy saver is on".to_string())
            } else if machine.on_battery == Some(true) {
                (Mode::Lite, "on battery".to_string())
            } else if runtime.slow_frames {
                (Mode::Lite, "frames were too slow".to_string())
            } else if machine.battery_present == Some(true) {
                (Mode::Full, "plugged in".to_string())
            } else {
                (Mode::Full, "hardware graphics".to_string())
            }
        }
    };

    Decision { mode, reason, machine }
}

pub fn apply_mode_to_ctx(ctx: &egui::Context, mode: Mode, default_animation_time: f32) {
    let anim = match mode {
        Mode::Lite => 0.0,
        Mode::Full => default_animation_time,
    };
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.animation_time = anim;
        ctx.set_style_of(theme, style);
    }
}

#[cfg(target_os = "windows")]
pub fn machine() -> Machine {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW};

    let mut m = Machine::default();

    let mut status = SYSTEM_POWER_STATUS {
        ACLineStatus: 255,
        BatteryFlag: 255,
        BatteryLifePercent: 255,
        SystemStatusFlag: 0,
        BatteryLifeTime: u32::MAX,
        BatteryFullLifeTime: u32::MAX,
    };
    if unsafe { GetSystemPowerStatus(&mut status) } != 0 {
        m.on_battery = match status.ACLineStatus {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        };
        m.battery_present = if status.BatteryFlag == 255 {
            None
        } else {
            Some((status.BatteryFlag & 128) == 0)
        };
        m.energy_saver = status.SystemStatusFlag == 1;
    }

    let mut animations: i32 = 1;
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&mut animations as *mut i32).cast(), 0) };
    if ok != 0 {
        m.animations_off = animations == 0;
    }

    m
}

#[cfg(target_os = "linux")]
pub fn machine() -> Machine {
    let mut m = probe_linux_power_supply(std::path::Path::new("/sys/class/power_supply"));

    if let Ok(out) = std::process::Command::new("powerprofilesctl").arg("get").output()
        && out.status.success()
        && String::from_utf8_lossy(&out.stdout).trim() == "power-saver"
    {
        m.energy_saver = true;
    }

    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "enable-animations"])
        .output()
        && out.status.success()
        && String::from_utf8_lossy(&out.stdout).trim() == "false"
    {
        m.animations_off = true;
    }

    m
}

#[cfg(target_os = "linux")]
fn probe_linux_power_supply(root: &std::path::Path) -> Machine {
    let mut m = Machine::default();
    let Ok(entries) = std::fs::read_dir(root) else {
        return m;
    };
    let mut saw_mains = false;

    for entry in entries.flatten() {
        let dir = entry.path();
        let Ok(kind) = std::fs::read_to_string(dir.join("type")) else {
            continue;
        };
        match kind.trim() {
            "Battery" => {
                if let Ok(present) = std::fs::read_to_string(dir.join("present"))
                    && present.trim() == "0"
                {
                    continue;
                }
                m.battery_present = Some(true);
                if let Ok(status) = std::fs::read_to_string(dir.join("status")) {
                    match status.trim() {
                        "Discharging" => m.on_battery = Some(true),
                        "Charging" | "Full" | "Not charging" => {
                            if m.on_battery != Some(true) {
                                m.on_battery = Some(false);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "Mains" | "USB" => {
                saw_mains = true;
                if let Ok(online) = std::fs::read_to_string(dir.join("online"))
                    && online.trim() == "1"
                    && m.on_battery != Some(true)
                {
                    m.on_battery = Some(false);
                }
            }
            _ => {}
        }
    }

    if m.battery_present.is_none() && saw_mains {
        m.battery_present = Some(false);
    }

    m
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn machine() -> Machine {
    Machine::default()
}

#[derive(Clone)]
pub struct WindowVisibility {
    tray_shown: Arc<AtomicBool>,
    effective: Arc<AtomicBool>,
}

impl Default for WindowVisibility {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowVisibility {
    pub fn new() -> Self {
        Self {
            tray_shown: Arc::new(AtomicBool::new(true)),
            effective: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.effective)
    }

    pub fn is_visible(&self) -> bool {
        self.effective.load(Ordering::Relaxed)
    }

    pub fn set_tray_visible(&self, shown: bool, ctx: &egui::Context) {
        self.tray_shown.store(shown, Ordering::Relaxed);
        let prev = self.effective.swap(shown, Ordering::Relaxed);
        if shown && !prev {
            ctx.request_repaint();
        }
    }

    pub fn sync_from_ctx(&self, ctx: &egui::Context) -> (bool, bool) {
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        self.sync_minimized(minimized)
    }

    pub fn sync_minimized(&self, minimized: bool) -> (bool, bool) {
        let tray_shown = self.tray_shown.load(Ordering::Relaxed);
        let now = tray_shown && !minimized;
        let prev = self.effective.swap(now, Ordering::Relaxed);
        (now, now && !prev)
    }
}

pub struct PowerWatcher {
    machine: Arc<Mutex<Machine>>,
    stop: Arc<(Mutex<bool>, Condvar)>,
}

impl PowerWatcher {
    pub fn spawn(initial: Machine, visible: Arc<AtomicBool>, ctx: egui::Context) -> Self {
        let state = Arc::new(Mutex::new(initial));
        let stop = Arc::new((Mutex::new(false), Condvar::new()));

        let shared = Arc::clone(&state);
        let stop_bg = Arc::clone(&stop);
        let _ = std::thread::Builder::new()
            .name("power-watch".to_string())
            .spawn(move || {
                let mut last = initial;
                let (lock, cvar) = &*stop_bg;
                loop {
                    let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                    if *guard {
                        break;
                    }
                    let (guard, _) = cvar
                        .wait_timeout(guard, POWER_POLL_INTERVAL)
                        .unwrap_or_else(|e| e.into_inner());
                    if *guard {
                        break;
                    }
                    drop(guard);

                    let now = machine();
                    if now != last {
                        last = now;
                        if let Ok(mut slot) = shared.lock() {
                            *slot = now;
                        }
                        if visible.load(Ordering::Relaxed) {
                            ctx.request_repaint();
                        }
                    }
                }
            });

        Self { machine: state, stop }
    }

    pub fn latest(&self) -> Machine {
        self.machine.lock().map(|m| *m).unwrap_or_default()
    }
}

impl Drop for PowerWatcher {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.stop;
        if let Ok(mut stopped) = lock.lock() {
            *stopped = true;
            cvar.notify_all();
        }
    }
}

#[derive(Debug, Default)]
pub struct FrameGuard {
    window_start: Option<Duration>,
    last_sample: Option<Duration>,
    sum_ms: f64,
    samples: u32,
    tripped: bool,
}

impl FrameGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tripped(&self) -> bool {
        self.tripped
    }

    pub fn reset(&mut self) {
        self.window_start = None;
        self.last_sample = None;
        self.sum_ms = 0.0;
        self.samples = 0;
    }

    pub fn record(&mut self, now: Duration, frame_cost_ms: f32) -> Option<f32> {
        if self.tripped || !frame_cost_ms.is_finite() || frame_cost_ms < 0.0 {
            return None;
        }
        if let Some(prev) = self.last_sample
            && now.saturating_sub(prev) > GUARD_MAX_GAP
        {
            self.reset();
        }

        let start = *self.window_start.get_or_insert(now);
        self.last_sample = Some(now);
        self.sum_ms += frame_cost_ms as f64;
        self.samples += 1;

        if now.saturating_sub(start) >= GUARD_WINDOW && self.samples > 0 {
            let avg = (self.sum_ms / self.samples as f64) as f32;
            if avg > SLOW_FRAME_MS {
                self.tripped = true;
                self.reset();
                return Some(avg);
            }
            self.window_start = Some(now);
            self.sum_ms = frame_cost_ms as f64;
            self.samples = 1;
        }
        None
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RepaintNeed {
    pub visible: bool,
    pub mode: Mode,
    pub animating: bool,
    pub periodic_telemetry: bool,
}

pub fn next_repaint(need: RepaintNeed) -> Option<Duration> {
    if !need.visible {
        return None;
    }
    match need.mode {
        Mode::Full => {
            if need.animating {
                Some(ANIMATION_INTERVAL)
            } else if need.periodic_telemetry {
                Some(TELEMETRY_INTERVAL_FULL)
            } else {
                None
            }
        }
        Mode::Lite => {
            if need.periodic_telemetry {
                Some(TELEMETRY_INTERVAL_LITE)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw_runtime() -> Runtime {
        Runtime::from_adapter_info("NVIDIA GeForce RTX 4080", false)
    }

    #[test]
    fn desktop_hardware_gpu_picks_full() {
        let d = decide(Preference::Auto, Machine::default(), &hw_runtime());
        assert_eq!(d.mode, Mode::Full);
        assert_eq!(d.reason, "hardware graphics");
        assert_eq!(d.status_line(Preference::Auto), "Auto -> Full: hardware graphics");
    }

    #[test]
    fn laptop_on_battery_vs_plugged_in() {
        let battery = Machine {
            on_battery: Some(true),
            battery_present: Some(true),
            ..Machine::default()
        };
        let d_batt = decide(Preference::Auto, battery, &hw_runtime());
        assert_eq!(d_batt.mode, Mode::Lite);
        assert_eq!(d_batt.reason, "on battery");
        assert_eq!(d_batt.status_line(Preference::Auto), "Auto -> Lite: on battery");

        let plugged = Machine {
            on_battery: Some(false),
            battery_present: Some(true),
            ..Machine::default()
        };
        let d_ac = decide(Preference::Auto, plugged, &hw_runtime());
        assert_eq!(d_ac.mode, Mode::Full);
        assert_eq!(d_ac.reason, "plugged in");
    }

    #[test]
    fn energy_saver_and_animations_off_pick_lite_in_priority_order() {
        let saver = Machine {
            energy_saver: true,
            battery_present: Some(true),
            on_battery: Some(false),
            animations_off: false,
        };
        let d_saver = decide(Preference::Auto, saver, &hw_runtime());
        assert_eq!(d_saver.mode, Mode::Lite);
        assert_eq!(d_saver.reason, "energy saver is on");

        let anim_off = Machine {
            animations_off: true,
            energy_saver: true,
            on_battery: Some(true),
            battery_present: Some(true),
        };
        let cpu_rt = Runtime {
            renderer: "SwiftShader".to_string(),
            is_cpu_adapter: true,
            slow_frames: true,
        };
        let d_anim = decide(Preference::Auto, anim_off, &cpu_rt);
        assert_eq!(d_anim.mode, Mode::Lite);
        let expected = if cfg!(windows) {
            "Windows animation effects are off"
        } else {
            "animation effects are off"
        };
        assert_eq!(d_anim.reason, expected);
    }

    #[test]
    fn software_renderers_and_cpu_adapter_pick_lite() {
        let cpu = Runtime {
            renderer: "Microsoft Basic Render Driver".to_string(),
            is_cpu_adapter: true,
            slow_frames: false,
        };
        let d_cpu = decide(Preference::Auto, Machine::default(), &cpu);
        assert_eq!(d_cpu.mode, Mode::Lite);
        assert_eq!(d_cpu.reason, "no hardware acceleration");

        for name in [
            "Microsoft Basic Render Driver",
            "Google SwiftShader",
            "llvmpipe (LLVM 17.0.6, 256 bits)",
            "softpipe",
            "Software Adapter",
            "GDI Generic",
            "Apple Software Renderer",
        ] {
            assert!(is_software_renderer(name), "{name} should be recognized as software");
            let rt = Runtime::from_adapter_info(name, false);
            let d = decide(Preference::Auto, Machine::default(), &rt);
            assert_eq!(d.mode, Mode::Lite);
            assert_eq!(d.reason, format!("software rendering ({name})"));
        }
    }

    #[test]
    fn slow_frames_picks_lite_after_battery_and_before_plugged_in() {
        let rt = Runtime {
            renderer: "AMD Radeon RX 7800 XT".to_string(),
            is_cpu_adapter: false,
            slow_frames: true,
        };
        let plugged = Machine {
            on_battery: Some(false),
            battery_present: Some(true),
            ..Machine::default()
        };
        let d = decide(Preference::Auto, plugged, &rt);
        assert_eq!(d.mode, Mode::Lite);
        assert_eq!(d.reason, "frames were too slow");
    }

    #[test]
    fn user_overrides_always_win_with_chosen_reason() {
        let on_battery = Machine {
            on_battery: Some(true),
            battery_present: Some(true),
            energy_saver: true,
            animations_off: true,
        };
        let sw = Runtime {
            renderer: "SwiftShader".to_string(),
            is_cpu_adapter: true,
            slow_frames: true,
        };
        let d_full = decide(Preference::Full, on_battery, &sw);
        assert_eq!(d_full.mode, Mode::Full);
        assert_eq!(d_full.reason, "chosen");
        assert_eq!(d_full.status_line(Preference::Full), "Set -> Full: chosen");

        let d_lite = decide(Preference::Lite, Machine::default(), &hw_runtime());
        assert_eq!(d_lite.mode, Mode::Lite);
        assert_eq!(d_lite.reason, "chosen");
        assert_eq!(d_lite.status_line(Preference::Lite), "Set -> Lite: chosen");
    }

    #[test]
    fn live_machine_probe_does_not_panic() {
        let m = machine();
        let _ = decide(Preference::Auto, m, &hw_runtime());
    }

    #[test]
    fn frame_guard_trips_once_after_five_seconds_of_slow_frames() {
        let mut g = FrameGuard::new();
        let mut tripped_at = None;
        for step in 0..200 {
            let now = Duration::from_millis(step * 33);
            if let Some(avg) = g.record(now, 25.0) {
                assert!(tripped_at.is_none(), "guard must trip only once");
                assert!(avg > SLOW_FRAME_MS);
                tripped_at = Some(now);
            }
        }
        let t = tripped_at.expect("guard should have tripped");
        assert!(t >= GUARD_WINDOW);
        assert!(g.tripped());
        assert_eq!(g.record(Duration::from_secs(20), 40.0), None);
    }

    #[test]
    fn frame_guard_never_trips_on_fast_frames() {
        let mut g = FrameGuard::new();
        for step in 0..400 {
            let now = Duration::from_millis(step * 33);
            assert_eq!(g.record(now, 12.0), None);
        }
        assert!(!g.tripped());
    }

    #[test]
    fn frame_guard_resets_window_on_large_gaps() {
        let mut g = FrameGuard::new();
        for step in 0..120 {
            let now = Duration::from_millis(step * 33);
            assert_eq!(g.record(now, 28.0), None);
        }
        let after_gap = Duration::from_millis(120 * 33 + 300);
        for step in 0..60 {
            let now = after_gap + Duration::from_millis(step * 33);
            assert_eq!(g.record(now, 28.0), None);
        }
        assert!(!g.tripped());
        let mut tripped = false;
        for step in 60..200 {
            let now = after_gap + Duration::from_millis(step * 33);
            if g.record(now, 28.0).is_some() {
                tripped = true;
                break;
            }
        }
        assert!(tripped);
    }

    #[test]
    fn repaint_helper_respects_visibility_animation_cap_and_telemetry_throttling() {
        for mode in [Mode::Full, Mode::Lite] {
            assert_eq!(
                next_repaint(RepaintNeed {
                    visible: false,
                    mode,
                    animating: true,
                    periodic_telemetry: true,
                }),
                None
            );
        }

        assert_eq!(
            next_repaint(RepaintNeed {
                visible: true,
                mode: Mode::Lite,
                animating: true,
                periodic_telemetry: false,
            }),
            None
        );

        assert_eq!(
            next_repaint(RepaintNeed {
                visible: true,
                mode: Mode::Full,
                animating: false,
                periodic_telemetry: false,
            }),
            None
        );

        let anim = next_repaint(RepaintNeed {
            visible: true,
            mode: Mode::Full,
            animating: true,
            periodic_telemetry: true,
        })
        .expect("full animation should schedule repaint");
        assert!(anim >= Duration::from_millis(33));

        assert_eq!(
            next_repaint(RepaintNeed {
                visible: true,
                mode: Mode::Full,
                animating: false,
                periodic_telemetry: true,
            }),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            next_repaint(RepaintNeed {
                visible: true,
                mode: Mode::Lite,
                animating: false,
                periodic_telemetry: true,
            }),
            Some(Duration::from_secs(5))
        );
    }

    #[test]
    fn window_visibility_tracks_minimized_and_restore_transition() {
        let vis = WindowVisibility::new();
        assert!(vis.is_visible());
        assert_eq!(vis.sync_minimized(false), (true, false));
        assert_eq!(vis.sync_minimized(true), (false, false));
        assert!(!vis.is_visible());
        assert_eq!(vis.sync_minimized(true), (false, false));
        assert_eq!(vis.sync_minimized(false), (true, true));
        assert!(vis.is_visible());
    }
}
