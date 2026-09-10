# D657 - Both warnings were mirrors

**Status:** measured
**Date:** 2026-09-09

## The contradiction was inside this crate

`REQ-20260909T1910Z-3d05` asked SELFish which of the two projects' documents was describing the
other's convention. They read this crate's source and answered: **neither against the other's - it
is orbistoun's against orbistoun's, one function apart.**

```text
NidHasher::hash        "read little-endian"
NidHasher::hash_bytes  "Big-endian, and this was wrong for a long time"
                       u64::from_be_bytes(...)
```

The implementation is `hash_bytes` and always was. The wording on `hash` was simply wrong, and had
been sitting one screen above the paragraph that contradicted it.

**A test vector would not have found this.** 3d05 offered one, and it would have confirmed what
both projects already agreed on - that the bytes match once reversed - while leaving a wrong
sentence in place. What found it was somebody reading the source rather than taking the question
at face value.

## The naming, which is the part worth keeping

Both docs now say it without an endian word:

> The first eight bytes of the SHA-1 digest, packed so that **`digest[0]` is the most significant
> byte**.

"Little-endian" and "big-endian" describe *the packing*, and the packing is exactly what differs
between the two projects. So each reached for the word that described its own, and called the
other one incorrect - in mirror-image notes, in two repositories, about the same eight bytes.
SELFish's data file said orbistoun's big-endian claim "is incorrect"; orbistoun's `hash_bytes`
said reading it the other way "agrees with nothing". Both were true locally and both were phrased
as though universal.

`digest[0] is the most significant byte` says which end without reference to whose machine or
whose `u64`, and cannot be mirrored.

## What was actually wrong, and what was not

Neither convention is a bug. The `u64` is an internal representation; each project unpacks the
same digest its own way and arrives at the same eleven characters. What matters is the **pairing**
- that a crate hashes and decodes the same way round - and `encode_nid` exists mainly so a
round-trip test pins exactly that (D070).

So `hash_bytes` now says "wrong **here**", with the reason the qualifier is not modesty. A warning
written as universal is a warning that will be contradicted by somebody equally careful, and then
one of the two gets believed for the wrong reason.

## The shape this keeps taking

Three times in two days a confident report turned out to be about units rather than about
substance: the `0x6bc000` table addresses, the four harness errors, and now this. Every one was
two things that shared a name and not a meaning, and every one produced a report that named a
defect somewhere. The differential's own comments carry the first two; this crate's now carries
the third.
