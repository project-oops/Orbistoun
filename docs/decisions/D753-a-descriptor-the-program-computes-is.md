# D753 - A descriptor the program computes is evaluated per draw

**Status:** decided
**Date:** 2026-10-07

A buffer access whose descriptor, or whose scalar offset, the program computes from more than user
data, constants and loads at fixed places is still traced (D733). Its source is the program up to
the access. For each draw, that prefix runs on the host over the draw's user data and guest memory,
and the descriptor and offset the scalar registers then hold are what the draw binds: the range from
the descriptor's base through the offset plus the descriptor's extent, and its fourth word for a
format load (D738). A scalar load from a base the program computes, or adding a scalar offset,
is found the same way: its range starts at the base plus the offset and runs as far as the load
reads.

The prefix runs through the translator's own instruction semantics, driven by a model whose values
are numbers rather than SPIR-V. Nothing about an instruction is written twice. A value the prefix
cannot know on the host is unknown, and an access that depends on one is refused by name, as an
untraced one is. Such values are a vector result written to a scalar, the execution mask at entry
(which differs from wave to wave), and anything after a branch. A scalar load reads guest memory as
the draw's snapshot of it holds.

**Why:** PPSA28061's vertex shader is a fetch shader. It loads its vertex descriptor from a table at
an offset taken from a word it reads from memory. It then rebuilds the descriptor's format and
channel selects from that word's bitfields and a constant table (`s_bfe_u64` of `s[6:7]`), and
offsets the load by another field. Every input is uniform for the draw, but none sits at a fixed
place, so no static trace can name the descriptor. Its value exists only per draw.

**Rejected:**
- Extending the trace's words into expressions. The shader uses bitfield extracts with computed
  widths, 64-bit extracts of constant tables and condition-code selects. An expression language
  covering them restates the instruction semantics the translator already has, and drifts from
  them.
- A second, host-side implementation of the scalar instructions, for the same reason.
- Refusing the access. That holds the title's every draw.
