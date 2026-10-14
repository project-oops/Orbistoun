# D875 - A module travels with the first drawn submission that binds it

**Status:** decided
**Date:** 2026-10-14
**known_by:** published

A translated shader module reaches the backend with the first submission that binds it and is
drawn, not with the preparation that made it. Every module the live pipeline makes is kept, with
the pipeline still held, until a submission carrying it draws. As each submission or segment is
carried out, every module its `BindShader` commands name that is still kept travels with it. A
module whose draws were refused stays kept for the next submission that binds it.

**Why:** the pipeline is shared by the guest's thread and the command processor's (D821). Carried
out asynchronously, the guest prepares submissions ahead of the one executing, and a segment
re-prepared on the command processor's thread (D729) finds a module one of those later submissions
made already translated. It binds it while the module's words wait in the queue behind it. In
SuperTuxKart, modules 0x10 and 0x11 were made on the guest's thread for a submission still queued,
and the segment of the one before it bound them: the backend refused the unknown resource, the
queue stalled (D710), and the title ran at 0.9 fps against 14.3 carried out synchronously. It now
runs at 34.8 fps asynchronously (`ab` against the commit before, three rounds of 45 s).

The earlier rule carried a submission's modules forward only when none of its draws ran, and only
into the next submission prepared, which under async was already prepared.
