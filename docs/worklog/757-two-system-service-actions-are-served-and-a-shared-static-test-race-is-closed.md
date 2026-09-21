# 757. Two system-service actions are served, and a shared-static test race is closed

**2026-09-21** — a fresh title picked up while PPSA02664's and PPSA28061's walls sit parked on
measurements. `./bin/orbistoun run PPSA04263` gives up at a fatal trap the guest reached through a
missing file, not through anything orbistoun answered wrongly — so its wall is a filesystem work
item, not a one-function fill. But the run flags five unimplemented stubs on the way, and two of
them are honest actions the console serves; the other three stay deferred, each for a reason already
written down. Serving the two turned up a pre-existing gate red that had nothing to do with them.

## PPSA04263's wall is not orbistoun's

The run ends at `image+0x196b91a`, reported as a read of `0xffffffffffffffff` — which the report
already knows is usually a general-protection fault, not a genuine read of -1 (D384). It is the guest
executing `int 0x41`, and the report is unambiguous about whose that is: obSCEne measured the same
trap fatal on hardware (REQ-…b3c2), so it raises a signal and does not return there either. The trap
is not the gap; the value that led the guest to it is. Reading the calls just before names it:

```text
just before: libkernel_fs::sceKernelOpen(0x6100080ffff0) -> 0x80020002   (ENOENT)
just before: libc::strchr / strncmp / strlen on "/app0/…"
orbistoun: the guest asked for 1 path nothing here holds:
  /host//ap/rpf.cache
```

The guest tried to open a file under `/app0`, was told it does not exist, and walked into its own
fatal assert. That is a **mount-table gap** — a file the platform has and orbistoun's mount table does
not — spelled by the thing that wanted it. It is real work, but it is filesystem work, not an API to
implement, so this tick banks the two clean API wins the same run surfaced and leaves the mount gap
named for a later one. (This is exactly the D708 discipline: the wall stays orbistoun's until
hardware proves otherwise, and here hardware already has — the *trap* is the title's, but the *reason*
the guest reached it is orbistoun's missing file.)

## What was served: two actions that succeed

Both live in `orbistoun-systemservice`, the crate that answered `sceErrorDialogInitialize` last tick,
and both are the same shape as that fill and as `sceSysmoduleUnloadModule`: an action, not a getter,
that takes a request and answers `OK` without writing an out-pointer or claiming anything happened.

- **`sceSystemServiceHideSplashScreen`** dismisses the boot-logo layer once a title is ready to draw.
  Nothing displays that layer through this path here, so hiding it is a no-op that succeeds — "nothing
  was paged in, so nothing is paged out." It was already declared at arity zero (hash-confirmed,
  D167); this adds the handler and the registration.
- **`sceSystemServiceDisableNoticeScreenSkipFlagAutoSet`** turns off a system flag's auto-set.
  Orbistoun keeps no such flag, so there is nothing to toggle — and a setter's contract is that the
  request was *taken*, not that a value was read back (D523). It was named but undeclared, so this
  declares it at the trampoline's arity six (not a claim about the real signature, D504 — the handler
  reads none of its arguments) and serves it.

Both answer `OK`, write nothing, and are recorded `known_by: assumed` with a note that a probe of the
real return retires the assumption. Answering `OK` rather than the loud `0xf7ff_0001` placeholder is
the point: a guest that checks either return and reads the placeholder takes an error path and skips
whatever the call was for, which is a lie in the direction that stops it.

## What stays deferred, and why it is not a gap left lazily

The same run flags three more stubs, and each is a deliberate non-implementation with a prior
decision behind it:

- **`scePthreadGetaffinity`** — D523 anticipated this call by name: the affinity *setter* can answer
  `OK` because pinning is the host scheduler's to decide, but the *getter* "needs a per-thread record
  and not a wider `Ok`," and orbistoun keeps none. Inventing a mask to hand back is the hack the
  standing instruction forbids.
- **`sceUserServiceGetGamePresets`** — already declared in the D346 block of deliberately
  unimplemented user-service getters: it writes a structure whose *meaning* is unmeasured, and a
  guessed layout is a wrong answer with no signature.
- **`sceKernelFstat`** — a stat struct filled from a file's real metadata. That is not a convention to
  answer `OK` with; it needs the file (the same file the mount gap above is missing) and a verified
  `stat` layout. Both are measurements, not assumptions.

The difference between the two served and the three deferred is the whole point of writing it down: an
action that succeeds is a convention; a value or a structure handed back is a measurement.

## The gate red that was not mine

With the two handlers in and `status --write` rerun (one new declaration and two new implementations
on top of the prior tick's uncommitted `970/769`, so the regenerated README and PROJECT_STATUS now
read `971` declared / `771` implemented), the full `./bin/orbistoun check` still went red — but on
`cargo test --workspace`, in `orbistoun-core::said`, a crate this change never touches:

```text
what_a_guest_says_comes_back_as_lines
  left:  ["cut off here", "finished"]
  right: ["beta", "alpha"]
```

`said` is a **process-wide static ring**, and two of its tests write it and then assert on its tail.
Run in parallel — the default — one test's lines land in the other's tail. It passes single-threaded
and fails multi-threaded, which is the signature of a shared-static race, and the third test in the
module already carries a comment acknowledging the static is shared. It is a pre-existing
test-isolation bug that reddens the gate on any unlucky scheduling, so it blocks a trustworthy green
gate every tick, not just this one.

Re-running to hope the race lands the other way is exactly the plausible-output hack §3 forbids, so
the fix is real: a `RING_WRITERS` mutex the two writers hold from their `note` through their
assertion, so no other writer interleaves (a silent reader cannot corrupt a tail, so it takes none;
poisoning is recovered from so one test's panic does not strand the rest). Three multi-threaded runs
in a row now pass.

## Gate state

Changed `crates/orbistoun-systemservice/src/lib.rs` (two handlers + registration + a test), the
`libSceSystemService` knowledge file (both entries recorded, `known_by: assumed`, retire-against-a-
probe), `crates/orbistoun-core/src/said.rs` (the writer-serialising lock, test-only), and the
regenerated `README.md`/`docs/PROJECT_STATUS.md`. The full `./bin/orbistoun check` reports
**`all checks passed`**. `cargo fmt --check` and `cargo clippy --all-targets` clean. Identity scan
clean. No commit.
