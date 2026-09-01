# 2026-08-30 - When, not which


The syscall record was a bitmap, so a run could say *`ftpsrv` asked for 20, 601 and 616* and
nothing more. The wrong half: whether those come before or after `Unable to change AuthID` is
the difference between the privilege path using them and the give-up path using them (D388).

In order now, with the first argument. All three are on the give-up path - the gadget that made
the first was called from the instruction after the `puts` that printed the failure.

And `ftpsrv` does not dereference the kernel addresses at all. Markers or nulls, it prints the
same failure and faults on nothing: it checks the primitive it would read *with*, and correctly
concludes it has none. **There is no syscall, import or global that moves it.** D382 said this
was a wall worth having; that was inference and this is measurement.

`elfldr` gave up three facts under the marker depths. It resolves its C library through
`sceKernelDlsym(1, name, out)` - the three-argument form, which is what we already implement -
and gets exactly two names out before failing, so resolution is not the wall. Field two of the
handoff structure is a pointer that must be readable. And the thing that kills it, a bad lock
pointer of `0x2001`, is **identical under two marker schemes that fill that memory completely
differently** - so it does not come from the handoff structure at all.

That third fact rules out the obvious next move rather than supporting it. Walking field two's
referent member by member was the plan; it is not where the number comes from. Better to know
that now than after a session of it.

