# 860. A direct-memory alias forgotten with its memory, and the in-game crash

**2026-09-25**. Neverball crashed entering its level-set screen. It now shows that screen, level
preview and all.

**Found with worklog 859's tools, which did what they were built for:**

- The operator's GUI run was recorded as it happened (41 steps), and the crash left the recording
  whole.
- The new fault section said the faulting page was `free - nothing mapped`, and that orbistoun's
  page guards had never changed it. That ruled out D717, D719 and D720 at once.
- `--input <recording>` reproduced the crash headless, at the same address.

**Cause.** `sceKernelMapDirectMemory` keeps a physical-offset to virtual-address table (D174), so a
guest that maps memory it still holds gets its data back.

- The table entry outlived the memory: neither `sceKernelReleaseDirectMemory` nor
  `sceKernelMunmap` forgot it.
- It answered regardless of length.
- Going in-game, Neverball released a span, allocated `0x40000` at the same physical offset, and
  was handed the old `0x10000` mapping. Its `memset` of the new buffer ran off the end into a free
  page.

**Fix:**

- An entry records the length it mapped, and answers only a map it covers.
- Release forgets entries in the span it frees.
- `munmap` forgets entries it unmaps.

Test: `an_alias_is_forgotten_with_its_memory_and_never_answers_a_longer_map`, watched failing with
release not forgetting. D174's own test (`one_physical_range_always_maps_to_the_same_address`)
passes.

**Replay determinism.** One replay of the recording followed the operator's exact menu route.
Another took a slightly different one and reached the same fault. The route is stable to within a
state change, which is what D721's flips can promise when the guest polls several times a frame.
