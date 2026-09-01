# D084 - Instrument the GPU before translating any of it

**assumed** · 2026-08-19

Two new pieces, neither of which translates anything: `orbistoun-gpu::packet` walks a
submitted command buffer into packets, and `orbistoun-shader` walks shader bytecode
into instructions. Both stop at "what is in here", and refuse to guess at meaning.

The reasoning is the one the import survey already proved. "Emulate the operating
system" was unbounded until it became a frequency-ranked list of functions; then it
became a queue. The GPU is in exactly that pre-survey state, and it is the larger of
the two problems.

**Translation is not a search problem** - no amount of iterating on failures writes a
compiler, because a half-written translator emits nothing rather than something
slightly wrong. But *deciding what to build next* is a counting problem, and counting
can start immediately, needs no GPU, no driver and no running emulator, and produces
the one number that ranks the work: which single instruction, supported, unblocks the
most shaders.

So the census exists first, and the translator is written against its output.

