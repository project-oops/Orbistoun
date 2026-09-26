//! Host-facing commands: `firmware`, `env`, `serve` and `paths`.

use anyhow::Result;
use orbistoun_service::Service;

/// Prints the firmware skeleton's libkernel layout: every export's stub kind and any overrun.
///
/// Uses the same pure planner the worker places from, so this prints what a run lays down. The
/// implemented set comes from the service's declared symbols, so an export shows as a trampoline
/// exactly when a run gives it one.
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
        // By default show only the anchor, the unimplemented exports and any collision; the full
        // layout is noise unless asked for.
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

/// `env` - every variable this build reads, and what is set right now.
///
/// Printed from the registry, so a new variable appears without a second list to maintain (D221).
/// Settings configure the emulator; diagnostics change the program to learn something and are
/// temporary.
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
            // The current value too: a stale variable from an earlier shell is a common reason a
            // run misbehaves.
            let state = var
                .get()
                .map_or_else(|| "-".to_owned(), |value| format!("= {value}"));
            println!("  {:<width$}  {state}", var.name);
            println!("  {:<width$}  {} ({})", "", var.summary, var.read_by);
            println!("  {:<width$}  e.g. {}={}", "", var.name, var.example);
        }
    }

    // The other half of what configures a run.
    println!();
    println!("most settings live in config.toml, not here - see `orbistoun-cli paths`");

    // A misspelled variable is an absence, not an error, so the run looks ordinary; the registry
    // reports it here.
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
/// The key is generated per start and printed once, as obSCEne does: a compiled-in secret is shared
/// by everyone with the binary, and a secret in a file outlives its purpose.
///
/// # Errors
///
/// When the address cannot be bound, or when a wide bind is asked for without a key.
pub(crate) fn cmd_serve(service: &Service, bind: &str, no_key: bool, once: bool) -> Result<()> {
    use std::net::TcpListener;

    // Refused rather than warned: "no password" and "reachable from the network" are separate
    // choices, and only the first was made.
    if no_key && !is_loopback(bind) {
        anyhow::bail!(
            "--no-key on {bind} would leave this open to anything on the network - bind to loopback, or drop --no-key"
        );
    }

    let listener = TcpListener::bind(bind)?;
    let shown = listener
        .local_addr()
        .map_or_else(|_| bind.to_owned(), |a| a.to_string());

    // Minted once, before anything connects; a per-connection secret could never be presented.
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
        // A fresh backend per connection for a new session identifier, with the same secret so the
        // printed key keeps working.
        let per_session =
            orbistoun_service::respond::ServiceAnswers::with_secret(service, secret.clone());
        let mut responder = orbistoun_probe::respond::Responder::new(stream, per_session);
        if let Err(e) = responder.serve() {
            // Not fatal: a driver disconnecting mid-command is ordinary.
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

/// `paths` - show where orbistoun reads and writes.
pub(crate) fn cmd_paths() {
    let paths = orbistoun_paths::Paths::resolve();
    // Read from the one list of named directories, so every writable location appears here.
    let named = paths.named_dirs();
    // Measured, so every path lines up.
    let width = named
        .iter()
        .map(|(name, _)| name.len())
        .chain(["build", "mode", "data", "config", "library"].map(str::len))
        .max()
        .unwrap_or(10);
    // The build first: the next question after a surprising answer is which binary gave it.
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
    // The one location that is read rather than written. A relative library root joins the data
    // root, not the working directory (D038).
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
