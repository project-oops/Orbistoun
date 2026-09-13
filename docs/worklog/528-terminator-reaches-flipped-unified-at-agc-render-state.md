# 528. Terminator reaches flipped: the TempOverflow was graphics cascade; now unified at AGC render-state

**2026-09-13** - executing the out-parameter trace resolves the int 0x41 assert decisively

Executing the Terminator trace queued in worklog 525 (Section 2) produced a decisive advance
rather than an out-parameter edit: the `int 0x41` TempOverflow assert is gone, Terminator
has reached `flipped` (1 frame to output), and its true wall is the exact same Phase-6 AGC
render-state object model that stops PPSA02664 and Summer Sports.

## 1. What happened to int 0x41

Worklog 522 identified that `int 0x41` was Unity's out-of-memory assert on a garbage size
(`105553124636954 = 0x6000_007F_BEDA`). Running the title directly under `orbistoun-cli run`
reveals that the assert does not reproduce:

- The assert was not an unwritten size out-parameter in early memory setup.
- It was Unity's main thread reacting to an asynchronous error reported by the graphics
  worker thread when AGC shader linkage functions were stubbed.
- With the AGC shader linkage functions (`sceAgcCreateInterpolantMapping`,
  `sceAgcUpdateInterpolantMapping`, `sceAgcUpdatePrimState`, `sceAgcLinkShaders`) and
  `libSceAppContent` handlers in place, the graphics subsystem initialized successfully,
  proceeded past the temp allocator, and pushed a presentation frame.

## 2. The Advance

| Metric | Before (worklog 522) | Measured Now | Change |
| :--- | :--- | :--- | :--- |
| Reach | `entered` | `flipped` | **Advanced to flipped** |
| Frames | 0 | 1 | **+1 frame** |
| Imports | 153 (150 answered) | 192 (178 answered) | **+39 imports** |
| Calls | 321,965 | 339,537 | **+17,572 calls** |
| Outcome | `image+0x17554a3` | `image+0x3b32b9` | Moved |

`compat/PPSA25872-app0.toml`, `COMPATIBILITY.md`, `docs/titles/PPSA25872-app0.md`,
`docs/compat-frontier.txt`, and `docs/PROJECT_STATUS.md` have all been updated and
verified via the automated compatibility tooling (`status --check`, `frontier` test).

## 3. The New Wall: Unified at the AGC Render-State Routine

The new fault is `read of 0x100` at `image+0x3b32b9`. Capstone disassembly of the faulting site
and its caller chain (`0x11c288c -> 0x11913bf -> 0x3b328e`):

```x86asm
0x3b328e:  push   rbp
0x3b328f:  mov    rbp, rsp
0x3b3292:  push   rbx
0x3b3293:  push   rax
0x3b3294:  mov    rbx, rdi            ; rbx = destination render state
0x3b3297:  cmp    qword ptr [rdi], rsi
0x3b329a:  je     +0x9e
0x3b32a0:  mov    qword ptr [rbx], rsi
0x3b32a3:  test   rsi, rsi
0x3b32a6:  je     ...
0x3b32a8:  mov    rdi, qword ptr [rsi + 0x8]   ; rdi = state->sub_object (= 0xd8)
0x3b32ac:  mov    qword ptr [rbx + 0x8], rdi
0x3b32b0:  movzx  eax, byte ptr [rsi + 0x5a]
0x3b32b4:  mov    byte ptr [rbx + 0x30], al
0x3b32b7:  movzx  eax, word ptr [rdi + 0x28]   ; FAULT: read of 0xd8 + 0x28 = 0x100
```

This is byte-for-byte identical to the routine disassembled in worklog 517 for PPSA02664
(`image+0x3f258`) and Summer Sports PPSA03416 (`image+0x3f258`). The guest dereferences
`[rsi + 0x8]->+0x28` expecting a populated AGC render-state sub-object, which orbistoun's
stubbed AGC object layer does not populate (leaving the uninitialized `0xd8` field).

## 4. Architectural Consequence

All three retail Unity titles in the corpus (PPSA02664, PPSA03416, PPSA25872) now:
1. Initialize their platform services, app content, and display contexts completely.
2. Advance to `flipped` (1 frame to output).
3. Converge on the **identical Phase-6 AGC render-state object model** wall.

The retail frontier is consolidated: there is no longer a separate Terminator memory-trace
blocker. What remains between these titles and further execution is the Phase-6 AGC->Vulkan
engine and obSCEne's pending AGC object model data (`c7d1`/`a2f9`).
