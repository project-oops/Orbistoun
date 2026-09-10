# D576 - A breakpoint was called stub padding without anything looking at the address

**Status:** measured
**Date:** 2026-09-07

## The message, and what stood behind it

Every guest breakpoint was reported as:

```text
breakpoint - stub padding - at 0x480002e68020 (the title's own modules+0x2e68020)
```

and the exit status as *"execution reached stub padding, so a stub was entered off its start"*.

**Nothing had looked at the address.** The fault handler matched the exception code and took a
fixed string:

```rust
BREAKPOINT => (at_instruction[1], rip),
```

`at_instruction[1]` is the constant `"breakpoint - stub padding - at"`. The handler holds `rip`,
the stub table's base and length are in atomics it can read, and `locate` already existed to
name the region containing an address - and none of that was consulted.

**The line contradicts itself, in the output, and had done for months.** Stubs occupy region
slot 1 and the title's modules slot 4; `locate` returns the first match. So an address the
report names as *the title's own modules* provably is not in the stub table, while the same line
calls it stub padding. The two halves came from different places and only one of them had
checked anything.

The Unix half of the same table was already honest - *"trap - execution reached stub padding, or
a debugger interrupted it"* - which is the tell: one platform's wording admitted the ambiguity
and the other asserted through it.

## It cost a wrong finding the same day

Worklog 422 recorded PPSA03416's wall as *"the guest then enters the stub table off a thunk's
start"*, and named the next step as a linking question for a session allowed to open the loader.
That reading came entirely from this message. It is withdrawn: the address is in the title's own
module code, the stub table does not cover it, and no part of the diagnosis was ever measured.

This is the failure principle 3 already names for the tools - *a message naming a cause must
come from the branch that determined it* - arriving in the one place where a reader has least
ability to check it, because a fault message is what they read instead of stepping through.

## Three answers, because three things can be known

`breakpoint_kind_in(address, stub_base, stub_len)` is a pure function beside `locate`, split
from the atomics for the reason `locate` gives: the handler that uses it cannot be stepped
through, and a wrong answer is a message asserting a cause at the moment it matters most.

| condition | kind |
|---|---|
| the address is inside the registered stub span | `breakpoint - stub padding - at` |
| the span is known and the address is outside it | `breakpoint - not stub padding - at` |
| no stub span was registered | `breakpoint - at` |

**The third is not padding on the design.** "Checked, and it is not there" and "there was
nothing to check against" are different states, and collapsing them would put the same
overstatement back one level down.

The exit-code table keeps no address at all, so it now says *"a trap instruction; the fault
record says whether it was stub padding"* rather than naming a cause it cannot reach.

The test asserts both edges of the span and the unregistered case, and it was watched failing:
with the classifier forced to answer "in stubs" it fails on the outside cases, which is the
property that makes it a guard rather than a decoration.

## What the guest is actually doing

PPSA03416 now reports:

```text
breakpoint - not stub padding - at 0x480002e68020 (the title's own modules+0x2e68020)
```

A trap instruction in the title's own module code, reached from the executable. An assertion
that fired, a deliberate trap on a path the guest decided was wrong, or a jump into data - and
a fault record cannot tell those apart, so the message names none of them. What it establishes
is where to look next: what the guest checked immediately before trapping. The bytes there
answer it.

## What the guest checked, measured properly the second time

**The bytes at the trap say it outright.** Immediately before `0x480002e68020` is a call, and
immediately after the `int3` is `mov ebx, 0x8002000c` - the vendor encoding of `ENOMEM` -
followed by a jump back into a common epilogue. That is the shape a compiler emits after a
call it believes never returns: the trap catches the case where it does.

The last import before it is `sceKernelMprotect`, answering `0x80020016` - `EINVAL` under the
same encoding - for 256 MiB of write access starting inside the title's own module. So the
guest checked that return, took its failure path, called a fatal routine that was not
supposed to come back, and came back.

Forced to answer success, **the wall moves**:

| | refused | forced to 0 |
|---|---|---|
| imports | 186 | **192** |
| fault | `modules+0x2e68020`, a trap | `image+0x1389269`, read of `0xa0` |
| verdict | - | **FURTHER** |

`image+0x1389269` is the address this title's compatibility record already held from
2026-09-04. Answering the call puts it back where it was, which is corroboration that the
refusal is the whole of the new wall rather than a step on the way to it.

## The first attempt at that measurement was void, and said so

It was first run as `ORBISTOUN_RETURN=0x366131779b0023bd:0x0`, the hash. Matching is by
**label** - `library::name`, or `library::0xhash` only where nothing has named it - so a hash
for a named import matches nothing. The run said so on its third line:

```text
orbistoun: ORBISTOUN_RETURN matched no import called "0x366131779b0023bd"
orbistoun: no import will answer a forced value
```

That warning exists because D230 anticipated this exact mistake - *a forced return that
matched nothing is visible rather than inferred from an unchanged run*. It was printed, and
the unchanged run was read as the answer anyway, because the grep that pulled the verdict out
did not include those words. **This entry asserted the refusal was ruled out; it was never
tested.** The tool was right, the reading was not, and the guard only works if somebody reads
the line it prints.

## What this does not establish

**That the other breakpoint readings in the back catalogue were wrong.** Any run whose fault was
a breakpoint carried this message, and whether the address was in the stub table was never
recorded. Only PPSA03416 was re-run.

**Nor that the fatal routine is the only way this title reaches a trap.** The bytes and the
forced run agree on this path; another failure reaching the same epilogue would look identical,
and only one input was varied.

**And the premise of that entry was a rounding error, corrected by D595.** The "one file read of
zero bytes" this reasoning starts from was four hundred and two bytes of `boot.config`, read
completely and successfully, printed as `0 KiB` by an integer division. What survives is that the
guest never reads `globalgamemanagers` through its descriptor - which is now measured per read
rather than inferred from a summary.

**And one reading here was wrong, corrected by D578.** The four paths this title probes and does
not find - `data.unity3d` and three `globalgamemanagers.res` variants - were read as evidence
that it never reached the loose-file layout it actually ships. It reaches it: a record of
*successful* opens shows it opening `globalgamemanagers`, the IL2CPP metadata and the boot config.
The four are the archive layout the title does not use, and reasoning from the missing half of
the evidence is what made them look like a cause.

**Nor what `sceKernelMprotect` should do about module memory.** Refusing it is measured to be
the wall, which says the current behaviour is wrong for this guest - not what the right
behaviour is. The range belongs to a placement `orbistoun-kernel` does not own, and the guard
that refuses it exists so a typo cannot re-protect this process's own code. Consulting the
regions already reported to the crate is the obvious shape, and is not attempted here.
