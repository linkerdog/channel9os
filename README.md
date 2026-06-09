# esp32_channel9

Minimal Rust hello world for the Channel9 ESP32-S3 firmware.

## Target

- Board class: ESP32-S3
- Current MCU target: `esp32s3`
- Rust target: `xtensa-esp32s3-espidf`
- Framework: ESP-IDF through `esp-idf-svc`
- Display: ST7789V2, 240 x 135

## Build

Install the ESP Rust tooling first:

```sh
cargo install ldproxy
cargo install espflash
cargo install espup
espup install
```

Then build the firmware:

```sh
cargo build
```

## Flash and Monitor

With the board connected over USB:

```sh
cargo run
```

The serial monitor should include:

```text
channel9 hello world on ESP32-S3
```

The built-in display should show:

```text
CHANNEL9
Push suggestions
No pushes yet
Open config to setup WiFi
SEL: Config
```

## Crate Layout

- `esp32_channel9`: firmware entry point and runtime wiring.
- `channel9-core`: device IDs and serializable app configuration.
- `channel9-board`: board-specific hardware support. The current implementation is `CardputerAdv` with LCD, microSD, and TCA8418 keyboard input.
- `channel9-storage`: JSON config persistence, SD directory listing, and a `littlefs2` build probe.
- `channel9-ui`: display rendering, Channel9 home screen, and Bruce-like menu/file screens.
- `channel9-wifi`: ESP-IDF WiFi runtime, scan support, and saved-credential connection.

## Current Controls

On Cardputer-Adv:

- `Enter`: open the config menu from the Channel9 home screen.
- `;` or `,`: move to the previous config item.
- `.` or `/`: move to the next config item.
- `` ` `` or `Backspace`: go back.

The config menu is a Bruce-like icon carousel. It currently exposes:

- `WiFi`: configure WiFi startup behavior.
- `Storage`: configure persistence behavior.
- `Files`: opens the SD file browser.
- `Back`: returns to the Channel9 home screen.

Current WiFi settings:

- `Auto Connect`: toggles `wifi.connect_at_startup` and persists it to `/sdcard/channel9/config.json`.
- `Saved Networks`: read-only summary for now.
- `Scan Networks`: scans nearby access points through `channel9-wifi` and shows the first results.

If `Auto Connect` is enabled and at least one credential exists in the config, the firmware tries to
connect to the first saved network during startup.

Current Storage settings:

- `Prefer SD`: toggles `storage.prefer_sdcard` and persists it to `/sdcard/channel9/config.json`.
- `Mount Path`: read-only mount path.
- `Files`: opens the SD file browser when the SD card is mounted.

## Cardputer-Adv LCD Pins

| LCD Signal | ESP32-S3 GPIO |
| --- | --- |
| DISP_BL | GPIO38 |
| RST | GPIO33 |
| RS/DC | GPIO34 |
| DAT/MOSI | GPIO35 |
| SCK | GPIO36 |
| CS | GPIO37 |

## Cardputer-Adv microSD Pins

The Cardputer-Adv microSD card is mounted as FATFS at `/sdcard`.

| SD Signal | ESP32-S3 GPIO |
| --- | --- |
| CS | GPIO12 |
| SCK | GPIO40 |
| MISO | GPIO39 |
| MOSI | GPIO14 |

The app config is stored at:

```text
/sdcard/channel9/config.json
```

On boot, the firmware renders the Channel9 home screen with push suggestions and a config entry.
The file browser screen is kept as a Bruce-like configuration/file-management view: status bar,
border, centered title, current path, `>` selected row, colored folders, file sizes, and a bottom hint line.

## Notes

- `cargo run` uses `espflash flash --monitor` from `.cargo/config.toml`.
- If ESP-IDF Python environment installation fails, check which `python3` is used by the shell before running Cargo. A system or Homebrew Python is usually less surprising than an Anaconda-managed Python for ESP-IDF tooling.
- The Cardputer-Adv LCD path follows the Rust `cardputer` reference implementation: SPI2 host, SPI mode 3, 80 MHz write clock, ST7789 inversion enabled, 240 x 135 display size, `(40, 53)` window offset, and LEDC PWM backlight on GPIO38.
- `littlefs2` is currently integrated as a compile-time storage probe. The active SD card filesystem remains ESP-IDF FATFS because `littlefs2` requires a dedicated block-device implementation.
- `sdkconfig.defaults` raises `CONFIG_ESP_MAIN_TASK_STACK_SIZE` to avoid stack overflow while initializing the display from Rust.
