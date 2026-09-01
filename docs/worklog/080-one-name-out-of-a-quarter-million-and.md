# One name out of a quarter million, and sixteen useful refusals


Swept 249,506 generated candidates against the seventeen hashes still unnamed in
PPSA28061's trace. One confirmed: `sceVideoOutRegisterBuffers2` (D167).

Worth having, because that call was observed taking `0x7FFF0001` as its first argument -
the guest handing our own unimplemented code back as a video-output handle, having got it
from `sceVideoOutOpen`. The name turns an anonymous bad-handle call into a sentence.
`Buffers` went into the vocabulary so the repository derives it rather than relying on this
session having found it.

The sixteen misses are the more useful half. The sweep covered every verb-object and
object-verb combination across every module observed, so those names are **not** built from
the vocabulary this project has - which says extending the word lists further is not how to
reach them. The most-called one, `libc::0xa75420e43cad1cdc` at 76 calls, sits between
allocate and map with a stack address as its argument: the shape of something formatting
the name string the map call takes. A printf-family name, and none of the obvious spellings
hashed.

The `sceKernel<posix-name>` pattern is the outstanding idea for naming - deriving from the
FreeBSD list already harvested rather than from invented words. Solver change, not a data
edit.

