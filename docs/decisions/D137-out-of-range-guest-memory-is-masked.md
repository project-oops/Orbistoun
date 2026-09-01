# D137 - Out-of-range guest memory is masked, because undefined is not comparable


**Status:** assumed

`Model::word_index` masks the computed index to the memory window.

An index past the end of the array is **undefined behaviour** in the emitted module - not
a fault, not a wrapped read, undefined. Two models handed the same out-of-range address
are therefore not required to produce the same answer, and any comparison between them
there is comparing a coincidence rather than a computation. That only became visible when
the generated-program comparison started producing addresses the hand-written tests never
did.

Wrapping is still the wrong *semantics*. A guest address outside its mapping should be
refused, and `MEMORY_WORDS` already says so. It cannot be refused at translation because
the address is only known while the shader runs, so this becomes a base, a length and a
branch that reports when a real mapping replaces the window. Until then,
defined-and-wrong beats undefined: one is a bug that can be found, the other is a
comparison that means nothing.

