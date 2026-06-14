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
PUSH
No pushes yet
Waiting for push
MENU
```

## Crate Layout

- `esp32_channel9`: firmware entry point and runtime wiring.
- `channel9-core`: device IDs and serializable app configuration.
- `channel9-board`: board-specific hardware support. The current implementation is `CardputerAdv` with LCD, microSD, and TCA8418 keyboard input.
- `channel9-runtime`: host-testable Channel9 device-code, token polling, and push/SSE state transitions.
- `channel9-storage`: JSON config persistence, SD directory listing, and a `littlefs2` build probe.
- `channel9-ui`: display rendering, Channel9 home screen, and Bruce-like menu/file screens.
- `channel9-wifi`: ESP-IDF WiFi runtime, scan support, and saved-credential connection.

## Test

Firmware-level checks use the ESP target from `.cargo/config.toml`:

```sh
cargo fmt --check
cargo check
```

Host-side Channel9 runtime tests must specify a host target because this repository defaults to the
ESP target:

```sh
rustc -vV
cargo +esp test -p channel9-runtime --target aarch64-apple-darwin
```

On GitHub Actions, `Host Tests` runs:

```sh
cargo +nightly test -p channel9-runtime --target x86_64-unknown-linux-gnu
```

## Current Controls

On Cardputer-Adv:

- `Enter`: open the function menu from the Channel9 home screen, or select the highlighted item.
- `;` or `,`: move to the previous item.
- `.` or `/`: move to the next item.
- `` ` `` or `Backspace`: go back.

The function menu is a phone-like icon carousel. It currently exposes:

- `Messages`: shows latest Channel9 push text, connection status, and message count.
- `WiFi`: configure WiFi startup behavior.
- `Storage`: configure persistence behavior.
- `Device`: shows board capability status.
- `Files`: opens the SD file browser.
- `Time`: configures SNTP server, sync behavior, and UTC offset.
- `Audio`: controls speaker volume.
- `Channel9`: creates or refreshes device-code login, polls activation, and clears login.
- `Recorder`: records, plays back, and deletes voice notes.

Current WiFi settings:

- `Auto Connect`: toggles `wifi.connect_at_startup` and persists it to `/sdcard/channel9/config.json`.
- `Status`: shows current connection details, IP, and DNS.
- `Saved Networks`: lists saved networks; `Enter` joins and `Backspace` deletes.
- `Scan Networks`: scans nearby access points, lets the user choose one, and stores the password.

If `Auto Connect` is enabled and at least one credential exists in the config, the firmware scans
nearby APs and tries matching saved credentials in scan order.

Current Storage settings:

- `Prefer SD`: toggles `storage.prefer_sdcard` and persists it to `/sdcard/channel9/config.json`.
- `Mount Path`: read-only mount path.
- `Files`: opens the SD file browser when the SD card is mounted.

Current Channel9 behavior:

- Device-code creation and token polling run through a single serialized network worker.
- Pending codes are polled automatically at the server-provided interval.
- Activated devices open the Channel9 SSE endpoint through the same serialized worker.
- New pushes update the home screen and play the pager-style notification sound.

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
