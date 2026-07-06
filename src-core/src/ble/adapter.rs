use btleplug::api::{Central, Manager as _};
use btleplug::platform::Manager;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::{DGLabError, Result};

static SCAN_LOCK: std::sync::OnceLock<Arc<Mutex<()>>> = std::sync::OnceLock::new();

fn scan_lock() -> &'static Arc<Mutex<()>> {
    SCAN_LOCK.get_or_init(|| Arc::new(Mutex::new(())))
}

pub async fn with_scan_lock<F, Fut, T>(f: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let _guard = scan_lock().lock().await;
    f().await
}

pub async fn get_adapter() -> Result<btleplug::platform::Adapter> {
    let manager = Manager::new().await?;
    manager
        .adapters()
        .await?
        .into_iter()
        .next()
        .ok_or(DGLabError::AdapterNotFound)
}

pub async fn is_bluetooth_available() -> bool {
    get_adapter().await.is_ok()
}

pub async fn start_scan(adapter: &btleplug::platform::Adapter) -> Result<()> {
    adapter.start_scan(btleplug::api::ScanFilter::default()).await?;
    Ok(())
}

pub async fn stop_scan(adapter: &btleplug::platform::Adapter) -> Result<()> {
    adapter.stop_scan().await?;
    Ok(())
}
