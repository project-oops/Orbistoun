# Open-toolchain payloads

How orbistoun runs payloads built with the open homebrew toolchain, and how Prosperous grades
it. A payload is an ordinary guest executable with its own C runtime, entered through a
handoff structure rather than through `main`.

## Prosperous as the grader

Prosperous is written against real hardware, independently of this project. Pointed at
orbistoun, its commands pass only if the guest does the work:

| Command | Who implements the protocol |
|---|---|
| `pros check` | nobody: it is `TcpStream::connect_timeout(...).is_ok()` and a timing. A service reads as up once the guest has a listening socket on its port |
| `pros klog` | the log server payload writes bytes to an accepted socket; orbistoun supplies `accept` and `write` |
| `pros titles`, `pros backup` | the file server payload implements FTP; orbistoun supplies sockets and the file calls |
| `pros send` | the ELF loader payload, which needs a guest able to load a second guest |

orbistoun never implements FTP or any other service protocol. The guest brings its own.

### Rules for this grader

- **Guest bytes only.** Every byte Prosperous sees is produced by guest code executing.
  orbistoun opening a port and speaking a protocol itself would pass `pros check` and prove
  nothing.
- **No special-casing by payload.** `/dev/klog` is a device the platform has, so orbistoun
  emulates the device. Recognising which payload is running and feeding it data is not
  emulation.

## The payload ABI

Payload imports are POSIX and FreeBSD almost entirely, with a single vendor function
(`sceKernelSendNotificationRequest`) shared across the servers. FreeBSD source is therefore
the reference for nearly all of it ([TESTING.md](TESTING.md)).

| Group | Where it lands in orbistoun |
|---|---|
| POSIX file I/O | adapters over `sceKernelOpen`, `Read`, `Write`, `Lseek`, `Stat` |
| BSD sockets | host sockets, one descriptor table shared with files (D371) |
| pthreads | adapters over `scePthreadCreate`, `scePthreadMutexLock` and the rest |
| stdio, `strerror`, `getopt`, `sysctl`, signals | `orbistoun-libc` and `orbistoun-kernel` |
| `execve`, `kqueue`, `kevent`, `rfork_thread`, `waitpid` | the process model |

Imports that name data objects (`__stderrp`, `optarg`) receive storage rather than a stub
(D323).

### Raw system calls

Payloads also reach the kernel directly: the runtime keeps a pointer to a syscall gadget in
the global `ptr_syscall`. A runtime that cannot look the gadget up falls back to the fixed
address `CONSOLE_SYSCALL_GADGET_BASE + 0x4ea` (the `syscall` instruction inside the
hardware's libkernel `getpid`), where orbistoun maps a trampoline
([ADDRESS_MAP.md](ADDRESS_MAP.md)). orbistoun supplies the gadget in both places, and `orbistoun-thunk`'s syscall dispatcher maps a FreeBSD syscall number, from
`sys/sys/syscall.h` in the harvested checkout, onto the implementation of the same name. An
unknown number answers `ENOSYS` and is reported once.

The dispatcher runs on the guest's stack, so it does not allocate, print or lock. It records,
and the reporting layer prints (D381).

## The handoff structure

A payload's entry point takes a pointer in `rdi` and calls through its fields. The runtime's
startup code (`__crt_start`) is the payload SDK's shared C runtime, identical across payloads,
so there is one structure to describe.

| Word | Contents |
|---|---|
| 0 | `getpid` when a firmware skeleton is present, otherwise the name resolver `sceKernelDlsym` (D407) |
| 1-5 | non-null pointers into firmware-skeleton storage, laid out as a payload receives them on the hardware (D408) |
| 6 onward | null |

The runtime resolves its C library by name through the resolver and stores each answer in a
named global in its own `.bss`. The measured layout for words 1-5 is `measured_handoff_fields`
in `orbistoun-worker`.

## Entry configuration

The `[entry]` table of the settings file (`orbistoun-cli paths`, the `config` entry) controls
how a guest is entered. Every key is optional.

| Key | Meaning |
|---|---|
| `convention` | `function` (default: the entry point is called) or `process` (jumped to, `rsp` at the argument count) |
| `argument` | what the entry point finds in `rdi`, below |
| `at` | start at this image-relative address instead of the declared entry; must lie in an executable segment |
| `handoff-fields` | `[[field, value], ...]` literals applied over the handoff block |
| `environment` | `NAME=value` strings |
| `extra-auxiliary` | extra auxiliary vector entries, `[[type, value], ...]` |

`argument` values:

| Value | The entry point receives |
|---|---|
| `image-address` | the process image, where `rsp` points (default) |
| `zeroed-block` | a zeroed block |
| `zero` | nothing |
| `main-arguments` | `argc` and `argv`, for entering at `main` |
| `handoff` | the handoff structure above |
| `sentinels` | a block of distinct unmapped markers; the fault address names the field read (diagnostic) |
| `answering` | every slot a function returning zero (diagnostic) |
| `reporting` | every slot a function that prints its slot number and first three arguments (diagnostic) |

### Entering past the runtime

`at` starts a payload past `__crt_start`, typically at `main`. The runtime's name-resolved
globals are then filled by orbistoun from the payload's own symbol table, answering each
name with the stub a relocation would have written; `ptr_syscall` gets the syscall gadget and
`payload_args` gets the handoff block. A name nothing implements gets a marker at
`UNSERVED_GLOBAL_BASE`, so its first use names it. Each fill is reported, and the run is
recorded as not an ordinary one (D376).

## Diagnostics

| Setting | Effect |
|---|---|
| `ORBISTOUN_ENTRY_ARGUMENT` | overrides `argument` for one run |
| `ORBISTOUN_HANDOFF_FIELDS` | what the unestablished handoff fields hold: `strict`, `markers`, `deep`, `members` or `zero` |
| `ORBISTOUN_HANDOFF_POISON` | puts an address nothing maps in one field; a fault on it means the runtime used that field |
| `ORBISTOUN_RUNTIME_GLOBALS` | comma-separated globals to point at a reporting stub that prints every register it is called with |
| `ORBISTOUN_BSS_FILL` | fills `.bss` with a byte, to expose a global nothing initialised |

`orbistoun-cli env` lists every setting with its full description.

```bash
orbistoun-cli handoff path/to/payload.elf --fields 12 --limit 5
```

`handoff` poisons one field per run and reports which fields the runtime uses (D390).
