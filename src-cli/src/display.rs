use std::io::{self, Write};

use shocking_vrc_core::cli::{CliConfig, CliStatus};
use shocking_vrc_core::raw_to_hz;

pub fn print_banner() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║       ShockingVRC CLI — Two-Channel OSC Controller   ║");
    println!("║  Channels A & B  ·  Zone mapping  ·  Power limits    ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();
}

pub fn print_config_summary(cfg: &CliConfig, w: &mut impl Write) -> io::Result<()> {
    writeln!(w)?;
    writeln!(w, "┌────────────────────────────────────────────────────────────┐")?;
    writeln!(w, "│                    Current CLI Config                      │")?;
    writeln!(w, "├─────────────────────────────────┬──────────────────────────┤")?;
    writeln!(w, "│  Channel A                      │  Channel B               │")?;
    writeln!(w, "├─────────────────────────────────┼──────────────────────────┤")?;

    let a = &cfg.channel_a;
    let b = &cfg.channel_b;
    let a_zones: Vec<_> = a.zones.iter().map(|e| e.to_string()).collect();
    let b_zones: Vec<_> = b.zones.iter().map(|e| e.to_string()).collect();
    let max_rows = a_zones.len().max(b_zones.len()).max(1);

    for i in 0..max_rows {
        let az = a_zones.get(i).map(|s| s.as_str()).unwrap_or("");
        let bz = b_zones.get(i).map(|s| s.as_str()).unwrap_or("");
        writeln!(w, "│  zone: {az:<25}│  zone: {bz:<17}│")?;
    }

    writeln!(w, "├─────────────────────────────────┼──────────────────────────┤")?;
    writeln!(
        w,
        "│  limits : {:>3}–{:<3}                 │  limits : {:>3}–{:<3}           │",
        a.limits.min, a.limits.max, b.limits.min, b.limits.max
    )?;

    let fmt_freq_hz = |f: &[u8; 4]| -> String {
        f.iter()
            .map(|&r| format!("{:>3.0}Hz", raw_to_hz(r)))
            .collect::<Vec<_>>()
            .join(" ")
    };
    writeln!(w, "│  freq   : {:?}  │  freq   : {:?} │", a.frequency, b.frequency)?;
    writeln!(
        w,
        "│    (Hz) : {:<21}│    (Hz) : {:<13}│",
        fmt_freq_hz(&a.frequency),
        fmt_freq_hz(&b.frequency)
    )?;
    writeln!(w, "│  intens : {:?}  │  intens : {:?} │", a.intensity, b.intensity)?;
    writeln!(w, "├─────────────────────────────────┴──────────────────────────┤")?;
    let u = &cfg.ukf;
    writeln!(
        w,
        "│  UKF: q={:.4}  r={:.4}  alpha={:.2}  beta={:.2}  kappa={:.2}    │",
        u.q, u.r, u.alpha, u.beta, u.kappa
    )?;
    let n = &cfg.norms;
    writeln!(
        w,
        "│  Norms: speed={:.2}  acc={:.2}  recoil={:.2}                    │",
        n.speed, n.acc, n.recoil
    )?;
    writeln!(w, "└────────────────────────────────────────────────────────────┘")?;
    writeln!(w)?;
    Ok(())
}

pub fn print_full_status(status: &CliStatus, w: &mut impl Write) -> io::Result<()> {
    let a = &status.channel_a;
    let b = &status.channel_b;
    let dev_str = if status.device_connected { "connected" } else { "searching..." };

    writeln!(w)?;
    writeln!(w, "┌──────────────────────────────────────────────────────┐")?;
    writeln!(w, "│  Device: {dev_str:<44}│")?;
    writeln!(w, "├──────────────────────────┬──────────────────────────┤")?;
    writeln!(w, "│  Channel A               │  Channel B               │")?;
    writeln!(w, "├──────────────────────────┼──────────────────────────┤")?;
    writeln!(w, "│  level    : {:<13.3}│  level    : {:<13.3}│", a.raw_level, b.raw_level)?;
    writeln!(w, "│  strength : {:<13}│  strength : {:<13}│", a.strength, b.strength)?;
    writeln!(w, "│  active zones:           │  active zones:           │")?;

    let max_zones = a.active_zones.len().max(b.active_zones.len()).max(1);
    for i in 0..max_zones {
        let az = a
            .active_zones
            .get(i)
            .map(|(id, lvl)| format!("{id} ({lvl:.2})"))
            .unwrap_or_default();
        let bz = b
            .active_zones
            .get(i)
            .map(|(id, lvl)| format!("{id} ({lvl:.2})"))
            .unwrap_or_default();
        writeln!(w, "│    {az:<22}│    {bz:<22}│")?;
    }

    writeln!(w, "└──────────────────────────┴──────────────────────────┘")?;
    Ok(())
}

pub fn print_status_header() {
    println!("{:<42} {:<42}", "  Channel A", "  Channel B");
    println!(
        "{:<8} {:<22} {:<8}   {:<8} {:<22} {:<8}",
        "Level", "Bar", "Str", "Level", "Bar", "Str"
    );
    println!("{}", "─".repeat(88));
}

pub fn print_status_line(la: f32, sa: u8, lb: f32, sb: u8) {
    let bar_a = power_bar(la, 20);
    let bar_b = power_bar(lb, 20);
    println!("{:.3}   {}  {:>3}     {:.3}   {}  {:>3}", la, bar_a, sa, lb, bar_b, sb);
}

pub fn power_bar(level: f32, width: usize) -> String {
    let filled = ((level * width as f32).round() as usize).min(width);
    let bar: String = "█".repeat(filled) + &"░".repeat(width - filled);
    format!("[{bar}]")
}
