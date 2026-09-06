# D546 - A permanent resident of the work queue

**decided** - 2026-09-04

Plan item (a): re-derive the remaining `hardware.rs` blockers by reading the code rather than the
sentence. D543 found one whose stated reason was stale and had never been the mechanism. This one
is different: **the reason is right and the list is wrong.**

## The sysctl group, checked against the knob table

Seven entries claim orbistoun refuses a knob. `orbistoun-libc`'s `answer_for` is a four-arm match
- `kern.osrelease`, `kern.ostype`, `kern.hostname`, and `None` for everything else - so six of the
seven are accurate: `kern.version`, `kern.sdk_version`, `kern.osrevision`, `hw.model`,
`hw.machine` and `hw.availpages` are refused, exactly as written.

The seventh is `kern.osrelease`, which that table **answers**. D447 corrected D397 for precisely
this knob: refusing it says "no such name", which is false, where an empty NUL-terminated string
says "exists, no value", which is what the console showed.

## One fact, filed two ways

Two checks measured its length as `0xe`, and the two landed in different lists.

```text
OPAQUE       135-sysctl/osrelease:kern.osrelease:length
             "a per-machine setting and empty by default. Orbistoun matching it would be
              matching that console's configuration, not the platform"

OUTSTANDING  135-sysctl/names:kern.osrelease:length
             "as above - the same per-machine value, measured by a second check"
```

The reasons agree. The lists do not, and this file is explicit about what they mean:
`OUTSTANDING` is *"one unit of work with an unambiguous completion condition"*, and `OPAQUE`
exists because entries that will never move up would leave the queue with **"permanent residents
- at which point it stops being read as a queue"**.

Matching `0.0-prototype` means matching one console's configuration. There is no completion
condition. It is a permanent resident, in the queue, and the file's own doctrine says so.

## Why "as above" hid it

The cross-reference points *upward in the list it sits in* - at `kern.version`, a knob orbistoun
genuinely refuses under D397 - while the reasoning it borrows belongs to a neighbour in the
**other** list. A reader checking the sysctl block finds a coherent story about refused knobs and
moves on.

Which is a sharper version of the cross-reference problem D543's neighbours had. There the
reference said nothing; here it said something true, about the wrong entry. **A reason that
stands alone cannot point at the wrong neighbour**, and nine `rec-symbols` entries reading only
*"as `symbols`, for the recording library"* now say what they mean.

## The guard, and an honest word about its reach

`two_checks_of_one_fact_do_not_disagree_about_which_list_it_is_in`: where two checks measured the
same subject, condition and value, both must be in the same list. Keyed on what the console
reported rather than on the id, because the id carries the check that took the reading and two
checks taking one reading is the case being looked for.

Made to fail by putting the entry back in the queue.

**One group qualifies today** - this one - and after the fix it fires on nothing. That is stated
in the test rather than dressed up. Its value is on the next capture, when a re-measured id
arrives beside one already filed and the two get decided by different people at different times;
its cost is a `BTreeMap` over 172 rows.

I considered a guard on cross-referencing reasons instead, since that is what hid this. It would
fire on ten entries and **would not have caught this one**: "as above - the same per-machine
value" is a cross-reference that also states its reason. Building it and claiming it as the catch
would have been the tidier story and the false one.
