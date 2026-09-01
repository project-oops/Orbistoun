# 2026-08-31 (later) - per-title device sandbox, from the overlay already there (D422)


The current obSCEne eboot (rebuilt from source; the corpus copy was a stale Aug-25 build with 27
sections against hardware's 37) crashed in orbistoun's report sink. Root cause was not the sink but
two unimplemented calls answering placeholders: sceKernelMkdir and sceKernelDebugOutText. Added
both to orbistoun-fs - mkdir creates under a writable mount and refuses elsewhere with 0x8002_00xx
(never the 0x7fff placeholder a caller misreads), DebugOutText forwards the guest's log line to the
captured host stderr (the probe writes its whole report there as an unconditional second channel).

The sandbox itself needed no new subsystem: D250/D251's overlay already materialises a title's
writable data as a layer over the filesystem.toml base tree. So /mnt/usb0, /mnt/usb1 and /download0
are three new manifest entries (guest-observed - the probe's sink opens them; the module report
header names /download0), and install() creates+mounts+layers them per title automatically.

Retention is ORBISTOUN_SANDBOX: default retain (writes persist - saves, reports), ephemeral empties
the title overlay at run *start* (a process guest never reaches a teardown). Archive-when-idle /
extract-on-demand noted as the next optimisation. env + kernel/core/shell tests pass; fs unit tests
added (mkdir creates-vs-refuses-without-placeholder; DebugOutText success-vs-null).

Still blocked for the whole eboot: it also faults reading its own PT_SCE_DYNLIBDATA vendor segment
(vaddr 0, memsz 0 - nothing mapped). That is a loader question (B), tracked next; the payload shape
(C), which streams to klog now that DebugOutText is captured, is the lower-risk path to try first.

