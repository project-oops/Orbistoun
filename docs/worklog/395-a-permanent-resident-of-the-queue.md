# 2026-09-04 - (/loop) A permanent resident of the work queue

```
OUTSTANDING 60 -> 59, OPAQUE 28 -> 29; 9 queue entries made self-contained
suites 130   clippy/fmt/identity clean on both repos
```

Thirty-second cron tick, plan item (a): re-derive the `hardware.rs` blockers by reading the code.
D543 found one whose reason was stale and had never been the mechanism. **This one is different -
the reason is right and the list is wrong.**

## Six of seven checked out

The sysctl group claims orbistoun refuses a knob. `answer_for` in `orbistoun-libc` is a four-arm
match - `kern.osrelease`, `kern.ostype`, `kern.hostname`, `None` otherwise - so `kern.version`,
`kern.sdk_version`, `kern.osrevision`, `hw.model`, `hw.machine` and `hw.availpages` are refused
exactly as written.

The seventh, `kern.osrelease`, that table **answers**. D447 corrected D397 for this knob: refusing
says "no such name", which is false; an empty NUL-terminated string says "exists, no value", which
is what the console showed.

## One fact, filed two ways

Two checks measured its length as `0xe`. One is OPAQUE - *"a per-machine setting... matching it
would be matching that console's configuration, not the platform"*. The other is OUTSTANDING -
*"as above - the same per-machine value, measured by a second check"*.

The reasons agree; the lists do not. `OUTSTANDING` is *"one unit of work with an unambiguous
completion condition"*; `OPAQUE` exists so the queue has no **"permanent residents - at which
point it stops being read as a queue"**. Matching `0.0-prototype` has no completion condition.
Moved.

## Why "as above" hid it

The cross-reference points upward *in the list it sits in* - at `kern.version`, a knob genuinely
refused under D397 - while the reasoning it borrows belongs to a neighbour in the **other** list.
A reader finds a coherent story about refused knobs and moves on. Sharper than D543's neighbours:
there the reference said nothing, here it said something true about the wrong entry. Nine
`rec-symbols` entries reading only *"as `symbols`, for the recording library"* now stand alone.

## The guard, and its reach stated

`two_checks_of_one_fact_do_not_disagree_about_which_list_it_is_in` - two checks measuring the same
subject, condition and value must be in the same list. Keyed on what the console reported, not on
the id. Made to fail by putting the entry back.

**One group qualifies today and after the fix it fires on nothing**, which is in the test rather
than dressed up. A guard on cross-referencing reasons would have fired on ten entries and **would
not have caught this one** - "as above - the same per-machine value" is a cross-reference that
also states its reason. Building that instead would have been the tidier story and the false one.

Decision: [D546](../decisions/D546-a-permanent-resident-of-the-work-queue.md).
