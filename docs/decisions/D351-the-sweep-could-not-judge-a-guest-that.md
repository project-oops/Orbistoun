# D351 - The sweep could not judge a guest that never faults


**decided** · 2026-08-27 · asked how to solve the one title shape the loop declined

PPSA04263 spins: `sceKernelDirectMemoryQuery` is 98.7% of twenty million calls and the guest
runs to the time limit rather than faulting. The dispatcher declined it:

> one call dominating the run is a loop the guest cannot leave; what it is waiting for is not
> varied by any diagnostic

**`ORBISTOUN_RETURN` varies exactly that**, and the finding's own evidence says so - *"a guest
that keeps asking the same question has not accepted the answer"*. The report and the
dispatcher disagreed, and the report was right.

### Why mapping it to the sweep would have been worse than the decline

The sweep's oracle is one comparison: `outcome.fault != baseline.fault`. A spinning guest
never faults, so both sides are `None`, and twenty-four boots answer `Unmoved` however well
the experiment worked.

Worse, the one way it *can* fire is a plant that causes a fault where there was none - so on
this title the oracle only reports success when the experiment **breaks** the guest. Inverted,
not merely blind. The decline was accidentally protective: wrong reason, right outcome.

### The oracle that survives is reach, and it was already measured

`Outcome::reached` carries distinct imports and has since the sweep was written. A guest that
escapes a loop starts calling imports it was never getting to, so `Finding::Escaped` reads
reach where there was no fault to compare - **no new data, no new runs, one comparison the
sweep was not making**.

Ordered before `Unmoved`, because on a run with no fault `Unmoved` is not a measurement:
reporting "tested and cleared" from a branch that could not see anything is the failure D229
and D230 both record.

### The bug it uncovered on the way

The first version returned `Derailed` instead. `Agreement::Derailed` fires on `!touched`,
which means *the fault was not at an address the guest asked for* - and a run that **did not
fault** carries `false` for the same reason an empty list has no first element. So any
non-faulting run was being reported as derailed into non-code.

Latent for as long as every swept baseline faulted. It now requires a fault before asking
where the fault was.

### What it does on the real title

`swept every argument: Unmoved { tested: [2], not_addresses: [0, 1, 3, 4, 5] }` - five of six
arguments hold sizes rather than addresses, and planting the sixth crossed with forced returns
did not break the loop. A measured negative where there was previously no measurement at all,
and one step of that turn moved from a person to the loop.

**Known wording gap**: `Unmoved` names a fault that did not move, and on a spinning baseline
there was no fault to move. The data is right and the word is fault-flavoured. Left as it is
rather than adding a variant nothing else reads, and recorded here so the next reader is not
misled by the name.

