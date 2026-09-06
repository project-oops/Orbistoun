# D505 - The declared surface now matches what guests ask for, and the coverage figure got worse

**decided** - 2026-09-03

Twenty-eight libraries declared, 212 names, none implemented. The point is not the names; it is
that **the denominator was wrong**.

```text
declared / implemented   674 / 647  (96%)  ->  947 / 647  (68%)
unresolved imports             342         ->        117
```

## The 96% was measuring the wrong set

D500 said coverage is not capability. This is why: orbistoun declared fifteen libraries, and
PPSA02664's executable imports from **thirty-five**. The declared surface was very nearly *what
had already been implemented*, so the ratio could not do anything but look finished.

Now it is 68%, and **the drop is the figure starting to mean something.** 947 - 273 = 674, the
old declared total exactly: nothing was removed, and everything added is a name a guest asks
for and nothing answers.

`orbistoun-cli status` reports the distinction rather than leaving it to be rediscovered:

```text
| Functions declared / implemented          | 947 / 647 |
| Declared in a library that serves nothing | 273 across 30 libraries - names written down, no implementation |
```

Counted by library, not by symbol, because a library with one implementation is being worked on
and a library with none has only had its names written down.

## What was declared, and where it went

Homes were chosen so an implementation would not have to move later:

| crate | libraries |
|---|---|
| `orbistoun-audio` | `libSceAjm`, `libSceAudioOut2`, `libSceAudio3d`, `libSceAudioIn` |
| `orbistoun-video` | `libSceAvPlayer`, `libSceVencCore`, `libSceVideoRecording` |
| `orbistoun-input` | `libSceKeyboard`, `libSceMouse`, `libSceIme`, `libSceImeDialog` |
| `orbistoun-net` (new) | `libSceHttp2`, `libSceNet`, `libSceSsl`, `libSceNpWebApi2`, `libSceNpManager`, `libSceHttp`, `libSceNetCtl` |
| `orbistoun-systemservice` | `libSceAppContent`, `libSceSaveData_native`, `libSceMsgDialog.native`, `libSceWebBrowserDialog`, `libSceRemoteplay`, `libSceCommonDialog`, `libSceErrorDialog`, `libSceCoredump`, `libSceJson2` |
| `orbistoun-gpu` | `libSceAmpr` |

One new crate, because networking had no home and putting it in one that fit badly would have
to be undone. `libSceAmpr` is placed by name association with the graphics submission path and
says so - a placement worth revisiting rather than a claim.

## Provenance, per library, because that is the whole licence to do this

Every name is read out of **a real module's import table** or measured resolving on hardware -
the provenance `orbistoun-input` documents for `libScePad`, and the strongest available without
a console. Each module's header names which: PPSA02664's own imports, other modules in the
recorded corpus, or obSCEne's `106-encoder` checks (all `unresolved = 0x0`, so those symbols
demonstrably exist).

**The `<unknown>` hashes in those same libraries are not declared.** Six of them sit in
`libSceAgc` alone. A hash is not a name, and declaring one would put a NID in the table with
nothing to say about it.

Arities are `6` throughout - the trampoline's full capture, and not a claim about argument
counts. D504 has the argument; the short form is that arity reaches reports and trace shape and
never the call path, so the honest choice is the one that discards no information.

## What it did not do, measured rather than assumed

**It did not move the wall, and it did not fix the oscillation.** Twelve runs after: nine at
2080 calls / 46 distinct, three at 2077 / 44 - the same bimodal pair and the same ratio D499
recorded. The first four samples came up 2080 four times, which is exactly what a claim of "the
non-determinism is gone" would have rested on; four is not enough to separate that from a coin
weighted 60/40, and the next eight settled it.

`standing` is unchanged at 2078 of 2080 answered by an implementation. The guest reaches none
of these libraries before it dies - which principle 6 predicts, and which is why every one of
the twenty-eight is in `SERVES_NOTHING` rather than half-implemented.
