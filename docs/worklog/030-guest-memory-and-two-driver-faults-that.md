# Guest memory, and two driver faults that a builder could have caught


Loads and stores now translate and run. `s_load_dword`, `global_load_dword` and
`global_store_dword` join the supported list, a translated module binds a second storage
buffer for guest memory, and `dispatch` returns both buffers so a test can assert on what
a store left behind. Both fidelity levels implement the memory path, and
`the_models_agree_about_memory` runs the same shader through each and compares - the
differential oracle doing the job it was built for. Sixteen execution tests, all against
a real device.

Getting there cost two driver faults, and both are worth recording because they share a
shape.

- **An access chain into a storage buffer needs two indices, not one.** The buffer is a
  struct containing an array, so reaching a word means the member - always zero - and
  then the element. Passing the element alone produces a module that validates
  structurally and faults the driver. `spirv-val` named it in one line.

- **An identifier reserved and never defined.** An `OpConstant` for an array's length was
  never emitted, so the array type referred to nothing. Every instruction well-formed;
  the module meaningless. `STATUS_ACCESS_VIOLATION` inside the graphics driver, no
  indication of which identifier or which instruction. Again `spirv-val` answered it in
  one line, and again from a virtual machine because the tool is not on the host.

The common shape: **a driver handed a well-formed module that means nothing does not
diagnose it, it faults** - and the fault carries no information about what was wrong.
Every minute spent on both was spent finding *where*, not fixing it. So the builder now
checks that its identifiers resolve before the module leaves the crate, and a failure is
a named `TranslateError` rather than a process death. The builder allocates every
identifier, so it is the only place that can say which were never given a meaning.

A third layout bug surfaced on the way: decorations must all precede types, and the
builder had one undifferentiated preamble, so that was a property of the order calls
were written in. It is now four sections concatenated in the order the format requires,
and the method chosen determines the section. That class of bug cannot recur.

**Surprises.**

- **A bulk rewrite of call sites lost every opcode it should have kept.** The routing
  pass used a Perl ternary that ran a regex to decide the destination section - and a
  *successful* match resets the capture variables, so the replacement interpolated an
  empty string for exactly those branches that matched. The failed branch was untouched
  and looked fine, which is why the damage was uneven and easy to misread as partial.
  It was recoverable only because the opcode is inferable from the arguments.

- **`clippy::match_same_arms` was diagnosing a design problem, not a style one.** Thirty
  match arms with identical bodies is a table written as code. Rewriting it as a table
  satisfied the lint as a side effect of being the right shape - and made the whole
  thing greppable, which the match never was.

- **The gate only re-ran one of the two device-dependent suites** with output shown, so
  a silent skip in the translator's execution tests would still have passed unnoticed.
  It now checks both. The original note about this failure mode was written when there
  was only one suite; adding the second did not extend it.

**Not done.** `Fidelity::Subgroup` is still a stub. The Lane model remains unmasked, so
control flow and the execution mask together - the largest unbuilt part of D098 - is
still ahead. `s_load_dwordx4`, `s_load_dwordx2`, `s_mov_b64` and `exp` remain on the
worklist, and three encoding families (SOPK, MTBUF, VINTRP) are still unverified against
a reference.

