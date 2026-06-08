use anyhow::Result;
use esp_idf_hal::i2c::I2cDriver;

const TCA8418_ADDR: u8 = 0x34;
const REG_CFG: u8 = 0x01;
const REG_INT_STAT: u8 = 0x02;
const REG_KEY_LCK_EC: u8 = 0x03;
const REG_KEY_EVENT_A: u8 = 0x04;
const REG_KP_GPIO1: u8 = 0x1D;
const REG_KP_GPIO2: u8 = 0x1E;
const REG_KP_GPIO3: u8 = 0x1F;
const REG_DEBOUNCE_DIS1: u8 = 0x29;
const REG_DEBOUNCE_DIS2: u8 = 0x2A;
const REG_DEBOUNCE_DIS3: u8 = 0x2B;

const CFG_KEY_INT: u8 = 0x01;
const INT_STAT_KEY_EVENT: u8 = 0x01;
const KEY_EVENT_PRESSED: u8 = 0x80;
const KEY_EVENT_VALUE_MASK: u8 = 0x7F;
const KEY_EVENT_COUNT_MASK: u8 = 0x0F;
const I2C_TIMEOUT_TICKS: u32 = 50;

const KEY_BACKSPACE: u8 = 0x2A;
const KEY_ENTER: u8 = 0x28;
const KEY_LEFT_SHIFT: u8 = 0x81;
const KEY_FN: u8 = 0xFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Select,
    Back,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Char(char),
}

pub struct Tca8418Keyboard {
    fn_pressed: bool,
    shift_pressed: bool,
}

impl Tca8418Keyboard {
    pub fn new(i2c: &mut I2cDriver<'static>) -> Result<Self> {
        let mut keyboard = Self {
            fn_pressed: false,
            shift_pressed: false,
        };
        keyboard.configure(i2c)?;
        Ok(keyboard)
    }

    pub fn poll(&mut self, i2c: &mut I2cDriver<'static>) -> Result<Option<InputEvent>> {
        let count = self.read_reg(i2c, REG_KEY_LCK_EC)? & KEY_EVENT_COUNT_MASK;
        if count == 0 {
            return Ok(None);
        }

        let mut selected = None;
        for _ in 0..count {
            let event = self.read_reg(i2c, REG_KEY_EVENT_A)?;
            let pressed = event & KEY_EVENT_PRESSED != 0;
            let value = event & KEY_EVENT_VALUE_MASK;
            if let Some(input) = self.map_key_event(value, pressed) {
                selected = Some(input);
            }
        }
        self.write_reg(i2c, REG_INT_STAT, INT_STAT_KEY_EVENT)?;
        Ok(selected)
    }

    fn configure(&mut self, i2c: &mut I2cDriver<'static>) -> Result<()> {
        self.write_reg(i2c, REG_CFG, CFG_KEY_INT)?;
        self.write_reg(i2c, REG_KP_GPIO1, 0xFF)?;
        self.write_reg(i2c, REG_KP_GPIO2, 0xFF)?;
        self.write_reg(i2c, REG_KP_GPIO3, 0xFF)?;
        self.write_reg(i2c, REG_DEBOUNCE_DIS1, 0x00)?;
        self.write_reg(i2c, REG_DEBOUNCE_DIS2, 0x00)?;
        self.write_reg(i2c, REG_DEBOUNCE_DIS3, 0x00)?;
        self.flush(i2c)?;
        self.write_reg(i2c, REG_INT_STAT, INT_STAT_KEY_EVENT)?;
        Ok(())
    }

    fn flush(&mut self, i2c: &mut I2cDriver<'static>) -> Result<()> {
        loop {
            let count = self.read_reg(i2c, REG_KEY_LCK_EC)? & KEY_EVENT_COUNT_MASK;
            if count == 0 {
                break;
            }
            for _ in 0..count {
                let _ = self.read_reg(i2c, REG_KEY_EVENT_A)?;
            }
        }
        Ok(())
    }

    fn read_reg(&mut self, i2c: &mut I2cDriver<'static>, reg: u8) -> Result<u8> {
        let mut value = [0_u8; 1];
        i2c.write_read(TCA8418_ADDR, &[reg], &mut value, I2C_TIMEOUT_TICKS)
            .map_err(|err| anyhow::anyhow!("tca8418 read 0x{reg:02x} failed: {err:?}"))?;
        Ok(value[0])
    }

    fn write_reg(&mut self, i2c: &mut I2cDriver<'static>, reg: u8, value: u8) -> Result<()> {
        i2c.write(TCA8418_ADDR, &[reg, value], I2C_TIMEOUT_TICKS)
            .map_err(|err| anyhow::anyhow!("tca8418 write 0x{reg:02x} failed: {err:?}"))
    }

    fn map_key_event(&mut self, value: u8, pressed: bool) -> Option<InputEvent> {
        let (row, col) = map_raw_key_to_physical(value)?;
        let key = key_value(row, col)?;

        if key == KEY_FN {
            self.fn_pressed = pressed;
            return None;
        }
        if key == KEY_LEFT_SHIFT {
            self.shift_pressed = pressed;
            return None;
        }

        if !pressed {
            return None;
        }

        match key {
            KEY_ENTER if col == 13 => Some(InputEvent::Select),
            KEY_BACKSPACE if col == 13 => Some(InputEvent::Backspace),
            b'`' if self.shift_pressed => Some(InputEvent::Char('~')),
            b'`' => Some(InputEvent::Back),
            b';' if self.shift_pressed => Some(InputEvent::Char(':')),
            b';' => Some(InputEvent::Up),
            b',' if self.shift_pressed => Some(InputEvent::Char('<')),
            b',' if self.fn_pressed => Some(InputEvent::Left),
            b',' => Some(InputEvent::Up),
            b'.' if self.shift_pressed => Some(InputEvent::Char('>')),
            b'.' => Some(InputEvent::Down),
            b'/' if self.shift_pressed => Some(InputEvent::Char('?')),
            b'/' if self.fn_pressed => Some(InputEvent::Right),
            b'/' => Some(InputEvent::Down),
            b' '..=b'~' => Some(InputEvent::Char(apply_shift(
                key as char,
                self.shift_pressed,
            ))),
            _ => None,
        }
    }
}

fn apply_shift(value: char, shift_pressed: bool) -> char {
    if !shift_pressed {
        return value;
    }

    match value {
        'a'..='z' => value.to_ascii_uppercase(),
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        '\'' => '"',
        '`' => '~',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        ';' => ':',
        _ => value,
    }
}

fn map_raw_key_to_physical(value: u8) -> Option<(usize, usize)> {
    let units = value % 10;
    let tens = value / 10;
    if !(1..=8).contains(&units) || tens > 6 {
        return None;
    }

    let unit_zero = units - 1;
    let row = (unit_zero & 0x03) as usize;
    let col = ((tens << 1) | (unit_zero >> 2)) as usize;
    Some((row, col))
}

fn key_value(row: usize, col: usize) -> Option<u8> {
    const KEY_MAP: [[u8; 14]; 4] = [
        [
            b'`',
            b'1',
            b'2',
            b'3',
            b'4',
            b'5',
            b'6',
            b'7',
            b'8',
            b'9',
            b'0',
            b'-',
            b'=',
            KEY_BACKSPACE,
        ],
        [
            0x2B, b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i', b'o', b'p', b'[', b']', b'\\',
        ],
        [
            0xFF, 0x81, b'a', b's', b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';', b'\'',
            KEY_ENTER,
        ],
        [
            0x80, 0x83, 0x82, b'z', b'x', b'c', b'v', b'b', b'n', b'm', b',', b'.', b'/', b' ',
        ],
    ];

    KEY_MAP.get(row).and_then(|cols| cols.get(col)).copied()
}
