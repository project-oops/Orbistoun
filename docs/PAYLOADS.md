# Running the open-toolchain payloads, and answering Prosperous

What it would take for `pros check` pointed at orbistoun to report the same five services a
jailbroken hardware reports, and for that to *mean* something.

Everything below is measured unless it says otherwise. The measurements are in D305, D306,
D307, D308 and D323.

## Why this is a target worth having

Every oracle orbistoun has, it grades itself with. `FURTHER` says a guest got past something
and says nothing at all once a run stops faulting (D301). The conformance probe is our probe
grading our emulator. A corpus of guests measures reach, and reach can be bought with a wrong
answer.

**Prosperous is none of those.** It is an independent tool, written against real hardware, by
somebody not trying to grade an emulator. It asks whether `pros titles` can list what is
installed and whether `pros backup` copies a save directory out without silently skipping a
file. No stub fakes that: either the bytes come back or they do not.

That is the property that makes the naming loop trustworthy - *"not because the generator is
clever, but because the oracle cannot be fooled"* - and this one already exists and costs
this project nothing to build.

## The bar is far lower than it looks

`pros check` does **one thing**: `TcpStream::connect_timeout(...).is_ok()`, plus how long it
took. No handshake, no protocol.

So a service reads as **up the moment the guest has a listening socket on its port.** For
`klogsrv` that means reaching its `listen()` call - not implementing anything about logs.

And for the deeper commands, note what orbistoun does *not* have to write:

| command | who implements the protocol |
|---|---|
| `pros klog` | klogsrv writes bytes to an accepted socket; orbistoun needs `accept` and `write` |
| `pros titles`, `pros backup` | **ftpsrv implements FTP.** orbistoun needs sockets and the file calls |
| `pros send` | elfldr, which needs a guest able to load a second guest |

**orbistoun never implements FTP.** Worth saying twice, because the surface looks enormous
until you notice the guest brings its own protocol.

## Where it stands

| stage | state |
|---|---|
| parse the container | works, all 23 payloads |
| read the import list | works, exact, `readelf` agrees on all five |
| name the imports | 24/24, 33/34, 39/41, 84/85, 159/160 |
| map and relocate | complete, zero unresolved slots |
| enter the guest | works |
| execute anything, entered at `main` | **yes** - klogsrv prints its banner, 9 imports, 18 calls |
| **the handoff structure** | field zero is a name resolver, measured (D365) |
| **run `__crt_start` at the declared entry** | **yes** - it resolves names and reaches `__kernel_init` (D366) |
| get past `__kernel_init` | **no** - it reads a handoff field nothing has established yet |

### What changed on 2026-08-29

The wall was "the entry point wants a structure nobody here can describe". It was answered
by asking the guest: a block of stubs that each report **which field was called and with
what** said, on the first run, that field zero is called with `(1, "sceKernelDlsym", out)` -
the string read out of the payload's own read-only data (D365).

So a payload does not get its C library from its import table. Its runtime asks for names
one at a time and stores them in its own `.bss`, which is why `klogsrv` carries `vsnprintf`,
`snprintf` and `sprintf` as eight-byte objects there, and why entering at `main` left them
null. Implementing `sceKernelDlsym` - answering with the same stub the linker would have
written - runs `__crt_start` for the first time (D366).

The next wall is a **measurement**, not a mystery: `__kernel_init` reads handoff field 2,
and the marker range says so by address.

## The research problem is smaller than it was

**Entering at `main` skips it for two of the five** (D343). `[entry] at = <image-relative>`
starts the guest past `__crt_start`, and ftpsrv and klogsrv immediately begin working:

```
klogsrv    signal(0xd)  then  getopt(...)     2 imports, image+0x0  -> image+0xd4
ftpsrv     getopt(...)                        1 import,  image+0x5ba0 -> image+0x5c64
```

`0xd` is `SIGPIPE`. A server installing a SIGPIPE handler and then parsing its options is
behaving exactly as it should, which is the part that matters - the code reached is the code
meant to run, not a plausible accident.

So Stage 1 has a measured front of the queue rather than a guessed one: **`signal`, `getopt`,
`optarg`**, ahead of the thirteen-function universal set below. `optarg` is a data object and
already has storage (D323).

The structure still has to be worked out for elfldr and pldmgr, which are stripped and have
no `main` to enter at.

**And it turned out not to be a way round the problem, only past its first wall** (D359).
Both payloads then jump to null inside their own `find_pid`, and the cause is the shortcut
itself: `__crt_start` is the program's *initialisation*, so skipping it leaves globals
holding what `.bss` holds - zero, which is also what an uninitialised function pointer looks
like. The guest calls a global nothing set.

Found by filling `.bss` with `0xB5` (`ORBISTOUN_BSS_FILL`): the fault changes completely,
identically in both payloads. Three other candidates were eliminated first - the `sysctl`
refusal, `signal` answering zero, the zeroed data-import storage.

So the handoff structure is back to being **the** problem, and worth more than it looked:
one structure against an unknown number of uninitialised globals.

## The original research problem

Every payload takes a pointer in `rdi` and calls through **slot 0** immediately (D308,
measured on all five, identical to the byte). Answering the fields it asks for moves the wall
three times, and then it reaches `0f 0b` - `ud2`, a deliberate compiler trap. It is not
derailed into data; it is **rejecting what it was handed**, on its own terms.

`__crt_start` is 828 bytes and byte-identical in klogsrv and ftpsrv, so this is the SDK's
shared C runtime and there is **one structure to work out, not five**.

Three routes, all legitimate, best used together:

1. ~~**Their documentation.**~~ **Tried, and it does not exist** (D361). Neither the SDK's
   README nor the loader's documents the calling convention or what the loader hands a
   payload; both describe usage and point at source. Principle 1 permits their prose and
   there is none to read, so this route is closed rather than untried.
2. **The marker sweep**, which already exists: `[entry] argument = "sentinels"` names the
   field a guest used, one boot for the whole structure. It confirms route 1 rather than
   replacing it - `published` promoted to `measured`.
3. **Build one.** D163's surviving conclusion: *"the value is not in downloading homebrew to
   run - it is in building a guest with the open toolchain."* A payload you built has a
   handoff contract you already know, with nothing to derive.

**Nothing else on this page is blocked on this**, except actually running a payload.

## What the ABI costs, exactly

The four servers - elfldr, klogsrv, shsrv, ftpsrv - need **100 distinct imports between them,
of which 75 are missing**:

| group | missing | notes |
|---|---|---|
| POSIX file I/O | 29 | `sceKernelOpen`/`Read`/`Write`/`Lseek`/`Stat` exist beneath - adapter work |
| BSD sockets | 13 | **nothing exists**; the one genuinely new subsystem |
| pthreads | 5 | `scePthreadCreate`, `scePthreadMutexLock` and the rest exist beneath |
| vendor `sce*` | **1** | `sceKernelSendNotificationRequest`, a toast notification |
| the rest | 27 | stdio, `strerror`, `getopt`, `sysctl`, signals, `kqueue`/`kevent`, `rfork_thread` |

**One vendor function across four payloads.** Everything else is POSIX or FreeBSD, which is
oracle #1 in `CLAUDE.md` - *"lawful, citable, and the strongest reference available"*. This is
the least speculative work available in this project.

`klogsrv` alone: 34 imports, 27 missing, of which 4 are adapters over calls that already
exist, leaving **23**. Two of those - `__stderrp` and `optarg` - are data objects that already
receive storage as of D323 and may need nothing further.

## Stages

Each ends in something checkable by a command, and none of them ends in "it looks right".

**Stage 0 - stage the payloads.** `orbistoun-fs` already has a generic `mount(prefix, root)`
and a `mount_data()` pointing `/data` at a writable host directory - which is where payloads
and `autoload.txt` live on the hardware. Probably already possible; untested.

**Stage 1 - the universal set.** The 13 imports all five payloads share: `open`, `close`,
`malloc`, `free`, `memset`, `strcmp`, `puts`, `vsnprintf`, `getpid`, `kill`, `strerror`,
`sysctl`, and the notification stub. All documented. *Checkable: a payload reaches its own
`main` and prints.*

**Stage 2 - sockets.** About 13 functions, mapping 1:1 onto host sockets, with no oracle
problem at all. *Checkable: `pros check` reports `klogsrv` up.* This is the milestone that
turns an independent tool into orbistoun's grader.

**Stage 3 - the file calls.** About 29, mostly adapters. *Checkable: `pros titles` lists what
is installed, and `pros backup` copies a save out with the bytes matching.*

**Stage 4 - shsrv and elfldr.** Both need things orbistoun has no seam for: a process model,
and a guest able to load a second guest. `pros check` reports them down until then, honestly,
which is what its report format exists for.

**pldmgr is not on this list.** 19 vendor functions across four services - launcher,
app-install, net, system-service - and it is the one that overlaps a home-screen replacement.

## The shortcut to refuse

orbistoun opening port 2121 and speaking FTP itself. It passes `pros check` in an afternoon
and proves **nothing** - principle 3's plausible output at the scale of a subsystem, and it
destroys the only reason the target was worth having in the first place.

The honest version is that every byte Prosperous sees was produced by guest code executing.

A related trap: special-casing by payload. `/dev/klog` is a device the platform has, so
emulating the device is the job. Noticing that the guest is klogsrv and feeding it the trace
is not.

## What can start today, with nothing unknown

- Stages 1 and 2. Neither is blocked on the entry contract, and a socket layer is testable
  against obSCEne or any other guest long before a payload runs.
- Reading the payload SDK's published ABI documentation, recorded `published`.
- Staging payloads under `/data` and confirming the mount serves them.

## What is not known

- **The handoff structure past field zero.** What is known:

  | field | what it is | how it is known |
  |---|---|---|
  | 0 | `sceKernelDlsym` | the guest called it with the string, out of its own `.rodata` (D365) |
  | 2 | a pointer the runtime reads through | a null there faults; a mapped marker does not (D368) |

  What field 2 should *point at* is not known. Every other field is unmeasured, and each is
  one run of the reporting block away from being described - which is the cheapest research
  this project has ever had.
- Whether the guest, past `__crt_start`, needs anything else the SDK's runtime would have set
  up for it.
- What `sysctl` MIB `[1.14.8.x]` should answer. `klogsrv`'s `find_pid` asks for it, takes its
  error path, and dies there when entered at `main`. `sys/sys/user.h` is in the FreeBSD
  checkout the constants are harvested from, so the structure is citable if it is wanted.

## The measured work list

Every payload in the local set, undefined dynamic symbols against what orbistoun implements.
The count is how many of the 25 want it.

| want | name | state |
|---|---|---|
| 22 | `vsnprintf` | **done** 2026-08-29 (D364) |
| 21 | `sceKernelSendNotificationRequest` | |
| 19 | `stat` | |
| 18 | `sleep` | **done** |
| 17 | `write`, `mkdir` | **done** |
| 17 | `readdir`, `opendir`, `closedir`, `munmap` | |
| 16 | `kill` | |
| 15 | `unlink`, `gettimeofday` | **done** |
| 14 | `time`, `socket`, `setsockopt`, `bind` | **done** (D371) |
| 13 | `rmdir`, `recv`, `listen`, `accept` | **done** |
| 12 | `fstat`, `fcntl`, `__inet_ntop` | |

### What each payload still needs

Measured by comparing every payload's undefined dynamic symbols against what orbistoun
implements, at the end of 2026-08-29. **Every payload in the set improved**; the ones that
did not reach zero are held up by the process model rather than by anything cheap.

| payload | missing | that morning |
|---|---|---|
| `kstuff-toggle` | **0 of 3** | 0 |
| `ps5debug-NG` | 1 of 15 | 1 |
| `klogsrv` | **2 of 34** - both data objects that already have storage | 16 |
| `elfldr` | 5 of 24 | 9 |
| `ftpsrv` | **7 of 85** - two of them data | 48 |
| `shsrv` | 7 of 41 - one of them data | 24 |

**`klogsrv` imports nothing that is not implemented.** Its two remaining names, `__stderrp`
and `optarg`, are objects rather than functions, and both already receive storage (D323).

`ftpsrv` needs **two functions**: `sscanf` and `strftime`. Its other five remaining names are
data objects, or were done late in the day - `fdopen`, `fileno` and `sendfile` all landed, and
`fprintf` follows a wrapped stream to its descriptor, which is how a server writes its replies
at all.

What `elfldr` and `shsrv` are missing is one subsystem, not a list: `execve`, `kqueue`,
`kevent`, `rfork_thread` and `waitpid` are a **process model**, which is Stage 4 and was
always going to be.

### orbistoun runs payload-SDK binaries, and the wall is one function

Settled on 2026-08-29 by building one (D375). A payload whose `main` was written in this
repository and linked with the real SDK, entered our own way with a `_start` that calls `main`
directly, **runs**: imports resolved, `puts` served, both calls on a conforming stack. The
same source entered at its declared entry fails **identically to `klogsrv`** - the same two
resolutions, then the same wild jump.

So the remaining gap is not a list of functions and not any payload. It is `__crt_start`'s
handshake with the loader, and the only unknown in it is what the fields after field zero
should hold.

That is now a sweep rather than a research problem: `[entry] handoff-fields = [[2, 0]]` puts a
literal in a named field, so a hypothesis costs a run rather than a rebuild.

### And klogsrv's last wall has a name

Entering past the runtime now fills the globals the runtime would have filled - by name, from
the payload's own symbol table, reported, and only in the mode that already declares itself
not an ordinary run (D376). `klogsrv` goes straight past the wall D359 spent a session on:
banner, `getopt`, `sysctl`, and into `klog_printf`, which renders its message with
`vsnprintf`.

It then calls **`ptr_syscall`** - a raw syscall gadget. The payloads do not reach the kernel
only through named imports; they issue syscalls directly. That is the last wall, it is a
subsystem rather than a question, and the numbers are in `sys/sys/syscall.h` in the checkout
the constants already come from.

And watching that gadget said something better than a register dump would have: with
`ptr_syscall` holding an unmapped marker the run reaches it and stops, reproducibly; with it
holding a real address the run fails **earlier**. The guest **tests it before using it** and
takes the syscall-available path when it is set - so a gadget that exists must work (D377).

The next unit is therefore the syscall boundary itself. Bounded, and nothing in it is unknown:
numbers from `sys/sys/syscall.h` in the checkout the ABI constants already come from, FreeBSD's
convention, and implementations that are written already under their names.

## klogsrv opens a port and something connects to it

Reached 2026-08-29 (D381). Entered past its own runtime, with the globals that runtime would
have filled written in by the loader:

```text
socket -> setsockopt -> bind -> listen -> select
```

It binds `0.0.0.0:3232` - dumped as `00 02 0c a0`, which is `AF_INET`, port 3232, `INADDR_ANY`
- prints its own interface address through `getifaddrs` and `__inet_ntop`, asks for a
notification, and waits in `select`. **A `TcpStream` from outside the process connects to it**,
which is exactly and entirely what `pros check` does.

With a connection made it goes on working: 26 distinct imports, 62 calls, `strncmp` and
`snprintf` six times each, and it runs to the time limit rather than stopping - which is what a
server does.

What stood between here and there was three lines of this project's own reporting code. The
syscall dispatcher runs **on the guest's stack**, so it must not allocate, print or lock; it
records, and the reporting layer prints (D381).

### What this is not

Not a clean run. It is entered past its own runtime, the handoff structure's unestablished
fields are still markers, and that mode declares itself. What is real is the socket: the guest
asked for it, bound it, listened on it, and something outside connected. orbistoun never opened
that port - `klogsrv` did.

**A guest can already open a port.** `socket`, `bind`, `listen`, `accept` and `select` work,
and a test does exactly what `pros check` does: opens a port from the guest side, then
connects to it from outside. What stands between that test and the milestone is not the socket
layer - it is `__crt_start` reaching `main`.
