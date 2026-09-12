# 517. PPSA02664 past common-dialog to the AGC render-state wall; and the AGC-wiring debt cleared

**2026-09-12** - operator: "go for PPSA02664", then "continue iterating the wall, log requests if you need data"

Pivoted from Earthion to PPSA02664, the best-positioned retail title
after Earthion turned out to need the Phase-6 GPU engine. PPSA02664 advanced, then reached the same layer
from the other side. Along the way, landing the change surfaced and cleared real wiring debt.

## The advance: common-dialog implemented (D678)

PPSA02664's honest wall was a graceful `exit`: it calls `sceCommonDialogInitialize`, is handed the negative
placeholder `0xf7ff0001`, prints `sceCommonDialogInitialize() failed`, and exits at 14,989 calls. The
knowledge file already carried the oracle (D670: returning `0` reaches +123 imports / +402,745 calls)
and three shipping Unity titles that stop here and run on a console. a3f7 resolved that obSCEne **cannot
measure** the return (the library is not linked into a title process by default), so guest-observed is
the ceiling of the evidence. Implemented `sceCommonDialogInitialize -> 0` in
`crates/orbistoun-systemservice/src/common_dialog.rs` (D678). Unlike the Earthion mapper it is a pure
status with no out-struct, so `0` is one honest value, not five.

**Result: 216 imports / 418,310 calls, up from the 14,989-call exit** - and above the old inflated
best-ever (199 / 417,670, recorded 2026-09-09 under the positive placeholder). A real, honest advance.

## The new wall: the AGC render-state object model (disassembled)

PPSA02664 now faults at `image+0x3f258`, `read of 0x100` - a null-plus-offset. `ORBISTOUN_WATCH` + capstone
show a render-state marshalling routine:

```x86asm
test rsi, rsi ; je ...              ; rsi (the draw/render state) is non-null
mov  rdi, [rsi+0x8]                  ; rdi = state->sub_object  (= 0xd8, garbage)
mov  [rbx+0x8], rdi
mov  al, [rsi+0x5a] ; mov [rbx+0x30], al
movzx eax, word [rdi+0x28]           ; FAULT: derefs the garbage sub-pointer at 0xd8+0x28 = 0x100
mov  rax, [rsi+0x28]                 ; another sub-object
movzx ecx, word [rax+0x16] ; movzx eax, word [rax+0x14] ; sub ecx, eax   ; 0x16-0x14 = a width/extent
```

The guest null-checks `rsi` but not `rsi->+0x8`, which it expects to be a valid sub-object (it reads a
16-bit field at its +0x28). The `[rsi+0x28]->0x16 - +0x14` pattern is a viewport/scissor extent. So
`rsi` is a draw/render state, and its `+0x8` sub-pointer is uninitialised because orbistoun's AGC layer
never populated it. Forcing the DCB marker/event functions to `0` does not help (verified) - the gap is
the render-state object model, not a marker's return. This is Phase-6 GPU work.

## Strategic: both retail titles converge on the Phase-6 GPU engine

Earthion (D677/worklog 516) needs a filled `sceKernelMapperGetParam` struct in its engine's must-succeed
gate; PPSA02664 needs a populated AGC render-state sub-object. Different entry points, same layer: the AGC
command-buffer / render-state / draw object model, which translates to Vulkan in Phase 6 and is not
built. The single-function measurements (create-shader, mapper, common-dialog) got these titles *to*
the GPU layer; getting *through* it is the GPU engine, not another one-off return. Logged the render-
state datum to obSCEne (below) in case the sub-object is a discrete measurable AGC object.

## common-dialog advanced three titles, not one

The knowledge file's three-title claim held: implementing `sceCommonDialogInitialize -> 0` moved all
three from their common-dialog exit, each FURTHER:

- **PPSA02664**: 14,989 -> 418,310 calls; faults at the AGC render-state site above.
- **PPSA03416 (Summer Sports)**: -> 470,305 calls (+143 imports); faults at the **identical**
  `image+0x3f258` render-state site - same Unity engine, same Phase-6 wall as PPSA02664.
- **PPSA25872 (Terminator 2D)**: -> 321,962 calls (+75 imports); faults **differently** - a software
  `int 0x41` at `image+0x17554a3` (GP fault), not the GPU object model.

## Terminator's wall is orbistoun-side: an APR index gap, not the GPU

Disassembled: `test byte [rax+0x30], 0x10; je +2; int 0x41` - a **conditional assert-trap** that fires
only when an error flag is set. The "just before" context is error-message formatting (a 776-byte
strlen, memcpy). The upstream cause: `sceKernelAprResolveFilepathsToIdsAndFileSizes` returns the
unimplemented placeholder because none of the guest's requested paths resolve -
`/app0/Media/il2cpp.usym`, `/app0/Media/x64/il2cpp.usym` (optional IL2CPP debug symbols, genuinely
absent) and `/app0/Media/RuntimeInitializeOnLoads.json`. **That last file exists on disk**
(`titles/PPSA25872-app0/Media/RuntimeInitializeOnLoads.json`) but is not in the title's
`/app0/ampr_emu.index`, which `look_up_in_index` matches by exact path - so a required, present file
resolves to nothing, APR returns the placeholder, and the guest asserts.

This is not a firmware question (no obSCEne request): the fix is orbistoun-side - APR should resolve a
file that exists on disk even when the dumped `ampr_emu.index` omits it (fall back to the mount table +
on-disk size), and needs a decision on what APR id to give a non-indexed file the guest may later read
back. The `.usym` files are genuinely absent and want a proper not-found rather than the unimplemented
placeholder. Left for the next iteration; it is the more tractable of the two retail walls (a kernel/FS
path, not Phase-6 GPU).

## Cleared: the AGC-wiring debt from de4218e/647952e

Landing common-dialog needed a green `orbistoun-service`/`-hle` suite, which exposed that the prior AGC
commits were tested only against `-p orbistoun-gpu` and had left:

- the 5 `libSceAgc` shader-linkage functions implemented in `agc::implementations()` but **undeclared**
  in the `guest_module!` and **unrecorded** in the knowledge file - fixed: declared the 4 missing ones,
  added 5 measured knowledge entries (from e4f1/9a41);
- `libSceAgcDriver`'s `CreateQueue`/`RegisterOwner`/`RegisterResource` implemented but unrecorded -
  added 3 measured entries (9a41/7b3c);
- both `libSceAgc` and `libSceAgcDriver` still listed in `SERVES_NOTHING` though they serve now -
  removed;
- `sceAgcCreateShader` and `sceKernelMapperGetParam` marked `measured` while still carrying
  `assumptions`, which the guard forbids - reconciled by moving their genuine open questions to the
  schema's `partial` field (and dropping create-shader's stale "no behaviour established" assumption
  that its own measured edge-cases contradict).

Full suite green: `cargo test -p orbistoun-service -p orbistoun-gpu -p orbistoun-systemservice -p
orbistoun-hle -p orbistoun-kernel` all pass.

## State and next

- Uncommitted; identity guard to run before any commit. Nothing committed (operator's call).
- Logged obSCEne request for the render-state sub-object model.
- Next honest lever for both retail titles is the Phase-6 AGC->Vulkan engine, not another single return.
