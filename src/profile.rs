//! User profile: load/save as INI and turn it into the device's report sequence.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::keys::{self, Key};
use crate::protocol::{self as p, LedMode, Report};

#[cfg(not(target_os = "macos"))]
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("copy", "ctrl+c"), ("paste", "ctrl+v"), ("cut", "ctrl+x"), ("select-all", "ctrl+a"),
    ("undo", "ctrl+z"), ("redo", "ctrl+shift+z"), ("find", "ctrl+f"), ("close-window", "alt+f4"),
];
#[cfg(not(target_os = "macos"))]
pub const COMMANDS: &[(&str, &str)] = &[("file-manager", "xdg-open ~"), ("lock", "loginctl lock-session")];

// "super" is the Command key on a Mac.
#[cfg(target_os = "macos")]
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("copy", "super+c"), ("paste", "super+v"), ("cut", "super+x"), ("select-all", "super+a"),
    ("undo", "super+z"), ("redo", "super+shift+z"), ("find", "super+f"), ("close-window", "super+w"),
    ("lock", "ctrl+super+q"),
];
#[cfg(target_os = "macos")]
pub const COMMANDS: &[(&str, &str)] = &[("file-manager", "open ~")];

const COLOR_NAMES: &[(&str, u32)] = &[
    ("red", 0xFF0000), ("green", 0x00FF00), ("blue", 0x0000FF), ("yellow", 0xFFFF00),
    ("cyan", 0x00FFFF), ("magenta", 0xFF00FF), ("pink", 0xFF00FF), ("white", 0xFFFFFF),
    ("orange", 0xFF8000), ("purple", 0x8000FF), ("black", 0x000000),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Repeat {
    Times(u32),
    /// Repeat while the button is held
    Hold,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Hw(u8),
    Consumer(u16),
    Keys(Vec<Key>),
    /// Shell command line
    Run(String),
    Macro(String, Repeat),
}

/// What a button does; `text` is how it is written in the profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub kind: ActionKind,
    pub text: String,
}

impl Action {
    pub fn code(&self) -> u8 {
        match self.kind {
            ActionKind::Hw(code) => code,
            _ => p::HOST_FUNCTION,
        }
    }

    pub fn is_host(&self) -> bool {
        !matches!(self.kind, ActionKind::Hw(_))
    }
}

fn find<T: Copy>(table: &[(&str, T)], name: &str) -> Option<T> {
    table.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

pub fn parse_action(text: &str) -> Result<Action> {
    let text = text.trim();
    let t = text.to_lowercase();
    let kind = if let Some(code) = find(p::HW_FUNCTIONS, &t) {
        ActionKind::Hw(code)
    } else if let Some(usage) = find(p::CONSUMER_USAGES, &t) {
        ActionKind::Consumer(usage)
    } else if let Some(chord) = find(SHORTCUTS, &t) {
        ActionKind::Keys(keys::parse_chord(chord)?)
    } else if let Some(cmd) = find(COMMANDS, &t) {
        ActionKind::Run(cmd.to_string())
    } else {
        let (kind, arg) = text.split_once(':').unwrap_or((text, ""));
        match (kind.to_lowercase().as_str(), arg) {
            (_, "") => bail!("unknown action {text:?} (see \"img339ctl actions\")"),
            ("keys", chord) => ActionKind::Keys(keys::parse_chord(chord)?),
            ("run", cmd) => ActionKind::Run(cmd.to_string()),
            ("macro", arg) => {
                let (name, rep) = arg.split_once(':').unwrap_or((arg, ""));
                let repeat = match rep {
                    "" | "once" => Repeat::Times(1),
                    "hold" => Repeat::Hold,
                    n => match n.parse::<u32>() {
                        Ok(n) if n > 0 => Repeat::Times(n),
                        _ => bail!("bad macro repeat {rep:?}: use a count or \"hold\""),
                    },
                };
                ActionKind::Macro(name.to_string(), repeat)
            }
            _ => bail!("unknown action {text:?} (see \"img339ctl actions\")"),
        }
    };
    let text = if matches!(kind, ActionKind::Hw(_) | ActionKind::Consumer(_)) || find(SHORTCUTS, &t).is_some() || find(COMMANDS, &t).is_some() {
        t
    } else {
        text.to_string()
    };
    Ok(Action { kind, text })
}

pub fn parse_color(text: &str) -> Result<u32> {
    let t = text.trim().to_lowercase();
    let t = t.trim_start_matches('#');
    if let Some(c) = find(COLOR_NAMES, t) {
        return Ok(c);
    }
    let hex: String = if t.len() == 3 { t.chars().flat_map(|c| [c, c]).collect() } else { t.to_string() };
    match (hex.len(), u32::from_str_radix(&hex, 16)) {
        (6, Ok(c)) => Ok(c),
        _ => bail!("bad colour {text:?}: use a name or RRGGBB"),
    }
}

/// Parse "1 3 5" into flags for items 1..=n.
pub fn parse_levels(text: &str, n: usize, what: &str) -> Result<Vec<bool>> {
    let mut flags = vec![false; n];
    for tok in text.replace(',', " ").split_whitespace() {
        match tok.parse::<usize>() {
            Ok(i) if (1..=n).contains(&i) => flags[i - 1] = true,
            _ => bail!("{what}: numbers must be 1-{n}"),
        }
    }
    Ok(flags)
}

fn format_levels(flags: &[bool]) -> String {
    flags.iter().enumerate().filter(|(_, on)| **on).map(|(i, _)| (i + 1).to_string()).collect::<Vec<_>>().join(" ")
}

/// Defaults equal the vendor software's "Reset" configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub buttons: Vec<Action>,
    pub dpi: Vec<u16>,
    pub enabled: Vec<bool>,
    pub colors: Vec<u32>,
    /// 0-based level the mouse starts in
    pub active: usize,
    pub led_mode: LedMode,
    pub cycle_time: u8,
    pub cycle_colors: Vec<bool>,
    /// Sent in 8 ms units, like the vendor tool (50 -> 6)
    pub fire_ms: u32,
    pub wheel_volume: bool,
    pub macros: BTreeMap<String, String>,
}

impl Default for Profile {
    fn default() -> Self {
        let buttons = ["left", "middle", "right", "forward", "back", "dpi+", "dpi-"];
        Profile {
            buttons: buttons.iter().map(|b| parse_action(b).unwrap()).collect(),
            dpi: vec![600, 1000, 1200, 2000, 3200, 4800],
            enabled: vec![true; p::NUM_LEVELS],
            colors: vec![0xFFFF00, 0x0000FF, 0xFF00FF, 0x00FF00, 0xFF0000, 0xFFFFFF],
            active: 1,
            led_mode: LedMode::Cycle,
            cycle_time: 2,
            cycle_colors: vec![true; p::NUM_CYCLE_COLORS],
            fire_ms: 50,
            wheel_volume: false,
            macros: BTreeMap::new(),
        }
    }
}

impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.dpi.len() != p::NUM_LEVELS || self.colors.len() != p::NUM_LEVELS {
            bail!("dpi levels and colours need {} entries", p::NUM_LEVELS);
        }
        for &d in &self.dpi {
            p::dpi_code(d)?;
        }
        if !self.enabled.iter().any(|&e| e) {
            bail!("at least one DPI level must stay enabled");
        }
        if self.active >= p::NUM_LEVELS {
            bail!("active level must be 1-6");
        }
        for a in &self.buttons {
            if let ActionKind::Macro(name, _) = &a.kind {
                if !self.macros.contains_key(name) {
                    bail!("button uses undefined macro {name:?}");
                }
            }
        }
        for (name, text) in &self.macros {
            keys::parse_macro(text).with_context(|| format!("macro {name:?}"))?;
        }
        p::fire_report(self.fire_ms)?;
        p::led_report(self.led_mode, self.cycle_time, 0)?;
        Ok(())
    }

    /// The full sequence of reports for "Apply".
    pub fn reports(&self) -> Result<Vec<Report>> {
        self.validate()?;
        let mut out: Vec<Report> = self.buttons.iter().enumerate().map(|(i, a)| p::button_report(i, a.code())).collect();
        out.push(p::wheel_report(self.wheel_volume));
        out.push(p::led_report(self.led_mode, self.cycle_time, p::enabled_mask(&self.cycle_colors))?);
        let mask = p::enabled_mask(&self.enabled);
        for level in 0..p::NUM_LEVELS {
            out.push(p::dpi_report(level, self.dpi[level], self.active, mask)?);
            out.push(p::color_report(level, self.colors[level], mask));
        }
        out.push(p::fire_report(self.fire_ms)?);
        Ok(out)
    }
}

pub fn default_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("img339ctl").join("profile.ini")
}

type Ini = BTreeMap<String, Vec<(String, String)>>;

fn parse_ini(text: &str) -> Result<Ini> {
    let mut ini = Ini::new();
    let mut section: Option<String> = None;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = Some(name.trim().to_string());
            ini.entry(name.trim().to_string()).or_default();
        } else if let (Some(sec), Some((k, v))) = (&section, line.split_once('=')) {
            ini.get_mut(sec).unwrap().push((k.trim().to_string(), v.trim().to_string()));
        } else {
            bail!("line {}: expected [section] or key = value", n + 1);
        }
    }
    Ok(ini)
}

fn get<'a>(ini: &'a Ini, section: &str, key: &str) -> Option<&'a str> {
    ini.get(section)?.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn num<T: std::str::FromStr>(value: &str, what: &str) -> Result<T> {
    value.parse().map_err(|_| anyhow::anyhow!("{what}: bad number {value:?}"))
}

pub fn load(path: &Path) -> Result<Profile> {
    let mut prof = Profile::default();
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(prof),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let ini = parse_ini(&text).with_context(|| format!("{}", path.display()))?;
    (|| -> Result<()> {
        if let Some(v) = get(&ini, "dpi", "levels") {
            prof.dpi = v.replace(',', " ").split_whitespace().map(|d| num(d, "dpi.levels")).collect::<Result<_>>()?;
        }
        if let Some(v) = get(&ini, "dpi", "enabled") {
            prof.enabled = parse_levels(v, p::NUM_LEVELS, "dpi.enabled")?;
        }
        if let Some(v) = get(&ini, "dpi", "colors") {
            prof.colors = v.replace(',', " ").split_whitespace().map(parse_color).collect::<Result<_>>()?;
        }
        if let Some(v) = get(&ini, "dpi", "active") {
            prof.active = num::<usize>(v, "dpi.active")?.wrapping_sub(1);
        }
        for (k, v) in ini.get("buttons").into_iter().flatten() {
            match k.parse::<usize>() {
                Ok(i) if (1..=p::NUM_BUTTONS).contains(&i) => prof.buttons[i - 1] = parse_action(v)?,
                _ => bail!("button numbers are 1-{}", p::NUM_BUTTONS),
            }
        }
        if let Some(v) = get(&ini, "led", "mode") {
            prof.led_mode = LedMode::parse(v)?;
        }
        if let Some(v) = get(&ini, "led", "cycle_time") {
            prof.cycle_time = num(v, "led.cycle_time")?;
        }
        if let Some(v) = get(&ini, "led", "cycle_colors") {
            prof.cycle_colors = parse_levels(v, p::NUM_CYCLE_COLORS, "led.cycle_colors")?;
        }
        if let Some(v) = get(&ini, "misc", "fire_interval_ms") {
            prof.fire_ms = num(v, "misc.fire_interval_ms")?;
        }
        if let Some(v) = get(&ini, "misc", "wheel") {
            prof.wheel_volume = v.eq_ignore_ascii_case("volume");
        }
        for (k, v) in ini.get("macros").into_iter().flatten() {
            prof.macros.insert(k.clone(), v.clone());
        }
        prof.validate()
    })()
    .with_context(|| format!("{}", path.display()))?;
    Ok(prof)
}

pub fn save(prof: &Profile, path: &Path) -> Result<()> {
    prof.validate()?;
    let mut s = String::new();
    let section = |s: &mut String, name: &str, items: &[(String, String)]| {
        s.push_str(&format!("[{name}]\n"));
        for (k, v) in items {
            s.push_str(&format!("{k} = {v}\n"));
        }
        s.push('\n');
    };
    let kv = |k: &str, v: String| (k.to_string(), v);
    section(&mut s, "dpi", &[
        kv("levels", prof.dpi.iter().map(u16::to_string).collect::<Vec<_>>().join(" ")),
        kv("enabled", format_levels(&prof.enabled)),
        kv("active", (prof.active + 1).to_string()),
        kv("colors", prof.colors.iter().map(|c| format!("{c:06x}")).collect::<Vec<_>>().join(" ")),
    ]);
    let buttons: Vec<_> = prof.buttons.iter().enumerate().map(|(i, a)| ((i + 1).to_string(), a.text.clone())).collect();
    section(&mut s, "buttons", &buttons);
    section(&mut s, "led", &[
        kv("mode", prof.led_mode.name().to_string()),
        kv("cycle_time", prof.cycle_time.to_string()),
        kv("cycle_colors", format_levels(&prof.cycle_colors)),
    ]);
    section(&mut s, "misc", &[
        kv("fire_interval_ms", prof.fire_ms.to_string()),
        kv("wheel", if prof.wheel_volume { "volume" } else { "scroll" }.to_string()),
    ]);
    let macros: Vec<_> = prof.macros.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    section(&mut s, "macros", &macros);

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, s).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    //! The expected byte sequences were checked against a real mouse.
    use super::*;

    const RESET_REPORTS: &str = "
        07 10 01 01 00 00 00 00
        07 10 02 02 00 00 00 00
        07 10 03 03 00 00 00 00
        07 10 05 05 00 00 00 00
        07 10 04 04 00 00 00 00
        07 10 08 0b 00 00 00 00
        07 10 06 0c 00 00 00 00
        07 11 00 00 00 00 00 00
        07 13 7f 92 00 00 00 00
        07 09 41 38 3f 00 00 00
        07 14 00 0f 3f 00 00 00
        07 09 41 59 3f 00 00 00
        07 14 2f f0 3f 00 00 00
        07 09 41 6a 3f 00 00 00
        07 14 4f 00 3f 00 00 00
        07 09 41 9b 3f 00 00 00
        07 14 60 ff 3f 00 00 00
        07 09 41 dc 3f 00 00 00
        07 14 8f 0f 3f 00 00 00
        07 09 41 fd 3f 00 00 00
        07 14 a0 00 3f 00 00 00
        07 12 00 06 00 00 00 00";

    // DPI1 -> 1600 and light blue, DPI3 disabled, starting level 1, fire 176 ms,
    // button 4 -> DPI-, LED fixed (DPI colour).
    const MODIFIED_REPORTS: &str = "
        07 10 01 01 00 00 00 00
        07 10 02 02 00 00 00 00
        07 10 03 03 00 00 00 00
        07 10 05 0c 00 00 00 00
        07 10 04 04 00 00 00 00
        07 10 08 0b 00 00 00 00
        07 10 06 0c 00 00 00 00
        07 11 00 00 00 00 00 00
        07 13 7f 82 00 00 00 00
        07 09 40 78 3b 00 00 00
        07 14 07 f0 3b 00 00 00
        07 09 40 59 3b 00 00 00
        07 14 2f f0 3b 00 00 00
        07 09 40 6a 3b 00 00 00
        07 14 4f 00 3b 00 00 00
        07 09 40 9b 3b 00 00 00
        07 14 60 ff 3b 00 00 00
        07 09 40 dc 3b 00 00 00
        07 14 8f 0f 3b 00 00 00
        07 09 40 fd 3b 00 00 00
        07 14 a0 00 3b 00 00 00
        07 12 00 16 00 00 00 00";

    fn parse(text: &str) -> Vec<Report> {
        text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let bytes: Vec<u8> = l.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect();
                bytes.try_into().unwrap()
            })
            .collect()
    }

    fn hex(s: &str) -> Report {
        parse(s)[0]
    }

    fn modified_profile() -> Profile {
        let mut prof = Profile::default();
        prof.dpi[0] = 1600;
        prof.colors[0] = 0x0088FF;
        prof.enabled[2] = false;
        prof.active = 0;
        prof.fire_ms = 176;
        prof.buttons[3] = parse_action("dpi-").unwrap();
        prof.led_mode = LedMode::Dpi;
        prof
    }

    #[test]
    fn reset_matches_vendor_traffic() {
        assert_eq!(Profile::default().reports().unwrap(), parse(RESET_REPORTS));
    }

    #[test]
    fn modified_profile_matches_vendor_traffic() {
        assert_eq!(modified_profile().reports().unwrap(), parse(MODIFIED_REPORTS));
    }

    #[test]
    fn host_functions_are_code_zero() {
        let mut prof = Profile::default();
        prof.buttons[3] = parse_action("volume-up").unwrap();
        assert_eq!(prof.reports().unwrap()[3], hex("07 10 05 00 00 00 00 00"));
    }

    #[test]
    fn media_and_macro_reports() {
        assert_eq!(p::consumer_report(0xE9), hex("07 17 00 e9 00 00 00 00"));
        assert_eq!(p::keys_report(&[0x07]).unwrap(), hex("07 15 07 00 00 00 00 00"));
        assert_eq!(p::keys_report(&[]).unwrap(), hex("07 15 00 00 00 00 00 00"));
        assert_eq!(p::mouse_buttons_report(0), hex("07 16 00 ff 00 00 00 00"));
    }

    #[test]
    fn led_modes() {
        assert_eq!(p::led_report(LedMode::Static, 2, 0x7F).unwrap()[3], 0x86);
        assert_eq!(p::led_report(LedMode::Off, 2, 0x7F).unwrap()[3], 0x87);
    }

    #[test]
    fn parse_event() {
        assert_eq!(p::parse_event(&[6, 0xF5, 1]), Some((3, true))); // button 4 pressed
        assert_eq!(p::parse_event(&[6, 0xF5, 0x80]), Some((3, false)));
        assert_eq!(p::parse_event(&[6, 0xF0, 0]), None);
        assert_eq!(p::parse_event(&[3, 0, 4]), None);
    }

    #[test]
    fn parse_level() {
        // DPI- at level 1, DPI+ at level 2, then release.
        assert_eq!(p::parse_level(&[6, 0xF6, 0]), Some(0));
        assert_eq!(p::parse_level(&[6, 0xF8, 1]), Some(1));
        assert_eq!(p::parse_level(&[6, 0xF8, 0x80]), None);
        assert_eq!(p::parse_level(&[6, 0xF8, 6]), None);
        assert_eq!(p::parse_level(&[3, 0, 4]), None);
    }

    #[test]
    fn profile_roundtrip() {
        let mut prof = modified_profile();
        prof.macros.insert("spam".into(), "text:testtest 50ms".into());
        prof.buttons[4] = parse_action("macro:spam:hold").unwrap();
        let dir = std::env::temp_dir().join(format!("img339ctl-test-{}", std::process::id()));
        let path = dir.join("profile.ini");
        save(&prof, &path).unwrap();
        let loaded = load(&path).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(loaded, prof);
    }

    #[test]
    fn reads_python_profile() {
        // As written by the earlier Python version (configparser).
        let text = "[dpi]\nlevels = 600 1000 1200 2000 3200 4800\nenabled = 1 2 3 4 5 6\nactive = 2\n\
                    colors = ffff00 ff0000 ff00ff 00ff00 ff0000 ffffff\n\n[buttons]\n1 = left\n2 = middle\n\
                    3 = right\n4 = volume-up\n5 = back\n6 = dpi+\n7 = dpi-\n\n[led]\nmode = dpi\ncycle_time = 2\n\
                    cycle_colors = 1 2 3 4 5 6 7\n\n[misc]\nfire_interval_ms = 50\nwheel = scroll\n\n[macros]\n\n";
        let dir = std::env::temp_dir().join(format!("img339ctl-py-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profile.ini");
        fs::write(&path, text).unwrap();
        let prof = load(&path).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(prof.colors[1], 0xFF0000);
        assert_eq!(prof.buttons[3].kind, ActionKind::Consumer(0xE9));
        assert_eq!(prof.led_mode, LedMode::Dpi);
    }

    #[test]
    fn undefined_macro_rejected() {
        let mut prof = Profile::default();
        prof.buttons[4] = parse_action("macro:missing").unwrap();
        assert!(prof.reports().is_err());
    }
}
