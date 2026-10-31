//! macOS backend: the vendor interface is reached through IOKit, by way of hidapi.

use std::ffi::CString;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use hidapi::{HidApi, HidDevice};

use crate::protocol::Report;

const VENDOR_ID: u16 = 0x18F8;
const PRODUCT_IDS: [u16; 2] = [0x0FC0, 0x1086];
/// The collection that carries report ID 7.
const VENDOR_PAGE: u16 = 0xFF01;

/// IOKit paths of the vendor interface of every connected IMG-339.
pub fn find() -> Vec<PathBuf> {
    let Ok(api) = HidApi::new() else { return Vec::new() };
    api.device_list()
        .filter(|d| d.vendor_id() == VENDOR_ID && PRODUCT_IDS.contains(&d.product_id()) && d.usage_page() == VENDOR_PAGE)
        .filter_map(|d| d.path().to_str().ok().map(PathBuf::from))
        .collect()
}

/// Every collection with the mouse's vendor ID, for reports from testers when it isn't found.
pub fn diagnostics() -> Vec<String> {
    let api = match HidApi::new() {
        Ok(api) => api,
        Err(e) => return vec![format!("HID library: {e}")],
    };
    api.device_list()
        .filter(|d| d.vendor_id() == VENDOR_ID)
        .map(|d| format!("seen: {:04x}:{:04x} usage {:04x}:{:04x} interface {}", d.vendor_id(), d.product_id(), d.usage_page(), d.usage(), d.interface_number()))
        .collect()
}

fn open_device(path: Option<&Path>) -> Result<HidDevice> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => match find().into_iter().next() {
            Some(p) => p,
            None => bail!("IMG-339 mouse not found (18f8:0fc0 / 18f8:1086). Is it plugged in?"),
        },
    };
    let c_path = CString::new(path.as_os_str().as_bytes()).context("bad device path")?;
    let api = HidApi::new().context("starting the HID library")?;
    api.open_path(&c_path).with_context(|| {
        format!(
            "opening {}; if macOS blocked it, allow this app under System Settings > Privacy & Security > Input Monitoring",
            path.display()
        )
    })
}

/// Reads input reports, first byte being the report ID as on Linux.
pub struct Events(HidDevice);

impl Read for Events {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf).map_err(io::Error::other)
    }
}

pub fn open(path: Option<&Path>, _write: bool) -> Result<Events> {
    Ok(Events(open_device(path)?))
}

pub struct Mouse {
    device: HidDevice,
}

impl Mouse {
    pub fn open(path: Option<&Path>) -> Result<Self> {
        Ok(Mouse { device: open_device(path)? })
    }

    pub fn send(&self, report: &Report) -> Result<()> {
        self.device.send_feature_report(report).context("sending report failed")
    }
}
