# D416 - Four HLE fixes from the hardware-vs-orbistoun obSCEne diff


**measured** - 2026-08-31

The latest obSCEne hardware run (a module build, firmware 12.40) diffed against the same suite in
orbistoun gave a concrete, ground-truthed defect list. Four fixes, each verified by the verdict
flipping when obSCEne re-runs in orbistoun:

- **`sceKernelWrite(_, _, 0)` returns 0** (`orbistoun-fs`). `guest_slice` rejects a zero length the
  way it rejects a null pointer, so a zero-length write returned `InvalidArgument` - a non-zero
  value `000-boot/write-returns-count` reads as a claim to have written bytes. Handled before the
  slice, still consulting the descriptor so an unwritable one refuses.
- **`rand`/`srand` implemented** (`orbistoun-libc`). Unimplemented, `rand` fell to the placeholder
  stub and answered the same number every call - `035-libc/rand-seeded` catches that as two
  identical draws. An atomic LCG, seedable so a re-seed reproduces a series; faithful *sequence* is
  not what the check measures.
- **`sceKernelGetSystemSwVersion` implemented** (`orbistoun-kernel`). Unimplemented it refused;
  hardware fills a `{size; char[0x1c]; u32 version}` struct. Written with the 12.40 this machine
  models (`0x1240_0000`), `130-layout/system-software-version` now passes.
- **`sceKernelLoadStartModule` returns real handles** (`orbistoun-kernel`). It returned `-1` for
  everything; hardware answers `0x2001` for libkernel, a handle for an `/app0` module, and
  `0x8002_0002` for a `/system` one. Classified by path so `110-modules/load` matches (verdict
  `0x0` -> `0x3`). **The honest-failure catch:** a first cut handed a handle to any non-system path,
  so `060-module/load-rejects-missing` (which loads a bogus path) reported a nonexistent module as
  loaded. Corrected to recognise only libkernel, `/system`, and `/app0`, refusing anything else.

Two divergences the diff surfaced were deliberately *not* changed. `sceKernelGetModuleInfo` still
refuses - hardware refuses it too in this run (`110-modules/names fail 0x8002_0016`), so the
principled refusal is correct, not a gap. And the TSC frequency differs by 79 Hz (`0x5f259b8e` vs
`0x5f259bdd`), which is boot-calibration variance, not a bug - the lesson there is that obSCEne's
`139-exports` exact-match on it is too strict, an obSCEne fix.

Recorded `measured`: each is grounded in a hardware reading and confirmed by re-running the probe.

