# 787. PPSA28061's register-defaults descriptors are answered with reserved count-zero regions, shipped as `assumed` knowledge; the wall moves twice (GetRegisterDefaults2 -> Internal -> abort), +67 calls

**2026-09-21** — worklog 786 decoded `sceAgcGetRegisterDefaults2`'s return structure and named a
`count = 0` descriptor as the honest, testable minimal answer, planning to implement it "in
`orbistoun-gpu` implementations()". This tick did the reverse engineering that **corrects that plan**,
found the right mechanism, built the missing piece to ship it, and measured the result: the wall moved
twice and PPSA28061 executed 67 more calls before reaching an `abort`.

## The plan correction: it is not an `implementations()` handler

Two findings from the call site turned worklog 786's approach on its head.

- **No output buffer is passed.** Disassembling the caller (`image+0x10eb44`) shows `mov edi, 0xd; call`
  - only the set-id in `edi`, nothing else set. The `(u32, ptr, u32, ptr, ptr, ptr)` the report inferred
  is stale registers, not arguments. So the function returns storage **it owns**, not a caller buffer it
  fills. A Rust `implementations()` handler cannot answer this: those run on the guest's stack with no way
  to reserve guest memory (principle 9, D300), so they can only write through a pointer the guest already
  handed them. This function hands the pointer *out*.
- **`count = 0` is a real, title-designed branch, not a fake success.** The full wrapper (`image+0x10b9e0`)
  reads `count` at `+0x38`; when it is zero it takes its own `je` (`0x10b9ef -> 0x10ba2b`), writes `-1`
  (unset) into ~90 register-default slots at `image+0x1f5a88..` and continues (`jmp 0x10cbc3`). It applies
  no override for register `0xe24f806d` and walks on. That is the title handling "no defaults available",
  distinct from the placeholder-as-pointer that crashes.

## The mechanism it needed, and the gap that was closed

Answering a pointer to memory orbistoun owns is the **region-return** the service already builds: it
reserves guest address space before the guest starts and hands the base back (`StubRegion { via = return,
bytes }`, D295/D300). A fresh reservation is zero-filled, so `*(u32*)(base + 0x38) == 0` - exactly the
`count = 0` the title's own branch is waiting for.

The gap: a region could only ship through the per-machine `learned.toml`, which **only a measurement**
writes. A non-export no probe can call (worklog 769) has no measurement and needs a shipped, `assumed`
home. That home is the knowledge file, and it did not reach the region mechanism. Closed here:

- `FunctionKnowledge` gains a `region: Option<StubRegion>` field, and `is_bare` counts it (a region is a
  behaviour claim, so it carries a provenance like any other).
- `Knowledge::region_policy()` collects the declared regions into a policy carrying each entry's own
  `known_by`, and the worker absorbs it **under** a person's file and this machine's learned cache
  (`policy.absorb(Knowledge::builtin().region_policy())`), so a run resting on one is accounted for as
  `assumed`, never mistaken for evidence (D557).
- A test drives it both ways: a declared region reaches the policy with its provenance; a `returns` kind
  does not become a region (a guard watched to fail).

## Measured: the wall moves twice

`sceAgcGetRegisterDefaults2` recorded with `region = { via = return, bytes = 256 }`, `known_by = assumed`:

- **First run: FURTHER.** `sceAgcGetRegisterDefaults2(0xd) -> 0x6b0000000000` (region reserved and
  returned); the guest read `count = 0`, took the graceful branch, and advanced to a new fault at
  `image+0x10ebc9` - now calling **`sceAgcGetRegisterDefaults2Internal(0xd)`**, the sibling variant,
  unimplemented, dereferenced at `+0x38` in the identical pattern. imports 19 (+1), 327 calls (+1).

The Internal variant's call site (`image+0x10ebc0`) is byte-for-byte the same shape - `count` at `+0x38`,
`je 0x10ec0b` on zero, its own `-1` slot table at `image+0x1f6150..`, searching for a *different* register
(`0x6ac156ef`). Same contract, same graceful floor, already in the arity table. So it got the same entry.

- **Second run: FURTHER.** Both descriptors accepted, the guest executed **67 more calls** across **6 more
  imports** and now reaches an `abort` call (was `image+0x10ebc9`). imports 25 (+6), 394 calls (+67); 390
  of 394 answered by an implementation.

The second observation D226 asks of an intervention: the guest did not accept the pointer and crash at
random - it executed the count-zero branch logic and reached the *next semantically related* call (the
`Internal` variant, then 67 calls of init), which is coherent forward motion, not a walk. The region is
`assumed`, so the run is on the helped side; that is honest and recorded.

## Next

The wall is now an `abort` the guest calls itself, past both register-defaults descriptors - a different
kind of wall (the title deciding to stop, likely an assertion in the +67 calls of init). Read what it
prints and what call precedes it; that names whether a downstream answer is wrong or a real defaults table
(a populated descriptor for set `0xd` sourced from Mesa/ISA reset state) is now what is gating. Either way
the register-defaults path is no longer the frontier.

## Gate state

Code changed: `orbistoun-hle` gains a shipped `region` on knowledge entries and `Knowledge::region_policy()`;
the worker folds it into the run policy; two `libSceAgc` entries (`sceAgcGetRegisterDefaults2`,
`sceAgcGetRegisterDefaults2Internal`) answer a `count = 0` reserved region, `known_by = assumed`. PPSA28061
moves FURTHER twice. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
