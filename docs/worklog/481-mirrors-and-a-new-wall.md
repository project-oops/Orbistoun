# 481. Mirrors, and a new wall

**2026-09-09** - directed, continuing 480

## The byte-order question answered itself, badly

`REQ-20260909T1910Z-3d05` asked SELFish which project's doc described the other's convention.
They read this crate's source and found the contradiction was **inside it**: `NidHasher::hash`
said "little-endian" one screen above `hash_bytes` saying "Big-endian, and this was wrong for a
long time", with `from_be_bytes` underneath. The `hash` wording was simply wrong.

Both docs now say it without an endian word - *"packed so that `digest[0]` is the most significant
byte"* - because "little-endian" and "big-endian" describe the packing, and the packing is what
differs. Each project reached for the word describing its own and called the other incorrect, in
mirror-image warnings, about the same eight bytes. `hash_bytes` now says "wrong **here**" and says
why the qualifier matters (D657).

SELFish corrected the same sentence on their side. Neither project's behaviour changes; what is
fixed is that a crate hashes and decodes the same way round, which `encode_nid`'s round trip pins.

## The new wall

PPSA25872, at 151 imports and 321,973 calls, faults at `image+0x17554a3` **reading through
`0xffffffffffffffff`** - a `-1` used as a pointer. Around it:

```text
rdi -> image+0x1e5dccc = "%s" ... "Hostname Look"
just before: libc::strlen, libc::memcpy, libkernel::scePthreadSelf
```

A `vsnprintf`/`strlen`/`strcpy` run assembling a diagnostic dump. **The title has already decided
something failed and is describing it**, so the `-1` is downstream and the thing worth finding is
what put it on that path. Four calls remain unimplemented -
`sceUserServiceGetLoginUserIdList`, `sceAppContentInitialize` (twice),
`sceAppContentTemporaryDataMount2`, `sceCommonDialogInitialize` - and a title that cannot get a
login user list has something to report. Filed as `REQ-20260909T2145Z-9e52`.

## Surprises

- **A test vector would have confirmed the wrong thing.** 3d05 offered one, and it would have
  agreed with what both projects already knew while leaving the wrong sentence in place. Reading
  the source is what found it.
- **Third units mix-up in two days**, after the `0x6bc000` addresses and the four harness errors.
  Every one was two things sharing a name and not a meaning, and every one produced a confident
  report naming a defect somewhere.
- **The trace's `returned` field is optional**, so a field-by-field text parse of the JSON silently
  carries the previous call's value forward. Nearly read a shifted column as evidence; the report's
  own ranked findings are the reliable source and were already correct.

## Next

- `9e52`, and whichever of the four turns out to be the gate.
- `image+0x17554a3` itself, once the path into it is known.
