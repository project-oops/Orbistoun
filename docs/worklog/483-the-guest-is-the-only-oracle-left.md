# 483. The guest is the only oracle left

**2026-09-09** - directed, continuing 482

## Four functions leave the queue

`REQ-20260909T2145Z-9e52` came back `not-possible`, terminally. `sceUserServiceGetLoginUserIdList`,
`sceAppContentInitialize`, `sceAppContentTemporaryDataMount2` and `sceCommonDialogInitialize` are
retail-title sysmodules that resolve in **no** obSCEne leg: the payload trips a kernel privilege
signal, the pkg leg is refused by the OS, and the eboot leg has them unlinked.

That is two walls now unmeasurable from any probe - these and `sceAgcCreateShader` - and both for
the same reason: no leg maps a native title's libraries. `REQ-20260909T1310Z-c8b5` with Prosperous
is now the common unblock, not a graphics-only ask (D659).

## So the guest becomes the instrument, which 482 just made possible

CLAUDE.md has always listed the guest as an oracle and D226 has always said why it was weak: an
intervention that moves a wall is not a diagnosis. With the guest logging its own boot, an
intervention gets a second observation of a different kind for free.

First result, on PPSA02664 - the menu candidate - forcing `sceAgcInit` (the name SELFish
attributed in D649) to `0`:

```text
unforced   198 imports   417,667 calls   image+0x39f7c   reaches GfxDevicePS5SharedData::CreateWorkload()
forced 0   153 imports   410,821 calls   image+0x3ac99   stops after /app0/Media
```

**Zero is worse than the placeholder.** Three unforced runs agree at 198 and the same fault site,
so that is the intervention and not variance. The guest checks this return - worth knowing before
anyone implements it, and not a licence to spray values, because without AGC's convention more
forcing is guessing with extra steps.

## Surprises

- **I concluded the override had not applied, and was wrong.** The forced run still listed the
  function as unimplemented, which I read as "not intervened" - but a forced return is answered at
  the stub, so that line stays correct. The report was printing `this run was under
  0x53bbd82b51d172db answers 0x0 (1 answered) … not recorded` the whole time. D224/226/227 built
  that line for exactly this moment and I checked a proxy instead of reading it.
- **PPSA02664 is stable to the call**, unlike PPSA25872 - three runs at 198 imports and 417,66x
  calls. The GC that makes PPSA25872 jitter has not started here.

## Next

- Prosperous `c8b5`: a leg where a native title's libraries map. It now unblocks two walls.
- PPSA02664 needs three AGC calls - `sceAgcDriverAddEqEvent`, `sceAgcDcbResetQueue`, and
  `sceAgcInit` - and nothing can measure any of them today.
