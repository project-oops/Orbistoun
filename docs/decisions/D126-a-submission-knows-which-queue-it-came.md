# D126 - A submission knows which queue it came from


**Status:** assumed

`Queue::Draw` and `Queue::Compute`, because the guest has two and they take different
work. A stage the queue cannot run - a vertex shader in a compute submission - is
**reported**, not filtered.

Filtering would be the tidier behaviour and would hide the signal. A vertex shader named
by a compute submission is not an unusual frame; it is a register decode that found the
wrong bits, and that is worth surfacing for the same reason the disagreement above is.

