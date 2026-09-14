# D682 - shader capture into the census corpus requires a terminator, not a translatable decode

**Status:** decided
**Date:** 2026-09-13

## The choice

`capture_shaders` (in `orbistoun-gpu`'s `pipeline`) walks a command stream, recovers the
shader addresses in its register writes, reads each shader out of guest memory, and stores it
in the content-addressed corpus the census ranks. A shader is captured when its extent is
known - it reached an end-of-program instruction - **and captured even if it does not decode
cleanly or cannot be translated**. The only refusals are an address memory cannot honour, and
a window with no terminator in it.

This is deliberately a weaker bar than [`Pipeline::submit`], which refuses a shader that is
not trustworthy or will not translate, because the two are answering different questions.

## Why

`submit` is building a frame: a shader it cannot translate becomes a draw it must omit, and
letting an untrustworthy decode through would emit a wrong module. Refusing is correct there.

The census is doing the opposite job. Its entire purpose is to **rank what blocks
translation** - so a shader carrying an instruction the translator does not handle yet is not
noise to reject, it is the single most valuable thing the corpus can hold. Applying `submit`'s
bar to capture would throw away exactly the shaders the census exists to find, and the corpus
would fill up only with shaders already fully supported - a coverage report that can never
show a gap because the gaps were filtered out before it ran. That is the "counts successes
instead of checking for failures" trap one level up.

A terminator is still required, and that is not the same bar as trustworthiness. Without an
end-of-program instruction the shader has no extent (D114): the address was wrong or the
window too small, and bytes of unknown length are a guess about where a shader stops, not a
shader. A desynchronised decode that nonetheless reaches the terminator has a real extent and
real blockers, and both are wanted.

## Consequence

Capture and translation share the fetch (`read_window` + `decode_program`) but not the
acceptance test, so they are separate paths rather than one with a flag. An address that
yields no shader is reported as a miss with its reason, never dropped and never turned into a
capture of whatever bytes happened to be there - the register-address mapping is a hypothesis
(D091), and a miss is evidence about it.

Nothing calls `capture_shaders` from a run yet, because no title reaches shader submission -
they wall earlier, in the AGC command-buffer builders, which are gated on obSCEne captures.
It is exercised by a synthetic command-stream fixture (a hand-built register-write stream over
a fake address space) so the whole chain - walk, extract, read, capture, census - is checked
without a title or a GPU, and is ready the moment a real command stream exists.
