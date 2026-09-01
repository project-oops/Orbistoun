//! The desktop window.
//!
//! # It holds no logic, and that is enforced by what it depends on
//!
//! Principle 13: the crates are the emulator, and `orbistoun-cli`, this, and worker mode
//! are interaction shims over them. This crate reads state and draws it. Every decision -
//! what a title is, what a container contains, whether a run got further - is made below
//! it and is reachable from the CLI too.
//!
//! Writing it is what proved the rule. Three things the CLI had quietly absorbed came out
//! within an hour of starting: the run comparison, the previous-trace load, and worker
//! bootstrap. None looked like logic in a shim until a second shim needed them (D160).
//!
//! # Why immediate mode
//!
//! A call tail, a register dump and an import ranking are tables that change wholesale
//! every time a run finishes. Immediate mode draws from current state each frame, which is
//! exactly that shape; a retained widget tree would need syncing against state that is
//! replaced rather than edited (D161).
//!
//! # What is deliberately absent
//!
//! No output surface. The guest executes in a child process (D032) while the window lives
//! here, so presenting a guest frame needs either a reparented child-owned window or
//! shared images through external-memory extensions. That cost was deferred deliberately
//! and stays deferred: there is no frame to present yet, and building the mechanism before
//! there is anything to put through it is speculation by principle 12's own test.

mod app;
mod capture;
mod icons;
mod input;
mod prefs;
mod probe;
mod run;
mod shell;
mod shell_draw;

/// Entry point.
///
/// **Worker mode is checked first, before any window exists.** `WorkerHandle::spawn_self`
/// re-executes this same binary with a flag, so a shim that cannot serve the protocol
/// cannot run a guest at all. Reaching the window code in a worker process would open a
/// second window every time a title was launched.
fn main() -> eframe::Result<()> {
    // This window had no logging at all. `run_native` does not return until it closes, so the
    // guard bound here outlives every frame - dropping it early is how a session's log ends up
    // missing the part somebody was reading it for.
    let _logging = oops_log::Logging::new("orbistoun-gui")
        .build(orbistoun_env::build::line_static())
        .init();

    if std::env::args().any(|arg| arg == orbistoun_worker::WORKER_FLAG) {
        if let Err(e) = orbistoun_worker::serve_as_worker_process() {
            eprintln!("orbistoun: worker: {e}");
        }
        return Ok(());
    }

    // Read before the window exists, so a contradictory command line is a message in the
    // terminal that launched it rather than a window that opened somewhere unexplained.
    // The stored default is read here too - it lives beside the library root, because both
    // describe how somebody wants to meet their own collection (D314).
    let paths = orbistoun_paths::Paths::resolve();
    let default_view = orbistoun_service::FileConfig::load(&paths.config_file())
        .map(|file| file.library.start_in)
        .unwrap_or_default();
    let start = match orbistoun_shell::startup::read(std::env::args(), default_view) {
        Ok(start) => start,
        Err(refusal) => {
            eprintln!("orbistoun: {}", refusal.say());
            std::process::exit(2);
        }
    };

    let mut viewport = egui::ViewportBuilder::default()
        // Large by default: the point of this window is showing a ranked import list
        // and a call tail side by side, and neither is readable in a small one.
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([900.0, 600.0])
        .with_title("orbistoun");

    // The same logo the readme and the site use, so the taskbar entry is recognisable as
    // this program rather than as whatever the platform picks for an unmarked window.
    //
    // `include_bytes!` resolves relative to *this file*, which is why the path is here and
    // not behind a shared helper: a crate in oops-libs cannot embed a consumer's asset, the
    // same constraint that keeps each application's documentation registry in the
    // application.
    match eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/logo.png")) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        // Reported and carried on, like the renderer line below. A window wearing the
        // platform's default icon is worth more than no window.
        Err(e) => eprintln!("orbistoun: window icon: {e}"),
    }

    let options = eframe::NativeOptions {
        viewport,
        ..eframe::NativeOptions::default()
    };

    eframe::run_native(
        "orbistoun",
        options,
        // **Which backend this window got, reported rather than assumed.** `wgpu` picks from
        // `Backends::PRIMARY`, which on this platform holds both Vulkan and DX12, and nothing
        // here pins the choice - so "the window and the guest are both on Vulkan" was an
        // assumption that any future frame-sharing work would have rested on. A window that
        // cannot say what it rendered with is a report about nothing (D317).
        Box::new(move |cc| {
            let renderer = cc.wgpu_render_state.as_ref().map_or_else(
                || "renderer unknown".to_owned(),
                |state| {
                    let info = state.adapter.get_info();
                    format!("{:?} - {}", info.backend, info.name)
                },
            );
            // To the terminal as well as the window, so it is answerable without opening
            // anything - which is what makes it usable as a measurement rather than a label.
            eprintln!("orbistoun: renderer: {renderer}");
            Ok(Box::new(app::App::new(start, renderer)))
        }),
    )
}
