# D528 - Two walls that need a measurement, and the mechanism that asks for one

**decided** - 2026-09-03

Both things now blocking PPSA02664 are structure layouts, and neither can be answered from
orbistoun's side. What this decision does is route them properly rather than guess.

## What is blocking, and why neither is implementable

**`sceAgcCreateShader`** - the wall D527 named. The caller passes a destination, then reads the
**first quadword out of it and dereferences that at `+0x50`**. So the destination's first field
is a pointer to an object of at least `0x51` bytes. Two levels of layout, neither established.

`orbistoun-gpu`'s pipeline finds shaders by reading the addresses a guest writes into hardware
registers (that is the design), so a *real* shader object is not needed for translation - but
the guest dereferences one immediately, and a fabricated pointer is dereferenced at `+0x50` on
the very next instruction.

**`sceKernelWaitEqueue`** - **1,177 calls in one run**, the largest unimplemented count in the
corpus by an order of magnitude. D524 built the queue's identity and deliberately built no
delivery, on the grounds that nothing waited and *"the shape of a delivery path should be
decided by the first guest that actually waits"*.

A guest waits now. It submits a flip, registers a flip event, and blocks indefinitely. And the
condition D524 set has been met in the least useful way: the first waiter shows **that** it
waits, not **what** it reads back. The event structure written into the output array is still
unknown.

## The mechanism, used rather than worked around

obSCEne's backlog 022 is *mostly generated* - from `orbistoun-cli questions`, which ranks every
unverified claim in the knowledge base by how often a guest calls the function. Neither of these
was in it, because **orbistoun's own records did not carry the question**. I had written both
entries with edge cases and no assumptions, so there was nothing for the generator to rank.

So the questions are recorded where they belong:

```text
1177 calls   libkernel::sceKernelWaitEqueue   [5 args]
  ? The layout of the event structure written into arg1 is not established...
   1 calls   libSceVideoOut::sceVideoOutAddFlipEvent   [3 args]
  ? What identifier a flip event carries, and what a waiter reads back out of it...
```

`sceKernelWaitEqueue` now enters that list at the top of its tier. **That is the point of the
generator**: the ask list re-ranks itself, and a question recorded once is asked with the weight
the corpus gives it, rather than with the weight I remembered to give it.

## And the hand-written half, because these are not "call it and see"

022 has a section for layouts that block a whole subsystem - it already carries `scePadReadState`
and `sceAudioOutOpen`, and it names the technique: **poison a generously oversized buffer, call,
and report the extent**, which measures a structure without being told one.

Both of these fit it, so both are written into that section with what makes each specific:

- For the equeue, a probe can create a queue, register a user event, trigger it and wait into a
  poisoned array - the changed bytes are the fields, and the answered count says how many entries
  an array holds.
- For the shader, the target is sharper than a layout. **orbistoun knows which byte it needs**:
  the destination's first quadword, and `+0x50` behind it.

The shader entry also carries something a probe author would otherwise have to rediscover: the
caller does not test for failure, it tests `== 0x8a6c003d`. And `0x8a6c` is libSceAgc's error
family **established from the guest's own wrapper**, which returns `0x8a6c000a` for a null
argument and `0x8a6c0002` for a bad one - not from any document.

## What this decision is not

It is not progress on the wall. It is the recognition that both remaining moves need a console,
that orbistoun already has a mechanism for asking, and that the mechanism was silent because
nothing had been recorded into it. Writing a plausible structure into either out-parameter would
have "moved" the wall and been the exact failure principle 3 forbids.
