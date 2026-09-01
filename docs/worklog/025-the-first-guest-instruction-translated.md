# 2026-08-19 - The first guest instruction, translated and executed


**Done.** A TDD cycle, red through green, on `v_mov_b32` with an inline constant
source.

- `orbistoun-translate::predicated`: a register file as a private array, per-instruction
  emission, and an epilogue that copies the low registers into the storage buffer so a
  test can assert on them.
- Three tests that say what the instruction should *do* and are settled by a GPU.

**Verified.** Gate green, and the gate says `executed against a real device`. The
translated module validates under `spirv-val` and produces the right register values on
hardware: `v_mov_b32 v3, 9` leaves 9 in v3 and zero everywhere else, and a later move
beats an earlier one.

**Surprises.**

- **The driver answered a malformed module with an access violation.** Not a validation
  error, not a rejected shader - a hard crash of the test process. `create_shader_module`
  had accepted it. The fault was mine and small: the buffer is a struct containing one
  array, so an access chain into it takes *two* indices - the member, always zero, then
  the element - and I passed the register number for both. In bounds for register zero,
  out of bounds for every other, and the first test to exercise a non-zero register
  brought the process down.

  `spirv-val` named it in one line. Without the validator wired up first this would have
  been a debugger session against a crashing GPU driver, which is exactly the afternoon
  D099 was written to avoid - and the vindication arrived within the hour.

- **The register file needed an explicit null initialiser.** A private variable is
  undefined at entry, so "an untouched register reads zero" would have been an assertion
  about whatever the driver happened to leave in memory. It would have passed on this
  machine and meant nothing.

**Not done.** One instruction of a few hundred. No arithmetic, no memory access, no
control flow - and control flow is where the predicated strategy earns its name, since
none of the execution-mask machinery exists yet. The register file is a plain array with
no mask applied to writes.

