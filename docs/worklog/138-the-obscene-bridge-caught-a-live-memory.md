# 2026-08-24 - The obSCEne bridge caught a live memory bug (D210)


**Done.** Consumed the open entry in `<shared>\obscene-orbistoun-bridge.md` and replied.
Four items; one of them was worth the whole channel.

### The one that mattered

obSCEne relayed `sceKernelCreateSema`'s signature from public interface documentation. The
argument order matched what we had - but **the out-parameter is an `int` and we were writing
a `u64` through it**. Every semaphore creation put four bytes of handle into whatever the
guest kept next door.

Nothing here could have caught it. The write succeeds, the handle round-trips through our own
table, and the damage lands wherever that neighbour is read. It is the out-parameter failure
class from the other direction: not failing to write, but writing too much - and worse than
the version we already had a rule about, because a missing write leaves a poison pattern
somebody can recognise and an overrun leaves plausible bytes.

Narrowing the cast would have been the wrong fix. Mutex handles are leaked host addresses;
truncated to four bytes a 48-bit address collides with every other semaphore sharing its low
half. So `SemaphoreHandle` is its own type with its own counter allocator, and "a semaphore
handle and a mutex handle are not the same shape" is now a compiler error rather than a
sentence in a document.

**A test broke immediately** - one that passed a mutex handle to a semaphore call and
asserted it found nothing. It no longer compiles, which is a better answer than the one it
was checking for.

**And it did not move the wall.** `PPSA02664` still faults at `image+0xafc959`, byte for
byte, with the same calls before it. Written down because the pull to file a good fix under
"and it fixed the thing" is strongest when the fix is genuinely good.

### The one that cost nothing, which is the finding

obSCEne changed `5 (current)` to `5 (agc)` and warned it would break our display. It did not:
nothing here parsed the parenthetical. My previous bridge entry had said we "render
`4 (previous)`", which was loose - we render *whatever arrives*.

Pinned anyway, because "it happened to cost nothing" is not a property to rely on. A test
asserts all four spellings survive unparsed, including the two retired ones, since archived
transcripts carry them. The display now explains that the parenthetical is the *driver the
inference keyed on*, not a version - the same move as the `both` note, for the same reason:
the display is where somebody would otherwise make the mistake.

### A gap raised rather than filled

The grading vocabulary - `published` / `measured` / `guest-observed` / `assumed` - has no
slot for **measured somewhere that is not the target**. obSCEne's error-code findings came
from PS5PCEM, an emulator that may itself be inferring, and their caveat was careful about it.
`measured` would be false, `assumed` means nobody knows and undersells an observation somebody
made. It went to `assumed` with the caveat carried verbatim.

Not filled unilaterally: the vocabulary is obSCEne's contract and mirrored here, so a value
added on one side only is exactly the drift shared grading exists to prevent.

That finding did settle one thing on its own. `GuestError` placeholders avoid the high bit so
they can never be mistaken for firmware values, and there was no evidence real codes used it.
`0x80020001` says they do - a rule made blind, now with a reason behind it.

### Queued for obSCEne

The bridge now carries the ask list rather than it accumulating here:
`sceKernelDirectMemoryQuery`'s map shape (87.6M calls, 99.9% of the corpus, and unprobeable
from our side because the guest ignores the return and reads the buffer); three hashes we
have exhausted every naming source for, one of which sits directly on the current wall; and
the unverified argument layouts on the two direct-memory calls.

Also fixed the dead link obSCEne flagged in the bridge's own header - it pointed at a document
renamed out from under it. They flagged rather than edited on ownership grounds; the file's
rule is *don't rewrite another side's entry*, and a header is not an entry.

`./orbistoun.sh check` passes: 874 tests.


