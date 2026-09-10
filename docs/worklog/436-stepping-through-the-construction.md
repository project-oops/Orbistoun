# 436. Stepping through the construction

**2026-09-08** - directed, continuing 435

## What was done

**Watchpoints traced the command buffer being built**, which is the one thing four iterations of
reading its fields could not do. The object's address is stable across runs even though its
storage is not, so a write watchpoint on the header names every instruction that touches it:

```text
0x2e7de51  sets the storage pointer
0x2e7de55  sets the size to 0x100000
0x2e66271  count -> 1
0x2e66275  the 0x14
0x2e66282  the 0x10000
```

All in the title's own modules - the fakelib and its neighbours, building the buffer in guest
code as D592 established.

**Then two capability gaps stopped the obvious next question.** Reading the bytes at those
addresses answered *"in no span this run published as readable"*, for memory the loader had
placed itself: the title's modules were named to the fault reporter and never published to the
readers. And a watch on a region that existed reported a diff and nothing else, so even a
published address would have said "nothing changed" and stopped (D593).

Both fixed. The span is published where it is described, and an unchanged watch prints what it
holds.

## The header is not what three entries said

```text
45 01 67 08    add  dword [r15+0x8], r12d
41 89 5f 04    mov  dword [r15+0x4], ebx
41 09 07       or   dword [r15], eax
41 21 07       and  dword [r15], eax
```

`+0x08` is a count and is **incremented**. `+0x04` is **assigned**, so `0x14` is a value and not
a running offset. `+0x00` is a **flags word**, OR-ed and AND-ed with masks.

D587 said which field was which had not been established, which was the right caution - and the
wrong shape still got into three later readings, because "not established" is easy to read as
"probably the obvious thing".

**Nothing in that sequence writes the storage buffer.** The header is bookkeeping.

## Surprises

- **The modules a title ships have been unreadable to every diagnostic since there were
  diagnostics.** They had a name in the fault reporter the whole time, which is what made the
  gap invisible: an address in them printed as `the title's own modules+0x…` and reading it
  answered as though it were unmapped.
- **A watch that found nothing changed said nothing else.** Complete as an answer to "what did
  the guest write here" and useless for "what is here", and the second is the question for
  anything the loader placed.
- **The frontier test is flaky while a session runs titles.** It compares committed numbers with
  compat records, and my own experiment runs update those mid-gate. Twice now.

## Next

- Where the command bytes are written, given the header sequence does not write them. A
  watchpoint on the storage would say, but its address drifts between runs - so the address has
  to come from the same run, which means asking from inside the submit handler.
- What `0x14` and the `0x10000` flag mean. Assigned-versus-incremented is a property of the
  instructions and says nothing about intent.
- Everything earlier iterations queued, still queued.
