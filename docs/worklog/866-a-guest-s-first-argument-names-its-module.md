# 866. A guest's first argument names its module under /app0, not the host path

**2026-09-25**. PPSA03416 prints its argument vector, and it printed
`Arg 0 = /app0/<library>\PPSA03416-app0/eboot.bin`: the whole host path, with `/app0/` in front.
`write_process_image` formatted `/app0/{module}` from the host path it was given. Its own comment
said it meant only the module's name.

The fix is `guest_argument_zero`: the file name alone, split on either separator, under
`APP_MOUNT`. Test: `argument_zero_is_the_module_under_app0`, watched failing with the old
formatting. PPSA03416 now prints `Arg 0 = /app0/eboot.bin`.

Its wall is unchanged (`image+0x3f8f0`, a write to `0x94`). That wall is still Unity's
workload-array element buffer (worklogs 789-791), waiting on the CreateWorkload trace (REQ-...7a5d).
