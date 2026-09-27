//! Background payload manager: discovering, configuring autoload, and running daemons.
//!
//! Open-toolchain daemons (like `sandbox-daemon`) run in the background to provide
//! filesystem namespace elevation, symbol routing, and runtime services to titles.
//! This module scans the shared `payloads` directory (`%APPDATA%/OOPS/payloads`),
//! manages background worker processes, and allows users to start, stop, and configure
//! autoload on startup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use orbistoun_service::FileConfig;

/// A payload found in the payloads directory.
#[derive(Debug, Clone)]
pub(crate) struct PayloadEntry {
    pub(crate) filename: String,
    pub(crate) path: PathBuf,
    pub(crate) size_bytes: u64,
}

/// A background daemon process running an ELF payload.
pub(crate) struct DaemonProcess {
    pub(crate) filename: String,
    pub(crate) stopper: orbistoun_worker::Stopper,
    pub(crate) started_at: Instant,
    pub(crate) logs: Arc<Mutex<Vec<String>>>,
    pub(crate) status: Arc<Mutex<String>>,
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
}

/// The payload manager window and supervisor.
pub(crate) struct PayloadManager {
    /// Whether the payload manager window is open.
    pub(crate) open: bool,
    /// Detected payload binaries.
    pub(crate) entries: Vec<PayloadEntry>,
    /// Currently running daemons, keyed by filename (e.g. "sandbox-daemon.elf").
    pub(crate) running: HashMap<String, DaemonProcess>,
    /// Selected payload in the list, for details view.
    pub(crate) selected: Option<String>,
    /// Status message displayed at the bottom of the window.
    pub(crate) status: Option<String>,
}

impl PayloadManager {
    pub(crate) fn new() -> Self {
        Self {
            open: false,
            entries: Vec::new(),
            running: HashMap::new(),
            selected: None,
            status: None,
        }
    }

    pub(crate) fn is_running(&self, filename: &str) -> bool {
        self.running.contains_key(filename)
    }

    pub(crate) fn running_count(&self) -> usize {
        self.running.len()
    }

    pub(crate) fn rescan(&mut self, payloads_dir: &Path) {
        self.entries.clear();
        if !payloads_dir.exists() {
            let _ = std::fs::create_dir_all(payloads_dir);
        }
        let mut entries = Vec::new();
        Self::scan_dir_for_payloads(payloads_dir, &mut entries);
        entries.sort_by(|a, b| a.filename.cmp(&b.filename));
        self.entries = entries;
        if self.selected.is_none() && !self.entries.is_empty() {
            self.selected = Some(self.entries[0].filename.clone());
        }
    }

    fn scan_dir_for_payloads(dir: &Path, out: &mut Vec<PayloadEntry>) {
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    if ext.eq_ignore_ascii_case("elf") {
                        let filename = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let size_bytes = entry.metadata().map_or(0, |m| m.len());
                        out.push(PayloadEntry {
                            filename,
                            path,
                            size_bytes,
                        });
                    }
                }
            } else if path.is_dir() {
                // Also scan immediate subdirectories (like sandbox-daemon/sandbox-daemon.elf)
                let Ok(sub_read) = std::fs::read_dir(&path) else {
                    continue;
                };
                for sub in sub_read.flatten() {
                    let sub_path = sub.path();
                    if sub_path.is_file() {
                        if let Some(ext) = sub_path.extension().and_then(|s| s.to_str()) {
                            if ext.eq_ignore_ascii_case("elf") {
                                let filename = sub_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string();
                                let size_bytes = sub.metadata().map_or(0, |m| m.len());
                                if !out.iter().any(|e| e.filename == filename) {
                                    out.push(PayloadEntry {
                                        filename,
                                        path: sub_path,
                                        size_bytes,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Finds the closest matching payload entry for a configured or autoload.txt name.
    pub(crate) fn find_matching_entry(&self, name: &str) -> Option<&PayloadEntry> {
        let clean = name.trim().trim_end_matches(".elf");
        // 1. Exact filename match (case-insensitive)
        if let Some(entry) = self
            .entries
            .iter()
            .find(|e| e.filename.eq_ignore_ascii_case(name.trim()))
        {
            return Some(entry);
        }
        // 2. Exact basename match without extension
        if let Some(entry) = self.entries.iter().find(|e| {
            e.filename
                .trim_end_matches(".elf")
                .eq_ignore_ascii_case(clean)
        }) {
            return Some(entry);
        }
        // 3. Prefix/version match: e.g. "ftpsrv_v0.21" matches "ftpsrv_v0.21.1.elf", or "ftpsrv" matches "ftpsrv_v0.21.elf"
        if let Some(entry) = self.entries.iter().find(|e| {
            let entry_clean = e.filename.trim_end_matches(".elf");
            entry_clean
                .to_ascii_lowercase()
                .starts_with(&clean.to_ascii_lowercase())
                || clean
                    .to_ascii_lowercase()
                    .starts_with(&entry_clean.to_ascii_lowercase())
        }) {
            return Some(entry);
        }
        None
    }

    /// Autoloads payloads in the exact sequence requested, honoring delays (`!delay_ms`)
    /// and pldmgr `autoload.txt` syntax.
    pub(crate) fn autoload_configured(&mut self, payloads_dir: &Path, autoload: &[String]) {
        self.rescan(payloads_dir);

        // Sequence source: config.toml `autoload` list takes precedence if non-empty;
        // otherwise check for autoload.txt in payloads_dir, payloads_dir/pldmgr, or C:/tmp/ps5-pldmgr.
        let lines: Vec<String> = if !autoload.is_empty() {
            autoload.to_vec()
        } else if let Ok(text) = std::fs::read_to_string(payloads_dir.join("autoload.txt")) {
            text.lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        } else if let Ok(text) =
            std::fs::read_to_string(payloads_dir.join("pldmgr").join("autoload.txt"))
        {
            text.lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        } else if let Ok(text) = std::fs::read_to_string("C:/tmp/ps5-pldmgr/autoload.txt") {
            text.lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        } else {
            Vec::new()
        };

        for item in lines {
            let item = item.trim();
            if item.is_empty() || item.starts_with('#') || item.starts_with("//") {
                continue;
            }

            // pldmgr delay syntax: "!<millis>"
            if let Some(delay_str) = item.strip_prefix('!') {
                if let Ok(ms) = delay_str.parse::<u64>() {
                    eprintln!("orbistoun: autoload delay {ms}ms (pldmgr syntax)");
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                    continue;
                }
            }

            if let Some(entry) = self.find_matching_entry(item).cloned() {
                if !self.is_running(&entry.filename) {
                    eprintln!("orbistoun: autoload starting daemon '{}'", entry.filename);
                    self.start_payload(&entry.filename);
                }
            } else {
                eprintln!("orbistoun: autoload payload '{item}' not found in payloads directory");
            }
        }
    }

    pub(crate) fn start_payload(&mut self, filename: &str) {
        if self.is_running(filename) {
            return;
        }
        let Some(entry) = self
            .entries
            .iter()
            .find(|e| e.filename == filename)
            .cloned()
        else {
            self.status = Some(format!("Payload '{filename}' not found"));
            return;
        };

        let mut worker = match orbistoun_worker::WorkerHandle::spawn_self() {
            Ok(w) => w,
            Err(e) => {
                self.status = Some(format!("Spawning worker failed: {e}"));
                return;
            }
        };

        let stopper = worker.stopper();
        let logs = Arc::new(Mutex::new(Vec::new()));
        let status = Arc::new(Mutex::new("Running".to_string()));
        let logs_thread = Arc::clone(&logs);
        let status_thread = Arc::clone(&status);
        let path_thread = entry.path.clone();

        let thread = std::thread::spawn(move || {
            let req = orbistoun_proto::Request::Run {
                path: path_thread,
                symbols_db: None,
                limit_seconds: None,
                call_budget: None,
                input_script: None,
                capture_input: None,
                staged: false,
                relink: false,
            };

            let _ = worker.request_streaming(&req, |event| {
                if let orbistoun_proto::Event::Failed { error } = event {
                    let mut l = logs_thread
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    l.push(format!("Error: {error}"));
                }
            });
            *status_thread.lock().unwrap() = "Stopped".to_string();
            let _ = worker.shutdown();
        });

        self.running.insert(
            filename.to_string(),
            DaemonProcess {
                filename: filename.to_string(),
                stopper,
                started_at: Instant::now(),
                logs,
                status,
                thread: Some(thread),
            },
        );
        self.status = Some(format!("Started daemon '{filename}'"));
    }

    pub(crate) fn stop_payload(&mut self, filename: &str) {
        if let Some(mut daemon) = self.running.remove(filename) {
            daemon.stopper.stop();
            if let Some(handle) = daemon.thread.take() {
                let _ = handle.join();
            }
            self.status = Some(format!("Stopped daemon '{filename}'"));
        }
    }

    pub(crate) fn stop_all(&mut self) {
        for (_, mut daemon) in self.running.drain() {
            daemon.stopper.stop();
            if let Some(handle) = daemon.thread.take() {
                let _ = handle.join();
            }
        }
        self.status = Some("Stopped all daemons".to_string());
    }

    pub(crate) fn poll(&mut self) {
        let mut stopped = Vec::new();
        for (name, daemon) in &self.running {
            if let Some(thread) = &daemon.thread {
                if thread.is_finished() {
                    stopped.push(name.clone());
                }
            }
        }
        for name in stopped {
            if let Some(mut daemon) = self.running.remove(&name) {
                if let Some(handle) = daemon.thread.take() {
                    let _ = handle.join();
                }
            }
        }
    }

    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        config: &mut FileConfig,
        config_path: &Path,
        payloads_dir: &Path,
    ) {
        if !self.open {
            return;
        }

        self.poll();

        let mut open = self.open;
        egui::Window::new("Payload Manager")
            .open(&mut open)
            .default_width(680.0)
            .default_height(460.0)
            .show(ctx, |ui| {
                self.body(ui, config, config_path, payloads_dir);
            });
        self.open = open;
    }

    #[allow(clippy::too_many_lines)]
    fn body(
        &mut self,
        ui: &mut egui::Ui,
        config: &mut FileConfig,
        config_path: &Path,
        payloads_dir: &Path,
    ) {
        let mut to_start = None;
        let mut to_stop = None;
        let mut config_changed = false;

        // Toolbar
        ui.horizontal(|ui| {
            if ui.button("Rescan").clicked() {
                self.rescan(payloads_dir);
            }
            if ui.button("Open Folder").clicked() {
                open_in_file_manager(payloads_dir);
            }
            let autoload_path = payloads_dir.join("autoload.txt");
            if autoload_path.exists()
                && ui
                    .button("Sync autoload.txt")
                    .on_hover_text("Load payload order and delays from payloads/autoload.txt")
                    .clicked()
            {
                if let Ok(text) = std::fs::read_to_string(&autoload_path) {
                    config.payloads.autoload = text
                        .lines()
                        .map(str::trim)
                        .filter(|s| !s.is_empty() && !s.starts_with('#') && !s.starts_with("//"))
                        .map(String::from)
                        .collect();
                    config_changed = true;
                    self.status = Some("Loaded autoload.txt into configuration".to_string());
                }
            }
            if ui
                .button("Export autoload.txt")
                .on_hover_text("Write current autoload sequence to payloads/autoload.txt")
                .clicked()
            {
                let text = config.payloads.autoload.join("\r\n");
                if std::fs::write(&autoload_path, text).is_ok() {
                    self.status = Some(format!("Exported sequence to {}", autoload_path.display()));
                }
            }
            ui.separator();
            ui.label(format!(
                "{} running / {} discovered",
                self.running.len(),
                self.entries.len()
            ));
        });

        ui.separator();

        // Main table of payloads
        ui.label(egui::RichText::new("Discovered Payloads:").strong());
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                egui::Grid::new("payloads_grid")
                    .striped(true)
                    .num_columns(5)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Status").strong());
                        ui.label(egui::RichText::new("Payload").strong());
                        ui.label(egui::RichText::new("Size").strong());
                        ui.label(egui::RichText::new("Autoload").strong());
                        ui.label(egui::RichText::new("Action").strong());
                        ui.end_row();

                        for entry in &self.entries {
                            let is_running = self.is_running(&entry.filename);
                            let is_selected = self.selected.as_deref() == Some(&entry.filename);

                            // Status column
                            if is_running {
                                ui.colored_label(egui::Color32::from_rgb(50, 205, 50), "● Running");
                            } else {
                                ui.weak("○ Stopped");
                            }

                            // Name column (clickable to select)
                            let label_text = if is_selected {
                                egui::RichText::new(&entry.filename).strong().underline()
                            } else {
                                egui::RichText::new(&entry.filename)
                            };
                            if ui.link(label_text).clicked() {
                                self.selected = Some(entry.filename.clone());
                            }

                            // Size column
                            let size_kb = entry.size_bytes.div_ceil(1024);
                            ui.label(format!("{size_kb} KB"));

                            // Autoload checkbox and sequence badge
                            let pos = config.payloads.autoload.iter().position(|s| {
                                s.eq_ignore_ascii_case(&entry.filename)
                                    || s.eq_ignore_ascii_case(
                                        entry.filename.trim_end_matches(".elf"),
                                    )
                            });
                            let mut autoloaded = pos.is_some();
                            let label = pos
                                .map_or_else(String::new, |idx| format!("#{pos}", pos = idx + 1));
                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut autoloaded, label).changed() {
                                    config_changed = true;
                                    if autoloaded {
                                        if !config
                                            .payloads
                                            .autoload
                                            .iter()
                                            .any(|s| s.eq_ignore_ascii_case(&entry.filename))
                                        {
                                            config.payloads.autoload.push(entry.filename.clone());
                                        }
                                    } else {
                                        config.payloads.autoload.retain(|s| {
                                            !s.eq_ignore_ascii_case(&entry.filename)
                                                && !s.eq_ignore_ascii_case(
                                                    entry.filename.trim_end_matches(".elf"),
                                                )
                                        });
                                    }
                                }
                            });

                            // Action column
                            if is_running {
                                if ui.button("Stop").clicked() {
                                    to_stop = Some(entry.filename.clone());
                                }
                            } else if ui.button("Start").clicked() {
                                to_start = Some(entry.filename.clone());
                            }

                            ui.end_row();
                        }
                    });
            });

        if config_changed {
            if let Ok(text) = config.to_toml() {
                let _ = std::fs::write(config_path, text);
            }
        }

        if let Some(f) = to_start {
            self.start_payload(&f);
        }
        if let Some(f) = to_stop {
            self.stop_payload(&f);
        }

        ui.separator();

        // Selected Payload Details
        if let Some(selected_name) = &self.selected {
            ui.label(egui::RichText::new(format!("Details: {selected_name}")).strong());
            if let Some(entry) = self.entries.iter().find(|e| &e.filename == selected_name) {
                ui.horizontal(|ui| {
                    ui.label("Path:");
                    ui.monospace(entry.path.to_string_lossy());
                });
                if let Some(daemon) = self.running.get(selected_name) {
                    let elapsed = daemon.started_at.elapsed().as_secs();
                    let st = daemon
                        .status
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    ui.label(format!("Status: {st} (uptime {elapsed}s)"));
                    if daemon.filename.starts_with("sandbox-daemon") {
                        ui.label(egui::RichText::new("Namespace Service: listening on TCP 127.0.0.1:9069 for client title elevation requests.").italics());
                    }
                    let logs = daemon
                        .logs
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    if !logs.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new("Recent Output:").strong());
                        egui::ScrollArea::vertical()
                            .max_height(100.0)
                            .show(ui, |ui| {
                                for line in &logs {
                                    ui.monospace(line);
                                }
                            });
                    }
                } else {
                    ui.weak("Status: Not currently running");
                }
            }
        }

        ui.separator();

        // Bottom Controls
        ui.horizontal(|ui| {
            if ui.button("Start All Autoload").clicked() {
                let autoload = config.payloads.autoload.clone();
                let to_start: Vec<String> = self
                    .entries
                    .iter()
                    .filter_map(|entry| {
                        let should_start = autoload.iter().any(|s| {
                            s.eq_ignore_ascii_case(&entry.filename)
                                || s.eq_ignore_ascii_case(entry.filename.trim_end_matches(".elf"))
                        });
                        if should_start && !self.is_running(&entry.filename) {
                            Some(entry.filename.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                for name in to_start {
                    self.start_payload(&name);
                }
            }
            if ui.button("Stop All").clicked() {
                self.stop_all();
            }
            if let Some(msg) = &self.status {
                ui.separator();
                ui.weak(msg);
            }
        });
    }
}

fn open_in_file_manager(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}
