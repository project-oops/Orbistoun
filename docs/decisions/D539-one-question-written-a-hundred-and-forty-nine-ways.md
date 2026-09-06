# D539 - One question, written a hundred and forty-nine ways

**decided** - 2026-09-04

D538 grouped the ask list by the sentence its entries share and said 275 premises behind 719
questions, with the caveat that 275 is a **floor**: grouping is word-for-word, so a premise
written two ways counts twice. This is what that caveat was hiding.

```text
before   719 open questions   275 premises   40 shared
after    713 open questions   135 premises   27 shared
```

## The delegation question was asked 149 times

`libScePosix` exports the POSIX spellings, and each resolves to the vendor-named function beside
it. Every one of those entries admitted the same thing - that the two spellings are *assumed* to
behave alike on the target - and every one said it **with the target's name inside the
question**:

```text
Resolves to `sceKernelClose`, and this describes that function. The two spellings are
assumed to behave alike; nothing on the target has confirmed it.
```

149 entries, 149 sentences, 149 premises. A fifth of the whole ask list, and nothing in the list
said that one measurement speaks to all of it.

**Two things were in one sentence**, which is the same fault D537 found in empty records and
this tick found again in the placeholder line below:

- *Resolves to `X`, and this describes that function* - a fact about what this project does. It
  goes in `edge_cases`, where the name is still there for a reader who needs it. **It has to
  stay somewhere**: for 122 of the 149 the question was the only place the delegate was named,
  so deleting the sentence would have lost it. That is why this is a move and not a rewording.
- *the two spellings are assumed to behave alike* - the question. It needs no name: the entry is
  the subject.

Split, they are one premise across 149 functions.

## Fixed where it is written, not where it landed

122 of the 149 were **generated** - `orbistoun-gen knowledge` derives entries from
implementation doc comments, and it built the sentence with `format!`, interpolating the target.
Editing the data alone would have left the generator to reintroduce it on the next new
delegation.

So the wording is now `orbistoun_hle::knowledge::DELEGATION_ASSUMPTION`, and the generator
pushes that constant rather than a string of its own. The data and the generator cannot drift
into two wordings, because there is only one wording.

`the_delegation_question_is_asked_in_one_wording` gates the rest: any *other* sentence in the
knowledge base about two spellings behaving alike is a fault, whoever wrote it. Made to fail by
reverting one entry to the old form.

## The placeholder line was not a question at all

24 entries carried *"The failure return is this project's placeholder rather than an errno. See
the file header."* - 101,511 calls - filed as an assumption.

Read the header it points at: the placeholder is a **settled choice**. It avoids the high bit so
it can never be mistaken for an established value; a guest testing `!= 0` takes its error path
correctly and one switching on specific values falls to its default branch. Orbistoun knows
exactly what it returns. No console can confirm or refute it.

What is genuinely unknown is one step along, and much sharper than the sentence it replaces:

> Which convention the platform's POSIX-named exports use on failure: POSIX's own - a returned
> errno, or -1 with errno set - or the vendor encoding `0x8002_0000 | errno` measured on their
> vendor-named twins (D398).

That is one provoked failure on hardware. D398 measured the encoding on seven failures across
five families of **vendor-named** calls; nothing has ever measured a POSIX-named one, and the
two conventions disagree about exactly the thing a guest switches on.

**The count did not drop**, and that is the point: this axis is not about shrinking the list. It
is about the list saying true things.

## Three that were facts pretending to be questions

Six questions retired, because they were never questions:

- `strtod` and `strtof`: *"The longest parsable prefix is taken, which is what the standard
  specifies"* - in an entry whose `known_by` is `published` and which cites ISO C 7.22. A
  sentence citing the standard as its authority is not admitting doubt about it.
- Four `scePthreadMutexattr*` entries: *"The out-parameter is a four-byte int. Writing a whole
  word through it overwrote the caller's neighbouring variable - which was a loop counter"*
  (D272). That is a thing somebody **watched happen**. `libSceSystemService` already files the
  identical fact as an edge case, so the project had made this call correctly once and not the
  other time.

## What is left, and what this does not do

27 shared premises remain. The next ones down are cross-references that do not stand alone in an
ask list - *"Same delivery caveat as posix_sigemptyset."*, *"As `_open`."* - which is a different
defect: readable in the file, useless in a queue somebody hands to a console.

Nothing here moves the wall or establishes anything about the platform. It is the third tick of
the same finding at three levels: D537, records that said nothing while the code held the
reasoning; D538, a list that could not see it was repeating itself; and now the repetition
itself, in the place that writes it.
