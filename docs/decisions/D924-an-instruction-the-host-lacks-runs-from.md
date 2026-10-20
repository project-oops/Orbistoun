# D924 - An instruction the host lacks runs from a trampoline, or at its fault where none fits

**Status:** decided
**Date:** 2026-10-20

D725 rewrites the instructions the host lacks at link. This entry says how each site is
rewritten. The loader's sweep finds the site and `orbistoun-loader::rewrite` decides it while
the image is placed and still writable:

- **`extrq` and `insertq` of five bytes or more** become `jmp rel32` to a trampoline, padded
  with `int3`. The trampolines sit in pages the module's span grows by, after its last segment,
  so the next module is placed past them. A trampoline first steps below the System V red zone.
  It saves the flags and the general registers it borrows, moves the operands through its frame
  with `movdqu`, and does the quadword arithmetic in general registers. It loads the result back
  over a cleared upper quadword and jumps to the instruction after the site.
- **`extrq` and `insertq` shorter than a jump** (the four-byte register form, with no REX
  prefix) keep their bytes. The illegal-instruction fault they raise is answered in the worker's
  vectored handler, which carries out the instruction against the context record's `xmm`
  registers and resumes past it.
- **`monitorx`** becomes `nop dword [rax]` and **`mwaitx`** becomes `pause`, each in its own
  bytes. A monitored wait may end at any time for any reason, so a wait that ends at once is one
  the architecture allows. The code around it already loops on the value it waits for.
- **`clzero`, `rdpru` and `mcommit`** are refused by name when they fault. No title in the
  library uses them, and `rdpru` reads console counters that nothing has measured.

The semantics are `published`: AMD64 Architecture Programmer's Manual, Volume 4, `EXTRQ` and
`INSERTQ`. The manual leaves two cases undefined: the destination's upper quadword, and a field
running past bit 63. For both, the behaviour is `measured` on an AMD host processor that has the
instructions (a Zen 5 desktop part): the upper quadword is cleared, and only the bits below 64
take part. The console's own answer is asked for on the obSCEne bus (`REQ-20261020T1532Z-amd1`, section 4). Each rewrite is
tested against the reference over generated inputs. On a host with SSE4a, the reference is also
tested against the instruction itself.

The plan lists every rewrite by address as `<mnemonic> <how>`. The rewrites enter the plan's
digest only when there are any, so a host that rewrites nothing keeps its plans' names.
`ORBISTOUN_HOST_LACKS` makes a host that has the instructions rewrite as one without them
would. It also plants `ud2` over each site left to fault, so the fault path runs too.

**Why:** the library census (2026-10-20) found 160 SSE4a sites in five retail executables:
60 `insertq` with immediates, 4 `extrq` with immediates and 96 `extrq` register forms. 27 of
the register forms are four bytes. The census also found one `monitorx`/`mwaitx` spin-wait, and
no `clzero`, `rdpru` or `mcommit`. A jump fits over 133 of the sites. The other 27 would each
need a second instruction moved into the trampoline, and that is safe only if no branch targets
it, which a jump table hides from a static sweep.

**Rejected:**
- Displacing the instruction after a short site into its trampoline. A branch into the
  displaced instruction would run half-rewritten code, and no sweep can rule one out.
- `ud2` over short sites on every host. On a host without SSE4a the instruction faults by
  itself, and the handler decodes it where it stands, including at a site the sweep missed. On a
  real host, writing over a site the sweep took from data would corrupt that data for nothing.
- Leaving `mwaitx` to fault and sleeping in the handler: a fault per wait, to model a timer the
  architecture lets an implementation ignore.
