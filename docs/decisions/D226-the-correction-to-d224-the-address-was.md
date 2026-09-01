# D226 - The correction to D224: the address was wrong after all


**decided** · 2026-08-25 · the watch on the mapped region contradicted the previous entry

D224 concluded that `0xfffe0` was a legitimate address the guest expected mapped. **That is
wrong**, and the evidence came from the same tool one run later.

### What the guest actually writes there

```
0xfffe0  ->  0x0000400001a2dde0
0xfffe8  ->  0x0000400001a2dde0
0xffff8  ->  0x0000000000000001
```

Two identical pointers and a count of one: a **circular list head with a single node**, an
arena descriptor. `0x400001a2dde0` is `0x28` past `0x400001a2ddb8` - the mutex the guest
locks immediately before the fault, so the descriptor and its lock are one structure.

### Which makes the arithmetic mean the opposite of what D224 said

The guest asks `libkernel::0x6abac2f3dc6f8cee` for `arg1 = 0x100000` bytes aligned to
`arg3 = 0x40000`, and lays its arena header at **`region_end - 0x20`**. With the base lost,
`region_end` is `0 + 0x100000`, so the header lands at `0xfffe0`.

So the address was **not** right. It is `size - 0x20` computed from a base of zero, and
mapping low memory did not supply a region the guest wanted - it supplied somewhere for a
wrong pointer to land.

**The `FURTHER` was bought by giving the guest a wrong answer**, which is precisely the
class of progress principle 3 refuses to count. D224 flagged that risk in its own text and
then drew the opposite conclusion two paragraphs later, which is worse than not flagging it.

### What is genuinely gained

The first *positive* characterisation of that function after seven eliminations: it is a
**region allocator**. `arg1` is a size, `arg3` is an alignment, and the caller expects a
base back that it can lay an arena header at the top of.

That is what to implement, and implementing it is the real fix. `ORBISTOUN_MAP` stays a
diagnostic and must never become the answer here.

### The lesson, which is the one this session keeps re-teaching

A diagnostic that makes a fault move is not thereby a diagnosis. The mapping changed the
outcome and *felt* like a confirmation, and the same tool pointed at the same region one run
later said what the guest was actually doing. **Ask what the guest wrote, not only whether
it got further.**

