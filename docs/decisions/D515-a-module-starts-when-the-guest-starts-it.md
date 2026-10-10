# D515 - A module starts when the guest starts it

**Status:** decided
**Date:** 2026-09-03

A placed module's `DT_INIT` and `DT_INIT_ARRAY` run only when the guest calls
`sceKernelLoadStartModule` for it. The loader records the initialiser array's runtime address and
length at placement, and the function pointers in it are read at start time, after relocation.

**Why:** whether the platform runs an import-bound module's initialisers at load time is
unmeasured, and starting every module to make a title progress would be inventing behaviour.
Running every placed module's initialisers before entry was tried and breaks a run, because
orbistoun's own libc is not usable at that point. Reading the pointers late means they are the
relocated values by construction.

**Amended 2026-10-10:** a module the title ships that nothing imports - a plugin it loads by
path, such as a Unity title's `Media/Plugins/` - is placed with the others but not linked against
by the title's own modules, since it is not loaded when they are linked; plugins bind to one
another, as a plugin loads with what it needs. A second start of the same path answers its first
handle and runs nothing. `DT_INIT` is handed the start's `(args, argp)` - guest-observed: a Unity
plugin's init entry reads the block at `argp` first - and each init-array entry the program's
`argc`, `argv` and `environ`, as FreeBSD's runtime linker calls one (`rtld.c`,
`call_init_pointer`).

**Rejected:**
- Handing back a handle and running nothing: indistinguishable from a started module, and a guest reads null globals later.
- Starting every placed module before the entry point: unmeasured on hardware and destroys runs here.
- Reading the array contents at placement: records pointers before relocation has written them.
