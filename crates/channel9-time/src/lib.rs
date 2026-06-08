use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use channel9_core::TimeConfig;
use esp_idf_svc::sntp::{EspSntp, SntpConf, SyncStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeSyncStatus {
    Disabled,
    Idle,
    InProgress,
    Synced,
    Failed,
}

pub struct Channel9Time {
    sntp: Option<EspSntp<'static>>,
    status: TimeSyncStatus,
    last_error: Option<String>,
}

impl Channel9Time {
    pub fn new() -> Self {
        Self {
            sntp: None,
            status: TimeSyncStatus::Idle,
            last_error: None,
        }
    }

    pub fn start(&mut self, config: &TimeConfig) -> Result<()> {
        self.sntp.take();

        if !config.sync_at_startup {
            self.status = TimeSyncStatus::Disabled;
            self.last_error = None;
            return Ok(());
        }

        let mut conf = SntpConf::default();
        conf.servers[0] = config.sntp_server.as_str();

        let sntp = EspSntp::new(&conf).context("failed to start sntp")?;
        self.sntp = Some(sntp);
        self.status = TimeSyncStatus::InProgress;
        self.last_error = None;
        Ok(())
    }

    pub fn sync_now(&mut self, config: &TimeConfig) -> Result<()> {
        self.start(config)
    }

    pub fn status(&mut self) -> TimeSyncStatus {
        if let Some(sntp) = self.sntp.as_ref() {
            self.status = match sntp.get_sync_status() {
                SyncStatus::Reset => TimeSyncStatus::InProgress,
                SyncStatus::InProgress => TimeSyncStatus::InProgress,
                SyncStatus::Completed => TimeSyncStatus::Synced,
            };
        }
        self.status
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

impl Default for Channel9Time {
    fn default() -> Self {
        Self::new()
    }
}

pub fn format_clock(offset_minutes: i32) -> heapless::String<6> {
    let mut value = heapless::String::<6>::new();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let local = now + offset_minutes as i64 * 60;
    let seconds_in_day = local.rem_euclid(86_400);
    let hour = seconds_in_day / 3_600;
    let minute = seconds_in_day % 3_600 / 60;
    let _ = core::fmt::write(&mut value, format_args!("{hour:02}:{minute:02}"));
    value
}
