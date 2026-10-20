use std::sync::Arc;
use std::time::Duration;

use shocking_vrc_app::runtime::{self, SessionOptions};
use shocking_vrc_cli::{tui, tui_logger};

#[tokio::main]
async fn main() {
    shocking_vrc_core::update::set_flavor(shocking_vrc_core::update::Flavor::Cli);
    shocking_vrc_core::update::cleanup_staging_files();

    let log_buffer = tui_logger::init("info");
    let opts = SessionOptions::from_args("Usage: shockingvrc-cli [--port <n>] [--scan-timeout <secs>] [--skip-update-check]");

    log::info!(
        "ShockingVRC CLI v{} — Hello DG-Lab Users",
        shocking_vrc_core::VERSION
    );

    let state = runtime::start_session(&opts).await;
    log::info!("[cli] Ready.");

    let pending_update =
        match tui::run(Arc::clone(&state), log_buffer, opts.skip_update_check).await {
            Ok(pending) => pending,
            Err(e) => {
                eprintln!("[cli] TUI error: {e}");
                None
            }
        };

    runtime::shutdown_session(&state).await;

    if let Some((downloaded, restart_args)) = pending_update {
        match shocking_vrc_core::update::apply_self_update(&downloaded, &restart_args) {
            Ok(()) => {
                let _ = std::fs::remove_file(&downloaded);
                return;
            }
            Err(e) => eprintln!("[update] install failed: {e}"),
        }
    }

    tokio::time::sleep(Duration::from_millis(150)).await;
    println!("[cli] Stopped. Goodbye.");
}
