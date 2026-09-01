# The filesystem is cleared, and there are four walls


Asked whether the data the guest loads is actually correct. Verifying content would double
every read; verifying *completeness* costs a counter and catches the failure that matters -
a truncated asset that faults later inside the guest's own parser (D175).

One distinction makes the count useful: reading to the end of a file is a short read by
definition, so only reads cut short *before* the end are counted.

```text
files    10 reads, 11328 KiB, none cut short
```

**Every byte the guest asked for arrived.** The filesystem is exonerated - worth as much as
a bug would have been, because it removes a layer from suspicion.

Then ran the whole corpus, which said something the single title never could: **the wall
was never singular.** Four titles, four walls, in four different places - plus two that do
not parse at all, being previous-generation containers. `image+0x43c4` had started to feel
like *the* blocker, and running one title repeatedly is exactly how that happens.

The better lead now is PPSA03416: an illegal instruction three frames from the entry point
with our error code sitting in `rax`, executing twenty-two megabytes into the image. That
reads as the guest calling through a pointer a stub handed it - much closer to the surface
than Earthion's wall.

And the previous-generation parse failure is the only item that changes the denominator:
four runnable titles out of six, not one wall out of one.

