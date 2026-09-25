# 853. Decodes kept by their bytes, and a sweep that stops allocating

**2026-09-25**. Preparing a Neverball submission drops from ~185 to ~151 ms of every second. The
gl1-probe verdicts and sampled pixels are identical, and so are the presented frames.

**What each change keeps, and what it is keyed by.** Each is exact because its answer is a function
of what it is keyed by:

- **The shader decode, by its bytes.** `Pipeline::prepare` read a window and decoded every
  candidate, on every submission, before it consulted the translation cache. Now it keeps the
  bytes each address decoded to, end-of-program included. When those bytes still stand in memory,
  it skips straight to the cache (`prepare_decoded`). A translation miss on that path decodes the
  same bytes again. A decode that ends differently the second time is refused as this crate's
  fault. Prepare's shaders span went from 70 to 43 ms/s.
- **D711's window base, by the program's bytes.** `place_window` decoded each vertex program
  every submission to find the constant base it forms. The base is now kept per address with the
  bytes it came from. Only a program that terminated is kept: one that ran off its window would
  decode differently were more of it mapped. The environment span went from 14 to 2 ms/s.
- **The register sweep's table, reused.** `RegisterSweep::new` allocated and zeroed a 1 MB table
  (65,536 `Option<usize>`) twice a submission, for streams that write a few hundred registers. It
  now takes a thread's spare table of `u32` indices and resets only the entries it wrote.
  - Writes past a `u32` index go to `beyond`, retiring the table's older entry.
  - The shaders span went from 43 to 29 ms/s, and geometry from 50 to 38.
- **`set_guest_memory`, by the words.** A window word-for-word the one held is not copied, hashed
  or counted as a state change. Neverball's window changes every submission (the guest writes its
  vertices into it), so this rarely fires there.

**Each has a test that was watched failing** with its check disabled:

- `a_shader_rewritten_in_place_is_decoded_again`;
- `a_vertex_program_rewritten_in_place_places_its_own_window`;
- `a_reused_sweep_table_remembers_nothing_of_the_last_stream`.

**Measuring:** the new spans `prepare: environment`, `prepare: window` and `draw whole` sit behind
`ORBISTOUN_PERF_DETAIL`. Only ~1,100 of Neverball's ~210k draws a second miss the open batch. Each
costs ~44 us, so they total ~47 ms/s.

**Neverputt** (`NVPT00001`, from the same source tree as Neverball) is in `corpus/sources.toml`,
pinned, with the release asset as its fallback. It runs at 13-14 flips a second and its menu renders
correctly.

A surprise: `frame-1.bin` in the traces directory is **not** a presented frame. It is the last
submission's target, written when the call budget ends a run, so it can be mid-frame. Compare the
`frame-100000N.bin` ring instead.

**Where the rest goes, per second of Neverball:**

| where | ms/s |
|---|---|
| the main thread waiting on the device thread | ~200 |
| prepare | ~150 |
| flip write-back, for 12 flips (the device read-back plus tiling into guest memory) | ~80 |
| reading the target | ~65 |
| DMA copies | ~50 |
| keeping snapshots | ~35 |

Guest code is ~15-22% of the main thread.
