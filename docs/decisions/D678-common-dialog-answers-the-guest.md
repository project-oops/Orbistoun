# D678 - common-dialog answers the guest-observed 0 once measurement is proven impossible

**Status:** decided
**Date:** 2026-09-12

## The choice

`sceCommonDialogInitialize` is now implemented to return `0` (`crates/orbistoun-systemservice/src/
common_dialog.rs`), where D670 had deliberately left it a visible refusal and used a policy override
to explore past it. The trigger for the change is that measurement is now closed off, not that the
evidence got stronger on its own.

## Why now, and why it is honest

- **The value cannot be measured.** obSCEne's a3f7 resolved that `libSceCommonDialog` is not linked
  into a title process by default: the symbol is `absent|shared` and unreachable without
  `sceSysmoduleLoadModule(SCE_SYSMODULE_MESSAGE_DIALOG)` first. So the return can never be read on
  hardware in the context that matters. The measurement path D670 was holding for does not exist.
- **The evidence that remains is strong for guest-observed.** Three retail Unity titles (PPSA02664,
  PPSA03416, PPSA25872) call it early and check the sign of the return; all three ship and run on a
  console, so it succeeds there. Answering `0` carries PPSA02664 from a 14,989-call exit to 418,310
  calls of coherent engine setup - the guest names its own work in debug markers ("Clear", "Disable
  MSAA/EQAA") - which is the second, different observation D227 asks of an intervention.
- **It is one value, not five.** Unlike the Earthion mapper gate (D677), the guest consumes only the
  sign of this return - there is no out-structure it reads - so `0` cannot hide unmeasured fields the
  way a faked mapper fill would. This is the same shape as `sceSysmoduleLoadModule`, which already
  answers `0` on guest-observed/reasoned grounds as shipped behaviour.

`known_by` stays `guest-observed`, which is the honest tier: the guest tells us `0` proceeds and three
shipping titles corroborate it, but no instrument read it. That is a real provenance, not `measured`
dressed up, and not the `assumed` it would be without the corroboration.

## Consequence

PPSA02664 advances past its first wall to the AGC/GPU command-buffer layer (worklog 517), which is the
same Phase-6 wall Earthion reaches from the other side - so this unifies the retail-graphics frontier
on the GPU engine rather than dispersing it. The frontier entry for these titles now rests on a
guest-observed return, which is comparable to any other guest-observed implementation and is not an
`experiment` (that tag is for runtime overrides, D181/D557).
