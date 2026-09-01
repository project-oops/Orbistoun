# Hardware-derived expectations are `assumed` until they come from the hardware


A Steam Deck is RDNA2 (`gfx1033`) and is being used as a stand-in while the hardware is not
here. Values measured on it are *measured* for that part and **`assumed`** for the target,
because the hardware's GPU is a custom RDNA2 derivative and this project has no way to know
where the two diverge.

The distinction is not pedantry. `orbistoun-gen`'s target module already says `gfx1030` is *a* member of
the generation rather than the exact part, and that is fine for **encodings** - an encoding
scheme is generation-wide. It is not fine for **values**: denormal handling, rounding and
the precision of the approximate instructions are exactly the things a derivative part can
differ in.

So anything arriving from a stand-in carries the part that produced it, and is labelled
`assumed` until the same measurement comes off the hardware. Without the part recorded, a
value measured on one and consumed as authoritative for the other is invisible - which is
D139 exactly: months pointing at the wrong generation with nothing saying so.

**A worked example already in the tree.** `v_rcp_f32_e32` is translated as an exact
`1.0 / x`, and the real instruction is a bounded approximation. The test that covers it
asserts `1.0 / 2.0 = 0.5`, which is exact in binary, so the two agree and the suite is
green. For `1 / 3` they need not agree at all. Nothing here can currently tell, and the
GPU these tests run on is an NVIDIA part, so even the execution oracle is answering a
different question from the one being asked.

