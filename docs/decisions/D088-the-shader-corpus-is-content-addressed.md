# D088 - The shader corpus is content-addressed, and is the regression suite

**assumed** · 2026-08-19

Captured shaders are stored under a truncated hash of their bytes.

Identity by content rather than by capture order means re-running a title adds
nothing, two titles can be diffed for what they share, and a second run costs
essentially nothing because every shader is recognised without a write. Titles
re-upload the same shaders constantly, which is what makes capture cheap enough to
leave switched on permanently.

The larger point is what the corpus becomes. Every shader ever captured, stored with
its analysis, is a test case with a known previous result. Change the translator,
re-run the corpus, diff - a regression is visible immediately rather than the next
time somebody loads the affected scene. **That suite writes itself as a side effect of
running titles**, which is the same trick as the run report and as obSCEne's census
list turning out to be a NID candidate corpus.

