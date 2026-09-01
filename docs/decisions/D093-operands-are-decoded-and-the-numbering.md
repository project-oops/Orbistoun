# D093 - Operands are decoded, and the numbering lives in data

**decided** · 2026-08-19

`orbistoun-shader::operand` reads what an instruction operates on.
`data/operands.toml` holds the unified operand numbering; `data/encodings.toml` gains
per-family operand layouts.

This is the step between counting instructions and translating them. Knowing a word is
`VOP1:0x1` ranks it in a worklist; translating it needs to know it moves the constant
zero into vector register 0. Register mapping, SPIR-V emission and control-flow
reconstruction are all blocked on this and on nothing else.

**A wrong boundary here is worse than a wrong instruction length**, and that asymmetry
drives the whole design. A wrong length desynchronises the decoder and everything after
it becomes obvious nonsense - loud, and trivially spotted. A wrong operand boundary
produces a *plausible register where a constant belongs*: code 128 is the integer zero,
and read as a register index it is scalar register 128, which exists. A translator built
on that emits a shader that compiles, runs, and draws the wrong thing, with nothing
anywhere to investigate.

So every operand is checked against the reference disassembler - **99 of them across
the fixture corpus** - and the table refuses overlapping ranges, since two ranges
claiming one code makes file order load-bearing without saying so.

**A missing operand layout is distinguished from an instruction with no operands.**
`Instruction::operands_decoded` exists so an unfilled family cannot pass a check by
producing an empty list.

