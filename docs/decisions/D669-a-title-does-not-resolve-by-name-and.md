# D669 - A title does not resolve by name, and orbistoun was calling itself a payload

**Status:** decided
**Date:** 2026-09-10

## The measurement I had been reading wrong

Thirteen conformance checks were on the list as divergences between orbistoun and hardware,
eight of them "orbistoun passes, hardware fails". Checking the other hardware leg before
working on any of them: **seven of the eight pass on the package leg.** orbistoun was not
wrong about direct memory, virtual query or the memory map. It was answering like the title
it is, and the comparison was against a payload.

The comparison went to the payload leg because orbistoun *reported itself as one*. obSCEne
writes an `OBS|context` record naming the run's delivery and generation, and orbistoun's said
`payload/unknown-gpu`. So the whole divergence list was built against the wrong expectations,
and I had reported thirteen where at most six were real. **A report is as capable of plausible
output as a stub is**, and this one was a measurement of my own methodology.

## What made it say payload

obSCEne's `obs_run_context` decides delivery on one thing: `obs_libkernel_base() != 0`. That
value is set in *two* places, and one of them is the **title** bootstrap -
`obs_bootstrap_title_output` opens libkernel, resolves `getpid`, and derives the base from it.
On hardware in a title that lookup fails, so the value stays zero and the classification comes
out right by luck rather than by construction.

orbistoun resolved `getpid`. So the base came out non-zero and orbistoun announced itself a
payload.

That is a fragile discriminator and it is obSCEne's to fix - filed rather than worked around.
But the reason orbistoun tripped it is orbistoun's own.

## The console does not resolve platform names for a title

Measured, on the package leg of sweep 20260909-234847, and not ambiguously:

- `060-module/dlsym-resolves-known-symbol` - a **known** symbol, a **valid** handle - **fails
  `0x8002_0003`**.
- Every `dlsym` measurement in that leg reads `0x0`: the fourteen video, pad and net names
  three separate sections ask for.
- `005-generation/detect` says it in prose: *this platform does not resolve modules by name*.

orbistoun resolved every platform name on both routes. D629 had already narrowed one corner of
this - libkernel's handle, for a name libkernel does not export - from a single measurement of
`memcpy`. The narrowing was right and far too small: the platform is not consulting the module,
it is not resolving at all.

## Route is an axis, and it is not the category

A **payload** is mapped and jumped to by a homebrew loader; a **title** is installed and
launched by the platform's own. The executable can be byte-identical - SELFish's pipeline wraps
one payload four ways - so nothing inside the file answers this. It is a property of how the
process was started.

`orbistoun-core::route` is the axis. It sits beside `category` (D662) and answers a different
question: the category is what a title *declared about itself* in `param.json` and decides
direct memory and the scanout; the route is not declared anywhere, and the only thing that
knows it is orbistoun, because orbistoun is the loader. The two are independent - a big app
delivered as a payload is an ordinary thing to build.

**The discriminator is the platform's own.** SELFish: *what makes a title native is
`param.json` and native registration, not the magic*. So `sce_sys/param.json` beside the
module, or `sce_sys/param.sfo` for a package - a package installs *as* a title, and the
compatibility sandbox is still a launched title. It splits the corpus exactly: all seven
titles carry one, `obscene-payload` carries an `eboot.bin` and nothing else.

The decision takes the filesystem predicate as an argument, so it is checkable without a disk -
the shape `orbistoun-mem` uses for address-space rules.

## Refusing, but only as far as the measurement goes

Scoped to the **stub table**: the platform declining to hand out its own functions is what was
measured. A name the guest's own binary exports is a different question - PPSA02664 asks for
its own `scriptingGetMem` through this call (D517) and nothing has asked a title for one - so
that path is untouched rather than refused on the strength of an adjacent finding.

And the refusal is `0x8002_0003`, not `0x7FFF_0001`. The placeholder means *nothing here
implements this*, which is orbistoun talking about itself; here the function exists and the
platform has a rule. Handing back a placeholder would report an emulator gap where there is a
platform behaviour. The work-list line says so too - three verdicts, not two, because
otherwise every platform name a title asks for lands on a list of things already done.

## The negative result, which is the load-bearing one

**Refusing it changed no title's reach at all.** Every guest in the corpus, before and after:
identical imports, identical calls, identical fault addresses - PPSA02664 199/181 at
`image+0x39f7c`, PPSA25872 151/145 at `image+0x17554a3`, PPSA21564, PPSA04263, PPSA28061, all
unmoved. `obscene-payload` unmoved, which is the half the payload route protects.

That is the strong evidence. Around 280,000 calls per sweep now refuse where they used to
answer - `sceKernelDlsym` went from 26.6% of all calls to 20.8% - and not one guest noticed.
Titles bind their imports; they *probe* by name and carry on. A change that removes a capability
and moves nothing is the shape of a capability nothing was using, which is what hardware
already said.

## What it bought

obSCEne now reports `title/unknown-gpu` - *title eboot* - so the conformance comparison finally
goes against the leg orbistoun should be measured on:

| | hardware, title leg | orbistoun |
|---|---|---|
| pass | 174 | 170 |
| checks both ran, disagreeing | | **37** |
| hardware passes, orbistoun does not | | **13** |

Most of those thirteen are `partial` or `skip` for capability orbistoun genuinely lacks - the
visual flip, the link-map walk, the two GNM dispatches. Two are outright failures worth
chasing: `100-input/oops-sdk-poll` and `900-surface/control`.

The `unknown-gpu` half is still wrong and is the same root: the generation is read by walking
the link map from `DT_DEBUG`, which orbistoun does not expose. That is `111-modlink/walk`, and
it is the next piece rather than this one.
