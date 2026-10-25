# D977 - A sub-word store merges into its word as one step a part

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

A byte or half store (`global_store_byte`, `global_store_short`, `buffer_store_byte`,
`buffer_store_short`) writes its bytes into the word holding them, and leaves the word's other
bytes as they are. The module does it as one indivisible step on the word: a compare-and-swap
loop. It reads the word, merges its bytes in, and swaps the result in where the word still holds
what it read. Where another invocation changed the word in between, it retries from the value the
swap returned. A store of a half at a word's last byte does the same to the next word for its high
byte. This holds wherever the store lands: the window, a dispatch's bound buffer (D746) or global
buffer of pages (D961), and guest memory by device address (D964).

The hardware's writes are byte-granular. Two lanes storing neighbouring bytes of one word both
land, and two storing the same byte leave one of their values. A whole-word read, merge and write
would lose a neighbour's byte where the lanes race: where each lane is an invocation of its own
(D880), or where another wave stores into the word. An atomic AND and OR would keep neighbours but
leave the OR of two stores to the same byte. The swap does neither, in any order the device runs
the lanes.

**Bounded.** A round fails only where another invocation changed the word between this one's read
and its swap. So each round some contender lands, and a word contended by every lane of a wave
settles within as many rounds as it has lanes. The loop gives up after 1024 rounds (`MERGE_ROUNDS`)
and reports the store as an escape. The dispatch's result is then refused (D950): a device that is
not progressing is never spun on silently.

**One loop a store.** The part stores run in the module's loop over the wave's lanes (D956), so a
store is one swap loop in the module rather than one per lane. Unrolled, a wave's 64 lanes times two
words gave a module of 87,000 words. The NVIDIA driver's compiler took more than two gigabytes and
did not finish it.

**Two words, two swaps.** A half that crosses a word boundary swaps each word on its own, not with
one 64-bit swap. Such a half is unaligned, and the hardware does not store an unaligned access as
one indivisible write either. Its two bytes can come from two different racing lanes, as they can
on the hardware. A 64-bit swap would also need its pair at an even word, which an unaligned half
does not give. Every aligned byte or half lies in one word and takes one swap loop. No wider swap
would remove that loop.

**Cost**, measured on an RTX 5070 Ti through device addressing. 4096 groups of 64 lanes, the median
of eleven dispatches, the dispatch's time on the host:

| Stores | Whole wave | Per lane |
|---|---|---|
| a word each (`global_store_dword`) | 9.4 ms | 0.24 ms |
| a byte each, four lanes to a word | 14.0 ms | 1.2 ms |
| a byte each, every lane into one word | 146 ms | 144 ms |

`a_byte_store_s_cost_beside_a_word_store` in `direct_memory.rs` measures this, under
`ORBISTOUN_DEVICE_BENCH`. An uncontended byte store costs about 1.5 times a word store with the
whole wave in one invocation, and 5 times per lane. Contention on one word serializes the stores,
as the hardware's own atomics would.

A global byte or half store with no scalar base is traced into a dispatch's global buffer of pages
like a word store. `global_store_dwordx3` stores three words like the other word widths. A byte or
half store through the flat segment by device address is refused by name. Its address may reach the
local data share or private memory, which the swap does not cover.

**Why:** the operand table gained the byte, half and three-word global stores (`probes/memory.s`),
and the translation refused them by name. A store merging a whole word non-atomically would
silently drop a neighbouring lane's byte under the per-lane model.

**Rejected:**
- One read, merge and write of the whole word: it races.
- An atomic AND then OR: it keeps neighbours, but two stores to the same byte leave their OR, which
  no hardware store produces.
- An unbounded loop: a lost device would hang the dispatch with no report.
