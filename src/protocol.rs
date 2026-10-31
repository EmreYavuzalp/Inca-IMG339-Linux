//! Wire protocol of the Inca IMG-339 (Maxxter 18f8:0fc0 / 18f8:1086) gaming mouse.
//!
//! Everything is sent as 8-byte HID feature reports with report ID 7 on the
//! vendor interface (usage page 0xFF01):
//!
//! ```text
//! 07 <cmd> <data...>
//! ```
//!
//! Settings cannot be read back, so the
//! host-side profile is the source of truth; only the current DPI level is
//! reported, in button events.

use anyhow::{bail, Result};

pub const REPORT_ID: u8 = 7;
pub const REPORT_LEN: usize = 8;
pub type Report = [u8; REPORT_LEN];

const CMD_BUTTON: u8 = 0x10; // 07 10 <hw button> <function>
const CMD_WHEEL: u8 = 0x11; // 07 11 <0 scroll | 1 volume>
const CMD_FIRE: u8 = 0x12; // 07 12 <interval/8 hi> <interval/8 lo>
const CMD_LED: u8 = 0x13; // 07 13 <cycle colour mask> <mode|time>
const CMD_DPI: u8 = 0x09; // 07 09 <0x40|active level> <dpi code<<4|8|level> <enabled mask>
const CMD_COLOR: u8 = 0x14; // 07 14 <level<<5|G> <R<<4|B> <enabled mask>
const CMD_KEYS: u8 = 0x15; // 07 15 <up to 6 HID keyboard usages>
const CMD_MOUSE: u8 = 0x16; // 07 16 00 <button mask, active low>
const CMD_CONSUMER: u8 = 0x17; // 07 17 <usage hi> <usage lo>

/// Input report sent by the mouse when a host-side (code 0) or DPI button changes
/// state: `06 <0xF0 + hw button> <0x80 released | current DPI level pressed>`.
const EVENT_REPORT_ID: u8 = 6;
const EVENT_RELEASED: u8 = 0x80;

pub const NUM_BUTTONS: usize = 7;
pub const NUM_LEVELS: usize = 6;
pub const NUM_CYCLE_COLORS: usize = 7;

/// Button rows as numbered on the vendor's UI (1-based) -> firmware button index.
const HW_BUTTON: [u8; NUM_BUTTONS] = [1, 2, 3, 5, 4, 8, 6];

pub const DPI_VALUES: [u16; 12] = [200, 400, 600, 800, 1000, 1200, 1600, 2000, 2400, 3200, 4000, 4800];
const DPI_CODES: [u8; 12] = [0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x9, 0xB, 0xD, 0xE, 0xF];

/// Functions the firmware performs on its own.
pub const HW_FUNCTIONS: &[(&str, u8)] = &[
    ("left", 0x1),
    ("middle", 0x2),
    ("right", 0x3),
    ("back", 0x4),
    ("forward", 0x5),
    ("dpi-loop", 0x6),
    ("show-desktop", 0x7),
    ("double-click", 0x8),
    ("fire", 0x9),
    ("off", 0xA),
    ("dpi+", 0xB),
    ("dpi-", 0xC),
];
/// Code for buttons whose action is carried out by the host (see daemon.rs).
pub const HOST_FUNCTION: u8 = 0x0;

/// Consumer-page usages for the "Media" functions.
pub const CONSUMER_USAGES: &[(&str, u16)] = &[
    ("browser", 0x223),
    ("player", 0x183),
    ("mail", 0x18A),
    ("volume-up", 0x0E9),
    ("volume-down", 0x0EA),
    ("previous", 0x0B6),
    ("next", 0x0B5),
    ("mute", 0x0E2),
    ("play-pause", 0x0CD),
    ("calculator", 0x192),
    ("stop", 0x0B7),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum LedMode {
    /// Colour of the current DPI level
    Dpi,
    Cycle,
    Static,
    Off,
}

impl LedMode {
    pub fn name(self) -> &'static str {
        match self {
            LedMode::Dpi => "dpi",
            LedMode::Cycle => "cycle",
            LedMode::Static => "static",
            LedMode::Off => "off",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s.trim().to_lowercase().as_str() {
            "dpi" => LedMode::Dpi,
            "cycle" => LedMode::Cycle,
            "static" => LedMode::Static,
            "off" => LedMode::Off,
            _ => bail!("LED mode must be one of dpi, cycle, static, off"),
        })
    }
}

pub const FIRE_MIN_MS: u32 = 8;
pub const FIRE_MAX_MS: u32 = 255 * 8;

fn report(cmd: u8, data: &[u8]) -> Report {
    let mut r = [0u8; REPORT_LEN];
    r[0] = REPORT_ID;
    r[1] = cmd;
    r[2..2 + data.len()].copy_from_slice(data);
    r
}

pub fn dpi_code(dpi: u16) -> Result<u8> {
    match DPI_VALUES.iter().position(|&d| d == dpi) {
        Some(i) => Ok(DPI_CODES[i]),
        None => {
            let all: Vec<String> = DPI_VALUES.iter().map(u16::to_string).collect();
            bail!("unsupported DPI {dpi}; choose one of {}", all.join(", "))
        }
    }
}

/// 24-bit RGB -> the device's 4-bit, active-low (R, G, B).
pub fn color_nibbles(rgb: u32) -> (u8, u8, u8) {
    let n = |shift: u32| {
        let c = (rgb >> shift) & 0xFF;
        15 - ((c * 15 + 127) / 255) as u8
    };
    (n(16), n(8), n(0))
}

pub fn button_report(row: usize, function_code: u8) -> Report {
    report(CMD_BUTTON, &[HW_BUTTON[row], function_code])
}

pub fn wheel_report(volume: bool) -> Report {
    report(CMD_WHEEL, &[volume as u8, 0])
}

pub fn fire_report(interval_ms: u32) -> Result<Report> {
    if !(FIRE_MIN_MS..=FIRE_MAX_MS).contains(&interval_ms) {
        bail!("fire interval must be {FIRE_MIN_MS}-{FIRE_MAX_MS} ms");
    }
    let units = interval_ms / 8;
    Ok(report(CMD_FIRE, &[(units >> 8) as u8, units as u8]))
}

pub fn led_report(mode: LedMode, cycle_time: u8, cycle_mask: u8) -> Result<Report> {
    let mode_byte = match mode {
        // Verified on hardware.
        LedMode::Static => 0x86,
        LedMode::Off => 0x87,
        LedMode::Dpi | LedMode::Cycle => {
            if !(1..=6).contains(&cycle_time) {
                bail!("cycle time must be 1-6 seconds");
            }
            (if mode == LedMode::Cycle { 0x90 } else { 0x80 }) | cycle_time
        }
    };
    Ok(report(CMD_LED, &[cycle_mask & 0x7F, mode_byte]))
}

pub fn enabled_mask(enabled: &[bool]) -> u8 {
    enabled.iter().enumerate().filter(|(_, on)| **on).map(|(i, _)| 1 << i).sum()
}

pub fn dpi_report(level: usize, dpi: u16, active: usize, mask: u8) -> Result<Report> {
    Ok(report(CMD_DPI, &[0x40 | active as u8, dpi_code(dpi)? << 4 | 0x8 | level as u8, mask]))
}

pub fn color_report(level: usize, rgb: u32, mask: u8) -> Report {
    let (r, g, b) = color_nibbles(rgb);
    // Byte 4 holds the enabled mask from the previous DPI report.
    report(CMD_COLOR, &[(level as u8) << 5 | g, r << 4 | b, mask])
}

pub fn keys_report(usages: &[u8]) -> Result<Report> {
    if usages.len() > 6 {
        bail!("at most 6 keys can be held at once");
    }
    Ok(report(CMD_KEYS, usages))
}

pub fn mouse_buttons_report(pressed_mask: u8) -> Report {
    report(CMD_MOUSE, &[0, !pressed_mask])
}

pub fn consumer_report(usage: u16) -> Report {
    report(CMD_CONSUMER, &[(usage >> 8) as u8, usage as u8])
}

/// Decode a host-function button event: `(row, pressed)`.
pub fn parse_event(data: &[u8]) -> Option<(usize, bool)> {
    if data.len() < 3 || data[0] != EVENT_REPORT_ID || data[1] <= 0xF0 {
        return None;
    }
    let row = HW_BUTTON.iter().position(|&hw| hw == data[1] - 0xF0)?;
    Some((row, data[2] != EVENT_RELEASED))
}

/// The 0-based DPI level carried by a button press event.
pub fn parse_level(data: &[u8]) -> Option<usize> {
    let (_, pressed) = parse_event(data)?;
    let level = data[2] as usize;
    (pressed && level < NUM_LEVELS).then_some(level)
}
