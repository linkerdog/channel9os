use anyhow::Result;
use esp_idf_hal::i2c::I2cDriver;

const ES8311_ADDR: u8 = 0x18;
const ES8311_REG_RESET: u8 = 0x00;
const I2C_TIMEOUT_TICKS: u32 = 50;

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

const SPEAKER_DISABLE_SEQUENCE: &[(u8, u8)] = &[];

const MICROPHONE_ENABLE_SEQUENCE: &[(u8, u8)] = &[
    (0x00, 0x80),
    (0x01, 0xBA),
    (0x02, 0x18),
    (0x0D, 0x01),
    (0x0E, 0x02),
    (0x14, 0x10),
    (0x17, 0xBF),
    (0x1C, 0x6A),
];

const MICROPHONE_DISABLE_SEQUENCE: &[(u8, u8)] = &[(0x0D, 0xFC), (0x0E, 0x6A), (0x00, 0x00)];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioStatus {
    pub codec_present: bool,
    pub codec_name: &'static str,
    pub codec_address: Option<u8>,
    pub speaker_ready: bool,
    pub microphone_ready: bool,
}

impl AudioStatus {
    pub const fn missing() -> Self {
        Self {
            codec_present: false,
            codec_name: "ES8311",
            codec_address: None,
            speaker_ready: false,
            microphone_ready: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Es8311Codec {
    speaker_ready: bool,
    microphone_ready: bool,
}

impl Es8311Codec {
    pub fn probe(i2c: &mut I2cDriver<'static>) -> Result<Self> {
        let _ = read_reg(i2c, ES8311_REG_RESET)?;
        Ok(Self {
            speaker_ready: false,
            microphone_ready: false,
        })
    }

    pub fn enable_speaker(&mut self, i2c: &mut I2cDriver<'static>) -> Result<()> {
        write_sequence(i2c, SPEAKER_ENABLE_SEQUENCE)?;
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
        self.microphone_ready = true;
        Ok(())
    }

    pub fn disable_microphone(&mut self, i2c: &mut I2cDriver<'static>) -> Result<()> {
        write_sequence(i2c, MICROPHONE_DISABLE_SEQUENCE)?;
        self.microphone_ready = false;
        Ok(())
    }

    pub fn status(&self) -> AudioStatus {
        AudioStatus {
            codec_present: true,
            codec_name: "ES8311",
            codec_address: Some(ES8311_ADDR),
            speaker_ready: self.speaker_ready,
            microphone_ready: self.microphone_ready,
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
        i2c.write(ES8311_ADDR, &[*reg, *value], I2C_TIMEOUT_TICKS)
            .map_err(|err| anyhow::anyhow!("es8311 write 0x{reg:02x} failed: {err:?}"))?;
    }
    Ok(())
}
