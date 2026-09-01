# 2026-08-27 - The guest names its own requirement


`getpid` and `sysctl` (D350). `sysctl` is the interesting one: it is implemented as a
**documented refusal**, and the work is in what it reports rather than what it answers.

`ORBISTOUN_DUMP=sysctl` gave the MIB - `[1, 14, 8, 0]`, `oldp` null, so a size query - and
the symbol table named the caller as **`find_pid`**, 209 bytes in, from `main+443`. So the
guest has said both what it asked for and what it wanted it for, which is more than any
lookup would have given.

What the MIB *means* is deliberately not recorded. `sys/sys/sysctl.h` is not in the local
FreeBSD checkout - only `lib/` is - and counting entries in the manual page's list to reach
`8` would be inference dressed as a citation. The number is checkable; the name is open.

`sysctl(3)` **is** in the checkout, and documents `ENOENT` for an unknown name. So refusing
is the implementation, and answering success would be worse: with `oldp` null the caller
wants only a length, and success without one hands it an uninitialised size to allocate
against. errno is left alone rather than guessed - its value is not derivable from anything
lawful here, and a caller branches on the return value.

### Where it stops

`klogsrv` still jumps to null inside `find_pid` *after* handling and reporting the failure -
a separate bug. First candidate: `signal` answers `SIG_DFL`, which is zero, and a caller
that invokes the handler it replaced would call exactly that.

### A guard caught me mid-write

Wrote a line-continued string literal in the new `sysctl` reporter - the exact thing D184's
guard exists for - and fixed it to `concat!` before running the gate rather than after.


### The one title shape the loop could not touch

PPSA04263 spins - `sceKernelDirectMemoryQuery` is 98.7% of twenty million calls and the guest
runs to the time limit rather than faulting. The dispatcher declined it, saying "what it is
waiting for is not varied by any diagnostic". `ORBISTOUN_RETURN` varies exactly that, and the
finding's own evidence says so: *a guest that keeps asking the same question has not accepted
the answer*. The report and the dispatcher disagreed and the report was right.

**But mapping it to the sweep would have been worse than declining.** The sweep's oracle is
`outcome.fault != baseline.fault`; a spinning guest never faults, so both sides are `None` and
twenty-four boots answer `Unmoved` whatever happened. Worse, the only way it *can* fire is a
plant that causes a fault where there was none - so on this title it reports success exactly
when the experiment breaks the guest. The decline was accidentally protective.

Reach is the oracle that survives, and `Outcome::reached` has carried it since the sweep was
written. **No new data, no new runs, one comparison nobody was making.** `Finding::Escaped`
reads it, ordered before `Unmoved`, because on a run with no fault `Unmoved` is not a
measurement.

On the real title: `Unmoved { tested: [2], not_addresses: [0, 1, 3, 4, 5] }` - five arguments
hold sizes rather than addresses and the sixth did not break the loop. A measured negative
where there had been no measurement, and one step of that turn moved from a person to the
loop.

### The latent bug it uncovered

The first version reported `Derailed` instead. That fires on `!touched` - *the fault was not
at an address the guest asked for* - and a run that **did not fault** carries `false` for the
same reason an empty list has no first element. Any non-faulting run was being reported as
derailed into non-code.

Latent for as long as every swept baseline faulted. Both are the same mistake: reading a field
whose meaning is only defined when a fault happened, on a run where none did.

