# Seven names, two words, and a stale binary


Promoting the five model-earned words renumbered the grammar and the audit went to 314
unaccounted. `--repair` fixed 307; the seven survivors were `sceAjmBatch*` and
`sceLibcMspace*`, all marked `found = generated` - so the grammar had built them once.
Every part was present except two: **`Batch` and `Mspace` were in no vocabulary list at
all**, almost certainly dropped when `learned` fell from 12,255 entries to 175. Restored to
`learned`, and every name is accounted for again (D259).

**Time went on a stale executable first.** `vendor.toml` is `include_str!`d, so the audit
checks against whatever grammar the binary was compiled with. A binary four minutes older
than a promotion reported ten unaccounted names, two of them the names that promotion had
just fixed (D260).

Where it ended: **782 names**, work list 3653, `all checks passed`. Of those names, five
words a model proposed account for `sceAgcDriverSubmitAcb`, `sceAgcDriverSubmitDcb`,
`sceAudio3dObjectReserve`, `sceNpAuthPollAsync` and `sceNpAuthWaitAsync`.

