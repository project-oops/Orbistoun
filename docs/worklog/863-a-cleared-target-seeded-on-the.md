# 863. A cleared target seeded on the device

**2026-09-25**. Neverball's `draw setup` goes from ~64 to ~2 ms/s in-game. Its frames and
gl1-probe's results are unchanged.

**Why.** Each frame's first submission starts from a target the guest just cleared. Worklog 857
found it uniform without detiling, then still:

- built an 8 MB `Vec` of the one word;
- converted it to bytes;
- settled the device;
- wrote 8 MB into the attachment's host buffer;
- copied it in.

**The change.** The drawer is told what it starts from as `agc_driver::Before`: `Held`,
`Words(&[u32])`, or `Uniform(u32)`, which replaces `Option<&[u32]>`.

- `read_target` answers `TargetRead::Uniform(word)` without building anything.
- The worker calls `VulkanBackend::seed_target_uniform`, which runs
  `ResidentAttachment::reload_uniform`: `vkCmdFillBuffer` of the word into the attachment's buffer,
  then a copy into the image.
  - The fill writes exact bits, not a float clear with its unorm rounding.
  - It is recorded in queue order behind a transfer barrier, so nothing waits: the host never
    touches the buffer.
- `Before::to_words` expands a uniform word where a caller wants words (the per-submission
  write-back path, and tests).

Test: `a_uniform_reload_leaves_exactly_its_word_everywhere` (device). An 8x4 attachment seeded with
a ramp and refilled with `0x80402010` reads back `10 20 40 80` in every pixel. It was watched failing
with the fill word altered.

**In-game, measured.** 27-28 flips a second, unchanged, so the limit is elsewhere. A profile of the
main thread:

| where | share |
|---|---|
| guest code | 21% |
| waiting on the device thread | 17% |
| `fill` (the guest's colour and depth clears, ~16 MB a frame) | 8% |
| the thunk's per-call trace | 6% |
| page protection (`set_all`) | 5% |

**Tried and removed: filling large clears with four threads.** "Other memory work" stayed at ~93
ms/s, and the frame rate did not move. That span wraps each fill's protection work as well as the
memset (the flip guard's release, D720's release), and the memset itself is not the cost.

Protection changes are four a flip, each over the whole 8.7 MB target: the flip's guard, the clear
releasing it, and D720's protect and release. All four are needed; going straight from read-only to
no-access at the flip would save one.
