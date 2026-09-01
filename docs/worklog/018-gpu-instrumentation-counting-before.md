# 2026-08-19 - GPU instrumentation: counting before translating


**Done.** Two walkers, neither of which translates anything.

- New crate `orbistoun-shader`: instruction decode, corpus capture, coverage and the
  ranked blocker list. Encoding families load from `data/encodings.toml`.
- New module `orbistoun-gpu::packet`: walks a submitted command buffer into packets,
  with a command histogram.
- `ACKNOWLEDGEMENTS.md` gains the AMD hardware documentation, with the reasoning for
  why chip-vendor documentation sits inside rule 1 where console firmware does not.
- Two backlog items: verifying the encoding table, and generating ground truth with
  LLVM.

**Verified.** 42 tests across the two crates, clippy clean at the workspace lint
level, no warnings.

**Surprises.**

- **A census of unknowns cannot distinguish "nothing is supported" from "the tool is
  broken"** - the same trap obSCEne hit with its symbol census earlier today, arriving
  by a completely different route. Here it is handled by `is_trustworthy`: a decode
  that desynchronised or overran is a lower bound, not a measurement, and the flag
  says which. Worth noting the pattern is now twice-observed: any tool whose output is
  mostly absences needs a way to prove it can detect a presence.

- **The obvious ranking is the wrong one.** Sorting blockers by occurrence count puts
  an instruction used ten thousand times in a single shader above one used once in
  four hundred shaders. The second unblocks four hundred times as much. Ranking by
  distinct shaders blocked is the whole value of the report and it is one line
  different from the version that would have looked fine. (D086)

- **An unknown instruction has unknown length, and there is no way out of that.**
  Every other decoder in this project reads a length from a header; here the length
  comes from the encoding, so failing to recognise an instruction means not knowing
  where the next one starts. Stopping loses the rest of the shader, guessing produces
  confidently decoded nonsense. Advancing minimally and marking the walk suspect is
  the least-bad option rather than a good one, and it is worth being clear about that
  in the code so nobody later mistakes it for a solved problem.

- **A blanket `pub const` to `pub(super) const` replacement caught three `impl`
  methods it should not have**, turning public API into dead code - which clippy then
  correctly reported as never used. The mechanical edit was scoped to a pattern rather
  than to the private module it was meant for. Same lesson as the census-list
  regeneration earlier: when a mechanical transform touches a source of truth, scope it
  to what it means rather than to what it matches.

**Not done.** Nothing is wired into the HLE registry yet - the submit functions are
still declarations, so no real command buffer has been walked and no real shader has
been captured. The encoding table is unverified (D085). No CLI surface: adding one
means editing `orbistoun-cli`, which another session is live in, so it is deliberately
left for whoever holds that file next.

