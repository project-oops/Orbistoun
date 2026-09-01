# D398 - The hardware trip happened, and it moved seven placeholders


**measured** - 2026-08-30

A complete conformance suite ran on a target console - 521 checks in 28 sections - and the
records came back. Most of this project is assumptions written down where they can be counted,
which is only worth doing if they are actually retired when evidence arrives. This is that.

### The encoding, which was the important one

Seven distinct failures were provoked across five unrelated call families, and every one came
back as the same shape: the POSIX `errno` in the low bits under `0x8002_0000`. Not owner, no
entry, no such, bad descriptor, denied, busy, invalid.

Before this it rested on **one** value seen on an emulator - which could itself have been
inferring the same rule, so it was evidence of nothing, and was recorded as a hypothesis with a
baseline rather than a measurement. Seven values from five families on the machine itself is a
different class of thing, and it is what makes `GuestError::vendor` honest: a code built from it
is one somebody watched the target produce.

`GuestError::Busy` is gone as a result. It existed because a `trylock` that could not take a
lock needed to be spelled differently from a caller error (D256) - the reasoning was right and
the value was a placeholder. The distinction is now made with the code the machine uses.

### What else it settled

- **Direct memory is five gibibytes**, `0x1_4000_0000`, where this project assumed eight. Not a
  power of two, so it was never going to be guessed - and it was already known to matter, since
  changing it moves where a guest queries next.
- **The counter runs at `0x5f25_9b8e`**, not the nominal billion chosen so the arithmetic would
  be exact. The run cross-checked itself without meaning to: a 20000us sleep advanced the
  counter `0x1f12cd9` ticks while the microsecond clock advanced `0x4fbb`, which is the same
  1.5963 GHz by a second route. `read_tsc` now scales to it, because a counter ticking at one
  rate while its paired frequency call reports another is the exact trap the nominal value was
  chosen to avoid, one step along.
- **`GetProcessTime` really is microseconds**, which had been inferred from the name's family.
  The probe's own monotonic check would have passed just as happily on a wrong unit.
- **A short query buffer is accepted.** This refused anything smaller than the whole structure,
  on the reasoning that a caller passing less wanted a different layout. The console accepted
  every declared size from 1 to 256, so the refusal was this project's idea rather than the
  platform's. What is written is capped at what the caller declared - whether the console
  truncates the same way is unrecorded, and of the two guesses, overrunning a buffer the guest
  sized is the one that cannot be undone.
- **Query flags 0 and 1 are accepted, 2 and 4 are not.** A measured boundary rather than a guess
  about which bits mean something.
- **The third query field is not a boolean.** It read `3` for the region at the bottom of the
  map, and `allocated` here could only ever be 0 or 1 - so the previous meaning was provably not
  the platform's. It now carries the memory type. Whether `3` denotes a type or some state is
  still open, and one run distinguishes them.
- **A fresh mutex attribute has type 1**, not the zero an empty block reported. The same run
  showed the types are not interchangeable - type 2 re-acquires without blocking, 1 and 3 refuse
  with the busy code, 4 is invalid - so this is a value with consequences rather than a tag.
- **Two clocks that were missing.** The console answers `GetProcessTimeCounter` and its
  frequency call; this had neither.

### What it did not settle, and one thing it exposed

`sceKernelGetModuleInfo` **failed on hardware too**, with the invalid-argument code. That is
worth more than it looks: D395 stopped short of inventing the structure and said the layout had
to be measured. The refusal says the call was made the wrong way rather than that the platform
will not answer - almost certainly a size field the caller has to fill in first - so the next
run has something specific to try instead of a layout to guess.

### The tension this surfaced, which is not resolved here

`known_by` is one value per entry, and evidence arrives per *claim*. `GetProcessTime` now has a
measured unit and an unmeasured origin in the same entry, and the accounting refuses `measured`
alongside an open question - correctly, since the queue would otherwise re-ask something
settled.

It is recorded as `guest-observed`, so the flag holds the weakest link rather than overstating,
and the measurement lives in an edge case with its citation. That is honest but it is not right:
the entry undersells what hardware established. Whether provenance should attach to a claim
rather than to a function is a change to how the whole database is shaped, so it is raised here
rather than assumed.

