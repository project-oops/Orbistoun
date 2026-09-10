# 461. A hypothesis tested and rejected

**2026-09-09** - directed, continuing 460

Bus idle a tenth pass. No sweep in eight hours; both my requests and four others unclaimed.

## The unattributed imports do not explain the null binding

`orbistoun-cli imports` shows **54 of the payload's imports attributed to `?`** - no library - and
one of them is `sceKernelGettimeofday`, the symbol D628 found the guest calling as `0`. A tempting
join: perhaps an import whose library cannot be determined is also the one that fails to relocate.

Tested rather than believed. `ORBISTOUN_DUMP` on three of them:

```text
ORBISTOUN_DUMP=sceKernelDeleteEqueue,sceKernelGetCpuFrequency,sceKernelCreateEqueue
  -> armed 4 of 1040 stub slot(s)      (and no "matched no import")
```

All three have labels and slots. **The attribution gap is a listing gap, not a resolution one**, and
the two findings are independent. D628's null binding still has no cause.

## And the gap itself is honest

`inspect` says the payload is a **bare ELF with zero vendor segments** and no proc-param. There is
no vendor library table in it, so for those 54 there is nothing to attribute *from*: the `?` is the
container declining to say, not orbistoun failing to read.

Orbistoun could fill them in from its own declarations - it declares `sceKernelDeleteEqueue` in
libkernel - but that is an inference, and showing an inference where a reading belongs is the
failure this session has spent all day finding. Runtime labels are unaffected and correct
(`libkernel_fs::sceKernelWrite` and the rest), so nothing downstream reads the `?`. Left alone.

## The unnamed hash the payload imports

`unknown::0x384a0ae5cd37b0d4` is imported only by the payload, and obSCEne's source is here, so it
should be findable. It is not in `imports.c`'s 251 declared pairs, nor among the 249 `OBS_WEAK`
platform declarations. So it comes from somewhere else in that link - the SDK, or a symbol declared
where neither grep looked - and the cheap routes are exhausted.

## Where the loop actually stands

Ten passes, and the productive local work is done. What is left is blocked, and it is worth being
plain about which kind:

| | blocked on |
|---|---|
| Six title-module imports, 87.6% of all recorded calls (D630, D633) | scope: import resolution |
| The null binding at `obs_sink_open` (D628) | same |
| A symbol that should be null, bound (D633) | same |
| `ORBISTOUN_DLSYM_STUBS` as a default (D632) | a decision |
| Seven declined-syscall casualties (D626) | `REQ-…-4c1e`, unclaimed 9h |
| `sceAgcCreateShader`, three titles (D621, D636) | `REQ-…-7a5d`, unclaimed 9h |
| Four unnamed vendor hashes, one aborting a title (D636) | a request slot, held at the protocol's two |
| Input records (`sceMouseRead`, `sceKeyboardReadState`) | two requests that are not mine |

Nothing on that list is waiting on more tooling. Eight of the day's records are instruments that
could only say one of the two things they appeared to say; those are fixed, and what they now say
is the list above.

## Next

- Nothing local that moves a wall. The loop continues to poll, and picks up whichever of the above
  unblocks first.
