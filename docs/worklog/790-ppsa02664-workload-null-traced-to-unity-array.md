# 790. PPSA02664's `image+0x3f8f0` null base is traced to a Unity workload-array element (`array[1]+0x18`), filled by the engine, not the AGC descriptor; the trace premise on the bus is corrected

**2026-09-22** — worklog 789 decoded the fault as a count-gated copy loop through `[r15+0x18]` and
disproved the empty-descriptor escape. This tick traced `r15` to its origin, which reframes the wall:
the null is a **Unity engine workload-array element**, not the AGC descriptor, so filling the descriptor
(the standing obSCEne premise) will not clear it.

## `r15` is `array[1]`, indexed by the engine

The tail-call into the loop (`0x385e0 -> 0x3f8b0`) and the getter it uses fix `r15` exactly:

```text
0x385e0  add rdi, 0x38 ; call 0x3fdd0 ; mov rdi,rax ; ... ; jmp 0x3f8b0
0x3fdd0  mov eax,esi ; imul rax,rax,0x70 ; add rax,rdi ; ret   ; -> rdi + index*0x70
```

So `r15 = (unity_obj + 0x38) + index*0x70` - element `index` of a `0x70`-byte-stride array at
`unity_obj+0x38`. The Unity caller (`0xf4f47c`) passes `index = 1` (`esi`), `count = 1` (`ecx`), and a
**stack** source `[rsp+0xe8]` (`r8`). The loop copies `count` 16-byte entries from that stack source into
`array[1] + 0x18`, and `array[1] + 0x18` is the null it stores through.

**Both the count and the index come from the engine, not the descriptor.** That is why zeroing
`0x71040c4df8235e1d`'s descriptor left the count non-zero and the wall identical (worklog 789): the copy
is driven entirely by Unity's own workload state.

## What the wall actually is

`array[1] + 0x18` is a per-workload-element **entry buffer** that the engine allocates and the copy loop
fills. On orbistoun's path it is null - never allocated - so the store faults. The AGC descriptor cluster
(`0x71040c4df8235e1d` fills it, `memcpy` copies it, `0x7d86501b8094ef57` sizes it) sits *beside* this in
call order but does not set `array[1]+0x18`: the report's "the guest used `0x7d86501b8094ef57`'s answer
as a pointer" is a heuristic mis-attribution here (that call is in a different function, `0x3f000`, whose
result is stored to `[rbx+0x10]`, not to the array element).

So the gap is upstream, in whatever allocates each `unity_obj+0x38` element's `+0x18` buffer - Unity
engine state established before the copy, which orbistoun runs natively. Finding the allocation (and the
AGC/kernel call it depends on) is a multi-level trace through the engine caller (`0xf4xxxx`), the same
computed/Il2Cpp-dispatch depth the native titles hit, or the CreateWorkload hardware trace (REQ-...7a5d).

## Bus corrected

The `0x71040c4df8235e1d` trace request asked only for the descriptor's 256 bytes, "enough to fill the
descriptor as a measured value." That is now known to be insufficient - the null is a workload-array
element buffer, not a descriptor field, and a filled descriptor may not allocate it. Filed an update on
the cross-project bus redirecting the eventual libSceAgc-mapped trace to the **whole CreateWorkload
sequence** (what allocates the element `+0x18` buffers), and noting the zero-fill disproof, so no
hardware time is spent synthesising a descriptor that cannot clear this fault.

## Gate state

No code changed - a decode that pins PPSA02664's null to `array[1]+0x18` (a Unity workload-array element,
engine-driven `count`/`index`, stack source), rules the AGC descriptor out as its direct source, and
corrects the obSCEne trace premise accordingly. `./bin/orbistoun check` green, worklog index regenerated,
identity scan clean. No commit.
