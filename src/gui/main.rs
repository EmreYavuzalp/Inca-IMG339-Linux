//! img339ctl-gui: graphical settings for the Inca IMG-339 gaming mouse.

mod i18n;

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use i18n::{Strings, ACTION_NAMES};
use img339ctl::{daemon, device};
use img339ctl::profile::{self, ActionKind, Profile, Repeat, COMMANDS, SHORTCUTS};
use img339ctl::protocol::{self as p, LedMode, CONSUMER_USAGES, DPI_VALUES, HW_FUNCTIONS};

const CURRENT: Color32 = Color32::from_rgb(60, 170, 90);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Dpi,
    Led,
    Buttons,
    Macros,
    Other,
}

enum Status {
    None,
    Ok(String),
    Err(String),
}

struct App {
    t: &'static Strings,
    turkish: bool,
    path: PathBuf,
    /// The profile being edited and the one last saved/applied.
    prof: Profile,
    saved: Profile,
    tab: Tab,
    /// Text fields for custom button actions, one per button.
    custom: [String; p::NUM_BUTTONS],
    new_macro: String,
    status: Status,
    mouse: Vec<PathBuf>,
    daemon: bool,
    last_poll: Instant,
    /// The mouse's current 0-based DPI level, once known.
    level: Arc<Mutex<Option<usize>>>,
    /// The level the radio buttons last jumped to.
    followed: Option<usize>,
}

/// Follows the DPI level the mouse reports with each DPI button press.
fn watch_level(level: Arc<Mutex<Option<usize>>>, ctx: egui::Context) {
    let mut buf = [0u8; 64];
    loop {
        if let Ok(mut events) = device::open(None, false) {
            while let Ok(n) = events.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if let Some(l) = p::parse_level(&buf[..n]) {
                    *level.lock().unwrap() = Some(l);
                    ctx.request_repaint();
                }
            }
            *level.lock().unwrap() = None;
            ctx.request_repaint();
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let turkish = i18n::turkish();
        let path = profile::default_path();
        let (prof, status) = match profile::load(&path) {
            Ok(p) => (p, Status::None),
            Err(e) => (Profile::default(), Status::Err(format!("{e:#}"))),
        };
        let level = Arc::new(Mutex::new(None));
        let (watched, ctx) = (Arc::clone(&level), ctx.clone());
        std::thread::spawn(move || watch_level(watched, ctx));
        let mut app = App {
            t: if turkish { &i18n::TR } else { &i18n::EN },
            turkish,
            path,
            saved: prof.clone(),
            prof,
            tab: Tab::Dpi,
            custom: Default::default(),
            new_macro: String::new(),
            status,
            mouse: Vec::new(),
            daemon: false,
            last_poll: Instant::now() - Duration::from_secs(10),
            level,
            followed: None,
        };
        app.sync_custom();
        app
    }

    fn sync_custom(&mut self) {
        for (i, a) in self.prof.buttons.iter().enumerate() {
            self.custom[i] = a.text.clone();
        }
    }

    fn label(&self, id: &str) -> String {
        match ACTION_NAMES.iter().find(|(n, _, _)| *n == id) {
            Some((_, tr, en)) => (if self.turkish { tr } else { en }).to_string(),
            None => id.to_string(),
        }
    }

    fn action_label(&self, a: &profile::Action) -> String {
        match &a.kind {
            ActionKind::Macro(name, Repeat::Hold) => format!("{name} ({})", self.t.while_held),
            ActionKind::Macro(name, Repeat::Times(1)) => format!("{name} ({})", self.t.once),
            ActionKind::Macro(name, Repeat::Times(n)) => format!("{name} (×{n})"),
            _ => self.label(&a.text),
        }
    }

    fn poll(&mut self) {
        if self.last_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_poll = Instant::now();
        self.mouse = device::find();
        self.daemon = daemon::is_running();
        let mut level = self.level.lock().unwrap();
        if self.mouse.is_empty() {
            *level = None;
        } else if self.daemon {
            // The daemon has been listening all along, also while this window was closed.
            *level = device::load_level().or(*level);
        }
    }

    fn current_level(&self) -> Option<usize> {
        *self.level.lock().unwrap()
    }

    /// Moves the radio buttons when the mouse changes level. This is not an edit,
    /// so it must not count as an unapplied change.
    fn follow_level(&mut self) {
        let current = self.current_level();
        if current == self.followed {
            return;
        }
        self.followed = current;
        if let Some(l) = current {
            self.prof.active = l;
            self.prof.enabled[l] = true;
            self.saved.active = l;
        }
    }

    fn apply(&mut self) {
        let result = self.prof.reports().and_then(|reports| {
            device::Mouse::open(None)?.send_all(&reports)?;
            profile::save(&self.prof, &self.path)
        });
        self.status = match result {
            Ok(()) => {
                self.saved = self.prof.clone();
                *self.level.lock().unwrap() = Some(self.prof.active);
                device::store_level(self.prof.active);
                Status::Ok(self.t.applied.to_string())
            }
            Err(e) => Status::Err(format!("{e:#}")),
        };
    }

    fn start_daemon(&mut self) {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("img339ctl")))
            .filter(|p| p.exists())
            .unwrap_or_else(|| PathBuf::from("img339ctl"));
        let spawned = Command::new(exe)
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn();
        self.status = match spawned {
            Ok(_) => Status::Ok(self.t.daemon_started.to_string()),
            Err(e) => Status::Err(format!("img339ctl daemon: {e}")),
        };
        self.last_poll = Instant::now() - Duration::from_secs(10);
    }

    fn dpi_tab(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        let current = self.current_level().filter(|_| !self.mouse.is_empty());
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}:", t.current_dpi)).size(16.0));
            match current {
                // The mouse holds the applied values, not the edits still in this window.
                Some(l) => ui.label(RichText::new(format!("{} ({} {})", self.saved.dpi[l], t.level, l + 1)).size(16.0).strong().color(CURRENT)),
                None => ui.label(RichText::new(t.level_unknown).weak()),
            };
        });
        ui.add_space(10.0);
        egui::Grid::new("dpi").num_columns(5).spacing([24.0, 10.0]).striped(true).show(ui, |ui| {
            for h in [t.level, t.current, t.enabled, "DPI", t.color] {
                ui.strong(h);
            }
            ui.end_row();
            for i in 0..p::NUM_LEVELS {
                if current == Some(i) {
                    ui.label(RichText::new(format!("{}", i + 1)).strong().color(CURRENT));
                } else {
                    ui.label(format!("{}", i + 1));
                }
                if ui.radio(self.prof.active == i, "").clicked() {
                    self.prof.active = i;
                    self.prof.enabled[i] = true;
                }
                let only_one = self.prof.enabled.iter().filter(|&&e| e).count() == 1 && self.prof.enabled[i];
                ui.add_enabled_ui(!only_one && self.prof.active != i, |ui| ui.checkbox(&mut self.prof.enabled[i], ""));
                egui::ComboBox::from_id_salt(("dpi", i))
                    .selected_text(self.prof.dpi[i].to_string())
                    .width(80.0)
                    .show_ui(ui, |ui| {
                        for v in DPI_VALUES {
                            ui.selectable_value(&mut self.prof.dpi[i], v, v.to_string());
                        }
                    });
                let c = self.prof.colors[i];
                let mut rgb = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    self.prof.colors[i] = (rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32;
                }
                ui.end_row();
            }
        });
        ui.add_space(12.0);
        ui.label(RichText::new(t.color_note).weak());
    }

    fn led_tab(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        ui.strong(t.led_mode);
        ui.horizontal(|ui| {
            for (mode, name) in [(LedMode::Dpi, t.led_dpi), (LedMode::Cycle, t.led_cycle), (LedMode::Static, t.led_static), (LedMode::Off, t.led_off)] {
                ui.radio_value(&mut self.prof.led_mode, mode, name);
            }
        });
        ui.add_space(12.0);
        if self.prof.led_mode == LedMode::Static {
            ui.horizontal(|ui| {
                ui.label(t.static_color);
                // The mouse has no separate static colour: every DPI level gets it.
                let c = self.prof.colors[self.prof.active];
                let mut rgb = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    let c = (rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32;
                    self.prof.colors.iter_mut().for_each(|x| *x = c);
                }
            });
            ui.label(RichText::new(t.static_note).weak());
            ui.add_space(12.0);
        }
        ui.add_enabled_ui(self.prof.led_mode == LedMode::Cycle, |ui| {
            ui.horizontal(|ui| {
                ui.label(t.cycle_time);
                ui.add(egui::Slider::new(&mut self.prof.cycle_time, 1..=6).suffix(" s"));
            });
            ui.add_space(8.0);
            ui.label(t.cycle_colors);
            ui.horizontal(|ui| {
                for i in 0..p::NUM_CYCLE_COLORS {
                    let only_one = self.prof.cycle_colors.iter().filter(|&&c| c).count() == 1 && self.prof.cycle_colors[i];
                    ui.add_enabled_ui(!only_one, |ui| ui.checkbox(&mut self.prof.cycle_colors[i], format!("{}", i + 1)));
                }
            });
        });
    }

    fn buttons_tab(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        egui::Grid::new("buttons").num_columns(3).spacing([16.0, 10.0]).striped(true).show(ui, |ui| {
            for i in 0..p::NUM_BUTTONS {
                ui.label(format!("{}  {}", i + 1, t.button_names[i]));
                let mut chosen: Option<String> = None;
                egui::ComboBox::from_id_salt(("button", i))
                    .selected_text(self.action_label(&self.prof.buttons[i]))
                    .width(200.0)
                    .height(420.0)
                    .show_ui(ui, |ui| {
                        let current = &self.prof.buttons[i].text;
                        let mut group = |ui: &mut egui::Ui, title: &str, ids: Vec<String>| {
                            ui.label(RichText::new(title).small().weak());
                            for id in ids {
                                let label = match id.strip_prefix("macro:") {
                                    Some(rest) => {
                                        let (name, rep) = rest.split_once(':').unwrap_or((rest, ""));
                                        let rep = if rep == "hold" { t.while_held } else { t.once };
                                        format!("{name} ({rep})")
                                    }
                                    None => self.label(&id),
                                };
                                if ui.selectable_label(*current == id, label).clicked() {
                                    chosen = Some(id);
                                }
                            }
                            ui.separator();
                        };
                        let names = |table: &[(&str, _)]| table.iter().map(|(n, _): &(&str, _)| n.to_string()).collect::<Vec<_>>();
                        group(ui, t.group_mouse, names(HW_FUNCTIONS));
                        group(ui, t.group_media, CONSUMER_USAGES.iter().map(|(n, _)| n.to_string()).collect());
                        group(ui, t.group_shortcuts, SHORTCUTS.iter().map(|(n, _)| n.to_string()).collect());
                        group(ui, t.group_commands, COMMANDS.iter().map(|(n, _)| n.to_string()).collect());
                        if !self.prof.macros.is_empty() {
                            let ids = self.prof.macros.keys().flat_map(|m| [format!("macro:{m}"), format!("macro:{m}:hold")]).collect();
                            group(ui, t.group_macros, ids);
                        }
                    });
                if let Some(id) = chosen {
                    if let Ok(a) = profile::parse_action(&id) {
                        self.custom[i] = a.text.clone();
                        self.prof.buttons[i] = a;
                    }
                }
                let parsed = profile::parse_action(&self.custom[i]);
                let valid = match &parsed {
                    Ok(a) => !matches!(&a.kind, ActionKind::Macro(m, _) if !self.prof.macros.contains_key(m)),
                    Err(_) => false,
                };
                let edit = egui::TextEdit::singleline(&mut self.custom[i])
                    .desired_width(180.0)
                    .hint_text("keys:ctrl+t")
                    .text_color_opt((!valid).then_some(Color32::from_rgb(220, 60, 60)));
                let resp = ui.add(edit).on_hover_text(t.custom_hint);
                if resp.changed() && valid {
                    self.prof.buttons[i] = parsed.unwrap();
                }
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new(t.custom_hint).weak());
    }

    fn macros_tab(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        if self.prof.macros.is_empty() {
            ui.label(t.macro_none);
        }
        let mut delete = None;
        let used: Vec<String> = self
            .prof
            .buttons
            .iter()
            .filter_map(|a| match &a.kind {
                ActionKind::Macro(m, _) => Some(m.clone()),
                _ => None,
            })
            .collect();
        egui::Grid::new("macros").num_columns(3).spacing([12.0, 10.0]).show(ui, |ui| {
            for (name, text) in self.prof.macros.iter_mut() {
                ui.strong(name.as_str());
                let valid = img339ctl::keys::parse_macro(text).is_ok();
                ui.add(
                    egui::TextEdit::singleline(text)
                        .desired_width(360.0)
                        .text_color_opt((!valid).then_some(Color32::from_rgb(220, 60, 60))),
                );
                let in_use = used.contains(name);
                if ui.add_enabled(!in_use, egui::Button::new(t.macro_delete)).on_disabled_hover_text(t.macro_in_use).clicked() {
                    delete = Some(name.clone());
                }
                ui.end_row();
            }
        });
        if let Some(name) = delete {
            self.prof.macros.remove(&name);
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(t.macro_name);
            ui.add(egui::TextEdit::singleline(&mut self.new_macro).desired_width(140.0));
            let name = self.new_macro.trim().to_string();
            let ok = !name.is_empty() && !name.contains([':', '=', '[', ']']) && !self.prof.macros.contains_key(&name);
            if ui.add_enabled(ok, egui::Button::new(t.macro_add)).clicked() {
                self.prof.macros.insert(name, "text:hello".into());
                self.new_macro.clear();
            }
        });
        ui.add_space(12.0);
        ui.label(RichText::new(t.macro_help).monospace().weak());
    }

    fn other_tab(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        ui.horizontal(|ui| {
            ui.label(t.fire);
            let mut units = self.prof.fire_ms / 8;
            if ui.add(egui::Slider::new(&mut units, 1..=255).custom_formatter(|v, _| format!("{} ms", v as u32 * 8))).changed() {
                self.prof.fire_ms = units * 8;
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(t.wheel);
            ui.radio_value(&mut self.prof.wheel_volume, false, t.wheel_scroll);
            ui.radio_value(&mut self.prof.wheel_volume, true, t.wheel_volume);
        });
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.t;
        let dirty = self.prof != self.saved;
        let valid = self.prof.validate();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (color, text) = if self.mouse.is_empty() {
                (Color32::from_rgb(220, 60, 60), t.mouse_missing)
            } else {
                (Color32::from_rgb(60, 170, 90), t.mouse_found)
            };
            status_dot(ui, color);
            ui.colored_label(color, text);
            if let Some(l) = self.current_level().filter(|_| !self.mouse.is_empty()) {
                ui.label(RichText::new(format!("· {} DPI", self.saved.dpi[l])).strong());
            }
            let needs_daemon = self.prof.buttons.iter().any(|a| a.is_host());
            if self.daemon {
                ui.label(RichText::new(format!("· {}", t.daemon_running)).weak());
            } else if needs_daemon {
                ui.label(RichText::new(format!("· {}", t.daemon_needed)).color(Color32::from_rgb(220, 150, 40)));
                if ui.small_button(t.daemon_start).clicked() {
                    self.start_daemon();
                }
            }
        });
        ui.horizontal(|ui| {
            match (&self.status, &valid) {
                (_, Err(e)) => ui.colored_label(Color32::from_rgb(220, 60, 60), format!("{}: {e:#}", t.invalid)),
                (Status::Err(e), _) => ui.colored_label(Color32::from_rgb(220, 60, 60), e),
                _ if dirty => ui.label(RichText::new(t.unsaved).italics()),
                (Status::Ok(m), _) => ui.colored_label(Color32::from_rgb(60, 170, 90), m),
                (Status::None, _) => ui.label(""),
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(valid.is_ok() && !self.mouse.is_empty(), egui::Button::new(RichText::new(t.apply).strong())).clicked() {
                    self.apply();
                }
                if ui.add_enabled(dirty, egui::Button::new(t.revert)).clicked() {
                    self.prof = self.saved.clone();
                    self.sync_custom();
                    self.status = Status::None;
                }
                if ui.button(t.defaults).clicked() {
                    self.prof = Profile::default();
                    self.sync_custom();
                }
            });
        });
        ui.add_space(6.0);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.follow_level();
        ctx.request_repaint_after(Duration::from_secs(2));
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let t = self.t;
                for (tab, name) in [(Tab::Dpi, t.tab_dpi), (Tab::Led, t.tab_led), (Tab::Buttons, t.tab_buttons), (Tab::Macros, t.tab_macros), (Tab::Other, t.tab_other)] {
                    if ui.selectable_value(&mut self.tab, tab, RichText::new(name).size(15.0)).clicked() {
                        // The tab strip was already drawn with the old selection.
                        ctx.request_repaint();
                    }
                }
            });
            ui.add_space(6.0);
        });
        egui::TopBottomPanel::bottom("bar").show(ctx, |ui| self.bottom_bar(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(8.0);
                match self.tab {
                    Tab::Dpi => self.dpi_tab(ui),
                    Tab::Led => self.led_tab(ui),
                    Tab::Buttons => self.buttons_tab(ui),
                    Tab::Macros => self.macros_tab(ui),
                    Tab::Other => self.other_tab(ui),
                }
            });
        });
    }
}

/// A small filled circle; drawn rather than typed because the bundled fonts lack "●".
fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, color);
}

/// Prints log records to stderr when IMG339_LOG is set (error, warn, info, debug or trace),
/// so graphics driver problems can be diagnosed without a debugger.
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{} {}] {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

/// Re-executes this program with Mesa's software OpenGL; returns only if that is not possible.
fn restart_with_software_gl() {
    if std::env::var_os("LIBGL_ALWAYS_SOFTWARE").is_some() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    log::warn!("only OpenGL ES 2.0 is available; restarting with software rendering");
    let err = Command::new(exe).args(std::env::args_os().skip(1)).env("LIBGL_ALWAYS_SOFTWARE", "1").exec();
    log::warn!("could not restart with software rendering: {err}");
}

fn init_logging() {
    let Ok(level) = std::env::var("IMG339_LOG") else { return };
    let level = level.parse().unwrap_or(log::LevelFilter::Debug);
    if log::set_logger(&StderrLogger).is_ok() {
        log::set_max_level(level);
    }
}

fn main() -> eframe::Result {
    init_logging();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(if i18n::turkish() { i18n::TR.title } else { i18n::EN.title })
            .with_app_id("img339ctl-gui")
            .with_inner_size([640.0, 520.0])
            .with_min_inner_size([560.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "img339ctl-gui",
        options,
        Box::new(|cc| {
            if let Some(gl) = &cc.gl {
                use eframe::glow::HasContext;
                let v = gl.version();
                log::debug!("GL {}.{} embedded={} {}", v.major, v.minor, v.is_embedded, v.vendor_info);
                // Old GPUs (e.g. Intel Ironlake) only offer OpenGL ES 2.0 here, where egui draws a
                // black window. Mesa's software renderer works fine for a small settings window.
                if v.is_embedded && v.major < 3 {
                    restart_with_software_gl();
                }
            }
            Ok(Box::new(App::new(&cc.egui_ctx)))
        }),
    )
}
