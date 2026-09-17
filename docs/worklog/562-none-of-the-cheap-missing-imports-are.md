# 562. None of the cheap missing imports are walls

**2026-09-14** - four functions tested before being written, and none of them were the problem

## The plan, and why it was wrong

Three titles each had one or two unimplemented imports left, which read like the cheapest possible
progress: implement them and the titles move. `ORBISTOUN_RETURN` answers an import without
implementing it, so each could be tested first. All three were.

| title | forced to answer | result |
|---|---|---|
| `PPSA21564` | `sceKernelGetGPI` | **nothing moved** - 57 imports, 500,260 calls, same fault |
| `PPSA04263` | `scePthreadGetaffinity`, `pthread_setschedparam` | **nothing moved** - 49 imports, 20,469 calls, same fault |
| `PPSA25872` | `sceUserServiceGetAgeLevel` | 153 -> 156 imports, **same fault** |
| `PPSA25872` | `sceAppContentTemporaryDataMount2` | 153 -> 155 imports, **same fault** |

Two are not walls at all. The other two are load-bearing by a handful of imports and move the wall
nowhere.

**Four functions, none written.** The experiment cost four runs of a few seconds each; implementing
them first would have cost an afternoon and produced a report saying four more imports were answered,
which would have been true and worthless.

## What the titles are actually blocked on

Not missing imports. Their own code:

- `PPSA21564` faults in **the title's own modules** at `+0x7af792`, with **500,258 of 500,260 calls
  answered by a real implementation** - 0% on stubs. There is almost nothing left for orbistoun to
  answer, and it still dies.
- `PPSA04263` faults at `image+0x2bfab2f`, 2 calls on stubs out of 20,469.
- `PPSA25872` faults at `image+0x17554a3` whatever is answered, 2 to 4 on stubs out of ~322,000.

So the "unanswered" column is not a work queue for these three. A title with two unanswered imports
out of a hundred and fifty is not two functions away from running.

## Why the tools said otherwise

`orbistoun-cli questions` and the unimplemented-import lists rank by call count, which answers *what
is missing* and not *what is blocking*. Those are different questions, and for these three the answer
to the second is "nothing that is missing". Worklog 542 recorded the same shape one level up:
`worklist`'s top row is an artefact of corpus composition rather than a priority.

The cheap check that separates them already exists and is one environment variable. It belongs in the
loop before implementing anything the reports suggest, not after.

## Surprises

**PPSA04263 is also below its record** - 49 imports against a recorded 70, its fault at
`image+0x2bfab2f` against a recorded `image+0x196b91a`. That is the same shape as PPSA25872 in
worklog 555, now on a second title, and it was found incidentally rather than looked for. Whatever
accounts for one may account for both; neither is explained.
