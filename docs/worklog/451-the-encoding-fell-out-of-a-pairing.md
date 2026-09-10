# 451. The encoding fell out of a pairing

**2026-09-08** - directed, continuing 450

## Four more sweeps, and thirty-six facts to place

`163046`, `164701`, `165236` and `170447` ingested: **504 distinct measurements, 360 constant**,
up from 462 and 324 an hour earlier. The thirty-six new constants are placed - fourteen addresses
into the opaque list, twenty-two into the queue with a completion condition each - so the hardware
gate is green again and each one now names what would retire it.

## libSceNet's error base is `0x8041_0100`, and the analogy is wrong

Three libSceNet codes were on record with no way to read any digit of them: `0x8041_0123`,
`0x8041_0139`, `0x8041_0130`. Every other subsystem numbers from a plain high half-word - the
kernel `0x8002_0000`, audio `0x8026_0000` - so by analogy the errnos would be `0x123`, `0x139` and
`0x130`, which are 291, 313 and 304 and appear in no errno list.

The `170447` payload leg ran the same two conditions through the **POSIX** calls as well and
recorded `__error()` beside each:

| condition | `sceNet*` | `errno` |
|---|---|---|
| recv, connected, nothing to read | `0x8041_0123` | 35 `EAGAIN` (`0x23`) |
| recv, listening socket | `0x8041_0139` | 57 `ENOTCONN` (`0x39`) |

**The base is `0x8041_0100`.** The `0x01` that read as part of the errno is part of the base.
`sceNetBind`'s `0x8041_0130` corroborates - `0x30` is 48, `EADDRINUSE`, and the POSIX `bind` in
that same check had already taken the address - but nobody read that one's errno, so it is not a
third confirmation.

Recorded as `orbistoun_net::NET_ERROR_BASE` with a test reconstructing all three codes and a
**negative** test asserting that `0x8041_0000 | EAGAIN` is not what any console answered, because
that is the value somebody will one day "correct" it to and it is wrong by exactly `0x100` on
every code (D627).

Nothing about this needed a new measurement. It needed two existing ones put beside each other.

## Surprises

- **Orbistoun already emits the gadget sled the payloads want.** D626's "next" was to give the
  by-name stubs a syscall gadget at byte ten; `emit` has done exactly that since D400 - bytes 2
  through 15 of every thunk are `nop`, sliding into a jump to the syscall gadget, deliberately a
  sled rather than a point because ten is one payload's number and not the platform's. So the
  handoff route's failure is **not** a missing gadget, and the item comes off the list.
- **The handoff run resolves 569 names and still dies.** Under `ORBISTOUN_ENTRY_ARGUMENT=handoff`
  the payload bootstraps properly - `getpid` answers `0x700000008e40`, so its gadget is a real one
  - prints three of its own progress lines, and then faults calling a null pointer at
  `image+0x28a163` having emitted **zero** `OBS|` records. It is inside obSCEne's own weak-symbol
  handling, the GLOB_DAT/JUMP_SLOT split its D323 describes, except answering `0x0` rather than
  the `0x2` sentinel that record names. Left as an open question rather than guessed at: two
  requests are already on the bus and the rule is to keep at most two.
- **The payload reaches a socket by raw syscall on hardware** (`SYS_socket` answered descriptor
  `0xc`) where libSceNet resolves by no route at all. That is the route the payload leg actually
  has, and it is the one orbistoun does not offer.
- **Word six of the handoff structure is not null on this console.** D408 measured "six onward all
  null"; this capture has the kernel export table pointer there. Recorded and not acted on -
  modelling a kernel export table is out of scope by construction, not by omission.

## Next

- Why the handoff route faults at `image+0x28a163` with nothing emitted. It is the difference
  between seven measured checks and seven false defects.
- The input group: hardware says `sceMouseRead` and `sceKeyboardReadState` both answer `0x0`, and
  they are the second and third most-called unimplemented imports in the payload run. **Blocked on
  somebody else's requests** - `REQ-…-8bcf` and `REQ-…-3e71` are asking for the record layouts with
  devices attached, and implementing a read that returns success over an unfilled structure is the
  exact thing principle 3 forbids.
- `sceAgcCreateShader`, waiting on a question that may have no answer.
