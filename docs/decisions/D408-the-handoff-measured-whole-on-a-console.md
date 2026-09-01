# D408 - The handoff, measured whole on a console, and made faithful here


**measured** - 2026-08-31

obSCEne ran on a real 12.40 console as an elfldr payload - driven through the full chain
(prosperous `pros send` -> elfldr -> obSCEne, D407's word-zero fix on this side, two obSCEne
output-channel bugs fixed on that side) - and its `136-kernel` section read the whole
`payload_args` the loader hands a payload. This is the first time this project has seen the
struct rather than inferred it.

### What the console handed over

| word | value | class |
|---|---|---|
| 0 | `0x8000005b0` | **getpid** - libkernel base `0x800000000` + `0x5b0`, exactly D209/D407 |
| 1 | `0x200698100` | userland pointer |
| 2 | `0x200698200` | userland pointer |
| 3 | `0xffff86615c607840` | **kernel** heap pointer |
| 4 | `0xffffffff8c290000` | **kernel** base (`kdata_base`) |
| 5 | `0x200698300` | userland pointer |
| 6-19 | `0x0` | null |

Word zero confirms the whole scheme D407 was built on, against the machine rather than by
derivation. Words three and four are the escape primitives D208 named - a kernel-heap pointer and
the kernel base - measured for the first time. `0xffffffff8c290000` is the anchor a `ucred`-offset
walk starts from, and it was said here, wrongly, to be unobtainable; it took one probe using the
primitives elfldr already hands over.

### What was implemented

The handoff this project builds now mirrors that shape when a firmware is present: getpid at word
zero (D407), non-null pointers at one through five, null from six on. The unknown fields became
*zero* rather than markers, because that is what the console's were and a payload checking a
field it expects null against a marker would branch wrongly.

**The real values cannot be reproduced and are not faked.** The kernel pointers are canonical
high-half addresses, which a user-mode host process cannot map, so a deref of the true number
would fault on the host before reaching anything. So each field is handed a pointer of the right
*shape* - non-null where the console's was, backed by mapped firmware memory - which is honest
about being a stand-in while still letting a payload's field check pass and its next read be
observed.

### What it did and did not move

It made the handoff faithful; it did not move the payloads. All six still stop at the same error
exit after the same six system calls. So the wall is not the shape of the handoff - the payloads
read fields one, two and five and no longer meet a marker there - it is one layer deeper: the
escape's *operation*. The primitives are pointers the payload writes a kernel-read request
through and reads a kernel value back from; backed by zeroed memory, that read returns zero, and
the escape gives up. Making it proceed means modelling what the kernel-r/w primitive returns,
which needs the crt0's escape sequence (open source, readable with provenance per D208) or a
second measurement - not more of the handoff.

