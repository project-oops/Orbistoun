# 626. The fs host-leak audit, measured: two exotic leaks reproduce, and none is on the `0x41` titles' path

**2026-09-16** - closing worklog 624's action item by measuring the remaining Windows/console fs
divergences through `std::fs` on this machine, and what the mostly-negative result means for `int 0x41`

## Why measure instead of read

Worklog 624 fixed case-sensitivity and left an action item: audit the `orbistoun-fs` host-call sites
for the other known divergences - `MAX_PATH`, reserved device names, trailing dot/space. The honest way
to audit a host-behaviour claim is to run it (the loop rule: measure before implementing). A probe
using Rust's own `std::fs` - the exact API `orbistoun-fs` calls, **not** the MSYS/Git-Bash layer, which
has its own POSIX emulation and would have lied - wrote and read back files in a temp directory and
reported what the host actually did. The first draft, run through MSYS, said `aux` created fine; the
`std::fs` probe is what a fix would actually face.

## What reproduced, and what did not

| divergence | result on this Windows 11 machine |
|---|---|
| `MAX_PATH` (a 441-char path) | **no leak** - created and read back fine, no `\\?\` needed (long-path support enabled) |
| `con`, `prn`, `aux`, `com1`, `lpt1` | **no leak** - each created a real file, read back its own bytes |
| reserved + extension (`aux.txt`, `con.dat`, `nul.png`) | **no leak** - all real files |
| bare `nul` | **leak** - write went to the null device, read back empty |
| trailing dot (`trail.`) | **leak** - the dot is stripped; `trail.` and `trail` collapse to one file |

Two leaks are live here: **bare `nul`** and **trailing dot/space**. Windows 11's device redirection is
far narrower than the classic `CON/PRN/AUX/NUL/COM*/LPT*` list the audit assumed - only bare `nul`
survived, and only without an extension.

## The surprise, and why it is the point

Two of the three assumed leaks **do not reproduce** on this configuration. Had I implemented the
verbatim-path plumbing the action item implied, most of it would have been for divergences that are not
active here - the exact wasted-effort the "measure first" rule exists to prevent. The measurement did
its job by *shrinking* the work, not growing it.

## Why the leaks that do reproduce are left open (D700)

Both live leaks are exotic. The Unity titles' paths - `/app0/il2cpp_data/Metadata/global-metadata.dat`
and the rest - contain no bare `nul` component and no trailing dot or space. **No current title reaches
either.** The single fix that closes them, the `\\?\` verbatim prefix, has to thread through
`mount::resolve`, whose result ~10 callers consume and some of them do path arithmetic on; and CI,
which runs on Linux, cannot verify it. Shipping that for near-zero reachability, unattended, is a
shortcut in reverse (principle 11) and speculative fidelity (principle 12). Recorded as **D700**, with
the exact reopening condition: a title observed to touch a `nul` or trailing-dot path.

## What this contributes to `int 0x41`

The audit was the actionable half of "tackle the `0x41`": the user's hypothesis was that a function
*implemented wrong* sends the guest down a bad path into the trap, and a host-semantic fs leak is
precisely "implemented wrong." The measurement **eliminates that hypothesis for these titles** - their
paths do not hit the two live leaks, so `orbistoun-fs` is not passing a host answer through on the way
to the il2cpp assertion (D699, worklog 620). Combined with worklog 620 (the gate file opens are
faithfully absent), the fs layer is now cleared as a cause of the `0x41` on GTA and Terminator. The
cause is upstream and inside il2cpp, and remains blocked on the il2cpp invariant, not on any fs fidelity
orbistoun still owes.

## The structural fix that still stands

Worklog 624's durable item - **run the full `cargo test` on the Windows runner**, so a divergence in a
*tested* path fails CI rather than a title - is unaffected and is the higher-value next step for this
class. It needs interactive CI iteration (it will surface pre-existing Windows-only failures to triage,
and a CI edit cannot be verified from an unattended loop without a push), so it is left for a session
that can iterate on CI rather than done blind here.

## Made to fail

Nothing to assert in code: the deliverable is a measurement and a recorded decision, not a behaviour
change. The probe is the evidence, and it is reproducible - `std::fs::write` to a bare `nul` reads back
empty, and to `trail.` collides with `trail`, on any comparably-configured Windows host. Had a fix been
shipped, its made-to-fail would create `nul`/`trail.` and assert a real, distinct file; that test is
specified in D700 for when the reopening condition is met.

## Gate state

No code changed. `./bin/orbistoun worklogs` unique, identity scan clean; the decision gate accepts
D700 (status `decided`, not left `RESERVED`).
