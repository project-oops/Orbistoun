# 792. PPSA04263's `image+0x19676d7` wall is a virtual call through a null vtable — an object allocated but never constructed; `START_MODULES=all` goes BACK, so it is the same computed-invocation un-run-construction wall as the other native titles

**2026-09-22** — after five ticks eliminating candidates on the AGC-descriptor titles, this tick gave
the least-recently-examined retail title, PPSA04263 (GTA), a fresh characterisation. Its wall is the
native un-run-construction pattern, which completes the survey: all six retail titles are now mapped to
tool-bound blockers from fresh angles this session.

## The fault is a null-vtable virtual call

`image+0x19676d7` disassembles to a C++ virtual dispatch:

```text
0x19676bf  mov  rdi, [rax + r14 + 0x2d0]  ; rdi = object pointer (array element field)
0x19676c7  test rdi, rdi ; je             ; non-null - passes
0x19676d4  mov  rax, [rdi]                 ; rax = object's vtable pointer  <-- 0
0x19676d7  call [rax + 0x58]              ; call virtual method  -> read of 0x58, fault
```

`r14 = image+0x5521c20`, `rax = [image+0x539eea0] * 0x4d0`, so `rdi` is the `+0x2d0` field of element
`idx` of a `0x4d0`-stride manager array. The object pointer is **non-null** (allocated, stored), but its
**vtable pointer `[rdi]` is zero** - the object's memory exists and is zeroed, and the constructor that
would write the vtable never ran. The report's "used `scePthreadMutexUnlock`'s answer as a pointer" is a
heuristic mis-attribution: the mutex calls return `0` correctly and the null is a vtable, not a return.

## Not module-init-gated

`ORBISTOUN_START_MODULES=all` (run every placed module's initialisers) sends the run **BACK** - 4 calls,
faulting immediately at a module stub (`the title's own modules+0x60f4`, reading a placeholder). So the
missing construction is not in a module initialiser orbistoun skips; it is in the eboot's own startup,
reached the same way the guest reaches everything else, and diverges there. Identical outcome to ASTRO
BOT (worklog 784) - eager module inits hit a stub sooner and never drive the missing construction.

## The retail frontier, surveyed end to end this session

Every retail title now ends at a wall needing a capability the single-tick loop does not have, each
re-confirmed from a fresh angle this session:

- **PPSA02664 / PPSA03416** (Unity): `array[1]+0x18` null in a workload copy loop - the AGC descriptor
  (zero-fill no-op, 789), the Ampr command buffer (fakelib no-op, 791) and module inits all ruled out;
  Unity workload-array state, trace-bound (REQ-...7a5d).
- **PPSA28061** (AGC): register-defaults answered (region-return, 787), now at libSceAmpr's
  `sceKernelMapperGetParam` abort - hardware-faithful (obSCEne a2f9/b7e2); needs the mapper's fill or
  populated register defaults.
- **PPSA04263** (GTA): this worklog - un-run C++ construction, null vtable, computed invocation.
- **PPSA21564** (ASTRO BOT): un-run arena-init, computed invocation (777-785).
- **PPSA25872** (Terminator): Il2Cpp metadata-indexed dispatch (771-775).

Three need un-run-construction traced through computed invocation (an execution trace from entry or a
reference diff); two need pending obSCEne hardware measurements; one needs the mapper fill. None is a
single-tick answer, and 787's region-return - the one wall moved since - was itself a small tool-build,
which is the shape the remaining progress takes.

## Gate state

No code changed - a characterisation that reads PPSA04263's wall as a null-vtable virtual call (object
allocated, never constructed), shows `START_MODULES=all` does not drive the construction, and completes
the six-title frontier survey. The `[experiment]` scratch the diagnostic runs wrote was restored.
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
