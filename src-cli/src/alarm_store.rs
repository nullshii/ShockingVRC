use shocking_vrc_core::cli::AlarmConfig;

pub const ALARM_FILE: &str = "alarm.json";

pub fn load_alarm() -> AlarmConfig {
    match std::fs::read_to_string(ALARM_FILE) {
        Ok(json) => match serde_json::from_str::<AlarmConfig>(&json) {
            Ok(mut cfg) => {
                cfg.sanitise();
                log::info!(
                    "[alarm] Loaded {ALARM_FILE} — {} ({})",
                    cfg.time_label(),
                    if cfg.enabled { "armed" } else { "off" }
                );
                cfg
            }
            Err(e) => {
                log::warn!("[alarm] Failed to parse {ALARM_FILE}: {e} — using defaults");
                AlarmConfig::default()
            }
        },
        Err(_) => AlarmConfig::default(),
    }
}

pub fn save_alarm(cfg: &AlarmConfig) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(cfg)?;
    std::fs::write(ALARM_FILE, json)?;
    Ok(())
}
