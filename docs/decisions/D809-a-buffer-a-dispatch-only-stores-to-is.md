# D809 - a buffer a dispatch only stores to is not read

**Status:** decided
**Date:** 2026-10-12

A compute dispatch's own buffer (D746) that every traced access to is a store - the program writes
it and never reads it - is not read from guest memory before the run. The device starts it at zero,
with nothing uploaded. Beside it, the module marks a write mask: a word for each of the buffer's
words, set to one where a store lands, plus one word past them that a store which did not land
marks instead. Neither a store outside the buffer nor one in a lane that is not running sets a
word's mark.

The host writes back exactly the words the mask marks, and nothing else. The device's zeros in
words the program did not store never reach guest memory. A buffer stored whole with one word is a
fill (D804), as before. A buffer the program also reads is handed over and written back as before,
by comparing its words.

Each of the dispatch's own slots has its buffer and its mask; the rest of the bound array binds
slot zero's, which the module never reaches. A read buffer has a mask too, though the module marks
only the slots its trace found stored to, so a mark always lands in a mask.

The mask is marked with a plain store of one, not an atomic: every invocation runs a whole wave's
lanes in turn, so its stores are many and one atomic each serialised them. Sending a store that
did not land to the word past the buffer keeps it from touching another word's mark, without a
read.

**Why:** PPSA28061 clears its 3840x2160 display buffer with a dispatch every frame. The dispatch
read 33 MB of guest memory first, which carried out the previous frame's deferred write-back
(D719), and uploaded it. That read is what the program never looked at. Its dispatch took about
78 ms and now takes 10-40 ms. The title runs 5491 submissions in 120 s on the host clock, where it
ran 4986, and draws its emulated game's title screen. SuperTuxKart, whose many small copies pay for
a mask and gain little from the skipped read, runs about 4% slower in its menu loop.

**Rejected:**
- A mask of one bit a word, ORed atomically: 32 lanes contend for each mask word, and the dispatch
  took 260 ms.
- One word a word, ORed atomically: no contention, and still 260 ms; the atomics themselves are
  the cost when a wave's lanes run in turn.
- A mask only for the buffers the host left unread: the module decides which slots it marks from
  its own trace, and a slot the host read with no mask of its own would take the marks in its
  buffer.
- Keeping the dispatch's buffers resident on the device between dispatches: worth it only once a
  later reader can take them from there, which the draws that follow cannot yet.
