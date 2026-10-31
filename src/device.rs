//! Find the mouse's vendor HID interface, send feature reports to it and read its events.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

use crate::protocol::{Report, NUM_LEVELS};

#[cfg(not(target_os = "macos"))]
mod linux;
#[cfg(not(target_os = "macos"))]
pub use linux::{diagnostics, find, open, Events, Mouse};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{diagnostics, find, open, Events, Mouse};

impl Mouse {
    pub fn send_all(&self, reports: &[Report]) -> Result<()> {
        for r in reports {
            self.send(r)?;
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }
}

/// A file for state shared between this user's programs while they run.
#[cfg(not(target_os = "macos"))]
pub(crate) fn runtime_file(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(dir).join(name))
}

/// A file for state shared between this user's programs while they run.
#[cfg(target_os = "macos")]
pub(crate) fn runtime_file(name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(home).join("Library/Caches").join(name))
}

/// Remember the 0-based current level. Only a display aid, so failures are ignored.
pub fn store_level(level: usize) {
    if let Some(path) = runtime_file("img339ctl-level") {
        let _ = fs::write(path, level.to_string());
    }
}

/// The level last stored; only up to date while the daemon is running.
pub fn load_level() -> Option<usize> {
    let level = fs::read_to_string(runtime_file("img339ctl-level")?).ok()?.trim().parse().ok()?;
    (level < NUM_LEVELS).then_some(level)
}
