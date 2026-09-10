# 492. orbistoun was calling itself a payload

**2026-09-10** - loop, continuing 491

Set out to work on the eight conformance checks where orbistoun passes and hardware fails.
Checked the other hardware leg first: **seven of the eight pass on the package leg.** They
were never divergences. orbistoun was answering like the title it is, and I had been
comparing against a payload for days.

## Why the comparison went to the wrong leg

Because orbistoun reported itself as a payload. obSCEne's `obs_run_context` decides delivery
on `obs_libkernel_base() != 0`, and the **title** bootstrap sets that value - it resolves
`getpid` and derives the base from it. On a console the lookup fails, so the classification
comes out right by luck. orbistoun resolved `getpid`, so it announced itself a payload, and
every section branching on `f.is_payload` took the payload path.

Filed to obSCEne as a fragile discriminator (`REQ-20260910T0410Z-e5d9`). The reason orbistoun
tripped it is orbistoun's own, and that is the change here.

## The console does not resolve platform names for a title

Measured, package leg of sweep 20260909-234847, three ways: `060-module/dlsym-resolves-known-symbol`
**fails `0x8002_0003`** for a known symbol from a valid handle; every `dlsym` measurement in
that leg reads `0x0`; and `005-generation/detect` says in prose that this platform does not
resolve modules by name.

`orbistoun-core::route` is the new axis - `Title` or `Payload`, decided by `sce_sys/param.json`
(or `param.sfo`) beside the module, which is the platform's own discriminator and splits the
corpus exactly. A title's `sceKernelDlsym` no longer consults the stub table and answers the
measured `ESRCH`; a payload's still does, and has to (D365). D669.

## Surprises

**Refusing it changed no title's reach at all.** Around 280,000 calls a sweep now refuse where
they used to answer - `sceKernelDlsym` fell from 26.6% of all calls to 20.8% - and every guest
in the corpus lands on the identical import count, call count and fault address. PPSA02664
199/181 at `image+0x39f7c`, PPSA25872 151/145 at `image+0x17554a3`, and so on down. Titles bind
their imports and *probe* by name; nothing was leaning on the answer. A change that removes a
capability and moves nothing is a capability nothing was using, which is what hardware already
said.

**A report is as capable of plausible output as a stub is.** Thirteen divergences on the list,
at most six real, and the error was in my comparison rather than in any code. The fix for it
was to check which leg before working on any of them - which cost one command.

**D629 was right and far too small.** It narrowed `dlsym` for one handle and one name, from one
measurement of `memcpy`, on the reading that the platform was not consulting the module. The
platform is not resolving at all.

**A test failing removed a branch that could never fire.** `route_for` had a `None` arm for a
module path with no parent, calling it a payload. `Path::new("eboot.bin").parent()` is
`Some("")`, not `None` - the empty path meaning the working directory - so the arm was dead and
the behaviour it described was wrong anyway. The test written for it went red, which is the
only reason it is not still sitting there answering for a case that does not occur.

**The category axis was built and never wired.** `orbistoun_core::category::present` has no
caller anywhere, so `presented()` has always answered the default. The mechanism from D662 is
sound and nothing declares into it. Not fixed here - noted, because it is the same shape of gap
the twelve orphaned knowledge files were (D668) and it will not announce itself.

## Where the comparison stands now

Against the leg orbistoun should be measured on: hardware 174 passes, orbistoun 170, with 37
checks both ran disagreeing and **13 where hardware passes and orbistoun does not**. Most are
`partial` or `skip` for capability orbistoun genuinely lacks.

## Next

- `100-input/oops-sdk-poll` and `900-surface/control` are the two outright failures where
  hardware passes.
- `111-modlink/walk` is the other half of the context record: the generation is read by walking
  the link map from `DT_DEBUG`, which orbistoun does not expose, so it still reports
  `unknown-gpu`. orbistoun has the module list already - what is missing is presenting it as an
  `r_debug` chain in guest memory.
- Wire `category::present` from the loader, so D662's mechanism is reachable.
- The two shell surfaces and the corpus origin-list fetch path are still unbuilt.
