# D245 - The probe's by-name census was arriving and going nowhere


**decided** · 2026-08-25 · found by reading obSCEne, which the review said to do

An external review said obSCEne's `resolve` - a by-name symbol lookup - is *"worth more than
the rest of this page combined"* and is specified but unbuilt, and that the useful action
from this side is to state what `orbistoun-probe` wants a `resolve` record to contain, so
the probe builds against a consumer rather than a guess.

Read-only access confirmed the claim and improved it. The record **already exists**:
`obs_report_resolve()` writes `OBS|resolve|<library>|<symbol>|present|absent|<address>`, and
its only caller is the symbol census in `sections/oracle.c`. What is missing is the network
verb - the announced capabilities are `call,read,report,gpu,blob,reset` - so obSCEne needs a
verb wired to an existing record, not a new record designed.

**And the consumer was already wrong.** obSCEne emits two records with the same first three
fields:

| record | fourth field |
|---|---|
| `sym` | how the symbol is reached |
| `resolve` | where it landed |

This reader had an arm for `sym` only. A `resolve` record was carried as `Record::Other` -
kept, exactly as the protocol requires of a record a consumer does not recognise, and
contributing **no symbol fact at all**.

So the census that answers for symbols *no title imports* - the one thing no collision search
over this repository's own candidates can ever reach, and the only item on either roadmap
that addresses the naming plateau directly - would have arrived and produced nothing, with
no error and no warning. Stating what the consumer wants would have been answering a question
about a consumer that did not work.

`Record::Resolve` is read, `Transcript::symbols()` collects it, and a test parses the exact
line `obs_report_resolve` writes.

### What `orbistoun-probe` wants, stated

- **The three fields it has**: library, symbol, `present`/`absent`. Existence is the fact;
  absence is as useful as presence and costs a candidate list nothing to check.
- **The address, verbatim.** Kept as written rather than parsed, because its meaning depends
  on the target and a stand-in's address is not the hardware's.
- **Availability, if the verb can carry it.** `sym` has it and `resolve` does not, and it is
  what decides how far a fact travels: existence is a property of the platform's interface,
  so a `present` from a stand-in still says the name is spelled correctly, while an address
  from a stand-in says nothing. Without it, a `resolve` fact cannot be graded by part.

`SymbolFact` now carries `availability: Option<String>` and `address: Option<String>`, each
`None` when its record did not have it. **Not an empty string**: "the record did not say" and
"the record said nothing" are different facts and must not be spelled the same - which is
the same rule as D241, one layer out.

### Not decided here

Whether obSCEne's `resolve` verb should also carry availability is obSCEne's call, and this
repository is read-only in it. The consumer accepts the record either way and says which
half it got.

