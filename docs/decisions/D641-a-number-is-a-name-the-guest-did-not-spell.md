# D641 - Four numbers, and the wall moved

**Status:** measured
**Date:** 2026-09-09

## The probe started making real syscalls, and orbistoun refused them

`REQ-20260908T1620Z-4c1e` came back delivered: obSCEne now reports its syscall route per leg. Run
locally, it says something nobody had asked for:

```text
OBS|measure|010-kernel/syscall-route|syscall|payload-args|0x0|flag
OBS|measure|010-kernel/syscall-route|syscall|gadget|0x700000008c0a|address
```

**`0x700000008c0a` is orbistoun's own thunk table** - `getpid` plus ten, the nop sled D400 laid
down so a payload's gadget hunt finds a real syscall entry. The probe's new build looks for one in
title mode and finds it. So for the first time a guest here issues **real syscalls**, and the run
that followed went backwards: 223 imports to 187, 394,000 calls to 4,886, a clean exit replaced by
a fault at `0x5e2d`.

The report named the reason without being asked:

```text
orbistoun: the guest made 4213 syscalls, in this order:
    0    603  nothing here implements it  (0x400000280000)
    1    603  nothing here implements it  (0x40000027c000)
    2    603  nothing here implements it  (0x400000278000)
```

Four thousand two hundred and thirteen calls to **number 603**, with a first argument walking
*backwards* through the guest image in `0x4000` steps. That is a program mapping its own address
space, never told where a region ends, stepping by hand and never stopping.

**603 is `sceKernelVirtualQuery`, which this project has implemented for months.** It answered by
name and refused by number - the same one-function-two-answers shape D632 found in
`sceKernelDlsym`, one layer down.

## Four numbers, each one observed before it was added

`vendor-syscalls.toml` sets the bar itself: *"Nothing may be added here because it seemed likely.
The bar is that a guest was watched asking for the number, and that what it does with the answer is
described."* Four entries now meet it, and the order matters:

| number | name | how it was reached |
|---|---|---|
| 603 | `sceKernelVirtualQuery` | 4,213 calls, walking the image backwards |
| 572 | `sceKernelAllocateDirectMemory` | **only after 603 was bound** |
| 573 | `sceKernelMapDirectMemory` | only after 572 |
| 574 | `sceKernelReleaseDirectMemory` | only after 573 |

The last three were invisible until the first was answered - the guest never got past its memory
walk to ask. That is what makes them observed rather than inferred, and it is why they were added
in two steps rather than one.

**585 and 586 are deliberately absent.** obSCEne picks `obs_is_ps5() ? 585 : 573` for map and
`586 : 574` for release; this guest asked for the lower pair, so under orbistoun its generation test
answers "not the current one" - a fact about what this emulator presents, worth knowing on its own.
Adding the upper pair because their siblings were seen is exactly the "it seemed likely" the file
refuses.

## What it moved

```text
imports  225 distinct (+38), 432605 calls (+427701)
fault the guest called exit   (was 0x5e2d)
verdict  FURTHER  executed code it could not reach before
```

**225 distinct imports** - past the previous best of 223 - and a clean exit. Against the console's
pkg leg:

| | before | after |
|---|---|---|
| checks concluding differently | 255 | **84** |
| passed there, failed here | 115 | **5** |
| distinct findings | 17 | **5** |

What is left is three libSceNet checks, one input-SDK poll, and `900-surface/control` - the weak
symbol that should be null and is bound (D633).

## And the AGC wall answered, in the same batch

`REQ-20260908T1621Z-7a5d` resolved `not-possible` - no leg maps libSceAgc - but the sweep carried
`166-agc/create-shader` measurements anyway, from the payload leg:

| condition | answer |
|---|---|
| well-formed header | `0x8a6c002f` |
| the `0xd8` payload shape | `0x8a6c002f` |
| the `0x118` payload shape | `0x8a6c002f` |
| **PPSA03416's own `0x108` shape**, supplied by this project | `0x8a6c002f` |
| null argument | `0xb` |

And `OBS|bytes|…|out-after` is unchanged poison: **the call writes nothing.**

So the header is not the variable. Every shape is refused identically, including one a real title
passes. Forcing that measured code into PPSA03416 was tried and moved nothing - as forcing `0x0`
did not, earlier - which says again that the guest acts on the out-parameter and not the return.
D621's wall stands, and it is now bounded: not a header question, and not a return-code question.
