//! The desktop window.
//!
//! An interaction shim like `orbistoun-cli` and worker mode (D034): it reads state and
//! draws it, and every decision is made in the crates below, reachable from the CLI too.
//! Immediate mode (D161) suits tables such as a call tail, a register dump and an import
//! ranking, which are replaced wholesale when a run finishes.

mod app;
mod capture;
mod controls;
mod frame;
mod icons;
mod input;
mod payloads;
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

    // Beside an orbistoun-aot manifest this program is that build's launcher, and plays only its
    // title (D724); `--headless` runs it in this process with no window, as a check.
    let build = orbistoun_service::aot::beside_this_program();
    if let Some((folder, manifest)) = &build
        && std::env::args().any(|arg| arg == HEADLESS_FLAG)
    {
        if let Err(e) = run_headless(folder, manifest) {
            eprintln!("orbistoun: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    // Read before the window exists, so a contradictory command line is reported in the
    // launching terminal. The stored default view lives beside the library root (D314).
    let paths = orbistoun_paths::Paths::resolve();
    let default_view = orbistoun_service::FileConfig::load(&paths.config_file())
        .map(|file| file.library.start_in)
        .unwrap_or_default();
    let start = match &build {
        Some((_, manifest)) => orbistoun_shell::startup::Start::Title {
            name: manifest.title.clone(),
            fallback: orbistoun_shell::startup::View::Shell,
        },
        None => match orbistoun_shell::startup::read(std::env::args(), default_view) {
            Ok(start) => start,
            Err(refusal) => {
                eprintln!("orbistoun: {}", refusal.say());
                std::process::exit(2);
            }
        },
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

    // A build's window is the title's: its published name, and its own icon when it ships one.
    let title = build
        .as_ref()
        .map(|(folder, manifest)| orbistoun_service::aot::title_entry(folder, manifest));
    let title_icon = title
        .as_ref()
        .and_then(|entry| entry.metadata.as_ref())
        .and_then(|meta| meta.icon.as_ref())
        .and_then(|path| std::fs::read(path).ok());
    if let Some(entry) = &title {
        viewport = viewport.with_title(entry.display_name());
    }

    // The project logo for the title bar and taskbar otherwise. `include_bytes!` resolves
    // relative to this file, so the path cannot move behind a shared helper.
    let icon_bytes: &[u8] = title_icon
        .as_deref()
        .unwrap_or(include_bytes!("../../../assets/logo.png"));
    match eframe::icon_data::from_png_bytes(icon_bytes) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        // Reported and carried on: a window with the default icon is still usable.
        Err(e) => eprintln!("orbistoun: window icon: {e}"),
    }

    // A build names what this machine lacks before it launches anything.
    let missing = if build.is_some() {
        orbistoun_worker::missing_requirements()
    } else {
        Vec::new()
    };

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
                app::App::new(start, renderer, build.map(|(folder, _)| folder))
                    .arm_playback(playback)
                    .lacking(missing),
            ))
        }),
    )
}

/// Runs an orbistoun-aot build with no window.
const HEADLESS_FLAG: &str = "--headless";

/// Runs the build's title in this process, as `orbistoun-cli run` asks a worker to, with its events
/// on stderr: `--limit <s>` and `--calls <n>` stop it as they stop a run.
fn run_headless(
    folder: &std::path::Path,
    manifest: &orbistoun_service::aot::Manifest,
) -> Result<(), orbistoun_worker::Error> {
    let value = |flag: &str| {
        std::env::args()
            .skip_while(|argument| argument != flag)
            .nth(1)
            .and_then(|v| v.parse::<u64>().ok())
    };
    orbistoun_worker::run_standalone(orbistoun_proto::Request::Run {
        path: folder.join(orbistoun_service::aot::EXECUTABLE_FILE),
        symbols_db: None,
        limit_seconds: value("--limit"),
        call_budget: value("--calls"),
        input_script: None,
        capture_input: None,
        staged: manifest.staged,
        relink: false,
    })
}
