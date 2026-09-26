# D027 - Windows is the primary host; Unix is verified in a VM

**Status:** decided
**Date:** 2026-08-19

Both platform implementations are written; Windows is the development host, and the Unix path
is built and tested in a virtual machine with the repository mounted.

**Why:** the platform layers are separate code, so a path that is never run ships defects that
the other platform cannot reveal. Neither platform carries a claim it has not passed.

**Rejected:**
- Unix left untested: an unverified implementation with a claim attached.
- Windows only: the Unix path rots unseen.
