# 770. PPSA02664 and PPSA03416 share a clean AGC-descriptor wall the frontier hid as a VCRUNTIME diagnostic artifact; the root NID is unnamed and a trace is requested

**2026-09-21** — worklog 769 picked the `VCRUNTIME140.dll+0x1dc8d` wall shared by PPSA02664 and
PPSA03416 as the highest-leverage next target (one fix, two titles). A clean run of each shows that wall
was **not real** - it was recorded under a diagnostic - and the actual shared wall is an AGC
descriptor-build cluster rooted in an **unnamed** NID. That reframes the target and a hardware trace is
filed.

## The recorded wall was a diagnostic artifact

`docs/compat-frontier.txt` had both titles at `VCRUNTIME140.dll+0x1dc8d`. A clean run (no diagnostics)
faults instead at `image+0x3f8f0`, and the report says why:

```text
fault image+0x3f8f0   (was VCRUNTIME140.dll+0x1dc8d)
! this run was under no diagnostics and the last was under 0x7d86501b8094ef57 answers 0xdead0000,
  so this verdict measures a settings change
```

So the `VCRUNTIME140` position was reached only because a previous run **forced** `0x7d86501b8094ef57`
to answer `0xdead0000`. Under that forced value the title ran further, into its own C runtime; with the
honest return it stops earlier, at `image+0x3f8f0`. The frontier row was a forced-run wall wearing a
clean-run's clothes. Both titles' clean runs now record `image+0x3f8f0` at 100 % standing, which is the
real shared frontier.

## The real shared wall

Identical on both titles, `image+0x3f8f0`, a `write to 0x94` (a `null + 0x94` store), reached by one
call chain:

```text
libSceAgc::0x71040c4df8235e1d(0x6000007fc1e0) -> 0xf7ff0001   (unimplemented; placeholder)
libc::memcpy(dst, src=0x6000007fc1e0, n=0x100)               (copies the 256-byte descriptor)
libSceAgc::sceAgcSetCxRegIndirectPatchAddRegisters(...) -> 0x0
libSceAgc::0x7d86501b8094ef57(0x6000007fc278) -> 0x0          (the phantom GetSize, worklog 725)
-> write to 0x94                                              (image+0x3f8f0, the fault)
```

`0x71040c4df8235e1d` is meant to **fill a 256-byte descriptor** at its `arg0` (a stack region the title
memcpy's `0x100` bytes from immediately after). It is **unnamed** - present in `symbols/wanted.txt`, in
no knowledge file, and unimplemented - so it returns the placeholder range and the descriptor is never
filled. The null the `+0x94` write goes through is either a pointer field left zero in that unfilled
descriptor, or `0x7d86501b8094ef57`'s `0x0` return used as a pointer (orbistoun answers it as a GetSize,
worklog 725). Both are in the same descriptor-build cluster; a trace settles which.

## Why a trace, not an implementation

The descriptor's 256 bytes are a hardware fact and `0x71040c4df8235e1d` is unnamed, so neither a guess
nor a call-by-name probe reaches it. An obSCEne request is filed: trace the NID as PPSA02664 calls it on
hardware (args, the bytes written to `arg0`, the return), and confirm whether `0x7d86501b8094ef57`
returns a pointer at this site. Both titles render on a console (operator, 2026-09-19), so the call is
made there and the trace is obtainable.

## The frontier's shape, now mapped

With this, the retail frontier is fully characterised. Three titles (PPSA02664, PPSA03416, PPSA28061)
are blocked on **AGC functions needing a hardware measurement** - two of them the same descriptor
cluster, one `sceAgcGetRegisterDefaults2` (worklog 769). Three (PPSA04263, PPSA25872, PPSA21564) are at
**execution faults in title code** needing multi-level disassembly. None is a call the loop can name and
answer from a lawful source today; the AGC three are unblocked by the two obSCEne requests now filed, and
the execution-fault three are disassembly grinds. This is the "frontier exhausted at the loop cadence"
state worklog 521 named, re-confirmed with each title's exact blocker.

## Gate state

No code changed - a correction that unmasks a diagnostic-artifact frontier row, states the real shared
wall and its unnamed-NID root, files the trace request that unblocks two titles, and records the
frontier as measurement-or-disassembly-blocked end to end. `./bin/orbistoun check` green, worklog index
regenerated, identity scan clean. No commit.
