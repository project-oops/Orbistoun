//! Worker mode: hosting the crates behind the protocol, and driving one from a shim.
//!
//! Guest code executes in a child process (D032). This crate is both halves of that
//! arrangement:
//!
//! - [`serve`] is the child. It reads [`Request`]s, calls the service, and writes
//!   [`Event`]s. It holds no logic of its own.
//! - [`WorkerHandle`] is the parent. It **re-invokes the running executable** with a
//!   hidden flag rather than spawning a separate worker binary (D033), so version skew
//!   between shim and worker is impossible by construction.
//!
//! No binary is privileged. Worker mode is a mode any shim can enter, and it is as
//! thin as the other shims.
//!
//! # Testability
//!
//! [`serve`] takes a reader and a writer rather than reaching for the real stdio, so
//! the entire protocol loop is exercised over in-memory pipes with no process spawned
//! at all. Spawning is then tested separately, so a protocol bug and a process bug are
//! distinguishable failures.

use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use orbistoun_proto::codec::{read_message, write_message};
use orbistoun_proto::{Event, Outcome, PROTOCOL_VERSION, Phase, Request, check_version};
pub mod device_thread;
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

/// Hidden flag that puts a shim into worker mode.
///
/// Named once here so the spawning side and the parsing side cannot disagree.
pub const WORKER_FLAG: &str = "--worker";

/// Runs the worker loop until the peer asks it to stop or the stream ends.
///
/// Errors that belong to a *request* are reported as [`Event::Failed`] and the loop
/// continues; only a broken stream ends it. A worker that exited on the first bad
/// request would turn a recoverable problem into a lost session.
///
/// # Why reading happens on its own thread
///
/// This loop used to read and handle in one place, which is correct for every request that
/// is answered *between* runs - and useless for the only one that is not. A run occupies
/// this thread from `Run` until the guest stops, so a shell action arriving in the middle
/// sat unread in the pipe until the run it was meant to interrupt had already finished.
///
/// The reader thread is therefore not an optimisation. It is the difference between a
/// shell button that works and one that is a decoration (D310).
///
/// [`Request::Shell`] is applied *on that thread*, because it needs no reply: it changes
/// session state and raises events into a queue the guest drains. Everything else is
/// forwarded here, so the output stream keeps exactly one writer.
///
/// # When the input ends without a `Shutdown`
///
/// `on_hangup` is called, on the reading thread, when the stream ends without a
/// [`Request::Shutdown`] before it. The only peer is [`WorkerHandle`], which holds the pipe
/// open until it sends `Shutdown`, so an end without one means the peer is gone (D715).
/// It is called on the reading thread and not left to this loop because this loop may be
/// inside a guest that never returns, and would never see the channel close.
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
                // Answered here and not forwarded. The main loop may be inside the guest,
                // which is the whole point of the arrangement.
                Ok(Some(Request::Shell { action })) => shell_action(action),
                // Same reason, more so: input arrives while a run is in flight or not at
                // all. It replies with nothing, so the output stream keeps one writer.
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
                // Forwarded rather than swallowed, so a broken stream still ends the loop
                // with the error that broke it. A reader thread that logged and exited
                // would turn a transport failure into a worker that mysteriously stopped
                // answering.
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
                    // A version mismatch is not recoverable: every later message would
                    // be parsed against the wrong contract.
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
            Request::Run {
                path,
                symbols_db,
                limit_seconds,
                call_budget,
                input_script,
                capture_input: armed_capture,
                staged,
            } => {
                // A title exists from here, so a shell action arriving now has something to
                // act on. Before this point there is nothing to interrupt, and a request
                // that arrives early is refused and counted rather than queued for a title
                // that may never start.
                session::begin();
                *run_input_script() = input_script;
                *run_capture_input() = armed_capture;
                RUN_STAGED.store(staged, std::sync::atomic::Ordering::Relaxed);
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
                capture_input(None);
                play_input(None);
                ran?;
            }
            // Answered on the reading thread, which is the entire reason that thread
            // exists. Spelled out rather than caught by a wildcard so that adding a
            // request later is a compile error here instead of a silent drop.
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
/// Runs on the reading thread. A refusal is counted rather than reported back: there is one
/// writer on the output stream and it is the handling loop, so answering here would mean
/// interleaving two writers to say something the shim can already work out. What it must
/// not do is vanish, which is what [`session::summarise`] is for.
fn shell_action(action: orbistoun_shell::Request) {
    let _ = session::apply(action);
}

/// Base a module is placed at when it links at zero.
///
/// A module carries no absolute addresses, so it needs somewhere to go. High enough to
/// be clear of anything the host maps, and granularity-aligned.
pub const DEFAULT_MODULE_BASE: u64 = 0x0000_4000_0000_0000;

/// Where the modules a title ships with itself are placed.
///
/// Between the main executable and the guest stack, and clear of both: the title's own
/// modules are placed *before* the executable (D482), so they need somewhere that cannot
/// collide with the base it will take. Each module then gets its own slot within this
/// region with a granule of unmapped guard between them.
///
/// **Not `0x5000_0000_0000`, which is a guest's.** D443 records that PPSA02664's C++ allocator
/// reserves its heap arena at exactly that address, and that a policy region placed there once
/// already cost a null-pointer fault: the guest's reservation fell back, its arena-relative size
/// arithmetic underflowed, and `tlsf_add_pool` rejected the pool.
///
/// This constant was put on that same address when the modules were first placed - the decision
/// log had the answer and was not read. Moved off it (D488).
pub const TITLE_MODULE_BASE: u64 = 0x0000_4800_0000_0000;

/// Where the per-import stub table is placed.
///
/// Far from [`DEFAULT_MODULE_BASE`] on purpose: a stray offset from either then lands
/// in unmapped space and faults immediately, rather than quietly reaching the other
/// allocation and producing a plausible wrong answer.
pub const THUNK_TABLE_BASE: u64 = orbistoun_thunk::SUGGESTED_BASE;

/// Where storage for data imports is reserved.
///
/// Clear of both the images and the thunk table, so a stray offset from any of the three
/// lands in unmapped space and faults rather than quietly hitting another allocation.
pub const DATA_BLOCK_BASE: u64 = orbistoun_thunk::SUGGESTED_DATA_BASE;

/// Where the guest stack is reserved.
///
/// Clear of both the image and the stub table, so an overrun of any of the three lands
/// in unmapped space and faults rather than quietly reaching another.
pub const GUEST_STACK_BASE: u64 = 0x0000_6000_0000_0000;

/// How many recorded calls are quoted in the summary.
///
/// The beginning of a boot is the readable part; the full log is available separately.
pub const SUMMARISED_CALLS: usize = 8;

/// Loads a guest as far as the implementation goes, reporting each phase reached.
///
/// Runs placement, relocation, protection, and stub generation; the entry jump is
/// outstanding. It says so rather than falling silent, because a silent stop is
/// indistinguishable from a guest that ran and did nothing (D010).
fn run_guest<W: Write>(
    output: &mut W,
    service: &Service,
    path: &Path,
    symbols_db: Option<&Path>,
    limits: Limits,
) -> io::Result<()> {
    // Refused here rather than at the transfer, where it is `unimplemented!()` and would
    // reach the user as a child-process panic - which reads as a bug in the tool rather
    // than as a build that was never able to do this (D208).
    // **How this process was delivered, decided from what sits beside the module and declared
    // before anything can ask.** The platform behaves differently either side of it - a title
    // does not resolve platform symbols by name and a payload does, and has to - so a run that
    // never said would answer one of them wrongly for the whole of its length (D669).
    //
    // Before the execution guard rather than after: the guard is about this *build*, and the
    // route is a property of the run whether or not the run gets to execute anything.
    orbistoun_core::route::present(orbistoun_core::route::route_for(path, Path::exists));

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

    // Parsing the container is the gate; **surveying imports is not**. A container
    // with no dynamic table is legitimate - a static binary imports nothing - and
    // refusing to load one because its import list is empty would conflate "cannot
    // read this file" with "this file needs nothing".
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

/// Reports that a run stopped, with the reason and the furthest phase reached.
///
/// Every failure below funnels through here so no branch can quietly return without a
/// verdict - a silent stop is indistinguishable from a guest that ran and did nothing
/// (D010).
/// Renders the guest's last submission and, when it produced a frame, emits the frame's descriptor.
///
/// Only the ordinary-return path calls this: it owns the protocol stream, so the descriptor crosses
/// it as an `Event::Frame` for a running shim (`REQ-...1f07`, worklog 821). The endings that stop the
/// process from their own thread call `render::render_and_log_last_submission` alone.
fn render_and_emit_frame<W: Write>(output: &mut W) -> io::Result<()> {
    match render::render_and_log_last_submission() {
        Some(frame) => write_message(output, &frame),
        None => Ok(()),
    }
}

fn halt<W: Write>(output: &mut W, reached: Phase, reason: String) -> io::Result<()> {
    write_message(
        output,
        &Event::Terminated {
            outcome: Outcome::Halted { reason },
            reached,
        },
    )
}

/// Places an image, links it, protects it, and hands it to the guest.
///
/// Split from [`run_guest`] so neither is long enough to hide a branch.
/// The two ways a run can be stopped, carried together because they always travel together.
///
/// Separate parameters put eight on `place_and_relocate` and read as two unrelated knobs.
/// They are one decision with two halves: the budget fixes the call count so a verdict
/// measures the build, and the clock catches the guest that stops calling imports and would
/// otherwise hang (D238).
#[derive(Debug, Clone, Copy, Default)]
struct Limits {
    /// Wall-clock seconds, or `None` for no limit. The backstop.
    seconds: Option<u64>,
    /// Imports the guest may call, or `None` for no budget. The deterministic one.
    calls: Option<u64>,
}

/// Puts the console's tree under the guest, with this title's own files over it.
///
/// The title is named by the directory holding the module, which is what a person
/// reading `title-data/` needs to recognise it by. Split out of `enter` only because
/// that function is at its line budget; the reason it exists is in D251.
fn install_filesystem(module: &str) {
    let paths = orbistoun_paths::Paths::resolve();
    let title = Path::new(module)
        .parent()
        .and_then(|d| d.file_name())
        .map_or_else(
            || "unknown".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
    // **The sandbox is established as one thing, in orbistoun-fs.** This crate supplies where the
    // bytes live and the retention policy; the order - base tree, its writable device overlays,
    // then the title over /app0 - and the ephemeral-empty are the fs crate's, so a second consumer
    // cannot re-derive them wrong (D422, D423). The retention default is `Retain`: what a guest
    // wrote persists (saves, a probe's reports); `ORBISTOUN_SANDBOX=ephemeral` empties it each run.
    let retention = match orbistoun_env::SANDBOX.get().as_deref() {
        Some("ephemeral") => orbistoun_fs::sandbox::Retention::Ephemeral,
        _ => orbistoun_fs::sandbox::Retention::Retain,
    };
    // **Which tree the title sees is a named per-title setting** (`filesystem_view`, shipped in its
    // compat record or set in the user's override file), never a check on the id. A launcher opts
    // out of its sandbox: it writes into the console's one shared overlay and sees the library at
    // /user/app, as a system application does on the device (worklog 842).
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

/// Whether the current run asked to be staged (`Request::Run::staged`), set from its request and
/// cleared as it ends.
static RUN_STAGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Where a module's title is stored, which decides whether its `/app0` is writable (D722).
///
/// Staged when its directory lies directly under the library's staging tree, or when the run
/// asked for it; an image otherwise. Only the path decides - never anything the title ships. The
/// staged id is the directory's name, which is what it is staged under on the console.
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
/// module and a declared id. A payload folder with no `param.json` is not an installed title, on a
/// console or here.
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
    // after them retires from work that ran (worklog 832).
    orbistoun_gpu::agc_driver::install_draw_executor(render::execute_draws);
    // And read back when the drawn frame is written into guest memory - at the flip by default, or
    // after every submission when `ORBISTOUN_TARGET_WRITEBACK=submit` asks (D714).
    orbistoun_gpu::agc_driver::install_frame_reader(render::read_frame);
    orbistoun_gpu::agc_driver::set_write_back_at_flip(
        orbistoun_env::TARGET_WRITEBACK.get().as_deref() != Some("submit"),
    );
    // A submission's finer spans, when asked for (worklog 852).
    orbistoun_gpu::perf::set_detail(orbistoun_env::PERF_DETAIL.get().as_deref() == Some("1"));
    // A copy out of a colour target still on the device, carried out when its destination is first
    // touched (worklog 850, D717); the fault handler that notices the touch is `report`'s.
    orbistoun_gpu::agc_driver::install_lazy_copies(orbistoun_gpu::agc_driver::LazyCopies {
        snapshot: render::keep_frame_on_device,
        take: render::take_snapshot,
        discard: render::discard_snapshot,
        replace: render::replace_snapshot,
        guard: page_guard::guard,
        protect_writes: page_guard::protect_writes,
        release: page_guard::release,
    });
    // Every flip, shown as it is presented (worklog 841).
    orbistoun_video::install_flip_observer(render::present_flip);
    // A launcher's request to start another title, which the front end carries out (worklog 842).
    orbistoun_systemservice::launch::install_launch_observer(render::request_launch);
}

/// Tells the guest-OS layer what this process actually laid out.
///
/// Both are told rather than derived: this crate places the stack and the module, and
/// re-deriving either from the constants it was built with is how the readable window
/// ended up a page too low (D217, D275).
fn describe_environment(stack: &GuestStack, module: &str) {
    orbistoun_kernel::note_stack_span(stack.lowest_usable(), stack.len());
    orbistoun_kernel::note_loaded_modules(vec![(1, module.to_owned())]);
}

/// Arms both ways a run can be stopped, before the guest can call anything.
///
/// Whichever is reached first stops the run, and the exit status says which - because "ran
/// out of clock" and "made the calls it was allowed" call for different next steps. Both
/// are armed rather than one chosen: a guest in a tight import loop wastes most of a clock,
/// and a guest that stops calling imports never reaches a budget (D238).
fn install_limits(limits: Limits, module: &str) {
    if let Some(seconds) = limits.seconds {
        report::start_time_limit(seconds, module.to_owned());
    }
    if let Some(budget) = limits.calls {
        report::start_call_budget(budget, module.to_owned());
    }
}

/// Everything an import can resolve to: a stub for code, storage for data.
///
/// What the guest gets in each import slot, and the title's own modules linked behind them.
///
/// **One stub table across the executable and every module the title ships** (D484). The
/// executable is module 0 at offset 0, so its imports keep the slot numbers every trace and
/// every measurement already refer to, and a module answering one of them is a real address
/// in relocated code rather than a stub.
///
/// **Before relocation**, because relocation is where the wrong answer would otherwise be
/// written into the slot (D307, D323).
///
/// The returned value owns the module images, so it has to outlive the run - dropping it
/// unmaps code the executable now has pointers into.
fn link_the_title(
    service: &Service,
    path: &Path,
    symbols: &orbistoun_nid::SymbolDbFile,
) -> Result<orbistoun_service::LinkedTitle, String> {
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
        .map_err(|e| format!("could not link the title: {e}"))
}

/// The symbol database this run names imports with.
///
/// A supplied path still wins, because a database under construction has to be testable
/// before it is committed.
fn symbol_database(symbols_db: Option<&Path>) -> orbistoun_nid::SymbolDbFile {
    symbols_db
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| orbistoun_nid::SymbolDbFile::from_json(&text).ok())
        .unwrap_or_else(orbistoun_nid::SymbolDbFile::builtin)
}

/// Applies the executable's relocations against the title's code, then the stubs.
///
/// # The order is the whole of it
///
/// A stub is the honest answer for a platform library orbistoun has not written. It is the
/// **wrong** answer for a function sitting relocated in the title's own data: the stub returns
/// a placeholder, the guest uses it as an address, and the fault that follows names neither
/// (D483, D489). So the title's own modules answer first, for the imports the caller decided
/// they should - and everything else falls through to the stub table exactly as before.
///
/// The resolvers are built here rather than by the caller because they borrow the tables, and
/// the borrow has to end before the run report starts moving things about.
/// Weak imports that nothing answers, which bind to zero under the ELF gABI (D676).
///
/// An import binds to zero when:
/// 1. Its binding is weak ([`orbistoun_elf::dynamic::Binding::Weak`]);
/// 2. It is not answered by any module the title ships (`!title.bound.contains_key(&sym_index)`);
/// 3. It is not answered by the platform (host implementation or symbol database: `!service.is_named_with(...)`).
fn unanswered_weak_imports(
    service: &Service,
    bytes: &[u8],
    title: &orbistoun_service::LinkedTitle,
    symbols: &orbistoun_nid::SymbolDbFile,
) -> std::collections::BTreeSet<usize> {
    let Ok(imports) = service.raw_imports_of(bytes) else {
        return std::collections::BTreeSet::new();
    };
    let db = orbistoun_nid::SymbolDb::from_file(symbols).map(|(db, _)| db);
    imports
        .into_iter()
        .filter(|imp| {
            imp.binding == orbistoun_elf::dynamic::Binding::Weak
                && !title.bound.contains_key(&imp.symbol_index)
                && !service.is_named_with(orbistoun_nid::Nid::from_raw(imp.nid), db.as_ref())
        })
        .map(|imp| imp.symbol_index as usize)
        .collect()
}

fn relocate_the_executable(
    service: &Service,
    image: &Image,
    bytes: &[u8],
    title: &orbistoun_service::LinkedTitle,
    refuse: Option<&std::collections::BTreeSet<usize>>,
    weak_zero: Option<&std::collections::BTreeSet<usize>>,
) -> Result<orbistoun_elf::reloc::RelocationTally, orbistoun_service::ServiceError> {
    // No offset: the executable is module 0, so its symbol index *is* its slot (D484).
    let stubs = orbistoun_loader::relocate::ImportResolver {
        thunks: &title.thunks,
        data: &title.data,
        refuse,
        weak_zero,
    };
    let resolver = orbistoun_loader::relocate::TitleResolver {
        bound: &title.bound,
        inner: &stubs,
    };
    service.relocate_image(image, bytes, &resolver)
}

/// Fills the globals a guest reads without ever calling anything that could fill them.
///
/// Both must happen **before** the guest runs. `_Stdout` and `_Stderr` are *data*, so nothing
/// the guest calls would populate them on demand (D307, D323), and `getenv` has to answer from
/// the same strings the process image was built from rather than from a second copy.
fn publish_what_the_guest_reads(service: &Service) {
    orbistoun_thunk::install_environment(service.entry_settings().environment.clone());
    orbistoun_libc::streams::install();
}

/// Tells the fault reporter where the title's own code is.
///
/// **One span covering every module.** Without it the instruction pointer of a fault inside a
/// module is outside every region the reporter knows, which is its exact test for "orbistoun's
/// own code faulted" - so the guest's own crash was reported as an emulator bug. Once an import
/// binds into a module, that is where the guest spends its time (D489).
fn describe_title_modules(title: &orbistoun_service::LinkedTitle) {
    let images = title.placed.images();
    let (Some((_, first)), Some((_, last))) = (images.first(), images.last()) else {
        return;
    };
    let base = first.span().0;
    let (end_base, end_len) = last.span();
    let len = end_base.saturating_add(end_len).saturating_sub(base);
    report::describe_region(report::Region::TitleModules, base, len);
    // **Which library names are the title's own**, so an unnamed hash from one is not sent to a
    // vendor word list that cannot contain it (D631).
    report::name_title_modules(title.placed.exports().keys().cloned().collect());
    // **Every import accounted for, bound or not, and printed whether or not anything went
    // wrong.** Six of PPSA25872's are answered by modules it ships, all six landed on
    // placeholders, and the run said nothing - so the largest wall in the corpus looked like a
    // missing implementation for a month. "Six of six bound" and silence are different
    // statements and only one of them is evidence (D640).
    for line in title.binding.lines() {
        eprintln!("{line}");
        orbistoun_core::klog::note(&line);
    }
    report::name_bound_imports(title.bound.keys().map(|i| *i as usize).collect());
    // **Named to the reporter and never published to the readers.** The modules a title ships
    // were mapped, and the fault reporter could name an address in them - and no diagnostic
    // could read one, because the readable spans are the image and the main stack. So a watch
    // on a module this title ships answered *"in no span this run published as readable"*,
    // which is what an unmapped address answers, for memory the loader had placed itself.
    //
    // These are guest material at rest, which `docs/PROVENANCE.md` calls `static` evidence -
    // the same category as a module's import table, which this project has read since its
    // first week (D593).
    orbistoun_thunk::note_readable_range(base, len);
}

#[allow(clippy::too_many_lines)]
fn place_and_relocate<W: Write>(
    output: &mut W,
    service: &Service,
    bytes: &[u8],
    reached: Phase,
    limits: Limits,
    symbols_db: Option<&Path>,
    path: &Path,
) -> io::Result<()> {
    // The name a report calls this run, derived here rather than passed alongside the path it
    // is derived from - two parameters that must agree is one more thing that can disagree.
    let module = &path.display().to_string();
    // Everything observed so far links at or near zero - modules by construction, and
    // the executable too - so a placement base is always needed. Honouring a zero base
    // literally would fail, since the null page is never mappable.
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
    // The data symbols are published inside the link, across every module at once - a second
    // install here would be ignored, and writing one would read as though it were doing
    // something (D344, D484).
    publish_what_the_guest_reads(service);
    // Weak imports that are unanswered bind to zero under the ELF gABI (D676).
    let weak_zero = unanswered_weak_imports(service, bytes, &title, &database);
    // Which imports this run will refuse, if it was asked to refuse any (D392).
    let mut unnameable = unnameable_imports(service, bytes, symbols_db);
    if let Some(refused) = unnameable.as_mut() {
        refused.retain(|idx| !weak_zero.contains(idx));
    }
    let tally = match relocate_the_executable(
        service,
        &image,
        bytes,
        &title,
        unnameable.as_ref(),
        Some(&weak_zero),
    ) {
        Ok(t) => t,
        Err(e) => {
            return halt(
                output,
                Phase::Mapped,
                format!("{placed}; relocation failed: {e}"),
            );
        }
    };
    let relocations = describe_relocations(&tally, data);

    // **A refusal is not a failure to link.** Under `ORBISTOUN_RESOLVE=named` this run
    // deliberately left some imports unresolved, so the image is exactly as linked as it was
    // asked to be - and refusing to enter would make the setting unusable (D392).
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

    // Only now: the image was populated read-write, and relocation wrote into text.
    // Protecting any earlier would make those writes fault.
    // **What the executable itself exports**, so `sceKernelDlsym` can answer for it. The
    // title's own modules are registered by the loader; nothing registers this one - and for
    // PPSA02664 it is the one that matters. Its export table has exactly one entry,
    // `scriptingGetMem`, and the guest asks for it by name through `dlsym` (D517).
    //
    // Here because this is where the service, the bytes and the placed image are all in scope
    // at once. A failure costs `dlsym` an answer rather than the run: a binary with no export
    // table is ordinary, and the report already names what went unanswered.
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

    orbistoun_core::klog::note(&format!("orbistoun: {placed}"));
    orbistoun_core::klog::note(&format!("orbistoun: {relocations}"));
    let summary = format!(
        "{placed}; {relocations}; protected {} runs ({} bytes executable, {} writable, {} both); {} import stubs at {:#x}, {} implemented",
        protection.runs,
        protection.executable,
        protection.writable,
        protection.writable_and_executable,
        thunks.len(),
        thunks.base(),
        orbistoun_thunk::implemented_count_within(thunks.len())
    );

    name_guest_functions_from(bytes);

    prepare_diagnostics(service, module, &image, thunks, limits, &title.labels);

    // Only a fully linked image is safe to enter. An unapplied relocation leaves a
    // pointer that looks valid and is not, so entering would produce a fault with no
    // relationship to the thing that is actually wrong (D010).
    if reached != Phase::Linked {
        return halt(
            output,
            reached,
            format!("{summary}; not entered - the image is not fully linked"),
        );
    }

    // **Only when the run says it is skipping the runtime.** An ordinary run reaches these
    // globals through the guest's own startup code, which is the whole point (D376).
    if service.entry_settings().at.is_some() {
        fill_runtime_globals(&image, bytes, service.entry_settings());
    }

    // A script-driven pad, if this run's controllers name one, starts playing now - the last
    // step before entry, so its clock is the run's own (D707). A malformed script ends the run
    // here rather than entering on an input nobody wrote.
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

/// Arranges everything a run needs in order to explain itself afterwards.
///
/// All of it must happen **before** the guest is entered: names, regions, and the trace
/// destination are only useful to a process that is about to fault, and setting them up
/// afterwards is setting them up too late.
/// Why the diagnostics count stubs and the reports count imports.
///
/// **One number used to mean both, and then it did not.** `ThunkTable::len` is the guest's own
/// import count, because that is what "1,410 import stubs, 254 implemented" has always meant
/// and a report that quietly counted everything this emulator can answer would be flattering
/// itself (D366).
///
/// Every *diagnostic* wants the other number. A dump, a forced write and a forced return are
/// indexed by the same slot the dispatcher is, and sizing their tables to the imports leaves
/// every by-name resolution outside them - so the knob appears to work, changes nothing, and
/// says nothing about having done nothing.
///
/// That was not hypothetical for long: forcing `vsnprintf` to answer a value did nothing at
/// all, because `klogsrv` reaches it through a global rather than an import, and the whole
/// experiment was silently vacuous - the same shape as D166, D082 and D187, which is three
/// previous times a setting was consulted nowhere (D379).
fn prepare_diagnostics(
    service: &Service,
    module: &str,
    image: &Image,
    thunks: &orbistoun_thunk::ThunkTable,
    limits: Limits,
    labels: &[String],
) {
    // What this run is subject to, recorded before anything can fault. A verdict comparing
    // two runs is only evidence when these match, and neither the wall-clock limit nor the
    // stub policy is visible in any number the run reports (D181).
    let (default_return, overrides, propping) = service.policy_summary();
    // Read once and used three times below - to record what the run is under, to install
    // each diagnostic, and to warn about a variable that is nearly one. Reading the
    // environment separately at each site is how the three of these came to disagree about
    // what a run was doing (D220).
    let experiments = experiment::Experiments::from_env();

    for name in orbistoun_env::unknown() {
        // A command-line flag typed wrongly is refused; a variable typed wrongly is simply
        // absent, and the run reports an ordinary result. Saying so is the only defence.
        eprintln!("orbistoun: {name} is not a diagnostic this build understands - ignored");
    }
    report::record_conditions(orbistoun_report::trace::Conditions {
        limit_seconds: limits.seconds,
        call_budget: limits.calls,
        // Filled in when the run ends, once the counts exist: nothing has applied yet.
        did_nothing: Vec::new(),
        default_return,
        overrides,
        propping,
        experiments: experiments.describe(),
        intervened: experiments.intervenes(),
        // Read from the map that was actually built, not from the setting that asked for it -
        // a shape that fell back because its regions did not fit would otherwise be recorded
        // as the shape nobody got (D357).
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
        // crate version - so a run record names the tree that produced it and a regression can be
        // bisected from the records (8ae5). `env!("CARGO_PKG_VERSION")` was `0.1.0` on every record.
        build: orbistoun_env::build::line(),
    });

    // Hand the graphics submit path the regions the guest can read, so a submitted command buffer
    // and the shader addresses it names resolve against the guest's own allocated memory rather than
    // being read blind - the difference between an unresolved count that measures D101 and one fixed
    // at "all unresolved" (5bff). Set before the guest is entered, from the same map recorded above.
    if let Ok(map) = orbistoun_kernel::direct::map().lock() {
        orbistoun_gpu::agc_driver::set_guest_regions(
            map.regions()
                .iter()
                .filter(|r| r.allocated)
                .map(|r| (r.start, r.end))
                .collect(),
        );
    }
    // And the live answer for everything the guest maps after this point - a GL context's command
    // buffer among it (worklog 815).
    orbistoun_gpu::agc_driver::install_region_lookup(orbistoun_kernel::is_guest_readable);
    // And where the command processor may write - a fill, a copy, a fence (worklog 816).
    orbistoun_gpu::agc_driver::install_write_lookup(orbistoun_kernel::is_guest_writable);
    install_presentation();

    // Where the trace goes. Named after the module so a sweep over a directory of
    // titles leaves one file per title rather than each overwriting the last.
    if let Some(paths) = service.paths() {
        report::trace_to(
            paths
                .traces_dir()
                .join(orbistoun_report::trace::trace_file_name(module)),
        );
        report::describe_module(module.to_owned());
        // A rendered frame's region goes beside the trace, where the shim reads it back (D695,
        // `REQ-...1f07`).
        render::frames_to(paths.traces_dir());
    }

    // Names for the stubs, so a call trace says which function the guest wanted rather
    // than which slot it landed in. A failure here costs labels, not the run.
    // The database is loaded here rather than at worker start-up because it is a
    // property of the run, not of the process, and a worker serving several requests
    // must not carry one request's names into the next.
    // **The shipped database is used when none is supplied**, rather than falling back to
    // no names at all. It used to fall back, so a run reported hashes the tree could
    // already name - `printf` and `memalign` among them - and then advised extending the
    // vocabulary to find work that was already done (D188).
    //
    // A supplied path still wins, because a database under construction has to be
    // testable before it is committed.
    // **Every slot of the shared table, not the executable's alone.** Sized to the executable,
    // the vector ends exactly where the second module's slots begin, so a title module's call
    // was named after whichever by-name stub shared its index - which is how a call answering a
    // heap pointer read as `libc::usleep` (D490).
    if !labels.is_empty() {
        report::name_imports(labels.to_vec());
        // Where each implementation starts, so a fault in orbistoun's own code can name the
        // function it landed in rather than an address nothing can look up (D380).
        report::name_implementations(service.implementation_addresses());
    }

    // Registered before entering, so a fault can be attributed to a region rather than
    // left as a bare address. Too late to do this afterwards - the only fault that
    // matters is the one that ends the process.
    // Where an argument may safely be dereferenced for a dump. The same spans the fault
    // reporter names, reused: an address inside one of them is mapped by this process, so
    // reading it cannot fault - and an argument outside them is a length or a flag rather
    // than a pointer, which is the other half of what this filters (D194).
    let (span_base, span_len) = image.span();
    // Imports named for a dump are dumped even though something implements them, because
    // the case that matters is when the implementation is yours and you suspect it (D198).
    declare_by_name(thunks, labels);
    arm_dumps(&experiments.dump, thunks.total());

    // After the labels exist, because it resolves an import by name where there is one and
    // by hash where there is not - which is the whole point for a function nobody has
    // named (D218).
    if !experiments.write.is_empty() {
        let mut writes: Vec<Vec<orbistoun_thunk::Plant>> = vec![Vec::new(); thunks.total()];
        let mut matched = 0_usize;
        // Counted per clause, not per import. A list where one clause names a function that
        // is not there and the rest are fine would otherwise report a match and plant less
        // than it was asked to - and the missing plant is invisible in the result.
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
            eprintln!("orbistoun: ORBISTOUN_WRITE matched no import called {name:?}");
        }
        if matched == 0 {
            // Said out loud rather than left to be inferred from an unchanged run. This is
            // the shape D187 and D191 both took: an experiment that never reached the thing
            // under test, reporting no change and being believed.
            eprintln!("orbistoun: nothing was planted");
        } else {
            orbistoun_thunk::install_forced_writes(
                writes.into_iter().map(Vec::into_boxed_slice).collect(),
            );
        }
    }

    // Consulted before the policy's own answer, and matched the same way as the writes -
    // by name where there is one and by hash where there is not, which is what makes it
    // reach the function this was built for (D230).
    force_returns(&experiments.returns, thunks.total());

    // The memory-query structure carries values that name themselves, so whatever the guest
    // does next says which field it read (D220).
    orbistoun_kernel::mark_query_fields(experiments.mark_query);

    // One line naming every diagnostic in force, so a verdict taken under one is never
    // compared with an ordinary run as though they measured the same thing (D181).
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

/// Writes the initial process stack and answers where the entry point's `rsp` goes.
///
/// The auxiliary vector entries are *derived from the loaded image* rather than
/// configured: the entry point, the load base and the page size are facts about this run,
/// and a setting that could disagree with them would be a setting able to lie. Anything
/// not derivable is `extra_auxiliary`, which is for trying a value rather than for
/// restating one.
/// The first argument a guest is started with: its module's name under `/app0`, which is where the
/// title's own files are mounted. Only the file name of the host path, whichever separator it
/// uses - the host directory is a fact about this machine, never something a console would pass.
fn guest_argument_zero(module: &str) -> String {
    let name = module.rsplit(['/', '\\']).next().unwrap_or(module);
    format!("{}/{name}", orbistoun_fs::mount::APP_MOUNT)
}

fn write_process_image(
    image: &Image,
    stack: &GuestStack,
    module: &str,
    settings: &process::EntrySettings,
) -> Result<u64, String> {
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
    // Deliberately absent: AT_PHDR, AT_PHENT and AT_PHNUM. A runtime that walks its own
    // program headers needs them, and they are derivable - but the placed image does not
    // expose where the headers landed, and inventing an address for them would hand the
    // guest a pointer into whatever is there. Absent is a case a program can handle;
    // wrong is not.
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
        // The module's own name under `/app0`, which is what a program expects in the first
        // argument. `module` is a host path, a fact about this machine that no console would
        // ever hand a guest, so only its file name goes in.
        arguments: vec![guest_argument_zero(module)],
        environment: settings.environment.clone(),
        auxiliary,
    };

    let built = process::build(stack.initial_pointer(), stack.len(), &description)
        .ok_or_else(|| "the description does not fit in the guest stack".to_owned())?;

    let Ok(at) = usize::try_from(built.stack_pointer) else {
        return Err(format!(
            "stack pointer {:#x} is not addressable",
            built.stack_pointer
        ));
    };
    // SAFETY: `built.stack_pointer` and the bytes above it lie inside `stack`, which was
    // just reserved as mapped, writable guest memory - `process::build` refuses rather
    // than returning an image that does not fit. The mapping is identity, so a guest
    // address is a host address (D014).
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
/// The whole of worker mode: resolve the real paths, read the run configuration, build a
/// service, speak the protocol. **Here rather than in a shim** because every shim needs
/// exactly this and none of it is presentation - the CLI owned a private copy, and the
/// moment a second shim wanted to spawn a worker out of its own binary it would have
/// needed a second one (D160).
///
/// # Errors
///
/// When the run configuration is malformed, or the protocol stream fails.
pub fn serve_as_worker_process() -> Result<(), String> {
    // Real paths, not the default of none. The worker is the only process that ever sees
    // a call trace, and a trace not written where the shims look for it may as well not
    // exist (D077).
    let paths = orbistoun_paths::Paths::resolve();
    let _ = paths.ensure_dirs();
    // A malformed file fails the run instead of falling back quietly: a setting silently
    // ignored is indistinguishable from a setting that had no effect, and observing the
    // effect is the entire point (D153).
    let file = orbistoun_service::FileConfig::load(&paths.config_file())
        .map_err(|e| format!("reading the run configuration: {e}"))?;
    // What this machine measured, folded in **underneath** what a person wrote. A separate
    // file so deleting it is a complete undo and a diff keeps the two apart; absorbed rather
    // than merged so a deliberate entry always wins (D296). The policy is *derived* from the
    // measurements rather than stored, which is what keeps the file submittable (D297).
    let learned = orbistoun_hle::learned::Learned::load(&paths.learned_file())
        .map_err(|e| format!("reading what was learned: {e}"))?;
    let mut policy = file.policy;
    policy.absorb(learned.policy());
    // **And the regions the shipped knowledge base declares, under both of the above.** A
    // `returns` kind ships in the knowledge file and is applied at the call; a region cannot be,
    // because the service has to reserve the memory before the guest starts (D300). So a function
    // that answers a pointer to a descriptor it owns - an inline AGC non-export no probe can call,
    // measured nowhere - records the region in its knowledge entry, and this is where that reaches
    // the dispatcher. Absorbed last so a person's file and this machine's own measurement both win
    // over a shipped assumption.
    policy.absorb(orbistoun_hle::knowledge::Knowledge::builtin().region_policy());

    // **What the console is set to, reaching the guest at last.** `console::configure` was
    // written and never called, so every setting a person chose in the shell stopped at the
    // window and the guest read defaults - which is the same shape as every other thing this
    // session found modelled and inert (D346).
    //
    // Read here rather than sent over the protocol, for the reason the trace path is: the
    // worker is a separate process and reads its own files, so a setting cannot arrive
    // half-applied or not at all because a message was dropped.
    let mut settings = orbistoun_shell::Settings::load(&paths.shell_file())
        .map_err(|e| format!("reading what the console is set to: {e}"))?;
    // **A named profile, when one was asked for, replaces the configured machine for this run.**
    // It is validated in the CLI before the worker is spawned, so an unknown name never reaches
    // here; a name that somehow does not resolve leaves the configured machine untouched rather
    // than presenting a default nobody chose.
    if let Ok(profile) = std::env::var("ORBISTOUN_MACHINE_PROFILE") {
        if let Some(machine) = orbistoun_shell::profiles::machine(&profile) {
            settings.machine = machine;
            let _ = writeln!(io::stderr(), "orbistoun: presenting profile {profile}");
        }
    }
    // **Which machine this run presents itself as**, published to the layer that answers a
    // guest about it. Here rather than inside `configure`, because the two crates that need
    // it cannot see each other and this one already depends on both (D394).
    let machine = format!(
        "orbistoun: presenting a {} machine",
        settings.machine.describe()
    );
    let _ = writeln!(io::stderr(), "{machine}");
    // **And to the kernel log while the guest can still read it.** The reports that feed it
    // otherwise all run after the guest has stopped, so a client connecting to `klogsrv`
    // found the device present and permanently empty - which is the same experience as it
    // not existing (D389, D396).
    orbistoun_core::klog::note(&machine);
    // **The firmware skeleton, stood up when a firmware is being presented.**
    // (profile applied above, before this note is composed - see the settings load.) A run that names a
    // firmware is one prepared to meet a guest that reaches past the named interface into the
    // memory image beneath it - the post-exploitation payloads do exactly that (D404). Reserving
    // the region here means their address arithmetic lands in mapped, observable memory this
    // project owns, rather than in an unmapped marker; a run that presents no firmware pays
    // nothing. Failure to reserve is reported and not fatal: the run proceeds exactly as it did
    // before this existed, with those accesses faulting as unmapped.
    if settings.machine.firmware != 0 {
        if let Err(e) = orbistoun_firmware::present() {
            let _ = writeln!(
                io::stderr(),
                "orbistoun: could not stand up the firmware skeleton: {e} - firmware accesses will fault as unmapped"
            );
        } else {
            let _ = writeln!(
                io::stderr(),
                "orbistoun: firmware skeleton mapped at {:#x}, base handed to guests at {:#x}",
                orbistoun_firmware::FIRMWARE_BASE,
                orbistoun_firmware::handed_base()
            );
        }
    }
    // **The console's own syscall gadget, served whether or not a firmware is presented.** A
    // first-party payload routes every system call through a gadget inside libkernel, and when it
    // cannot resolve that gadget by name it falls back to a hardcoded address
    // (`orbistoun_firmware::console_gadget_address`). A proper module reaches that path during its
    // own startup - the open-toolchain cube issues its first `klog` there - so this is not tied to
    // the firmware skeleton the elfldr payloads need, and is set up unconditionally. Placing a
    // trampoline into this run's syscall gadget there turns the guest's `callq *<gadget>` into a
    // dispatch-and-return instead of a fault on an unmapped instruction fetch. One page, and a
    // reservation failure is reported and not fatal: the run proceeds as before, the call faulting
    // exactly as it did.
    {
        let dispatch: unsafe extern "sysv64" fn(*const u64) -> u64 =
            orbistoun_thunk::syscall::orbistoun_syscall_dispatch;
        match orbistoun_abi::enter::syscall_gadget(
            dispatch as *const () as usize as u64,
            orbistoun_thunk::syscall::SAVED,
        ) {
            Some(gadget) => {
                let trampoline = compact_trampoline(gadget);
                if let Err(e) = orbistoun_firmware::present_console_gadget(&trampoline) {
                    let _ = writeln!(
                        io::stderr(),
                        "orbistoun: could not serve the console syscall gadget: {e} - a payload's fallback syscall path will fault as unmapped"
                    );
                } else {
                    let _ = writeln!(
                        io::stderr(),
                        "orbistoun: console syscall gadget served at {:#x}",
                        orbistoun_firmware::console_gadget_address()
                    );
                }
            }
            None => {
                let _ = writeln!(
                    io::stderr(),
                    "orbistoun: no syscall gadget built, cannot serve the console gadget address"
                );
            }
        }
    }
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
    // The handle rather than a lock on it. Reading happens on its own thread now, and a
    // `StdinLock` holds a `MutexGuard` - which is deliberately not `Send`, so a lock taken
    // here could never get there. Nothing is given up: this process is the only reader.
    //
    // **And stdout unlocked too** (worklog 841): a presented frame is announced from the thread
    // that flips it, mid-run, which a lock held for the whole run would block for good. Each
    // message is written in one call (`write_message`), and `Stdout` locks per call, so two
    // writers interleave only between whole lines.
    render::stream_events_to(|event| {
        let _ = write_message(&mut io::stdout(), event);
    });
    serve(
        BufReader::new(io::stdin()),
        io::stdout(),
        &service,
        end_orphaned_worker,
    )
    .map_err(|e| format!("worker loop: {e}"))
}

/// Exit status of a worker that ended because its parent went away (D715).
///
/// Nobody reads it, because the parent is the process that would. It is not 0 because the run
/// was abandoned, not finished, and a person reading an exit log should see that.
pub const EXIT_ORPHANED: i32 = 3;

/// Ends a worker whose control channel closed without a `Shutdown` (D715).
///
/// **The whole process, not just the run.** The run is on another thread, inside guest machine
/// code that may never return, and nothing can unwind it from outside. Ending the process is
/// what [`Stopper`] already does for a stop button (D032). A run's trace is written as it goes,
/// so ending here keeps everything recorded so far.
///
/// Before this, a worker whose parent had gone kept running the guest with nobody to report to.
/// One was found an hour after its window closed, still holding the release executable open so
/// the next build could not replace it.
fn end_orphaned_worker() {
    let _ = writeln!(
        io::stderr(),
        "orbistoun: worker: the control channel closed without a shutdown - the parent is gone, ending this worker and any run in it"
    );
    std::process::exit(EXIT_ORPHANED);
}

/// What relocation did, for the run summary.
///
/// Its own function because `place_and_relocate` grew past the line ceiling, and a format
/// string is the part of it with no decisions in it.
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
/// # What "cannot name" means, and why it is the right line
///
/// An import is a hash. This project resolves it to a name through a symbol database it can
/// re-derive from its own inputs, and a name it cannot produce is one it knows **nothing**
/// about - not that the function is unimplemented, but that no evidence anywhere here says
/// such a symbol exists.
///
/// Giving that a stub tells the guest the symbol is present. Under `named` it is left
/// unresolved instead, which is what a console does with a symbol no library exports.
///
/// **Not the default.** Every measurement in `compat/` was taken with everything resolving,
/// and a run that refuses reaches fewer imports by construction - so the two are not
/// comparable and the setting says which one a run was under (D392).
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
        let _ = writeln!(
            io::stderr(),
            "orbistoun: asked to refuse unnameable imports, but the import list could not be read - nothing is refused"
        );
        return None;
    };
    // A label this could not name ends in the hash it could not name, which is the only
    // thing there is to say about it.
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
    let _ = writeln!(
        io::stderr(),
        "orbistoun: refusing {} of {} imports this build cannot name, so a guest can tell a symbol that exists from one that does not",
        refused.len(),
        labels.len()
    );
    Some(refused)
}

/// Hands control to the guest and reports what came back.
///
/// Split out because it is the one function here that may not return: guest code can
/// fault and take the process with it. That is contained rather than prevented - it is
/// why the worker is a separate process (D032) - so the job here is to make sure
/// everything worth knowing has already been written when it happens.
/// Makes the imports named in `ORBISTOUN_RETURN` answer the values asked for.
///
/// Split out of `prepare_diagnostics` only because that function was over its line budget;
/// the reason it belongs at all is in D230. Matched the same way as the plants - by name
/// where there is one and by hash where there is not, which is what makes it reach the
/// function it was built for.
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
            eprintln!(
                "orbistoun: ORBISTOUN_RETURN matched no import called {:?}",
                target.as_str()
            );
        }
        matched += hits;
    }
    if matched == 0 {
        eprintln!("orbistoun: no import will answer a forced value");
    } else {
        orbistoun_thunk::install_forced_returns(forced);
    }
}

/// Applies the diagnostics that change guest memory before the guest sees it.
///
/// Both must happen after relocation - which writes into static data - and before the
/// entry jump. Together in one function because they share that window and nothing else
/// does, and because `enter` is long enough already.
fn apply_memory_diagnostics(
    image: &Image,
    stack: &GuestStack,
    experiments: &experiment::Experiments,
) {
    // The zero-initialised tail of every segment, filled before the guest can read it.
    //
    // **This breaks a contract on purpose.** A guest is entitled to assume `.bss` is zero,
    // so a run under it misbehaves in ways an ordinary one does not - which is exactly what
    // makes it an answer. A value the guest reads from a static that nothing ever wrote
    // stops being zero, and the fault moves (D223).
    //
    // Writable segments only. A read-only one cannot be written here and its zeroed tail is
    // not somewhere a guest is waiting for an answer to appear.
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
            // SAFETY: the span the loader zeroed for this segment, in a writable mapping
            // this process made, before the guest has begun - nothing else can observe it.
            unsafe {
                std::ptr::write_bytes(std::ptr::with_exposed_provenance_mut::<u8>(at), byte, len);
            }
            filled = filled.saturating_add(segment.zeroed);
        }
        // Said out loud with the count. A fill that matched no writable segment would
        // otherwise be a run that reported an ordinary result, which is the failure every
        // diagnostic here is built to avoid.
        eprintln!("orbistoun: filled {filled} bytes of static data with {byte:#04x}");
    }

    // Reserved before anything reads it, and **leaked on purpose**: an `AddressSpace`
    // dropped at the end of this function would unmap the region before the guest ever
    // ran, and the run would report the same fault while claiming to have mapped it.
    if let Some((base, len)) = experiments.map {
        let mut space = orbistoun_mem::AddressSpace::new();
        match space.reserve(base, len, orbistoun_mem::Protection::READ_WRITE) {
            Ok(_) => {
                eprintln!("orbistoun: reserved {base:#x}+{len:#x} for this run only");
                std::mem::forget(space);
            }
            // Said out loud rather than left to be inferred from an unchanged fault. A
            // reservation that failed and a region the guest did not want look identical
            // from the fault address alone (D224).
            Err(e) => eprintln!("orbistoun: could not reserve {base:#x}+{len:#x}: {e}"),
        }
    }

    // Planted after relocation, so the loader cannot overwrite it, and before the entry
    // jump, so the guest reads it rather than whatever was there. Refused outside a writable
    // segment: poking read-only text would fault inside the emulator and produce a crash
    // with no relation to the guest (D223).
    if let Some((at, value)) = experiments.poke {
        // Any writable mapping, not just the image. The first version allowed image
        // segments only, which put every stack slot out of reach - and the arguments worth
        // poking at a wall are stack structures, so the restriction excluded the case the
        // tool exists for. Read-only text is still refused, which was the actual point
        // (D229).
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
                // SAFETY: the check above put eight bytes from `at` inside a writable
                // segment this process mapped, and the guest has not started.
                unsafe {
                    std::ptr::write_unaligned(
                        std::ptr::with_exposed_provenance_mut::<u64>(address),
                        value,
                    );
                }
                eprintln!("orbistoun: poked {value:#x} into {at:#x}");
            }
        } else {
            // Said out loud. A poke that landed nowhere would read as a run that changed
            // nothing, which is the failure every diagnostic here is built to avoid.
            eprintln!("orbistoun: {at:#x} is not inside a writable segment - nothing poked");
        }
    }
}

/// Everything that has to be watching before the guest starts, in the order it has to be.
///
/// Returns whether fault reporting is available on this host, and refuses the run if a
/// diagnostic that was asked for cannot be honoured.
///
/// **The ordering is the content of this function**, which is why the three live together:
///
/// - the stack is named first, so a fault inside it reads as `stack+...` rather than as a
///   bare address;
/// - the stop handler goes in before the guest can call anything, because a guest that stops
///   itself must have its trace written, the same as one that faults (D177);
/// - the watchpoints are armed last, **on this thread and after the handler that reports the
///   traps**. Debug registers belong to one thread rather than to the process, so arming
///   anywhere else watches nothing, and a trap arriving before the handler exists is an
///   unhandled debug exception rather than a finding (D276).
///
/// A watchpoint that was asked for and did not arm is **refused, not skipped**: it would
/// produce a report identical to one where nothing ever touched the address, which reads as
/// an answer and is not one (D185).
fn install_reporting(
    stack: &GuestStack,
    experiments: &experiment::Experiments,
) -> Result<bool, String> {
    report::describe_region(
        report::Region::Stack,
        stack.guard(),
        stack.len().saturating_add(orbistoun_mem::stack::GUARD_SIZE),
    );
    let reporting = report::install();
    report::install_stop_handler();
    // **The filesystem, handed to the crate that declares `libkernel`.** The asynchronous file
    // path is file I/O under a kernel name, and those two are sibling subsystems - so the reader
    // is installed from here, which owns both, rather than one reaching across (D589).
    orbistoun_kernel::apr::on_file_read(read_guest_file);
    orbistoun_kernel::apr::on_index_lookup(look_up_in_index);

    let armed = experiments
        .watchpoints()
        .and_then(|requests| watchpoint::arm(&requests))
        .map_err(|why| format!("the watchpoints could not be armed: {why}"))?;
    if !armed.is_empty() {
        eprintln!("orbistoun: watching {armed}");
    }
    Ok(reporting)
}

/// Where this run actually starts.
///
/// Normally the address the container declares. A diagnostic may name somewhere else -
/// entering at `main`, past a runtime start that rejects what orbistoun can hand it - and
/// `Some(0)` is a real request rather than an absent one, because an image's first byte is
/// an address two payloads put `main` at (D343).
///
/// **Says so on stderr when it is not the declared entry**, because every later number in
/// the report is then about a program that did not start where its container says it does.
/// What the guest finds in its first two argument registers.
///
/// **Two, because `main` takes two.** Every other variant answers what a *process entry
/// point* finds in one register and leaves `rsi` to whatever was there; `MainArguments`
/// answers a different question, for a run entering at `main` rather than at the declared
/// entry (D343, D348).
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
/// # Why a knob rather than only a setting
///
/// Which of these a guest wants is a fact about the guest, and the wrong one ends the run in
/// the entry function - an open-toolchain payload reads its first argument as a pointer to a
/// resolver table, and handing it an argument count means it calls whatever address the count
/// happens to be. `elfldr` calls address 1, because argc is 1.
///
/// It was already configurable. What it was not was *reachable from an instrument*: the
/// handoff command varied a field of the handoff structure while the run it measured was
/// handed something else entirely, so it reported on a block the guest never saw (D399).
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
        // An unrecognised value is not silently the default: a run under a misspelled setting
        // that behaves exactly like a run under none is the shape that made the handoff
        // instrument wrong for as long as it was.
        Some(other) => {
            let _ = writeln!(
                io::stderr(),
                "orbistoun: {other} is not an entry argument this knows - the configured one stands"
            );
            None
        }
        None => None,
    }
}

/// What orbistoun puts in a named global when it has something better than a stub.
///
/// # Why one name is a table and not a special case
///
/// `ptr_syscall` is what the open-toolchain runtime calls the slot it keeps a syscall gadget
/// in, and orbistoun has a syscall gadget. Serving a name a guest asks for is what this whole
/// layer does; that the ask arrives through a global rather than through a relocation changes
/// where it is written, not what it means (D378).
///
/// A table rather than an `if`, because the next one will be `getargv` or `environ` and each
/// wants its own sentence about what is being promised.
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
        // **The structure the runtime was handed, in the global it keeps it in.** This is the
        // one fill that is not a stub at all: `payload_args` holds what the entry point
        // received, and a run entering past the entry point never received it. Handing over
        // the same block the declared-entry path is handed makes the two modes agree about
        // what the guest is looking at - and makes the fields it reads say which they are,
        // because that block's unestablished fields are markers (D379).
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
    // SAFETY: the bounds check above established that eight bytes at `at` lie inside the
    // image this run mapped writable, and the guest has not started.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u64>(destination),
            value,
        );
    }
}

/// Where the markers for globals nothing implements start.
///
/// Its own range, distinct from the handoff structure's, so a fault address says which of the
/// two questions it belongs to before anybody has to decode it.
const UNSERVED_GLOBAL_BASE: u64 = 0x0000_5E29_0000_0000;

/// How far apart consecutive ones sit, so a small displacement still lands inside its own.
const UNSERVED_GLOBAL_STRIDE: u64 = 0x1000;

/// Fills the named globals a skipped runtime would have filled.
///
/// # Why this is part of the diagnostic rather than a lie
///
/// `[entry] at` starts a guest past its own startup code, and `orbistoun-env` already records
/// that such a run is not an ordinary one. The trouble is what it produced: a program whose
/// C library pointers are all null, dying on the first call through one - which measures the
/// skipping rather than the program.
///
/// The open-toolchain runtime resolves its C library at startup **by name** and stores each
/// answer in a named global in `.bss`. Those names are in the program's own symbol table. So
/// this does the same resolution, from the same names, answering with the same stub the
/// linker would have written for an import of that name (D376).
///
/// It is not what the platform does - the platform runs the startup code - and it is bounded
/// to the mode that already says so. Every fill is reported, because a run that quietly
/// initialised a guest differently from how it says it did is worth nothing.
///
/// A name nothing implements gets a marker, so its first *use* names it - or the null the
/// loader left it, under `ORBISTOUN_RUNTIME_GLOBALS=zero`, which is what a program with a
/// hundred of them needs (D385).
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

    // **A name that names five slots does not name a function.** `klogsrv` has five separate
    // globals called `calloc` and four called `strcpy` - which a table of resolved pointers
    // would not, so those are statics that happen to share a name with a function, and filling
    // them writes a stub address into unrelated state. Only names that occur once are filled
    // (D378).
    let mut seen: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for global in &globals {
        *seen.entry(global.name.as_str()).or_default() += 1;
    }

    let mut filled = 0_usize;
    let mut ambiguous = 0_usize;
    let mut unserved: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut marked: Vec<(u64, String)> = Vec::new();

    // **A gadget is not a function, so watching one needs a different stub.** Under
    // `markers` an unserved global says its own name when it is used and nothing more; under
    // `gadgets` it reports every register it was called with, `rax` included - which is the
    // only way to see a syscall number, because no argument-shaped report can (D377).
    // **Named rather than blanket.** Pointing every unserved global at a stub changes what
    // the data ones hold, and one of them fed a `va_list` on the first try - a diagnostic
    // that moved the wall it was installed to look at. So a run names the globals it wants
    // watched, and everything else keeps the marker it had (D377).
    // **`zero` rather than a name list**: leave every unserved global holding the null the
    // loader left it, instead of a marker. The doc above promised this and the code did the
    // other thing, which is its own kind of wrong (D385).
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
        // A name nothing implements gets a marker rather than the null it would otherwise
        // keep. Null is what the guest would have had anyway and says nothing when it is
        // used; a marker makes the *next* wall name itself, which is the difference between
        // "it jumped to zero" and "it called `ptr_syscall`" (D376).
        // A global orbistoun can serve with something other than a stub. Today that is one
        // name: the slot a payload keeps its syscall gadget in.
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
                // **Zero, which is what `.bss` holds and what the guest would have had.**
                //
                // A marker is the right answer for a payload whose unserved globals are all
                // kernel addresses a loader supplies: there are a handful, none is state,
                // and making the next wall name itself is worth more than a null.
                //
                // It is the wrong answer for a program with a hundred of them. `zftpd` has
                // 126 named globals and 24 that nothing here implements, and a marker in any
                // one that a `while` reads is a **diagnostic changing the program** - the
                // thing principle 3 and D227 exist to forbid. Under this setting they keep
                // the zero the loader left, which is the value a correct program checks
                // against (D385).
                return 0;
            }
            // **Numbered by where the global sits, not by how many markers came before it.**
            // Counting markers meant that watching one global renumbered every marker after
            // it - so a diagnostic aimed at one changed the values every other one held, and
            // the guest computes with those (D368, D377).
            let says = UNSERVED_GLOBAL_BASE + (position as u64) * UNSERVED_GLOBAL_STRIDE;
            marked.push((says, global.name.clone()));
            says
        });
        let at = image.base().saturating_add(global.address);
        let (span_base, span_len) = image.span();
        // Inside the image this run mapped, with room for the whole pointer. A symbol table
        // is not a promise about where anything landed.
        if at < span_base || at.saturating_add(8) > span_base.saturating_add(span_len) {
            continue;
        }
        let Ok(destination) = usize::try_from(at) else {
            continue;
        };
        // SAFETY: `contains` established that eight bytes at `at` lie inside the image this
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

/// Says what the fill did, because a run that quietly initialised a guest differently from
/// how it says it did is worth nothing.
fn report_globals_filled(what: &Filled) {
    let _ = writeln!(
        io::stderr(),
        "orbistoun: entering past the runtime, so {} of {} named globals were resolved the way it would have resolved them",
        what.resolved,
        what.total
    );
    if what.ambiguous > 0 {
        let _ = writeln!(
            io::stderr(),
            "orbistoun: {} were left alone because their names are not unique - a name that names several slots does not name a function",
            what.ambiguous
        );
    }
    if !what.gadget_names.is_empty() {
        let _ = writeln!(
            io::stderr(),
            "orbistoun: {} named by this run hold a stub that reports how it was called",
            what.gadget_names.len()
        );
        orbistoun_abi::enter::install_global_names(what.gadget_names.clone());
    }
    for (says, name) in &what.marked {
        let _ = writeln!(
            io::stderr(),
            "orbistoun: {name} names nothing implemented - it holds {says:#x}, so a use of it faults on an address that says which it was"
        );
    }
}

/// The handoff structure, with the resolver in the field a payload calls first.
///
/// **Says so when there is no resolver**, rather than handing over a null and letting the
/// guest fault on it: a payload calls field zero before it does anything else, so a null
/// there is a run that ends immediately with a fault that explains nothing (D366).
/// Lays out libkernel in the firmware region and answers `getpid`'s address, or `None`.
///
/// # Why `getpid` is what word zero holds
///
/// elfldr hands a payload `getpid`'s address as `payload_args[0]` and resolves nothing else; the
/// payload's CRT computes `libkernel_base = args[0] - 0x5b0` and reaches every other export at
/// `base + vaddr` (obSCEne D208/D209). This project had been putting the *resolver* there, which
/// is a value the CRT never expects to see - `base` came out garbage and the CRT bailed to its
/// error exit, which is the wall every payload was hitting (D407).
///
/// So each export this project has a measured vaddr for is stubbed at `LIBKERNEL_BASE + vaddr`,
/// and `getpid`'s address in that layout is what word zero becomes. The stub bytes are the
/// function's own thunk, copied - the thunks are position-independent, so one works wherever it
/// is placed.
///
/// Answers `None` when no firmware region is reserved, so a run presenting no firmware keeps the
/// old resolver-in-word-zero behaviour untouched.
/// A compact absolute trampoline (`mov r11, <target>; jmp r11`, 13 bytes).
fn compact_trampoline(target: u64) -> [u8; 13] {
    let mut code = [0u8; 13];
    code[0..2].copy_from_slice(&[0x49, 0xBB]); // mov r11, imm64
    code[2..10].copy_from_slice(&target.to_le_bytes());
    code[10..13].copy_from_slice(&[0x41, 0xFF, 0xE3]); // jmp r11
    code
}

/// The libkernel exports laid out this run, kept so the unimplemented-export handler can name
/// the one a guest called from the vaddr its stub passes. Set once at layout, read on faults.
static LIBKERNEL_LAYOUT: std::sync::OnceLock<Vec<(String, u64)>> = std::sync::OnceLock::new();

/// Records an unimplemented libkernel export a guest reached, once per distinct name.
///
/// **This is the work list.** A payload reaching an export by `base + vaddr` that this project
/// has a vaddr for but no implementation of is the clearest statement it makes of what it needs
/// next; printing only "an export" threw that away. The stub passes its own vaddr in `rdi`, which
/// maps back to the name here (D407).
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
        let _ = writeln!(
            io::stderr(),
            "orbistoun: payload called unimplemented libkernel export {name} at vaddr {vaddr:#x}"
        );
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
/// # Why getpid cannot use its ordinary 64-byte thunk here
///
/// The measured export table packs functions **0x20 bytes apart** (getpid `0x5b0`, mount
/// `0x5d0`, unmount `0x5f0`). A 64-byte thunk at getpid's vaddr runs straight over its
/// neighbours - its own dispatch lands at `+0x20`, exactly where the next export's stub goes, so
/// the two corrupt each other and a call to getpid ends up in mount's stub (D407).
///
/// So getpid gets a 23-byte slot that fits the gap and preserves the one thing the payloads need
/// besides the call: **byte 10 is a jump to the syscall gadget**. A payload takes getpid's
/// address, adds ten, and calls that for every system call (D400), because on the real console
/// getpid's own `syscall` instruction sits ten bytes in. Here:
///
/// - offset 0: `mov eax, 20` - getpid's syscall number - then padding to offset 10, so a plain
///   call to getpid sets the number and falls into the gadget, issuing syscall 20;
/// - offset 10: `mov r11, gadget; jmp r11`, so getpid+10 reaches the same gadget with whatever
///   number the caller placed - which is the +10 convention exactly.
///
/// `20` is `SYS_getpid`, the one syscall number stable enough to anchor on; the gadget address is
/// lifted from getpid's own thunk, where the landing zone already embedded it.
fn getpid_export_slot(gadget: u64) -> [u8; 23] {
    let mut code = [0x90u8; 23]; // nop-filled, so offsets 5..10 are the fall-through sled
    code[0] = 0xB8; // mov eax, imm32
    code[1..5].copy_from_slice(&20u32.to_le_bytes());
    code[10..12].copy_from_slice(&[0x49, 0xBB]); // mov r11, imm64
    code[12..20].copy_from_slice(&gadget.to_le_bytes());
    code[20..23].copy_from_slice(&[0x41, 0xFF, 0xE3]); // jmp r11
    code
}

/// A stub that records which export it is, then answers unimplemented.
///
/// `mov edi, vaddr` (the vaddr fits in 32 bits for any real libkernel offset, and `edi`
/// zero-extends into `rdi`, the handler's first argument), then an absolute jump to the handler,
/// which the firmware region is too far from to reach with a relative one.
fn unimplemented_export_stub(vaddr: u64, handler: u64) -> [u8; 18] {
    let mut code = [0u8; 18];
    code[0] = 0xBF; // mov edi, imm32
    code[1..5].copy_from_slice(&(vaddr as u32).to_le_bytes());
    code[5..7].copy_from_slice(&[0x49, 0xBB]); // mov r11, imm64
    code[7..15].copy_from_slice(&handler.to_le_bytes());
    code[15..18].copy_from_slice(&[0x41, 0xFF, 0xE3]); // jmp r11
    code
}

fn libkernel_word0() -> Option<u64> {
    if !orbistoun_firmware::is_present() {
        return None;
    }
    let exports: Vec<(String, u64)> = orbistoun_firmware::libkernel_exports()
        .iter()
        .map(|(n, v)| ((*n).to_owned(), *v))
        .collect();
    // The layout is decided by a pure planner (testable, diff-able) and only placed here. That
    // split is what makes a stub-overruns-its-neighbour collision a unit-test failure rather than
    // a corrupted guest (D407).
    let plan = orbistoun_firmware::plan_layout(&exports, |name| {
        orbistoun_thunk::name_thunk(name).is_some()
    });

    // The table the unimplemented-export handler names a called export from. Set before any stub
    // can run, since the guest only reaches one after entry, which is well after this.
    let _ = LIBKERNEL_LAYOUT.set(plan.iter().map(|p| (p.name.clone(), p.vaddr)).collect());

    let unimpl_target = unimplemented_libkernel_export as *const () as usize as u64;
    let mut collisions = 0_usize;

    for placement in &plan {
        if let Some((next_name, next_vaddr)) = &placement.collides_with {
            collisions += 1;
            let _ = writeln!(
                io::stderr(),
                "orbistoun: libkernel {} at {:#x} overruns {next_name} at {next_vaddr:#x}",
                placement.name,
                placement.vaddr
            );
        }

        let code_bytes: Vec<u8> = match placement.kind {
            orbistoun_firmware::SlotKind::Anchor => getpid_anchor_bytes()?,
            orbistoun_firmware::SlotKind::Trampoline => {
                // The planner already established this name has a thunk; if it somehow does not,
                // fall through to an unimplemented stub rather than skip the slot.
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
            let _ = writeln!(
                io::stderr(),
                "orbistoun: could not lay out libkernel {}: {e}",
                placement.name
            );
            return None;
        }
    }

    let _ = writeln!(
        io::stderr(),
        "orbistoun: libkernel laid out ({} exports, {collisions} collisions), getpid at {:#x} handed as payload_args[0]",
        plan.len(),
        orbistoun_firmware::getpid_address()
    );
    Some(orbistoun_firmware::getpid_address())
}

/// The compact getpid anchor slot, with the syscall gadget lifted from getpid's own thunk.
///
/// Split out of the layout loop so the loop reads as one match on slot kind. `None` if getpid has
/// no thunk, which would mean no syscall gadget to anchor the whole scheme on.
fn getpid_anchor_bytes() -> Option<Vec<u8>> {
    let thunk_addr = orbistoun_thunk::name_thunk("getpid").or_else(|| {
        let _ = writeln!(
            io::stderr(),
            "orbistoun: no thunk for getpid, cannot lay out the syscall gadget"
        );
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
    // code without carrying the whole 64-byte thunk into a 0x20-byte gap.
    let gadget_at = orbistoun_thunk::LANDING_END + 2;
    let mut gadget_bytes = [0u8; 8];
    gadget_bytes.copy_from_slice(&slice[gadget_at..gadget_at + 8]);
    Some(getpid_export_slot(u64::from_le_bytes(gadget_bytes)).to_vec())
}

/// The pointer fields a 12.40 console was measured to hand a payload, in this project's own
/// mapped memory.
///
/// # What was measured, and what this can and cannot reproduce
///
/// obSCEne read the whole `payload_args` off a jailbroken console (D408). Word zero was getpid;
/// words one, two and five were userland pointers; words three and four were **kernel** pointers -
/// one a kernel-heap address, one the kernel base itself; six onward were null.
///
/// This project cannot hand over the real values. The kernel ones are canonical high-half
/// addresses (`0xffff...`), which a user process cannot map, so a deref of the real number would
/// fault on the host before it reached anything. What it *can* do is hand a pointer of the right
/// **shape** at each field: non-null where the console's was non-null, null where it was null,
/// and - the part that makes it useful rather than a bare guess - every one backed by mapped
/// firmware memory, so a payload that checks a field and then reads through it finds zeroes and
/// says what it did, rather than bailing on a marker or faulting on an unmapped number.
///
/// The open-source `elfldr.c` contract (ps5-payload-dev, GPL-3.0 - read as prose per
/// ACKNOWLEDGEMENTS.md) sets up:
/// - Word 0: `getpid` function pointer
/// - Word 1: `rwpipe` pointer to `int[2]` with pipe read/write file descriptors
/// - Word 2: `rwpair` pointer to `int[2]` with socket descriptors
/// - Word 3: `kpipe_addr` kernel pointer
/// - Word 4: `kdata_base_addr` kernel base
/// - Word 5: `payloadout` pointer to int (0)
/// - Words 6+: 0
///
/// The offsets between the fields are this project's, not the console's: distinct so a fault
/// names which field a payload used, and spread so a small displacement stays inside one field.
fn measured_handoff_fields() -> Vec<[u64; 2]> {
    let base = orbistoun_firmware::FIRMWARE_BASE;
    let rwpipe_offset = 0x0010_0000_u64;
    let rwpair_offset = 0x0020_0000_u64;
    let payloadout_offset = 0x0050_0000_u64;

    // Initialize rwpipe buffer with [pipe0, pipe1] FDs (int[2]) as elfldr does.
    let pipe_fds: [i32; 2] = [3, 4];
    let mut pipe_bytes = [0u8; 8];
    pipe_bytes[0..4].copy_from_slice(&pipe_fds[0].to_le_bytes());
    pipe_bytes[4..8].copy_from_slice(&pipe_fds[1].to_le_bytes());
    let _ = orbistoun_firmware::place_export(rwpipe_offset, &pipe_bytes);

    // Initialize rwpair buffer with [master_sock, victim_sock] FDs (int[2]) as elfldr does.
    let sock_fds: [i32; 2] = [5, 6];
    let mut sock_bytes = [0u8; 8];
    sock_bytes[0..4].copy_from_slice(&sock_fds[0].to_le_bytes());
    sock_bytes[4..8].copy_from_slice(&sock_fds[1].to_le_bytes());
    let _ = orbistoun_firmware::place_export(rwpair_offset, &sock_bytes);

    // Initialize payloadout buffer with 0 (int) as elfldr does.
    let payloadout_val: i32 = 0;
    let _ = orbistoun_firmware::place_export(payloadout_offset, &payloadout_val.to_le_bytes());

    vec![
        [1, base + rwpipe_offset],
        [2, base + rwpair_offset],
        // Fields three and four are the measured canonical high-half kernel pointers (D408).
        // The payload's kernel_copyout validates `kpipe_addr >> 48 != 0` before attempting kernel r/w.
        [3, 0xffff_8661_5c60_7840],
        [4, 0xffff_ffff_8c29_0000],
        [5, base + payloadout_offset],
    ]
}

fn handoff_block(named_fields: &[[u64; 2]]) -> u64 {
    // **`getpid` at word zero when a firmware is present, the resolver otherwise.** The payloads
    // expect the former and this project long provided the latter; see `libkernel_word0` (D407).
    let resolver = libkernel_word0()
        .or_else(|| orbistoun_thunk::name_thunk("sceKernelDlsym"))
        .unwrap_or_else(|| {
            let _ = writeln!(
                io::stderr(),
                "orbistoun: nothing implements sceKernelDlsym, so the handoff structure has no resolver to offer"
            );
            0
        });
    // **Markers by default, because naming a field is what this is for.** A run that needs
    // the guest to get past a field it merely reads can ask for zeroes instead - the value a
    // correct program checks - and `orbistoun-env` records that the run was not an ordinary
    // one (D368).
    // **The measured layout when a firmware is present and nothing overrode it.** A 12.40 console
    // was watched handing a payload the whole struct (D408): word zero getpid, words one to five
    // non-null pointers, six onward all null. So a firmware run mirrors that - the unknown fields
    // are *zero*, not markers, because that is what the console's were, and a payload that checks
    // a field it expects null against a marker would branch wrongly.
    let firmware_present = orbistoun_firmware::is_present();
    let explicit_fields = orbistoun_env::HANDOFF_FIELDS.get();
    let unknown = match explicit_fields.as_deref() {
        Some("zero") => orbistoun_abi::enter::UnknownFields::Zero,
        Some("deep") => orbistoun_abi::enter::UnknownFields::Markers {
            base: mapped_unknown_fields_marked(),
        },
        // The second level: what a field points at is a table of stubs, so a *call* through
        // a member is as legible as the entry point's call through field zero was (D375).
        Some("members") => orbistoun_abi::enter::UnknownFields::Markers {
            base: mapped_unknown_fields_stubbed(),
        },
        // Unmapped markers: any *use* of a field stops the run at an address that names it.
        // The strictest of the four, and the only one that can say which field a guest read
        // through - mapping the region makes that read succeed and say nothing (D369).
        Some("strict") => orbistoun_abi::enter::UnknownFields::Markers {
            base: orbistoun_abi::enter::SENTINEL_BASE,
        },
        // The measured default: null unknowns for a firmware run, markers otherwise.
        _ if firmware_present => orbistoun_abi::enter::UnknownFields::Zero,
        _ => orbistoun_abi::enter::UnknownFields::Markers {
            base: mapped_unknown_fields(),
        },
    };
    // **One field, poisoned, so its use names it.**
    //
    // Applied last so it wins over both the block's own markers and any field a run named:
    // this is the question "does the runtime touch field N at all", and it is only answerable
    // if nothing else has put something usable there.
    // The measured pointers for fields one to five, unless a run named its own fields or set
    // `HANDOFF_FIELDS` explicitly - either of those is a deliberate experiment and wins.
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
/// Its own base, distinct from every marker range this project uses, so a fault address says
/// *poisoned field* rather than being confused for a sentinel or a content marker.
pub const POISON_BASE: u64 = 0x0000_5E2A_0000_0000;

/// How far apart consecutive poisoned fields sit.
///
/// Sixteen bytes: far enough that a small displacement stays inside its own field's value and
/// close enough that the field number is legible in the fault address at a glance.
pub const POISON_STRIDE: u64 = 16;

/// The field this run poisoned, and what it holds.
///
/// # What the answer means
///
/// A run that faults on the value **used** the field. A run that faults somewhere else, or
/// does not fault at all, **never reached** it - which is as much of an answer, and the half
/// that is hard to get any other way.
///
/// One field per run, deliberately. Poisoning several at once answers "did it touch any of
/// these", which is a question nobody asked, and the first fault would hide the rest (D390).
fn poisoned_field() -> Option<(u64, u64)> {
    let raw = orbistoun_env::HANDOFF_POISON.get()?;
    let field: u64 = raw.trim().parse().ok()?;
    let value = POISON_BASE + field * POISON_STRIDE;
    let _ = writeln!(
        io::stderr(),
        "orbistoun: handoff field {field} holds {value:#x}, which nothing maps - a fault on it means the runtime used the field"
    );
    Some((field, value))
}

/// The same region, with every word naming where it was read from.
///
/// **The second depth.** A zeroed page behind a field answers zero, which names nothing; a
/// page of content markers answers a value that says which field and which offset, so a
/// runtime reading a pointer out of a structure it was handed faults on something that
/// describes the member rather than on a null (D369).
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
/// **The question markers cannot answer.** A marker behind a field says the guest read that
/// member; it cannot say what the guest then did with it, because using it as a function
/// pointer ends the run on an unmapped address with the arguments already gone (D375).
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
/// **Mapped and zeroed rather than absent.** A runtime reading an unknown field as a
/// pointer gets a null it can check and carries on to the next thing it needs, instead of
/// ending the run - and the address it read from still names the field, because the region
/// starts at the same base the markers are decoded against (D366).
///
/// Falls back to the unmapped base when the host refuses the reservation, which is the
/// older behaviour: stricter, and still says which field.
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
                // Leaked deliberately: the guest holds addresses inside it for as long as
                // it runs, and unmapping it underneath would turn a diagnostic into a
                // fault about this emulator.
                std::mem::forget(space);
                base
            }
            Err(e) => {
                let _ = writeln!(
                    io::stderr(),
                    "orbistoun: could not map the handoff structure's unknown fields ({e}) - they stay unmapped markers"
                );
                base
            }
        }
    })
}

/// `argc` and `argv`, read out of the process image already written to the guest stack.
///
/// The System V process image begins with the argument count, and the argument pointers
/// follow it - so a C `main` wants the first word and the address just past it. **Read
/// rather than constructed**: building a second copy would let the two disagree, and the
/// one on the stack is the one anything else in the guest will find.
fn main_arguments(entry_stack: u64) -> (u64, u64) {
    let Ok(at) = usize::try_from(entry_stack) else {
        return (0, 0);
    };
    // SAFETY: `entry_stack` is the start of a process image this run just wrote inside a
    // mapped, writable guest stack, so its first word is initialised and readable.
    let argc = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at)) };
    (argc, entry_stack.saturating_add(8))
}

fn starting_address(image: &Image, settings: &process::EntrySettings) -> u64 {
    let Some(at) = settings.at else {
        return image.entry();
    };
    let entry = image.base().saturating_add(at);
    if entry != image.entry() {
        let _ = writeln!(
            io::stderr(),
            "orbistoun: entering at {entry:#x} (image+{at:#x}), not the declared entry {:#x}",
            image.entry()
        );
    }
    entry
}

/// Where the main thread's thread-local block is reserved.
///
/// A separate arena from the image (`0x4000…`), the guest stack, and the mapping arena, so a stray
/// thread pointer is recognisable by its address alone - the same reasoning the other bases carry.
const MAIN_TLS_BASE: u64 = 0x0000_6900_0000_0000;

/// Where **spawned** threads' thread-local blocks are reserved, one after another.
///
/// Its own arena, clear of the main block (`0x6900…`), the thread stacks (`0x6100…`), the reentrant
/// stacks (`0x6800…`) and the mapping arena (`0x7400…`), so a stray thread pointer still names its
/// kind by its address (worklog 286).
const THREAD_TLS_BASE: u64 = 0x0000_6A00_0000_0000;

/// The next spawned-thread block base, bump-allocated and never reused.
static NEXT_THREAD_TLS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(THREAD_TLS_BASE);

/// The thread-local template - layout and initialised `.tdata` - captured when the main thread's
/// block is built, so a spawned thread can build its own from it without re-parsing the image (the
/// kernel that spawns it cannot reach the loader anyway). `Some(None)` once a run with no
/// thread-locals has set up: nothing to build, and the hook stays a cheap no-op.
static TLS_TEMPLATE: std::sync::OnceLock<Option<(orbistoun_loader::tls::TlsLayout, Vec<u8>)>> =
    std::sync::OnceLock::new();

/// Installs the guest thread pointer for the thread about to enter, when the image declares
/// thread-local storage.
///
/// Reserves a block, copies the `.tdata` init image the loader already placed at `image base +
/// vaddr` (read from there rather than re-parsed, so the container's wrapper decode is not
/// repeated), lays out the block variant-II with the self-pointer at the thread pointer, and installs
/// the `fs` base - reading it back to confirm, because a disabled `FSGSBASE` faults on the write
/// rather than failing quietly. `Ok(None)` when the image declares none, which is an ordinary answer:
/// plenty of executables use no thread-locals at all.
///
/// The reservation is deliberately leaked: the block must outlive the guest, which runs until the
/// process ends, and freeing it on the way out of this function would unmap it under a running guest.
///
/// **This installs the base once, which is enough on Linux and not on Windows.** A Linux kernel with
/// `FSGSBASE` saves and restores the real `fs` base across context switches, so a base written here
/// survives for the run. Windows does not: it resets the user `fs` base to zero on the next context
/// switch (measured directly - a base written and read back correctly reads back as zero after a 2 ms
/// sleep). So on Windows this block, layout and priming write are the correct foundation but do not by
/// themselves keep guest thread-locals alive - that needs re-installing the base after each context
/// switch, which install-once cannot see (recorded as the next step, not pretended here).
/// Builds a thread-local block for the **calling** thread at `base_arena`, installs its `fs` base,
/// and remembers it for the backstop - the effectful half both the main thread and every spawned
/// one need. The template (`layout`, `tdata`) is the same for every thread of a run; only the arena
/// differs.
fn build_tls_block(
    base_arena: u64,
    layout: &orbistoun_loader::tls::TlsLayout,
    tdata: &[u8],
) -> Result<u64, String> {
    let alloc = layout.allocation_size();
    let alloc_len = usize::try_from(alloc)
        .map_err(|_| "thread-local block does not fit a pointer".to_owned())?;
    let region = GuestStack::reserve(base_arena, alloc)
        .map_err(|e| format!("could not reserve a thread-local block: {e}"))?;
    let base = region.lowest_usable();

    // SAFETY: `region` is freshly reserved and committed read-write at `base` for `alloc_len` bytes,
    // and it is leaked below, so the block outlives every read the guest makes through it.
    let dest = unsafe { std::slice::from_raw_parts_mut(base as *mut u8, alloc_len) };
    let tp = layout.render_block(base, dest, tdata);
    std::mem::forget(region);

    // SAFETY: sets the calling thread's `fs` base to the block just built, which is what that
    // thread reads its thread-locals through.
    unsafe { orbistoun_abi::thread_pointer::install(tp) }
        .map_err(|e| format!("could not install the thread pointer: {e}"))?;
    match orbistoun_abi::thread_pointer::current() {
        Some(v) if v == tp => {
            // Remembered so the fault handler can restore it after a host context switch drops it
            // (D433). On Linux, where the base survives, this is recorded and never needed.
            tls_backstop::remember(tp);
            Ok(tp)
        }
        other => Err(format!(
            "the thread pointer read back as {other:x?}, not the {tp:#x} that was written"
        )),
    }
}

fn install_main_thread_tls(image: &Image, bytes: &[u8]) -> Result<Option<u64>, String> {
    let Some((layout, _index, vaddr)) = orbistoun_loader::tls::layout_of(bytes)
        .map_err(|e| format!("could not read the thread-local layout: {e}"))?
    else {
        // Remembered as "no thread-locals" so a spawned thread's hook has a definite answer.
        let _ = TLS_TEMPLATE.set(None);
        return Ok(None);
    };

    // The init image, copied out of the placed image. Only `init_size` bytes are the `.tdata`; the
    // rest of the block is `.tbss`, which `render_block` zeroes.
    let init = usize::try_from(layout.init_size).unwrap_or(0);
    let mut tdata = vec![0_u8; init];
    if init > 0 {
        let source = image.base().saturating_add(vaddr);
        // SAFETY: `source` is inside the placed, populated image - the loader copied `.tdata` there
        // during placement - and `init` is the header's own init size, so the range is within it.
        let placed = unsafe { std::slice::from_raw_parts(source as *const u8, init) };
        tdata.copy_from_slice(placed);
    }

    // Kept so a spawned thread can build its own block from it (worklog 286).
    let _ = TLS_TEMPLATE.set(Some((layout, tdata.clone())));
    // And register the hook the kernel calls at the top of every new guest thread, now that there
    // is a template for it to read - only a title with thread-locals needs one built per thread.
    orbistoun_kernel::thread::install_thread_start(set_up_this_threads_tls);
    build_tls_block(MAIN_TLS_BASE, &layout, &tdata).map(Some)
}

/// Builds a thread-local block for the calling **spawned** guest thread - the per-thread half of
/// what [`install_main_thread_tls`] does for the main one. Installed as the kernel's thread-start
/// hook, so it runs at the top of every new guest thread, before any guest code (worklog 286).
///
/// A plain `fn` with no captures, so it is a `fn()` the kernel can call; it reads the template the
/// main-thread setup stored. Silent when the image declared no thread-locals - the hook is
/// registered unconditionally and most threads then have nothing to build. A failure leaves a
/// thread that will fault on its first `fs:`-relative access, so it is said out loud, not swallowed.
fn set_up_this_threads_tls() {
    // The run's watchpoints go with every guest thread, not only the first.
    watchpoint::arm_this_thread();
    let Some(Some((layout, tdata))) = TLS_TEMPLATE.get() else {
        return;
    };
    let alloc = layout.allocation_size();
    // Spaced by the block's own size and a gap, so two threads' blocks never overlap; bump-
    // allocated and never reused, and a run makes far too few threads to exhaust the arena.
    let unit = orbistoun_mem::allocation_granularity().max(orbistoun_core::GUEST_PAGE_SIZE);
    let step = alloc
        .checked_next_multiple_of(unit)
        .unwrap_or(alloc)
        .saturating_add(unit);
    let base = NEXT_THREAD_TLS.fetch_add(step, std::sync::atomic::Ordering::Relaxed);
    if let Err(e) = build_tls_block(base, layout, tdata) {
        eprintln!(
            "orbistoun: could not set up thread-local storage for a spawned guest thread: {e}"
        );
    }
}
/// Everything the run is put under, planted before the guest starts, and the spans it may touch.
///
/// # Why this is one unit
///
/// The order inside it is load-bearing and the mistakes are invisible: a stack fill run after a
/// poke silently erases it, and the run then reports an ordinary result under a diagnostic that did
/// nothing (D229, D185). Keeping the sequence in one named place says that it *is* a sequence,
/// rather than leaving it as a stretch of `enter` that reads like setup.
///
/// The reason for a refusal is returned rather than written, so the halt stays in `enter` with
/// every other way entry can be declined and there is one place that decides what a refusal looks
/// like.
fn arm_diagnostics(
    stack: &mut GuestStack,
    image: &Image,
    module: &str,
) -> Result<experiment::Experiments, String> {
    // Every diagnostic this run is under. Read here rather than passed in: `enter` is
    // reached long after `prepare_diagnostics`, and threading one struct through four
    // signatures to avoid a second read of the environment would be the worse trade.
    let experiments = experiment::Experiments::from_env();

    // Filled before anything is written onto it, so the only zeros the guest sees are ones
    // something deliberately wrote. Refused rather than skipped on failure: a diagnostic
    // that silently did not run answers the question wrongly and confidently (D185).
    if let Some(byte) = experiments.stack_fill {
        if let Err(e) = stack.fill(byte) {
            return Err(format!(
                "could not fill the guest stack with {byte:#04x}: {e}"
            ));
        }
    }

    // **After the stack fill, not before.** A poke plants a value at one address and a fill
    // writes the whole stack; run the other way round the fill silently erases the poke, and
    // the run reports an ordinary result under a diagnostic that did nothing. The same
    // ordering makes the watch snapshot the state the guest actually starts from (D229).
    apply_memory_diagnostics(image, stack, &experiments);

    // Where an argument may safely be dereferenced for a dump. The same spans the fault
    // reporter names, reused: an address inside one of them is mapped by this process, so
    // reading it cannot fault - and an argument outside them is a length or a flag rather
    // than a pointer, which is the other half of what this filters (D194).
    //
    // **Asked of the stack rather than re-derived from the constants it was built with.**
    // This lived in `prepare_diagnostics`, which runs before the stack exists, so it
    // declared `(GUEST_STACK_BASE, DEFAULT_STACK_SIZE)` - and `reserve` puts a guard page
    // at the base with usable memory starting one page *above* it. The window was therefore
    // shifted down by a page: it offered the one page mapped specifically to fault, and
    // refused the top page of real stack.
    //
    // Not a rounding error. `libkernel::0x6abac2f3dc6f8cee` - the lead on the
    // `image+0xafc959` wall - is called with `0x600000800d38`, which lands in exactly the
    // page that was excluded, so every attempt to dump it came back as a bare scalar and
    // the argument looked like a count. Two copies of one span, and the copy that was wrong
    // was the one the diagnostic used (D217).
    orbistoun_thunk::install_readable_ranges(vec![
        image.span(),
        (stack.lowest_usable(), stack.len()),
    ]);
    // **The stack only, and deliberately not the image.** A forced write is the one thing
    // here that modifies guest memory, and the image's runs are protected after relocation
    // - so planting a value in a read-only one would fault inside the emulator and produce
    // a crash with no relation to the guest. An out-parameter lives on the stack anyway,
    // which is what this exists to test (D218).
    orbistoun_thunk::install_writable_ranges(vec![(stack.lowest_usable(), stack.len())]);
    // Told where the stack is, so `sceKernelIsStack` answers from the span this process
    // actually mapped rather than from the constants it was built with (D275).
    describe_environment(stack, module);
    // And where the image is: it lives in the loader's address space, which the kernel's runtime
    // map never sees, so without this `sceKernelVirtualQuery` refuses the guest's own code (D446).
    // **After the readable ranges, not before them.** This ran during loading, where the only
    // published span was nothing at all - so a watch on an image address, which is mapped and
    // has been since the module was placed, reported "did not exist when the guest started".
    // Copied before the guest can change it, so what it did is a comparison rather than a
    // guess; cheaper than a watchpoint and answering a different question - see `watch` (D586).
    if let Some((base, len)) = experiments.watch {
        watch::snapshot(base, len);
    }
    let (image_span_base, image_span_len) = image.span();
    orbistoun_kernel::note_region(image_span_base, image_span_len);
    Ok(experiments)
}

/// What runs just before the guest's entry when asked: the placed modules' initialisers, and the
/// sampler that says where the entering thread spends its time (worklog 852).
fn before_entry_if_asked() {
    start_placed_modules_if_asked();
    profile::watch_this_thread("guest main");
}

/// Runs every placed module's initialisers, when `ORBISTOUN_START_MODULES` asks for it.
///
/// # Why it says what it is about to do
///
/// A constructor is guest code and can fault. The first version of this reported afterwards,
/// and the first run faulted inside the third module - so the loop never returned, the line
/// never printed, and the intervention read as though it had not run. That is the ambiguity
/// D325 exists to remove, reproduced in a new diagnostic on its first use (D520).
///
/// What each module actually did is recorded as it goes and reported from `persist`, which
/// every ending reaches - so the per-module evidence survives a fault even though the total
/// below does not.
fn start_placed_modules_if_asked() {
    if orbistoun_env::START_MODULES.get().is_none() {
        return;
    }
    eprintln!("orbistoun: starting every placed module before entry");
    let (modules, initialisers) = orbistoun_kernel::start_every_placed_module();
    eprintln!(
        "orbistoun: started {modules} placed module(s) before entry, {initialisers} initialiser(s) ran"
    );
}

/// Starts a script-driven pad playing, if the run's controller configuration names one.
///
/// **Called at the entry path, not at process start**, so the script's clock (`at_ms` is
/// milliseconds from the start of the run) begins as the guest is entered rather than while the
/// title is still being placed and linked - which for a large title is seconds apart (D707).
///
/// A run with no scripted port installs nothing and a fresh worker process holds no prior
/// script, so a keyboard or gamepad run is untouched. Reading or validating a named script that
/// fails ends the run with the reason rather than entering on an input nobody wrote (D153).
///
/// # Errors
///
/// When the configured script cannot be read, parsed, or validated.
fn install_scripted_input(service: &Service) -> Result<(), String> {
    // A flip-keyed script counts the guest's own flips (D721), whichever run it is.
    orbistoun_input::script::install_flip_clock(orbistoun_video::flips_accepted);
    // A capture asked for before the launch starts here, with the run (D721).
    let armed = run_capture_input().clone();
    if let Some(to) = armed {
        capture_input(Some(&to));
    }
    // The run's own script first (D721), then one the configuration names.
    let named = run_input_script().clone();
    let chosen = match named {
        Some(path) => Some((path.clone(), orbistoun_service::read_pad_script(&path)?)),
        None => match service.paths() {
            Some(paths) => {
                let config = paths.config_file();
                let base = config.parent().unwrap_or_else(|| Path::new("."));
                orbistoun_service::scripted_pad(service.pads(), base)?
            }
            None => None,
        },
    };
    let Some((path, script)) = chosen else {
        return Ok(());
    };
    let steps = script.len();
    orbistoun_input::script::install(script);
    let _ = writeln!(
        io::stderr(),
        "orbistoun: playing input script {} ({steps} step(s)) on player 1",
        path.display()
    );
    Ok(())
}

/// The pad script the current run asked for itself (D721), set from its request and cleared as it
/// ends.
fn run_input_script() -> std::sync::MutexGuard<'static, Option<std::path::PathBuf>> {
    static NAMED: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);
    NAMED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A capture asked for before the run started, begun at its entry (D721).
fn run_capture_input() -> std::sync::MutexGuard<'static, Option<std::path::PathBuf>> {
    static ARMED: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);
    ARMED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The file input is being captured to (D721).
fn capture_file() -> std::sync::MutexGuard<'static, Option<std::fs::File>> {
    static FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);
    FILE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// **Captures what the guest reads from its pad into `to`, or stops with `None`** - only when asked
/// (D721): the toolbar's "capture input", or `Request::Run::capture_input`. A step is appended and
/// flushed as each change happens, so a run ending in a fault leaves the capture whole. Its flips
/// count from now. A file that cannot be opened is said, and nothing is captured.
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
            let _ = writeln!(
                io::stderr(),
                "orbistoun: input is not captured - {}: {e}",
                path.display()
            );
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
    let _ = writeln!(
        io::stderr(),
        "orbistoun: capturing input to {}",
        path.display()
    );
}

/// **Plays the pad script `script` from now, or stops with `None`** (D721) - the toolbar's
/// "playback input" while a title runs. A script that cannot be read or does not validate is said,
/// and nothing plays.
fn play_input(script: Option<&Path>) {
    let Some(path) = script else {
        orbistoun_input::script::clear();
        return;
    };
    match orbistoun_service::read_pad_script(path) {
        Ok(script) => {
            let steps = script.len();
            orbistoun_input::script::install(script);
            let _ = writeln!(
                io::stderr(),
                "orbistoun: playing input script {} ({steps} step(s)) on player 1",
                path.display()
            );
        }
        Err(why) => {
            let _ = writeln!(io::stderr(), "orbistoun: input is not played - {why}");
        }
    }
}

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

    // Refused rather than attempted. Jumping to an address that is not executable is
    // certain to fault, and the fault alone says nothing about why - whereas this names
    // the actual problem (D010). It applies to an overridden entry exactly as it does to
    // the declared one: a diagnostic that jumps into data reports a fault about itself.
    if !image.is_executable(entry) {
        return halt(
            output,
            Phase::Linked,
            format!("{summary}; not entered - {entry:#x} is not inside an executable segment"),
        );
    }

    // The guest's own files, mounted before it can ask for them. A title asks for
    // `/app0/game.bin` and the file is sitting next to the module that was just loaded -
    // the files were always there, there was simply nothing to hand them over (D165). The base
    // tree, its writable device overlays, and the title over `/app0` are established together and
    // in order by `sandbox::establish` - the order that, split apart, once cost a title its
    // textures (D269, D423).
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

    // Every diagnostic this run is under, and the spans they may touch, in the order that order
    // matters in - see `arm_diagnostics`.
    let experiments = match arm_diagnostics(&mut stack, image, module) {
        Ok(experiments) => experiments,
        Err(why) => return halt(output, Phase::Linked, format!("{summary}; {why}")),
    };

    let reporting = match install_reporting(&stack, &experiments) {
        Ok(active) => active,
        Err(why) => return halt(output, Phase::Linked, format!("{summary}; {why}")),
    };

    // Started before the jump, because after it this thread belongs to the guest. A
    // guest with every import unimplemented can settle into a loop waiting for something
    // that will never happen, and without this the run simply hangs - taking the call
    // trace with it, which is the one thing worth having (D066).
    install_limits(limits, module);

    // Written and flushed *before* the jump, deliberately. If the guest faults, this
    // process ends without another chance to speak, and a phase recorded only
    // afterwards would leave the parent unable to distinguish "never entered" from
    // "entered and died" - which is the single most useful thing it could know.
    write_message(
        output,
        &Event::Reached {
            phase: Phase::Entered,
        },
    )?;

    // What the program finds on its stack when it starts. Written before the transfer
    // because the entry point reads it at its first instruction - that is the whole
    // finding of D152, and there is no later moment at which this could be done.
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
    // access reads the host's - zero on Windows, whose TEB lives in `gs` (the wall PPSA28061 hit, a
    // `mov rax, fs:[0]` reading `0`). Refused loudly rather than entered blind: a title that declares
    // thread-locals and cannot get a pointer will fault on its first one, and this names the reason
    // instead (the honest-failure the thread-pointer mechanism was written around).
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

    // The float environment the console starts a title in: denormals-are-zero,
    // flush-to-zero, every exception masked. Guest code runs natively, so without this it
    // inherits the host thread's - which has DAZ and FTZ clear, and quietly computes
    // different answers for denormal inputs rather than failing anywhere visible.
    orbistoun_abi::enter::adopt_guest_float_environment();

    // **Ask whether an import-bound module needed initialising before the guest started.**
    // Off unless asked for. D515 starts a module when the guest loads it by name; a module
    // the loader placed and resolved imports against is never loaded by name and so never
    // started. Here, after protection and after the float environment, because a constructor
    // is guest code and this is the last point before the guest's own entry (D520).
    before_entry_if_asked();

    if entry_settings.convention == process::Convention::Process {
        // SAFETY: the image is fully relocated - checked above - and its text was made
        // executable by the protection pass. `entry_stack` is the sixteen-byte-aligned
        // start of a process image just written inside a mapped, writable guest stack.
        // This never returns; the fault handler and the time limit each persist the call
        // trace from the guest's own thread, which are the paths that actually fire.
        unsafe { orbistoun_abi::enter::enter_process(entry, entry_stack, argument) };
    }

    // SAFETY: as above, but called as an ordinary function - the other hypothesis about
    // what this entry point is (D153). `stack` and the image both outlive the call.
    let returned = unsafe {
        orbistoun_abi::enter::enter_guest_with_arguments(entry, entry_stack, argument, second)
    };

    // Persisted on the ordinary path as well, so a guest that stops by itself is
    // recorded exactly as fully as one that had to be stopped.
    report::what_the_guest_asked_for();
    // **The renderer, on the run path** (-36c0, D695). If the guest handed over a command buffer,
    // drive it to a headless graphics backend now the guest has returned - a real submission
    // reaching a real backend rather than only a report. The time-limit and call-budget endings make
    // the same call (worklog 814). **After the trace is collected**: rendering *takes* the
    // submission, and the trace's submission summary reads it - rendered first, the summary was
    // always empty.
    let trace = report::collect_calls(module, "Entered");
    render_and_emit_frame(output)?;
    report::persist(&trace);

    let calls = orbistoun_thunk::total_calls();
    let reported = if reporting {
        "fault reporting was active"
    } else {
        "fault reporting is unavailable on this host"
    };
    // **The opening, from the record kept for it** - not the head of the ring. The ring is
    // circular now, so its first entry is whatever the guest called eight thousand calls ago
    // rather than what it started with (D571).
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
    /// Shared, because two threads legitimately write to it.
    ///
    /// **The argument [`Stopper`] makes, one step further.** The thread owning this handle
    /// is blocked reading events for the whole of a run, and a shell action has to be
    /// honoured *during* one - so the ability to send must be takeable before the handle is
    /// moved. A mutex rather than a second pipe: these messages are whole lines and rare,
    /// and the worker already reads them on a thread of its own (D310).
    stdin: std::sync::Arc<std::sync::Mutex<ChildStdin>>,
    stdout: BufReader<ChildStdout>,
}

/// Carries a shell action to a worker whose handle is busy.
///
/// Holds only the sending half, so it cannot read events and cannot be mistaken for a way
/// to drive a run. The one thing it does is the one thing that has to work while the run
/// thread is blocked.
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
/// # Why this is not just `Child::kill`
///
/// Killing needs `&mut Child`, and the handle is owned by whichever thread is blocked
/// reading events from it - which is exactly the thread that cannot act on a stop
/// request. So the *identity* is taken before the handle is moved, and the kill goes
/// through the operating system rather than through the handle.
///
/// Terminating rather than asking politely is correct here: the worker is blocked inside
/// arbitrary guest machine code that may never return, which is the whole reason a guest
/// runs in a process of its own (D032). There is nothing to unwind and nothing worth
/// saving - the trace is written from inside the worker as it goes, so a killed run keeps
/// everything it had already recorded.
#[derive(Debug, Clone, Copy)]
pub struct Stopper {
    // Read only by the Windows `stop()`; off Windows there is no way to signal the process,
    // so the identity is carried but unused. Precise about where it is dead rather than a
    // blanket allow that would also hide it going unused on Windows.
    #[cfg_attr(not(windows), allow(dead_code))]
    process_id: u32,
}

impl Stopper {
    /// Terminates the worker. Answers whether the request was accepted.
    ///
    /// A worker that has already exited answers `false`, which is not a failure - it is
    /// the ordinary race between a run finishing and somebody pressing stop.
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
        // Away from Windows this needs a signal, which this build has no dependency for.
        // Reported as refused rather than silently doing nothing, so a shim can disable
        // the control instead of offering one that lies.
        false
    }

    /// Whether stopping is possible on this platform at all.
    ///
    /// Offered so a shim can disable the control rather than present one that does
    /// nothing - a stop button that silently fails is worse than no stop button.
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

    /// Spawns the **current executable** in worker mode.
    ///
    /// Self-reinvocation rather than a separate binary: the worker is then literally
    /// the same build, so it cannot be a stale copy from a previous install.
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
            // stderr is left inherited on purpose: the worker's log should reach the
            // same place the shim's does, rather than vanishing into a pipe nobody
            // drains.
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
    /// Taken *before* the handle is moved into the thread that will block on it, exactly as
    /// [`Self::stopper`] is, and for the same reason.
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
    /// Terminal means [`Event::SurveyComplete`], [`Event::Terminated`], or
    /// [`Event::Failed`] - anything that ends the exchange.
    pub fn request(&mut self, request: &Request) -> io::Result<Vec<Event>> {
        self.request_streaming(request, |_| {})
    }

    /// [`Self::request`], handing each event to `on_event` as it arrives (worklog 841) - so a front
    /// end shows a presented frame while the run is still going rather than after it ends.
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
                Event::SurveyComplete(_) | Event::Terminated { .. } | Event::Failed { .. }
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
    /// The stream closing with no verdict means the worker died mid-run - in practice,
    /// guest code faulting, which is the expected outcome for a long time yet. Falling
    /// silent here would be indistinguishable from a guest that ran and did nothing,
    /// which is the failure mode D010 exists to prevent. The furthest phase already
    /// announced is reported alongside, because "died having entered the guest" and
    /// "died while parsing" are different problems.
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
        // Best-effort: a worker that has already died is a successful shutdown, not an
        // error to propagate.
        let _ = self.send(&Request::Shutdown);
        drop(self.stdin);
        self.child.wait()?;
        Ok(())
    }
}

/// The configured memory settings, with the map-shape diagnostic applied over them.
///
/// # Why a diagnostic can reach a setting here
///
/// The map shape is ordinarily a *setting* - it says what machine the guest is shown, and a
/// person choosing one is configuring the emulator. But the shape the guest will accept is an
/// **open question**, ranked first of 277 by call volume, and answering it means running the
/// same title against different shapes and reading which offsets it queries next.
///
/// `MapShape` has had three variants since D218 and nothing ever selected between them: the
/// apparatus for that experiment was built and left unwired, so the question stayed open while
/// the function it blocks took 67.5% of every guest call ever recorded.
///
/// **A bad value is refused rather than ignored.** A misspelt shape silently falling back to
/// the configured one produces a run that looks like the experiment and is not - the same
/// failure `did_nothing` exists to catch one level up (D229, D356).
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
    match orbistoun_kernel::direct::MapShape::named(asked) {
        Some(shape) => settings.map_shape = shape,
        None => eprintln!(
            "orbistoun: {} is not a map shape ({}) - left as configured",
            asked,
            orbistoun_kernel::direct::MapShape::NAMES.join(", ")
        ),
    }
    settings
}

/// What the title's own index says about a guest path.
///
/// **Read once and kept.** The index is a file, so this is installed from here for the reason
/// `read_guest_file` is - and parsed on first use rather than at load, because a title without
/// one is the ordinary case and paying for it at every launch would be a cost for nothing.
fn look_up_in_index(guest_path: &str) -> Option<(u64, u64)> {
    use std::sync::OnceLock;

    /// Where a dumped title puts it.
    const INDEX: &str = "/app0/ampr_emu.index";

    static ENTRIES: OnceLock<Vec<orbistoun_fs::amprindex::Entry>> = OnceLock::new();
    let entries = ENTRIES.get_or_init(|| {
        // **Through the mount table to a host path, not through the guest's filesystem.** This
        // is orbistoun reading a file, not the guest reading one, and going through
        // `open`/`read` put it in the descriptor table and in the statistics the guest is
        // measured by - a run reported a cut-short read that was entirely orbistoun's own. A
        // diagnostic that changes what it observes is what principle 9 forbids, and this one
        // was changing the filesystem report (D595).
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
/// **Bounded by what the caller says it has room for**, and by what the file holds. A read that
/// filled more than the guest's buffer would corrupt whatever it neighbours, which is a fault
/// with no relation to the question being asked.
fn read_guest_file(guest_path: &str, address: u64, most: u64) -> Option<usize> {
    let handle = orbistoun_fs::open::open(guest_path)?;
    let at = usize::try_from(address).ok()?;
    let room = usize::try_from(most).ok()?;
    if at == 0 || room == 0 {
        return None;
    }
    // SAFETY: the guest supplied this address in a structure it built, and the caller has
    // checked it against the length in the same structure. The mapping is identity (D014), and
    // the guest is inside this call so nothing else is writing the range.
    let into = unsafe {
        std::slice::from_raw_parts_mut(std::ptr::with_exposed_provenance_mut::<u8>(at), room)
    };
    let got = orbistoun_fs::open::read(handle, into);
    orbistoun_fs::open::close(handle);
    got
}

/// Publishes by-name stubs for declared imports, when a run asks for them.
///
/// **Only under `ORBISTOUN_DLSYM_STUBS`**, so a run that did not ask behaves exactly as it always
/// has and the difference between two runs is the whole measurement. The map is built from the
/// import labels, which are `library::name`, and keyed by the bare name because that is what a
/// guest passes to `sceKernelDlsym` (D632).
///
/// Implemented names are left out: they are already reachable by name, and adding them here would
/// mean two entries for one function with nothing saying which won.
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
    eprintln!(
        "orbistoun: {} declared name(s) are resolvable by name under ORBISTOUN_DLSYM_STUBS",
        named.len()
    );
    orbistoun_thunk::install_declared_thunks(named);
}

/// Arms the imports a run named with `ORBISTOUN_DUMP`, and says what it armed.
///
/// **Counted per clause, exactly as the writes are.** A clause naming an import that is not
/// there dumped nothing and said nothing, so a run with no captured arguments read as "the guest
/// never called it" when it meant "nothing here is called that" - and the arming count is said
/// out loud because a list that matched and a list that did not otherwise produce identical
/// silence (D625).
///
/// matters is when the implementation is yours and you suspect it (D198).
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
        eprintln!("orbistoun: ORBISTOUN_DUMP matched no import called {name:?}");
    }
    let slots = forced.iter().filter(|f| **f).count();
    eprintln!("orbistoun: ORBISTOUN_DUMP armed {slots} of {total} stub slot(s)");
    // **Which labels, not just how many.** A `Gap::Captured` finding answers a question somebody
    // asked, so the reporter has to know which imports were asked about - otherwise it answers
    // for every import that happened to be dumped (D637).
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
/// **Every commercial title is stripped and this does nothing for one.** The open-toolchain
/// guests are not, and they are the ones this project runs most and reads least well at a fault:
/// `image+0x28a163` names a byte where the file could have named `obs_sink_open+0x73` (D628).
///
/// Read before entry, because the fault handler must neither allocate nor parse (principle 9).
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
    eprintln!(
        "orbistoun: the module names {} of its own functions, so a fault in it can say which",
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

    /// **Pad state survives the wire, and answers nothing on the way.**
    ///
    /// The one property the input transport can have before a layout exists to hand it to a
    /// guest: what the window said arrives intact on the far side. Asserted through the real
    /// protocol loop rather than by round-tripping the type, because the interesting failure
    /// is a message the worker does not route to the right place.
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

    /// **A shell action is answered without a word on the output stream.**
    ///
    /// The property that keeps a second writer off the pipe. A shell action is handled on
    /// the reading thread precisely because the handling loop may be inside the guest at
    /// that moment - and anything the reader wrote would interleave with whatever
    /// `run_guest` was in the middle of saying.
    ///
    /// Asserted as *no* event rather than as a successful one: a reply here would not fail
    /// a test that only checked the handshake still worked, and would corrupt the stream
    /// only under the timing this whole arrangement exists to support.
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

    /// Drives `serve` over in-memory pipes, so the protocol loop is tested with no
    /// process involved at all.
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

    /// **A stream that ends without a `Shutdown` is a hangup (D715).**
    ///
    /// The worker ends its process on a hangup, which is what stops a guest from outliving the
    /// window that launched it. If this stopped firing, an orphaned worker would keep running
    /// its guest and holding the executable open.
    #[test]
    fn an_input_that_ends_without_a_shutdown_is_a_hangup() {
        let (events, hung_up) = exchange_noting_hangup(&[Request::Hello {
            protocol_version: PROTOCOL_VERSION,
        }]);
        assert!(matches!(events[..], [Event::Hello { .. }]), "{events:?}");
        assert!(hung_up, "the peer went away and serve did not say so");
    }

    /// **And a `Shutdown` is not one.** The ordinary end must not reach the orphan exit, or every
    /// clean shutdown would leave with the orphan status.
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

    #[test]
    fn a_version_mismatch_ends_the_session_rather_than_continuing() {
        // Continuing would parse every later message against the wrong contract, which
        // is far harder to diagnose than an outright refusal.
        let events = exchange(&[
            Request::Hello {
                protocol_version: PROTOCOL_VERSION + 1,
            },
            Request::Shutdown,
        ]);
        assert_eq!(events.len(), 1, "nothing after the refusal");
        assert!(matches!(events[0], Event::Failed { .. }));
    }

    #[test]
    fn a_failing_request_does_not_end_the_session() {
        // A worker that exited on the first bad request would turn a recoverable
        // problem into a lost session.
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

    #[test]
    fn a_closed_stream_ends_the_loop_cleanly() {
        assert!(exchange(&[]).is_empty());
    }

    /// **Input is captured only when asked, and a capture is a script that reads back** (D721):
    /// with no capture asked for, what the guest reads goes nowhere; asked, it lands in the named
    /// file as it happens; stopped, nothing more is written.
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

    #[test]
    fn a_missing_guest_is_a_request_failure_not_a_halted_run() {
        // The distinction matters to anything reading the stream: `Failed` means the
        // request was wrong, `Terminated` means a guest was actually loaded and then
        // stopped. Collapsing them would make "the path was a typo" and "the emulator
        // cannot go further" look identical.
        let events = exchange(&[Request::Run {
            path: "no/such/file".into(),
            symbols_db: None,
            limit_seconds: None,
            call_budget: None,
            input_script: None,
            capture_input: None,
            staged: false,
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

    /// Only where a module lies, or the run's own flag, makes it staged (D722) - a library title
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

    #[test]
    fn a_real_container_reaches_placement_and_halts_honestly() {
        // A silent stop would look like a guest that ran and did nothing, which is the
        // exact confusion D010 exists to prevent.
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

    /// A bare ELF with one loadable segment. **Generated, never extracted** (D051).
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

    #[test]
    fn the_worker_flag_is_declared_once() {
        // Spawning side and parsing side read the same constant, so they cannot drift.
        assert_eq!(WORKER_FLAG, "--worker");
    }
}

#[cfg(test)]
mod stop_wording {
    /// **The rung depends on two crates agreeing about one string, so this asserts they do.**
    ///
    /// `orbistoun-report` awards `Reach::Exited` by matching the worker's wording for a deliberate
    /// stop, and cannot depend on `orbistoun-core` to reference the enum that produces it. If the
    /// label is ever reworded on one side only, nothing breaks loudly: the rung simply stops being
    /// awarded and every run quietly falls back to `Entered`, which is the kind of silent
    /// mis-measurement principle 3 exists to refuse. This crate sees both, so the guard lives here.
    #[test]
    fn the_exit_wording_the_ladder_matches_is_the_wording_core_produces() {
        assert_eq!(
            orbistoun_overrides::DELIBERATE_EXIT,
            orbistoun_core::StopReason::Exited.label(),
            "the ladder matches a string core no longer produces - Reach::Exited is now unreachable"
        );
    }
}
