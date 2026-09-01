# The learned file becomes something you can send somebody


Your idea: someone running orbistoun as a **binary**, no repository, no source, generates these
patches and submits them back. That works, and the reason it works is worth stating - accepting
contributions to an emulator is normally a provenance problem, and a measurement is not one. It
is derived from running a binary the submitter owns, reproducible by anyone with the title, and
**falsifiable by a command**. That is a stronger thing to accept than a diff.

So the file stopped being settings and became evidence (D297):

```toml
[[measurement]]
function = "sceKernelReserveVirtualRange"
measured = "PPSA02664-app0"
on       = "2026-08-26"
by       = "orbistoun 0.1.0"
known    = "guest-observed"
evidence = "conformance-check"
answers  = "ok"
assumes  = ["0x200000 bytes is a guess: the sweep measured where the guest faulted, not how much it asked for", ...]

[measurement.writes]
slot = 0
region_bytes = 2097152
```

`measured`, `assumes` and `evidence` were all being printed to a terminal and thrown away. They
are the difference between a measurement and an assertion. The policy the emulator runs under
is **derived** from these rather than stored, which keeps the distinction that makes the file
sendable at all: a measurement is a claim about a guest, a setting is a decision about a
machine.

### `--verify`, and the bug it found immediately

```
$ orbistoun-cli turn <title> --verify submitted.toml
  verified 1 submitted measurement(s) against 1 of our own
  every one agrees with what this machine measured
```

The **first** attempt reported `against 0 of our own` - on the machine that had produced the
file. Not a comparison bug: the measurement had already been *applied*, so the wall it was
measured at no longer existed, so the sweep found nothing to re-derive.

Left alone that is fatal to the idea. Applying a measurement would make it permanently
unverifiable, and later submissions would be checked against a machine already changed by the
answers being checked. A verifying turn now runs in **its own data directory** - nothing
learned, nothing accumulated - which is the state the original measurement was taken in
(D298).

**The shape will come back.** Anything that accumulates needs a way to be consulted with the
accumulation switched off, or its own output becomes the thing that confirms it. The name
vocabulary is the other one already in this position.

### All three paths tested

| submission | result |
|---|---|
| honest | `every one agrees with what this machine measured` |
| wrong `region_bytes` | `! here …2097152, submitted …999424` |
| names a function we never saw | `? not measured here - the title may be absent, or the run never reached it` |

That last one is reported and is **not** a refutation. "We did not look" and "it is wrong" are
different facts, and collapsing them would be the failure the whole `known_by` vocabulary
exists to prevent.

And deleting the file is still a complete undo: `BACK - reaching less of the interface than it
did`, fault returned to `image+0xafc959`.

