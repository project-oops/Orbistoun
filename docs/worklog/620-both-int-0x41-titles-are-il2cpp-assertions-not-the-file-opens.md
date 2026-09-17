# 620. Both `int 0x41` titles are il2cpp assertions, and the gate file opens are faithful - so the wall is a Unity invariant, not a value to name

**2026-09-16** - the taxonomy points `int 0x41` at the calls before it; for the two titles that hit it,
those calls are faithful, and the trap is one level deeper than the tooling can reach without symbols

## What the gates actually are

`int 0x41` is a measured-fatal guest trap whose cause is upstream (D699, worklog 611), and the finding
names the calls before it. Read for the two titles that reach it, they name **file opens of paths that
are faithfully absent**:

- **PPSA04263 (Grand Theft Auto V)** - `sceKernelOpen("/host//ap/rpf.cache") -> ENOENT`. `/host/` is a
  **devkit** path; retail has no such mount, so the console returns the same `ENOENT` and the title
  runs anyway. Its own `/app0` open was orbistoun's bug and is fixed (worklog 615); this one is not a
  bug.
- **PPSA25872 (Terminator)** - `/app0/Media/il2cpp.usym`, `/app0/Media/x64/il2cpp.usym`, `/devlog`. The
  `.usym` files are il2cpp **symbol** files, stripped from a retail build - and confirmed absent from
  the title's own `Media/` here, so the probe fails on retail too.

So for both, the immediately-preceding call the taxonomy points at is a **faithful failure**: the
console answers it the same way. The `int 0x41` is not caused by it.

## What the trap actually is

Disassembling the two trap sites shows the same shape, and it is not a kernel entry - it is a compiled
**assertion** in Unity's il2cpp runtime:

- GTA at `image+0x196b91a`: `cmp byte [rbx+0x19], 0x40; jne ...` then `int 0x41`.
- Terminator at `image+0x17554a3`: `cmp byte [rbp-0x39], 0x40 ... call [rbp-0x90]; mov rax,[rbp-0x88];
  test byte [rax+0x30], 0x10; jz +2; int 0x41` - the trap fires when bit 4 of `[rax+0x30]` is set.

The `0x40` byte test in both is libc++'s **short-string-optimisation flag** (a `std::string` is long
when that bit is set), so both traps live in string-heavy il2cpp code and fire on a violated il2cpp
invariant - the `int 0x41` is this platform's `abort`/`__builtin_trap` for a failed
`IL2CPP_ASSERT`. A title that runs on retail does not trip it there; under orbistoun some state il2cpp
depends on differs, and the assertion catches it.

## Why this is the finding, not a fix

The whole point of the reclassification (worklog 611) was that `int 0x41` is reached via an upstream
wrong value orbistoun fed the guest. For these two titles that value is **not** the file open - it is
whatever il2cpp state the assertion guards, several frames up
(`image+0x1755b55 <- 0x7b6081 <- 0x7b62dd <- 0x49421e` for Terminator). Pinning it is il2cpp-internals
reverse engineering - the type-metadata and string tables il2cpp builds at startup - which is a
separate, larger effort than naming a return value, and one no single run report resolves. Recorded so
the next session does not re-chase the faithfully-absent files (as this one nearly did with GTA's
`/host/` path): the lead is the il2cpp invariant, not the mount table.

## Gate state

No code changed - this is a characterisation of a wall, the deliverable being that both `int 0x41`
walls are one il2cpp assertion and the file opens beneath them are faithful. `./bin/orbistoun worklogs`
unique, identity scan clean.
