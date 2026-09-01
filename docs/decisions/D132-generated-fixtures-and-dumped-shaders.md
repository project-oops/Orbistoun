# D132 - Generated fixtures and dumped shaders do not share an extension


**Status:** assumed

Shader fixtures generated here are `.gcn`; a shader dumped out of a title stays `.bin`.

The two carry **opposite obligations**. A dumped shader is console-derived and must never
be tracked - the provenance guard bans that extension outright and that ban is correct. A
generated fixture must be tracked, so the differential test runs on a machine with no
LLVM.

They shared an extension until the provenance guard rejected the fixtures, which was the
guard being right rather than over-broad. The alternative was to add a path exception to
the guard in all three places it lives; that would have weakened a principle-1 mechanism
to accommodate a naming collision, and left a directory where a real dump could later be
committed unnoticed.

Splitting the extension keeps the guard exactly as strict and encodes the distinction
where it belongs. `corpus::is_shader` accepts both, because the difference is provenance
rather than content - they decode identically.

