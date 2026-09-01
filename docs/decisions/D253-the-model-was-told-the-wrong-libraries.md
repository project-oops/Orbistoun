# D253 - The model was told the wrong libraries, and earned names from ones it was never shown


**decided** · 2026-08-25 · found by checking where the earned names actually came from

The prompt names the libraries the identifiers belong to, and the live run supplied four
hardcoded strings: `libkernel`, `libSceNpManager`, `libSceAudioOut`, `libSceVideoOut`.

Every name the model has ever earned came from a library that is **not** in that list:

| earned | library | shown? |
|---|---|---|
| `sceAgcDriverRegisterResource` | `libSceAgcDriver` | no |
| `sceNpAuthPollAsync` | `libSceNpAuth` | no - `NpManager` is a different library |
| `sceAudio3dObjectReserve` | `libSceAudio3d` | no - `AudioOut` is a different library |

It was succeeding *despite* its context. A model told the wrong domain is being pointed
away from the answer, and the one thing this model is measurably good at - vendor domain
nouns - is exactly the thing that clusters by subsystem.

Derived instead from the round's own examples. A vendor name is `sce` + a module word +
the rest, and the module words are a list the grammar already carries, so the segmentation
is read rather than guessed. 80% of the example set resolves, across 47 distinct libraries,
and the three names above all resolve correctly.

The examples already rotate per round, so the libraries rotate with them: each round now
names the subsystems whose examples it is looking at, and the question narrows on its own
without a separate per-library mechanism.

