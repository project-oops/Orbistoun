//! The window: a menu strip, a toolbar, a library, and a title.

use orbistoun_service::{Service, ServiceConfig, TitleEntry};
use orbistoun_shell::{Start, View};

use crate::capture;
use crate::icons::{self, Icons};
use crate::prefs::{self, Preferences};
use crate::run;

/// What is known about the selected title without running it.
///
/// Held rather than recomputed each frame, because parsing a container is real work.
struct Detail {
    /// Container summary, or why it could not be read.
    inspect: Result<String, String>,
    /// How many imports it needs, and how many are named.
    imports: Option<(usize, usize)>,
}

/// Per-title settings, while the window for them is open.
struct TitleConfig {
    /// Which title these belong to, since the window can outlive a selection change.
    title: String,
    /// The user layer as editable text.
    ///
    /// Text rather than a form, because override entries carry mandatory reasons that a
    /// form would drop or have to invent a control for.
    text: String,
    /// What happened to the last save.
    status: Option<Result<String, String>>,
}

/// What was asked for while drawing, to be acted on after it.
///
/// The list, the toolbar, a shell menu and a double-click can ask for something while
/// borrowing the state that would change, so each request is deferred to the end of the
/// frame.
#[derive(Debug, Clone, Copy, Default)]
struct Deferred {
    /// Start the selected title.
    launch: bool,
    /// Ask the viewport for its pixels.
    screenshot: bool,
    /// Close the window.
    close: bool,
}

/// The application.
pub(crate) struct App {
    /// The documentation reader. Holds which page is open and the parsed pages already
    /// viewed, so markdown is not re-parsed every frame.
    docs: oops_docs::DocsWindow,
    service: Service,
    paths: orbistoun_paths::Paths,
    /// Titles found, or why the scan failed; a failure is not an empty list.
    titles: Result<Vec<TitleEntry>, String>,
    /// What the library list draws, built when the library or a run changes.
    ///
    /// Not rebuilt per frame, because each row carries a last-run summary read from a
    /// trace file on disk.
    rows: Vec<Row>,
    /// The probe window.
    ///
    /// Kept on the application so a session survives the window being closed and reopened.
    probe: crate::probe::Panel,
    selected: Option<usize>,
    detail: Option<Detail>,
    prefs: Preferences,
    title_config: Option<TitleConfig>,
    icons: Icons,
    running: Option<run::InFlight>,
    /// The frame the running title last presented, uploaded. Shown in place of the library
    /// while the title runs; cleared when the next run starts.
    live: Option<egui::TextureHandle>,
    /// Whether [`Self::live`] holds a frame from the run in flight, rather than the last run's.
    live_fresh: bool,
    /// Where the running title's last second went, drawn over its picture.
    perf: Option<orbistoun_proto::PerfReport>,
    /// Whether that overlay is showing; [`crate::perf_overlay::TOGGLE`] flips it.
    show_perf: bool,
    /// The launcher a running title was started from, returned to when that title's run
    /// ends, as the system returns to its home screen.
    home: Option<std::path::PathBuf>,
    finished: Option<run::Finished>,
    /// What was asked for while drawing. See [`Deferred`].
    ///
    /// A screenshot request and its reply are on different frames (see [`crate::capture`]).
    deferred: Deferred,
    /// Which build this is, computed once.
    build: String,
    /// Where the last capture went, or why it did not.
    ///
    /// Shown in the toolbar rather than logged, so the user sees the outcome.
    last_capture: Option<capture::Outcome>,
    /// The file pad input is being captured into, or with nothing running, will be from the
    /// next launch (D721). Set only by the toolbar's "capture input".
    input_capture: Option<std::path::PathBuf>,
    /// The last capture's file, for the toolbar to say where it went.
    input_captured: Option<std::path::PathBuf>,
    /// The pad script playing, or with nothing running, to play from the next launch (D721).
    input_playback: Option<std::path::PathBuf>,
    /// Which view is showing.
    ///
    /// The shell and the list draw the same scan and selection.
    view: View,
    /// Which graphics backend this window actually got, and the adapter behind it.
    ///
    /// Reported rather than assumed: `wgpu` chooses from a set holding both Vulkan and
    /// DX12 on Windows, and nothing pins it.
    renderer: String,
    /// A title named on the command line that has not been found yet.
    ///
    /// Kept rather than resolved immediately, so a rescan after a failed scan retries it.
    wanted_title: Option<String>,
    /// The last frame of pad state, so the controllers pane can light a pressed button.
    last_pads: Vec<orbistoun_input::PadState>,
    /// Host input, and the shell button's press across frames.
    input: crate::input::Reader,
    /// Where the title stands, as this process sees it. The worker keeps its own copy, and a
    /// disagreement is counted (D310); this copy decides what is drawn.
    session: orbistoun_shell::Lifecycle,
    /// Which menu the shell button opened, if any.
    ///
    /// The overlay itself is [`orbistoun_shell::Lifecycle::Overlaid`], not a flag here.
    power_menu: bool,
    /// When the last frame was drawn, for the press-versus-hold decision.
    last_frame: std::time::Instant,
    /// Where the highlight is in the shell.
    ///
    /// Held here rather than in the drawing, so a controller and a pointer move the same
    /// highlight.
    at: orbistoun_shell::Cross,
}

impl App {
    /// Builds the window state and scans the default library.
    ///
    /// `start` has already reconciled the command line with the stored setting in
    /// `orbistoun_shell::startup`.
    pub(crate) fn new(start: Start, renderer: String) -> Self {
        let paths = orbistoun_paths::Paths::resolve();
        let _ = paths.ensure_dirs();
        // Read first: it carries the library folder the scan below uses.
        let prefs = Preferences::load(&paths.config_file(), &paths.shell_file(), paths.data_root());
        let service = Service::new(ServiceConfig {
            paths: Some(paths.clone()),
            entry_settings: prefs.file.entry.clone(),
            thread_settings: prefs.file.threads,
            memory_settings: prefs.file.memory,
            ..ServiceConfig::default()
        });
        let mut app = Self {
            service,
            paths,
            titles: Ok(Vec::new()),
            rows: Vec::new(),
            probe: crate::probe::Panel::default(),
            selected: None,
            detail: None,
            prefs,
            title_config: None,
            icons: Icons::default(),
            running: None,
            live: None,
            live_fresh: false,
            perf: None,
            show_perf: true,
            home: None,
            finished: None,
            deferred: Deferred::default(),
            build: orbistoun_env::build::line(),
            docs: oops_docs::DocsWindow::default(),

            last_capture: None,
            input_capture: None,
            input_captured: None,
            input_playback: None,
            view: match &start {
                Start::In(view) => *view,
                Start::Title { fallback, .. } => *fallback,
            },
            renderer,
            wanted_title: match start {
                Start::In(_) => None,
                Start::Title { name, .. } => Some(name),
            },
            last_pads: Vec::new(),
            input: crate::input::Reader::default(),
            // Nothing is running yet.
            session: orbistoun_shell::Lifecycle::Exited,
            power_menu: false,

            last_frame: std::time::Instant::now(),
            at: orbistoun_shell::Cross {
                category: crate::shell::Category::START,
                item: 0,
            },
        };
        app.rescan();
        // After the scan, which it matches against. An unmatched name leaves the window in
        // the fallback view with a message.
        app.take_wanted_title();
        app
    }

    /// Selects and launches a title named on the command line, if one was.
    ///
    /// Matched case-insensitively on either the folder name or the title identifier. A
    /// name nothing matches is kept, so a rescan after fixing the library folder retries it.
    fn take_wanted_title(&mut self) {
        let Some(wanted) = self.wanted_title.clone() else {
            return;
        };
        let found = self.rows.iter().position(|row| {
            row.key.eq_ignore_ascii_case(&wanted) || row.id.eq_ignore_ascii_case(&wanted)
        });
        if let Some(index) = found {
            self.inspect(index);
            self.deferred.launch = true;
            self.wanted_title = None;
        }
    }

    /// Acts on what somebody did in the shell.
    fn shell_action(&mut self, action: crate::shell::Action) {
        match action {
            crate::shell::Action::Launch(index) => {
                self.inspect(index);
                // Deferred, as the list does it: the borrow held while drawing is one a run
                // needs.
                self.deferred.launch = true;
            }
            crate::shell::Action::ToList => self.view = View::List,
            crate::shell::Action::Settings => self.prefs.open = true,
            crate::shell::Action::Rescan => self.rescan(),
            crate::shell::Action::Quit => {
                self.power_menu = false;
                self.quit_running_title();
            }
            crate::shell::Action::Resume => {
                self.power_menu = false;
                if self.session == orbistoun_shell::Lifecycle::Overlaid {
                    self.shell_request(orbistoun_shell::Request::CloseOverlay);
                }
            }
            crate::shell::Action::CloseEmulator => self.deferred.close = true,
        }
    }

    /// Reads input and acts on the shell button.
    ///
    /// A tap moves the session between `Lifecycle::Foreground` and `Lifecycle::Overlaid`,
    /// and a hold opens the power menu, both through the tested `Lifecycle::on`. The same
    /// request goes to the worker, which delivers the event only if its code is measured
    /// (D311).
    fn read_input(&mut self, ctx: &egui::Context) {
        use orbistoun_shell::Request;

        let elapsed = self.last_frame.elapsed();
        self.last_frame = std::time::Instant::now();
        let elapsed_ms = u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX);

        let frame = self.input.read(ctx, &self.prefs.file.pads, elapsed_ms);
        // Kept for the controllers pane, which lights a button as it goes down.
        self.last_pads = frame.pads;

        // What the title may see is decided here: the shell's button is always removed, and
        // a title without focus is handed a neutral pad (D345).
        if let Some(in_flight) = &self.running {
            let neutral = self.session.focus().neutral_for_title();
            let seen: Vec<orbistoun_input::PadState> = self
                .last_pads
                .iter()
                .map(|pad| {
                    if neutral {
                        orbistoun_input::PadState::neutral()
                    } else {
                        pad.as_title_sees_it()
                    }
                })
                .collect();
            in_flight.input(&seen);
        }

        // Set every frame rather than at launch, because a run ends on its own.
        self.session = if self.running.is_some() {
            match self.session {
                orbistoun_shell::Lifecycle::Exited => orbistoun_shell::Lifecycle::Foreground,
                held => held,
            }
        } else {
            orbistoun_shell::Lifecycle::Exited
        };

        match frame.shell {
            orbistoun_input::ShellPress::None => {}
            orbistoun_input::ShellPress::Tap => {
                // A tap closes an open power menu without also toggling the overlay.
                if self.power_menu {
                    self.power_menu = false;
                } else if self.session == orbistoun_shell::Lifecycle::Overlaid {
                    self.shell_request(Request::CloseOverlay);
                } else {
                    self.shell_request(Request::OpenOverlay);
                }
            }
            orbistoun_input::ShellPress::Hold => self.power_menu = true,
        }

        // Navigation only while the shell is showing; otherwise a direction belongs to the
        // title.
        if self.view == View::Shell && self.session != orbistoun_shell::Lifecycle::Foreground {
            let shape = crate::shell::shape(self.rows.len(), self.running.is_some());
            if let Some(direction) = crate::input::steering(frame.just_pressed) {
                self.at.steer(direction, &shape);
            } else {
                // Also when nothing moved, because a rescan can shorten the library.
                self.at.clamp(&shape);
            }
            if frame.just_pressed & orbistoun_input::Button::South.bit() != 0 {
                self.confirm();
            }
        }

        // A hold in progress needs frames without input, or it never completes.
        if frame.hold_progress > 0.0 && frame.hold_progress < 1.0 {
            ctx.request_repaint();
        }
    }

    /// Acts on whatever the highlight is sitting on.
    ///
    /// The same actions the pointer produces, as one match over the highlight, so a
    /// controller and a pointer reach the same things.
    fn confirm(&mut self) {
        use crate::shell::Category;

        let action = match Category::ROW[self.at.category] {
            Category::User => Some(crate::shell::Action::Settings),
            Category::Titles => (self.at.item < self.rows.len())
                .then_some(crate::shell::Action::Launch(self.at.item)),
            Category::Settings => match self.at.item {
                0 => Some(crate::shell::Action::Settings),
                1 => Some(crate::shell::Action::Rescan),
                _ => Some(crate::shell::Action::ToList),
            },
            Category::Power => {
                let quitting = self.running.is_some() && self.at.item == 0;
                Some(if quitting {
                    crate::shell::Action::Quit
                } else {
                    crate::shell::Action::CloseEmulator
                })
            }
        };
        if let Some(action) = action {
            self.shell_action(action);
        }
    }

    /// Puts a shell request to both copies of the session.
    ///
    /// This copy decides what is drawn and the worker's decides what the guest is told;
    /// one call updates both.
    fn shell_request(&mut self, request: orbistoun_shell::Request) {
        let Ok(taken) = self.session.on(request) else {
            // A request that does not apply from this state is ignored.
            return;
        };
        self.session = taken.state;
        if let Some(in_flight) = &self.running {
            in_flight.shell(request);
        }
    }

    /// Ends the running title, telling it first.
    ///
    /// A `Stopper` terminates the process without notice, so the shell action goes first
    /// over the control channel (D310) and the termination follows. The `Quitting` event
    /// reaches the guest only once its code is measured (D311).
    fn quit_running_title(&mut self) {
        if let Some(in_flight) = &self.running {
            in_flight.shell(orbistoun_shell::Request::Quit);
            in_flight.stop();
        }
    }

    /// Draws the shell.
    fn shell_panel(&mut self, ui: &mut egui::Ui) {
        // Scoped, so every field borrow is released before `shell_action` takes `self`.
        let action = {
            let tiles: Vec<crate::shell::Tile<'_>> = self
                .rows
                .iter()
                .map(|row| crate::shell::Tile {
                    key: &row.key,
                    title: &row.title,
                    icon: row.icon.as_deref(),
                })
                .collect();
            // The scan result, not the rows, so an unreadable folder is not an empty library.
            let library = match &self.titles {
                Ok(_) => Ok(tiles.as_slice()),
                Err(why) => Err(why.as_str()),
            };
            crate::shell::draw(
                ui,
                &self.prefs.shell,
                library,
                &mut self.icons,
                &format!("{} - {}", self.build, self.renderer),
                self.running.as_ref().map(|run| run.module.as_str()),
                &mut self.at,
            )
        };
        if let Some(action) = action {
            self.shell_action(action);
        }
    }

    /// Takes the reply to a capture request, if one arrived this frame.
    ///
    /// The image is cloned out of the event before writing, because the write borrows
    /// `self` mutably while the events are borrowed from the context.
    fn collect_screenshot(&mut self, ctx: &egui::Context) {
        let captured = ctx.input(|i| {
            i.raw.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = captured else {
            return;
        };
        // The title's directory name, which keys every other artefact this project writes.
        let label = self.selected_title().map(|t| t.name.clone());
        self.last_capture = Some(capture::save(
            &self.paths.screenshots_dir(),
            label.as_deref(),
            &image,
            capture::now_ms(),
        ));
    }

    /// The selected title, if there is one.
    fn selected_title(&self) -> Option<&TitleEntry> {
        let titles = self.titles.as_ref().ok()?;
        titles.get(self.selected?)
    }

    /// Re-reads the settings file, then the library folder.
    ///
    /// Discards unsaved edits in the preferences window rather than merging them.
    fn reload_settings(&mut self) {
        self.prefs = Preferences::load(
            &self.paths.config_file(),
            &self.paths.shell_file(),
            self.paths.data_root(),
        );
        self.rescan();
    }

    /// Re-reads the library folder.
    ///
    /// Through `resolve`, so a relative root means the same folder however the window was
    /// started (D038).
    fn rescan(&mut self) {
        self.titles = self
            .service
            .discover_titles(&self.prefs.file.library.resolve(self.paths.data_root()))
            .map_err(|e| e.to_string());
        self.selected = None;
        self.detail = None;
        // Forgotten on rescan, so an icon that changed on disk is picked up.
        self.icons.clear();
        self.rebuild_rows();
    }

    /// Rebuilds the library rows.
    ///
    /// Called when the library changes and when a run finishes, the only things that
    /// change a row.
    fn rebuild_rows(&mut self) {
        let traces_dir = self.paths.traces_dir();
        self.rows = self
            .titles
            .as_ref()
            .map(|titles| {
                titles
                    .iter()
                    .map(|t| Row {
                        key: t.name.clone(),
                        title: t.display_name().to_owned(),
                        // The identifier under the name, as it appears in traces and trace
                        // file names.
                        id: t
                            .metadata
                            .as_ref()
                            .map_or_else(String::new, |m| m.title_id.clone()),
                        icon: t.metadata.as_ref().and_then(|m| m.icon.clone()),
                        requires: t.metadata.as_ref().and_then(|m| m.requires.clone()),
                        last_run: summarise_last_run(&traces_dir, &t.module),
                    })
                    .collect()
            })
            .unwrap_or_default();
    }

    /// Inspects a title, once.
    fn inspect(&mut self, index: usize) {
        self.selected = Some(index);
        let Some(module) = self.selected_title().map(|t| t.module.clone()) else {
            return;
        };
        let inspect = self
            .service
            .inspect_path(&module)
            .map(|info| format!("{info:#?}"))
            .map_err(|e| e.to_string());
        let imports = std::fs::read(&module)
            .ok()
            .and_then(|bytes| self.service.explain_imports(&bytes).ok());
        self.detail = Some(Detail { inspect, imports });
    }

    /// Starts the selected title.
    fn launch(&mut self) {
        let Some(module) = self.selected_title().map(|t| t.module.clone()) else {
            return;
        };
        // Chosen from the library, so nothing is returned to when it ends.
        self.home = None;
        self.start_module(&module);
    }

    /// Starts a run of `module`, replacing the picture of whatever ran before.
    fn start_module(&mut self, module: &std::path::Path) {
        self.finished = None;
        // A new run starts with no picture. The texture is hidden, not freed, because wgpu
        // may still be submitting a frame that uses it; the next frame overwrites it.
        self.live_fresh = false;
        self.perf = None;
        self.running = Some(run::start(
            module,
            self.prefs.file.library.run_limit_seconds,
            self.prefs.file.library.run_call_budget,
            self.paths.traces_dir(),
            run::RunInput {
                play: self.input_playback.clone(),
                capture: self.input_capture.clone(),
            },
        ));
    }

    /// Arms `script` for the next launch, as `--playback <file>` does (D721).
    pub(crate) fn arm_playback(mut self, script: Option<std::path::PathBuf>) -> Self {
        self.input_playback = script;
        self
    }

    /// The toolbar's "capture input" (D721): starts capturing what the title reads from its
    /// pad, now or from the next launch, or stops a capture. The file is
    /// `<logs>/input/<title>-<unix ms>.toml`, a pad script `playback input` can play back.
    fn toggle_input_capture(&mut self) {
        if let Some(path) = self.input_capture.take() {
            if let Some(in_flight) = &self.running {
                in_flight.capture_input(None);
            }
            self.input_captured = Some(path);
            return;
        }
        let module = self
            .running
            .as_ref()
            .map(|in_flight| std::path::PathBuf::from(&in_flight.module))
            .or_else(|| self.selected_title().map(|t| t.module.clone()));
        let name = module
            .as_deref()
            .map_or_else(|| "run".to_owned(), capture_name);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        let path = self
            .paths
            .logs_dir()
            .join("input")
            .join(format!("{name}-{stamp}.toml"));
        if let Some(in_flight) = &self.running {
            in_flight.capture_input(Some(path.clone()));
        }
        self.input_capture = Some(path);
    }

    /// The toolbar's "playback input" (D721): plays `script` on the running title from now,
    /// or from the next launch, or stops playing with `None`.
    fn choose_input_playback(&mut self, script: Option<std::path::PathBuf>) {
        if let Some(in_flight) = &self.running {
            in_flight.play_input(script.clone());
        }
        self.input_playback = script;
    }

    /// The toolbar's input section (D721): "capture input", which toggles, and "playback
    /// input", a menu of captured files. Each acts on the running title from now, or with
    /// nothing running arms the next launch from its start.
    fn input_controls(&mut self, ui: &mut egui::Ui) {
        let running = self.running.is_some();
        let capture_hover = match (&self.input_capture, running) {
            (Some(path), true) => format!("capturing to {}", path.display()),
            (Some(path), false) => format!(
                "armed: the next launch captures from its start to {}",
                path.display()
            ),
            (None, true) => "capture what the title reads from its pad, from now".to_owned(),
            (None, false) => {
                "arm a capture: the next launch captures what the title reads from its pad"
                    .to_owned()
            }
        };
        let capture_label = if self.input_capture.is_some() {
            "⏹ stop capture"
        } else {
            "🎮 capture input"
        };
        if ui
            .button(capture_label)
            .on_hover_text(capture_hover)
            .clicked()
        {
            self.toggle_input_capture();
        }

        let mut chosen: Option<Option<std::path::PathBuf>> = None;
        ui.menu_button("▶ playback input", |ui| {
            if self.input_playback.is_some() && ui.button("⏹ stop playback").clicked() {
                chosen = Some(None);
                ui.close_menu();
            }
            let captures = self.input_captures();
            if captures.is_empty() {
                ui.weak("no captured input yet");
            }
            for path in captures.into_iter().take(PLAYBACK_CHOICES) {
                let name = path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                if ui
                    .button(name)
                    .on_hover_text(path.display().to_string())
                    .clicked()
                {
                    chosen = Some(Some(path));
                    ui.close_menu();
                }
            }
        })
        .response
        .on_hover_text(if running {
            "play captured input on the running title, from now"
        } else {
            "choose captured input for the next launch to play from its start"
        });
        if let Some(choice) = chosen {
            self.choose_input_playback(choice);
        }

        // Short and fixed in shape; the file name is on hover.
        if let Some(path) = &self.input_playback {
            ui.small(if running { "playing" } else { "playback armed" })
                .on_hover_text(path.display().to_string());
        } else if let (None, Some(path)) = (&self.input_capture, &self.input_captured) {
            ui.small("captured")
                .on_hover_text(path.display().to_string());
        }
    }

    /// A run's capture and playback end with it (D721): the capture's file is kept to say
    /// where it went, and nothing replays into the next launch unasked.
    fn end_run_input(&mut self) {
        if let Some(path) = self.input_capture.take() {
            self.input_captured = Some(path);
        }
        self.input_playback = None;
    }

    /// Captured input files, newest first, as "playback input" offers them.
    fn input_captures(&self) -> Vec<std::path::PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.paths.logs_dir().join("input")) else {
            return Vec::new();
        };
        let mut found: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|e| e == "toml"))
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .collect();
        found.sort_by_key(|&(modified, _)| std::cmp::Reverse(modified));
        found.into_iter().map(|(_, path)| path).collect()
    }

    /// Takes what the running title has presented and asked for, and its result once it ends.
    fn collect_run(&mut self, ctx: &egui::Context) {
        if let Some(in_flight) = &self.running {
            // The newest presented frame, onto the one texture.
            if let Some(report) = in_flight.latest_perf() {
                self.perf = Some(report);
            }
            if let Some(frame) = in_flight.latest_frame() {
                self.live_fresh = true;
                match &mut self.live {
                    Some(texture) => texture.set(frame, egui::TextureOptions::LINEAR),
                    None => {
                        self.live = Some(ctx.load_texture(
                            "live-frame",
                            frame,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                }
            }
        }
        self.follow_launch_request();
        if let Some(in_flight) = &self.running {
            match in_flight.poll() {
                Ok(Some(finished)) => {
                    self.finished = Some(finished);
                    self.running = None;
                    self.end_run_input();
                    // A title started from a launcher goes back to it when it ends.
                    if let Some(home) = self.home.take() {
                        self.start_module(&home);
                    }
                    // The run wrote a trace, so the last-run column is stale.
                    self.rebuild_rows();
                }
                Ok(None) => {
                    // Immediate mode redraws only on input, so poll the run on a timer.
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(()) => {
                    self.finished = Some(run::Finished::stopped());
                    self.running = None;
                    self.end_run_input();
                }
            }
        }
    }

    /// Carries out a running guest's request to start another title: the asking run ends,
    /// the title starts from the library, and the asker is remembered as home. A title id
    /// the library does not hold is refused visibly and the run continues.
    fn follow_launch_request(&mut self) {
        let Some(title_id) = self
            .running
            .as_ref()
            .and_then(run::InFlight::requested_launch)
        else {
            return;
        };
        let target = self.titles.as_ref().ok().and_then(|titles| {
            titles
                .iter()
                .find(|t| {
                    t.name == title_id
                        || t.metadata.as_ref().is_some_and(|m| m.title_id == title_id)
                })
                .map(|t| t.module.clone())
        });
        let Some(target) = target else {
            eprintln!("[launch] {title_id} is not in the library; the request is ignored");
            return;
        };
        if let Some(in_flight) = self.running.take() {
            // A home that is itself a launched title's is kept: the chain returns to the first.
            if self.home.is_none() {
                self.home = Some(std::path::PathBuf::from(&in_flight.module));
            }
            in_flight.stop();
        }
        self.start_module(&target);
    }

    /// Opens the per-title override file for editing.
    fn open_title_config(&mut self) {
        let Some(title) = self.selected_title() else {
            return;
        };
        let name = title.name.clone();
        let path = self.overrides_path(&name);
        // A missing file opens as a commented template rather than an error.
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            format!(
                concat!(
                    "# Per-title overrides for {}.\n",
                    "#\n",
                    "# Settings are merged per key over the shipped defaults, never wholesale,\n",
                    "# so anything left out keeps the value it already had.\n",
                    "#\n",
                    "# A compatibility entry names the *behaviour*, never the title, and carries\n",
                    "# a mandatory reason - that is what lets a second title needing the same\n",
                    "# thing add a line rather than a code path.\n"
                ),
                name
            )
        });
        self.title_config = Some(TitleConfig {
            title: name,
            text,
            status: None,
        });
    }

    /// Where a title's user-layer overrides live.
    fn overrides_path(&self, title: &str) -> std::path::PathBuf {
        self.paths.overrides_dir().join(format!("{title}.toml"))
    }

    /// Draws the menu strip.
    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("file", |ui| {
                    if ui.button("rescan library").clicked() {
                        self.rescan();
                        ui.close_menu();
                    }
                    // Switches views without a restart or a settings change.
                    if ui
                        .button("shell")
                        .on_hover_text("the library as a console presents it")
                        .clicked()
                    {
                        self.view = View::Shell;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("probe", |ui| {
                    // Named for what is on the other end, a probe; what it runs on is not
                    // knowable from here.
                    if ui.button("connect...").clicked() {
                        self.probe.open = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("settings", |ui| {
                    if ui.button("preferences...").clicked() {
                        self.prefs.open = true;
                        ui.close_menu();
                    }
                    let has_title = self.selected_title().is_some();
                    if ui
                        .add_enabled(has_title, egui::Button::new("title overrides..."))
                        .on_disabled_hover_text("select a title first")
                        .clicked()
                    {
                        self.open_title_config();
                        ui.close_menu();
                    }
                    ui.separator();
                    // Picks up a hand edit of config.toml; "rescan library" reuses the
                    // settings already in memory.
                    if ui
                        .button("reload settings file")
                        .on_hover_text("re-reads config.toml and scans again")
                        .clicked()
                    {
                        self.reload_settings();
                        ui.close_menu();
                    }
                });
                ui.menu_button("help", |ui| {
                    if ui.button("documentation...").clicked() {
                        self.docs.open();
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label("orbistoun");
                    ui.small("guest code runs natively; this window is a shim over the crates");
                });
            });
        });
    }

    /// Draws the toolbar.
    ///
    /// Every control is disabled rather than hidden when it does not apply, and says why
    /// on hover.
    fn toolbar(&mut self, ctx: &egui::Context) {
        // Acted on after the strip is drawn, because rescanning replaces the list being
        // drawn.
        let mut rescan = false;
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let selected = self.selected_title().is_some();
                let busy = self.running.is_some();

                if ui
                    .add_enabled(selected && !busy, egui::Button::new("▶ start"))
                    .on_disabled_hover_text(if busy {
                        "a guest is already running"
                    } else {
                        "select a title first"
                    })
                    .clicked()
                {
                    self.deferred.launch = true;
                }

                // Stop is enabled only where it can be honoured, disabled with the reason
                // elsewhere.
                let can_stop = busy && orbistoun_worker::Stopper::is_supported();
                if ui
                    .add_enabled(can_stop, egui::Button::new("■ stop"))
                    .on_disabled_hover_text(if busy {
                        "stopping is not supported on this platform"
                    } else {
                        "nothing is running"
                    })
                    .clicked()
                {
                    if let Some(in_flight) = &self.running {
                        in_flight.stop();
                    }
                }

                ui.separator();

                if ui
                    .add_enabled(selected, egui::Button::new("⚙ configure"))
                    .on_disabled_hover_text("select a title first")
                    .clicked()
                {
                    self.open_title_config();
                }

                ui.separator();

                // Also on the toolbar, because the library is scanned only at startup.
                if ui
                    .button("⟳ refresh")
                    .on_hover_text("rescan the library folder")
                    .clicked()
                {
                    rescan = true;
                }

                ui.separator();

                // A screenshot of this window: the running title's picture with the
                // diagnostic panels around it.
                if ui
                    .button("📷 screenshot")
                    .on_hover_text("write this window to a PNG in the screenshots folder")
                    .clicked()
                {
                    self.deferred.screenshot = true;
                }

                // Recording is not built: shown disabled with the reason, per this toolbar's
                // rule.
                ui.add_enabled(false, egui::Button::new("⏺ record"))
                    .on_disabled_hover_text(concat!(
                        "recording needs a frame source and an encoder, and neither exists ",
                        "yet"
                    ));

                // Short and fixed in shape so the controls beside it do not move; the path
                // is on hover.
                match &self.last_capture {
                    Some(Ok(path)) => {
                        ui.small("saved").on_hover_text(path.display().to_string());
                    }
                    Some(Err(why)) => {
                        ui.colored_label(egui::Color32::from_rgb(0xd0, 0x60, 0x60), "✖ screenshot")
                            .on_hover_text(why.as_str());
                    }
                    None => {}
                }

                ui.separator();
                self.input_controls(ui);

                ui.separator();
                ui.label("limit");
                ui.add(
                    egui::DragValue::new(&mut self.prefs.file.library.run_limit_seconds)
                        .range(0..=600),
                );
                ui.label("s");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(in_flight) = &self.running {
                        ui.label(format!("running {}", in_flight.module));
                        ui.spinner();
                    } else {
                        // How many titles the last scan found, so an empty library visibly
                        // comes from a scan that ran.
                        match &self.titles {
                            Ok(titles) => ui.weak(format!("{} titles", titles.len())),
                            Err(_) => ui.weak("library unavailable"),
                        };
                    }
                });
            });
        });
        if rescan {
            self.rescan();
        }
    }

    /// Says which settings file produced the folder above it.
    ///
    /// A missing `config.toml` is the ordinary first launch and not an error, so without
    /// this line a file that was read and a file that was never found look identical.
    fn settings_provenance(&self, ui: &mut egui::Ui) {
        let config = self.paths.config_file();
        ui.add_space(4.0);
        ui.small(format!("settings {}", config.display()));
        if !config.is_file() {
            ui.small("no such file - every setting above is a default");
        }
    }

    /// Draws the library list.
    fn library_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        // Above the scan result, because it explains it: a settings file that failed to
        // parse falls back to defaults, so the folder below is not the configured one.
        if let Some(error) = &self.prefs.load_error {
            ui.colored_label(egui::Color32::from_rgb(230, 180, 80), "settings not loaded");
            ui.small(error);
            ui.add_space(4.0);
        }
        match &self.titles {
            Err(error) => {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
                self.settings_provenance(ui);
                ui.small("set the library folder in settings - preferences");
            }
            Ok(titles) if titles.is_empty() => {
                ui.label("no titles here");
                ui.small("a title is a directory containing an entry module");
                // Which folder was empty.
                ui.add_space(2.0);
                ui.weak(
                    self.prefs
                        .file
                        .library
                        .resolve(self.paths.data_root())
                        .display()
                        .to_string(),
                );
                self.settings_provenance(ui);
                ui.small("settings - preferences - general to point somewhere else");
            }
            Ok(_) => {
                // Drawn from `self.rows` (see `rebuild_rows`), taken out so the icon cache
                // can be borrowed mutably while iterating.
                let rows = std::mem::take(&mut self.rows);
                let mut clicked = None;
                let mut launched = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, row) in rows.iter().enumerate() {
                        let selected = self.selected == Some(index);
                        let texture = self
                            .icons
                            .get(ui.ctx(), &row.key, row.icon.as_deref())
                            .map(egui::TextureHandle::id);
                        let response = draw_row(ui, row, texture, selected);
                        if response.clicked() {
                            clicked = Some(index);
                        }
                        // Double-click selects and launches, so the detail panel shows the
                        // title that started.
                        if response.double_clicked() {
                            launched = Some(index);
                        }
                    }
                });
                self.rows = rows;
                if let Some(index) = clicked.or(launched) {
                    self.inspect(index);
                }
                if launched.is_some() && self.running.is_none() {
                    self.deferred.launch = true;
                }
            }
        }
        self.build_stamp(ui);
    }

    /// Which build this is, at the bottom of the sidebar.
    ///
    /// Ties a screenshot, bug report or run result to a build (D222). Without a commit it
    /// shows when the binary was compiled. Pinned to the bottom so it does not scroll away.
    fn build_stamp(&self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::bottom("build")
            .show_separator_line(false)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.small(&self.build);
                    // The short form fits; the full detail is on hover.
                    ui.label("")
                        .on_hover_text(orbistoun_env::build::commit().map_or_else(
                            || format!("no commit - {}", self.build),
                            |c| format!("built from commit {c}"),
                        ));
                });
            });
    }

    /// Draws the per-title view.
    fn detail_panel(&mut self, ui: &mut egui::Ui) {
        let Some(title) = self.selected_title() else {
            ui.centered_and_justified(|ui| ui.label("select a title"));
            return;
        };
        let key = title.name.clone();
        let heading = title.display_name().to_owned();
        let subtitle = title.metadata.as_ref().map(|m| {
            let version = m.version.as_deref().unwrap_or("unknown version");
            let requires = m
                .requires
                .as_deref()
                .map_or_else(String::new, |r| format!("  |  requires {r}"));
            let built = m
                .built_with
                .as_deref()
                .map_or_else(String::new, |b| format!("  |  built with {b}"));
            format!("{}  |  {version}{requires}{built}  |  {key}", m.title_id)
        });
        let icon = title.metadata.as_ref().and_then(|m| m.icon.clone());

        ui.horizontal(|ui| {
            if let Some(texture) = self.icons.get(ui.ctx(), &key, icon.as_deref()) {
                let id = texture.id();
                let size = egui::vec2(icons::HEADER_ICON, icons::HEADER_ICON);
                ui.add(egui::Image::from_texture((id, size)).rounding(8.0));
            }
            ui.vertical(|ui| {
                ui.heading(&heading);
                match &subtitle {
                    Some(text) => ui.small(text),
                    // No metadata is ordinary for homebrew; stated rather than left blank.
                    None => ui.small("no published metadata - named by its folder"),
                };
            });
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if let Some(finished) = &self.finished {
                Self::run_result(ui, finished);
                ui.separator();
            }
            if let Some(detail) = &self.detail {
                Self::static_detail(ui, detail);
            }
        });
    }

    /// Draws what a run produced.
    fn run_result(ui: &mut egui::Ui, finished: &run::Finished) {
        if let Some(error) = &finished.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
            return;
        }

        if let Some(progress) = &finished.progress {
            ui.heading("progress");
            // The same words the CLI prints, from the same place (D034).
            let colour = if progress.verdict.is_progress() {
                egui::Color32::LIGHT_GREEN
            } else {
                ui.visuals().text_color()
            };
            ui.colored_label(
                colour,
                format!(
                    "{} {}",
                    progress.verdict.label(),
                    progress.verdict.summary()
                ),
            );
            ui.label(format!(
                "imports {:+}, calls {:+}",
                progress.distinct_delta, progress.calls_delta
            ));
            match &progress.previous_fault {
                Some(previous) => ui.label(format!("fault {} (was {previous})", progress.fault)),
                None => ui.label(format!("fault {}", progress.fault)),
            };
        }

        if let Some(trace) = &finished.trace {
            // Stack conformance, shown even when clean, so a missing line means a missing
            // check (D159).
            ui.separator();
            if trace.abi.misaligned_calls == 0 {
                ui.label(format!(
                    "{} calls, all on a conforming stack",
                    trace.total_calls
                ));
            } else {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!(
                        "{} of {} calls arrived on a misaligned stack",
                        trace.abi.misaligned_calls, trace.total_calls
                    ),
                );
            }

            if trace.fault.is_some() && !trace.tail.is_empty() {
                ui.separator();
                ui.heading("last calls before the fault");
                for call in &trace.tail {
                    ui.monospace(format!("{}({:#x})", call.label, call.args[0]));
                }
            }

            ui.separator();
            ui.heading("what it asked for");
            egui::Grid::new("imports").striped(true).show(ui, |ui| {
                for called in &trace.calls {
                    ui.monospace(called.calls.to_string());
                    ui.monospace(&called.label);
                    ui.end_row();
                }
            });
        }

        if !finished.events.is_empty() {
            ui.separator();
            ui.heading("events");
            for event in &finished.events {
                ui.monospace(event);
            }
        }
    }

    /// Draws what is known without running anything.
    fn static_detail(ui: &mut egui::Ui, detail: &Detail) {
        if let Some((total, named)) = detail.imports {
            ui.heading("imports");
            ui.label(format!("{named} of {total} named"));
        }
        ui.heading("container");
        match &detail.inspect {
            Ok(text) => ui.monospace(text),
            Err(error) => ui.colored_label(egui::Color32::LIGHT_RED, error),
        };
    }

    /// Draws the preferences window.
    fn preferences_window(&mut self, ctx: &egui::Context) {
        let mut open = self.prefs.open;
        let mut save = false;
        let mut rescan = false;
        egui::Window::new("preferences")
            .open(&mut open)
            .default_size([620.0, 460.0])
            .resizable(true)
            .show(ctx, |ui| {
                // The actions are laid out before the panes and pinned to the bottom: the
                // vertical `separator` between pane list and pane grows to the available
                // height, which would push them off a self-sizing window.
                egui::TopBottomPanel::bottom("preferences-actions").show_inside(ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("save").clicked() {
                            save = true;
                        }
                        if ui.button("rescan library").clicked() {
                            rescan = true;
                        }
                        // Stated, because the settings do not take effect immediately.
                        ui.small("settings apply to the next run");
                    });
                    if let Some(status) = &self.prefs.status {
                        match status {
                            Ok(message) => ui.small(message),
                            Err(error) => ui.colored_label(egui::Color32::LIGHT_RED, error),
                        };
                    }
                    ui.add_space(2.0);
                });
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                for pane in prefs::Pane::ALL {
                                    ui.selectable_value(&mut self.prefs.pane, pane, pane.label());
                                }
                            });
                            ui.separator();
                            ui.vertical(|ui| {
                                prefs::pane_contents(ui, &mut self.prefs, &self.last_pads);
                            });
                        });
                    });
                });
            });
        self.prefs.open = open;
        if save {
            let path = self.paths.config_file();
            self.prefs.save(&path, &self.paths.shell_file());
        }
        if rescan {
            self.rescan();
        }
    }

    /// Draws the per-title override window.
    fn title_config_window(&mut self, ctx: &egui::Context) {
        let Some(config) = &mut self.title_config else {
            return;
        };
        let mut open = true;
        let mut save = false;
        egui::Window::new(format!("overrides - {}", config.title))
            .open(&mut open)
            .default_width(620.0)
            .show(ctx, |ui| {
                ui.small(concat!(
                    "Merged per key over the shipped defaults, never wholesale - anything ",
                    "left out keeps the value it already had."
                ));
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut config.text)
                                .code_editor()
                                .desired_width(f32::INFINITY),
                        );
                    });
                ui.separator();
                if ui.button("save").clicked() {
                    save = true;
                }
                if let Some(status) = &config.status {
                    match status {
                        Ok(message) => ui.small(message),
                        Err(error) => ui.colored_label(egui::Color32::LIGHT_RED, error),
                    };
                }
            });

        if save {
            let (title, text) = (config.title.clone(), config.text.clone());
            let path = self.overrides_path(&title);
            let result = std::fs::write(&path, text)
                .map(|()| format!("saved to {}", path.display()))
                .map_err(|e| format!("could not save: {e}"));
            if let Some(config) = &mut self.title_config {
                config.status = Some(result);
            }
        }
        if !open {
            self.title_config = None;
        }
    }
}

/// One library row, copied out of the title list so drawing can borrow the icon cache.
struct Row {
    /// Directory name - the cache key, and the fallback label.
    key: String,
    /// What to show.
    title: String,
    /// The identifier, or empty when the title publishes none.
    id: String,
    /// Where the icon is, if there is one.
    icon: Option<std::path::PathBuf>,
    /// The system version this title requires, when it says.
    ///
    /// It indicates the interface generation the title was built against.
    requires: Option<String>,
    /// How the last run of this title went.
    ///
    /// Shown per row, so the library shows where each title stops.
    last_run: Option<String>,
}

/// One line summarising a title's last run.
///
/// `None` when it has never been run, which is distinct from a run that reached nothing.
fn summarise_last_run(traces_dir: &std::path::Path, module: &std::path::Path) -> Option<String> {
    let trace = orbistoun_report::trace::load_previous(traces_dir, module)?;
    let ended = trace.fault.as_ref().map_or_else(
        || "ran to the limit".to_owned(),
        |fault| match (&fault.region, fault.offset) {
            (Some(region), Some(offset)) => format!("{region}+{offset:#x}"),
            _ => format!("{:#x}", fault.instruction_pointer),
        },
    );
    Some(format!("{} imports, {ended}", trace.distinct))
}

/// Draws one library row and answers whether it was interacted with.
///
/// The whole row is one clickable region, icon included.
fn draw_row(
    ui: &mut egui::Ui,
    row: &Row,
    texture: Option<egui::TextureId>,
    selected: bool,
) -> egui::Response {
    let fill = if selected {
        ui.visuals().selection.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.scope(|ui| {
        egui::Frame::none()
            .fill(fill)
            .rounding(4.0)
            .inner_margin(egui::Margin::symmetric(4.0, 3.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let size = egui::vec2(icons::LIST_ICON, icons::LIST_ICON);
                    match texture {
                        Some(id) => {
                            ui.add(egui::Image::from_texture((id, size)).rounding(4.0));
                        }
                        // A blank of the same size, so rows line up.
                        None => ui.add_space(icons::LIST_ICON),
                    }
                    ui.vertical(|ui| {
                        ui.label(&row.title);
                        // Identifier and required version share one line to keep rows short.
                        let second = match (&row.id, &row.requires) {
                            (id, Some(requires)) if id.is_empty() => format!("fw {requires}"),
                            (id, Some(requires)) => format!("{id}  fw {requires}"),
                            (id, None) => id.clone(),
                        };
                        if !second.is_empty() {
                            ui.small(second);
                        }
                        match &row.last_run {
                            Some(summary) => {
                                ui.small(egui::RichText::new(summary).weak());
                            }
                            // Stated rather than left blank: never run is not ran badly.
                            None => {
                                ui.small(egui::RichText::new("never run").weak());
                            }
                        }
                    });
                    // Fill the row so the click region is the full width.
                    ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                });
            });
    })
    .response
    .interact(egui::Sense::click())
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Before drawing, so the frame that removes the spinner shows the result.
        self.collect_run(ctx);

        // Before drawing, so a capture reply that arrived this frame is reported this frame.
        self.collect_screenshot(ctx);

        // Before drawing, so a press acts on its own frame and the session state below is
        // current.
        self.read_input(ctx);

        // Once a running title has presented a frame, the window shows that frame in place
        // of the library.
        let playing = self
            .running
            .as_ref()
            .and(self.live.as_ref())
            .filter(|_| self.live_fresh)
            .map(|texture| (texture.id(), texture.size_vec2()));
        if ctx.input(|i| i.key_pressed(crate::perf_overlay::TOGGLE)) {
            self.show_perf = !self.show_perf;
        }
        let overlay = self.perf.clone().filter(|_| self.show_perf);
        let overlay = overlay.as_ref();
        match (self.view, playing) {
            (View::Shell, Some(frame)) => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(egui::Color32::BLACK))
                    .show(ctx, |ui| live_frame(ui, frame, overlay));
            }
            // The shell gets the whole window, with no toolbar.
            (View::Shell, None) => {
                egui::CentralPanel::default().show(ctx, |ui| self.shell_panel(ui));
            }
            (View::List, playing) => {
                self.menu_bar(ctx);
                self.toolbar(ctx);
                egui::SidePanel::left("library")
                    .default_width(260.0)
                    .show(ctx, |ui| self.library_panel(ui));
                match playing {
                    Some(frame) => {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
                            .show(ctx, |ui| live_frame(ui, frame, overlay));
                    }
                    None => match &self.running {
                        // A run that has not presented yet is shown as starting, on black.
                        Some(in_flight) => {
                            let name = std::path::Path::new(&in_flight.module)
                                .parent()
                                .and_then(|d| d.file_name())
                                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                            egui::CentralPanel::default()
                                .frame(egui::Frame::none().fill(egui::Color32::BLACK))
                                .show(ctx, |ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.label(format!(
                                            "starting {name} - waiting for its first frame"
                                        ));
                                    });
                                });
                        }
                        None => {
                            egui::CentralPanel::default().show(ctx, |ui| self.detail_panel(ui));
                        }
                    },
                }
            }
        }
        // Over everything, in both views: the shell button works in either.
        let menu = if self.power_menu {
            Some(crate::shell::Menu::Power)
        } else if self.session == orbistoun_shell::Lifecycle::Overlaid {
            Some(crate::shell::Menu::Overlay)
        } else {
            None
        };
        if let Some(which) = menu {
            let running = self.running.as_ref().map(|run| run.module.clone());
            let mut action = None;
            egui::Area::new(egui::Id::new("shell-menu"))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    action = crate::shell::menu(ui, which, running.as_deref());
                });
            if let Some(action) = action {
                self.shell_action(action);
            }
        }

        self.preferences_window(ctx);
        self.title_config_window(ctx);
        // Outside either view, so the menu item works from both.
        self.docs.show(ctx, DOCS);
        // No repaint timer: the probe worker requests a repaint when it has something.
        self.probe.show(ctx);

        // Deferred actions run after drawing, once each, when no borrow of the frame is held.
        if std::mem::take(&mut self.deferred.close) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if std::mem::take(&mut self.deferred.launch) && self.running.is_none() {
            self.launch();
        }
        // Requested after the frame is composed, so the capture shows this frame.
        if std::mem::take(&mut self.deferred.screenshot) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        }
    }
}

/// The pages this build ships, and their order in the reader.
///
/// `include_str!` puts them in the binary, so they match the build. Only the manual is
/// listed; the development records stay in the repository.
const DOCS: &[oops_docs::Doc] = &[
    oops_docs::Doc::new(
        "user-guide",
        "User Guide",
        "Requirements, compatibility status, and local-first storage",
        include_str!("../../../docs/features/user-guide.md"),
    ),
    oops_docs::Doc::new(
        "library",
        "The library",
        "Finding titles, and what the detail panel is telling you",
        include_str!("../../../docs/features/library.md"),
    ),
    oops_docs::Doc::new(
        "running",
        "Running a title",
        "The report, honest failure, and what a verdict is not",
        include_str!("../../../docs/features/running.md"),
    ),
    oops_docs::Doc::new(
        "inspector",
        "Execution Inspector",
        "Live call trace inspection and HLE resolution monitoring",
        include_str!("../../../docs/features/inspector.md"),
    ),
    oops_docs::Doc::new(
        "memory",
        "Memory & Registers",
        "Host register context and guest virtual memory layout",
        include_str!("../../../docs/features/memory.md"),
    ),
    oops_docs::Doc::new(
        "graphics",
        "Graphics Settings",
        "PM4 command-stream decode and shader translation to SPIR-V; presentation not implemented yet",
        include_str!("../../../docs/features/graphics.md"),
    ),
    oops_docs::Doc::new(
        "controllers",
        "Controller Setup",
        "DualSense, XInput, and keyboard button mapping",
        include_str!("../../../docs/features/controllers.md"),
    ),
    oops_docs::Doc::new(
        "naming",
        "Names and hashes",
        "Why imports show as hex, and how that is undone",
        include_str!("../../../docs/features/naming.md"),
    ),
    oops_docs::Doc::new(
        "paths",
        "Where it writes",
        "The data root, portable mode, and what is under it",
        include_str!("../../../docs/features/paths.md"),
    ),
];

/// Draws a running title's presented frame, as large as fits with its aspect kept, centred
/// on black.
fn live_frame(
    ui: &mut egui::Ui,
    (texture, size): (egui::TextureId, egui::Vec2),
    overlay: Option<&orbistoun_proto::PerfReport>,
) {
    let room = ui.available_size();
    let scale = (room.x / size.x).min(room.y / size.y).max(0.0);
    let shown = ui
        .centered_and_justified(|ui| ui.add(egui::Image::new((texture, size * scale))))
        .inner;
    // Where the time goes, over the picture it went into.
    if let Some(report) = overlay {
        crate::perf_overlay::draw(ui, shown.rect, report);
    }
}

/// How many captured files "playback input" lists, newest first.
const PLAYBACK_CHOICES: usize = 20;

/// What an input capture of `module`'s run is named after (D721): the title ID among its
/// path's folders (four capitals and five digits, as `NVRB00001`), or else the module's name.
fn capture_name(module: &std::path::Path) -> String {
    let is_title_id = |name: &str| {
        name.len() == 9
            && name[..4].bytes().all(|b| b.is_ascii_uppercase())
            && name[4..].bytes().all(|b| b.is_ascii_digit())
    };
    module
        .ancestors()
        .filter_map(|folder| folder.file_name()?.to_str())
        .find(|name| is_title_id(name))
        .or_else(|| module.file_stem()?.to_str())
        .unwrap_or("run")
        .to_owned()
}

#[cfg(test)]
mod docs_tests {
    /// Every shipped page is non-empty, has a heading and a unique slug.
    #[test]
    fn the_registry_is_sound() {
        assert_eq!(oops_docs::check(super::DOCS), Vec::<String>::new());
    }
}
