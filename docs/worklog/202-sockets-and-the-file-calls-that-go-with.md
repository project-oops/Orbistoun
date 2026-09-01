# 2026-08-29 - Sockets, and the file calls that go with them


The measured work list said what to do next and in what order, which is the whole reason for
having one.

### The file calls first, because they cost nothing to get right

`mkdir` (seventeen payloads), `unlink` (fifteen), `rmdir` and `rename` (eleven each),
`remove`, `access`, `truncate`, `ftruncate`. POSIX says what each does; the only judgement was
where they may point, and that was already decided - every one goes through the two gates
`create` goes through, so a guest still cannot touch the user's own title (D250). There is a
test that tries, three ways, and fails all three.

`remove` is deliberately **not** an alias for `unlink`: C's takes a directory too, and binding
them together would make a guest tidying up fail on a call documented to work.

### Then sockets, which is the milestone

`pros check` does one thing per service - a connect, no handshake - so a service reads as up
the moment the guest has a listening socket. `socket`, `bind`, `listen`, `accept`, `connect`,
`setsockopt`, `getsockname`, `getpeername`, `send`, `recv`, `shutdown`, and a test that does
exactly what the grader does: open a port from the guest side, then connect to it from
outside. It passes (D371).

**One descriptor table**, because a guest has one: `close`, `read` and `write` take a file or
a socket without being told which, and two tables would have meant two numbering spaces.

**`sin_len` is the byte that catches people.** The family is at offset *one* of a
`sockaddr_in` on this platform; offset zero is a length. A shim written from memory of Linux
reads the length and calls it a family. It is in the checkout, so it was read rather than
recalled, and a test asserts it.

**`setsockopt` is accepted and applied to nothing**, which is stated rather than hidden. A
server's first act is `setsockopt(SO_REUSEADDR)` and failing it stops the server - so refusing
would end every payload before `bind` - while applying the rest would need a per-option
mapping nothing here can verify.

### Where the day left the payloads

| payload | missing | was this morning |
|---|---|---|
| `elfldr` | 7 of 24 | 9 |
| `klogsrv` | **8 of 34** | 16 |
| `shsrv` | 17 of 41 | 24 |
| `ftpsrv` | 30 of 85 | 48 |

Two of `klogsrv`'s eight are data objects that already have storage. Six functions left.

### And a rule that needed its second half

`orbistoun-input`'s port table is process-global and two of its tests write to it, so one
truncated the table the other was checking. It failed in the gate and passed alone, which is
the signature. **Fifth appearance** of this hazard - and the first where the standing fix,
*pass the thing rather than reaching for it*, does not apply, because the shared table **is**
what is under test. So the rule now has both halves (D372):

> Where the shared state is incidental, pass it. Where it is the thing under test, serialise
> the tests that touch it.

