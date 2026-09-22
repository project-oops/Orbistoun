# 800. `sceKernelGetdirentries` is a 43-match-site directory-descriptor feature, disproportionate to its single call, so it is deferred; the session's accessible work is delivered and the loop drops to a heartbeat pending an obSCEne result or an operator's priority for the larger builds

**2026-09-22** — worklog 799 planned the directory-descriptor build. Scoping it settled that it is a large
feature, not a hardening tick, and that changes the honest thing to do next.

## The scope, measured

A faithful `sceKernelGetdirentries` needs a `Target::Directory` kind in `orbistoun-fs`'s descriptor table,
and the enum is matched at **43 sites** in `descriptor.rs` (`read`, `write`, `close`, `fstat`, `lseek`
and the rest), every one of which would need a directory arm; there is no fd→path map to build a lighter
version on (`opened.rs` notes paths for reporting, it does not map a descriptor to one). So it is a real
FS feature - a new descriptor kind threaded through every call that takes one - for a syscall PPSA04263
calls **once**, whose result likely does not gate its wall (the un-run static constructor, 794-796). A
bare `0` would be the plausible-empty lie principle 3 refuses, and the full build is disproportionate to
one uncertain call. Deferred, honestly, rather than half-built.

## The session, delivered

This run of the loop has produced, against the six retail titles:

- **A moved wall and the mechanism that moved it** (787): region-return in the knowledge file, which
  cleared two PPSA28061 register-defaults walls.
- **A comprehensive frontier survey** (788-796): every title mapped to its exact blocker, each
  buildable-looking lead (AGC descriptor, Ampr, GetIsTrinityMode, init-array, static construction-finder)
  tested and ruled out, the walls settled as obSCEne- or tracer-bound and correctly attributed as
  orbistoun's (D708).
- **Two honest exports** (797-798): `scePthreadGetaffinity` and `sceCoredumpRegisterCoredumpHandler`,
  turning placeholder-lies into real answers, PPSA04263's stubs 6 → 4.

What is left is not a per-tick item: the walls wait on an obSCEne measurement (age level, the CreateWorkload
trace REQ-...7a5d - both filed, neither answered) or an execution/branch tracer build; the remaining
exports need measured values or a large feature like this one. None is a clean five-minute grind.

## The loop drops to a heartbeat

So the cadence goes from a five-minute working pace to a ~20-minute heartbeat. Each wake checks the
cross-project bus for an obSCEne result - the event that actually unblocks a wall - and picks the grind
back up the instant one lands, or takes an operator's steer toward a specific large build (the
directory-descriptor feature, the execution tracer) that is worth doing deliberately rather than
speculatively. This is not concluding the titles are broken (they run on a console; the walls are
orbistoun's, D708) - it is pacing honestly to the work that actually remains.

## Gate state

No code changed - a scoping that measures `sceKernelGetdirentries` at 43 descriptor match sites and defers
it as disproportionate, records the session's delivered work, and drops the loop to a heartbeat pending an
obSCEne result or an operator's priority. `./bin/orbistoun check` green, worklog index regenerated,
identity scan clean. No commit.
