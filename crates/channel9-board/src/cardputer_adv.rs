use anyhow::{Context, Result};
use channel9_core::{DeviceId, SdCardPins};
use display_interface_spi::SPIInterfaceNoCS;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::Text;
use embedded_hal::spi::MODE_3;
use esp_idf_hal::delay::Ets;
use esp_idf_hal::gpio::{
    AnyIOPin, Gpio12, Gpio14, Gpio39, Gpio40, Gpio41, Gpio42, Gpio43, Gpio46, Input, Output,
    PinDriver, Pins, Pull,
};
use esp_idf_hal::i2c::{config as i2c_config, I2cDriver, I2C1};
use esp_idf_hal::i2s::config::{
    Config as I2sConfig, DataBitWidth, SlotBitWidth, SlotMode, StdClkConfig, StdConfig,
    StdGpioConfig, StdSlotConfig, StdSlotMask,
};
use esp_idf_hal::i2s::{I2sBiDir, I2sDriver, I2S0};
use esp_idf_hal::ledc::{config as ledc_config, LedcDriver, LedcTimerDriver};
use esp_idf_hal::modem::Modem;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::sd::{spi::SdSpiHostDriver, SdCardConfiguration, SdCardDriver};
use esp_idf_hal::spi::{config, Dma, SpiDeviceDriver, SpiDriver, SpiDriverConfig, SPI2, SPI3};
use esp_idf_hal::units::FromValueType;
use esp_idf_svc::fs::fatfs::Fatfs;
use esp_idf_svc::io::vfs::MountedFatfs;
use mipidsi::models::ST7789;
use mipidsi::{Builder, ColorInversion, ColorOrder, Display, Orientation, TearingEffect};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::audio::{AudioStatus, Es8311Codec};
use crate::imu::{Bmi270, ImuStatus};
use crate::input::{InputEvent, Tca8418Keyboard};

const DISPLAY_SPI_BAUDRATE_MHZ: u32 = 80;
const DISPLAY_BACKLIGHT_PWM_KHZ: u32 = 12;
const DISPLAY_SIZE_WIDTH: u16 = 240;
const DISPLAY_SIZE_HEIGHT: u16 = 135;
const DISPLAY_ORIENTATION: Orientation = Orientation::Landscape(true);
const DISPLAY_COLOR_ORDER: ColorOrder = ColorOrder::Rgb;
const SDCARD_MOUNT_PATH: &str = "/sdcard";
const SDCARD_MAX_OPEN_FILES: usize = 4;
const SDCARD_DMA_BUFFER_SIZE: usize = 4096;
const I2C_BAUDRATE_KHZ: u32 = 100;
const AUDIO_SAMPLE_RATE_HZ: u32 = 16_000;
const AUDIO_RECORDING_GAIN: f32 = 1.0;
const AUDIO_DC_FILTER_SHIFT: u8 = 11;
const AUDIO_SOFT_LIMIT_THRESHOLD: i32 = 28_000;
const AUDIO_BUFFER_BYTES: usize = 2048;
const AUDIO_DMA_BUFFER_COUNT: u32 = 8;
const AUDIO_DMA_FRAMES_PER_BUFFER: u32 = 124;
const AUDIO_READ_TIMEOUT_TICKS: u32 = 100;
const AUDIO_WRITE_TIMEOUT_TICKS: u32 = 100;
const WAV_HEADER_BYTES: usize = 44;
const STARTUP_PROGRESS_WIDTH: u32 = 184;
const STARTUP_PROGRESS_HEIGHT: u32 = 10;

type DisplayInterface<'a> =
    SPIInterfaceNoCS<SpiDeviceDriver<'a, SpiDriver<'a>>, PinDriver<'a, Output>>;

pub type LcdDisplay<'a> = Display<DisplayInterface<'a>, ST7789, PinDriver<'a, Output>>;

type SdSpiDriver<'a> = SdSpiHostDriver<'a, SpiDriver<'a>>;
type SdFatfs<'a> = Fatfs<SdCardDriver<SdSpiDriver<'a>>>;
type MountedSdFatfs<'a> = MountedFatfs<SdFatfs<'a>>;

pub struct CardputerAdv {
    display: LcdDisplay<'static>,
    backlight: LedcDriver<'static>,
    _sdcard: Option<MountedSdFatfs<'static>>,
    i2c: Option<I2cDriver<'static>>,
    keyboard: Option<Tca8418Keyboard>,
    go_button: PinDriver<'static, Input>,
    go_button_pressed: bool,
    imu: Option<Bmi270>,
    audio: Option<Es8311Codec>,
    i2s: Option<I2sDriver<'static, I2sBiDir>>,
    recording: Option<VoiceRecording>,
}

pub struct CardputerAdvPeripherals {
    pub pins: Pins,
    pub ledc: esp_idf_hal::ledc::LEDC,
    pub spi2: SPI2<'static>,
    pub spi3: SPI3<'static>,
    pub i2c1: I2C1<'static>,
    pub i2s0: I2S0<'static>,
}

impl CardputerAdv {
    pub const DEVICE_ID: DeviceId = DeviceId::CardputerAdv;
    pub const SDCARD_PINS: SdCardPins = SdCardPins::cardputer_adv();
    pub const SDCARD_MOUNT_PATH: &'static str = SDCARD_MOUNT_PATH;

    pub fn split(peripherals: Peripherals) -> (CardputerAdvPeripherals, Modem<'static>) {
        (
            CardputerAdvPeripherals {
                pins: peripherals.pins,
                ledc: peripherals.ledc,
                spi2: peripherals.spi2,
                spi3: peripherals.spi3,
                i2c1: peripherals.i2c1,
                i2s0: peripherals.i2s0,
            },
            peripherals.modem,
        )
    }

    pub fn new(peripherals: CardputerAdvPeripherals) -> Result<Self> {
        let pins = peripherals.pins;

        let go_button = PinDriver::input(pins.gpio0, Pull::Up)?;
        let go_button_pressed = go_button.is_low();

        let rst = PinDriver::output(pins.gpio33)?;
        let dc = PinDriver::output(pins.gpio34)?;
        let mut backlight = LedcDriver::new(
            peripherals.ledc.channel0,
            LedcTimerDriver::new(
                peripherals.ledc.timer0,
                &ledc_config::TimerConfig::new().frequency(DISPLAY_BACKLIGHT_PWM_KHZ.kHz().into()),
            )?,
            pins.gpio38,
        )?;
        backlight.set_duty(0)?;

        let spi_config = config::Config::new()
            .baudrate(DISPLAY_SPI_BAUDRATE_MHZ.MHz().into())
            .data_mode(MODE_3);

        let spi = SpiDeviceDriver::new_single(
            peripherals.spi2,
            pins.gpio36,
            pins.gpio35,
            Option::<AnyIOPin>::None,
            Some(pins.gpio37),
            &SpiDriverConfig::new(),
            &spi_config,
        )?;

        let interface = SPIInterfaceNoCS::new(spi, dc);
        let mut delay = Ets;

        let mut display = Builder::st7789(interface)
            .with_invert_colors(ColorInversion::Inverted)
            .with_color_order(DISPLAY_COLOR_ORDER)
            .with_display_size(DISPLAY_SIZE_WIDTH, DISPLAY_SIZE_HEIGHT)
            .with_window_offset_handler(|_| (40, 53))
            .init(&mut delay, Some(rst))
            .map_err(|err| anyhow::anyhow!("display init failed: {err:?}"))?;

        display
            .set_tearing_effect(TearingEffect::Off)
            .map_err(|err| anyhow::anyhow!("tearing effect disable failed: {err:?}"))?;
        display
            .set_orientation(DISPLAY_ORIENTATION)
            .map_err(|err| anyhow::anyhow!("first orientation refresh failed: {err:?}"))?;
        display
            .set_orientation(DISPLAY_ORIENTATION)
            .map_err(|err| anyhow::anyhow!("second orientation refresh failed: {err:?}"))?;
        display
            .set_scroll_offset(0)
            .map_err(|err| anyhow::anyhow!("scroll offset reset failed: {err:?}"))?;
        backlight.set_duty(backlight.get_max_duty())?;
        draw_startup_screen(&mut display, "Display", 15)?;

        let sdcard = mount_sdcard(
            peripherals.spi3,
            pins.gpio40,
            pins.gpio14,
            pins.gpio39,
            pins.gpio12,
        )
        .map_err(|err| anyhow::anyhow!("sdcard mount failed: {err:?}"))
        .inspect_err(|err| log::warn!("{err:?}"))
        .ok();
        draw_startup_screen(&mut display, "Storage", 35)?;

        let mut i2c = I2cDriver::new(
            peripherals.i2c1,
            pins.gpio8,
            pins.gpio9,
            &i2c_config::Config::new().baudrate(I2C_BAUDRATE_KHZ.kHz().into()),
        )
        .inspect_err(|err| log::warn!("i2c init failed: {err:?}"))
        .ok();
        draw_startup_screen(&mut display, "Input", 50)?;

        let keyboard = i2c.as_mut().and_then(|i2c| {
            Tca8418Keyboard::new(i2c)
                .inspect_err(|err| log::warn!("keyboard init failed: {err:?}"))
                .ok()
        });
        let imu = i2c.as_mut().and_then(|i2c| {
            Bmi270::probe(i2c)
                .inspect_err(|err| log::warn!("imu probe failed: {err:?}"))
                .ok()
        });
        draw_startup_screen(&mut display, "Sensors", 65)?;

        let audio = i2c.as_mut().and_then(|i2c| {
            initialize_audio(i2c)
                .inspect_err(|err| log::warn!("audio init failed: {err:?}"))
                .ok()
        });
        draw_startup_screen(&mut display, "Audio", 80)?;

        let i2s = initialize_i2s(
            peripherals.i2s0,
            pins.gpio41,
            pins.gpio46,
            pins.gpio42,
            pins.gpio43,
        )
        .inspect_err(|err| log::warn!("i2s init failed: {err:?}"))
        .ok();
        draw_startup_screen(&mut display, "Ready", 95)?;

        Ok(Self {
            display,
            backlight,
            _sdcard: sdcard,
            i2c,
            keyboard,
            go_button,
            go_button_pressed,
            imu,
            audio,
            i2s,
            recording: None,
        })
    }

    pub fn display_mut(&mut self) -> &mut LcdDisplay<'static> {
        &mut self.display
    }

    pub fn set_backlight_full(&mut self) -> Result<()> {
        self.backlight.set_duty(self.backlight.get_max_duty())?;
        Ok(())
    }

    pub fn sdcard_mounted(&self) -> bool {
        self._sdcard.is_some()
    }

    pub fn storage_root(&self) -> &'static str {
        SDCARD_MOUNT_PATH
    }

    pub fn keyboard_available(&self) -> bool {
        self.keyboard.is_some()
    }

    pub fn imu_status(&self) -> ImuStatus {
        self.imu
            .as_ref()
            .map(Bmi270::status)
            .unwrap_or_else(ImuStatus::missing)
    }

    pub fn audio_status(&self) -> AudioStatus {
        self.audio
            .as_ref()
            .map(Es8311Codec::status)
            .unwrap_or_else(AudioStatus::missing)
    }

    pub fn set_speaker_volume_percent(&mut self, volume_percent: u8) -> Result<()> {
        let Some(audio) = self.audio.as_mut() else {
            anyhow::bail!("audio codec is not available");
        };
        let Some(i2c) = self.i2c.as_mut() else {
            anyhow::bail!("audio i2c bus is not available");
        };

        audio.set_speaker_volume_percent(i2c, volume_percent)
    }

    pub fn voice_recorder_available(&self) -> bool {
        self.sdcard_mounted() && self.audio.is_some() && self.i2s.is_some()
    }

    pub fn voice_recording_active(&self) -> bool {
        self.recording.is_some()
    }

    pub fn start_voice_recording(&mut self) -> Result<String> {
        if !self.voice_recorder_available() {
            anyhow::bail!("voice recorder is not available");
        }
        if self.recording.is_some() {
            anyhow::bail!("voice recorder is already active");
        }

        let recording_dir = channel9_fs::recordings_dir(SDCARD_MOUNT_PATH);
        channel9_fs::ensure_dir(&recording_dir).with_context(|| {
            format!(
                "failed to create recording directory {}",
                recording_dir.display()
            )
        })?;
        let path = channel9_fs::next_recording_path(SDCARD_MOUNT_PATH)?;
        let path_text = path.to_string_lossy().into_owned();

        let mut file = RecordingFile::create(path.as_path())?;
        let empty_header = [0_u8; WAV_HEADER_BYTES];
        file.write_all(&empty_header)
            .with_context(|| format!("failed to write wav placeholder {}", path.display()))?;

        let speaker_was_ready = self.audio_status().speaker_ready;
        if speaker_was_ready {
            self.set_speaker_enabled(false)?;
        }
        self.set_microphone_enabled(true)?;
        let i2s = self
            .i2s
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("i2s is not available"))?;
        i2s.tx_enable()?;
        i2s.rx_enable()?;
        log::info!("voice recording uses ES8311 hardware ALC and gentle software filter");

        self.recording = Some(VoiceRecording {
            file,
            path: path_text.clone(),
            data_bytes: 0,
            read_timeouts: 0,
            sample_rate: AUDIO_SAMPLE_RATE_HZ,
            denoiser: VoiceDenoiser::new(),
            speaker_was_ready,
        });

        Ok(path_text)
    }

    pub fn poll_voice_recording(&mut self) -> Result<u32> {
        let Some(recording) = self.recording.as_mut() else {
            return Ok(0);
        };
        let i2s = self
            .i2s
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("i2s is not available"))?;
        let mut buffer = [0_u8; AUDIO_BUFFER_BYTES];
        let bytes_read = match i2s.read(&mut buffer, AUDIO_READ_TIMEOUT_TICKS) {
            Ok(bytes_read) => bytes_read,
            Err(err) if err.code() == esp_idf_svc::sys::ESP_ERR_TIMEOUT => {
                recording.read_timeouts = recording.read_timeouts.saturating_add(1);
                0
            }
            Err(err) => return Err(err.into()),
        };
        if bytes_read > 0 {
            recording
                .denoiser
                .process_pcm16_le(&mut buffer[..bytes_read]);
            apply_gain_to_pcm16_le(&mut buffer[..bytes_read], AUDIO_RECORDING_GAIN);
            recording.file.write_all(&buffer[..bytes_read])?;
            recording.data_bytes = recording.data_bytes.saturating_add(bytes_read as u32);
        }

        Ok(recording.data_bytes)
    }

    pub fn stop_voice_recording(&mut self) -> Result<Option<String>> {
        let Some(mut recording) = self.recording.take() else {
            return Ok(None);
        };
        let i2s_disable_result = if let Some(i2s) = self.i2s.as_mut() {
            let rx_result = i2s.rx_disable().map_err(anyhow::Error::from);
            let tx_result = i2s.tx_disable().map_err(anyhow::Error::from);
            rx_result.and(tx_result)
        } else {
            Ok(())
        };
        let mic_disable_result = self.set_microphone_enabled(false);
        let speaker_enable_result = if recording.speaker_was_ready {
            self.set_speaker_enabled(true)
        } else {
            Ok(())
        };
        i2s_disable_result?;
        mic_disable_result?;
        speaker_enable_result?;

        recording.file.rewind()?;
        let header = wav_header(recording.data_bytes, recording.sample_rate);
        recording.file.write_all(&header)?;
        recording.file.flush()?;
        recording.file.close()?;
        log::info!(
            "voice note stopped: bytes={}, read_timeouts={}",
            recording.data_bytes,
            recording.read_timeouts
        );

        Ok(Some(recording.path))
    }

    pub fn play_voice_note(&mut self, path: &str) -> Result<()> {
        let mut file = RecordingFile::open(Path::new(path))?;
        let mut header = [0_u8; WAV_HEADER_BYTES];
        file.read_exact(&mut header)?;
        if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
            anyhow::bail!("recording is not a wav file");
        }

        self.set_speaker_enabled(true)?;
        let mut buffer = [0_u8; AUDIO_BUFFER_BYTES];
        let mut data_bytes = 0_u32;
        let mut write_timeouts = 0_u32;

        let playback_result = (|| -> Result<()> {
            let i2s = self
                .i2s
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("i2s is not available"))?;
            i2s.tx_enable()?;

            loop {
                let bytes_read = file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                match i2s.write_all(&buffer[..bytes_read], AUDIO_WRITE_TIMEOUT_TICKS) {
                    Ok(()) => data_bytes = data_bytes.saturating_add(bytes_read as u32),
                    Err(err) if err.code() == esp_idf_svc::sys::ESP_ERR_TIMEOUT => {
                        write_timeouts = write_timeouts.saturating_add(1);
                    }
                    Err(err) => return Err(err.into()),
                }
            }
            Ok(())
        })();

        let tx_disable_result = if let Some(i2s) = self.i2s.as_mut() {
            i2s.tx_disable().map_err(anyhow::Error::from)
        } else {
            Ok(())
        };
        let speaker_disable_result = self.set_speaker_enabled(false);
        file.close()?;

        playback_result?;
        tx_disable_result?;
        speaker_disable_result?;

        log::info!("voice note played: bytes={data_bytes}, write_timeouts={write_timeouts}");
        Ok(())
    }

    pub fn delete_voice_note(&self, path: &str) -> Result<()> {
        fs::remove_file(path)?;
        Ok(())
    }

    pub fn poll_input(&mut self) -> Result<Option<InputEvent>> {
        if self.poll_go_button() {
            return Ok(Some(InputEvent::Back));
        }

        match (self.keyboard.as_mut(), self.i2c.as_mut()) {
            (Some(keyboard), Some(i2c)) => keyboard.poll(i2c),
            _ => Ok(None),
        }
    }

    fn poll_go_button(&mut self) -> bool {
        let pressed = self.go_button.is_low();
        let pressed_edge = pressed && !self.go_button_pressed;
        self.go_button_pressed = pressed;
        pressed_edge
    }

    fn set_microphone_enabled(&mut self, enabled: bool) -> Result<()> {
        let Some(audio) = self.audio.as_mut() else {
            anyhow::bail!("audio codec is not available");
        };
        let Some(i2c) = self.i2c.as_mut() else {
            anyhow::bail!("audio i2c bus is not available");
        };

        if enabled {
            audio.enable_microphone(i2c)
        } else {
            audio.disable_microphone(i2c)
        }
    }

    fn set_speaker_enabled(&mut self, enabled: bool) -> Result<()> {
        let Some(audio) = self.audio.as_mut() else {
            anyhow::bail!("audio codec is not available");
        };
        let Some(i2c) = self.i2c.as_mut() else {
            anyhow::bail!("audio i2c bus is not available");
        };

        if enabled {
            audio.enable_speaker(i2c)
        } else {
            audio.disable_speaker(i2c)
        }
    }
}

fn draw_startup_screen(display: &mut LcdDisplay<'static>, stage: &str, progress: u8) -> Result<()> {
    let progress = progress.min(100);
    let fill_width = STARTUP_PROGRESS_WIDTH * progress as u32 / 100;
    let bg = Rgb565::new(28, 57, 28);
    let bar = Rgb565::new(0, 28, 31);
    let text = Rgb565::new(31, 63, 31);
    let muted = Rgb565::new(18, 38, 18);
    let accent = Rgb565::new(31, 36, 0);

    display
        .clear(bg)
        .map_err(|err| anyhow::anyhow!("startup clear failed: {err:?}"))?;
    Text::new(
        "Channel9",
        Point::new(60, 42),
        MonoTextStyle::new(&FONT_10X20, text),
    )
    .draw(display)
    .map_err(|err| anyhow::anyhow!("startup title draw failed: {err:?}"))?;
    Text::new(
        stage,
        Point::new(60, 62),
        MonoTextStyle::new(&FONT_6X10, muted),
    )
    .draw(display)
    .map_err(|err| anyhow::anyhow!("startup stage draw failed: {err:?}"))?;
    Rectangle::new(
        Point::new(28, 82),
        Size::new(STARTUP_PROGRESS_WIDTH, STARTUP_PROGRESS_HEIGHT),
    )
    .into_styled(
        PrimitiveStyleBuilder::new()
            .stroke_color(bar)
            .stroke_width(1)
            .build(),
    )
    .draw(display)
    .map_err(|err| anyhow::anyhow!("startup progress frame draw failed: {err:?}"))?;
    if fill_width > 2 {
        Rectangle::new(
            Point::new(30, 84),
            Size::new(fill_width.saturating_sub(4), STARTUP_PROGRESS_HEIGHT - 4),
        )
        .into_styled(PrimitiveStyle::with_fill(accent))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("startup progress fill draw failed: {err:?}"))?;
    }

    Ok(())
}

struct VoiceRecording {
    file: RecordingFile,
    path: String,
    data_bytes: u32,
    read_timeouts: u32,
    sample_rate: u32,
    denoiser: VoiceDenoiser,
    speaker_was_ready: bool,
}

struct VoiceDenoiser {
    dc_estimate: i32,
}

impl VoiceDenoiser {
    fn new() -> Self {
        Self { dc_estimate: 0 }
    }

    fn process_pcm16_le(&mut self, buffer: &mut [u8]) {
        for sample in buffer.chunks_exact_mut(2) {
            let raw = i16::from_le_bytes([sample[0], sample[1]]) as i32;
            self.dc_estimate += (raw - self.dc_estimate) >> AUDIO_DC_FILTER_SHIFT;

            let centered = raw - self.dc_estimate;
            let limited = soft_limit_i16(centered);
            sample.copy_from_slice(&limited.to_le_bytes());
        }
    }
}

fn initialize_audio(i2c: &mut I2cDriver<'static>) -> Result<Es8311Codec> {
    Es8311Codec::probe(i2c)
}

fn initialize_i2s(
    i2s0: I2S0<'static>,
    bclk: Gpio41<'static>,
    din: Gpio46<'static>,
    dout: Gpio42<'static>,
    ws: Gpio43<'static>,
) -> Result<I2sDriver<'static, I2sBiDir>> {
    let slot_config = StdSlotConfig::philips_slot_default(DataBitWidth::Bits16, SlotMode::Mono)
        .slot_bit_width(SlotBitWidth::Bits16)
        .slot_mode_mask(SlotMode::Mono, StdSlotMask::Left)
        .ws_width(16)
        .bit_shift(true)
        .left_align(true)
        .big_endian(false)
        .bit_order_lsb(false);
    let config = StdConfig::new(
        I2sConfig::default()
            .dma_buffer_count(AUDIO_DMA_BUFFER_COUNT)
            .frames_per_buffer(AUDIO_DMA_FRAMES_PER_BUFFER),
        StdClkConfig::from_sample_rate_hz(AUDIO_SAMPLE_RATE_HZ),
        slot_config,
        StdGpioConfig::default(),
    );
    Ok(I2sDriver::new_std_bidir(
        i2s0,
        &config,
        bclk,
        din,
        dout,
        Option::<AnyIOPin>::None,
        ws,
    )?)
}

fn mount_sdcard(
    spi3: esp_idf_hal::spi::SPI3<'static>,
    sck: Gpio40<'static>,
    mosi: Gpio14<'static>,
    miso: Gpio39<'static>,
    cs: Gpio12<'static>,
) -> Result<MountedSdFatfs<'static>> {
    let spi_driver = SpiDriver::new(
        spi3,
        sck,
        mosi,
        Some(miso),
        &SpiDriverConfig::new().dma(Dma::Auto(SDCARD_DMA_BUFFER_SIZE)),
    )?;
    let host = SdSpiHostDriver::new(
        spi_driver,
        Some(cs),
        Option::<AnyIOPin>::None,
        Option::<AnyIOPin>::None,
        Option::<AnyIOPin>::None,
        None,
    )?;
    let card = SdCardDriver::new_spi(host, &SdCardConfiguration::new())?;
    let fatfs = Fatfs::new_sdcard(0, card)?;
    Ok(MountedFatfs::mount(
        fatfs,
        SDCARD_MOUNT_PATH,
        SDCARD_MAX_OPEN_FILES,
    )?)
}

fn wav_header(data_bytes: u32, sample_rate: u32) -> [u8; WAV_HEADER_BYTES] {
    let byte_rate = sample_rate * 2;
    let block_align = 2_u16;
    let bits_per_sample = 16_u16;
    let file_size = data_bytes
        .saturating_add(WAV_HEADER_BYTES as u32)
        .saturating_sub(8);

    let mut header = [0_u8; WAV_HEADER_BYTES];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&file_size.to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16_u32.to_le_bytes());
    header[20..22].copy_from_slice(&1_u16.to_le_bytes());
    header[22..24].copy_from_slice(&1_u16.to_le_bytes());
    header[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    header[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    header[32..34].copy_from_slice(&block_align.to_le_bytes());
    header[34..36].copy_from_slice(&bits_per_sample.to_le_bytes());
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    header
}

fn apply_gain_to_pcm16_le(buffer: &mut [u8], gain: f32) {
    if gain == 1.0 {
        return;
    }

    for sample in buffer.chunks_exact_mut(2) {
        let value = i16::from_le_bytes([sample[0], sample[1]]) as f32;
        let amplified = (value * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        sample.copy_from_slice(&amplified.to_le_bytes());
    }
}

fn soft_limit_i16(sample: i32) -> i16 {
    let sign = if sample < 0 { -1 } else { 1 };
    let magnitude = sample.abs();
    let limited = if magnitude <= AUDIO_SOFT_LIMIT_THRESHOLD {
        magnitude
    } else {
        let excess = magnitude - AUDIO_SOFT_LIMIT_THRESHOLD;
        AUDIO_SOFT_LIMIT_THRESHOLD + excess / 4
    };
    (limited * sign).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

struct RecordingFile {
    file: fs::File,
}

impl RecordingFile {
    fn create(path: &Path) -> Result<Self> {
        let file = channel9_fs::create_file(path)
            .with_context(|| format!("failed to create recording {}", path.display()))?;
        Ok(Self { file })
    }

    fn open(path: &Path) -> Result<Self> {
        let file = channel9_fs::open_file(path)
            .with_context(|| format!("failed to open recording {}", path.display()))?;
        Ok(Self { file })
    }

    fn write_all(&mut self, data: &[u8]) -> Result<()> {
        self.file.write_all(data).context("failed to write file")
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        self.file.read(buffer).context("failed to read file")
    }

    fn read_exact(&mut self, buffer: &mut [u8]) -> Result<()> {
        self.file
            .read_exact(buffer)
            .context("failed to read wav header")
    }

    fn rewind(&mut self) -> Result<()> {
        self.file
            .seek(SeekFrom::Start(0))
            .map(|_| ())
            .context("failed to seek file")
    }

    fn flush(&mut self) -> Result<()> {
        self.file.flush().context("failed to flush file")
    }

    fn close(self) -> Result<()> {
        Ok(())
    }
}
