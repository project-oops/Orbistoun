# D522 - A fault now says what its registers point at, and the label is "None"

**measured** - 2026-09-03

The argument dumper has named and dumped what a pointer points at since D198 - but only for
imports. **A fault printed sixteen bare values and said nothing about any of them**, which is
the harder case, because a fault is where a reader has least other information.

## What it cost, this run

D521 left one question: why does the module invoke the callback? The callback takes a short
text label, and the fault registers showed `rbx=4` and `r12=0x6000007fc390` - so the answer was
four characters sitting at a known address, and unreadable.

The attempt without an instrument was a watchpoint on that stack address. It reported eight
sites, **every one of them a host address** (`0x7ff7...`): orbistoun's own shims run on the
guest stack and churn that slot, and the thirty-two-site recorder filled with them before the
guest's own access appeared. A correct instrument, pointed at a question it cannot answer.

## The instrument

`FaultSite::pointees` - filled by the worker, rendered under the register dump. For each
register holding a **readable** address (`orbistoun_thunk::is_mapped`, the same question the
argument dumper asks), sixteen bytes and the region it lives in.

Registers that fail that test are left out entirely rather than reported as unreadable. Most of
the sixteen hold scalars, and fourteen "not an address" lines would bury the two that are.

```text
rax -> stack+0x7fc2e0 = 4e 6f 6e 65 00 00 ...  "None"
rsi -> stack+0x7fc240 = 65 4e f1 22 ff ee 1f 3c 00 ...
r12 -> stack+0x7fc390 = 4e 6f 6e 65 00 00 ...  "None"
```

## The answer, and it points away from the hypothesis it was built to test

**The label is `"None"`.** Four characters, matching `rbx=4`.

That is a memory-allocation *scope name*, and `"None"` is what a scope is called when it has no
name. It is the most ordinary value such a hook can be given - **not an error tag**. So the
shape D521 floated, that a module reports through this hook only on an unusual path, is
weakened rather than supported: the module appears to be doing something entirely routine, and
the routine thing needs a memory manager that does not exist yet.

Recorded because it is a negative that cost nothing to get once the instrument existed, and
because it was the *reason* for building the instrument - which is worth separating from what
the instrument then found.

## Quoting bytes as text is a place a report can lie

A fault dump is read closely and believed, so a quoted string in one has to be a string. Three
conditions, all required: something before the terminator, a **terminator inside the window**,
and every character printable.

The second is the one that matters and it came from this run's own data: `rsi` points at
`65 4e f1 22 ...`, a pointer whose first two bytes are `"eN"`. A looser rule quotes a fragment
of a pointer as a string in the middle of a fault report.

Both halves were broken and watched to fail. What the test cannot check is stated in it: a
four-byte integer whose bytes are all printable and whose fifth byte is zero is
indistinguishable from a short string, and this reports it as one.

## Where this leaves the wall

Unchanged - 181 distinct imports, 415,415 calls, same fault. What changed is that every future
fault, in every title, says what its registers were holding. That is the sort of thing this
project should have built the first time it needed it and instead worked around twice.
