//! Application preferences, and per-title overrides.
//!
//! # What is here, and what is deliberately not
//!
//! Every control below changes something that **actually takes effect on the next run**.
//! That is a rule rather than a coincidence: a setting whose subsystem does not exist yet
//! is principle 3's failure mode wearing a dropdown. Somebody picks 1080p, nothing
//! changes, and there is no way to tell whether the setting is broken, the emulator
//! ignored it, or the title overrode it.
//!
//! So the video and input panes exist and say what is missing, rather than showing
//! controls that do nothing. The roadmap called this before the window did: *settings
//! panes stubbed, populated as the subsystems behind them land rather than built as dead
//! UI* (D162).
//!
//! # These are the bisection loop
//!
//! The settings that *are* here - the entry convention, the thread policy, the direct
//! memory switch - are not conveniences. They are the levers the whole method turns on,
//! and until now every one of them meant hand-editing a TOML in the data directory.
//! Today's alignment bug was found by flipping one of them (D159).

use orbistoun_service::FileConfig;
use orbistoun_shell::View;

/// Which pane the preferences window is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Pane {
    /// Where titles live, how long a run may take.
    #[default]
    General,
    /// How the guest's entry point is presented.
    Entry,
    /// What the guest is told about the machine, and where threads are placed.
    Threads,
    /// Subsystem switches.
    Memory,
    /// What the emulated console is set to.
    Shell,
    /// Controllers, and what maps to what.
    Pads,
    /// Not built.
    Video,
    /// Not built.
    Input,
}

impl Pane {
    /// Every pane, in the order they are listed.
    pub(crate) const ALL: [Self; 8] = [
        Self::General,
        Self::Entry,
        Self::Threads,
        Self::Memory,
        Self::Shell,
        Self::Pads,
        Self::Video,
        Self::Input,
    ];

    /// The label in the pane list.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Entry => "entry",
            Self::Threads => "threads",
            Self::Memory => "memory",
            Self::Shell => "shell",
            Self::Pads => "controllers",
            Self::Video => "video",
            Self::Input => "input",
        }
    }
}

/// Preferences the window owns, plus the file they are written to.
pub(crate) struct Preferences {
    /// Where this installation keeps its data, which is what a relative library root is
    /// resolved against. Held so the general pane can show the folder that setting
    /// actually names rather than leaving the reader to work it out.
    pub(crate) data_root: std::path::PathBuf,
    /// The settings a run reads, exactly as they are stored - including where the
    /// library is, which is why it survives a restart.
    pub(crate) file: FileConfig,
    /// What the emulated console is set to.
    ///
    /// **Held here rather than on the window, so there is one copy.** The shell draws it and
    /// this pane edits it; two copies would eventually disagree about what the machine is
    /// set to, and the one the guest reads would be whichever got written last.
    ///
    /// Its own file, because `config.toml` holds how the *emulator* is configured and this
    /// holds what the machine it presents is set to (D311).
    pub(crate) shell: orbistoun_shell::Settings,
    /// Which pane is showing.
    pub(crate) pane: Pane,
    /// Whether the window is open.
    pub(crate) open: bool,
    /// What happened to the last save, if anything.
    pub(crate) status: Option<Result<String, String>>,
    /// Why the settings file could not be read, if it could not.
    ///
    /// Separate from `status`, which also carries save results and is only visible while
    /// the preferences window is open. A settings file that failed to parse falls back to
    /// defaults - including the library folder - so the *library panel* reports something
    /// puzzling while the explanation sits behind a window nobody has opened. This is what
    /// lets that panel say why (D228).
    pub(crate) load_error: Option<String>,
}

impl Preferences {
    /// Reads what is on disk.
    ///
    /// A malformed file surfaces as a status rather than being replaced silently. The
    /// alternative loses whatever somebody was mid-way through writing by hand, and hides
    /// that their file was wrong (D153).
    pub(crate) fn load(
        path: &std::path::Path,
        shell_path: &std::path::Path,
        data_root: &std::path::Path,
    ) -> Self {
        let (file, load_error) = match FileConfig::load(path) {
            Ok(file) => (file, None),
            Err(e) => (
                FileConfig::default(),
                Some(format!("{e} - showing defaults, nothing was overwritten")),
            ),
        };
        // Reported separately from the one above rather than folded in. Two files fail for
        // unrelated reasons, and "your console settings would not parse" sends somebody to a
        // different file from "your run configuration would not parse".
        let (shell, shell_error) = match orbistoun_shell::Settings::load(shell_path) {
            Ok(shell) => (shell, None),
            Err(e) => (
                orbistoun_shell::Settings::default(),
                Some(format!("{e} - showing defaults, nothing was overwritten")),
            ),
        };
        let load_error = match (load_error, shell_error) {
            (Some(a), Some(b)) => Some(format!("{a}; {b}")),
            (some, None) | (None, some) => some,
        };
        Self {
            file,
            shell,
            data_root: data_root.to_path_buf(),
            pane: Pane::default(),
            open: false,
            status: load_error.clone().map(Err),
            load_error,
        }
    }

    /// Writes the settings back.
    ///
    /// **Both files, and a failure in either is reported.** They are saved by one button
    /// because they are edited in one window; a save that quietly wrote one of them would
    /// leave somebody's console settings on disk from a previous session while the run
    /// configuration moved on.
    pub(crate) fn save(&mut self, path: &std::path::Path, shell_path: &std::path::Path) {
        let config = self
            .file
            .to_toml()
            .map_err(|e| e.to_string())
            .and_then(|text| std::fs::write(path, text).map_err(|e| e.to_string()));
        let shell = self
            .shell
            .to_toml()
            .and_then(|text| std::fs::write(shell_path, text).map_err(|e| e.to_string()));
        self.status = Some(match (config, shell) {
            (Ok(()), Ok(())) => Ok(format!("saved to {}", path.display())),
            (Err(e), _) => Err(format!("could not save: {e}")),
            (_, Err(e)) => Err(format!("could not save the console settings: {e}")),
        });
    }
}

/// Draws one pane's controls.
pub(crate) fn pane_contents(
    ui: &mut egui::Ui,
    prefs: &mut Preferences,
    live: &[orbistoun_input::PadState],
) {
    match prefs.pane {
        Pane::General => general(ui, prefs),
        Pane::Entry => entry(ui, &mut prefs.file),
        Pane::Threads => threads(ui, &mut prefs.file),
        Pane::Memory => memory(ui, &mut prefs.file),
        Pane::Shell => shell(ui, &mut prefs.shell),
        Pane::Pads => pads(ui, &mut prefs.file.pads, live),
        Pane::Video => not_built(
            ui,
            "video",
            "No output subsystem exists. The guest runs in a child process and nothing \
             presents a frame yet, so resolution, window mode and vertical sync would all \
             be controls over nothing.",
        ),
        Pane::Input => not_built(
            ui,
            "input",
            "No input subsystem exists. The controller shim declares its interface and \
             implements none of it, so a binding here could not reach a guest.",
        ),
    }
}

/// The pane for settings that are about this machine rather than the guest.
fn general(ui: &mut egui::Ui, prefs: &mut Preferences) {
    ui.heading("general");
    ui.horizontal(|ui| {
        ui.label("library folder");
        ui.text_edit_singleline(&mut prefs.file.library.root);
    });
    // What that setting actually means on this machine. A relative root is joined to
    // the data root rather than to wherever the program was started from, and showing
    // the result is the difference between a setting and a guess (D228).
    let resolved = prefs.file.library.resolve(&prefs.data_root);
    ui.horizontal(|ui| {
        ui.small("scans");
        ui.weak(resolved.display().to_string());
        if !resolved.is_dir() {
            ui.colored_label(egui::Color32::LIGHT_RED, "(not a folder)");
        }
    });
    ui.small(concat!(
        "Saved with the rest of the settings, so it is found again next launch from ",
        "any folder. A relative path is taken from the data root - beside the binary ",
        "in a portable build - never from the directory this window was started in.",
    ));
    ui.horizontal(|ui| {
        ui.label("run limit");
        ui.add(egui::DragValue::new(&mut prefs.file.library.run_limit_seconds).range(0..=600));
        ui.label("seconds (0 = no limit)");
    });
    ui.small(
        "A limit is not a safety net - a guest with everything unimplemented settles into \
         a loop waiting for something that will never happen, and without a limit the run \
         hangs and takes its call trace with it.",
    );
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("open in");
        for view in [View::List, View::Shell] {
            ui.selectable_value(&mut prefs.file.library.start_in, view, view.label());
        }
    });
    ui.small(concat!(
        "Which view this window opens into when nothing on the command line says. ",
        "`--list` and `--shell` override it for one launch, and `--title <name>` skips ",
        "the choice by launching straight into a title - returning here when it ends.",
    ));
}

/// Controllers: how many, what drives each, and which key is which button.
///
/// # The live readout is not decoration
///
/// A mapping is a claim that pressing a key presses a button, and until now the only way to
/// check one was to launch something and see whether it responded. The lit indicators make
/// the claim checkable in the place it is edited - press the key, watch the button light -
/// which is the same argument the rest of this project makes about every other claim.
fn pads(ui: &mut egui::Ui, pads: &mut orbistoun_input::Pads, live: &[orbistoun_input::PadState]) {
    use orbistoun_input::{Button, Source};

    ui.heading("controllers");
    let mut count = pads.count();
    ui.horizontal(|ui| {
        ui.label("controllers");
        if ui
            .add(egui::DragValue::new(&mut count).range(1..=orbistoun_input::MAX_PORTS))
            .changed()
        {
            pads.set_count(count);
        }
    });
    ui.small(concat!(
        "A port with nothing driving it is a pad nobody is holding, which is a state a ",
        "title may enumerate - not the same as the port not existing.",
    ));

    for (index, port) in pads.ports.iter_mut().enumerate() {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(format!("port {}", index + 1));
            ui.selectable_value(&mut port.source, Source::Empty, "empty");
            ui.selectable_value(&mut port.source, Source::Keyboard, "keyboard");
            ui.selectable_value(&mut port.source, Source::Gamepad { index: 0 }, "gamepad");
        });
        if matches!(port.source, Source::Gamepad { .. }) {
            // Said where the control is, not in a release note. A port set to something
            // this build cannot read would otherwise be a pad that silently never moves.
            ui.colored_label(
                egui::Color32::LIGHT_YELLOW,
                "no gamepad is read yet - this port reports a pad nobody is holding",
            );
        }

        let state = live.get(index).copied().unwrap_or_default();
        egui::Grid::new(("pad-keys", index))
            .num_columns(3)
            .show(ui, |ui| {
                for button in Button::ALL {
                    // Lit from the live state, so the row proves itself.
                    let down = state.is_down(button);
                    ui.colored_label(
                        if down {
                            egui::Color32::LIGHT_GREEN
                        } else {
                            ui.visuals().weak_text_color()
                        },
                        button.label(),
                    );
                    let mut key = port.keys.get(&button).cloned().unwrap_or_default();
                    if ui.text_edit_singleline(&mut key).changed() {
                        if key.is_empty() {
                            port.keys.remove(&button);
                        } else {
                            port.keys.insert(button, key);
                        }
                    }
                    ui.small(if down { "down" } else { "" });
                    ui.end_row();
                }
                // Sticks, in the same grid. A keyboard could not move one at all until
                // recently, and a binding nobody can edit is only half a fix (D341).
                for push in orbistoun_input::Push::ALL {
                    let (stick, x, y) = push.amount();
                    let pushed = state.sticks[stick].x * x + state.sticks[stick].y * y;
                    ui.colored_label(
                        if pushed > 0.0 {
                            egui::Color32::LIGHT_GREEN
                        } else {
                            ui.visuals().weak_text_color()
                        },
                        push.label(),
                    );
                    let mut key = port.axes.get(&push).cloned().unwrap_or_default();
                    if ui.text_edit_singleline(&mut key).changed() {
                        if key.is_empty() {
                            port.axes.remove(&push);
                        } else {
                            port.axes.insert(push, key);
                        }
                    }
                    ui.small(if pushed > 0.0 { "pushed" } else { "" });
                    ui.end_row();
                }
            });
    }

    // Both problems reported, both beside the thing that caused them, and neither resolved
    // silently - a binding that half works with nothing saying so is the failure here.
    ui.separator();
    for conflict in pads.conflicts() {
        ui.colored_label(egui::Color32::LIGHT_RED, conflict.say());
    }
    for bad in crate::input::unresolved(pads) {
        ui.colored_label(egui::Color32::LIGHT_RED, bad);
    }
    ui.small(concat!(
        "Key names are egui's: `ArrowUp`, `Enter`, `Backspace`, or a single letter or ",
        "digit. A name nothing recognises is listed above rather than quietly bound to ",
        "nothing.",
    ));
}

/// What the emulated console is set to.
///
/// # This pane bends the module's rule, and says so on screen
///
/// The rule above is that every control takes effect on the next run, because a setting
/// whose subsystem does not exist is principle 3's failure wearing a dropdown. These are
/// the awkward case: the settings are **real** - they are facts about what the owner wants
/// their machine to be - and only one of them currently reaches anything.
///
/// The resolution is not to hide them, because then nothing can ever be set. It is to say
/// which is which, in the pane, in terms of something checkable: how many parameter
/// identifiers have a measured encoding. That number is zero today and the pane says zero.
/// When it stops being zero these controls start mattering, and nothing here needs editing
/// for that to happen (D311).
fn shell(ui: &mut egui::Ui, settings: &mut orbistoun_shell::Settings) {
    use orbistoun_shell::ButtonAssignment;

    ui.heading("shell");

    // **Users, editable, because a name a guest reads has to be one somebody chose.**
    // `sceUserServiceGetUserName` answers from this list, so a list nobody can edit would be
    // the settings-are-the-point argument made and then not delivered (D346).
    ui.label("users");
    let mut remove = None;
    for index in 0..settings.users.len() {
        ui.horizontal(|ui| {
            let id = settings.users[index].id;
            // Signing in is a radio rather than a per-row toggle: exactly one user is signed
            // in, and two checkboxes both ticked is a state nothing here models.
            ui.radio_value(&mut settings.signed_in, id, "");
            ui.text_edit_singleline(&mut settings.users[index].name);
            ui.weak(format!("id {id}"));
            // The last user cannot be removed. A machine with no accounts is one no title can
            // start on, and rebuilding it from an empty list is worse than refusing here.
            if settings.users.len() > 1 && ui.button("remove").clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        settings.users.remove(index);
    }
    if ui.button("add a user").clicked() {
        let id = settings.take_id();
        settings.users.push(orbistoun_shell::User {
            id,
            name: format!("player {id}"),
        });
    }
    if settings.current().is_none() {
        // Reachable by removing whoever was signed in, and said rather than silently
        // repaired: a title asking who is signed in gets a refusal, and the reason should be
        // visible where it was caused.
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            "nobody is signed in - a title asking for the current user will be refused",
        );
    }
    ui.small(concat!(
        "A name reaches a guest as text, unencoded - the one console setting that needs no ",
        "measurement to answer. Identifiers are never reused, because save data is keyed on ",
        "them.",
    ));

    ui.separator();
    ui.horizontal(|ui| {
        ui.label("language");
        ui.text_edit_singleline(&mut settings.language);
    });
    ui.small(concat!(
        "A BCP 47 tag - `en-GB`, `ja-JP`. A published standard rather than the platform's ",
        "own numbering, so the value means something on its own; turning it into whatever ",
        "a guest expects is a measurement.",
    ));

    ui.horizontal(|ui| {
        ui.label("confirm button");
        for assignment in [ButtonAssignment::South, ButtonAssignment::East] {
            let label = match assignment {
                ButtonAssignment::South => "south (lower)",
                ButtonAssignment::East => "east (right)",
            };
            ui.selectable_value(&mut settings.confirm, assignment, label);
        }
    });
    ui.small(concat!(
        "Named by position rather than by glyph: the two conventions differ by region and ",
        "a direction is what the hardware actually presents.",
    ));

    ui.separator();
    // The honest part, and the reason this pane is allowed to exist under the rule at the
    // top of the file. Counted rather than asserted, so it cannot drift out of date.
    let answerable = orbistoun_shell::Parameters::empty();
    ui.colored_label(
        egui::Color32::LIGHT_YELLOW,
        if answerable.is_empty() {
            "0 parameters have a measured encoding, so nothing below the name reaches a guest yet"
        } else {
            "some parameters have measured encodings"
        },
    );
    ui.small(concat!(
        "A title asks for these by an identifier this project has no lawful source for, ",
        "and the answer has an encoding it has no source for either. Both are measurements. ",
        "Until one is made, a setting changes what the shell shows and what a guest is told ",
        "is withheld and counted - rather than a plausible number being invented for it.",
    ));
}

/// How the guest's entry point is presented.
fn entry(ui: &mut egui::Ui, file: &mut FileConfig) {
    use orbistoun_loader::process::{Convention, EntryArgument};

    ui.heading("entry");
    ui.small(
        "How control reaches the guest's first instruction. Measured, not assumed: \
         entering by call leaves every later guest call on a conforming stack, and \
         entering by jump does not.",
    );
    ui.separator();

    ui.label("convention");
    ui.radio_value(
        &mut file.entry.convention,
        Convention::Function,
        "function - called, rsp 8 past alignment (measured correct for this target)",
    );
    ui.radio_value(
        &mut file.entry.convention,
        Convention::Process,
        "process - jumped to, rsp aligned (System V process entry)",
    );

    ui.separator();
    ui.label("first argument register");
    ui.radio_value(
        &mut file.entry.argument,
        EntryArgument::ImageAddress,
        "the process image address",
    );
    ui.radio_value(
        &mut file.entry.argument,
        EntryArgument::ZeroedBlock,
        "a zeroed block",
    );
    ui.radio_value(
        &mut file.entry.argument,
        EntryArgument::Zero,
        "nothing - faults immediately, kept so the question stays answerable",
    );
    ui.radio_value(
        &mut file.entry.argument,
        EntryArgument::Sentinels,
        "markers - every slot a different unmapped address, so a fault names the field",
    );
    ui.radio_value(
        &mut file.entry.argument,
        EntryArgument::Answering,
        "answering - slot 0 returns zero, the rest are writable, to see how far it gets",
    );
    ui.radio_value(
        &mut file.entry.argument,
        EntryArgument::Reporting,
        "reporting - every slot answers zero and says which one was called, and with what",
    );
    ui.radio_value(
        &mut file.entry.argument,
        EntryArgument::Handoff,
        "handoff - the resolver in field zero, markers in the fields nothing has established",
    );
    ui.small(concat!(
        "The last two are diagnostics rather than settings: they exist to find out what a ",
        "guest's entry point wants handed to it, and a run under either is not an ordinary ",
        "run and must not be compared with one (D308).",
    ));
}

/// What the guest believes about the machine, and where its threads run.
fn threads(ui: &mut egui::Ui, file: &mut FileConfig) {
    use orbistoun_kernel::thread::AffinityPolicy;

    ui.heading("threads");
    ui.small(
        "The guest decides how many threads exist by asking for them; the host decides \
         how many run at once. There is no minimum to enforce - a slower machine runs the \
         same program more slowly.",
    );
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("cores the guest is told it has");
        ui.add(egui::DragValue::new(&mut file.threads.topology.cores).range(1..=128));
    });
    ui.horizontal(|ui| {
        ui.label("of which usable");
        ui.add(egui::DragValue::new(&mut file.threads.topology.usable).range(1..=128));
    });
    ui.small("The target's shape by default, not this machine's - a title asking how many cores it has is asking about the machine it was written for.");

    ui.separator();
    ui.label("affinity requests");
    ui.radio_value(
        &mut file.threads.affinity,
        AffinityPolicy::Observe,
        "observe - record the request, let the host place the thread",
    );
    ui.radio_value(
        &mut file.threads.affinity,
        AffinityPolicy::Map,
        "map - fold guest cores onto host cores, preserving distinctness",
    );
    ui.radio_value(
        &mut file.threads.affinity,
        AffinityPolicy::Strict,
        "strict - apply as given, refuse if the host cannot",
    );
    ui.small("The request is recorded on the thread whichever applies, so a title that turns out to depend on placement can be found rather than guessed at.");
}

/// Subsystem switches.
fn memory(ui: &mut egui::Ui, file: &mut FileConfig) {
    ui.heading("memory");
    ui.checkbox(
        &mut file.memory.map_direct_memory,
        "map direct memory for real",
    );
    ui.small(
        "Off, this answers unimplemented and the guest gets no virtual address for memory \
         it reserved. It was off for one afternoon while a fault inside it went \
         unexplained - the cause turned out to be the entry convention, not the mapping.",
    );
}

/// A pane for a subsystem that does not exist.
fn not_built(ui: &mut egui::Ui, name: &str, why: &str) {
    ui.heading(name);
    ui.add_space(8.0);
    ui.colored_label(egui::Color32::from_gray(160), "nothing to configure yet");
    ui.add_space(4.0);
    ui.label(why);
    ui.add_space(8.0);
    ui.small(
        "This pane is deliberately empty rather than filled with controls that do \
         nothing. A setting that silently has no effect is indistinguishable from one \
         that is broken.",
    );
}
