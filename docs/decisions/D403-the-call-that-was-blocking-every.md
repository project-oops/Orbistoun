# D403 - The call that was blocking every payload, and what it says the machine is


**measured** - 2026-08-30

Four open-toolchain payloads stop in the same place, and it is not a missing function. They ask
the kernel directly for call **649**, get nothing, print `Unable to initialize rtld`, and exit.
`elfldr`, `pldmgr`, `klogsrv` and `shsrv` all do it; the work list could not show it because the
work list ranked imports and this is not one (D401).

### What the guest's own instructions say it is

Called as `(2, 8, out)` - a kind, a length, and somewhere to put eight bytes. What comes back is
a **pointer**, and the caller reads exactly one field of what it points at:

```
68d5:  mov    -0x20(%rbp),%rax   ; the pointer it was handed
68de:  movzwl 0x16(%rax),%r14d   ; sixteen bits at offset 0x16
68e3:  shl    $0x10,%r14d
68ec:  cmp    $0x700ffff,%r14d   ; and then a ladder of comparisons
```

The ladder runs 0x0700FFFF, 0x085FFFFF, 0x093FFFFF, 0x103FFFFF, then finer: 0x121FFFFF,
0x12FFFFFF, 0x133FFFFF, and exact matches on 0x13400000, 0x13420000, 0x13600000. Those are
**firmware versions** - 7.00, 8.50, 9.30, 10.30, 12.1F, 12.FF, 13.3F, 13.40, 13.42, 13.60 - so
the field is the version of the system the guest is running on, and the call is how it asks.

That reading is inferred from what the guest does with the answer, not from any document. It is
checkable, which is the point: a value in a different band sends the guest down a different
branch and a run can watch which.

### Where the number lives, and where it does not

`abi-constants.toml` is generated from FreeBSD headers and its own comment forbids hand-editing,
on the grounds that a typed-in value cannot be traced back to a source. 649 has no header to be
traced to - it is a number read out of four running programs - so it goes in
`vendor-syscalls.toml` beside it, hand-written and saying so. Merging them would make the
generated file's guarantee false for some of its rows with no way to tell which.

### The version is a setting, and unset refuses

`Machine` already said which console this presents as - generation, retail or devkit, base or
faster revision. It now says which firmware, in the packed form the guest compares, so 13.09 is
`0x1309`.

**Zero means unset, and unset refuses the call.** Zero is inside the lowest band the guest
tests, so answering it would not fail - it would quietly select the path meant for the oldest
system there is. Same rule as the kernel release string, for the same reason (D397).

### What it bought, and what it did not

`elfldr` goes from three system calls to six and gets past the version check. It then stops
again, further along, and the next thing it wants is not a function at all - see D404.

