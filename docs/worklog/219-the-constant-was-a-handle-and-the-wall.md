# 2026-08-30 - The constant was a handle, and the wall was an argument


`0x2001` had been the thing `elfldr` and `pldmgr` die on for three sessions, established as
invariant to every handoff variation and never identified. Two sources settled it within an hour
of each other and they agree.

The hardware run said `sceKernelLoadStartModule` returns small integers for application modules
and `0x2001` for a system one - a numbering scheme. The payload's own instructions said the
rest: it reads its first argument as a **table of function pointers**, calls field zero to
resolve a name with module handle 1, and falls back to `0x2001` if that misses. The value was
never something this project produced. It is a value the payload carries, and it matches what
the console returns.

It never got there because the guest was entered with an argument *count* where it expected a
*pointer*. `argc` is 1, so `call *(%rbx)` reads address 1, in the second call of the program.

Handed the resolver table instead: `sceKernelDlsym` and `getpid` both resolve, three calls where
there had been none, and the fault moves from `0x1` to somewhere with `0x2001` in it - which is
the payload finally running its own fallback path.

### The part worth remembering

**The instrument had been reporting on a structure the guest never received.** `handoff` poisons
a field and asks whether the guest used it; it set the poison and nothing else, so every run it
made was under whatever entry argument the configuration named - not the handoff. It poisoned
fields of a block nobody was handed and said *no field was reached*.

That is the eighth instrument caught this way, and the shape has never varied: the tool changes
one input and assumes the rest of the world is what it has in mind. Worth stating as a check
rather than a lesson, because the lesson clearly does not stick - **an instrument that varies X
must also establish the conditions under which X means anything**, and say so if it cannot.

Fixed, it produces the first real structure knowledge for these payloads: field 0 called, 1 and
2 read, 5 written.

The default entry argument is deliberately unchanged. Which one a guest wants is a fact about
the guest, and six titles are measured against the current one; choosing per guest needs those
six re-measured under it.

### A flaky gate, found by accident and worth more than the fix

The gate failed on a loader test reserving pages another test already held. It passed when run
alone, which is the tell.

`Range::take` hands out an address no other caller of **that instance** will get - the cursor is
atomic and concurrent takes are safe. Two tests in `relocate.rs` each declared
`static RANGE` *inside their own function*, so each got its own instance, each cursor started at
zero, and both handed out the same addresses. Whichever ran second failed to reserve.

The mechanism was correct and the usage defeated it, which is why the comment above each one
said the opposite of what was happening: *"so two tests running at once cannot reserve the same
page"*. Both tests said that. Both were wrong, in the same file, four lines apart.

Now one static per test module, and `take` documents the distinction where somebody about to
repeat it will read it. The other two crates using this have a single test each and were fine by
luck rather than by design.

Worth the space because this project has already written down why an intermittent failure is
worse than a plain one: it teaches you to re-run until green. This one would have been re-run
until green.

