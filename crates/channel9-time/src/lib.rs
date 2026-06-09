use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use channel9_core::TimeConfig;
use esp_idf_svc::sntp::{EspSntp, SntpConf, SyncStatus};

const SNTP_SYNC_TIMEOUT: Duration = Duration::from_secs(20);

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
    sync_started_at: Option<Instant>,
    last_error: Option<String>,
}

impl Channel9Time {
    pub fn new() -> Self {
        Self {
            sntp: None,
            status: TimeSyncStatus::Idle,
            sync_started_at: None,
            last_error: None,
        }
    }

    pub fn start(&mut self, config: &TimeConfig) -> Result<()> {
        if !config.sync_at_startup {
            self.sntp.take();
            self.status = TimeSyncStatus::Disabled;
            self.sync_started_at = None;
            self.last_error = None;
            return Ok(());
        }

        self.start_sync(config)
    }

    pub fn sync_now(&mut self, config: &TimeConfig) -> Result<()> {
        self.start_sync(config)
    }

    fn start_sync(&mut self, config: &TimeConfig) -> Result<()> {
        self.sntp.take();

        let mut conf = SntpConf::default();
        conf.servers[0] = config.sntp_server.as_str();

        let sntp = match EspSntp::new(&conf).context("failed to start sntp") {
            Ok(sntp) => sntp,
            Err(err) => {
                self.status = TimeSyncStatus::Failed;
                self.sync_started_at = None;
                self.last_error = Some(err.to_string());
                return Err(err);
            }
        };
        self.sntp = Some(sntp);
        self.status = TimeSyncStatus::InProgress;
        self.sync_started_at = Some(Instant::now());
        self.last_error = None;
        Ok(())
    }

    pub fn status(&mut self) -> TimeSyncStatus {
        if let Some(sntp) = self.sntp.as_ref() {
            self.status = match sntp.get_sync_status() {
                SyncStatus::Reset => TimeSyncStatus::InProgress,
                SyncStatus::InProgress => TimeSyncStatus::InProgress,
                SyncStatus::Completed => {
                    self.sync_started_at = None;
                    self.last_error = None;
                    TimeSyncStatus::Synced
                }
            };
        }
        if self.status == TimeSyncStatus::InProgress
            && self
                .sync_started_at
                .map(|started_at| started_at.elapsed() >= SNTP_SYNC_TIMEOUT)
                .unwrap_or(false)
        {
            self.sntp.take();
            self.sync_started_at = None;
            self.status = TimeSyncStatus::Failed;
            self.last_error = Some("sntp sync timeout".to_owned());
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
