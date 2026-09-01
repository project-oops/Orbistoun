# D441 - dlsym handle validation and sysmodule id-0, from user direction on the D440 tail


**measured** - 2026-09-01 (user-directed, /loop)

The user asked for the two D440 "needs a decision" module checks to be answered in code, and confirmed
neo-mode is the existing Pro toggle. Both are now implemented and verified equal to hardware by the probe:

- **`sceKernelDlsym` rejects a bad module handle.** obSCEne passes `OBS_HANDLE_INVALID` (`-1`); hardware
  answers `ESRCH` (`0x80020003`), orbistoun ignored the handle and looked the name up globally. Now the
  handle is checked first: every handle this kernel hands out is non-negative (`0x2001`, then `0x40+`),
  so a value that is negative **read as a signed 32-bit int** names no module and earns `ESRCH`. The
  32-bit read matters - `-1` in an `int` argument leaves the high register half undefined, and `(x as
  i64) < 0` missed it when it arrived zero-extended; `(x as u32 as i32) < 0` is right. Valid (non-negative)
  handles, including 0, still resolve globally, so no real call regresses.
- **`sceSysmoduleIsLoaded(0)` reports unloaded.** Identifier 0 is not a loadable module; hardware answers
  `0x805a1000` (the sysmodule error base's unloaded code), orbistoun answered `0` (loaded). Now id 0
  answers `SYSMODULE_UNLOADED`; a real module id still answers loaded, since every module a title names is
  resolved by the loader (the D428 model), so this adds the invalid-id case without the regression risk of
  reporting linked modules as unloaded.

**neo-mode is not a code change.** "Neo" is Pro - the faster hardware revision - and orbistoun already
carries the toggle (`Machine::revision` = `Base`|`Pro`, set from the settings file, default `Base`, which
makes `sceKernelIsNeoMode` answer 0). The measured console answers 1, so matching it is presenting a Pro
machine via settings, not new code - left as a configuration choice because a guest told it is neo may
expect capabilities orbistoun does not provide.

The rejects-* error-code sweep is now complete: every one matches hardware. Kernel/systemservice tests
green; no title regression.

