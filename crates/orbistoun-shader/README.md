# orbistoun-shader

Guest shader bytecode: decode it, and measure how much of it is understood.

**Models:** the instruction encoding tables, the decoder, and the census that ranks what is
missing.

**Deliberately fakes:** nothing. An opcode it cannot solve is reported as unsolved, never
decoded to something plausible.

**Design note.** Translating shaders is the hardest remaining problem here, and it is not a
search problem - no amount of iterating on failures writes a compiler. But *deciding what
to build next* is a counting problem, and counting can start immediately. For a real corpus
this answers: how many distinct instructions are in use, which are not understood, and
**which single one, if supported, would unblock the most shaders**.

That turns an unbounded compiler project into a ranked worklist - the same move the import
survey made on "emulate the operating system", one layer down.

It needs no GPU, no driver, and no running emulator.

**The failure mode this crate taught the project.** A test that iterates over what the code
produced and validates each item **passes when the code produces nothing**.
`every_decoded_operand_appears_in_the_reference` was green while the most common
instruction in any shader decoded to a mnemonic and an *empty operand list*. The fix is an
exact inventory of what produces nothing, each with a reason - see
[TESTING.md](../../docs/TESTING.md).

**Status:** decoding and the census work, tested against a reference disassembler and
against bytes that are not instructions at all. Nothing has reached it from a running
guest.
