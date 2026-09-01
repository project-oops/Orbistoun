# Promotion: what a turn measured, and what it did not


The loop measured an out-parameter contract, satisfied it, cleared a wall - and then the run
ended and **nothing was written down**. Step 5 of the loop is `orbistoun-cli learn` for
exactly that reason, and nothing was calling it.

`turn::promote` is a pure function from a finding to a proposed entry. Its vocabulary is
`learn`'s, because `learn` already had the right one: `guest-observed` is defined as *"the
guest proceeded when answered this way, and stopped otherwise"*, which is a description of a
sweep.

**The `assumptions` half is the point.** For `sceKernelReserveVirtualRange` the sweep proves
three things: `arg0` is written back, the guest indexes `+0xfffe0` from it, the call must
answer zero first. It proves nothing about `arg3` being an alignment, what `arg2` selects, or
what the function is *for* - and an entry silent on those reads as though it had measured
them. Three assumptions go in every promoted entry for that reason, and a test asserts each.

`promote` returns `None` for `Unmoved`, `NeverPlanted` and `Dereferenced`. "We looked and
found none" is a completed search, not knowledge.

### Why this is the gate for auto-implementation and not a side quest

A patch may use what the entry records as **measured**; anything beyond that has to appear
under `assumptions` or it came from somewhere nobody can point at. A diff against the entry is
a provenance audit - which is the mechanism CLAUDE.md asks for in place of abstinence, applied
to a model writing code rather than a person.

Without the entry there is nothing to diff against, so promotion had to come first.

### Two things found on the way

- `cmd_learn` holds the recording logic **in the shim**, which principle 13 says shims do not
  do. `promote` sidesteps it by producing fields rather than writing files, but the extraction
  is still owed.
- The probe reports `libc::sqrt`, `round`, `pow` and `trunc` as landing on stubs **while
  `037-math` passes all thirteen checks**. Both true: the probe reaches them through
  `sceKernelDlsym`, which nothing implements. A name looked up at run time does not resolve
  where the same name in the import table does - a whole resolution path never exercised,
  exposed by the first guest that asks for symbols by name (D290).

