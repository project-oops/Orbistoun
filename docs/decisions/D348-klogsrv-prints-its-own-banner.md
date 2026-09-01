# D348 - klogsrv prints its own banner


**decided** · 2026-08-27

[D343](#d343---enter-at-main-and-the-payloads-start-working) got two payloads as far as
calling `getopt`. Six functions later one of them runs:

```
.----------------------------------------------------------------------.
|  _      _                                                   _    __  |
| | | __ | |   ___     __ _   ___   _ __  __   __       ___  | |  / _| |
...
| v0.9                                Copyright (C) 2025 John Tornblom |
'----------------------------------------------------------------------'
main.c:278:sysctl: error 0 (orbistoun has no message table)
```

**That is the guest's own output, and the last line is the guest naming its own next
blocker** - by file and line, through this project's own `strerror` text. `CLAUDE.md` calls
`printf` *"the cheapest oracle in the project"* for exactly this reason; here the whole
diagnostic path had to exist before the program could say anything at all.

klogsrv went from 3 imports and 3 calls to **9 and 18**; ftpsrv from 1 to 8.

### What was built, and the one shape they share

| function | why it was next |
|---|---|
| `signal` | first instruction after `main` - `SIGPIPE`, so a write to a closed socket does not kill the server |
| `getopt` | second - a server that cannot parse its arguments never opens a socket |
| `__error` | `errno` is `*__error()`, so a placeholder becomes a wild pointer the guest reads |
| `strerror` | fed straight into `printf` unchecked - a placeholder faults *inside* the print |
| `fprintf` | how the error path speaks |
| `puts` | eight calls of banner, ignored |

Three of the six - `getopt`, `__error`, `strerror` - **cannot be implemented as a return
value**. They report through storage: the guest's own `optind`, a per-thread `errno`, a
message buffer. D307 predicted this was coming (*"the first time the HLE layer would own
state rather than functions"*) and D323 built half of it; this is the other half.

`orbistoun-thunk::data_symbol` closes it. `DataBlocks` already reserved a page per data
import; it now keeps the **names** too, and publishes them before entry. An implementation
that must leave something where the guest will read it can find the guest's own slot. A
guest that never imported `optarg` gets no write rather than an invented one.

### Entering at `main` needed arguments to match

`main` is `main(int argc, char **argv)`, and `EntryArgument`'s options all answer a
different question - what a *process entry point* finds in its first register. Handing that
to `main` gives it a wild count to iterate over wild pointers, which `getopt` then has to
refuse.

`EntryArgument::MainArguments` reads `argc` and `argv` out of the process image already
written to the guest stack. **Read rather than constructed**: a second copy could disagree
with the one everything else in the guest will find.

`enter_guest_with_argument` grew a sibling taking two, rather than a second copy of forty
lines of inline assembly - `rsi` was always being set, to nothing in particular.

### Six knowledge records, because the tree refuses without them

`every_implemented_function_is_written_down` failed the moment `signal` was bound: *"signal
is implemented but nothing is recorded about it"*. That is principle 1's accounting working
exactly as designed, and it caught all six in one go.

Five are `published` - POSIX and FreeBSD are the citable reference. **`strerror` is
`assumed`**, and deliberately: its message is ours, says so, and does not imitate a table
nobody here has seen. A guest comparing it against a known string would fail; no caller
measured does, they print it.

### What is next, and the guest said it

`sysctl`, at `main.c:278`. Then `getpid`. Then the sockets that are the actual point.


