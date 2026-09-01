# Typed buffer translation, and the last thing in this lane that needed nobody


MTBUF translates. **112 to 118 translatable instructions**, and all five typed-buffer
opcodes came off the blocker list.

`buffer_access` is now the shared body: descriptor, addressing equation, both bounds
checks. A typed access adds a component count and a format that has to pass a gate first;
an untyped one is that with one component and no format. Sharing was the point - two copies
of the addressing is two chances to be silently wrong about an address.

Three refusals, each by name and each tested without a GPU: a format needing conversion, a
format whose component count disagrees with the channel count, and a code with no meaning.

### Surprises

**No fixture completed.** Six instruction uses cleared and the count stayed at seven of ten,
because `unreached` carries all three VINTRP opcodes as well as its five typed accesses.
Worth noting because "instructions translatable" and "shaders complete" move independently,
and only the second is the real measure.

**The bounds check had to step with the components and it would have been easy not to.**
Checking only the first component's offset lets a four-channel access at the end of a
buffer read three words past it - the exact case the check exists for, defeated by the
feature being added on top of it.

**A doc comment got cut in half by an anchored insertion.** The anchor matched a line
*inside* `buffer_memory`'s doc block rather than before it, so the new code landed between
`/// # What is and is not translated` and its body. Clippy caught it as an empty line after
a doc comment, which is a lint I would not have guessed was load-bearing. The repair also
turned up the stale paragraph left behind: that doc still said typed accesses "are a
separate piece of work".

**The four-channel round trip failed on channel three and it was not the translation.**
Only v0 to v7 are copied out of a run, and the test loaded into v5 to v8. The fix was to
compare against *memory* rather than against the source registers, which is the stronger
check anyway - a translation writing every channel to the same address passes a
register-to-register comparison and fails this one.

