# D007 - Symbol databases store names; NIDs are derived

**decided** · 2026-08-19

A file listing both names and hashes could contain a pair that does not actually
hash to each other, and that inconsistency would surface as a mystery unresolved
import much later. Derivation makes the file unable to disagree with itself.

