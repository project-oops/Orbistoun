# 2026-08-29 - What about klogsrv


The answer to "so what about the real payloads" turned out to be in their own symbol tables.

**`payload_args`** is the first named object in `klogsrv`'s `.bss`, and it is in `ftpsrv`, in
`shsrv`, and in the probe built here - so it is the runtime's global for the handoff
structure, named by the thing that uses it. Three sessions of calling it "the structure the
entry point wants" and it had a name all along, in a section nothing here read.

Sections are a link-time view and a loader is entitled to ignore them. This one did, for a
year, correctly - until the question was *which named globals does this program have*.

### Entering past the runtime now does what the runtime would have done

Thirty-four of `klogsrv`'s fifty-nine globals resolve by name to the same stub an import of
that name resolves to. Bounded to `[entry] at`, which already declares a run not an ordinary
one, and every fill reported.

It goes straight past the wall D359 and D360 spent a session on. Banner, `getopt`, `sysctl`,
into `klog_printf`, `vsnprintf` renders the message.

### And then it calls ptr_syscall

The twenty-five globals nothing implements keep a marker instead of a null, so the next wall
names itself - and it did, on the first run:

```text
before   instruction fetch from 0x0
after    instruction fetch from 0x5e2900002000, which is ptr_syscall
```

**The payloads do not reach the kernel only through named imports.** They keep a raw syscall
gadget and call it. So the last wall for `klogsrv` is a subsystem rather than a mystery:
orbistoun has to be the kernel at the syscall boundary as well as the library one. The numbers
are in `sys/sys/syscall.h` in the same checkout, the convention is FreeBSD's, and the
implementations they map onto are written already.

