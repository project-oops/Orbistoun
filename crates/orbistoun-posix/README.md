# orbistoun-posix

The POSIX-named half of the platform.

**Models:** nothing of its own. Every function it serves is the POSIX spelling of a call
another crate already implements, and both resolve to the **same function pointer**.

**Deliberately fakes:** nothing. It either delegates or leaves a name declared and unserved.

## Why a library of aliases is worth a crate

A title imports `pthread_create` from `libScePosix` and `scePthreadCreate` from `libkernel`.
They are two names for one behaviour - and a NID is the hash of a name, so the POSIX spelling
resolved to nothing while its twin worked. Forty-nine names were being asked for and answered
by nobody (D349).

Twenty-four now delegate. The rest are declared so a trace can name them; most are sockets,
which belong to a library this project does not model at all.

## The return convention, which is the one real cost

POSIX answers `0` or an errno. The vendor-named calls answer their own codes. **The success
paths coincide and the failure paths do not.**

Nothing here invents an errno. A failure returns this project's placeholder, which avoids the
high bit precisely so it can never be mistaken for an established value. A guest testing
`!= 0` behaves correctly; one switching on specific errno values falls to its default branch
rather than matching the wrong case - worse than a real errno, and much better than a
plausible guess. It improves the day somebody reads the values out of FreeBSD's headers,
which is a citable source.

## What it does not claim

That the two spellings are the same behaviour is **inferred from the names** and from both
being exported by one platform. It is not measured, and every knowledge entry says so.

Nothing has called one yet: the title that imports them reaches the vendor-named twins during
boot and stops before these. Served ahead of demand, and said so rather than letting a count
imply otherwise.
