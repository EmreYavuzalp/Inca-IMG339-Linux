//! img339ctl: configure the Inca IMG-339 gaming mouse on Linux.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

use img339ctl::profile::{self, Profile};
use img339ctl::protocol::{self as p, LedMode};
use img339ctl::{daemon, device, keys};

#[derive(Parser)]
#[command(version, about = "Configure the Inca IMG-339 gaming mouse.")]
struct Cli {
    /// Profile file [default: ~/.config/img339ctl/profile.ini]
    #[arg(long, global = true, value_name = "FILE")]
    profile: Option<PathBuf>,
    /// hidraw node [default: auto-detect]
    #[arg(long, global = true, value_name = "PATH")]
    device: Option<PathBuf>,
    /// Print the reports instead of sending them (the profile is not saved)
    #[arg(short = 'n', long, global = true)]
    dry_run: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show the detected mouse and the profile
    Status,
    /// Send the whole profile to the mouse
    Apply,
    /// Restore factory defaults (like the vendor "Reset")
    Reset,
    /// List what buttons can be bound to
    Actions,
    /// Set a level's DPI, or which levels are enabled / active
    Dpi {
        /// DPI level 1-6
        #[arg(value_parser = clap::value_parser!(u8).range(1..=6))]
        level: u8,
        /// 200 400 600 800 1000 1200 1600 2000 2400 3200 4000 4800
        value: Option<u16>,
        /// Enable the level
        #[arg(long, conflicts_with = "off")]
        on: bool,
        /// Disable the level
        #[arg(long)]
        off: bool,
        /// Start in this level
        #[arg(long)]
        active: bool,
    },
    /// Set a DPI level's LED colour
    Color {
        /// DPI level 1-6, or "all"
        #[arg(value_parser = parse_level_or_all)]
        level: u8,
        /// RRGGBB or a name (red, orange, cyan, ...)
        color: String,
    },
    /// Bind a button (1-7, numbered as in the vendor tool)
    Button {
        #[arg(value_parser = clap::value_parser!(u8).range(1..=7))]
        button: u8,
        /// See "img339ctl actions"
        action: String,
    },
    /// LED mode
    Led {
        mode: LedMode,
        /// Cycle period 1-6 s
        #[arg(long)]
        time: Option<u8>,
        /// Which of the 7 cycle colours to use, e.g. "1 3 5"
        #[arg(long)]
        colors: Option<String>,
    },
    /// Repeat interval of the "fire" function
    Fire { ms: u32 },
    /// What the wheel does
    Wheel {
        #[arg(value_parser = ["scroll", "volume"])]
        mode: String,
    },
    /// Define (or with no steps, delete) a macro
    Macro {
        name: String,
        /// e.g.  ctrl+c 100ms text:hello  (see "img339ctl actions")
        #[arg(allow_hyphen_values = true)]
        steps: Vec<String>,
    },
    /// Run host-side button functions (media keys, macros, ...)
    Daemon,
}

/// "1".."6", or "all" (returned as 0).
fn parse_level_or_all(s: &str) -> Result<u8, String> {
    match s {
        "all" => Ok(0),
        _ => match s.parse::<u8>() {
            Ok(n) if (1..=6).contains(&n) => Ok(n),
            _ => Err("level must be 1-6 or \"all\"".into()),
        },
    }
}

fn show(prof: &Profile) {
    println!("DPI levels:");
    for i in 0..p::NUM_LEVELS {
        let mark = if i == prof.active { '*' } else { ' ' };
        let state = if prof.enabled[i] { "on " } else { "off" };
        println!(" {mark}{}: {:>4} dpi  {state}  #{:06x}", i + 1, prof.dpi[i], prof.colors[i]);
    }
    println!("Buttons:");
    for (i, a) in prof.buttons.iter().enumerate() {
        let host = if a.is_host() { "   (needs \"img339ctl daemon\")" } else { "" };
        println!("  {}: {}{host}", i + 1, a.text);
    }
    let cyc: Vec<String> = (0..p::NUM_CYCLE_COLORS).filter(|&i| prof.cycle_colors[i]).map(|i| (i + 1).to_string()).collect();
    println!("LED: {}, cycle {} s, cycle colours {}", prof.led_mode.name(), prof.cycle_time, cyc.join(" "));
    println!("Fire interval: {} ms   Wheel: {}", prof.fire_ms, if prof.wheel_volume { "volume" } else { "scroll" });
    if !prof.macros.is_empty() {
        println!("Macros:");
        for (name, text) in &prof.macros {
            println!("  {name} = {text}");
        }
    }
}

fn names<T>(table: &[(&str, T)]) -> String {
    table.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(" ")
}

fn actions() {
    println!("Handled by the mouse itself:");
    println!("  {}", names(p::HW_FUNCTIONS));
    println!("Need the daemon running:");
    println!("  media:     {}", names(p::CONSUMER_USAGES));
    println!("  shortcuts: {}", names(profile::SHORTCUTS));
    println!("  commands:  {}", names(profile::COMMANDS));
    println!("  keys:CHORD             e.g. keys:ctrl+shift+t");
    println!("  macro:NAME[:N|:hold]   run a macro from the profile once, N times or while held");
    println!("  run:COMMAND            run a shell command");
    println!();
    println!("{}", keys::MACRO_HELP);
}

fn modify(cmd: Cmd, mut prof: Profile) -> Result<Profile> {
    match cmd {
        Cmd::Reset => return Ok(Profile::default()),
        Cmd::Dpi { level, value, on, off, active } => {
            let i = level as usize - 1;
            if value.is_none() && !(on || off || active) {
                bail!("give a DPI value or --on/--off/--active");
            }
            if let Some(v) = value {
                p::dpi_code(v)?;
                prof.dpi[i] = v;
            }
            if on || off {
                prof.enabled[i] = on;
            }
            if active {
                prof.active = i;
                prof.enabled[i] = true;
            }
        }
        Cmd::Color { level, color } => {
            let c = profile::parse_color(&color)?;
            match level {
                0 => prof.colors.iter_mut().for_each(|x| *x = c),
                n => prof.colors[n as usize - 1] = c,
            }
        }
        Cmd::Button { button, action } => prof.buttons[button as usize - 1] = profile::parse_action(&action)?,
        Cmd::Led { mode, time, colors } => {
            prof.led_mode = mode;
            if let Some(t) = time {
                prof.cycle_time = t;
            }
            if let Some(c) = colors {
                prof.cycle_colors = profile::parse_levels(&c, p::NUM_CYCLE_COLORS, "--colors")?;
            }
        }
        Cmd::Fire { ms } => prof.fire_ms = ms,
        Cmd::Wheel { mode } => prof.wheel_volume = mode == "volume",
        Cmd::Macro { name, steps } => {
            if steps.is_empty() {
                prof.macros.remove(&name);
            } else {
                let text = steps.join(" ");
                keys::parse_macro(&text)?;
                prof.macros.insert(name, text);
            }
        }
        Cmd::Status | Cmd::Apply | Cmd::Actions | Cmd::Daemon => {}
    }
    prof.validate()?;
    Ok(prof)
}

fn run(cli: Cli) -> Result<()> {
    let path = cli.profile.clone().unwrap_or_else(profile::default_path);
    let prof = profile::load(&path)?;
    match cli.cmd {
        Cmd::Actions => actions(),
        Cmd::Daemon => daemon::run(cli.device.as_deref(), &path)?,
        Cmd::Status => {
            let found: Vec<String> = device::find().iter().map(|p| p.display().to_string()).collect();
            println!("Mouse: {}", if found.is_empty() { "not found".to_string() } else { found.join(", ") });
            if found.is_empty() {
                device::diagnostics().iter().for_each(|l| println!("  {l}"));
            }
            println!("Profile: {}", path.display());
            if !found.is_empty() {
                match device::load_level().filter(|_| daemon::is_running()) {
                    Some(l) => println!("Current DPI: {} (level {})", prof.dpi[l], l + 1),
                    None => println!("Current DPI: unknown (needs \"img339ctl daemon\" running)"),
                }
            }
            show(&prof);
        }
        cmd => {
            let saving = !matches!(cmd, Cmd::Apply);
            let host_button = match &cmd {
                Cmd::Button { button, .. } => Some(*button as usize - 1),
                _ => None,
            };
            let prof = if saving { modify(cmd, prof)? } else { prof };
            let reports = prof.reports()?;
            if cli.dry_run {
                for r in &reports {
                    println!("{}", r.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
                }
                if saving {
                    eprintln!("(dry run: profile not saved)");
                }
                return Ok(());
            }
            device::Mouse::open(cli.device.as_deref())?.send_all(&reports)?;
            device::store_level(prof.active);
            if saving {
                profile::save(&prof, &path)?;
            }
            if let Some(b) = host_button.filter(|&b| prof.buttons[b].is_host()) {
                eprintln!("note: button {} needs \"img339ctl daemon\" running", b + 1);
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    // Exit quietly when stdout is closed early (e.g. `img339ctl status | head`).
    // SAFETY: called before any other thread exists.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("img339ctl: {e:#}");
            ExitCode::FAILURE
        }
    }
}
