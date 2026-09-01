# G13 measured rather than assumed, and the work that does have an oracle


**The subgroup level is blocked on hardware.** It materialises the execution mask with a
subgroup ballot, which is correct only when the hardware's subgroup is as wide as the
guest's wavefront - sixty-four. The GPU here is thirty-two and the software renderer in
the build VM is eight, so the level can neither run nor be compared against the wavefront
model. The differential oracle needs both halves and one of them would be refused.

The roadmap said this was the last item with a real oracle and no dependency. That was
wrong, and wrong in the direction that matters: it would have led to building the one
thing in this subsystem that could not be checked - `spirv-val`-valid SPIR-V that had
never executed, in a crate where everything else runs on a real device. `cargo run
--example subgroup-size -p orbistoun-gpu-vulkan` settles it on any machine in a second,
and it was run because the claim was load-bearing rather than because anything looked
wrong. D133 records what would unblock it.

**What was built instead**, all of it with a device oracle: wide flat memory
(`global_load_dwordx2/x4`, `global_store_dwordx2/x4`) and `s_wqm_b64`, whole quad mode.
The census went from 102 to 104 of 126 and from 3 to 4 of 10 fixtures complete.
Sixty-five execution tests.

**Surprises.**

- **A test helper conflated a load's operand layout with a store's**, and the test that
  caught it was the one asserting a *refusal*. A load's destination is at bit 24 and a
  store's data at bit 8; one helper for both put the destination where nothing decoded
  it, so the register meant to overflow the file was never read and the translation
  succeeded. The two positive tests passed anyway - one by luck, because its destination
  and address were both register zero.

  This is D096 restated from the test side: **loads and stores do not share an operand
  layout, and anything that assumes they do will be right often enough to look correct.**

- **Whole quad mode needed no per-lane loop.** Folding each group of four bits down to
  its lowest with two shifts and two ors, masking to the group starts and multiplying by
  0b1111 spreads the answer back exactly - the multiply cannot carry between groups
  because only one bit per group survives the mask. Two tests pin the boundary: a bit in
  the second group must light that group and no other.

**Not done.** The division helpers and everything needing the resource model, unchanged.
`Fidelity::Subgroup` now has a measured reason rather than an assumed one. The design
question D133 raises - a variant carrying two lanes per invocation, which would run on
the thirty-two-wide hardware that actually exists - is deliberately left for a decision
rather than assumed into being.

