use anyhow::Result;
use channel9_ble::{BleStatus, Channel9Ble};
use channel9_board::{AudioStatus, CardputerAdv, ImuStatus, InputEvent};
use channel9_client::{Channel9HttpClient, DeviceCode, PollToken};
use channel9_core::{AppConfig, WifiCredential};
use channel9_storage::{ConfigStore, JsonConfigStore, list_directory, littlefs2_probe};
use channel9_time::{Channel9Time, TimeSyncStatus, format_clock};
use channel9_ui::{
    FileListItem, HomeView, MenuItem, SettingItem, StatusBar, StatusBle, StatusWifi,
};
use channel9_wifi::{Channel9Wifi, WifiNetwork, WifiStatus};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::thread;
use std::time::Duration;

const UI_SCHEDULER_STACK_BYTES: usize = 8192;
const UI_CLOCK_TICK_INTERVAL: Duration = Duration::from_secs(1);
const CHANNEL9_API_BASE_URL: &str = "https://app.linkerdog.work";

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("channel9 hello world on ESP32-S3");
    log::info!("storage probe: {}", littlefs2_probe());

    if let Err(err) = run_display() {
        log::error!("failed to draw hello world on display: {err:?}");
    }

    loop {
        FreeRtos::delay_ms(10);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Config { selected: usize },
    Wifi { selected: usize },
    WifiSaved { selected: usize },
    WifiScan { selected: usize },
    WifiPassword { network_index: usize },
    WifiResult,
    Storage { selected: usize },
    Device,
    Files,
    Time { selected: usize },
    Audio { selected: usize },
    Channel9 { selected: usize },
    Channel9Input { field: Channel9InputField },
    Recorder { selected: usize },
    Recording,
    RecorderMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel9InputField {
    Device,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordingItem {
    name: String,
    path: String,
    size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Channel9LoginState {
    active_code: Option<DeviceCode>,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiEvent {
    ClockTick,
}

fn run_display() -> Result<()> {
    let peripherals = Peripherals::take()?;
    let (board_peripherals, modem) = CardputerAdv::split(peripherals);
    let (wifi_modem, ble_modem) = modem.split();
    let mut board = CardputerAdv::new(board_peripherals)?;
    let mut wifi = Channel9Wifi::new(wifi_modem)
        .inspect_err(|err| log::warn!("wifi init failed: {err:?}"))
        .ok();
    let ble = Channel9Ble::new(ble_modem)
        .and_then(|ble| {
            ble.start_advertising()?;
            Ok(ble)
        })
        .inspect_err(|err| log::warn!("ble init failed: {err:?}"))
        .ok();
    let mut time = Channel9Time::new();
    let mut config = load_config(&board);
    apply_audio_config(&mut board, &config);
    let mut scan_results = Vec::new();
    let mut recordings = load_recordings(&board);
    let mut password_input = String::new();
    let mut channel9_input = String::new();
    let mut channel9_login = Channel9LoginState::default();
    let mut recorder_message = heapless::String::<64>::new();
    let (ui_events_tx, ui_events_rx) = sync_channel::<UiEvent>(4);
    let _ui_scheduler_task = spawn_ui_scheduler(ui_events_tx)?;
    if let Some(wifi) = wifi.as_mut() {
        if let Err(err) = wifi.connect_first_saved(&config.wifi) {
            log::warn!("wifi auto connect failed: {err:?}");
        }
    }
    if wifi
        .as_ref()
        .map(|wifi| wifi.status() == WifiStatus::Connected)
        .unwrap_or(false)
    {
        if let Err(err) = time.start(&config.time) {
            log::warn!("sntp startup sync failed: {err:?}");
        }
    }
    let mut screen = Screen::Home;
    let mut paused_radios = PausedRadios::default();
    let mut rendered_clock = format_clock(config.time.timezone_offset_minutes);
    render_screen(
        &mut board,
        wifi.as_ref(),
        ble.as_ref(),
        &mut time,
        &config,
        &scan_results,
        &recordings,
        &password_input,
        &channel9_input,
        &channel9_login,
        recorder_message.as_str(),
        screen,
    )?;
    board.set_backlight_full()?;

    loop {
        if screen == Screen::Recording {
            if let Err(err) = board.poll_voice_recording() {
                log::warn!("voice note record failed: {err:?}");
                recorder_message.clear();
                let _ = core::fmt::write(
                    &mut recorder_message,
                    format_args!("Record failed: {err:?}"),
                );
                if let Err(stop_err) = board.stop_voice_recording() {
                    log::warn!("voice note stop failed after record error: {stop_err:?}");
                }
                resume_radios_after_recording(
                    wifi.as_mut(),
                    ble.as_ref(),
                    &config,
                    &paused_radios,
                    &mut time,
                );
                paused_radios = PausedRadios::default();
                recordings = load_recordings(&board);
                screen = Screen::RecorderMessage;
                render_screen(
                    &mut board,
                    wifi.as_ref(),
                    ble.as_ref(),
                    &mut time,
                    &config,
                    &scan_results,
                    &recordings,
                    &password_input,
                    &channel9_input,
                    &channel9_login,
                    recorder_message.as_str(),
                    screen,
                )?;
            }
        }

        if let Some(input) = board.poll_input()? {
            if screen == Screen::Recording {
                if matches!(input, InputEvent::Back | InputEvent::Select) {
                    recorder_message.clear();
                    match board.stop_voice_recording() {
                        Ok(Some(path)) => {
                            let _ = core::fmt::write(
                                &mut recorder_message,
                                format_args!("Saved {}", file_name(path.as_str())),
                            );
                            log::info!("voice note recorded: {path}");
                        }
                        Ok(None) => {
                            let _ = recorder_message.push_str("No active recording");
                        }
                        Err(err) => {
                            let _ = core::fmt::write(
                                &mut recorder_message,
                                format_args!("Stop failed: {err:?}"),
                            );
                            log::warn!("voice note stop failed: {err:?}");
                        }
                    }
                    resume_radios_after_recording(
                        wifi.as_mut(),
                        ble.as_ref(),
                        &config,
                        &paused_radios,
                        &mut time,
                    );
                    paused_radios = PausedRadios::default();
                    recordings = load_recordings(&board);
                    screen = Screen::RecorderMessage;
                    render_screen(
                        &mut board,
                        wifi.as_ref(),
                        ble.as_ref(),
                        &mut time,
                        &config,
                        &scan_results,
                        &recordings,
                        &password_input,
                        &channel9_input,
                        &channel9_login,
                        recorder_message.as_str(),
                        screen,
                    )?;
                }
                FreeRtos::delay_ms(10);
                continue;
            }

            let mut next = reduce_screen(
                &mut board,
                wifi.as_mut(),
                &mut time,
                &mut config,
                &mut scan_results,
                &mut recordings,
                &mut password_input,
                &mut channel9_input,
                &mut channel9_login,
                screen,
                input,
            );
            if next == Screen::Recording {
                paused_radios = pause_radios_for_recording(wifi.as_mut(), ble.as_ref());
                recorder_message.clear();
                match board.start_voice_recording() {
                    Ok(path) => {
                        log::info!("voice note recording started: {path}");
                    }
                    Err(err) => {
                        let _ = core::fmt::write(
                            &mut recorder_message,
                            format_args!("Record failed: {err:?}"),
                        );
                        log::warn!("voice note start failed: {err:?}");
                        resume_radios_after_recording(
                            wifi.as_mut(),
                            ble.as_ref(),
                            &config,
                            &paused_radios,
                            &mut time,
                        );
                        paused_radios = PausedRadios::default();
                        next = Screen::RecorderMessage;
                    }
                }
            }
            screen = next;
            render_screen(
                &mut board,
                wifi.as_ref(),
                ble.as_ref(),
                &mut time,
                &config,
                &scan_results,
                &recordings,
                &password_input,
                &channel9_input,
                &channel9_login,
                recorder_message.as_str(),
                screen,
            )?;
            rendered_clock = format_clock(config.time.timezone_offset_minutes);
        }

        drain_ui_events(
            &ui_events_rx,
            &mut board,
            wifi.as_ref(),
            ble.as_ref(),
            &mut time,
            &config,
            &scan_results,
            &recordings,
            &password_input,
            &channel9_input,
            &channel9_login,
            recorder_message.as_str(),
            screen,
            &mut rendered_clock,
        )?;
        FreeRtos::delay_ms(10);
    }
}

fn reduce_screen(
    board: &mut CardputerAdv,
    wifi: Option<&mut Channel9Wifi>,
    time: &mut Channel9Time,
    config: &mut AppConfig,
    scan_results: &mut Vec<WifiNetwork>,
    recordings: &mut Vec<RecordingItem>,
    password_input: &mut String,
    channel9_input: &mut String,
    channel9_login: &mut Channel9LoginState,
    screen: Screen,
    input: InputEvent,
) -> Screen {
    match (screen, input) {
        (Screen::Home, InputEvent::Select) => Screen::Config { selected: 0 },
        (Screen::Config { .. }, InputEvent::Back) => Screen::Home,
        (Screen::Config { selected }, InputEvent::Up | InputEvent::Left) => Screen::Config {
            selected: if selected == 0 {
                CONFIG_ITEMS.len() - 1
            } else {
                selected - 1
            },
        },
        (Screen::Config { selected }, InputEvent::Down | InputEvent::Right) => Screen::Config {
            selected: (selected + 1) % CONFIG_ITEMS.len(),
        },
        (Screen::Config { selected }, InputEvent::Select) => match selected {
            0 => Screen::Wifi { selected: 0 },
            1 => Screen::Storage { selected: 0 },
            2 => Screen::Device,
            3 => Screen::Files,
            4 => Screen::Time { selected: 0 },
            5 => Screen::Audio { selected: 0 },
            6 => {
                ensure_channel9_device_id(board, config);
                if !channel9_logged_in(config) && channel9_login.active_code.is_none() {
                    create_channel9_device_code(board, wifi, config, channel9_login);
                }
                Screen::Channel9 { selected: 0 }
            }
            7 => Screen::Recorder { selected: 0 },
            _ => Screen::Config { selected },
        },
        (Screen::Wifi { .. }, InputEvent::Back) => Screen::Config { selected: 0 },
        (Screen::Wifi { selected }, InputEvent::Up | InputEvent::Left) => Screen::Wifi {
            selected: selected.saturating_sub(1),
        },
        (Screen::Wifi { selected }, InputEvent::Down | InputEvent::Right) => Screen::Wifi {
            selected: (selected + 1).min(WIFI_ITEMS.len() - 1),
        },
        (Screen::Wifi { selected }, InputEvent::Select) => {
            if selected == 4 {
                return Screen::Config { selected: 0 };
            }
            if selected == 1 {
                return Screen::WifiResult;
            }
            if selected == 2 {
                return Screen::WifiSaved { selected: 0 };
            }
            if selected == 3 {
                scan_results.clear();
                if let Some(wifi) = wifi {
                    match wifi.scan() {
                        Ok(networks) => {
                            *scan_results = networks;
                            return Screen::WifiScan { selected: 0 };
                        }
                        Err(err) => log::warn!("wifi scan failed: {err:?}"),
                    }
                } else {
                    log::warn!("wifi is not available");
                }
                return Screen::Wifi { selected };
            }
            apply_wifi_setting(board, config, selected);
            Screen::Wifi { selected }
        }
        (Screen::WifiResult, InputEvent::Back | InputEvent::Select) => Screen::Wifi { selected: 1 },
        (Screen::WifiSaved { .. }, InputEvent::Back) => Screen::Wifi { selected: 2 },
        (Screen::WifiSaved { selected }, InputEvent::Up | InputEvent::Left) => Screen::WifiSaved {
            selected: selected.saturating_sub(1),
        },
        (Screen::WifiSaved { selected }, InputEvent::Down | InputEvent::Right) => {
            Screen::WifiSaved {
                selected: (selected + 1).min(config.wifi.credentials.len().saturating_sub(1)),
            }
        }
        (Screen::WifiSaved { selected }, InputEvent::Backspace) => {
            if selected < config.wifi.credentials.len() {
                config.wifi.credentials.remove(selected);
                if config.wifi.credentials.is_empty() {
                    config.wifi.connect_at_startup = false;
                }
                save_config(board, config);
            }
            Screen::WifiSaved {
                selected: selected.min(config.wifi.credentials.len().saturating_sub(1)),
            }
        }
        (Screen::WifiSaved { selected }, InputEvent::Select) => {
            let credential = config.wifi.credentials.get(selected).cloned();
            if let (Some(wifi), Some(credential)) = (wifi, credential) {
                if let Err(err) = wifi.connect(&credential) {
                    log::warn!("wifi saved connect failed: {err:?}");
                } else if let Err(err) = time.sync_now(&config.time) {
                    log::warn!("sntp sync after wifi connect failed: {err:?}");
                }
                Screen::WifiResult
            } else {
                Screen::WifiSaved { selected }
            }
        }
        (Screen::WifiScan { .. }, InputEvent::Back) => Screen::Wifi { selected: 3 },
        (Screen::WifiScan { selected }, InputEvent::Up | InputEvent::Left) => Screen::WifiScan {
            selected: selected.saturating_sub(1),
        },
        (Screen::WifiScan { selected }, InputEvent::Down | InputEvent::Right) => Screen::WifiScan {
            selected: (selected + 1).min(scan_results.len().saturating_sub(1)),
        },
        (Screen::WifiScan { selected }, InputEvent::Select) => {
            if scan_results.is_empty() {
                Screen::Wifi { selected: 3 }
            } else {
                password_input.clear();
                Screen::WifiPassword {
                    network_index: selected.min(scan_results.len() - 1),
                }
            }
        }
        (Screen::WifiPassword { network_index }, InputEvent::Back) => Screen::WifiScan {
            selected: network_index,
        },
        (Screen::WifiPassword { network_index }, InputEvent::Backspace) => {
            password_input.pop();
            Screen::WifiPassword { network_index }
        }
        (Screen::WifiPassword { network_index }, InputEvent::Char(value)) => {
            if password_input.len() < 64 {
                password_input.push(value);
            }
            Screen::WifiPassword { network_index }
        }
        (Screen::WifiPassword { network_index }, InputEvent::Select) => {
            let selected_network = scan_results.get(network_index).cloned();
            if let (Some(wifi), Some(network)) = (wifi, selected_network) {
                let credential = WifiCredential {
                    ssid: network.ssid,
                    password: password_input.clone(),
                };
                upsert_wifi_credential(config, credential.clone());
                config.wifi.connect_at_startup = true;
                save_config(board, config);
                if let Err(err) = wifi.connect(&credential) {
                    log::warn!("wifi connect failed: {err:?}");
                } else if let Err(err) = time.sync_now(&config.time) {
                    log::warn!("sntp sync after wifi connect failed: {err:?}");
                }
                password_input.clear();
                Screen::WifiResult
            } else {
                Screen::Wifi { selected: 2 }
            }
        }
        (Screen::Storage { .. }, InputEvent::Back) => Screen::Config { selected: 1 },
        (Screen::Storage { selected }, InputEvent::Up | InputEvent::Left) => Screen::Storage {
            selected: selected.saturating_sub(1),
        },
        (Screen::Storage { selected }, InputEvent::Down | InputEvent::Right) => Screen::Storage {
            selected: (selected + 1).min(STORAGE_ITEMS.len() - 1),
        },
        (Screen::Storage { selected }, InputEvent::Select) => {
            match selected {
                0 => apply_storage_setting(board, config, selected),
                2 if board.sdcard_mounted() => return Screen::Files,
                3 => return Screen::Config { selected: 1 },
                _ => {}
            }
            Screen::Storage { selected }
        }
        (Screen::Device, InputEvent::Back | InputEvent::Select) => Screen::Config { selected: 2 },
        (Screen::Files, InputEvent::Back | InputEvent::Select) => Screen::Config { selected: 3 },
        (Screen::Time { .. }, InputEvent::Back) => Screen::Config { selected: 4 },
        (Screen::Time { selected }, InputEvent::Up | InputEvent::Left) => Screen::Time {
            selected: selected.saturating_sub(1),
        },
        (Screen::Time { selected }, InputEvent::Down | InputEvent::Right) => Screen::Time {
            selected: (selected + 1).min(TIME_ITEMS.len() - 1),
        },
        (Screen::Time { selected }, InputEvent::Select) => {
            match selected {
                0 => {
                    config.time.sync_at_startup = !config.time.sync_at_startup;
                    save_config(board, config);
                    if let Err(err) = time.start(&config.time) {
                        log::warn!("sntp setting apply failed: {err:?}");
                    }
                }
                1 => {
                    config.time.sntp_server = next_sntp_server(config.time.sntp_server.as_str());
                    save_config(board, config);
                    if let Err(err) = time.sync_now(&config.time) {
                        log::warn!("sntp server apply failed: {err:?}");
                    }
                }
                2 => {
                    config.time.timezone_offset_minutes =
                        next_timezone_offset(config.time.timezone_offset_minutes);
                    save_config(board, config);
                }
                3 => {
                    if let Err(err) = time.sync_now(&config.time) {
                        log::warn!("manual sntp sync failed: {err:?}");
                    }
                }
                4 => return Screen::Config { selected: 4 },
                _ => {}
            }
            Screen::Time { selected }
        }
        (Screen::Audio { .. }, InputEvent::Back) => {
            save_config(board, config);
            Screen::Config { selected: 5 }
        }
        (Screen::Audio { selected }, InputEvent::Up) => Screen::Audio {
            selected: selected.saturating_sub(1),
        },
        (Screen::Audio { selected }, InputEvent::Down) => Screen::Audio {
            selected: (selected + 1).min(AUDIO_ITEMS.len() - 1),
        },
        (Screen::Audio { selected: 0 }, InputEvent::Left) => {
            adjust_speaker_volume(board, config, -5);
            Screen::Audio { selected: 0 }
        }
        (Screen::Audio { selected: 0 }, InputEvent::Right | InputEvent::Select) => {
            adjust_speaker_volume(board, config, 5);
            Screen::Audio { selected: 0 }
        }
        (Screen::Audio { selected: 1 }, InputEvent::Select) => {
            save_config(board, config);
            Screen::Config { selected: 5 }
        }
        (Screen::Audio { selected }, _) => Screen::Audio { selected },
        (Screen::Channel9 { .. }, InputEvent::Back) => Screen::Config { selected: 6 },
        (Screen::Channel9 { selected }, InputEvent::Up | InputEvent::Left) => Screen::Channel9 {
            selected: selected.saturating_sub(1),
        },
        (Screen::Channel9 { selected }, InputEvent::Down | InputEvent::Right) => Screen::Channel9 {
            selected: (selected + 1).min(channel9_item_count(config).saturating_sub(1)),
        },
        (Screen::Channel9 { selected }, InputEvent::Select) => {
            if channel9_logged_in(config) {
                match selected {
                    5 => {
                        config.channel9.access_token = None;
                        config.channel9.token_expires_at = None;
                        channel9_login.active_code = None;
                        channel9_login.message = "Token cleared".to_owned();
                        save_config(board, config);
                        return Screen::Channel9 { selected: 0 };
                    }
                    6 => return Screen::Config { selected: 6 },
                    _ => {}
                }
            } else {
                match selected {
                    0 if channel9_login.active_code.is_some() => {
                        poll_channel9_device_token(board, wifi, config, channel9_login)
                    }
                    0 => create_channel9_device_code(board, wifi, config, channel9_login),
                    2 => poll_channel9_device_token(board, wifi, config, channel9_login),
                    1 => {
                        channel9_input.clear();
                        channel9_input.push_str(config.channel9.device_id.as_str());
                        return Screen::Channel9Input {
                            field: Channel9InputField::Device,
                        };
                    }
                    3 => create_channel9_device_code(board, wifi, config, channel9_login),
                    4 => return Screen::Config { selected: 6 },
                    _ => {}
                }
            }
            Screen::Channel9 { selected }
        }
        (Screen::Channel9Input { .. }, InputEvent::Back) => Screen::Channel9 { selected: 0 },
        (Screen::Channel9Input { field }, InputEvent::Backspace) => {
            channel9_input.pop();
            Screen::Channel9Input { field }
        }
        (Screen::Channel9Input { field }, InputEvent::Char(value)) => {
            if channel9_input.len() < 128 {
                channel9_input.push(value);
            }
            Screen::Channel9Input { field }
        }
        (Screen::Channel9Input { field }, InputEvent::Select) => {
            match field {
                Channel9InputField::Device => {
                    config.channel9.device_id = channel9_input.trim().to_owned();
                }
            }
            channel9_input.clear();
            save_config(board, config);
            channel9_login.active_code = None;
            channel9_login.message = "Press Refresh".to_owned();
            Screen::Channel9 { selected: 0 }
        }
        (Screen::Recorder { .. }, InputEvent::Back) => Screen::Config { selected: 7 },
        (Screen::Recorder { selected }, InputEvent::Up | InputEvent::Left) => Screen::Recorder {
            selected: selected.saturating_sub(1),
        },
        (Screen::Recorder { selected }, InputEvent::Down | InputEvent::Right) => Screen::Recorder {
            selected: (selected + 1).min(recordings.len()),
        },
        (Screen::Recorder { selected }, InputEvent::Backspace) => {
            if selected > 0 {
                if let Some(recording) = recordings.get(selected - 1) {
                    if let Err(err) = board.delete_voice_note(recording.path.as_str()) {
                        log::warn!("voice note delete failed: {err:?}");
                    }
                }
                *recordings = load_recordings(board);
            }
            Screen::Recorder {
                selected: selected.min(recordings.len()),
            }
        }
        (Screen::Recorder { selected }, InputEvent::Select) => {
            if selected == 0 {
                return Screen::Recording;
            }
            if let Some(recording) = recordings.get(selected - 1) {
                if let Err(err) = board.play_voice_note(recording.path.as_str()) {
                    log::warn!("voice note playback failed: {err:?}");
                }
            }
            Screen::Recorder { selected }
        }
        (Screen::RecorderMessage, InputEvent::Back | InputEvent::Select) => {
            Screen::Recorder { selected: 0 }
        }
        (Screen::Recording, _) => Screen::Recording,
        (current, _) => current,
    }
}

const CONFIG_ITEMS: &[&str] = &[
    "WiFi", "Storage", "Device", "Files", "Time", "Audio", "Channel9", "Recorder",
];
const WIFI_ITEMS: &[&str] = &[
    "Auto Connect",
    "Status",
    "Saved Networks",
    "Scan Networks",
    "Back",
];
const STORAGE_ITEMS: &[&str] = &["Prefer SD", "Mount Path", "Files", "Back"];
const TIME_ITEMS: &[&str] = &["Auto Sync", "SNTP Server", "UTC Offset", "Sync Now", "Back"];
const AUDIO_ITEMS: &[&str] = &["Volume", "Back"];
const CHANNEL9_LOGIN_ITEMS: &[&str] = &["User Code", "Device", "Poll", "Refresh", "Back"];
const CHANNEL9_STATUS_ITEMS: &[&str] = &[
    "Status",
    "Workspace",
    "Device",
    "Token",
    "Expires",
    "Clear",
    "Back",
];
const SNTP_SERVERS: &[&str] = &[
    "ntp.tuna.tsinghua.edu.cn",
    "time.pool.aliyun.com",
    "time1.aliyun.com",
    "time2.aliyun.com",
    "time3.aliyun.com",
    "time4.aliyun.com",
    "time5.aliyun.com",
    "time6.aliyun.com",
    "time7.aliyun.com",
    "0.pool.ntp.org",
    "1.pool.ntp.org",
    "2.pool.ntp.org",
    "3.pool.ntp.org",
];
const TIMEZONE_OFFSETS: &[i32] = &[8 * 60, 0, -8 * 60, 9 * 60];

fn render_screen(
    board: &mut CardputerAdv,
    wifi: Option<&Channel9Wifi>,
    ble: Option<&Channel9Ble>,
    time: &mut Channel9Time,
    config: &AppConfig,
    scan_results: &[WifiNetwork],
    recordings: &[RecordingItem],
    password_input: &str,
    channel9_input: &str,
    channel9_login: &Channel9LoginState,
    recorder_message: &str,
    screen: Screen,
) -> Result<()> {
    let status_bar = status_bar(wifi, ble, config.time.timezone_offset_minutes);
    match screen {
        Screen::Home => {
            let sd_mounted = board.sdcard_mounted();
            let wifi_connected = wifi
                .map(|wifi| wifi.status() == WifiStatus::Connected)
                .unwrap_or(false);
            channel9_ui::draw_home_screen(
                board.display_mut(),
                HomeView {
                    suggestion: "No pushes yet",
                    detail: if wifi_connected {
                        "WiFi connected"
                    } else {
                        "Open config to setup WiFi"
                    },
                    storage_label: &config.storage.sdcard_mount_path,
                    wifi_enabled: config.wifi.connect_at_startup,
                    sd_mounted,
                },
                status_bar,
            )
        }
        Screen::Config { selected } => {
            let items: Vec<MenuItem<'_>> = CONFIG_ITEMS
                .iter()
                .enumerate()
                .map(|(index, label)| MenuItem {
                    label,
                    selected: index == selected,
                    enabled: true,
                })
                .collect();
            channel9_ui::draw_menu_screen(
                board.display_mut(),
                "",
                "",
                &items,
                "SEL: Open  ESC: Back",
                status_bar,
            )
        }
        Screen::Wifi { selected } => {
            let saved_networks = config.wifi.credentials.len();
            let saved_networks_text = match wifi.map(|wifi| wifi.status()) {
                Some(WifiStatus::Connected) => "online",
                Some(WifiStatus::Failed) => "failed",
                _ if saved_networks == 0 => "0",
                _ => "saved",
            };
            let scan_value = if wifi.is_some() { "scan" } else { "no wifi" };
            let items = [
                SettingItem {
                    label: WIFI_ITEMS[0],
                    value: bool_label(config.wifi.connect_at_startup),
                    selected: false,
                    enabled: true,
                },
                SettingItem {
                    label: WIFI_ITEMS[1],
                    value: saved_networks_text,
                    selected: false,
                    enabled: true,
                },
                SettingItem {
                    label: WIFI_ITEMS[2],
                    value: saved_network_count_label(saved_networks),
                    selected: false,
                    enabled: saved_networks > 0,
                },
                SettingItem {
                    label: WIFI_ITEMS[3],
                    value: scan_value,
                    selected: false,
                    enabled: wifi.is_some(),
                },
                SettingItem {
                    label: WIFI_ITEMS[4],
                    value: "",
                    selected: false,
                    enabled: true,
                },
            ];
            let visible_items = visible_setting_items(&items, selected);
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "WIFI",
                "Connection setup",
                &visible_items,
                "SEL: Open/Toggle  ESC: Back",
                status_bar,
            )
        }
        Screen::WifiResult => {
            let status = wifi
                .map(|wifi| wifi.status())
                .map(wifi_status_label)
                .unwrap_or("unavailable");
            let info = wifi.and_then(|wifi| wifi.connection_info());
            let ssid = info
                .as_ref()
                .map(|info| info.ssid.as_str())
                .or_else(|| wifi.and_then(|wifi| wifi.last_ssid()))
                .or_else(|| {
                    config
                        .wifi
                        .credentials
                        .first()
                        .map(|credential| credential.ssid.as_str())
                })
                .unwrap_or("-");
            let ip = info.as_ref().map(|info| info.ip.as_str()).unwrap_or("-");
            let dns = info
                .as_ref()
                .map(|info| dns_label(info.dns_primary.as_str(), info.dns_secondary.as_str()))
                .unwrap_or_else(empty_dns_label);
            let message = wifi.and_then(|wifi| wifi.last_error()).unwrap_or("");
            channel9_ui::draw_wifi_status_screen(
                board.display_mut(),
                status,
                ssid,
                ip,
                dns.as_str(),
                message,
                "Enter/ESC: Back",
                status_bar,
            )
        }
        Screen::WifiSaved { selected } => {
            let active_ssid = wifi.and_then(|wifi| wifi.last_ssid());
            let start = selected.saturating_sub(3);
            let items: Vec<SettingItem<'_>> = config
                .wifi
                .credentials
                .iter()
                .skip(start)
                .take(4)
                .enumerate()
                .map(|(index, credential)| SettingItem {
                    label: credential.ssid.as_str(),
                    value: if Some(credential.ssid.as_str()) == active_ssid {
                        "active"
                    } else {
                        "saved"
                    },
                    selected: start + index == selected,
                    enabled: wifi.is_some(),
                })
                .collect();
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "SAVED",
                "Stored networks",
                &items,
                "SEL: Join  BS: Del  ESC: Back",
                status_bar,
            )
        }
        Screen::WifiScan { selected } => {
            let start = selected.saturating_sub(3);
            let items: Vec<SettingItem<'_>> = scan_results
                .iter()
                .skip(start)
                .take(4)
                .enumerate()
                .map(|(index, network)| SettingItem {
                    label: network.ssid.as_str(),
                    value: signal_label(network.signal_dbm),
                    selected: start + index == selected,
                    enabled: true,
                })
                .collect();
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "SCAN",
                "Nearby networks",
                &items,
                "SEL: Join  ESC: Back",
                status_bar,
            )
        }
        Screen::WifiPassword { network_index } => {
            let ssid = scan_results
                .get(network_index)
                .map(|network| network.ssid.as_str())
                .unwrap_or("unknown");
            channel9_ui::draw_password_screen(
                board.display_mut(),
                ssid,
                password_input.len(),
                "Enter: Save  ESC: Back",
                status_bar,
            )
        }
        Screen::Storage { selected } => {
            let items = [
                SettingItem {
                    label: STORAGE_ITEMS[0],
                    value: bool_label(config.storage.prefer_sdcard),
                    selected: selected == 0,
                    enabled: true,
                },
                SettingItem {
                    label: STORAGE_ITEMS[1],
                    value: config.storage.sdcard_mount_path.as_str(),
                    selected: selected == 1,
                    enabled: false,
                },
                SettingItem {
                    label: STORAGE_ITEMS[2],
                    value: if board.sdcard_mounted() {
                        "open"
                    } else {
                        "no sd"
                    },
                    selected: selected == 2,
                    enabled: board.sdcard_mounted(),
                },
                SettingItem {
                    label: STORAGE_ITEMS[3],
                    value: "",
                    selected: selected == 3,
                    enabled: true,
                },
            ];
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "STORAGE",
                "Persistence setup",
                &items,
                "SEL: Toggle/Open  ESC: Back",
                status_bar,
            )
        }
        Screen::Device => {
            let imu = board.imu_status();
            let audio = board.audio_status();
            let imu_value = imu_status_label(imu);
            let audio_value = audio_status_label(audio);
            let ble_value = ble
                .map(|ble| ble_status_label(ble.status()))
                .unwrap_or("missing");
            let items = [
                SettingItem {
                    label: "Keyboard",
                    value: bool_label(board.keyboard_available()),
                    selected: false,
                    enabled: board.keyboard_available(),
                },
                SettingItem {
                    label: "IMU",
                    value: imu_value.as_str(),
                    selected: false,
                    enabled: imu.present,
                },
                SettingItem {
                    label: "Audio",
                    value: audio_value.as_str(),
                    selected: false,
                    enabled: audio.codec_present,
                },
                SettingItem {
                    label: "BLE",
                    value: ble_value,
                    selected: false,
                    enabled: ble.is_some(),
                },
                SettingItem {
                    label: "SD",
                    value: bool_label(board.sdcard_mounted()),
                    selected: false,
                    enabled: board.sdcard_mounted(),
                },
            ];
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "DEVICE",
                "Board capability",
                &items,
                "Enter/ESC: Back",
                status_bar,
            )
        }
        Screen::Time { selected } => {
            let sync_status = time.status();
            let offset_value = timezone_offset_label(config.time.timezone_offset_minutes);
            let status_value = time_status_label(sync_status);
            let items = [
                SettingItem {
                    label: TIME_ITEMS[0],
                    value: bool_label(config.time.sync_at_startup),
                    selected: selected == 0,
                    enabled: true,
                },
                SettingItem {
                    label: TIME_ITEMS[1],
                    value: config.time.sntp_server.as_str(),
                    selected: selected == 1,
                    enabled: true,
                },
                SettingItem {
                    label: TIME_ITEMS[2],
                    value: offset_value.as_str(),
                    selected: selected == 2,
                    enabled: true,
                },
                SettingItem {
                    label: TIME_ITEMS[3],
                    value: status_value,
                    selected: selected == 3,
                    enabled: wifi
                        .map(|wifi| wifi.status() == WifiStatus::Connected)
                        .unwrap_or(false),
                },
                SettingItem {
                    label: TIME_ITEMS[4],
                    value: "",
                    selected: selected == 4,
                    enabled: true,
                },
            ];
            let visible_items = visible_setting_items(&items, selected);
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "TIME",
                "SNTP setup",
                &visible_items,
                "SEL: Toggle/Cycle  ESC: Back",
                status_bar,
            )
        }
        Screen::Audio { selected } => {
            let volume = percent_label(config.audio.speaker_volume_percent);
            let items = [
                SettingItem {
                    label: AUDIO_ITEMS[0],
                    value: volume.as_str(),
                    selected: selected == 0,
                    enabled: board.audio_status().codec_present,
                },
                SettingItem {
                    label: AUDIO_ITEMS[1],
                    value: "",
                    selected: selected == 1,
                    enabled: true,
                },
            ];
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "AUDIO",
                "Speaker output",
                &items,
                "LEFT/RIGHT: Volume  ESC: Back",
                status_bar,
            )
        }
        Screen::Channel9 { selected } => {
            let workspace = truncate_runtime_label(config.channel9.workspace_id.as_str());
            let device = truncate_runtime_label(config.channel9.device_id.as_str());
            let expires = channel9_epoch_label(config.channel9.token_expires_at);
            let code_value = channel9_user_code_label(config, wifi, channel9_login);
            let (items, footer) = if channel9_logged_in(config) {
                (
                    [
                        SettingItem {
                            label: CHANNEL9_STATUS_ITEMS[0],
                            value: "logged in",
                            selected: selected == 0,
                            enabled: false,
                        },
                        SettingItem {
                            label: CHANNEL9_STATUS_ITEMS[1],
                            value: workspace.as_str(),
                            selected: selected == 1,
                            enabled: false,
                        },
                        SettingItem {
                            label: CHANNEL9_STATUS_ITEMS[2],
                            value: device.as_str(),
                            selected: selected == 2,
                            enabled: false,
                        },
                        SettingItem {
                            label: CHANNEL9_STATUS_ITEMS[3],
                            value: "saved",
                            selected: selected == 3,
                            enabled: false,
                        },
                        SettingItem {
                            label: CHANNEL9_STATUS_ITEMS[4],
                            value: expires.as_str(),
                            selected: selected == 4,
                            enabled: false,
                        },
                        SettingItem {
                            label: CHANNEL9_STATUS_ITEMS[5],
                            value: "",
                            selected: selected == 5,
                            enabled: true,
                        },
                        SettingItem {
                            label: CHANNEL9_STATUS_ITEMS[6],
                            value: "",
                            selected: selected == 6,
                            enabled: true,
                        },
                    ],
                    if channel9_login.message.is_empty() {
                        "SEL: Clear  ESC: Back"
                    } else {
                        channel9_login.message.as_str()
                    },
                )
            } else {
                (
                    [
                        SettingItem {
                            label: CHANNEL9_LOGIN_ITEMS[0],
                            value: code_value.as_str(),
                            selected: selected == 0,
                            enabled: channel9_login_ready(config, wifi)
                                || channel9_login.active_code.is_some(),
                        },
                        SettingItem {
                            label: CHANNEL9_LOGIN_ITEMS[1],
                            value: device.as_str(),
                            selected: selected == 1,
                            enabled: true,
                        },
                        SettingItem {
                            label: CHANNEL9_LOGIN_ITEMS[2],
                            value: "check",
                            selected: selected == 2,
                            enabled: channel9_login.active_code.is_some(),
                        },
                        SettingItem {
                            label: CHANNEL9_LOGIN_ITEMS[3],
                            value: "new",
                            selected: selected == 3,
                            enabled: channel9_login_ready(config, wifi),
                        },
                        SettingItem {
                            label: CHANNEL9_LOGIN_ITEMS[4],
                            value: "",
                            selected: selected == 4,
                            enabled: true,
                        },
                        SettingItem {
                            label: "",
                            value: "",
                            selected: false,
                            enabled: false,
                        },
                        SettingItem {
                            label: "",
                            value: "",
                            selected: false,
                            enabled: false,
                        },
                    ],
                    if channel9_login.message.is_empty() {
                        "SEL: Poll/Edit  ESC: Back"
                    } else {
                        channel9_login.message.as_str()
                    },
                )
            };
            let visible_items =
                visible_setting_items(&items[..channel9_item_count(config)], selected);
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "CHANNEL9",
                "Device login",
                &visible_items,
                footer,
                status_bar,
            )
        }
        Screen::Channel9Input { field } => {
            let title = match field {
                Channel9InputField::Device => "DEVICE",
            };
            let input_value = truncate_runtime_label(channel9_input);
            let items = [SettingItem {
                label: title,
                value: input_value.as_str(),
                selected: true,
                enabled: true,
            }];
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "CHANNEL9",
                "Type value",
                &items,
                "Enter: Save  ESC: Back",
                status_bar,
            )
        }
        Screen::Recorder { selected } => {
            let recording_rows: Vec<(String, heapless::String<16>)> = recordings
                .iter()
                .map(|recording| {
                    (
                        recording.name.clone(),
                        recording_size_label(recording.size_bytes),
                    )
                })
                .collect();
            let mut items = Vec::with_capacity(recording_rows.len() + 1);
            items.push(SettingItem {
                label: "Record",
                value: if board.voice_recorder_available() {
                    "ready"
                } else {
                    "no audio"
                },
                selected: selected == 0,
                enabled: board.voice_recorder_available(),
            });
            for (index, (name, size_value)) in recording_rows.iter().enumerate() {
                items.push(SettingItem {
                    label: name.as_str(),
                    value: size_value.as_str(),
                    selected: selected == index + 1,
                    enabled: true,
                });
            }
            let visible_items = visible_setting_items(&items, selected);
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "REC",
                "Voice notes",
                &visible_items,
                "SEL: Rec/Play  BS: Del  ESC: Back",
                status_bar,
            )
        }
        Screen::Recording => {
            let items = [
                SettingItem {
                    label: "Recording",
                    value: if board.voice_recording_active() {
                        "writing"
                    } else {
                        "idle"
                    },
                    selected: true,
                    enabled: true,
                },
                SettingItem {
                    label: "Press to stop",
                    value: "",
                    selected: false,
                    enabled: true,
                },
            ];
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "REC",
                "Recording",
                &items,
                "Enter/ESC: Stop",
                status_bar,
            )
        }
        Screen::RecorderMessage => {
            let items = [SettingItem {
                label: recorder_message,
                value: "",
                selected: true,
                enabled: true,
            }];
            channel9_ui::draw_settings_screen(
                board.display_mut(),
                "REC",
                "Done",
                &items,
                "Enter/ESC: Back",
                status_bar,
            )
        }
        Screen::Files => {
            let files = load_file_list(board);
            let file_items: Vec<FileListItem<'_>> = files
                .iter()
                .map(|entry| FileListItem {
                    name: entry.name.as_str(),
                    is_dir: entry.is_dir,
                    operation: entry.operation,
                    size_bytes: entry.size_bytes,
                })
                .collect();
            channel9_ui::draw_file_list_screen(
                board.display_mut(),
                config.storage.sdcard_mount_path.as_str(),
                &file_items,
                "SEL/ESC: Back",
                status_bar,
            )
        }
    }
}

fn signal_label(signal_dbm: i8) -> &'static str {
    match signal_dbm {
        -55..=0 => "strong",
        -70..=-56 => "good",
        -85..=-71 => "weak",
        _ => "poor",
    }
}

fn bool_label(value: bool) -> &'static str {
    if value { "ON" } else { "OFF" }
}

fn saved_network_count_label(count: usize) -> &'static str {
    match count {
        0 => "0",
        1 => "1",
        _ => "many",
    }
}

fn recording_size_label(size_bytes: u64) -> heapless::String<16> {
    let mut value = heapless::String::<16>::new();
    if size_bytes < 1024 {
        let _ = core::fmt::write(&mut value, format_args!("{size_bytes}B"));
    } else {
        let _ = core::fmt::write(&mut value, format_args!("{}KB", size_bytes / 1024));
    }
    value
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn imu_status_label(status: ImuStatus) -> heapless::String<24> {
    let mut value = heapless::String::<24>::new();
    if let (true, Some(address), Some(chip_id)) = (status.present, status.address, status.chip_id) {
        let _ = core::fmt::write(
            &mut value,
            format_args!("{} 0x{address:02X}/0x{chip_id:02X}", status.name),
        );
    } else {
        let _ = value.push_str("missing");
    }
    value
}

fn audio_status_label(status: AudioStatus) -> heapless::String<24> {
    let mut value = heapless::String::<24>::new();
    if status.codec_present {
        let _ = core::fmt::write(
            &mut value,
            format_args!(
                "{} S{} M{} A{} {}%",
                status.codec_name,
                if status.speaker_ready { "Y" } else { "N" },
                if status.microphone_ready { "Y" } else { "N" },
                if status.microphone_alc_enabled {
                    "Y"
                } else {
                    "N"
                },
                status.speaker_volume_percent
            ),
        );
    } else {
        let _ = value.push_str("missing");
    }
    value
}

fn wifi_status_label(status: WifiStatus) -> &'static str {
    match status {
        WifiStatus::Disabled => "disabled",
        WifiStatus::Idle => "idle",
        WifiStatus::Started => "started",
        WifiStatus::Connected => "connected",
        WifiStatus::Failed => "failed",
    }
}

fn dns_label(primary: &str, secondary: &str) -> heapless::String<32> {
    let mut value = heapless::String::<32>::new();
    if secondary == "0.0.0.0" || secondary == primary {
        let _ = value.push_str(primary);
    } else {
        let _ = core::fmt::write(&mut value, format_args!("{primary}/{secondary}"));
    }
    value
}

fn empty_dns_label() -> heapless::String<32> {
    let mut value = heapless::String::<32>::new();
    let _ = value.push('-');
    value
}

fn ble_status_label(status: BleStatus) -> &'static str {
    match status {
        BleStatus::Idle => "idle",
        BleStatus::Ready => "ready",
        BleStatus::Advertising => "adv",
        BleStatus::Failed => "failed",
    }
}

fn time_status_label(status: TimeSyncStatus) -> &'static str {
    match status {
        TimeSyncStatus::Disabled => "off",
        TimeSyncStatus::Idle => "idle",
        TimeSyncStatus::InProgress => "syncing",
        TimeSyncStatus::Synced => "synced",
        TimeSyncStatus::Failed => "failed",
    }
}

fn timezone_offset_label(offset_minutes: i32) -> heapless::String<8> {
    let mut value = heapless::String::<8>::new();
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let absolute = offset_minutes.abs();
    let _ = core::fmt::write(&mut value, format_args!("UTC{sign}{:02}", absolute / 60));
    value
}

fn percent_label(percent: u8) -> heapless::String<8> {
    let mut value = heapless::String::<8>::new();
    let _ = core::fmt::write(&mut value, format_args!("{}%", percent.min(100)));
    value
}

fn next_sntp_server(current: &str) -> String {
    let next = SNTP_SERVERS
        .iter()
        .position(|server| *server == current)
        .map(|index| (index + 1) % SNTP_SERVERS.len())
        .unwrap_or(0);
    SNTP_SERVERS[next].to_owned()
}

fn next_timezone_offset(current: i32) -> i32 {
    TIMEZONE_OFFSETS
        .iter()
        .position(|offset| *offset == current)
        .map(|index| TIMEZONE_OFFSETS[(index + 1) % TIMEZONE_OFFSETS.len()])
        .unwrap_or(TIMEZONE_OFFSETS[0])
}

fn status_bar(
    wifi: Option<&Channel9Wifi>,
    ble: Option<&Channel9Ble>,
    offset_minutes: i32,
) -> StatusBar {
    let wifi = match wifi.map(|wifi| wifi.status()) {
        Some(WifiStatus::Connected) => StatusWifi::Connected,
        Some(WifiStatus::Failed) => StatusWifi::Failed,
        Some(WifiStatus::Started) => StatusWifi::Started,
        Some(WifiStatus::Disabled | WifiStatus::Idle) | None => StatusWifi::Off,
    };
    let ble = match ble.map(|ble| ble.status()) {
        Some(BleStatus::Advertising) => StatusBle::Advertising,
        Some(BleStatus::Ready | BleStatus::Idle) => StatusBle::Ready,
        Some(BleStatus::Failed) => StatusBle::Failed,
        None => StatusBle::Off,
    };
    StatusBar {
        wifi,
        ble,
        hour_minute: format_clock(offset_minutes),
    }
}

fn spawn_ui_scheduler(events: SyncSender<UiEvent>) -> Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("channel9-ui-scheduler".to_owned())
        .stack_size(UI_SCHEDULER_STACK_BYTES)
        .spawn(move || {
            loop {
                thread::sleep(UI_CLOCK_TICK_INTERVAL);
                match events.try_send(UiEvent::ClockTick) {
                    Ok(()) | Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => break,
                }
            }
        })
        .map_err(|err| anyhow::anyhow!("failed to spawn ui scheduler task: {err:?}"))
}

#[allow(clippy::too_many_arguments)]
fn drain_ui_events(
    events: &Receiver<UiEvent>,
    board: &mut CardputerAdv,
    wifi: Option<&Channel9Wifi>,
    ble: Option<&Channel9Ble>,
    time: &mut Channel9Time,
    config: &AppConfig,
    scan_results: &[WifiNetwork],
    recordings: &[RecordingItem],
    password_input: &str,
    channel9_input: &str,
    channel9_login: &Channel9LoginState,
    recorder_message: &str,
    screen: Screen,
    rendered_clock: &mut heapless::String<6>,
) -> Result<()> {
    loop {
        match events.try_recv() {
            Ok(UiEvent::ClockTick) => {
                if screen == Screen::Recording {
                    continue;
                }

                let current_clock = format_clock(config.time.timezone_offset_minutes);
                if current_clock == *rendered_clock {
                    continue;
                }

                render_screen(
                    board,
                    wifi,
                    ble,
                    time,
                    config,
                    scan_results,
                    recordings,
                    password_input,
                    channel9_input,
                    channel9_login,
                    recorder_message,
                    screen,
                )?;
                *rendered_clock = current_clock;
            }
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

fn visible_setting_items<'a>(items: &[SettingItem<'a>], selected: usize) -> Vec<SettingItem<'a>> {
    let start = selected.saturating_sub(3);
    items
        .iter()
        .skip(start)
        .take(4)
        .enumerate()
        .map(|(index, item)| SettingItem {
            selected: start + index == selected,
            ..*item
        })
        .collect()
}

fn apply_wifi_setting(board: &CardputerAdv, config: &mut AppConfig, selected: usize) {
    match selected {
        0 => {
            config.wifi.connect_at_startup = !config.wifi.connect_at_startup;
            save_config(board, config);
        }
        3 => {}
        _ => log::info!(
            "wifi setting is not implemented yet: {}",
            WIFI_ITEMS[selected]
        ),
    }
}

fn apply_storage_setting(board: &CardputerAdv, config: &mut AppConfig, selected: usize) {
    if selected == 0 {
        config.storage.prefer_sdcard = !config.storage.prefer_sdcard;
        save_config(board, config);
    }
}

fn apply_audio_config(board: &mut CardputerAdv, config: &AppConfig) {
    if let Err(err) = board.set_speaker_volume_percent(config.audio.speaker_volume_percent) {
        log::warn!("speaker volume apply failed: {err:?}");
    }
}

fn create_channel9_device_code(
    board: &CardputerAdv,
    wifi: Option<&mut Channel9Wifi>,
    config: &mut AppConfig,
    login: &mut Channel9LoginState,
) {
    login.message.clear();
    if let Some(reason) = channel9_login_blocked_reason(config, wifi.as_deref()) {
        login.message = reason.to_owned();
        return;
    }

    let client = Channel9HttpClient::new(CHANNEL9_API_BASE_URL);
    match client.create_device_code(
        config.channel9.device_id.as_str(),
        &config.channel9.interfaces,
    ) {
        Ok(code) => {
            let user_code = channel9_format_user_code(code.user_code.as_str());
            login.message = format!("Code {user_code}");
            login.active_code = Some(code);
            config.channel9.access_token = None;
            config.channel9.token_expires_at = None;
            save_config(board, config);
        }
        Err(err) => {
            log::warn!("channel9 device code create failed: {err:?}");
            login.message = channel9_error_label("Create failed", &err);
        }
    }
}

fn ensure_channel9_device_id(board: &CardputerAdv, config: &mut AppConfig) {
    let current = config.channel9.device_id.trim();
    if !current.is_empty() && current != "cardputer-adv" {
        return;
    }

    let random = unsafe { esp_idf_svc::sys::esp_random() };
    config.channel9.device_id = format!("cardputer_{random:08x}");
    save_config(board, config);
}

fn poll_channel9_device_token(
    board: &CardputerAdv,
    wifi: Option<&mut Channel9Wifi>,
    config: &mut AppConfig,
    login: &mut Channel9LoginState,
) {
    login.message.clear();
    if !wifi
        .as_deref()
        .map(|wifi| wifi.status() == WifiStatus::Connected)
        .unwrap_or(false)
    {
        login.message = "WiFi offline".to_owned();
        return;
    }

    let Some(code) = login.active_code.as_ref() else {
        login.message = "Create code first".to_owned();
        return;
    };

    let client = Channel9HttpClient::new(CHANNEL9_API_BASE_URL);
    match client.poll_device_token(code.device_code.as_str()) {
        Ok(PollToken::Pending) => {
            let user_code = channel9_format_user_code(code.user_code.as_str());
            login.message = format!("Pending {user_code}");
        }
        Ok(PollToken::Approved(token)) => {
            config.channel9.access_token = Some(token.access_token);
            config.channel9.token_expires_at = Some(token.expires_at);
            config.channel9.workspace_id = token.workspace_id;
            config.channel9.device_id = token.device_id;
            login.message = "Login saved".to_owned();
            login.active_code = None;
            save_config(board, config);
        }
        Err(err) => {
            log::warn!("channel9 token poll failed: {err:?}");
            login.message = channel9_error_label("Poll failed", &err);
        }
    }
}

fn channel9_login_ready(config: &AppConfig, wifi: Option<&Channel9Wifi>) -> bool {
    channel9_login_blocked_reason(config, wifi).is_none()
}

fn channel9_login_blocked_reason(
    config: &AppConfig,
    wifi: Option<&Channel9Wifi>,
) -> Option<&'static str> {
    if config.channel9.device_id.trim().is_empty() {
        return Some("Set Device first");
    }
    if !wifi
        .map(|wifi| wifi.status() == WifiStatus::Connected)
        .unwrap_or(false)
    {
        return Some("WiFi offline");
    }
    None
}

fn channel9_logged_in(config: &AppConfig) -> bool {
    config.channel9.access_token.is_some()
}

fn channel9_item_count(config: &AppConfig) -> usize {
    if channel9_logged_in(config) {
        CHANNEL9_STATUS_ITEMS.len()
    } else {
        CHANNEL9_LOGIN_ITEMS.len()
    }
}

fn channel9_user_code_label(
    config: &AppConfig,
    wifi: Option<&Channel9Wifi>,
    login: &Channel9LoginState,
) -> heapless::String<24> {
    let mut label = heapless::String::new();
    if let Some(code) = login.active_code.as_ref() {
        let user_code = channel9_format_user_code(code.user_code.as_str());
        let _ = label.push_str(user_code.as_str());
    } else if let Some(reason) = channel9_login_blocked_reason(config, wifi) {
        let _ = label.push_str(reason);
    } else {
        let _ = label.push_str("Enter Create");
    }
    label
}

fn channel9_format_user_code(value: &str) -> heapless::String<16> {
    let mut output = heapless::String::<16>::new();
    let normalized = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase());
    for (index, ch) in normalized.take(12).enumerate() {
        if index == 4 {
            let _ = output.push('-');
        }
        let _ = output.push(ch);
    }
    output
}

fn channel9_error_label(prefix: &str, error: &anyhow::Error) -> String {
    let detail = error.to_string();
    if detail.is_empty() {
        return prefix.to_owned();
    }
    let mut message = String::with_capacity(prefix.len() + 2 + 32);
    message.push_str(prefix);
    message.push_str(": ");
    message.extend(detail.chars().take(32));
    message
}

fn channel9_epoch_label(value: Option<i64>) -> heapless::String<24> {
    let mut label = heapless::String::new();
    if let Some(value) = value {
        let _ = core::fmt::write(&mut label, format_args!("{value}"));
    } else {
        let _ = label.push_str("-");
    }
    label
}

fn truncate_runtime_label(value: &str) -> heapless::String<24> {
    let mut output = heapless::String::<24>::new();
    let mut chars = value.chars();
    for _ in 0..23 {
        let Some(ch) = chars.next() else {
            return output;
        };
        let _ = output.push(ch);
    }
    if chars.next().is_some() {
        let _ = output.push('~');
    }
    output
}

#[derive(Debug, Default, Clone)]
struct PausedRadios {
    wifi_was_connected: bool,
    wifi_ssid: Option<String>,
    ble_was_advertising: bool,
}

fn pause_radios_for_recording(
    wifi: Option<&mut Channel9Wifi>,
    ble: Option<&Channel9Ble>,
) -> PausedRadios {
    let mut paused = PausedRadios {
        wifi_was_connected: wifi
            .as_ref()
            .map(|wifi| wifi.status() == WifiStatus::Connected)
            .unwrap_or(false),
        wifi_ssid: wifi
            .as_ref()
            .and_then(|wifi| wifi.last_ssid().map(str::to_owned)),
        ble_was_advertising: ble
            .as_ref()
            .map(|ble| ble.status() == BleStatus::Advertising)
            .unwrap_or(false),
    };

    if let Some(wifi) = wifi {
        if let Err(err) = wifi.stop() {
            log::warn!("wifi stop before recording failed: {err:?}");
            paused.wifi_was_connected = false;
        }
    }

    if let Some(ble) = ble {
        if let Err(err) = ble.stop_advertising() {
            log::warn!("ble pause before recording failed: {err:?}");
            paused.ble_was_advertising = false;
        }
    }

    paused
}

fn resume_radios_after_recording(
    wifi: Option<&mut Channel9Wifi>,
    ble: Option<&Channel9Ble>,
    config: &AppConfig,
    paused: &PausedRadios,
    time: &mut Channel9Time,
) {
    if let Some(wifi) = wifi.filter(|_| paused.wifi_was_connected) {
        match reconnect_paused_wifi(wifi, config, paused.wifi_ssid.as_deref()) {
            Ok(Some(_)) => {
                if let Err(err) = time.sync_now(&config.time) {
                    log::warn!("sntp sync after wifi resume failed: {err:?}");
                }
            }
            Ok(None) => {}
            Err(err) => log::warn!("wifi resume after recording failed: {err:?}"),
        }
    }

    if let Some(ble) = ble.filter(|_| paused.ble_was_advertising) {
        if let Err(err) = ble.start_advertising() {
            log::warn!("ble resume after recording failed: {err:?}");
        }
    }
}

fn reconnect_paused_wifi(
    wifi: &mut Channel9Wifi,
    config: &AppConfig,
    paused_ssid: Option<&str>,
) -> Result<Option<String>> {
    if let Some(ssid) = paused_ssid {
        if let Some(credential) = config
            .wifi
            .credentials
            .iter()
            .find(|item| item.ssid == ssid)
        {
            wifi.connect(credential)?;
            return Ok(Some(credential.ssid.clone()));
        }
    }

    wifi.connect_first_saved(&config.wifi)
}

fn adjust_speaker_volume(board: &mut CardputerAdv, config: &mut AppConfig, delta: i8) {
    let current = config.audio.speaker_volume_percent.min(100) as i16;
    let next = (current + delta as i16).clamp(0, 100) as u8;
    if next == config.audio.speaker_volume_percent {
        return;
    }

    config.audio.speaker_volume_percent = next;
    apply_audio_config(board, config);
}

fn upsert_wifi_credential(config: &mut AppConfig, credential: WifiCredential) {
    if let Some(saved) = config
        .wifi
        .credentials
        .iter_mut()
        .find(|saved| saved.ssid == credential.ssid)
    {
        *saved = credential;
    } else {
        config.wifi.credentials.push(credential);
    }
}

fn save_config(board: &CardputerAdv, config: &AppConfig) {
    if !board.sdcard_mounted() {
        log::warn!("sdcard is not mounted; config change is not persisted");
        return;
    }

    let store = JsonConfigStore::new(board.storage_root());
    if let Err(err) = store.save(config) {
        log::warn!("failed to save config: {err:?}");
    }
}

fn load_file_list(board: &CardputerAdv) -> Vec<channel9_storage::FileEntry> {
    if !board.sdcard_mounted() {
        return Vec::new();
    }

    match list_directory(board.storage_root(), 12) {
        Ok(files) => files,
        Err(err) => {
            log::warn!("failed to list sdcard files: {err:?}");
            Vec::new()
        }
    }
}

fn load_recordings(board: &CardputerAdv) -> Vec<RecordingItem> {
    if !board.sdcard_mounted() {
        return Vec::new();
    }

    let recordings_dir = channel9_fs::recordings_dir(board.storage_root());
    let Ok(entries) = std::fs::read_dir(recordings_dir) else {
        return Vec::new();
    };

    let mut recordings = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("wav") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        recordings.push(RecordingItem {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
            size_bytes: metadata.len(),
        });
    }
    recordings.sort_by(|left, right| right.name.cmp(&left.name));
    recordings
}

fn load_config(board: &CardputerAdv) -> AppConfig {
    if !board.sdcard_mounted() {
        log::warn!("sdcard is not mounted; using default config");
        return AppConfig::default();
    }

    let store = JsonConfigStore::new(board.storage_root());
    match store.load_or_create() {
        Ok(config) => config,
        Err(err) => {
            log::warn!("failed to load config from sdcard; using defaults: {err:?}");
            AppConfig::default()
        }
    }
}
