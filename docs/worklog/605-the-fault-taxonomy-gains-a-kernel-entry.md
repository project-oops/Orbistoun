# 605. The fault taxonomy gains a kernel-entry class, and it names itself in the finding

**2026-09-15** - the ranked findings, not only the crash print, now classify a trap

## What 604 left unfinished

Worklog 604 made the **live crash print** name `int 0x41` and call it orbistoun's gap. But the
print is not what the loop routes on - the **ranked findings** are (`diagnose.rs`, the `!`/`->`
worklist each run ends with). And those still could not classify the instruction, for a structural
reason: `FaultSite` carried the registers, the pointees and the address, but **not the faulting
instruction's bytes**. The worker read them, used them for its own print, and threw them away. So
`diagnose.rs` had nothing to classify with, and a kernel entry surfaced as a generic
`Gap::Faulted` "read of 0xffff..." with the arrow "read the calls just before it" - pointing at a
bad pointer that does not exist.

## What changed

- **`FaultSite` now carries `instruction: Vec<u8>`.** The worker populates it from the bytes it
  already reads. A trace can now be classified after the fact, not only at fault time.
- **One classifier, `orbistoun_report::trace::classify_trap`**, used by *both* the live print and
  the finding. They used to be two matches that agreed by luck; now they cannot drift, which is
  the whole point of a deterministic diagnosis - the crash output and the worklist say the same
  thing about the same fault.
- **`Gap::KernelEntryUnimplemented`**, a new taxonomy variant. A kernel entry is a different job
  from a bad pointer - "characterise this vector and add a handler", not "find the wrong pointer" -
  so it routes differently: `where_to_look` points at the interrupt/syscall handling and an obSCEne
  measurement, and `orbistoun-turn`'s `step` returns a `Person` step that says the handler needs
  the vector measured on the device first.

The `Gap` enum is `match`ed exhaustively in `orbistoun-turn`, so adding the variant was a compile
error there until it was handled - a new kind of wall cannot silently produce no work. That guard
did its job.

## What the finding says now

```
! the guest entered the kernel via int 0x41 at image+0x196b91a, which orbistoun does not implement
  -> characterise what int 0x41 reads and returns - an obSCEne measurement, since it is below the
     NID/library layer - then add the handler. A retail title reaching this runs on real hardware,
     so the wall is orbistoun's, not the guest's
```

That is the sentence I had to assemble by hand, hours late, from raw bytes and a wrong-turn through
a filesystem path (worklog 603). Now the run says it. The model is the last resort, not the
disassembler.

## Guests that raise their own trap are split off

`ud2` classifies as `TrapKind::GuestTrap`, not a kernel entry: the guest aborted itself on a check
it failed, so the cause is upstream and the finding says "read the calls just before it: the guest
decided to abort". Keeping the two apart is deliberate - "orbistoun has no handler" and "the guest
gave up" are opposite conclusions and were the two readings this whole thread got confused between.

## Made to fail

- `a_kernel_entry_fault_names_its_vector_and_routes_to_a_handler` (diagnose): a fault whose
  instruction is `int 0x41` produces `Gap::KernelEntryUnimplemented`, names `int 0x41`, and routes
  to a measurement; a `mov` through null stays `Gap::Faulted`. Removing the classifier call fails
  it with the exact old string - "faulted at image+0x196b91a, read of 0xffffffffffffffff".
- `a_software_interrupt_names_its_vector_and_says_the_gap_is_ours` (worker, worklog 604): the live
  print half.

## What is still only half-taxonomised, honestly

This closes the class that walled a title. The broader taxonomy the request pointed at - *every*
fault naming itself - is further along but not finished:

- **Kernel entry / guest trap:** done (this unit).
- **Placeholder-as-pointer:** already its own class (`Gap::ErrorUsedAsPointer`, and the live
  `NOT AN EMULATOR BUG` note).
- **Emulator's own code:** already `EMULATOR BUG`.
- **Null-deref / bad pointer:** partly - `faulted` names the null base register ("find where rax
  was set to zero") but does not yet route it to the call that zeroed it the way a kernel entry
  routes to a measurement. That is the next fault class to make self-describing.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,344 pass / 0 fail, worklogs unique, identity scan clean. Verified on the
live PPSA04263 run: the ranked finding names `int 0x41` and routes it, end to end.
