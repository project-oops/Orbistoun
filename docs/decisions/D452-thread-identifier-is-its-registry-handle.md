# D452 - A thread's platform-visible identifier is its handle in the thread registry

**Status:** assumed
**Date:** 2026-09-01

The call that answers a thread's platform-visible numeric identifier answers the same handle
value the thread's own self-reference call answers.

**Why:** Every thread this project registers already carries a stable handle unique for its life,
so it doubles as a valid identifier; a small counter-based identifier would answer zero for the
process's first thread, which runs guest code without ever being created through the usual path.
A handle, unlike a small integer, is the address of a real block, so a guest that mistakenly
dereferences it as a pointer lands on safe memory rather than a low address.

**Rejected:** a small monotonic integer distinct from the thread handle - unmeasured against the
target library's actual identifier width and namespace, and reproduces the low-address-dereference
class of fault a handle avoids.
