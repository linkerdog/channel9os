use serde::{Deserialize, Serialize};

use crate::device::{DeviceId, SdCardPins};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    pub device: DeviceId,
    pub storage: StorageConfig,
    pub wifi: WifiConfig,
    #[serde(default)]
    pub time: TimeConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageConfig {
    pub prefer_sdcard: bool,
    pub sdcard_mount_path: String,
    pub sdcard_pins: SdCardPins,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiConfig {
    pub connect_at_startup: bool,
    pub credentials: Vec<WifiCredential>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiCredential {
    pub ssid: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeConfig {
    pub sync_at_startup: bool,
    pub sntp_server: String,
    pub timezone_offset_minutes: i32,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            sync_at_startup: true,
            sntp_server: "ntp.tuna.tsinghua.edu.cn".to_owned(),
            timezone_offset_minutes: 8 * 60,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            device: DeviceId::CardputerAdv,
            storage: StorageConfig {
                prefer_sdcard: true,
                sdcard_mount_path: "/sdcard".to_owned(),
                sdcard_pins: SdCardPins::cardputer_adv(),
            },
            wifi: WifiConfig {
                connect_at_startup: false,
                credentials: Vec::new(),
            },
            time: TimeConfig::default(),
        }
    }
}
