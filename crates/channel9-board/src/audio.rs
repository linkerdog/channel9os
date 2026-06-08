use anyhow::Result;
use esp_idf_hal::i2c::I2cDriver;

const ES8311_ADDR: u8 = 0x18;
const ES8311_REG_RESET: u8 = 0x00;
const ES8311_REG_SYSTEM_ADC_SELECT: u8 = 0x14;
const ES8311_REG_ADC_VOLUME: u8 = 0x17;
const ES8311_REG_ADC_ALC_ENABLE: u8 = 0x18;
const ES8311_REG_ADC_ALC_LEVEL: u8 = 0x19;
const ES8311_REG_ADC_ALC_AUTOMUTE: u8 = 0x1A;
const ES8311_REG_ADC_ALC_AUTOMUTE_CONTROL: u8 = 0x1B;
const ES8311_REG_DAC_VOLUME: u8 = 0x32;
const I2C_TIMEOUT_TICKS: u32 = 50;
const DEFAULT_SPEAKER_VOLUME_PERCENT: u8 = 75;

const SPEAKER_ENABLE_SEQUENCE: &[(u8, u8)] = &[
    (0x00, 0x80),
    (0x01, 0xB5),
    (0x02, 0x18),
    (0x0D, 0x01),
    (0x12, 0x00),
    (0x13, 0x10),
    (0x32, 0xBF),
    (0x37, 0x08),
];

const SPEAKER_DISABLE_SEQUENCE: &[(u8, u8)] = &[(ES8311_REG_DAC_VOLUME, 0x00)];

const MICROPHONE_ENABLE_SEQUENCE: &[(u8, u8)] = &[
    (0x00, 0x80),
    (0x01, 0xBA),
    (0x02, 0x18),
    (0x0D, 0x01),
    (0x0E, 0x02),
    (ES8311_REG_SYSTEM_ADC_SELECT, 0x1A),
    (ES8311_REG_ADC_VOLUME, 0xBF),
    (0x1C, 0x6A),
];

const MICROPHONE_ALC_ENABLE_SEQUENCE: &[(u8, u8)] = &[
    (ES8311_REG_ADC_ALC_ENABLE, 0x8A),
    (ES8311_REG_ADC_ALC_LEVEL, 0xC3),
    (ES8311_REG_ADC_ALC_AUTOMUTE, 0x00),
    (ES8311_REG_ADC_ALC_AUTOMUTE_CONTROL, 0x00),
];

const MICROPHONE_ALC_DISABLE_SEQUENCE: &[(u8, u8)] = &[
    (ES8311_REG_ADC_ALC_ENABLE, 0x00),
    (ES8311_REG_ADC_ALC_AUTOMUTE, 0x00),
    (ES8311_REG_ADC_ALC_AUTOMUTE_CONTROL, 0x00),
];

const MICROPHONE_DISABLE_SEQUENCE: &[(u8, u8)] = &[(0x0D, 0xFC), (0x0E, 0x6A), (0x00, 0x00)];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioStatus {
    pub codec_present: bool,
    pub codec_name: &'static str,
    pub codec_address: Option<u8>,
    pub speaker_ready: bool,
    pub microphone_ready: bool,
    pub microphone_alc_enabled: bool,
    pub speaker_volume_percent: u8,
}

impl AudioStatus {
    pub const fn missing() -> Self {
        Self {
            codec_present: false,
            codec_name: "ES8311",
            codec_address: None,
            speaker_ready: false,
            microphone_ready: false,
            microphone_alc_enabled: false,
            speaker_volume_percent: DEFAULT_SPEAKER_VOLUME_PERCENT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Es8311Codec {
    speaker_ready: bool,
    microphone_ready: bool,
    microphone_alc_enabled: bool,
    speaker_volume_percent: u8,
}

impl Es8311Codec {
    pub fn probe(i2c: &mut I2cDriver<'static>) -> Result<Self> {
        let _ = read_reg(i2c, ES8311_REG_RESET)?;
        Ok(Self {
            speaker_ready: false,
            microphone_ready: false,
            microphone_alc_enabled: false,
            speaker_volume_percent: DEFAULT_SPEAKER_VOLUME_PERCENT,
        })
    }

    pub fn enable_speaker(&mut self, i2c: &mut I2cDriver<'static>) -> Result<()> {
        write_sequence(i2c, SPEAKER_ENABLE_SEQUENCE)?;
        write_reg(
            i2c,
            ES8311_REG_DAC_VOLUME,
            speaker_volume_register(self.speaker_volume_percent),
        )?;
        self.speaker_ready = true;
        Ok(())
    }

    pub fn disable_speaker(&mut self, i2c: &mut I2cDriver<'static>) -> Result<()> {
        write_sequence(i2c, SPEAKER_DISABLE_SEQUENCE)?;
        self.speaker_ready = false;
        Ok(())
    }

    pub fn enable_microphone(&mut self, i2c: &mut I2cDriver<'static>) -> Result<()> {
        write_sequence(i2c, MICROPHONE_ENABLE_SEQUENCE)?;
        write_sequence(i2c, MICROPHONE_ALC_ENABLE_SEQUENCE)?;
        self.microphone_ready = true;
        self.microphone_alc_enabled = true;
        Ok(())
    }

    pub fn disable_microphone(&mut self, i2c: &mut I2cDriver<'static>) -> Result<()> {
        write_sequence(i2c, MICROPHONE_ALC_DISABLE_SEQUENCE)?;
        write_sequence(i2c, MICROPHONE_DISABLE_SEQUENCE)?;
        self.microphone_ready = false;
        self.microphone_alc_enabled = false;
        Ok(())
    }

    pub fn set_speaker_volume_percent(
        &mut self,
        i2c: &mut I2cDriver<'static>,
        volume_percent: u8,
    ) -> Result<()> {
        self.speaker_volume_percent = volume_percent.min(100);
        if self.speaker_ready {
            write_reg(
                i2c,
                ES8311_REG_DAC_VOLUME,
                speaker_volume_register(self.speaker_volume_percent),
            )?;
        }
        Ok(())
    }

    pub fn status(&self) -> AudioStatus {
        AudioStatus {
            codec_present: true,
            codec_name: "ES8311",
            codec_address: Some(ES8311_ADDR),
            speaker_ready: self.speaker_ready,
            microphone_ready: self.microphone_ready,
            microphone_alc_enabled: self.microphone_alc_enabled,
            speaker_volume_percent: self.speaker_volume_percent,
        }
    }
}

fn read_reg(i2c: &mut I2cDriver<'static>, reg: u8) -> Result<u8> {
    let mut value = [0_u8; 1];
    i2c.write_read(ES8311_ADDR, &[reg], &mut value, I2C_TIMEOUT_TICKS)
        .map_err(|err| anyhow::anyhow!("es8311 read 0x{reg:02x} failed: {err:?}"))?;
    Ok(value[0])
}

fn write_sequence(i2c: &mut I2cDriver<'static>, sequence: &[(u8, u8)]) -> Result<()> {
    for (reg, value) in sequence {
        write_reg(i2c, *reg, *value)?;
    }
    Ok(())
}

fn write_reg(i2c: &mut I2cDriver<'static>, reg: u8, value: u8) -> Result<()> {
    i2c.write(ES8311_ADDR, &[reg, value], I2C_TIMEOUT_TICKS)
        .map_err(|err| anyhow::anyhow!("es8311 write 0x{reg:02x} failed: {err:?}"))?;
    Ok(())
}

fn speaker_volume_register(volume_percent: u8) -> u8 {
    let value = (volume_percent.min(100) as u16 * u8::MAX as u16) / 100;
    value as u8
}
