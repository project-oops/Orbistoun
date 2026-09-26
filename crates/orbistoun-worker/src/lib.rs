//! Worker mode: hosting the crates behind the protocol, and driving one from a shim.
//!
//! Guest code executes in a child process (D032). This crate is both halves of that arrangement:
//!
//! - [`serve`] is the child. It reads [`Request`]s, calls the service, and writes [`Event`]s,
//!   holding no logic of its own.
//! - [`WorkerHandle`] is the parent. It re-invokes the running executable with a hidden flag rather
//!   than spawning a separate worker binary, so shim and worker cannot be different versions.
//!
//! [`serve`] takes a reader and a writer rather than the real stdio, so the protocol loop is tested
//! over in-memory pipes and spawning is tested separately.

use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use orbistoun_proto::codec::{read_message, write_message};
use orbistoun_proto::{Event, Outcome, PROTOCOL_VERSION, Phase, Request, check_version};
pub mod device_thread;
mod error;
pub mod experiment;
pub mod fault;
pub mod frame_region;
pub mod page_guard;
pub mod profile;
pub mod render;
pub mod report;
pub mod session;
pub mod tls_backstop;
pub mod watch;
pub mod watchpoint;

use orbistoun_loader::Image;
use orbistoun_loader::process;
use orbistoun_mem::stack::{DEFAULT_STACK_SIZE, GuestStack};
use orbistoun_service::Service;

pub use error::Error;

/// Hidden flag that puts a shim into worker mode.
///
/// Named once here so the spawning side and the parsing side cannot disagree.
pub const WORKER_FLAG: &str = "--worker";

/// Runs the worker loop until the peer asks it to stop or the stream ends.
///
/// Errors that belong to a request are reported as [`Event::Failed`] and the loop continues; only a
/// broken stream ends it.
///
/// Reading happens on its own thread, because a run occupies this thread from `Run` until the guest
/// stops and a shell action must arrive during it (D310). [`Request::Shell`] is applied on the
/// reading thread, since it needs no reply; everything else is forwarded here, so the output stream
/// keeps one writer.
///
/// `on_hangup` is called on the reading thread when the stream ends without a
/// [`Request::Shutdown`]: the only peer, [`WorkerHandle`], holds the pipe open until it sends one,
/// so the peer is gone (D715). It is not left to this loop, which may be inside a guest that never
/// returns.
pub fn serve<R: BufRead + Send + 'static, W: Write>(
    input: R,
    mut output: W,
    service: &Service,
    on_hangup: impl FnOnce() + Send + 'static,
) -> io::Result<()> {
    let (sender, receiver) = std::sync::mpsc::channel::<io::Result<Request>>();
    let reader = std::thread::spawn(move || {
        let mut input = input;
        loop {
            match read_message::<_, Request>(&mut input) {
                Ok(None) => {
                    on_hangup();
                    return;
                }
                // Answered here and not forwarded: the main loop may be inside the guest.
                Ok(Some(Request::Shell { action })) => shell_action(action),
                // Same reason: input arrives while a run is in flight. It replies with nothing, so
                // the output stream keeps one writer.
                Ok(Some(Request::Input { pads })) => orbistoun_input::latest::arrived(&pads),
                // The toolbar's input capture and playback (D721), mid-run, with no reply.
                Ok(Some(Request::CaptureInput { to })) => capture_input(to.as_deref()),
                Ok(Some(Request::PlayInput { script })) => play_input(script.as_deref()),
                Ok(Some(request)) => {
                    let last = matches!(request, Request::Shutdown);
                    if sender.send(Ok(request)).is_err() || last {
                        return;
                    }
                }
                // Forwarded, so a broken stream ends the loop with the error that broke it.
                Err(e) => {
                    let _ = sender.send(Err(e));
                    return;
                }
            }
        }
    });

    let outcome = serve_requests(&receiver, &mut output, service);
    // The reader stops on its own once the channel's receiving end is dropped, so this
    // never blocks on a peer that has gone quiet.
    drop(receiver);
    let _ = reader.join();
    outcome
}

/// The handling half, once requests are arriving on a channel.
fn serve_requests<W: Write>(
    receiver: &std::sync::mpsc::Receiver<io::Result<Request>>,
    mut output: &mut W,
    service: &Service,
) -> io::Result<()> {
    while let Ok(message) = receiver.recv() {
        let request = message?;
        match request {
            Request::Hello { protocol_version } => {
                if let Err(mismatch) = check_version(protocol_version) {
                    write_message(
                        &mut output,
                        &Event::Failed {
                            error: mismatch.to_string(),
                        },
                    )?;
                    // A version mismatch is not recoverable: every later message would be parsed
                    // against the wrong contract.
                    return Ok(());
                }
                write_message(
                    &mut output,
                    &Event::Hello {
                        protocol_version: PROTOCOL_VERSION,
                        worker_version: env!("CARGO_PKG_VERSION").to_owned(),
                    },
                )?;
            }
            Request::Survey { path } => match service.survey_path(&path) {
                Ok(summary) => {
                    write_message(
                        &mut output,
                        &Event::Reached {
                            phase: Phase::ContainerParsed,
                        },
                    )?;
                    write_message(&mut output, &Event::SurveyComplete(summary))?;
                }
                Err(e) => {
                    write_message(
                        &mut output,
                        &Event::Failed {
                            error: e.to_string(),
                        },
                    )?;
                }
            },
            Request::Link {
                path,
                symbols_db,
                relink,
            } => write_message(
                &mut output,
                &link_ahead(service, &path, symbols_db.as_deref(), relink),
            )?,
            Request::Run {
                path,
                symbols_db,
                limit_seconds,
                call_budget,
                input_script,
                capture_input: armed_capture,
                staged,
                relink,
            } => {
                // A title exists from here, so a shell action has something to act on. An earlier
                // request is refused and counted rather than queued for a title that may never
                // start.
                session::begin();
                *run_input_script() = input_script;
                *run_capture_input() = armed_capture;
                RUN_STAGED.store(staged, std::sync::atomic::Ordering::Relaxed);
                RUN_RELINK.store(relink, std::sync::atomic::Ordering::Relaxed);
                let ran = run_guest(
                    &mut output,
                    service,
                    &path,
                    symbols_db.as_deref(),
                    Limits {
                        seconds: limit_seconds,
                        calls: call_budget,
                    },
                );
                session::end();
                *run_input_script() = None;
                *run_capture_input() = None;
                RUN_STAGED.store(false, std::sync::atomic::Ordering::Relaxed);
                RUN_RELINK.store(false, std::sync::atomic::Ordering::Relaxed);
                capture_input(None);
                play_input(None);
                ran?;
            }
            // Answered on the reading thread. Spelled out rather than caught by a wildcard, so a
            // new request is a compile error here instead of a silent drop.
            Request::Shell { .. }
            | Request::Input { .. }
            | Request::CaptureInput { .. }
            | Request::PlayInput { .. } => {
                unreachable!("answered on the reading thread as they arrive")
            }
            Request::Shutdown => return Ok(()),
        }
    }
    Ok(())
}

/// Carries a shell action into the running session.
///
/// Runs on the reading thread. A refusal is counted rather than reported back, because the handling
/// loop is the output stream's one writer; [`session::summarise`] reports the count.
fn shell_action(action: orbistoun_shell::Request) {
    let _ = session::apply(action);
}

/// Base a module is placed at when it links at zero.
///
/// A module carries no absolute addresses, so it needs somewhere to go: clear of anything the host
/// maps, and granularity-aligned.
pub const DEFAULT_MODULE_BASE: u64 = 0x0000_4000_0000_0000;

/// Where the modules a title ships with itself are placed.
///
/// Between the main executable and the guest stack, and clear of both: the title's own modules are
/// placed before the executable (D482), so they need a region that cannot collide with its base.
/// Each module gets its own slot with a granule of unmapped guard between them. Clear of
/// `0x5000_0000_0000`, where a guest allocator reserves its heap arena (D488).
pub const TITLE_MODULE_BASE: u64 = 0x0000_4800_0000_0000;

/// Where the per-import stub table is placed.
///
/// Far from [`DEFAULT_MODULE_BASE`], so a stray offset from either lands in unmapped space and
/// faults rather than reaching the other allocation.
pub const THUNK_TABLE_BASE: u64 = orbistoun_thunk::SUGGESTED_BASE;

/// Where storage for data imports is reserved.
///
/// Clear of both the images and the thunk table, so a stray offset from any of the three faults
/// rather than hitting another allocation.
pub const DATA_BLOCK_BASE: u64 = orbistoun_thunk::SUGGESTED_DATA_BASE;

/// Where the guest stack is reserved.
///
/// Clear of both the image and the stub table, so an overrun of any of the three faults rather than
/// reaching another.
pub const GUEST_STACK_BASE: u64 = 0x0000_6000_0000_0000;

/// How many recorded calls are quoted in the summary.
///
/// The beginning of a boot is the readable part; the full log is available separately.
pub const SUMMARISED_CALLS: usize = 8;

/// Loads a guest as far as the implementation goes, reporting each phase reached.
///
/// A run that stops says so, because a silent stop is indistinguishable from a guest that ran and
/// did nothing (D010).
fn run_guest<W: Write>(
    output: &mut W,
    service: &Service,
    path: &Path,
    symbols_db: Option<&Path>,
    limits: Limits,
) -> io::Result<()> {
    // Declares how this process was delivered, from what sits beside the module, before anything
    // can ask: a title does not resolve platform symbols by name and a payload does (D669). Before
    // the execution guard, because the route is a property of the run whether or not it executes
    // anything.
    orbistoun_core::route::present(orbistoun_core::route::route_for(path, Path::exists));

    // Refused here rather than at the transfer, where it would reach the user as a child-process
    // panic that reads as a bug in the tool.
    if !orbistoun_abi::enter::can_execute_guests() {
        return halt(
            output,
            Phase::ContainerParsed,
            concat!(
                "this build cannot execute guest code: guest instructions are x86-64 and ",
                "run natively, so a build for another architecture can analyse a title but ",
                "never run one. Every other command works here."
            )
            .to_owned(),
        );
    }

    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            write_message(
                output,
                &Event::Failed {
                    error: format!("reading {}: {e}", path.display()),
                },
            )?;
            return Ok(());
        }
    };

    // Parsing the container is the gate; surveying imports is not. A static binary legitimately
    // imports nothing.
    if let Err(e) = service.inspect_bytes(&bytes) {
        write_message(
            output,
            &Event::Failed {
                error: e.to_string(),
            },
        )?;
        return Ok(());
    }
    write_message(
        output,
        &Event::Reached {
            phase: Phase::ContainerParsed,
        },
    )?;

    let mut reached = Phase::ContainerParsed;
    if service.survey_bytes(&bytes).is_ok() {
        reached = Phase::ImportsResolved;
        write_message(output, &Event::Reached { phase: reached })?;
    }

    place_and_relocate(output, service, &bytes, reached, limits, symbols_db, path)
}

/// Renders the guest's last submission and, when it produced a frame, emits the frame's descriptor.
///
/// Only the ordinary-return path calls this: it owns the protocol stream, so the descriptor crosses
/// it as an `Event::Frame` for a running shim. Endings that stop the process from their own thread
/// call `render::render_and_log_last_submission` alone.
fn render_and_emit_frame<W: Write>(output: &mut W) -> io::Result<()> {
    match render::render_and_log_last_submission() {
        Some(frame) => write_message(output, &frame),
        None => Ok(()),
    }
}

/// Reports that a run stopped, with the reason and the furthest phase reached.
///
/// Every failure funnels through here so no branch returns without a verdict (D010).
fn halt<W: Write>(output: &mut W, reached: Phase, reason: String) -> io::Result<()> {
    write_message(
        output,
        &Event::Terminated {
            outcome: Outcome::Halted { reason },
            reached,
        },
    )
}

/// The two ways a run can be stopped, carried together because they always travel together.
///
/// The budget fixes the call count so a verdict measures the build, and the clock catches a guest
/// that stops calling imports and would otherwise hang (D238).
#[derive(Debug, Clone, Copy, Default)]
struct Limits {
    /// Wall-clock seconds, or `None` for no limit. The backstop.
    seconds: Option<u64>,
    /// Imports the guest may call, or `None` for no budget. The deterministic one.
    calls: Option<u64>,
}

/// Puts the platform's tree under the guest, with this title's own files over it (D251).
///
/// The title is named by the directory holding the module, which is what a person reading
/// `title-data/` recognises it by.
fn install_filesystem(module: &str) {
    let paths = orbistoun_paths::Paths::resolve();
    let title = orbistoun_service::linkplan::title_of(Path::new(module));
    // The sandbox is established as one thing, in orbistoun-fs: this crate supplies where the bytes
    // live and the retention policy, and the fs crate owns the order (D423). The retention default
    // is `Retain`, so what a guest wrote persists; `ORBISTOUN_SANDBOX=ephemeral` empties it each
    // run.
    let retention = match orbistoun_env::SANDBOX.get().as_deref() {
        Some("ephemeral") => orbistoun_fs::sandbox::Retention::Ephemeral,
        _ => orbistoun_fs::sandbox::Retention::Retain,
    };
    // Which tree the title sees is a named per-title setting (`filesystem_view`, in its compat
    // record or the user's override file), never a check on the id. A launcher opts out of its
    // sandbox: it writes into the platform's shared overlay and sees the library at /user/app, as a
    // system application does.
    let settings = orbistoun_overrides::Resolved::for_run(&title, &paths.overrides_dir());
    let system = settings.text(orbistoun_overrides::FILESYSTEM_VIEW)
        == Some(orbistoun_overrides::FILESYSTEM_VIEW_SYSTEM);
    let overlay = if system {
        paths.console_overlay_dir()
    } else {
        paths.title_overlay_dir(&title)
    };
    let origin = origin_of(
        Path::new(module),
        &paths.staged_titles_dir(),
        RUN_STAGED.load(std::sync::atomic::Ordering::Relaxed),
    );
    orbistoun_fs::sandbox::establish(
        &paths.filesystem_dir(),
        &overlay,
        Path::new(module),
        &origin,
        retention,
    );
    if system {
        orbistoun_fs::sandbox::expose_library(
            &paths.filesystem_dir(),
            &installed_titles(&paths.titles_dir()),
        );
    }
}

/// Links a title and stores its plan without entering it (`Request::Link`, D724).
fn link_ahead(service: &Service, path: &Path, symbols_db: Option<&Path>, relink: bool) -> Event {
    let bases = orbistoun_service::TitleBases {
        modules: TITLE_MODULE_BASE,
        thunks: THUNK_TABLE_BASE,
        data: DATA_BLOCK_BASE,
    };
    let symbols = symbol_database(symbols_db);
    match service.link_ahead(path, DEFAULT_MODULE_BASE, bases, &symbols, relink) {
        Ok(summary) => Event::Linked(summary),
        Err(e) => Event::Failed {
            error: e.to_string(),
        },
    }
}

/// Whether the current run asked to replace its stored link plan (`Request::Run::relink`), set from
/// its request and cleared as it ends.
static RUN_RELINK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the current run asked to be staged (`Request::Run::staged`), set from its request and
/// cleared as it ends.
static RUN_STAGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Where a module's title is stored, which decides whether its `/app0` is writable (D722).
///
/// Staged when its directory lies directly under the library's staging tree, or when the run asked
/// for it; an image otherwise. Only the path decides, never anything the title ships. The staged id
/// is the directory's name.
fn origin_of(module: &Path, staging: &Path, asked: bool) -> orbistoun_fs::sandbox::Origin {
    let directory = module.parent();
    let id = directory
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned());
    let under = |candidate: &Path| {
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        candidate.parent().map(canonical) == Some(canonical(staging))
    };
    match (directory, id) {
        (Some(directory), Some(id)) if asked || under(directory) => {
            orbistoun_fs::sandbox::Origin::Staged { id }
        }
        _ => orbistoun_fs::sandbox::Origin::Image,
    }
}

/// Each installed title in the library as `(title id, its directory)`: a directory with an entry
/// module and a declared id. A payload folder with no `param.json` is not an installed title.
fn installed_titles(library: &Path) -> Vec<(String, std::path::PathBuf)> {
    let Ok(entries) = std::fs::read_dir(library) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|directory| {
            directory
                .join(orbistoun_service::TITLE_ENTRY_FILE)
                .is_file()
        })
        .filter_map(|directory| {
            let id = orbistoun_service::read_title_metadata(&directory)?.title_id;
            (!id.is_empty()).then_some((id, directory))
        })
        .collect()
}

/// Connects what a running guest shows and asks for to the front end.
fn install_presentation() {
    // The draws, carried out at submit and written back where the guest reads them, so the fence
    // after them retires from work that ran.
    orbistoun_gpu::agc_driver::install_draw_executor(render::execute_draws);
    // Read back when the drawn frame is written into guest memory: at the flip by default, or after
    // every submission under `ORBISTOUN_TARGET_WRITEBACK=submit` (D714).
    orbistoun_gpu::agc_driver::install_frame_reader(render::read_frame);
    orbistoun_gpu::agc_driver::set_write_back_at_flip(
        orbistoun_env::TARGET_WRITEBACK.get().as_deref() != Some("submit"),
    );
    // A submission's finer spans, when asked for.
    orbistoun_gpu::perf::set_detail(orbistoun_env::PERF_DETAIL.get().as_deref() == Some("1"));
    // A copy out of a colour target still on the device, carried out when its destination is first
    // touched (D717); the fault handler that notices the touch is `report`'s.
    orbistoun_gpu::agc_driver::install_lazy_copies(orbistoun_gpu::agc_driver::LazyCopies {
        snapshot: render::keep_frame_on_device,
        take: render::take_snapshot,
        discard: render::discard_snapshot,
        replace: render::replace_snapshot,
        guard: page_guard::guard,
        protect_writes: page_guard::protect_writes,
        release: page_guard::release,
    });
    // Every flip, shown as it is presented.
    orbistoun_video::install_flip_observer(render::present_flip);
    // A launcher's request to start another title, which the front end carries out.
    orbistoun_systemservice::launch::install_launch_observer(render::request_launch);
}

/// Tells the guest-OS layer what this process actually laid out.
///
/// Told rather than derived: this crate places the stack and the module, and re-deriving either
/// from the constants it was built with can disagree with the real layout (D275).
fn describe_environment(stack: &GuestStack, module: &str) {
    orbistoun_kernel::note_stack_span(stack.lowest_usable(), stack.len());
    orbistoun_kernel::note_loaded_modules(vec![(1, module.to_owned())]);
}

/// Arms both ways a run can be stopped, before the guest can call anything.
///
/// Whichever is reached first stops the run, and the exit status says which. Both are armed: a
/// guest in a tight import loop wastes a clock, and a guest that stops calling imports never
/// reaches a budget (D238).
fn install_limits(limits: Limits, module: &str) {
    if let Some(seconds) = limits.seconds {
        report::start_time_limit(seconds, module.to_owned());
    }
    if let Some(budget) = limits.calls {
        report::start_call_budget(budget, module.to_owned());
    }
}

/// What the guest gets in each import slot, and the title's own modules linked behind them.
///
/// One stub table spans the executable and every module the title ships (D484). The executable is
/// module 0 at offset 0, so its imports keep their slot numbers, and a module answering one of them
/// is a real address in relocated code. Done before relocation, which writes the answer into the
/// slot. The returned value owns the module images and must outlive the run: dropping it unmaps
/// code the executable has pointers into.
fn link_the_title(
    service: &Service,
    path: &Path,
    symbols: &orbistoun_nid::SymbolDbFile,
) -> Result<orbistoun_service::LinkedTitle, Error> {
    service
        .link_title_modules(
            path,
            orbistoun_service::TitleBases {
                modules: TITLE_MODULE_BASE,
                thunks: THUNK_TABLE_BASE,
                data: DATA_BLOCK_BASE,
            },
            symbols,
        )
        .map_err(Error::Link)
}

/// The symbol database this run names imports with.
///
/// Loaded per run rather than at worker start-up, so one request's names never carry into the next.
/// The shipped database is used when none is supplied (D188); a supplied path wins, so a database
/// under construction is testable before it is committed.
fn symbol_database(symbols_db: Option<&Path>) -> orbistoun_nid::SymbolDbFile {
    symbols_db
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| orbistoun_nid::SymbolDbFile::from_json(&text).ok())
        .unwrap_or_else(orbistoun_nid::SymbolDbFile::builtin)
}

/// Records the title's link plan: the executable first, then the modules it ships (D724).
///
/// The digest reaches the run's conditions, so a verdict between two runs that linked differently
/// says so rather than crediting the difference to an implementation. The plan is settled against
/// the one stored in the title library, and on a mismatch the report names what differed.
fn record_link_plan(
    service: &Service,
    image: &Image,
    writes: Vec<orbistoun_loader::plan::SlotWrite>,
    title: &orbistoun_service::LinkedTitle,
    executable: (&Path, &[u8]),
) {
    let (path, bytes) = executable;
    let plan = match orbistoun_service::linkplan::plan_of(image, bytes, writes, title) {
        Ok(plan) => plan,
        Err(e) => {
            tracing::warn!("no link plan for {}: {e}", path.display());
            return;
        }
    };
    let relink = RUN_RELINK.load(std::sync::atomic::Ordering::Relaxed);
    let summary = service.settle_link_plan(executable, &plan, title, relink);
    tracing::info!(
        "link plan {} ({}): {} modules, {} relocation writes, {} raw syscall sites",
        summary.digest,
        summary.stored,
        summary.modules,
        summary.writes,
        summary.syscalls
    );
    if relink {
        if summary.differs.is_empty() {
            tracing::info!(
                "relinked: the stored plan is now {}, and nothing differed",
                summary.digest
            );
        } else {
            tracing::info!(
                "relinked: the stored plan is now {}; what differed:",
                summary.digest
            );
        }
        for line in &summary.differs {
            tracing::info!("  {line}");
        }
    }
    let mut summary = summary;
    // Only a mismatch is a finding; a relink's differences were printed above.
    if summary.stored != orbistoun_loader::plan::Standing::Mismatch.word() {
        summary.differs.clear();
    }
    report::note_link_plan(summary);
}

/// Fills the globals a guest reads without ever calling anything that could fill them.
///
/// Both happen before the guest runs. `_Stdout` and `_Stderr` are data, so nothing the guest calls
/// populates them on demand (D323), and `getenv` answers from the strings the process image was
/// built from.
fn publish_what_the_guest_reads(service: &Service) {
    orbistoun_thunk::install_environment(service.entry_settings().environment.clone());
    orbistoun_libc::streams::install();
}

/// Tells the fault reporter where the title's own code is.
///
/// One span covers every module. Without it, a fault inside a module is outside every region the
/// reporter knows, which is its test for a fault in orbistoun's own code (D489).
fn describe_title_modules(title: &orbistoun_service::LinkedTitle) {
    let images = title.placed.images();
    let (Some((_, first)), Some((_, last))) = (images.first(), images.last()) else {
        return;
    };
    let base = first.span().0;
    let (end_base, end_len) = last.span();
    let len = end_base.saturating_add(end_len).saturating_sub(base);
    report::describe_region(report::Region::TitleModules, base, len);
    // Which library names are the title's own, so an unnamed hash from one is not sent to a vendor
    // word list that cannot contain it.
    report::name_title_modules(title.placed.exports().keys().cloned().collect());
    // Every import is accounted for, bound or not, and printed whether or not anything went wrong
    // (D640).
    for line in title.binding.lines() {
        tracing::info!("{line}");
        orbistoun_core::klog::note(&line);
    }
    report::name_bound_imports(title.bound.keys().map(|i| *i as usize).collect());
    // Named to the reporter and published to the readers, so a watch or dump on a module the title
    // ships reads it rather than answering "in no span this run published as readable". Module
    // bytes are guest material at rest, `static` evidence under `docs/PROVENANCE.md`.
    orbistoun_thunk::note_readable_range(base, len);
}

/// Places an image, links it, protects it, and hands it to the guest.
fn place_and_relocate<W: Write>(
    output: &mut W,
    service: &Service,
    bytes: &[u8],
    reached: Phase,
    limits: Limits,
    symbols_db: Option<&Path>,
    path: &Path,
) -> io::Result<()> {
    // The name a report calls this run, derived here rather than passed beside the path it comes
    // from.
    let module = &path.display().to_string();
    // Modules and the executable link at or near zero, so a placement base is always needed; the
    // null page is never mappable.
    let mut image = match service.place_image(bytes, DEFAULT_MODULE_BASE) {
        Ok(i) => i,
        Err(e) => return halt(output, reached, format!("could not place the image: {e}")),
    };
    write_message(
        output,
        &Event::Reached {
            phase: Phase::Mapped,
        },
    )?;

    let placed = format!(
        "placed {} segments ({} copied, {} zeroed) at {:#x}",
        image.segments().len(),
        image.bytes_copied(),
        image.bytes_zeroed(),
        image.base()
    );

    let database = symbol_database(symbols_db);
    let title = match link_the_title(service, path, &database) {
        Ok(linked) => linked,
        Err(e) => return halt(output, Phase::Mapped, format!("{placed}; {e}")),
    };
    let (thunks, data) = (&title.thunks, &title.data);
    describe_title_modules(&title);
    // The data symbols are published inside the link, across every module at once; a second install
    // here would be ignored (D484).
    publish_what_the_guest_reads(service);
    let (tally, unnameable) =
        match relocate_with_refusals(service, &image, bytes, &title, &database, symbols_db) {
            Ok(linked) => {
                record_link_plan(
                    service,
                    &image,
                    linked.applied.writes,
                    &title,
                    (path, bytes),
                );
                (linked.applied.tally, linked.refused)
            }
            Err(e) => {
                return halt(
                    output,
                    Phase::Mapped,
                    format!("{placed}; relocation failed: {e}"),
                );
            }
        };
    let relocations = describe_relocations(&tally, data);

    // A refusal is not a failure to link: under `ORBISTOUN_RESOLVE=named` the image is exactly as
    // linked as it was asked to be (D392).
    let refused = unnameable
        .as_ref()
        .map_or(0, std::collections::BTreeSet::len);
    let linked = tally.complete() || tally.unresolved == refused;
    let reached = if linked {
        write_message(
            output,
            &Event::Reached {
                phase: Phase::Linked,
            },
        )?;
        Phase::Linked
    } else {
        Phase::Mapped
    };

    // What the executable itself exports, so `sceKernelDlsym` can answer for it (D517). The title's
    // own modules are registered by the loader; nothing else registers this one. A failure costs
    // `dlsym` an answer rather than the run: a binary with no export table is ordinary.
    if let Ok(exports) = service.export_addresses(bytes, image.base()) {
        orbistoun_kernel::note_guest_exports(service.nid_suffix(), &exports);
    }
    let protection = match service.protect_image(&mut image) {
        Ok(p) => p,
        Err(e) => {
            return halt(
                output,
                reached,
                format!("{placed}; {relocations}; protection failed: {e}"),
            );
        }
    };

    let summary = placement_summary(&placed, &relocations, &protection, thunks);

    name_guest_functions_from(bytes);

    prepare_diagnostics(service, module, &image, thunks, limits, &title.labels);

    // Only a fully linked image is safe to enter: an unapplied relocation leaves a pointer that
    // looks valid and is not (D010).
    if reached != Phase::Linked {
        return halt(
            output,
            reached,
            format!("{summary}; not entered - the image is not fully linked"),
        );
    }

    // Only when the run skips the runtime: an ordinary run reaches these globals through the
    // guest's own startup code (D376).
    if service.entry_settings().at.is_some() {
        fill_runtime_globals(&image, bytes, service.entry_settings());
    }

    // A script-driven pad, if this run's controllers name one, starts playing now, the last step
    // before entry, so its clock is the run's own (D707). A malformed script ends the run here.
    if let Err(why) = install_scripted_input(service) {
        return halt(output, Phase::Linked, format!("{summary}; {why}"));
    }

    enter(
        output,
        &image,
        bytes,
        &summary,
        limits,
        module,
        service.entry_settings(),
    )
}

/// Relocates the executable, refusing the imports this run was asked to leave unnamed.
fn relocate_with_refusals(
    service: &Service,
    image: &Image,
    bytes: &[u8],
    title: &orbistoun_service::LinkedTitle,
    database: &orbistoun_nid::SymbolDbFile,
    symbols_db: Option<&Path>,
) -> Result<orbistoun_service::linkplan::ExecutableLink, orbistoun_service::ServiceError> {
    // Which imports this run will refuse, if it was asked to refuse any (D392).
    let refuse = unnameable_imports(service, bytes, symbols_db);
    service.relocate_executable(image, bytes, title, database, refuse)
}

/// Notes the placement and relocation lines and builds the one-line summary of a placed image.
fn placement_summary(
    placed: &str,
    relocations: &str,
    protection: &orbistoun_loader::protect::ProtectionTally,
    thunks: &orbistoun_thunk::ThunkTable,
) -> String {
    orbistoun_core::klog::note(&format!("orbistoun: {placed}"));
    orbistoun_core::klog::note(&format!("orbistoun: {relocations}"));
    format!(
        "{placed}; {relocations}; protected {} runs ({} bytes executable, {} writable, {} both); {} import stubs at {:#x}, {} implemented",
        protection.runs,
        protection.executable,
        protection.writable,
        protection.writable_and_executable,
        thunks.len(),
        thunks.base(),
        orbistoun_thunk::implemented_count_within(thunks.len())
    )
}

/// Arranges everything a run needs in order to explain itself afterwards.
///
/// All of it happens before the guest is entered: names, regions and the trace destination are only
/// useful to a process that is about to fault. Diagnostic tables (dumps, forced writes, forced
/// returns) are sized by the whole stub table, including by-name resolutions, while reports count
/// the guest's own imports (D379).
fn prepare_diagnostics(
    service: &Service,
    module: &str,
    image: &Image,
    thunks: &orbistoun_thunk::ThunkTable,
    limits: Limits,
    labels: &[String],
) {
    let experiments = record_run_conditions(service, limits);

    install_guest_region_lookups();
    install_presentation();

    // Where the trace goes. Named after the module so a sweep over titles leaves one file per
    // title.
    if let Some(paths) = service.paths() {
        report::trace_to(
            paths
                .traces_dir()
                .join(orbistoun_report::trace::trace_file_name(module)),
        );
        report::describe_module(module.to_owned());
        // A rendered frame's region goes beside the trace, where the shim reads it back (D695).
        render::frames_to(paths.traces_dir());
    }

    // Names for every slot of the shared table, not the executable's alone, so a call trace says
    // which function the guest wanted rather than which slot it landed in (D490). A failure here
    // costs labels, not the run.
    if !labels.is_empty() {
        report::name_imports(labels.to_vec());
        // Where each implementation starts, so a fault in orbistoun's own code names the function
        // it landed in (D380).
        report::name_implementations(service.implementation_addresses());
    }

    // Registered before entering, so a fault is attributed to a region rather than left as a bare
    // address; the fault that matters ends the process.
    let (span_base, span_len) = image.span();
    // Imports named for a dump are dumped even though something implements them.
    declare_by_name(thunks, labels);
    arm_dumps(&experiments.dump, thunks.total());

    // After the labels exist, because it resolves an import by name where there is one and by hash
    // where there is not.
    if !experiments.write.is_empty() {
        plant_forced_writes(&experiments, thunks);
    }

    // Consulted before the policy's own answer, and matched the same way as the writes (D166).
    force_returns(&experiments.returns, thunks.total());

    // The memory-query structure carries values that name themselves, so whatever the guest does
    // next says which field it read.
    orbistoun_kernel::mark_query_fields(experiments.mark_query);

    // One line naming every diagnostic in force, so a verdict taken under one is never compared
    // with an ordinary run (D181).
    if !experiments.is_empty() {
        report::note_experiments(experiments.describe());
    }

    report::describe_region(report::Region::Image, span_base, span_len);
    report::describe_region(
        report::Region::Stubs,
        thunks.base(),
        (thunks.total() as u64).saturating_mul(orbistoun_thunk::THUNK_SIZE),
    );
}

/// Records the conditions this run is under and returns the diagnostics it was asked for.
fn record_run_conditions(service: &Service, limits: Limits) -> experiment::Experiments {
    // What this run is subject to, recorded before anything can fault. A comparison between runs is
    // evidence only when these match, and neither the wall-clock limit nor the stub policy shows in
    // any number the run reports (D181).
    let (default_return, overrides, propping) = service.policy_summary();
    // Read once and used for recording the run's conditions, installing each diagnostic and warning
    // about a near-miss variable, so the three cannot disagree (D221).
    let experiments = experiment::Experiments::from_env();

    for name in orbistoun_env::unknown() {
        // A mistyped variable is simply absent and the run reports an ordinary result, so it is
        // warned about.
        tracing::warn!("{name} is not a diagnostic this build understands - ignored");
    }
    report::record_conditions(orbistoun_report::trace::Conditions {
        limit_seconds: limits.seconds,
        call_budget: limits.calls,
        // Filled in when the run ends, once the counts exist.
        did_nothing: Vec::new(),
        default_return,
        overrides,
        propping,
        experiments: experiments.describe(),
        intervened: experiments.intervenes(),
        // Read from the map that was actually built, not the setting that asked for it: a shape
        // that fell back because its regions did not fit is recorded as what the guest got (D357).
        memory_map: orbistoun_kernel::direct::map()
            .lock()
            .map(|m| {
                m.regions()
                    .iter()
                    .map(|r| (r.start, r.end, r.allocated))
                    .collect()
            })
            .unwrap_or_default(),
        // The commit the worker was built from (with `-dirty` for an uncommitted tree), not the
        // crate version, so a run record names the tree that produced it and a regression can be
        // bisected.
        build: orbistoun_env::build::line(),
        // Linking comes later; the report merges the digest in when it collects the trace.
        link_plan: String::new(),
        link_plan_stored: String::new(),
        link_plan_differs: Vec::new(),
        link_syscalls: 0,
    });
    experiments
}

/// Hands the graphics submit path the guest's readable and writable regions.
fn install_guest_region_lookups() {
    // The regions the guest can read, so a submitted command buffer and the shader addresses it
    // names resolve against the guest's own memory (D101). Set before the guest is entered, from
    // the map recorded above.
    if let Ok(map) = orbistoun_kernel::direct::map().lock() {
        orbistoun_gpu::agc_driver::set_guest_regions(
            map.regions()
                .iter()
                .filter(|r| r.allocated)
                .map(|r| (r.start, r.end))
                .collect(),
        );
    }
    // The live answer for everything the guest maps after this point, such as a graphics context's
    // command buffer.
    orbistoun_gpu::agc_driver::install_region_lookup(orbistoun_kernel::is_guest_readable);
    // Where the command processor may write: a fill, a copy, a fence.
    orbistoun_gpu::agc_driver::install_write_lookup(orbistoun_kernel::is_guest_writable);
}

/// Plants the `ORBISTOUN_WRITE` values on every import each clause names.
fn plant_forced_writes(
    experiments: &experiment::Experiments,
    thunks: &orbistoun_thunk::ThunkTable,
) {
    let mut writes: Vec<Vec<orbistoun_thunk::Plant>> = vec![Vec::new(); thunks.total()];
    let mut matched = 0_usize;
    // Counted per clause, not per import, so a clause naming a function that is not there is
    // reported even when the rest match.
    let mut unmatched: Vec<&str> = Vec::new();
    for (target, slot, offset, value) in &experiments.write {
        let mut hits = 0_usize;
        for (index, plants) in writes.iter_mut().enumerate() {
            let Some(label) = report::label_of(index) else {
                continue;
            };
            if target.matches(label) {
                plants.push(orbistoun_thunk::Plant {
                    position: *slot,
                    offset: *offset,
                    value: *value,
                });
                hits += 1;
            }
        }
        if hits == 0 {
            unmatched.push(target.as_str());
        }
        matched += hits;
    }
    for name in &unmatched {
        tracing::warn!("ORBISTOUN_WRITE matched no import called {name:?}");
    }
    if matched == 0 {
        // Said out loud rather than left to be inferred from an unchanged run.
        tracing::warn!("nothing was planted");
    } else {
        orbistoun_thunk::install_forced_writes(
            writes.into_iter().map(Vec::into_boxed_slice).collect(),
        );
    }
}

/// The first argument a guest is started with: its module's name under `/app0`, where the title's
/// own files are mounted. Only the file name of the host path, whichever separator it uses.
fn guest_argument_zero(module: &str) -> String {
    let name = module.rsplit(['/', '\\']).next().unwrap_or(module);
    format!("{}/{name}", orbistoun_fs::mount::APP_MOUNT)
}

/// Writes the initial process stack and answers where the entry point's `rsp` goes.
///
/// The auxiliary vector entries are derived from the loaded image (entry point, load base, page
/// size) rather than configured, so no setting can disagree with the run. Anything not derivable is
/// `extra_auxiliary`, for trying a value.
fn write_process_image(
    image: &Image,
    stack: &GuestStack,
    module: &str,
    settings: &process::EntrySettings,
) -> Result<u64, Error> {
    let mut auxiliary = vec![
        process::AuxEntry {
            kind: process::aux::AT_ENTRY,
            value: image.entry(),
        },
        process::AuxEntry {
            kind: process::aux::AT_BASE,
            value: image.base(),
        },
        process::AuxEntry {
            kind: process::aux::AT_PAGESZ,
            value: orbistoun_core::GUEST_PAGE_SIZE,
        },
    ];
    // AT_PHDR, AT_PHENT and AT_PHNUM are absent: the placed image does not expose where the headers
    // landed, and an invented address would hand the guest a pointer into whatever is there. Absent
    // is a case a program can handle; wrong is not.
    auxiliary.extend(
        settings
            .extra_auxiliary
            .iter()
            .map(|[kind, value]| process::AuxEntry {
                kind: *kind,
                value: *value,
            }),
    );

    let description = process::Description {
        // The module's own name under `/app0`: the host path is a fact about this machine, so only
        // its file name goes in.
        arguments: vec![guest_argument_zero(module)],
        environment: settings.environment.clone(),
        auxiliary,
    };

    let built = process::build(stack.initial_pointer(), stack.len(), &description)
        .ok_or(Error::StackTooSmall)?;

    let Ok(at) = usize::try_from(built.stack_pointer) else {
        return Err(Error::StackPointer(built.stack_pointer));
    };
    // SAFETY: `built.stack_pointer` and the bytes above it lie inside `stack`, which was just
    // reserved as mapped, writable guest memory - `process::build` refuses an image that does not
    // fit. The mapping is identity, so a guest address is a host address.
    unsafe {
        std::ptr::copy_nonoverlapping(
            built.bytes.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            built.bytes.len(),
        );
    }
    Ok(built.stack_pointer)
}

/// Serves the protocol on stdio, as a worker process.
///
/// The whole of worker mode: resolve the real paths, read the run configuration, build a service,
/// speak the protocol. Here rather than in a shim, because every shim needs exactly this and none
/// of it is presentation (D034).
///
/// # Errors
///
/// When the run configuration is malformed, or the protocol stream fails.
pub fn serve_as_worker_process() -> Result<(), Error> {
    // Real paths: the worker is the only process that sees a call trace, and it has to be written
    // where the shims look for it.
    let paths = orbistoun_paths::Paths::resolve();
    let _ = paths.ensure_dirs();
    // A malformed file fails the run instead of falling back quietly: a setting silently ignored is
    // indistinguishable from one that had no effect.
    let file =
        orbistoun_service::FileConfig::load(&paths.config_file()).map_err(Error::Configuration)?;
    // What this machine measured, folded in underneath what a person wrote (D297). A separate file,
    // so deleting it is a complete undo, and absorbed rather than merged, so a deliberate entry
    // wins.
    let learned =
        orbistoun_hle::learned::Learned::load(&paths.learned_file()).map_err(Error::Learned)?;
    let mut policy = file.policy;
    policy.absorb(learned.policy());
    // The regions the shipped knowledge base declares, under both of the above. A region cannot be
    // applied at the call like a `returns` kind, because the service reserves the memory before the
    // guest starts (D300); this is where a knowledge entry's region reaches the dispatcher.
    // Absorbed last, so a person's file and this machine's measurements both win over a shipped
    // assumption.
    policy.absorb(orbistoun_hle::knowledge::Knowledge::builtin().region_policy());

    // The platform settings a person chose in the shell reach the guest here (D346). Read from the
    // worker's own files rather than sent over the protocol, so a setting cannot arrive
    // half-applied.
    let mut settings =
        orbistoun_shell::Settings::load(&paths.shell_file()).map_err(Error::ConsoleSettings)?;
    // A named profile, when one was asked for, replaces the configured machine for this run. The
    // CLI validates it before spawning the worker; a name that does not resolve leaves the
    // configured machine untouched.
    if let Ok(profile) = std::env::var("ORBISTOUN_MACHINE_PROFILE") {
        if let Some(machine) = orbistoun_shell::profiles::machine(&profile) {
            settings.machine = machine;
            tracing::info!("presenting profile {profile}");
        }
    }
    // Which machine this run presents itself as, published to the layer that answers a guest about
    // it. Here because the two crates that need it cannot see each other and this one depends on
    // both (D394).
    let machine = format!(
        "orbistoun: presenting a {} machine",
        settings.machine.describe()
    );
    tracing::info!("presenting a {} machine", settings.machine.describe());
    // And to the kernel log while the guest can still read it: the other reports feeding it run
    // after the guest has stopped (D396).
    orbistoun_core::klog::note(&machine);
    stand_up_firmware(&settings);
    serve_console_gadget();
    orbistoun_core::machine::present(settings.machine.clone());
    orbistoun_systemservice::console::configure(
        settings,
        orbistoun_shell::Parameters::empty(),
        orbistoun_shell::Delivery::empty(),
    );
    let service = Service::new(orbistoun_service::ServiceConfig {
        paths: Some(paths),
        entry_settings: file.entry,
        thread_settings: file.threads,
        memory_settings: with_shape_diagnostic(file.memory),
        stub_policy: policy,
        pads: file.pads,
        ..orbistoun_service::ServiceConfig::default()
    });
    // The handle rather than a lock on it: reading happens on its own thread, and a `StdinLock`
    // holds a `MutexGuard`, which is not `Send`. This process is the only reader.
    //
    // Stdout is unlocked too: a presented frame is announced mid-run from the thread that flips it,
    // which a lock held for the whole run would block. Each message is written in one call
    // (`write_message`), and `Stdout` locks per call, so writers interleave only between whole
    // lines. Stdout is the shim protocol, not a log: protocol messages only, never diagnostics.
    render::stream_events_to(|event| {
        let _ = write_message(&mut io::stdout(), event);
    });
    serve(
        BufReader::new(io::stdin()),
        io::stdout(),
        &service,
        end_orphaned_worker,
    )
    .map_err(Error::WorkerLoop)
}

/// Stands up the firmware skeleton when the run presents a firmware.
fn stand_up_firmware(settings: &orbistoun_shell::Settings) {
    // A run that presents a firmware is prepared to meet a guest that reaches past the named
    // interface into the memory image beneath it (D404). Reserving the region makes that address
    // arithmetic land in mapped, observable memory; a run presenting no firmware pays nothing. A
    // failed reservation is reported and not fatal: those accesses then fault as unmapped.
    if settings.machine.firmware != 0 {
        if let Err(e) = orbistoun_firmware::present() {
            tracing::warn!(
                "could not stand up the firmware skeleton: {e} - firmware accesses will fault as unmapped"
            );
        } else {
            tracing::debug!(
                "firmware skeleton mapped at {:#x}, base handed to guests at {:#x}",
                orbistoun_firmware::FIRMWARE_BASE,
                orbistoun_firmware::handed_base()
            );
        }
    }
}

/// Serves the platform's syscall gadget address with a trampoline into this run's dispatcher.
fn serve_console_gadget() {
    // The platform's own syscall gadget, served whether or not a firmware is presented. A payload
    // routes every system call through a gadget inside libkernel, falling back to a fixed address
    // (`orbistoun_firmware::console_gadget_address`) when it cannot resolve it by name, and
    // ordinary modules reach that path during startup. A trampoline there turns the guest's `callq
    // *<gadget>` into a dispatch-and-return instead of a fault on an unmapped fetch. One page; a
    // failed reservation is reported and not fatal.
    let dispatch: unsafe extern "sysv64" fn(*const u64) -> u64 =
        orbistoun_thunk::syscall::orbistoun_syscall_dispatch;
    match orbistoun_abi::enter::syscall_gadget(
        dispatch as *const () as usize as u64,
        orbistoun_thunk::syscall::SAVED,
    ) {
        Some(gadget) => {
            let trampoline = compact_trampoline(gadget);
            if let Err(e) = orbistoun_firmware::present_console_gadget(&trampoline) {
                tracing::warn!(
                    "could not serve the console syscall gadget: {e} - a payload's fallback syscall path will fault as unmapped"
                );
            } else {
                tracing::debug!(
                    "console syscall gadget served at {:#x}",
                    orbistoun_firmware::console_gadget_address()
                );
            }
        }
        None => {
            tracing::warn!("no syscall gadget built, cannot serve the console gadget address");
        }
    }
}

/// Exit status of a worker that ended because its parent went away (D715).
///
/// Nobody reads it, since the parent would. It is not 0 because the run was abandoned, not
/// finished.
pub const EXIT_ORPHANED: i32 = 3;

/// Ends a worker whose control channel closed without a `Shutdown` (D715).
///
/// The whole process, not just the run: the run is inside guest machine code on another thread that
/// nothing can unwind from outside, as with [`Stopper`]. A run's trace is written as it goes, so
/// ending here keeps everything recorded so far, and an orphaned worker never keeps a guest running
/// or the executable open.
fn end_orphaned_worker() {
    tracing::warn!(
        "worker: the control channel closed without a shutdown - the parent is gone, ending this worker and any run in it"
    );
    std::process::exit(EXIT_ORPHANED);
}

/// What relocation did, for the run summary: the format string, separate from the decisions in
/// `place_and_relocate`.
fn describe_relocations(
    tally: &orbistoun_elf::reloc::RelocationTally,
    data: &orbistoun_thunk::DataBlocks,
) -> String {
    format!(
        concat!(
            "relocations {}/{} applied ({} weak-zero, {} TLS-deferred, {} unsupported, {} unresolved); ",
            "{} imports name data and were given storage rather than a stub"
        ),
        tally.applied,
        tally.total(),
        tally.weak_zero,
        tally.tls_deferred,
        tally.unsupported,
        tally.unresolved,
        data.len()
    )
}

/// Imports this build cannot even name, when a run asked for those to be refused.
///
/// A name this project cannot derive from its symbol database is one no evidence here says exists.
/// Under `named` such an import is left unresolved instead of stubbed, which is what the platform
/// does with a symbol no library exports. Not the default: a run that refuses reaches fewer imports
/// by construction, so the setting records which way a run was made (D392).
fn unnameable_imports(
    service: &Service,
    bytes: &[u8],
    symbols_db: Option<&Path>,
) -> Option<std::collections::BTreeSet<usize>> {
    if orbistoun_env::RESOLVE.get().as_deref() != Some("named") {
        return None;
    }
    let supplied = symbols_db
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| orbistoun_nid::SymbolDbFile::from_json(&text).ok());
    let file = supplied.unwrap_or_else(orbistoun_nid::SymbolDbFile::builtin);
    let Ok(labels) = service.import_labels_with(bytes, &file) else {
        tracing::warn!(
            "asked to refuse unnameable imports, but the import list could not be read - nothing is refused"
        );
        return None;
    };
    // A label this could not name ends in the hash it could not name.
    let refused: std::collections::BTreeSet<usize> = labels
        .iter()
        .enumerate()
        .filter(|(_, label)| {
            label
                .rsplit("::")
                .next()
                .is_some_and(|name| name.starts_with("0x"))
        })
        .map(|(index, _)| index)
        .collect();
    tracing::info!(
        "refusing {} of {} imports this build cannot name, so a guest can tell a symbol that exists from one that does not",
        refused.len(),
        labels.len()
    );
    Some(refused)
}

/// Makes the imports named in `ORBISTOUN_RETURN` answer the values asked for.
///
/// Matched the same way as the plants, by name where there is one and by hash where there is not
/// (D166).
fn force_returns(wanted: &[(experiment::Target, u64)], count: usize) {
    if wanted.is_empty() {
        return;
    }
    let mut forced: Vec<Option<u64>> = vec![None; count];
    let mut matched = 0_usize;
    for (target, value) in wanted {
        let mut hits = 0_usize;
        for (index, answer) in forced.iter_mut().enumerate() {
            let Some(label) = report::label_of(index) else {
                continue;
            };
            if target.matches(label) {
                *answer = Some(*value);
                hits += 1;
            }
        }
        if hits == 0 {
            tracing::warn!(
                "ORBISTOUN_RETURN matched no import called {:?}",
                target.as_str()
            );
        }
        matched += hits;
    }
    if matched == 0 {
        tracing::warn!("no import will answer a forced value");
    } else {
        orbistoun_thunk::install_forced_returns(forced);
    }
}

/// Applies the diagnostics that change guest memory before the guest sees it.
///
/// Both happen after relocation, which writes into static data, and before the entry jump; they
/// share that window and nothing else does.
fn apply_memory_diagnostics(
    image: &Image,
    stack: &GuestStack,
    experiments: &experiment::Experiments,
) {
    // The zero-initialised tail of every segment, filled before the guest can read it.
    //
    // This deliberately breaks the contract that `.bss` is zero: a value the guest reads from a
    // static nothing wrote stops being zero, and the fault moves (D325). Writable segments only.
    if let Some(byte) = experiments.bss_fill {
        let mut filled = 0_u64;
        for segment in image.segments() {
            if segment.zeroed == 0 || !segment.protection().write {
                continue;
            }
            let start = segment.address.saturating_add(segment.copied);
            let (Ok(at), Ok(len)) = (usize::try_from(start), usize::try_from(segment.zeroed))
            else {
                continue;
            };
            // SAFETY: the span the loader zeroed for this segment, in a writable mapping this
            // process made, before the guest has begun, so nothing else can observe it.
            unsafe {
                std::ptr::write_bytes(std::ptr::with_exposed_provenance_mut::<u8>(at), byte, len);
            }
            filled = filled.saturating_add(segment.zeroed);
        }
        // Reported with the count, so a fill that matched no writable segment is visible.
        tracing::info!("filled {filled} bytes of static data with {byte:#04x}");
    }

    // Reserved before anything reads it, and leaked on purpose: an `AddressSpace` dropped at the
    // end of this function would unmap the region before the guest ran.
    if let Some((base, len)) = experiments.map {
        let mut space = orbistoun_mem::AddressSpace::new();
        match space.reserve(base, len, orbistoun_mem::Protection::READ_WRITE) {
            Ok(_) => {
                tracing::info!("reserved {base:#x}+{len:#x} for this run only");
                std::mem::forget(space);
            }
            // Warned rather than inferred from an unchanged fault: a failed reservation and a
            // region the guest did not want look identical from the fault address alone.
            Err(e) => tracing::warn!("could not reserve {base:#x}+{len:#x}: {e}"),
        }
    }

    // Planted after relocation, so the loader cannot overwrite it, and before the entry jump, so
    // the guest reads it. Refused outside a writable segment, where the write would fault inside
    // the emulator.
    if let Some((at, value)) = experiments.poke {
        // Any writable mapping, not just the image, so stack structures are in reach. Read-only
        // text is refused.
        let in_image = image.segments().iter().any(|s| {
            s.protection().write
                && at >= s.address
                && at.saturating_add(8) <= s.address.saturating_add(s.memsz())
        });
        let in_stack = at >= stack.lowest_usable()
            && at.saturating_add(8) <= stack.lowest_usable().saturating_add(stack.len());
        let writable = in_image || in_stack;
        if writable {
            if let Ok(address) = usize::try_from(at) {
                // SAFETY: the check above put eight bytes from `at` inside a writable segment this
                // process mapped, and the guest has not started.
                unsafe {
                    std::ptr::write_unaligned(
                        std::ptr::with_exposed_provenance_mut::<u64>(address),
                        value,
                    );
                }
                tracing::info!("poked {value:#x} into {at:#x}");
            }
        } else {
            // A poke that landed nowhere would otherwise read as a run that changed nothing.
            tracing::warn!("{at:#x} is not inside a writable segment - nothing poked");
        }
    }
}

/// Everything that has to be watching before the guest starts, in the order it has to be.
///
/// Returns whether fault reporting is available on this host, and refuses the run if a requested
/// diagnostic cannot be honoured. The order: the stack is named first, so a fault in it reads as
/// `stack+...`; the stop handler goes in before the guest can call anything, so a guest that stops
/// itself has its trace written (D177); the watchpoints are armed last, on this thread and after
/// the handler that reports their traps, because debug registers belong to one thread. A watchpoint
/// that did not arm refuses the run, since its report would read as nothing touching the address.
fn install_reporting(
    stack: &GuestStack,
    experiments: &experiment::Experiments,
) -> Result<bool, Error> {
    report::describe_region(
        report::Region::Stack,
        stack.guard(),
        stack.len().saturating_add(orbistoun_mem::stack::GUARD_SIZE),
    );
    let reporting = report::install();
    report::install_stop_handler();
    // The filesystem reader, installed into the crate that declares `libkernel`: the asynchronous
    // file path is file I/O under a kernel name, and this crate owns both sibling subsystems.
    orbistoun_kernel::apr::on_file_read(read_guest_file);
    orbistoun_kernel::apr::on_index_lookup(look_up_in_index);

    let armed = experiments
        .watchpoints()
        .and_then(|requests| watchpoint::arm(&requests))
        .map_err(|why| Error::Watchpoints(Box::new(why)))?;
    if !armed.is_empty() {
        tracing::info!("watching {armed}");
    }
    Ok(reporting)
}

/// What the guest finds in its first two argument registers.
///
/// Two, because `main` takes two: `MainArguments` answers for a run entering at `main` rather than
/// at the declared entry, while every other variant fills one register (D343).
fn entry_arguments(
    argument: process::EntryArgument,
    entry_stack: u64,
    named_fields: &[[u64; 2]],
) -> (u64, u64) {
    let argument = overridden_entry_argument().unwrap_or(argument);
    match argument {
        process::EntryArgument::MainArguments => main_arguments(entry_stack),
        process::EntryArgument::ImageAddress => (entry_stack, 0),
        process::EntryArgument::ZeroedBlock => (orbistoun_abi::enter::process_argument_block(), 0),
        process::EntryArgument::Sentinels => (orbistoun_abi::enter::sentinel_argument_block(), 0),
        process::EntryArgument::Answering => (orbistoun_abi::enter::answering_argument_block(), 0),
        process::EntryArgument::Reporting => (orbistoun_abi::enter::reporting_argument_block(), 0),
        process::EntryArgument::Handoff => (handoff_block(named_fields), 0),
        process::EntryArgument::Zero => (0, 0),
    }
}

/// The entry argument a run was told to use, when one was.
///
/// Which argument a guest wants is a fact about the guest, and the wrong one ends the run in the
/// entry function: a payload that reads its first argument as a resolver table pointer calls
/// whatever address an argument count makes. The knob makes the choice reachable from an
/// instrument, so a handoff measurement is made on the block the guest actually sees (D399).
fn overridden_entry_argument() -> Option<process::EntryArgument> {
    match orbistoun_env::ENTRY_ARGUMENT.get().as_deref() {
        Some("handoff") => Some(process::EntryArgument::Handoff),
        Some("main") => Some(process::EntryArgument::MainArguments),
        Some("zero") => Some(process::EntryArgument::Zero),
        Some("zeroed") => Some(process::EntryArgument::ZeroedBlock),
        Some("image") => Some(process::EntryArgument::ImageAddress),
        Some("sentinels") => Some(process::EntryArgument::Sentinels),
        Some("answering") => Some(process::EntryArgument::Answering),
        Some("reporting") => Some(process::EntryArgument::Reporting),
        // An unrecognised value is refused rather than silently the default.
        Some(other) => {
            tracing::warn!(
                "{other} is not an entry argument this knows - the configured one stands"
            );
            None
        }
        None => None,
    }
}

/// What orbistoun puts in a named global when it has something better than a stub.
///
/// `ptr_syscall` is the slot the open-toolchain runtime keeps a syscall gadget in, and orbistoun
/// has one. A global rather than a relocation changes where the answer is written, not what it
/// means (D378). A table rather than an `if`, so each name states its own promise.
fn runtime_gadget(name: &str, entry: &process::EntrySettings) -> Option<u64> {
    match name {
        // The slot the runtime keeps a syscall gadget in, and orbistoun has one.
        "ptr_syscall" => {
            let dispatch: unsafe extern "sysv64" fn(*const u64) -> u64 =
                orbistoun_thunk::syscall::orbistoun_syscall_dispatch;
            orbistoun_abi::enter::syscall_gadget(
                dispatch as *const () as usize as u64,
                orbistoun_thunk::syscall::SAVED,
            )
        }
        // The structure the runtime was handed, in the global it keeps it in: `payload_args` holds
        // what the entry point received, which a run entering past the entry point never received.
        // The same block the declared-entry path gets, so both modes agree and its unestablished
        // fields are markers (D365).
        "payload_args" => Some(handoff_block(&entry.handoff_fields)),
        _ => None,
    }
}

/// Writes one word into a global, if it lies inside the image this run mapped.
fn write_global(image: &Image, address: u64, value: u64) {
    let at = image.base().saturating_add(address);
    let (span_base, span_len) = image.span();
    if at < span_base || at.saturating_add(8) > span_base.saturating_add(span_len) {
        return;
    }
    let Ok(destination) = usize::try_from(at) else {
        return;
    };
    // SAFETY: the bounds check above established that eight bytes at `at` lie inside the image this
    // run mapped writable, and the guest has not started.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u64>(destination),
            value,
        );
    }
}

/// Where the markers for globals nothing implements start.
///
/// Its own range, distinct from the handoff structure's, so a fault address says which it belongs
/// to.
const UNSERVED_GLOBAL_BASE: u64 = 0x0000_5E29_0000_0000;

/// How far apart consecutive ones sit, so a small displacement still lands inside its own.
const UNSERVED_GLOBAL_STRIDE: u64 = 0x1000;

/// Fills the named globals a skipped runtime would have filled.
///
/// `[entry] at` starts a guest past its own startup code, which `orbistoun-env` records as not an
/// ordinary run; without this, every C library pointer is null. The open-toolchain runtime resolves
/// its C library at startup by name and stores each answer in a named `.bss` global, so this does
/// the same resolution from the program's own symbol table, answering with the stub the linker
/// would have written for an import of that name (D376). Every fill is reported. A name nothing
/// implements gets a marker, or keeps the loader's null under `ORBISTOUN_RUNTIME_GLOBALS=zero`.
fn fill_runtime_globals(image: &Image, bytes: &[u8], entry: &process::EntrySettings) {
    let Ok(container) = orbistoun_elf::Container::parse(bytes) else {
        return;
    };
    let Ok(globals) = container.named_globals(bytes) else {
        return;
    };
    if globals.is_empty() {
        return;
    }

    // A name that names several slots does not name a function: those are statics sharing a name
    // with one, and filling them would write a stub address into unrelated state. Only names that
    // occur once are filled (D378).
    let mut seen: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for global in &globals {
        *seen.entry(global.name.as_str()).or_default() += 1;
    }

    let mut filled = 0_usize;
    let mut ambiguous = 0_usize;
    let mut unserved: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut marked: Vec<(u64, String)> = Vec::new();

    // `ORBISTOUN_RUNTIME_GLOBALS` either says `zero`, leaving every unserved global holding the
    // null the loader left it, or names globals to watch. A watched global gets a gadget stub that
    // reports every register it was called with, `rax` included, which is the only way to see a
    // syscall number (D377). Watching is by name rather than blanket, because a stub in a data
    // global changes what the guest computes.
    let leave_unserved_null = orbistoun_env::RUNTIME_GLOBALS.get().as_deref() == Some("zero");
    let watched: Vec<String> = orbistoun_env::RUNTIME_GLOBALS
        .get()
        .filter(|value| value != "zero")
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let mut stubs = orbistoun_abi::enter::gadget_stubs(watched.len()).into_iter();
    let mut gadget_names: Vec<String> = Vec::new();
    for (position, global) in globals.iter().enumerate() {
        if global.size < 8 {
            continue;
        }
        if seen.get(global.name.as_str()).copied().unwrap_or(0) > 1 {
            ambiguous += 1;
            continue;
        }
        // A global orbistoun can serve with something other than a stub: the slot a payload keeps
        // its syscall gadget in.
        if let Some(gadget) = runtime_gadget(&global.name, entry) {
            write_global(image, global.address, gadget);
            filled += 1;
            continue;
        }
        let stub = orbistoun_thunk::name_thunk(&global.name).unwrap_or_else(|| {
            unserved.insert(global.name.clone());
            if watched.contains(&global.name)
                && let Some(gadget) = stubs.next()
            {
                gadget_names.push(global.name.clone());
                return gadget;
            }
            if leave_unserved_null {
                // Zero, which is what `.bss` holds: a marker in a global a `while` loop reads would
                // be a diagnostic changing the program (D227).
                return 0;
            }
            // Numbered by where the global sits, not by how many markers came before it, so
            // watching one global never renumbers the others.
            let says = UNSERVED_GLOBAL_BASE + (position as u64) * UNSERVED_GLOBAL_STRIDE;
            marked.push((says, global.name.clone()));
            says
        });
        let at = image.base().saturating_add(global.address);
        let (span_base, span_len) = image.span();
        // Inside the image this run mapped, with room for the whole pointer: a symbol table is not
        // a promise about where anything landed.
        if at < span_base || at.saturating_add(8) > span_base.saturating_add(span_len) {
            continue;
        }
        let Ok(destination) = usize::try_from(at) else {
            continue;
        };
        // SAFETY: the bounds check established that eight bytes at `at` lie inside the image this
        // run mapped writable, and the guest has not started.
        unsafe {
            std::ptr::write_unaligned(
                std::ptr::with_exposed_provenance_mut::<u64>(destination),
                stub,
            );
        }
        filled += 1;
    }

    report_globals_filled(&Filled {
        resolved: filled,
        total: globals.len(),
        ambiguous,
        gadget_names,
        marked,
    });
}

/// What one run's globals fill did, for the report that follows it.
struct Filled {
    /// How many were resolved to the stub the runtime would have written.
    resolved: usize,
    /// How many named globals the image has.
    total: usize,
    /// How many were left alone because their names are not unique.
    ambiguous: usize,
    /// Which ones this run asked to watch, and now hold a reporting stub.
    gadget_names: Vec<String>,
    /// Which ones hold a marker, and what it says.
    marked: Vec<(u64, String)>,
}

/// Says what the fill did, so a guest initialised differently from an ordinary run says so.
fn report_globals_filled(what: &Filled) {
    tracing::info!(
        "entering past the runtime, so {} of {} named globals were resolved the way it would have resolved them",
        what.resolved,
        what.total
    );
    if what.ambiguous > 0 {
        tracing::warn!(
            "{} were left alone because their names are not unique - a name that names several slots does not name a function",
            what.ambiguous
        );
    }
    if !what.gadget_names.is_empty() {
        tracing::info!(
            "{} named by this run hold a stub that reports how it was called",
            what.gadget_names.len()
        );
        orbistoun_abi::enter::install_global_names(what.gadget_names.clone());
    }
    for (says, name) in &what.marked {
        tracing::info!(
            "{name} names nothing implemented - it holds {says:#x}, so a use of it faults on an address that says which it was"
        );
    }
}

/// A compact absolute trampoline (`mov r11, <target>; jmp r11`, 13 bytes).
fn compact_trampoline(target: u64) -> [u8; 13] {
    let mut code = [0u8; 13];
    code[0..2].copy_from_slice(&[0x49, 0xBB]); // mov r11, imm64
    code[2..10].copy_from_slice(&target.to_le_bytes());
    code[10..13].copy_from_slice(&[0x41, 0xFF, 0xE3]); // jmp r11
    code
}

/// The libkernel exports laid out this run, so the unimplemented-export handler can name the one a
/// guest called from the vaddr its stub passes. Set once at layout, read on faults.
static LIBKERNEL_LAYOUT: std::sync::OnceLock<Vec<(String, u64)>> = std::sync::OnceLock::new();

/// Records an unimplemented libkernel export a guest reached, once per distinct name.
///
/// The work list: an export reached by `base + vaddr` with a known vaddr and no implementation. The
/// stub passes its own vaddr in `rdi`, which maps back to the name here (D407).
extern "sysv64" fn unimplemented_libkernel_export(vaddr: u64) -> u64 {
    let name = LIBKERNEL_LAYOUT
        .get()
        .and_then(|layout| {
            layout
                .iter()
                .find(|(_, v)| *v == vaddr)
                .map(|(n, _)| n.as_str())
        })
        .unwrap_or("unknown");
    if first_time_unimplemented_export(vaddr) {
        tracing::warn!("payload called unimplemented libkernel export {name} at vaddr {vaddr:#x}");
        orbistoun_core::klog::note(&format!(
            "orbistoun: unimplemented libkernel export {name} at {vaddr:#x}"
        ));
    }
    u64::from(orbistoun_core::GuestError::Unimplemented.as_raw())
}

/// Whether this export vaddr has not been reported before, so the work list is one line per
/// export rather than one per call.
fn first_time_unimplemented_export(vaddr: u64) -> bool {
    static SEEN: std::sync::Mutex<Option<std::collections::BTreeSet<u64>>> =
        std::sync::Mutex::new(None);
    let Ok(mut seen) = SEEN.lock() else {
        return false;
    };
    seen.get_or_insert_with(Default::default).insert(vaddr)
}

/// getpid's export slot, compact enough to fit the real libkernel packing.
///
/// The measured export table packs functions 0x20 bytes apart (getpid `0x5b0`, mount `0x5d0`,
/// unmount `0x5f0`), so a 64-byte thunk would overrun its neighbours. The 23-byte slot keeps byte
/// 10 a jump to the syscall gadget, because a payload calls getpid's address plus ten for every
/// system call (D400):
///
/// - offset 0: `mov eax, 20` (`SYS_getpid`) then padding to offset 10, so a plain call to getpid
///   falls into the gadget with syscall 20;
/// - offset 10: `mov r11, gadget; jmp r11`, so getpid+10 reaches the gadget with whatever number
///   the caller set.
///
/// The gadget address is lifted from getpid's own thunk.
fn getpid_export_slot(gadget: u64) -> [u8; 23] {
    let mut code = [0x90u8; 23]; // nop-filled, so offsets 5..10 fall through
    code[0] = 0xB8; // mov eax, imm32
    code[1..5].copy_from_slice(&20u32.to_le_bytes());
    code[10..12].copy_from_slice(&[0x49, 0xBB]); // mov r11, imm64
    code[12..20].copy_from_slice(&gadget.to_le_bytes());
    code[20..23].copy_from_slice(&[0x41, 0xFF, 0xE3]); // jmp r11
    code
}

/// A stub that records which export it is, then answers unimplemented.
///
/// `mov edi, vaddr` (the vaddr fits in 32 bits, and `edi` zero-extends into `rdi`, the handler's
/// first argument), then an absolute jump to the handler, which is too far for a relative one.
fn unimplemented_export_stub(vaddr: u64, handler: u64) -> [u8; 18] {
    let mut code = [0u8; 18];
    code[0] = 0xBF; // mov edi, imm32
    code[1..5].copy_from_slice(&(vaddr as u32).to_le_bytes());
    code[5..7].copy_from_slice(&[0x49, 0xBB]); // mov r11, imm64
    code[7..15].copy_from_slice(&handler.to_le_bytes());
    code[15..18].copy_from_slice(&[0x41, 0xFF, 0xE3]); // jmp r11
    code
}

/// Lays out libkernel in the firmware region and answers `getpid`'s address, or `None`.
///
/// A payload loader hands a payload `getpid`'s address as `payload_args[0]` and resolves nothing
/// else; the payload computes `libkernel_base = args[0] - 0x5b0` and reaches every other export at
/// `base + vaddr` (D407). So each export with a measured vaddr is stubbed at `LIBKERNEL_BASE +
/// vaddr`, and `getpid`'s address in that layout becomes word zero. The stub bytes are the
/// function's own position-independent thunk, copied. `None` when no firmware region is reserved,
/// which leaves the resolver in word zero.
fn libkernel_word0() -> Option<u64> {
    if !orbistoun_firmware::is_present() {
        return None;
    }
    let exports: Vec<(String, u64)> = orbistoun_firmware::libkernel_exports()
        .iter()
        .map(|(n, v)| ((*n).to_owned(), *v))
        .collect();
    // The layout is decided by a pure planner and only placed here, so a stub overrunning its
    // neighbour is a unit-test failure rather than a corrupted guest (D407).
    let plan = orbistoun_firmware::plan_layout(&exports, |name| {
        orbistoun_thunk::name_thunk(name).is_some()
    });

    // The table the unimplemented-export handler names a called export from. Set before any stub
    // can run.
    let _ = LIBKERNEL_LAYOUT.set(plan.iter().map(|p| (p.name.clone(), p.vaddr)).collect());

    let unimpl_target = unimplemented_libkernel_export as *const () as usize as u64;
    let mut collisions = 0_usize;

    for placement in &plan {
        if let Some((next_name, next_vaddr)) = &placement.collides_with {
            collisions += 1;
            tracing::warn!(
                "libkernel {} at {:#x} overruns {next_name} at {next_vaddr:#x}",
                placement.name,
                placement.vaddr
            );
        }

        let code_bytes: Vec<u8> = match placement.kind {
            orbistoun_firmware::SlotKind::Anchor => getpid_anchor_bytes()?,
            orbistoun_firmware::SlotKind::Trampoline => {
                // The planner established that this name has a thunk; if it does not, an
                // unimplemented stub fills the slot.
                match orbistoun_thunk::name_thunk(&placement.name) {
                    Some(thunk_addr) => compact_trampoline(thunk_addr).to_vec(),
                    None => unimplemented_export_stub(placement.vaddr, unimpl_target).to_vec(),
                }
            }
            orbistoun_firmware::SlotKind::Unimplemented => {
                unimplemented_export_stub(placement.vaddr, unimpl_target).to_vec()
            }
        };

        if let Err(e) = orbistoun_firmware::place_export(placement.vaddr, &code_bytes) {
            tracing::warn!("could not lay out libkernel {}: {e}", placement.name);
            return None;
        }
    }

    tracing::debug!(
        "libkernel laid out ({} exports, {collisions} collisions), getpid at {:#x} handed as payload_args[0]",
        plan.len(),
        orbistoun_firmware::getpid_address()
    );
    Some(orbistoun_firmware::getpid_address())
}

/// The compact getpid anchor slot, with the syscall gadget lifted from getpid's own thunk.
///
/// `None` if getpid has no thunk, which would mean no syscall gadget to anchor the scheme on.
fn getpid_anchor_bytes() -> Option<Vec<u8>> {
    let thunk_addr = orbistoun_thunk::name_thunk("getpid").or_else(|| {
        tracing::warn!("no thunk for getpid, cannot lay out the syscall gadget");
        None
    })?;
    let at = usize::try_from(thunk_addr).ok()?;
    // SAFETY: `name_thunk` answers the address of a live thunk this process built, exactly
    // `THUNK_SIZE` bytes long and readable.
    let slice = unsafe {
        std::slice::from_raw_parts(
            std::ptr::with_exposed_provenance::<u8>(at),
            orbistoun_thunk::THUNK_SIZE as usize,
        )
    };
    // The gadget address the landing zone embedded, lifted so the compact slot reaches the same
    // code.
    let gadget_at = orbistoun_thunk::LANDING_END + 2;
    let mut gadget_bytes = [0u8; 8];
    gadget_bytes.copy_from_slice(&slice[gadget_at..gadget_at + 8]);
    Some(getpid_export_slot(u64::from_le_bytes(gadget_bytes)).to_vec())
}

/// The pointer fields a payload loader hands a payload, backed by this project's own mapped memory.
///
/// Measured on the hardware (D408): word zero is getpid; words one, two and five are userland
/// pointers; words three and four are kernel pointers (a kernel-heap address and the kernel base);
/// six onward are null. The kernel values are high-half addresses a user process cannot map, so
/// this hands a pointer of the right shape at each field: non-null where the hardware's was, null
/// where it was null, and backed by mapped firmware memory so a payload that reads through a field
/// finds zeroes.
///
/// The fields, by the public loader contract:
/// - word 0: `getpid` function pointer
/// - word 1: `rwpipe`, pointer to `int[2]` pipe descriptors
/// - word 2: `rwpair`, pointer to `int[2]` socket descriptors
/// - word 3: `kpipe_addr`, kernel pointer
/// - word 4: `kdata_base_addr`, kernel base
/// - word 5: `payloadout`, pointer to int (0)
/// - words 6+: 0
///
/// The offsets between the fields are this project's: distinct so a fault names the field, and
/// spread so a small displacement stays inside one.
fn measured_handoff_fields() -> Vec<[u64; 2]> {
    let base = orbistoun_firmware::FIRMWARE_BASE;
    let rwpipe_offset = 0x0010_0000_u64;
    let rwpair_offset = 0x0020_0000_u64;
    let payloadout_offset = 0x0050_0000_u64;

    // The rwpipe buffer: pipe read and write descriptors (int[2]).
    let pipe_fds: [i32; 2] = [3, 4];
    let mut pipe_bytes = [0u8; 8];
    pipe_bytes[0..4].copy_from_slice(&pipe_fds[0].to_le_bytes());
    pipe_bytes[4..8].copy_from_slice(&pipe_fds[1].to_le_bytes());
    let _ = orbistoun_firmware::place_export(rwpipe_offset, &pipe_bytes);

    // The rwpair buffer: two socket descriptors (int[2]).
    let sock_fds: [i32; 2] = [5, 6];
    let mut sock_bytes = [0u8; 8];
    sock_bytes[0..4].copy_from_slice(&sock_fds[0].to_le_bytes());
    sock_bytes[4..8].copy_from_slice(&sock_fds[1].to_le_bytes());
    let _ = orbistoun_firmware::place_export(rwpair_offset, &sock_bytes);

    // The payloadout buffer, holding 0 (int).
    let payloadout_val: i32 = 0;
    let _ = orbistoun_firmware::place_export(payloadout_offset, &payloadout_val.to_le_bytes());

    vec![
        [1, base + rwpipe_offset],
        [2, base + rwpair_offset],
        // Fields three and four carry the measured high-half kernel pointers: a payload validates
        // `kpipe_addr >> 48 != 0` before attempting kernel access.
        [3, 0xffff_8661_5c60_7840],
        [4, 0xffff_ffff_8c29_0000],
        [5, base + payloadout_offset],
    ]
}

/// The handoff structure, with the resolver in the field a payload calls first.
///
/// When nothing implements the resolver this says so, rather than handing over a null the payload
/// calls before anything else (D365).
fn handoff_block(named_fields: &[[u64; 2]]) -> u64 {
    // `getpid` at word zero when a firmware is present, the resolver otherwise; see
    // `libkernel_word0`.
    let resolver = libkernel_word0()
        .or_else(|| orbistoun_thunk::name_thunk("sceKernelDlsym"))
        .unwrap_or_else(|| {
            tracing::warn!(
                "nothing implements sceKernelDlsym, so the handoff structure has no resolver to offer"
            );
            0
        });
    // Unknown fields hold markers by default, because naming a field is the point. A firmware run
    // with nothing overridden mirrors the measured layout instead (D408): unknown fields are zero,
    // as on the hardware, so a payload expecting a null does not branch on a marker. A run can also
    // ask for zeroes, and `orbistoun-env` records that it is not ordinary.
    let firmware_present = orbistoun_firmware::is_present();
    let explicit_fields = orbistoun_env::HANDOFF_FIELDS.get();
    let unknown = match explicit_fields.as_deref() {
        Some("zero") => orbistoun_abi::enter::UnknownFields::Zero,
        Some("deep") => orbistoun_abi::enter::UnknownFields::Markers {
            base: mapped_unknown_fields_marked(),
        },
        // Member stubs: what a field points at is a table of stubs, so a call through a member is
        // as legible as the entry point's call through field zero.
        Some("members") => orbistoun_abi::enter::UnknownFields::Markers {
            base: mapped_unknown_fields_stubbed(),
        },
        // Unmapped markers: any use of a field stops the run at an address that names it. The only
        // mode that says which field a guest read through; a mapped region makes that read succeed
        // silently.
        Some("strict") => orbistoun_abi::enter::UnknownFields::Markers {
            base: orbistoun_abi::enter::SENTINEL_BASE,
        },
        // The measured default: null unknowns for a firmware run, markers otherwise.
        _ if firmware_present => orbistoun_abi::enter::UnknownFields::Zero,
        _ => orbistoun_abi::enter::UnknownFields::Markers {
            base: mapped_unknown_fields(),
        },
    };
    // The measured pointers for fields one to five, unless a run named its own fields or set
    // `HANDOFF_FIELDS`, which are deliberate experiments and win. One poisoned field is applied
    // last, so it wins over everything else: it asks whether the runtime touches that field at all.
    let mut named_fields =
        if firmware_present && explicit_fields.is_none() && named_fields.is_empty() {
            measured_handoff_fields()
        } else {
            named_fields.to_vec()
        };
    if let Some((field, value)) = poisoned_field() {
        named_fields.retain(|[at, _]| *at != field);
        named_fields.push([field, value]);
    }
    orbistoun_abi::enter::handoff_argument_block(resolver, unknown, &named_fields)
}

/// Where a poisoned handoff field points.
///
/// Its own base, distinct from every marker range, so a fault address reads as a poisoned field.
pub const POISON_BASE: u64 = 0x0000_5E2A_0000_0000;

/// How far apart consecutive poisoned fields sit.
///
/// Sixteen bytes: a small displacement stays inside its own field's value, and the field number is
/// legible in the fault address.
pub const POISON_STRIDE: u64 = 16;

/// The field this run poisoned, and what it holds.
///
/// A run that faults on the value used the field; a run that faults elsewhere, or not at all, never
/// reached it. One field per run, because the first fault would hide the rest (D390).
fn poisoned_field() -> Option<(u64, u64)> {
    let raw = orbistoun_env::HANDOFF_POISON.get()?;
    let field: u64 = raw.trim().parse().ok()?;
    let value = POISON_BASE + field * POISON_STRIDE;
    tracing::info!(
        "handoff field {field} holds {value:#x}, which nothing maps - a fault on it means the runtime used the field"
    );
    Some((field, value))
}

/// The same region, with every word naming where it was read from.
///
/// A zeroed page answers zero, which names nothing; content markers name the field and offset, so a
/// runtime reading a pointer out of the structure faults on something that describes the member
/// (D365).
fn mapped_unknown_fields_marked() -> u64 {
    let base = mapped_unknown_fields();
    let len = (orbistoun_abi::enter::ARGUMENT_BLOCK_SIZE as u64 / 8)
        * orbistoun_abi::enter::SENTINEL_STRIDE;
    // SAFETY: `mapped_unknown_fields` reserved exactly this region read-write and leaked the
    // reservation, so it stays mapped and owned for as long as the guest runs.
    unsafe { orbistoun_abi::enter::fill_with_content_markers(base, len) };
    base
}

/// The same region, with every word a stub that says how it was called.
///
/// A marker says the guest read a member but not what it did with it: using it as a function
/// pointer ends the run on an unmapped address with the arguments gone.
fn mapped_unknown_fields_stubbed() -> u64 {
    let base = mapped_unknown_fields();
    let len = (orbistoun_abi::enter::ARGUMENT_BLOCK_SIZE as u64 / 8)
        * orbistoun_abi::enter::SENTINEL_STRIDE;
    // SAFETY: `mapped_unknown_fields` reserved exactly this region read-write and leaked the
    // reservation, so it stays mapped and owned for as long as the guest runs.
    unsafe { orbistoun_abi::enter::fill_with_member_stubs(base, len) };
    base
}

/// Backing for the handoff fields nothing has established yet.
///
/// Mapped and zeroed rather than absent: a runtime reading an unknown field as a pointer gets a
/// null it can check, and the address it read from still names the field, because the region starts
/// at the base the markers are decoded against (D365). Falls back to the unmapped base when the
/// host refuses the reservation.
fn mapped_unknown_fields() -> u64 {
    use orbistoun_mem::{AddressSpace, Protection};
    static REGION: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *REGION.get_or_init(|| {
        let base = orbistoun_abi::enter::SENTINEL_BASE;
        let len = (orbistoun_abi::enter::ARGUMENT_BLOCK_SIZE as u64 / 8)
            * orbistoun_abi::enter::SENTINEL_STRIDE;
        let mut space = AddressSpace::new();
        match space.reserve(base, len, Protection::READ_WRITE) {
            Ok(_region) => {
                // Leaked deliberately: the guest holds addresses inside it for as long as it runs.
                std::mem::forget(space);
                base
            }
            Err(e) => {
                tracing::warn!(
                    "could not map the handoff structure's unknown fields ({e}) - they stay unmapped markers"
                );
                base
            }
        }
    })
}

/// `argc` and `argv`, read out of the process image already written to the guest stack.
///
/// The System V process image begins with the argument count, followed by the argument pointers, so
/// `main` wants the first word and the address just past it. Read rather than constructed, so the
/// two cannot disagree.
fn main_arguments(entry_stack: u64) -> (u64, u64) {
    let Ok(at) = usize::try_from(entry_stack) else {
        return (0, 0);
    };
    // SAFETY: `entry_stack` is the start of a process image this run just wrote inside a
    // mapped, writable guest stack, so its first word is initialised and readable.
    let argc = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at)) };
    (argc, entry_stack.saturating_add(8))
}

/// Where this run actually starts.
///
/// Normally the address the container declares. A diagnostic may name somewhere else, such as
/// `main` past a runtime start that rejects what orbistoun hands it; `Some(0)` is a real request,
/// since an image's first byte can be `main` (D343). Logged when it is not the declared entry,
/// because every later number is then about a program that did not start where its container says.
fn starting_address(image: &Image, settings: &process::EntrySettings) -> u64 {
    let Some(at) = settings.at else {
        return image.entry();
    };
    let entry = image.base().saturating_add(at);
    if entry != image.entry() {
        tracing::info!(
            "entering at {entry:#x} (image+{at:#x}), not the declared entry {:#x}",
            image.entry()
        );
    }
    entry
}

/// Where the main thread's thread-local block is reserved.
///
/// A separate arena from the image (`0x4000...`), the guest stack and the mapping arena, so a stray
/// thread pointer is recognisable by its address.
const MAIN_TLS_BASE: u64 = 0x0000_6900_0000_0000;

/// Where spawned threads' thread-local blocks are reserved, one after another.
///
/// Its own arena, clear of the main block (`0x6900...`), the thread stacks (`0x6100...`), the
/// reentrant stacks (`0x6800...`) and the mapping arena (`0x7400...`), so a stray thread pointer
/// names its kind by its address.
const THREAD_TLS_BASE: u64 = 0x0000_6A00_0000_0000;

/// The next spawned-thread block base, bump-allocated and never reused.
static NEXT_THREAD_TLS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(THREAD_TLS_BASE);

/// The thread-local template (layout and initialised `.tdata`), captured when the main thread's
/// block is built so a spawned thread builds its own without re-parsing the image. `Some(None)`
/// once a run with no thread-locals has set up, which makes the hook a no-op.
static TLS_TEMPLATE: std::sync::OnceLock<Option<(orbistoun_loader::tls::TlsLayout, Vec<u8>)>> =
    std::sync::OnceLock::new();

/// Builds a thread-local block for the calling thread at `base_arena`, installs its `fs` base, and
/// remembers it for the backstop: the effectful half both the main thread and every spawned one
/// need. The template (`layout`, `tdata`) is the same for every thread of a run.
fn build_tls_block(
    base_arena: u64,
    layout: &orbistoun_loader::tls::TlsLayout,
    tdata: &[u8],
) -> Result<u64, Error> {
    let alloc = layout.allocation_size();
    let alloc_len = usize::try_from(alloc).map_err(|_| Error::TlsTooLarge)?;
    let region = GuestStack::reserve(base_arena, alloc).map_err(Error::TlsReserve)?;
    let base = region.lowest_usable();

    // SAFETY: `region` is freshly reserved and committed read-write at `base` for `alloc_len`
    // bytes, and it is leaked below, so the block outlives every read the guest makes through it.
    let dest = unsafe { std::slice::from_raw_parts_mut(base as *mut u8, alloc_len) };
    let tp = layout.render_block(base, dest, tdata);
    std::mem::forget(region);

    // SAFETY: sets the calling thread's `fs` base to the block just built, which is what that
    // thread reads its thread-locals through.
    unsafe { orbistoun_abi::thread_pointer::install(tp) }.map_err(Error::ThreadPointerInstall)?;
    match orbistoun_abi::thread_pointer::current() {
        Some(v) if v == tp => {
            // Remembered so the fault handler can restore it after a host context switch drops it
            // (D433).
            tls_backstop::remember(tp);
            Ok(tp)
        }
        other => Err(Error::ThreadPointerMismatch {
            read: other,
            written: tp,
        }),
    }
}

/// Installs the guest thread pointer for the thread about to enter, when the image declares
/// thread-local storage.
///
/// Reserves a block, copies the `.tdata` init image the loader already placed at `image base +
/// vaddr`, lays out the block variant II with the self-pointer at the thread pointer (D433), and
/// installs the `fs` base, reading it back to confirm. `Ok(None)` when the image declares no
/// thread-locals. The reservation is leaked, because the block must outlive the guest. A host that
/// resets the user `fs` base on a context switch drops it; the backstop restores it.
fn install_main_thread_tls(image: &Image, bytes: &[u8]) -> Result<Option<u64>, Error> {
    let Some((layout, _index, vaddr)) =
        orbistoun_loader::tls::layout_of(bytes).map_err(Error::TlsLayout)?
    else {
        // Remembered as "no thread-locals" so a spawned thread's hook has a definite answer.
        let _ = TLS_TEMPLATE.set(None);
        return Ok(None);
    };

    // The init image, copied out of the placed image. Only `init_size` bytes are `.tdata`; the rest
    // of the block is `.tbss`, which `render_block` zeroes.
    let init = usize::try_from(layout.init_size).unwrap_or(0);
    let mut tdata = vec![0_u8; init];
    if init > 0 {
        let source = image.base().saturating_add(vaddr);
        // SAFETY: `source` is inside the placed, populated image (the loader copied `.tdata` there
        // during placement), and `init` is the header's own init size, so the range is within it.
        let placed = unsafe { std::slice::from_raw_parts(source as *const u8, init) };
        tdata.copy_from_slice(placed);
    }

    // Kept so a spawned thread can build its own block from it.
    let _ = TLS_TEMPLATE.set(Some((layout, tdata.clone())));
    // Registers the hook the kernel calls at the top of every new guest thread, now that there is a
    // template for it to read.
    orbistoun_kernel::thread::install_thread_start(set_up_this_threads_tls);
    build_tls_block(MAIN_TLS_BASE, &layout, &tdata).map(Some)
}

/// Builds a thread-local block for the calling spawned guest thread, the per-thread half of what
/// [`install_main_thread_tls`] does for the main one. Installed as the kernel's thread-start hook,
/// so it runs before any guest code on each new thread.
///
/// A plain `fn` with no captures, so the kernel can call it; it reads the template the main-thread
/// setup stored. Silent when the image declared no thread-locals. A failure leaves a thread that
/// faults on its first `fs:`-relative access, so it is logged.
fn set_up_this_threads_tls() {
    // The run's watchpoints go with every guest thread, not only the first.
    watchpoint::arm_this_thread();
    let Some(Some((layout, tdata))) = TLS_TEMPLATE.get() else {
        return;
    };
    let alloc = layout.allocation_size();
    // Spaced by the block's own size and a gap, so two threads' blocks never overlap;
    // bump-allocated and never reused.
    let unit = orbistoun_mem::allocation_granularity().max(orbistoun_core::GUEST_PAGE_SIZE);
    let step = alloc
        .checked_next_multiple_of(unit)
        .unwrap_or(alloc)
        .saturating_add(unit);
    let base = NEXT_THREAD_TLS.fetch_add(step, std::sync::atomic::Ordering::Relaxed);
    if let Err(e) = build_tls_block(base, layout, tdata) {
        tracing::warn!("could not set up thread-local storage for a spawned guest thread: {e}");
    }
}
/// Everything the run is put under, planted before the guest starts, and the spans it may touch.
///
/// One unit because the order inside it matters and mistakes are silent: a stack fill after a poke
/// erases it. The reason for a refusal is returned rather than written, so `enter` stays the one
/// place that decides what a refusal looks like.
fn arm_diagnostics(
    stack: &mut GuestStack,
    image: &Image,
    module: &str,
) -> Result<experiment::Experiments, Error> {
    // Every diagnostic this run is under, read here rather than threaded from `prepare_diagnostics`
    // through four signatures.
    let experiments = experiment::Experiments::from_env();

    // Filled before anything is written onto it, so the only zeros the guest sees are ones
    // something wrote. Refused rather than skipped on failure (D325).
    if let Some(byte) = experiments.stack_fill {
        if let Err(source) = stack.fill(byte) {
            return Err(Error::StackFill { byte, source });
        }
    }

    // After the stack fill, which would otherwise erase a poke; the watch then snapshots the state
    // the guest starts from.
    apply_memory_diagnostics(image, stack, &experiments);

    // Where an argument may safely be dereferenced for a dump: the same spans the fault reporter
    // names, so reading them cannot fault, and an argument outside them is a scalar (D194). The
    // stack span is asked of the stack itself, because `reserve` puts a guard page at the base and
    // usable memory starts one page above it.
    orbistoun_thunk::install_readable_ranges(vec![
        image.span(),
        (stack.lowest_usable(), stack.len()),
    ]);
    // The stack only, not the image: the image's runs are protected after relocation, so a forced
    // write there would fault inside the emulator. An out-parameter lives on the stack.
    orbistoun_thunk::install_writable_ranges(vec![(stack.lowest_usable(), stack.len())]);
    // Told where the stack is, so `sceKernelIsStack` answers from the span this process mapped
    // (D275).
    describe_environment(stack, module);
    // Copied before the guest can change it, so what the guest did is a comparison; a snapshot, not
    // a watchpoint - see `watch`. After the readable ranges, so an image address reads as mapped.
    if let Some((base, len)) = experiments.watch {
        watch::snapshot(base, len);
    }
    // Where the image is: it lives in the loader's address space, which the kernel's runtime map
    // never sees, so without this `sceKernelVirtualQuery` refuses the guest's own code (D446).
    let (image_span_base, image_span_len) = image.span();
    orbistoun_kernel::note_region(image_span_base, image_span_len);
    Ok(experiments)
}

/// What runs just before the guest's entry when asked: the placed modules' initialisers, and the
/// sampler that says where the entering thread spends its time.
fn before_entry_if_asked() {
    start_placed_modules_if_asked();
    profile::watch_this_thread("guest main");
}

/// Runs every placed module's initialisers, when `ORBISTOUN_START_MODULES` asks for it.
///
/// Says what it is about to do before each constructor, because a constructor is guest code and can
/// fault, and a line printed only afterwards would never appear (D325). What each module did is
/// recorded as it goes and reported from `persist`, which every ending reaches.
fn start_placed_modules_if_asked() {
    if orbistoun_env::START_MODULES.get().is_none() {
        return;
    }
    tracing::info!("starting every placed module before entry");
    let (modules, initialisers) = orbistoun_kernel::start_every_placed_module();
    tracing::info!(
        "started {modules} placed module(s) before entry, {initialisers} initialiser(s) ran"
    );
}

/// Starts a script-driven pad playing, if the run's controller configuration names one.
///
/// Called at the entry path, not at process start, so the script's clock (`at_ms` from the start of
/// the run) begins as the guest is entered rather than while a large title is still placed and
/// linked (D707). A run with no scripted port installs nothing. A named script that fails to read
/// or validate ends the run with the reason.
///
/// # Errors
///
/// When the configured script cannot be read, parsed, or validated.
fn install_scripted_input(service: &Service) -> Result<(), Error> {
    // A flip-keyed script counts the guest's own flips, whichever run it is.
    orbistoun_input::script::install_flip_clock(orbistoun_video::flips_accepted);
    // A capture asked for before the launch starts here, with the run.
    let armed = run_capture_input().clone();
    if let Some(to) = armed {
        capture_input(Some(&to));
    }
    // The run's own script first, then one the configuration names (D721).
    let named = run_input_script().clone();
    let chosen = match named {
        Some(path) => Some((
            path.clone(),
            orbistoun_service::read_pad_script(&path).map_err(Error::PadScript)?,
        )),
        None => match service.paths() {
            Some(paths) => {
                let config = paths.config_file();
                let base = config.parent().unwrap_or_else(|| Path::new("."));
                orbistoun_service::scripted_pad(service.pads(), base).map_err(Error::PadScript)?
            }
            None => None,
        },
    };
    let Some((path, script)) = chosen else {
        return Ok(());
    };
    let steps = script.len();
    orbistoun_input::script::install(script);
    tracing::info!(
        "playing input script {} ({steps} step(s)) on player 1",
        path.display()
    );
    Ok(())
}

/// The pad script the current run asked for itself, set from its request and cleared as it ends.
fn run_input_script() -> std::sync::MutexGuard<'static, Option<std::path::PathBuf>> {
    static NAMED: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);
    NAMED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A capture asked for before the run started, begun at its entry.
fn run_capture_input() -> std::sync::MutexGuard<'static, Option<std::path::PathBuf>> {
    static ARMED: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);
    ARMED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The file input is being captured to.
fn capture_file() -> std::sync::MutexGuard<'static, Option<std::fs::File>> {
    static FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);
    FILE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Captures what the guest reads from its pad into `to`, or stops with `None`, only when asked
/// (D721): the toolbar's "capture input", or `Request::Run::capture_input`. Each step is appended
/// and flushed as it happens, so a run ending in a fault leaves the capture whole. Flips count from
/// now. A file that cannot be opened is logged, and nothing is captured.
fn capture_input(to: Option<&Path>) {
    orbistoun_input::script::stop_recording();
    *capture_file() = None;
    let Some(path) = to else {
        return;
    };
    let opened = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::File::create(path));
    let mut file = match opened {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!("input is not captured - {}: {e}", path.display());
            return;
        }
    };
    let _ = writeln!(
        file,
        "# The pad as the title read it, a step per change, timed in its own flips (D721)."
    );
    *capture_file() = Some(file);
    orbistoun_input::script::record(|step| {
        if let Some(file) = capture_file().as_mut() {
            let _ = file.write_all(orbistoun_service::pad_script_step(step).as_bytes());
            let _ = file.flush();
        }
    });
    tracing::info!("capturing input to {}", path.display());
}

/// Plays the pad script `script` from now, or stops with `None` (D721): the toolbar's "playback
/// input" while a title runs. A script that cannot be read or does not validate is logged, and
/// nothing plays.
fn play_input(script: Option<&Path>) {
    let Some(path) = script else {
        orbistoun_input::script::clear();
        return;
    };
    match orbistoun_service::read_pad_script(path) {
        Ok(script) => {
            let steps = script.len();
            orbistoun_input::script::install(script);
            tracing::info!(
                "playing input script {} ({steps} step(s)) on player 1",
                path.display()
            );
        }
        Err(why) => {
            tracing::warn!("input is not played - {why}");
        }
    }
}

/// Hands control to the guest and reports what came back.
///
/// The one function here that may not return: guest code can fault and take the process with it.
/// That is contained rather than prevented, which is why the worker is a separate process (D032),
/// so everything worth knowing is written before the jump.
fn enter<W: Write>(
    output: &mut W,
    image: &Image,
    bytes: &[u8],
    summary: &str,
    limits: Limits,
    module: &str,
    entry_settings: &process::EntrySettings,
) -> io::Result<()> {
    let entry = starting_address(image, entry_settings);

    // Refused rather than attempted: a jump to a non-executable address faults without saying why
    // (D010). It applies to an overridden entry exactly as to the declared one.
    if !image.is_executable(entry) {
        return halt(
            output,
            Phase::Linked,
            format!("{summary}; not entered - {entry:#x} is not inside an executable segment"),
        );
    }

    // The guest's own files, mounted before it can ask for them. The base tree, its writable device
    // overlays and the title over `/app0` are established together and in order by
    // `sandbox::establish` (D423).
    install_filesystem(module);

    let mut stack = match GuestStack::reserve(GUEST_STACK_BASE, DEFAULT_STACK_SIZE) {
        Ok(s) => s,
        Err(e) => {
            return halt(
                output,
                Phase::Linked,
                format!("{summary}; could not reserve a guest stack: {e}"),
            );
        }
    };

    // Every diagnostic this run is under, and the spans they may touch, in order - see
    // `arm_diagnostics`.
    let experiments = match arm_diagnostics(&mut stack, image, module) {
        Ok(experiments) => experiments,
        Err(why) => return halt(output, Phase::Linked, format!("{summary}; {why}")),
    };

    let reporting = match install_reporting(&stack, &experiments) {
        Ok(active) => active,
        Err(why) => return halt(output, Phase::Linked, format!("{summary}; {why}")),
    };

    // Started before the jump, because after it this thread belongs to the guest. A guest waiting
    // on something that never happens would otherwise hang the run and lose its call trace (D238).
    install_limits(limits, module);

    // Written and flushed before the jump: a guest fault ends this process, and the parent must be
    // able to tell "never entered" from "entered and died".
    write_message(
        output,
        &Event::Reached {
            phase: Phase::Entered,
        },
    )?;

    // What the program finds on its stack when it starts, written before the transfer because the
    // entry point reads it at its first instruction.
    let entry_stack = match write_process_image(image, &stack, module, entry_settings) {
        Ok(pointer) => pointer,
        Err(e) => {
            return halt(
                output,
                Phase::Linked,
                format!("{summary}; could not build the process entry image: {e}"),
            );
        }
    };
    let (argument, second) = entry_arguments(
        entry_settings.argument,
        entry_stack,
        &entry_settings.handoff_fields,
    );

    // The guest thread pointer, installed for this thread before the jump. Guest code reads its
    // thread-locals through the `fs` base, and until it points at a real block every `fs:`-relative
    // access reads the host's (zero on Windows, whose TEB lives in `gs`). Refused rather than
    // entered blind: a title that declares thread-locals and cannot get a pointer would fault on
    // its first one.
    match install_main_thread_tls(image, bytes) {
        Ok(None) => {}
        Ok(Some(tp)) => {
            orbistoun_core::klog::note(&format!(
                "orbistoun: installed the main thread pointer at {tp:#x}"
            ));
        }
        Err(why) => {
            return halt(
                output,
                Phase::Linked,
                format!("{summary}; not entered - could not set up thread-local storage: {why}"),
            );
        }
    }

    let returned = transfer_to_guest(entry, entry_stack, argument, second, entry_settings);

    finish_returned_run(output, image, summary, module, reporting, returned)
}

/// Adopts the guest float environment, runs asked-for initialisers, and jumps to the entry.
fn transfer_to_guest(
    entry: u64,
    entry_stack: u64,
    argument: u64,
    second: u64,
    entry_settings: &process::EntrySettings,
) -> u64 {
    // The float environment the platform starts a title in: denormals-are-zero, flush-to-zero,
    // every exception masked. Guest code runs natively and would otherwise inherit the host
    // thread's, with DAZ and FTZ clear, and compute different answers for denormal inputs.
    orbistoun_abi::enter::adopt_guest_float_environment();

    // Initialises import-bound modules before the guest starts, when asked. A module starts when
    // the guest loads it by name (D515), and a module the loader placed and resolved imports
    // against is never loaded by name. Here because a constructor is guest code and this is the
    // last point before the guest's own entry.
    before_entry_if_asked();

    if entry_settings.convention == process::Convention::Process {
        // SAFETY: the image is fully relocated (checked above) and its text was made executable by
        // the protection pass. `entry_stack` is the sixteen-byte-aligned start of a process image
        // just written inside a mapped, writable guest stack. This never returns; the fault handler
        // and the time limit each persist the call trace from the guest's own thread.
        unsafe { orbistoun_abi::enter::enter_process(entry, entry_stack, argument) };
    }

    // SAFETY: as above, but called as an ordinary function. `stack` and the image both outlive the
    // call.
    unsafe {
        orbistoun_abi::enter::enter_guest_with_arguments(entry, entry_stack, argument, second)
    }
}

/// Records and renders what a guest that returned by itself left behind, then halts the run.
fn finish_returned_run<W: Write>(
    output: &mut W,
    image: &Image,
    summary: &str,
    module: &str,
    reporting: bool,
    returned: u64,
) -> io::Result<()> {
    // Persisted on the ordinary path too, so a guest that stops by itself is recorded as fully as
    // one that had to be stopped.
    report::what_the_guest_asked_for();
    // The renderer, on the run path (D695): a submitted command buffer is driven to a headless
    // graphics backend now the guest has returned, as the time-limit and call-budget endings also
    // do. After the trace is collected, because rendering takes the submission that the trace's
    // summary reads.
    let trace = report::collect_calls(module, "Entered");
    render_and_emit_frame(output)?;
    report::persist(&trace);

    let calls = orbistoun_thunk::total_calls();
    let reported = if reporting {
        "fault reporting was active"
    } else {
        "fault reporting is unavailable on this host"
    };
    // The opening, from the record kept for it rather than the head of the circular ring (D571).
    let first: Vec<String> = orbistoun_thunk::opening_calls()
        .iter()
        .take(SUMMARISED_CALLS)
        .map(|index| format!("#{index}"))
        .collect();
    let trace = if first.is_empty() {
        "no imports were called".to_owned()
    } else {
        format!("first imports called: {}", first.join(", "))
    };

    halt(
        output,
        Phase::Entered,
        format!(
            "{summary}; entered at {:#x} and returned {returned:#x} after {calls} import calls; {trace}; {reported}",
            image.entry()
        ),
    )
}

/// A running worker, driven from a shim.
#[derive(Debug)]
pub struct WorkerHandle {
    child: Child,
    /// Shared, because two threads write to it.
    ///
    /// The thread owning this handle is blocked reading events for a whole run, and a shell action
    /// has to be honoured during one, so the ability to send is taken before the handle is moved. A
    /// mutex rather than a second pipe: these messages are whole lines and rare, and the worker
    /// reads them on a thread of its own (D310).
    stdin: std::sync::Arc<std::sync::Mutex<ChildStdin>>,
    stdout: BufReader<ChildStdout>,
}

/// Carries a shell action to a worker whose handle is busy.
///
/// Holds only the sending half, so it cannot read events or drive a run.
#[derive(Debug)]
pub struct Control {
    stdin: std::sync::Arc<std::sync::Mutex<ChildStdin>>,
}

impl Control {
    /// Sends a shell action.
    ///
    /// # Errors
    ///
    /// When the pipe is gone, which is the ordinary race between a run ending and somebody
    /// pressing a button.
    pub fn shell(&self, action: orbistoun_shell::Request) -> io::Result<()> {
        let mut stdin = self
            .stdin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write_message(&mut *stdin, &Request::Shell { action })
    }

    /// Starts capturing the title's pad input into `to`, or stops with `None` (D721).
    ///
    /// # Errors
    ///
    /// When the pipe is gone.
    pub fn capture_input(&self, to: Option<std::path::PathBuf>) -> io::Result<()> {
        let mut stdin = self
            .stdin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write_message(&mut *stdin, &Request::CaptureInput { to })
    }

    /// Starts playing the pad script `script` from now, or stops with `None` (D721).
    ///
    /// # Errors
    ///
    /// When the pipe is gone.
    pub fn play_input(&self, script: Option<std::path::PathBuf>) -> io::Result<()> {
        let mut stdin = self
            .stdin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write_message(&mut *stdin, &Request::PlayInput { script })
    }

    /// Sends what the pads are doing, as a title is allowed to see them.
    ///
    /// # Errors
    ///
    /// When the pipe is gone, which is the ordinary race between a run ending and somebody
    /// still holding a controller.
    pub fn input(&self, pads: &[orbistoun_input::PadState]) -> io::Result<()> {
        let mut stdin = self
            .stdin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write_message(
            &mut *stdin,
            &Request::Input {
                pads: pads.to_vec(),
            },
        )
    }
}

/// Terminates a worker from a thread that does not own it.
///
/// Killing needs `&mut Child`, and the handle is owned by the thread blocked reading its events, so
/// the identity is taken before the handle is moved and the kill goes through the operating system.
/// Terminating is correct: the worker may be inside guest machine code that never returns (D032),
/// and the trace is written as it goes, so a killed run keeps everything already recorded.
#[derive(Debug, Clone, Copy)]
pub struct Stopper {
    // Read only by the Windows `stop()`; elsewhere the identity is carried but unused. Scoped so a
    // blanket allow does not hide it going unused on Windows.
    #[cfg_attr(not(windows), allow(dead_code))]
    process_id: u32,
}

impl Stopper {
    /// Terminates the worker. Answers whether the request was accepted.
    ///
    /// A worker that has already exited answers `false`, which is not a failure: a run can finish
    /// just as somebody presses stop.
    #[cfg(windows)]
    pub fn stop(self) -> bool {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_TERMINATE, TerminateProcess,
        };

        // SAFETY: `OpenProcess` is safe to call with any identifier; it answers null when
        // the process is gone or access is refused, which is checked before use.
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, self.process_id) };
        if handle.is_null() {
            return false;
        }
        // SAFETY: `handle` was just returned non-null by `OpenProcess` with terminate
        // rights, and is closed exactly once below.
        let terminated = unsafe { TerminateProcess(handle, 1) };
        // SAFETY: closing a handle this function opened and no longer uses.
        unsafe { CloseHandle(handle) };
        terminated != 0
    }

    /// Terminates the worker. Answers whether the request was accepted.
    #[cfg(not(windows))]
    pub fn stop(self) -> bool {
        // Away from Windows this needs a signal, which this build has no dependency for. Reported
        // as refused, so a shim can disable the control.
        false
    }

    /// Whether stopping is possible on this platform at all, so a shim can disable a stop control
    /// that would do nothing.
    pub const fn is_supported() -> bool {
        cfg!(windows)
    }
}

impl WorkerHandle {
    /// Something that can terminate this worker from another thread.
    ///
    /// Taken *before* the handle is moved into the thread that will block on it.
    pub fn stopper(&self) -> Stopper {
        Stopper {
            process_id: self.child.id(),
        }
    }

    /// Spawns the current executable in worker mode.
    ///
    /// Self-reinvocation rather than a separate binary: the worker is the same build, never a stale
    /// copy (D033).
    pub fn spawn_self() -> io::Result<Self> {
        let exe = std::env::current_exe()?;
        Self::spawn(&exe)
    }

    /// Spawns a specific executable in worker mode.
    pub fn spawn(exe: &Path) -> io::Result<Self> {
        let mut child = Command::new(exe)
            .arg(WORKER_FLAG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // stderr is inherited, so the worker's log reaches the same place the shim's does.
            .stderr(Stdio::inherit())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "worker stdin unavailable"))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "worker stdout unavailable")
        })?;

        let mut handle = Self {
            child,
            stdin: std::sync::Arc::new(std::sync::Mutex::new(stdin)),
            stdout: BufReader::new(stdout),
        };
        handle.handshake()?;
        Ok(handle)
    }

    /// Exchanges versions, refusing rather than proceeding on a mismatch.
    fn handshake(&mut self) -> io::Result<()> {
        self.send(&Request::Hello {
            protocol_version: PROTOCOL_VERSION,
        })?;
        match self.next_event()? {
            Some(Event::Hello {
                protocol_version, ..
            }) => check_version(protocol_version)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
            Some(Event::Failed { error }) => Err(io::Error::new(io::ErrorKind::InvalidData, error)),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected a handshake, got {other:?}"),
            )),
        }
    }

    /// Sends one request.
    pub fn send(&mut self, request: &Request) -> io::Result<()> {
        let mut stdin = self
            .stdin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write_message(&mut *stdin, request)
    }

    /// Something that can carry a shell action in while this handle is blocked reading.
    ///
    /// Taken before the handle is moved into the thread that will block on it, as [`Self::stopper`]
    /// is.
    #[must_use]
    pub fn control(&self) -> Control {
        Control {
            stdin: std::sync::Arc::clone(&self.stdin),
        }
    }

    /// Reads the next event, or `None` at end of stream.
    pub fn next_event(&mut self) -> io::Result<Option<Event>> {
        read_message(&mut self.stdout)
    }

    /// Sends a request and collects events until a terminal one arrives.
    ///
    /// Terminal means [`Event::SurveyComplete`], [`Event::Linked`], [`Event::Terminated`], or
    /// [`Event::Failed`] - anything that ends the exchange.
    pub fn request(&mut self, request: &Request) -> io::Result<Vec<Event>> {
        self.request_streaming(request, |_| {})
    }

    /// [`Self::request`], handing each event to `on_event` as it arrives, so a front end shows a
    /// presented frame while the run is still going.
    ///
    /// # Errors
    ///
    /// As [`Self::request`].
    pub fn request_streaming(
        &mut self,
        request: &Request,
        mut on_event: impl FnMut(&Event),
    ) -> io::Result<Vec<Event>> {
        self.send(request)?;
        let mut events = Vec::new();
        let mut verdict = false;
        while let Some(event) = self.next_event()? {
            on_event(&event);
            let terminal = matches!(
                event,
                Event::SurveyComplete(_)
                    | Event::Linked(_)
                    | Event::Terminated { .. }
                    | Event::Failed { .. }
            );
            events.push(event);
            if terminal {
                verdict = true;
                break;
            }
        }
        if !verdict {
            events.push(self.postmortem(&events)?);
        }
        Ok(events)
    }

    /// Explains a worker that ended without saying why.
    ///
    /// The stream closing with no verdict means the worker died mid-run, usually guest code
    /// faulting. Silence would be indistinguishable from a guest that ran and did nothing (D010).
    /// The furthest phase announced is reported alongside, since dying after entering the guest and
    /// dying while parsing are different problems.
    fn postmortem(&mut self, events: &[Event]) -> io::Result<Event> {
        let reached = events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Reached { phase } => Some(*phase),
                _ => None,
            })
            .unwrap_or(Phase::Start);

        let status = self.child.wait()?;
        #[cfg(unix)]
        let signal = std::os::unix::process::ExitStatusExt::signal(&status);
        #[cfg(not(unix))]
        let signal = None;

        Ok(Event::Terminated {
            outcome: Outcome::Crashed {
                signal: fault::describe(status.code(), signal),
            },
            reached,
        })
    }

    /// Asks the worker to stop and waits for it.
    pub fn shutdown(mut self) -> io::Result<()> {
        // Best-effort: a worker that has already died is a successful shutdown.
        let _ = self.send(&Request::Shutdown);
        drop(self.stdin);
        self.child.wait()?;
        Ok(())
    }
}

/// The configured memory settings, with the map-shape diagnostic applied over them.
///
/// The map shape is ordinarily a setting, but which shape a guest accepts is an open question,
/// answered by running a title against different shapes. A bad value is refused rather than
/// ignored, because a misspelt shape falling back to the configured one would look like the
/// experiment and not be it.
fn with_shape_diagnostic(
    mut settings: orbistoun_kernel::direct::Settings,
) -> orbistoun_kernel::direct::Settings {
    let Some(asked) = orbistoun_env::MAP_SHAPE.get() else {
        return settings;
    };
    let asked = asked.trim();
    if asked.is_empty() {
        return settings;
    }
    if let Some(shape) = orbistoun_kernel::direct::MapShape::named(asked) {
        settings.map_shape = shape;
    } else {
        tracing::warn!(
            "{} is not a map shape ({}) - left as configured",
            asked,
            orbistoun_kernel::direct::MapShape::NAMES.join(", ")
        );
    }
    settings
}

/// What the title's own index says about a guest path.
///
/// Read once and kept, and parsed on first use, because a title without an index is the ordinary
/// case.
fn look_up_in_index(guest_path: &str) -> Option<(u64, u64)> {
    use std::sync::OnceLock;

    /// Where a dumped title puts it.
    const INDEX: &str = "/app0/ampr_emu.index";

    static ENTRIES: OnceLock<Vec<orbistoun_fs::amprindex::Entry>> = OnceLock::new();
    let entries = ENTRIES.get_or_init(|| {
        // Through the mount table to a host path, not through the guest's filesystem: this is
        // orbistoun reading a file, and going through `open`/`read` would put it in the descriptor
        // table and the file statistics the guest is measured by.
        let Some(host) = orbistoun_fs::mount::resolve(INDEX) else {
            return Vec::new();
        };
        let bytes = std::fs::read(host).unwrap_or_default();
        orbistoun_fs::amprindex::parse(&bytes).unwrap_or_default()
    });
    entries
        .iter()
        .find(|e| e.path == guest_path)
        .map(|e| (e.id, e.size))
}

/// Reads a guest path into guest memory, for the asynchronous file path's experiment.
///
/// Bounded by what the caller says it has room for and by what the file holds: filling more than
/// the guest's buffer would corrupt its neighbour.
fn read_guest_file(guest_path: &str, address: u64, most: u64) -> Option<usize> {
    let handle = orbistoun_fs::open::open(guest_path)?;
    let at = usize::try_from(address).ok()?;
    let room = usize::try_from(most).ok()?;
    if at == 0 || room == 0 {
        return None;
    }
    // SAFETY: the guest supplied this address in a structure it built, and the caller checked it
    // against the length in the same structure. The mapping is identity, and the guest is inside
    // this call, so nothing else is writing the range.
    let into = unsafe {
        std::slice::from_raw_parts_mut(std::ptr::with_exposed_provenance_mut::<u8>(at), room)
    };
    let got = orbistoun_fs::open::read(handle, into);
    orbistoun_fs::open::close(handle);
    got
}

/// Publishes by-name stubs for declared imports, when a run asks for them.
///
/// Only under `ORBISTOUN_DLSYM_STUBS`, so a run that did not ask is unchanged and the difference
/// between two runs is the measurement. Built from the import labels (`library::name`) and keyed by
/// the bare name a guest passes to `sceKernelDlsym`. Implemented names are left out: they are
/// already reachable by name.
fn declare_by_name(thunks: &orbistoun_thunk::ThunkTable, labels: &[String]) {
    if !orbistoun_env::DLSYM_STUBS.is_set() {
        return;
    }
    let mut named = std::collections::BTreeMap::new();
    for (index, label) in labels.iter().enumerate() {
        let Some((_, name)) = label.split_once("::") else {
            continue;
        };
        if name.is_empty() || name.starts_with("0x") || orbistoun_thunk::name_thunk(name).is_some()
        {
            continue;
        }
        if let Some(at) = thunks.address_of(index) {
            named.insert(name.to_owned(), at);
        }
    }
    tracing::info!(
        "{} declared name(s) are resolvable by name under ORBISTOUN_DLSYM_STUBS",
        named.len()
    );
    orbistoun_thunk::install_declared_thunks(named);
}

/// Arms the imports a run named with `ORBISTOUN_DUMP`, and says what it armed.
///
/// Counted per clause, as the writes are, so a clause naming an import that is not there is
/// reported rather than reading as "the guest never called it". Named imports are dumped even when
/// implemented, since a suspect implementation needs its arguments shown too.
fn arm_dumps(targets: &[experiment::Target], total: usize) {
    if targets.is_empty() {
        return;
    }
    let mut forced = vec![false; total];
    let mut unmatched: Vec<&str> = Vec::new();
    for target in targets {
        let mut hits = 0_usize;
        for (index, slot) in forced.iter_mut().enumerate() {
            let Some(label) = report::label_of(index) else {
                continue;
            };
            if target.matches(label) {
                *slot = true;
                hits += 1;
            }
        }
        if hits == 0 {
            unmatched.push(target.as_str());
        }
    }
    for name in &unmatched {
        tracing::warn!("ORBISTOUN_DUMP matched no import called {name:?}");
    }
    let slots = forced.iter().filter(|f| **f).count();
    tracing::info!("ORBISTOUN_DUMP armed {slots} of {total} stub slot(s)");
    // Which labels, not just how many: a `Gap::Captured` finding answers a question somebody asked,
    // so the reporter must know which imports were asked about.
    report::name_forced_dumps(
        forced
            .iter()
            .enumerate()
            .filter(|(_, on)| **on)
            .filter_map(|(index, _)| report::label_of(index).map(str::to_owned))
            .collect(),
    );
    orbistoun_thunk::install_forced_dumps(forced);
}
/// Tells the reporter what a guest module calls its own functions, where it says.
///
/// Commercial titles are stripped; open-toolchain guests are not, and a symbol table turns
/// `image+0x28a163` into `obs_sink_open+0x73`. Read before entry, because the fault handler must
/// neither allocate nor parse.
fn name_guest_functions_from(bytes: &[u8]) {
    let Ok(container) = orbistoun_elf::Container::parse(bytes) else {
        return;
    };
    let Ok(functions) = container.function_symbols(bytes) else {
        return;
    };
    if functions.is_empty() {
        return;
    }
    tracing::debug!(
        "the module names {} of its own functions, so a fault in it can say which",
        functions.len()
    );
    report::name_guest_functions(
        functions
            .into_iter()
            .map(|s| (s.value, s.size, s.name))
            .collect(),
    );
}

#[cfg(test)]
mod tests {
    use super::{WORKER_FLAG, serve};
    use orbistoun_proto::codec::{read_message, write_message};
    use orbistoun_proto::{Event, PROTOCOL_VERSION, Phase, Request};
    use orbistoun_service::{Service, ServiceConfig};
    use std::io::{BufReader, Cursor};

    fn service() -> Service {
        Service::new(ServiceConfig::default())
    }

    /// Pad state survives the wire intact, and the worker answers nothing on the way.
    #[test]
    fn pad_state_crosses_the_protocol_and_answers_nothing() {
        let mut held = orbistoun_input::PadState::neutral();
        held.set(orbistoun_input::Button::South, true);
        held.set_trigger(true, 1.0);

        let events = exchange(&[
            Request::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
            Request::Input { pads: vec![held] },
            Request::Shutdown,
        ]);

        assert_eq!(events.len(), 1, "only the handshake answers: {events:?}");

        let arrived = orbistoun_input::latest::port(0);
        assert!(arrived.is_down(orbistoun_input::Button::South));
        assert!(
            arrived.is_down(orbistoun_input::Button::R2),
            "the trigger's button crossed with it"
        );
        assert!(
            (arrived.triggers[1] - 1.0).abs() < f32::EPSILON,
            "and so did its travel, which a bit alone could not carry"
        );
    }

    /// A shell action is answered without a word on the output stream, so the reading thread never
    /// becomes a second writer.
    #[test]
    fn a_shell_action_is_carried_out_without_answering_on_the_stream() {
        let events = exchange(&[
            Request::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
            Request::Shell {
                action: orbistoun_shell::Request::ToShell,
            },
            Request::Shutdown,
        ]);

        assert_eq!(
            events.len(),
            1,
            "only the handshake may answer, and it said: {events:?}"
        );
        assert!(matches!(events[0], Event::Hello { .. }));
    }

    /// Drives `serve` over in-memory pipes, so the protocol loop is tested with no process
    /// involved.
    fn exchange(requests: &[Request]) -> Vec<Event> {
        exchange_noting_hangup(requests).0
    }

    /// [`exchange`], also answering whether `serve` reported a hangup.
    fn exchange_noting_hangup(requests: &[Request]) -> (Vec<Event>, bool) {
        let mut input = Vec::new();
        for r in requests {
            write_message(&mut input, r).expect("encode");
        }
        let hung_up = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let noted = std::sync::Arc::clone(&hung_up);
        let mut output = Vec::new();
        serve(
            BufReader::new(Cursor::new(input)),
            &mut output,
            &service(),
            move || noted.store(true, std::sync::atomic::Ordering::SeqCst),
        )
        .expect("serve");

        let mut reader = BufReader::new(Cursor::new(output));
        let mut events = Vec::new();
        while let Some(e) = read_message::<_, Event>(&mut reader).expect("decode") {
            events.push(e);
        }
        (events, hung_up.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// A stream that ends without a `Shutdown` is a hangup, which ends an orphaned worker (D715).
    #[test]
    fn an_input_that_ends_without_a_shutdown_is_a_hangup() {
        let (events, hung_up) = exchange_noting_hangup(&[Request::Hello {
            protocol_version: PROTOCOL_VERSION,
        }]);
        assert!(matches!(events[..], [Event::Hello { .. }]), "{events:?}");
        assert!(hung_up, "the peer went away and serve did not say so");
    }

    /// A `Shutdown` is not a hangup, so a clean shutdown never exits with the orphan status.
    #[test]
    fn a_shutdown_is_not_a_hangup() {
        let (_, hung_up) = exchange_noting_hangup(&[
            Request::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
            Request::Shutdown,
        ]);
        assert!(!hung_up, "a clean shutdown was reported as a hangup");
    }

    /// The handshake reports this build's version.
    #[test]
    fn the_handshake_reports_this_builds_version() {
        let events = exchange(&[Request::Hello {
            protocol_version: PROTOCOL_VERSION,
        }]);
        assert!(matches!(
            events.as_slice(),
            [Event::Hello { protocol_version, .. }] if *protocol_version == PROTOCOL_VERSION
        ));
    }

    /// A version mismatch ends the session.
    #[test]
    fn a_version_mismatch_ends_the_session_rather_than_continuing() {
        // Continuing would parse every later message against the wrong contract.
        let events = exchange(&[
            Request::Hello {
                protocol_version: PROTOCOL_VERSION + 1,
            },
            Request::Shutdown,
        ]);
        assert_eq!(events.len(), 1, "nothing after the refusal");
        assert!(matches!(events[0], Event::Failed { .. }));
    }

    /// A failing request is reported and the session continues.
    #[test]
    fn a_failing_request_does_not_end_the_session() {
        // A recoverable request error must not end the session.
        let events = exchange(&[
            Request::Survey {
                path: "no/such/file".into(),
            },
            Request::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
        ]);
        assert!(matches!(events[0], Event::Failed { .. }));
        assert!(
            matches!(events[1], Event::Hello { .. }),
            "the loop kept going"
        );
    }

    /// `Shutdown` ends the loop and anything after it is ignored.
    #[test]
    fn shutdown_ends_the_loop_and_anything_after_it_is_ignored() {
        let events = exchange(&[
            Request::Shutdown,
            Request::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
        ]);
        assert!(events.is_empty(), "nothing is processed after shutdown");
    }

    /// A closed stream ends the loop cleanly.
    #[test]
    fn a_closed_stream_ends_the_loop_cleanly() {
        assert!(exchange(&[]).is_empty());
    }

    /// Input is captured only when asked, lands in the named file as it happens, and stops when
    /// told (D721).
    #[test]
    fn input_is_captured_only_when_asked_and_reads_back_as_a_script() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("input").join("capture.toml");
        let mut pressed = orbistoun_input::PadState::default();
        pressed.set(orbistoun_input::Button::South, true);

        super::capture_input(None);
        orbistoun_input::script::delivered(&pressed);
        assert!(!path.exists(), "nothing asked, nothing captured");

        super::capture_input(Some(&path));
        orbistoun_input::script::delivered(&orbistoun_input::PadState::default());
        orbistoun_input::script::delivered(&pressed);
        super::capture_input(None);
        orbistoun_input::script::delivered(&orbistoun_input::PadState::default());

        let script = orbistoun_service::read_pad_script(&path).expect("a capture is a script");
        assert_eq!(
            script.len(),
            2,
            "one step per change while capturing, none after"
        );
        assert!(script.at(u64::MAX).is_down(orbistoun_input::Button::South));
    }

    /// A missing guest is a request failure, not a halted run.
    #[test]
    fn a_missing_guest_is_a_request_failure_not_a_halted_run() {
        // `Failed` means the request was wrong; `Terminated` means a guest was loaded and then
        // stopped. The two must stay distinguishable.
        let events = exchange(&[Request::Run {
            path: "no/such/file".into(),
            symbols_db: None,
            limit_seconds: None,
            call_budget: None,
            input_script: None,
            capture_input: None,
            staged: false,
            relink: false,
        }]);
        assert!(matches!(events.as_slice(), [Event::Failed { .. }]));
    }

    /// A guest's first argument names its module under `/app0`, never the host path.
    #[test]
    fn argument_zero_is_the_module_under_app0() {
        assert_eq!(
            super::guest_argument_zero(r"C:\library\PPSA03416-app0/eboot.bin"),
            "/app0/eboot.bin"
        );
        assert_eq!(
            super::guest_argument_zero(r"D:\t\x\eboot.bin"),
            "/app0/eboot.bin"
        );
        assert_eq!(
            super::guest_argument_zero("/lib/t/game.elf"),
            "/app0/game.elf"
        );
    }

    /// Only where a module lies, or the run's own flag, makes it staged (D722): a library title
    /// beside the staging tree, or one nested deeper inside it, is an image.
    #[test]
    fn a_title_is_staged_by_where_it_lies() {
        use orbistoun_fs::sandbox::Origin;
        let dir = tempfile::tempdir().expect("tempdir");
        let titles = dir.path();
        let staging = orbistoun_paths::staged_under(titles);
        for d in [staging.join("NVRB00001"), titles.join("PPSA00001")] {
            std::fs::create_dir_all(&d).expect("mkdir");
        }
        let staged = Origin::Staged {
            id: "NVRB00001".to_owned(),
        };
        assert_eq!(
            super::origin_of(&staging.join("NVRB00001/eboot.bin"), &staging, false),
            staged
        );
        assert_eq!(
            super::origin_of(&titles.join("PPSA00001/eboot.bin"), &staging, false),
            Origin::Image
        );
        assert_eq!(
            super::origin_of(&staging.join("NVRB00001/sub/eboot.bin"), &staging, false),
            Origin::Image,
            "the title's own directory must be the staged one"
        );
        assert_eq!(
            super::origin_of(&titles.join("NVRB00001/eboot.bin"), &staging, true),
            staged,
            "--staged stages a loose build"
        );
    }

    /// A real container reaches placement and halts with a stated reason.
    #[test]
    fn a_real_container_reaches_placement_and_halts_honestly() {
        // A run that stops says so, rather than looking like a guest that ran and did nothing
        // (D010).
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("guest.elf");
        std::fs::write(&path, minimal_loadable_elf()).expect("write");

        let events = exchange(&[Request::Run {
            path,
            symbols_db: None,
            limit_seconds: None,
            call_budget: None,
            input_script: None,
            capture_input: None,
            staged: false,
            relink: false,
        }]);
        let reached: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Reached { phase } => Some(*phase),
                _ => None,
            })
            .collect();
        assert!(
            reached.contains(&Phase::Mapped),
            "placement should be reached: {events:?}"
        );
        assert!(
            matches!(events.last(), Some(Event::Terminated { .. })),
            "and it should say why it stopped: {events:?}"
        );
    }

    /// A link of a missing executable fails the request and leaves the worker serving.
    #[test]
    fn a_link_of_a_missing_executable_fails_the_request() {
        let events = exchange(&[
            Request::Link {
                path: "no/such/file".into(),
                symbols_db: None,
                relink: false,
            },
            Request::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
        ]);
        assert!(matches!(events[0], Event::Failed { .. }), "{events:?}");
        assert!(
            matches!(events[1], Event::Hello { .. }),
            "the loop kept going"
        );
    }

    /// A bare ELF with one loadable segment, generated rather than extracted (D051).
    fn minimal_loadable_elf() -> Vec<u8> {
        const EHDR: usize = 64;
        const PHDR: usize = 56;
        let data_at = EHDR + PHDR;
        let mut v = vec![0_u8; data_at];
        v[..4].copy_from_slice(b"\x7fELF");
        v[4] = 2; // 64-bit
        v[5] = 1; // little-endian
        v[7] = 9; // FreeBSD
        v[16..18].copy_from_slice(&0xFE18_u16.to_le_bytes());
        v[18..20].copy_from_slice(&0x3E_u16.to_le_bytes());
        v[32..40].copy_from_slice(&(EHDR as u64).to_le_bytes());
        v[54..56].copy_from_slice(&(PHDR as u16).to_le_bytes());
        v[56..58].copy_from_slice(&1_u16.to_le_bytes());
        let p = EHDR;
        v[p..p + 4].copy_from_slice(&1_u32.to_le_bytes()); // PT_LOAD
        v[p + 4..p + 8].copy_from_slice(&6_u32.to_le_bytes()); // RW
        v[p + 8..p + 16].copy_from_slice(&(data_at as u64).to_le_bytes());
        v[p + 16..p + 24].copy_from_slice(&0x1000_u64.to_le_bytes());
        v[p + 32..p + 40].copy_from_slice(&32_u64.to_le_bytes());
        v[p + 40..p + 48].copy_from_slice(&64_u64.to_le_bytes());
        v.extend(std::iter::repeat_n(0xCC_u8, 32));
        v
    }

    /// The worker flag is declared once and read by both sides.
    #[test]
    fn the_worker_flag_is_declared_once() {
        // Spawning side and parsing side read the same constant, so they cannot drift.
        assert_eq!(WORKER_FLAG, "--worker");
    }
}

#[cfg(test)]
mod stop_wording {
    /// The worker's wording for a deliberate stop matches what `orbistoun-report` awards
    /// `Reach::Exited` for; the report crate cannot reference the enum, so this crate, seeing both,
    /// guards the string.
    #[test]
    fn the_exit_wording_the_ladder_matches_is_the_wording_core_produces() {
        assert_eq!(
            orbistoun_overrides::DELIBERATE_EXIT,
            orbistoun_core::StopReason::Exited.label(),
            "the ladder matches a string core no longer produces - Reach::Exited is now unreachable"
        );
    }
}
