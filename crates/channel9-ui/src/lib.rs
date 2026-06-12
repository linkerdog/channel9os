use anyhow::Result;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{
    Arc, Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, Triangle,
};
use embedded_graphics::text::Text;

const SCREEN_WIDTH: i32 = 240;
const SCREEN_HEIGHT: i32 = 135;
const BORDER_X: i32 = 5;
const BORDER_Y: i32 = 5;
const STATUS_BAR_HEIGHT: i32 = 20;
const CONTENT_Y: i32 = 54;
const ROW_X: i32 = 10;
const ROW_START_Y: i32 = 58;
const ROW_HEIGHT: i32 = 15;
const MAX_FILE_ROWS: usize = 4;

const BG: Rgb565 = Rgb565::new(28, 57, 28);
const PANEL: Rgb565 = Rgb565::new(31, 63, 31);
const BAR: Rgb565 = Rgb565::new(0, 28, 31);
const PRIMARY: Rgb565 = Rgb565::new(0, 0, 0);
const SECONDARY: Rgb565 = Rgb565::new(3, 18, 24);
const OPERATION: Rgb565 = Rgb565::new(31, 36, 0);
const SELECTED: Rgb565 = Rgb565::new(0, 28, 31);
const MUTED: Rgb565 = Rgb565::new(18, 38, 18);
const WHITE: Rgb565 = Rgb565::new(31, 63, 31);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileListItem<'a> {
    pub name: &'a str,
    pub is_dir: bool,
    pub operation: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeView<'a> {
    pub suggestion: &'a str,
    pub detail: &'a str,
    pub storage_label: &'a str,
    pub wifi_enabled: bool,
    pub sd_mounted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBar {
    pub wifi: StatusWifi,
    pub ble: StatusBle,
    pub hour_minute: heapless::String<6>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusWifi {
    Off,
    Started,
    Connected,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusBle {
    Off,
    Ready,
    Advertising,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuItem<'a> {
    pub label: &'a str,
    pub selected: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingItem<'a> {
    pub label: &'a str,
    pub value: &'a str,
    pub selected: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel9View<'a> {
    pub logged_in: bool,
    pub selected: usize,
    pub user_code: &'a str,
    pub device: &'a str,
    pub workspace: &'a str,
    pub expires: &'a str,
    pub message: &'a str,
    pub ready: bool,
    pub has_active_code: bool,
}

pub fn draw_hello_screen<D>(display: &mut D, storage_path: &str) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(Rgb565::BLACK)
        .map_err(|err| anyhow::anyhow!("display clear failed: {err:?}"))?;

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let body_style = MonoTextStyle::new(&FONT_6X10, Rgb565::CYAN);

    Text::new("Channel9", Point::new(12, 32), title_style)
        .draw(display)
        .map_err(|err| anyhow::anyhow!("title draw failed: {err:?}"))?;

    Text::new("hello world on ESP32-S3", Point::new(12, 58), body_style)
        .draw(display)
        .map_err(|err| anyhow::anyhow!("body draw failed: {err:?}"))?;

    Text::new("Cardputer-Adv", Point::new(12, 80), body_style)
        .draw(display)
        .map_err(|err| anyhow::anyhow!("device draw failed: {err:?}"))?;

    Text::new(storage_path, Point::new(12, 104), body_style)
        .draw(display)
        .map_err(|err| anyhow::anyhow!("storage draw failed: {err:?}"))?;

    Ok(())
}

pub fn draw_file_list_screen<D>(
    display: &mut D,
    storage_path: &str,
    items: &[FileListItem<'_>],
    footer: &str,
    status: StatusBar,
) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(BG)
        .map_err(|err| anyhow::anyhow!("display clear failed: {err:?}"))?;

    draw_shell(display, "FILES", storage_path, footer, status)?;

    if items.is_empty() {
        draw_text(display, "No files", Point::new(ROW_X + 11, 58), PRIMARY)?;
        return Ok(());
    }

    draw_file_rows(display, items, 0)?;

    Ok(())
}

pub fn draw_home_screen<D>(display: &mut D, view: HomeView<'_>, status: StatusBar) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(BG)
        .map_err(|err| anyhow::anyhow!("display clear failed: {err:?}"))?;

    draw_shell(display, "", "Push suggestions", "Enter: Config", status)?;

    Rectangle::new(Point::new(12, 54), Size::new(216, 48))
        .into_styled(panel_style(false))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("suggestion panel draw failed: {err:?}"))?;

    draw_text(display, view.suggestion, Point::new(18, 72), PRIMARY)?;
    draw_text(display, view.detail, Point::new(18, 88), MUTED)?;

    let mut status = heapless::String::<48>::new();
    let _ = core::fmt::write(
        &mut status,
        format_args!(
            "SD:{} WIFI:{}",
            if view.sd_mounted { "ON" } else { "OFF" },
            if view.wifi_enabled { "AUTO" } else { "OFF" }
        ),
    );
    draw_text(display, status.as_str(), Point::new(18, 113), MUTED)?;
    draw_selected_button(display, "CONFIG", Point::new(158, 105), Size::new(62, 17))?;

    Ok(())
}

pub fn draw_menu_screen<D>(
    display: &mut D,
    _title: &str,
    _subtitle: &str,
    items: &[MenuItem<'_>],
    footer: &str,
    status: StatusBar,
) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(BG)
        .map_err(|err| anyhow::anyhow!("display clear failed: {err:?}"))?;
    draw_menu_shell(display, footer, status)?;

    if items.is_empty() {
        draw_text(display, "No config items", Point::new(18, 82), MUTED)?;
        return Ok(());
    }

    let selected = items.iter().position(|item| item.selected).unwrap_or(0);
    let prev = if selected == 0 {
        items.len() - 1
    } else {
        selected - 1
    };
    let next = (selected + 1) % items.len();
    let current = items[selected];

    draw_menu_arrow(display, Point::new(13, 72), false)?;
    draw_menu_arrow(display, Point::new(227, 72), true)?;
    draw_app_tile(display, items[prev], Point::new(32, 46), false)?;
    draw_app_tile(display, current, Point::new(91, 38), true)?;
    draw_app_tile(display, items[next], Point::new(162, 46), false)?;

    Ok(())
}

pub fn draw_settings_screen<D>(
    display: &mut D,
    title: &str,
    subtitle: &str,
    items: &[SettingItem<'_>],
    footer: &str,
    status: StatusBar,
) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(BG)
        .map_err(|err| anyhow::anyhow!("display clear failed: {err:?}"))?;
    draw_shell(display, title, subtitle, footer, status)?;

    if items.is_empty() {
        draw_text(display, "No settings", Point::new(18, 82), MUTED)?;
        return Ok(());
    }

    Rectangle::new(Point::new(10, CONTENT_Y), Size::new(220, 62))
        .into_styled(panel_style(false))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("settings panel draw failed: {err:?}"))?;

    for (index, item) in items.iter().take(4).enumerate() {
        draw_setting_row(display, item, 13, CONTENT_Y + 4 + index as i32 * 14)?;
    }

    Ok(())
}

pub fn draw_channel9_screen<D>(
    display: &mut D,
    view: Channel9View<'_>,
    status: StatusBar,
) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(BG)
        .map_err(|err| anyhow::anyhow!("display clear failed: {err:?}"))?;

    let footer = if view.message.is_empty() {
        if view.logged_in {
            "ENTER: Clear  ESC: Back"
        } else if view.has_active_code {
            "ENTER: Poll  RIGHT: More"
        } else {
            "ENTER: Create  ESC: Back"
        }
    } else {
        view.message
    };
    draw_shell(
        display,
        "CHANNEL9",
        if view.logged_in {
            "Connected"
        } else {
            "Device login"
        },
        footer,
        status,
    )?;

    Rectangle::new(Point::new(10, 54), Size::new(220, 49))
        .into_styled(panel_style(false))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("channel9 panel draw failed: {err:?}"))?;

    if view.logged_in {
        draw_text(display, "CONNECTED", Point::new(18, 72), OPERATION)?;
        draw_text(display, "Workspace", Point::new(18, 87), MUTED)?;
        draw_text(
            display,
            truncate_name(view.workspace, 18).as_str(),
            Point::new(82, 87),
            PRIMARY,
        )?;
        draw_text(display, "Device", Point::new(18, 100), MUTED)?;
        draw_text(
            display,
            truncate_name(view.device, 21).as_str(),
            Point::new(82, 100),
            PRIMARY,
        )?;
        draw_channel9_action_bar(
            display,
            &[("CLEAR", view.selected == 5), ("BACK", view.selected == 6)],
        )?;
        return Ok(());
    }

    if view.has_active_code {
        draw_centered_big_text(display, view.user_code, 82, OPERATION)?;
        draw_text(
            display,
            "Open LinkerDog and approve",
            Point::new(47, 99),
            MUTED,
        )?;
    } else {
        draw_centered_big_text(
            display,
            if view.ready { "READY" } else { "WAIT" },
            80,
            if view.ready { OPERATION } else { MUTED },
        )?;
        draw_text(
            display,
            truncate_name(view.user_code, 30).as_str(),
            Point::new(24, 99),
            MUTED,
        )?;
    }

    let primary = if view.has_active_code {
        "POLL"
    } else {
        "CREATE"
    };
    draw_channel9_action_bar(
        display,
        &[
            (primary, view.selected == 0 || view.selected == 2),
            (
                if view.has_active_code {
                    "POLL"
                } else {
                    "RETRY"
                },
                view.selected == 3,
            ),
            ("DEVICE", view.selected == 1),
            ("BACK", view.selected == 4),
        ],
    )
}

pub fn draw_password_screen<D>(
    display: &mut D,
    ssid: &str,
    password_len: usize,
    footer: &str,
    status: StatusBar,
) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(BG)
        .map_err(|err| anyhow::anyhow!("display clear failed: {err:?}"))?;
    draw_shell(display, "WIFI", "Password", footer, status)?;

    draw_text(display, "SSID", Point::new(18, 76), MUTED)?;
    draw_text(
        display,
        truncate_name(ssid, 24).as_str(),
        Point::new(64, 76),
        PRIMARY,
    )?;

    Rectangle::new(Point::new(18, 88), Size::new(204, 18))
        .into_styled(panel_style(true))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("password input draw failed: {err:?}"))?;

    let mut masked = heapless::String::<34>::new();
    let visible_len = password_len.min(28);
    for _ in 0..visible_len {
        let _ = masked.push('*');
    }
    if password_len > visible_len {
        let _ = masked.push_str("...");
    }
    if masked.is_empty() {
        let _ = masked.push('_');
    }

    draw_text(display, masked.as_str(), Point::new(24, 101), WHITE)?;
    draw_text(
        display,
        "Type password, Enter to save",
        Point::new(18, 118),
        MUTED,
    )?;

    Ok(())
}

pub fn draw_wifi_status_screen<D>(
    display: &mut D,
    status: &str,
    ssid: &str,
    ip: &str,
    dns: &str,
    message: &str,
    footer: &str,
    bar: StatusBar,
) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    display
        .clear(BG)
        .map_err(|err| anyhow::anyhow!("display clear failed: {err:?}"))?;
    draw_shell(display, "WIFI", "Status", footer, bar)?;

    draw_text(display, "State", Point::new(18, 72), MUTED)?;
    draw_text(display, status, Point::new(72, 72), OPERATION)?;

    draw_text(display, "SSID", Point::new(18, 86), MUTED)?;
    draw_text(
        display,
        truncate_name(ssid, 22).as_str(),
        Point::new(72, 86),
        PRIMARY,
    )?;

    draw_text(display, "IP", Point::new(18, 100), MUTED)?;
    draw_text(display, ip, Point::new(72, 100), PRIMARY)?;

    draw_text(display, "DNS", Point::new(18, 114), MUTED)?;
    draw_text(
        display,
        truncate_name(dns, 22).as_str(),
        Point::new(72, 114),
        PRIMARY,
    )?;

    if !message.is_empty() {
        draw_text(
            display,
            truncate_name(message, 32).as_str(),
            Point::new(18, 126),
            MUTED,
        )?;
    }

    Ok(())
}

fn draw_shell<D>(
    display: &mut D,
    title: &str,
    path: &str,
    footer: &str,
    status: StatusBar,
) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Rectangle::new(
        Point::new(0, 0),
        Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(BG))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("shell background draw failed: {err:?}"))?;

    Rectangle::new(
        Point::new(BORDER_X, BORDER_Y),
        Size::new(
            (SCREEN_WIDTH - BORDER_X * 2) as u32,
            STATUS_BAR_HEIGHT as u32,
        ),
    )
    .into_styled(PrimitiveStyle::with_fill(BAR))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("status bar draw failed: {err:?}"))?;

    draw_text(display, "Channel9", Point::new(10, 18), WHITE)?;
    draw_text(
        display,
        status.hour_minute.as_str(),
        Point::new(91, 18),
        WHITE,
    )?;
    draw_status_wifi_icon(display, Point::new(147, 6), status.wifi)?;
    draw_status_ble_icon(display, Point::new(176, 7), status.ble)?;
    draw_battery(display, Point::new(199, 8), 72)?;

    if !title.is_empty() {
        draw_text(display, title, Point::new(12, 39), PRIMARY)?;
    }

    draw_text(display, path, Point::new(12, 50), MUTED)?;
    draw_text(display, footer, Point::new(12, 128), MUTED)?;

    Ok(())
}

fn draw_menu_shell<D>(display: &mut D, footer: &str, status: StatusBar) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Rectangle::new(
        Point::new(0, 0),
        Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(BG))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("menu background draw failed: {err:?}"))?;

    Rectangle::new(
        Point::new(BORDER_X, BORDER_Y),
        Size::new(
            (SCREEN_WIDTH - BORDER_X * 2) as u32,
            STATUS_BAR_HEIGHT as u32,
        ),
    )
    .into_styled(PrimitiveStyle::with_fill(BAR))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("menu status bar draw failed: {err:?}"))?;

    draw_text(display, "Channel9", Point::new(10, 18), WHITE)?;
    draw_text(
        display,
        status.hour_minute.as_str(),
        Point::new(91, 18),
        WHITE,
    )?;
    draw_status_wifi_icon(display, Point::new(147, 6), status.wifi)?;
    draw_status_ble_icon(display, Point::new(176, 7), status.ble)?;
    draw_battery(display, Point::new(199, 8), 72)?;
    draw_text(display, footer, Point::new(12, 128), MUTED)
}

fn draw_menu_arrow<D>(display: &mut D, center: Point, right: bool) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    let points = if right {
        (
            Point::new(center.x - 6, center.y - 12),
            Point::new(center.x + 7, center.y),
            Point::new(center.x - 6, center.y + 12),
        )
    } else {
        (
            Point::new(center.x + 6, center.y - 12),
            Point::new(center.x - 7, center.y),
            Point::new(center.x + 6, center.y + 12),
        )
    };

    Triangle::new(points.0, points.1, points.2)
        .into_styled(PrimitiveStyle::with_fill(OPERATION))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("menu arrow draw failed: {err:?}"))?;
    Ok(())
}

fn draw_app_tile<D>(
    display: &mut D,
    item: MenuItem<'_>,
    origin: Point,
    selected: bool,
) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    let size = if selected {
        Size::new(58, 56)
    } else {
        Size::new(48, 46)
    };
    let fill = if selected { SELECTED } else { PANEL };
    let stroke = if selected { OPERATION } else { PANEL };
    Rectangle::new(origin, size)
        .into_styled(
            PrimitiveStyleBuilder::new()
                .fill_color(fill)
                .stroke_color(stroke)
                .stroke_width(2)
                .build(),
        )
        .draw(display)
        .map_err(|err| anyhow::anyhow!("app tile draw failed: {err:?}"))?;

    let center = Point::new(origin.x + size.width as i32 / 2, origin.y + 18);
    draw_config_icon(
        display,
        item.label,
        center,
        if selected { WHITE } else { SECONDARY },
    )?;
    draw_centered_small_text_in(
        display,
        item.label,
        origin.x,
        size.width as i32,
        origin.y + size.height as i32 - 4,
        if item.enabled {
            if selected {
                WHITE
            } else {
                PRIMARY
            }
        } else {
            MUTED
        },
    )
}

fn draw_setting_row<D>(display: &mut D, item: &SettingItem<'_>, x: i32, y: i32) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    let fill = if item.selected { SELECTED } else { PANEL };
    let text = if !item.enabled {
        MUTED
    } else if item.selected {
        WHITE
    } else {
        PRIMARY
    };
    let stroke = if item.selected { OPERATION } else { PANEL };
    Rectangle::new(Point::new(x, y), Size::new(214, 13))
        .into_styled(
            PrimitiveStyleBuilder::new()
                .fill_color(fill)
                .stroke_color(stroke)
                .stroke_width(1)
                .build(),
        )
        .draw(display)
        .map_err(|err| anyhow::anyhow!("setting row draw failed: {err:?}"))?;

    draw_text(
        display,
        truncate_name(item.label, 17).as_str(),
        Point::new(x + 8, y + 10),
        text,
    )?;
    draw_text(
        display,
        truncate_name(item.value, 11).as_str(),
        Point::new(x + 138, y + 10),
        text,
    )?;
    draw_chevron(display, Point::new(x + 204, y + 6), text)
}

fn draw_file_rows<D>(
    display: &mut D,
    items: &[FileListItem<'_>],
    selected_index: usize,
) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    let start = selected_index.saturating_sub(MAX_FILE_ROWS - 1);
    Rectangle::new(Point::new(10, 54), Size::new(220, 64))
        .into_styled(panel_style(false))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("file panel draw failed: {err:?}"))?;

    for (row, item) in items.iter().skip(start).take(MAX_FILE_ROWS).enumerate() {
        let item_index = start + row;
        let selected = item_index == selected_index;
        let y = ROW_START_Y + row as i32 * ROW_HEIGHT;
        let fill = if selected { SELECTED } else { PANEL };
        let color = if selected {
            WHITE
        } else if item.operation {
            OPERATION
        } else if item.is_dir {
            SECONDARY
        } else {
            PRIMARY
        };
        Rectangle::new(Point::new(13, y - 10), Size::new(214, 14))
            .into_styled(PrimitiveStyle::with_fill(fill))
            .draw(display)
            .map_err(|err| anyhow::anyhow!("file row background draw failed: {err:?}"))?;
        let mut line = heapless::String::<36>::new();

        let name = truncate_name(item.name, 24);
        if item.operation || item.is_dir {
            let _ = core::fmt::write(&mut line, format_args!("{name}"));
        } else {
            let _ = core::fmt::write(
                &mut line,
                format_args!("{name} {}", format_size(item.size_bytes)),
            );
        }

        draw_text(display, line.as_str(), Point::new(ROW_X + 8, y), color)?;
        if selected {
            draw_chevron(display, Point::new(216, y - 4), WHITE)?;
        }
    }

    if items.len() > MAX_FILE_ROWS {
        let mut scroll = heapless::String::<24>::new();
        let _ = core::fmt::write(
            &mut scroll,
            format_args!("{}/{}", selected_index + 1, items.len()),
        );
        draw_text(display, scroll.as_str(), Point::new(204, 128), MUTED)?;
    }

    Ok(())
}

fn draw_status_wifi_icon<D>(display: &mut D, origin: Point, status: StatusWifi) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    let color = match status {
        StatusWifi::Connected => WHITE,
        StatusWifi::Failed => OPERATION,
        StatusWifi::Started | StatusWifi::Off => Rgb565::new(16, 40, 28),
    };
    let center = Point::new(origin.x + 11, origin.y + 13);

    Circle::new(Point::new(center.x - 2, center.y + 1), 4)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("status wifi dot draw failed: {err:?}"))?;
    for diameter in [14_u32, 22_u32] {
        Arc::new(
            Point::new(
                center.x - diameter as i32 / 2,
                center.y + 2 - diameter as i32 / 2,
            ),
            diameter,
            225.0.deg(),
            90.0.deg(),
        )
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("status wifi arc draw failed: {err:?}"))?;
    }

    if status == StatusWifi::Failed {
        Line::new(
            Point::new(origin.x + 20, origin.y + 2),
            Point::new(origin.x + 27, origin.y + 9),
        )
        .into_styled(PrimitiveStyle::with_stroke(OPERATION, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("status wifi fail draw failed: {err:?}"))?;
        Line::new(
            Point::new(origin.x + 27, origin.y + 2),
            Point::new(origin.x + 20, origin.y + 9),
        )
        .into_styled(PrimitiveStyle::with_stroke(OPERATION, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("status wifi fail draw failed: {err:?}"))?;
    }

    Ok(())
}

fn draw_status_ble_icon<D>(display: &mut D, origin: Point, status: StatusBle) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    let color = match status {
        StatusBle::Advertising => WHITE,
        StatusBle::Ready => Rgb565::new(16, 40, 28),
        StatusBle::Failed => OPERATION,
        StatusBle::Off => Rgb565::new(8, 24, 18),
    };
    let x = origin.x;
    let y = origin.y;

    Line::new(Point::new(x + 6, y), Point::new(x + 6, y + 13))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("status ble stem draw failed: {err:?}"))?;
    Line::new(Point::new(x + 6, y), Point::new(x + 13, y + 5))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("status ble top draw failed: {err:?}"))?;
    Line::new(Point::new(x + 13, y + 5), Point::new(x + 6, y + 8))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("status ble top return draw failed: {err:?}"))?;
    Line::new(Point::new(x + 6, y + 5), Point::new(x + 13, y + 9))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("status ble bottom draw failed: {err:?}"))?;
    Line::new(Point::new(x + 13, y + 9), Point::new(x + 6, y + 13))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("status ble bottom return draw failed: {err:?}"))?;

    if status == StatusBle::Failed {
        Line::new(Point::new(x + 15, y), Point::new(x + 20, y + 5))
            .into_styled(PrimitiveStyle::with_stroke(OPERATION, 1))
            .draw(display)
            .map_err(|err| anyhow::anyhow!("status ble fail draw failed: {err:?}"))?;
        Line::new(Point::new(x + 20, y), Point::new(x + 15, y + 5))
            .into_styled(PrimitiveStyle::with_stroke(OPERATION, 1))
            .draw(display)
            .map_err(|err| anyhow::anyhow!("status ble fail draw failed: {err:?}"))?;
    }

    Ok(())
}

fn draw_battery<D>(display: &mut D, origin: Point, percent: u8) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Rectangle::new(origin, Size::new(34, 15))
        .into_styled(PrimitiveStyle::with_stroke(WHITE, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("battery outline draw failed: {err:?}"))?;
    Rectangle::new(Point::new(origin.x + 34, origin.y + 4), Size::new(3, 7))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("battery cap draw failed: {err:?}"))?;
    let width = 30 * percent.min(100) as u32 / 100;
    Rectangle::new(Point::new(origin.x + 2, origin.y + 2), Size::new(width, 11))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("battery fill draw failed: {err:?}"))?;
    Ok(())
}

fn draw_config_icon<D>(display: &mut D, label: &str, center: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    match label {
        "WiFi" => draw_wifi_icon(display, center, color),
        "Storage" => draw_storage_icon(display, center, color),
        "Device" => draw_device_icon(display, center, color),
        "Files" => draw_files_icon(display, center, color),
        "Time" => draw_time_icon(display, center, color),
        "Audio" => draw_audio_icon(display, center, color),
        "Channel9" => draw_channel9_icon(display, center, color),
        "Recorder" => draw_recorder_icon(display, center, color),
        _ => draw_generic_icon(display, center, color),
    }
}

fn draw_wifi_icon<D>(display: &mut D, center: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    let scale = icon_scale(center);
    let dot_radius = (3 * scale) as u32;
    let dot_origin = Point::new(center.x - dot_radius as i32, center.y + 11 * scale as i32);
    Circle::new(dot_origin, dot_radius * 2)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("wifi dot draw failed: {err:?}"))?;

    for diameter in [18, 30] {
        let diameter = diameter * scale as i32;
        Arc::new(
            Point::new(
                center.x - diameter / 2,
                center.y + 11 * scale as i32 - diameter / 2,
            ),
            diameter as u32,
            225.0.deg(),
            90.0.deg(),
        )
        .into_styled(PrimitiveStyle::with_stroke(color, 2))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("wifi arc draw failed: {err:?}"))?;
    }
    Ok(())
}

fn icon_scale(center: Point) -> i32 {
    if center.x == 120 {
        1
    } else {
        1
    }
}

fn draw_storage_icon<D>(display: &mut D, center: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Rectangle::new(Point::new(center.x - 17, center.y - 12), Size::new(34, 24))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("storage icon draw failed: {err:?}"))?;
    Rectangle::new(Point::new(center.x - 13, center.y - 7), Size::new(26, 5))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("storage slot draw failed: {err:?}"))?;
    draw_text(display, "SD", Point::new(center.x - 7, center.y + 8), color)
}

fn draw_files_icon<D>(display: &mut D, center: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    for offset in [-10, 0, 10] {
        Rectangle::new(
            Point::new(center.x - 10 + offset / 3, center.y - 14 + offset),
            Size::new(20, 26),
        )
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("files icon draw failed: {err:?}"))?;
    }
    Ok(())
}

fn draw_device_icon<D>(display: &mut D, center: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Rectangle::new(Point::new(center.x - 16, center.y - 10), Size::new(32, 20))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("device body draw failed: {err:?}"))?;
    for x in [-11, -4, 3, 10] {
        Line::new(
            Point::new(center.x + x, center.y - 14),
            Point::new(center.x + x, center.y - 10),
        )
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("device pin draw failed: {err:?}"))?;
        Line::new(
            Point::new(center.x + x, center.y + 10),
            Point::new(center.x + x, center.y + 14),
        )
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("device pin draw failed: {err:?}"))?;
    }
    Circle::new(Point::new(center.x - 4, center.y - 4), 8)
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("device sensor draw failed: {err:?}"))?;
    draw_text(display, "IO", Point::new(center.x - 6, center.y + 5), color)
}

fn draw_time_icon<D>(display: &mut D, center: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Circle::new(Point::new(center.x - 12, center.y - 12), 24)
        .into_styled(PrimitiveStyle::with_stroke(color, 2))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("time icon face draw failed: {err:?}"))?;
    Circle::new(Point::new(center.x - 2, center.y - 2), 4)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("time icon hub draw failed: {err:?}"))?;
    Line::new(center, Point::new(center.x, center.y - 8))
        .into_styled(PrimitiveStyle::with_stroke(color, 2))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("time icon hour hand draw failed: {err:?}"))?;
    Line::new(center, Point::new(center.x + 7, center.y + 4))
        .into_styled(PrimitiveStyle::with_stroke(color, 2))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("time icon minute hand draw failed: {err:?}"))?;
    Ok(())
}

fn draw_audio_icon<D>(display: &mut D, center: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Rectangle::new(Point::new(center.x - 17, center.y - 7), Size::new(7, 14))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("audio icon sound port draw failed: {err:?}"))?;
    Line::new(
        Point::new(center.x - 10, center.y - 7),
        Point::new(center.x + 3, center.y - 14),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 2))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("audio icon horn draw failed: {err:?}"))?;
    Line::new(
        Point::new(center.x + 3, center.y - 14),
        Point::new(center.x + 3, center.y + 14),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 2))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("audio icon horn draw failed: {err:?}"))?;
    Line::new(
        Point::new(center.x + 3, center.y + 14),
        Point::new(center.x - 10, center.y + 7),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 2))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("audio icon horn draw failed: {err:?}"))?;

    for diameter in [15_u32, 24_u32] {
        Arc::new(
            Point::new(center.x, center.y - diameter as i32 / 2),
            diameter,
            315.0.deg(),
            90.0.deg(),
        )
        .into_styled(PrimitiveStyle::with_stroke(color, 2))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("audio icon wave draw failed: {err:?}"))?;
    }
    Ok(())
}

fn draw_recorder_icon<D>(display: &mut D, center: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Rectangle::new(Point::new(center.x - 6, center.y - 14), Size::new(12, 22))
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(color)
                .stroke_width(2)
                .build(),
        )
        .draw(display)
        .map_err(|err| anyhow::anyhow!("recorder icon capsule draw failed: {err:?}"))?;
    Line::new(
        Point::new(center.x - 12, center.y - 2),
        Point::new(center.x - 12, center.y + 2),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 2))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("recorder icon left level draw failed: {err:?}"))?;
    Line::new(
        Point::new(center.x + 12, center.y - 2),
        Point::new(center.x + 12, center.y + 2),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 2))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("recorder icon right level draw failed: {err:?}"))?;
    Line::new(
        Point::new(center.x, center.y + 8),
        Point::new(center.x, center.y + 14),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 2))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("recorder icon stem draw failed: {err:?}"))?;
    Line::new(
        Point::new(center.x - 8, center.y + 14),
        Point::new(center.x + 8, center.y + 14),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 2))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("recorder icon base draw failed: {err:?}"))?;
    Circle::new(Point::new(center.x - 2, center.y - 4), 4)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("recorder icon record dot draw failed: {err:?}"))?;
    Ok(())
}

fn draw_channel9_icon<D>(display: &mut D, center: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Rectangle::new(Point::new(center.x - 19, center.y - 16), Size::new(38, 28))
        .into_styled(PrimitiveStyle::with_stroke(color, 2))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("channel9 badge draw failed: {err:?}"))?;
    Text::new(
        "C9",
        Point::new(center.x - 11, center.y + 6),
        MonoTextStyle::new(&FONT_10X20, color),
    )
    .draw(display)
    .map_err(|err| anyhow::anyhow!("channel9 mark draw failed: {err:?}"))?;
    Line::new(
        Point::new(center.x - 13, center.y + 17),
        Point::new(center.x + 13, center.y + 17),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 2))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("channel9 push line draw failed: {err:?}"))?;
    Line::new(
        Point::new(center.x - 6, center.y + 21),
        Point::new(center.x + 6, center.y + 21),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 2))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("channel9 push line draw failed: {err:?}"))?;
    Ok(())
}

fn draw_generic_icon<D>(display: &mut D, center: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Rectangle::new(Point::new(center.x - 13, center.y - 13), Size::new(26, 26))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("generic icon draw failed: {err:?}"))?;
    Ok(())
}

fn draw_selected_button<D>(display: &mut D, text: &str, origin: Point, size: Size) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Rectangle::new(origin, size)
        .into_styled(
            PrimitiveStyleBuilder::new()
                .fill_color(SELECTED)
                .stroke_color(OPERATION)
                .stroke_width(1)
                .build(),
        )
        .draw(display)
        .map_err(|err| anyhow::anyhow!("button border draw failed: {err:?}"))?;
    draw_text(
        display,
        text,
        Point::new(origin.x + 6, origin.y + 11),
        WHITE,
    )
}

fn draw_channel9_action_bar<D>(display: &mut D, actions: &[(&str, bool)]) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    let mut x = 14;
    for (label, selected) in actions.iter().copied() {
        let width = match label.len() {
            0..=3 => 42,
            4..=5 => 48,
            _ => 58,
        };
        let fill = if selected { SELECTED } else { PANEL };
        let text = if selected { WHITE } else { PRIMARY };
        let stroke = if selected { OPERATION } else { MUTED };
        Rectangle::new(Point::new(x, 107), Size::new(width, 16))
            .into_styled(
                PrimitiveStyleBuilder::new()
                    .fill_color(fill)
                    .stroke_color(stroke)
                    .stroke_width(1)
                    .build(),
            )
            .draw(display)
            .map_err(|err| anyhow::anyhow!("channel9 action draw failed: {err:?}"))?;
        draw_centered_small_text_in(display, label, x, width as i32, 119, text)?;
        x += width as i32 + 5;
    }
    Ok(())
}

fn draw_centered_big_text<D>(
    display: &mut D,
    text: &str,
    baseline_y: i32,
    color: Rgb565,
) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    let visible_chars = text.chars().take(12).count() as i32;
    let text_width = visible_chars * 10;
    let x = (SCREEN_WIDTH - text_width) / 2;
    let label = truncate_name(text, 12);
    Text::new(
        label.as_str(),
        Point::new(x, baseline_y),
        MonoTextStyle::new(&FONT_10X20, color),
    )
    .draw(display)
    .map_err(|err| anyhow::anyhow!("big text draw failed: {err:?}"))?;
    Ok(())
}

fn draw_text<D>(display: &mut D, text: &str, position: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Text::new(text, position, MonoTextStyle::new(&FONT_6X10, color))
        .draw(display)
        .map_err(|err| anyhow::anyhow!("text draw failed: {err:?}"))?;
    Ok(())
}

fn panel_style(selected: bool) -> PrimitiveStyle<Rgb565> {
    PrimitiveStyleBuilder::new()
        .fill_color(if selected { SELECTED } else { PANEL })
        .stroke_color(if selected {
            OPERATION
        } else {
            Rgb565::new(22, 45, 22)
        })
        .stroke_width(1)
        .build()
}

fn draw_centered_small_text_in<D>(
    display: &mut D,
    text: &str,
    x: i32,
    width: i32,
    y: i32,
    color: Rgb565,
) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    let text_width = text.chars().count() as i32 * 6;
    let text_x = x + ((width - text_width) / 2).max(0);
    draw_text(display, text, Point::new(text_x, y), color)
}

fn draw_chevron<D>(display: &mut D, center: Point, color: Rgb565) -> Result<()>
where
    D: DrawTarget<Color = Rgb565>,
    D::Error: core::fmt::Debug,
{
    Line::new(
        Point::new(center.x - 4, center.y - 5),
        Point::new(center.x + 2, center.y),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 1))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("chevron draw failed: {err:?}"))?;
    Line::new(
        Point::new(center.x + 2, center.y),
        Point::new(center.x - 4, center.y + 5),
    )
    .into_styled(PrimitiveStyle::with_stroke(color, 1))
    .draw(display)
    .map_err(|err| anyhow::anyhow!("chevron draw failed: {err:?}"))?;
    Ok(())
}

fn truncate_name(name: &str, max_chars: usize) -> heapless::String<24> {
    let mut value = heapless::String::<24>::new();
    for ch in name.chars().take(max_chars) {
        let _ = value.push(ch);
    }
    value
}

fn format_size(size_bytes: u64) -> heapless::String<8> {
    let mut value = heapless::String::<8>::new();
    if size_bytes >= 1024 * 1024 {
        let _ = core::fmt::write(&mut value, format_args!("{}M", size_bytes / 1024 / 1024));
    } else if size_bytes >= 1024 {
        let _ = core::fmt::write(&mut value, format_args!("{}K", size_bytes / 1024));
    } else {
        let _ = core::fmt::write(&mut value, format_args!("{size_bytes}B"));
    }
    value
}
