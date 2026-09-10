# 454. The handle was never consulted

**2026-09-08** - directed, continuing 453

Bus idle for a third pass: six requests open, two mine, none claimed, no sweep since 17:04.

## The loudest defect, half fixed and honestly so

`900-surface/control` invalidates ninety-nine census entries by itself, and behind it is a
divergence already on record: `sceKernelDlsym` resolves from one flat table and never consults the
module handle, so libkernel answers for `memcpy` where the console refuses with `0x8002_0003`.

The record said the fix needed a per-module export list this project does not have. That was half
right - it does not know what the console's libkernel exports, but it knows what **it declares** in
libkernel, which is enough to correct a wrong success without inventing a refusal.

The narrowing fires on three conditions together: handle `0x2001`, the name resolved from the stub
table, and its library is not the libkernel family. A name with no known library still resolves; a
name only the guest's own binary exports still resolves (PPSA02664 asks for its own
`scriptingGetMem` this way, D517); every other handle still resolves anything. It can turn a wrong
success into the measured refusal and cannot turn a working resolution into a failure on a guess
(D629).

The set is published by the service into `orbistoun-thunk`, beside the by-name stubs already
published there, because the three libkernel declarations live in three crates that must not reach
sideways (D536).

## And it does not fix the check that motivated it

`110-modules/symbol` does not use handle `0x2001`. It walks module paths calling
`sceKernelLoadStartModule` and asks whichever first succeeds - and under orbistoun every firmware
path is refused with `ENOENT`, so it gets an `/app0` handle from `0x40`. The measurement still
reports `0x0`.

Covering that would mean deciding what a *title's own module* exports, which orbistoun knows only
for the executable, and risking the resolution path the payload makes three hundred thousand calls
through. Not attempted on that evidence, and said here rather than left for somebody to discover
that the fix and the measurement never met.

The payload is unchanged either way: 223 imports, 100% standing, exits cleanly, same call count.

## The record it replaced was about to go stale

`dlsym_divergence.rs` asserted "orbistoun answers `0` here, and if this is no longer `0` the
OUTSTANDING entry needs rewriting". It is no longer `0` for that handle, so it was rewritten - and
it now asserts three states rather than one: **inert** before any list is published, the console's
`0x8002_0003` **with nothing written** after it, and a libkernel-declared name still passing
through. The third is there because a guard that refused everything would satisfy the second.

## Surprises

- **The knowledge base does not know where `memcpy` lives.** The first attempt at this used
  `Knowledge::library_of`, which covers documented functions rather than declared ones - so the
  narrowing compiled, ran, and refused nothing. The declaration tables are the source of truth for
  "which library", and they are only assembled in the service.
- **Every firmware module path is refused with `ENOENT`**, which is measured and correct, and it
  means no guest under orbistoun ever holds libkernel's real handle unless it uses the well-known
  `0x2001` directly. That is a bigger fact about module resolution here than the divergence was.

## Next

- What a title's own module exports, which is the other half of the handle question.
- The unapplied relocation behind `obs_sink_open` (D628), when loader work is in scope.
- The declined-syscall casualties, waiting on `REQ-20260908T1620Z-4c1e`.
- `sceAgcCreateShader`, waiting on `REQ-20260908T1621Z-7a5d`.
