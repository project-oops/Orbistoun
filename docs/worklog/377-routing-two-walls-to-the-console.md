# 2026-09-03 - (/loop) Two walls that need a measurement, routed to the mechanism that asks

```
suites 125   tests 2013   clippy/fmt/identity clean
wall unchanged: read of 0x50 at image+0xf56e09, 197 distinct
```

Fourteenth cron tick. No code. Both remaining moves need a console, and the point of this tick is
that orbistoun already had a way to ask and it was silent.

## What is blocking

**`sceAgcCreateShader`** (D527). The caller reads the **first quadword out of the destination and
dereferences it at `+0x50`** - two levels of layout, neither established. `orbistoun-gpu` finds
shaders by reading the addresses a guest writes into hardware registers, so a real shader object
is not needed for translation; the guest dereferences one anyway, immediately.

**`sceKernelWaitEqueue`** - **1,177 calls in one run**, the largest unimplemented count in the
corpus by an order of magnitude. D524 built the queue's identity and no delivery, on the grounds
that nothing waited and *"the shape of a delivery path should be decided by the first guest that
actually waits"*.

A guest waits now - and the condition has been met in the least useful way. **The first waiter
shows *that* it waits, not *what* it reads back.**

## The mechanism was silent because nothing was recorded into it

obSCEne's backlog 022 is mostly *generated*, from `orbistoun-cli questions`, which ranks every
unverified claim by how often a guest calls the function. Neither of these was in it, because
**orbistoun's own entries carried edge cases and no assumptions** - nothing for the generator to
rank.

Recorded now, and `sceKernelWaitEqueue` enters at the top of its tier:

```text
1177 calls   libkernel::sceKernelWaitEqueue   [5 args]
  ? The layout of the event structure written into arg1 is not established...
```

That is what the generator is for: a question recorded once is asked with the weight the corpus
gives it, not the weight I remembered to give it.

## And the hand-written half

022 has a section for layouts that block a subsystem (`scePadReadState`, `sceAudioOutOpen`) and
names the technique - **poison an oversized buffer, call, report the extent**. Both fit, and both
are written in with what makes them specific:

- the equeue: create, register a user event, trigger, wait into a poisoned array - changed bytes
  are the fields, the answered count says how many entries fit;
- the shader: sharper than a layout, because **orbistoun knows which byte it needs** - the
  destination's first quadword and `+0x50` behind it.

The shader entry also carries what a probe author would otherwise rediscover: the caller tests
`== 0x8a6c003d`, not "did it fail", and `0x8a6c` is libSceAgc's family **established from the
guest's own wrapper** (`0x8a6c000a`, `0x8a6c0002`), not from a document.

## What this is not

Progress on the wall. Writing a plausible structure into either out-parameter would have "moved"
it and been exactly the failure principle 3 forbids.

Decision: [D528](../decisions/D528-two-walls-that-need-a-measurement-and-the-mechanism-that-asks-for-it.md).
