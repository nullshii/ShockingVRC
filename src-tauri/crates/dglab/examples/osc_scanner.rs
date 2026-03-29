/// VRChat OSC avatar scanner with SPS zone discovery.
///
/// Run with:
///   cargo run --example osc_scanner
///   cargo run --example osc_scanner -- --port 9001
///   $env:RUST_LOG="debug"; cargo run --example osc_scanner

use dglab::{AvatarScanner, ZoneEvent, ZoneType};

// Target zones to look for on the avatar
const TARGET_ZONES: &[(ZoneType, &str)] = &[
    // Socket (Orf) zones
    (ZoneType::Orf, "Anal"),
    (ZoneType::Orf, "Blowjob"),
    (ZoneType::Orf, "Feet_Footjob"),
    (ZoneType::Orf, "Feet_Steppies_Left"),
    (ZoneType::Orf, "Feet_Steppies_Right"),
    (ZoneType::Orf, "Handjob_Double_Handjob"),
    (ZoneType::Orf, "Handjob_Handjob_Left"),
    (ZoneType::Orf, "Handjob_Handjob_Right"),
    (ZoneType::Orf, "O_A"),
    (ZoneType::Orf, "O_A_2"),
    (ZoneType::Orf, "O_Al"),
    (ZoneType::Orf, "O_V"),
    (ZoneType::Orf, "O_V_2"),
    (ZoneType::Orf, "O_Vl"),
    (ZoneType::Orf, "Pussy"),
    (ZoneType::Orf, "Special_Assjob"),
    (ZoneType::Orf, "Special_Thighjob"),
    (ZoneType::Orf, "Special_Titjob"),
    // Plug (Pen) zones
    (ZoneType::Pen, "Cock"),
];

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let port = parse_port_arg().unwrap_or(9001);

    println!("=== DGLab VRChat OSC Scanner ===");
    println!("UDP port: {port}  |  RUST_LOG=debug for verbose output");
    println!("Make sure VRChat → Settings → OSC is enabled.\n");

    let scanner = AvatarScanner::new(port);
    let mut events = scanner.subscribe();
    // This receiver fires every time a bulk-fetch completes (avatar load / change)
    let mut refreshes = scanner.subscribe_refreshes();

    // Start UDP listener (no auto-discovery — we do it explicitly below)
    scanner.start().await.expect("failed to start OSC listener");

    // First discovery
    println!("[discovery] Scanning for VRChat (up to 5 s)...");
    match scanner.discover_wait().await {
        Ok(true) => {
            if let Some(addr) = scanner.vrchat_address().await {
                println!(
                    "[discovery] VRChat found: {} (OSC {}:{})",
                    addr.http_addr, addr.osc_ip, addr.osc_port
                );
            }
            // The refresh event was already sent by discover_wait, but we also
            // have the data right here — print the report immediately.
            print_zone_report(&scanner.zones().await);
        }
        Ok(false) => {
            println!("[discovery] VRChat not found — is OSC enabled in settings?");
            println!("[discovery] Continuing; will retry on /avatar/change.\n");
        }
        Err(e) => {
            println!("[discovery] Error: {e}\n");
        }
    }

    println!("\n[live] Listening for contact events (Ctrl-C to exit)...\n");

    loop {
        tokio::select! {
            // ── Real-time zone level update ──────────────────────────────────
            result = events.recv() => match result {
                Ok(ev) if ev.level > 0.001 => print_live_event(&ev),
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("[warn] event receiver lagged {n} messages");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            // ── Bulk refresh — avatar changed or newly loaded ────────────────
            result = refreshes.recv() => match result {
                Ok(zones) => {
                    println!("\n[avatar changed — new zone report]");
                    if let Some(addr) = scanner.vrchat_address().await {
                        println!(
                            "[discovery] VRChat: {} (OSC {}:{})",
                            addr.http_addr, addr.osc_ip, addr.osc_port
                        );
                    }
                    print_zone_report(&zones);
                    println!("\n[live] Continuing...\n");
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("[warn] refresh receiver lagged {n} messages");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
        }
    }
}

// Zone discovery report 
fn print_zone_report(found: &[ZoneEvent]) {
    println!();
    println!("┌─────────────────────────────────────────────────┐");
    println!("│          Avatar zone discovery report            │");
    println!("├──────┬──────────┬────────────────────────────────┤");
    println!("│ Stat │ Type     │ ID                             │");
    println!("├──────┼──────────┼────────────────────────────────┤");

    let mut total_found = 0usize;

    for (zone_type, id) in TARGET_ZONES {
        let is_found = found
            .iter()
            .any(|z| &z.zone_type == zone_type && z.id.as_str() == *id);

        let stat = if is_found { "✓" } else { "✗" };
        let type_str = zone_type_str(zone_type);
        println!("│  {stat}   │ {type_str:<8} │ {id:<30} │");

        if is_found {
            total_found += 1;
        }
    }

    println!("├──────┴──────────┴────────────────────────────────┤");
    println!(
        "│  Found {total_found:>2} / {total:<2} target zones                  │",
        total = TARGET_ZONES.len()
    );
    println!("└─────────────────────────────────────────────────┘");

    // Zones on the avatar not in the target list
    let extra: Vec<_> = found
        .iter()
        .filter(|z| {
            !TARGET_ZONES
                .iter()
                .any(|(t, id)| t == &z.zone_type && *id == z.id.as_str())
        })
        .collect();

    if !extra.is_empty() {
        println!("\n  [extra zones on avatar]");
        for z in extra {
            println!("    {}:{}", zone_type_str(&z.zone_type), z.id);
        }
    }
    println!();
}

// Live event line
fn print_live_event(ev: &ZoneEvent) {
    let kind = match ev.zone_type {
        ZoneType::Pen => "PEN  ",
        ZoneType::Orf => "ORF  ",
        ZoneType::Touch => "TOUCH",
        ZoneType::Dgb => "DGB  ",
    };
    let tps = if ev.is_tps { "[TPS]" } else { "     " };
    let bar = level_bar(ev.level, 20);
    println!("{kind} {tps} {:<28} {bar} {:.3}", ev.id, ev.level);
}

// Helpers
fn zone_type_str(t: &ZoneType) -> &'static str {
    match t {
        ZoneType::Pen => "Pen",
        ZoneType::Orf => "Orf",
        ZoneType::Touch => "Touch",
        ZoneType::Dgb => "DGB",
    }
}

fn parse_port_arg() -> Option<u16> {
    let args: Vec<String> = std::env::args().collect();
    let idx = args.iter().position(|a| a == "--port")?;
    args.get(idx + 1)?.parse().ok()
}

fn level_bar(level: f32, width: usize) -> String {
    let filled = ((level * width as f32).round() as usize).min(width);
    std::iter::repeat('#')
        .take(filled)
        .chain(std::iter::repeat('.').take(width - filled))
        .collect::<String>()
        .chars()
        .fold(String::from("["), |mut s, c| {
            s.push(c);
            s
        })
        + "]"
}
