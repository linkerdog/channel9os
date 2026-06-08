use anyhow::Result;
use esp_idf_hal::i2c::I2cDriver;

const BMI270_ADDR_PRIMARY: u8 = 0x68;
const BMI270_ADDR_SECONDARY: u8 = 0x69;
const BMI270_REG_CHIP_ID: u8 = 0x00;
const BMI270_CHIP_ID: u8 = 0x24;
const I2C_TIMEOUT_TICKS: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImuStatus {
    pub present: bool,
    pub name: &'static str,
    pub address: Option<u8>,
    pub chip_id: Option<u8>,
}

impl ImuStatus {
    pub const fn missing() -> Self {
        Self {
            present: false,
            name: "BMI270",
            address: None,
            chip_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bmi270 {
    address: u8,
    chip_id: u8,
}

impl Bmi270 {
    pub fn probe(i2c: &mut I2cDriver<'static>) -> Result<Self> {
        let mut last_error = None;
        for address in [BMI270_ADDR_PRIMARY, BMI270_ADDR_SECONDARY] {
            match read_reg(i2c, address, BMI270_REG_CHIP_ID) {
                Ok(chip_id) if chip_id == BMI270_CHIP_ID => return Ok(Self { address, chip_id }),
                Ok(chip_id) => {
                    last_error = Some(anyhow::anyhow!(
                        "bmi270 unexpected chip id 0x{chip_id:02x} at 0x{address:02x}"
                    ));
                }
                Err(err) => last_error = Some(err),
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("bmi270 not found")))
    }

    pub fn status(&self) -> ImuStatus {
        ImuStatus {
            present: true,
            name: "BMI270",
            address: Some(self.address),
            chip_id: Some(self.chip_id),
        }
    }
}

fn read_reg(i2c: &mut I2cDriver<'static>, address: u8, reg: u8) -> Result<u8> {
    let mut value = [0_u8; 1];
    i2c.write_read(address, &[reg], &mut value, I2C_TIMEOUT_TICKS)
        .map_err(|err| anyhow::anyhow!("bmi270 read 0x{reg:02x} failed: {err:?}"))?;
    Ok(value[0])
}
