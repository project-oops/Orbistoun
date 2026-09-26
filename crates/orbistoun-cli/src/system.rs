//! Host-facing commands: `firmware`, `env`, `serve` and `paths`.

use anyhow::Result;
use orbistoun_service::Service;

/// `env` - every variable this build reads, and what is set right now.
///
/// # Why this is a command and not a paragraph in a document
///
/// It was a paragraph in a document, hand-copied from three decision entries, and that is a
/// second list. The registry is the first one, so this prints from it - a variable added
/// anywhere appears here without anybody remembering to write it down (D221).
///
/// Settings and diagnostics are separated because they are different kinds of thing: one
/// configures the emulator, the other changes the program in order to learn something and
/// is meant to go away afterwards.
/// Prints the firmware skeleton's libkernel layout: every export's stub kind and any overrun.
///
/// Uses the same pure planner the worker places from, so what this prints is what a run lays
/// down. The implemented set comes from the service's declared symbols, so an export shows as a
/// trampoline exactly when a run would give it one.
pub(crate) fn cmd_firmware_layout(service: &Service, all: bool) {
    use orbistoun_firmware::SlotKind;

    let implemented: std::collections::BTreeSet<String> = service
        .declared_symbols()
        .into_iter()
        .filter(|d| d.implemented)
        .map(|d| d.symbol)
        .collect();

    let exports: Vec<(String, u64)> = orbistoun_firmware::libkernel_exports()
        .iter()
        .map(|(n, v)| ((*n).to_owned(), *v))
        .collect();
    let plan = orbistoun_firmware::plan_layout(&exports, |name| implemented.contains(name));

    let (mut trampolines, mut unimplemented, mut collisions, mut confirmed) =
        (0_usize, 0_usize, 0_usize, 0_usize);
    println!(
        "libkernel laid out at {:#x}, {} exports",
        orbistoun_firmware::LIBKERNEL_BASE,
        plan.len()
    );
    println!(
        "{:>10}  {:<12}  {:<10}  {:<40}  NOTE",
        "VADDR", "KIND", "PROV", "EXPORT"
    );
    for p in &plan {
        match p.kind {
            SlotKind::Anchor => {}
            SlotKind::Trampoline => trampolines += 1,
            SlotKind::Unimplemented => unimplemented += 1,
        }
        let kind = match p.kind {
            SlotKind::Anchor => "anchor",
            SlotKind::Trampoline => "trampoline",
            SlotKind::Unimplemented => "unimplemented",
        };
        let prov = match orbistoun_firmware::libkernel_provenance(&p.name) {
            orbistoun_firmware::Provenance::Confirmed => {
                confirmed += 1;
                "confirmed"
            }
            orbistoun_firmware::Provenance::Candidate => "candidate",
        };
        let note = match &p.collides_with {
            Some((next, at)) => {
                collisions += 1;
                format!("OVERRUNS {next} at {at:#x}")
            }
            None => String::new(),
        };
        // By default a full 1,867-line dump is noise; show the anchor, the unimplemented ones
        // (the work list) and any collision unless asked for everything.
        let worth_showing = all
            || p.kind == SlotKind::Anchor
            || p.kind == SlotKind::Unimplemented
            || p.collides_with.is_some();
        if worth_showing {
            println!(
                "{:>10x}  {kind:<12}  {prov:<10}  {:<40}  {note}",
                p.vaddr, p.name
            );
        }
    }
    println!(
        "\n1 anchor, {trampolines} implemented, {unimplemented} unimplemented, {collisions} collisions"
    );
    println!(
        "{confirmed} vaddrs behaviourally confirmed, {} still candidate",
        plan.len().saturating_sub(confirmed)
    );
    if !all && unimplemented + collisions > 0 {
        println!(
            "(showing the anchor, unimplemented exports and collisions; --all for every export)"
        );
    }
}

pub(crate) fn cmd_env() {
    use orbistoun_env::Kind;

    let width = orbistoun_env::REGISTRY
        .iter()
        .map(|v| v.name.len())
        .max()
        .unwrap_or(24);

    for (kind, heading) in [
        (Kind::Setting, "settings - configure how orbistoun behaves"),
        (
            Kind::Diagnostic,
            "diagnostics - change a run to find something out, then go away",
        ),
    ] {
        println!();
        println!("{heading}");
        for var in orbistoun_env::REGISTRY.iter().filter(|v| v.kind == kind) {
            // The current value, because "what are the names" and "what is set" are the
            // same question in practice - somebody reads this when a run did not do what
            // they expected, and a stale variable from an earlier shell is a real cause.
            let state = var
                .get()
                .map_or_else(|| "-".to_owned(), |value| format!("= {value}"));
            println!("  {:<width$}  {state}", var.name);
            println!("  {:<width$}  {} ({})", "", var.summary, var.read_by);
            println!("  {:<width$}  e.g. {}={}", "", var.name, var.example);
        }
    }

    // The other half of "what configures a run", because somebody reading this list is
    // asking that question and the environment is the smaller half of the answer.
    println!();
    println!("most settings live in config.toml, not here - see `orbistoun-cli paths`");

    // **The reason the registry exists**, printed where somebody will see it rather than
    // only inside a run. A misspelled variable is not an error - it is an absence, so the
    // run reports an ordinary result and is believed.
    let unknown = orbistoun_env::unknown();
    if !unknown.is_empty() {
        println!();
        println!(
            "{} variable(s) set that look like orbistoun's and are not:",
            unknown.len()
        );
        for name in &unknown {
            println!("  {name}   - misspelled? nothing reads this");
        }
    }
}

/// `serve` - answer the conformance probe's command protocol.
///
/// # Why the key is printed rather than configured
///
/// It is generated per start and shown once, which is the same shape obSCEne uses and for
/// the same reason: a secret compiled in is shared by everyone holding the binary, and a
/// secret read from a file is one that outlives the reason it was created.
///
/// # Errors
///
/// When the address cannot be bound, or when a wide bind is asked for without a key.
pub(crate) fn cmd_serve(service: &Service, bind: &str, no_key: bool, once: bool) -> Result<()> {
    use std::net::TcpListener;

    // Refused rather than warned about. The two decisions - "no password" and "anything on
    // this network may invoke this" - are separate, and only the first one was made here.
    if no_key && !is_loopback(bind) {
        anyhow::bail!(
            "--no-key on {bind} would leave this open to anything on the network - bind to loopback, or drop --no-key"
        );
    }

    let listener = TcpListener::bind(bind)?;
    let shown = listener
        .local_addr()
        .map_or_else(|_| bind.to_owned(), |a| a.to_string());

    // Once, before anything can connect. A secret minted per connection is one no driver
    // could have presented, which would make the check unpassable rather than secure.
    let secret = (!no_key).then(orbistoun_service::respond::ServiceAnswers::generate_secret);

    println!("listening  {shown}");
    match &secret {
        Some(key) => println!("key {key}"),
        None => println!("key none - unauthenticated, loopback only"),
    }
    println!("serving report");
    println!("declining  call, read - no guest is loaded, so neither is announced");

    for incoming in listener.incoming() {
        let stream = incoming?;
        let peer = stream
            .peer_addr()
            .map_or_else(|_| "unknown".to_owned(), |a| a.to_string());
        println!("session {peer}");
        // A fresh backend per connection so the *session identifier* is new, carrying the
        // same secret so the key printed above stays the one that works.
        let per_session =
            orbistoun_service::respond::ServiceAnswers::with_secret(service, secret.clone());
        let mut responder = orbistoun_probe::respond::Responder::new(stream, per_session);
        if let Err(e) = responder.serve() {
            // Not fatal. A driver that disconnects mid-command is ordinary, and taking the
            // listener down with it would make every dropped connection look like a crash.
            println!("session ended: {e}");
        }
        if once {
            break;
        }
    }
    Ok(())
}

/// Whether an address is one only this machine can reach.
fn is_loopback(bind: &str) -> bool {
    use std::net::ToSocketAddrs as _;

    bind.to_socket_addrs()
        .is_ok_and(|mut addresses| addresses.all(|address| address.ip().is_loopback()))
}

pub(crate) fn cmd_paths() {
    let paths = orbistoun_paths::Paths::resolve();
    // Read from the one list rather than re-typed here. This was a second hand-written
    // enumeration of the same directories, which meant a new writable location could pass
    // the containment test and still never appear in the answer to "where did it go?"
    // (D215).
    let named = paths.named_dirs();
    // Measured, not typed. A ten-wide column was right until `screenshots` arrived and
    // pushed its own path out of line - the same class of thing as the list above, one
    // step smaller.
    let width = named
        .iter()
        .map(|(name, _)| name.len())
        .chain(["build", "mode", "data", "config", "library"].map(str::len))
        .max()
        .unwrap_or(10);
    // Which build this is, first. A path listing is what someone reads when an answer
    // surprised them, and the second question is always "which binary said that".
    println!("{:<width$} {}", "build", orbistoun_env::build::line());
    println!(
        "{:<width$} {}",
        "mode",
        if paths.is_portable() {
            "portable - everything lives beside the binary"
        } else {
            "installed - the platform data directory"
        }
    );
    println!("{:<width$} {}", "data", paths.data_root().display());
    for (name, dir) in &named {
        println!("{name:<width$} {}", dir.display());
    }
    println!("{:<width$} {}", "config", paths.config_file().display());
    // The one location here that is *read* rather than written, and the one nobody could
    // find out. A relative library root is joined to the data root above rather than to
    // the working directory, so "where does it look for titles" has a single answer -
    // which is exactly what it did not have (D228).
    let library = orbistoun_service::FileConfig::load(&paths.config_file())
        .unwrap_or_default()
        .library
        .resolve(paths.data_root());
    println!(
        "{:<width$} {}{}",
        "library",
        library.display(),
        if library.is_dir() {
            ""
        } else {
            "   (not a folder - nothing will be found)"
        }
    );
}
