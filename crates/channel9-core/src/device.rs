use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceId {
    CardputerAdv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SdCardPins {
    pub cs: i32,
    pub sck: i32,
    pub miso: i32,
    pub mosi: i32,
}

impl SdCardPins {
    pub const fn cardputer_adv() -> Self {
        Self {
            cs: 12,
            sck: 40,
            miso: 39,
            mosi: 14,
        }
    }
}
