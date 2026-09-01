# The loop finds a gap nobody had looked at


Turning the loop on all five commercial titles produced one success and four refusals, and the
refusals were the useful part. `PPSA21564` dies at **`write to 0x7fff0001`** - `GuestError::
Unimplemented`, one of our own placeholder codes, used as an address. The guest asked
something, got "not handled", and followed it.

D125 already said what the fix is. What no finding said is **which function answered**: one
names the call that *received* the code, the other the import the fault happened inside, and
the action says *"find what answered with that code just before"* - an instruction to a person
to go looking (D299).

Looking is a sweep. Every call the trace recorded is a candidate; force each to answer zero and
see which one stops the fault being a placeholder dereference. Crisp oracle, no judgement, a
tenth of a second per candidate.

```
*** sceLibcMspaceMalloc answered the code the guest followed:
    reached 25 against 13, and did not fault
```

**Nobody had looked at that function.** It appears nowhere in the session's analysis before the
sweep named it. The earlier `sceKernelReserveVirtualRange` result was the loop *reproducing* a
contract a person had already worked out; this is the loop finding one that had not been.

### What it did not do

It did not fix the function. `sceLibcMspaceMalloc` is an allocator and null is not the right
answer - what it did was replace a wild pointer the guest **follows** with a null the guest
**tests**, which is D125's rule and not a measurement of correct behaviour. The entry says so
in its own assumption: *"what it should really return is not measured."*

The rule was pre-existing and the machinery was written an hour earlier. What was genuinely the
loop's is the diagnosis: which of the recorded calls produced the code, established by trying
them.

### Two things this turned up

- **The finding did not carry what its own action pointed at.** It told a reader to look at the
  preceding calls and did not include them, so the first sweep reported "no unimplemented call
  before it" and stopped. A finding whose action sends somebody looking has to carry the thing
  to look at, or the search is a person's by construction.
- **Accumulation changes what later titles can measure.** `PPSA03416` measured nothing this
  round because the entry from `PPSA02664` had already removed its wall. Run order now matters
  and nothing records it - the same shape as D298, one level out.

### Duplication removed on the way

`cmd_turn` and the integration test had grown a match over every `Taken` variant each. That is
the drift the dispatcher itself was rescued from in D289, reappearing in its output. One
`Taken::say` on the type, and both callers print through it.

