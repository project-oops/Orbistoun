# D259 - Two missing words made seven confirmed names unreproducible


**decided** · 2026-08-25 · found by reading what survived `audit --repair`

Promoting five model-earned words renumbered the grammar, so every generated derivation's
index moved and the audit reported 314 names unaccounted. `--repair` re-derived 307. The
seven that survived are the interesting ones:

```
sceAjmBatchCancel  sceAjmBatchInitialize  sceAjmBatchStart  sceAjmBatchWait
sceLibcMspaceCreate  sceLibcMspaceDestroy  sceLibcMspaceFree
```

All seven are `found = generated`, which means the grammar *did* build them once. `Ajm` and
`Libc` are in `module`, and every verb they use is in `verb` - but **`Batch` and `Mspace`
were in no list at all**. Two words, seven names.

They were almost certainly lost when `learned` went from 12,255 entries to 175. Restored to
`learned` rather than `object`: it is the semantically right home for a word read out of a
guest module, and it is far cheaper - `learned` appears in two patterns once each, while
`object` appears in six and twice in two of them. `audit --repair` then reported every name
accounted for.

**The lesson is about which list a word lives in, not about the words.** A vocabulary list
is not a bag: removing an entry can silently strand names that were proved by hash long ago,
and the only thing that notices is an audit nobody runs until something else breaks. The
ceiling file's own header says an entry belongs there when the grammar *genuinely* cannot
produce a name - these could be produced, so the ceiling would have been the wrong answer,
and a permanent one.

