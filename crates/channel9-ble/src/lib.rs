#[cfg(esp_idf_bt_enabled)]
use std::sync::{Arc, Mutex};

#[cfg(esp_idf_bt_enabled)]
use anyhow::Context;
use anyhow::Result;
use esp_idf_hal::modem::BluetoothModem;
#[cfg(esp_idf_bt_enabled)]
use esp_idf_svc::bt::ble::gap::{AdvConfiguration, BleGapEvent, EspBleGap};
#[cfg(esp_idf_bt_enabled)]
use esp_idf_svc::bt::{Ble, BtDriver, BtStatus};
#[cfg(esp_idf_bt_enabled)]
use esp_idf_svc::nvs::EspDefaultNvsPartition;

#[cfg(esp_idf_bt_enabled)]
const DEVICE_NAME: &str = "Channel9";

#[cfg(esp_idf_bt_enabled)]
type BleDriver = BtDriver<'static, Ble>;
#[cfg(esp_idf_bt_enabled)]
type BleGap = Arc<EspBleGap<'static, Ble, Arc<BleDriver>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BleStatus {
    Idle,
    Ready,
    Advertising,
    Failed,
}

#[cfg(esp_idf_bt_enabled)]
#[derive(Debug)]
struct BleState {
    status: BleStatus,
    last_error: Option<String>,
}

#[cfg(esp_idf_bt_enabled)]
pub struct Channel9Ble {
    _driver: Arc<BleDriver>,
    gap: BleGap,
    state: Arc<Mutex<BleState>>,
}

#[cfg(not(esp_idf_bt_enabled))]
pub struct Channel9Ble;

#[cfg(esp_idf_bt_enabled)]
impl Channel9Ble {
    pub fn new(modem: BluetoothModem<'static>) -> Result<Self> {
        let nvs = EspDefaultNvsPartition::take().ok();
        let driver = Arc::new(BtDriver::new(modem, nvs).context("failed to initialize bt")?);
        let gap = Arc::new(EspBleGap::new(driver.clone()).context("failed to initialize ble gap")?);
        let state = Arc::new(Mutex::new(BleState {
            status: BleStatus::Ready,
            last_error: None,
        }));

        let state_for_gap = state.clone();
        let gap_for_gap = gap.clone();
        gap.subscribe(move |event| {
            handle_gap_event(&gap_for_gap, &state_for_gap, event);
        })?;

        Ok(Self {
            _driver: driver,
            gap,
            state,
        })
    }

    pub fn start_advertising(&self) -> Result<()> {
        self.gap
            .set_device_name(DEVICE_NAME)
            .context("failed to set ble device name")?;
        self.gap
            .set_adv_conf(&AdvConfiguration {
                include_name: true,
                flag: 2,
                ..Default::default()
            })
            .context("failed to configure ble advertising")?;

        self.set_status(BleStatus::Ready, None);
        Ok(())
    }

    pub fn stop_advertising(&self) -> Result<()> {
        if self.status() == BleStatus::Advertising {
            self.gap
                .stop_advertising()
                .context("failed to stop ble advertising")?;
        }
        self.set_status(BleStatus::Ready, None);
        Ok(())
    }

    pub fn status(&self) -> BleStatus {
        self.state
            .lock()
            .map(|state| state.status)
            .unwrap_or(BleStatus::Failed)
    }

    pub fn last_error(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.last_error.clone())
    }

    fn set_status(&self, status: BleStatus, last_error: Option<String>) {
        if let Ok(mut state) = self.state.lock() {
            state.status = status;
            state.last_error = last_error;
        }
    }
}

#[cfg(not(esp_idf_bt_enabled))]
impl Channel9Ble {
    pub fn new(_modem: BluetoothModem<'static>) -> Result<Self> {
        Ok(Self)
    }

    pub fn start_advertising(&self) -> Result<()> {
        Ok(())
    }

    pub fn stop_advertising(&self) -> Result<()> {
        Ok(())
    }

    pub fn status(&self) -> BleStatus {
        BleStatus::Idle
    }

    pub fn last_error(&self) -> Option<String> {
        None
    }
}

#[cfg(esp_idf_bt_enabled)]
fn handle_gap_event(gap: &BleGap, state: &Arc<Mutex<BleState>>, event: BleGapEvent<'_>) {
    match event {
        BleGapEvent::AdvertisingConfigured(BtStatus::Success) => {
            if let Err(err) = gap.start_advertising() {
                set_shared_status(state, BleStatus::Failed, Some(format!("{err:?}")));
            }
        }
        BleGapEvent::AdvertisingConfigured(status) => {
            set_shared_status(
                state,
                BleStatus::Failed,
                Some(format!("advertising config failed: {status:?}")),
            );
        }
        BleGapEvent::AdvertisingStarted(BtStatus::Success) => {
            set_shared_status(state, BleStatus::Advertising, None);
        }
        BleGapEvent::AdvertisingStarted(status) => {
            set_shared_status(
                state,
                BleStatus::Failed,
                Some(format!("advertising start failed: {status:?}")),
            );
        }
        BleGapEvent::AdvertisingStopped(BtStatus::Success) => {
            set_shared_status(state, BleStatus::Ready, None);
        }
        BleGapEvent::AdvertisingStopped(status) => {
            set_shared_status(
                state,
                BleStatus::Failed,
                Some(format!("advertising stop failed: {status:?}")),
            );
        }
        _ => {}
    }
}

#[cfg(esp_idf_bt_enabled)]
fn set_shared_status(state: &Arc<Mutex<BleState>>, status: BleStatus, last_error: Option<String>) {
    if let Ok(mut state) = state.lock() {
        state.status = status;
        state.last_error = last_error;
    }
}
