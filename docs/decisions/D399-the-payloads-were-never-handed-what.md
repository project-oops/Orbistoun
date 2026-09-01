# D399 - The payloads were never handed what they read, and the instrument could not see it


**measured** - 2026-08-30

`0x2001` had been recorded as a constant `elfldr` and `pldmgr` die on, invariant to every
handoff variation tried. It is a **module handle**, and the reason it looked like a constant is
that nothing here had ever reached the code that uses it.

### What the hardware run said first

`sceKernelLoadStartModule` on a target console returned `0x15` and `0x14` for application
modules and **`0x2001`** for a system one (D398). That is a numbering scheme, not an error.

### What the payload's own instructions said second

The entry function reads its first argument as a **pointer to a table of function pointers**
and calls through field zero straight away:

```
4a76:  mov  %rdi,%rbx       ; the argument, kept
4a8c:  mov  $0x1,%edi       ; a module handle
4a91:  call *(%rbx)         ; resolve a name, through field zero
...
4aa2:  mov  $0x2001,%edi    ; and if that failed, the system one
4aa7:  call *(%rbx)
```

So `0x2001` is the **second** handle it tries, in a fallback path. It was never a value this
project produced; it is a value the payload carries, and it agrees exactly with what hardware
returns for a system module. Two independent sources, one number.

### Why it never got there

The guest was entered with the argument count and vector a title expects, so `rdi` was
**argc**, which is 1. `call *(%rbx)` then reads address 1. That is the whole of the wall three
sessions arrived at from different directions: not a handoff field with a wrong value, but the
handoff never being handed over.

Entered with the resolver table instead, the same payload resolves `sceKernelDlsym` and
`getpid` through it and makes three calls where it previously made none.

### The instrument was reporting on a structure the guest never received

`orbistoun-cli handoff` poisons one field and asks whether the guest used it. It set the poison
**and nothing else** - so every run it made was under whatever entry argument the configuration
named, and for a bare payload that is not the handoff. It poisoned fields of a block nobody was
given and concluded *no field was reached*.

That is the third principle's failure one level up: a report saying more than its measurement
supports, and it is the eighth instrument in this project caught doing it. The pattern is
identical every time - the tool varies one input and assumes the rest of the world is the shape
it has in mind.

`ORBISTOUN_ENTRY_ARGUMENT` now exists so the instrument can select what it is asking about, and
it is registered with `Experiments` in the same change - an unregistered setting is one a run
can be under while reporting itself ordinary, which is how an honest status slot got written by
a propped run once already. An unrecognised value says so rather than silently meaning the
default, because a misspelled setting that behaves exactly like no setting is what let the
handoff instrument stay wrong for as long as it did.

### Adding a setting touches three lists, not two

The registry, the `DECLARED` mirror beside its test, and `Experiments`. This change remembered
the third - the one that was forgotten last time and let a propped run write an honest record -
and missed the second, which the env crate's own guard caught immediately.

Worth writing down rather than filing as carelessness: the count is **three**, the failure mode
of each is different, and knowing that the last time this went wrong it was a different one of
the three is exactly the information a person adding the next setting does not have.

### What it measured once it could

For `elfldr`: field 0 is called, fields 1 and 2 are read, field 5 is **written**. That is
structure knowledge taken from the guest rather than guessed, and it is the first of it.

### And what fields one and two are

Reading the sites the instrument named, both are **pointers to a pair of 32-bit integers**, and
the payload rejects the lot if either is negative:

```
682b:  mov  0x10(%rdi),%rcx   ; field two, as a pointer
682f:  mov  (%rcx),%edx       ; an int
683e:  js   7469              ; negative, and the whole call returns -9
6844:  mov  0x4(%rcx),%ecx    ; a second int beside it
6855:  mov  0x8(%rdi),%rcx    ; field one, the same shape again
```

Two non-negative integers, in pairs, checked before anything else runs: that is the shape of
**descriptors**, and a loader daemon wanting two pairs of them is unremarkable. It is not called
proven here - the shape is measured, the meaning is inferred from it - but it is specific enough
to act on, and the next run says whether it was right. Markers will not do: a marker is a large
address whose low half reads as a negative int, which is the one value this code refuses.

### What is deliberately not changed

The default entry argument stays as it is. Which one a guest wants is a fact about the guest -
payloads want the resolver table, titles want the argument vector - and six real titles are
measured against the current default. Choosing per guest is the right shape and it needs its own
change, with those six re-measured under it.

