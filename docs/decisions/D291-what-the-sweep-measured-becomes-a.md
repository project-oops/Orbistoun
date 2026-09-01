# D291 - What the sweep measured becomes a knowledge entry, and what it did not becomes an assumption


**decided** · 2026-08-26 · the loop measured a contract and then dropped it on the floor

`turn::turn` now measures an out-parameter contract and satisfies it, and then the run ends
and **nothing is written down**. CLAUDE.md names that failure directly - *"anything that
exists only in a conversation is already lost"* - and the loop's own step 5 is
`orbistoun-cli learn` for exactly this reason.

`learn`'s vocabulary already fits the measurement, which is the argument for using it rather
than inventing a second record:

| what the sweep produces | how `learn` spells it |
|---|---|
| the guest proceeded when answered this way and stopped otherwise | `--known guest-observed`, whose own definition is that sentence |
| slot, offset, and the answer the read depends on | `--edge`, "behaviour a reimplementation would otherwise get wrong" |
| everything the sweep did **not** establish | `--assumes`, "a claim `--known` does not cover" |
| which title it was seen in | `--seen-in` |

**The `--assumes` column is the point.** For `sceKernelReserveVirtualRange` the sweep proves
three things and no more: `arg0` is written back, the guest indexes `+0xfffe0` from it, and
the call must answer zero first. It proves nothing about `arg3 = 0x40000` being an alignment,
about what `arg2` selects, or about what the function is *for*. An entry that recorded those
as known would be the convergence problem arriving through the loop instead of through a
person - a fact **recalled** and dressed as one measured.

So promotion is a **pure function from a finding to a proposed entry**, in
`orbistoun-propose`, and it lives there rather than in the shim that writes files because the
hard part is the judgement about which column each claim goes in. Writing TOML is not the
hard part and already exists.

It is also the accounting an autopatcher needs before it can exist at all. A patch may use
what the entry records as measured; anything it does beyond that has to appear in
`--assumes` or it came from somewhere nobody can point at. A diff against the entry is a
provenance audit, which is the mechanism CLAUDE.md asks for in place of abstinence.

