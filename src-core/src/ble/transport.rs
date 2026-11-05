use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use btleplug::api::{CharPropFlags, Characteristic, Peripheral as _, WriteType};
use btleplug::platform::Peripheral;
use log::{debug, error, info, warn};
use tokio::sync::{Mutex, watch};
use tokio::time::{MissedTickBehavior, interval};

use crate::error::{DGLabError, Result};
use crate::protocol::{WaveformBF, WaveformV3};

const WAVEFORM_INTERVAL: Duration = Duration::from_millis(100);

pub(super) trait DeviceIo: Send + Sync {
    fn connected(&self) -> impl Future<Output = bool> + Send;
    fn write<'a>(
        &'a self,
        characteristic: &'a Characteristic,
        data: &'a [u8],
        mode: WriteType,
    ) -> impl Future<Output = Result<()>> + Send + 'a;
}

impl DeviceIo for Peripheral {
    async fn connected(&self) -> bool {
        self.is_connected().await.unwrap_or(false)
    }

    async fn write<'a>(&'a self, characteristic: &'a Characteristic, data: &'a [u8], mode: WriteType) -> Result<()> {
        btleplug::api::Peripheral::write(self, characteristic, data, mode)
            .await
            .map_err(|e| DGLabError::WriteError(e.to_string()))
    }
}

pub(super) async fn write_packet(io: &impl DeviceIo, ch: &Characteristic, data: &[u8]) -> Result<bool> {
    let with_response = ch.properties.contains(CharPropFlags::WRITE);
    let without_response = ch.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE);
    let mode = if with_response {
        WriteType::WithResponse
    } else if without_response {
        WriteType::WithoutResponse
    } else {
        return Err(DGLabError::WriteError(format!(
            "Characteristic {} has no write property (props: {:?})",
            ch.uuid, ch.properties
        )));
    };

    debug!("Writing {} bytes to {} ({mode:?}): {data:02X?}", data.len(), ch.uuid);
    match io.write(ch, data, mode).await {
        Ok(()) => Ok(true),
        Err(first) if with_response && without_response => {
            warn!("WriteWithResponse failed ({first}), retrying with WriteWithoutResponse");
            io.write(ch, data, WriteType::WithoutResponse)
                .await
                .map_err(|second| DGLabError::WriteError(format!("both write modes failed: {first}, {second}")))?;
            Ok(true)
        }
        Err(e) => Err(e),
    }
}

pub(super) async fn initialize_output(io: &impl DeviceIo, ch: &Characteristic) -> Result<()> {
    write_packet(io, ch, &WaveformBF::symmetric(0).to_bytes()).await?;
    write_packet(io, ch, &WaveformV3::default().to_bytes()).await?;
    Ok(())
}

pub(super) async fn run_output(
    io: impl DeviceIo,
    ch: Characteristic,
    wave_now: Arc<Mutex<WaveformV3>>,
    write_lock: Arc<Mutex<()>>,
    mut stop: watch::Receiver<bool>,
) {
    info!("[dglab] Waveform output loop started");
    let mut ticks = interval(WAVEFORM_INTERVAL);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_log = String::new();

    loop {
        tokio::select! {
            biased;
            _ = stop.changed() => break,
            _ = ticks.tick() => {},
        }
        if *stop.borrow() {
            break;
        }
        if !io.connected().await {
            warn!("[dglab] Device disconnected, stopping output loop");
            break;
        }

        let _guard = tokio::select! {
            biased;
            _ = stop.changed() => break,
            guard = write_lock.lock() => guard,
        };
        if *stop.borrow() {
            break;
        }
        let wave = tokio::select! {
            biased;
            _ = stop.changed() => break,
            wave = wave_now.lock() => wave.clone(),
        };
        ticks.reset();
        match write_packet(&io, &ch, &wave.to_bytes()).await {
            Ok(_) => {
                let desc = format!("{wave}");
                if desc != last_log {
                    debug!("Sent waveform: {desc}");
                    last_log = desc;
                }
            }
            Err(e) => error!("Waveform write failed: {e}"),
        }
    }
    info!("[dglab] Waveform output loop ended");
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Mutex as StdMutex;

    use tokio::time::Instant;

    use super::*;
    use crate::ble::constants::{CHARACTERISTIC_WRITE, SERVICE_WRITE};

    #[derive(Default)]
    struct FakeIo {
        writes: StdMutex<Vec<(Instant, Vec<u8>, bool)>>,
        fail_with_response: bool,
        fail_all: bool,
        latency: Duration,
    }

    impl DeviceIo for Arc<FakeIo> {
        async fn connected(&self) -> bool {
            true
        }

        async fn write<'a>(&'a self, _: &'a Characteristic, data: &'a [u8], mode: WriteType) -> Result<()> {
            let with_response = matches!(mode, WriteType::WithResponse);
            self.writes
                .lock()
                .unwrap()
                .push((Instant::now(), data.to_vec(), with_response));
            tokio::time::sleep(self.latency).await;
            if self.fail_all || (with_response && self.fail_with_response) {
                Err(DGLabError::WriteError("simulated ATT failure".into()))
            } else {
                Ok(())
            }
        }
    }

    fn characteristic(properties: CharPropFlags) -> Characteristic {
        Characteristic {
            uuid: CHARACTERISTIC_WRITE,
            service_uuid: SERVICE_WRITE,
            properties,
            descriptors: BTreeSet::new(),
        }
    }

    #[tokio::test]
    async fn initialization_needs_no_bf_reply_on_either_write_capability() {
        for properties in [CharPropFlags::WRITE, CharPropFlags::WRITE_WITHOUT_RESPONSE] {
            let io = Arc::new(FakeIo::default());
            initialize_output(&io, &characteristic(properties)).await.unwrap();
            let writes = io.writes.lock().unwrap();
            assert_eq!(writes.len(), 2);
            assert_eq!(writes[0].1, [0xBF, 0, 0, 0, 0, 0, 0]);
            assert_eq!(&writes[1].1[..4], &[0xB0, 0x0F, 0, 0]);
            assert_eq!(&writes[1].1[8..12], &[0; 4]);
            assert_eq!(&writes[1].1[16..20], &[0; 4]);
        }
    }

    #[tokio::test]
    async fn initialization_failure_prevents_following_output() {
        let io = Arc::new(FakeIo {
            fail_all: true,
            ..FakeIo::default()
        });
        assert!(
            initialize_output(&io, &characteristic(CharPropFlags::WRITE))
                .await
                .is_err()
        );
        assert_eq!(io.writes.lock().unwrap().len(), 1, "B0 must not follow a failed BF");
    }

    #[tokio::test]
    async fn write_fallback_respects_advertised_properties() {
        for properties in [
            CharPropFlags::WRITE,
            CharPropFlags::WRITE_WITHOUT_RESPONSE,
            CharPropFlags::WRITE | CharPropFlags::WRITE_WITHOUT_RESPONSE,
            CharPropFlags::READ,
        ] {
            let io = Arc::new(FakeIo {
                fail_with_response: true,
                ..FakeIo::default()
            });
            let result = write_packet(&io, &characteristic(properties), &[0xBF; 7]).await;
            let writes = io.writes.lock().unwrap();
            if properties == CharPropFlags::READ {
                assert!(result.is_err());
                assert!(writes.is_empty());
            } else if properties == CharPropFlags::WRITE {
                assert!(result.is_err());
                assert_eq!(writes.len(), 1, "never try an unsupported write mode");
            } else {
                assert!(result.is_ok());
                assert_eq!(
                    writes.len(),
                    if properties.contains(CharPropFlags::WRITE) {
                        2
                    } else {
                        1
                    }
                );
                assert!(!writes.last().unwrap().2);
            }
        }
    }

    #[tokio::test]
    async fn both_advertised_write_modes_failing_returns_an_error() {
        let io = Arc::new(FakeIo {
            fail_all: true,
            ..FakeIo::default()
        });
        let ch = characteristic(CharPropFlags::WRITE | CharPropFlags::WRITE_WITHOUT_RESPONSE);
        let result = write_packet(&io, &ch, &WaveformBF::symmetric(20).to_bytes()).await;
        assert!(result.unwrap_err().to_string().contains("both write modes failed"));
        let writes = io.writes.lock().unwrap();
        assert_eq!(writes.len(), 2);
        assert!(writes[0].2);
        assert!(!writes[1].2);
    }

    #[tokio::test(start_paused = true)]
    async fn output_uses_100ms_cadence_including_write_time_and_stops() {
        for properties in [CharPropFlags::WRITE, CharPropFlags::WRITE_WITHOUT_RESPONSE] {
            let io = Arc::new(FakeIo {
                latency: Duration::from_millis(20),
                ..FakeIo::default()
            });
            let (stop_tx, stop_rx) = watch::channel(false);
            let task = tokio::spawn(run_output(
                Arc::clone(&io),
                characteristic(properties),
                Arc::new(Mutex::new(WaveformV3::new(20, 30, [100; 4], [50; 4], [200; 4], [60; 4]))),
                Arc::new(Mutex::new(())),
                stop_rx,
            ));
            tokio::time::sleep(Duration::from_millis(350)).await;
            stop_tx.send(true).unwrap();
            task.await.unwrap();
            let writes = io.writes.lock().unwrap();
            assert_eq!(writes.len(), 4);
            for pair in writes.windows(2) {
                assert_eq!(pair[1].0.duration_since(pair[0].0), WAVEFORM_INTERVAL);
            }
            assert!(writes.iter().all(|(_, bytes, _)| bytes.len() == 20 && bytes[1] == 0x0F));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn stop_cancels_a_frame_waiting_behind_another_write() {
        let io = Arc::new(FakeIo::default());
        let lock = Arc::new(Mutex::new(()));
        let guard = lock.lock().await;
        let (stop_tx, stop_rx) = watch::channel(false);
        let task = tokio::spawn(run_output(
            Arc::clone(&io),
            characteristic(CharPropFlags::WRITE),
            Arc::new(Mutex::new(WaveformV3::default())),
            Arc::clone(&lock),
            stop_rx,
        ));
        tokio::task::yield_now().await;
        stop_tx.send(true).unwrap();
        tokio::time::timeout(Duration::from_millis(10), task)
            .await
            .unwrap()
            .unwrap();
        assert!(io.writes.lock().unwrap().is_empty());
        drop(guard);
    }

    #[tokio::test(start_paused = true)]
    async fn a_delayed_write_slot_does_not_create_a_catch_up_burst() {
        let io = Arc::new(FakeIo::default());
        let lock = Arc::new(Mutex::new(()));
        let guard = lock.lock().await;
        let (stop_tx, stop_rx) = watch::channel(false);
        let task = tokio::spawn(run_output(
            Arc::clone(&io),
            characteristic(CharPropFlags::WRITE),
            Arc::new(Mutex::new(WaveformV3::default())),
            Arc::clone(&lock),
            stop_rx,
        ));
        tokio::time::sleep(Duration::from_millis(180)).await;
        drop(guard);
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(250)).await;
        stop_tx.send(true).unwrap();
        task.await.unwrap();
        let writes = io.writes.lock().unwrap();
        assert_eq!(writes.len(), 3);
        for pair in writes.windows(2) {
            assert_eq!(pair[1].0.duration_since(pair[0].0), WAVEFORM_INTERVAL);
        }
    }
}
