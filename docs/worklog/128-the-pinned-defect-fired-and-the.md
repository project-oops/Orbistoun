# The pinned defect fired, and the replacement pins the shape instead


`06-read.txt` is corrected upstream - two sixteen-byte chunks at offsets 0 and 16, valid
hex. Re-copied, and the test asserting the defect failed on the next run, which is the
trigger it was built to be: it named the fixture, said what was wrong, and said what to
replace it with.

Replaced with `a_read_arrives_in_chunks_and_is_assembled_by_offset`, which pins the thing
that actually matters now that the digits are right: **a read longer than sixteen bytes
comes back as several records**, each no more than sixteen bytes, with an ascending decimal
offset. The consumer concatenates by offset, and `done|returned|<len>` is the total rather
than any one record's size.

The reassembly already sorted by offset, so nothing needed changing - but a second test now
proves it with chunks arriving **out of order**, which the fixture cannot demonstrate
because its chunks are in order.

### Surprises

**Out-of-order assembly is the one worth having and the one no fixture can show.** Records
almost always arrive in offset order, which is precisely what makes a consumer that
concatenates in arrival order work right up until it does not - and when it fails it
produces a buffer of the correct length, full of real bytes, in the wrong sequence. That is
the least visible kind of wrong answer this project deals in, so it is tested against
constructed input rather than left to the happy path.

**A test written to fail later did its whole job.** It was green for one session, went red
the moment the upstream fixture changed, and its failure message was the instruction for
what to do next. Cheaper than a comment, and unlike a comment it could not be missed.

