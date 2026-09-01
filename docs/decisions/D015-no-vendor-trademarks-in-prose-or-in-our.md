# D015 - No vendor trademarks in prose or in our own API

**decided** · 2026-08-19

Not concealment: what this targets is obvious from the first paragraph of any file,
and that is fine. The goal is a **low profile** - nothing here is advertised, so
there is no reason to repeat brand names, and a project that reads like marketing
invites attention it has no use for. Glossary in CLAUDE.md principle 2.

**The one exception:** literal symbol and library strings inside `guest_module!`
declarations are ABI identifiers the guest imports by, and the NID is computed from
them. Renaming them stops the tool working. Invented placeholders in tests and docs
use vendor-free names (`libTest`, `libExample`).

