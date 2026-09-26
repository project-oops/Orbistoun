# D088 - The shader corpus is content-addressed

**Status:** assumed
**Date:** 2026-08-19

Captured shaders are stored under a hash of their bytes, and the corpus is the regression suite
for the translator.

**Why:** content identity makes a re-run add nothing and lets two titles be compared for what
they share, so capture is cheap enough to leave on. Every captured shader with its analysis is a
test case with a known previous result.

**Rejected:**
- Storing by capture order: duplicates on every run and no identity across titles.
