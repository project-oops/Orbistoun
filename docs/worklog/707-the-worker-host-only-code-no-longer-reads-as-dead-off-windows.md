# 707. The worker's host-only code no longer reads as dead off Windows

**2026-09-19** — inbox `-caf9`, slice 1 (the `dead_code` slice): orbistoun's CI has been red on every
commit, and one of its four structural causes is a `dead_code` wall on the Linux and macOS runners.
`orbistoun-worker` carries a host-only subsystem - the Windows fault reporter, the process-terminating
`Stopper`, and the debug-register watchpoint arming - whose functions are live on the Windows dev
machine and unreachable off it. CI builds `-D warnings`, so every one of them is a hard error there.
This pass gates that subsystem to the platform that uses it, so it is **not compiled** off Windows
rather than merely `#[allow]`-ed dead.

## What was gated, and why not `#[allow(dead_code)]`

The request is explicit that the fix is to cfg-gate, not to allow: an `#[allow(dead_code)]` leaves the
code compiled and silences the signal, so a function that goes *genuinely* unused later stays hidden.
`#[cfg(windows)]` removes it from the build on the platforms where nothing calls it, which is the honest
statement - "this exists only where its caller does" - and keeps the dead-code lint working everywhere.

- **19 fault-report items in `report.rs`** - the reporter rooted at `emit`, reached only from the
  `#[cfg(windows)] mod imp` installer: `REPORTED`, `readable`, `host_module_of`, `instruction_bytes`,
  `bytes_before`, `NEWLINE`, `note_placeholder_fault`, `NEAR_NULL_FAULT`, `is_null_pointer_fault`,
  `note_emulator_fault`, `note_null_pointer_to_libc`, `serviced_interrupt`, `note_instruction_shape`,
  `emit`, `describe_pointees`, `printable_text`, `own_code_site`, `walk_frames`, `inside_import_name`.
- **The two `orbistoun_report::trace` imports they alone consume** - `Frame` (stack-walk records, for
  `walk_frames`) and `Registers` (for `emit`/`describe_pointees`) - moved to a `#[cfg(windows)]` `use`,
  or they read as unused imports on Linux once their only consumers are gone.
- **The `Stopper::process_id` field** (`lib.rs`), read only by the Windows `stop()`; off Windows there
  is no way to signal the process, so it is carried but unused. Annotated `#[cfg_attr(not(windows),
  allow(dead_code))]` - a field cannot be cfg'd away because the cross-platform `stopper()` constructor
  fills it, so the precise statement is "dead specifically off Windows", not a blanket allow that would
  also hide it going unused *on* Windows.
- **`watchpoint::Kind::bits`**, the `R/W` field only the Windows debug-control path writes -
  `#[cfg(windows)]`. Its sibling `length_bits` stays ungated: it is also read by a cross-platform
  validity check, so it is not dead off Windows.

## The test cascade this uncovered

Gating the functions broke the `#[cfg(test)] mod tests` on Linux: it imported and exercised those
functions unconditionally, so the imports became `error[E0432]: unresolved import` and the tests
referenced symbols that no longer existed. The fix mirrors the gating into the tests:

- The `use super::{…}` block was split - the cross-platform helpers (`Line`, `Region`,
  `describe_region`, `describe_unreadable`, `locate`, `published_envelope`) stay, and the five
  fault helpers moved to a `#[cfg(windows)] use super::{…}`.
- The three fault tests that drive them
  (`a_null_ish_fault_in_own_code_is_the_guests_libc_pointer_not_an_emulator_bug`,
  `a_software_interrupt_with_a_handler_is_serviced_and_resumes_past_it`,
  `a_software_interrupt_names_its_vector_and_its_measured_category`) each carry `#[cfg(windows)]`.
- In `mod pointee_tests`, only the one test that uses `printable_text` and its `use` were gated; the
  cross-platform breakpoint tests beside it stay ungated, so their coverage is unchanged off Windows.

## Verification, both directions

- **Linux (the failing runner's target):** `cargo check --target x86_64-unknown-linux-gnu --all-targets
  -p orbistoun-worker` is clean, and `cargo clippy` for the same target with `-D warnings` is clean -
  the whole `dead_code` wall for this crate is gone.
- **Windows (no regression):** `#[cfg(windows)]` is a no-op inclusion on Windows, so nothing is lost.
  `cargo check --all-targets` is clean and `cargo test -p orbistoun-worker` passes - the four gated
  tests still run and pass there (65 lib tests, all green), fmt and native clippy clean.
- **The rest of the workspace** cross-checks clean for Linux with `--all-targets` too (every member
  except the five that cannot be built from a Windows host - `orbistoun-cli`/`-gui` embed a Windows
  icon via a host-gated `build.rs`, and `-corpus`/`-llm`/`-propose` pull `ring`, which needs a Linux C
  cross-compiler this host lacks). Those five carry no cfg-driven `dead_code` exposure: four have zero
  platform-gated code, and `orbistoun-llm`'s six sites are all paired define-and-use (e.g.
  `CREATE_NO_WINDOW` is defined and used under `cfg(windows)`, so it simply does not exist on Linux).

## What this does and does not clear

This clears **slice 1** of caf9 - the `orbistoun-worker` `dead_code` that was the bulk of it - for the
Linux Check/Test jobs. It does **not** make CI green: three slices remain, each its own job -
`error[E0570]: "sysv64" is not a supported ABI` on macOS/aarch64, cargo-machete unused deps, and the
symbol-provenance audit. Those are larger and separate, and caf9 stays OPEN with this slice marked done.

## Gate state

`cargo check`/`clippy`/`test`/`fmt` clean for `orbistoun-worker` on both the Windows host and the
Linux target as above. No decision needed (this is mechanical gating, not a new concept). No commit.
