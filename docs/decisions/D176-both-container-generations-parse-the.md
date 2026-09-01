# D176 - Both container generations parse; the refusal was never tested


**decided** · 2026-08-21

Two of six local titles would not load: *"previous-generation wrapper; only the current
generation is parsed"*. A named, deliberate refusal - and nobody had checked whether it was
necessary.

Read with the current layout, a previous-generation header yields the **same** version,
mode, endianness, attributes, key type and header sizes, a plausible segment count, and
descriptors that parse. Side by side the two headers differ in four magic bytes and one
version byte:

```text
current   54 14 f5 ee | 00 01 01 12 | 01 01 00 00 | 60 05 10 06 | ...
previous  4f 15 3d 1d | 00 01 01 12 | 01 01 00 00 | 60 05 10 05 | ...
```

Accepting the magic was a four-line change. Both titles then parsed completely - same ELF
offset, same entry, fourteen program headers, vendor segments located - and **both executed
guest code**, one to 53 calls and one to 131.

The generation is *reported* rather than flattened away. The two parse the same, but a
title built for the previous console is a different emulation problem and a report that
cannot say which it read is hiding the most useful fact about it.

**The general point:** this was a refusal written from a reasonable assumption, documented
honestly, and never measured. It cost two titles - a third of the corpus - for as long as it
stood. Worth checking any other place where the code declines to try.

