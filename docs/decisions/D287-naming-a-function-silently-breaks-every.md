# D287 - Naming a function silently breaks every experiment aimed at its hash


**decided** · 2026-08-26 · the wall sweep reported `NeverPlanted` against the wall it was written for

`ORBISTOUN_WRITE` and its siblings match a target against an import's **label**: the bare
symbol, or any substring of `library::symbol`. That is what lets an experiment be aimed at a
function nothing has named - `ORBISTOUN_DUMP=0x6abac2f3dc6f8cee` - which the workflow
documentation calls out as the point rather than a convenience, because the functions most
worth experimenting on are the ones with no name.

**The moment a name lands, the hash stops being in the label.** `libkernel::0x6abac2f3dc6f8cee`
becomes `libkernel::sceKernelReserveVirtualRange`, and every experiment addressed by the hash
matches nothing. `tests/wall.rs` targets that hash in a constant and had been the instrument
for that wall; after the name arrived it swept twenty-four times, planted nothing, and
reported `NeverPlanted`.

**Nothing was silently wrong, and that is the part worth keeping.** `NeverPlanted` exists
precisely so a sweep that never planted cannot be read as "not this function", and its
message says *"check the target matches an import the guest actually calls."* The distinction
was written for a different cause and caught this one unmodified - which is what a
well-chosen failure mode does.

So the trap is not in the matching, it is in **holding a hash in a constant**. The naming
loop is designed to make hashes disappear; anything pinned to one has an expiry date it does
not know about. The same shape as D213, where a vocabulary that grows renumbers the
derivation indices that were correct yesterday: in both cases the loop working invalidates
its own earlier records.

Fixed by targeting the name, since there is one. A sweep aimed at something still unnamed
keeps using its hash, and will need the same edit when the name arrives - which is
acceptable, because the alternative is resolving names inside the experiment layer and that
layer deliberately knows nothing about the symbol database.

