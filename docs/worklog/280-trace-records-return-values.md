# 2026-09-01 - (/loop) The call trace records what each call answered

The user asked to focus on the watchpoint feature and crack the refers, and - reading the last
fault report - whether we really had to walk backwards up the call chain, and whether there was
any baseline to check against. Reading the dispatch path answered both: every recording captured
what the guest passed *in* and nothing captured what our function handed *back*, so the wrong
return - the thing at both of PPSA02664's walls - was structurally invisible, and the "baseline"
is our own graded oracle (FreeBSD source, the guest as a 1-bit oracle, obSCEne on hardware),
already used in this codebase (the flexible-memory figure in `orbistoun-kernel`, D444).

So rather than walk backwards by hand, made the record complete. `on_guest_call` now stores the
answer in two allocation-free rings beside the argument (`RING_RET` + a `Release` flag
`RING_RETURNED`, because zero is `OK` and must not read as "not seen"), funnelling the four
return paths through one `resolve` helper so none can forget to record. `RecordedCall.ret` /
`TracedCall.returned` are `Option<u64>`; the report prints `-> 0xNN` on the tail and the "just
before" lines when known. Recorded D459. `cargo test` green (report 68, thunk 26, +3 new
round-trip tests pinning zero-survives, unknown-is-absent, and older-trace-loads); fmt clean;
clippy adds no new warnings (the two it reports - `enter` too_many_lines and a `sha256` map/
unwrap_or in the CLI - are pre-existing debt, untouched here).

The surprise is the payoff. On PPSA02664 the return column named both non-deterministic walls
(D450) at a glance:

- `image+0xb14be3`: `_Getpctype(0x34) -> 0x7fff0001` (`Unimplemented`) - an unimplemented ctype
  function whose placeholder the guest dereferenced as a table pointer. D125, self-named.
- `image+0xafcc08`: `sceKernelMapDirectMemory -> 0x7fff0004` (`NoMemory`) while Reserve, Query
  and Allocate all answered `0x0`. The guest reserves the range, then maps physical memory at
  the reserved address; our `map_named_direct_memory` reserves it a *second* time and conflicts.
  This overturned the previous turn's guess (a bad pool size / address-space mismatch) in one run.

Next, as two separate units, each verified by whether the wall moves rather than by looking
right: (1) make `map_named_direct_memory` commit into an existing reservation instead of
reserving afresh; (2) implement `_Getpctype` to answer a real ctype table - taking the layout
from an oracle first, and noting that the earlier revert of a `_Getpctype` attempt was driven by
the D450 non-determinism read as a regression, not by a real one.
