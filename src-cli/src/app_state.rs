use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use shocking_vrc_core::{AvatarScanner, CliEngine};

pub struct AppState {
    pub engine: CliEngine,
    pub scanner: AvatarScanner,
    pub monitor_enabled: Arc<AtomicBool>,
}
