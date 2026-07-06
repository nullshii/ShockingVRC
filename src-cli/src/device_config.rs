use shocking_vrc_core::cli::CliConfig;

const DEVICES_DIR: &str = "devices";

pub fn mac_to_filename(mac: &str) -> String {
    mac.to_uppercase().replace(':', "")
}

pub fn device_config_path(mac: &str) -> String {
    format!("{}/{}.json", DEVICES_DIR, mac_to_filename(mac))
}

pub fn load_device_config(mac: &str, default: &CliConfig) -> CliConfig {
    let path = device_config_path(mac);
    match std::fs::read_to_string(&path) {
        Ok(json) => match serde_json::from_str(&json) {
            Ok(cfg) => {
                log::info!("[config] Loaded device config from {path}");
                cfg
            }
            Err(e) => {
                log::warn!("[config] Failed to parse {path}: {e} — using default");
                default.clone()
            }
        },
        Err(_) => {
            log::info!("[config] No config for {mac} — using default");
            default.clone()
        }
    }
}

pub fn save_device_config(mac: &str, config: &CliConfig) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(DEVICES_DIR)?;
    let path = device_config_path(mac);
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, json)?;
    Ok(())
}
