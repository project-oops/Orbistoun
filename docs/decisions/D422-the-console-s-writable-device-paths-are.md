# D422 - The console's writable device paths are a per-title sandbox, in the overlay already built


**decided** - 2026-08-31 (user-directed)

Running the current obSCEne eboot in orbistoun crashed in its report sink: `sceKernelMkdir` was
unimplemented, so it answered the `0x7fff_0001` placeholder - a small positive number - and the
probe read it as a made directory, opened a report file that was therefore never there, and read
the sink back into a fault (the D125/D273 shape, one layer up). The instinct was to special-case the
sink; the right frame, from the user, is that **the console sandboxes these paths and the write
simply lands there** - so orbistoun should model the sandbox, not the crash.

And the sandbox is already built. D250/D251 made a title's writable data a *layer* over a read-only
base tree, materialised from the `filesystem.toml` knowledge file: for every `writable` entry,
`filesystem::install` creates the base directory, mounts it, and stacks a per-title overlay over it
with `allow_writes`. So the console's writable device paths are not a new subsystem - they are three
new manifest entries: `/mnt/usb0`, `/mnt/usb1`, `/download0`, each `known_by = "guest-observed"`
(obSCEne's sink opens them, and the module build's hardware report header names `/download0` as the
sink it used). `sceKernelMkdir` now creates under a writable mount and refuses elsewhere with the
console's `0x8002_00xx`, never a placeholder; `sceKernelDebugOutText` is implemented too, because the
probe writes its whole report there as an unconditional second channel and orbistoun can capture it.

**Retention is a setting, and its default is deliberately not the console's.** A real sandbox
presumably carries no state between launches; here what a guest writes **persists** by default,
because a proof of concept wants the saves and the reports a run produced to survive it.
`ORBISTOUN_SANDBOX=ephemeral` empties the title's overlay at the *start* of a run instead - at the
start, not a teardown, because a process guest is jumped to and leaves by calling exit, so nothing
after the entry reliably runs. Archiving an idle sandbox and extracting it on demand (into memory
when small) is the noted next step, an optimisation over always-on-disk, not a change to this shape.

This unblocks the sink but not the whole eboot: the current build also faults reading its own
`PT_SCE_DYNLIBDATA` vendor segment (vaddr 0, memsz 0, so nothing is mapped for it), which is a
loader question, tracked separately.

