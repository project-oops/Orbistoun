# 423. The fault message was guessing, and it cost a finding

**2026-09-07** - directed, continuing 422

## What was done

**Every guest breakpoint was reported as stub padding, with nothing having looked at the
address.** The handler matched the exception code and took a fixed string; it holds the
faulting address, the stub table's span sits in atomics beside it, and `locate` already existed
to name the region containing an address. None of it was consulted (D576).

The line contradicted itself in the output: stubs are region slot 1 and the title's modules slot
4, `locate` returns the first match, so an address it names as *the title's own modules* cannot
be in the stub table - and the same line called it stub padding. The Unix half of the same table
already said *"or a debugger interrupted it"*, which is the tell that one side admitted the
ambiguity and the other asserted through it.

A pure `breakpoint_kind_in(address, stub_base, stub_len)` decides it now, three ways: inside the
span, outside a known span, or no span registered. The third is deliberate - "checked, and it is
not there" and "there was nothing to check against" are different states. The exit-code table
holds no address at all, so it stops naming a cause. The test asserts both edges and was watched
failing with the classifier forced to one answer.

## It had already cost a finding, from yesterday afternoon

Worklog 422 recorded PPSA03416's wall as *"the guest then enters the stub table off a thunk's
start"* and put a linking question in the Next list, for a session allowed to open the loader.
That came entirely from this message. **Withdrawn**, in 422 and in D575's table.

What the title actually does:

```text
breakpoint - not stub padding - at 0x480002e68020 (the title's own modules+0x2e68020)
```

A trap instruction in its own module code, reached from the executable. An assertion, a
deliberate `__debugbreak`, or a jump into data - a fault record cannot separate those, so the
report names none of them. The useful part is where it points: at what the guest checked just
before trapping, not at the linker.

## What the guest checked, and a measurement that had to be made twice

The bytes at the trap say it. Before `0x480002e68020` is a call; after the `int3` is
`mov ebx, 0x8002000c` - `ENOMEM` in the vendor encoding - then a jump into a shared epilogue.
That is what a compiler emits after a call it believes never returns.

The last import before it is `sceKernelMprotect`, answering `0x80020016` (`EINVAL`) for 256 MiB
of write access inside the title's own module. The guest checked that, took its failure path,
called a routine that was not supposed to come back, and came back.

Answered with success instead:

| | refused | forced |
|---|--:|---|
| imports | 186 | **192** |
| fault | `modules+0x2e68020`, a trap | `image+0x1389269`, read of `0xa0` |
| verdict | | **FURTHER** |

`image+0x1389269` is the wall this title's record already held from 2026-09-04, so answering the
call puts it back where it was.

**The first run of that experiment was void and the run said so.** It used the hash;
`ORBISTOUN_RETURN` matches by label, and a named import's label carries the name, so it matched
nothing - printed on line three, in the words D230 added for exactly this case:

```text
orbistoun: ORBISTOUN_RETURN matched no import called "0x366131779b0023bd"
orbistoun: no import will answer a forced value
```

The unchanged run was then read as the answer, because the grep that pulled out the verdict did
not include those words, and "the refusal is ruled out" went into this worklog and into D576
before either was true. The guard worked; the reading did not.

## Surprises

- **The self-contradiction was visible in every report for months.** Region name and fault kind
  disagreed on one line and nobody read them against each other, including the session that had
  just added a region-naming improvement.
- **`AT_THE_INSTRUCTION` turned out to be documentation only.** Nothing reads it; the handler
  indexes it positionally. Adding kinds to it was safe, and that it is unread is worth knowing
  before anyone trusts it as a contract.
- **A diagnostic that matches nothing looks exactly like one that changed nothing**, and the
  only difference is a line near the top of the run. Read it before reading the verdict. Twice
  today a conclusion came from the bottom of a report whose top said not to bother.

## Next

- **`sceKernelMprotect` on a range the crate did not itself map.** Measured to be PPSA03416's
  wall. The guard that refuses it is there so a typo cannot re-protect this process's own code,
  so the fix is to consult the regions already reported to the crate rather than to drop the
  guard.
- `image+0x1389269`, read of `0xa0` - where the title lands once the protection call is
  answered, and where its record already said it was.
- Whether any other breakpoint reading in the back catalogue rested on the old message. Only
  this title was re-run, and whether the address was in the stub table was never recorded.
