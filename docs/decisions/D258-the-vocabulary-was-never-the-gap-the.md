# D258 - The vocabulary was never the gap; the shapes were


**decided** · 2026-08-25 · four unnamed imports, each one step out of reach

Every harvesting mechanism had been at zero marginal yield against the corpus for weeks, and
the conclusion drawn from that was that the *vocabulary* was exhausted. It was the wrong
conclusion, and four imports the conformance probe calls showed why.

`posix_vocabulary()` already existed. It already capitalised all 3,018 names harvested from
FreeBSD source, and it was already injected into the grammar. The words were there the whole
time. **No pattern used them behind a module**: `prefix-posix` spells `sceUsleep`, and the
vendor writes `sceKernelUsleep`.

One new shape - `["prefix", "module", "posix", "tail"]` - reaches `sceKernelUsleep` and
`sceKernelDlsym` with nothing added to any word list. It costs 75 modules x 3,018 names
against an existing space of 2.8 billion.

Two more gaps, found the same way:

- **No shape allowed a second verb.** `sceKernelLoadStartModule` loads *and* starts, and
  every name of that form was unreachable however complete the vocabulary was.
  `prefix-module-verb-verb-object` fixes it.
- **`posix_vocabulary` joined underscore parts rather than emitting them.** `pmap_unset`
  became `PmapUnset`, so `Unset` - the morpheme a vendor name actually reuses - was never
  offered to the generator. Emitting each part as well takes the list 2,923 to 3,741.

### What this says about "exhausted"

A miss proves the name was not among those *tried*, and the tried set is the vocabulary
**crossed with the shapes**. Reporting exhaustion on the strength of a full vocabulary
measures one factor of a product and calls it the product.

The corpus sweep after these three changes named **38** imports it had never reached - none
from module strings, none from published names tried whole, all from the generated grammar
against material that has been in this repository the whole time.

### And a hazard found on the way

`names` widens the grammar as a side effect of running, so pointing it at another project's
binary imported 12,080 of that project's strings into the vocabulary with nothing said and
nobody deciding. Reverted. The refusal filter also could not see the injected `posix` list -
it read the file text - so it re-learned words the grammar already had, which is what the
model in the other thread had been paying for by hand.

