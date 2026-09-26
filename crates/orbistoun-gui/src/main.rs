//! The desktop window.
//!
//! An interaction shim like `orbistoun-cli` and worker mode (D034): it reads state and
//! draws it, and every decision is made in the crates below, reachable from the CLI too.
//! Immediate mode (D161) suits tables such as a call tail, a register dump and an import
//! ranking, which are replaced wholesale when a run finishes.

mod app;
mod capture;
mod frame;
mod icons;
mod input;
mod perf_overlay;
mod prefs;
mod probe;
mod run;
mod shell;
mod shell_draw;

/// Entry point.
///
/// Worker mode is checked first, before any window exists: `WorkerHandle::spawn_self`
/// re-executes this binary with a flag (D033), and a worker must not open a window.
fn main() -> eframe::Result<()> {
    // `run_native` does not return until the window closes, so this guard outlives every
    // frame and the log covers the whole session.
    let _logging = oops_log::Logging::new("orbistoun-gui")
        .build(orbistoun_env::build::line_static())
        .init();

    if std::env::args().any(|arg| arg == orbistoun_worker::WORKER_FLAG) {
        if let Err(e) = orbistoun_worker::serve_as_worker_process() {
            eprintln!("orbistoun: worker: {e}");
        }
        return Ok(());
    }

    // A played window reads the host clock unless chosen otherwise (D723): the logical clock
    // advances per reading, so a guest spinning on its counter races ahead of the player.
    // Set before any worker exists, because each worker inherits it.
    if std::env::var_os(orbistoun_env::CLOCK.name).is_none() {
        // SAFETY: no worker or guest thread exists yet, and nothing started so far reads the
        // environment concurrently; the spawned workers inherit it at their start.
        unsafe { std::env::set_var(orbistoun_env::CLOCK.name, "host") };
    }

    // Read before the window exists, so a contradictory command line is reported in the
    // launching terminal. The stored default view lives beside the library root (D314).
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
    // `--playback <file>`: captured input armed for the first launch, as if chosen from the
    // toolbar's "playback input" (D721).
    let playback = std::env::args()
        .skip_while(|argument| argument != "--playback")
        .nth(1)
        .map(std::path::PathBuf::from);

    let mut viewport = egui::ViewportBuilder::default()
        // Large by default, to fit a ranked import list and a call tail side by side.
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([900.0, 600.0])
        .with_title("orbistoun");

    // The project logo for the title bar and taskbar. `include_bytes!` resolves relative to
    // this file, so the path cannot move behind a shared helper.
    match eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/logo.png")) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        // Reported and carried on: a window with the default icon is still usable.
        Err(e) => eprintln!("orbistoun: window icon: {e}"),
    }

    let options = eframe::NativeOptions {
        viewport,
        ..eframe::NativeOptions::default()
    };

    eframe::run_native(
        "orbistoun",
        options,
        // The backend is reported rather than assumed: `wgpu` picks from
        // `Backends::PRIMARY`, which on Windows holds both Vulkan and DX12, unpinned here.
        Box::new(move |cc| {
            let renderer = cc.wgpu_render_state.as_ref().map_or_else(
                || "renderer unknown".to_owned(),
                |state| {
                    let info = state.adapter.get_info();
                    format!("{:?} - {}", info.backend, info.name)
                },
            );
            // To the terminal as well as the window, so it is readable without the window.
            eprintln!("orbistoun: renderer: {renderer}");
            Ok(Box::new(
                app::App::new(start, renderer).arm_playback(playback),
            ))
        }),
    )
}
