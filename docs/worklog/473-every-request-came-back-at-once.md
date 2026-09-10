# 473. Every request came back at once

**2026-09-09** - directed, continuing 472

Seven requests resolved between passes - five in obSCEne's bus, one in SELFish's inbox, one in
Prosperous's - and two arrived in Orbistoun's own. This pass consumed them.

## The signal contract, half of it

obSCEne measured both exception-handler calls as `libkernel` exports and returned six values.
`install` answers `0x0` first time and errno 35 for a duplicate; `install(sig, NULL)` is **not**
an uninstall; `remove` answers `0x0`; a raise naming a handle the process never issued answers
`ESRCH`. All four are implemented.

**The delivering raise is not**, and that is the point of D648. Every raise in the sweep passed
the literal handle `1`, so no handler ever ran and the ordering question the request was built
around is still open. Answering `0` would move PPSA25872's wall today and silently decide the
call is asynchronous.

```text
PPSA25872   stub calls 8 -> 6, both calls now answered
            still quiet from 0.2s of a 20s run
```

## A gloss corrected on the way in

The sweep glosses `0x8002_0023` as "35 = `EEXIST`". 35 is right; `EEXIST` is **17** in this
platform's own harvested headers and 35 is `EAGAIN`. `errno::AGAIN` is recorded as 35 with the
gloss explicitly rejected in its doc comment, and a request filed asking whether obSCEne's own
table carries the wrong mapping or whether the gloss was hand-written.

## A name that cannot be confirmed

SELFish named `libSceAgc::0x53bbd82b51d172db` as **`sceAgcInit`** from five published tables - and
showed that the name does not hash to it, so no search could ever have found it. 8 of 116
published `libSceAgc` identifiers are like this. Recorded as knowledge at `known_by: published`
rather than in the symbol database, because every entry there must reproduce from its name and
this one cannot (D649).

## Surprises

- **The verdict said BACK and the guest had lost nothing.** 140 distinct against 141. Both builds
  were run and their label sets compared: identical, 114 each. `distinct` counts symbol *indices*,
  and two indices can share a label - so implementing a function moved the count without moving
  the guest. Worth chasing rather than explaining away, and it took one experiment.
- **The knowledge gate caught a real mistake.** The two functions had been recorded under
  `libkernel_unity` - the library the *title imports from* - and implemented under `libkernel`,
  where the console exports them. `every_implemented_function_is_written_down` failed on exactly
  that, which is what it is for.
- **The third unnameable hash was obSCEne's own bug.** `0x384a0ae5cd37b0d4` is the hash of the
  literal string `$fYZQG4CU71c` - something hashed a name that was already a hash - and the file
  it is in has no vendor tables at all, so neither of orbistoun's two hypotheses held.

## The mesh

Resolved oops-libs' portable-sentinel question (shared, with a caller-supplied body, and write no
note when given none; orbistoun's copy goes once the shared signature exists). Claimed SELFish's
request for a two-reader differential over the module corpus - real work, started, not finished.
Filed two into obSCEne: the delivering raise with a real thread handle, and the errno glosses.

## Next

- The differential, which is claimed and owed.
- The delivering raise, when 2a90 answers.
- `_Stdout` and `_Stderr` now have 64 measured bytes each; the data blocks are still zeroed.
