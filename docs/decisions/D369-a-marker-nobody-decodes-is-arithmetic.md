# D369 - A marker nobody decodes is arithmetic somebody does by hand


**decided** - 2026-08-29

`sentinel_slot` has existed since D308 and **nothing called it**. Every marker fault this
project has ever reported was decoded by a person dividing an address by a stride they had to
go and look up, and it was done three times in one session before anybody noticed the decoder
was already written.

The reporter asks it now. The first run afterwards said:

```text
read of 0x5e2700002000 is handoff field 2 itself, which nothing has established
```

which is the finding D368 had to infer from a pair of runs, stated outright by one.

### The decoder lives where the marker is made

`orbistoun-report` took a dependency on `orbistoun-abi` rather than copying two constants.
Two copies of that arithmetic is how a decoder comes to disagree with the thing it decodes -
and the disagreement would be silent, because both would produce a plausible field number.

### Two depths, because one could not answer the next question

A field marker says the guest used field `n`, and that is all it can say: the moment the guest
reads *through* the field, the marker has done its job and what comes back is whatever is in
the page. A zeroed page answers zero, which names nothing.

So the page behind each field can hold markers of its own, one per word, each naming the field
**and the offset it was read from**. `ORBISTOUN_HANDOFF_FIELDS` now has four settings, and
each answers a different question:

| setting | what it answers |
|---|---|
| `strict` | which field the guest used - any use stops the run at an address that names it |
| `markers` | how far it gets when reads through a field succeed |
| `deep` | which *member* of what a field points at was used |
| `zero` | how far it gets when every field is a value it can check |

`strict` is the one that named field 2, and it is the one that had been quietly lost when the
region was mapped in D365 - mapping makes the read succeed, which is what let the guest get
further and is exactly what stops it saying which field it read.

### The stride is not shared, and that is not tidiness

Content markers use their own stride, deliberately far from the field markers'. A guest
truncating a marker to thirty-two bits - which they do, because a structure member is often an
`int` - keeps only the low half; with one stride, both depths produce the **same** low half,
and the question the two depths exist to tell apart would have the same answer either way.

That is not hypothetical. `klogsrv` carries a truncated marker into an address it then
dereferences: `0x2001` is field 2's low half plus one. With a shared stride, "did that come
from the field or from what the field points at" would have been unanswerable; with separate
strides, one run said it came from the field.

