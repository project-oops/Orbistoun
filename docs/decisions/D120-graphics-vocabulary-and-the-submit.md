# D120 - Graphics vocabulary, and the submit function

**decided** · 2026-08-19

The generator already had `Agc` as a module and `Submit` as a verb, and had still found
almost nothing in the graphics libraries. The missing ingredient was **domain nouns** -
`Dcb`, `Ccb`, `CommandBuffer`, `Flip`, `Eop`, `DrawIndex` - which no generic word list
produces and no amount of generic extension would have reached.

The search space went from 251 million to **2.4 billion**, searched in 94 seconds. Names
for the 96 MB executable went from 11.6% to 18.2%, and across four titles the database
now names between 20% and 36% of imports.

**The result the GPU thread was blocked on:**

```
0xc3b2c69821490952  libSceAgcDriver  sceAgcDriverSubmitDcb
0xd4f245bfaf672481  libSceAgcDriver  sceAgcDriverSubmitAcb
```

Plus a working command-stream vocabulary - `sceAgcDcbDrawIndex`, `sceAgcDcbDrawIndirect`,
`sceAgcDcbSetIndexBuffer`, `sceAgcAcbDispatchIndirect`, `sceAgcCreateShader`, and more.

Two things are worth carrying forward. **D117 made this possible**: without correct
library attribution the search space was all 1,410 imports rather than the 260 in the
graphics libraries, and there would have been no way to tell a graphics name from a lucky
collision elsewhere. And **the cost has grown** - a search is now 94 seconds per module
rather than nine, which is what makes the staleness check in `run` load-bearing rather
than a nicety.

**A bug the merge introduced and the count caught.** The revision-mark list was written on
one line, so a merge that parsed line-by-line saw zero existing entries and replaced them.
The empty string went with them, which would have made every generated name carry a
suffix. Noticed because the reported count went *down*.

