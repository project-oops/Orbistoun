# orbistoun-posix

The POSIX-named half of the platform.

It implements nothing of its own. Every function it serves is the POSIX spelling of a call
[orbistoun-kernel](../orbistoun-kernel/), [orbistoun-fs](../orbistoun-fs/) or
[orbistoun-libc](../orbistoun-libc/) already implements, and both spellings resolve to the
same function pointer. A POSIX name with no vendor-named twin is declared and left unserved,
so a trace can name it.

## Two names, one behaviour

A title imports `pthread_create` from `libScePosix` and `scePthreadCreate` from `libkernel`.
They are two names for one behaviour, but a NID is the hash of a name, so each spelling needs
its own registry entry or it resolves to nothing.

That the two spellings share a behaviour is inferred from the names and from both being
exported by one platform, not measured, and every knowledge entry for them says so.

## Return convention

POSIX answers `0` or an errno; the vendor-named calls answer their own codes. The success
paths coincide and the failure paths do not.

**Rule:** nothing here invents an errno. A failure returns the project's placeholder code,
from a range no real firmware value occupies (D670). A guest testing `!= 0` behaves
correctly; one switching on specific errno values falls to its default branch rather than
matching the wrong case. Real errno values come from FreeBSD's headers, a citable source.
