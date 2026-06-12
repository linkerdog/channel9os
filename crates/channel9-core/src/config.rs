use serde::{Deserialize, Serialize};

use crate::device::{DeviceId, SdCardPins};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    pub device: DeviceId,
    pub storage: StorageConfig,
    pub wifi: WifiConfig,
    #[serde(default)]
    pub time: TimeConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub channel9: Channel9Config,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioConfig {
    pub speaker_volume_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel9Config {
    #[serde(default = "default_channel9_api_base_url")]
    pub api_base_url: String,
    pub workspace_id: String,
    pub device_id: String,
    #[serde(default = "default_channel9_device_description")]
    pub description: String,
    pub interfaces: Vec<String>,
    pub access_token: Option<String>,
    pub token_expires_at: Option<i64>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            speaker_volume_percent: 75,
        }
    }
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

impl Default for Channel9Config {
    fn default() -> Self {
        Self {
            api_base_url: default_channel9_api_base_url(),
            workspace_id: String::new(),
            device_id: "cardputer-adv".to_owned(),
            description: default_channel9_device_description(),
            interfaces: vec!["display".to_owned(), "speaker".to_owned()],
            access_token: None,
            token_expires_at: None,
        }
    }
}

fn default_channel9_api_base_url() -> String {
    "https://app.linkerdog.work".to_owned()
}

fn default_channel9_device_description() -> String {
    "Cardputer-Adv Channel9 device".to_owned()
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
            audio: AudioConfig::default(),
            channel9: Channel9Config::default(),
        }
    }
}
