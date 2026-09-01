# D083 - The guest ignores return codes; the buffer is the channel

**decided** · 2026-08-19 · established empirically

Four experiments against a real guest, each cheap and each conclusive. Recorded in full
because the negative results cost as much to obtain as the positive one and would
otherwise be repeated.

### 1. The structure size, from the guest

Logging what a guest passes gave `rcx = 0x18` - twenty-four bytes, matching the
three-field layout the model assumed. Not consulted anywhere; the guest was asked.

### 2. The guest reads the answers

Changing the reported memory size from 8 GiB to 6 GiB moved the guest's second query
from `0x200000000` to `0x180000000`, exactly tracking. It is driving a genuine
enumeration, not tolerating a stub.

### 3. The return value is ignored - **negative result**

Ten candidate end-of-list codes were swept: vendor-shaped (`0x8002xxxx` over several
errnos), bare errnos, both signs. **Every one behaved identically** - ~175 million calls,
same loop. Whatever terminates that walk, it is not the value in `rax`.

This killed the leading hypothesis at a cost of about a minute, which is the argument for
making an unknown sweepable before reasoning about it.

### 4. The buffer is the channel - **the finding**

The terminal path returned an error **without writing the structure**, so a guest that
ignores return values re-read the previous answer, advanced to the same address, and
asked again. Forever.

Clearing the structure changed the behaviour outright:

```
before   0x0  END  END  END  END  END ...
after    0x0  END  0x0  END  0x0  END ...
```

The walk now **terminates and restarts**. That is the proof that the buffer is what the
guest reads, and it is why the clear is load-bearing rather than tidiness.

### 5. The third field is not the filter - **negative result**

Swept 0 through 10 in the third slot, in case it carries a memory type rather than an
allocated flag. No value changed the pattern. Still unverified, and still not what the
guest is selecting on.

### Where this leaves it

The guest walks the whole map, is shown one free 8 GiB region, **rejects it**, and starts
over. It is looking for something a single-region map does not contain - most likely a
realistic layout with several regions of differing kinds, or a structure field this has
not identified.

**Sweeping one field at a time has stopped paying.** The next thing needed is the
structure's actual layout, and the way to get it without reading anything is the
conformance probe: a binary *we* write, calling this function with inputs we chose, so
the answer can be observed rather than inferred. That moves obSCEne from roadmap item to
the tool the work now requires.

**The probes were removed rather than left behind.** Two environment knobs and a debug
print did their job and would otherwise be machinery paying no rent, with the findings
they produced living here instead.

