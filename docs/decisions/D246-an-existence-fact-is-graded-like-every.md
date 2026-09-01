# D246 - An existence fact is graded like every other fact, and only the target may name


**decided** · 2026-08-25 · the answer to "how far does a fact travel", asked before the campaign

`Transcript::symbols()` returned existence facts **ungraded**, on stated reasoning:

> Existence is a property of the platform's interface rather than of the silicon, and a name
> that resolves on a stand-in is still spelled correctly.

The first clause is true. The second does not follow, and D242 is what makes the gap matter.

**Where does a stand-in's symbol table come from?** Mined name lists - the same lists D242
refuses to import. So asking shadPS4 or fpPS4 whether `sceKernelSomething` resolves is asking
a mined list, and a `present` coming back is that list speaking. Ungraded, it would have been
recorded as a **probe measurement**: the strongest provenance this project has, awarded to
the one source the naming rule exists to exclude.

That is not a hypothetical channel. `resolve` is a by-name query, the probe runs on four
loaders of which three are emulators, and the reader had just been taught to consume it
(D245). The laundering path was complete and would have been used first on the targets that
are easiest to reach.

### The answer to how far a fact travels

The grading input was already in the transcript and needed no new field. `Origin::is_target`
asks whether the silicon was **the thing being emulated** - deliberately not "was it real
hardware", because a Steam Deck is real and is not it. The behaviour grading already demotes
`Hardware` to `Assumed` under it. Existence now takes the same path:

| where it ran | existence fact | may source a name |
|---|---|---|
| the console | `Measured` | yes |
| a Deck, a stand-in, an emulator | `Assumed` | **no** |
| unasserted | `Assumed` | **no** |

So the answer to the open question from D245 - *should `resolve` carry availability?* - is
that it is **not load-bearing and this project should not block on it**. Availability says
how a symbol is reached; admissibility turns on what the run was, which the transcript
already knows per session. Carrying it would be a useful detail. Waiting for it would be
waiting for something that was never the deciding input.

### What a stand-in is still worth

Not nothing, and the demotion is not a dismissal. A stand-in run exercises the reader,
proves the verb works, and compares implementations - which is the whole point of running
obSCEne under orbistoun. It simply cannot mint a name.

`SymbolFact::may_source_a_name()` answers it in one place rather than at each call site,
because the alternative is two implementations of one rule, and this project has now spent a
day on what that costs (D239).

