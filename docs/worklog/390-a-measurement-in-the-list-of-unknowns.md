# 2026-09-04 - (/loop) A measurement was sitting in the list of things nobody knows

```
745 questions / 140 premises   ->   743 / 136
two guards added, both made to fail; two plan items came back clean
```

Twenty-seventh cron tick, working the plan's three named items.

## The item that was predicted wrong

`scePthreadMutexUnlock` was flagged for a suspected **overclaim** - its question says the
correspondence is inferred "from the name and the guest's usage" while `known_by` said
`assumed`. It was the reverse. The entry carried, in `assumptions`:

```text
CONFIRMED ON HARDWARE: unlocking a mutex nobody holds returns 0x80020001.
```

`questions` ranks assumptions and backlog 022 is generated from that ranking, so **the queue was
asking a console to establish something a console had already established**, in a sentence
saying so in capitals, behind 4,999 calls.

Beside it, one D398 retired months ago: *"Vendor error codes appear to be `0x8002_0000 | errno`
... a structure worth testing for, not an established encoding."* D398 provoked seven failures
across five families on hardware and every one came back in that shape - and **this entry's own
code was one of them**. A sentence that outlived its gap (check 13).

Both moved to `edge_cases` with their reasoning intact. `known_by` is now `guest-observed`,
matching the sibling `scePthreadMutexLock` - an inference from 4,999 proceeding calls, not new
evidence, and deliberately not `measured`, because two open questions remain and check 10
forbids one measured fact promoting an entry past them.

## The two that came back clean, filters stated

**Namesake test, widened past D540's single phrase.** Ten premises claim a POSIX correspondence;
the guard covered one. Widening the candidate set to several prefix strips - `sceKernelWrite`
gives both `write` and `kernel_write`, since which prefix is the vendor's cannot be read off the
name - and applying it to all five claim sentences plus the negative one:

```text
same shape (14fn, 872,904 calls) 0    behaviour follows (3) 0    resembles (1) 0
same name (32) 0                      "there is no POSIX function" (9) 0 that do have one
```

**Cross-references inlined.** *"As `_open`."* and *"Same delivery caveat as posix_sigemptyset."*
now say what they mean; six entries joined the premises they pointed at.

## The exception that turns the D537 split into a test

Inlining put *"Nothing here delivers signals, so a handler never runs"* in front of the D537
test, and it is plainly a statement about orbistoun. I was about to move it. The implementation
says why not: **"Recorded as an assumption rather than implied by this reporting success."**

`edge_cases` are not in the ask list. An entry whose success return is a lie needs that where its
unknowns are read. **So the split is a test, not a rule** - what orbistoun does goes in
`edge_cases` unless leaving it there lets a success be read as working. Reading the code before
editing is what caught it.

## Guards

`an_open_question_does_not_announce_a_measurement` (new) and
`a_claimed_posix_namesake_exists_and_a_denied_one_does_not` (widened, both directions, and it
asserts the widening still rejects the three names D540 caught). Both made to fail by
reinstating the faults.

Decision: [D541](../decisions/D541-a-measurement-was-sitting-in-the-list-of-things-nobody-knows.md).
