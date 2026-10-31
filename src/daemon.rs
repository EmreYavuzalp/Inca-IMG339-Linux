//! Runs host-side button functions: media keys, shortcuts, macros and commands.
//!
//! Buttons bound to these are programmed with function code 0. The mouse then
//! reports presses on its vendor interface and the host answers with keyboard /
//! consumer / mouse-button reports that the mouse replays as real input, exactly
//! like the vendor's Windows software does.

use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use anyhow::Result;

use crate::device::{self, Mouse};
use crate::keys::{self, Key, Step};
use crate::profile::{self, ActionKind, Profile, Repeat};
use crate::protocol::{self as p, NUM_BUTTONS};

struct Runner {
    mouse: Mutex<Mouse>,
    held: [AtomicBool; NUM_BUTTONS],
}

impl Runner {
    fn send(&self, report: &p::Report) -> Result<()> {
        self.mouse.lock().unwrap().send(report)
    }

    fn run(&self, profile: &Profile, row: usize) -> Result<()> {
        match &profile.buttons[row].kind {
            ActionKind::Hw(_) => {}
            ActionKind::Consumer(usage) => self.send(&p::consumer_report(*usage))?,
            ActionKind::Keys(chord) => {
                let mut steps: Vec<Step> = chord.iter().map(|&k| Step::Down(k)).collect();
                steps.extend(chord.iter().rev().map(|&k| Step::Up(k)));
                self.play(&steps, || false)?;
            }
            ActionKind::Run(cmd) => {
                Command::new("sh").args(["-c", cmd]).stdin(Stdio::null()).spawn()?;
            }
            ActionKind::Macro(name, repeat) => {
                let steps = keys::parse_macro(&profile.macros[name])?;
                match repeat {
                    Repeat::Hold => {
                        let released = || !self.held[row].load(Ordering::Relaxed);
                        while !released() {
                            self.play(&steps, released)?;
                        }
                    }
                    Repeat::Times(n) => {
                        for _ in 0..*n {
                            self.play(&steps, || false)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn play(&self, steps: &[Step], stop: impl Fn() -> bool) -> Result<()> {
        let result = self.play_inner(steps, stop);
        // Never leave keys or buttons stuck down.
        self.send(&p::mouse_buttons_report(0))?;
        self.send(&p::keys_report(&[])?)?;
        result
    }

    fn play_inner(&self, steps: &[Step], stop: impl Fn() -> bool) -> Result<()> {
        let (mut pressed, mut buttons) = (Vec::<u8>::new(), 0u8);
        for step in steps {
            if stop() {
                break;
            }
            let (down, key) = match *step {
                Step::Wait(ms) => {
                    thread::sleep(Duration::from_millis(ms));
                    continue;
                }
                Step::Down(k) => (true, k),
                Step::Up(k) => (false, k),
            };
            match key {
                Key::Mouse(bit) => {
                    buttons = if down { buttons | bit } else { buttons & !bit };
                    self.send(&p::mouse_buttons_report(buttons))?;
                }
                Key::Kbd(usage) => {
                    if down && !pressed.contains(&usage) {
                        pressed.push(usage);
                    } else if !down {
                        pressed.retain(|&u| u != usage);
                    }
                    self.send(&p::keys_report(&pressed[pressed.len().saturating_sub(6)..])?)?;
                }
            }
            thread::sleep(Duration::from_millis(keys::DEFAULT_GAP_MS));
        }
        Ok(())
    }
}

/// The profile file is re-read whenever it changes, so bindings edited with the
/// CLI or the GUI take effect without restarting the daemon.
struct Watched<'a> {
    path: &'a Path,
    modified: Option<SystemTime>,
    profile: Arc<Profile>,
}

impl<'a> Watched<'a> {
    fn load(path: &'a Path) -> Result<Self> {
        let modified = fs::metadata(path).and_then(|m| m.modified()).ok();
        Ok(Watched { path, modified, profile: Arc::new(profile::load(path)?) })
    }

    fn refresh(&mut self) {
        let modified = fs::metadata(self.path).and_then(|m| m.modified()).ok();
        if modified == self.modified {
            return;
        }
        self.modified = modified;
        match profile::load(self.path) {
            Ok(p) => {
                eprintln!("profile reloaded");
                self.profile = Arc::new(p);
            }
            Err(e) => eprintln!("keeping the previous profile: {e:#}"),
        }
    }
}

/// Whether an `img339ctl daemon` process is running for this user.
#[cfg(not(target_os = "macos"))]
pub fn is_running() -> bool {
    let uid = unsafe { libc::getuid() };
    let Ok(dir) = fs::read_dir("/proc") else { return false };
    dir.filter_map(Result::ok).any(|e| {
        use std::os::unix::fs::MetadataExt;
        if e.metadata().map(|m| m.uid()).ok() != Some(uid) {
            return false;
        }
        let Ok(cmd) = fs::read(e.path().join("cmdline")) else { return false };
        let mut args = cmd.split(|&b| b == 0);
        let exe = args.next().unwrap_or_default();
        exe.ends_with(b"img339ctl") && args.any(|a| a == b"daemon")
    })
}

/// There is no /proc on macOS: the running daemon holds a lock on this file instead.
#[cfg(target_os = "macos")]
fn lock_file() -> Option<std::path::PathBuf> {
    device::runtime_file("img339ctl-daemon.lock")
}

#[cfg(target_os = "macos")]
fn try_lock(file: &fs::File) -> bool {
    use std::os::fd::AsRawFd;
    // SAFETY: flock only uses the file descriptor, which `file` keeps open.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

/// Whether an `img339ctl daemon` process is running for this user.
#[cfg(target_os = "macos")]
pub fn is_running() -> bool {
    let file = lock_file().and_then(|path| fs::File::open(path).ok());
    file.is_some_and(|f| !try_lock(&f))
}

/// The lock lasts as long as the returned file stays open.
#[cfg(target_os = "macos")]
fn lock_instance() -> Result<Option<fs::File>> {
    let Some(path) = lock_file() else { return Ok(None) };
    let file = fs::File::create(path)?;
    // is_running() in another program may hold the lock for a moment.
    for _ in 0..5 {
        if try_lock(&file) {
            return Ok(Some(file));
        }
        thread::sleep(Duration::from_millis(50));
    }
    anyhow::bail!("the daemon is already running");
}

pub fn run(device_path: Option<&Path>, profile_path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    let _lock = lock_instance()?;
    let mut watched = Watched::load(profile_path)?;
    if watched.profile.buttons.iter().all(|a| !a.is_host()) {
        eprintln!("note: no button uses a host-side function yet; waiting for profile changes");
    }
    loop {
        let (events, mouse) = match device::open(device_path, false).and_then(|f| Ok((f, Mouse::open(device_path)?))) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{e:#}; retrying in 3 s");
                thread::sleep(Duration::from_secs(3));
                continue;
            }
        };
        watched.refresh();
        if let Err(e) = mouse.send_all(&watched.profile.reports()?) {
            eprintln!("{e:#}");
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        device::store_level(watched.profile.active);
        eprintln!("img339ctl daemon: profile applied, listening for button events");
        let runner = Arc::new(Runner { mouse: Mutex::new(mouse), held: Default::default() });
        if let Err(e) = listen(events, &runner, &mut watched) {
            eprintln!("{e}");
        }
        eprintln!("mouse disconnected; waiting for it to come back");
        thread::sleep(Duration::from_secs(1));
    }
}

fn listen(mut events: device::Events, runner: &Arc<Runner>, watched: &mut Watched) -> io::Result<()> {
    let mut workers: [Option<JoinHandle<()>>; NUM_BUTTONS] = Default::default();
    let mut buf = [0u8; 64];
    loop {
        let n = events.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        if let Some(level) = p::parse_level(&buf[..n]) {
            device::store_level(level);
        }
        let Some((row, pressed)) = p::parse_event(&buf[..n]) else { continue };
        runner.held[row].store(pressed, Ordering::Relaxed);
        if !pressed {
            continue;
        }
        watched.refresh();
        if !watched.profile.buttons[row].is_host() {
            continue;
        }
        if workers[row].as_ref().is_some_and(|w| !w.is_finished()) {
            continue;
        }
        let (r, profile) = (Arc::clone(runner), Arc::clone(&watched.profile));
        workers[row] = Some(thread::spawn(move || {
            if let Err(e) = r.run(&profile, row) {
                eprintln!("button {}: {e:#}", row + 1);
            }
        }));
    }
}
