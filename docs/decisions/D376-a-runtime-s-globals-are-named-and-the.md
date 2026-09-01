# D376 - A runtime's globals are named, and the last wall is a syscall gadget


**decided** - 2026-08-29

D375 left the runtime handshake as the only thing between orbistoun and `klogsrv`. This is
what the payloads' own symbol tables say about it, and where it ends.

### The globals have names, and the first of them is the answer to a three-session question

`klogsrv`'s `.bss` holds fifty-nine named objects. The first is **`payload_args`** - the
global the startup code stores the handoff structure in - and it is there in `ftpsrv`, in
`shsrv`, and in the probe built here, so it is the runtime's and not any payload's.

The rest are a table of C library pointers: `vsnprintf`, `snprintf`, `strerror`, `__error`,
four separate slots called `strcpy`, five called `calloc`. That is what the startup code does
- resolve the library by name, one name at a time, and store each answer in a global.

Those names live in `.symtab`, which is a **section**. This project had never read one:
loading needs program headers, sections are a link-time view, and a loader is entitled to
ignore them. It ignored them for a year, correctly, until this question.

### Filling them is part of the diagnostic, not a claim

`[entry] at` already declares a run *not an ordinary one* - it starts a guest past its own
startup code. What that produced was a program whose library pointers were all null, dying on
the first call through one, which measures the skipping rather than the program.

So when and only when that setting is in use, the loader now performs the same resolution the
skipped code would have performed: by name, from the program's own symbol table, answering
with the same stub an import of that name resolves to. It is **not** what the platform does -
the platform runs the startup code - and every fill is reported, because a run that quietly
initialised a guest differently from how it says it did is worth nothing.

Thirty-four of `klogsrv`'s fifty-nine were served this way, and it went straight past the wall
D359 and D360 spent a session on: it now reaches `klog_printf`, calls `vsnprintf`, and renders
its message.

### And a global nothing implements says its own name

The twenty-five left over keep a marker rather than the null they would otherwise have. Null
is what the guest would have had anyway and says nothing when it is used; a marker makes the
next wall name itself. The difference, on the first run:

```text
before   instruction fetch from 0x0
after    instruction fetch from 0x5e2900002000, which is ptr_syscall
```

### What `ptr_syscall` means

**The payloads do not reach the kernel only through named imports.** They keep a pointer to a
raw syscall gadget and call it directly - `klog_printf` renders its message with `vsnprintf`
and then issues a syscall to deliver it.

So the last wall for `klogsrv` is a subsystem with a name: orbistoun has to *be* the kernel at
the syscall boundary, not only at the library one. That is a bounded piece of work rather than
a question - the numbers are in `sys/sys/syscall.h` in the same checkout the constants are
harvested from, the calling convention is FreeBSD's, and the implementations they map onto are
already written.

Everything else left over is what a loader supplies on real hardware and orbistoun has no
equivalent of: `KERNEL_ADDRESS_TEXT_BASE` and eleven of its relatives, `pipe_addr`,
`proc_cache`. A kernel-log server wants kernel addresses, and there is no kernel here to have
them.

