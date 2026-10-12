# D804 - a dispatch that fills a buffer writes it as a fill

**Status:** decided
**Date:** 2026-10-12

A compute dispatch's own buffer that the run changed and left holding one word throughout is
written back as a fill of the buffer's whole range: the command processor's fill path, not the
changed words. Memory ends holding exactly what the changed words would have written, since every
word of the buffer is that word. A fill of a whole page-aligned range is deferred (D719), and a
copy it covers is dropped unread. A later reader of the range, such as the next draw over a colour
target there, is answered with the word rather than reading the bytes back.

Buffers left holding more than one word write their changed runs as before (D746). A buffer the
run did not change writes nothing.

**Why:** PPSA28061 clears its 3840x2160 display buffer with a compute dispatch every frame. Each
clear wrote 31 MB of zeros, and the draws after it then read and detiled that zero-filled target.
As a fill it runs 4396 submissions in 120 s on the host clock where it ran 3941, and reaches the
emulated game's title screen.

**Rejected:**
- Recognising the clear before running it, from the program alone: that needs the stored value
  and the threads' coverage of the buffer read off the program, which no part of the translator
  proves today. Running it and looking at the result is exact.
