# D934 - An image descriptor loaded through a buffer is read where the buffer holds it

**Status:** decided
**Date:** 2026-10-20

A pixel shader's image descriptor can come from three places, and the translator traces each one
along the paths that reach the sample.

- **Read out of a buffer.** The program loads a buffer's descriptor from its table
  (`s_load_dwordx4`), then reads the image descriptor out of that buffer
  (`s_buffer_load_dwordx8`). The texture source then names both: the buffer descriptor's offset
  in the table, and the image descriptor's offset in the buffer. The pipeline reads the buffer
  descriptor where the table holds it, and the image descriptor at the buffer's base plus that
  offset.
- **A load some paths skip.** Where paths join, a load that some arriving path carries still
  reaches the access, unless another path carries a different load of the same registers. A path
  that skipped the load left no descriptor there, and a title samples only where it loaded one.
- **Seeded registers no path wrote.** Registers that no path to an access has written still hold
  the user data the stage was seeded with, even where another path writes them. The access reads
  that user data's texture. A loop's head counts as having written every register, and so does
  any instruction the analysis cannot name.

A texture's sampler is the table load that reached its registers at the texture's first access,
where one did. The program may load another sampler into the same registers elsewhere.

**Why:** Grand Theft Auto V's loading-screen lighting reads its point lights' shadow cubes in three
ways: out of a constant buffer whose descriptor is at table + 0xf0, 0x150 into it; from user data 8,
on a path that branches past the block that reloads those registers; and with a sampler loaded on
one path of two. Each was refused as a descriptor loaded from more than one place, or bound to a
texture it is not.

**Rejected:**
- Requiring every arriving path to agree, as before. The path a branch takes past a load carries
  no descriptor at all, not a different one, and it refused GTA V's shader.
