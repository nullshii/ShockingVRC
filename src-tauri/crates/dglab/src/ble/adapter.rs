use btleplug::api::{Central, Manager as _};
use btleplug::platform::Manager;

use crate::error::{DGLabError, Result};

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
    adapter
        .start_scan(btleplug::api::ScanFilter::default())
        .await?;
    Ok(())
}

pub async fn stop_scan(adapter: &btleplug::platform::Adapter) -> Result<()> {
    adapter.stop_scan().await?;
    Ok(())
}
