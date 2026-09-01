# D087 - Decoding never fails; it reports

**assumed** · 2026-08-19

Neither `shader::decode` nor `gpu::packet::walk` returns `Result`. A binary that
cannot be walked is a *finding*, carried on the returned value.

A sweep over a corpus has to say how many binaries were strange. One that stops at
the first strange binary answers a question nobody asked.

Three findings are carried, and the distinction matters:

- **desynchronised** - an unrecognised item of unknown length was passed, so
  everything after it is suspect. Coverage from such a walk is a lower bound, not a
  measurement.
- **overran** - something claimed to extend past the buffer. Following it would read
  unrelated memory as instructions, and it is the best single indicator that a length
  rule is wrong.
- **trailing bytes** - not a whole number of dwords, so the buffer is not what it was
  thought to be.

`is_trustworthy` exists so a caller can tell a measurement from a lower bound. A tool
that cannot say when to distrust it is worse than no tool, because its numbers get
quoted anyway.

