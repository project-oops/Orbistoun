//! The launcher an orbistoun-aot build carries beside its title (D724).
//!
//! A shim: it names the `eboot.bin` in its own folder and asks the worker's run path to run it
//! in this process, exactly as `orbistoun-cli run` asks a worker process to.

use anyhow::{Context, Result};
use clap::Parser;

/// Runs the title this launcher sits beside.
#[derive(Debug, Parser)]
#[command(version = orbistoun_env::build::line_static())]
struct Launch {
    /// Seconds of guest execution before the run is stopped; none runs until the title ends.
    #[arg(long)]
    limit: Option<u64>,
    /// Imports the guest may call before the run is stopped; none sets no budget.
    #[arg(long)]
    calls: Option<u64>,
}

fn main() -> Result<()> {
    let _logging = oops_log::Logging::new("orbistoun-aot")
        .build(orbistoun_env::build::line_static())
        .init();
    let launch = Launch::parse();

    let exe = std::env::current_exe().context("finding this launcher")?;
    let folder = exe
        .parent()
        .context("this launcher sits in no folder")?
        .to_path_buf();
    let manifest = orbistoun_service::aot::Manifest::read(&folder).with_context(|| {
        format!(
            "no {} beside this launcher in {}",
            orbistoun_service::aot::MANIFEST_FILE,
            folder.display()
        )
    })?;

    // A played build reads the host clock unless chosen otherwise, as the window does (D723).
    if std::env::var_os(orbistoun_env::CLOCK.name).is_none() {
        // SAFETY: nothing else runs yet, so nothing reads the environment concurrently.
        unsafe { std::env::set_var(orbistoun_env::CLOCK.name, "host") };
    }

    orbistoun_worker::run_standalone(orbistoun_proto::Request::Run {
        path: folder.join(orbistoun_service::aot::EXECUTABLE_FILE),
        symbols_db: None,
        limit_seconds: launch.limit,
        call_budget: launch.calls,
        input_script: None,
        capture_input: None,
        staged: manifest.staged,
        relink: false,
    })
    .with_context(|| format!("running {}", manifest.title))
}
