# 2026-08-24 - Four eliminations, no confirmations (D218)


**Done.** Two experiment mechanisms built, both walls swept, and **both leading hypotheses
killed**. Written down because a negative result nobody records gets re-run.

### PPSA04263: the map shape was the variable nobody had varied

Three answers had been swept and changed nothing - return code, third structure field,
buffer clearing. The *map* had always been one free region starting at zero.

That is worse than an untried option: a guest hunting for a region matching a criterion
cannot tell "wrong value" from "wrong shape" when there is one region to inspect, so the
third-field sweep proved less than it read as proving. Same shape as D187.

`Settings::map_shape` is data in the run configuration - `whole`, `reserved-low`,
`fragmented` - defaulting to what every earlier measurement was taken against.

**Given four regions, the guest queried every one, in order, correctly, and restarted at
zero anyway.** Map shape eliminated. Four things now swept, and the guest is indifferent to
all of them.

### And a claim I nearly made and had to withdraw

I built the fragmented shape partly to settle whether the second field is `end` or
`start + size`. It cannot. A **contiguous** map has each region beginning where the last
ended, so a guest feeding back the previous end produces identical offsets under either
layout - however many regions there are. That needs a map with a *gap*, which every current
shape is explicitly tested against producing.

Caught it while reading the offsets rather than after writing it down, which is luck rather
than method.

### PPSA02664: planting a value, and it changed nothing

A stub policy sets what a function *answers*; nothing set what it *does*. Both walls had
been narrowed to "an out-parameter nobody wrote" (D217), and the only mechanism for a side
effect is a declaration keyed by a name this function does not have.

`ORBISTOUN_WRITE=<import>:<slot>:<value>` plants a `u64` at the address in an argument,
resolving the import by hash where there is no name. Three things it does deliberately:
only the **stack** is writable (the image's runs are protected after relocation, so a plant
there would fault inside the emulator); a request matching **no import says so** rather than
reporting an unchanged run; and the **counts go in the run conditions**, because a write
that matched an import and refused every target looks exactly like one that landed and
changed nothing.

**`arg0` and `arg5` are both not it.** Both point into the stack, both planted
(`1 planted, 0 refused`), and the fault stayed at `0xfffe0` exactly.

### Where both walls stand

Eliminated by measurement, not argument: the stub return, unwritten stack, `arg0`'s target,
`arg5`'s target, `memalign`, and the map shape. The out-parameter reading was the only
surviving explanation this morning and is now in trouble itself.

That is a worse position and a better-understood one.

### What was deliberately not built

Declaration by NID - the mechanism to *implement* an unnamed import. It was conditional on
the out-parameter hypothesis surviving, and it did not. Building a way to fill an
out-parameter nothing has shown to exist is speculation, which principle 11 refuses.

