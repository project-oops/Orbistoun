//! The window: a menu strip, a toolbar, a library, and a title.

use orbistoun_service::{Service, ServiceConfig, TitleEntry};
use orbistoun_shell::{Start, View};

use crate::capture;
use crate::icons::{self, Icons};
use crate::prefs::{self, Preferences};
use crate::run;

/// What is known about the selected title without running it.
///
/// Held rather than recomputed each frame: parsing a container is real work and an
/// immediate-mode redraw happens whenever the pointer moves.
struct Detail {
    /// Container summary, or why it could not be read.
    inspect: Result<String, String>,
    /// How many imports it needs, and how many are named.
    imports: Option<(usize, usize)>,
}

/// Per-title settings, while the window for them is open.
struct TitleConfig {
    /// Which title these belong to - the window can outlive a selection change.
    title: String,
    /// The user layer as editable text.
    ///
    /// **Text rather than a form**, and that is a real choice: the override format carries
    /// compatibility entries with mandatory reasons, and a form would have to either drop
    /// the reason or invent a control for it. Editing the layer directly keeps the
    /// requirement visible until there is a design that respects it (D162).
    text: String,
    /// What happened to the last save.
    status: Option<Result<String, String>>,
}

/// What was asked for while drawing, to be acted on after it.
///
/// **One value rather than a flag each**, because they are one idea. The list, the toolbar,
/// a shell menu and a double-click can all ask for something while holding a borrow of the
/// very state that would have to change, so every one of them is deferred to the end of the
/// frame. Three separate bools said that three times, and the lint that counted them was
/// the more useful complaint.
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
    /// The documentation reader. Holds which page is open and the parsed form of the ones
    /// already looked at, so a megabyte of markdown is not re-parsed sixty times a second.
    docs: oops_docs::DocsWindow,
    service: Service,
    paths: orbistoun_paths::Paths,
    /// Titles found, or why the scan failed. **Not an empty list on failure** - "you own
    /// no titles" and "that folder does not exist" are different answers.
    titles: Result<Vec<TitleEntry>, String>,
    /// What the library list draws, built when the library or a run changes.
    ///
    /// **Not rebuilt per frame, and the first version was.** Each row carries a last-run
    /// summary read from a trace file on disk, so drawing them directly meant a file read
    /// and a JSON parse per title per repaint - and immediate mode repaints whenever the
    /// pointer moves. The icon cache two files away carries a comment warning about
    /// exactly this, which did not stop me writing it (D164).
    rows: Vec<Row>,
    /// The probe window.
    ///
    /// Kept on the application rather than opened per use so a session survives the window
    /// being closed and reopened - a connection is a thing somebody set up, not a dialog's
    /// local state.
    probe: crate::probe::Panel,
    selected: Option<usize>,
    detail: Option<Detail>,
    prefs: Preferences,
    title_config: Option<TitleConfig>,
    icons: Icons,
    running: Option<run::InFlight>,
    finished: Option<run::Finished>,
    /// What was asked for while drawing. See [`Deferred`].
    ///
    /// A window's pixels in particular are not available to the code drawing it: asking is
    /// a viewport command and the answer arrives as an input event on a later frame, so the
    /// request and the reply are necessarily two halves (see [`crate::capture`]).
    deferred: Deferred,
    /// Which build this is, computed once - it cannot change while the window is open.
    build: String,
    /// Where the last capture went, or why it did not.
    ///
    /// Shown in the toolbar rather than logged. A file written somewhere the user cannot
    /// see is the same to them as no file, and a failure that reaches only a log is worse -
    /// the button looked like it worked.
    last_capture: Option<capture::Outcome>,
    /// Which view is showing.
    ///
    /// **One library, two presentations.** The shell and the list draw the same scan and
    /// the same selection; nothing is duplicated between them, so switching cannot show two
    /// different answers to "what do I own".
    view: View,
    /// Which graphics backend this window actually got, and the adapter behind it.
    ///
    /// Reported rather than assumed: `wgpu` chooses from a set holding both Vulkan and
    /// DX12 on this platform, and nothing pins it. Any future work that shares an image
    /// between this process and the guest depends on the answer (D317).
    renderer: String,
    /// A title named on the command line that has not been found yet.
    ///
    /// Kept rather than resolved immediately, because the library scan can fail and a
    /// rescan should get another chance at what somebody asked for on the way in.
    wanted_title: Option<String>,
    /// The last frame of pad state, so the controllers pane can light a button as it is
    /// pressed - which is what turns a mapping from a claim into something checkable.
    last_pads: Vec<orbistoun_input::PadState>,
    /// Host input, and the shell button's press across frames.
    input: crate::input::Reader,
    /// Where the title stands, as **this** process sees it.
    ///
    /// The worker keeps its own and they can disagree; that is by design and a disagreement
    /// is counted rather than designed away (D310). This copy decides what is drawn.
    session: orbistoun_shell::Lifecycle,
    /// Which menu the shell button opened, if any.
    ///
    /// The overlay is not stored here - it is [`orbistoun_shell::Lifecycle::Overlaid`], so
    /// there is one answer to "is the shell over a title" rather than a flag that can
    /// disagree with the session.
    power_menu: bool,
    /// When the last frame was drawn, for the press-versus-hold decision.
    last_frame: std::time::Instant,
    /// Where the highlight is in the shell.
    ///
    /// Held here rather than in the drawing, so a pad and a pointer move **the same**
    /// highlight - two positions would let a controller and a mouse disagree about what is
    /// selected, and confirming would act on whichever the code happened to read.
    at: orbistoun_shell::Cross,
}

impl App {
    /// Builds the window state and scans the default library.
    ///
    /// `start` has already reconciled the command line with the stored setting - see
    /// `orbistoun_shell::startup`, which is where that decision is tested. What is left
    /// here is acting on the answer.
    pub(crate) fn new(start: Start, renderer: String) -> Self {
        let paths = orbistoun_paths::Paths::resolve();
        let _ = paths.ensure_dirs();
        // Read before anything else: it carries the library folder, so the scan below
        // looks in the place this window was last pointed at rather than at a relative
        // path that depends on where the program happened to be started from.
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
            finished: None,
            deferred: Deferred::default(),
            build: orbistoun_env::build::line(),
            docs: oops_docs::DocsWindow::default(),

            last_capture: None,
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
            // Nothing is running yet, which is exactly what `Exited` means.
            session: orbistoun_shell::Lifecycle::Exited,
            power_menu: false,

            last_frame: std::time::Instant::now(),
            at: orbistoun_shell::Cross {
                category: crate::shell::Category::START,
                item: 0,
            },
        };
        app.rescan();
        // After the scan, because that is the first moment there is anything to match
        // against. A name nothing matches leaves the window in the fallback view with a
        // message, rather than silently opening as though no title had been asked for.
        app.take_wanted_title();
        app
    }

    /// Selects and launches a title named on the command line, if one was.
    ///
    /// # Matched on identifier as well as folder name
    ///
    /// A title's folder is whatever somebody called it and its identifier is what the title
    /// calls itself, and a person passing `--title` will reach for either. Matching only one
    /// would refuse half the names visible in the window it is meant to skip.
    ///
    /// Case-insensitive for the same reason: an identifier is upper case on the tile and
    /// nobody types it that way twice.
    ///
    /// A name nothing matches is **left in place rather than dropped**, so a rescan after
    /// fixing the library folder still does what was asked for.
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
                // Asked for rather than started, exactly as the list does it: the borrow
                // held while drawing is the one a run needs to read.
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
    /// # The whole point of the input subsystem, in one function
    ///
    /// A tap moves the session between `Lifecycle::Foreground` and
    /// `Lifecycle::Overlaid`, and a hold opens the power menu. Both go through
    /// `Lifecycle::on`, so the transitions are the ones already tested rather than a second
    /// set written against a menu - and a request that does not apply is refused there
    /// rather than producing a state nobody can reach on purpose.
    ///
    /// The same request is carried to the worker, so the guest is told it lost the machine.
    /// Whether it hears is a separate question with a measured answer of "not yet" (D311).
    fn read_input(&mut self, ctx: &egui::Context) {
        use orbistoun_shell::Request;

        let elapsed = self.last_frame.elapsed();
        self.last_frame = std::time::Instant::now();
        let elapsed_ms = u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX);

        let frame = self.input.read(ctx, &self.prefs.file.pads, elapsed_ms);
        // Kept for the controllers pane, which lights a button as it goes down. That is
        // what makes a mapping checkable where it is edited rather than by launching
        // something and seeing whether it responds.
        self.last_pads = frame.pads;

        // **Where `Focus` stops being a tested function with no effect.** What the title is
        // allowed to see is decided here and nothing downstream can widen it: the shell's
        // own button is always removed, and a title without focus is handed a pad nobody is
        // holding rather than the last one it saw held forever (D345).
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

        // A guest running is what makes the session live. Set here rather than at launch
        // because a run ends on its own - and a session left `Foreground` after the title
        // stopped would offer a resume for something that is gone.
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
                // A tap closes the power menu if it is open, rather than also toggling the
                // overlay underneath it. One press, one effect.
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

        // Navigation, only while the shell is what somebody is looking at. A direction
        // pressed with a title in front belongs to the title, not to a highlight nobody can
        // see - which is the same rule `Focus` states, applied to this side of it.
        if self.view == View::Shell && self.session != orbistoun_shell::Lifecycle::Foreground {
            let shape = crate::shell::shape(self.rows.len(), self.running.is_some());
            if let Some(direction) = crate::input::steering(frame.just_pressed) {
                self.at.steer(direction, &shape);
            } else {
                // Also when nothing moved: a rescan can shorten the library underneath the
                // highlight, and one left past the end draws as nothing being selected.
                self.at.clamp(&shape);
            }
            if frame.just_pressed & orbistoun_input::Button::South.bit() != 0 {
                self.confirm();
            }
        }

        // Somebody is holding the button, so the next frame has to come without waiting for
        // them to move the pointer - otherwise the hold never completes in a window that
        // only redraws on input.
        if frame.hold_progress > 0.0 && frame.hold_progress < 1.0 {
            ctx.request_repaint();
        }
    }

    /// Acts on whatever the highlight is sitting on.
    ///
    /// **The same actions the pointer produces**, reached the other way. Written as one
    /// match over the highlight rather than by asking the drawing what it drew, so a
    /// controller cannot end up able to reach something a mouse cannot or the reverse.
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
    /// **This one decides what is drawn and the worker's decides what the guest is told**,
    /// and they are applied from the same call so they cannot drift apart through somebody
    /// updating one and forgetting the other.
    fn shell_request(&mut self, request: orbistoun_shell::Request) {
        let Ok(taken) = self.session.on(request) else {
            // Refused here means the button did something that does not apply from where
            // the session stands - a menu offering an action it should not have. Not worth
            // interrupting anybody over, and not worth acting on either.
            return;
        };
        self.session = taken.state;
        if let Some(in_flight) = &self.running {
            in_flight.shell(request);
        }
    }

    /// Ends the running title, telling it first.
    ///
    /// # Two acts, in this order, and the order is the whole point
    ///
    /// A `Stopper` is `TerminateProcess`. On its own that is pulling the power out - the
    /// guest gets no notice, runs no shutdown path, and any question of it saving anything
    /// never arises. So the shell action goes first, over the control channel that exists
    /// precisely because the run thread is blocked (D310), and the termination follows.
    ///
    /// **Whether the title actually hears is a separate question, and today the answer is
    /// no**: no code has been measured for `Quitting`, so it is withheld and counted rather
    /// than invented (D311). The order still matters, because the day a code is measured
    /// this becomes correct without anything here changing.
    fn quit_running_title(&mut self) {
        if let Some(in_flight) = &self.running {
            in_flight.shell(orbistoun_shell::Request::Quit);
            in_flight.stop();
        }
    }

    /// Draws the shell.
    fn shell_panel(&mut self, ui: &mut egui::Ui) {
        // Scoped, so every borrow of a field is released before anything acts on the
        // answer - `shell_action` needs the whole of `self` and the tiles hold `rows`.
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
            // The scan result, not the rows: an empty wall and an unreadable folder are
            // different answers, and the shell has to tell them apart (D228).
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
    /// The image is cloned out of the event before anything is written, because the write
    /// borrows `self` mutably and the events are borrowed from the context. Cheap in the
    /// only case that matters - there is at most one of these per button press, and none
    /// at all on every other frame.
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
        // The title it was taken against, so a folder of captures reads without opening
        // any of them. The directory name rather than the published one: it is what every
        // other artefact this project writes is keyed by.
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
    /// Discards unsaved edits in the preferences window, which is the honest behaviour
    /// for something labelled *reload*: the alternative merges a file with a form and
    /// leaves nobody able to say what the settings now are.
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
    /// Through `resolve` rather than straight off the setting, so a relative root means
    /// the same folder however this window was started. It did not, and the symptom was
    /// a library that filled up under `cargo run` and was empty when the same binary was
    /// launched from `target/debug` (D228).
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
    /// Called when the library changes and when a run finishes - the only two things that
    /// can alter what a row says.
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
                        // The identifier under the name, because that is what appears in
                        // traces and trace file names - somebody reading a report needs
                        // to get from one to the other.
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
        self.finished = None;
        self.running = Some(run::start(
            &module,
            self.prefs.file.library.run_limit_seconds,
            self.prefs.file.library.run_call_budget,
            self.paths.traces_dir(),
        ));
    }

    /// Opens the per-title override file for editing.
    fn open_title_config(&mut self) {
        let Some(title) = self.selected_title() else {
            return;
        };
        let name = title.name.clone();
        let path = self.overrides_path(&name);
        // A file that does not exist yet opens as a commented template rather than as an
        // error: the first thing anybody does here is create one.
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            format!(
                "# Per-title overrides for {name}.\n\
                 #\n\
                 # Settings are merged per key over the shipped defaults, never wholesale,\n\
                 # so anything left out keeps the value it already had.\n\
                 #\n\
                 # A compatibility entry names the *behaviour*, never the title, and carries\n\
                 # a mandatory reason - that is what lets a second title needing the same\n\
                 # thing add a line rather than a code path.\n"
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
                    // The way back. Without it the shell is reachable only by restarting
                    // with a flag or changing a setting, which makes one of the two views a
                    // one-way door - and the setting is about what somebody wants *usually*,
                    // not a control for right now.
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
                    // The one place this application asks a live question rather than
                    // reading a file somebody already wrote. Named for what is on the other
                    // end - a probe - because what it is *running on* is not knowable from
                    // here and naming it would be asserting it.
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
                    // Hand-editing config.toml is a supported way to work - it is how
                    // every setting in this window was reached before the window existed.
                    // So there has to be a way to pick the edit up, and "rescan library"
                    // is not it: that re-reads the folder using settings already in
                    // memory, which looks like it should have worked and did not (D228).
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
    /// on hover. A control that vanishes reads as a bug; a greyed one reads as a state.
    fn toolbar(&mut self, ctx: &egui::Context) {
        // Acted on after the strip is drawn: rescanning replaces the very list the
        // buttons were drawn from, and doing that mid-draw invalidates the selection
        // under the pointer.
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

                // Stop is offered only where it can actually be honoured. Elsewhere it is
                // disabled with the reason, rather than present and silently useless.
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

                // Refresh lives here rather than only in the menu: the library is scanned
                // once at startup, so noticing a title that appeared since is a thing
                // somebody wants to do without hunting through menus.
                if ui
                    .button("⟳ refresh")
                    .on_hover_text("rescan the library folder")
                    .clicked()
                {
                    rescan = true;
                }

                ui.separator();

                // **Captures this window, and the label says so.** There is no guest frame
                // yet - no title reaches its own main loop - so a button reading
                // "screenshot" would be borrowing a meaning it cannot honour. What it does
                // capture is worth having on its own: the panels here are a call tail, a
                // register dump and a ranked finding list, and "paste the panel that says
                // this" otherwise means an operating-system screen grab (D215).
                if ui
                    .button("📷 capture")
                    .on_hover_text("write this window to a PNG in the screenshots folder")
                    .clicked()
                {
                    self.deferred.screenshot = true;
                }

                // Disabled rather than absent, which is this toolbar's rule throughout: a
                // control that vanishes reads as a bug, a greyed one reads as a state. It
                // is here because recording is a thing somebody will look for, and finding
                // it greyed with the reason is a better answer than finding nothing.
                ui.add_enabled(false, egui::Button::new("⏺ record"))
                    .on_disabled_hover_text(concat!(
                        "recording needs a frame source and an encoder, and neither exists ",
                        "yet - no guest has rendered a pixel. See docs/ROADMAP.md phase 6"
                    ));

                // Short and fixed in shape, so a long path does not shove the controls
                // beside it around every time one is taken. The path itself is on hover.
                match &self.last_capture {
                    Some(Ok(path)) => {
                        ui.small("saved").on_hover_text(path.display().to_string());
                    }
                    Some(Err(why)) => {
                        ui.colored_label(egui::Color32::from_rgb(0xd0, 0x60, 0x60), "✖ capture")
                            .on_hover_text(why.as_str());
                    }
                    None => {}
                }

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
                        // How many titles the last scan found, so an empty library is
                        // visibly a scan that ran and found nothing rather than a scan
                        // that never happened.
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
    /// # Why a failed scan has to name this too
    ///
    /// A missing `config.toml` is **not an error** - it is the ordinary first launch, and
    /// the defaults are used silently and correctly. But that makes two very different
    /// situations render identically: settings that say `titles` and were read, and
    /// settings that were never found. Both scan the same folder and report the same
    /// failure, and only one of them is fixed by editing the file.
    ///
    /// That cost a round trip to work out from the outside, which is the definition of a
    /// diagnostic the window could have given and did not (D228).
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
        // Above whatever the scan found, because it explains it. A settings file that
        // failed to parse falls back to defaults, so the library reported below is not
        // the one this installation was configured with - and without saying so, the
        // panel describes a folder nobody chose.
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
                // Which folder was empty. Without this the message is indistinguishable
                // from the window having looked somewhere the reader did not expect.
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
                // Drawn from `self.rows`, which is rebuilt only when something changes -
                // see `rebuild_rows`. Taken out so the icon cache can be borrowed
                // mutably while iterating.
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
                        // Double-click launches, which is what a library list is expected
                        // to do. It also selects, so the panel behind it is not showing a
                        // different title than the one that started.
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
    /// # Why a running application says which build it is
    ///
    /// So a screenshot, a bug report or a run result can be tied to a tree somebody else can
    /// check out. A result that cannot be attributed to a build is a result nobody can
    /// reproduce, and this project's whole argument is that its results are reproducible.
    ///
    /// Where there is no commit - which is every build of this repository so far - it shows
    /// **when the binary was compiled** instead, which answers the question a developer is
    /// actually asking: *am I looking at my last change, or at one from an hour ago?*
    ///
    /// Pinned to the bottom rather than placed after the list, so it does not scroll away
    /// with a long library and does not move when the library is empty (D222).
    fn build_stamp(&self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::bottom("build")
            .show_separator_line(false)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.small(&self.build);
                    // The full detail on hover: the short form is what fits, and the long
                    // form is what somebody pastes into a report.
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
                    // No metadata at all is the ordinary case for homebrew, and saying so
                    // beats an empty gap that reads as a failure.
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
            // The same words the CLI prints, from the same place - two shims describing
            // one measurement differently is what D160 fixed.
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
            // Stack conformance, shown even when clean: a line that only appears on
            // failure cannot be told apart from one nobody wired up (D159).
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
                    ui.monospace(format!("{}({:#x})", call.label, call.arg0));
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
                // **The actions are placed before the panes and pinned to the bottom.**
                // Laid out in reading order they disappeared: the `separator` between the
                // pane list and the pane is a vertical rule in a horizontal layout, so it
                // grows to fill the available height - and in a window that sizes itself
                // to its content, "available" is however much screen there is. The row
                // holding *save* was pushed past the bottom edge of the window, and the
                // only way to reach it was to tab to it blind (D228).
                egui::TopBottomPanel::bottom("preferences-actions").show_inside(ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("save").clicked() {
                            save = true;
                        }
                        if ui.button("rescan library").clicked() {
                            rescan = true;
                        }
                        // Saying so plainly, because a settings window that appears to
                        // take effect immediately and does not is the same lie as a dead
                        // control.
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
                ui.small(
                    "Merged per key over the shipped defaults, never wholesale - anything \
                     left out keeps the value it already had.",
                );
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
    /// The one field a title publishes that predicts anything about the emulator problem
    /// it poses: an interface era, not a marketing number.
    requires: Option<String>,
    /// How the last run of this title went.
    ///
    /// **This is what makes the library a work queue rather than a menu.** The terminal
    /// sweep already answers "where is each title stuck"; showing it per row means the
    /// answer is in front of whoever is choosing what to work on next.
    last_run: Option<String>,
}

/// One line summarising a title's last run.
///
/// `None` when it has never been run, which is different from a run that reached nothing
/// and must not look the same.
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
/// The whole row is one clickable region rather than just the text, because a list where
/// clicking the icon does nothing feels broken in a way that is hard to articulate and
/// easy to notice.
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
                        // A blank of the same size, so rows with and without an icon
                        // still line up.
                        None => ui.add_space(icons::LIST_ICON),
                    }
                    ui.vertical(|ui| {
                        ui.label(&row.title);
                        // Identifier and required version on one line: both are short,
                        // and a row three lines tall turns a library into a scroll.
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
                            // Said rather than left blank: never run and ran badly are
                            // different states, and a gap reads as the second.
                            None => {
                                ui.small(egui::RichText::new("never run").weak());
                            }
                        }
                    });
                    // Fill the row so the click region is the full width rather than
                    // only as wide as the longest name.
                    ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                });
            });
    })
    .response
    .interact(egui::Sense::click())
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Collect a finished run before drawing, so the frame that removes the spinner is
        // the same frame that shows the result.
        if let Some(in_flight) = &self.running {
            match in_flight.poll() {
                Ok(Some(finished)) => {
                    self.finished = Some(finished);
                    self.running = None;
                    // The run just wrote a trace, so the last-run column is stale until
                    // this. The other half of not rebuilding per frame is remembering to
                    // rebuild when something actually changed (D164).
                    self.rebuild_rows();
                }
                Ok(None) => {
                    // Immediate mode only redraws on input, and a guest running on another
                    // thread is not input - without this the spinner freezes and the
                    // result never appears until the pointer moves.
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(()) => {
                    self.finished = Some(run::Finished::stopped());
                    self.running = None;
                }
            }
        }

        // Before drawing, because the reply to a capture asked for on an earlier frame
        // arrives as an ordinary input event and the toolbar wants to report it this
        // frame rather than next.
        self.collect_screenshot(ctx);

        // Before drawing, so a press acts on the frame it happened in rather than the next
        // one. It also settles where the session stands, which decides what is drawn below.
        self.read_input(ctx);

        match self.view {
            // The shell gets the whole window. A console's library is not a panel beside
            // something else, and leaving the toolbar visible would make it the list view
            // with different tiles rather than a second way of meeting the same library.
            View::Shell => {
                egui::CentralPanel::default().show(ctx, |ui| self.shell_panel(ui));
            }
            View::List => {
                self.menu_bar(ctx);
                self.toolbar(ctx);
                egui::SidePanel::left("library")
                    .default_width(260.0)
                    .show(ctx, |ui| self.library_panel(ui));
                egui::CentralPanel::default().show(ctx, |ui| self.detail_panel(ui));
            }
        }
        // **Over everything, in both views.** The shell button is the system's, so it works
        // while the list view is showing too - a person who reaches for it should not have
        // to know which view they happen to be in.
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
        // Beside the other windows rather than inside a view, so the menu item works from
        // whichever of the two happens to be showing.
        self.docs.show(ctx, DOCS);
        // No repaint timer here on purpose. The worker asks for a frame when it has
        // something to show, which is the only time one is needed - a clock redraws the
        // window whether or not anything happened, and that reads as a cursor that will
        // not settle.
        self.probe.show(ctx);

        // Acted on after drawing, so a request made while the state was borrowed for the
        // list or the toolbar is honoured exactly once.
        // After drawing, like every other deferred action: closing while a menu still holds
        // a borrow of the frame is a crash rather than an exit.
        if std::mem::take(&mut self.deferred.close) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if std::mem::take(&mut self.deferred.launch) && self.running.is_none() {
            self.launch();
        }
        // Asked for after the frame is composed, so what comes back is the window as it
        // was just drawn rather than the one before it.
        if std::mem::take(&mut self.deferred.screenshot) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        }
    }
}

/// The pages this build ships, and their order in the reader.
///
/// `include_str!` puts them in the binary, so they cannot disagree with the build somebody is
/// running - there is no version to keep in step and nothing to fetch. What is listed is the
/// *manual*: the decision log and the worklog are development record, they are in the
/// repository, and one of them is most of a megabyte.
const DOCS: &[oops_docs::Doc] = &[
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

#[cfg(test)]
mod docs_tests {
    /// `include_str!` proves a file *exists*. It cannot notice one truncated to nothing, two
    /// entries claiming a slug, or a page with no heading - and all three ship silently,
    /// because a documentation window showing an empty page looks like a page nobody wrote yet.
    #[test]
    fn the_registry_is_sound() {
        assert_eq!(oops_docs::check(super::DOCS), Vec::<String>::new());
    }
}
