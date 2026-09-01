# D449 - PPSA02664 regressed to the allocator wall; the policy-region/reserve collision is the mechanism (open)


**measured** - 2026-09-01 (user-directed /loop)

D443 recorded PPSA02664 reaching 1541 calls (fault `image+0xb14be3`, `_Getpctype`); it now walls at 234
(`image+0xafcc08`, the tlsf allocator) - the exact pre-D443 state. The saved run from 13:41 confirms the
1541 was real, so this is a genuine regression between then and 15:15, not a misreading. **The status
summary that said PPSA02664 reaches ~1500 is corrected: it currently walls at 234.**

**The mechanism, now understood.** `sceKernelReserveVirtualRange` has a region policy (a learned
measurement, `0x200000` bytes "handed back through argument 0", `learned.rs::policy`). At load,
`install_policy_writes` reserves that region at `POLICY_REGION_BASE` (`reserve_somewhere`, which
platform-reserves and leaks the range) and plants its base into the call's argument 0. But argument 0 of
`sceKernelReserveVirtualRange` is the guest's own **out-pointer**, so the plant overwrites the guest's hint
with the region base. The handler then tries to reserve that base - which orbistoun already holds as the
policy region - so the reservation conflicts, falls back to the far-higher mapping arena
(`0x72…`), writes that back, and the guest's allocator, addressing its arena from the base it was handed,
underflows: `tlsf_add_pool` rejects the pool (`size must be between 0x28 and 0x100000000`), the next
allocation returns null, and the guest writes through it. `POLICYDBG`/`RVRDBG` traces confirmed each step
(hint arriving as `0x6b0000000000`, reserve falling back to `0x720000140000`).

Why D443 did not hit this and now does is **not** established. The region policy derives from a learned
measurement whose activation may have shifted across the flexible/virtual-query/sysctl turns, or a
concurrent session was restructuring shared project data at the same time (it was actively splitting
obSCEne's decision log). That is left as the open question rather than guessed at.

**Two speculative fixes were tried this turn and reverted**, because neither was clean and piling changes
onto the reservation path without understanding is the shortcut principle 11 refuses:

- *Implemented supersedes region* - skip the region policy for any function orbistoun implements. Correct
  as a principle, but it removes the plant, and the plant is **load-bearing**: it is what hands the guest a
  low, backed arena. Without it the guest gets the high arena and underflows anyway.
- *Honour a reserved hint* - when the guest reserves a range orbistoun already holds for it, hand it back
  rather than conflict. Nudged the guest one import further, but the guest issues **two** reserves both
  hinted at the region base, so honouring returns the same range twice - overlapping allocations.

**The fix direction, for a careful turn (not more loop speculation):** a region delivered to a function the
guest itself reserves (`sceKernelReserveVirtualRange`) must be one the guest *can* reserve - either not
pre-reserved by orbistoun, or honoured with a distinct sub-range per call. The tree is left net-zero for
this turn (both attempts reverted, no stray diagnostics); obSCEne still runs its full suite unaffected.

**Update - the pool size is not the reserve base or the query extent (both tried and eliminated).**

Chased the tlsf underflow directly. Handing the guest a low, backed arena (carving a sub-range of the
policy region) and answering `sceKernelVirtualQuery` on it - first with the whole region extent, then with
the exact reserved length - were all built and tested. Every one still ends in
`tlsf_add_pool: size must be between 0x28 and 0x100000000` at 234 calls. So the size the guest hands
`tlsf_add_pool` is **not** computed from the reservation base or from what `sceKernelVirtualQuery` reports -
those inputs were varied across their whole plausible range and the fault is invariant. It comes from some
other value the guest holds, and finding it needs the size the guest actually passes to `tlsf_add_pool`
(a trace or disassembly of that call site), not more trial against the reserve/query path. All of the above
was reverted; the tree is back to the 234 baseline. This is a focused debugging task, not loop-tickable, and
the reserve/query machinery is a dead end for it.
