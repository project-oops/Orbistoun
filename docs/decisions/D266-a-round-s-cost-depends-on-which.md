# D266 - A round's cost depends on which position it grows, by a factor of twenty


**decided** · 2026-08-25 · read off the first run of the new binary

D264 established that a shape has two costs and they rank differently. The first real run of
`orbistoun-suggest` shows the same effect across *positions*, and larger:

| position grown | candidates swept in one round |
|---|---|
| `learned` | 214,798,320 |
| `verb` | 4,158,806,400 |
| `tail` | **5,059,730,000** |

A round re-sweeps every pattern using the grown position, so cost tracks how widely that
position appears. `learned` is in four patterns; `tail` is in nearly all of them. Growing
`tail` by one word is twenty-three times the work of growing `learned` by one.

That inverts the ordering the slot list was written with. It asks the shortest list first,
on the reasoning that one word changes the most where there are fewest - which is true about
*coverage* and exactly wrong about *cost*. Nothing is changed here yet, because the cheap
position is also the least valuable one and the trade has not been measured; recorded so the
next person to tune the loop starts from the numbers rather than from the comment.

