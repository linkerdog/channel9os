use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use esp_idf_hal::modem::BluetoothModem;
use esp_idf_svc::bt::ble::gap::{AdvConfiguration, BleGapEvent, EspBleGap};
use esp_idf_svc::bt::{Ble, BtDriver, BtStatus};
use esp_idf_svc::nvs::EspDefaultNvsPartition;

const DEVICE_NAME: &str = "Channel9";

type BleDriver = BtDriver<'static, Ble>;
type BleGap = Arc<EspBleGap<'static, Ble, Arc<BleDriver>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BleStatus {
    Idle,
    Ready,
    Advertising,
    Failed,
}

#[derive(Debug)]
struct BleState {
    status: BleStatus,
    last_error: Option<String>,
}

pub struct Channel9Ble {
    _driver: Arc<BleDriver>,
    gap: BleGap,
    state: Arc<Mutex<BleState>>,
}

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

fn set_shared_status(state: &Arc<Mutex<BleState>>, status: BleStatus, last_error: Option<String>) {
    if let Ok(mut state) = state.lock() {
        state.status = status;
        state.last_error = last_error;
    }
}
