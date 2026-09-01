# The dispatch speaks names


The other half of D139. `model.rs` dispatched on `(family, opcode)` everywhere below the
top-level match; it dispatches on the instruction's name now, resolved once, and refuses
an opcode this target has no name for rather than acting on a bare number.

Sixty-seven names in `SUPPORTED`, six internal matches converted, and the retarget guard
that came with the list now has nothing behind it still keyed on numbers. 202 tests green
across the four shader-side crates.

### Surprises

- **Two width tables were opcode arithmetic.** Scalar loads did `1 << opcode`; flat
  accesses had a hand-written lookup with a comment explaining the opcodes are *not*
  consecutive. Both were reading a coincidence of one generation's numbering. The width
  is written in the name - `dwordx4` is four - so both tables collapsed into one function
  that parses the suffix, and neither can drift on a retarget.
- **A family was passed where a name was wanted, and it compiled.** `resolve()` decides
  auto-fidelity by asking whether any instruction touches a lane mask; it handed
  `touches_mask` the family string after that function started expecting the name. Both
  are `&str`. Every shader silently resolved to the lane model, and the two tests that
  caught it did so through *consequences* - a local-share test and a branch test - not
  through the fidelity decision itself. This is the standing cost of keying on strings:
  the compiler stops checking. Accepted for the reason in D139, but it is why the
  behavioural tests earn their keep.
- **`scalar_move` had an inner `name` binding shadowing the new parameter.** It reads the
  destination operand's name to spot `s_mov_b64 exec, ...`. The shadow made the parameter
  look unused, which is the only reason it was noticed at all.

### Outstanding

`MCPU` is still `gfx900`. Flipping it and regenerating the tables is next, and is now a
one-constant change with a test that names what breaks instead of a silent rebind.

