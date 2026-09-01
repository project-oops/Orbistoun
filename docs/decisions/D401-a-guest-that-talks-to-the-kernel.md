# D401 - A guest that talks to the kernel directly left no trace on the work list


**measured** - 2026-08-30

`orbistoun-cli worklist` ranks what to implement next by totalling every import a guest called,
across every run recorded. It is the list this project works from.

It could not see a system call.

### Why that is not an edge case

A guest reaching the kernel by number touches **no stub**, so it contributes nothing to the
ranked imports - and that is not unusual behaviour, it is how every open-toolchain payload
works. `elfldr` resolves exactly two names, uses one of them to build a syscall entry (D400),
and from there talks to the kernel directly. The call that stops it dead is number 649, and the
work list had 358 entries and not one of them was it.

The information existed. It reached `stderr` at the end of a run - *the guest asked the kernel
for call 649 directly, and nothing here implements it* - and then went nowhere. Same shape as
the sysctl names before D397 and the unanswered paths before D387: a fact the loop produced,
printed once, and dropped.

### Recorded beside the imports, not among them

`CallTrace` gains a `syscalls` field rather than folding these into `calls`. Folding would have
been shorter and wrong twice: `distinct` means distinct **imports** and every report that prints
it says so, and a syscall has no stub index to be indexed by.

### Ranked by runs, because the count does not exist

The recorder is a bitmap - sixty-four words, one bit per number - so it knows *that* a number
came up and not how many times. Ranking these by call volume alongside the imports would have
meant inventing the volume, which is the one thing this project spends its effort not doing. So
they rank by **how many runs asked**, which is a fact, and is the better question anyway for
something that blocks a payload outright rather than costing it time.

The first argument is carried with each, because for a call nobody can name it is most of what
there is to go on: `649` says which entry to write, and `649(2, ...)` starts to say what it is
for.

### The compatibility trap, which is the part that would have bitten

The traces directory is not wiped between versions and the work list reads every file in it.
`cmd_worklist` skips a file it cannot parse **with a note rather than a failure**, so a field
without a `serde` default would have turned the entire history into skipped files and a work
list quietly counting only today. There is a test that an older trace still loads, and it exists
because the failure would have been silent.

